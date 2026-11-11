//! The Stash tab: the stashes, the files of the selected one, its diff.

use iced::Task;

use super::{Entry, GitMsg, Land, Panel, Viewer};
use crate::app::Message;
use crate::git::history::{self, Stash};
use crate::git::{Change, Kind, Source};

#[derive(Default)]
pub struct State {
    pub list: Vec<Stash>,
    pub selected: Option<usize>,
    pub files: Vec<Change>,
    pub viewer: Viewer,
    /// The message of the next stash.
    pub message: String,
    /// Keep the staged changes in the index when stashing.
    pub keep_index: bool,
    /// Stash only the staged changes (`git stash push --staged`).
    pub staged: bool,
    /// The name of the branch to create from a stash.
    pub branch: String,
}

impl State {
    /// The viewer's entries for the files of stash `index`.
    fn entries(index: usize, files: &[Change]) -> Vec<Entry> {
        files
            .iter()
            .map(|change| Entry {
                source: if change.kind == Kind::Untracked {
                    // Untracked files sit in the stash's third parent.
                    Source::Commit(format!("stash@{{{index}}}^3"))
                } else {
                    Source::Stash(index)
                },
                change: change.clone(),
                staged: false,
            })
            .collect()
    }
}

impl Panel {
    pub(super) fn load_stashes(&self) -> Task<Message> {
        let Some(repo) = self.repo.clone() else {
            return Task::none();
        };
        let asked = repo.clone();
        Task::perform(async move { history::stashes(&repo).await }, move |list| {
            Message::Git(GitMsg::StashesLoaded(asked.clone(), list))
        })
    }

    pub(super) fn stashes_loaded(&mut self, result: Result<Vec<Stash>, String>) -> Task<Message> {
        match result {
            Ok(list) => {
                if self.stash.list == list {
                    return Task::none();
                }
                self.stash.list = list;
                // Keep the same stash selected, or the first one.
                let keep = self.stash.selected.filter(|i| *i < self.stash.list.len());
                match keep.or((!self.stash.list.is_empty()).then_some(0)) {
                    Some(i) => self.select_stash(i),
                    None => {
                        self.stash.selected = None;
                        self.stash.files.clear();
                        self.stash.viewer = Viewer::default();
                        Task::none()
                    }
                }
            }
            Err(error) => {
                self.notice = Some((error, true));
                Task::none()
            }
        }
    }

    pub(super) fn select_stash(&mut self, index: usize) -> Task<Message> {
        let Some(repo) = self.repo.clone() else {
            return Task::none();
        };
        self.stash.selected = Some(index);
        Task::perform(
            async move { (index, history::stash_files(&repo, index).await) },
            |(index, files)| Message::Git(GitMsg::StashFilesLoaded(index, files)),
        )
    }

    pub(super) fn stash_files_loaded(
        &mut self,
        index: usize,
        result: Result<Vec<Change>, String>,
    ) -> Task<Message> {
        if self.stash.selected != Some(index) {
            return Task::none();
        }
        let Some(repo) = self.repo.clone() else {
            return Task::none();
        };
        match result {
            Ok(files) => {
                let entries = State::entries(index, &files);
                self.stash.files = files;
                self.stash.viewer = Viewer::default();
                self.stash.viewer.entries = entries;
                if self.stash.viewer.entries.is_empty() {
                    Task::none()
                } else {
                    self.stash.viewer.open(&repo, 0, Land::First)
                }
            }
            Err(error) => {
                self.notice = Some((error, true));
                Task::none()
            }
        }
    }

    pub(super) fn stash_action(&mut self, index: usize, action: StashAction) -> Task<Message> {
        self.act(move |repo| async move {
            match action {
                StashAction::Apply => history::stash_apply(&repo, index).await?,
                StashAction::Pop => history::stash_pop(&repo, index).await?,
                StashAction::Drop => history::stash_drop(&repo, index).await?,
            }
            Ok(String::new())
        })
    }

    pub(super) fn stash_push(&mut self) -> Task<Message> {
        let message = std::mem::take(&mut self.stash.message);
        let (keep_index, staged) = (self.stash.keep_index, self.stash.staged);
        self.act(move |repo| async move {
            history::stash_push(&repo, &message, keep_index, staged).await?;
            Ok(String::new())
        })
    }

    pub(super) fn stash_to_branch(&mut self, index: usize) -> Task<Message> {
        let name = std::mem::take(&mut self.stash.branch);
        self.act(move |repo| async move {
            history::stash_branch(&repo, &name, index).await?;
            Ok(String::new())
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub enum StashAction {
    Apply,
    Pop,
    Drop,
}

/// `03/09/2026 16:12` (the order of the day and the month follows the language)
pub fn format_time(epoch: i64) -> String {
    use chrono::TimeZone;
    chrono::Local
        .timestamp_opt(epoch, 0)
        .single()
        .map(|t| t.format(&t!("git.datetime_format")).to_string())
        .unwrap_or_default()
}

/// `03/09/2026`
pub fn format_date(epoch: i64) -> String {
    use chrono::TimeZone;
    chrono::Local
        .timestamp_opt(epoch, 0)
        .single()
        .map(|t| t.format(&t!("git.date_format")).to_string())
        .unwrap_or_default()
}
