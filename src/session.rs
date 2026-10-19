use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSession {
    pub path: String,
    /// Content stored for untitled files and dirty real files.
    pub content: Option<String>,
    pub language: Option<String>,
    pub dirty: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Session {
    pub files: Vec<FileSession>,
    pub active_file: Option<String>,
    pub untitled_counter: u32,
}

impl Session {
    fn path() -> Option<PathBuf> {
        dirs::data_dir().map(|d| d.join("simpleedit").join("session.json"))
    }

    pub fn load() -> Self {
        Self::path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Serialized form, so the caller can skip a write when nothing changed.
    pub fn to_json(&self) -> Option<String> {
        serde_json::to_string(self).ok()
    }

    /// Writes already-serialized session JSON. The file holds unsaved buffer
    /// contents, so it is owner-only.
    pub fn write(json: &str) {
        if let Some(path) = Self::path() {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = crate::fsutil::write_atomic(&path, json.as_bytes(), true);
        }
    }
}
