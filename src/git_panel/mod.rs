//! The Git panel: Commit, Stash and Log tabs at the bottom of the window.
//!
//! [`Panel`] holds what the tabs show; [`GitMsg`] is what happens to it. The
//! work itself is done by [`crate::git`], always off the interface thread:
//! each action is a task that answers with a message.

pub mod branches;
pub mod conflicts_view;
pub mod diff_view;
pub mod icon;
pub mod log;
pub mod log_view;
pub mod merge;
pub mod stash;
pub mod stash_view;
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
    /// Where its diff scrolls.
    pub scroll: iced::widget::Id,
    /// How many characters a side of the side-by-side view holds on a line,
    /// as the view last saw it (0: not known yet); long lines wrap there.
    pub columns: std::cell::Cell<usize>,
    /// How far down the diff is scrolled and how tall it shows: only the rows
    /// around that are built.
    pub view_top: std::cell::Cell<f32>,
    pub view_height: std::cell::Cell<f32>,
    /// The rows picked with the mouse (to copy them).
    pub selection: Option<Selection>,
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
            scroll: iced::widget::Id::unique(),
            columns: std::cell::Cell::new(0),
            view_top: std::cell::Cell::new(0.0),
            // Until the scrollable says, a tall screen's worth.
            view_height: std::cell::Cell::new(1400.0),
            selection: None,
            dragging: false,
            last_press: None,
        }
    }
}

impl Viewer {
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
            (true, Some(diff)) => diff_view::side_top(diff, row, self.columns.get()),
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
        let source = entry.source.clone();
        let path = entry.change.path.clone();
        Task::perform(
            async move { git::diff_of(&repo, &source, &path).await },
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
        Task::perform(
            async move { git::diff_of(&repo, &entry.source, &entry.change.path).await },
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
            // What was picked may be other lines now.
            self.selection = None;
            self.change = self.change.min(self.changes().saturating_sub(1));
            return Task::none();
        }
        self.diff = Some(result);
        let count = self.changes();
        match self.land.take() {
            Some(Land::Last) => self.change = count.saturating_sub(1),
            _ => self.change = self.change.min(count.saturating_sub(1)),
        }
        self.scroll_to_change()
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
        self.next_file(repo, Land::First)
    }

    /// Up: the previous change, or the last of the previous file.
    pub fn prev_change(&mut self, repo: &Path) -> Task<Message> {
        if self.change > 0 {
            self.change -= 1;
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

/// The Git panel.
pub struct Panel {
    pub open: bool,
    pub tab: Tab,
    pub repo: Option<PathBuf>,
    /// The folder the repository was looked for from.
    pub(super) probed: Option<PathBuf>,
    /// Whether that folder is in a repository (false: offer `git init`).
    pub searched: bool,
    pub status: Option<Status>,
    pub viewer: Viewer,
    pub stash: stash::State,
    pub log: log::State,
    pub message: text_editor::Content,
    /// The message box has the keyboard: the app's shortcuts stay out.
    pub typing: bool,
    pub amend: bool,
    /// The commit is of this file only (from its Git menu).
    pub only: Option<String>,
    pub busy: bool,
    pub branches: branches::State,
    /// What was asked once and waits for a second click to be done.
    pub pending: Option<Pending>,
    /// A repository chosen by hand (from a file's Git menu), kept until
    /// another file comes to the front: the panel then follows that file.
    pinned: Option<Pinned>,
    /// What the last action said, and whether it failed.
    pub notice: Option<(String, bool)>,
}

impl Default for Panel {
    fn default() -> Self {
        Panel {
            open: false,
            tab: Tab::Commit,
            repo: None,
            probed: None,
            searched: false,
            status: None,
            viewer: Viewer::default(),
            stash: stash::State::default(),
            log: log::State::default(),
            message: text_editor::Content::new(),
            typing: false,
            amend: false,
            only: None,
            busy: false,
            branches: branches::State::default(),
            pending: None,
            pinned: None,
            notice: None,
        }
    }
}

/// Something that can't be undone, waiting for a second click.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pending {
    /// Throw away a staged file that also has changes that are not staged.
    Rollback(String),
    StashDrop(usize),
    Abort,
}

/// The folder a repository was chosen from, and the file in front then.
#[derive(Debug, Clone)]
struct Pinned {
    dir: PathBuf,
    file: Option<PathBuf>,
}

/// What the panel needs to know of the app.
pub struct Ctx<'a> {
    pub current_file: Option<&'a Path>,
    pub ctrl: bool,
    pub shift: bool,
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
    /// Time to look again (the panel is open).
    Tick,
    /// A file was saved: look again at once.
    Refresh,
    Detected(Option<PathBuf>, Option<PathBuf>),
    StatusLoaded(Result<Status, String>),
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
    RefAction(branches::RefAction),
    /// Back to the history of the whole branch.
    ClearLogFile,
    Commit(bool),
    Init,
    /// An action ended: what it said, or why it failed.
    Done(Result<String, String>),
    DismissNotice,
    // Stash
    StashesLoaded(Result<Vec<git::history::Stash>, String>),
    SelectStash(usize),
    StashFilesLoaded(usize, Result<Vec<Change>, String>),
    StashApply(usize),
    StashPop(usize),
    StashDrop(usize),
    StashMessage(String),
    StashPush,
    // Log
    RefsLoaded(Result<git::history::Refs, String>),
    BranchClicked(String),
    ToggleGroup(String),
    BranchFilter(String),
    LogLoaded(
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
    RewriteReady(log::Action, bool, String),
    DialogMessage(text_editor::Action),
    DialogOk,
    DialogCancel,
}

impl GitMsg {
    /// An answer or a timer, not something the user did: it must not close
    /// an open menu (the panel refreshes itself every two seconds).
    pub fn is_background(&self) -> bool {
        matches!(
            self,
            GitMsg::Tick
                | GitMsg::Refresh
                | GitMsg::Detected(..)
                | GitMsg::StatusLoaded(_)
                | GitMsg::DiffLoaded(..)
                | GitMsg::DiffScrolled(..)
                | GitMsg::LogScrolled(..)
                | GitMsg::StashesLoaded(_)
                | GitMsg::StashFilesLoaded(..)
                | GitMsg::RefsLoaded(_)
                | GitMsg::LogLoaded(..)
                | GitMsg::CommitFilesLoaded(..)
                | GitMsg::CommitInfoLoaded(..)
                | GitMsg::RewriteReady(..)
                | GitMsg::Done(_)
        )
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
            .is_some_and(|p| p.file.as_deref() != ctx.current_file)
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
            .or_else(dirs::home_dir);
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
        Task::perform(
            async move { git::status(&repo, untracked).await },
            |status| Message::Git(GitMsg::StatusLoaded(status)),
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
        });
        self.open = true;
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
                GitMsg::Rollback(_) | GitMsg::StashDrop(_) | GitMsg::AbortOperation
            )
        {
            self.pending = None;
        }
        match msg {
            GitMsg::Tick => {
                if self.busy {
                    return Task::none();
                }
                self.look_again(ctx)
            }
            GitMsg::Refresh => self.look_again(ctx),
            GitMsg::Detected(dir, root) => {
                self.probed = dir;
                self.searched = true;
                if root != self.repo {
                    self.reset_for_another_repo();
                }
                self.repo = root;
                self.load_status()
            }
            GitMsg::StatusLoaded(result) => {
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
                self.act(move |repo| async move {
                    let paths = [path];
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
                    status.unstaged.iter().map(|c| c.path.clone()).collect()
                } else {
                    status.staged.iter().map(|c| c.path.clone()).collect()
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
                // Rolling back what is staged also takes the file back to
                // HEAD on disk: changes not staged would go with it.
                let also_unstaged = entry.staged
                    && self
                        .status
                        .as_ref()
                        .is_some_and(|s| s.unstaged.iter().any(|c| c.path == entry.change.path));
                if also_unstaged
                    && !self.confirmed(
                        Pending::Rollback(entry.change.path.clone()),
                        t!("git.confirm_rollback_both").to_string(),
                    )
                {
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
                        Err(error) => Message::Git(GitMsg::Done(Err(error))),
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
            GitMsg::MergeBranch(branch) => self.act(move |repo| async move {
                git::merge_branch(&repo, &branch).await?;
                Ok(String::new())
            }),
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
                        Message::Git(GitMsg::Done(m.map(|m| format!("\u{1}{m}"))))
                    });
                }
                Task::none()
            }
            GitMsg::Commit(and_push) => {
                let text = self.message.text();
                let message = text.trim_end().to_string();
                let amend = self.amend;
                let only = self.only.clone();
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
                        Some(path) => git::commit_only(&repo, &message, amend, path).await?,
                        None => git::commit(&repo, &message, amend).await?,
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
            GitMsg::RefName(name) => {
                self.typing = true;
                self.branches.name = name;
                self.branches.confirm = None;
                Task::none()
            }
            GitMsg::RefAction(action) => self.ref_action(action),
            GitMsg::CancelOnly => {
                self.only = None;
                Task::none()
            }
            GitMsg::ClearLogFile => {
                self.log.file = None;
                let rev = self.log.shown.clone().unwrap_or_else(|| "HEAD".into());
                self.show_log(&rev)
            }
            GitMsg::Init => {
                let dir = self.probed.clone().or_else(|| {
                    ctx.current_file
                        .and_then(Path::parent)
                        .map(Path::to_path_buf)
                });
                let Some(dir) = dir else {
                    return Task::none();
                };
                self.busy = true;
                Task::perform(
                    async move { git::run(&dir, &["init", "-q"]).await.map(|_| String::new()) },
                    |result| Message::Git(GitMsg::Done(result)),
                )
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
                            "\u{3}pushed" => {
                                self.notice = Some((t!("git.pushed").to_string(), false));
                            }
                            _ => self.notice = None,
                        }
                    }
                    // Conflicts are not a failure: they are to be settled.
                    Err(error) if error == git::CONFLICT => {
                        self.notice = Some((t!("git.conflicts_to_settle").to_string(), false));
                        opened = Some(Message::OpenConflicts);
                    }
                    Err(error) => self.notice = Some((error, true)),
                }
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
            GitMsg::StashesLoaded(result) => self.stashes_loaded(result),
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
            GitMsg::RefsLoaded(result) => self.refs_loaded(result),
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
            GitMsg::LogLoaded(rev, result) => self.log_loaded(rev, result),
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
            GitMsg::DialogMessage(action) => {
                if let Some(log::Dialog::Message { content, .. }) = &mut self.log.dialog {
                    content.perform(action);
                }
                Task::none()
            }
            GitMsg::DialogOk => self.dialog_ok(),
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
        };
        let _ = panel.update(GitMsg::SelectRef(Ref::Tag("v1".into())), &ctx);
        assert_eq!(panel.branches.confirm, None);
        let _ = panel.ref_action(ForcePush);
        assert_eq!(panel.branches.confirm, None, "a tag isn't pushed");
    }

    fn ctx() -> Ctx<'static> {
        Ctx {
            current_file: None,
            ctrl: false,
            shift: false,
        }
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
        // A file that is only staged goes at once.
        let mut panel = panel_with_branches();
        let mut status = panel.status.clone().unwrap();
        status.staged = vec![change(Kind::Modified)];
        panel.viewer.entries = Panel::commit_entries(&status);
        panel.status = Some(status);
        let _ = panel.update(GitMsg::Rollback(0), &ctx());
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
        };
        let _ = panel.refresh(&task_ctx);
        assert!(panel.pinned.is_some(), "same file in front: still pinned");
        let b = PathBuf::from("/a/y.rs");
        let other_ctx = Ctx {
            current_file: Some(&b),
            ctrl: false,
            shift: false,
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
}
