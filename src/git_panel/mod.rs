//! The Git panel: Commit, Stash and Log tabs at the bottom of the window.
//!
//! [`Panel`] holds what the tabs show; [`GitMsg`] is what happens to it. The
//! work itself is done by [`crate::git`], always off the interface thread:
//! each action is a task that answers with a message.

pub mod branches;
pub mod conflicts_view;
pub mod diff_syntax;
pub mod diff_view;
pub mod icon;
pub mod log;
pub mod log_view;
pub mod merge;
pub mod stash;
pub mod stash_view;
pub mod tools;
pub mod view;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use iced::widget::text_editor;
use iced::Task;

use crate::app::Message;
use crate::git::{self, Change, FileDiff, Kind, Source, Status};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Commit,
    Stash,
    Log,
    Branches,
}

/// A file of the list the viewer walks through.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub source: Source,
    pub change: Change,
    /// In the index (the "staged" part of the commit list).
    pub staged: bool,
}

/// Where to land in a file once its diff has loaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Land {
    First,
    Last,
}

/// The diff pane: a list of files, one open, and a place in it.
pub struct Viewer {
    pub entries: Vec<Entry>,
    pub index: Option<usize>,
    pub diff: Option<Result<FileDiff, String>>,
    /// The change block the arrows are on.
    pub change: usize,
    pub side_by_side: bool,
    land: Option<Land>,
    /// The load under way only looks for a newer diff of the open file: it
    /// changes nothing if there is none, and moves no scrollbar.
    silent: bool,
    /// The id of this viewer's last request: a late answer for another file
    /// (or another viewer) is ignored.
    token: u64,
    /// Bumped each time `diff` really changes: the syntax colors follow it,
    /// not the request token (a quiet look that finds nothing new keeps them).
    diff_rev: u64,
    /// What drawing the diff needs that costs a walk through its text.
    layout: diff_view::LayoutCache,
    /// Where its diff scrolls.
    pub scroll: iced::widget::Id,
    /// How many characters a side of the side-by-side view holds on a line,
    /// as the view last saw it (0: not known yet); long lines wrap there.
    pub columns: std::cell::Cell<usize>,
    /// How far down the diff is scrolled and how tall it shows: only the rows
    /// around that are built.
    pub view_top: std::cell::Cell<f32>,
    pub view_height: std::cell::Cell<f32>,
    /// The syntax colors of the open diff (for the theme they were made for,
    /// and the request they belong to).
    syntax: std::cell::RefCell<Option<(bool, u64, std::sync::Arc<diff_syntax::Spans>)>>,
    /// The rows picked with the mouse (to copy them).
    pub selection: Option<Selection>,
    /// How the diff is read: changes of blanks left out, lines of context.
    pub ignore_blanks: bool,
    pub context: u32,
    /// The mouse button is held down over the diff.
    dragging: bool,
    /// The last press, to tell a double click.
    last_press: Option<(std::time::Instant, Pos)>,
}

/// A place in the diff: a row and a character of its text (tabs counted as
/// four spaces, as shown).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pos {
    pub row: usize,
    pub col: usize,
}

/// Text of the diff picked with the mouse, from `anchor` to `cursor`; in the
/// side-by-side view, within one side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub anchor: Pos,
    pub cursor: Pos,
    pub right: bool,
}

impl Selection {
    fn ends(&self) -> (Pos, Pos) {
        (self.anchor.min(self.cursor), self.anchor.max(self.cursor))
    }

    /// The characters of a row of `len` that are picked.
    pub fn span(&self, row: usize, len: usize) -> Option<(usize, usize)> {
        let (lo, hi) = self.ends();
        if row < lo.row || row > hi.row {
            return None;
        }
        let start = if row == lo.row { lo.col.min(len) } else { 0 };
        let end = if row == hi.row { hi.col.min(len) } else { len };
        (start < end).then_some((start, end))
    }
}

/// What the mouse does on the text of a diff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffPointer {
    /// Pressed on a character of a row (side by side: of the right half or not).
    Press(usize, bool, usize),
    /// Dragged to a character of a row.
    Drag(usize, usize),
    Release,
}

/// The word around a character of a text (or the run of blanks, or the one
/// symbol), as the characters it spans.
fn word_at(text: &str, col: usize) -> (usize, usize) {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return (0, 0);
    }
    let at = col.min(chars.len() - 1);
    let class = |c: char| {
        if c.is_alphanumeric() || c == '_' {
            0
        } else if c.is_whitespace() {
            1
        } else {
            2
        }
    };
    let kind = class(chars[at]);
    if kind == 2 {
        return (at, at + 1);
    }
    let mut start = at;
    while start > 0 && class(chars[start - 1]) == kind {
        start -= 1;
    }
    let mut end = at + 1;
    while end < chars.len() && class(chars[end]) == kind {
        end += 1;
    }
    (start, end)
}

/// The part of a text between two columns of its shown form, where a tab
/// counts as four.
fn original_slice(text: &str, start: usize, end: usize) -> String {
    let mut shown = 0;
    let mut out = String::new();
    for c in text.chars() {
        let width = if c == '\t' { 4 } else { 1 };
        if shown >= start && shown < end {
            out.push(c);
        }
        shown += width;
    }
    out
}

/// Tokens are unique across the viewers.
static TOKENS: AtomicU64 = AtomicU64::new(1);

impl Default for Viewer {
    fn default() -> Self {
        Viewer {
            entries: Vec::new(),
            index: None,
            diff: None,
            change: 0,
            side_by_side: true,
            land: None,
            silent: false,
            token: 0,
            diff_rev: 0,
            layout: std::cell::RefCell::new(None),
            scroll: iced::widget::Id::unique(),
            columns: std::cell::Cell::new(0),
            view_top: std::cell::Cell::new(0.0),
            // Until the scrollable says, a tall screen's worth.
            view_height: std::cell::Cell::new(1400.0),
            syntax: std::cell::RefCell::new(None),
            selection: None,
            ignore_blanks: false,
            context: 3,
            dragging: false,
            last_press: None,
        }
    }
}

impl Viewer {
    /// The syntax colors of the open diff, made when first asked for.
    pub fn syntax_spans(&self, dark: bool) -> Option<std::sync::Arc<diff_syntax::Spans>> {
        let diff = self.file_diff()?;
        let path = &self.current()?.change.path;
        let mut cache = self.syntax.borrow_mut();
        match &*cache {
            Some((was_dark, token, spans)) if *was_dark == dark && *token == self.diff_rev => {
                return Some(spans.clone());
            }
            _ => {}
        }
        let spans = std::sync::Arc::new(diff_syntax::highlight(diff, path, dark));
        *cache = Some((dark, self.diff_rev, spans.clone()));
        Some(spans)
    }

    pub fn diff_rev(&self) -> u64 {
        self.diff_rev
    }

    pub fn options(&self) -> git::DiffOptions {
        git::DiffOptions {
            context: self.context,
            ignore_blanks: self.ignore_blanks,
        }
    }

    /// 3 lines of context, then 10, 30, the whole file, and back.
    pub fn cycle_context(&mut self) {
        self.context = match self.context {
            3 => 10,
            10 => 30,
            30 => u32::MAX,
            _ => 3,
        };
    }

    /// The same file read again with the options (the place in it is kept).
    pub fn reopen(&mut self, repo: &Path) -> Task<Message> {
        match self.index {
            Some(i) => {
                let keep = self.change;
                let task = self.open(repo, i, Land::First);
                self.change = keep;
                task
            }
            None => Task::none(),
        }
    }

    /// The changed lines of the rows picked, by their place in the diff.
    pub fn picked_changes(&self) -> std::collections::HashSet<(usize, usize)> {
        let Some(sel) = self.selection else {
            return Default::default();
        };
        let (lo, hi) = sel.ends();
        self.row_lines(sel.right)
            .into_iter()
            .enumerate()
            .filter(|(row, _)| (lo.row..=hi.row).contains(row))
            .filter_map(|(_, line)| line.filter(|l| l.tag != crate::git::diff::Tag::Context))
            .map(|l| l.pos)
            .collect()
    }

    /// What staging (or unstaging) a block or lines of the open diff takes:
    /// the patch and which way it goes, when this diff can do it.
    pub fn index_patch(&self, block: Option<usize>) -> Option<(String, bool)> {
        use crate::git::Source;
        // Blanks left out or a file that is new: the patch would not fit.
        if self.ignore_blanks {
            return None;
        }
        let reverse = match self.current()?.source {
            Source::Unstaged => false,
            Source::Staged => true,
            _ => return None,
        };
        let diff = self.file_diff()?;
        let patch = match block {
            Some(b) => diff.block_patch(b, reverse)?,
            None => diff.lines_patch(&self.picked_changes(), reverse)?,
        };
        Some((patch, reverse))
    }

    pub fn is_dragging(&self) -> bool {
        self.dragging
    }

    pub fn pick(&mut self, pick: DiffPointer) {
        match pick {
            DiffPointer::Press(row, right, col) => {
                let here = Pos { row, col };
                let double = self.last_press.is_some_and(|(at, p)| {
                    at.elapsed() < std::time::Duration::from_millis(400)
                        && p.row == row
                        && p.col.abs_diff(col) <= 1
                });
                self.last_press = Some((std::time::Instant::now(), here));
                self.dragging = true;
                let (anchor, cursor) = match (double, self.line_text(row, right)) {
                    (true, Some(text)) => {
                        let (a, b) = word_at(&text, col);
                        (Pos { row, col: a }, Pos { row, col: b })
                    }
                    _ => (here, here),
                };
                self.selection = Some(Selection {
                    anchor,
                    cursor,
                    right,
                });
            }
            DiffPointer::Drag(row, col) => {
                if let (true, Some(sel)) = (self.dragging, self.selection.as_mut()) {
                    sel.cursor = Pos { row, col };
                }
            }
            DiffPointer::Release => self.dragging = false,
        }
    }

    /// The line each row of the layout in use holds (of one side, side by
    /// side), `None` for a header or an empty half. Built once per use.
    fn row_lines(&self, right: bool) -> Vec<Option<&crate::git::diff::Line>> {
        let Some(diff) = self.file_diff() else {
            return Vec::new();
        };
        if self.side_by_side {
            diff.side_rows()
                .into_iter()
                .map(|row| match row {
                    crate::git::diff::SideRow::Pair(left, r) => {
                        if right {
                            r
                        } else {
                            left
                        }
                    }
                    crate::git::diff::SideRow::Hunk(_) => None,
                })
                .collect()
        } else {
            diff.rows()
                .into_iter()
                .map(|row| match row {
                    crate::git::diff::Row::Line(l) => Some(l),
                    crate::git::diff::Row::Hunk(_) => None,
                })
                .collect()
        }
    }

    /// The text of a row as shown (tabs expanded), if it has one.
    fn line_text(&self, row: usize, right: bool) -> Option<String> {
        let lines = self.row_lines(right);
        let line = lines.get(row).copied().flatten()?;
        Some(line.text.replace('\t', "    "))
    }

    /// Every row (of the right side, or of the side last picked).
    pub fn select_all(&mut self) {
        let right = self.selection.is_none_or(|s| s.right);
        let lines = self.row_lines(right);
        let Some(last) = lines.len().checked_sub(1) else {
            return;
        };
        let col = lines[last].map_or(0, |l| l.text.replace('\t', "    ").chars().count());
        self.selection = Some(Selection {
            anchor: Pos { row: 0, col: 0 },
            cursor: Pos { row: last, col },
            right,
        });
    }

    /// Whether something is picked (a click alone picks nothing).
    pub fn has_selection(&self) -> bool {
        self.selection.is_some_and(|s| s.anchor != s.cursor)
    }

    /// The picked text, as it is in the file (tabs back).
    pub fn selected_text(&self) -> Option<String> {
        let sel = self.selection?;
        let (lo, hi) = sel.ends();
        if lo == hi {
            return None;
        }
        let lines = self.row_lines(sel.right);
        let mut out: Vec<String> = Vec::new();
        for row in lo.row..=hi.row {
            let Some(line) = lines.get(row).copied().flatten() else {
                continue;
            };
            let shown = line.text.replace('\t', "    ");
            let Some((start, end)) = sel.span(row, shown.chars().count()) else {
                // A picked line with nothing in it, between the ends.
                if row > lo.row && row < hi.row {
                    out.push(String::new());
                }
                continue;
            };
            out.push(original_slice(&line.text, start, end));
        }
        if out.is_empty() {
            None
        } else {
            Some(out.join("\n"))
        }
    }

    pub fn current(&self) -> Option<&Entry> {
        self.index.and_then(|i| self.entries.get(i))
    }

    pub fn file_diff(&self) -> Option<&FileDiff> {
        match &self.diff {
            Some(Ok(diff)) => Some(diff),
            _ => None,
        }
    }

    pub fn changes(&self) -> usize {
        self.file_diff().map_or(0, FileDiff::changes)
    }

    /// How far down a row starts: long lines of the side-by-side view wrap,
    /// so rows are not all as tall.
    fn row_top(&self, row: usize) -> f32 {
        match (self.side_by_side, self.file_diff()) {
            (true, Some(diff)) => {
                let layout =
                    diff_view::layout(&self.layout, self.diff_rev, diff, self.columns.get(), true);
                diff_view::side_top(&layout, row)
            }
            _ => row as f32 * diff_view::ROW_HEIGHT,
        }
    }

    /// The row to bring into view for the current change.
    pub fn change_row(&self) -> Option<usize> {
        let diff = self.file_diff()?;
        let rows = if self.side_by_side {
            diff.change_side_rows()
        } else {
            diff.change_rows()
        };
        rows.get(self.change).copied()
    }

    /// Whether an answer is for this viewer's last request.
    pub fn owns(&self, token: u64) -> bool {
        self.token == token
    }

    /// Opens an entry: its diff is asked for, the answer comes as
    /// [`GitMsg::DiffLoaded`].
    pub fn open(&mut self, repo: &Path, index: usize, land: Land) -> Task<Message> {
        let Some(entry) = self.entries.get(index) else {
            return Task::none();
        };
        self.index = Some(index);
        self.diff = None;
        self.selection = None;
        self.change = 0;
        self.land = Some(land);
        self.silent = false;
        self.token = TOKENS.fetch_add(1, Ordering::Relaxed);
        let token = self.token;
        let repo = repo.to_path_buf();
        let options = self.options();
        let source = entry.source.clone();
        let path = entry.change.path.clone();
        let from = entry.change.from.clone();
        Task::perform(
            async move { git::diff_with(&repo, &source, &path, from.as_deref(), options).await },
            move |result| Message::Git(GitMsg::DiffLoaded(token, result)),
        )
    }

    /// Keeps the open file when the list changes under it.
    pub fn set_entries(&mut self, repo: &Path, entries: Vec<Entry>) -> Task<Message> {
        let current = self.current().cloned();
        let same = entries == self.entries;
        self.entries = entries;
        if same {
            return Task::none();
        }
        match current {
            Some(entry) => match self.entries.iter().position(|e| e == &entry) {
                Some(i) => {
                    self.index = Some(i);
                    // The content may have changed with the list: looked at
                    // quietly, so the diff on screen doesn't blink.
                    self.refresh(repo)
                }
                None => self.select_after_loss(repo),
            },
            None => Task::none(),
        }
    }

    /// The open file left the list: the viewer goes to the first one.
    fn select_after_loss(&mut self, repo: &Path) -> Task<Message> {
        self.index = None;
        self.diff = None;
        self.selection = None;
        if self.entries.is_empty() {
            Task::none()
        } else {
            self.open(repo, 0, Land::First)
        }
    }

    /// Looks at the open file again, quietly: the diff changes on screen only
    /// if the file did (a save, a stage, a rollback made elsewhere).
    pub fn refresh(&mut self, repo: &Path) -> Task<Message> {
        let Some(entry) = self.current().cloned() else {
            return Task::none();
        };
        self.token = TOKENS.fetch_add(1, Ordering::Relaxed);
        self.silent = true;
        let (token, repo) = (self.token, repo.to_path_buf());
        let options = self.options();
        Task::perform(
            async move {
                let change = &entry.change;
                git::diff_with(
                    &repo,
                    &entry.source,
                    &change.path,
                    change.from.as_deref(),
                    options,
                )
                .await
            },
            move |result| Message::Git(GitMsg::DiffLoaded(token, result)),
        )
    }

    /// The answer of a load.
    pub fn loaded(&mut self, token: u64, result: Result<FileDiff, String>) -> Task<Message> {
        if token != self.token {
            return Task::none();
        }
        if std::mem::take(&mut self.silent) {
            if self.diff.as_ref() == Some(&result) {
                return Task::none();
            }
            self.diff = Some(result);
            self.diff_rev += 1;
            // What was picked may be other lines now.
            self.selection = None;
            self.change = self.change.min(self.changes().saturating_sub(1));
            return Task::none();
        }
        self.diff = Some(result);
        self.diff_rev += 1;
        let count = self.changes();
        match self.land.take() {
            Some(Land::Last) => self.change = count.saturating_sub(1),
            _ => self.change = self.change.min(count.saturating_sub(1)),
        }
        self.scroll_to_change()
    }

    /// Shows a diff made elsewhere (no Git to ask), beginning at its start.
    pub fn show(&mut self, entry: Entry, diff: FileDiff) {
        self.token += 1;
        self.entries = vec![entry];
        self.index = Some(0);
        self.diff = Some(Ok(diff));
        self.diff_rev += 1;
        self.change = 0;
        self.selection = None;
        self.view_top.set(0.0);
    }

    pub fn scroll_to_change(&self) -> Task<Message> {
        let y = match self.change_row() {
            Some(row) => self.row_top(row.saturating_sub(3)).max(0.0),
            None => 0.0,
        };
        // The rows there are built right away, not when the scrollable says.
        self.view_top.set(y);
        iced::widget::operation::scroll_to(
            self.scroll.clone(),
            iced::widget::scrollable::AbsoluteOffset { x: 0.0, y },
        )
    }

    /// Down: the next change, or the first of the next file.
    pub fn next_change(&mut self, repo: &Path) -> Task<Message> {
        if self.change + 1 < self.changes() {
            self.change += 1;
            return self.scroll_to_change();
        }
        // The last one of all: it is brought back into view (with a whole
        // file shown, it may be far from where the eye is).
        if self.index.is_some_and(|i| i + 1 >= self.entries.len()) {
            return self.scroll_to_change();
        }
        self.next_file(repo, Land::First)
    }

    /// Up: the previous change, or the last of the previous file.
    pub fn prev_change(&mut self, repo: &Path) -> Task<Message> {
        if self.change > 0 {
            self.change -= 1;
            return self.scroll_to_change();
        }
        if self.index == Some(0) {
            return self.scroll_to_change();
        }
        self.prev_file(repo, Land::Last)
    }

    pub fn next_file(&mut self, repo: &Path, land: Land) -> Task<Message> {
        match self.index {
            Some(i) if i + 1 < self.entries.len() => self.open(repo, i + 1, land),
            _ => Task::none(),
        }
    }

    pub fn prev_file(&mut self, repo: &Path, land: Land) -> Task<Message> {
        match self.index {
            Some(i) if i > 0 => self.open(repo, i - 1, land),
            _ => Task::none(),
        }
    }
}

/// Where "Initialize a repository" creates one for the folder `dir`: the
/// project it is in, else the folder itself. Never the home folder or above
/// it, which would put every project under it in one repository.
fn init_target(dir: &Path, roots: &[PathBuf]) -> Option<PathBuf> {
    let target = roots
        .iter()
        .filter(|r| dir.starts_with(r))
        .max_by_key(|r| r.components().count())
        .cloned()
        .unwrap_or_else(|| dir.to_path_buf());
    let above_home = dirs::home_dir().is_some_and(|home| home.starts_with(&target));
    (!above_home).then_some(target)
}

/// The Git panel.
pub struct Panel {
    pub open: bool,
    pub tab: Tab,
    pub repo: Option<PathBuf>,
    /// The folder the repository was looked for from.
    pub(super) probed: Option<PathBuf>,
    /// Whether that folder is in a repository (false: offer `git init`).
    pub searched: bool,
    /// Where `git init` would create the repository, shown on its button.
    pub(super) init_dir: Option<PathBuf>,
    pub status: Option<Status>,
    pub viewer: Viewer,
    pub stash: stash::State,
    pub log: log::State,
    pub message: text_editor::Content,
    /// The message box has the keyboard: the app's shortcuts stay out.
    pub typing: bool,
    pub amend: bool,
    pub commit_options: git::tools::CommitOptions,
    pub commit_options_shown: bool,
    /// The commit is of this file only (from its Git menu).
    pub only: Option<String>,
    pub busy: bool,
    /// When the timer last asked for a status that has not come back yet:
    /// a slow `git status` is not asked again on top of itself.
    polling: Option<std::time::Instant>,
    pub branches: branches::State,
    /// How a pull and a merge are made (the Preferences, changed here too).
    pub pull_mode: git::PullMode,
    pub merge_mode: git::MergeMode,
    /// What was asked once and waits for a second click to be done.
    pub pending: Option<Pending>,
    /// A repository chosen by hand (from a file's Git menu), kept until
    /// another file comes to the front: the panel then follows that file.
    pinned: Option<Pinned>,
    /// What the last action said, and whether it failed.
    pub notice: Option<(String, bool)>,
    /// The dialog open over the panel.
    pub dialog: Option<tools::Dialog>,
    /// The last push asked for, to do it again once caught up.
    pub last_push: Option<git::tools::PushSpec>,
}

impl Default for Panel {
    fn default() -> Self {
        Panel {
            open: false,
            tab: Tab::Commit,
            repo: None,
            probed: None,
            init_dir: None,
            searched: false,
            status: None,
            viewer: Viewer::default(),
            stash: stash::State::default(),
            log: log::State::default(),
            message: text_editor::Content::new(),
            typing: false,
            amend: false,
            commit_options: git::tools::CommitOptions::default(),
            commit_options_shown: false,
            only: None,
            busy: false,
            polling: None,
            branches: branches::State::default(),
            pull_mode: git::PullMode::default(),
            merge_mode: git::MergeMode::default(),
            pending: None,
            pinned: None,
            notice: None,
            dialog: None,
            last_push: None,
        }
    }
}

/// Something that can't be undone, waiting for a second click.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pending {
    /// Throw away a staged file that also has changes that are not staged.
    Rollback(String),
    /// Delete a file from the disk (and from git).
    Delete(String),
    StashDrop(usize),
    Abort,
    /// Take back a commit that is already on the remote.
    UndoCommit,
}

/// The folder a repository was chosen from, and the file in front then.
#[derive(Debug, Clone)]
struct Pinned {
    dir: PathBuf,
    file: Option<PathBuf>,
    /// Chosen among the projects: it stays whatever file is in front.
    sticky: bool,
}

/// What the panel needs to know of the app.
pub struct Ctx<'a> {
    pub current_file: Option<&'a Path>,
    pub ctrl: bool,
    pub shift: bool,
    /// The projects, and their colors.
    pub roots: &'a [PathBuf],
    pub colors: &'a std::collections::BTreeMap<String, usize>,
}

/// A conflicted file to settle by hand, in an editor tab.
#[derive(Debug)]
pub struct MergeSpec {
    pub repo: PathBuf,
    pub path: String,
    pub merge: crate::git::conflicts::Merge,
}

/// A diff to open in the editor: the files to walk through and where to
/// start.
#[derive(Debug, Clone)]
pub struct DiffTab {
    pub repo: PathBuf,
    pub title: String,
    pub entries: Vec<Entry>,
    pub start: usize,
}

#[derive(Debug, Clone)]
pub enum GitMsg {
    /// The edge of the Branches tab's actions is dragged.
    ActionsResizeStart,
    ActionsResize(f32),
    ActionsResizeEnd,
    /// Time to look again (the panel is open).
    Tick,
    /// A file was saved: look again at once.
    Refresh,
    Detected(Option<PathBuf>, Option<PathBuf>),
    /// The answer of a repository: one that is no longer shown is dropped.
    StatusLoaded(PathBuf, Result<Status, String>),
    SelectTab(Tab),
    // The diff pane
    SelectFile(usize),
    DiffLoaded(u64, Result<FileDiff, String>),
    NextChange,
    PrevChange,
    NextFile,
    PrevFile,
    ToggleSideBySide,
    /// The diff was scrolled: (offset, height shown).
    DiffScrolled(i32, i32),
    /// Stage (or unstage) a block of the open diff, or the lines picked.
    StageBlock(usize),
    StageSelection,
    /// Leave changes of blanks out of the diff; more or less context.
    ToggleBlanks,
    CycleContext,
    /// The commit list was scrolled: (offset, height shown).
    LogScrolled(i32, i32),
    /// The mouse on the rows of the diff.
    Pick(DiffPointer),
    /// Copy the picked text / pick everything (context menu of the diff).
    CopyPicked,
    PickAll,
    // The commit list
    ToggleStaged(usize),
    StageAll(bool),
    Rollback(usize),
    Delete(usize),
    JumpToSource(usize),
    DiffInEditor(usize),
    /// Puts one block of changes of the open file back as it was.
    RollbackBlock(usize),
    /// Opens the file of the diff in the editor.
    OpenFile,
    /// Pushes the current branch (from its Git shortcut).
    /// The push dialog, for the current branch.
    OpenPush,
    /// Opens the modes behind the chevron of a split button, or closes them.
    OpenSplit(branches::Split, iced::Point),
    CloseSplit,
    /// Nothing (a button that only looks pressable).
    Noop,

    // Conflicts
    /// Keep your side (`true`) or theirs of a conflicted file.
    AcceptSide(String, bool),
    /// Settle a conflicted file by hand, in three columns.
    OpenMerge(String),
    ContinueOperation,
    /// Leaves out the commit a rebase or a cherry-pick stopped on.
    SkipOperation,
    AbortOperation,
    MergeBranch(String),
    RebaseOnto(String),
    // Committing
    MessageAction(text_editor::Action),
    MessageFocused,
    ToggleAmend(bool),
    /// Back to committing everything that is staged.
    CancelOnly,
    // The Branches tab
    SelectRef(branches::Ref),
    RefName(String),
    /// Opens (or closes) the popin that names a new branch or tag.
    RefDialog(Option<branches::RefDialog>),
    RefDialogOk,
    RefTagName(String),
    RefTagMessage(String),
    RefAction(branches::RefAction),
    /// A fetch in the background, and whether it worked.
    AutoFetch,
    AutoFetched(bool),
    /// Takes the last commit back (its changes stay staged, its message
    /// comes back to the box).
    UndoCommit,
    /// Puts one file of the list aside (stash), or has git ignore it.
    StashFile(usize),
    IgnoreFile(usize),
    /// Back to the history of the whole branch.
    ClearLogFile,
    Commit(bool),
    Tool(tools::ToolMsg),
    CommitOptionsShown,
    CommitSignoff(bool),
    CommitNoVerify(bool),
    CommitAuthor(String),
    /// The commits a branch has that the current one lacks, or the other way.
    CompareBranch(String),
    CompareSwap,
    CompareClear,
    /// The filters of the log.
    LogFiltersShown,
    LogFilterText(log::FilterField, String),
    LogFilterFlag(log::FilterField, bool),
    LogFilterClear,
    TickCommitFile(String, bool),
    Init,
    /// An action ended: what it said, or why it failed.
    Done(Result<String, String>),
    /// The answer of something that is not an action (it doesn't hold the
    /// panel busy, so it doesn't free it either).
    Reply(Result<String, String>),
    DismissNotice,
    // Stash
    StashesLoaded(PathBuf, Result<Vec<git::history::Stash>, String>),
    SelectStash(usize),
    StashFilesLoaded(usize, Result<Vec<Change>, String>),
    StashApply(usize),
    StashPop(usize),
    StashDrop(usize),
    StashMessage(String),
    StashPush,
    // Log
    RefsLoaded(PathBuf, Result<git::history::Refs, String>),
    BranchClicked(String),
    ToggleGroup(String),
    BranchFilter(String),
    LogLoaded(
        u64,
        String,
        Result<(Vec<git::graph::Commit>, std::collections::HashSet<String>), String>,
    ),
    SelectCommit(usize),
    SelectForMenu(usize),
    CommitFilesLoaded(String, Result<Vec<Change>, String>),
    CommitInfoLoaded(String, Result<(String, Vec<String>), String>),
    LoadMoreCommits,
    Checkout(String),
    BranchDiff(String),
    CommitFileDiff(usize),
    CopyHash,
    Rewrite(log::Action),
    /// What the menu of a commit does besides rewriting history.
    LogAction(log::LogAction),
    /// The name typed in a dialog of the log.
    DialogName(String),
    /// The log is searched (text, in what, and go).
    LogSearch(String),
    LogSearchMode(git::history::SearchMode),
    LogSearchRun,
    /// An interactive rebase: its plan is read, a commit is moved, given
    /// another action or another subject.
    RebasePlanReady(Option<String>, Result<Vec<git::history::PlanItem>, String>),
    RebaseMove(usize, bool),
    RebaseSet(usize, git::history::PlanAction),
    RebaseSubject(usize, String),
    RewriteReady(log::Action, bool, String),
    GuardReady(log::Guarded, bool),
    DialogMessage(text_editor::Action),
    DialogOk,
    DialogCancel,
}

/// How long a status is waited for before the timer asks again.
const POLL_PATIENCE: std::time::Duration = std::time::Duration::from_secs(30);

impl GitMsg {
    /// An answer or a timer, not something the user did: it must not close
    /// an open menu (the panel refreshes itself every two seconds).
    pub fn is_background(&self) -> bool {
        matches!(
            self,
            GitMsg::Tick
                | GitMsg::Refresh
                | GitMsg::Detected(..)
                | GitMsg::StatusLoaded(..)
                | GitMsg::DiffLoaded(..)
                | GitMsg::DiffScrolled(..)
                | GitMsg::AutoFetched(_)
                | GitMsg::RebasePlanReady(..)
                | GitMsg::Noop
                | GitMsg::LogScrolled(..)
                | GitMsg::StashesLoaded(..)
                | GitMsg::StashFilesLoaded(..)
                | GitMsg::RefsLoaded(..)
                | GitMsg::LogLoaded(..)
                | GitMsg::CommitFilesLoaded(..)
                | GitMsg::CommitInfoLoaded(..)
                | GitMsg::RewriteReady(..)
                | GitMsg::GuardReady(..)
                | GitMsg::Done(_)
                | GitMsg::Reply(_)
        ) || matches!(self, GitMsg::Tool(m) if m.is_background())
    }
}

impl Panel {
    /// The status, and the diff the tab shows, looked at again.
    fn look_again(&mut self, ctx: &Ctx) -> Task<Message> {
        let repo = self.repo.clone();
        let more = match (self.open, self.tab, repo) {
            (false, ..) => Task::none(),
            (_, Tab::Stash, _) => self.load_stashes(),
            (_, Tab::Commit, Some(repo)) => self.viewer.refresh(&repo),
            _ => Task::none(),
        };
        Task::batch([self.refresh(ctx), more])
    }

    pub fn toggle(&mut self, ctx: &Ctx) -> Task<Message> {
        self.open = !self.open;
        if self.open {
            self.refresh(ctx)
        } else {
            self.typing = false;
            Task::none()
        }
    }

    /// The diff pane of the tab in front.
    pub fn active_viewer(&mut self) -> &mut Viewer {
        match self.tab {
            Tab::Stash => &mut self.stash.viewer,
            _ => &mut self.viewer,
        }
    }

    /// Looks at the repository again: where it is, then its status.
    pub fn refresh(&mut self, ctx: &Ctx) -> Task<Message> {
        if self
            .pinned
            .as_ref()
            .is_some_and(|p| !p.sticky && p.file.as_deref() != ctx.current_file)
        {
            self.pinned = None;
        }
        let dir = self
            .pinned
            .as_ref()
            .map(|p| p.dir.clone())
            .or_else(|| {
                ctx.current_file
                    .filter(|p| !crate::app::is_untitled(p))
                    .and_then(Path::parent)
                    .map(Path::to_path_buf)
            })
            .or_else(|| self.probed.clone())
            .or_else(|| ctx.roots.first().cloned());
        let Some(dir) = dir else {
            return Task::none();
        };
        if self.probed.as_ref() != Some(&dir) {
            let asked = dir.clone();
            return Task::perform(
                async move {
                    let root = git::repo_root(&asked).await;
                    (asked, root)
                },
                |(dir, root)| Message::Git(GitMsg::Detected(Some(dir), root)),
            );
        }
        self.load_status()
    }

    fn load_status(&self) -> Task<Message> {
        let Some(repo) = self.repo.clone() else {
            return Task::none();
        };
        // Closed, the panel only watches for conflicts: listing every
        // untracked file of a big folder would be wasted.
        let untracked = self.open;
        let asked = repo.clone();
        Task::perform(
            async move { git::status(&repo, untracked).await },
            move |status| Message::Git(GitMsg::StatusLoaded(asked.clone(), status)),
        )
    }

    /// The file list of the Commit tab: staged first, then the rest.
    pub fn commit_entries(status: &Status) -> Vec<Entry> {
        let mut entries: Vec<Entry> = status
            .staged
            .iter()
            .map(|c| Entry {
                source: Source::Staged,
                change: c.clone(),
                staged: true,
            })
            .collect();
        entries.extend(status.unstaged.iter().map(|c| Entry {
            source: if c.kind == Kind::Untracked {
                Source::Untracked
            } else {
                Source::Unstaged
            },
            change: c.clone(),
            staged: false,
        }));
        entries
    }

    /// Stages (or unstages) a block of the open diff, or the lines picked in
    /// it, through a patch applied to the index.
    fn stage_patch(&mut self, block: Option<usize>) -> Task<Message> {
        let Some((patch, reverse)) = self.active_viewer().index_patch(block) else {
            return Task::none();
        };
        self.act(move |repo| async move {
            git::apply_patch(&repo, &patch, reverse).await?;
            Ok(String::new())
        })
    }

    /// Switches to a branch of the list (made first when `create`), from the
    /// switcher of the status bar.
    pub fn switch_branch(&mut self, name: String, create: bool) -> Task<Message> {
        if create {
            let name = name.split_whitespace().collect::<Vec<_>>().join("-");
            self.act(move |repo| async move {
                git::history::create_branch(&repo, &name, None).await?;
                Ok(format!("\u{4}{name}"))
            })
        } else {
            self.checkout(name)
        }
    }

    /// Another repository: what the tabs showed of the previous one is gone.
    fn reset_for_another_repo(&mut self) {
        self.status = None;
        self.viewer = Viewer::default();
        self.stash = stash::State::default();
        self.log = log::State::default();
        self.branches = branches::State::default();
        self.only = None;
        self.pending = None;
    }

    /// Opens the panel on a repository chosen from a file's Git menu, and
    /// keeps it while that file stays in front.
    pub fn show_repo(&mut self, root: PathBuf, current_file: Option<PathBuf>) {
        if self.repo.as_ref() != Some(&root) {
            self.reset_for_another_repo();
        }
        self.repo = Some(root.clone());
        self.probed = Some(root.clone());
        self.searched = true;
        self.pinned = Some(Pinned {
            dir: root,
            file: current_file,
            sticky: false,
        });
        self.open = true;
    }

    /// The repository shown stays until another is chosen.
    pub(super) fn stick_to_repo(&mut self) {
        if let Some(pinned) = &mut self.pinned {
            pinned.sticky = true;
        }
    }

    /// Back to the repository of the file in front.
    pub(super) fn unpin(&mut self) {
        self.pinned = None;
        self.probed = None;
    }

    /// The second click of something that can't be undone: `true` when it is
    /// the second, else it is remembered and `message` says what to do.
    fn confirmed(&mut self, what: Pending, message: String) -> bool {
        if self.pending.as_ref() == Some(&what) {
            self.pending = None;
            return true;
        }
        self.pending = Some(what);
        self.notice = Some((message, false));
        false
    }

    /// Runs `work` on the repository, then looks again.
    fn act<F, Fut>(&mut self, work: F) -> Task<Message>
    where
        F: FnOnce(PathBuf) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<String, String>> + Send + 'static,
    {
        // Two git commands at once would meet at `index.lock`.
        if self.busy {
            return Task::none();
        }
        let Some(repo) = self.repo.clone() else {
            return Task::none();
        };
        self.busy = true;
        Task::perform(work(repo), |result| Message::Git(GitMsg::Done(result)))
    }

    pub fn update(&mut self, msg: GitMsg, ctx: &Ctx) -> Task<Message> {
        // Leaving the message box with a click on anything else gives the
        // keyboard back to the app's shortcuts.
        if matches!(
            msg,
            GitMsg::SelectTab(_)
                | GitMsg::SelectFile(_)
                | GitMsg::ToggleStaged(_)
                | GitMsg::StageAll(_)
                | GitMsg::Rollback(_)
                | GitMsg::Delete(_)
                | GitMsg::JumpToSource(_)
                | GitMsg::DiffInEditor(_)
                | GitMsg::SelectStash(_)
                | GitMsg::SelectRef(_)
                | GitMsg::RefAction(_)
                | GitMsg::BranchClicked(_)
                | GitMsg::SelectCommit(_)
                | GitMsg::NextChange
                | GitMsg::PrevChange
                | GitMsg::NextFile
                | GitMsg::PrevFile
        ) {
            self.typing = false;
        }
        if !msg.is_background()
            && !matches!(
                msg,
                GitMsg::Rollback(_)
                    | GitMsg::Delete(_)
                    | GitMsg::StashDrop(_)
                    | GitMsg::AbortOperation
                    | GitMsg::UndoCommit
            )
        {
            self.pending = None;
        }
        // The modes of a split button give way to anything but its chevron.
        if !msg.is_background() && !matches!(msg, GitMsg::OpenSplit(..)) {
            self.branches.menu = None;
        }
        match msg {
            GitMsg::Tick => {
                if self.busy {
                    return Task::none();
                }
                // Still waiting for the last look (a big repository): wait
                // too, unless it was lost long ago.
                if self.polling.is_some_and(|at| at.elapsed() < POLL_PATIENCE) {
                    return Task::none();
                }
                self.polling = Some(std::time::Instant::now());
                self.look_again(ctx)
            }
            GitMsg::Refresh => self.look_again(ctx),
            GitMsg::Detected(dir, root) => {
                self.init_dir = dir.as_deref().and_then(|d| init_target(d, ctx.roots));
                self.probed = dir;
                self.searched = true;
                if root != self.repo {
                    self.reset_for_another_repo();
                }
                self.repo = root;
                self.load_status()
            }
            // Another repository's answer, late: not for this one.
            GitMsg::StatusLoaded(repo, _) if self.repo.as_ref() != Some(&repo) => Task::none(),
            GitMsg::StatusLoaded(_, result) => {
                self.polling = None;
                match result {
                    Ok(status) => {
                        if self.status.as_ref() != Some(&status) {
                            let entries = Self::commit_entries(&status);
                            self.status = Some(status);
                            let first = self.viewer.entries.is_empty();
                            let repo = self.repo.clone().unwrap_or_default();
                            let task = self.viewer.set_entries(&repo, entries);
                            if first
                                && self.viewer.index.is_none()
                                && !self.viewer.entries.is_empty()
                            {
                                return Task::batch([
                                    task,
                                    self.viewer.open(&repo, 0, Land::First),
                                ]);
                            }
                            return task;
                        }
                    }
                    Err(error) => {
                        self.status = None;
                        self.notice = Some((error, true));
                    }
                }
                Task::none()
            }
            GitMsg::ActionsResizeStart => {
                self.branches.resize_start();
                Task::none()
            }
            GitMsg::ActionsResize(x) => {
                self.branches.resize_to(x);
                Task::none()
            }
            GitMsg::ActionsResizeEnd => {
                self.branches.resize_end();
                Task::none()
            }
            GitMsg::SelectTab(tab) => {
                self.tab = tab;
                self.typing = false;
                match tab {
                    Tab::Commit => Task::none(),
                    Tab::Stash => self.load_stashes(),
                    Tab::Log | Tab::Branches => self.load_refs(),
                }
            }
            GitMsg::SelectFile(index) => match self.repo.clone() {
                Some(repo) => self.active_viewer().open(&repo, index, Land::First),
                None => Task::none(),
            },
            GitMsg::DiffLoaded(token, result) => {
                // The answer is for one viewer: no copy for the others.
                if self.viewer.owns(token) {
                    self.viewer.loaded(token, result)
                } else if self.stash.viewer.owns(token) {
                    self.stash.viewer.loaded(token, result)
                } else {
                    Task::none()
                }
            }
            GitMsg::NextChange => match self.repo.clone() {
                Some(repo) => self.active_viewer().next_change(&repo),
                None => Task::none(),
            },
            GitMsg::PrevChange => match self.repo.clone() {
                Some(repo) => self.active_viewer().prev_change(&repo),
                None => Task::none(),
            },
            GitMsg::NextFile => match self.repo.clone() {
                Some(repo) => self.active_viewer().next_file(&repo, Land::First),
                None => Task::none(),
            },
            GitMsg::PrevFile => match self.repo.clone() {
                Some(repo) => self.active_viewer().prev_file(&repo, Land::First),
                None => Task::none(),
            },
            GitMsg::ToggleSideBySide => {
                let viewer = self.active_viewer();
                viewer.side_by_side = !viewer.side_by_side;
                viewer.selection = None;
                viewer.scroll_to_change()
            }
            GitMsg::Pick(pick) => {
                self.active_viewer().pick(pick);
                Task::none()
            }
            GitMsg::LogScrolled(top, height) => {
                self.log.view_top.set(top as f32);
                self.log.view_height.set(height as f32);
                Task::none()
            }
            GitMsg::StageBlock(block) => self.stage_patch(Some(block)),
            GitMsg::StageSelection => self.stage_patch(None),
            GitMsg::ToggleBlanks => {
                let Some(repo) = self.repo.clone() else {
                    return Task::none();
                };
                let viewer = self.active_viewer();
                viewer.ignore_blanks = !viewer.ignore_blanks;
                viewer.reopen(&repo)
            }
            GitMsg::CycleContext => {
                let Some(repo) = self.repo.clone() else {
                    return Task::none();
                };
                let viewer = self.active_viewer();
                viewer.cycle_context();
                viewer.reopen(&repo)
            }
            GitMsg::DiffScrolled(top, height) => {
                let viewer = self.active_viewer();
                viewer.view_top.set(top as f32);
                viewer.view_height.set(height as f32);
                Task::none()
            }
            GitMsg::CopyPicked => match self.active_viewer().selected_text() {
                Some(text) => iced::clipboard::write(text),
                None => Task::none(),
            },
            GitMsg::PickAll => {
                self.active_viewer().select_all();
                Task::none()
            }
            GitMsg::ToggleStaged(index) => {
                let Some(entry) = self.viewer.entries.get(index).cloned() else {
                    return Task::none();
                };
                let path = entry.change.path.clone();
                let was_staged = entry.staged;
                // Unstaging a rename also brings the old name back.
                let from = entry.change.from.clone().filter(|_| was_staged);
                // Staging a conflicted file marks it settled: with its
                // markers still in, they would go into the commit.
                if !was_staged && entry.change.kind == Kind::Conflict {
                    let markers = self
                        .repo
                        .as_ref()
                        .and_then(|repo| std::fs::read_to_string(repo.join(&path)).ok())
                        .is_some_and(|text| git::conflicts::parse(&text, false).is_some());
                    if markers {
                        self.notice = Some((
                            t!("git.stage_conflict_markers", name = path.as_str()).to_string(),
                            true,
                        ));
                        return Task::none();
                    }
                }
                self.act(move |repo| async move {
                    let paths: Vec<String> = std::iter::once(path).chain(from).collect();
                    if was_staged {
                        git::unstage(&repo, &paths).await?;
                    } else {
                        git::stage(&repo, &paths).await?;
                    }
                    Ok(String::new())
                })
            }
            GitMsg::StageAll(stage) => {
                let Some(status) = &self.status else {
                    return Task::none();
                };
                let paths: Vec<String> = if stage {
                    // Conflicts are left out: staging one marks it settled,
                    // markers and all. Each is staged by itself once settled.
                    status
                        .unstaged
                        .iter()
                        .filter(|c| c.kind != Kind::Conflict)
                        .map(|c| c.path.clone())
                        .collect()
                } else {
                    status
                        .staged
                        .iter()
                        .flat_map(|c| std::iter::once(c.path.clone()).chain(c.from.clone()))
                        .collect()
                };
                if paths.is_empty() {
                    return Task::none();
                }
                self.act(move |repo| async move {
                    if stage {
                        git::stage(&repo, &paths).await?;
                    } else {
                        git::unstage(&repo, &paths).await?;
                    }
                    Ok(String::new())
                })
            }
            GitMsg::Rollback(index) => {
                let Some(entry) = self.viewer.entries.get(index).cloned() else {
                    return Task::none();
                };
                // Rolling back throws the changes away for good: a file that
                // is not tracked yet is deleted, a staged one also goes back
                // to HEAD on disk (changes not staged go with it).
                let also_unstaged = entry.staged
                    && self
                        .status
                        .as_ref()
                        .is_some_and(|s| s.unstaged.iter().any(|c| c.path == entry.change.path));
                let name = entry.change.path.as_str();
                let message = if entry.change.kind == Kind::Untracked {
                    t!("git.confirm_rollback_untracked", name = name)
                } else if also_unstaged {
                    t!("git.confirm_rollback_both")
                } else {
                    t!("git.confirm_rollback", name = entry.change.path.as_str())
                };
                if !self.confirmed(
                    Pending::Rollback(entry.change.path.clone()),
                    message.to_string(),
                ) {
                    return Task::none();
                }
                self.act(move |repo| async move {
                    git::rollback(&repo, &entry.change, entry.staged).await?;
                    Ok(String::new())
                })
            }
            GitMsg::Delete(index) => {
                let Some(entry) = self.viewer.entries.get(index).cloned() else {
                    return Task::none();
                };
                if !self.confirmed(
                    Pending::Delete(entry.change.path.clone()),
                    t!("git.confirm_delete", name = entry.change.path.as_str()).to_string(),
                ) {
                    return Task::none();
                }
                self.act(move |repo| async move {
                    git::delete(&repo, &entry.change).await?;
                    Ok(String::new())
                })
            }
            GitMsg::RollbackBlock(block) => {
                let Some(entry) = self.viewer.current().cloned() else {
                    return Task::none();
                };
                let Some(diff) = self.viewer.file_diff().cloned() else {
                    return Task::none();
                };
                if !matches!(entry.source, Source::Unstaged | Source::Staged) {
                    return Task::none();
                }
                self.act(move |repo| async move {
                    git::rollback_block(&repo, &entry.change.path, &diff, block).await?;
                    Ok(String::new())
                })
            }
            GitMsg::OpenFile => {
                let Some(repo) = self.repo.clone() else {
                    return Task::none();
                };
                let Some(entry) = self.active_viewer().current() else {
                    return Task::none();
                };
                Task::done(Message::OpenRecentFile(repo.join(&entry.change.path)))
            }
            GitMsg::AcceptSide(path, yours) => {
                let rebasing = self
                    .status
                    .as_ref()
                    .is_some_and(|s| s.operation == Some(git::Operation::Rebase));
                self.act(move |repo| async move {
                    git::accept(&repo, &path, yours, rebasing).await?;
                    Ok(String::new())
                })
            }
            GitMsg::OpenMerge(path) => {
                let Some(repo) = self.repo.clone() else {
                    return Task::none();
                };
                let rebasing = self
                    .status
                    .as_ref()
                    .is_some_and(|s| s.operation == Some(git::Operation::Rebase));
                Task::perform(
                    async move {
                        // Rebuilt from the three versions (so each conflict knows
                        // its base) unless the file already has fewer conflicts
                        // than that: some were settled by hand.
                        let rebuilt = git::merge_text(&repo, &path).await;
                        let disk = tokio::fs::read_to_string(repo.join(&path)).await.ok();
                        let merge =
                            git::conflicts::choose(rebuilt.as_deref(), disk.as_deref(), rebasing)
                                .map_err(|why| match why {
                                git::conflicts::Unavailable::Settled => {
                                    t!("git.already_resolved").to_string()
                                }
                                git::conflicts::Unavailable::NotMergeable => {
                                    t!("git.not_mergeable").to_string()
                                }
                            })?;
                        Ok::<_, String>(MergeSpec { repo, path, merge })
                    },
                    |result| match result {
                        Ok(spec) => Message::OpenMergeTab(std::sync::Arc::new(spec)),
                        Err(error) => Message::Git(GitMsg::Reply(Err(error))),
                    },
                )
            }
            GitMsg::ContinueOperation => {
                let Some(op) = self.status.as_ref().and_then(|s| s.operation) else {
                    return Task::none();
                };
                self.act(move |repo| async move {
                    git::continue_operation(&repo, op).await?;
                    Ok(String::new())
                })
            }
            GitMsg::SkipOperation => {
                let Some(op) = self.status.as_ref().and_then(|s| s.operation) else {
                    return Task::none();
                };
                self.act(move |repo| async move {
                    git::skip_operation(&repo, op).await?;
                    Ok(String::new())
                })
            }
            GitMsg::AbortOperation => {
                let Some(op) = self.status.as_ref().and_then(|s| s.operation) else {
                    return Task::none();
                };
                let op_name = conflicts_view::operation_label(op);
                let asked = t!("git.confirm_abort", op = op_name.as_str()).to_string();
                if !self.confirmed(Pending::Abort, asked) {
                    return Task::none();
                }
                self.act(move |repo| async move {
                    git::abort_operation(&repo, op).await?;
                    Ok(String::new())
                })
            }
            GitMsg::MergeBranch(branch) => {
                let mode = self.merge_mode;
                self.act(move |repo| async move {
                    git::merge_branch(&repo, &branch, mode).await?;
                    Ok(String::new())
                })
            }
            GitMsg::RebaseOnto(branch) => self.act(move |repo| async move {
                git::rebase_onto(&repo, &branch).await?;
                Ok(String::new())
            }),
            GitMsg::DiffInEditor(index) => {
                let Some(repo) = self.repo.clone() else {
                    return Task::none();
                };
                let viewer = self.active_viewer();
                if index >= viewer.entries.len() {
                    return Task::none();
                }
                let title = match self.tab {
                    Tab::Stash => t!("git.tab_stash").to_string(),
                    _ => t!("git.tab_commit").to_string(),
                };
                let tab = DiffTab {
                    repo,
                    title,
                    entries: self.active_viewer().entries.clone(),
                    start: index,
                };
                Task::done(Message::OpenDiffTab(tab))
            }
            GitMsg::JumpToSource(index) => {
                let (Some(repo), Some(entry)) = (&self.repo, self.viewer.entries.get(index)) else {
                    return Task::none();
                };
                let path = repo.join(&entry.change.path);
                if path.is_file() {
                    Task::done(Message::OpenRecentFile(path))
                } else {
                    Task::none()
                }
            }
            GitMsg::MessageAction(action) => {
                self.typing = true;
                self.message.perform(action);
                Task::none()
            }
            GitMsg::MessageFocused => {
                self.typing = true;
                Task::none()
            }
            GitMsg::ToggleAmend(on) => {
                self.amend = on;
                if on && self.message.text().trim().is_empty() {
                    let Some(repo) = self.repo.clone() else {
                        return Task::none();
                    };
                    return Task::perform(async move { git::last_message(&repo).await }, |m| {
                        Message::Git(GitMsg::Reply(m.map(|m| format!("\u{1}{m}"))))
                    });
                }
                Task::none()
            }
            GitMsg::Commit(and_push) => {
                let text = self.message.text();
                let message = text.trim_end().to_string();
                let amend = self.amend;
                let only = self.only.clone();
                let options = self.commit_options.clone();
                if message.trim().is_empty() {
                    self.notice = Some((t!("git.empty_message").to_string(), true));
                    return Task::none();
                }
                let (branch, has_upstream) = self
                    .status
                    .as_ref()
                    .map(|s| (s.branch.clone(), s.upstream.is_some()))
                    .unwrap_or_default();
                // No branch to push (HEAD is detached): nothing is committed
                // either, so the user can choose.
                if and_push && branch.starts_with('(') {
                    self.notice = Some((t!("git.err_detached_push").to_string(), true));
                    return Task::none();
                }
                self.act(move |repo| async move {
                    match &only {
                        Some(path) => {
                            git::commit_only(&repo, &message, amend, path, &options).await?
                        }
                        None => git::commit(&repo, &message, amend, &options).await?,
                    }
                    if and_push {
                        // The commit is made: a push that fails says so, and
                        // the message box is emptied all the same.
                        match git::push(&repo, &branch, has_upstream).await {
                            Ok(()) => Ok("\u{2}pushed".to_string()),
                            Err(e) => Ok(format!("\u{5}{e}")),
                        }
                    } else {
                        Ok("\u{2}committed".to_string())
                    }
                })
            }
            GitMsg::SelectRef(r) => {
                // A second click on the one picked gives it up (to start a
                // branch from HEAD again).
                self.branches.selected = if self.branches.selected.as_ref() == Some(&r) {
                    None
                } else {
                    Some(r)
                };
                self.branches.confirm = None;
                Task::none()
            }
            GitMsg::RefDialog(kind) => {
                self.typing = kind.is_some();
                self.branches.dialog = kind;
                if kind == Some(branches::RefDialog::Rename) {
                    if let Some(branches::Ref::Branch(name)) = &self.branches.selected {
                        self.branches.name = name.clone();
                    }
                }
                if kind.is_some() {
                    return iced::widget::operation::focus(branches::ref_dialog_field());
                }
                Task::none()
            }
            GitMsg::RefDialogOk => {
                let action = match self.branches.dialog {
                    Some(branches::RefDialog::Branch) => branches::RefAction::Create,
                    Some(branches::RefDialog::Tag) => branches::RefAction::CreateTag,
                    Some(branches::RefDialog::Rename) => branches::RefAction::Rename,
                    None => return Task::none(),
                };
                if !self.can(action) {
                    return Task::none();
                }
                self.branches.dialog = None;
                self.typing = false;
                self.ref_action(action)
            }
            GitMsg::RefTagName(name) => {
                self.typing = true;
                self.branches.tag_name = name;
                self.branches.confirm = None;
                Task::none()
            }
            GitMsg::RefTagMessage(text) => {
                self.typing = true;
                self.branches.tag_message = text;
                Task::none()
            }
            GitMsg::AutoFetch => {
                let Some(repo) = self.repo.clone().filter(|_| !self.busy) else {
                    return Task::none();
                };
                Task::perform(async move { git::fetch(&repo).await.is_ok() }, |worked| {
                    Message::Git(GitMsg::AutoFetched(worked))
                })
            }
            GitMsg::AutoFetched(worked) => {
                if !worked || self.busy {
                    return Task::none();
                }
                // The branches and the counts of the remote look again.
                let more = match self.tab {
                    Tab::Log | Tab::Branches => self.load_refs(),
                    _ => Task::none(),
                };
                Task::batch([self.load_status(), more])
            }
            GitMsg::UndoCommit => {
                // A commit that is already on the remote: asks first.
                let pushed = self
                    .status
                    .as_ref()
                    .is_some_and(|s| s.upstream.is_some() && s.ahead == 0);
                if pushed
                    && !self.confirmed(
                        Pending::UndoCommit,
                        t!("git.confirm_undo_commit").to_string(),
                    )
                {
                    return Task::none();
                }
                self.act(move |repo| async move {
                    let message = git::undo_last_commit(&repo).await?;
                    Ok(format!("\u{8}{message}"))
                })
            }
            GitMsg::StashFile(index) => {
                let Some(entry) = self.viewer.entries.get(index).cloned() else {
                    return Task::none();
                };
                self.act(move |repo| async move {
                    git::stash_file(&repo, &entry.change.path, "").await?;
                    Ok(String::new())
                })
            }
            GitMsg::IgnoreFile(index) => {
                let Some(entry) = self.viewer.entries.get(index).cloned() else {
                    return Task::none();
                };
                self.act(move |repo| async move {
                    git::ignore_file(&repo, &entry.change.path).await?;
                    Ok("\u{a}ignored".to_string())
                })
            }
            GitMsg::RefName(name) => {
                self.typing = true;
                self.branches.name = name;
                self.branches.confirm = None;
                Task::none()
            }
            GitMsg::RefAction(action) => self.ref_action(action),
            GitMsg::OpenSplit(which, at) => {
                self.branches.menu = Some((which, at));
                Task::none()
            }
            GitMsg::Noop => Task::none(),
            // (anything that isn't an opening already closed them)
            GitMsg::CloseSplit => Task::none(),
            GitMsg::OpenPush => self.tool_update(tools::ToolMsg::OpenPush(None), ctx),
            GitMsg::CancelOnly => {
                self.only = None;
                Task::none()
            }
            GitMsg::Tool(msg) => self.tool_update(msg, ctx),
            GitMsg::CommitOptionsShown => {
                self.commit_options_shown = !self.commit_options_shown;
                Task::none()
            }
            GitMsg::CommitSignoff(on) => {
                self.commit_options.signoff = on;
                Task::none()
            }
            GitMsg::CommitNoVerify(on) => {
                self.commit_options.no_verify = on;
                Task::none()
            }
            GitMsg::CommitAuthor(author) => {
                self.typing = true;
                self.commit_options.author = author;
                Task::none()
            }
            GitMsg::CompareBranch(name) => self.compare_branch(name),
            GitMsg::CompareSwap => self.compare_swap(),
            GitMsg::CompareClear => {
                self.log.compare = None;
                self.show_log("HEAD")
            }
            GitMsg::LogFiltersShown => {
                self.log.filters_shown = !self.log.filters_shown;
                Task::none()
            }
            GitMsg::LogFilterText(field, value) => {
                self.typing = true;
                self.log.set_filter_text(field, value);
                let rev = self.log.shown.clone().unwrap_or_else(|| "HEAD".into());
                self.show_log(&rev)
            }
            GitMsg::LogFilterFlag(field, on) => {
                self.log.set_filter_flag(field, on);
                let rev = self.log.shown.clone().unwrap_or_else(|| "HEAD".into());
                self.show_log(&rev)
            }
            GitMsg::LogFilterClear => {
                let lines = self.log.narrow.lines.take();
                self.log.narrow = Default::default();
                self.log.narrow.lines = lines;
                let rev = self.log.shown.clone().unwrap_or_else(|| "HEAD".into());
                self.show_log(&rev)
            }
            GitMsg::TickCommitFile(path, on) => {
                if on {
                    self.log.ticked.insert(path);
                } else {
                    self.log.ticked.remove(&path);
                }
                Task::none()
            }
            GitMsg::ClearLogFile => {
                self.log.file = None;
                self.log.narrow.lines = None;
                let rev = self.log.shown.clone().unwrap_or_else(|| "HEAD".into());
                self.show_log(&rev)
            }
            GitMsg::Init => {
                let Some(dir) = self.init_dir.clone() else {
                    return Task::none();
                };
                self.busy = true;
                Task::perform(
                    async move { git::run(&dir, &["init", "-q"]).await.map(|_| String::new()) },
                    |result| Message::Git(GitMsg::Done(result)),
                )
            }
            GitMsg::Reply(result) => {
                let busy = self.busy;
                let task = self.update(GitMsg::Done(result), ctx);
                self.busy = busy;
                task
            }
            GitMsg::Done(result) => {
                self.busy = false;
                self.branches.confirm = None;
                self.pending = None;
                let mut opened: Option<Message> = None;
                match result {
                    Ok(text) => {
                        if let Some(old) = text.strip_prefix('\u{1}') {
                            // The last message, for an amend.
                            self.message = text_editor::Content::with_text(old);
                            return Task::none();
                        }
                        match text.as_str() {
                            "\u{2}committed" | "\u{2}pushed" => {
                                self.only = None;
                                self.message = text_editor::Content::new();
                                self.amend = false;
                                self.notice = Some((
                                    if text == "\u{2}pushed" {
                                        t!("git.committed_pushed").to_string()
                                    } else {
                                        t!("git.committed").to_string()
                                    },
                                    false,
                                ));
                            }
                            failed if failed.starts_with('\u{5}') => {
                                self.only = None;
                                self.message = text_editor::Content::new();
                                self.amend = false;
                                if git::tools::is_rejected(failed) {
                                    self.push_refused();
                                }
                                self.notice = Some((
                                    t!("git.commit_push_failed", error = &failed[1..]).to_string(),
                                    true,
                                ));
                            }
                            created if created.starts_with('\u{4}') => {
                                self.notice = Some((
                                    t!("git.branch_created", name = &created[1..]).to_string(),
                                    false,
                                ));
                            }
                            unmerged if unmerged.starts_with('\u{d}') => {
                                // Git kept a branch that is not merged: the
                                // next click on Delete forces it.
                                let name = unmerged[1..].to_string();
                                self.notice = Some((
                                    t!("git.branch_not_merged", name = name.as_str()).to_string(),
                                    true,
                                ));
                                self.branches.selected = Some(branches::Ref::Branch(name.clone()));
                                self.branches.force_delete = Some(name);
                                self.branches.confirm = Some(branches::RefAction::Delete);
                            }
                            "\u{7}fetched" => {
                                self.notice = Some((t!("git.fetched").to_string(), false));
                            }
                            "\u{a}ignored" => {
                                self.notice = Some((t!("git.ignored").to_string(), false));
                            }
                            tag if tag.starts_with('\u{9}') => {
                                self.notice = Some((
                                    t!("git.tag_created", name = &tag[1..]).to_string(),
                                    false,
                                ));
                            }
                            undone if undone.starts_with('\u{8}') => {
                                // The commit is back in the index; its message in the box.
                                self.message = text_editor::Content::with_text(&undone[1..]);
                                self.notice = Some((t!("git.commit_undone").to_string(), false));
                            }
                            "\u{6}pulled" => {
                                self.notice = Some((t!("git.pulled").to_string(), false));
                            }
                            "\u{3}pushed" => {
                                self.notice = Some((t!("git.pushed").to_string(), false));
                            }
                            "\u{b}patch" => {
                                self.notice = Some((t!("git.patch_saved").to_string(), false));
                            }
                            "\u{c}applied" => {
                                self.notice = Some((t!("git.patch_applied").to_string(), false));
                            }
                            _ => self.notice = None,
                        }
                    }
                    // Conflicts are not a failure: they are to be settled.
                    Err(error) if error == git::CONFLICT => {
                        self.notice = Some((t!("git.conflicts_to_settle").to_string(), false));
                        opened = Some(Message::OpenConflicts);
                    }
                    Err(error) => {
                        if git::tools::is_rejected(&error) {
                            self.push_refused();
                        }
                        self.notice = Some((error, true));
                    }
                }
                self.last_push = None;
                // Look again at once, whatever the tab shows.
                let more = match self.tab {
                    Tab::Commit => match self.repo.clone() {
                        // The open file changed under its diff: looked at
                        // quietly, so it doesn't blink.
                        Some(repo) => self.viewer.refresh(&repo),
                        None => Task::none(),
                    },
                    Tab::Stash => self.load_stashes(),
                    Tab::Log => {
                        let rev = self.log.shown.clone().unwrap_or_else(|| "HEAD".into());
                        Task::batch([self.load_refs(), self.show_log(&rev)])
                    }
                    Tab::Branches => self.load_refs(),
                };
                let dialog = opened.map_or_else(Task::none, Task::done);
                Task::batch([self.load_status(), more, dialog])
            }
            GitMsg::StashesLoaded(repo, _) | GitMsg::RefsLoaded(repo, _)
                if self.repo.as_ref() != Some(&repo) =>
            {
                Task::none()
            }
            GitMsg::StashesLoaded(_, result) => self.stashes_loaded(result),
            GitMsg::SelectStash(index) => self.select_stash(index),
            GitMsg::StashFilesLoaded(index, result) => self.stash_files_loaded(index, result),
            GitMsg::StashApply(index) => self.stash_action(index, stash::StashAction::Apply),
            GitMsg::StashPop(index) => self.stash_action(index, stash::StashAction::Pop),
            GitMsg::StashDrop(index) => {
                if !self.confirmed(
                    Pending::StashDrop(index),
                    t!("git.confirm_stash_drop").to_string(),
                ) {
                    return Task::none();
                }
                self.stash_action(index, stash::StashAction::Drop)
            }
            GitMsg::StashMessage(text) => {
                self.typing = true;
                self.stash.message = text;
                Task::none()
            }
            GitMsg::StashPush => self.stash_push(),
            GitMsg::RefsLoaded(_, result) => self.refs_loaded(result),
            GitMsg::BranchClicked(name) => self.branch_clicked(name),
            GitMsg::ToggleGroup(key) => {
                if !self.log.folded.remove(&key) {
                    self.log.folded.insert(key);
                }
                Task::none()
            }
            GitMsg::BranchFilter(text) => {
                self.typing = true;
                self.log.filter = text;
                Task::none()
            }
            GitMsg::LogLoaded(token, rev, result) => self.log_loaded(token, rev, result),
            GitMsg::SelectCommit(index) => self.select_commit(index, ctx.ctrl, ctx.shift),
            GitMsg::SelectForMenu(index) => self.select_for_menu(index),
            GitMsg::CommitFilesLoaded(sha, result) => self.commit_files_loaded(sha, result),
            GitMsg::CommitInfoLoaded(sha, result) => {
                self.commit_info_loaded(sha, result);
                Task::none()
            }
            GitMsg::LoadMoreCommits => self.load_more_commits(),
            GitMsg::Checkout(name) => self.checkout(name),
            GitMsg::BranchDiff(name) => self.branch_diff(name),
            GitMsg::CommitFileDiff(index) => self.commit_file_diff(index),
            GitMsg::CopyHash => match self.log.selected_commits().first() {
                Some(commit) => iced::clipboard::write(commit.sha.clone()),
                None => Task::none(),
            },
            GitMsg::Rewrite(action) => self.rewrite(action),
            GitMsg::RewriteReady(action, pushed, text) => {
                self.rewrite_ready(action, pushed, text);
                Task::none()
            }
            GitMsg::GuardReady(then, pushed) => self.guard_ready(then, pushed),
            GitMsg::DialogMessage(action) => {
                if let Some(log::Dialog::Message { content, .. }) = &mut self.log.dialog {
                    content.perform(action);
                }
                Task::none()
            }
            GitMsg::DialogOk => self.dialog_ok(),
            GitMsg::LogAction(action) => self.log_action(action),
            GitMsg::DialogName(text) => {
                if let Some(log::Dialog::Name { input, .. }) = &mut self.log.dialog {
                    *input = text;
                }
                Task::none()
            }
            GitMsg::LogSearch(text) => {
                self.typing = true;
                self.log.search = text;
                Task::none()
            }
            GitMsg::LogSearchMode(mode) => {
                self.log.search_mode = mode;
                if self.log.search.trim().is_empty() {
                    return Task::none();
                }
                let rev = self.log.shown.clone().unwrap_or_else(|| "HEAD".into());
                self.show_log(&rev)
            }
            GitMsg::LogSearchRun => {
                let rev = self.log.shown.clone().unwrap_or_else(|| "HEAD".into());
                self.show_log(&rev)
            }
            GitMsg::RebasePlanReady(base, plan) => {
                self.rebase_plan_ready(base, plan);
                Task::none()
            }
            GitMsg::RebaseMove(index, up) => {
                self.rebase_move(index, up);
                Task::none()
            }
            GitMsg::RebaseSet(index, action) => {
                self.rebase_set(index, action);
                Task::none()
            }
            GitMsg::RebaseSubject(index, subject) => {
                self.typing = true;
                self.rebase_subject(index, subject);
                Task::none()
            }
            GitMsg::DialogCancel => {
                self.log.dialog = None;
                Task::none()
            }
            GitMsg::DismissNotice => {
                self.notice = None;
                Task::none()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry() -> Entry {
        Entry {
            source: Source::Unstaged,
            change: Change {
                path: "a.txt".into(),
                from: None,
                kind: Kind::Modified,
            },
            staged: false,
        }
    }

    fn diff(lines: &[&str]) -> FileDiff {
        let mut text = String::from("diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n");
        text.push_str(&format!("@@ -1,1 +1,{} @@\n keep\n", lines.len() + 1));
        for line in lines {
            text.push_str(&format!("+{line}\n"));
        }
        crate::git::parse_diff(&text).pop().unwrap()
    }

    fn viewer_on(first: FileDiff) -> Viewer {
        let mut viewer = Viewer {
            entries: vec![entry()],
            index: Some(0),
            ..Viewer::default()
        };
        viewer.token = 7;
        viewer.diff = Some(Ok(first));
        viewer
    }

    #[test]
    fn a_quiet_refresh_shows_the_new_diff_of_a_file_whose_status_did_not_change() {
        let mut viewer = viewer_on(diff(&["one"]));
        let _ = viewer.refresh(Path::new("/no/such/repo"));
        assert!(viewer.silent);
        let token = viewer.token;
        // The same file, edited again and saved: another diff.
        let _ = viewer.loaded(token, Ok(diff(&["one", "two", "three"])));
        assert_eq!(viewer.file_diff().unwrap().added(), 3);
        assert!(!viewer.silent);
    }

    #[test]
    fn a_quiet_refresh_that_finds_nothing_new_leaves_everything_alone() {
        let mut viewer = viewer_on(diff(&["one"]));
        viewer.change = 0;
        let _ = viewer.refresh(Path::new("/no/such/repo"));
        let token = viewer.token;
        let _ = viewer.loaded(token, Ok(diff(&["one"])));
        assert_eq!(viewer.file_diff().unwrap().added(), 1);
        assert_eq!(viewer.change, 0);
        // A stale answer (another request was made since) is ignored.
        let _ = viewer.refresh(Path::new("/no/such/repo"));
        let _ = viewer.loaded(token, Ok(diff(&["x", "y"])));
        assert_eq!(viewer.file_diff().unwrap().added(), 1);
    }

    #[test]
    fn a_quiet_refresh_that_finds_nothing_new_keeps_the_diff_revision() {
        let mut viewer = viewer_on(diff(&["one"]));
        let rev = viewer.diff_rev();
        let _ = viewer.refresh(Path::new("/no/such/repo"));
        let token = viewer.token;
        let _ = viewer.loaded(token, Ok(diff(&["one"])));
        assert_eq!(viewer.diff_rev(), rev, "same diff, same colors");
        let _ = viewer.refresh(Path::new("/no/such/repo"));
        let token = viewer.token;
        let _ = viewer.loaded(token, Ok(diff(&["one", "two"])));
        assert_ne!(viewer.diff_rev(), rev);
    }

    #[test]
    fn the_current_change_stays_valid_when_the_diff_shrinks() {
        let mut viewer = viewer_on(diff(&["one"]));
        viewer.change = 5;
        let _ = viewer.refresh(Path::new("/no/such/repo"));
        let token = viewer.token;
        let _ = viewer.loaded(token, Ok(diff(&["one", "two"])));
        assert_eq!(viewer.change, 0);
    }

    fn panel_with_branches() -> Panel {
        use crate::git::history::{Branch, Refs};
        let branch = |name: &str, current: bool, remote: bool| Branch {
            name: name.into(),
            sha: String::new(),
            upstream: None,
            current,
            remote,
            ahead: 0,
            behind: 0,
        };
        let mut panel = Panel {
            repo: Some(PathBuf::from("/tmp")),
            ..Default::default()
        };
        panel.log.refs = Some(Refs {
            branches: vec![
                branch("main", true, false),
                branch("topic", false, false),
                branch("origin/topic", false, true),
            ],
            tags: vec!["v1".into()],
        });
        panel.status = Some(Status {
            operation: None,
            branch: "main".into(),
            upstream: None,
            ahead: 0,
            behind: 0,
            staged: vec![],
            unstaged: vec![],
        });
        panel
    }

    #[test]
    fn what_can_be_done_depends_on_what_is_picked() {
        use branches::{Ref, RefAction::*};
        let mut panel = panel_with_branches();
        for action in [Checkout, Merge, Rebase, Push, ForcePush, Delete, Create] {
            assert!(!panel.can(action), "{action:?}: nothing picked");
        }
        panel.branches.selected = Some(Ref::Branch("main".into()));
        // The current branch can be pushed, not switched to, merged or deleted.
        assert!(panel.can(Push) && panel.can(ForcePush));
        assert!(!panel.can(Checkout) && !panel.can(Merge) && !panel.can(Delete));
        panel.branches.selected = Some(Ref::Branch("topic".into()));
        assert!(panel.can(Checkout) && panel.can(Merge) && panel.can(Rebase) && panel.can(Delete));
        // A remote branch can't be pushed from here, but can be deleted.
        panel.branches.selected = Some(Ref::Branch("origin/topic".into()));
        assert!(!panel.can(Push) && panel.can(Delete) && panel.can(Merge));
        // A tag can be checked out (detached), deleted or started from, not merged.
        panel.branches.selected = Some(Ref::Tag("v1".into()));
        assert!(panel.can(Delete) && panel.can(Checkout) && !panel.can(Merge));
        // A name is needed to create.
        panel.branches.name = "  ".into();
        assert!(!panel.can(Create));
        panel.branches.name = "my topic".into();
        assert!(panel.can(Create));
        // Nothing while something is running, nor during a merge.
        panel.busy = true;
        assert!(!panel.can(Create));
        panel.busy = false;
        panel.status.as_mut().unwrap().operation = Some(git::Operation::Merge);
        panel.branches.selected = Some(Ref::Branch("topic".into()));
        assert!(!panel.can(Merge) && !panel.can(Rebase) && panel.can(Checkout));
    }

    #[test]
    fn deleting_and_forcing_ask_twice() {
        use branches::{Ref, RefAction::*};
        let mut panel = panel_with_branches();
        panel.branches.selected = Some(Ref::Branch("topic".into()));
        let _ = panel.ref_action(Delete);
        assert_eq!(panel.branches.confirm, Some(Delete));
        assert!(!panel.busy, "the first click only asks");
        // Picking something else gives up.
        let ctx = Ctx {
            current_file: None,
            ctrl: false,
            shift: false,
            roots: &[],
            colors: test_colors(),
        };
        let _ = panel.update(GitMsg::SelectRef(Ref::Tag("v1".into())), &ctx);
        assert_eq!(panel.branches.confirm, None);
        let _ = panel.ref_action(ForcePush);
        assert_eq!(panel.branches.confirm, None, "a tag isn't pushed");
    }

    fn test_colors() -> &'static std::collections::BTreeMap<String, usize> {
        static COLORS: std::collections::BTreeMap<String, usize> =
            std::collections::BTreeMap::new();
        &COLORS
    }

    fn ctx() -> Ctx<'static> {
        Ctx {
            current_file: None,
            ctrl: false,
            shift: false,
            roots: &[],
            colors: test_colors(),
        }
    }

    #[test]
    fn a_repository_is_made_in_the_project_and_never_in_the_home_folder() {
        let home = dirs::home_dir().unwrap();
        let project = home.join("work/app");
        let roots = [project.clone()];
        // A file deep in the project: the project, not its folder.
        assert_eq!(
            init_target(&project.join("src/ui"), &roots),
            Some(project.clone())
        );
        assert_eq!(
            init_target(Path::new("/srv/site"), &[]),
            Some("/srv/site".into())
        );
        // Nothing open: the home folder, or above it, is never offered.
        assert_eq!(init_target(&home, &roots), None);
        assert_eq!(init_target(Path::new("/"), &[]), None);
        let mut panel = Panel::default();
        let _ = panel.update(GitMsg::Detected(Some(home), None), &ctx());
        assert!(panel.init_dir.is_none());
        let _ = panel.update(GitMsg::Init, &ctx());
        assert!(!panel.busy, "nothing to run");
    }

    #[test]
    fn a_second_click_on_the_one_picked_gives_it_up() {
        use branches::Ref;
        let mut panel = panel_with_branches();
        let topic = Ref::Branch("topic".into());
        let _ = panel.update(GitMsg::SelectRef(topic.clone()), &ctx());
        assert_eq!(panel.branches.selected, Some(topic.clone()));
        let _ = panel.update(GitMsg::SelectRef(topic), &ctx());
        assert_eq!(panel.branches.selected, None);
    }

    #[test]
    fn typing_a_name_keeps_the_shortcuts_of_the_editor_out() {
        let mut panel = panel_with_branches();
        let _ = panel.update(GitMsg::RefName("fea".into()), &ctx());
        assert!(panel.typing);
        let _ = panel.update(GitMsg::SelectTab(Tab::Commit), &ctx());
        assert!(!panel.typing);
        let _ = panel.update(GitMsg::StashMessage("wip".into()), &ctx());
        assert!(panel.typing);
        let _ = panel.update(GitMsg::BranchFilter("m".into()), &ctx());
        assert!(panel.typing);
    }

    #[test]
    fn giving_up_an_operation_and_dropping_a_stash_ask_twice() {
        let mut panel = panel_with_branches();
        panel.status.as_mut().unwrap().operation = Some(git::Operation::Rebase);
        let _ = panel.update(GitMsg::AbortOperation, &ctx());
        assert_eq!(panel.pending, Some(Pending::Abort));
        assert!(!panel.busy, "the first click only asks");
        // Something else in between: asking starts over.
        let _ = panel.update(GitMsg::StashDrop(0), &ctx());
        assert_eq!(panel.pending, Some(Pending::StashDrop(0)));
        let _ = panel.update(GitMsg::DismissNotice, &ctx());
        assert_eq!(panel.pending, None);
        // The second click of the same thing does it.
        let _ = panel.update(GitMsg::AbortOperation, &ctx());
        let _ = panel.update(GitMsg::AbortOperation, &ctx());
        assert!(panel.busy);
    }

    #[test]
    fn a_staged_file_that_is_also_modified_asks_before_it_is_thrown_away() {
        let mut panel = panel_with_branches();
        let change = |kind| Change {
            path: "a.txt".into(),
            from: None,
            kind,
        };
        let status = panel.status.as_mut().unwrap();
        status.staged = vec![change(Kind::Modified)];
        status.unstaged = vec![change(Kind::Modified)];
        let status = status.clone();
        panel.viewer.entries = Panel::commit_entries(&status);
        let _ = panel.update(GitMsg::Rollback(0), &ctx());
        assert_eq!(panel.pending, Some(Pending::Rollback("a.txt".into())));
        assert!(!panel.busy);
        let _ = panel.update(GitMsg::Rollback(0), &ctx());
        assert!(panel.busy);
        // A file that is only staged asks too, and so does an untracked one.
        let mut panel = panel_with_branches();
        let mut status = panel.status.clone().unwrap();
        status.staged = vec![change(Kind::Modified)];
        panel.viewer.entries = Panel::commit_entries(&status);
        panel.status = Some(status);
        let _ = panel.update(GitMsg::Rollback(0), &ctx());
        assert!(!panel.busy);
        assert_eq!(panel.pending, Some(Pending::Rollback("a.txt".into())));
        let _ = panel.update(GitMsg::Rollback(0), &ctx());
        assert!(panel.busy);
        let mut panel = panel_with_branches();
        let mut status = panel.status.clone().unwrap();
        status.unstaged = vec![change(Kind::Untracked)];
        panel.viewer.entries = Panel::commit_entries(&status);
        panel.status = Some(status);
        let _ = panel.update(GitMsg::Delete(0), &ctx());
        assert!(!panel.busy);
        assert_eq!(panel.pending, Some(Pending::Delete("a.txt".into())));
        let _ = panel.update(GitMsg::Delete(0), &ctx());
        assert!(panel.busy);
    }

    #[test]
    fn only_one_git_command_runs_at_a_time() {
        let mut panel = panel_with_branches();
        panel.status.as_mut().unwrap().operation = Some(git::Operation::Rebase);
        let _ = panel.update(GitMsg::ContinueOperation, &ctx());
        assert!(panel.busy);
        // Another one while it runs does nothing (it would find index.lock).
        let before = panel.notice.clone();
        let _ = panel.update(GitMsg::SkipOperation, &ctx());
        assert!(panel.busy);
        assert_eq!(panel.notice, before);
    }

    #[test]
    fn another_repository_starts_from_a_clean_panel() {
        let mut panel = panel_with_branches();
        panel.branches.selected = Some(branches::Ref::Tag("v1".into()));
        panel.only = Some("a.txt".into());
        let _ = panel.update(
            GitMsg::Detected(Some(PathBuf::from("/other")), Some(PathBuf::from("/other"))),
            &ctx(),
        );
        assert!(panel.log.refs.is_none());
        assert!(panel.branches.selected.is_none());
        assert!(panel.only.is_none());
        // Opened from a file's menu, it stays pinned until another file is in front.
        panel.show_repo(PathBuf::from("/b"), Some(PathBuf::from("/a/x.rs")));
        assert_eq!(panel.repo, Some(PathBuf::from("/b")));
        let a = PathBuf::from("/a/x.rs");
        let task_ctx = Ctx {
            current_file: Some(&a),
            ctrl: false,
            shift: false,
            roots: &[],
            colors: test_colors(),
        };
        let _ = panel.refresh(&task_ctx);
        assert!(panel.pinned.is_some(), "same file in front: still pinned");
        let b = PathBuf::from("/a/y.rs");
        let other_ctx = Ctx {
            current_file: Some(&b),
            ctrl: false,
            shift: false,
            roots: &[],
            colors: test_colors(),
        };
        let _ = panel.refresh(&other_ctx);
        assert!(panel.pinned.is_none(), "another file: the panel follows it");
    }

    #[test]
    fn a_status_that_keeps_the_open_file_leaves_its_diff_on_screen() {
        let mut viewer = viewer_on(diff(&["one", "two"]));
        let repo = PathBuf::from("/tmp");
        viewer.pick(DiffPointer::Press(1, true, 0));
        let mut other = entry();
        other.change.path = "b.txt".into();
        let _ = viewer.set_entries(&repo, vec![entry(), other]);
        // Still there while it is looked at again in the background.
        assert!(viewer.file_diff().is_some());
        assert_eq!(viewer.index, Some(0));
        // A different answer replaces it, and what was picked is dropped.
        let token = viewer.token;
        let _ = viewer.loaded(token, Ok(diff(&["one", "two", "three"])));
        assert!(viewer.selection.is_none());
        assert_eq!(viewer.file_diff().unwrap().added(), 3);
    }

    #[test]
    fn an_answer_for_another_viewer_is_not_taken() {
        let viewer = viewer_on(diff(&["one"]));
        let before = viewer.token;
        assert!(viewer.owns(before));
        assert!(!viewer.owns(before + 1));
    }

    #[test]
    fn the_chevron_opens_the_modes_and_the_button_runs_the_action() {
        use branches::{RefAction, Split};
        let mut panel = panel_with_branches();
        let _ = panel.update(GitMsg::OpenSplit(Split::Pull, iced::Point::ORIGIN), &ctx());
        assert_eq!(panel.branches.menu.map(|m| m.0), Some(Split::Pull));
        assert!(!panel.busy, "opening the modes runs nothing");
        // A click outside (or on the chevron again) closes them.
        let _ = panel.update(GitMsg::CloseSplit, &ctx());
        assert!(panel.branches.menu.is_none());
        // Another one takes the place of the first; anything else closes it,
        // a timer doesn't.
        let _ = panel.update(GitMsg::OpenSplit(Split::Pull, iced::Point::ORIGIN), &ctx());
        let _ = panel.update(GitMsg::OpenSplit(Split::Merge, iced::Point::ORIGIN), &ctx());
        assert_eq!(panel.branches.menu.map(|m| m.0), Some(Split::Merge));
        let _ = panel.update(GitMsg::Tick, &ctx());
        assert_eq!(panel.branches.menu.map(|m| m.0), Some(Split::Merge));
        let _ = panel.update(GitMsg::DismissNotice, &ctx());
        assert!(panel.branches.menu.is_none());
        // The button itself runs the action, as the mode of the settings says.
        let _ = panel.update(GitMsg::RefAction(RefAction::Pull), &ctx());
        assert!(panel.busy);
        // What can't be done now can't be run.
        let mut panel = panel_with_branches();
        panel.status.as_mut().unwrap().operation = Some(git::Operation::Merge);
        assert!(!panel.can(RefAction::Merge));
    }

    #[test]
    fn fetching_and_tags_are_actions_of_the_branches_tab() {
        use branches::{Ref, RefAction::*};
        let mut panel = panel_with_branches();
        assert!(panel.can(Fetch));
        // A tag: pushed or removed on origin (asks twice), made from it.
        assert!(!panel.can(PushTag) && !panel.can(DeleteRemoteTag));
        panel.branches.selected = Some(Ref::Tag("v1".into()));
        assert!(panel.can(PushTag) && panel.can(DeleteRemoteTag));
        let _ = panel.ref_action(DeleteRemoteTag);
        assert_eq!(panel.branches.confirm, Some(DeleteRemoteTag));
        assert!(!panel.busy);
        // Creating one needs a name.
        assert!(!panel.can(CreateTag));
        let _ = panel.update(GitMsg::RefTagName("v2".into()), &ctx());
        assert!(panel.typing && panel.can(CreateTag));
        let _ = panel.update(GitMsg::RefAction(CreateTag), &ctx());
        assert!(panel.busy);
        assert!(panel.branches.tag_name.is_empty(), "the field is emptied");
        // Fetching runs.
        let mut panel = panel_with_branches();
        let _ = panel.update(GitMsg::RefAction(Fetch), &ctx());
        assert!(panel.busy);
    }

    #[test]
    fn a_fetch_in_the_background_is_silent_and_never_during_another_action() {
        let mut panel = panel_with_branches();
        let _ = panel.update(GitMsg::AutoFetch, &ctx());
        assert!(!panel.busy, "it doesn't show as an action");
        // A fetch that failed (no network) changes nothing.
        let before = panel.notice.clone();
        let _ = panel.update(GitMsg::AutoFetched(false), &ctx());
        assert_eq!(panel.notice, before);
        // While something runs, it waits.
        panel.busy = true;
        let _ = panel.update(GitMsg::AutoFetch, &ctx());
        assert!(panel.busy);
    }

    #[test]
    fn taking_the_last_commit_back_asks_when_it_is_already_on_the_remote() {
        let mut panel = panel_with_branches();
        // No upstream: nothing was pushed, it just goes.
        let _ = panel.update(GitMsg::UndoCommit, &ctx());
        assert!(panel.busy);
        // Pushed (an upstream, nothing ahead): asks first.
        let mut panel = panel_with_branches();
        panel.status.as_mut().unwrap().upstream = Some("origin/main".into());
        let _ = panel.update(GitMsg::UndoCommit, &ctx());
        assert_eq!(panel.pending, Some(Pending::UndoCommit));
        assert!(!panel.busy);
        let _ = panel.update(GitMsg::UndoCommit, &ctx());
        assert!(panel.busy);
        // Its message comes back to the box.
        let _ = panel.update(GitMsg::Done(Ok("\u{8}the old message".into())), &ctx());
        assert_eq!(panel.message.text().trim_end(), "the old message");
    }

    fn panel_with_log(selected: &[usize]) -> Panel {
        let mut panel = panel_with_branches();
        let commit = |sha: &str, parent: Option<&str>| git::graph::Commit {
            sha: sha.into(),
            parents: parent.into_iter().map(str::to_string).collect(),
            author: "A".into(),
            email: "a@example.com".into(),
            time: 0,
            subject: format!("commit {sha}"),
            refs: vec![],
        };
        panel.log.commits = vec![
            commit("ccc", Some("bbb")),
            commit("bbb", Some("aaa")),
            commit("aaa", None),
        ];
        panel.log.head_set = ["ccc", "bbb", "aaa"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        panel.log.selected = selected.to_vec();
        panel
    }

    #[test]
    fn the_menu_of_a_commit_makes_branches_tags_resets_and_comparisons() {
        use log::{Dialog, LogAction, NameKind};
        // One commit: a name to type, a reset to confirm.
        let mut panel = panel_with_log(&[1]);
        let _ = panel.update(GitMsg::LogAction(LogAction::BranchHere), &ctx());
        assert!(matches!(
            &panel.log.dialog,
            Some(Dialog::Name { kind: NameKind::Branch, sha, .. }) if sha == "bbb"
        ));
        assert!(panel.typing);
        let _ = panel.update(GitMsg::DialogName("topic x".into()), &ctx());
        let _ = panel.update(GitMsg::DialogOk, &ctx());
        assert!(panel.busy, "the branch is made");
        // An empty name does nothing, and the dialog stays.
        let mut panel = panel_with_log(&[1]);
        let _ = panel.update(GitMsg::LogAction(LogAction::TagHere), &ctx());
        let _ = panel.update(GitMsg::DialogOk, &ctx());
        assert!(!panel.busy);
        assert!(matches!(panel.log.dialog, Some(Dialog::Name { .. })));
        // Reset asks first, whatever its mode.
        let mut panel = panel_with_log(&[1]);
        let _ = panel.update(
            GitMsg::LogAction(LogAction::Reset(git::ResetMode::Hard)),
            &ctx(),
        );
        assert!(matches!(
            panel.log.dialog,
            Some(Dialog::Reset {
                mode: git::ResetMode::Hard,
                ..
            })
        ));
        assert!(!panel.busy);
        let _ = panel.update(GitMsg::DialogOk, &ctx());
        assert!(panel.busy);
        // Checking out a commit needs exactly one.
        let mut panel = panel_with_log(&[0, 1]);
        let _ = panel.update(GitMsg::LogAction(LogAction::CheckoutHere), &ctx());
        assert!(!panel.busy);
        let mut panel = panel_with_log(&[2]);
        let _ = panel.update(GitMsg::LogAction(LogAction::CheckoutHere), &ctx());
        assert!(panel.busy);
        // Comparing the two needs two.
        let avail = panel_with_log(&[0, 2]).log.availability();
        assert!(avail.two && !avail.one && !avail.rebase);
        let avail = panel_with_log(&[1]).log.availability();
        assert!(avail.one && !avail.two && avail.rebase);
    }

    #[test]
    fn an_interactive_rebase_is_arranged_in_a_dialog_before_it_runs() {
        use git::history::{PlanAction, PlanItem};
        use log::Dialog;
        let item = |sha: &str, subject: &str| PlanItem {
            sha: sha.into(),
            action: PlanAction::Pick,
            subject: subject.into(),
            message: format!("{subject}\n\nbody of {sha}"),
        };
        let mut panel = panel_with_log(&[1]);
        let plan = vec![item("bbb", "second"), item("ccc", "third")];
        let _ = panel.update(
            GitMsg::RebasePlanReady(Some("aaa".into()), Ok(plan)),
            &ctx(),
        );
        let order = |panel: &Panel| -> Vec<String> {
            match &panel.log.dialog {
                Some(Dialog::Rebase { items, .. }) => items.iter().map(|i| i.sha.clone()).collect(),
                _ => vec![],
            }
        };
        assert_eq!(order(&panel), vec!["bbb", "ccc"]);
        // Moving: the edges stay.
        let _ = panel.update(GitMsg::RebaseMove(1, true), &ctx());
        assert_eq!(order(&panel), vec!["ccc", "bbb"]);
        let _ = panel.update(GitMsg::RebaseMove(0, true), &ctx());
        let _ = panel.update(GitMsg::RebaseMove(1, false), &ctx());
        assert_eq!(order(&panel), vec!["ccc", "bbb"]);
        // A reworded commit takes the subject typed and keeps its body.
        let _ = panel.update(GitMsg::RebaseSet(1, PlanAction::Reword), &ctx());
        let _ = panel.update(GitMsg::RebaseSubject(1, "better".into()), &ctx());
        let Some(Dialog::Rebase { items, .. }) = &panel.log.dialog else {
            panic!("the dialog is open");
        };
        assert_eq!(items[1].message, "better\n\nbody of bbb");
        // The first one folded into nothing: refused, and the dialog stays.
        let _ = panel.update(GitMsg::RebaseSet(0, PlanAction::Squash), &ctx());
        let _ = panel.update(GitMsg::DialogOk, &ctx());
        assert!(!panel.busy);
        assert!(panel.notice.as_ref().is_some_and(|n| n.1));
        assert!(matches!(panel.log.dialog, Some(Dialog::Rebase { .. })));
        // Fixed, it runs.
        let _ = panel.update(GitMsg::RebaseSet(0, PlanAction::Pick), &ctx());
        let _ = panel.update(GitMsg::DialogOk, &ctx());
        assert!(panel.busy);
        assert!(panel.log.dialog.is_none());
        // A plan that could not be read says why.
        let mut panel = panel_with_log(&[1]);
        let _ = panel.update(GitMsg::RebasePlanReady(None, Err("merges".into())), &ctx());
        assert!(panel.log.dialog.is_none());
        assert_eq!(panel.notice, Some(("merges".into(), true)));
    }

    #[test]
    fn the_log_search_keeps_the_keyboard_and_looks_again_when_asked() {
        let mut panel = panel_with_log(&[]);
        let _ = panel.update(GitMsg::LogSearch("fix".into()), &ctx());
        assert!(panel.typing);
        assert_eq!(panel.log.search, "fix");
        let _ = panel.update(
            GitMsg::LogSearchMode(git::history::SearchMode::Author),
            &ctx(),
        );
        assert_eq!(panel.log.search_mode, git::history::SearchMode::Author);
        // The log being searched shows in the shown rev.
        assert_eq!(panel.log.shown.as_deref(), Some("HEAD"));
    }

    #[test]
    fn staging_a_block_or_lines_goes_through_the_index_and_one_command_at_a_time() {
        let mut panel = panel_with_branches();
        panel.viewer = viewer_on(diff(&["added"]));
        panel.viewer.index = Some(0);
        // A block of the working tree's diff: staged.
        let _ = panel.update(GitMsg::StageBlock(0), &ctx());
        assert!(panel.busy);
        // Lines: nothing picked, nothing to do.
        panel.busy = false;
        let _ = panel.update(GitMsg::StageSelection, &ctx());
        assert!(!panel.busy);
        // A diff that can't be staged (a commit's) does nothing.
        panel.viewer.entries[0].source = Source::Commit("abc".into());
        let _ = panel.update(GitMsg::StageBlock(0), &ctx());
        assert!(!panel.busy);
        // Blanks left out and more context: the same file, read again.
        let _ = panel.update(GitMsg::ToggleBlanks, &ctx());
        assert!(panel.viewer.ignore_blanks);
        let _ = panel.update(GitMsg::CycleContext, &ctx());
        assert_eq!(panel.viewer.context, 10);
        assert!(panel.viewer.file_diff().is_none(), "being read again");
    }

    #[test]
    fn a_branch_or_a_tag_is_named_in_a_popin() {
        use branches::RefDialog;
        let mut panel = panel_with_branches();
        let _ = panel.update(GitMsg::RefDialog(Some(RefDialog::Tag)), &ctx());
        assert_eq!(panel.branches.dialog, Some(RefDialog::Tag));
        assert!(panel.typing);
        // Without a name, OK does nothing and the popin stays.
        let _ = panel.update(GitMsg::RefDialogOk, &ctx());
        assert!(!panel.busy && panel.branches.dialog.is_some());
        let _ = panel.update(GitMsg::RefTagName("v3".into()), &ctx());
        let _ = panel.update(GitMsg::RefDialogOk, &ctx());
        assert!(panel.busy && panel.branches.dialog.is_none() && !panel.typing);
        // Cancelling closes it.
        let mut panel = panel_with_branches();
        let _ = panel.update(GitMsg::RefDialog(Some(RefDialog::Branch)), &ctx());
        let _ = panel.update(GitMsg::RefDialog(None), &ctx());
        assert!(panel.branches.dialog.is_none() && !panel.typing);
    }

    fn branch(name: &str, remote: bool) -> git::history::Branch {
        git::history::Branch {
            name: name.into(),
            sha: String::new(),
            remote,
            current: false,
            upstream: None,
            ahead: 0,
            behind: 0,
        }
    }

    fn with_refs() -> Panel {
        let mut panel = Panel {
            repo: Some(PathBuf::from("/nowhere")),
            status: Some(Status {
                branch: "main".into(),
                upstream: Some("origin/main".into()),
                ..Status::default()
            }),
            ..Panel::default()
        };
        panel.log.refs = Some(git::history::Refs {
            branches: vec![branch("topic", false), branch("origin/x", true)],
            tags: Vec::new(),
        });
        panel
    }

    #[test]
    fn a_local_branch_can_be_renamed_with_its_name_ready() {
        let mut panel = with_refs();
        assert!(!panel.can(branches::RefAction::Rename));
        panel.branches.selected = Some(branches::Ref::Branch("topic".into()));
        let _ = panel.update(GitMsg::RefDialog(Some(branches::RefDialog::Rename)), &ctx());
        assert_eq!(panel.branches.name, "topic");
        assert!(panel.can(branches::RefAction::Rename));
        // Not a remote branch.
        panel.branches.selected = Some(branches::Ref::Branch("origin/x".into()));
        assert!(!panel.can(branches::RefAction::Rename));
    }

    #[test]
    fn a_branch_is_compared_with_the_current_one_both_ways() {
        let mut panel = with_refs();
        let _ = panel.update(GitMsg::CompareBranch("topic".into()), &ctx());
        assert_eq!(panel.tab, Tab::Log);
        assert_eq!(panel.log.shown.as_deref(), Some("main..topic"));
        let _ = panel.update(GitMsg::CompareSwap, &ctx());
        assert_eq!(panel.log.shown.as_deref(), Some("topic..main"));
        let _ = panel.update(GitMsg::CompareClear, &ctx());
        assert_eq!(panel.log.compare, None);
        assert_eq!(panel.log.shown.as_deref(), Some("HEAD"));
    }

    #[test]
    fn a_refused_push_asks_how_to_catch_up() {
        let mut panel = with_refs();
        panel.busy = true;
        let refused = "! [rejected] main -> main (fetch first)".to_string();
        let _ = panel.update(GitMsg::Done(Err(refused)), &ctx());
        let Some(tools::Dialog::Rejected(spec)) = &panel.dialog else {
            panic!("the dialog");
        };
        assert_eq!(
            (spec.remote.as_str(), spec.target.as_str()),
            ("origin", "main")
        );
        // Any other failure only says so.
        let mut panel = with_refs();
        let _ = panel.update(GitMsg::Done(Err("no network".into())), &ctx());
        assert_eq!(panel.dialog, None);
    }

    #[test]
    fn force_in_the_push_dialog_asks_twice() {
        let mut panel = with_refs();
        panel.dialog = Some(tools::Dialog::Push(tools::PushDialog {
            remotes: vec!["origin".into()],
            spec: git::tools::PushSpec {
                remote: "origin".into(),
                source: "main".into(),
                target: "main".into(),
                ..Default::default()
            },
            ..Default::default()
        }));
        let force = || GitMsg::Tool(tools::ToolMsg::PushRun(true));
        let _ = panel.update(force(), &ctx());
        assert!(
            matches!(&panel.dialog, Some(tools::Dialog::Push(d)) if d.force_asked),
            "first click: only asked"
        );
        let _ = panel.update(force(), &ctx());
        assert_eq!(panel.dialog, None);
        assert!(panel.last_push.as_ref().is_some_and(|s| s.force));
    }

    #[test]
    fn filters_leave_only_what_acts_on_single_commits() {
        let mut panel = with_refs();
        let commit = |sha: &str, parent: &str| git::graph::Commit {
            sha: sha.into(),
            parents: vec![parent.into()],
            author: String::new(),
            email: String::new(),
            time: 0,
            subject: String::new(),
            refs: Vec::new(),
        };
        panel.log.commits = vec![commit("b", "a"), commit("a", "0")];
        panel.log.head_set = ["a".to_string(), "b".to_string()].into();
        panel.log.selected = vec![0];
        assert!(panel.log.availability().drop);
        let flag = GitMsg::LogFilterFlag(log::FilterField::NoMerges, true);
        let _ = panel.update(flag, &ctx());
        assert!(!panel.log.availability().drop, "the list skips commits");
        let _ = panel.update(GitMsg::LogFilterClear, &ctx());
        assert!(panel.log.availability().drop);
    }

    #[test]
    fn a_repository_picked_among_the_projects_stays_until_given_up() {
        let mut panel = Panel::default();
        panel.show_repo(PathBuf::from("/a"), None);
        panel.stick_to_repo();
        let file = PathBuf::from("/b/file.rs");
        let other = Ctx {
            current_file: Some(&file),
            ctrl: false,
            shift: false,
            roots: &[],
            colors: test_colors(),
        };
        let _ = panel.refresh(&other);
        assert_eq!(panel.repo, Some(PathBuf::from("/a")));
        assert!(panel.pinned.is_some());
        panel.unpin();
        assert!(panel.pinned.is_none());
    }

    #[test]
    fn files_of_a_commit_are_those_ticked_and_the_one_clicked_with_old_names() {
        let mut panel = with_refs();
        let change = |path: &str, from: Option<&str>| Change {
            path: path.into(),
            from: from.map(str::to_string),
            kind: Kind::Modified,
        };
        panel.log.files = vec![
            change("a", None),
            change("b", Some("old-b")),
            change("c", None),
        ];
        panel.log.files_for = Some("sha".into());
        let _ = panel.update(GitMsg::TickCommitFile("c".into(), true), &ctx());
        // The check for a pushed commit is asynchronous: what runs once it
        // has answered is what opens the dialog.
        let _ = panel.run_commit_file_action(tools::CommitFile::Extract, 1);
        let Some(tools::Dialog::Extract { paths, sha, .. }) = &panel.dialog else {
            panic!("the dialog");
        };
        assert_eq!(sha, "sha");
        assert_eq!(paths, &["b", "old-b", "c"]);
    }

    #[test]
    fn a_recent_message_goes_into_the_box() {
        let mut panel = with_refs();
        let list = Ok(vec!["one\n\nbody".to_string()]);
        let _ = panel.update(GitMsg::Tool(tools::ToolMsg::MessagesLoaded(list)), &ctx());
        assert!(matches!(panel.dialog, Some(tools::Dialog::Messages(_))));
        let pick = tools::ToolMsg::UseMessage("one\n\nbody".into());
        let _ = panel.update(GitMsg::Tool(pick), &ctx());
        assert_eq!(panel.dialog, None);
        assert_eq!(panel.message.text().trim_end(), "one\n\nbody");
    }

    #[test]
    fn a_conflicted_file_is_not_staged_while_it_still_has_its_markers() {
        let dir = tempfile::tempdir().unwrap();
        let mut panel = panel_with_branches();
        panel.repo = Some(dir.path().to_path_buf());
        let change = |path: &str, kind| Change {
            path: path.into(),
            from: None,
            kind,
        };
        let status = panel.status.as_mut().unwrap();
        status.unstaged = vec![change("both.txt", Kind::Conflict)];
        let status = status.clone();
        panel.viewer.entries = Panel::commit_entries(&status);
        std::fs::write(
            dir.path().join("both.txt"),
            "<<<<<<< HEAD\nx\n=======\ny\n>>>>>>> t\n",
        )
        .unwrap();
        // The heading's checkbox leaves conflicts out.
        let _ = panel.update(GitMsg::StageAll(true), &ctx());
        assert!(!panel.busy, "nothing else to stage");
        // Its own checkbox refuses, and says why.
        let _ = panel.update(GitMsg::ToggleStaged(0), &ctx());
        assert!(!panel.busy);
        assert!(panel.notice.as_ref().is_some_and(|(_, error)| *error));
        // Settled by hand: staging it is how it is marked settled.
        std::fs::write(dir.path().join("both.txt"), "x\ny\n").unwrap();
        let _ = panel.update(GitMsg::ToggleStaged(0), &ctx());
        assert!(panel.busy);
    }

    #[test]
    fn an_unmerged_branch_is_deleted_only_by_one_more_click_right_after_the_refusal() {
        use branches::{Ref, RefAction::Delete};
        let mut panel = panel_with_branches();
        panel.busy = true;
        // Git kept `topic`: it is picked again and Delete waits to force it.
        let _ = panel.update(GitMsg::Done(Ok("\u{d}topic".into())), &ctx());
        assert_eq!(panel.branches.selected, Some(Ref::Branch("topic".into())));
        assert_eq!(panel.branches.confirm, Some(Delete));
        assert_eq!(panel.branches.force_delete.as_deref(), Some("topic"));
        assert!(panel.notice.as_ref().is_some_and(|(_, error)| *error));
        let _ = panel.ref_action(Delete);
        assert!(panel.busy);
        assert_eq!(panel.branches.force_delete, None);
        // Anything else in between: the next Delete asks afresh, unforced.
        let mut panel = panel_with_branches();
        panel.busy = true;
        let _ = panel.update(GitMsg::Done(Ok("\u{d}topic".into())), &ctx());
        let _ = panel.update(GitMsg::SelectRef(Ref::Branch("topic".into())), &ctx());
        let _ = panel.update(GitMsg::SelectRef(Ref::Branch("topic".into())), &ctx());
        let _ = panel.ref_action(Delete);
        assert!(!panel.busy, "the first click only asks");
        assert_eq!(panel.branches.force_delete, None);
    }
}
