pub use lua_table_derive::FromLuaTable;
use std::collections::HashMap;

/// Every primitive value that can appear inside a Luau sub-table.
///
/// This enum is the lingua franca between the Luau VM (managed by
/// `lua_script_manager`) and the engine. The manager converts `mlua::Value`
/// into `LuaTableValue`; the engine never touches mlua directly.
#[derive(Debug, Clone)]
pub enum LuaTableValue {
    String(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    List(Vec<LuaTableValue>),
    /// Nested Luau table — used for sub-structs that also implement `FromLuaTable`.
    Map(HashMap<String, LuaTableValue>),
}

/// Conversion trait from a flat Luau sub-table to a typed Rust struct.
///
/// Implementors receive the contents of one Luau sub-table as a
/// `HashMap<String, LuaTableValue>` and are expected to extract every field
/// they need, returning `Err(String)` with a human-readable message on failure.
///
/// In practice this trait is never implemented manually — use
/// `#[derive(FromLuaTable)]` from the `lua_table_derive` crate instead.
pub trait FromLuaTable: Sized {
    fn from_lua_table(map: HashMap<String, LuaTableValue>) -> Result<Self, String>;
}
