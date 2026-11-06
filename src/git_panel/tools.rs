//! The dialogs of the panel: a push with its options, a push that was
//! refused, the remotes, the patches, a merge or a rebase with options, the
//! last messages, a commit split, and the repositories of the projects.

use std::path::PathBuf;

use iced::widget::{
    button, checkbox, column, container, pick_list, row, scrollable, text, text_input, Space,
};
use iced::{Alignment, Element, Length, Task};

use super::{GitMsg, Panel};
use crate::app::Message;
use crate::git::graph::Commit;
use crate::git::history;
use crate::git::tools::{self as gt, MergeOptions, PushSpec, PushTags, RebaseOptions, Remote};
use crate::git::{self, MergeMode, PullMode};
use crate::theme as se_theme;

/// A push being set.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PushDialog {
    pub remotes: Vec<String>,
    pub spec: PushSpec,
    /// What the push would send.
    pub commits: Vec<Commit>,
    /// Force asked once: the second click pushes.
    pub force_asked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RemotesDialog {
    pub list: Vec<Remote>,
    /// The remote being changed (none: a new one).
    pub editing: Option<String>,
    pub name: String,
    pub url: String,
    pub error: Option<String>,
}

/// What a repository of the projects stands at.
#[derive(Debug, Clone, PartialEq)]
pub struct RepoLine {
    pub root: PathBuf,
    pub name: String,
    pub color: iced::Color,
    pub branch: String,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub changes: usize,
}

/// What is done to every repository at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bulk {
    Fetch,
    Pull,
    Push,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Dialog {
    Push(PushDialog),
    Rejected(PushSpec),
    Remotes(RemotesDialog),
    Patch,
    Merge(String, MergeOptions),
    Rebase(String, RebaseOptions),
    Messages(Vec<String>),
    Repos(Vec<RepoLine>, Vec<(String, Result<(), String>)>),
    Extract {
        sha: String,
        paths: Vec<String>,
        message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeFlag {
    NoCommit,
    NoVerify,
    AllowUnrelated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RebaseFlag {
    Merges,
    KeepEmpty,
    Root,
    UpdateRefs,
    Autosquash,
}

/// What a right click on a file of a commit does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitFile {
    /// The file comes back as that commit has it.
    Get,
    /// Its changes are applied, not committed.
    Apply,
    /// Taken out of the commit.
    Remove,
    /// Moved to a commit of its own.
    Extract,
}

#[derive(Debug, Clone)]
pub enum ToolMsg {
    Close,
    OpenPush(Option<String>),
    PushLoaded(Result<PushDialog, String>),
    PushRemote(String),
    PushTarget(String),
    PushUpstream(bool),
    PushTagsSet(PushTags),
    PushRun(bool),
    Update(PullMode),
    OpenRemotes,
    RemotesLoaded(Result<Vec<Remote>, String>),
    RemoteName(String),
    RemoteUrl(String),
    RemoteEdit(Option<Remote>),
    RemoteSave,
    RemoteRemove(String),
    OpenPatch,
    PatchLocal(bool),
    PatchCommits(bool),
    PatchMade(bool, Result<String, String>),
    PatchFile,
    PatchClipboard,
    PatchApply(Option<String>),
    OpenMerge(String),
    MergeModeSet(MergeMode),
    MergeFlagSet(MergeFlag, bool),
    MergeMessage(String),
    MergeRun,
    OpenRebase(String),
    RebaseFlagSet(RebaseFlag, bool),
    RebaseOnto(String),
    RebaseRun,
    OpenMessages,
    MessagesLoaded(Result<Vec<String>, String>),
    UseMessage(String),
    OpenRepos,
    ReposLoaded(Vec<RepoLine>),
    RepoPick(PathBuf),
    RepoFollow,
    ReposRun(Bulk),
    ReposDone(Vec<(String, Result<(), String>)>),
    FileOf(CommitFile, usize),
    ExtractMessage(String),
    ExtractRun,
}

impl ToolMsg {
    pub fn is_background(&self) -> bool {
        matches!(
            self,
            ToolMsg::PushLoaded(_)
                | ToolMsg::RemotesLoaded(_)
                | ToolMsg::PatchMade(..)
                | ToolMsg::MessagesLoaded(_)
                | ToolMsg::ReposLoaded(_)
                | ToolMsg::ReposDone(_)
        )
    }
}

fn tool(msg: ToolMsg) -> Message {
    Message::Git(GitMsg::Tool(msg))
}

/// The remote and the branch of `origin/main`.
fn split_upstream(upstream: &str) -> Option<(&str, &str)> {
    upstream.split_once('/')
}

impl Panel {
    /// The push of the current branch to where it goes.
    fn current_push(&self) -> Option<PushSpec> {
        let status = self.status.as_ref()?;
        if status.branch.starts_with('(') {
            return None;
        }
        let (remote, target) = status
            .upstream
            .as_deref()
            .and_then(split_upstream)
            .unwrap_or(("origin", status.branch.as_str()));
        Some(PushSpec {
            remote: remote.to_string(),
            source: status.branch.clone(),
            target: target.to_string(),
            set_upstream: status.upstream.is_none(),
            ..PushSpec::default()
        })
    }

    /// A refused push asks how to catch up instead of only saying so.
    pub(super) fn push_refused(&mut self) -> bool {
        // Another branch than the current one can't be caught up here.
        if self.last_push.is_none() && self.branches.selected.is_some() {
            let current = self.status.as_ref().map(|s| s.branch.as_str());
            if self.tab == super::Tab::Branches
                && !matches!(&self.branches.selected, Some(super::branches::Ref::Branch(n)) if Some(n.as_str()) == current)
            {
                return false;
            }
        }
        let Some(spec) = self.last_push.clone().or_else(|| self.current_push()) else {
            return false;
        };
        self.dialog = Some(Dialog::Rejected(spec));
        true
    }

    fn push_commits(&self, repo: PathBuf, dialog: PushDialog) -> Task<Message> {
        Task::perform(
            async move {
                let target = format!("{}/{}", dialog.spec.remote, dialog.spec.target);
                let known = git::run(&repo, &["rev-parse", "--verify", "-q", &target])
                    .await
                    .is_ok();
                let target = known.then_some(target);
                let commits = gt::outgoing(&repo, &dialog.spec.source, target.as_deref()).await?;
                Ok(PushDialog { commits, ..dialog })
            },
            |result| tool(ToolMsg::PushLoaded(result)),
        )
    }

    fn reload_remotes<F, Fut>(&self, work: F) -> Task<Message>
    where
        F: FnOnce(PathBuf) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<(), String>> + Send + 'static,
    {
        let Some(repo) = self.repo.clone() else {
            return Task::none();
        };
        Task::perform(
            async move {
                work(repo.clone()).await?;
                gt::remotes(&repo).await
            },
            |result| tool(ToolMsg::RemotesLoaded(result)),
        )
    }

    /// Saves a patch where the user says.
    fn save_patch(patch: String) -> Task<Message> {
        Task::perform(
            async move {
                let Some(file) = rfd::AsyncFileDialog::new()
                    .set_file_name("changes.patch")
                    .save_file()
                    .await
                else {
                    return Ok(String::new());
                };
                tokio::fs::write(file.path(), patch)
                    .await
                    .map_err(|e| e.to_string())?;
                Ok("\u{b}patch".to_string())
            },
            |result| Message::Git(GitMsg::Done(result)),
        )
    }

    pub(super) fn tool_update(&mut self, msg: ToolMsg, ctx: &super::Ctx) -> Task<Message> {
        let repo = self.repo.clone();
        match msg {
            ToolMsg::Close => {
                self.dialog = None;
                self.typing = false;
                Task::none()
            }
            ToolMsg::OpenPush(commit) => {
                let (Some(repo), Some(mut spec)) = (repo, self.current_push()) else {
                    self.notice = Some((t!("git.err_detached_push").to_string(), true));
                    return Task::none();
                };
                if let Some(sha) = commit {
                    spec.source = sha;
                }
                Task::perform(
                    async move {
                        let remotes = gt::remotes(&repo).await?;
                        let mut dialog = PushDialog {
                            remotes: remotes.into_iter().map(|r| r.name).collect(),
                            spec,
                            ..PushDialog::default()
                        };
                        if !dialog.remotes.contains(&dialog.spec.remote) {
                            if let Some(first) = dialog.remotes.first() {
                                dialog.spec.remote = first.clone();
                            }
                        }
                        let target = format!("{}/{}", dialog.spec.remote, dialog.spec.target);
                        let known = git::run(&repo, &["rev-parse", "--verify", "-q", &target])
                            .await
                            .is_ok();
                        let target = known.then_some(target);
                        dialog.commits =
                            gt::outgoing(&repo, &dialog.spec.source, target.as_deref()).await?;
                        Ok(dialog)
                    },
                    |result| tool(ToolMsg::PushLoaded(result)),
                )
            }
            ToolMsg::PushLoaded(result) => {
                match result {
                    Ok(dialog) => {
                        self.dialog = Some(Dialog::Push(dialog));
                        self.typing = true;
                    }
                    Err(e) => self.notice = Some((e, true)),
                }
                Task::none()
            }
            ToolMsg::PushRemote(remote) => {
                let Some(Dialog::Push(dialog)) = &mut self.dialog else {
                    return Task::none();
                };
                dialog.spec.remote = remote;
                dialog.force_asked = false;
                let dialog = dialog.clone();
                repo.map_or_else(Task::none, |repo| self.push_commits(repo, dialog))
            }
            ToolMsg::PushTarget(target) => {
                let Some(Dialog::Push(dialog)) = &mut self.dialog else {
                    return Task::none();
                };
                dialog.spec.target = target;
                dialog.force_asked = false;
                let dialog = dialog.clone();
                repo.map_or_else(Task::none, |repo| self.push_commits(repo, dialog))
            }
            ToolMsg::PushUpstream(on) => {
                if let Some(Dialog::Push(dialog)) = &mut self.dialog {
                    dialog.spec.set_upstream = on;
                }
                Task::none()
            }
            ToolMsg::PushTagsSet(tags) => {
                if let Some(Dialog::Push(dialog)) = &mut self.dialog {
                    dialog.spec.tags = tags;
                }
                Task::none()
            }
            ToolMsg::PushRun(force) => {
                let Some(Dialog::Push(dialog)) = &mut self.dialog else {
                    return Task::none();
                };
                if force && !dialog.force_asked {
                    dialog.force_asked = true;
                    return Task::none();
                }
                let mut spec = dialog.spec.clone();
                spec.force = force;
                if spec.target.trim().is_empty() {
                    return Task::none();
                }
                self.dialog = None;
                self.typing = false;
                self.last_push = Some(spec.clone());
                self.act(move |repo| async move {
                    gt::push(&repo, &spec).await?;
                    Ok("\u{3}pushed".to_string())
                })
            }
            ToolMsg::Update(mode) => {
                let Some(Dialog::Rejected(spec)) = self.dialog.take() else {
                    return Task::none();
                };
                self.act(move |repo| async move {
                    gt::update_and_push(&repo, &spec, mode).await?;
                    Ok("\u{3}pushed".to_string())
                })
            }
            ToolMsg::OpenRemotes => {
                self.dialog = Some(Dialog::Remotes(RemotesDialog::default()));
                self.typing = true;
                self.reload_remotes(|_| async { Ok(()) })
            }
            ToolMsg::RemotesLoaded(result) => {
                if let Some(Dialog::Remotes(dialog)) = &mut self.dialog {
                    match result {
                        Ok(list) => {
                            *dialog = RemotesDialog {
                                list,
                                ..RemotesDialog::default()
                            };
                        }
                        Err(e) => dialog.error = Some(e),
                    }
                }
                Task::none()
            }
            ToolMsg::RemoteName(name) => {
                if let Some(Dialog::Remotes(dialog)) = &mut self.dialog {
                    dialog.name = name;
                }
                Task::none()
            }
            ToolMsg::RemoteUrl(url) => {
                if let Some(Dialog::Remotes(dialog)) = &mut self.dialog {
                    dialog.url = url;
                }
                Task::none()
            }
            ToolMsg::RemoteEdit(remote) => {
                if let Some(Dialog::Remotes(dialog)) = &mut self.dialog {
                    dialog.error = None;
                    match remote {
                        Some(r) => {
                            dialog.editing = Some(r.name.clone());
                            dialog.name = r.name;
                            dialog.url = r.url;
                        }
                        None => {
                            dialog.editing = None;
                            dialog.name.clear();
                            dialog.url.clear();
                        }
                    }
                }
                Task::none()
            }
            ToolMsg::RemoteSave => {
                let Some(Dialog::Remotes(dialog)) = &self.dialog else {
                    return Task::none();
                };
                let (name, url) = (
                    dialog.name.trim().to_string(),
                    dialog.url.trim().to_string(),
                );
                if name.is_empty() || url.is_empty() {
                    return Task::none();
                }
                let old = dialog.editing.clone();
                self.reload_remotes(move |repo| async move {
                    match old {
                        Some(old) => gt::edit_remote(&repo, &old, &name, &url).await,
                        None => gt::add_remote(&repo, &name, &url).await,
                    }
                })
            }
            ToolMsg::RemoteRemove(name) => self
                .reload_remotes(move |repo| async move { gt::remove_remote(&repo, &name).await }),
            ToolMsg::OpenPatch => {
                self.dialog = Some(Dialog::Patch);
                Task::none()
            }
            ToolMsg::PatchLocal(save) => {
                let Some(repo) = repo else {
                    return Task::none();
                };
                Task::perform(async move { gt::local_patch(&repo).await }, move |r| {
                    tool(ToolMsg::PatchMade(save, r))
                })
            }
            ToolMsg::PatchCommits(save) => {
                let Some(repo) = repo else {
                    return Task::none();
                };
                // Oldest first, to apply in order.
                let shas: Vec<String> = self
                    .log
                    .selected_commits()
                    .iter()
                    .rev()
                    .map(|c| c.sha.clone())
                    .collect();
                Task::perform(
                    async move { gt::commits_patch(&repo, &shas).await },
                    move |r| tool(ToolMsg::PatchMade(save, r)),
                )
            }
            ToolMsg::PatchMade(save, result) => {
                self.dialog = None;
                match result {
                    Ok(patch) if patch.trim().is_empty() => {
                        self.notice = Some((t!("git.err_empty_patch").to_string(), true));
                        Task::none()
                    }
                    Ok(patch) if save => Self::save_patch(patch),
                    Ok(patch) => {
                        self.notice = Some((t!("git.patch_copied").to_string(), false));
                        iced::clipboard::write(patch)
                    }
                    Err(e) => {
                        self.notice = Some((e, true));
                        Task::none()
                    }
                }
            }
            ToolMsg::PatchFile => Task::perform(
                async move {
                    let file = rfd::AsyncFileDialog::new()
                        .add_filter("patch", &["patch", "diff"])
                        .pick_file()
                        .await?;
                    tokio::fs::read_to_string(file.path()).await.ok()
                },
                |text| tool(ToolMsg::PatchApply(text)),
            ),
            ToolMsg::PatchClipboard => {
                iced::clipboard::read().map(|text| tool(ToolMsg::PatchApply(text)))
            }
            ToolMsg::PatchApply(text) => {
                let Some(patch) = text else {
                    return Task::none();
                };
                self.dialog = None;
                self.act(move |repo| async move {
                    gt::apply_patch(&repo, &patch).await?;
                    Ok("\u{c}applied".to_string())
                })
            }
            ToolMsg::OpenMerge(branch) => {
                let options = MergeOptions {
                    mode: self.merge_mode,
                    ..MergeOptions::default()
                };
                self.dialog = Some(Dialog::Merge(branch, options));
                self.typing = true;
                Task::none()
            }
            ToolMsg::MergeModeSet(mode) => {
                if let Some(Dialog::Merge(_, options)) = &mut self.dialog {
                    options.mode = mode;
                }
                Task::none()
            }
            ToolMsg::MergeFlagSet(flag, on) => {
                if let Some(Dialog::Merge(_, options)) = &mut self.dialog {
                    match flag {
                        MergeFlag::NoCommit => options.no_commit = on,
                        MergeFlag::NoVerify => options.no_verify = on,
                        MergeFlag::AllowUnrelated => options.allow_unrelated = on,
                    }
                }
                Task::none()
            }
            ToolMsg::MergeMessage(message) => {
                if let Some(Dialog::Merge(_, options)) = &mut self.dialog {
                    options.message = message;
                }
                Task::none()
            }
            ToolMsg::MergeRun => {
                let Some(Dialog::Merge(branch, options)) = self.dialog.take() else {
                    return Task::none();
                };
                self.typing = false;
                self.act(move |repo| async move {
                    gt::merge(&repo, &branch, &options).await?;
                    Ok(String::new())
                })
            }
            ToolMsg::OpenRebase(branch) => {
                self.dialog = Some(Dialog::Rebase(branch, RebaseOptions::default()));
                self.typing = true;
                Task::none()
            }
            ToolMsg::RebaseFlagSet(flag, on) => {
                if let Some(Dialog::Rebase(_, options)) = &mut self.dialog {
                    match flag {
                        RebaseFlag::Merges => options.rebase_merges = on,
                        RebaseFlag::KeepEmpty => options.keep_empty = on,
                        RebaseFlag::Root => options.root = on,
                        RebaseFlag::UpdateRefs => options.update_refs = on,
                        RebaseFlag::Autosquash => options.autosquash = on,
                    }
                }
                Task::none()
            }
            ToolMsg::RebaseOnto(onto) => {
                if let Some(Dialog::Rebase(_, options)) = &mut self.dialog {
                    options.onto = onto;
                }
                Task::none()
            }
            ToolMsg::RebaseRun => {
                let Some(Dialog::Rebase(branch, options)) = self.dialog.take() else {
                    return Task::none();
                };
                self.typing = false;
                self.act(move |repo| async move {
                    gt::rebase(&repo, &branch, &options).await?;
                    Ok(String::new())
                })
            }
            ToolMsg::OpenMessages => {
                let Some(repo) = repo else {
                    return Task::none();
                };
                Task::perform(async move { gt::recent_messages(&repo, 20).await }, |r| {
                    tool(ToolMsg::MessagesLoaded(r))
                })
            }
            ToolMsg::MessagesLoaded(result) => {
                match result {
                    Ok(list) => self.dialog = Some(Dialog::Messages(list)),
                    Err(e) => self.notice = Some((e, true)),
                }
                Task::none()
            }
            ToolMsg::UseMessage(message) => {
                self.dialog = None;
                self.message = iced::widget::text_editor::Content::with_text(&message);
                Task::none()
            }
            ToolMsg::OpenRepos => {
                let roots: Vec<(PathBuf, iced::Color)> = ctx
                    .roots
                    .iter()
                    .map(|r| {
                        let color = crate::projects::color_of(ctx.colors, ctx.roots, r);
                        (r.clone(), color)
                    })
                    .collect();
                Task::perform(repo_lines(roots), |lines| tool(ToolMsg::ReposLoaded(lines)))
            }
            ToolMsg::ReposLoaded(lines) => {
                let said = match &self.dialog {
                    Some(Dialog::Repos(_, said)) => said.clone(),
                    _ => Vec::new(),
                };
                self.dialog = Some(Dialog::Repos(lines, said));
                Task::none()
            }
            ToolMsg::RepoPick(root) => {
                self.dialog = None;
                self.show_repo(root, ctx.current_file.map(std::path::Path::to_path_buf));
                self.stick_to_repo();
                self.refresh(ctx)
            }
            ToolMsg::RepoFollow => {
                self.dialog = None;
                self.unpin();
                self.refresh(ctx)
            }
            ToolMsg::ReposRun(what) => {
                let Some(Dialog::Repos(lines, said)) = &mut self.dialog else {
                    return Task::none();
                };
                said.clear();
                let lines = lines.clone();
                let mode = self.pull_mode;
                Task::perform(
                    async move {
                        let mut said = Vec::new();
                        for line in lines {
                            said.push((line.name.clone(), bulk(&line, what, mode).await));
                        }
                        said
                    },
                    |said| tool(ToolMsg::ReposDone(said)),
                )
            }
            ToolMsg::ReposDone(results) => {
                if let Some(Dialog::Repos(_, said)) = &mut self.dialog {
                    *said = results;
                }
                self.tool_update(ToolMsg::OpenRepos, ctx)
            }
            ToolMsg::FileOf(action, index) => self.commit_file_action(action, index),
            ToolMsg::ExtractMessage(message) => {
                if let Some(Dialog::Extract { message: m, .. }) = &mut self.dialog {
                    *m = message;
                }
                Task::none()
            }
            ToolMsg::ExtractRun => {
                let Some(Dialog::Extract {
                    sha,
                    paths,
                    message,
                }) = self.dialog.clone()
                else {
                    return Task::none();
                };
                if message.trim().is_empty() {
                    return Task::none();
                }
                self.dialog = None;
                self.typing = false;
                self.act(move |repo| async move {
                    gt::extract_from_commit(&repo, &sha, &paths, &message).await?;
                    Ok(String::new())
                })
            }
        }
    }

    /// The files an action on a file of the commit concerns: those ticked,
    /// and the one clicked, with the old name of a renamed one.
    fn commit_paths(&self, index: usize) -> Vec<String> {
        let mut paths: Vec<String> = Vec::new();
        for (i, change) in self.log.files.iter().enumerate() {
            if i == index || self.log.ticked.contains(&change.path) {
                paths.push(change.path.clone());
                paths.extend(change.from.clone());
            }
        }
        paths
    }

    fn commit_file_action(&mut self, action: CommitFile, index: usize) -> Task<Message> {
        let Some(sha) = self.log.files_for.clone() else {
            return Task::none();
        };
        let paths = self.commit_paths(index);
        if paths.is_empty() {
            return Task::none();
        }
        match action {
            CommitFile::Get => {
                let Some(path) = self.log.files.get(index).map(|c| c.path.clone()) else {
                    return Task::none();
                };
                self.act(move |repo| async move {
                    gt::get_from_revision(&repo, &sha, &path).await?;
                    Ok(String::new())
                })
            }
            CommitFile::Apply => self.act(move |repo| async move {
                gt::apply_from_commit(&repo, &sha, &paths).await?;
                Ok("\u{c}applied".to_string())
            }),
            CommitFile::Remove => self.act(move |repo| async move {
                gt::remove_from_commit(&repo, &sha, &paths).await?;
                Ok(String::new())
            }),
            CommitFile::Extract => {
                self.dialog = Some(Dialog::Extract {
                    sha,
                    paths,
                    message: String::new(),
                });
                self.typing = true;
                Task::none()
            }
        }
    }

    /// The dialog over the panel, if one is open.
    pub(super) fn tool_dialog_view<'a>(
        &'a self,
        dialog: &'a Dialog,
        dark: bool,
    ) -> Element<'a, Message> {
        let p = se_theme::palette(dark);
        let ghost = move |active: bool| (se_theme::GhostButton { dark, active }).style();
        // Every popin names itself and closes with a cross.
        let title = |label: String| -> Element<'a, Message> {
            row![
                text(label)
                    .size(13.5)
                    .font(se_theme::UI_SEMIBOLD)
                    .color(p.text),
                Space::new().width(Length::Fill),
                button(text("×").size(16).color(p.muted))
                    .padding([0, 7])
                    .on_press(Message::Git(GitMsg::Tool(ToolMsg::Close)))
                    .style(ghost(false)),
            ]
            .align_y(Alignment::Center)
            .into()
        };
        let primary = |label: String, msg: Option<Message>| {
            button(text(label).size(12.5))
                .padding([5, 14])
                .on_press_maybe(msg)
                .style((se_theme::Primary { dark }).style())
        };
        let plain = |label: String, msg: Option<Message>| {
            button(text(label).size(12.5))
                .padding([5, 14])
                .on_press_maybe(msg)
                .style((se_theme::Outline { dark }).style())
        };
        let field = |placeholder: String, value: &str| {
            text_input(&placeholder, value)
                .size(13)
                .padding([6, 10])
                .style(se_theme::Field::new(dark).style())
        };
        let flag = |label: String, on: bool| checkbox(on).label(label).size(14).text_size(12.5);
        let idle = !self.busy;
        let body: Element<Message> = match dialog {
            Dialog::Push(d) => {
                let mut commits = column![].spacing(1);
                for commit in d.commits.iter().take(100) {
                    commits = commits.push(
                        row![
                            text(commit.sha.chars().take(8).collect::<String>())
                                .size(12)
                                .font(se_theme::MONO)
                                .color(p.accent),
                            text(commit.subject.as_str())
                                .size(12.5)
                                .wrapping(iced::widget::text::Wrapping::None),
                        ]
                        .spacing(8),
                    );
                }
                if d.commits.is_empty() {
                    commits = commits.push(
                        text(t!("git.push_nothing").to_string())
                            .size(12.5)
                            .color(p.muted),
                    );
                }
                let can = idle && !d.remotes.is_empty() && !d.spec.target.trim().is_empty();
                let force_label = if d.force_asked {
                    t!("git.force_push_confirm").to_string()
                } else {
                    t!("git.force_push").to_string()
                };
                column![
                    title(t!("git.push_title", source = d.spec.source.as_str()).to_string()),
                    row![
                        pick_list(d.remotes.clone(), Some(d.spec.remote.clone()), |r| {
                            tool(ToolMsg::PushRemote(r))
                        })
                        .text_size(12.5)
                        .padding([4, 8])
                        .width(Length::Fixed(130.0))
                        .style(se_theme::Select { dark }.style())
                        .menu_style(se_theme::Select { dark }.menu_style()),
                        field(t!("git.push_target").to_string(), &d.spec.target)
                            .on_input(|t| tool(ToolMsg::PushTarget(t))),
                    ]
                    .spacing(6),
                    scrollable(commits)
                        .style(se_theme::Scrollbar { dark }.style())
                        .height(Length::Fixed(70.0)),
                    row![
                        flag(t!("git.push_set_upstream").to_string(), d.spec.set_upstream)
                            .on_toggle(|on| tool(ToolMsg::PushUpstream(on))),
                        Space::new().width(Length::Fill),
                        text(t!("git.push_tags").to_string())
                            .size(12.5)
                            .color(p.muted),
                        pick_list(PushTags::ALL, Some(d.spec.tags), |t| {
                            tool(ToolMsg::PushTagsSet(t))
                        })
                        .text_size(12.5)
                        .padding([3, 8])
                        .style(se_theme::Select { dark }.style())
                        .menu_style(se_theme::Select { dark }.menu_style()),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center),
                    row![
                        primary(
                            t!("git.push").to_string(),
                            can.then_some(tool(ToolMsg::PushRun(false)))
                        ),
                        plain(force_label, can.then_some(tool(ToolMsg::PushRun(true)))),
                    ]
                    .spacing(8),
                ]
                .spacing(10)
                .into()
            }
            Dialog::Rejected(spec) => column![
                title(t!("git.rejected_title").to_string()),
                text(t!("git.rejected_text", target = spec.target.as_str()).to_string())
                    .size(12.5)
                    .color(p.muted),
                row![
                    primary(
                        t!("git.rejected_rebase").to_string(),
                        idle.then_some(tool(ToolMsg::Update(PullMode::Rebase)))
                    ),
                    plain(
                        t!("git.rejected_merge").to_string(),
                        idle.then_some(tool(ToolMsg::Update(PullMode::Merge)))
                    ),
                ]
                .spacing(8),
            ]
            .spacing(10)
            .into(),
            Dialog::Remotes(d) => {
                let mut list = column![].spacing(2);
                for remote in &d.list {
                    list = list.push(
                        row![
                            text(remote.name.as_str())
                                .size(12.5)
                                .font(se_theme::UI_MEDIUM),
                            container(
                                text(remote.url.as_str())
                                    .size(12)
                                    .color(p.muted)
                                    .wrapping(iced::widget::text::Wrapping::None),
                            )
                            .width(Length::Fill)
                            .clip(true),
                            plain(
                                t!("git.remote_edit").to_string(),
                                Some(tool(ToolMsg::RemoteEdit(Some(remote.clone()))))
                            ),
                            plain(
                                t!("git.delete").to_string(),
                                Some(tool(ToolMsg::RemoteRemove(remote.name.clone())))
                            ),
                        ]
                        .spacing(8)
                        .align_y(Alignment::Center),
                    );
                }
                if d.list.is_empty() {
                    list = list.push(
                        text(t!("git.no_remote").to_string())
                            .size(12.5)
                            .color(p.muted),
                    );
                }
                let mut form = column![
                    title(t!("git.remotes").to_string()),
                    list,
                    row![
                        field(t!("git.remote_name").to_string(), &d.name)
                            .on_input(|t| tool(ToolMsg::RemoteName(t)))
                            .width(Length::Fixed(130.0)),
                        field(t!("git.remote_url").to_string(), &d.url)
                            .on_input(|t| tool(ToolMsg::RemoteUrl(t)))
                            .on_submit(tool(ToolMsg::RemoteSave)),
                    ]
                    .spacing(6),
                ]
                .spacing(10);
                if let Some(error) = &d.error {
                    form = form.push(
                        text(error.as_str())
                            .size(12)
                            .color(se_theme::error_color(dark)),
                    );
                }
                let save = if d.editing.is_some() {
                    t!("git.remote_save").to_string()
                } else {
                    t!("git.remote_add").to_string()
                };
                let filled = !d.name.trim().is_empty() && !d.url.trim().is_empty();
                form.push(
                    row![primary(save, filled.then_some(tool(ToolMsg::RemoteSave))),].spacing(8),
                )
                .into()
            }
            Dialog::Patch => column![
                title(t!("git.patch_title").to_string()),
                text(t!("git.patch_local").to_string())
                    .size(12.5)
                    .color(p.muted),
                row![
                    plain(
                        t!("git.patch_copy").to_string(),
                        Some(tool(ToolMsg::PatchLocal(false)))
                    ),
                    plain(
                        t!("git.patch_save").to_string(),
                        Some(tool(ToolMsg::PatchLocal(true)))
                    ),
                ]
                .spacing(8),
                text(t!("git.patch_apply").to_string())
                    .size(12.5)
                    .color(p.muted),
                row![
                    plain(
                        t!("git.patch_from_file").to_string(),
                        idle.then_some(tool(ToolMsg::PatchFile))
                    ),
                    plain(
                        t!("git.patch_from_clipboard").to_string(),
                        idle.then_some(tool(ToolMsg::PatchClipboard))
                    ),
                ]
                .spacing(8),
            ]
            .spacing(10)
            .into(),
            Dialog::Merge(branch, o) => column![
                title(t!("git.merge_title", name = branch.as_str()).to_string()),
                pick_list(MergeMode::ALL, Some(o.mode), |m| {
                    tool(ToolMsg::MergeModeSet(m))
                })
                .text_size(12.5)
                .padding([4, 8])
                .style(se_theme::Select { dark }.style())
                .menu_style(se_theme::Select { dark }.menu_style()),
                flag(t!("git.merge_no_commit").to_string(), o.no_commit)
                    .on_toggle(|on| tool(ToolMsg::MergeFlagSet(MergeFlag::NoCommit, on))),
                flag(t!("git.no_verify").to_string(), o.no_verify)
                    .on_toggle(|on| tool(ToolMsg::MergeFlagSet(MergeFlag::NoVerify, on))),
                flag(t!("git.merge_unrelated").to_string(), o.allow_unrelated)
                    .on_toggle(|on| tool(ToolMsg::MergeFlagSet(MergeFlag::AllowUnrelated, on))),
                field(t!("git.merge_message").to_string(), &o.message)
                    .on_input(|t| tool(ToolMsg::MergeMessage(t))),
                row![primary(
                    t!("git.merge").to_string(),
                    idle.then_some(tool(ToolMsg::MergeRun))
                ),]
                .spacing(8),
            ]
            .spacing(10)
            .into(),
            Dialog::Rebase(branch, o) => column![
                title(t!("git.rebase_title", name = branch.as_str()).to_string()),
                field(t!("git.rebase_onto_field").to_string(), &o.onto)
                    .on_input(|t| tool(ToolMsg::RebaseOnto(t))),
                flag(t!("git.rebase_merges").to_string(), o.rebase_merges)
                    .on_toggle(|on| tool(ToolMsg::RebaseFlagSet(RebaseFlag::Merges, on))),
                flag(t!("git.rebase_keep_empty").to_string(), o.keep_empty)
                    .on_toggle(|on| tool(ToolMsg::RebaseFlagSet(RebaseFlag::KeepEmpty, on))),
                flag(t!("git.rebase_root").to_string(), o.root)
                    .on_toggle(|on| tool(ToolMsg::RebaseFlagSet(RebaseFlag::Root, on))),
                flag(t!("git.rebase_update_refs").to_string(), o.update_refs)
                    .on_toggle(|on| tool(ToolMsg::RebaseFlagSet(RebaseFlag::UpdateRefs, on))),
                flag(t!("git.rebase_autosquash").to_string(), o.autosquash)
                    .on_toggle(|on| tool(ToolMsg::RebaseFlagSet(RebaseFlag::Autosquash, on))),
                row![primary(
                    t!("git.rebase").to_string(),
                    idle.then_some(tool(ToolMsg::RebaseRun))
                ),]
                .spacing(8),
            ]
            .spacing(10)
            .into(),
            Dialog::Messages(list) => {
                let mut rows = column![].spacing(1);
                for message in list {
                    let first = message.lines().next().unwrap_or("").to_string();
                    rows = rows.push(
                        button(
                            text(first)
                                .size(12.5)
                                .wrapping(iced::widget::text::Wrapping::None),
                        )
                        .padding([5, 8])
                        .width(Length::Fill)
                        .on_press(tool(ToolMsg::UseMessage(message.clone())))
                        .style(ghost(false)),
                    );
                }
                column![
                    title(t!("git.recent_messages").to_string()),
                    scrollable(rows)
                        .style(se_theme::Scrollbar { dark }.style())
                        .height(Length::Fixed(150.0)),
                ]
                .spacing(10)
                .into()
            }
            Dialog::Repos(lines, said) => {
                let mut rows = column![].spacing(2);
                for line in lines {
                    let mut state = line.branch.clone();
                    if line.ahead > 0 {
                        state.push_str(&format!(" ↑{}", line.ahead));
                    }
                    if line.behind > 0 {
                        state.push_str(&format!(" ↓{}", line.behind));
                    }
                    let changes = line.changes.to_string();
                    let current = self.repo.as_ref() == Some(&line.root);
                    rows = rows.push(
                        button(
                            row![
                                container(Space::new().width(8).height(8))
                                    .style(se_theme::dot(line.color)),
                                text(line.name.as_str())
                                    .size(12.5)
                                    .font(se_theme::UI_MEDIUM)
                                    .width(Length::Fill),
                                text(state).size(12).color(p.muted),
                                text(t!("git.repo_changes", n = changes.as_str()).to_string())
                                    .size(12)
                                    .color(p.muted),
                            ]
                            .spacing(8)
                            .align_y(Alignment::Center),
                        )
                        .padding([5, 8])
                        .width(Length::Fill)
                        .on_press(tool(ToolMsg::RepoPick(line.root.clone())))
                        .style(ghost(current)),
                    );
                }
                if lines.is_empty() {
                    rows = rows.push(
                        text(t!("git.no_repos").to_string())
                            .size(12.5)
                            .color(p.muted),
                    );
                }
                for (name, result) in said {
                    let (line, color) = match result {
                        Ok(()) => (
                            t!("git.repo_done", name = name.as_str()).to_string(),
                            p.muted,
                        ),
                        Err(e) => (format!("{name}: {e}"), se_theme::error_color(dark)),
                    };
                    rows = rows.push(text(line).size(12).color(color));
                }
                column![
                    title(t!("git.repos_title").to_string()),
                    scrollable(rows)
                        .style(se_theme::Scrollbar { dark }.style())
                        .height(Length::Fixed(120.0)),
                    row![
                        plain(
                            t!("git.fetch").to_string(),
                            Some(tool(ToolMsg::ReposRun(Bulk::Fetch)))
                        ),
                        plain(
                            t!("git.pull").to_string(),
                            Some(tool(ToolMsg::ReposRun(Bulk::Pull)))
                        ),
                        plain(
                            t!("git.push").to_string(),
                            Some(tool(ToolMsg::ReposRun(Bulk::Push)))
                        ),
                        Space::new().width(Length::Fill),
                        plain(
                            t!("git.repo_follow").to_string(),
                            Some(tool(ToolMsg::RepoFollow))
                        ),
                    ]
                    .spacing(6),
                ]
                .spacing(10)
                .into()
            }
            Dialog::Extract { paths, message, .. } => {
                let files = paths.len().to_string();
                column![
                    title(t!("git.extract_title", n = files.as_str()).to_string()),
                    field(t!("git.extract_message").to_string(), message)
                        .on_input(|t| tool(ToolMsg::ExtractMessage(t)))
                        .on_submit(tool(ToolMsg::ExtractRun)),
                    row![primary(
                        t!("git.extract_run").to_string(),
                        (idle && !message.trim().is_empty()).then_some(tool(ToolMsg::ExtractRun))
                    ),]
                    .spacing(8),
                ]
                .spacing(10)
                .into()
            }
        };
        // The panel can be low: what doesn't fit scrolls.
        let card = container(
            scrollable(container(body).width(Length::Fixed(520.0)).padding([0, 8]))
                .style(se_theme::Scrollbar { dark }.style()),
        )
        .padding(10)
        .style(se_theme::card(dark));
        iced::widget::mouse_area(
            container(card)
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .style(move |_: &iced::Theme| iced::widget::container::Style {
                    background: Some(iced::Color::from_rgba(0.0, 0.0, 0.0, 0.35).into()),
                    ..Default::default()
                }),
        )
        .into()
    }
}

/// What each project's repository stands at (one line by repository).
async fn repo_lines(roots: Vec<(PathBuf, iced::Color)>) -> Vec<RepoLine> {
    let mut lines: Vec<RepoLine> = Vec::new();
    for (root, color) in roots {
        let Some(repo) = git::repo_root(&root).await else {
            continue;
        };
        if lines.iter().any(|l| l.root == repo) {
            continue;
        }
        let Ok(status) = git::status(&repo, false).await else {
            continue;
        };
        lines.push(RepoLine {
            name: repo
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            color,
            branch: status.branch.clone(),
            upstream: status.upstream.clone(),
            ahead: status.ahead,
            behind: status.behind,
            changes: status.staged.len() + status.unstaged.len(),
            root: repo,
        });
    }
    lines
}

async fn bulk(line: &RepoLine, what: Bulk, mode: PullMode) -> Result<(), String> {
    let repo = &line.root;
    match what {
        Bulk::Fetch => git::fetch(repo).await,
        Bulk::Pull => git::pull(repo, mode, None).await,
        Bulk::Push => {
            history::push_branch(repo, &line.branch, line.upstream.as_deref(), false).await
        }
    }
}
