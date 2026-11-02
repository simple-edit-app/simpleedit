//! The Log tab: branches on the left, the commit graph, what a commit
//! changed, and what can be done to commits.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use iced::widget::text_editor;
use iced::Task;

use super::{DiffTab, Entry, GitMsg, Panel};
use crate::app::Message;
use crate::git::graph::{self, Commit, GraphRow};
use crate::git::history::{self, Refs};
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
    /// A confirmation (drop).
    Confirm { shas: Vec<String>, pushed: bool },
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
    pub commits: Vec<Commit>,
    pub rows: Vec<GraphRow>,
    pub lanes: usize,
    /// Indices into `commits`, ascending.
    pub selected: Vec<usize>,
    anchor: Option<usize>,
    /// The commits of HEAD, to know what can be rewritten.
    pub head_set: HashSet<String>,
    pub limit: usize,
    /// The files of the commit when exactly one is selected.
    pub files: Vec<Change>,
    pub files_for: Option<String>,
    pub message: String,
    pub contained_in: Vec<String>,
    last_click: Option<(String, Instant)>,
    pub dialog: Option<Dialog>,
    /// A commit to find and select once the log is there (from a blame).
    pub reveal: Option<Reveal>,
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
}

impl State {
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
        }
    }
}

impl Panel {
    pub(super) fn load_refs(&self) -> Task<Message> {
        let Some(repo) = self.repo.clone() else {
            return Task::none();
        };
        Task::perform(async move { history::refs(&repo).await }, |refs| {
            Message::Git(GitMsg::RefsLoaded(refs))
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
        let rev = rev.to_string();
        let asked = rev.clone();
        let limit = self.log.limit;
        Task::perform(
            async move {
                let commits = history::log(&repo, &rev, limit).await?;
                let head = history::head_commits(&repo, 5000).await.unwrap_or_default();
                Ok::<_, String>((commits, head))
            },
            move |result| Message::Git(GitMsg::LogLoaded(asked.clone(), result)),
        )
    }

    pub(super) fn log_loaded(
        &mut self,
        rev: String,
        result: Result<(Vec<Commit>, HashSet<String>), String>,
    ) -> Task<Message> {
        // The answer of a branch that is no longer the one shown.
        if self.log.shown.as_deref() != Some(rev.as_str()) {
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
                        Message::Git(GitMsg::Done(Err(t!("git.no_diff").to_string())))
                    } else {
                        Message::OpenDiffTab(DiffTab {
                            repo,
                            title: t!("git.diff_with_tree", name = name).to_string(),
                            entries,
                            start: 0,
                        })
                    }
                }
                Err(error) => Message::Git(GitMsg::Done(Err(error))),
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
                            let branches = history::branches_containing(&repo, sha)
                                .await
                                .unwrap_or_default();
                            pushed |= branches.iter().any(|b| b.contains('/'));
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

    pub(super) fn dialog_ok(&mut self) -> Task<Message> {
        let Some(dialog) = self.log.dialog.take() else {
            return Task::none();
        };
        match dialog {
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
