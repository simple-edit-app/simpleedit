//! Watching the disk: the project folders and the folders of the open files
//! tell when something changed, so that the tree, the Git marks and the open
//! file are looked at then, and not every second.
//!
//! When no watcher can be made (the limit of watches of the system, a network
//! drive), the app goes on as before: it looks at intervals.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

/// How many relevant events were heard, in all.
static EVENTS: AtomicU64 = AtomicU64::new(0);

/// Whether a change to `path` matters: the files of the work, and of `.git`
/// only what moves the marks (the index, HEAD, the references).
pub fn relevant(path: &Path) -> bool {
    let mut in_git = false;
    for part in path.components() {
        let name = part.as_os_str().to_string_lossy();
        match name.as_ref() {
            // Built things and downloaded ones change all the time and are
            // never what the user is looking at.
            "node_modules" | "target" | ".venv" | "__pycache__" | ".DS_Store" => return false,
            ".git" => in_git = true,
            _ => {}
        }
    }
    if !in_git {
        return true;
    }
    let after: Vec<String> = path
        .components()
        .skip_while(|c| c.as_os_str() != ".git")
        .skip(1)
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    matches!(
        after.first().map(String::as_str),
        Some("index" | "HEAD" | "refs" | "MERGE_HEAD" | "rebase-merge" | "rebase-apply")
    )
}

/// The watches of the app.
#[derive(Default)]
pub struct Watch {
    watcher: Option<RecommendedWatcher>,
    /// What is watched, with how.
    watched: Vec<(PathBuf, bool)>,
    /// Something could not be watched: the app looks at intervals too.
    pub degraded: bool,
}

impl Watch {
    /// Watches `recursive` folders with everything below them and `single`
    /// folders alone, and no more than that: a change of what was watched
    /// before is followed.
    pub fn set(&mut self, recursive: &[PathBuf], single: &[PathBuf]) {
        let mut wanted: Vec<(PathBuf, bool)> =
            recursive.iter().map(|p| (p.clone(), true)).collect();
        for dir in single {
            if !recursive.iter().any(|r| dir.starts_with(r))
                && !wanted.iter().any(|(p, _)| p == dir)
            {
                wanted.push((dir.clone(), false));
            }
        }
        wanted.sort();
        if wanted == self.watched && self.watcher.is_some() {
            return;
        }
        if wanted.is_empty() {
            *self = Watch::default();
            return;
        }
        let made = notify::recommended_watcher(|event: notify::Result<notify::Event>| {
            let Ok(event) = event else {
                // Events were lost: everything is looked at again.
                EVENTS.fetch_add(1, Ordering::Relaxed);
                return;
            };
            if matches!(event.kind, EventKind::Access(_)) {
                return;
            }
            if event.paths.iter().any(|p| relevant(p)) {
                EVENTS.fetch_add(1, Ordering::Relaxed);
            }
        });
        let mut watcher = match made {
            Ok(w) => w,
            Err(_) => {
                *self = Watch {
                    degraded: true,
                    ..Watch::default()
                };
                return;
            }
        };
        let mut degraded = false;
        for (path, deep) in &wanted {
            let mode = if *deep {
                RecursiveMode::Recursive
            } else {
                RecursiveMode::NonRecursive
            };
            if watcher.watch(path, mode).is_err() {
                degraded = true;
            }
        }
        *self = Watch {
            watcher: Some(watcher),
            watched: wanted,
            degraded,
        };
    }

    /// Whether the disk is watched (and so need not be looked at often).
    pub fn active(&self) -> bool {
        self.watcher.is_some() && !self.degraded
    }
}

/// A message when something changed, at most every 200 ms: an idle disk does
/// not wake the window.
pub fn changes() -> iced::Subscription<()> {
    iced::Subscription::run(|| {
        iced::stream::channel(1, async |mut output| {
            use iced::futures::SinkExt;
            let mut seen = EVENTS.load(Ordering::Relaxed);
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                let now = EVENTS.load(Ordering::Relaxed);
                if now != seen {
                    seen = now;
                    let _ = output.send(()).await;
                }
            }
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_folders_are_left_out_and_git_only_where_it_matters() {
        assert!(relevant(Path::new("/p/src/main.rs")));
        assert!(!relevant(Path::new("/p/node_modules/a/b.js")));
        assert!(!relevant(Path::new("/p/target/debug/x")));
        assert!(relevant(Path::new("/p/.git/index")));
        assert!(relevant(Path::new("/p/.git/HEAD")));
        assert!(relevant(Path::new("/p/.git/refs/heads/main")));
        assert!(!relevant(Path::new("/p/.git/objects/ab/cdef")));
        assert!(!relevant(Path::new("/p/.git/index.lock")));
    }

    #[test]
    fn a_change_in_a_watched_folder_is_heard() {
        let dir = tempfile::tempdir().unwrap();
        let mut watch = Watch::default();
        watch.set(&[dir.path().to_path_buf()], &[]);
        assert!(watch.active());
        let before = EVENTS.load(Ordering::Relaxed);
        std::fs::write(dir.path().join("a.txt"), "x").unwrap();
        let heard = (0..50).any(|_| {
            std::thread::sleep(std::time::Duration::from_millis(100));
            EVENTS.load(Ordering::Relaxed) != before
        });
        assert!(heard);
    }
}
