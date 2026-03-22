mod api;
mod cache;
mod call_result;
mod convert;
mod event_bus;
mod manifest;
mod registry;

// ── Public re-exports ─────────────────────────────────────────────────────────

pub use api::{LuaApiModule, LuaContextModule, bind_module, register_module};
pub use call_result::{CallResult, LuaScriptError};
pub use event_bus::{EventApi, HandlerId, with_bus_read, with_bus_write};
pub use lua_table::{FromLuaTable, LuaTableValue};
pub use registry::ScriptId;

use std::{path::Path, rc::Rc};

use mlua::prelude::*;

pub use cache::LuaTableCache;
use convert::extract_sub_table;
use event_bus::EventBus;
use manifest::{find_manifests, load_manifest};
use registry::ScriptRegistry;

// ── LuaScriptManager ──────────────────────────────────────────────────────────

/// The single boundary between Rust and Luau.
///
/// Owns the Luau VM, the script registry, the table cache, and the event bus.
/// The engine interacts exclusively through this struct's public API — it never
/// touches `mlua` types directly.
///
/// Each API module registers itself as a top-level global in the Luau VM using
/// its own namespace (e.g. `events`, `game`, `world`). There is no intermediate
/// `engine` table — scripts write `events.on(...)` directly.
///
/// # Setup
///
/// ```rust
/// let mut manager = LuaScriptManager::new(512, &[Path::new("assets")])?;
///
/// // Register stateless API modules (pure functions, constants)
/// manager.register_api::<MyMathApi>()?;
///
/// // Bind stateful context modules (mutable shared state)
/// manager.bind_context::<GameApi>(GameState::default())?;
///
/// // Optionally preload hot scripts ahead of time
/// manager.preload("cards.lightning_bolt")?;
/// ```
pub struct LuaScriptManager {
    lua: Lua,
    registry: ScriptRegistry,
    cache: LuaTableCache,
}

impl LuaScriptManager {
    // ── Construction ──────────────────────────────────────────────────────────

    /// Create a new manager.
    ///
    /// Initialises the Luau VM, installs the `events` global, then walks
    /// `assets_roots` for `manifest.toml` files and registers all declared
    /// scripts.
    pub fn new(cache_capacity: usize, assets_roots: &[&Path]) -> LuaResult<Self> {
        let lua = Lua::new();
        let registry = ScriptRegistry::new();
        let cache = LuaTableCache::new_from_capacity(cache_capacity)?;

        // Bind the EventBus — registers `events` directly as a Luau global.
        bind_module::<EventApi>(&lua, EventBus::new())?;

        let mut manager = Self {
            lua,
            registry,
            cache,
        };
        manager.build_registry(assets_roots);
        Ok(manager)
    }

    /// Walk `roots` for `manifest.toml` files and register all declared scripts.
    ///
    /// Safe to call multiple times (e.g. on hot-reload). Conflicts are logged
    /// as warnings and the newer registration wins.
    pub fn build_registry(&mut self, roots: &[&Path]) {
        for root in roots {
            for manifest_path in find_manifests(root) {
                match load_manifest(&manifest_path) {
                    Ok(manifest) => {
                        let pkg_dir = manifest_path.parent().unwrap_or(Path::new(".")).to_owned();

                        #[cfg(debug_assertions)]
                        println!(
                            "[lua_script_manager] registering package '{}' ({} scripts)",
                            manifest.package.name,
                            manifest.scripts.len()
                        );

                        for entry in manifest.scripts {
                            let id = Rc::from(entry.id.as_str());
                            let path = pkg_dir.join(&entry.path);
                            if self.registry.get(&entry.id).is_some() {
                                self.cache.remove(&entry.id);
                            }
                            self.registry.register(id, path);
                        }
                    }
                    Err(e) => eprintln!(
                        "[lua_script_manager] invalid manifest at '{}': {e}",
                        manifest_path.display()
                    ),
                }
            }
        }
    }

    // ── Script lifecycle ──────────────────────────────────────────────────────

    /// Execute the global scope of `id` and cache the resulting environment.
    ///
    /// Idempotent — subsequent calls return the cached module without
    /// re-executing. The script runs in a sandboxed environment table that
    /// inherits VM globals via `__index`, keeping per-script state isolated.
    pub fn preload(&mut self, id: &str) -> LuaResult<()> {
        self.get_module(id).map(|_| ())
    }

    /// Remove `id` from the cache so it is re-executed on the next access.
    ///
    /// Previously registered event handlers remain active — if hook isolation
    /// on reload is needed, call `manager.off_all(event)` separately.
    pub fn invalidate(&mut self, id: &str) {
        self.cache.remove(id);
        if let Some(meta) = self.registry.get_mut(id) {
            meta.version += 1;
        }
    }

    // ── Public API (engine-facing) ────────────────────────────────────────────

    /// Call a named function in a script, passing `args` as its arguments.
    ///
    /// Performs a lazy load if the script is not yet cached.
    /// Returns `CallResult::FunctionNotFound` if the function is not defined —
    /// expected for cards that omit optional procedures.
    pub fn call<A, R>(&mut self, id: &str, func: &str, args: A) -> CallResult<R>
    where
        A: IntoLuaMulti,
        R: FromLuaMulti,
    {
        let module = match self.get_module(id) {
            Ok(m) => m,
            Err(e) => {
                let msg = e.to_string();
                eprintln!("[lua_script_manager] load error for '{id}': {msg}");
                return CallResult::ScriptError(LuaScriptError::RuntimeError(msg));
            }
        };

        let f: LuaValue = match module.get(func) {
            Ok(v) => v,
            Err(e) => {
                let msg = format!("{id}::{func}: {e}");
                eprintln!("[lua_script_manager] {msg}");
                return CallResult::ScriptError(LuaScriptError::RuntimeError(msg));
            }
        };

        let f = match f {
            LuaValue::Function(f) => f,
            LuaValue::Nil => return CallResult::FunctionNotFound,
            other => {
                let msg = format!("{id}::{func}: expected function, got {}", other.type_name());
                eprintln!("[lua_script_manager] {msg}");
                return CallResult::ScriptError(LuaScriptError::RuntimeError(msg));
            }
        };

        match f.call::<R>(args) {
            Ok(v) => CallResult::Ok(v),
            Err(e) => {
                let msg = e.to_string();
                eprintln!("[lua_script_manager] runtime error in {id}::{func}: {msg}");
                CallResult::ScriptError(LuaScriptError::RuntimeError(msg))
            }
        }
    }

    /// Read a named sub-table from a script's environment and convert it to `T`.
    ///
    /// Returns `None` if the key is absent, the script fails to load, or the
    /// conversion fails. The manager logs the reason in the latter case.
    pub fn get_table<T: FromLuaTable>(&mut self, id: &str, table_name: &str) -> Option<T> {
        let module = self
            .get_module(id)
            .map_err(|e| eprintln!("[lua_script_manager] load error for '{id}': {e}"))
            .ok()?;

        match extract_sub_table(&module, table_name) {
            Ok(Some(map)) => match T::from_lua_table(map) {
                Ok(v) => Some(v),
                Err(msg) => {
                    eprintln!(
                        "[lua_script_manager] conversion error in '{id}'.{table_name}: {msg}"
                    );
                    None
                }
            },
            Ok(None) => None,
            Err(e) => {
                eprintln!("[lua_script_manager] error reading '{id}'.{table_name}: {e}");
                None
            }
        }
    }

    // ── EventBus — Rust-side API ──────────────────────────────────────────────

    /// Subscribe a Rust handler to `event`. Returns a `HandlerId` for later
    /// unsubscription via `off`.
    pub fn on<F>(&self, event: &str, handler: F) -> LuaResult<HandlerId>
    where
        F: Fn(&Lua, LuaMultiValue) -> LuaResult<()> + 'static,
    {
        with_bus_write(&self.lua, |bus| Ok(bus.on(event, handler)))
    }

    /// Unsubscribe a Rust handler by `HandlerId`.
    pub fn off(&self, event: &str, id: HandlerId) -> LuaResult<()> {
        with_bus_write(&self.lua, |bus| {
            bus.off(event, id);
            Ok(())
        })
    }

    /// Unsubscribe all handlers (Rust and Luau) from `event`.
    pub fn off_all(&self, event: &str) -> LuaResult<()> {
        with_bus_write(&self.lua, |bus| bus.off_all(&self.lua, event))
    }

    /// Dispatch `args` to every handler subscribed to `event`.
    pub fn emit<A>(&self, event: &str, args: A) -> LuaResult<()>
    where
        A: IntoLuaMulti + Clone,
    {
        with_bus_read(&self.lua, |bus| bus.emit(&self.lua, event, args))
    }

    // ── API module registration (Scripting API Layer §7.7) ────────────────────

    /// Register a stateless API module as a top-level Luau global.
    pub fn register_api<M: LuaApiModule>(&self) -> LuaResult<()> {
        register_module::<M>(&self.lua)
    }

    /// Bind a stateful context module as a top-level Luau global.
    pub fn bind_module<M: LuaContextModule>(&self, ctx: M::Context) -> LuaResult<()> {
        bind_module::<M>(&self.lua, ctx)
    }

    // ── Diagnostics ───────────────────────────────────────────────────────────

    pub fn registry_len(&self) -> usize {
        self.registry.len()
    }
    pub fn cache_len(&self) -> usize {
        self.cache.len()
    }
    pub fn is_cached(&self, id: &str) -> bool {
        self.cache.contains(id)
    }

    /// Toma prestado el contexto de un `LuaContextModule` bound y llama a `f`
    /// con él.
    ///
    /// Útil para inspeccionar el estado mutado por scripts Luau sin
    /// pasar por la API de Luau.
    pub fn with_state<M, R, F>(&self, f: F) -> R
    where
        M: LuaContextModule,
        F: FnOnce(&M::Context) -> R,
    {
        use std::cell::RefCell;
        use std::rc::Rc;
        let rc = self
            .lua
            .app_data_ref::<Rc<RefCell<M::Context>>>()
            .expect("context not bound")
            .clone();
        f(&rc.borrow())
    }

    // ── Private ───────────────────────────────────────────────────────────────

    /// Return the sandboxed environment table for `id`, loading and caching it
    /// on first access.
    fn get_module(&mut self, id: &str) -> LuaResult<LuaTable> {
        if let Some(table) = self.cache.get(id) {
            return Ok(table);
        }

        let path = self
            .registry
            .get(id)
            .ok_or_else(|| LuaError::runtime(format!("script '{id}' not found in registry")))?
            .path
            .clone();

        let source = std::fs::read_to_string(&path)
            .map_err(|e| LuaError::runtime(format!("cannot read '{}': {e}", path.display())))?;

        // Each script runs in its own environment table. `__index` delegates
        // to VM globals so the script can access built-ins and all registered
        // API modules without polluting the shared global namespace.
        let env = self.lua.create_table()?;
        let meta = self.lua.create_table()?;
        meta.set("__index", self.lua.globals())?;
        env.set_metatable(Some(meta))?;

        self.lua
            .load(&source)
            .set_name(&format!("@{}", path.display()))
            .set_environment(env.clone())
            .exec()?;

        self.cache.put(Rc::from(id), env.clone());
        Ok(env)
    }
}
