//! Bookmarks: marked lines, per file, kept across restarts.
//!
//! Lines are 0-based. They are stored by path in the app's data directory,
//! next to the session, and follow the text when lines are added or removed
//! above them.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Bookmarks {
    /// Sorted, duplicate-free lines by file path. A file without bookmarks
    /// has no entry.
    files: BTreeMap<String, Vec<usize>>,
    #[serde(skip)]
    dirty: bool,
}

fn key(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

impl Bookmarks {
    fn store() -> Option<PathBuf> {
        crate::fsutil::data_dir().map(|d| d.join("simpleedit").join("bookmarks.json"))
    }

    pub fn load() -> Self {
        let Some(path) = Self::store() else {
            return Self::default();
        };
        let Ok(raw) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        serde_json::from_str(&raw).unwrap_or_else(|_| {
            // Keep the unreadable file instead of overwriting it.
            let _ = crate::fsutil::set_aside(&path);
            Self::default()
        })
    }

    /// Writes the bookmarks if they changed since the last write. A failed
    /// write is retried on the next call.
    pub fn save(&mut self) {
        if !self.dirty {
            return;
        }
        let (Some(path), Ok(json)) = (Self::store(), serde_json::to_string(self)) else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if crate::fsutil::write_atomic(&path, json.as_bytes(), true).is_ok() {
            self.dirty = false;
        }
    }

    /// Forgets the bookmarks of files that no longer exist (untitled buffers
    /// are not files).
    pub fn prune_missing(&mut self) {
        let before = self.files.len();
        self.files
            .retain(|path, _| path.starts_with("untitled://") || Path::new(path).exists());
        if self.files.len() != before {
            self.dirty = true;
        }
    }

    /// The bookmarked lines of a file, in order.
    pub fn lines(&self, path: &Path) -> &[usize] {
        self.files.get(&key(path)).map_or(&[], Vec::as_slice)
    }

    pub fn has(&self, path: &Path, line: usize) -> bool {
        self.lines(path).binary_search(&line).is_ok()
    }

    /// Adds the bookmark, or removes it when the line already has one.
    /// Returns whether the line is bookmarked afterwards.
    pub fn toggle(&mut self, path: &Path, line: usize) -> bool {
        let lines = self.files.entry(key(path)).or_default();
        let added = match lines.binary_search(&line) {
            Ok(i) => {
                lines.remove(i);
                false
            }
            Err(i) => {
                lines.insert(i, line);
                true
            }
        };
        if lines.is_empty() {
            self.files.remove(&key(path));
        }
        self.dirty = true;
        added
    }

    /// Removes every bookmark of a file.
    pub fn clear(&mut self, path: &Path) {
        if self.files.remove(&key(path)).is_some() {
            self.dirty = true;
        }
    }

    /// Moves a file's bookmarks to a new path (an untitled file that was
    /// saved).
    pub fn rename(&mut self, from: &Path, to: &Path) {
        if let Some(lines) = self.files.remove(&key(from)) {
            self.files.insert(key(to), lines);
            self.dirty = true;
        }
    }

    /// The first bookmark after `line` among the first `total` lines, or,
    /// past the last one, the first of the file.
    pub fn next(&self, path: &Path, line: usize, total: usize) -> Option<usize> {
        let valid = || self.lines(path).iter().copied().filter(|l| *l < total);
        valid().find(|l| *l > line).or_else(|| valid().next())
    }

    /// The last bookmark before `line`, or, before the first one, the last
    /// of the file.
    pub fn previous(&self, path: &Path, line: usize, total: usize) -> Option<usize> {
        let valid = || self.lines(path).iter().copied().filter(|l| *l < total);
        valid()
            .rev()
            .find(|l| *l < line)
            .or_else(|| valid().next_back())
    }

    /// An edit at `top` added (`delta` > 0) or removed (`delta` < 0) lines
    /// below it: bookmarks under the change follow it, those on removed
    /// lines go.
    pub fn shift(&mut self, path: &Path, top: usize, delta: isize) {
        let Some(lines) = self.files.get_mut(&key(path)) else {
            return;
        };
        let removed = delta.unsigned_abs();
        let moved: Vec<usize> = lines
            .iter()
            .filter_map(|&l| {
                if l <= top {
                    Some(l)
                } else if delta >= 0 {
                    Some(l + removed)
                } else if l <= top + removed {
                    None
                } else {
                    Some(l - removed)
                }
            })
            .collect();
        if *lines != moved {
            *lines = moved;
            self.dirty = true;
        }
        if lines.is_empty() {
            self.files.remove(&key(path));
        }
    }

    /// The text of a file was replaced as a whole (a line moved or deleted,
    /// undo, format): bookmarks above and below the changed region follow
    /// their lines; inside it they stay put while there is a line to hold
    /// them.
    pub fn remap(&mut self, path: &Path, old: &str, new: &str) {
        let Some(lines) = self.files.get_mut(&key(path)) else {
            return;
        };
        let old: Vec<&str> = old.split('\n').collect();
        let new: Vec<&str> = new.split('\n').collect();
        let common = old.len().min(new.len());
        let prefix = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
        let suffix = old
            .iter()
            .rev()
            .zip(new.iter().rev())
            .take(common - prefix)
            .take_while(|(a, b)| a == b)
            .count();
        let (old_end, new_end) = (old.len() - suffix, new.len() - suffix);
        let moved: Vec<usize> = lines
            .iter()
            .filter_map(|&l| {
                if l < prefix {
                    Some(l)
                } else if l >= old_end {
                    Some(l - old_end + new_end)
                } else {
                    (l < new_end).then_some(l)
                }
            })
            .collect();
        if *lines != moved {
            *lines = moved;
            self.dirty = true;
        }
        if lines.is_empty() {
            self.files.remove(&key(path));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file() -> PathBuf {
        PathBuf::from("/tmp/a.rs")
    }

    fn with(lines: &[usize]) -> Bookmarks {
        let mut b = Bookmarks::default();
        for &l in lines {
            b.toggle(&file(), l);
        }
        b
    }

    #[test]
    fn toggling_adds_then_removes_and_keeps_lines_sorted() {
        let mut b = with(&[9, 2, 5]);
        assert_eq!(b.lines(&file()), &[2, 5, 9]);
        assert!(b.has(&file(), 5));
        assert!(!b.toggle(&file(), 5));
        assert!(!b.has(&file(), 5));
        assert!(b.toggle(&file(), 5));
    }

    #[test]
    fn an_empty_file_has_no_entry_and_files_are_independent() {
        let mut b = with(&[3]);
        b.toggle(&file(), 3);
        assert!(b.files.is_empty());
        b.toggle(Path::new("/tmp/other"), 1);
        assert!(b.lines(&file()).is_empty());
        assert_eq!(b.lines(Path::new("/tmp/other")), &[1]);
    }

    #[test]
    fn clear_removes_only_that_file() {
        let mut b = with(&[1, 2]);
        b.toggle(Path::new("/tmp/other"), 7);
        b.clear(&file());
        assert!(b.lines(&file()).is_empty());
        assert_eq!(b.lines(Path::new("/tmp/other")), &[7]);
    }

    #[test]
    fn rename_moves_the_bookmarks() {
        let mut b = with(&[4]);
        b.rename(&file(), Path::new("/tmp/b.rs"));
        assert!(b.lines(&file()).is_empty());
        assert_eq!(b.lines(Path::new("/tmp/b.rs")), &[4]);
    }

    #[test]
    fn next_and_previous_wrap_around_and_ignore_lines_past_the_end() {
        let b = with(&[2, 10, 50]);
        let f = file();
        assert_eq!(b.next(&f, 0, 100), Some(2));
        assert_eq!(b.next(&f, 2, 100), Some(10));
        assert_eq!(b.next(&f, 50, 100), Some(2));
        assert_eq!(b.previous(&f, 10, 100), Some(2));
        assert_eq!(b.previous(&f, 2, 100), Some(50));
        // The file shrank: line 50 no longer exists.
        assert_eq!(b.next(&f, 10, 20), Some(2));
        assert_eq!(b.previous(&f, 2, 20), Some(10));
        assert_eq!(b.next(Path::new("/nope"), 0, 10), None);
    }

    #[test]
    fn inserting_lines_pushes_the_bookmarks_below_down() {
        let mut b = with(&[1, 5, 8]);
        b.shift(&file(), 5, 2);
        assert_eq!(b.lines(&file()), &[1, 5, 10]);
    }

    #[test]
    fn removing_lines_drops_their_bookmarks_and_pulls_the_rest_up() {
        let mut b = with(&[1, 6, 7, 12]);
        // Two lines removed below line 5: lines 6 and 7 were among them.
        b.shift(&file(), 5, -2);
        assert_eq!(b.lines(&file()), &[1, 10]);
    }

    #[test]
    fn remap_follows_a_deleted_line() {
        let mut b = with(&[0, 1, 3]);
        b.remap(&file(), "a\nb\nc\nd", "a\nc\nd");
        assert_eq!(b.lines(&file()), &[0, 2]);
    }

    #[test]
    fn remap_follows_an_inserted_line() {
        let mut b = with(&[0, 2]);
        b.remap(&file(), "a\nb\nc", "a\nx\nb\nc");
        assert_eq!(b.lines(&file()), &[0, 3]);
    }

    #[test]
    fn remap_keeps_bookmarks_when_lines_swap() {
        let mut b = with(&[1, 2]);
        b.remap(&file(), "a\nb\nc\nd", "a\nc\nb\nd");
        assert_eq!(b.lines(&file()), &[1, 2]);
    }

    #[test]
    fn remap_drops_bookmarks_when_the_text_shrinks_under_them() {
        let mut b = with(&[4]);
        b.remap(&file(), "a\nb\nc\nd\ne", "x");
        assert!(b.lines(&file()).is_empty());
    }

    #[test]
    fn saving_and_loading_round_trips() {
        let b = with(&[3, 9]);
        let json = serde_json::to_string(&b).unwrap();
        let back: Bookmarks = serde_json::from_str(&json).unwrap();
        assert_eq!(back.lines(&file()), &[3, 9]);
        assert!(!back.dirty);
    }
}
