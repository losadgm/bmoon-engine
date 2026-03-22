use std::path::{Path, PathBuf};

use serde::Deserialize;
use walkdir::WalkDir;

// ── Serde structs ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct Manifest {
    pub package: PackageMeta,
    #[serde(default, rename = "script")]
    pub scripts: Vec<ScriptEntry>,
}

#[derive(Deserialize)]
pub struct PackageMeta {
    pub name: String,
}

#[derive(Deserialize)]
pub struct ScriptEntry {
    pub id:   String,
    pub path: String,
}

// ── Loading ───────────────────────────────────────────────────────────────────

/// Parse a single `manifest.toml` file.
pub fn load_manifest(path: &Path) -> Result<Manifest, Box<dyn std::error::Error>> {
    Ok(toml::from_str(&std::fs::read_to_string(path)?)?)
}

/// Walk `root` recursively and yield every `manifest.toml` path found.
pub fn find_manifests(root: &Path) -> impl Iterator<Item = PathBuf> {
    WalkDir::new(root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name() == "manifest.toml")
        .map(|e| e.path().to_owned())
}
