//! The Git menu of a file (right click on the file list or in the editor):
//! stage, commit, diff, compare with a branch, a tag or a revision, history.

use super::*;
use crate::git::history;
use crate::git::{Change, Kind, Source};
use crate::git_panel::{DiffTab, Entry, GitMsg, Panel, Tab};
use iced::widget::{column, row};

/// What the menu does with the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileAction {
    /// Adds the file to the index, or takes it out.
    Stage,
    Commit,
    Diff,
    CompareRef,
    CompareRevision,
    History,
    /// The history of the lines selected (or of the cursor's).
    HistorySelection,
    /// The file comes back as a branch, a tag or a revision has it.
    GetFromRevision,
    /// Adds the file to the `.gitignore` of its repository.
    Ignore,
}

/// What a comparison can be made against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickItem {
    pub label: String,
    pub detail: String,
    /// What git is given: a branch, a tag, a commit.
    pub rev: String,
}

/// The list to choose a branch, a tag or a revision from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pick {
    pub repo: PathBuf,
    /// The file, relative to the repository.
    pub file: String,
    pub revisions: bool,
    /// The choice brings the file back, rather than comparing with it.
    pub get: bool,
    pub items: Vec<PickItem>,
    pub filter: String,
}

pub(super) fn branch_input_id() -> iced::widget::Id {
    iced::widget::Id::new("git-branch-input")
}

fn pick_input_id() -> iced::widget::Id {
    iced::widget::Id::new("git-pick-input")
}

/// Branches, then tags, as things to compare with.
fn ref_items(refs: &history::Refs) -> Vec<PickItem> {
    let mut items = Vec::new();
    for branch in &refs.branches {
        items.push(PickItem {
            label: branch.name.clone(),
            detail: t!("git.pick_branch").to_string(),
            rev: branch.name.clone(),
        });
    }
    for tag in &refs.tags {
        items.push(PickItem {
            label: tag.clone(),
            detail: t!("git.pick_tag").to_string(),
            rev: tag.clone(),
        });
    }
    items
}

/// A commit, as something to pick.
fn revision_item(c: crate::git::graph::Commit) -> PickItem {
    PickItem {
        detail: format!("{} · {}", &c.sha[..c.sha.len().min(8)], c.author),
        label: c.subject,
        rev: c.sha,
    }
}

impl SimpleEditApp {
    /// The lines the history of a selection is about, from 1: the selected
    /// ones of the file in front, else the cursor's, else the first.
    fn lines_of(&self, path: &Path) -> (usize, usize) {
        if self.current_file.as_deref() != Some(path) {
            return (1, 1);
        }
        let (first, last) = self.lines_to_edit(self.selection_span());
        (first + 1, last + 1)
    }

    /// The list that switches branch from the status bar: type to filter,
    /// Enter takes the first, and a name no branch has can be made.
    pub(super) fn branch_switch_overlay(&self, dark: bool, filter: &str) -> Element<'_, Message> {
        let p = se_theme::palette(dark);
        let typed = filter.trim().to_lowercase();
        let current = self.git.status.as_ref().map(|s| s.branch.as_str());
        let mut matching: Vec<(&str, bool, bool)> = self
            .git
            .log
            .refs
            .as_ref()
            .map(|refs| {
                refs.branches
                    .iter()
                    .filter(|b| b.name.to_lowercase().contains(&typed))
                    .map(|b| (b.name.as_str(), b.remote, Some(b.name.as_str()) == current))
                    .collect()
            })
            .unwrap_or_default();
        // The local ones first.
        matching.sort_by_key(|(_, remote, _)| *remote);
        let exists = matching
            .iter()
            .any(|(name, ..)| name.to_lowercase() == typed);
        let mut list = column![].spacing(1);
        for (name, remote, is_current) in matching.iter().take(200) {
            list = list.push(
                button(
                    row![
                        text(if *is_current { "●" } else { " " })
                            .size(10)
                            .color(p.accent)
                            .width(14),
                        text(*name).size(13).width(Length::Fill),
                        text(if *remote {
                            t!("git.pick_remote").to_string()
                        } else {
                            String::new()
                        })
                        .size(11)
                        .color(p.muted),
                    ]
                    .align_y(iced::Alignment::Center),
                )
                .padding([6, 10])
                .width(Length::Fill)
                .on_press(Message::BranchSwitchChoose(name.to_string(), false))
                .style(
                    (se_theme::MenuItem {
                        dark,
                        radius: se_theme::R_SM,
                        highlighted: false,
                    })
                    .style(),
                ),
            );
        }
        // Nothing of that name: it can be made.
        let new_name = filter.split_whitespace().collect::<Vec<_>>().join("-");
        if !typed.is_empty() && !exists {
            list = list.push(
                button(
                    text(t!("git.switch_create", name = new_name.as_str()).to_string()).size(13),
                )
                .padding([6, 10])
                .width(Length::Fill)
                .on_press(Message::BranchSwitchChoose(new_name.clone(), true))
                .style(
                    (se_theme::MenuItem {
                        dark,
                        radius: se_theme::R_SM,
                        highlighted: false,
                    })
                    .style(),
                ),
            );
        }
        // Enter: the first of the list, or the new branch.
        let first = match matching.first() {
            Some((name, ..)) => Message::BranchSwitchChoose(name.to_string(), false),
            None => Message::BranchSwitchChoose(new_name, true),
        };
        container(
            column![
                text(t!("git.switch_title").to_string())
                    .size(14)
                    .font(se_theme::UI_SEMIBOLD)
                    .color(p.text),
                text_input(&t!("git.switch_placeholder"), filter)
                    .id(branch_input_id())
                    .on_input(Message::BranchSwitchFilter)
                    .on_submit(first)
                    .padding([7, 10])
                    .size(13)
                    .style(se_theme::Field::new(dark).style()),
                scrollable(container(list).padding([0, 2]))
                    .style(se_theme::Scrollbar { dark }.style())
                    .height(Length::Fixed(280.0)),
            ]
            .spacing(10)
            .padding(16),
        )
        .width(420)
        .style(se_theme::card(dark))
        .into()
    }

    pub(super) fn git_file_overlay(&self, dark: bool, path: &Path) -> Element<'_, Message> {
        let p = se_theme::palette(dark);
        let staged = self.git.repo.as_ref().is_some_and(|repo| {
            self.git.status.as_ref().is_some_and(|s| {
                s.staged
                    .iter()
                    .any(|c| repo.join(&c.path).as_path() == path)
            })
        });
        let item = |label: String, action: FileAction| {
            button(text(label).size(13))
                .padding([8, 12])
                .width(Length::Fill)
                .on_press(Message::GitFileDo(path.to_path_buf(), action))
                .style(
                    (se_theme::MenuItem {
                        dark,
                        radius: se_theme::R_SM,
                        highlighted: false,
                    })
                    .style(),
                )
        };
        container(
            column![
                text(format!("Git — {}", file_label(path)))
                    .size(14)
                    .font(se_theme::UI_SEMIBOLD)
                    .color(p.text),
                column![
                    item(
                        if staged {
                            t!("git.file_unstage").to_string()
                        } else {
                            t!("git.file_stage").to_string()
                        },
                        FileAction::Stage
                    ),
                    item(t!("git.file_commit").to_string(), FileAction::Commit),
                    item(t!("git.file_diff").to_string(), FileAction::Diff),
                    item(
                        t!("git.file_compare_ref").to_string(),
                        FileAction::CompareRef
                    ),
                    item(
                        t!("git.file_compare_revision").to_string(),
                        FileAction::CompareRevision
                    ),
                    item(t!("git.file_history").to_string(), FileAction::History),
                    item(
                        t!("git.file_history_selection").to_string(),
                        FileAction::HistorySelection
                    ),
                    item(
                        t!("git.file_get_revision").to_string(),
                        FileAction::GetFromRevision
                    ),
                    item(t!("git.ignore_file").to_string(), FileAction::Ignore),
                ]
                .spacing(1),
            ]
            .spacing(10)
            .padding(16),
        )
        .width(320)
        .style(se_theme::card(dark))
        .into()
    }

    pub(super) fn git_pick_overlay<'a>(
        &'a self,
        dark: bool,
        pick: &'a Pick,
    ) -> Element<'a, Message> {
        let p = se_theme::palette(dark);
        let filter = pick.filter.to_lowercase();
        let mut list = column![].spacing(1);
        let mut shown = 0;
        for item in pick
            .items
            .iter()
            .filter(|i| {
                filter.is_empty()
                    || i.label.to_lowercase().contains(&filter)
                    || i.detail.to_lowercase().contains(&filter)
                    || i.rev.to_lowercase().starts_with(&filter)
            })
            .take(200)
        {
            shown += 1;
            list = list.push(
                button(
                    row![
                        text(item.label.as_str())
                            .size(13)
                            .width(Length::Fill)
                            .wrapping(iced::widget::text::Wrapping::None),
                        text(item.detail.as_str())
                            .size(11.5)
                            .color(p.muted)
                            .wrapping(iced::widget::text::Wrapping::None),
                    ]
                    .spacing(10)
                    .align_y(Alignment::Center),
                )
                .padding([6, 10])
                .width(Length::Fill)
                .on_press(Message::GitPickChoose(item.rev.clone()))
                .style(
                    (se_theme::MenuItem {
                        dark,
                        radius: se_theme::R_SM,
                        highlighted: false,
                    })
                    .style(),
                ),
            );
        }
        if shown == 0 {
            list = list.push(
                text(t!("git.pick_none").to_string())
                    .size(12.5)
                    .color(p.muted),
            );
        }
        container(
            column![
                text(if pick.get {
                    t!("git.pick_get_title", file = pick.file.as_str()).to_string()
                } else if pick.revisions {
                    t!("git.pick_revision_title", file = pick.file.as_str()).to_string()
                } else {
                    t!("git.pick_ref_title", file = pick.file.as_str()).to_string()
                })
                .size(14)
                .font(se_theme::UI_SEMIBOLD)
                .color(p.text),
                text_input(&t!("git.pick_placeholder"), &pick.filter)
                    .id(pick_input_id())
                    .on_input(Message::GitPickFilter)
                    .on_submit(Message::GitPickChoose(pick.filter.trim().to_string()))
                    .padding([7, 10])
                    .size(13)
                    .style(se_theme::Field::new(dark).style()),
                scrollable(container(list).padding([0, 2]))
                    .style(se_theme::Scrollbar { dark }.style())
                    .height(Length::Fixed(280.0)),
            ]
            .spacing(10)
            .padding(16),
        )
        .width(520)
        .style(se_theme::card(dark))
        .into()
    }

    /// The Git menu was chosen from a file's menu: find its repository.
    pub(super) fn git_file_do(&mut self, path: PathBuf, action: FileAction) -> Task<Message> {
        self.overlay = ActiveOverlay::None;
        Task::perform(
            async move {
                let root = crate::git::repo_root(&path).await;
                (path, root)
            },
            move |(path, root)| Message::GitFileRoot(path, action, root),
        )
    }

    pub(super) fn git_file_root(
        &mut self,
        path: PathBuf,
        action: FileAction,
        root: Option<PathBuf>,
    ) -> Task<Message> {
        let Some(root) = root else {
            self.set_status(t!("git.file_no_repo"), true);
            return Task::none();
        };
        // The path as the repository names it.
        let canon = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
        let Ok(rel) = canon(&path)
            .strip_prefix(canon(&root))
            .map(Path::to_path_buf)
        else {
            self.set_status(t!("git.file_no_repo"), true);
            return Task::none();
        };
        let rel = rel.to_string_lossy().replace('\\', "/");
        match action {
            FileAction::Stage => Task::perform(
                async move {
                    let listed =
                        crate::git::run(&root, &["diff", "--cached", "--name-only", "--", &rel])
                            .await?;
                    let staged = !listed.trim().is_empty();
                    if staged {
                        crate::git::unstage(&root, &[rel]).await?;
                        Ok(t!("git.file_unstaged").to_string())
                    } else {
                        crate::git::stage(&root, &[rel]).await?;
                        Ok(t!("git.file_staged").to_string())
                    }
                },
                Message::GitFileDone,
            ),
            FileAction::Commit => {
                self.show_git_panel(root);
                self.git.tab = Tab::Commit;
                self.git.only = Some(rel);
                self.git.typing = false;
                Task::done(Message::Git(GitMsg::Refresh))
            }
            FileAction::Ignore => Task::perform(
                async move {
                    let untracked = crate::git::ignore_file(&root, &rel).await?;
                    Ok(if untracked {
                        t!("git.file_ignored_untracked").to_string()
                    } else {
                        t!("git.file_ignored").to_string()
                    })
                },
                Message::GitFileDone,
            ),
            FileAction::History => {
                self.show_git_panel(root);
                self.git.tab = Tab::Log;
                self.git.log.file = Some(rel);
                // So that the list is read again for this file.
                self.git.log.shown = None;
                Task::done(Message::Git(GitMsg::SelectTab(Tab::Log)))
            }
            FileAction::HistorySelection => {
                let (first, last) = self.lines_of(&path);
                self.show_git_panel(root);
                self.git.tab = Tab::Log;
                self.git.log.file = None;
                self.git.log.narrow.lines = Some((rel, first, last));
                self.git.log.shown = None;
                Task::done(Message::Git(GitMsg::SelectTab(Tab::Log)))
            }
            FileAction::GetFromRevision => Task::perform(
                async move {
                    let refs = history::refs(&root).await?;
                    let mut items = ref_items(&refs);
                    let commits = history::log(&root, "HEAD", 300, Some(&rel)).await?;
                    items.extend(commits.into_iter().map(revision_item));
                    Ok(Pick {
                        repo: root,
                        file: rel,
                        revisions: false,
                        get: true,
                        items,
                        filter: String::new(),
                    })
                },
                Message::GitFilePick,
            ),
            FileAction::Diff => Task::perform(
                async move {
                    let status = crate::git::status(&root, true).await?;
                    let mut entries = Panel::commit_entries(&status);
                    entries.retain(|e| e.change.path == rel);
                    if entries.is_empty() {
                        return Err(t!("git.file_no_changes").to_string());
                    }
                    Ok(DiffTab {
                        repo: root,
                        title: rel.clone(),
                        entries,
                        start: 0,
                    })
                },
                Message::GitFileDiff,
            ),
            FileAction::CompareRef => Task::perform(
                async move {
                    let refs = history::refs(&root).await?;
                    Ok(Pick {
                        repo: root,
                        file: rel,
                        revisions: false,
                        get: false,
                        items: ref_items(&refs),
                        filter: String::new(),
                    })
                },
                Message::GitFilePick,
            ),
            FileAction::CompareRevision => Task::perform(
                async move {
                    let commits = history::log(&root, "HEAD", 300, Some(&rel)).await?;
                    let items = commits.into_iter().map(revision_item).collect();
                    Ok(Pick {
                        repo: root,
                        file: rel,
                        revisions: true,
                        get: false,
                        items,
                        filter: String::new(),
                    })
                },
                Message::GitFilePick,
            ),
        }
    }

    /// Opens the Git panel on the repository of the file.
    fn show_git_panel(&mut self, root: PathBuf) {
        self.git.show_repo(root, self.current_file.clone());
        self.terminal.open = false;
        self.terminal.focused = false;
        self.scratch.open = false;
        self.bookmark_panel.open = false;
    }

    pub(super) fn git_file_done(&mut self, result: Result<String, String>) -> Task<Message> {
        match result {
            Ok(said) => self.set_status(said, false),
            Err(e) => self.set_status(format!("{}: {e}", t!("status.error")), true),
        }
        Task::done(Message::Git(GitMsg::Refresh))
    }

    pub(super) fn git_file_diff(&mut self, result: Result<DiffTab, String>) -> Task<Message> {
        match result {
            Ok(tab) => self.open_diff_tab(tab),
            Err(e) => {
                self.set_status(e, false);
                Task::none()
            }
        }
    }

    pub(super) fn git_file_pick(&mut self, result: Result<Pick, String>) -> Task<Message> {
        match result {
            Ok(pick) => {
                self.overlay = ActiveOverlay::GitPick(Box::new(pick));
                iced::widget::operation::focus(pick_input_id())
            }
            Err(e) => {
                self.set_status(format!("{}: {e}", t!("status.error")), true);
                Task::none()
            }
        }
    }

    pub(super) fn git_pick_choose(&mut self, rev: String) -> Task<Message> {
        let ActiveOverlay::GitPick(pick) = &self.overlay else {
            return Task::none();
        };
        if rev.is_empty() {
            return Task::none();
        }
        let (repo, file, get) = (pick.repo.clone(), pick.file.clone(), pick.get);
        self.overlay = ActiveOverlay::None;
        // Typed by hand: checked before git reads it (it could be an option).
        Task::perform(
            async move {
                crate::git::verify_rev(&repo, &rev).await?;
                Ok((repo, file, rev, get))
            },
            Message::GitPickVerified,
        )
    }

    pub(super) fn git_pick_verified(
        &mut self,
        result: Result<(PathBuf, String, String, bool), String>,
    ) -> Task<Message> {
        let (repo, file, rev, get) = match result {
            Ok(found) => found,
            Err(e) => {
                self.set_status(e, true);
                return Task::none();
            }
        };
        if get {
            return Task::perform(
                async move {
                    crate::git::tools::get_from_revision(&repo, &rev, &file).await?;
                    Ok(t!("git.file_got").to_string())
                },
                Message::GitFileDone,
            );
        }
        let short = rev.chars().take(8).collect::<String>();
        let title = format!("{file} ↔ {short}");
        self.open_diff_tab(DiffTab {
            repo,
            title,
            entries: vec![Entry {
                source: Source::Tree(rev),
                change: Change {
                    path: file,
                    from: None,
                    kind: Kind::Modified,
                },
                staged: false,
            }],
            start: 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> SimpleEditApp {
        let mut app = SimpleEditApp::blank(Config::default());
        let _ = app.restore_session(crate::session::Session::default());
        app
    }

    #[test]
    fn a_file_in_the_back_has_its_first_line_for_the_history_of_a_selection() {
        let app = app();
        assert_eq!(app.lines_of(Path::new("/not/in/front")), (1, 1));
    }

    #[test]
    fn a_blame_option_changes_and_hidden_commits_are_restored() {
        let mut app = app();
        let _ = app.update(Message::BlameOption(BlameOption::Blanks));
        assert!(app.blame_options.ignore_blanks);
        let _ = app.update(Message::BlameOption(BlameOption::MovesInFile));
        let _ = app.update(Message::BlameOption(BlameOption::MovesAcross));
        assert!(app.blame_options.moves_in_file && app.blame_options.moves_across);
        app.blame_options.hidden.push("sha".into());
        let _ = app.update(Message::BlameOption(BlameOption::ShowHidden));
        assert!(app.blame_options.hidden.is_empty());
    }

    #[tokio::test]
    async fn a_revision_picked_to_get_a_file_restores_it_instead_of_comparing() {
        let repo = crate::git::testing::Repo::new();
        std::fs::write(repo.path().join("f"), "old").unwrap();
        repo.git(&["add", "."]);
        repo.git(&["commit", "-q", "-m", "one"]);
        let old = repo.git(&["rev-parse", "HEAD"]).trim().to_string();
        std::fs::write(repo.path().join("f"), "new").unwrap();
        repo.git(&["commit", "-qam", "two"]);
        let mut app = app();
        let tabs = app.sidebar.files().len();
        let _ = app.update(Message::GitPickVerified(Ok((
            repo.path().to_path_buf(),
            "f".into(),
            old,
            true,
        ))));
        assert_eq!(app.sidebar.files().len(), tabs, "no diff tab opens");
    }

    /// Draws the panel and two dialogs, to look at them without a screen:
    /// `SNAPSHOT=/tmp/x cargo test git_look -- --ignored`.
    #[test]
    #[ignore]
    fn git_look() {
        use crate::git::tools::PushSpec;
        use crate::git_panel::tools::{Dialog, PushDialog};
        let Ok(out) = std::env::var("SNAPSHOT") else {
            return;
        };
        let mut app = app();
        app.git.open = true;
        app.git.repo = Some(PathBuf::from("/repo"));
        app.git.searched = true;
        app.git.status = Some(crate::git::Status {
            branch: "main".into(),
            upstream: Some("origin/main".into()),
            ahead: 2,
            ..Default::default()
        });
        let commit = |sha: &str, subject: &str| crate::git::graph::Commit {
            sha: sha.into(),
            parents: Vec::new(),
            author: "me".into(),
            email: "me@x".into(),
            time: 0,
            subject: subject.into(),
            refs: Vec::new(),
        };
        let shoot = |app: &SimpleEditApp, name: &str| {
            let settings = iced::Settings {
                default_font: se_theme::UI,
                fonts: se_theme::font_files(),
                ..Default::default()
            };
            let mut ui = iced_test::Simulator::with_size(settings, (1280.0, 800.0), app.view());
            let shot = ui.snapshot(&se_theme::ink_dark()).unwrap();
            let path = format!("{out}-{name}.png");
            let _ = std::fs::remove_file(format!("{out}-{name}-wgpu.png"));
            assert!(shot.matches_image(&path).unwrap());
        };
        shoot(&app, "commit");
        app.git.dialog = Some(Dialog::Push(PushDialog {
            remotes: vec!["origin".into(), "upstream".into()],
            spec: PushSpec {
                remote: "origin".into(),
                source: "main".into(),
                target: "main".into(),
                ..PushSpec::default()
            },
            commits: vec![
                commit("0123456789", "feat: a new thing"),
                commit("abcdef0123", "fix: the old thing"),
            ],
            force_asked: false,
        }));
        shoot(&app, "push");
        app.git.dialog = None;
        app.git.tab = Tab::Log;
        app.git.log.filters_shown = true;
        let _ = app.update(Message::Git(GitMsg::LogLoaded(
            "HEAD".into(),
            Ok((
                vec![commit("0123456789", "feat: a new thing")],
                Default::default(),
            )),
        )));
        app.git.log.shown = Some("HEAD".into());
        let _ = app.update(Message::Git(GitMsg::LogLoaded(
            "HEAD".into(),
            Ok((
                vec![commit("0123456789", "feat: a new thing")],
                Default::default(),
            )),
        )));
        shoot(&app, "log");
    }
}
