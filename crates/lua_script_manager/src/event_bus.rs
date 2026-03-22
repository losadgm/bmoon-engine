use std::{cell::RefCell, rc::Rc};

use ahash::AHashMap;
use mlua::prelude::*;

use crate::api::LuaContextModule;

// ── HandlerId ─────────────────────────────────────────────────────────────────

/// Token opaco devuelto por `EventBus::on` (lado Rust).
///
/// El caller debe conservarlo y pasarlo a `EventBus::off` para desuscribir.
/// Cada llamada a `on` produce un `HandlerId` distinto aunque el mismo handler
/// esté suscrito a múltiples eventos.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HandlerId(u64);

/// Genera el siguiente `HandlerId` en orden creciente.
///
/// El contador vive en un `AtomicU64` estático, los IDs son únicos
/// globalmente entre todas las instancias del bus sin necesidad de que
/// `EventBus` cargue con ese estado.
fn next_handler_id() -> HandlerId {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    HandlerId(COUNTER.fetch_add(1, Ordering::Relaxed))
}

/// Extrae el puntero interno de una `LuaFunction` como `usize`.
///
/// Luau garantiza que dos referencias a la misma función comparten el mismo
/// puntero de heap, es decir, `fn_ptr(f) == fn_ptr(f)` si y solo si son
/// la misma referencia de función. Se usa como clave en `lua_fn_index` para
/// lograr desuscripción O(1) por referencia a función desde Luau.
///
/// El puntero nunca se dereferencía, solo se usa como clave de hash.
#[inline]
fn fn_ptr(func: &LuaFunction) -> usize {
    LuaValue::Function(func.clone()).to_pointer() as usize
}

// ── Handler types ─────────────────────────────────────────────────────────────

/// Handler del lado Rust.
///
/// `Rc` en lugar de `Box` para que el mismo closure pueda suscribirse a
/// múltiples eventos sin clonar el closure, solo se incrementa el refcount.
type RustHandler = Rc<dyn Fn(&Lua, LuaMultiValue) -> LuaResult<()>>;

// ── EventBus ──────────────────────────────────────────────────────────────────

/// Bus de eventos central compartido entre Rust y Luau.
///
/// Usa un layout de doble mapa por evento:
/// `nombre de evento -> HandlerId -> handler`
///
/// Esto permite eliminar cualquier handler individual en O(1) sin reindexar.
/// Para handlers Luau, un segundo mapa inverso `fn_ptr → HandlerId` permite
/// también la desuscripción por referencia a función en O(1).
///
/// Los closures Luau están anclados en el registro del VM via `LuaRegistryKey`
/// para que el GC no los recolecte mientras estén activos.
#[derive(Default)]
pub struct EventBus {
    /// Handlers Rust: evento -> HandlerId -> closure.
    rust_handlers: AHashMap<String, AHashMap<HandlerId, RustHandler>>,
    /// Handlers Luau: evento -> HandlerId -> clave de registro.
    lua_handlers: AHashMap<String, AHashMap<HandlerId, LuaRegistryKey>>,
    /// Índice inverso para desuscripción O(1) por referencia a función Luau:
    /// evento -> puntero de función -> HandlerId.
    lua_fn_index: AHashMap<String, AHashMap<usize, HandlerId>>,
}

impl EventBus {
    pub fn new() -> Self {
        Self::default()
    }

    // ── Subscribe ─────────────────────────────────────────────────────────────

    /// Suscribe un handler Rust a `event`.
    ///
    /// Devuelve un `HandlerId` que el caller debe conservar para desuscribir.
    /// El mismo closure puede suscribirse a múltiples eventos, cada llamada
    /// devuelve un `HandlerId` distinto y solo clona el puntero `Rc`.
    pub fn on<F>(&mut self, event: &str, handler: F) -> HandlerId
    where
        F: Fn(&Lua, LuaMultiValue) -> LuaResult<()> + 'static,
    {
        let id = next_handler_id();
        self.rust_handlers
            .entry(event.to_owned())
            .or_default()
            .insert(id, Rc::new(handler));
        id
    }

    /// Suscribe una función Luau a `event`.
    ///
    /// Llamado por el binding `events.on` en Luau. La función se ancla en el
    /// registro del VM para evitar que el GC la recolecte mientras la
    /// suscripción esté activa.
    ///
    /// Inserta simultáneamente en `lua_handlers` (id -> clave) y en
    /// `lua_fn_index` (puntero -> id) para habilitar desuscripción O(1) por
    /// referencia a función.
    pub fn on_lua(&mut self, lua: &Lua, event: &str, func: LuaFunction) -> LuaResult<HandlerId> {
        let id = next_handler_id();
        let ptr = fn_ptr(&func);
        let key = lua.create_registry_value(func)?;

        self.lua_handlers
            .entry(event.to_owned())
            .or_default()
            .insert(id, key);

        self.lua_fn_index
            .entry(event.to_owned())
            .or_default()
            .insert(ptr, id);

        Ok(id)
    }

    // ── Unsubscribe ───────────────────────────────────────────────────────────

    /// Desuscribe un handler Rust por `HandlerId`. O(1).
    pub fn off(&mut self, event: &str, id: HandlerId) {
        if let Some(map) = self.rust_handlers.get_mut(event) {
            map.remove(&id);
        }
    }

    /// Desuscribe un handler Luau por referencia a función. O(1).
    ///
    /// Usa el puntero interno de la función como clave en `lua_fn_index` para
    /// localizar el `HandlerId` en O(1), luego elimina de `lua_handlers` en
    /// O(1). Hacer drop del `LuaRegistryKey` libera el anchor del GC.
    pub fn off_lua(&mut self, lua: &Lua, event: &str, func: &LuaFunction) -> LuaResult<()> {
        let ptr = fn_ptr(func);

        let id = self
            .lua_fn_index
            .get_mut(event)
            .and_then(|idx| idx.remove(&ptr));

        if let Some(id) = id {
            if let Some(map) = self.lua_handlers.get_mut(event) {
                // Dropping LuaRegistryKey → GC puede recolectar el closure.
                if let Some(key) = map.remove(&id) {
                    lua.remove_registry_value(key)?;
                }
            }
        }

        Ok(())
    }

    /// Desuscribe todos los handlers (Rust y Luau) de `event`.
    pub fn off_all(&mut self, lua: &Lua, event: &str) -> LuaResult<()> {
        if let Some(map) = self.lua_handlers.remove(event) {
            for (_, key) in map {
                lua.remove_registry_value(key)?;
            }
        }
        self.lua_fn_index.remove(event);
        self.rust_handlers.remove(event);
        Ok(())
    }

    // ── Emit ──────────────────────────────────────────────────────────────────

    /// Despacha `args` síncronamente a todos los handlers suscritos a `event`.
    ///
    /// Los handlers Luau se disparan antes que los Rust. `A` debe ser `Clone`
    /// porque cada llamada a un handler Luau consume el multi-valor.
    pub fn emit<A>(&self, lua: &Lua, event: &str, args: A) -> LuaResult<()>
    where
        A: IntoLuaMulti + Clone,
    {
        // ── Handlers Luau ─────────────────────────────────────────────────────
        if let Some(map) = self.lua_handlers.get(event) {
            let funcs: Vec<LuaFunction> = map
                .values()
                .map(|key| lua.registry_value(key))
                .collect::<LuaResult<_>>()?;
            dispatch_lua(&funcs, args.clone())?;
        }

        // ── Handlers Rust ─────────────────────────────────────────────────────
        if let Some(map) = self.rust_handlers.get(event) {
            let multi = args.into_lua_multi(lua)?;
            for handler in map.values() {
                handler(lua, multi.clone())?;
            }
        }

        Ok(())
    }
}

// ── Helper de dispatch compartido (§8.4) ─────────────────────────────────────

/// Llama a cada función de `funcs` con `args`, evitando un clone innecesario
/// en la última invocación.
fn dispatch_lua<A>(funcs: &[LuaFunction], args: A) -> LuaResult<()>
where
    A: IntoLuaMulti + Clone,
{
    if let [init @ .., last] = funcs {
        for f in init {
            f.call::<()>(args.clone())?;
        }
        last.call::<()>(args)?;
    }
    Ok(())
}

// ── Helpers de acceso Rc<RefCell<EventBus>> ───────────────────────────────────

/// Toma prestado el `EventBus` inmutablemente desde Lua app data.
pub fn with_bus_read<R, F>(lua: &Lua, f: F) -> LuaResult<R>
where
    F: FnOnce(&EventBus) -> LuaResult<R>,
{
    let rc = bus_rc(lua)?;
    f(&rc.borrow())
}

/// Toma prestado el `EventBus` mutablemente desde Lua app data.
pub fn with_bus_write<R, F>(lua: &Lua, f: F) -> LuaResult<R>
where
    F: FnOnce(&mut EventBus) -> LuaResult<R>,
{
    let rc = bus_rc(lua)?;
    f(&mut rc.borrow_mut())
}

fn bus_rc(lua: &Lua) -> LuaResult<Rc<RefCell<EventBus>>> {
    lua.app_data_ref::<Rc<RefCell<EventBus>>>()
        .ok_or_else(|| LuaError::runtime("EventBus not bound — call bind_module::<EventApi> first"))
        .map(|r| r.clone())
}

// ── EventApi: bindings Luau (Scripting API Layer Fases 1 + 4) ───────────────

/// Expone la tabla global `events` en el VM de Luau.
///
/// Implementa la Fase 1 (API global) y la Fase 4 (bus de eventos bidireccional)
/// de la Scripting API Layer (§7.7). La tabla se registra directamente en el
/// entorno global de Luau, no bajo `engine.*`, porque `events` debe ser
/// accesible desde cualquier script sin necesidad de `ctx`.
///
/// API expuesta a Luau:
/// ```lua
/// events.on("nombre_evento", handler_fn)
/// events.off("nombre_evento", handler_fn)   -- por referencia a la función, O(1)
/// events.off_all("nombre_evento")
/// events.emit("nombre_evento", ...args)
/// ```
pub struct EventApi;

impl LuaContextModule for EventApi {
    type Context = EventBus;

    fn namespace() -> &'static str {
        "events"
    }

    fn bind(lua: &Lua, table: &LuaTable) -> LuaResult<()> {
        // events.on(event, handler)
        table.set(
            "on",
            lua.create_function(|lua, (event, func): (String, LuaFunction)| {
                with_bus_write(lua, |bus| {
                    bus.on_lua(lua, &event, func)?;
                    Ok(())
                })
            })?,
        )?;

        // events.off(event, handler), por referencia a la función, O(1)
        table.set(
            "off",
            lua.create_function(|lua, (event, func): (String, LuaFunction)| {
                with_bus_write(lua, |bus| bus.off_lua(lua, &event, &func))
            })?,
        )?;

        // events.off_all(event)
        table.set(
            "off_all",
            lua.create_function(|lua, event: String| {
                with_bus_write(lua, |bus| bus.off_all(lua, &event))
            })?,
        )?;

        // events.emit(event, ...args)
        table.set(
            "emit",
            lua.create_function(|lua, (event, args): (String, LuaMultiValue)| {
                with_bus_read(lua, |bus| bus.emit(lua, &event, args.clone()))
            })?,
        )?;

        Ok(())
    }
}
