//! Local history, as IntelliJ IDEA has it: what files were, kept on this
//! machine whatever Git knows, with labels on moments that matter.
//!
//! Texts are stored once, by their hash, in `blobs/`; each file has its list
//! of revisions in `files/`. Old revisions go by working days (days the file
//! changed), and the oldest of all when the whole grows too big.

use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Bigger texts are not kept.
const MAX_TEXT: usize = 5 * 1024 * 1024;
/// How much the history takes on the disk at most.
pub const MAX_BYTES: u64 = 512 * 1024 * 1024;
/// Labels kept at most.
const MAX_LABELS: usize = 500;
/// Files read at once when a folder is about to go.
const MAX_TREE_FILES: usize = 2000;
const DAY: i64 = 24 * 60 * 60;

/// Writing goes one at a time: two lists read, changed and written back at
/// once would lose what one of them added.
static WRITING: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn writing() -> std::sync::MutexGuard<'static, ()> {
    WRITING.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Revision {
    /// Seconds since the epoch.
    pub time: i64,
    /// The hash of the text.
    pub blob: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct FileRecord {
    pub path: PathBuf,
    pub revisions: Vec<Revision>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Label {
    pub time: i64,
    pub name: String,
    /// Put by the app (a commit), not by hand.
    pub auto: bool,
}

/// A line of the history of a file or a folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    Revision {
        path: PathBuf,
        time: i64,
        blob: String,
    },
    Label(Label),
}

impl Row {
    pub fn time(&self) -> i64 {
        match self {
            Row::Revision { time, .. } => *time,
            Row::Label(label) => label.time,
        }
    }
}

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

fn hash(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        text.push_str(&format!("{byte:02x}"));
    }
    text
}

/// Writes a file readable by its owner only (copies of private files).
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)?.write_all(bytes)
}

fn write_atomically(path: &Path, bytes: &[u8]) -> Option<()> {
    let temporary = path.with_extension("tmp");
    write_private(&temporary, bytes).ok()?;
    std::fs::rename(&temporary, path).ok()
}

/// Where the history is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Store {
    dir: PathBuf,
}

impl Store {
    pub fn at(dir: PathBuf) -> Self {
        Store { dir }
    }

    /// The history of this machine.
    pub fn global() -> Option<Self> {
        let dir = crate::fsutil::data_dir()?;
        Some(Store::at(dir.join("simpleedit").join("local-history")))
    }

    fn file_path(&self, path: &Path) -> PathBuf {
        let name = hash(path.to_string_lossy().as_bytes());
        self.dir.join("files").join(format!("{name}.json"))
    }

    fn labels_path(&self) -> PathBuf {
        self.dir.join("labels.json")
    }

    /// Keeps a text of a file, unless it is what was kept last.
    pub fn record(&self, path: &Path, text: &str, time: i64) -> Option<()> {
        if text.len() > MAX_TEXT || text.as_bytes().contains(&0) {
            return None;
        }
        let blob = hash(text.as_bytes());
        let _writing = writing();
        let mut record = self.file(path);
        if record.revisions.last().is_some_and(|r| r.blob == blob) {
            return Some(());
        }
        std::fs::create_dir_all(self.dir.join("blobs")).ok()?;
        std::fs::create_dir_all(self.dir.join("files")).ok()?;
        let stored = self.dir.join("blobs").join(&blob);
        if !stored.exists() {
            write_atomically(&stored, text.as_bytes())?;
        }
        record.path = path.to_path_buf();
        record.revisions.push(Revision { time, blob });
        let json = serde_json::to_vec(&record).ok()?;
        write_atomically(&self.file_path(path), &json)
    }

    /// Keeps the text of a file as it is on the disk.
    pub fn record_file(&self, path: &Path, time: i64) -> Option<()> {
        let bytes = std::fs::read(path).ok()?;
        self.record(path, std::str::from_utf8(&bytes).ok()?, time)
    }

    /// Keeps every file of a folder (before it is deleted).
    pub fn record_tree(&self, dir: &Path, time: i64) {
        let mut pending = vec![dir.to_path_buf()];
        let mut seen = 0;
        while let Some(folder) = pending.pop() {
            let Ok(entries) = std::fs::read_dir(&folder) else {
                continue;
            };
            for entry in entries.filter_map(Result::ok) {
                let Ok(kind) = entry.file_type() else {
                    continue;
                };
                if kind.is_dir() {
                    if entry.file_name() != ".git" && entry.file_name() != "node_modules" {
                        pending.push(entry.path());
                    }
                } else if kind.is_file() {
                    seen += 1;
                    if seen > MAX_TREE_FILES {
                        return;
                    }
                    let _ = self.record_file(&entry.path(), time);
                }
            }
        }
    }

    pub fn file(&self, path: &Path) -> FileRecord {
        std::fs::read(self.file_path(path))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    /// Every file known, gone or not.
    fn all_files(&self) -> Vec<FileRecord> {
        let Ok(entries) = std::fs::read_dir(self.dir.join("files")) else {
            return Vec::new();
        };
        entries
            .filter_map(Result::ok)
            .filter_map(|e| std::fs::read(e.path()).ok())
            .filter_map(|bytes| serde_json::from_slice(&bytes).ok())
            .collect()
    }

    pub fn text(&self, blob: &str) -> Option<String> {
        // A name from the index, never a path.
        if !blob.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        std::fs::read_to_string(self.dir.join("blobs").join(blob)).ok()
    }

    pub fn labels(&self) -> Vec<Label> {
        std::fs::read(self.labels_path())
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn put_label(&self, name: &str, auto: bool, time: i64) -> Option<()> {
        let _writing = writing();
        let mut labels = self.labels();
        labels.push(Label {
            time,
            name: name.to_string(),
            auto,
        });
        let extra = labels.len().saturating_sub(MAX_LABELS);
        labels.drain(..extra);
        std::fs::create_dir_all(&self.dir).ok()?;
        write_atomically(&self.labels_path(), &serde_json::to_vec(&labels).ok()?)
    }

    /// The history of a file or of what a folder holds, latest first, labels
    /// included from the first revision on.
    pub fn rows(&self, scope: &Path) -> Vec<Row> {
        let mut rows: Vec<Row> = self
            .all_files()
            .into_iter()
            .filter(|f| f.path.starts_with(scope))
            .flat_map(|f| {
                let path = f.path;
                f.revisions.into_iter().map(move |r| Row::Revision {
                    path: path.clone(),
                    time: r.time,
                    blob: r.blob,
                })
            })
            .collect();
        let oldest = rows.iter().map(Row::time).min();
        if let Some(oldest) = oldest {
            rows.extend(
                self.labels()
                    .into_iter()
                    .filter(|l| l.time >= oldest)
                    .map(Row::Label),
            );
        }
        // Stable: at the same second, what was put last comes first.
        rows.reverse();
        rows.sort_by_key(|r| std::cmp::Reverse(r.time()));
        rows
    }

    /// The text each file of a folder had at a moment, for those that have
    /// changed since.
    pub fn state_at(&self, scope: &Path, time: i64) -> Vec<(PathBuf, String)> {
        self.all_files()
            .into_iter()
            .filter(|f| f.path.starts_with(scope))
            .filter_map(|f| {
                let then = f.revisions.iter().rev().find(|r| r.time <= time)?;
                let last = f.revisions.last()?;
                if last.blob == then.blob {
                    return None;
                }
                Some((f.path.clone(), self.text(&then.blob)?))
            })
            .collect()
    }

    /// Forgets what is older than `days` working days (those a file changed
    /// on), then the oldest of all while the history is over `max_bytes`.
    pub fn prune(&self, days: u32, max_bytes: u64) {
        let _writing = writing();
        let mut files = self.all_files();
        for file in &mut files {
            let worked: BTreeSet<i64> = file.revisions.iter().map(|r| r.time / DAY).collect();
            let kept: BTreeSet<i64> = worked.into_iter().rev().take(days as usize).collect();
            file.revisions.retain(|r| kept.contains(&(r.time / DAY)));
        }
        let size = |file: &[FileRecord]| {
            let blobs: HashSet<&str> = file
                .iter()
                .flat_map(|f| f.revisions.iter().map(|r| r.blob.as_str()))
                .collect();
            blobs
                .iter()
                .filter_map(|b| std::fs::metadata(self.dir.join("blobs").join(b)).ok())
                .map(|m| m.len())
                .sum::<u64>()
        };
        while size(&files) > max_bytes {
            let oldest = files
                .iter_mut()
                .filter(|f| !f.revisions.is_empty())
                .min_by_key(|f| f.revisions[0].time);
            match oldest {
                Some(file) => {
                    file.revisions.remove(0);
                }
                None => break,
            }
        }
        for file in &files {
            let at = self.file_path(&file.path);
            if file.revisions.is_empty() {
                let _ = std::fs::remove_file(at);
            } else if let Ok(json) = serde_json::to_vec(file) {
                let _ = write_atomically(&at, &json);
            }
        }
        let used: HashSet<&str> = files
            .iter()
            .flat_map(|f| f.revisions.iter().map(|r| r.blob.as_str()))
            .collect();
        if let Ok(entries) = std::fs::read_dir(self.dir.join("blobs")) {
            for entry in entries.filter_map(Result::ok) {
                if !used.contains(entry.file_name().to_string_lossy().as_ref()) {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }
}

// ─── What differs ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Same,
    /// In the revision, not in the file now.
    Old,
    /// In the file now, not in the revision.
    New,
}

/// A group of changes between a revision and the file now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    /// Lines of each text it covers.
    pub old: std::ops::Range<usize>,
    pub new: std::ops::Range<usize>,
    pub lines: Vec<(LineKind, String)>,
}

fn split_lines(text: &str) -> Vec<&str> {
    text.split_inclusive('\n').collect()
}

/// The changes from `old` to `new`, with a few lines around.
pub fn chunks(old: &str, new: &str) -> Vec<Chunk> {
    use similar::{ChangeTag, TextDiff};
    let diff = TextDiff::from_lines(old, new);
    diff.grouped_ops(3)
        .iter()
        .filter_map(|group| {
            let (first, last) = (group.first()?, group.last()?);
            let lines = group
                .iter()
                .flat_map(|op| diff.iter_changes(op))
                .map(|change| {
                    let kind = match change.tag() {
                        ChangeTag::Equal => LineKind::Same,
                        ChangeTag::Delete => LineKind::Old,
                        ChangeTag::Insert => LineKind::New,
                    };
                    (kind, change.value().trim_end_matches('\n').to_string())
                })
                .collect();
            Some(Chunk {
                old: first.old_range().start..last.old_range().end,
                new: first.new_range().start..last.new_range().end,
                lines,
            })
        })
        .collect()
}

/// `new` with one chunk put back as `old` has it.
pub fn revert_chunk(old: &str, new: &str, chunk: &Chunk) -> String {
    let (old_lines, new_lines) = (split_lines(old), split_lines(new));
    let mut out = String::with_capacity(new.len());
    out.extend(new_lines[..chunk.new.start].iter().copied());
    out.extend(old_lines[chunk.old.clone()].iter().copied());
    out.extend(new_lines[chunk.new.end..].iter().copied());
    out
}

/// `text` with the lines from `at` (`count` of them) replaced by `lines`.
pub fn replace_lines(text: &str, at: usize, count: usize, lines: &[String]) -> String {
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let current = split_lines(text);
    let start = at.min(current.len());
    let end = (at + count).min(current.len());
    let mut out = String::with_capacity(text.len());
    out.extend(current[..start].iter().copied());
    for line in lines {
        out.push_str(line);
        out.push_str(eol);
    }
    out.extend(current[end..].iter().copied());
    out
}

/// The whole of both texts, as a diff the Git view reads: every line is in
/// it, for the two files side by side.
pub fn whole_diff(name: &str, old: &str, new: &str) -> crate::git::FileDiff {
    let body = similar::TextDiff::from_lines(old, new)
        .unified_diff()
        .context_radius(usize::MAX / 4)
        .header(&format!("a/{name}"), &format!("b/{name}"))
        .to_string();
    let text = format!("diff --git a/{name} b/{name}\n{body}");
    crate::git::diff::parse_diff(&text)
        .into_iter()
        .next()
        .unwrap_or_else(|| crate::git::FileDiff {
            path: name.to_string(),
            ..Default::default()
        })
}

/// The way from `old` to `new`, as a patch of `name`.
pub fn patch(name: &str, old: &str, new: &str) -> String {
    similar::TextDiff::from_lines(old, new)
        .unified_diff()
        .context_radius(3)
        .header(&format!("a/{name}"), &format!("b/{name}"))
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::at(dir.path().to_path_buf());
        (dir, store)
    }

    #[cfg(unix)]
    #[test]
    fn kept_copies_are_private() {
        use std::os::unix::fs::PermissionsExt;
        let (dir, store) = store();
        store.record(Path::new("/p/.env"), "SECRET=1", 10);
        for sub in ["blobs", "files"] {
            for entry in std::fs::read_dir(dir.path().join(sub)).unwrap() {
                let mode = entry.unwrap().metadata().unwrap().permissions().mode();
                assert_eq!(mode & 0o077, 0, "{sub}");
            }
        }
    }

    #[test]
    fn a_text_is_kept_once_and_read_back() {
        let (_dir, store) = store();
        let path = Path::new("/p/a.rs");
        store.record(path, "one", 10);
        store.record(path, "one", 20);
        store.record(path, "two", 30);
        let revisions = store.file(path).revisions;
        assert_eq!(revisions.len(), 2);
        assert_eq!(store.text(&revisions[0].blob).as_deref(), Some("one"));
        assert_eq!(
            store.text("../secret"),
            None,
            "a blob is a hash, not a path"
        );
    }

    #[test]
    fn binary_and_huge_texts_are_left_out() {
        let (_dir, store) = store();
        let path = Path::new("/p/a.bin");
        assert_eq!(store.record(path, "a\0b", 1), None);
        assert_eq!(store.record(path, &"x".repeat(MAX_TEXT + 1), 1), None);
        assert!(store.file(path).revisions.is_empty());
    }

    #[test]
    fn a_folder_shows_its_files_and_the_labels_latest_first() {
        let (_dir, store) = store();
        store.put_label("too old", false, 1);
        store.record(Path::new("/p/a.rs"), "a", 10);
        store.record(Path::new("/p/sub/b.rs"), "b", 30);
        store.record(Path::new("/q/c.rs"), "c", 40);
        store.put_label("before", false, 20);
        let rows = store.rows(Path::new("/p"));
        let times: Vec<i64> = rows.iter().map(Row::time).collect();
        assert_eq!(times, [30, 20, 10]);
        assert!(matches!(&rows[1], Row::Label(l) if l.name == "before"));
    }

    #[test]
    fn a_deleted_file_stays_in_the_history_of_its_folder() {
        let (_dir, store) = store();
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("gone.txt");
        std::fs::write(&file, "keep me").unwrap();
        store.record_tree(dir.path(), 5);
        std::fs::remove_file(&file).unwrap();
        let rows = store.rows(dir.path());
        assert!(matches!(&rows[0], Row::Revision { path, .. } if *path == file));
    }

    #[test]
    fn a_moment_gives_back_what_changed_since() {
        let (_dir, store) = store();
        let (a, b) = (Path::new("/p/a"), Path::new("/p/b"));
        store.record(a, "a1", 10);
        store.record(b, "b1", 10);
        store.record(a, "a2", 30);
        let state = store.state_at(Path::new("/p"), 20);
        assert_eq!(state, vec![(a.to_path_buf(), "a1".to_string())]);
    }

    #[test]
    fn old_working_days_go_and_their_texts_with_them() {
        let (_dir, store) = store();
        let path = Path::new("/p/a");
        // Three days of work, far apart: the days in between don't count.
        store.record(path, "day one", DAY);
        store.record(path, "day two", 100 * DAY);
        store.record(path, "day three", 200 * DAY);
        let gone = store.file(path).revisions[0].blob.clone();
        store.prune(2, MAX_BYTES);
        let kept = store.file(path).revisions;
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[0].time, 100 * DAY);
        assert_eq!(store.text(&gone), None);
    }

    #[test]
    fn the_oldest_goes_when_the_history_is_too_big() {
        let (_dir, store) = store();
        store.record(Path::new("/p/a"), "aaaaaaaaaa", 1);
        store.record(Path::new("/p/b"), "bbbbbbbbbb", 2);
        store.prune(30, 15);
        assert!(store.file(Path::new("/p/a")).revisions.is_empty());
        assert_eq!(store.file(Path::new("/p/b")).revisions.len(), 1);
    }

    #[test]
    fn one_change_is_put_back_and_the_others_stay() {
        let lines: Vec<String> = (0..30).map(|i| format!("line {i}\n")).collect();
        let old = lines.concat();
        let new = old
            .replace("line 2\n", "two\n")
            .replace("line 25\n", "twenty-five\n");
        let found = chunks(&old, &new);
        assert_eq!(found.len(), 2);
        assert!(found[0].lines.contains(&(LineKind::New, "two".to_string())));
        let back = revert_chunk(&old, &new, &found[1]);
        assert!(back.contains("line 25\n"));
        assert!(back.contains("two\n"), "the first change stays");
    }

    #[test]
    fn the_whole_diff_has_every_line_and_its_blocks() {
        let old = "a\nb\nc\nd\ne\nf\ng\nh\n";
        let new = "a\nB\nc\nd\ne\nf\ng\nh\n";
        let diff = whole_diff("x.rs", old, new);
        let lines: usize = diff.hunks.iter().map(|h| h.lines.len()).sum();
        assert_eq!(lines, 9, "the eight lines, one of them twice");
        let blocks = diff.blocks();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].at, 1);
        let back = replace_lines(new, 1, blocks[0].added.len(), &blocks[0].removed);
        assert_eq!(back, old);
    }

    #[test]
    fn lines_are_put_back_with_the_line_ends_of_the_file() {
        assert_eq!(
            replace_lines("a\r\nb\r\n", 1, 1, &["c".to_string()]),
            "a\r\nc\r\n"
        );
        assert_eq!(replace_lines("a\n", 1, 0, &["b".to_string()]), "a\nb\n");
    }

    #[test]
    fn a_patch_names_the_file() {
        let text = patch("src/a.rs", "a\n", "b\n");
        assert!(text.contains("--- a/src/a.rs"));
        assert!(text.contains("-a\n+b\n"));
    }
}
