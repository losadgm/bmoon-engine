/// The outcome of calling a Luau function from Rust.
///
/// The manager logs every `ScriptError` internally before returning — the
/// engine never needs to log at the call site, only decide how to react.
#[derive(Debug)]
pub enum CallResult<T> {
    /// The function ran and its return value converted successfully to `T`.
    Ok(T),
    /// The function is not defined in the script. Expected no-op — the engine
    /// applies its default behaviour (e.g. a card with no `when_leaves_battlefield`).
    FunctionNotFound,
    /// The function ran but something went wrong.
    ScriptError(LuaScriptError),
}

#[derive(Debug)]
pub enum LuaScriptError {
    /// A Luau runtime error (nil index, type error, explicit `error()`, etc.).
    RuntimeError(String),
    /// The function returned a value that could not be converted to `T`.
    ConversionError(String),
}

// ── Convenience conversion ────────────────────────────────────────────────────

/// Enables `manager.call(...).into().unwrap_or(default)` at non-critical sites.
impl<T> From<CallResult<T>> for Option<T> {
    fn from(r: CallResult<T>) -> Self {
        match r {
            CallResult::Ok(v) => Some(v),
            _                 => None,
        }
    }
}
