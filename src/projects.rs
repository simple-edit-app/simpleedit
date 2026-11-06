//! Projects: folders added to the sidebar, shown as a tree of their files.
//!
//! A folder is read when it is opened, not before; the ones that are open
//! are looked at again now and then, to follow what happens on the disk.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// What a folder holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
}

/// Names that are never shown: Git's own folder, and what the systems
/// leave about.
fn hidden(name: &str) -> bool {
    matches!(name, ".git" | ".DS_Store" | "Thumbs.db")
}

/// The content of a folder: folders first, then files, each by name.
pub fn read_children(dir: &Path) -> Vec<Entry> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut list: Vec<Entry> = entries
        .filter_map(Result::ok)
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if hidden(&name) {
                return None;
            }
            // A link to a folder is a folder.
            let is_dir = e.path().is_dir();
            Some(Entry {
                path: e.path(),
                name,
                is_dir,
            })
        })
        .collect();
    list.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.name.cmp(&b.name))
    });
    list
}

fn modified(dir: &Path) -> Option<SystemTime> {
    std::fs::metadata(dir).and_then(|m| m.modified()).ok()
}

/// How the files stand, for their color in the tree and on the tabs: what
/// Git says, and the files the lint found errors in.
#[derive(Debug, Default)]
pub struct Marks {
    files: HashMap<PathBuf, crate::git::FileState>,
    /// Folders with a change somewhere in them.
    changed: std::collections::HashSet<PathBuf>,
    /// Folders Git ignores as a whole.
    ignored: Vec<PathBuf>,
    /// Errors found by the lint, per file.
    pub lint: HashMap<PathBuf, usize>,
}

impl Marks {
    /// Takes in what Git said (`file_states`): paths, states, folder or not.
    pub fn set_git(&mut self, states: Vec<(PathBuf, crate::git::FileState, bool)>) {
        use crate::git::FileState;
        self.files.clear();
        self.changed.clear();
        self.ignored.clear();
        for (path, state, is_dir) in states {
            if state == FileState::Ignored {
                if is_dir {
                    self.ignored.push(path);
                } else {
                    self.files.insert(path, state);
                }
                continue;
            }
            for dir in path.ancestors().skip(1) {
                if !self.changed.insert(dir.to_path_buf()) {
                    break;
                }
            }
            self.files.insert(path, state);
        }
    }

    /// How a file or folder stands (a folder: changed when something in it
    /// is).
    /// The files Git sees as changed (modified, new, in conflict).
    pub fn changed_files(&self) -> Vec<PathBuf> {
        use crate::git::FileState;
        let mut files: Vec<PathBuf> = self
            .files
            .iter()
            .filter(|(_, s)| !matches!(s, FileState::Ignored))
            .map(|(p, _)| p.clone())
            .collect();
        files.sort();
        files
    }

    pub fn git(&self, path: &Path, is_dir: bool) -> Option<crate::git::FileState> {
        use crate::git::FileState;
        if self.ignored.iter().any(|d| path.starts_with(d)) {
            return Some(FileState::Ignored);
        }
        if is_dir {
            return self.changed.contains(path).then_some(FileState::Modified);
        }
        self.files.get(path).copied()
    }

    /// Whether the lint found errors in a file (in a folder: in a file of it).
    pub fn has_errors(&self, path: &Path, is_dir: bool) -> bool {
        if is_dir {
            self.lint.keys().any(|f| f.starts_with(path))
        } else {
            self.lint.contains_key(path)
        }
    }

    /// The errors the lint found in a file, now (none: forgotten).
    pub fn set_errors(&mut self, path: &Path, errors: usize) {
        if errors == 0 {
            self.lint.remove(path);
        } else {
            self.lint.insert(path.to_path_buf(), errors);
        }
    }
}

/// A line of the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub entry: Entry,
    pub depth: usize,
    pub open: bool,
    /// The folder that was added (the top of a project).
    pub root: bool,
}

/// Which of `entries` (of the folder `dir`) Git ignores. Nothing is, when
/// the folder is not in a repository or Git is not there.
fn ignored_of(dir: &Path, entries: &[Entry]) -> std::collections::HashSet<PathBuf> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    if entries.is_empty() {
        return Default::default();
    }
    let Ok(mut child) = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["check-ignore", "-z", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return Default::default();
    };
    // A folder is told by its trailing slash, as Git patterns say them.
    let input: Vec<u8> = entries
        .iter()
        .flat_map(|e| {
            let mut name = e.name.clone().into_bytes();
            if e.is_dir {
                name.push(b'/');
            }
            name.push(0);
            name
        })
        .collect();
    let writer = child.stdin.take().map(|mut stdin| {
        std::thread::spawn(move || {
            let _ = stdin.write_all(&input);
        })
    });
    let Ok(output) = child.wait_with_output() else {
        return Default::default();
    };
    if let Some(writer) = writer {
        let _ = writer.join();
    }
    output
        .stdout
        .split(|b| *b == 0)
        .filter(|name| !name.is_empty())
        .map(|name| {
            let name = String::from_utf8_lossy(name);
            dir.join(name.trim_end_matches('/'))
        })
        .collect()
}

/// The colors of the projects: bright enough on a dark and a light bar.
pub const COLORS: [(u8, u8, u8); 8] = [
    (0x4c, 0x8d, 0xff),
    (0xf0, 0x8a, 0x3c),
    (0x3f, 0xb5, 0x6b),
    (0xa3, 0x71, 0xf7),
    (0xe8, 0x5d, 0x9e),
    (0x2b, 0xb3, 0xb3),
    (0xd9, 0xa4, 0x00),
    (0xe5, 0x53, 0x4b),
];

/// The color of a project: the one it was given, its place otherwise.
pub fn color_of(
    colors: &std::collections::BTreeMap<String, usize>,
    roots: &[PathBuf],
    root: &Path,
) -> iced::Color {
    let stored = colors.get(root.to_string_lossy().as_ref()).copied();
    let place = roots.iter().position(|r| r == root);
    let (r, g, b) = COLORS[stored.or(place).unwrap_or(0) % COLORS.len()];
    iced::Color::from_rgb8(r, g, b)
}

#[derive(Debug, Default)]
pub struct Projects {
    /// What Git ignores is left out.
    pub hide_ignored: bool,
    /// Names left out (see `finder::excluded`).
    pub exclude: Vec<String>,
    pub roots: Vec<PathBuf>,
    pub expanded: BTreeSet<PathBuf>,
    /// The line picked (by a click or the arrows), and the folder a dragged
    /// line would go to.
    pub selected: Option<PathBuf>,
    pub drop_target: Option<PathBuf>,
    /// The content of the folders that were read, and when they changed.
    children: HashMap<PathBuf, (Option<SystemTime>, Vec<Entry>)>,
}

impl Projects {
    /// The projects of the settings, with the folders that were open.
    pub fn new(
        roots: Vec<PathBuf>,
        expanded: Vec<PathBuf>,
        hide_ignored: bool,
        exclude: Vec<String>,
    ) -> Self {
        let mut projects = Projects {
            hide_ignored,
            exclude,
            roots,
            expanded: expanded.into_iter().collect(),
            children: HashMap::new(),
            selected: None,
            drop_target: None,
        };
        projects.roots.retain(|r| r.is_dir());
        for root in projects.roots.clone() {
            projects.expanded.insert(root);
        }
        projects.expanded.retain(|d| d.is_dir());
        for dir in projects.expanded.clone() {
            projects.load(&dir);
        }
        projects
    }

    fn load(&mut self, dir: &Path) {
        let mut list = read_children(dir);
        list.retain(|e| !crate::finder::excluded(&e.name, e.is_dir, &self.exclude));
        if self.hide_ignored {
            let ignored = ignored_of(dir, &list);
            list.retain(|e| !ignored.contains(&e.path));
        }
        self.children
            .insert(dir.to_path_buf(), (modified(dir), list));
    }

    /// Shows or leaves out what Git ignores.
    pub fn set_hide_ignored(&mut self, hide: bool) {
        if self.hide_ignored != hide {
            self.hide_ignored = hide;
            self.refresh(true);
        }
    }

    /// Leaves out other names.
    pub fn set_exclude(&mut self, exclude: Vec<String>) {
        if self.exclude != exclude {
            self.exclude = exclude;
            self.refresh(true);
        }
    }

    /// Closes every folder, the tops of the projects showing what they hold.
    pub fn collapse_all(&mut self) {
        let roots = &self.roots;
        self.expanded.retain(|d| roots.contains(d));
    }

    /// Follows a folder or file that was renamed: the projects and the open
    /// folders inside it take the new path.
    pub fn renamed(&mut self, old: &Path, new: &Path) {
        let moved = |p: &PathBuf| -> PathBuf {
            p.strip_prefix(old)
                .map_or_else(|_| p.clone(), |rest| new.join(rest))
        };
        self.roots = self.roots.iter().map(moved).collect();
        self.expanded = self.expanded.iter().map(moved).collect();
        self.children.retain(|d, _| !d.starts_with(old));
        self.refresh(true);
        // The folder it is in has a new content.
        if let Some(parent) = new.parent() {
            if self.expanded.contains(parent) {
                self.load(parent);
            }
        }
    }

    /// Opens a folder (and loads it) if it was not.
    pub fn open(&mut self, dir: &Path) {
        if self.expanded.insert(dir.to_path_buf()) {
            self.load(dir);
        }
    }

    /// Adds a folder as a project. A folder that is inside one already
    /// there is not a project of its own: it is shown open in it.
    /// Returns whether a project was added.
    pub fn add(&mut self, dir: PathBuf) -> bool {
        if !dir.is_dir() || self.roots.contains(&dir) {
            return false;
        }
        if let Some(root) = self.roots.iter().find(|r| dir.starts_with(r)).cloned() {
            self.reveal(&dir, &root);
            return false;
        }
        // A project that holds the new one's folder gives way to it.
        self.roots.retain(|r| !r.starts_with(&dir));
        self.expanded.insert(dir.clone());
        self.load(&dir);
        self.roots.push(dir);
        true
    }

    /// Opens every folder from the project's top down to `dir`.
    fn reveal(&mut self, dir: &Path, root: &Path) {
        for ancestor in dir.ancestors() {
            if ancestor.starts_with(root) {
                self.expanded.insert(ancestor.to_path_buf());
                self.load(ancestor);
            }
        }
    }

    /// Opens the folders above `file`, if it is in a project; true if it is.
    pub fn show(&mut self, file: &Path) -> bool {
        let Some(root) = self.roots.iter().find(|r| file.starts_with(r)).cloned() else {
            return false;
        };
        if let Some(parent) = file.parent() {
            self.reveal(parent, &root);
        }
        true
    }

    pub fn remove(&mut self, root: &Path) {
        self.roots.retain(|r| r != root);
        self.expanded.retain(|d| !d.starts_with(root));
        self.children.retain(|d, _| !d.starts_with(root));
    }

    /// Opens or closes a folder.
    pub fn toggle(&mut self, dir: &Path) {
        if self.expanded.remove(dir) {
            return;
        }
        self.expanded.insert(dir.to_path_buf());
        self.load(dir);
    }

    /// Reads the open folders again if they changed on the disk. True if
    /// anything did.
    pub fn refresh(&mut self, force: bool) -> bool {
        let mut changed = false;
        for dir in self.expanded.clone() {
            let now = modified(&dir);
            let known = self.children.get(&dir).map(|(at, _)| *at);
            if force || known != Some(now) {
                let before = self.children.get(&dir).map(|(_, list)| list.clone());
                self.load(&dir);
                changed |= before.as_ref() != self.children.get(&dir).map(|(_, l)| l);
            }
        }
        // A folder that is gone is no longer open.
        let gone: Vec<PathBuf> = self
            .expanded
            .iter()
            .filter(|d| !d.is_dir())
            .cloned()
            .collect();
        for dir in gone {
            self.expanded.remove(&dir);
            self.children.remove(&dir);
            changed = true;
        }
        changed
    }

    /// The line the arrows go to from the one picked: `Up`, `Down`, `Left`
    /// (closes a folder, or goes to the folder above), `Right` (opens a
    /// folder, or goes into it). Returns the line picked now.
    pub fn step(&mut self, key: &str) -> Option<PathBuf> {
        let rows = self.rows();
        if rows.is_empty() {
            return None;
        }
        let at = self
            .selected
            .as_ref()
            .and_then(|s| rows.iter().position(|r| r.entry.path == *s));
        let Some(at) = at else {
            self.selected = Some(rows[0].entry.path.clone());
            return self.selected.clone();
        };
        let row = &rows[at];
        let next = match key {
            "ArrowDown" => rows.get(at + 1).map(|r| r.entry.path.clone()),
            "ArrowUp" => at.checked_sub(1).map(|i| rows[i].entry.path.clone()),
            "ArrowRight" if row.entry.is_dir => {
                if row.open {
                    rows.get(at + 1)
                        .filter(|r| r.depth > row.depth)
                        .map(|r| r.entry.path.clone())
                } else {
                    let dir = row.entry.path.clone();
                    self.toggle(&dir);
                    None
                }
            }
            "ArrowLeft" => {
                if row.entry.is_dir && row.open {
                    let dir = row.entry.path.clone();
                    self.toggle(&dir);
                    None
                } else {
                    row.entry
                        .path
                        .parent()
                        .filter(|p| rows.iter().any(|r| r.entry.path == *p))
                        .map(Path::to_path_buf)
                }
            }
            _ => None,
        };
        if let Some(next) = next {
            self.selected = Some(next);
        }
        self.selected.clone()
    }

    /// Where the line of `path` is in the tree.
    pub fn index_of(&self, path: &Path) -> Option<usize> {
        self.rows().iter().position(|r| r.entry.path == path)
    }

    /// The lines to draw: every project, and what is open in it.
    pub fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        for root in &self.roots {
            let name = root.file_name().map_or_else(
                || root.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            );
            let open = self.expanded.contains(root);
            rows.push(Row {
                entry: Entry {
                    path: root.clone(),
                    name,
                    is_dir: true,
                },
                depth: 0,
                open,
                root: true,
            });
            if open {
                self.push_children(root, 1, &mut rows);
            }
        }
        rows
    }

    fn push_children(&self, dir: &Path, depth: usize, rows: &mut Vec<Row>) {
        let Some((_, list)) = self.children.get(dir) else {
            return;
        };
        for entry in list {
            let open = entry.is_dir && self.expanded.contains(&entry.path);
            rows.push(Row {
                entry: entry.clone(),
                depth,
                open,
                root: false,
            });
            if open {
                self.push_children(&entry.path, depth + 1, rows);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        std::fs::create_dir_all(p.join("src/deep")).unwrap();
        std::fs::create_dir_all(p.join(".git")).unwrap();
        std::fs::create_dir_all(p.join("Assets")).unwrap();
        std::fs::write(p.join("b.txt"), "").unwrap();
        std::fs::write(p.join("A.md"), "").unwrap();
        std::fs::write(p.join("src/main.rs"), "").unwrap();
        std::fs::write(p.join("src/deep/x.rs"), "").unwrap();
        std::fs::write(p.join(".DS_Store"), "").unwrap();
        dir
    }

    fn names(rows: &[Row]) -> Vec<String> {
        rows.iter()
            .map(|r| format!("{}{}", "  ".repeat(r.depth), r.entry.name))
            .collect()
    }

    #[test]
    fn folders_come_first_then_files_each_by_name_and_git_is_hidden() {
        let dir = tree();
        let list = read_children(dir.path());
        let names: Vec<_> = list.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["Assets", "src", "A.md", "b.txt"]);
        assert!(list[0].is_dir && !list[2].is_dir);
    }

    #[test]
    fn a_project_shows_its_top_and_opens_folder_by_folder() {
        let dir = tree();
        let mut projects = Projects::default();
        assert!(projects.add(dir.path().to_path_buf()));
        let root = dir
            .path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert_eq!(
            names(&projects.rows()),
            vec![
                root.clone(),
                "  Assets".into(),
                "  src".into(),
                "  A.md".into(),
                "  b.txt".into()
            ]
        );
        // `src` opens, and `deep` inside it; then `src` closes with its content.
        projects.toggle(&dir.path().join("src"));
        projects.toggle(&dir.path().join("src/deep"));
        let shown = names(&projects.rows());
        assert!(
            shown.contains(&"    deep".to_string()) && shown.contains(&"      x.rs".to_string())
        );
        assert!(shown.contains(&"    main.rs".to_string()));
        projects.toggle(&dir.path().join("src"));
        assert!(!names(&projects.rows())
            .iter()
            .any(|n| n.contains("main.rs")));
        // Opened again, what was open inside is still open.
        projects.toggle(&dir.path().join("src"));
        assert!(names(&projects.rows()).iter().any(|n| n.contains("x.rs")));
        // The top can be closed too.
        projects.toggle(dir.path());
        assert_eq!(projects.rows().len(), 1);
    }

    #[test]
    fn adding_a_folder_twice_or_inside_a_project_makes_no_second_project() {
        let dir = tree();
        let mut projects = Projects::default();
        assert!(projects.add(dir.path().to_path_buf()));
        assert!(!projects.add(dir.path().to_path_buf()));
        // A folder inside it is shown open in it.
        assert!(!projects.add(dir.path().join("src/deep")));
        assert_eq!(projects.roots.len(), 1);
        assert!(names(&projects.rows()).iter().any(|n| n.contains("x.rs")));
        // Not a folder: nothing.
        assert!(!projects.add(dir.path().join("b.txt")));
        // A folder that holds the project takes its place.
        let parent = dir.path().parent().unwrap().to_path_buf();
        if parent.parent().is_some() && parent.is_dir() {
            assert!(projects.add(parent.clone()));
            assert_eq!(projects.roots, vec![parent]);
        }
    }

    #[test]
    fn what_changes_on_the_disk_is_noticed_for_the_folders_that_are_open() {
        let dir = tree();
        let mut projects = Projects::default();
        projects.add(dir.path().to_path_buf());
        assert!(!projects.refresh(false), "nothing changed");
        std::fs::write(dir.path().join("new.txt"), "").unwrap();
        // The folder's own time may not move at once on every system.
        assert!(projects.refresh(true));
        assert!(names(&projects.rows())
            .iter()
            .any(|n| n.contains("new.txt")));
        // A folder that went is closed.
        projects.toggle(&dir.path().join("Assets"));
        std::fs::remove_dir(dir.path().join("Assets")).unwrap();
        assert!(projects.refresh(true));
        assert!(!projects.expanded.contains(&dir.path().join("Assets")));
        assert!(!names(&projects.rows()).iter().any(|n| n.contains("Assets")));
    }

    #[test]
    fn a_file_is_shown_by_opening_the_folders_above_it() {
        let dir = tree();
        let mut projects = Projects::default();
        projects.add(dir.path().to_path_buf());
        assert!(projects.show(&dir.path().join("src/deep/x.rs")));
        assert!(names(&projects.rows()).iter().any(|n| n.ends_with("x.rs")));
        assert!(!projects.show(Path::new("/elsewhere/file.rs")));
        projects.remove(dir.path());
        assert!(projects.rows().is_empty() && projects.expanded.is_empty());
    }

    fn git(dir: &Path, args: &[&str]) {
        let ok = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .is_ok_and(|o| o.status.success());
        assert!(ok, "git {args:?}");
    }

    #[test]
    fn what_git_ignores_can_be_left_out() {
        let dir = tree();
        git(dir.path(), &["init", "-q"]);
        std::fs::write(dir.path().join(".gitignore"), "*.txt\nAssets/\n").unwrap();
        let mut projects = Projects::default();
        projects.add(dir.path().to_path_buf());
        let all = names(&projects.rows());
        assert!(
            all.iter().any(|n| n.contains("b.txt")) && all.iter().any(|n| n.contains("Assets"))
        );
        projects.set_hide_ignored(true);
        let shown = names(&projects.rows());
        assert!(
            !shown
                .iter()
                .any(|n| n.contains("b.txt") || n.contains("Assets")),
            "{shown:?}"
        );
        assert!(shown.iter().any(|n| n.contains("A.md")));
        // Back as they were.
        projects.set_hide_ignored(false);
        assert!(names(&projects.rows()).iter().any(|n| n.contains("b.txt")));
        // Outside a repository, nothing is left out.
        let plain = tree();
        let mut projects = Projects::new(vec![plain.path().to_path_buf()], vec![], true, vec![]);
        assert!(names(&projects.rows()).iter().any(|n| n.contains("b.txt")));
        projects.set_hide_ignored(false);
    }

    #[test]
    fn a_renamed_folder_takes_its_open_folders_with_it() {
        let dir = tree();
        let mut projects = Projects::default();
        projects.add(dir.path().to_path_buf());
        projects.toggle(&dir.path().join("src"));
        projects.toggle(&dir.path().join("src/deep"));
        std::fs::rename(dir.path().join("src"), dir.path().join("lib")).unwrap();
        projects.renamed(&dir.path().join("src"), &dir.path().join("lib"));
        assert!(projects.expanded.contains(&dir.path().join("lib/deep")));
        assert!(!projects.expanded.contains(&dir.path().join("src")));
        let shown = names(&projects.rows());
        assert!(
            shown.iter().any(|n| n.ends_with("x.rs")) && !shown.iter().any(|n| n.trim() == "src")
        );
        // A project renamed keeps being one.
        let moved = dir.path().join("moved");
        std::fs::rename(dir.path().join("lib"), &moved).unwrap();
        projects.renamed(&dir.path().join("lib"), &moved);
        assert_eq!(projects.roots, vec![dir.path().to_path_buf()]);
    }

    #[test]
    fn marks_color_files_and_the_folders_above_them() {
        use crate::git::FileState;
        let mut marks = Marks::default();
        marks.set_git(vec![
            (PathBuf::from("/p/src/app.rs"), FileState::Modified, false),
            (PathBuf::from("/p/new.txt"), FileState::Untracked, false),
            (PathBuf::from("/p/target"), FileState::Ignored, true),
            (PathBuf::from("/p/debug.log"), FileState::Ignored, false),
        ]);
        assert_eq!(
            marks.git(Path::new("/p/src/app.rs"), false),
            Some(FileState::Modified)
        );
        assert_eq!(
            marks.git(Path::new("/p/src"), true),
            Some(FileState::Modified)
        );
        assert_eq!(marks.git(Path::new("/p"), true), Some(FileState::Modified));
        assert_eq!(marks.git(Path::new("/p/docs"), true), None);
        assert_eq!(
            marks.git(Path::new("/p/new.txt"), false),
            Some(FileState::Untracked)
        );
        assert_eq!(
            marks.git(Path::new("/p/target/debug/x"), false),
            Some(FileState::Ignored)
        );
        assert_eq!(
            marks.git(Path::new("/p/target"), true),
            Some(FileState::Ignored)
        );
        assert_eq!(
            marks.git(Path::new("/p/debug.log"), false),
            Some(FileState::Ignored)
        );
        assert_eq!(marks.git(Path::new("/p/README.md"), false), None);
        // The lint's errors, on the file and the folders above.
        marks.set_errors(Path::new("/p/src/app.rs"), 2);
        assert!(marks.has_errors(Path::new("/p/src/app.rs"), false));
        assert!(marks.has_errors(Path::new("/p/src"), true));
        assert!(!marks.has_errors(Path::new("/p/new.txt"), false));
        marks.set_errors(Path::new("/p/src/app.rs"), 0);
        assert!(!marks.has_errors(Path::new("/p/src"), true));
        // A new status replaces the old one.
        marks.set_git(Vec::new());
        assert_eq!(marks.git(Path::new("/p/src"), true), None);
    }

    #[test]
    fn names_can_be_left_out_and_everything_folded_at_once() {
        let dir = tree();
        let mut projects = Projects::default();
        projects.add(dir.path().to_path_buf());
        projects.toggle(&dir.path().join("src"));
        projects.set_exclude(vec!["*.txt".into(), "Assets/".into()]);
        let shown = names(&projects.rows());
        assert!(
            !shown
                .iter()
                .any(|n| n.contains("b.txt") || n.contains("Assets")),
            "{shown:?}"
        );
        assert!(shown.iter().any(|n| n.contains("main.rs")));
        projects.collapse_all();
        let shown = names(&projects.rows());
        assert!(!shown.iter().any(|n| n.contains("main.rs")), "folded");
        assert!(
            shown.iter().any(|n| n.trim() == "src"),
            "the top still shows"
        );
    }

    #[test]
    fn the_arrows_move_in_the_tree_and_open_and_close_folders() {
        let dir = tree();
        let mut projects = Projects::default();
        projects.add(dir.path().to_path_buf());
        let root = dir.path().to_path_buf();
        // Nothing picked: the first line.
        assert_eq!(projects.step("ArrowDown"), Some(root.clone()));
        assert_eq!(projects.step("ArrowDown"), Some(root.join("Assets")));
        assert_eq!(projects.step("ArrowDown"), Some(root.join("src")));
        // Right opens the folder, then goes into it.
        assert_eq!(projects.step("ArrowRight"), Some(root.join("src")));
        assert!(projects.expanded.contains(&root.join("src")));
        assert_eq!(projects.step("ArrowRight"), Some(root.join("src/deep")));
        // Left goes up to the folder, then closes it.
        assert_eq!(projects.step("ArrowLeft"), Some(root.join("src")));
        assert_eq!(projects.step("ArrowLeft"), Some(root.join("src")));
        assert!(!projects.expanded.contains(&root.join("src")));
        // Up stops at the top.
        projects.selected = Some(root.clone());
        assert_eq!(projects.step("ArrowUp"), Some(root.clone()));
        assert_eq!(projects.index_of(&root.join("A.md")), Some(3));
    }

    #[test]
    fn projects_come_back_with_their_open_folders_and_lose_the_ones_gone() {
        let dir = tree();
        let open = vec![dir.path().join("src"), dir.path().join("gone")];
        let projects = Projects::new(
            vec![dir.path().to_path_buf(), PathBuf::from("/no/such/project")],
            open,
            false,
            vec![],
        );
        assert_eq!(projects.roots, vec![dir.path().to_path_buf()]);
        assert!(projects.expanded.contains(&dir.path().join("src")));
        assert!(!projects.expanded.contains(&dir.path().join("gone")));
        assert!(names(&projects.rows())
            .iter()
            .any(|n| n.contains("main.rs")));
    }
}
