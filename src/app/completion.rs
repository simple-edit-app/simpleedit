//! Code completion in the app: when the language server is asked, the list
//! under the cursor and its keys, accepting an item (with the import it
//! needs), and the places of a snippet that Tab goes through.
//!
//! Nothing is guessed: without a language server there is no list, and the
//! list only holds what the server proposed, kept or dropped by what was
//! typed since it answered.

use super::*;
use crate::completion::{self as cmp, Base, Item, Ranked};
use crate::lsp::{self, Trigger};
use iced::widget::{column, mouse_area, rich_text, span};

/// Rows of the list shown at once.
pub(super) const VISIBLE: usize = 10;
/// Height of a row.
const ROW: f32 = 24.0;
const LIST_WIDTH: f32 = 470.0;
const DOC_WIDTH: f32 = 380.0;
/// Room kept under the list for the status bar.
const BOTTOM_ROOM: f32 = 34.0;
/// How often a server still loading the project is looked at again.
const BUSY_POLL: std::time::Duration = std::time::Duration::from_millis(600);

/// The list being shown (or awaited) for a place of the file.
pub struct Session {
    pub path: PathBuf,
    ext: String,
    /// The text and cursor the items were asked for.
    base: Base,
    pub items: Vec<Item>,
    /// The items kept for what was typed since, best first.
    pub ranked: Vec<Ranked>,
    /// Index in `ranked`.
    pub selected: usize,
    /// First row shown.
    pub top: usize,
    incomplete: bool,
    /// The server is still loading the project.
    pub busy: bool,
    /// Asked with the shortcut (says so when nothing is found).
    manual: bool,
    /// Opened by typing a name: erasing all of it closes the list.
    by_name: bool,
    /// The request the items come from.
    items_of: u64,
    /// Where the list hangs: (line, byte column), and where that is in the
    /// window.
    pub anchor: (usize, usize),
    pub anchor_at: Option<(iced::Point, f32)>,
    /// Items whose documentation was asked for.
    resolving: HashSet<usize>,
}

/// The places of a snippet still to fill, as byte ranges of the text.
pub struct SnippetRun {
    path: PathBuf,
    /// The text they are places of.
    text: String,
    /// The place being filled, then its copies.
    current: Vec<cmp::Stretch>,
    /// The places after it, each with its copies.
    stops: Vec<Vec<cmp::Stretch>>,
}

/// An item accepted before the server said which import it needs.
struct PendingImport {
    token: u64,
    path: PathBuf,
    base: Base,
    item: Item,
    /// Where the item's own text starts, in the asked text: the import goes
    /// before.
    start: usize,
}

/// Which server (language and project) a file is served by.
type ServerKey = (String, PathBuf);

/// What the editor follows for the floating cards: where the list and the
/// help on parameters hang, and the list's area (a click there keeps the
/// focus in the editor).
pub struct Floating {
    pub list: Option<(usize, usize)>,
    pub hint: Option<(usize, usize)>,
    pub area: Option<iced::Rectangle>,
}

/// The help on the parameters of the call being typed.
pub struct Hint {
    path: PathBuf,
    /// Where it hangs (the cursor when it opened) and where that is.
    pub anchor: (usize, usize),
    pub anchor_at: crate::code_editor::Spot,
    pub signatures: cmp::Signatures,
    /// The signature shown: the server's, or one picked among overloads.
    pub shown: usize,
    picked: bool,
}

impl Hint {
    pub fn path(&self) -> &PathBuf {
        &self.path
    }
}

/// A request for the help on parameters, waiting for its answer.
struct HintAsked {
    generation: u64,
    path: PathBuf,
    anchor: (usize, usize),
    manual: bool,
}

/// Width of the help on parameters at most.
const HINT_WIDTH: f32 = 640.0;

#[derive(Default)]
pub struct CompletionState {
    pub session: Option<Session>,
    /// Requests sent; an answer to an older one is dropped.
    generation: u64,
    /// What each server told about its completion.
    info: HashMap<ServerKey, lsp::CompletionInfo>,
    /// Servers already started (or being started) for completion.
    asked: HashSet<ServerKey>,
    /// Whether a server is installed, by file extension.
    installed: HashMap<String, bool>,
    pub snippet: Option<SnippetRun>,
    import: Option<PendingImport>,
    import_token: u64,
    pub hint: Option<Hint>,
    hint_generation: u64,
    hint_asked: Option<HintAsked>,
}

/// Where the list and its documentation go in the window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Geometry {
    pub x: f32,
    pub y: f32,
    pub list_height: f32,
    /// The documentation's left edge, when there is room for it.
    pub doc_x: Option<f32>,
    pub height: f32,
}

impl Geometry {
    fn area(&self) -> iced::Rectangle {
        let left = self.doc_x.map_or(self.x, |d| d.min(self.x));
        let right = self.doc_x.map_or(self.x + LIST_WIDTH, |d| {
            (d + DOC_WIDTH).max(self.x + LIST_WIDTH)
        });
        iced::Rectangle {
            x: left,
            y: self.y,
            width: right - left,
            height: self.height,
        }
    }
}

fn server_key(path: &Path, ext: &str) -> ServerKey {
    let root = crate::definition::project_root(path)
        .or_else(|| path.parent().map(Path::to_path_buf))
        .unwrap_or_default();
    (ext.to_string(), root)
}

/// Which characters of a label the typed ones are, from their places in the
/// filter text (the label may be part of it, or the other way round).
fn label_marks(label: &str, filter: &str, positions: &[usize]) -> Vec<bool> {
    let count = label.chars().count();
    let mut marks = vec![false; count];
    let shift: isize = if let Some(at) = label.find(filter) {
        label[..at].chars().count() as isize
    } else if let Some(at) = filter.find(label) {
        -(filter[..at].chars().count() as isize)
    } else {
        return marks;
    };
    for &p in positions {
        let i = p as isize + shift;
        if i >= 0 && (i as usize) < count {
            marks[i as usize] = true;
        }
    }
    marks
}

fn clip(text: &str, most: usize) -> String {
    if text.chars().count() <= most {
        return text.to_string();
    }
    let mut cut: String = text.chars().take(most.saturating_sub(1)).collect();
    cut.push('…');
    cut
}

impl SimpleEditApp {
    // ── Which file, which server ──

    /// The file completion is for, and its extension: a real file whose
    /// language has a server installed.
    fn completion_target(&mut self) -> Option<(PathBuf, String)> {
        if !self.config.completion_enabled {
            return None;
        }
        let path = self
            .current_file
            .clone()
            .filter(|p| !is_untitled(p) && !is_virtual(p))?;
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())?;
        let installed = *self
            .completion
            .installed
            .entry(ext.clone())
            .or_insert_with(|| lsp::installed(&ext));
        installed.then_some((path, ext))
    }

    /// Whether a server is installed for files of `ext` (asked once).
    pub(super) fn server_installed(&mut self, ext: &str) -> bool {
        *self
            .completion
            .installed
            .entry(ext.to_string())
            .or_insert_with(|| lsp::installed(ext))
    }

    /// Starts the server of the file shown, once per project: it loads the
    /// project in the background before the first list is asked.
    pub(super) fn completion_prepare(&mut self) -> Task<Message> {
        let Some((path, ext)) = self.completion_target() else {
            return Task::none();
        };
        let key = server_key(&path, &ext);
        if !self.completion.asked.insert(key.clone()) {
            return Task::none();
        }
        let text = crate::editor::buffer_text(&self.editor.content);
        Task::perform(
            async move {
                let info = lsp::prepare_completion(&ext, &path, &text).await;
                (key, info)
            },
            |(key, info)| Message::CompletionInfo(key, info),
        )
    }

    /// The servers were stopped: they are prepared again on next use.
    pub(super) fn completion_servers_restarted(&mut self) {
        self.completion.asked.clear();
        self.completion.info.clear();
        self.completion.session = None;
        self.completion.hint = None;
    }

    pub(super) fn completion_info_loaded(
        &mut self,
        key: ServerKey,
        info: Result<lsp::CompletionInfo, String>,
    ) {
        match info {
            Ok(info) => {
                self.completion.info.insert(key, info);
            }
            // Asked again on the next file of that project.
            Err(_) => {
                self.completion.asked.remove(&key);
            }
        }
    }

    fn triggers(&self, path: &Path, ext: &str) -> Vec<String> {
        self.completion
            .info
            .get(&server_key(path, ext))
            .map(|i| i.triggers.clone())
            .unwrap_or_default()
    }

    // ── Asking ──

    /// Asks the server for the list at the cursor.
    pub(super) fn completion_request(&mut self, trigger: Trigger, manual: bool) -> Task<Message> {
        let Some((path, ext)) = self.completion_target() else {
            if manual {
                let ext = self
                    .current_file
                    .as_deref()
                    .and_then(Path::extension)
                    .map(|e| e.to_string_lossy().to_ascii_lowercase())
                    .unwrap_or_default();
                if self.config.completion_enabled && !ext.is_empty() {
                    self.set_status(t!("completion.no_server", ext = ext.as_str()), true);
                }
            }
            return Task::none();
        };
        let text = crate::editor::buffer_text(&self.editor.content);
        let origin = self.editor.content.cursor_position();
        self.completion.generation += 1;
        let generation = self.completion.generation;
        let base = Base {
            text: text.clone(),
            origin,
        };
        let refresh = trigger == Trigger::Incomplete
            && self
                .completion
                .session
                .as_ref()
                .is_some_and(|s| s.path == path);
        if !refresh {
            let anchor = match &trigger {
                Trigger::Character(_) => origin,
                _ => (origin.0, base.word_start()),
            };
            let manual = manual
                || self
                    .completion
                    .session
                    .as_ref()
                    .is_some_and(|s| s.manual && s.path == path);
            let by_name = trigger == Trigger::Invoked && !manual;
            self.completion.session = Some(Session {
                by_name,
                path: path.clone(),
                ext: ext.clone(),
                base: base.clone(),
                items: Vec::new(),
                ranked: Vec::new(),
                selected: 0,
                top: 0,
                incomplete: false,
                busy: false,
                manual,
                items_of: 0,
                anchor,
                anchor_at: None,
                resolving: HashSet::new(),
            });
        }
        Task::perform(
            async move {
                let found = lsp::complete(&ext, &path, &text, origin.0, origin.1, trigger).await;
                (base, found)
            },
            move |(base, found)| Message::CompletionAnswer(generation, Box::new((base, found))),
        )
    }

    /// The server answered: the list, kept for what was typed since.
    pub(super) fn completion_answer(
        &mut self,
        generation: u64,
        base: Base,
        found: Result<lsp::Completions, String>,
    ) -> Task<Message> {
        if generation != self.completion.generation {
            return Task::none();
        }
        let current = self.current_file.clone();
        let Some(session) = self.completion.session.as_mut() else {
            return Task::none();
        };
        if current.as_ref() != Some(&session.path) {
            self.completion.session = None;
            return Task::none();
        }
        let found = match found {
            Ok(found) => found,
            Err(e) => {
                let manual = session.manual;
                self.completion.session = None;
                if manual {
                    self.set_status(format!("{}: {e}", t!("status.error")), true);
                }
                return Task::none();
            }
        };
        session.items = found.items;
        session.base = base;
        session.incomplete = found.incomplete;
        session.busy = found.busy;
        session.items_of = generation;
        session.resolving.clear();
        self.completion_refilter();
        let Some(session) = self.completion.session.as_ref() else {
            return Task::none();
        };
        let busy = session.busy;
        if session.ranked.is_empty() && !busy {
            let manual = session.manual;
            self.completion.session = None;
            if manual {
                self.set_status(t!("completion.none"), false);
            }
            return Task::none();
        }
        let mut tasks = vec![self.completion_resolve_selected()];
        if busy {
            tasks.push(self.completion_poll(generation));
        }
        Task::batch(tasks)
    }

    /// While the server loads the project, it is looked at again; once it is
    /// done, the list is asked again (it may have lacked items).
    fn completion_poll(&self, generation: u64) -> Task<Message> {
        let Some(session) = self.completion.session.as_ref() else {
            return Task::none();
        };
        let (ext, path) = (session.ext.clone(), session.path.clone());
        Task::perform(
            async move {
                tokio::time::sleep(BUSY_POLL).await;
                lsp::completion_busy(&ext, &path).await
            },
            move |busy| Message::CompletionBusy(generation, busy),
        )
    }

    pub(super) fn completion_busy_answer(&mut self, generation: u64, busy: bool) -> Task<Message> {
        let Some(session) = self.completion.session.as_ref() else {
            return Task::none();
        };
        if session.items_of != generation || generation != self.completion.generation {
            return Task::none();
        }
        if busy {
            return self.completion_poll(generation);
        }
        let manual = session.manual;
        // Everything is loaded now: the whole list, for the same word.
        let anchor = session.anchor;
        let caret = self.editor.content.cursor_position();
        if caret.0 != anchor.0 || caret.1 < anchor.1 {
            self.completion.session = None;
            return Task::none();
        }
        let trigger = if session.incomplete {
            Trigger::Incomplete
        } else {
            Trigger::Invoked
        };
        let task = self.completion_request(trigger, manual);
        if let Some(s) = self.completion.session.as_mut() {
            s.anchor = anchor;
        }
        task
    }

    /// Keeps the items that match what was typed since the answer.
    fn completion_refilter(&mut self) {
        let caret = self.editor.content.cursor_position();
        let line = self
            .editor
            .content
            .line(caret.0)
            .map(|l| l.to_string())
            .unwrap_or_default();
        let Some(session) = self.completion.session.as_mut() else {
            return;
        };
        session.ranked = cmp::rank(&session.items, &session.base, &line, caret);
        session.selected = cmp::first_selected(&session.items, &session.ranked);
        session.top = session.selected.saturating_sub(VISIBLE - 1);
    }

    pub(super) fn completion_close(&mut self) {
        self.completion.session = None;
    }

    // ── Typing ──

    /// What an action of the editor does to the list: a name typed asks for
    /// it or narrows it, one of the server's characters asks for a new one,
    /// anything else closes it.
    pub(super) fn completion_after_edit(&mut self, action: &text_editor::Action) -> Task<Message> {
        let list = self.completion_list_after_edit(action);
        let hint = self.hint_after_edit(action);
        Task::batch([list, hint])
    }

    fn completion_list_after_edit(&mut self, action: &text_editor::Action) -> Task<Message> {
        use text_editor::{Action, Edit};
        match action {
            Action::Scroll { .. } => Task::none(),
            Action::Edit(Edit::Insert(c)) => self.completion_typed(*c),
            Action::Edit(Edit::Backspace) => self.completion_erased(),
            _ => {
                self.completion_close();
                Task::none()
            }
        }
    }

    fn completion_typed(&mut self, c: char) -> Task<Message> {
        let Some((path, ext)) = self.completion_target() else {
            self.completion_close();
            return Task::none();
        };
        let (line, col) = self.editor.content.cursor_position();
        let before = self
            .editor
            .content
            .line(line)
            .map(|l| l[..col.min(l.len())].to_string())
            .unwrap_or_default();
        // One of the server's characters (`.`, `::`, `"`, …) just typed.
        let trigger = self
            .triggers(&path, &ext)
            .into_iter()
            .filter(|t| !t.is_empty() && t.ends_with(c) && before.ends_with(t.as_str()))
            .max_by_key(String::len);
        if let Some(trigger) = trigger {
            if self.config.completion_auto || self.completion.session.is_some() {
                return self.completion_request(Trigger::Character(trigger), false);
            }
            return Task::none();
        }
        if !cmp::is_word_char(c) {
            self.completion_close();
            return Task::none();
        }
        if let Some(session) = self.completion.session.as_ref().filter(|s| s.path == path) {
            let incomplete = session.incomplete;
            self.completion_refilter();
            let resolve = self.completion_resolve_selected();
            if incomplete {
                return Task::batch([resolve, self.completion_request(Trigger::Incomplete, false)]);
            }
            if self
                .completion
                .session
                .as_ref()
                .is_some_and(|s| s.ranked.is_empty() && !s.busy && s.items_of != 0)
            {
                self.completion_close();
            }
            return resolve;
        }
        if !self.config.completion_auto {
            return Task::none();
        }
        // A number is no name.
        let word = &before[cmp::word_start_col(&before, before.len())..];
        if word.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            return Task::none();
        }
        self.completion_request(Trigger::Invoked, false)
    }

    fn completion_erased(&mut self) -> Task<Message> {
        let caret = self.editor.content.cursor_position();
        let Some(session) = self.completion.session.as_ref() else {
            return Task::none();
        };
        let emptied = session.by_name && caret.1 == session.anchor.1;
        if caret.0 != session.anchor.0 || caret.1 < session.anchor.1 || emptied {
            self.completion_close();
            return Task::none();
        }
        let incomplete = session.incomplete;
        self.completion_refilter();
        if incomplete {
            return self.completion_request(Trigger::Incomplete, false);
        }
        if self
            .completion
            .session
            .as_ref()
            .is_some_and(|s| s.ranked.is_empty() && !s.busy && s.items_of != 0)
        {
            self.completion_close();
        }
        Task::none()
    }

    // ── The list's keys ──

    /// Whether the list is on screen: its keys are its own then.
    pub(super) fn completion_visible(&self) -> bool {
        self.completion.session.as_ref().is_some_and(|s| {
            self.current_file.as_ref() == Some(&s.path)
                && (!s.ranked.is_empty() || (s.busy && s.manual))
        })
    }

    /// A key while the list is shown: the arrows and pages move in it, Enter
    /// puts the selected item in (Tab, replacing the rest of the word).
    pub(super) fn completion_key(&mut self, key: &str) -> Option<Task<Message>> {
        let len = self.completion.session.as_ref()?.ranked.len();
        let task = match key {
            "ArrowDown" => self.completion_move(1, true),
            "ArrowUp" => self.completion_move(-1, true),
            "PageDown" => self.completion_move(VISIBLE as isize, false),
            "PageUp" => self.completion_move(-(VISIBLE as isize), false),
            "Return" if len > 0 => {
                let selected = self.completion.session.as_ref()?.selected;
                self.completion_accept(selected, false)
            }
            "Tab" if len > 0 => {
                let selected = self.completion.session.as_ref()?.selected;
                self.completion_accept(selected, true)
            }
            _ => return None,
        };
        Some(task)
    }

    /// Moves the selection; the arrows go round, the pages stop at the ends.
    fn completion_move(&mut self, delta: isize, wrap: bool) -> Task<Message> {
        let Some(session) = self.completion.session.as_mut() else {
            return Task::none();
        };
        let len = session.ranked.len() as isize;
        if len == 0 {
            return Task::none();
        }
        let next = session.selected as isize + delta;
        session.selected = if wrap {
            next.rem_euclid(len) as usize
        } else {
            next.clamp(0, len - 1) as usize
        };
        if session.selected < session.top {
            session.top = session.selected;
        } else if session.selected >= session.top + VISIBLE {
            session.top = session.selected + 1 - VISIBLE;
        }
        self.completion_resolve_selected()
    }

    /// The wheel over the list scrolls it.
    pub(super) fn completion_wheel(&mut self, lines: f32) {
        if let Some(session) = self.completion.session.as_mut() {
            let most = session.ranked.len().saturating_sub(VISIBLE) as isize;
            let top = session.top as isize - lines.round() as isize;
            session.top = top.clamp(0, most.max(0)) as usize;
        }
    }

    /// Asks the server for the documentation of the selected item.
    fn completion_resolve_selected(&mut self) -> Task<Message> {
        let Some(session) = self.completion.session.as_mut() else {
            return Task::none();
        };
        let Some(index) = session.ranked.get(session.selected).map(|r| r.index) else {
            return Task::none();
        };
        let item = &session.items[index];
        if !item.can_resolve() || !session.resolving.insert(index) {
            return Task::none();
        }
        let (ext, path, raw, of) = (
            session.ext.clone(),
            session.path.clone(),
            item.raw.clone(),
            session.items_of,
        );
        Task::perform(
            async move { lsp::resolve_completion(&ext, &path, raw).await },
            move |resolved| Message::CompletionResolved(of, index, resolved),
        )
    }

    pub(super) fn completion_resolved(
        &mut self,
        items_of: u64,
        index: usize,
        resolved: Result<serde_json::Value, String>,
    ) {
        let Some(session) = self.completion.session.as_mut() else {
            return;
        };
        if session.items_of != items_of {
            return;
        }
        if let (Ok(value), Some(item)) = (resolved, session.items.get_mut(index)) {
            cmp::merge_resolved(item, &value, &session.base.text);
        }
    }

    // ── Accepting ──

    /// What a snippet's `$TM_FILENAME` and the like are.
    fn snippet_variable(&self, name: &str) -> Option<String> {
        let path = self.current_file.as_deref()?;
        let (line, _) = self.editor.content.cursor_position();
        Some(match name {
            "TM_FILENAME" => path.file_name()?.to_string_lossy().into_owned(),
            "TM_FILENAME_BASE" => path.file_stem()?.to_string_lossy().into_owned(),
            "TM_DIRECTORY" => path.parent()?.to_string_lossy().into_owned(),
            "TM_FILEPATH" => path.to_string_lossy().into_owned(),
            "TM_LINE_INDEX" => line.to_string(),
            "TM_LINE_NUMBER" => (line + 1).to_string(),
            "TM_CURRENT_LINE" => self.editor.content.line(line)?.to_string(),
            "TM_SELECTED_TEXT" => String::new(),
            _ => return None,
        })
    }

    /// Puts the item at `selected` (in the ranked list) in: Enter keeps what
    /// follows the cursor, Tab (`replace`) replaces the rest of the word.
    pub(super) fn completion_accept(&mut self, selected: usize, replace: bool) -> Task<Message> {
        let Some(session) = self.completion.session.take() else {
            return Task::none();
        };
        let Some(ranked) = session.ranked.get(selected) else {
            return Task::none();
        };
        let item = session.items[ranked.index].clone();
        let now = crate::editor::buffer_text(&self.editor.content);
        let caret = self.editor.content.cursor_position();
        let variable = |name: &str| self.snippet_variable(name);
        let Some(plan) = cmp::plan(&item, &session.base, &now, caret, replace, &variable) else {
            return Task::none();
        };
        let start = cmp::offset_of(&plan.text, plan.caret);
        let selection = (plan.select > 0).then(|| Span {
            start: plan.caret,
            end: cmp::pos_of_offset(&plan.text, start + plan.select),
        });
        let mut tasks =
            vec![self.apply_text_edit(plan.text.clone(), plan.caret.0, plan.caret.1, selection)];
        self.completion.snippet =
            (!plan.stops.is_empty() || !plan.copies.is_empty()).then(|| SnippetRun {
                path: session.path.clone(),
                text: crate::editor::buffer_text(&self.editor.content),
                current: std::iter::once((start, start + plan.select))
                    .chain(plan.copies.iter().copied())
                    .collect(),
                stops: plan.stops.clone(),
            });
        // The import some servers only tell once the item is chosen.
        if item.can_resolve() && item.additional.is_empty() {
            self.completion.import_token += 1;
            let token = self.completion.import_token;
            let item_start = item
                .insert
                .or(item.replace)
                .map_or((session.base.origin.0, session.base.word_start()), |r| {
                    r.start
                });
            self.completion.import = Some(PendingImport {
                token,
                path: session.path.clone(),
                start: cmp::offset_of(&session.base.text, item_start),
                base: session.base.clone(),
                item: item.clone(),
            });
            let (ext, path, raw) = (session.ext.clone(), session.path.clone(), item.raw.clone());
            tasks.push(Task::perform(
                async move { lsp::resolve_completion(&ext, &path, raw).await },
                move |resolved| Message::CompletionImport(token, resolved),
            ));
        }
        // Some items ask for the list again (a module path, a keyword).
        match item.command.as_deref() {
            Some("editor.action.triggerSuggest") => {
                tasks.push(self.completion_request(Trigger::Invoked, false));
            }
            Some("editor.action.triggerParameterHints") => {
                tasks.push(self.hint_request(1, None, false));
            }
            _ => {}
        }
        Task::batch(tasks)
    }

    /// The server told which import an accepted item needs: it goes in,
    /// above the item, in the same undo step.
    pub(super) fn completion_import(
        &mut self,
        token: u64,
        resolved: Result<serde_json::Value, String>,
    ) -> Task<Message> {
        let Some(pending) = self.completion.import.take() else {
            return Task::none();
        };
        let (Ok(value), true) = (resolved, pending.token == token) else {
            return Task::none();
        };
        if self.current_file.as_ref() != Some(&pending.path) {
            return Task::none();
        }
        let mut item = pending.item;
        cmp::merge_resolved(&mut item, &value, &pending.base.text);
        let base = &pending.base.text;
        // Only edits before the item, in text that has not changed since.
        let mut edits: Vec<(usize, usize, String)> = item
            .additional
            .iter()
            .map(|e| {
                (
                    cmp::offset_of(base, e.range.start),
                    cmp::offset_of(base, e.range.end),
                    e.text.clone(),
                )
            })
            .filter(|(from, to, _)| from <= to && *to <= pending.start)
            .collect();
        let now = crate::editor::buffer_text(&self.editor.content);
        let reach = edits.iter().map(|e| e.1).max().unwrap_or(0);
        if edits.is_empty() || now.get(..reach) != base.get(..reach) {
            return Task::none();
        }
        edits.sort_by_key(|e| std::cmp::Reverse(e.0));
        let mut text = now.clone();
        let mut delta: isize = 0;
        for (from, to, new) in &edits {
            text.replace_range(*from..*to, new);
            delta += new.len() as isize - (*to - *from) as isize;
        }
        let shift = |at: usize| (at as isize + delta).max(0) as usize;
        // The cursor (and selection) keep their place on the text.
        let caret = self.editor.content.cursor_position();
        let caret_at = shift(cmp::offset_of(&now, caret));
        let snippet = self.completion.snippet.as_ref().filter(|s| s.text == now);
        let selection = snippet
            .and_then(|s| s.current.first())
            .map(|c| (shift(c.0), shift(c.1)));
        let undo_before = self.undo_stack.len();
        let pos = cmp::pos_of_offset(&text, caret_at);
        let span = selection.filter(|(a, b)| a < b).map(|(a, b)| Span {
            start: cmp::pos_of_offset(&text, a),
            end: cmp::pos_of_offset(&text, b),
        });
        let (line, col) = span.map_or(pos, |s| s.start);
        let task = self.apply_text_edit(text.clone(), line, col, span);
        // One undo step with the item.
        if self.undo_stack.len() > undo_before {
            self.undo_stack.pop_back();
        }
        if let Some(run) = self.completion.snippet.as_mut().filter(|s| s.text == now) {
            for stop in run.current.iter_mut().chain(run.stops.iter_mut().flatten()) {
                *stop = (shift(stop.0), shift(stop.1));
            }
            run.text = text;
        }
        task
    }

    // ── Snippet places ──

    /// After an edit, the places of a snippet follow the text; an edit
    /// outside the current place ends the snippet.
    pub(super) fn snippet_follow(&mut self) {
        let Some(run) = self.completion.snippet.as_ref() else {
            return;
        };
        if self.current_file.as_ref() != Some(&run.path) {
            self.completion.snippet = None;
            return;
        }
        let now = crate::editor::buffer_text(&self.editor.content);
        if now == run.text {
            return;
        }
        let Some(followed) = cmp::follow_edit(&run.current, &run.stops, &run.text, &now) else {
            self.completion.snippet = None;
            return;
        };
        if followed.text != now {
            // The copies get what was typed, in the same undo step as the
            // typing; the cursor keeps its place in the place it is in.
            let caret = cmp::offset_of(&now, self.editor.content.cursor_position());
            let place = followed.current[0];
            let within = caret.saturating_sub(followed.typed.0);
            crate::editor::replace_text(&mut self.editor.content, &now, &followed.text);
            let (line, col) = cmp::pos_of_offset(&followed.text, (place.0 + within).min(place.1));
            self.editor.content.select_bytes(line, col, 0);
        }
        let run = self.completion.snippet.as_mut().unwrap();
        run.stops = followed.stops;
        run.current = followed.current;
        run.text = followed.text;
    }

    /// Tab in a snippet: the next place, selected. False when there is none.
    pub(super) fn snippet_next(&mut self) -> bool {
        let Some(run) = self.completion.snippet.as_mut() else {
            return false;
        };
        if run.stops.is_empty() {
            self.completion.snippet = None;
            return false;
        }
        let group = run.stops.remove(0);
        let (start, end) = group[0];
        let copies = group.len() > 1;
        run.current = group;
        let text = run.text.clone();
        // The last place ends the snippet, unless it has copies to follow.
        if run.stops.is_empty() && !copies {
            self.completion.snippet = None;
        }
        let (line, col) = cmp::pos_of_offset(&text, start);
        self.editor.content.select_bytes(line, col, end - start);
        true
    }

    // ── On screen ──

    /// Where the list goes: under its anchor, or above when there is no room
    /// below; the documentation beside it when there is room.
    pub(super) fn completion_geometry(&self) -> Option<Geometry> {
        if !self.completion_visible() {
            return None;
        }
        let session = self.completion.session.as_ref()?;
        let (at, row) = session.anchor_at?;
        let rows = session.ranked.len().clamp(1, VISIBLE) as f32;
        let footer = if session.busy { ROW } else { 0.0 };
        let list_height = rows * ROW + 8.0 + footer;
        let has_doc = session
            .ranked
            .get(session.selected)
            .map(|r| &session.items[r.index])
            .is_some_and(|i| i.doc.is_some() || i.detail.is_some());
        let height = if has_doc {
            list_height.max(ROW * 7.0)
        } else {
            list_height
        };
        let window_w = self.config.window_width;
        let window_h = self.config.window_height;
        // The labels start under the word: the icon is to its left.
        let mut x = (at.x - 30.0).clamp(4.0, (window_w - LIST_WIDTH - 4.0).max(4.0));
        let below = at.y + 2.0;
        let y = if below + height > window_h - BOTTOM_ROOM && at.y - row - height - 2.0 > 0.0 {
            at.y - row - height - 2.0
        } else {
            below
        };
        let both = LIST_WIDTH + 2.0 + DOC_WIDTH;
        let doc_x = if !has_doc {
            None
        } else if x + both <= window_w - 4.0 {
            Some(x + LIST_WIDTH + 2.0)
        } else if x - DOC_WIDTH - 2.0 >= 4.0 {
            Some(x - DOC_WIDTH - 2.0)
        } else if window_w - 4.0 - both >= 4.0 {
            // Neither side has room: the list moves left to make some.
            x = window_w - 4.0 - both;
            Some(x + LIST_WIDTH + 2.0)
        } else {
            None
        };
        Some(Geometry {
            x,
            y,
            list_height,
            doc_x,
            height,
        })
    }

    /// What the editor follows while the list or the help on parameters is
    /// open: where they hang, and the list's area.
    pub(super) fn completion_hook(&self) -> Option<Floating> {
        let list = self
            .completion_visible()
            .then(|| self.completion.session.as_ref().map(|s| s.anchor))
            .flatten();
        let hint = self
            .completion
            .hint
            .as_ref()
            .filter(|h| self.current_file.as_ref() == Some(&h.path))
            .map(|h| h.anchor);
        if list.is_none() && hint.is_none() {
            return None;
        }
        Some(Floating {
            list,
            hint,
            area: self.completion_geometry().map(|g| g.area()),
        })
    }

    // ── The parameters of a call ──

    /// Asks the server for the parameters of the call at the cursor: `kind`
    /// 1 when asked, 2 for a character typed, 3 when the text or the cursor
    /// moved while the help is open.
    pub(super) fn hint_request(
        &mut self,
        kind: u8,
        character: Option<String>,
        manual: bool,
    ) -> Task<Message> {
        let Some((path, ext)) = self.completion_target() else {
            if manual && self.config.completion_enabled {
                let ext = self
                    .current_file
                    .as_deref()
                    .and_then(Path::extension)
                    .map(|e| e.to_string_lossy().to_ascii_lowercase())
                    .unwrap_or_default();
                if !ext.is_empty() {
                    self.set_status(t!("completion.no_server", ext = ext.as_str()), true);
                }
            }
            return Task::none();
        };
        let text = crate::editor::buffer_text(&self.editor.content);
        let caret = self.editor.content.cursor_position();
        let open = self.completion.hint.as_ref().filter(|h| h.path == path);
        // The help stays where it opened while the cursor is on that line.
        let anchor = open
            .map(|h| h.anchor)
            .filter(|a| a.0 == caret.0)
            .unwrap_or(caret);
        let open = open.map(|h| h.signatures.raw.clone());
        self.completion.hint_generation += 1;
        let generation = self.completion.hint_generation;
        self.completion.hint_asked = Some(HintAsked {
            generation,
            path: path.clone(),
            anchor,
            manual,
        });
        let trigger = lsp::SignatureTrigger {
            kind,
            character,
            open,
        };
        Task::perform(
            async move { lsp::signature_help(&ext, &path, &text, caret.0, caret.1, trigger).await },
            move |answer| Message::HintAnswer(generation, answer),
        )
    }

    pub(super) fn hint_answer(
        &mut self,
        generation: u64,
        answer: Result<Option<serde_json::Value>, String>,
    ) -> Task<Message> {
        let Some(asked) = self
            .completion
            .hint_asked
            .take_if(|a| a.generation == generation)
        else {
            return Task::none();
        };
        if self.current_file.as_ref() != Some(&asked.path) {
            self.completion.hint = None;
            return Task::none();
        }
        let signatures = match answer {
            Ok(value) => value.as_ref().and_then(cmp::parse_signatures),
            Err(e) => {
                if asked.manual {
                    self.set_status(format!("{}: {e}", t!("status.error")), true);
                }
                None
            }
        };
        let Some(signatures) = signatures else {
            self.completion.hint = None;
            if asked.manual {
                self.set_status(t!("hint.none"), false);
            }
            return Task::none();
        };
        // An overload picked by hand stays while the list is the same.
        let before = self.completion.hint.take();
        let (shown, picked) = match &before {
            Some(h) if h.picked && h.signatures.list.len() == signatures.list.len() => {
                (h.shown, true)
            }
            _ => (signatures.active, false),
        };
        let anchor_at = before
            .filter(|h| h.anchor == asked.anchor)
            .and_then(|h| h.anchor_at);
        self.completion.hint = Some(Hint {
            path: asked.path,
            anchor: asked.anchor,
            anchor_at,
            signatures,
            shown,
            picked,
        });
        Task::none()
    }

    /// The help on parameters after an action of the editor: `(` or `,`
    /// (the server's characters) open it, any change while it is open asks
    /// again (the parameter being typed may change, or the call end).
    fn hint_after_edit(&mut self, action: &text_editor::Action) -> Task<Message> {
        use text_editor::{Action, Edit};
        let Some(path) = self.current_file.clone() else {
            return Task::none();
        };
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        let info = self
            .completion
            .info
            .get(&server_key(&path, &ext))
            .cloned()
            .unwrap_or_default();
        let open = self
            .completion
            .hint
            .as_ref()
            .is_some_and(|h| h.path == path);
        match action {
            Action::Scroll { .. } => Task::none(),
            Action::Edit(Edit::Insert(c)) => {
                let c = c.to_string();
                if info.signature_triggers.contains(&c)
                    || (open && info.signature_retriggers.contains(&c))
                {
                    self.hint_request(2, Some(c), false)
                } else if open {
                    self.hint_request(3, None, false)
                } else {
                    Task::none()
                }
            }
            _ if open => self.hint_request(3, None, false),
            _ => Task::none(),
        }
    }

    /// Alt+↑ / Alt+↓: the previous or next signature of an overloaded
    /// function. False when there is no other.
    pub(super) fn hint_cycle(&mut self, delta: isize) -> bool {
        let Some(hint) = self.completion.hint.as_mut() else {
            return false;
        };
        let len = hint.signatures.list.len() as isize;
        if len < 2 {
            return false;
        }
        hint.shown = (hint.shown as isize + delta).rem_euclid(len) as usize;
        hint.picked = true;
        true
    }

    /// The help on parameters, and where it goes: above the line it opened
    /// on (its bottom there), or under the line (and the list) when there is
    /// no room above.
    pub(super) fn hint_view(&self, dark: bool) -> Option<(iced::Point, Element<'_, Message>)> {
        let hint = self.completion.hint.as_ref()?;
        if self.current_file.as_ref() != Some(&hint.path) {
            return None;
        }
        let (at, row) = hint.anchor_at?;
        let p = se_theme::palette(dark);
        let signature = hint.signatures.list.get(hint.shown)?;
        let param = hint.signatures.param_of(hint.shown);
        let label = &signature.label;
        let (from, to) = param
            .and_then(|i| signature.params.get(i).copied())
            .unwrap_or((label.len(), label.len()));
        let piece = |a: usize, b: usize| label.get(a..b).unwrap_or("").to_string();
        let spans: Vec<iced::widget::text::Span<'_, ()>> = vec![
            span(piece(0, from)).color(p.text).font(se_theme::MONO),
            span(piece(from, to)).color(p.accent).font(iced::Font {
                weight: iced::font::Weight::Medium,
                ..se_theme::MONO
            }),
            span(piece(to, label.len()))
                .color(p.text)
                .font(se_theme::MONO),
        ];
        let mut head = row![rich_text(spans).size(12.5)]
            .spacing(10)
            .align_y(Alignment::Center);
        let count = hint.signatures.list.len();
        if count > 1 {
            head = head.push(
                text(format!("{}/{count}  Alt+↑↓", hint.shown + 1))
                    .size(11)
                    .color(p.muted)
                    .wrapping(iced::widget::text::Wrapping::None),
            );
        }
        let mut body = column![head].spacing(6);
        let doc_text = |doc: &cmp::Doc| -> Element<'_, Message> {
            if doc.markdown {
                let blocks = crate::preview::markdown::parse(&doc.text);
                column(
                    blocks
                        .iter()
                        .take(3)
                        .map(|b| crate::preview::markdown::block_view(b, dark, None)),
                )
                .spacing(6)
                .into()
            } else {
                text(clip(&doc.text, 400))
                    .size(12)
                    .color(p.muted)
                    .shaping(iced::widget::text::Shaping::Advanced)
                    .into()
            }
        };
        if let Some(doc) = param
            .and_then(|i| signature.param_docs.get(i))
            .and_then(Option::as_ref)
        {
            body = body.push(doc_text(doc));
        }
        if let Some(doc) = &signature.doc {
            body = body.push(doc_text(doc));
        }
        let bubble = container(body)
            .padding([6, 10])
            .max_width(HINT_WIDTH)
            .style(se_theme::card(dark));
        let x = (at.x - 12.0).clamp(4.0, (self.config.window_width - HINT_WIDTH - 4.0).max(4.0));
        let above = at.y - row - 2.0;
        if above >= 90.0 {
            Some((
                iced::Point::new(x, 0.0),
                container(bubble)
                    .height(Length::Fixed(above))
                    .align_y(iced::alignment::Vertical::Bottom)
                    .into(),
            ))
        } else {
            let below = self
                .completion_geometry()
                .map_or(at.y + 2.0, |g| g.y + g.height + 2.0);
            Some((iced::Point::new(x, below), bubble.into()))
        }
    }

    /// The list (and the documentation of its selected item), and where it
    /// goes in the window.
    pub(super) fn completion_view(
        &self,
        dark: bool,
    ) -> Option<(iced::Point, Element<'_, Message>)> {
        let geometry = self.completion_geometry()?;
        let session = self.completion.session.as_ref()?;
        let p = se_theme::palette(dark);
        let mut rows = column![].spacing(0);
        let end = (session.top + VISIBLE).min(session.ranked.len());
        for (pos, ranked) in session.ranked[session.top..end].iter().enumerate() {
            let pos = session.top + pos;
            let item = &session.items[ranked.index];
            let selected = pos == session.selected;
            let (glyph, (r, g, b)) = item
                .kind
                .map(cmp::Kind::icon)
                .unwrap_or((' ', (0x9a, 0xa0, 0xad)));
            let marks = label_marks(&item.label, &item.filter_text, &ranked.score.positions);
            let base_color = if item.deprecated { p.muted } else { p.text };
            let mut spans = Vec::new();
            let chars: Vec<char> = item.label.chars().collect();
            let mut i = 0;
            while i < chars.len() {
                let marked = marks.get(i).copied().unwrap_or(false);
                let run_end = (i..chars.len())
                    .find(|&j| marks.get(j).copied().unwrap_or(false) != marked)
                    .unwrap_or(chars.len());
                let piece: String = chars[i..run_end].iter().collect();
                // The code's font; the typed letters in its medium weight.
                let font = if marked {
                    iced::Font {
                        weight: iced::font::Weight::Medium,
                        ..se_theme::MONO
                    }
                } else {
                    se_theme::MONO
                };
                let s: iced::widget::text::Span<'_, ()> = span(piece)
                    .color(if marked { p.accent } else { base_color })
                    .font(font);
                spans.push(s);
                i = run_end;
            }
            let mut line = row![
                container(
                    text(glyph)
                        .font(iced_fonts::NERD_FONT)
                        .size(14)
                        .color(iced::Color::from_rgb8(r, g, b)),
                )
                .width(18)
                .center_x(18),
                rich_text(spans)
                    .size(12.5)
                    .font(se_theme::MONO)
                    .wrapping(iced::widget::text::Wrapping::None),
            ]
            .spacing(6)
            .align_y(Alignment::Center);
            if let Some(detail) = &item.label_detail {
                line = line.push(
                    text(clip(detail, 40))
                        .size(11.5)
                        .color(p.muted)
                        .wrapping(iced::widget::text::Wrapping::None),
                );
            }
            line = line.push(Space::new().width(Length::Fill));
            if let Some(right) = item.description.as_ref().or(item.detail.as_ref()) {
                line = line.push(
                    text(clip(right.lines().next().unwrap_or(""), 38))
                        .size(11.5)
                        .color(p.muted)
                        .wrapping(iced::widget::text::Wrapping::None),
                );
            }
            let accent_soft = p.accent_soft;
            let row = container(line)
                .padding([0, 8])
                .height(ROW)
                .width(Length::Fill)
                .clip(true)
                .center_y(ROW)
                .style(move |_: &iced::Theme| container::Style {
                    background: selected.then_some(iced::Background::Color(accent_soft)),
                    ..container::Style::default()
                });
            rows = rows.push(
                mouse_area(row)
                    .on_press(Message::CompletionPick(pos))
                    .interaction(iced::mouse::Interaction::Pointer),
            );
        }
        if session.ranked.is_empty() {
            rows = rows.push(
                container(
                    text(t!("completion.loading").to_string())
                        .size(12)
                        .color(p.muted),
                )
                .padding([0, 10])
                .center_y(ROW),
            );
        }
        if session.busy {
            rows = rows.push(
                container(
                    text(t!("completion.indexing").to_string())
                        .size(11)
                        .color(p.muted)
                        .wrapping(iced::widget::text::Wrapping::None),
                )
                .padding([0, 10])
                .clip(true)
                .center_y(ROW),
            );
        }
        let list = mouse_area(
            container(rows)
                .padding(4)
                .width(LIST_WIDTH)
                .height(geometry.list_height)
                .style(se_theme::card(dark)),
        )
        .on_scroll(|delta| {
            Message::CompletionWheel(match delta {
                iced::mouse::ScrollDelta::Lines { y, .. } => y,
                iced::mouse::ScrollDelta::Pixels { y, .. } => y / ROW,
            })
        });

        let doc: Option<Element<Message>> = geometry.doc_x.and_then(|_| {
            let item = &session.items[session.ranked.get(session.selected)?.index];
            let mut body = column![].spacing(8);
            if let Some(detail) = &item.detail {
                body = body.push(
                    text(detail.clone())
                        .size(12)
                        .font(se_theme::MONO)
                        .color(p.text)
                        .shaping(iced::widget::text::Shaping::Advanced),
                );
            }
            if let Some(doc) = &item.doc {
                if item.detail.is_some() {
                    body = body.push(
                        container(Space::new().height(1))
                            .width(Length::Fill)
                            .style(se_theme::rule(dark)),
                    );
                }
                if doc.markdown {
                    for block in crate::preview::markdown::parse(&doc.text) {
                        body = body.push(crate::preview::markdown::block_view(&block, dark, None));
                    }
                } else {
                    body = body.push(
                        text(doc.text.clone())
                            .size(12.5)
                            .shaping(iced::widget::text::Shaping::Advanced),
                    );
                }
            }
            Some(
                container(
                    scrollable(container(body).padding(10).width(Length::Fill))
                        .style(se_theme::Scrollbar { dark }.style()),
                )
                .width(DOC_WIDTH)
                .height(geometry.height)
                .style(se_theme::card(dark))
                .into(),
            )
        });
        let (content, x): (Element<Message>, f32) = match (doc, geometry.doc_x) {
            (Some(doc), Some(doc_x)) if doc_x > geometry.x => {
                (row![list, Space::new().width(2), doc].into(), geometry.x)
            }
            (Some(doc), Some(doc_x)) => (row![doc, Space::new().width(2), list].into(), doc_x),
            _ => (list.into(), geometry.x),
        };
        Some((iced::Point::new(x, geometry.y), content))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::completion::Range;
    use serde_json::json;
    use text_editor::{Action, Edit};

    /// An app with `text` open in `name` (a file of a temporary folder),
    /// its language served.
    fn app_with(text: &str, name: &str) -> (SimpleEditApp, PathBuf, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(name);
        std::fs::write(&path, text).unwrap();
        let mut app = SimpleEditApp::blank(Config::default());
        let _ = app.restore_session(crate::session::Session::default());
        let _ = app.update(Message::TabRead(Ok(Loaded {
            path: path.clone(),
            text: text.to_string(),
            format: TextFormat::default(),
            stamp: None,
        })));
        let ext = path.extension().unwrap().to_string_lossy().into_owned();
        app.completion.installed.insert(ext, true);
        (app, path, dir)
    }

    fn type_text(app: &mut SimpleEditApp, text: &str) {
        for c in text.chars() {
            let _ = app.update(Message::EditorAction(Action::Edit(Edit::Insert(c))));
        }
    }

    fn key(app: &mut SimpleEditApp, key: &str) {
        let _ = app.update(Message::KeyPressed {
            key: key.to_string(),
            ctrl: false,
            shift: false,
            alt: false,
        });
    }

    fn text_of(app: &SimpleEditApp) -> String {
        crate::editor::buffer_text(&app.editor.content)
    }

    fn item(label: &str) -> Item {
        let (mut items, _) = cmp::parse_list(&json!([{ "label": label }]), "");
        items.remove(0)
    }

    /// The server's answer to the last request, about the text it was asked
    /// about (`asked`, the cursor at `origin`).
    fn answer(
        app: &mut SimpleEditApp,
        asked: &str,
        origin: (usize, usize),
        items: Vec<Item>,
        incomplete: bool,
        busy: bool,
    ) {
        let generation = app.completion.generation;
        let base = Base {
            text: asked.to_string(),
            origin,
        };
        let _ = app.update(Message::CompletionAnswer(
            generation,
            Box::new((
                base,
                Ok(lsp::Completions {
                    items,
                    incomplete,
                    busy,
                }),
            )),
        ));
    }

    fn shown(app: &SimpleEditApp) -> Vec<String> {
        let session = app.completion.session.as_ref().unwrap();
        session
            .ranked
            .iter()
            .map(|r| session.items[r.index].label.clone())
            .collect()
    }

    #[test]
    fn a_name_typed_asks_and_the_list_narrows_as_one_types() {
        let (mut app, _path, _dir) = app_with("fn main() {\n    \n}\n", "a.rs");
        app.editor.content.select_bytes(1, 4, 0);
        type_text(&mut app, "p");
        assert_eq!(app.completion.generation, 1, "asked");
        assert!(!app.completion_visible(), "nothing to show yet");
        // Typed while the server answered: the answer is narrowed to it.
        type_text(&mut app, "r");
        assert_eq!(
            app.completion.generation, 1,
            "a complete list is not asked again"
        );
        let items = vec![item("println"), item("print"), item("panic"), item("Vec")];
        answer(
            &mut app,
            "fn main() {\n    p\n}\n",
            (1, 5),
            items,
            false,
            false,
        );
        assert!(app.completion_visible());
        assert_eq!(shown(&app), vec!["print", "println"]);
        type_text(&mut app, "intl");
        assert_eq!(shown(&app), vec!["println"]);
        // Enter puts it in.
        key(&mut app, "Return");
        assert_eq!(text_of(&app), "fn main() {\n    println\n}\n");
        assert!(app.completion.session.is_none());
        assert_eq!(app.editor.content.cursor_position(), (1, 11));
    }

    #[test]
    fn arrows_choose_and_tab_replaces_the_rest_of_the_word() {
        let (mut app, _path, _dir) = app_with("x\n", "a.rs");
        app.editor.content.select_bytes(0, 0, 0);
        type_text(&mut app, "f");
        let mut a = item("foo");
        let mut b = item("format");
        // Ranges as servers give them: the word typed, and the whole word.
        for it in [&mut a, &mut b] {
            it.insert = Some(Range {
                start: (0, 0),
                end: (0, 1),
            });
            it.replace = Some(Range {
                start: (0, 0),
                end: (0, 2),
            });
        }
        answer(&mut app, "fx\n", (0, 1), vec![a, b], false, false);
        key(&mut app, "ArrowDown");
        assert_eq!(app.completion.session.as_ref().unwrap().selected, 1);
        key(&mut app, "ArrowDown");
        assert_eq!(
            app.completion.session.as_ref().unwrap().selected,
            0,
            "it goes round"
        );
        key(&mut app, "ArrowUp");
        key(&mut app, "Tab");
        assert_eq!(
            text_of(&app),
            "format\n",
            "the x after the cursor is replaced"
        );
    }

    #[test]
    fn an_old_answer_is_dropped_and_an_incomplete_list_is_asked_again() {
        let (mut app, _path, _dir) = app_with("\n", "a.rs");
        type_text(&mut app, "a");
        answer(
            &mut app,
            "a\n",
            (0, 1),
            vec![item("abc"), item("abd")],
            true,
            false,
        );
        type_text(&mut app, "b");
        assert_eq!(app.completion.generation, 2, "asked again while typing");
        // The answer to the first request comes late: dropped.
        let base = Base {
            text: "a\n".into(),
            origin: (0, 1),
        };
        let _ = app.update(Message::CompletionAnswer(
            1,
            Box::new((
                base,
                Ok(lsp::Completions {
                    items: vec![item("zzz")],
                    incomplete: false,
                    busy: false,
                }),
            )),
        ));
        assert_eq!(
            shown(&app),
            vec!["abc", "abd"],
            "the list in place is kept, narrowed"
        );
        answer(
            &mut app,
            "ab\n",
            (0, 2),
            vec![item("abd"), item("abx")],
            true,
            false,
        );
        assert_eq!(shown(&app), vec!["abd", "abx"]);
    }

    #[test]
    fn a_trigger_character_asks_and_other_characters_close() {
        let (mut app, path, _dir) = app_with("\n", "a.rs");
        app.completion.info.insert(
            server_key(&path, "rs"),
            lsp::CompletionInfo {
                supported: true,
                triggers: vec![".".into(), "::".into()],
                busy: false,
                ..Default::default()
            },
        );
        type_text(&mut app, "u.");
        let session = app.completion.session.as_ref().unwrap();
        assert_eq!(session.anchor, (0, 2), "the list hangs after the dot");
        answer(
            &mut app,
            "u.\n",
            (0, 2),
            vec![item("name"), item("age")],
            false,
            false,
        );
        assert!(app.completion_visible());
        type_text(&mut app, " ");
        assert!(app.completion.session.is_none(), "a space closes it");
        let before = app.completion.generation;
        type_text(&mut app, "a:");
        let after_colon = app.completion.generation;
        type_text(&mut app, ":");
        assert!(app.completion.generation > after_colon && after_colon > before);
    }

    #[test]
    fn escape_a_motion_or_erasing_past_the_start_close_the_list() {
        let (mut app, _path, _dir) = app_with("\n", "a.rs");
        type_text(&mut app, "ab");
        answer(&mut app, "ab\n", (0, 2), vec![item("abc")], false, false);
        assert!(app.completion_visible());
        let _ = app.update(Message::EscapePressed);
        assert!(!app.completion_visible());
        type_text(&mut app, "c");
        answer(&mut app, "abc\n", (0, 3), vec![item("abcd")], false, false);
        let _ = app.update(Message::EditorAction(Action::Move(
            text_editor::Motion::Left,
        )));
        assert!(app.completion.session.is_none());
        type_text(&mut app, "x");
        answer(&mut app, "abxc\n", (0, 3), vec![item("abxd")], false, false);
        for _ in 0..3 {
            let _ = app.update(Message::EditorAction(Action::Edit(Edit::Backspace)));
        }
        assert!(app.completion.session.is_none(), "erased before the word");
    }

    #[test]
    fn digits_do_not_ask_and_without_a_server_nothing_is_asked() {
        let (mut app, _path, _dir) = app_with("\n", "a.rs");
        type_text(&mut app, "12");
        assert_eq!(app.completion.generation, 0);
        let (mut app, _path, _dir) = app_with("\n", "a.xyz");
        app.completion.installed.insert("xyz".into(), false);
        type_text(&mut app, "ab");
        assert_eq!(app.completion.generation, 0);
        let _ = app.update(Message::CompletionInvoke);
        assert!(app.status_is_error, "the shortcut says there is no server");
        // Turned off: never asked.
        let (mut app, _path, _dir) = app_with("\n", "a.rs");
        app.config.completion_enabled = false;
        type_text(&mut app, "ab");
        assert_eq!(app.completion.generation, 0);
    }

    #[test]
    fn a_snippet_selects_its_places_one_after_the_other() {
        let (mut app, _path, _dir) = app_with("\n", "a.rs");
        type_text(&mut app, "gr");
        let mut greet = item("greet");
        greet.new_text = "greet(${1:loud}, ${2:n})$0".into();
        greet.snippet = true;
        answer(&mut app, "gr\n", (0, 2), vec![greet], false, false);
        key(&mut app, "Return");
        assert_eq!(text_of(&app), "greet(loud, n)\n");
        assert_eq!(app.editor.content.selection().as_deref(), Some("loud"));
        type_text(&mut app, "true");
        assert_eq!(text_of(&app), "greet(true, n)\n");
        key(&mut app, "Tab");
        assert_eq!(app.editor.content.selection().as_deref(), Some("n"));
        type_text(&mut app, "42");
        key(&mut app, "Tab");
        assert_eq!(
            app.editor.content.cursor_position(),
            (0, 15),
            "after the call"
        );
        assert!(app.completion.snippet.is_none());
    }

    #[test]
    fn typing_in_a_place_writes_its_copies() {
        let (mut app, _path, _dir) = app_with("\n", "a.rs");
        type_text(&mut app, "le");
        let mut binding = item("let");
        binding.new_text = "let ${1:x} = ${1};$0".into();
        binding.snippet = true;
        answer(&mut app, "le\n", (0, 2), vec![binding], false, false);
        key(&mut app, "Return");
        assert_eq!(text_of(&app), "let x = x;\n");
        assert_eq!(app.editor.content.selection().as_deref(), Some("x"));
        type_text(&mut app, "ab");
        assert_eq!(text_of(&app), "let ab = ab;\n");
        assert_eq!(
            app.editor.content.cursor_position(),
            (0, 6),
            "still in the first place"
        );
        key(&mut app, "Tab");
        assert_eq!(app.editor.content.cursor_position(), (0, 12), "at the end");
        let _ = app.update(Message::Undo);
        assert_eq!(
            text_of(&app),
            "let x = x;\n",
            "the copy is undone with the typing"
        );
    }

    #[test]
    fn the_import_the_server_adds_comes_in_the_same_undo_step() {
        let text = "const x = 1;\n\n";
        let (mut app, _path, _dir) = app_with(text, "a.ts");
        app.editor.content.select_bytes(1, 0, 0);
        type_text(&mut app, "form");
        let (mut items, _) =
            cmp::parse_list(&json!([{ "label": "formatDate", "data": { "id": 1 } }]), "");
        let format = items.remove(0);
        answer(
            &mut app,
            "const x = 1;\nform\n",
            (1, 4),
            vec![format],
            false,
            false,
        );
        key(&mut app, "Return");
        assert_eq!(text_of(&app), "const x = 1;\nformatDate\n");
        let token = app.completion.import_token;
        let _ = app.update(Message::CompletionImport(
            token,
            Ok(json!({ "label": "formatDate", "additionalTextEdits": [{
                "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 0 } },
                "newText": "import { formatDate } from \"./b\";\n" }] })),
        ));
        assert_eq!(
            text_of(&app),
            "import { formatDate } from \"./b\";\nconst x = 1;\nformatDate\n"
        );
        assert_eq!(
            app.editor.content.cursor_position(),
            (2, 10),
            "the cursor stays after the name"
        );
        let _ = app.update(Message::Undo);
        assert_eq!(
            text_of(&app),
            "const x = 1;\nform\n",
            "one step undoes both"
        );
    }

    #[test]
    fn while_the_project_loads_the_list_is_asked_again_once_it_is_done() {
        let (mut app, _path, _dir) = app_with("\n", "a.rs");
        type_text(&mut app, "ve");
        answer(&mut app, "ve\n", (0, 2), Vec::new(), false, true);
        assert!(
            app.completion.session.is_some(),
            "kept while the server loads"
        );
        let generation = app.completion.generation;
        let _ = app.update(Message::CompletionBusy(generation, true));
        assert_eq!(app.completion.generation, generation, "still loading: wait");
        let _ = app.update(Message::CompletionBusy(generation, false));
        assert_eq!(
            app.completion.generation,
            generation + 1,
            "loaded: asked again"
        );
        answer(
            &mut app,
            "ve\n",
            (0, 2),
            vec![item("vec"), item("Vec")],
            false,
            false,
        );
        assert_eq!(shown(&app), vec!["vec", "Vec"]);
    }

    fn hint_answer_with(app: &mut SimpleEditApp, value: serde_json::Value) {
        let generation = app.completion.hint_generation;
        let _ = app.update(Message::HintAnswer(generation, Ok(Some(value))));
    }

    fn served_with_hints(app: &mut SimpleEditApp, path: &Path) {
        app.completion.info.insert(
            server_key(path, "rs"),
            lsp::CompletionInfo {
                supported: true,
                signatures: true,
                signature_triggers: vec!["(".into(), ",".into()],
                ..Default::default()
            },
        );
    }

    #[test]
    fn the_help_on_parameters_opens_follows_and_closes() {
        let (mut app, path, _dir) = app_with("\n", "a.rs");
        served_with_hints(&mut app, &path);
        type_text(&mut app, "greet(");
        assert_eq!(app.completion.hint_generation, 1, "asked on (");
        let help = json!({ "signatures": [ { "label": "fn greet(loud: bool, n: u32)",
            "parameters": [ { "label": [9, 19] }, { "label": [21, 27] } ] } ],
            "activeSignature": 0, "activeParameter": 0 });
        hint_answer_with(&mut app, help.clone());
        let hint = app.completion.hint.as_ref().unwrap();
        assert_eq!(hint.anchor, (0, 6));
        assert_eq!(hint.signatures.param_of(0), Some(0));
        // Typing in the call asks again (the parameter may change).
        type_text(&mut app, "t");
        assert_eq!(app.completion.hint_generation, 2);
        // An old answer is dropped.
        let _ = app.update(Message::HintAnswer(1, Ok(None)));
        assert!(app.completion.hint.is_some());
        let mut second = help.clone();
        second["activeParameter"] = json!(1);
        type_text(&mut app, ",");
        hint_answer_with(&mut app, second);
        assert_eq!(
            app.completion.hint.as_ref().unwrap().signatures.param_of(0),
            Some(1)
        );
        // Out of the call: the server says so, the help closes.
        type_text(&mut app, ")");
        let generation = app.completion.hint_generation;
        let _ = app.update(Message::HintAnswer(generation, Ok(None)));
        assert!(app.completion.hint.is_none());
    }

    #[test]
    fn overloads_are_gone_through_and_escape_closes() {
        let (mut app, path, _dir) = app_with("\n", "a.rs");
        served_with_hints(&mut app, &path);
        type_text(&mut app, "f(");
        let help = json!({ "signatures": [ { "label": "f(a: string)" }, { "label": "f(a: number, b: number)" } ],
            "activeSignature": 1 });
        hint_answer_with(&mut app, help.clone());
        assert_eq!(app.completion.hint.as_ref().unwrap().shown, 1);
        let alt = |app: &mut SimpleEditApp, key: &str| {
            let _ = app.update(Message::KeyPressed {
                key: key.into(),
                ctrl: false,
                shift: false,
                alt: true,
            });
        };
        alt(&mut app, "ArrowDown");
        assert_eq!(
            app.completion.hint.as_ref().unwrap().shown,
            0,
            "it goes round"
        );
        // The pick stays when the server answers again with the same list.
        type_text(&mut app, "x");
        hint_answer_with(&mut app, help);
        assert_eq!(app.completion.hint.as_ref().unwrap().shown, 0);
        let _ = app.update(Message::EscapePressed);
        assert!(app.completion.hint.is_none());
        // Asked with the shortcut, outside any call: said so.
        let _ = app.update(Message::HintInvoke);
        let generation = app.completion.hint_generation;
        let _ = app.update(Message::HintAnswer(generation, Ok(None)));
        assert!(app.completion.hint.is_none());
        assert!(!app.status_is_error);
    }

    #[test]
    fn a_language_setting_starts_its_servers_again() {
        let (mut app, path, _dir) = app_with("\n", "a.rs");
        let key = server_key(&path, "rs");
        app.completion.asked.insert(key.clone());
        app.completion
            .info
            .insert(key.clone(), lsp::CompletionInfo::default());
        type_text(&mut app, "ab");
        let _ = app.update(Message::Preferences(
            crate::preferences::PreferencesMessage::CompletionOption("rust.postfix".into(), false),
        ));
        assert_eq!(
            app.config.completion_options.get("rust.postfix"),
            Some(&false)
        );
        assert!(
            app.completion.info.is_empty(),
            "what the old server said is gone"
        );
        assert!(
            app.completion.asked.contains(&key),
            "the file's server is prepared again, with the setting"
        );
        assert!(app.completion.session.is_none());
    }

    #[test]
    fn the_typed_letters_are_marked_in_the_label() {
        // The filter text is the label.
        assert_eq!(label_marks("map", "map", &[0, 1]), vec![true, true, false]);
        // TypeScript's filter text has the dot before the label.
        assert_eq!(
            label_marks("age", ".age", &[0, 1]),
            vec![true, false, false]
        );
        // rust-analyzer's label has the parentheses after.
        assert_eq!(
            label_marks("greet(…)", "greet", &[0]),
            vec![true, false, false, false, false, false, false, false]
        );
        assert_eq!(label_marks("x", "unrelated", &[0]), vec![false]);
    }

    #[test]
    fn long_texts_are_cut_with_an_ellipsis() {
        assert_eq!(clip("abc", 5), "abc");
        assert_eq!(clip("abcdef", 4), "abc…");
    }

    /// Draws the window with a list open, to look at it without a screen:
    /// `SNAPSHOT=/tmp/list.png cargo test completion_list_look -- --ignored`.
    #[test]
    #[ignore]
    fn completion_list_look() {
        let Ok(out) = std::env::var("SNAPSHOT") else {
            return;
        };
        let text = "struct User { name: String, age: u32 }\n\nfn main() {\n    let u = User { name: \"x\".into(), age: 1 };\n    u.\n}\n";
        let (mut app, path, _dir) = app_with(text, "main.rs");
        app.completion.info.insert(
            server_key(&path, "rs"),
            lsp::CompletionInfo {
                supported: true,
                triggers: vec![".".into()],
                busy: false,
                ..Default::default()
            },
        );
        app.editor.content.select_bytes(4, 6, 0);
        type_text(&mut app, "n");
        let found = cmp::parse_list(
            &json!({ "isIncomplete": true, "items": [
                { "label": "name", "kind": 5, "labelDetails": { "description": "String" }, "sortText": "1", "detail": "String",
                  "documentation": { "kind": "markdown", "value": "The **name** of the user.\n\n```rust\nlet n = u.name;\n```" } },
                { "label": "greet(…)", "kind": 2, "filterText": "greet", "labelDetails": { "description": "fn(&self, bool) -> String" }, "sortText": "2" },
                { "label": "into()", "kind": 2, "filterText": "into", "labelDetails": { "detail": "(as Into)", "description": "fn(self) -> T" }, "sortText": "3" },
                { "label": "not_now()", "kind": 2, "filterText": "not_now", "labelDetails": { "detail": "(as Thing)", "description": "fn(&self)" }, "sortText": "4" },
                { "label": "new", "kind": 3, "sortText": "5", "deprecated": true },
            ]}),
            "",
        )
        .0;
        let asked = crate::editor::buffer_text(&app.editor.content);
        answer(&mut app, &asked, (4, 6), found, true, false);
        // The help on parameters too, above the line.
        app.completion.hint = Some(Hint {
            path: path.clone(),
            anchor: (4, 6),
            anchor_at: None,
            signatures: cmp::parse_signatures(&json!({ "signatures": [
                { "label": "fn greet(&self, loud: bool, n: u32) -> String",
                  "documentation": "Says hello.",
                  "parameters": [ { "label": [9, 14] }, { "label": [16, 26] }, { "label": [28, 34] } ] },
                { "label": "fn greet(&self) -> String" } ], "activeParameter": 1 }))
            .unwrap(),
            shown: 0,
            picked: false,
        });
        let at = Some((iced::Point::new(380.0, 260.0), 21.0));
        let _ = app.update(Message::CompletionAnchor(at, at));
        let theme = se_theme::ink_dark();
        let settings = iced::Settings {
            default_font: se_theme::UI,
            fonts: se_theme::font_files(),
            ..Default::default()
        };
        let mut ui = iced_test::Simulator::with_size(settings, (1280.0, 800.0), app.view());
        let shot = ui.snapshot(&theme).unwrap();
        let _ = std::fs::remove_file(format!("{out}-wgpu.png"));
        assert!(shot.matches_image(&out).unwrap());
    }
}
