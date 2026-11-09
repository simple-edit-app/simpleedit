//! Find and replace in files (Ctrl+Shift+F, Ctrl+Shift+R), as IntelliJ IDEA
//! has them: one window that looks in the projects, a folder (and its
//! subfolders or not), the open, recent, changed or current files, with
//! the case, whole words, a regular expression, a file mask and a context
//! (in or except comments and strings); it lists what it finds with a
//! preview, replaces one or all, and sends the results to the Results panel.

use super::*;
use crate::find_replace::{Context, Options, Replace};
use crate::finder::TextHit;
use iced::widget::{column, rich_text, span, text_input};

/// Height of a line of the list, to scroll it to the one picked.
const ROW: f32 = 30.0;
/// Searches and replacements remembered (Alt+↓).
const HISTORY: usize = 20;

/// What the right-click menu of the preview does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewMenu {
    Copy,
    Cut,
    Paste,
    Delete,
    SelectAll,
}

/// Where the search looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Scope {
    #[default]
    Project,
    /// The folder in `FinderState::dir`.
    Directory,
    Open,
    Recent,
    Changed,
    Current,
}

impl Scope {
    const ALL: [Scope; 6] = [
        Scope::Project,
        Scope::Directory,
        Scope::Open,
        Scope::Recent,
        Scope::Changed,
        Scope::Current,
    ];
}

/// A match found, what it would be replaced by, and whether it is left out
/// of the replacement.
#[derive(Debug, Clone, PartialEq)]
pub struct Occurrence {
    pub hit: TextHit,
    pub replacement: Option<String>,
    pub excluded: bool,
}

/// A search as it was run: to replace what it found, or run it again.
#[derive(Debug, Clone, PartialEq)]
pub struct Search {
    pub options: Options,
    pub replace: Option<Replace>,
    pub scope: Scope,
    pub dir: Option<PathBuf>,
    pub recursive: bool,
}

/// A list that opens under the fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Menu {
    Context,
    History,
    /// The folders to look in (the projects, the last ones).
    Dirs,
}

/// A file a replacement wrote, to put it back.
#[derive(Debug, Clone)]
pub struct Written {
    pub path: PathBuf,
    pub before: String,
    pub after: String,
}

pub struct FinderState {
    /// The files of the projects (or of the folder looked in), with how
    /// they are shown.
    pub files: Vec<(PathBuf, String)>,
    pub loading: bool,
    pub selected: usize,
    pub text: String,
    /// The search written on several lines (it shows instead of the field).
    pub multiline: bool,
    pub lines: text_editor::Content,
    pub replace: String,
    /// Replace in files, not only find.
    pub replace_mode: bool,
    pub case_sensitive: bool,
    pub words: bool,
    pub regex: bool,
    pub preserve_case: bool,
    pub context: Context,
    pub mask_on: bool,
    pub mask: String,
    pub scope: Scope,
    pub dir: Option<PathBuf>,
    pub recursive: bool,
    pub hits: Vec<Occurrence>,
    pub searching: bool,
    pub error: Option<String>,
    generation: u64,
    /// The search the list is from.
    pub searched: Option<Search>,
    pub menu: Option<Menu>,
    /// Pinned: it stays open when a match is opened or the editor clicked.
    pub pinned: bool,
    /// "Replace all" asked: the confirmation shows.
    pub confirm_all: bool,
    /// What the last replacement wrote (to undo it).
    pub undo: Vec<Written>,
    /// The text of the file of the selected match, for the preview.
    preview: Option<(PathBuf, crate::code_editor::Content)>,
    /// The preview was typed in: the list keeps its place when it is looked
    /// at again, and the preview its cursor.
    preview_edited: bool,
    /// The last row clicked, to tell a double click.
    last_click: Option<(usize, std::time::Instant)>,
    /// The folder as typed in its field (it may not be one yet).
    dir_text: String,
    /// The folders looked in, the last first.
    dir_history: Vec<PathBuf>,
}

impl Default for FinderState {
    fn default() -> Self {
        Self {
            files: Vec::new(),
            loading: false,
            selected: 0,
            text: String::new(),
            multiline: false,
            lines: text_editor::Content::new(),
            replace: String::new(),
            replace_mode: false,
            case_sensitive: false,
            words: false,
            regex: false,
            preserve_case: false,
            context: Context::Anywhere,
            mask_on: false,
            mask: String::new(),
            scope: Scope::Project,
            dir: None,
            recursive: true,
            hits: Vec::new(),
            searching: false,
            error: None,
            generation: 0,
            searched: None,
            menu: None,
            pinned: false,
            confirm_all: false,
            undo: Vec::new(),
            preview: None,
            preview_edited: false,
            last_click: None,
            dir_text: String::new(),
            dir_history: Vec::new(),
        }
    }
}

pub(super) fn find_field_id() -> iced::widget::Id {
    iced::widget::Id::new("find-in-files")
}

fn replace_field_id() -> iced::widget::Id {
    iced::widget::Id::new("replace-in-files")
}

fn list_id() -> iced::widget::Id {
    iced::widget::Id::new("finder-list")
}

/// Where a file's text is: in the editor, in a tab behind it, on disk.
pub(super) enum Holder {
    Editor,
    Tab,
    Disk,
}

fn context_label(context: Context) -> String {
    match context {
        Context::Anywhere => t!("findrep.anywhere"),
        Context::InComments => t!("findrep.in_comments"),
        Context::InStrings => t!("findrep.in_strings"),
        Context::ExceptComments => t!("findrep.except_comments"),
        Context::ExceptStrings => t!("findrep.except_strings"),
        Context::ExceptBoth => t!("findrep.except_both"),
    }
    .to_string()
}

fn scope_label(scope: Scope) -> String {
    match scope {
        Scope::Project => t!("findrep.scope_project"),
        Scope::Directory => t!("findrep.scope_directory"),
        Scope::Open => t!("findrep.scope_open"),
        Scope::Recent => t!("findrep.scope_recent"),
        Scope::Changed => t!("findrep.scope_changed"),
        Scope::Current => t!("findrep.scope_current"),
    }
    .to_string()
}

/// How many matches, in how many files.
pub(super) fn counts(hits: &[Occurrence]) -> (usize, usize) {
    let files: HashSet<&PathBuf> = hits.iter().map(|o| &o.hit.path).collect();
    (hits.len(), files.len())
}

/// "1 match", "3 matches", as the language writes them.
pub(super) fn matches_text(n: usize) -> String {
    let count = n.to_string();
    match n {
        1 => t!("findrep.one_match").to_string(),
        _ => t!("findrep.matches", n = count.as_str()).to_string(),
    }
}

/// The comments that call for work in the files of `roots`.
fn todo_hits(
    roots: &[PathBuf],
    exclude: &[String],
    open: &HashMap<PathBuf, String>,
) -> Result<Vec<Occurrence>, String> {
    let files: Vec<PathBuf> = roots
        .iter()
        .flat_map(|root| crate::finder::list_files(root, exclude, false))
        .collect();
    let options = Options {
        query: r"\b(?:TODO|FIXME|HACK|XXX|BUG)\b.*".into(),
        case_sensitive: true,
        words: false,
        regex: true,
        context: Context::InComments,
        mask: None,
    };
    crate::finder::find_occurrences(&files, open, &options, None).map(|found| {
        found
            .into_iter()
            .map(|(hit, replacement)| Occurrence {
                hit,
                replacement,
                excluded: false,
            })
            .collect()
    })
}

/// "1 file", "3 files".
pub(super) fn files_text(n: usize) -> String {
    let count = n.to_string();
    match n {
        1 => t!("findrep.one_file").to_string(),
        _ => t!("findrep.files", n = count.as_str()).to_string(),
    }
}

fn remember(list: &mut Vec<String>, value: &str) {
    if value.is_empty() {
        return;
    }
    list.retain(|v| v != value);
    list.insert(0, value.to_string());
    list.truncate(HISTORY);
}

impl SimpleEditApp {
    /// Where to look: the projects, or the project of the file in the editor.
    pub(super) fn finder_roots(&self) -> Vec<PathBuf> {
        if !self.projects.roots.is_empty() {
            return self.projects.roots.clone();
        }
        self.current_file
            .as_deref()
            .filter(|p| !is_untitled(p) && !is_virtual(p))
            .and_then(crate::definition::project_root)
            .into_iter()
            .collect()
    }

    /// Lists the files again, in the background: those of `scope` (a folder
    /// of a project), or of every project.
    fn load_finder_files(&mut self, scope: Option<PathBuf>) -> Task<Message> {
        let all = self.finder_roots();
        let several = all.len() > 1;
        // Each folder looked in, with the top of its project (the paths are
        // shown from there).
        let roots: Vec<(PathBuf, PathBuf)> = match scope {
            Some(dir) => {
                let top = all
                    .iter()
                    .find(|r| dir.starts_with(r))
                    .cloned()
                    .unwrap_or_else(|| dir.clone());
                vec![(dir, top)]
            }
            None => all.into_iter().map(|r| (r.clone(), r)).collect(),
        };
        let exclude = self.config.projects_exclude.clone();
        let ignored = self.config.find_ignored;
        self.finder.loading = true;
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    roots
                        .iter()
                        .flat_map(|(dir, root)| {
                            let top = root
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_default();
                            crate::finder::list_files(dir, &exclude, ignored)
                                .into_iter()
                                .map(move |file| {
                                    let rel = file
                                        .strip_prefix(root)
                                        .map(|r| r.to_string_lossy().replace('\\', "/"))
                                        .unwrap_or_default();
                                    let shown = if several { format!("{top}/{rel}") } else { rel };
                                    (file, shown)
                                })
                                .collect::<Vec<_>>()
                        })
                        .collect::<Vec<_>>()
                })
                .await
                .unwrap_or_default()
            },
            Message::FinderFiles,
        )
    }

    /// Opens find in files.
    pub(super) fn open_find_in_files(&mut self) -> Task<Message> {
        self.open_finder(false)
    }

    /// Opens find in files (or replace in files, `replace`): what is
    /// selected in the editor is looked for; a folder picked in the tree is
    /// where.
    pub(super) fn open_finder(&mut self, replace: bool) -> Task<Message> {
        self.finder.replace_mode = replace;
        self.finder.selected = 0;
        self.finder.error = None;
        self.finder.menu = None;
        self.finder.confirm_all = false;
        if let Some(selected) = self.editor.content.selection().filter(|s| !s.is_empty()) {
            self.finder.multiline = selected.contains('\n');
            self.finder_set_text(selected);
        }
        // A folder picked in the tree (the project of a file picked there).
        let picked = self.projects.selected.as_ref().and_then(|picked| {
            if picked.is_dir() {
                Some(picked.clone())
            } else {
                self.projects
                    .roots
                    .iter()
                    .find(|r| picked.starts_with(r))
                    .cloned()
            }
        });
        let whole =
            self.projects.roots.len() == 1 && picked.as_ref() == self.projects.roots.first();
        match picked {
            Some(dir) if !whole => {
                self.finder.scope = Scope::Directory;
                self.set_finder_dir(dir);
            }
            _ if self.finder.scope == Scope::Directory => self.finder.scope = Scope::Project,
            _ => {}
        }
        self.overlay = ActiveOverlay::FindInFiles;
        let focus = if replace && !self.finder.text.is_empty() {
            replace_field_id()
        } else {
            find_field_id()
        };
        Task::batch([
            self.load_scope_files(),
            iced::widget::operation::focus(focus),
            iced::widget::operation::select_all(find_field_id()),
        ])
    }

    /// The folder to look in, remembered for the list.
    pub(super) fn set_finder_dir(&mut self, dir: PathBuf) {
        self.finder.dir_text = dir.display().to_string();
        self.finder.dir_history.retain(|d| *d != dir);
        self.finder.dir_history.insert(0, dir.clone());
        self.finder.dir_history.truncate(HISTORY);
        self.finder.dir = Some(dir);
    }

    /// The folder field typed in: a folder that exists is looked in.
    pub(super) fn finder_dir_typed(&mut self, text: String) -> Task<Message> {
        let dir = PathBuf::from(text.trim());
        let valid = !text.trim().is_empty() && dir.is_dir();
        self.finder.dir_text = text;
        if !valid || self.finder.dir.as_ref() == Some(&dir) {
            return Task::none();
        }
        self.finder.dir = Some(dir);
        self.load_scope_files()
    }

    /// Looks in a folder of a project (from its menu in the tree).
    pub(super) fn find_in_folder(&mut self, dir: PathBuf, replace: bool) -> Task<Message> {
        self.projects.selected = Some(dir);
        self.open_finder(replace)
    }

    /// The files the scope needs listed (the projects, or the folder).
    pub(super) fn load_scope_files(&mut self) -> Task<Message> {
        let dir = (self.finder.scope == Scope::Directory)
            .then(|| self.finder.dir.clone())
            .flatten();
        self.load_finder_files(dir)
    }

    pub(super) fn finder_files(&mut self, files: Vec<(PathBuf, String)>) -> Task<Message> {
        self.finder.files = files;
        self.finder.loading = false;
        if self.overlay == ActiveOverlay::FindInFiles {
            return self.search_files(false);
        }
        Task::none()
    }

    /// The search as set in the window.
    pub(super) fn finder_search(&self) -> Search {
        let f = &self.finder;
        Search {
            options: Options {
                query: f.text.clone(),
                case_sensitive: f.case_sensitive,
                words: f.words,
                regex: f.regex,
                context: f.context,
                mask: (f.mask_on && !f.mask.trim().is_empty()).then(|| f.mask.clone()),
            },
            replace: f.replace_mode.then(|| Replace {
                template: f.replace.clone(),
                preserve_case: f.preserve_case,
            }),
            scope: f.scope,
            dir: f.dir.clone(),
            recursive: f.recursive,
        }
    }

    /// The files a search looks in.
    fn files_of(&self, search: &Search) -> Vec<PathBuf> {
        let real = |p: &PathBuf| !is_untitled(p) && !is_virtual(p);
        match search.scope {
            Scope::Project => self.finder.files.iter().map(|(p, _)| p.clone()).collect(),
            Scope::Directory => {
                let Some(dir) = &search.dir else {
                    return Vec::new();
                };
                self.finder
                    .files
                    .iter()
                    .map(|(p, _)| p.clone())
                    .filter(|p| {
                        p.starts_with(dir)
                            && (search.recursive || p.parent() == Some(dir.as_path()))
                    })
                    .collect()
            }
            Scope::Open => self
                .sidebar
                .files()
                .iter()
                .filter(|p| real(p))
                .cloned()
                .collect(),
            Scope::Recent => {
                let mut seen = HashSet::new();
                self.mru
                    .iter()
                    .rev()
                    .cloned()
                    .chain(self.config.recent_files.iter().map(PathBuf::from))
                    .filter(|p| real(p) && p.is_file() && seen.insert(p.clone()))
                    .collect()
            }
            Scope::Changed => self
                .file_marks
                .changed_files()
                .into_iter()
                .filter(|p| p.is_file())
                .collect(),
            Scope::Current => self
                .current_file
                .iter()
                .filter(|p| real(p))
                .cloned()
                .collect(),
        }
    }

    /// The texts of the files open in the editor (their unsaved changes are
    /// what is searched and replaced).
    pub(super) fn open_texts(&self) -> HashMap<PathBuf, String> {
        let mut texts: HashMap<PathBuf, String> = self
            .file_cache
            .iter()
            .filter_map(|(p, b)| b.text().map(|t| (p.clone(), t)))
            .collect();
        if let Some(path) = &self.current_file {
            texts.insert(
                path.clone(),
                crate::editor::buffer_text(&self.editor.content),
            );
        }
        texts
    }

    /// Runs `search` in the background; `on_done` makes the message that
    /// brings the matches back.
    pub(super) fn run_search(
        &self,
        search: Search,
        wait: bool,
        on_done: impl Fn(Result<Vec<Occurrence>, String>) -> Message + Send + 'static,
    ) -> Task<Message> {
        let files = self.files_of(&search);
        let open = self.open_texts();
        Task::perform(
            async move {
                if wait {
                    tokio::time::sleep(std::time::Duration::from_millis(220)).await;
                }
                tokio::task::spawn_blocking(move || {
                    crate::finder::find_occurrences(
                        &files,
                        &open,
                        &search.options,
                        search.replace.as_ref(),
                    )
                    .map(|found| {
                        found
                            .into_iter()
                            .map(|(hit, replacement)| Occurrence {
                                hit,
                                replacement,
                                excluded: false,
                            })
                            .collect()
                    })
                })
                .await
                .unwrap_or_else(|e| Err(e.to_string()))
            },
            on_done,
        )
    }

    /// The TODO, FIXME, HACK, XXX and BUG comments of the projects, to the
    /// Results panel.
    pub(super) fn find_todos(&mut self) -> Task<Message> {
        let roots = self.finder_roots();
        if roots.is_empty() {
            self.set_status(t!("finder.no_project"), true);
            return Task::none();
        }
        let exclude = self.config.projects_exclude.clone();
        let open = self.open_texts();
        self.set_status(t!("todo.looking"), false);
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || todo_hits(&roots, &exclude, &open))
                    .await
                    .unwrap_or_else(|e| Err(e.to_string()))
            },
            Message::TodosFound,
        )
    }

    /// Looks again, a moment after the last key (`wait`), or at once.
    pub(super) fn search_files(&mut self, wait: bool) -> Task<Message> {
        self.finder.generation += 1;
        let generation = self.finder.generation;
        self.finder.error = None;
        self.finder.confirm_all = false;
        if self.finder.text.is_empty() {
            self.finder.hits.clear();
            self.finder.searching = false;
            self.finder.searched = None;
            return Task::none();
        }
        self.finder.searching = true;
        let search = self.finder_search();
        self.finder.searched = Some(search.clone());
        self.run_search(search, wait, move |result| {
            Message::FindResults(generation, result)
        })
    }

    pub(super) fn find_results(
        &mut self,
        generation: u64,
        result: Result<Vec<Occurrence>, String>,
    ) {
        // An older search, for what was typed before.
        if generation != self.finder.generation {
            return;
        }
        self.finder.searching = false;
        match result {
            Ok(hits) => {
                self.finder.hits = hits;
                self.finder.selected = if self.finder.preview_edited {
                    self.finder
                        .selected
                        .min(self.finder.hits.len().saturating_sub(1))
                } else {
                    0
                };
            }
            Err(e) => {
                self.finder.hits.clear();
                self.finder.error = Some(e);
            }
        }
        self.refresh_finder_preview();
    }

    /// Reads the file of the selected match, for the preview.
    fn refresh_finder_preview(&mut self) {
        let Some(path) = self
            .finder
            .hits
            .get(self.finder.selected)
            .map(|o| o.hit.path.clone())
        else {
            self.finder.preview = None;
            return;
        };
        let hit = self.finder.hits[self.finder.selected].hit.clone();
        let same = self
            .finder
            .preview
            .as_ref()
            .is_some_and(|(p, _)| *p == path);
        if !same {
            let text = self.file_text(&path).map(|(t, _)| t).unwrap_or_default();
            self.finder.preview = Some((path, crate::code_editor::Content::with_text(&text)));
        }
        if !(same && self.finder.preview_edited) {
            if let Some((_, content)) = self.finder.preview.as_mut() {
                content.select_bytes(hit.line, hit.col, hit.len);
            }
        }
    }

    /// A file's text now (the editor's, a tab's, or the file's), and where
    /// it is.
    pub(super) fn file_text(&self, path: &Path) -> Option<(String, Holder)> {
        if self.current_file.as_deref() == Some(path) {
            return Some((
                crate::editor::buffer_text(&self.editor.content),
                Holder::Editor,
            ));
        }
        if let Some(text) = self.file_cache.get(path).and_then(CachedBuffer::text) {
            return Some((text, Holder::Tab));
        }
        let bytes = std::fs::read(path).ok()?;
        String::from_utf8(bytes).ok().map(|t| (t, Holder::Disk))
    }

    /// Goes to the match picked (or the selected one); the window stays
    /// when pinned.
    pub(super) fn find_open(&mut self, pick: Option<usize>) -> Task<Message> {
        let index = pick.unwrap_or(self.finder.selected);
        let Some(hit) = self.finder.hits.get(index).map(|o| o.hit.clone()) else {
            return Task::none();
        };
        let query = self.finder.text.clone();
        remember(&mut self.config.find_history, &query);
        self.config_dirty = true;
        if !self.finder.pinned {
            self.overlay = ActiveOverlay::None;
        }
        self.remember_place();
        self.jump_to(crate::definition::Hit {
            path: hit.path,
            line: hit.line,
            col: hit.col,
            len: hit.len,
        })
    }

    /// The arrows move in the list.
    pub(super) fn finder_step(&mut self, delta: i32) -> Task<Message> {
        if self.overlay != ActiveOverlay::FindInFiles {
            return Task::none();
        }
        let len = self.finder.hits.len();
        self.finder.selected = crate::palette::step(self.finder.selected, delta, len);
        self.refresh_finder_preview();
        iced::widget::operation::scroll_to(
            list_id(),
            iced::widget::scrollable::AbsoluteOffset {
                x: 0.0,
                y: ((self.finder.selected as f32 - 4.0) * ROW).max(0.0),
            },
        )
    }

    /// What is looked for, in the field and in the lines.
    pub(super) fn finder_set_text(&mut self, text: String) {
        self.finder.lines = text_editor::Content::with_text(&text);
        self.finder.text = text;
    }

    /// One line or several: going back to one keeps the first.
    pub(super) fn finder_toggle_multiline(&mut self) -> Task<Message> {
        self.finder.multiline = !self.finder.multiline;
        if !self.finder.multiline {
            let first = self.finder.text.lines().next().unwrap_or("").to_string();
            self.finder_set_text(first);
        }
        self.search_files(false)
    }

    /// A key in the lines of the search.
    pub(super) fn finder_lines_action(&mut self, action: text_editor::Action) -> Task<Message> {
        let edited = action.is_edit();
        self.finder.lines.perform(action);
        if !edited {
            return Task::none();
        }
        self.finder.text = self.finder.lines.text().trim_end_matches('\n').to_string();
        self.search_files(true)
    }

    #[cfg(test)]
    pub(super) fn finder_generation(&self) -> u64 {
        self.finder.generation
    }

    /// A row clicked: it is selected; clicked twice, it is opened.
    pub(super) fn finder_select(&mut self, index: usize) -> Task<Message> {
        let now = std::time::Instant::now();
        let twice = self.finder.last_click.is_some_and(|(i, at)| {
            i == index && now.duration_since(at) < std::time::Duration::from_millis(450)
        });
        self.finder.last_click = Some((index, now));
        self.finder.selected = index;
        self.finder.preview_edited = false;
        self.refresh_finder_preview();
        if twice {
            self.finder.last_click = None;
            return self.find_open(Some(index));
        }
        Task::none()
    }

    /// A key or a click in the preview: it is the file, edited there.
    pub(super) fn finder_preview_action(&mut self, action: text_editor::Action) -> Task<Message> {
        let edited = action.is_edit();
        let Some((path, content)) = self.finder.preview.as_mut() else {
            return Task::none();
        };
        content.perform(action);
        if !edited {
            return Task::none();
        }
        let path = path.clone();
        let new = crate::editor::buffer_text(content);
        let Some((old, holder)) = self.file_text(&path) else {
            return Task::none();
        };
        if old != new && self.put_text(&path, &holder, &old, &new) {
            self.finder.preview_edited = true;
            return self.search_files(true);
        }
        Task::none()
    }

    /// The right-click menu of the preview, on its own text.
    pub(super) fn finder_preview_menu(&mut self, what: PreviewMenu) -> Task<Message> {
        let Some((_, content)) = self.finder.preview.as_mut() else {
            return Task::none();
        };
        let selected = content.selection();
        match what {
            PreviewMenu::Copy => selected.map_or_else(Task::none, iced::clipboard::write),
            PreviewMenu::Cut => {
                let Some(text) = selected else {
                    return Task::none();
                };
                let task = self
                    .finder_preview_action(text_editor::Action::Edit(text_editor::Edit::Delete));
                Task::batch([iced::clipboard::write(text), task])
            }
            PreviewMenu::Delete => {
                if selected.is_none() {
                    return Task::none();
                }
                self.finder_preview_action(text_editor::Action::Edit(text_editor::Edit::Delete))
            }
            PreviewMenu::Paste => iced::clipboard::read().map(Message::FindPreviewPasted),
            PreviewMenu::SelectAll => {
                content.perform(text_editor::Action::SelectAll);
                Task::none()
            }
        }
    }

    // ── Replacing ──

    /// Replaces, as `search` says, the matches of `chosen` (found by it)
    /// that are still there: in the editor and the tabs (undone with
    /// Ctrl+Z), in the other files on disk (undone with "Undo the
    /// replacement"). Gives how many matches, in how many files.
    pub(super) fn replace_found(&mut self, search: &Search, chosen: &[TextHit]) -> (usize, usize) {
        let Some(replace) = &search.replace else {
            return (0, 0);
        };
        let Ok(re) = crate::find_replace::compile(&search.options) else {
            return (0, 0);
        };
        let mut by_file: Vec<(PathBuf, HashSet<(usize, usize)>)> = Vec::new();
        for hit in chosen {
            match by_file.iter_mut().find(|(p, _)| *p == hit.path) {
                Some((_, places)) => {
                    places.insert((hit.line, hit.col));
                }
                None => by_file.push((hit.path.clone(), HashSet::from([(hit.line, hit.col)]))),
            }
        }
        let (mut count, mut files) = (0, 0);
        let mut written = Vec::new();
        for (path, places) in by_file {
            let Some((text, holder)) = self.file_text(&path) else {
                continue;
            };
            let ext = path
                .extension()
                .map(|e| e.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            // Looked for again in the text of now: only the matches that
            // are still where they were found.
            let kept: Vec<crate::find_replace::Found> = crate::find_replace::find_in(
                &text,
                &ext,
                &re,
                search.options.context,
                search.options.regex,
                Some(replace),
            )
            .into_iter()
            .filter(|f| places.contains(&(f.line, f.col)))
            .collect();
            if kept.is_empty() {
                continue;
            }
            let new = crate::find_replace::apply(&text, &kept);
            if new == text {
                continue;
            }
            self.history_record(&path, &text);
            if !self.put_text(&path, &holder, &text, &new) {
                continue;
            }
            self.history_record(&path, &new);
            count += kept.len();
            files += 1;
            written.push(Written {
                path,
                before: text,
                after: new,
            });
        }
        if !written.is_empty() {
            self.finder.undo = written;
        }
        (count, files)
    }

    /// A file's text becomes `new`, where it is held: undone with Ctrl+Z in
    /// the editor and the tabs. Whether it worked.
    pub(super) fn put_text(&mut self, path: &Path, holder: &Holder, text: &str, new: &str) -> bool {
        match holder {
            Holder::Editor => {
                let (line, col) = self.editor.content.cursor_position();
                let line = line.min(new.split('\n').count().saturating_sub(1));
                let _ = self.apply_text_edit(new.to_string(), line, col, None);
            }
            Holder::Tab => {
                self.bookmarks.remap(path, text, new);
                if let Some(buffer) = self.file_cache.get_mut(path) {
                    if let Some(content) = buffer.content.as_mut() {
                        crate::editor::replace_text(content, text, new);
                    }
                    buffer.dirty = true;
                }
                let (undo, redo) = self.undo_cache.entry(path.to_path_buf()).or_default();
                undo.push_back(Snapshot {
                    text: text.to_string(),
                    cursor: (0, 0),
                });
                redo.clear();
            }
            Holder::Disk => {
                if std::fs::write(path, new).is_err() {
                    return false;
                }
                self.bookmarks.remap(path, text, new);
            }
        }
        true
    }

    /// Puts back what the last replacement wrote, in the files that have not
    /// changed since.
    pub(super) fn undo_replacement(&mut self) -> Task<Message> {
        let written = std::mem::take(&mut self.finder.undo);
        let mut restored = 0;
        for w in &written {
            let Some((text, holder)) = self.file_text(&w.path) else {
                continue;
            };
            if text != w.after {
                continue;
            }
            match holder {
                Holder::Editor => {
                    let (line, col) = self.editor.content.cursor_position();
                    let _ = self.apply_text_edit(w.before.clone(), line, col, None);
                }
                Holder::Tab => {
                    if let Some(content) = self
                        .file_cache
                        .get_mut(&w.path)
                        .and_then(|b| b.content.as_mut())
                    {
                        crate::editor::replace_text(content, &w.after, &w.before);
                    }
                    self.bookmarks.remap(&w.path, &w.after, &w.before);
                }
                Holder::Disk => {
                    if std::fs::write(&w.path, &w.before).is_err() {
                        continue;
                    }
                    self.bookmarks.remap(&w.path, &w.after, &w.before);
                }
            }
            restored += 1;
        }
        let files = files_text(restored);
        self.set_status(t!("findrep.undone", files = files.as_str()), false);
        self.search_files(false)
    }

    /// Replace (in the window): the selected match, then the next one.
    pub(super) fn finder_replace_one(&mut self) -> Task<Message> {
        let Some(search) = self.finder.searched.clone().filter(|s| s.replace.is_some()) else {
            return Task::none();
        };
        let Some(occurrence) = self.finder.hits.get(self.finder.selected).cloned() else {
            return Task::none();
        };
        let replace = self.finder.replace.clone();
        remember(&mut self.config.find_history, &search.options.query);
        remember(&mut self.config.replace_history, &replace);
        self.config_dirty = true;
        let (count, _) = self.replace_found(&search, std::slice::from_ref(&occurrence.hit));
        if count > 0 {
            let index = self.finder.selected;
            self.finder.hits.remove(index);
            self.finder.selected = index.min(self.finder.hits.len().saturating_sub(1));
            self.finder.preview = None;
            self.refresh_finder_preview();
        }
        Task::none()
    }

    /// Replace all (after the confirmation): every match not left out.
    pub(super) fn finder_replace_all(&mut self) -> Task<Message> {
        self.finder.confirm_all = false;
        let Some(search) = self.finder.searched.clone().filter(|s| s.replace.is_some()) else {
            return Task::none();
        };
        let replace = self.finder.replace.clone();
        remember(&mut self.config.find_history, &search.options.query);
        remember(&mut self.config.replace_history, &replace);
        self.config_dirty = true;
        let chosen: Vec<TextHit> = self
            .finder
            .hits
            .iter()
            .filter(|o| !o.excluded)
            .map(|o| o.hit.clone())
            .collect();
        let (count, files) = self.replace_found(&search, &chosen);
        let (matches, files) = (matches_text(count), files_text(files));
        let done = t!("findrep.replaced", matches = matches, files = files);
        self.set_status(done, false);
        self.search_files(false)
    }

    // ── On screen ──

    fn finder_row<'a>(
        &self,
        dark: bool,
        highlighted: bool,
        content: Element<'a, Message>,
        on_press: Message,
    ) -> Element<'a, Message> {
        button(container(content).center_y(Length::Fixed(ROW)))
            .padding([0, 10])
            .width(Length::Fill)
            .on_press(on_press)
            .style(
                (se_theme::MenuItem {
                    dark,
                    radius: se_theme::R_ROW,
                    highlighted,
                })
                .style(),
            )
            .into()
    }

    /// A match as a line of a list: the file and line, then the line with
    /// the match marked (and what replaces it).
    pub(super) fn occurrence_line<'a>(
        &self,
        o: &'a Occurrence,
        dark: bool,
        highlighted: bool,
    ) -> Element<'a, Message> {
        let p = se_theme::palette(dark);
        let hit = &o.hit;
        let name = hit
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let (from, to) = hit.preview_at;
        let part = |a: usize, b: usize| hit.preview.get(a..b).unwrap_or("").to_string();
        let text_color = if o.excluded { p.muted } else { p.text };
        let mut spans: Vec<iced::widget::text::Span<'_, ()>> = vec![
            span(part(0, from)).color(text_color).font(se_theme::MONO),
            span(part(from, to))
                .color(if o.excluded { p.muted } else { p.accent })
                .font(se_theme::MONO),
        ];
        if let Some(new) = o.replacement.as_ref().filter(|_| !o.excluded) {
            spans.push(
                span(format!(" → {}", new.replace('\n', "⏎")))
                    .color(iced::Color::from_rgb8(0x6a, 0xb8, 0x4f))
                    .font(se_theme::MONO),
            );
        }
        spans.push(
            span(part(to, hit.preview.len()))
                .color(text_color)
                .font(se_theme::MONO),
        );
        let mut line = row![
            crate::file_icons::file(&hit.path, dark),
            text(format!("{name}:{}", hit.line + 1))
                .size(12)
                .color(if highlighted { p.accent } else { p.muted })
                .width(Length::Fixed(170.0))
                .wrapping(iced::widget::text::Wrapping::None),
            rich_text(spans)
                .size(12.5)
                .wrapping(iced::widget::text::Wrapping::None),
        ]
        .spacing(10)
        .align_y(Alignment::Center);
        if o.excluded {
            line = line.push(Space::new().width(Length::Fill)).push(
                text(t!("findrep.excluded").to_string())
                    .size(11)
                    .color(p.muted),
            );
        }
        line.clip(true).into()
    }

    /// The file of the selected match, in an editor: the match selected,
    /// the text editable, a right-click menu.
    fn finder_preview(&self, dark: bool) -> Element<'_, Message> {
        let Some((path, content)) = self.finder.preview.as_ref() else {
            return Space::new().height(Length::Fill).into();
        };
        let first = content.line(0).map(|l| l.to_string()).unwrap_or_default();
        let extension = crate::editor::detect_language(path, &first)
            .map(str::to_string)
            .unwrap_or_default();
        let editor = crate::code_editor::CodeEditor::new(content)
            .size(12)
            .font(se_theme::MONO)
            .on_action(Message::FindPreviewAction)
            .highlight::<crate::editor::highlighter::Highlighter>(
                crate::editor::highlighter::Settings { dark, extension },
                crate::editor::highlighter::to_format,
            )
            .style(crate::theme::editor_style(dark))
            .height(Length::Fill);
        let labels = (
            t!("ctx.cut").to_string(),
            t!("ctx.copy").to_string(),
            t!("ctx.paste").to_string(),
            t!("ctx.delete").to_string(),
            t!("ctx.select_all").to_string(),
        );
        crate::pointer_area::context_menu(editor, move || {
            use crate::menu_bar::item;
            let entry = |label: &String, what| {
                item(label.clone(), None, dark, Message::FindPreviewMenu(what))
            };
            container(
                column![
                    entry(&labels.0, PreviewMenu::Cut),
                    entry(&labels.1, PreviewMenu::Copy),
                    entry(&labels.2, PreviewMenu::Paste),
                    entry(&labels.3, PreviewMenu::Delete),
                    entry(&labels.4, PreviewMenu::SelectAll),
                ]
                .spacing(1)
                .padding(6)
                .width(Length::Fixed(200.0)),
            )
            .style(crate::theme::card(dark))
            .into()
        })
    }

    pub(super) fn view_find_in_files(&self, dark: bool) -> Element<'_, Message> {
        let p = se_theme::palette(dark);
        let f = &self.finder;
        let ghost = move |active: bool| (se_theme::GhostButton { dark, active }).style();
        let toggle = |label: &'static str, on: bool, tip: String, msg: Message| {
            iced::widget::tooltip(
                button(text(label).size(12.5).font(se_theme::MONO))
                    .padding([3, 8])
                    .on_press(msg)
                    .style(ghost(on)),
                container(text(tip).size(12))
                    .padding([4, 8])
                    .style(se_theme::tooltip(dark)),
                iced::widget::tooltip::Position::Bottom,
            )
        };

        // Find or replace, how many, the filter, the mask, the pin.
        let (n, files) = counts(&f.hits);
        let summary = if f.searching {
            t!("finder.searching").to_string()
        } else if f.text.is_empty() {
            String::new()
        } else {
            let (matches, files) = (matches_text(n), files_text(files));
            t!("findrep.count", matches = matches, files = files).to_string()
        };
        let context_color = if f.context == Context::Anywhere {
            p.text
        } else {
            p.accent
        };
        let mut header = row![
            button(text(t!("findrep.find").to_string()).size(12.5))
                .padding([4, 10])
                .on_press(Message::FindMode(false))
                .style(ghost(!f.replace_mode)),
            button(text(t!("findrep.replace").to_string()).size(12.5))
                .padding([4, 10])
                .on_press(Message::FindMode(true))
                .style(ghost(f.replace_mode)),
            text(summary).size(12).color(p.muted),
            Space::new().width(Length::Fill),
            button(
                text(format!("{} ▾", context_label(f.context)))
                    .size(12)
                    .color(context_color)
            )
            .padding([4, 10])
            .on_press(Message::FindMenu(Some(Menu::Context)))
            .style(ghost(f.menu == Some(Menu::Context))),
            iced::widget::checkbox(f.mask_on)
                .label(t!("findrep.mask").to_string())
                .text_size(12)
                .size(14)
                .on_toggle(Message::FindMaskOn),
        ]
        .spacing(6)
        .align_y(Alignment::Center);
        if f.mask_on {
            header = header.push(
                text_input("*.rs, !*_test.rs", &f.mask)
                    .on_input(Message::FindMask)
                    .size(12.5)
                    .padding([3, 8])
                    .width(150)
                    .style(se_theme::Field::new(dark).style()),
            );
        }
        header = header.push(toggle(
            "⊙",
            f.pinned,
            t!("findrep.pin").to_string(),
            Message::FindPin,
        ));
        header = header.push(iced::widget::tooltip(
            button(text("×").size(16).color(p.muted))
                .padding([1, 8])
                .on_press(Message::CloseOverlay)
                .style(ghost(false)),
            container(text(t!("history.close").to_string()).size(12))
                .padding([4, 8])
                .style(se_theme::tooltip(dark)),
            iced::widget::tooltip::Position::Bottom,
        ));

        let find_field: Element<Message> = if f.multiline {
            iced::widget::text_editor(&f.lines)
                .on_action(Message::FindLines)
                .font(se_theme::MONO)
                .size(13)
                .height(Length::Fixed(72.0))
                .into()
        } else {
            text_input(&t!("finder.find_placeholder"), &f.text)
                .id(find_field_id())
                .on_input(Message::FindQuery)
                .on_submit(Message::FindOpen(None))
                .size(15)
                .padding(0)
                .style((se_theme::BareField { dark }).style())
                .into()
        };
        let find_row = row![
            find_field,
            toggle(
                "Aa",
                f.case_sensitive,
                t!("search.case_sensitive").to_string(),
                Message::FindToggleCase
            ),
            toggle(
                "W",
                f.words,
                t!("everywhere.words").to_string(),
                Message::FindToggleWords
            ),
            toggle(
                ".*",
                f.regex,
                t!("search.regex").to_string(),
                Message::FindToggleRegex
            ),
            toggle(
                "⏎",
                f.multiline,
                t!("findrep.multiline").to_string(),
                Message::FindToggleMultiline
            ),
            toggle(
                "⌄",
                f.menu == Some(Menu::History),
                t!("findrep.history").to_string(),
                Message::FindMenu(Some(Menu::History))
            ),
        ]
        .spacing(6)
        .align_y(Alignment::Center);

        let mut body = column![
            container(header).padding([8, 12]),
            container(Space::new().height(1))
                .width(Length::Fill)
                .style(se_theme::rule(dark)),
            container(find_row)
                .padding([0, 16])
                .height(if f.multiline { 84 } else { 46 })
                .center_y(if f.multiline { 84 } else { 46 }),
        ];
        if f.replace_mode {
            let replace_row = row![
                text_input(&t!("findrep.replace_placeholder"), &f.replace)
                    .id(replace_field_id())
                    .on_input(Message::FindReplaceText)
                    .on_submit(Message::FindReplaceOne)
                    .size(15)
                    .padding(0)
                    .style((se_theme::BareField { dark }).style()),
                toggle(
                    "AB",
                    f.preserve_case,
                    t!("findrep.preserve_case").to_string(),
                    Message::FindTogglePreserve
                ),
            ]
            .spacing(6)
            .align_y(Alignment::Center);
            body = body.push(
                container(replace_row)
                    .padding([0, 16])
                    .height(40)
                    .center_y(40),
            );
        }
        // The list under the fields: the contexts, or the searches made.
        match f.menu {
            Some(Menu::Context) => {
                let mut list = column![].spacing(1);
                for context in Context::ALL {
                    list = list.push(
                        button(text(context_label(context)).size(12.5))
                            .padding([5, 10])
                            .width(Length::Fixed(280.0))
                            .on_press(Message::FindContext(context))
                            .style(ghost(context == f.context)),
                    );
                }
                body = body.push(container(list).padding([4, 16]));
            }
            Some(Menu::History) => {
                let mut list = column![].spacing(1);
                if self.config.find_history.is_empty() {
                    list = list.push(
                        text(t!("findrep.no_history").to_string())
                            .size(12)
                            .color(p.muted),
                    );
                }
                for past in &self.config.find_history {
                    list = list.push(
                        button(text(past.clone()).size(12.5).font(se_theme::MONO))
                            .padding([5, 10])
                            .width(Length::Fill)
                            .on_press(Message::FindHistory(past.clone()))
                            .style(ghost(false)),
                    );
                }
                body = body.push(container(list).padding([4, 16]));
            }
            Some(Menu::Dirs) | None => {}
        }
        // Where.
        let mut scopes = row![text(t!("findrep.where").to_string())
            .size(12)
            .color(p.muted)]
        .spacing(4)
        .align_y(Alignment::Center);
        for scope in Scope::ALL {
            let label = scope_label(scope);
            let mut b = button(text(label).size(12))
                .padding([3, 8])
                .style(ghost(scope == f.scope));
            b = b.on_press(if scope == Scope::Directory && f.dir.is_none() {
                Message::FindPickDir
            } else {
                Message::FindScope(scope)
            });
            scopes = scopes.push(b);
        }
        if f.scope == Scope::Directory {
            scopes = scopes.push(
                iced::widget::checkbox(f.recursive)
                    .label(t!("findrep.recursive").to_string())
                    .text_size(12)
                    .size(14)
                    .on_toggle(|_| Message::FindRecursive),
            );
        }
        scopes = scopes.push(
            iced::widget::checkbox(self.config.find_ignored)
                .label(t!("findrep.ignored").to_string())
                .text_size(12)
                .size(14)
                .on_toggle(Message::FindIgnored),
        );
        // Wraps onto a second line rather than overflowing the window.
        body = body.push(container(scopes.wrap().vertical_spacing(4)).padding([4, 12]));
        // The folder, as a field with the folders already looked in, and a
        // button to pick one.
        if f.scope == Scope::Directory {
            let valid = f
                .dir
                .as_ref()
                .is_some_and(|d| d.display().to_string() == f.dir_text.trim());
            let field = text_input("~/", &f.dir_text)
                .on_input(Message::FindDirText)
                .size(12.5)
                .padding([4, 8])
                .width(Length::Fill)
                .style(se_theme::Field::new(dark).style());
            let dir_row = row![
                field,
                button(text("▾").size(12))
                    .padding([3, 8])
                    .on_press(Message::FindMenu(Some(Menu::Dirs)))
                    .style(ghost(f.menu == Some(Menu::Dirs))),
                button(text("…").size(12))
                    .padding([3, 8])
                    .on_press(Message::FindPickDir)
                    .style(ghost(false)),
            ]
            .spacing(6)
            .align_y(Alignment::Center);
            body = body.push(container(dir_row).padding([2, 12]));
            if !valid && !f.dir_text.trim().is_empty() {
                body = body.push(
                    container(
                        text(t!("findrep.bad_dir").to_string())
                            .size(11.5)
                            .color(p.muted),
                    )
                    .padding([0, 14]),
                );
            }
            if f.menu == Some(Menu::Dirs) {
                let mut list = column![].spacing(1);
                let roots = self.projects.roots.iter();
                let mut dirs: Vec<&PathBuf> = f.dir_history.iter().chain(roots).collect();
                let mut seen = std::collections::HashSet::new();
                dirs.retain(|d| seen.insert((*d).clone()));
                for dir in dirs {
                    list = list.push(
                        button(text(display_path(dir)).size(12.5).font(se_theme::MONO))
                            .padding([5, 10])
                            .width(Length::Fill)
                            .on_press(Message::FindDirChosen(dir.clone()))
                            .style(ghost(f.dir.as_ref() == Some(dir))),
                    );
                }
                body = body.push(container(list).padding([4, 16]));
            }
        }
        body = body.push(
            container(Space::new().height(1))
                .width(Length::Fill)
                .style(se_theme::rule(dark)),
        );

        // The matches.
        let rows: Vec<Element<Message>> = f
            .hits
            .iter()
            .enumerate()
            .map(|(i, o)| {
                let highlighted = i == f.selected;
                self.finder_row(
                    dark,
                    highlighted,
                    self.occurrence_line(o, dark, highlighted),
                    Message::FindSelect(i),
                )
            })
            .collect();
        let mut list = column(rows).spacing(1).padding(6);
        let note = if let Some(error) = &f.error {
            error.clone()
        } else if self.finder_roots().is_empty() && f.scope == Scope::Project {
            t!("finder.no_project").to_string()
        } else if !f.searching && !f.text.is_empty() && f.hits.is_empty() {
            t!("everywhere.nothing").to_string()
        } else if f.hits.len() >= crate::finder::MAX_HITS {
            t!("finder.too_many", n = crate::finder::MAX_HITS).to_string()
        } else {
            String::new()
        };
        if !note.is_empty() {
            list = list.push(container(text(note).size(12.5).color(p.muted)).padding([9, 10]));
        }
        let rule = || {
            container(Space::new().height(1))
                .width(Length::Fill)
                .style(se_theme::rule(dark))
        };
        body = body
            .push(
                scrollable(list)
                    .id(list_id())
                    .style(se_theme::Scrollbar { dark }.style())
                    .height(Length::Fixed(250.0)),
            )
            .push(rule())
            .push(container(self.finder_preview(dark)).height(Length::Fixed(200.0)))
            .push(rule());

        // What to do.
        let mut actions = row![
            button(text(t!("findrep.to_window").to_string()).size(12.5))
                .padding([5, 12])
                .on_press_maybe((!f.hits.is_empty()).then_some(Message::FindToWindow))
                .style((se_theme::Outline { dark }).style()),
            text(t!("findrep.keys").to_string()).size(11).color(p.muted),
            Space::new().width(Length::Fill),
        ]
        .spacing(10)
        .align_y(Alignment::Center);
        if !f.undo.is_empty() {
            actions = actions.push(
                button(text(t!("findrep.undo").to_string()).size(12.5))
                    .padding([5, 12])
                    .on_press(Message::FindUndoReplace)
                    .style(ghost(false)),
            );
        }
        if f.replace_mode {
            let can =
                !f.hits.is_empty() && f.searched.as_ref().is_some_and(|s| s.replace.is_some());
            if f.confirm_all {
                let left: Vec<Occurrence> =
                    f.hits.iter().filter(|o| !o.excluded).cloned().collect();
                let (n, files) = counts(&left);
                let (matches, files) = (matches_text(n), files_text(files));
                let question = t!("findrep.confirm", matches = matches, files = files);
                actions = actions
                    .push(text(question.to_string()).size(12.5).color(p.accent))
                    .push(
                        button(text(t!("findrep.replace_all").to_string()).size(12.5))
                            .padding([5, 12])
                            .on_press(Message::FindReplaceConfirm(true))
                            .style((se_theme::Primary { dark }).style()),
                    )
                    .push(
                        button(text(t!("findrep.cancel").to_string()).size(12.5))
                            .padding([5, 12])
                            .on_press(Message::FindReplaceConfirm(false))
                            .style(ghost(false)),
                    );
            } else {
                actions = actions
                    .push(
                        button(text(t!("findrep.replace").to_string()).size(12.5))
                            .padding([5, 12])
                            .on_press_maybe(can.then_some(Message::FindReplaceOne))
                            .style((se_theme::Outline { dark }).style()),
                    )
                    .push(
                        button(text(t!("findrep.replace_all").to_string()).size(12.5))
                            .padding([5, 12])
                            .on_press_maybe(can.then_some(Message::FindReplaceAll))
                            .style((se_theme::Primary { dark }).style()),
                    );
            }
        }
        body = body.push(container(actions).padding([8, 12]));
        container(body)
            .width(920)
            .style(se_theme::palette_card(dark))
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app_on(dir: &Path) -> SimpleEditApp {
        let mut app = SimpleEditApp::blank(Config::default());
        let _ = app.restore_session(crate::session::Session::default());
        let _ = app.update(Message::ProjectPicked(Some(dir.to_path_buf())));
        app
    }

    fn project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src/sidebar")).unwrap();
        std::fs::write(
            dir.path().join("src/app.rs"),
            "fn main() {\n    run(); // run it\n}\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("src/sidebar/mod.rs"), "pub fn run() {}\n").unwrap();
        dir
    }

    fn listed(app: &mut SimpleEditApp, dir: &Path) {
        let files = crate::finder::list_files(dir, &[], true)
            .into_iter()
            .map(|f| {
                let rel = f.strip_prefix(dir).unwrap().to_string_lossy().into_owned();
                (f, rel)
            })
            .collect();
        let _ = app.update(Message::FinderFiles(files));
    }

    /// Runs the search set in the window at once, as the background would.
    fn searched(app: &mut SimpleEditApp) {
        let search = app.finder_search();
        app.finder.searched = Some(search.clone());
        let files = app.files_of(&search);
        let open = app.open_texts();
        let found = crate::finder::find_occurrences(
            &files,
            &open,
            &search.options,
            search.replace.as_ref(),
        )
        .map(|f| {
            f.into_iter()
                .map(|(hit, replacement)| Occurrence {
                    hit,
                    replacement,
                    excluded: false,
                })
                .collect()
        });
        let generation = app.finder.generation;
        let _ = app.update(Message::FindResults(generation, found));
    }

    #[test]
    fn a_text_is_found_in_the_files_and_its_place_opened() {
        let dir = project();
        let mut app = app_on(dir.path());
        let _ = app.update(Message::OpenFindInFiles);
        assert_eq!(app.overlay, ActiveOverlay::FindInFiles);
        listed(&mut app, dir.path());
        let _ = app.update(Message::FindQuery("run".into()));
        assert!(app.finder.searching);
        let generation = app.finder.generation;
        let _ = app.update(Message::FindResults(generation - 1, Ok(Vec::new())));
        assert!(app.finder.searching, "an older answer is dropped");
        searched(&mut app);
        assert_eq!(app.finder.hits.len(), 3);
        // Only outside the comments.
        let _ = app.update(Message::FindContext(Context::ExceptComments));
        searched(&mut app);
        assert_eq!(app.finder.hits.len(), 2);
        assert!(
            app.finder.preview.is_some(),
            "the selected match is previewed"
        );
        let target = app.finder.hits[1].hit.clone();
        let _ = app.update(Message::FindOpen(Some(1)));
        assert_eq!(app.overlay, ActiveOverlay::None);
        assert_eq!(app.pending_open.as_ref(), Some(&target.path));
        assert_eq!(
            app.config.find_history.first().map(String::as_str),
            Some("run")
        );
        assert!(app.config_dirty, "the history is kept");
    }

    #[test]
    fn a_row_clicked_twice_is_opened() {
        let dir = project();
        let mut app = app_on(dir.path());
        listed(&mut app, dir.path());
        let _ = app.update(Message::OpenFindInFiles);
        let _ = app.update(Message::FindQuery("run".into()));
        searched(&mut app);
        let _ = app.update(Message::FindSelect(1));
        assert_eq!(app.overlay, ActiveOverlay::FindInFiles, "one click selects");
        let _ = app.update(Message::FindSelect(1));
        assert_eq!(app.overlay, ActiveOverlay::None, "two clicks open");
    }

    #[test]
    fn the_folder_is_typed_or_taken_from_the_list() {
        let dir = project();
        let mut app = app_on(dir.path());
        let _ = app.update(Message::OpenFindInFiles);
        let src = dir.path().join("src");
        let _ = app.update(Message::FindScope(Scope::Directory));
        let _ = app.update(Message::FindDirText(src.display().to_string()));
        assert_eq!(app.finder.dir.as_deref(), Some(src.as_path()));
        let _ = app.update(Message::FindDirText("/does/not/exist".into()));
        assert_eq!(app.finder.dir.as_deref(), Some(src.as_path()), "kept");
        let _ = app.update(Message::FindDirChosen(dir.path().to_path_buf()));
        assert_eq!(app.finder.dir.as_deref(), Some(dir.path()));
        assert_eq!(app.finder.dir_history.len(), 1);
    }

    #[test]
    fn the_preview_is_edited_in_the_file() {
        let dir = project();
        let mut app = app_on(dir.path());
        listed(&mut app, dir.path());
        let _ = app.update(Message::OpenFindInFiles);
        let _ = app.update(Message::FindQuery("run".into()));
        searched(&mut app);
        let path = app.finder.hits[app.finder.selected].hit.path.clone();
        let _ = app.update(Message::FindPreviewAction(text_editor::Action::Edit(
            text_editor::Edit::Paste(std::sync::Arc::new("X".into())),
        )));
        assert!(
            std::fs::read_to_string(&path).unwrap().contains('X'),
            "the typing reaches the file"
        );
    }

    #[test]
    fn a_folder_its_subfolders_or_not_and_a_mask() {
        let dir = project();
        let mut app = app_on(dir.path());
        let _ = app.update(Message::FindInFolder(dir.path().join("src")));
        assert_eq!(app.finder.scope, Scope::Directory);
        listed(&mut app, dir.path());
        let _ = app.update(Message::FindQuery("fn".into()));
        searched(&mut app);
        assert_eq!(app.finder.hits.len(), 2);
        let _ = app.update(Message::FindRecursive);
        searched(&mut app);
        assert_eq!(app.finder.hits.len(), 1, "not in src/sidebar");
        let _ = app.update(Message::FindRecursive);
        let _ = app.update(Message::FindMaskOn(true));
        let _ = app.update(Message::FindMask("*.rs, !mod.rs".into()));
        searched(&mut app);
        assert_eq!(app.finder.hits.len(), 1);
        // The open files only: none open.
        let _ = app.update(Message::FindScope(Scope::Open));
        searched(&mut app);
        assert!(app.finder.hits.is_empty());
    }

    #[test]
    fn replace_one_then_all_then_undo() {
        let dir = project();
        let mut app = app_on(dir.path());
        let _ = app.update(Message::OpenReplaceInFiles);
        assert!(app.finder.replace_mode);
        listed(&mut app, dir.path());
        let _ = app.update(Message::FindQuery("(r)un".into()));
        let _ = app.update(Message::FindToggleRegex);
        let _ = app.update(Message::FindReplaceText(r"\u${1}esume".into()));
        searched(&mut app);
        assert_eq!(app.finder.hits.len(), 3);
        assert_eq!(app.finder.hits[0].replacement.as_deref(), Some("Resume"));
        // One: the selected match goes, in its file on disk.
        let first = app.finder.hits[0].hit.path.clone();
        let _ = app.update(Message::FindReplaceOne);
        assert_eq!(app.finder.hits.len(), 2);
        assert!(std::fs::read_to_string(&first).unwrap().contains("Resume"));
        // All the others, but one left out; after a confirmation.
        app.finder.hits[0].excluded = true;
        let _ = app.update(Message::FindReplaceAll);
        assert!(app.finder.confirm_all);
        let _ = app.update(Message::FindReplaceConfirm(true));
        let read_all = || -> String {
            [
                dir.path().join("src/app.rs"),
                dir.path().join("src/sidebar/mod.rs"),
            ]
            .iter()
            .map(|p| std::fs::read_to_string(p).unwrap())
            .collect()
        };
        let all = read_all();
        assert_eq!(all.matches("Resume").count(), 2);
        assert_eq!(all.matches("run").count(), 1, "the one left out stays");
        // Undo puts back what the last replacement wrote.
        let _ = app.update(Message::FindUndoReplace);
        assert_eq!(read_all().matches("Resume").count(), 1);
        assert_eq!(
            app.config.replace_history.first().map(String::as_str),
            Some(r"\u${1}esume")
        );
    }

    #[test]
    fn a_file_open_in_the_editor_is_replaced_there_and_undone_with_ctrl_z() {
        let dir = project();
        let mut app = app_on(dir.path());
        let path = dir.path().join("src/app.rs");
        let _ = app.update(Message::TabRead(Ok(Loaded {
            path: path.clone(),
            text: std::fs::read_to_string(&path).unwrap(),
            format: TextFormat::default(),
            stamp: None,
        })));
        // An unsaved change: it is what is searched.
        app.editor.content.select_bytes(0, 0, 0);
        let _ = app.update(Message::EditorAction(text_editor::Action::Edit(
            text_editor::Edit::Paste(std::sync::Arc::new("// run\n".into())),
        )));
        let _ = app.update(Message::OpenReplaceInFiles);
        let _ = app.update(Message::FindScope(Scope::Current));
        let _ = app.update(Message::FindQuery("run".into()));
        let _ = app.update(Message::FindReplaceText("go".into()));
        searched(&mut app);
        assert_eq!(app.finder.hits.len(), 3);
        let _ = app.update(Message::FindReplaceAll);
        let _ = app.update(Message::FindReplaceConfirm(true));
        let text = crate::editor::buffer_text(&app.editor.content);
        assert_eq!(text.matches("go").count(), 3);
        assert!(
            !std::fs::read_to_string(&path).unwrap().contains("go"),
            "not written to disk"
        );
        // Back in the editor, Ctrl+Z.
        let _ = app.update(Message::CloseOverlay);
        let _ = app.update(Message::Undo);
        assert_eq!(
            crate::editor::buffer_text(&app.editor.content)
                .matches("run")
                .count(),
            3
        );
    }

    #[test]
    fn git_ignored_files_can_be_left_out() {
        let dir = project();
        std::process::Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args(["init", "-q"])
            .output()
            .unwrap();
        std::fs::write(dir.path().join(".gitignore"), ".env\n").unwrap();
        std::fs::write(dir.path().join(".env"), "# run\n").unwrap();
        let mut app = app_on(dir.path());
        assert!(app.config.find_ignored, "on by default");
        let _ = app.update(Message::OpenFindInFiles);
        listed(&mut app, dir.path());
        assert!(app.finder.files.iter().any(|(p, _)| p.ends_with(".env")));
        let _ = app.update(Message::FindIgnored(false));
        assert!(!app.config.find_ignored);
        assert!(app.finder.loading, "the files are listed again");
    }

    #[test]
    fn a_search_on_several_lines() {
        let dir = project();
        let mut app = app_on(dir.path());
        let _ = app.update(Message::OpenReplaceInFiles);
        listed(&mut app, dir.path());
        let _ = app.update(Message::FindToggleMultiline);
        assert!(app.finder.multiline);
        let _ = app.update(Message::FindLines(text_editor::Action::Edit(
            text_editor::Edit::Paste(std::sync::Arc::new("fn main() {\n    run();".into())),
        )));
        assert_eq!(app.finder.text, "fn main() {\n    run();");
        let _ = app.update(Message::FindReplaceText("start();".into()));
        searched(&mut app);
        assert_eq!(app.finder.hits.len(), 1);
        let _ = app.update(Message::FindReplaceAll);
        let _ = app.update(Message::FindReplaceConfirm(true));
        let text = std::fs::read_to_string(dir.path().join("src/app.rs")).unwrap();
        assert!(text.starts_with("start(); // run it"), "{text}");
        // Back to one line: the first one is kept.
        let _ = app.update(Message::FindToggleMultiline);
        assert_eq!(app.finder.text, "fn main() {");
    }

    #[test]
    fn counts_of_matches_and_files_and_the_history() {
        let hit = |p: &str| Occurrence {
            hit: TextHit {
                path: PathBuf::from(p),
                line: 0,
                col: 0,
                len: 1,
                preview: String::new(),
                preview_at: (0, 0),
            },
            replacement: None,
            excluded: false,
        };
        assert_eq!(counts(&[hit("a"), hit("a"), hit("b")]), (3, 2));
        let mut list = Vec::new();
        for i in 0..30 {
            remember(&mut list, &format!("q{i}"));
        }
        remember(&mut list, "q5");
        assert_eq!(list.len(), HISTORY);
        assert_eq!(list[0], "q5");
    }
}

#[cfg(test)]
mod todo_tests {
    use super::*;

    #[test]
    fn comments_that_call_for_work_are_listed_but_not_strings() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("a.rs"),
            "// TODO: first\nlet s = \"TODO not this\";\nfn f() {} // FIXME later\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("b.py"), "# HACK: ugly\nx = 1\n").unwrap();
        let found = todo_hits(&[dir.path().to_path_buf()], &[], &HashMap::new()).unwrap();
        let mut lines: Vec<(String, usize)> = found
            .iter()
            .map(|o| {
                (
                    o.hit
                        .path
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                    o.hit.line,
                )
            })
            .collect();
        lines.sort();
        assert_eq!(
            lines,
            vec![("a.rs".into(), 0), ("a.rs".into(), 2), ("b.py".into(), 0)]
        );
        // The text open in the editor counts, not the file.
        let mut open = HashMap::new();
        open.insert(dir.path().join("b.py"), "x = 1\n".to_string());
        let found = todo_hits(&[dir.path().to_path_buf()], &[], &open).unwrap();
        assert_eq!(found.len(), 2);
    }
}
