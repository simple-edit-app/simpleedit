//! The Git panel: Commit, Stash and Log tabs at the bottom of the window.
//!
//! [`Panel`] holds what the tabs show; [`GitMsg`] is what happens to it. The
//! work itself is done by [`crate::git`], always off the interface thread:
//! each action is a task that answers with a message.

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
            side_by_side: false,
            land: None,
            silent: false,
            token: 0,
            scroll: iced::widget::Id::unique(),
        }
    }
}

impl Viewer {
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

    /// Opens an entry: its diff is asked for, the answer comes as
    /// [`GitMsg::DiffLoaded`].
    pub fn open(&mut self, repo: &Path, index: usize, land: Land) -> Task<Message> {
        let Some(entry) = self.entries.get(index) else {
            return Task::none();
        };
        self.index = Some(index);
        self.diff = None;
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
                    // The content may have changed with the list.
                    self.reload(repo)
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
        if self.entries.is_empty() {
            Task::none()
        } else {
            self.open(repo, 0, Land::First)
        }
    }

    pub fn reload(&mut self, repo: &Path) -> Task<Message> {
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
        match self.change_row() {
            Some(row) => iced::widget::operation::scroll_to(
                self.scroll.clone(),
                iced::widget::scrollable::AbsoluteOffset {
                    x: 0.0,
                    y: ((row as f32 - 3.0) * diff_view::ROW_HEIGHT).max(0.0),
                },
            ),
            None => iced::widget::operation::scroll_to(
                self.scroll.clone(),
                iced::widget::scrollable::AbsoluteOffset { x: 0.0, y: 0.0 },
            ),
        }
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
    pub busy: bool,
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
            busy: false,
            notice: None,
        }
    }
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
    // The commit list
    ToggleStaged(usize),
    StageAll(bool),
    Rollback(usize),
    Delete(usize),
    JumpToSource(usize),
    DiffInEditor(usize),
    /// Puts one block of changes of the open file back as it was.
    RollbackBlock(usize),
    // Conflicts
    /// Keep your side (`true`) or theirs of a conflicted file.
    AcceptSide(String, bool),
    /// Settle a conflicted file by hand, in three columns.
    OpenMerge(String),
    ContinueOperation,
    AbortOperation,
    MergeBranch(String),
    RebaseOnto(String),
    // Committing
    MessageAction(text_editor::Action),
    MessageFocused,
    ToggleAmend(bool),
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
        let more = match (self.tab, repo) {
            (Tab::Stash, _) => self.load_stashes(),
            (Tab::Commit, Some(repo)) => self.viewer.refresh(&repo),
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
        let dir = ctx
            .current_file
            .filter(|p| !crate::app::is_untitled(p))
            .and_then(Path::parent)
            .map(Path::to_path_buf)
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
        Task::perform(async move { git::status(&repo).await }, |status| {
            Message::Git(GitMsg::StatusLoaded(status))
        })
    }

    /// The file list of the Commit tab: staged first, then the rest.
    fn commit_entries(status: &Status) -> Vec<Entry> {
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

    /// Runs `work` on the repository, then looks again.
    fn act<F, Fut>(&mut self, work: F) -> Task<Message>
    where
        F: FnOnce(PathBuf) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<String, String>> + Send + 'static,
    {
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
                | GitMsg::BranchClicked(_)
                | GitMsg::SelectCommit(_)
                | GitMsg::NextChange
                | GitMsg::PrevChange
                | GitMsg::NextFile
                | GitMsg::PrevFile
        ) {
            self.typing = false;
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
                    self.status = None;
                    self.viewer = Viewer::default();
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
                    Tab::Log => self.load_refs(),
                }
            }
            GitMsg::SelectFile(index) => match self.repo.clone() {
                Some(repo) => self.active_viewer().open(&repo, index, Land::First),
                None => Task::none(),
            },
            GitMsg::DiffLoaded(token, result) => Task::batch([
                self.viewer.loaded(token, result.clone()),
                self.stash.viewer.loaded(token, result),
            ]),
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
                viewer.scroll_to_change()
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
                if entry.source != Source::Unstaged {
                    return Task::none();
                }
                self.act(move |repo| async move {
                    git::rollback_block(&repo, &entry.change.path, &diff, block).await?;
                    Ok(String::new())
                })
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
                        // its base); the file's own markers if there are none.
                        let rebuilt = git::merge_text(&repo, &path).await;
                        let merge = rebuilt
                            .and_then(|text| git::conflicts::parse(&text, rebasing))
                            .or_else(|| {
                                std::fs::read_to_string(repo.join(&path))
                                    .ok()
                                    .and_then(|text| git::conflicts::parse(&text, rebasing))
                            })
                            .ok_or_else(|| t!("git.not_mergeable").to_string())?;
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
            GitMsg::AbortOperation => {
                let Some(op) = self.status.as_ref().and_then(|s| s.operation) else {
                    return Task::none();
                };
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
                if message.trim().is_empty() {
                    self.notice = Some((t!("git.empty_message").to_string(), true));
                    return Task::none();
                }
                let (branch, has_upstream) = self
                    .status
                    .as_ref()
                    .map(|s| (s.branch.clone(), s.upstream.is_some()))
                    .unwrap_or_default();
                self.act(move |repo| async move {
                    git::commit(&repo, &message, amend).await?;
                    if and_push {
                        git::push(&repo, &branch, has_upstream).await?;
                        Ok("\u{2}pushed".to_string())
                    } else {
                        Ok("\u{2}committed".to_string())
                    }
                })
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
                        // The open file changed under its diff.
                        Some(repo) => self.viewer.reload(&repo),
                        None => Task::none(),
                    },
                    Tab::Stash => self.load_stashes(),
                    Tab::Log => {
                        let rev = self.log.shown.clone().unwrap_or_else(|| "HEAD".into());
                        Task::batch([self.load_refs(), self.show_log(&rev)])
                    }
                };
                let dialog = opened.map_or_else(Task::none, Task::done);
                Task::batch([self.load_status(), more, dialog])
            }
            GitMsg::StashesLoaded(result) => self.stashes_loaded(result),
            GitMsg::SelectStash(index) => self.select_stash(index),
            GitMsg::StashFilesLoaded(index, result) => self.stash_files_loaded(index, result),
            GitMsg::StashApply(index) => self.stash_action(index, stash::StashAction::Apply),
            GitMsg::StashPop(index) => self.stash_action(index, stash::StashAction::Pop),
            GitMsg::StashDrop(index) => self.stash_action(index, stash::StashAction::Drop),
            GitMsg::StashMessage(text) => {
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
}
