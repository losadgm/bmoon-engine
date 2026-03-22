use std::collections::HashMap;

use mlua::prelude::*;

use lua_table::LuaTableValue;

/// Convert a `mlua::Value` into a `LuaTableValue`.
///
/// Only the variants present in `LuaTableValue` are supported. Unsupported
/// types (functions, userdata, threads) return an error rather than silently
/// dropping data.
pub fn value_to_lua_table_value(value: LuaValue) -> LuaResult<LuaTableValue> {
    match value {
        LuaValue::String(s)  => Ok(LuaTableValue::String(s.to_str()?.to_owned())),
        LuaValue::Integer(i) => Ok(LuaTableValue::Int(i)),
        LuaValue::Number(f)  => Ok(LuaTableValue::Float(f)),
        LuaValue::Boolean(b) => Ok(LuaTableValue::Bool(b)),
        LuaValue::Table(t)   => table_to_value(t),
        LuaValue::Nil => Err(LuaError::runtime(
            "cannot convert Nil to LuaTableValue",
        )),
        other => Err(LuaError::runtime(format!(
            "unsupported Luau type '{}' in LuaTableValue conversion",
            other.type_name()
        ))),
    }
}

/// Convert a `LuaTable` to `List` if all keys are consecutive 1-based
/// integers, or to `Map` otherwise.
fn table_to_value(table: LuaTable) -> LuaResult<LuaTableValue> {
    let mut int_pairs:    Vec<(i64, LuaValue)>    = Vec::new();
    let mut string_pairs: Vec<(String, LuaValue)> = Vec::new();

    for pair in table.pairs::<LuaValue, LuaValue>() {
        let (k, v) = pair?;
        match k {
            LuaValue::Integer(i) => int_pairs.push((i, v)),
            LuaValue::String(s)  => string_pairs.push((s.to_str()?.to_owned(), v)),
            other => return Err(LuaError::runtime(format!(
                "unsupported table key type '{}' in LuaTableValue conversion",
                other.type_name()
            ))),
        }
    }

    // Pure consecutive 1-based integer keys → List.
    if string_pairs.is_empty() {
        int_pairs.sort_by_key(|(i, _)| *i);
        let is_array = int_pairs
            .iter()
            .enumerate()
            .all(|(idx, (key, _))| *key == idx as i64 + 1);

        if is_array {
            let items = int_pairs
                .into_iter()
                .map(|(_, v)| value_to_lua_table_value(v))
                .collect::<LuaResult<_>>()?;
            return Ok(LuaTableValue::List(items));
        }
    }

    // Mixed or string keys → Map.
    let mut map: HashMap<String, LuaTableValue> = HashMap::new();
    for (k, v) in string_pairs {
        map.insert(k, value_to_lua_table_value(v)?);
    }
    for (k, v) in int_pairs {
        map.insert(k.to_string(), value_to_lua_table_value(v)?);
    }
    Ok(LuaTableValue::Map(map))
}

/// Extract a named sub-table from a script's environment table and convert it
/// to `HashMap<String, LuaTableValue>` for `FromLuaTable::from_lua_table`.
///
/// Returns `None` if the key is absent or nil; returns an error if the value
/// is present but not a table.
pub fn extract_sub_table(
    env: &LuaTable,
    name: &str,
) -> LuaResult<Option<HashMap<String, LuaTableValue>>> {
    match env.get::<LuaValue>(name)? {
        LuaValue::Nil => Ok(None),
        LuaValue::Table(t) => match table_to_value(t)? {
            LuaTableValue::Map(m)  => Ok(Some(m)),
            LuaTableValue::List(items) => {
                // Wrap an unexpected top-level array as a 1-indexed Map.
                let map = items
                    .into_iter()
                    .enumerate()
                    .map(|(i, v)| ((i + 1).to_string(), v))
                    .collect();
                Ok(Some(map))
            }
            _ => unreachable!(),
        },
        other => Err(LuaError::runtime(format!(
            "sub-table '{name}' is not a table (got {})",
            other.type_name()
        ))),
    }
}
