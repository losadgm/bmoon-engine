/// Estado falso que simula el estado interno del engine.
///
/// En producción este módulo no existe — su propósito es únicamente
/// verificar que el ciclo completo manager → Luau → estado Rust funciona
/// correctamente antes de que el engine real esté implementado.
///
/// Cuando el engine tenga tipos reales (`World`, `EntityId`, etc.), este
/// archivo desaparece y `engine_lua` implementa `LuaContextModule` sobre
/// el estado real.
#[derive(Default, Debug)]
pub struct TestState {
    /// Registro de llamadas recibidas desde scripts Luau.
    /// Clave: nombre de la acción. Valor: lista de argumentos recibidos.
    pub calls: std::collections::HashMap<String, Vec<String>>,
    /// Contador genérico que los scripts pueden incrementar.
    pub counter: i64,
}
