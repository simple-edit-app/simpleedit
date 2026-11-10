//! What can be done to the files and folders of a project from its tree:
//! make a file or a folder, rename, delete.

use super::*;
use iced::widget::column;

/// What the name asked for is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NameFor {
    /// A file in this folder.
    File(PathBuf),
    /// A folder in this folder.
    Folder(PathBuf),
    /// A new name for this file or folder.
    Rename(PathBuf),
    /// The name of a copy of this file or folder, next to it.
    Duplicate(PathBuf),
}

/// The name offered for a copy: `name copy.ext`.
fn copy_name(path: &Path) -> String {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if std::fs::symlink_metadata(path).is_ok_and(|m| m.is_dir()) {
        return format!("{name} copy");
    }
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => format!("{stem} copy.{ext}"),
        _ => format!("{name} copy"),
    }
}

/// Copies a file, or a folder with everything in it. A link is copied as a
/// link, not followed.
fn copy_all(from: &Path, to: &Path) -> std::io::Result<()> {
    let kind = std::fs::symlink_metadata(from)?.file_type();
    if kind.is_symlink() {
        copy_link(from, to)
    } else if kind.is_dir() {
        std::fs::create_dir(to)?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            copy_all(&entry.path(), &to.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(from, to).map(drop)
    }
}

#[cfg(unix)]
fn copy_link(from: &Path, to: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(std::fs::read_link(from)?, to)
}

#[cfg(windows)]
fn copy_link(from: &Path, to: &Path) -> std::io::Result<()> {
    let target = std::fs::read_link(from)?;
    if from.is_dir() {
        std::os::windows::fs::symlink_dir(target, to)
    } else {
        std::os::windows::fs::symlink_file(target, to)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameDialog {
    pub purpose: NameFor,
    pub input: String,
}

pub(super) fn name_field_id() -> iced::widget::Id {
    iced::widget::Id::new("project-name")
}

/// A name for something in a folder: one name, not a path.
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\', '\0'])
        && name.trim() == name
}

impl SimpleEditApp {
    /// The tabs that are inside `path` (or are it).
    fn tabs_inside(&self, path: &Path) -> Vec<PathBuf> {
        self.sidebar
            .files()
            .iter()
            .filter(|p| !is_untitled(p) && p.starts_with(path))
            .cloned()
            .collect()
    }

    pub(super) fn ask_name(&mut self, purpose: NameFor) -> Task<Message> {
        let input = match &purpose {
            NameFor::Rename(path) => path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            NameFor::Duplicate(path) => copy_name(path),
            _ => String::new(),
        };
        self.overlay = ActiveOverlay::ProjectName(NameDialog { purpose, input });
        iced::widget::operation::focus(name_field_id())
    }

    pub(super) fn name_typed(&mut self, text: String) {
        if let ActiveOverlay::ProjectName(dialog) = &mut self.overlay {
            dialog.input = text;
        }
    }

    /// The name is given: the file or folder is made, or renamed.
    pub(super) fn name_submitted(&mut self) -> Task<Message> {
        let ActiveOverlay::ProjectName(dialog) = self.overlay.clone() else {
            return Task::none();
        };
        let name = dialog.input.clone();
        if !valid_name(&name) {
            self.set_status(t!("projects.invalid_name"), true);
            return Task::none();
        }
        let target = match &dialog.purpose {
            NameFor::File(dir) | NameFor::Folder(dir) => dir.join(&name),
            NameFor::Rename(path) | NameFor::Duplicate(path) => path.with_file_name(&name),
        };
        // Case-only rename on a case-insensitive filesystem: `target` "exists" but is the file itself.
        let case_only_rename = matches!(&dialog.purpose, NameFor::Rename(path)
        if path.file_name().is_some_and(|n| {
            let old = n.to_string_lossy();
            old != name && old.to_lowercase() == name.to_lowercase()
        }));
        if target.exists() && !case_only_rename {
            self.set_status(t!("projects.exists", name = name.as_str()), true);
            return Task::none();
        }
        self.overlay = ActiveOverlay::None;
        match dialog.purpose {
            NameFor::File(dir) => {
                let made = std::fs::create_dir_all(&dir).and_then(|()| {
                    std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&target)
                        .map(drop)
                });
                match made {
                    Ok(()) => {
                        self.projects.open(&dir);
                        self.projects.refresh(true);
                        self.save_projects();
                        self.activate(target)
                    }
                    Err(e) => {
                        self.set_status(format!("{}: {e}", t!("status.error")), true);
                        Task::none()
                    }
                }
            }
            NameFor::Folder(dir) => {
                match std::fs::create_dir_all(&target) {
                    Ok(()) => {
                        self.projects.open(&dir);
                        self.projects.open(&target);
                        self.projects.refresh(true);
                        self.save_projects();
                    }
                    Err(e) => self.set_status(format!("{}: {e}", t!("status.error")), true),
                }
                Task::none()
            }
            NameFor::Rename(old) => self.rename_entry(old, target),
            NameFor::Duplicate(from) => match copy_all(&from, &target) {
                Ok(()) => {
                    self.projects.refresh(true);
                    if target.is_file() {
                        return self.activate(target);
                    }
                    Task::none()
                }
                Err(e) => {
                    self.set_status(format!("{}: {e}", t!("status.error")), true);
                    Task::none()
                }
            },
        }
    }

    /// Renames a file or folder. The tabs inside it must have nothing
    /// unsaved; they follow to the new name, bookmarks and local history too.
    /// Moves a file or folder to another place (see `rename_entry`).
    pub(super) fn move_entry(&mut self, old: PathBuf, new: PathBuf) -> Task<Message> {
        self.rename_entry(old, new)
    }

    fn rename_entry(&mut self, old: PathBuf, new: PathBuf) -> Task<Message> {
        let inside = self.tabs_inside(&old);
        let dirty = self.dirty_files();
        if inside.iter().any(|p| dirty.contains(p)) {
            self.set_status(t!("projects.unsaved"), true);
            return Task::none();
        }
        let back = self
            .current_file
            .clone()
            .filter(|c| c.starts_with(&old))
            .and_then(|c| c.strip_prefix(&old).ok().map(|rest| new.join(rest)));
        // Renamed first: the tabs stay as they are when it fails.
        if let Err(e) = std::fs::rename(&old, &new) {
            self.set_status(format!("{}: {e}", t!("status.error")), true);
            return Task::none();
        }
        let moved = |p: &Path| -> PathBuf {
            match p.strip_prefix(&old) {
                Ok(rest) if rest.as_os_str().is_empty() => new.clone(),
                Ok(rest) => new.join(rest),
                Err(_) => p.to_path_buf(),
            }
        };
        // The tabs that are not in the editor follow to the new path.
        for p in inside
            .iter()
            .filter(|p| self.current_file.as_ref() != Some(*p))
        {
            let to = moved(p);
            self.sidebar.rename_file(p, to.clone());
            if let Some(buffer) = self.file_cache.remove(p) {
                self.file_cache.insert(to.clone(), buffer);
            }
            if let Some(undo) = self.undo_cache.remove(p) {
                self.undo_cache.insert(to.clone(), undo);
            }
            if let Some(large) = self.large.remove(p) {
                self.large.insert(to.clone(), large);
            }
            if self.preview_tab.as_ref() == Some(p) {
                self.preview_tab = Some(to);
            }
        }
        let mut tasks: Vec<Task<Message>> = Vec::new();
        if let Some(current) = self.current_file.clone().filter(|c| inside.contains(c)) {
            tasks.push(self.close_path(current));
        }
        for closed in &mut self.closed_tabs {
            *closed = moved(closed);
        }
        self.bookmarks.rename(&old, &new);
        let days = self.config.local_history_days;
        if days > 0 {
            if let Some(store) = crate::local_history::Store::global() {
                let (from, to) = (old.clone(), new.clone());
                tasks.push(Task::future(async move { store.rename_tree(&from, &to) }).discard());
            }
        }
        self.projects.renamed(&old, &new);
        self.save_projects();
        if let Some(path) = back {
            tasks.push(self.activate(path));
        }
        tasks.push(Task::done(Message::Git(crate::git_panel::GitMsg::Refresh)));
        Task::batch(tasks)
    }

    /// A folder to delete for good, after a question.
    pub(super) fn ask_delete_folder(&mut self, dir: PathBuf) {
        let dirty = self.dirty_files();
        if self.tabs_inside(&dir).iter().any(|p| dirty.contains(p)) {
            self.set_status(t!("projects.unsaved"), true);
            return;
        }
        self.overlay = ActiveOverlay::ConfirmFolderDelete(dir);
    }

    pub(super) fn delete_folder(&mut self, dir: PathBuf) -> Task<Message> {
        self.overlay = ActiveOverlay::None;
        let mut tasks: Vec<Task<Message>> = self
            .tabs_inside(&dir)
            .into_iter()
            .map(|p| {
                self.bookmarks.clear(&p);
                self.close_path(p)
            })
            .collect();
        let days = self.config.local_history_days;
        tasks.push(Task::perform(
            async move {
                if days > 0 {
                    if let Some(store) = crate::local_history::Store::global() {
                        store.record_tree(&dir, crate::local_history::now());
                    }
                }
                tokio::fs::remove_dir_all(&dir)
                    .await
                    .map(|()| dir.clone())
                    .map_err(|e| format!("{}: {e}", dir.display()))
            },
            Message::FolderDeleted,
        ));
        Task::batch(tasks)
    }

    pub(super) fn folder_deleted(&mut self, result: Result<PathBuf, String>) -> Task<Message> {
        match result {
            Ok(dir) => {
                // A project that was deleted is one no more.
                if self.projects.roots.contains(&dir) {
                    self.projects.remove(&dir);
                }
                self.projects.refresh(true);
                self.save_projects();
                self.set_status(t!("status.file_deleted"), false);
            }
            Err(e) => self.set_status(format!("{}: {e}", t!("status.error")), true),
        }
        Task::done(Message::Git(crate::git_panel::GitMsg::Refresh))
    }

    /// The card that asks for a name.
    pub(super) fn view_name_overlay<'a>(
        &'a self,
        dark: bool,
        dialog: &'a NameDialog,
    ) -> Element<'a, Message> {
        let p = se_theme::palette(dark);
        let folder_name = |dir: &Path| {
            dir.file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
        };
        let (title, confirm) = match &dialog.purpose {
            NameFor::File(dir) => (
                t!("projects.new_file_title", dir = folder_name(dir).as_str()).to_string(),
                t!("projects.create").to_string(),
            ),
            NameFor::Folder(dir) => (
                t!("projects.new_folder_title", dir = folder_name(dir).as_str()).to_string(),
                t!("projects.create").to_string(),
            ),
            NameFor::Rename(path) => (
                t!("projects.rename_title", name = folder_name(path).as_str()).to_string(),
                t!("projects.rename").to_string(),
            ),
            NameFor::Duplicate(path) => (
                {
                    let name = folder_name(path);
                    t!("projects.duplicate_title", name = name.as_str()).to_string()
                },
                t!("projects.duplicate").to_string(),
            ),
        };
        container(
            column![
                text(title).size(14).color(p.text),
                iced::widget::text_input(&t!("projects.name_placeholder"), &dialog.input)
                    .id(name_field_id())
                    .on_input(Message::ProjectNameChanged)
                    .on_submit(Message::ProjectNameSubmit)
                    .padding([8, 10])
                    .size(14)
                    .style(se_theme::Field::new(dark).style()),
                row![
                    button(text(t!("goto_line.cancel").to_string()).size(13))
                        .padding([7, 16])
                        .on_press(Message::CloseOverlay)
                        .style(
                            (se_theme::GhostButton {
                                dark,
                                active: false
                            })
                            .style()
                        ),
                    Space::new().width(Length::Fill),
                    button(text(confirm).size(13))
                        .padding([7, 16])
                        .on_press(Message::ProjectNameSubmit)
                        .style((se_theme::GhostButton { dark, active: true }).style()),
                ],
            ]
            .spacing(12),
        )
        .padding(20)
        .width(360)
        .style(se_theme::card(dark))
        .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn duplicating_a_folder_copies_links_as_links() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir(&src).unwrap();
        std::fs::write(src.join("a.txt"), "a").unwrap();
        std::os::unix::fs::symlink("..", src.join("up")).unwrap();
        let to = dir.path().join("copy");
        copy_all(&src, &to).unwrap();
        assert!(std::fs::symlink_metadata(to.join("up"))
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(std::fs::read_to_string(to.join("a.txt")).unwrap(), "a");
    }

    fn app_with_project() -> (SimpleEditApp, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/main.rs"), "fn main() {}\n").unwrap();
        let mut app = SimpleEditApp::blank(Config::default());
        let _ = app.restore_session(crate::session::Session::default());
        let _ = app.update(Message::ProjectPicked(Some(dir.path().to_path_buf())));
        (app, dir)
    }

    fn type_name(app: &mut SimpleEditApp, name: &str) {
        let _ = app.update(Message::ProjectNameChanged(name.into()));
        let _ = app.update(Message::ProjectNameSubmit);
    }

    #[test]
    fn a_name_is_one_name_and_not_a_path() {
        assert!(valid_name("main.rs") && valid_name(".env") && valid_name("a b"));
        for bad in ["", ".", "..", "a/b", "a\\b", " a", "a "] {
            assert!(!valid_name(bad), "{bad:?}");
        }
    }

    #[test]
    fn a_file_and_a_folder_are_made_from_the_tree() {
        let (mut app, dir) = app_with_project();
        let src = dir.path().join("src");
        let _ = app.update(Message::NewProjectEntry(src.clone(), false));
        assert!(matches!(app.overlay, ActiveOverlay::ProjectName(_)));
        // A name that is not one, or that is taken, is refused and asked again.
        type_name(&mut app, "a/b.rs");
        assert!(app.status_is_error && matches!(app.overlay, ActiveOverlay::ProjectName(_)));
        type_name(&mut app, "main.rs");
        assert!(matches!(app.overlay, ActiveOverlay::ProjectName(_)));
        type_name(&mut app, "lib.rs");
        assert!(src.join("lib.rs").is_file());
        assert!(matches!(app.overlay, ActiveOverlay::None));
        assert_eq!(
            app.pending_open.as_ref(),
            Some(&src.join("lib.rs")),
            "it is opened"
        );
        assert!(app.projects.rows().iter().any(|r| r.entry.name == "lib.rs"));
        // A folder, shown open.
        let _ = app.update(Message::NewProjectEntry(dir.path().to_path_buf(), true));
        type_name(&mut app, "docs");
        assert!(dir.path().join("docs").is_dir());
        assert!(app.projects.expanded.contains(&dir.path().join("docs")));
    }

    #[test]
    fn a_file_or_a_folder_is_duplicated_next_to_it() {
        let (mut app, dir) = app_with_project();
        let file = dir.path().join("src/main.rs");
        let _ = app.update(Message::DuplicateEntry(file.clone()));
        let ActiveOverlay::ProjectName(dialog) = &app.overlay else {
            panic!("a name is asked");
        };
        assert_eq!(dialog.input, "main copy.rs");
        let _ = app.update(Message::ProjectNameSubmit);
        let copy = dir.path().join("src/main copy.rs");
        assert_eq!(std::fs::read_to_string(&copy).unwrap(), "fn main() {}\n");
        assert_eq!(app.pending_open.as_ref(), Some(&copy), "the copy opens");
        // A folder, with what is in it.
        let _ = app.update(Message::DuplicateEntry(dir.path().join("src")));
        type_name(&mut app, "lib");
        assert!(dir.path().join("lib/main.rs").is_file());
        assert!(
            dir.path().join("src/main.rs").is_file(),
            "the original stays"
        );
        assert_eq!(copy_name(Path::new("/x/Makefile")), "Makefile copy");
        assert_eq!(copy_name(Path::new("/x/.env")), ".env copy");
    }

    #[test]
    fn a_file_is_renamed_and_its_tab_follows() {
        let (mut app, dir) = app_with_project();
        let old = dir.path().join("src/main.rs");
        let _ = app.update(Message::ToggleProjectFolder(dir.path().join("src")));
        let _ = app.update(Message::TabRead(Ok(Loaded {
            path: old.clone(),
            text: "fn main() {}\n".into(),
            format: TextFormat::default(),
            stamp: None,
        })));
        assert_eq!(app.current_file.as_ref(), Some(&old));
        let _ = app.update(Message::RenameProjectEntry(old.clone()));
        let ActiveOverlay::ProjectName(dialog) = &app.overlay else {
            panic!("a name is asked");
        };
        assert_eq!(dialog.input, "main.rs", "it starts from the current name");
        type_name(&mut app, "app.rs");
        let new = dir.path().join("src/app.rs");
        assert!(new.is_file() && !old.exists());
        assert!(!app.sidebar.files().contains(&old));
        assert_eq!(app.pending_open.as_ref(), Some(&new), "opened again");
        assert!(app.projects.rows().iter().any(|r| r.entry.name == "app.rs"));
    }

    #[test]
    fn what_is_not_saved_is_never_renamed_or_deleted_from_under_its_tab() {
        let (mut app, dir) = app_with_project();
        let file = dir.path().join("src/main.rs");
        let _ = app.update(Message::TabRead(Ok(Loaded {
            path: file.clone(),
            text: "fn main() {}\\n".into(),
            format: TextFormat::default(),
            stamp: None,
        })));
        let _ = app.update(Message::EditorAction(text_editor::Action::Edit(
            text_editor::Edit::Insert('x'),
        )));
        let _ = app.update(Message::RenameProjectEntry(dir.path().join("src")));
        type_name(&mut app, "lib");
        assert!(dir.path().join("src").is_dir() && !dir.path().join("lib").exists());
        assert!(app.status_is_error);
        let _ = app.update(Message::AskDeleteFolder(dir.path().join("src")));
        assert!(!matches!(
            app.overlay,
            ActiveOverlay::ConfirmFolderDelete(_)
        ));
    }

    #[test]
    fn a_folder_is_deleted_after_a_question_and_its_tabs_close() {
        let (mut app, dir) = app_with_project();
        let src = dir.path().join("src");
        let _ = app.update(Message::TabRead(Ok(Loaded {
            path: src.join("main.rs"),
            text: "fn main() {}\n".into(),
            format: TextFormat::default(),
            stamp: None,
        })));
        let _ = app.update(Message::AskDeleteFolder(src.clone()));
        assert!(matches!(app.overlay, ActiveOverlay::ConfirmFolderDelete(_)));
        assert!(src.is_dir(), "nothing before the answer");
        let _ = app.update(Message::DeleteFolder(src.clone()));
        assert!(app.sidebar.files().iter().all(|p| !p.starts_with(&src)));
        // The deletion itself runs in the background: its answer is what the
        // tree takes in.
        std::fs::remove_dir_all(&src).unwrap();
        let _ = app.update(Message::FolderDeleted(Ok(src.clone())));
        assert!(!app.projects.rows().iter().any(|r| r.entry.name == "src"));
    }

    #[test]
    fn a_file_dragged_onto_a_folder_moves_there_after_a_question() {
        let (mut app, dir) = app_with_project();
        std::fs::create_dir_all(dir.path().join("docs")).unwrap();
        let _ = app.update(Message::RefreshProjects);
        let file = dir.path().join("src/main.rs");
        let docs = dir.path().join("docs");
        let _ = app.update(Message::TreePress(file.clone()));
        assert_eq!(app.projects.selected.as_ref(), Some(&file));
        // Over its own folder: nowhere to go.
        let _ = app.update(Message::TreeHover(dir.path().join("src")));
        assert_eq!(app.projects.drop_target, None);
        // Over another folder, or a file of it: there.
        let _ = app.update(Message::TreeHover(docs.clone()));
        assert_eq!(app.projects.drop_target.as_ref(), Some(&docs));
        let _ = app.update(Message::TreeRelease);
        assert!(
            matches!(&app.overlay, ActiveOverlay::ConfirmMove(s, d) if *s == file && *d == docs)
        );
        assert!(file.is_file(), "nothing before the answer");
        let _ = app.update(Message::MoveEntry(file.clone(), docs.clone()));
        assert!(docs.join("main.rs").is_file() && !file.exists());
        // A folder can't go into itself.
        let src = dir.path().join("src");
        std::fs::create_dir_all(src.join("inner")).unwrap();
        let _ = app.update(Message::TreePress(src.clone()));
        let _ = app.update(Message::TreeHover(src.join("inner")));
        assert_eq!(app.projects.drop_target, None);
        let _ = app.update(Message::TreeRelease);
        assert_eq!(app.overlay, ActiveOverlay::None);
        // A press and a release on the same line move nothing.
        let _ = app.update(Message::TreePress(docs.clone()));
        let _ = app.update(Message::TreeRelease);
        assert_eq!(app.overlay, ActiveOverlay::None);
    }

    #[test]
    fn the_arrows_and_enter_work_in_the_tree_once_it_was_clicked() {
        let (mut app, dir) = app_with_project();
        let src = dir.path().join("src");
        let key = |app: &mut SimpleEditApp, key: &str| {
            let _ = app.update(Message::KeyPressed {
                key: key.into(),
                ctrl: false,
                shift: false,
                alt: false,
                captured: false,
            });
        };
        app.panels.projects = true;
        app.panels.files = false;
        // Not clicked: the arrows are the editor's.
        key(&mut app, "ArrowDown");
        assert_eq!(app.projects.selected, None);
        let _ = app.update(Message::ToggleProjectFolder(src.clone()));
        let _ = app.update(Message::ToggleProjectFolder(src.clone()));
        assert!(app.tree_focus);
        key(&mut app, "ArrowRight");
        assert!(app.projects.expanded.contains(&src));
        key(&mut app, "ArrowRight");
        assert_eq!(app.projects.selected, Some(src.join("main.rs")));
        key(&mut app, "Return");
        assert_eq!(app.pending_open, Some(src.join("main.rs")));
        // Back in the editor, the arrows are its own again.
        let _ = app.update(Message::EditorAction(text_editor::Action::Move(
            text_editor::Motion::Down,
        )));
        assert!(!app.tree_focus);
    }

    #[test]
    fn names_left_out_and_folding_everything() {
        let (mut app, dir) = app_with_project();
        std::fs::write(dir.path().join("debug.log"), "").unwrap();
        let _ = app.update(Message::RefreshProjects);
        assert!(app
            .projects
            .rows()
            .iter()
            .any(|r| r.entry.name == "debug.log"));
        let _ = app.update(Message::SetProjectsExclude(" *.log ,, dist/ ".into()));
        assert_eq!(
            app.config.projects_exclude,
            vec!["*.log".to_string(), "dist/".to_string()]
        );
        assert_eq!(app.preferences.exclude_draft, "*.log, dist/");
        assert!(!app
            .projects
            .rows()
            .iter()
            .any(|r| r.entry.name == "debug.log"));
        let _ = app.update(Message::ToggleProjectFolder(dir.path().join("src")));
        let _ = app.update(Message::CollapseProjects);
        assert!(!app
            .projects
            .rows()
            .iter()
            .any(|r| r.entry.name == "main.rs"));
        assert!(app.config.projects_open.iter().all(|d| d == dir.path()));
    }
}
