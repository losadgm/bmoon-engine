use std::{path::PathBuf, rc::Rc};

use ahash::AHashMap;

// ── ScriptId ──────────────────────────────────────────────────────────────────

/// Interned identifier for a registered script (e.g. `"cards.blood_moon"`).
///
/// Uses `Arc<str>` so IDs can be cheaply cloned and used as cache keys without
/// heap-allocating a new `String` each time.
pub type ScriptId = Rc<str>;

// ── ScriptMeta ────────────────────────────────────────────────────────────────

/// Static metadata for a registered script. Populated from the manifest at
/// startup; `version` is incremented on hot-reload to invalidate the cache.
pub struct ScriptMeta {
    pub path: PathBuf,
    pub version: u64,
}

// ── ScriptRegistry ────────────────────────────────────────────────────────────

/// Map of all scripts discovered from `manifest.toml` files under `assets/`.
///
/// Populated by `LuaScriptManager::build_registry`; treated as read-only
/// during gameplay (hot-reload aside).
#[derive(Default)]
pub struct ScriptRegistry {
    entries: AHashMap<ScriptId, ScriptMeta>,
}

impl ScriptRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert or overwrite a script entry.
    ///
    /// If the same id is registered twice (e.g. two packages declare the same
    /// script), the second registration wins, the version is bumped, and a
    /// warning is printed — matching the behaviour of the prototype.
    pub fn register(&mut self, id: ScriptId, path: PathBuf) {
        match self.entries.get_mut(&id) {
            Some(meta) => {
                eprintln!("[lua_script_manager] conflict: '{id}' already registered — overwriting");
                meta.path = path;
                meta.version += 1;
            }
            None => {
                self.entries.insert(id, ScriptMeta { path, version: 0 });
            }
        }
    }

    pub fn get(&self, id: &str) -> Option<&ScriptMeta> {
        self.entries.get(id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut ScriptMeta> {
        self.entries.get_mut(id)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}
