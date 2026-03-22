use engine::test_state::TestState;
use lua_script_manager::LuaContextModule;
use mlua::prelude::*;

/// Adaptador Luau para `TestState`.
///
/// Expone el estado del engine falso bajo el namespace global `test`,
/// accesible desde cualquier script como `test.record(...)`, `test.increment()`,
/// etc.
///
/// Solo existe para verificar el manager antes de que el engine real esté
/// implementado. Cuando `engine_lua` tenga módulos reales (`GameApi`,
/// `WorldApi`, etc.) este archivo desaparece.
pub struct TestStateLua;

impl LuaContextModule for TestStateLua {
    type Context = TestState;

    fn namespace() -> &'static str {
        "test"
    }

    fn bind(lua: &Lua, table: &LuaTable) -> LuaResult<()> {
        // test.record(action, value)
        // Registra una llamada desde Luau.
        table.set(
            "record",
            lua.create_function(|lua, (action, value): (String, String)| {
                Self::ctx_write(lua, |state| {
                    state.calls.entry(action).or_default().push(value);
                    Ok(())
                })
            })?,
        )?;

        // test.increment()
        // Incrementa el contador.
        table.set(
            "increment",
            lua.create_function(|lua, ()| {
                Self::ctx_write(lua, |state| {
                    state.counter += 1;
                    Ok(())
                })
            })?,
        )?;

        // test.counter() -> i64
        // Devuelve el valor actual del contador para verificarlo desde Luau
        // si fuera necesario.
        table.set(
            "counter",
            lua.create_function(|lua, ()| Self::ctx_read(lua, |state| Ok(state.counter)))?,
        )?;

        Ok(())
    }
}
