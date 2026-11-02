use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::tabs::DiskStamp;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSession {
    pub path: String,
    /// The exact bytes of a path that isn't valid UTF-8 (Unix only); `path`
    /// then only holds a readable approximation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_raw: Option<Vec<u8>>,
    /// Content stored for untitled files and dirty real files.
    pub content: Option<String>,
    pub language: Option<String>,
    pub dirty: bool,
    /// `\r\n` line endings on disk.
    #[serde(default)]
    pub crlf: bool,
    /// Whether the file ends with a line break (new files do).
    #[serde(default = "yes")]
    pub final_newline: bool,
    /// The state of the file on disk when `content` was read, to notice
    /// edits made while the app was closed.
    #[serde(default)]
    pub stamp: Option<DiskStamp>,
}

fn yes() -> bool {
    true
}

impl FileSession {
    pub fn set_path(&mut self, path: &Path) {
        self.path = path.to_string_lossy().into_owned();
        self.path_raw = None;
        #[cfg(unix)]
        if path.to_str().is_none() {
            use std::os::unix::ffi::OsStrExt;
            self.path_raw = Some(path.as_os_str().as_bytes().to_vec());
        }
    }

    pub fn path(&self) -> PathBuf {
        #[cfg(unix)]
        if let Some(raw) = &self.path_raw {
            use std::os::unix::ffi::OsStrExt;
            return PathBuf::from(std::ffi::OsStr::from_bytes(raw));
        }
        PathBuf::from(&self.path)
    }
}

/// The right to write the session, held while the app runs. A second window
/// would otherwise overwrite the session of the first when it closes.
#[derive(Debug)]
pub struct InstanceLock {
    _file: std::fs::File,
}

impl InstanceLock {
    /// `None` when another running instance already holds the lock. A system
    /// that can't lock at all counts as free: the app must still start.
    pub fn acquire() -> Option<InstanceLock> {
        let dir = crate::fsutil::data_dir()?.join("simpleedit");
        std::fs::create_dir_all(&dir).ok()?;
        let file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(dir.join("instance.lock"))
            .ok()?;
        match file.try_lock() {
            Err(std::fs::TryLockError::WouldBlock) => None,
            _ => Some(InstanceLock { _file: file }),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Session {
    pub files: Vec<FileSession>,
    pub active_file: Option<String>,
    pub untitled_counter: u32,
}

/// What reading the session file gave.
pub struct Loaded {
    pub session: Session,
    /// The file was unreadable and was set aside under this name.
    pub set_aside: Option<PathBuf>,
}

impl Session {
    fn path() -> Option<PathBuf> {
        crate::fsutil::data_dir().map(|d| d.join("simpleedit").join("session.json"))
    }

    /// A missing file is a fresh start; a corrupted one is moved aside so the
    /// next autosave doesn't destroy the only copy of unsaved buffers.
    pub fn load() -> Loaded {
        let Some(path) = Self::path() else {
            return Loaded {
                session: Self::default(),
                set_aside: None,
            };
        };
        let Ok(raw) = std::fs::read_to_string(&path) else {
            return Loaded {
                session: Self::default(),
                set_aside: path
                    .exists()
                    .then(|| crate::fsutil::set_aside(&path))
                    .flatten(),
            };
        };
        match serde_json::from_str(&raw) {
            Ok(session) => Loaded {
                session,
                set_aside: None,
            },
            Err(_) => Loaded {
                session: Self::default(),
                set_aside: crate::fsutil::set_aside(&path),
            },
        }
    }

    /// Serialized form, so the caller can skip a write when nothing changed.
    pub fn to_json(&self) -> Option<String> {
        serde_json::to_string(self).ok()
    }

    /// Writes already-serialized session JSON. The file holds unsaved buffer
    /// contents, so it is owner-only.
    pub fn write(json: &str) -> std::io::Result<()> {
        let path = Self::path().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "no data directory")
        })?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        crate::fsutil::write_atomic(&path, json.as_bytes(), true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &Path) -> FileSession {
        let mut fs = FileSession {
            path: String::new(),
            path_raw: None,
            content: Some("x".into()),
            language: None,
            dirty: true,
            crlf: false,
            final_newline: true,
            stamp: None,
        };
        fs.set_path(path);
        fs
    }

    #[test]
    fn a_session_round_trips_through_json() {
        let mut fs = entry(Path::new("/a/b.txt"));
        fs.crlf = true;
        fs.stamp = DiskStamp::of(Path::new("/"));
        let session = Session {
            files: vec![fs],
            active_file: Some("/a/b.txt".into()),
            untitled_counter: 3,
        };
        let back: Session = serde_json::from_str(&session.to_json().unwrap()).unwrap();
        assert_eq!(back.files[0].path(), PathBuf::from("/a/b.txt"));
        assert!(back.files[0].crlf);
        assert_eq!(back.files[0].stamp, session.files[0].stamp);
        assert_eq!(back.untitled_counter, 3);
    }

    #[test]
    fn an_unreadable_session_file_is_kept_aside_not_overwritten() {
        let path = Session::path().unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{ not json").unwrap();
        let loaded = Session::load();
        assert!(loaded.session.files.is_empty());
        let aside = loaded.set_aside.expect("the broken file is kept");
        assert_eq!(std::fs::read_to_string(aside).unwrap(), "{ not json");
        assert!(!path.exists());
    }

    #[test]
    fn only_one_instance_holds_the_lock() {
        let first = InstanceLock::acquire();
        assert!(first.is_some());
        assert!(InstanceLock::acquire().is_none());
        drop(first);
        assert!(InstanceLock::acquire().is_some());
    }

    #[test]
    fn sessions_written_by_older_versions_still_load() {
        let old = r#"{"files":[{"path":"/a","content":"x","language":null,"dirty":true}],
                      "active_file":null,"untitled_counter":1}"#;
        let session: Session = serde_json::from_str(old).unwrap();
        assert!(session.files[0].final_newline);
        assert!(!session.files[0].crlf);
    }

    #[cfg(unix)]
    #[test]
    fn a_path_that_is_not_utf8_survives() {
        use std::os::unix::ffi::OsStrExt;
        let path = PathBuf::from(std::ffi::OsStr::from_bytes(b"/tmp/caf\xe9.txt"));
        let fs = entry(&path);
        let back: FileSession = serde_json::from_str(&serde_json::to_string(&fs).unwrap()).unwrap();
        assert_eq!(back.path(), path);
    }
}
