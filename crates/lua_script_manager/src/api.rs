use std::{cell::RefCell, rc::Rc};

use mlua::prelude::*;

// ── Traits ────────────────────────────────────────────────────────────────────

/// A stateless API module: registers functions and values into a top-level
/// global table named `<namespace>` in the Luau VM.
///
/// Use this for pure utility APIs that don't need mutable shared state
/// (math helpers, string utilities, read-only game constants, etc.).
pub trait LuaApiModule {
    fn namespace() -> &'static str;
    fn register(lua: &Lua, table: &LuaTable) -> LuaResult<()>;
}

/// A stateful context module: wraps a `Context` value in `Rc<RefCell<T>>`,
/// stores it in Lua app data, and exposes functions that access it via
/// `ctx_read` / `ctx_write`.
///
/// Use this for any shared mutable state that Luau scripts need to observe or
/// mutate (EventBus, game state snapshots, etc.).
///
/// # Why `Rc<RefCell<T>>` and not `Arc<RwLock<T>>`
///
/// The engine is single-threaded and mlua's `set_app_data` only requires
/// `T: 'static` when the `send` feature is disabled (which it is — we use
/// `luau` + `vendored` without `send`). `Rc<RefCell<T>>` is therefore both
/// sufficient and more appropriate: it has no atomic overhead and its borrow
/// panics are statically impossible in synchronous single-threaded code.
pub trait LuaContextModule {
    type Context: 'static;

    fn namespace() -> &'static str;

    /// Register the module's Luau-facing functions into `table`.
    ///
    /// Functions access the context via `Self::ctx_read` / `Self::ctx_write`.
    fn bind(lua: &Lua, table: &LuaTable) -> LuaResult<()>;

    /// Borrow the context immutably for the duration of `f`.
    fn ctx_read<R, F>(lua: &Lua, f: F) -> LuaResult<R>
    where
        F: FnOnce(&Self::Context) -> LuaResult<R>,
    {
        let rc = lua
            .app_data_ref::<Rc<RefCell<Self::Context>>>()
            .ok_or_else(|| {
                LuaError::runtime(format!(
                    "context '{}' is not bound — call bind_module first",
                    std::any::type_name::<Self::Context>()
                ))
            })?
            .clone();

        f(&rc.borrow())
    }

    /// Borrow the context mutably for the duration of `f`.
    fn ctx_write<R, F>(lua: &Lua, f: F) -> LuaResult<R>
    where
        F: FnOnce(&mut Self::Context) -> LuaResult<R>,
    {
        let rc = lua
            .app_data_ref::<Rc<RefCell<Self::Context>>>()
            .ok_or_else(|| {
                LuaError::runtime(format!(
                    "context '{}' is not bound — call bind_module first",
                    std::any::type_name::<Self::Context>()
                ))
            })?
            .clone();

        f(&mut rc.borrow_mut())
    }
}

// ── Registration helpers ──────────────────────────────────────────────────────

/// Register a stateless `LuaApiModule` as a top-level global named `<namespace>`.
pub fn register_module<M: LuaApiModule>(lua: &Lua) -> LuaResult<()> {
    let table = lua.create_table()?;
    M::register(lua, &table)?;
    lua.globals().set(M::namespace(), table)?;
    Ok(())
}

/// Wrap `ctx` in `Rc<RefCell<T>>`, store it in Lua app data, and register the
/// module's functions as a top-level global named `<namespace>`.
pub fn bind_module<M: LuaContextModule>(lua: &Lua, ctx: M::Context) -> LuaResult<()> {
    lua.set_app_data(Rc::new(RefCell::new(ctx)));
    let table = lua.create_table()?;
    M::bind(lua, &table)?;
    lua.globals().set(M::namespace(), table)?;
    Ok(())
}
