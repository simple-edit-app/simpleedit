//! The Log tab: branches on the left, the commit graph, what a commit
//! changed, and what can be done to commits.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use iced::widget::text_editor;
use iced::Task;

use super::{DiffTab, Entry, GitMsg, Panel};
use crate::app::Message;
use crate::git::graph::{self, Commit, GraphRow};
use crate::git::history::{self, PlanAction, PlanItem, Refs, SearchMode};
use crate::git::{self};
use crate::git::{Change, Source};

/// How many commits a branch shows at first, and per "load more".
pub const PAGE: usize = 300;

/// A change to the history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Squash,
    Drop,
    Reword,
    Revert,
    CherryPick,
}

/// A question asked before a rewrite.
pub enum Dialog {
    /// A message to write (squash, reword).
    Message {
        action: Action,
        shas: Vec<String>,
        content: text_editor::Content,
        pushed: bool,
    },
    /// A rewrite of the history to confirm, with the warning when the
    /// commits are already pushed.
    Guard { then: Guarded, pushed: bool },
    /// A confirmation (drop).
    Confirm { shas: Vec<String>, pushed: bool },
    /// A name to type for a branch or a tag made at a commit.
    Name {
        kind: NameKind,
        sha: String,
        input: String,
    },
    /// A reset to a commit, to confirm.
    Reset { sha: String, mode: git::ResetMode },
    /// An interactive rebase, to arrange: the commits after `base`, oldest
    /// first, each with what to do with it.
    Rebase {
        base: Option<String>,
        items: Vec<PlanItem>,
    },
}

/// A rewrite that runs once the user has been told about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Guarded {
    Log(LogAction),
    File(super::tools::CommitFile, usize),
}

/// What a name typed in the log is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameKind {
    Branch,
    Tag,
}

/// What the menu of a commit does besides rewriting history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogAction {
    BranchHere,
    TagHere,
    CheckoutHere,
    Reset(git::ResetMode),
    /// The files that differ between that commit and HEAD / the other
    /// commit selected.
    CompareHead,
    CompareSelected,
    /// An interactive rebase of the commits from this one on.
    RebaseFrom,
    /// What is staged goes into this commit: its message stays (fixup), or
    /// the new one is added to it (squash).
    Fixup,
    SquashInto,
}

/// What a filter of the log narrows by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterField {
    Author,
    Since,
    Until,
    Path,
    FirstParent,
    NoMerges,
}

#[derive(Default)]
pub struct State {
    pub refs: Option<Refs>,
    pub filter: String,
    /// Folded groups of the branch tree.
    pub folded: HashSet<String>,
    /// The branch highlighted in the tree.
    pub branch: Option<String>,
    /// What the commit list shows: a branch, a tag, `HEAD`.
    pub shown: Option<String>,
    /// Only the commits that touched this file (the file's history).
    pub file: Option<String>,
    pub commits: Vec<Commit>,
    pub rows: Vec<GraphRow>,
    pub lanes: usize,
    /// Indices into `commits`, ascending.
    pub selected: Vec<usize>,
    anchor: Option<usize>,
    /// The commits of HEAD, to know what can be rewritten.
    pub head_set: HashSet<String>,
    pub limit: usize,
    /// The number of the last listing asked for: an older answer is dropped.
    pub request: u64,
    /// The files of the commit when exactly one is selected.
    pub files: Vec<Change>,
    pub files_for: Option<String>,
    pub message: String,
    pub contained_in: Vec<String>,
    last_click: Option<(String, Instant)>,
    pub dialog: Option<Dialog>,
    /// A commit to find and select once the log is there (from a blame).
    pub reveal: Option<Reveal>,
    /// How far down the commit list is scrolled and how tall it shows: only
    /// the rows around that are built.
    pub view_top: std::cell::Cell<f32>,
    pub view_height: std::cell::Cell<f32>,
    /// What the commits are searched for, and in what.
    pub search: String,
    pub search_mode: SearchMode,
    pub narrow: history::LogFilter,
    pub filters_shown: bool,
    /// Compared: the commits of `.1` that `.0` lacks.
    pub compare: Option<(String, String)>,
    /// The files of the commit ticked for an action on several.
    pub ticked: HashSet<String>,
}

/// A commit to bring up, and the file whose diff to open.
#[derive(Debug, Clone)]
pub struct Reveal {
    pub sha: String,
    pub path: Option<String>,
}

/// Where the commit list scrolls.
pub fn scroll_id() -> iced::widget::Id {
    iced::widget::Id::new("git-log-scroll")
}

/// What can be done with the selection.
#[derive(Debug, Clone, Copy, Default)]
pub struct Avail {
    pub squash: bool,
    pub drop: bool,
    pub reword: bool,
    pub revert: bool,
    pub cherry_pick: bool,
    /// One commit is selected / exactly two, and the arrangements that need
    /// a linear run of commits of the current branch.
    pub one: bool,
    pub two: bool,
    pub rebase: bool,
}

impl State {
    pub fn set_filter_text(&mut self, field: FilterField, value: String) {
        match field {
            FilterField::Author => self.narrow.author = value,
            FilterField::Since => self.narrow.since = value,
            FilterField::Until => self.narrow.until = value,
            FilterField::Path => self.narrow.path = value,
            FilterField::FirstParent | FilterField::NoMerges => {}
        }
    }

    pub fn set_filter_flag(&mut self, field: FilterField, on: bool) {
        match field {
            FilterField::FirstParent => self.narrow.first_parent = on,
            FilterField::NoMerges => self.narrow.no_merges = on,
            _ => {}
        }
    }

    pub fn selected_commits(&self) -> Vec<&Commit> {
        self.selected
            .iter()
            .filter_map(|i| self.commits.get(*i))
            .collect()
    }

    /// Which actions the selection allows, as in the IDE: rewriting needs
    /// commits of the current branch, squashing needs them side by side.
    pub fn availability(&self) -> Avail {
        let chosen = self.selected_commits();
        if chosen.is_empty() {
            return Avail::default();
        }
        // A file's history skips commits: only what acts on single commits
        // is offered there.
        if self.file.is_some() || self.narrow.is_set() || self.narrow.lines.is_some() {
            return Avail {
                revert: true,
                cherry_pick: chosen.iter().all(|c| !self.head_set.contains(&c.sha)),
                one: chosen.len() == 1,
                two: chosen.len() == 2,
                ..Avail::default()
            };
        }
        let in_head = chosen.iter().all(|c| self.head_set.contains(&c.sha));
        let no_merge = chosen.iter().all(|c| c.parents.len() <= 1);
        let touching = self.selected.windows(2).all(|w| {
            w[1] == w[0] + 1 && self.commits[w[0]].parents.first() == Some(&self.commits[w[1]].sha)
        });
        Avail {
            squash: chosen.len() >= 2 && in_head && no_merge && touching,
            drop: in_head && no_merge,
            reword: chosen.len() == 1 && in_head && no_merge,
            revert: true,
            cherry_pick: chosen.iter().all(|c| !self.head_set.contains(&c.sha)),
            one: chosen.len() == 1,
            two: chosen.len() == 2,
            rebase: chosen.len() == 1 && in_head && no_merge,
        }
    }
}

impl Panel {
    pub fn load_refs(&self) -> Task<Message> {
        let Some(repo) = self.repo.clone() else {
            return Task::none();
        };
        let asked = repo.clone();
        Task::perform(async move { history::refs(&repo).await }, move |refs| {
            Message::Git(GitMsg::RefsLoaded(asked.clone(), refs))
        })
    }

    pub(super) fn refs_loaded(&mut self, result: Result<Refs, String>) -> Task<Message> {
        match result {
            Ok(refs) => {
                self.log.refs = Some(refs);
                if self.log.shown.is_none() {
                    return self.show_log("HEAD");
                }
                Task::none()
            }
            Err(error) => {
                self.notice = Some((error, true));
                Task::none()
            }
        }
    }

    /// Lists the commits of `rev`.
    pub fn show_log(&mut self, rev: &str) -> Task<Message> {
        let Some(repo) = self.repo.clone() else {
            return Task::none();
        };
        if self.log.limit == 0 || self.log.shown.as_deref() != Some(rev) {
            self.log.limit = PAGE;
        }
        self.log.shown = Some(rev.to_string());
        self.log.request += 1;
        let token = self.log.request;
        let rev = rev.to_string();
        let asked = rev.clone();
        let limit = self.log.limit;
        let file = self.log.file.clone();
        let search = (self.log.search.trim().to_string(), self.log.search_mode);
        let filter = self.log.narrow.clone();
        Task::perform(
            async move {
                let (commits, head) = tokio::join!(
                    history::log_filtered(
                        &repo,
                        &rev,
                        limit,
                        file.as_deref(),
                        Some((search.1, search.0.as_str())),
                        &filter
                    ),
                    history::head_commits(&repo, 5000)
                );
                Ok::<_, String>((commits?, head.unwrap_or_default()))
            },
            move |result| Message::Git(GitMsg::LogLoaded(token, asked.clone(), result)),
        )
    }

    pub(super) fn log_loaded(
        &mut self,
        token: u64,
        rev: String,
        result: Result<(Vec<Commit>, HashSet<String>), String>,
    ) -> Task<Message> {
        // The answer of a branch, a filter or a search that is no longer the
        // one shown.
        if token != self.log.request || self.log.shown.as_deref() != Some(rev.as_str()) {
            return Task::none();
        }
        match result {
            Ok((commits, head)) => {
                let previous: Vec<String> = self
                    .log
                    .selected_commits()
                    .iter()
                    .map(|c| c.sha.clone())
                    .collect();
                self.log.rows = graph::layout(&commits);
                self.log.lanes = graph::width(&self.log.rows);
                self.log.commits = commits;
                self.log.head_set = head;
                if let Some(reveal) = self.log.reveal.clone() {
                    match self.log.commits.iter().position(|c| c.sha == reveal.sha) {
                        Some(index) => {
                            self.log.selected = vec![index];
                            self.log.anchor = Some(index);
                            // The rows there are built right away.
                            self.log.view_top.set((index as f32 - 3.0).max(0.0) * 24.0);
                            return Task::batch([
                                self.load_selection(),
                                iced::widget::operation::scroll_to(
                                    scroll_id(),
                                    iced::widget::scrollable::AbsoluteOffset {
                                        x: 0.0,
                                        y: (index as f32 - 3.0).max(0.0) * 24.0,
                                    },
                                ),
                            ]);
                        }
                        None if self.log.commits.len() >= self.log.limit
                            && self.log.limit < 20_000 =>
                        {
                            // Further back than what was loaded.
                            self.log.limit *= 4;
                            let rev = self.log.shown.clone().unwrap_or_else(|| "HEAD".into());
                            return self.show_log(&rev);
                        }
                        None => self.log.reveal = None,
                    }
                }
                self.log.selected = self
                    .log
                    .commits
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| previous.contains(&c.sha))
                    .map(|(i, _)| i)
                    .collect();
                if self.log.selected.is_empty() {
                    self.log.files.clear();
                    self.log.files_for = None;
                    self.log.message.clear();
                    self.log.contained_in.clear();
                    Task::none()
                } else {
                    self.load_selection()
                }
            }
            Err(error) => {
                self.log.commits.clear();
                self.log.rows.clear();
                self.notice = Some((error, true));
                Task::none()
            }
        }
    }

    pub(super) fn branch_clicked(&mut self, name: String) -> Task<Message> {
        let double = self
            .log
            .last_click
            .as_ref()
            .is_some_and(|(n, at)| *n == name && at.elapsed() < Duration::from_millis(500));
        self.log.branch = Some(name.clone());
        if double {
            self.log.last_click = None;
            self.log.selected.clear();
            self.log.files.clear();
            self.log.files_for = None;
            self.show_log(&name)
        } else {
            self.log.last_click = Some((name, Instant::now()));
            Task::none()
        }
    }

    pub(super) fn select_commit(&mut self, index: usize, ctrl: bool, shift: bool) -> Task<Message> {
        if index >= self.log.commits.len() {
            return Task::none();
        }
        if shift {
            let from = self.log.anchor.unwrap_or(index);
            let (a, b) = (from.min(index), from.max(index));
            self.log.selected = (a..=b).collect();
        } else if ctrl {
            match self.log.selected.iter().position(|i| *i == index) {
                Some(at) => {
                    self.log.selected.remove(at);
                }
                None => {
                    self.log.selected.push(index);
                    self.log.selected.sort_unstable();
                }
            }
            self.log.anchor = Some(index);
        } else {
            self.log.selected = vec![index];
            self.log.anchor = Some(index);
        }
        self.load_selection()
    }

    /// A right click on a row that isn't selected selects it alone.
    pub(super) fn select_for_menu(&mut self, index: usize) -> Task<Message> {
        if self.log.selected.contains(&index) {
            Task::none()
        } else {
            self.select_commit(index, false, false)
        }
    }

    /// Asks for the files and the details of the selected commit.
    fn load_selection(&mut self) -> Task<Message> {
        let Some(repo) = self.repo.clone() else {
            return Task::none();
        };
        let chosen = self.log.selected_commits();
        if chosen.len() != 1 {
            self.log.files.clear();
            self.log.files_for = None;
            self.log.message.clear();
            self.log.contained_in.clear();
            return Task::none();
        }
        let commit = chosen[0].clone();
        let sha = commit.sha.clone();
        let (repo_files, repo_info) = (repo.clone(), repo);
        let (sha_files, sha_info) = (sha.clone(), sha.clone());
        Task::batch([
            Task::perform(
                async move { history::commit_files(&repo_files, &commit).await },
                move |files| Message::Git(GitMsg::CommitFilesLoaded(sha_files.clone(), files)),
            ),
            Task::perform(
                async move {
                    let message = history::message(&repo_info, &sha_info).await?;
                    let branches = history::branches_containing(&repo_info, &sha_info)
                        .await
                        .unwrap_or_default();
                    Ok((message, branches))
                },
                move |info| Message::Git(GitMsg::CommitInfoLoaded(sha.clone(), info)),
            ),
        ])
    }

    pub(super) fn commit_files_loaded(
        &mut self,
        sha: String,
        result: Result<Vec<Change>, String>,
    ) -> Task<Message> {
        let still = self.log.selected_commits();
        if still.len() != 1 || still[0].sha != sha {
            return Task::none();
        }
        self.log.files = result.unwrap_or_default();
        self.log.files_for = Some(sha.clone());
        self.log.ticked.clear();
        // Asked for by a blame: the diff of that file opens in the editor.
        match self.log.reveal.take() {
            Some(reveal) if reveal.sha == sha => {
                let index = reveal
                    .path
                    .and_then(|p| self.log.files.iter().position(|c| c.path == p))
                    .unwrap_or(0);
                if self.log.files.is_empty() {
                    Task::none()
                } else {
                    self.commit_file_diff(index)
                }
            }
            other => {
                self.log.reveal = other;
                Task::none()
            }
        }
    }

    /// Shows a commit in the Log tab (the panel opens if need be).
    pub fn reveal_commit(
        &mut self,
        sha: String,
        path: Option<String>,
        repo: std::path::PathBuf,
    ) -> Task<Message> {
        self.open = true;
        self.tab = super::Tab::Log;
        self.probed = Some(repo.clone());
        self.repo = Some(repo);
        self.searched = true;
        self.log.reveal = Some(Reveal { sha, path });
        self.log.limit = PAGE;
        Task::batch([self.load_refs(), self.show_log("HEAD")])
    }

    pub(super) fn commit_info_loaded(
        &mut self,
        sha: String,
        result: Result<(String, Vec<String>), String>,
    ) {
        let still = self.log.selected_commits();
        if still.len() == 1 && still[0].sha == sha {
            if let Ok((message, branches)) = result {
                self.log.message = message;
                self.log.contained_in = branches;
            }
        }
    }

    pub(super) fn load_more_commits(&mut self) -> Task<Message> {
        self.log.limit += PAGE;
        match self.log.shown.clone() {
            Some(rev) => self.show_log(&rev),
            None => Task::none(),
        }
    }

    pub(super) fn checkout(&mut self, name: String) -> Task<Message> {
        let Some(branch) = self
            .log
            .refs
            .as_ref()
            .and_then(|r| r.branches.iter().find(|b| b.name == name))
            .cloned()
        else {
            return Task::none();
        };
        self.act(move |repo| async move {
            history::checkout(&repo, &branch).await?;
            Ok(String::new())
        })
    }

    /// The commits a branch has that the current one lacks, in the log.
    pub(super) fn compare_branch(&mut self, name: String) -> Task<Message> {
        let Some(current) = self.status.as_ref().map(|s| s.branch.clone()) else {
            return Task::none();
        };
        self.tab = super::Tab::Log;
        self.log.compare = Some((current.clone(), name.clone()));
        self.show_log(&format!("{current}..{name}"))
    }

    /// The same two branches, the other way round.
    pub(super) fn compare_swap(&mut self) -> Task<Message> {
        let Some((base, other)) = self.log.compare.take() else {
            return Task::none();
        };
        self.log.compare = Some((other.clone(), base.clone()));
        self.show_log(&format!("{other}..{base}"))
    }

    /// The diff between a branch and the working tree, in the editor.
    pub(super) fn branch_diff(&self, name: String) -> Task<Message> {
        let Some(repo) = self.repo.clone() else {
            return Task::none();
        };
        Task::perform(
            async move {
                let files = history::tree_diff_files(&repo, &name).await?;
                Ok::<_, String>((repo, name, files))
            },
            |result| match result {
                Ok((repo, name, files)) => {
                    let entries: Vec<Entry> = files
                        .into_iter()
                        .map(|change| Entry {
                            source: Source::Tree(name.clone()),
                            change,
                            staged: false,
                        })
                        .collect();
                    if entries.is_empty() {
                        Message::Git(GitMsg::Reply(Err(t!("git.no_diff").to_string())))
                    } else {
                        Message::OpenDiffTab(DiffTab {
                            repo,
                            title: t!("git.diff_with_tree", name = name).to_string(),
                            entries,
                            start: 0,
                        })
                    }
                }
                Err(error) => Message::Git(GitMsg::Reply(Err(error))),
            },
        )
    }

    /// The diff of one file of the selected commit, in the editor.
    pub(super) fn commit_file_diff(&self, index: usize) -> Task<Message> {
        let (Some(repo), Some(sha)) = (self.repo.clone(), self.log.files_for.clone()) else {
            return Task::none();
        };
        let title = self
            .log
            .selected_commits()
            .first()
            .map(|c| format!("{} {}", short(&c.sha), c.subject))
            .unwrap_or_default();
        let entries = self
            .log
            .files
            .iter()
            .map(|change| Entry {
                source: Source::Commit(sha.clone()),
                change: change.clone(),
                staged: false,
            })
            .collect();
        Task::done(Message::OpenDiffTab(DiffTab {
            repo,
            title,
            entries,
            start: index,
        }))
    }

    /// Starts a change to the history: some need a question first.
    pub(super) fn rewrite(&mut self, action: Action) -> Task<Message> {
        let Some(repo) = self.repo.clone() else {
            return Task::none();
        };
        let avail = self.log.availability();
        let allowed = match action {
            Action::Squash => avail.squash,
            Action::Drop => avail.drop,
            Action::Reword => avail.reword,
            Action::Revert => avail.revert,
            Action::CherryPick => avail.cherry_pick,
        };
        if !allowed {
            return Task::none();
        }
        // Newest first, as listed.
        let shas: Vec<String> = self
            .log
            .selected_commits()
            .iter()
            .map(|c| c.sha.clone())
            .collect();
        match action {
            Action::Revert => self.act(move |repo| async move {
                history::revert(&repo, &shas).await?;
                Ok(String::new())
            }),
            Action::CherryPick => self.act(move |repo| async move {
                let oldest_first: Vec<String> = shas.into_iter().rev().collect();
                history::cherry_pick(&repo, &oldest_first).await?;
                Ok(String::new())
            }),
            Action::Squash | Action::Drop | Action::Reword => {
                // Is any of them already on a remote branch?
                Task::perform(
                    async move {
                        let mut pushed = false;
                        for sha in &shas {
                            pushed |= history::on_remote(&repo, sha).await;
                        }
                        // The text to start the message from, oldest first.
                        let mut messages = Vec::new();
                        for sha in shas.iter().rev() {
                            messages.push(history::message(&repo, sha).await.unwrap_or_default());
                        }
                        (pushed, messages.join("\n\n"))
                    },
                    move |(pushed, text)| Message::Git(GitMsg::RewriteReady(action, pushed, text)),
                )
            }
        }
    }

    pub(super) fn rewrite_ready(&mut self, action: Action, pushed: bool, text: String) {
        let shas: Vec<String> = self
            .log
            .selected_commits()
            .iter()
            .map(|c| c.sha.clone())
            .collect();
        self.log.dialog = Some(match action {
            Action::Drop => Dialog::Confirm { shas, pushed },
            _ => Dialog::Message {
                action,
                shas,
                content: text_editor::Content::with_text(&text),
                pushed,
            },
        });
    }

    /// The oldest commit and the newest of the selection, and the one.
    fn selected_shas(&self) -> Vec<String> {
        self.log
            .selected_commits()
            .iter()
            .map(|c| c.sha.clone())
            .collect()
    }

    /// Asks whether the commit is already pushed, then goes on with `then`.
    pub(super) fn guard(&mut self, then: Guarded, sha: String) -> Task<Message> {
        let Some(repo) = self.repo.clone() else {
            return Task::none();
        };
        Task::perform(
            async move { history::on_remote(&repo, &sha).await },
            move |pushed| Message::Git(GitMsg::GuardReady(then, pushed)),
        )
    }

    pub(super) fn guard_ready(&mut self, then: Guarded, pushed: bool) -> Task<Message> {
        if pushed {
            self.log.dialog = Some(Dialog::Guard { then, pushed });
            return Task::none();
        }
        self.run_guarded(then)
    }

    fn run_guarded(&mut self, then: Guarded) -> Task<Message> {
        match then {
            Guarded::Log(action) => self.run_log_action(action),
            Guarded::File(action, index) => self.run_commit_file_action(action, index),
        }
    }

    pub(super) fn log_action(&mut self, action: LogAction) -> Task<Message> {
        if matches!(
            action,
            LogAction::Fixup | LogAction::SquashInto | LogAction::RebaseFrom
        ) && self.log.availability().rebase
        {
            if let Some(sha) = self.selected_shas().into_iter().next() {
                return self.guard(Guarded::Log(action), sha);
            }
        }
        self.run_log_action(action)
    }

    fn run_log_action(&mut self, action: LogAction) -> Task<Message> {
        let avail = self.log.availability();
        let shas = self.selected_shas();
        let Some(repo) = self.repo.clone() else {
            return Task::none();
        };
        match action {
            LogAction::BranchHere | LogAction::TagHere | LogAction::CheckoutHere if !avail.one => {
                Task::none()
            }
            LogAction::BranchHere => {
                self.log.dialog = Some(Dialog::Name {
                    kind: NameKind::Branch,
                    sha: shas[0].clone(),
                    input: String::new(),
                });
                self.typing = true;
                Task::none()
            }
            LogAction::TagHere => {
                self.log.dialog = Some(Dialog::Name {
                    kind: NameKind::Tag,
                    sha: shas[0].clone(),
                    input: String::new(),
                });
                self.typing = true;
                Task::none()
            }
            LogAction::CheckoutHere => {
                let sha = shas[0].clone();
                self.act(move |repo| async move {
                    history::checkout_ref(&repo, &sha).await?;
                    Ok(String::new())
                })
            }
            LogAction::Reset(mode) => {
                if !avail.one {
                    return Task::none();
                }
                self.log.dialog = Some(Dialog::Reset {
                    sha: shas[0].clone(),
                    mode,
                });
                Task::none()
            }
            LogAction::CompareHead | LogAction::CompareSelected => {
                // Newest first, as listed: the older one is the last.
                let (from, to) = match (action, shas.as_slice()) {
                    (LogAction::CompareHead, [one]) => (one.clone(), "HEAD".to_string()),
                    (LogAction::CompareSelected, [newer, older]) => (older.clone(), newer.clone()),
                    _ => return Task::none(),
                };
                let title = format!("{}..{}", short(&from), short(&to));
                Task::perform(
                    async move {
                        let files = history::range_files(&repo, &from, &to).await?;
                        Ok::<_, String>((repo, from, to, files))
                    },
                    move |result| match result {
                        Ok((repo, from, to, files)) if !files.is_empty() => {
                            Message::OpenDiffTab(DiffTab {
                                repo,
                                title: title.clone(),
                                entries: files
                                    .into_iter()
                                    .map(|change| Entry {
                                        source: Source::Range(from.clone(), to.clone()),
                                        change,
                                        staged: false,
                                    })
                                    .collect(),
                                start: 0,
                            })
                        }
                        Ok(_) => Message::Git(GitMsg::Reply(Err(t!("git.no_diff").to_string()))),
                        Err(e) => Message::Git(GitMsg::Reply(Err(e))),
                    },
                )
            }
            LogAction::Fixup | LogAction::SquashInto => {
                if !avail.rebase {
                    return Task::none();
                }
                let sha = shas[0].clone();
                let squash = action == LogAction::SquashInto;
                self.act(move |repo| async move {
                    git::tools::fixup(&repo, &sha, squash).await?;
                    Ok(String::new())
                })
            }
            LogAction::RebaseFrom => {
                if !avail.rebase {
                    return Task::none();
                }
                let commit = self.log.selected_commits()[0].clone();
                // From this commit on: its parent is the base (none: the root).
                let base = commit.parents.first().cloned();
                Task::perform(
                    async move {
                        let plan = history::plan_from(&repo, base.as_deref()).await;
                        (base, plan)
                    },
                    |(base, plan)| Message::Git(GitMsg::RebasePlanReady(base, plan)),
                )
            }
        }
    }

    /// The plan of an interactive rebase is read: the dialog to arrange it.
    pub(super) fn rebase_plan_ready(
        &mut self,
        base: Option<String>,
        plan: Result<Vec<PlanItem>, String>,
    ) {
        match plan {
            Ok(items) => self.log.dialog = Some(Dialog::Rebase { base, items }),
            Err(error) => self.notice = Some((error, true)),
        }
    }

    pub(super) fn rebase_move(&mut self, index: usize, up: bool) {
        if let Some(Dialog::Rebase { items, .. }) = &mut self.log.dialog {
            let other = if up {
                index.checked_sub(1)
            } else {
                Some(index + 1).filter(|n| *n < items.len())
            };
            if let (Some(other), true) = (other, index < items.len()) {
                items.swap(index, other);
            }
        }
    }

    pub(super) fn rebase_set(&mut self, index: usize, action: PlanAction) {
        if let Some(Dialog::Rebase { items, .. }) = &mut self.log.dialog {
            if let Some(item) = items.get_mut(index) {
                item.action = action;
            }
        }
    }

    /// The first line of a reworded commit: the rest of its message stays.
    pub(super) fn rebase_subject(&mut self, index: usize, subject: String) {
        if let Some(Dialog::Rebase { items, .. }) = &mut self.log.dialog {
            if let Some(item) = items.get_mut(index) {
                let body = item
                    .message
                    .split_once('\n')
                    .map(|(_, rest)| rest.to_string())
                    .unwrap_or_default();
                item.message = if body.is_empty() {
                    subject.clone()
                } else {
                    format!("{subject}\n{body}")
                };
                item.subject = subject;
            }
        }
    }

    pub(super) fn dialog_ok(&mut self) -> Task<Message> {
        let Some(dialog) = self.log.dialog.take() else {
            return Task::none();
        };
        match dialog {
            Dialog::Name { kind, sha, input } => {
                let name = input.split_whitespace().collect::<Vec<_>>().join("-");
                if name.is_empty() {
                    self.log.dialog = Some(Dialog::Name { kind, sha, input });
                    return Task::none();
                }
                self.act(move |repo| async move {
                    match kind {
                        NameKind::Branch => {
                            history::create_branch(&repo, &name, Some(&sha)).await?;
                            Ok(format!("\u{4}{name}"))
                        }
                        NameKind::Tag => {
                            history::create_tag(&repo, &name, Some(&sha), None).await?;
                            Ok(format!("\u{9}{name}"))
                        }
                    }
                })
            }
            Dialog::Reset { sha, mode } => self.act(move |repo| async move {
                git::reset(&repo, &sha, mode).await?;
                Ok(String::new())
            }),
            Dialog::Rebase { base, items } => {
                if let Err(error) = history::check_plan(&items) {
                    self.notice = Some((error, true));
                    self.log.dialog = Some(Dialog::Rebase { base, items });
                    return Task::none();
                }
                self.act(move |repo| async move {
                    history::interactive_rebase(&repo, base.as_deref(), &items).await?;
                    Ok(String::new())
                })
            }
            Dialog::Guard { then, .. } => self.run_guarded(then),
            Dialog::Confirm { shas, .. } => self.act(move |repo| async move {
                history::drop_commits(&repo, &shas).await?;
                Ok(String::new())
            }),
            Dialog::Message {
                action,
                shas,
                content,
                ..
            } => {
                let message = content.text().trim_end().to_string();
                if message.trim().is_empty() {
                    self.notice = Some((t!("git.empty_message").to_string(), true));
                    return Task::none();
                }
                self.act(move |repo| async move {
                    match action {
                        Action::Reword => {
                            if let Some(sha) = shas.first() {
                                history::reword(&repo, sha, &message).await?;
                            }
                        }
                        _ => history::squash_commits(&repo, &shas, &message).await?,
                    }
                    Ok(String::new())
                })
            }
        }
    }
}

pub fn short(sha: &str) -> &str {
    sha.get(..7).unwrap_or(sha)
}
