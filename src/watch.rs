//! Watching the disk: the project folders and the folders of the open files
//! tell when something changed, so that the tree, the Git marks and the open
//! file are looked at then, and not every second.
//!
//! When no watcher can be made (the limit of watches of the system, a network
//! drive), the app goes on as before: it looks at intervals.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

/// How many relevant events were heard, in all.
static EVENTS: AtomicU64 = AtomicU64::new(0);

/// Built things and downloaded ones: they change all the time and are never
/// what the user is looking at.
fn skipped(name: &str) -> bool {
    matches!(
        name,
        "node_modules" | "target" | ".venv" | "__pycache__" | ".DS_Store"
    )
}

/// Whether a change to `path` matters: the files of the work, and of `.git`
/// only what moves the marks (the index, HEAD, the references).
pub fn relevant(path: &Path) -> bool {
    let mut in_git = false;
    for part in path.components() {
        let name = part.as_os_str().to_string_lossy();
        if skipped(&name) {
            return false;
        }
        if name == ".git" {
            in_git = true;
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

/// The folders under `root` (itself included) whose changes matter, for a
/// watch of each alone: neither the skipped folders nor, in `.git`, more than
/// itself and its references. Links are not followed.
fn folders(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut todo = vec![root.to_path_buf()];
    while let Some(dir) = todo.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            if !entry.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if skipped(&name) {
                continue;
            }
            let path = entry.path();
            if name == ".git" {
                found.push(path.clone());
                todo.push(path.join("refs"));
            } else {
                todo.push(path);
            }
        }
        found.push(dir);
    }
    found
}

/// The watches of the app.
#[derive(Default)]
pub struct Watch {
    watcher: Option<RecommendedWatcher>,
    /// What is watched, with how.
    watched: Vec<(PathBuf, bool)>,
    /// The folders made in a watched one since: they are watched in turn.
    created: Arc<Mutex<Vec<PathBuf>>>,
    /// Something could not be watched: the app looks at intervals too.
    pub degraded: bool,
}

/// On Linux a recursive watch walks the whole tree on the calling thread,
/// `node_modules` and `.git/objects` included: tens of thousands of folders
/// in a big project, with the window frozen meanwhile. The folders that
/// matter are watched one by one instead. Elsewhere the system watches a
/// tree at once.
const ONE_BY_ONE: bool = cfg!(target_os = "linux");

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
            self.watch_created();
            return;
        }
        if wanted.is_empty() {
            *self = Watch::default();
            return;
        }
        let created = Arc::new(Mutex::new(Vec::new()));
        let heard = Arc::clone(&created);
        let made = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            let Ok(event) = event else {
                // Events were lost: everything is looked at again.
                EVENTS.fetch_add(1, Ordering::Relaxed);
                return;
            };
            if matches!(event.kind, EventKind::Access(_)) {
                return;
            }
            if ONE_BY_ONE && matches!(event.kind, EventKind::Create(_) | EventKind::Modify(_)) {
                let made: Vec<PathBuf> = event
                    .paths
                    .iter()
                    .filter(|p| relevant(p) && p.is_dir())
                    .cloned()
                    .collect();
                if !made.is_empty() {
                    if let Ok(mut list) = heard.lock() {
                        list.extend(made);
                    }
                }
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
            if *deep && ONE_BY_ONE {
                for dir in folders(path) {
                    if watcher.watch(&dir, RecursiveMode::NonRecursive).is_err() {
                        degraded = true;
                    }
                }
                continue;
            }
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
            created,
            degraded,
        };
    }

    /// Watches the folders made in the projects since the last time, and what
    /// they hold (a folder moved in, an archive unpacked).
    pub fn watch_created(&mut self) {
        let Some(watcher) = self.watcher.as_mut() else {
            return;
        };
        let made = match self.created.lock() {
            Ok(mut list) => std::mem::take(&mut *list),
            Err(_) => return,
        };
        for root in made {
            for dir in folders(&root) {
                if watcher.watch(&dir, RecursiveMode::NonRecursive).is_err() {
                    self.degraded = true;
                }
            }
        }
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
    fn only_the_folders_that_matter_are_watched_one_by_one() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for sub in [
            "src/app",
            "node_modules/a/b",
            "web/node_modules/c",
            "target/debug",
            ".git/objects/ab",
            ".git/refs/heads",
        ] {
            std::fs::create_dir_all(root.join(sub)).unwrap();
        }
        let mut found = folders(root);
        found.sort();
        let want: Vec<PathBuf> = [
            "",
            ".git",
            ".git/refs",
            ".git/refs/heads",
            "src",
            "src/app",
            "web",
        ]
        .iter()
        .map(|p| {
            if p.is_empty() {
                root.to_path_buf()
            } else {
                root.join(p)
            }
        })
        .collect();
        assert_eq!(found, want);
    }

    #[test]
    fn a_folder_made_after_the_watch_is_watched_too() {
        let dir = tempfile::tempdir().unwrap();
        let mut watch = Watch::default();
        watch.set(&[dir.path().to_path_buf()], &[]);
        std::fs::create_dir(dir.path().join("new")).unwrap();
        // The folder is heard, then watched at the next look.
        let listed = (0..50).any(|_| {
            std::thread::sleep(std::time::Duration::from_millis(100));
            !watch.created.lock().unwrap().is_empty()
        });
        assert!(listed || !ONE_BY_ONE);
        watch.set(&[dir.path().to_path_buf()], &[]);
        let before = EVENTS.load(Ordering::Relaxed);
        std::fs::write(dir.path().join("new/a.txt"), "x").unwrap();
        let heard = (0..50).any(|_| {
            std::thread::sleep(std::time::Duration::from_millis(100));
            EVENTS.load(Ordering::Relaxed) != before
        });
        assert!(heard);
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
