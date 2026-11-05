//! The Branches tab: local and remote branches and tags, and what can be
//! done to them: create, switch, merge, rebase, push, delete.

use iced::widget::{button, column, container, row, scrollable, text, text_input, Space};
use iced::{Alignment, Element, Length, Task};

use super::{GitMsg, Panel};
use crate::app::Message;
use crate::git;
use crate::git::history::{self, Branch};
use crate::theme as se_theme;

/// What is picked in the lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ref {
    Branch(String),
    Tag(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefAction {
    Checkout,
    /// The picked branch into the current one.
    Merge,
    /// The current branch on top of the picked one.
    Rebase,
    /// `git pull` as the settings say: the current branch's upstream, or the
    /// remote branch that is picked.
    Pull,
    /// Gets what the remotes have, without touching the files.
    Fetch,
    Push,
    ForcePush,
    /// A tag: sent to origin, or removed from it.
    PushTag,
    DeleteRemoteTag,
    CreateTag,
    Delete,
    Create,
}

impl RefAction {
    /// Those that can't be undone ask for a second click.
    fn asks_twice(self) -> bool {
        matches!(
            self,
            RefAction::ForcePush | RefAction::Delete | RefAction::DeleteRemoteTag
        )
    }
}

/// A button with options behind its chevron: the button does the action as
/// the mode shown says, the chevron opens the modes to choose from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Split {
    Pull,
    Merge,
}

#[derive(Debug, Default)]
pub struct State {
    /// The split button whose modes are open, and where the window was
    /// pressed to open them (they open there).
    pub menu: Option<(Split, iced::Point)>,
    pub selected: Option<Ref>,
    /// The name typed for a new branch.
    pub name: String,
    /// The name and the message (an annotated tag) typed for a new tag.
    pub tag_name: String,
    pub tag_message: String,
    /// The action waiting for its second click.
    pub confirm: Option<RefAction>,
    /// The popin open to name a new branch or tag.
    pub dialog: Option<RefDialog>,
}

/// What the popin of the Branches tab creates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefDialog {
    Branch,
    Tag,
}

/// Width of the modes of a split button.
pub const SPLIT_OPTIONS_WIDTH: f32 = 250.0;

/// The mode a split button will use, in a word.
fn short_pull(mode: git::PullMode) -> String {
    match mode {
        git::PullMode::Merge => t!("git.short_merge"),
        git::PullMode::Rebase => t!("git.short_rebase"),
        git::PullMode::FastForwardOnly => t!("git.short_ff_only"),
    }
    .to_string()
}

fn short_merge(mode: git::MergeMode) -> String {
    match mode {
        git::MergeMode::Default => t!("git.short_default"),
        git::MergeMode::NoFastForward => t!("git.short_no_ff"),
        git::MergeMode::FastForwardOnly => t!("git.short_ff_only"),
        git::MergeMode::Squash => t!("git.short_squash"),
    }
    .to_string()
}

/// Git refuses blanks in a name: dashes instead.
fn branch_name(typed: &str) -> String {
    typed.split_whitespace().collect::<Vec<_>>().join("-")
}

impl Panel {
    fn picked_branch(&self) -> Option<&Branch> {
        let Some(Ref::Branch(name)) = &self.branches.selected else {
            return None;
        };
        self.log
            .refs
            .as_ref()?
            .branches
            .iter()
            .find(|b| b.name == *name)
    }

    /// Whether the picked ref (and the state of the repository) allow it.
    pub fn can(&self, action: RefAction) -> bool {
        if self.busy || self.repo.is_none() {
            return false;
        }
        let branch = self.picked_branch();
        let idle = self.status.as_ref().is_some_and(|s| s.operation.is_none());
        match action {
            RefAction::Checkout => match &self.branches.selected {
                Some(Ref::Tag(_)) => true,
                _ => branch.is_some_and(|b| !b.current),
            },
            RefAction::Merge | RefAction::Rebase => branch.is_some_and(|b| !b.current) && idle,
            RefAction::Pull => idle,
            RefAction::Fetch => true,
            RefAction::PushTag | RefAction::DeleteRemoteTag => {
                matches!(self.branches.selected, Some(Ref::Tag(_)))
            }
            RefAction::CreateTag => !self.branches.tag_name.trim().is_empty(),
            RefAction::Push | RefAction::ForcePush => branch.is_some_and(|b| !b.remote),
            RefAction::Delete => match &self.branches.selected {
                Some(Ref::Tag(_)) => true,
                Some(Ref::Branch(_)) => branch.is_some_and(|b| !b.current),
                None => false,
            },
            RefAction::Create => !branch_name(&self.branches.name).is_empty(),
        }
    }

    pub(super) fn ref_action(&mut self, action: RefAction) -> Task<Message> {
        let (pull_mode, merge_mode) = (self.pull_mode, self.merge_mode);
        if !self.can(action) {
            return Task::none();
        }
        if action.asks_twice() && self.branches.confirm != Some(action) {
            self.branches.confirm = Some(action);
            return Task::none();
        }
        self.branches.confirm = None;
        let selected = self.branches.selected.clone();
        let branch = self.picked_branch().cloned();
        match (action, selected, branch) {
            (RefAction::Fetch, ..) => self.act(move |repo| async move {
                git::fetch(&repo).await?;
                Ok("\u{7}fetched".to_string())
            }),
            (RefAction::PushTag, Some(Ref::Tag(name)), _) => self.act(move |repo| async move {
                history::push_tag(&repo, &name).await?;
                Ok("\u{3}pushed".to_string())
            }),
            (RefAction::DeleteRemoteTag, Some(Ref::Tag(name)), _) => {
                self.act(move |repo| async move {
                    history::delete_remote_tag(&repo, &name).await?;
                    Ok(String::new())
                })
            }
            (RefAction::CreateTag, selected, _) => {
                let name = branch_name(&self.branches.tag_name);
                let message = std::mem::take(&mut self.branches.tag_message);
                let start = selected.map(|r| match r {
                    Ref::Branch(n) | Ref::Tag(n) => n,
                });
                self.branches.tag_name.clear();
                self.act(move |repo| async move {
                    history::create_tag(&repo, &name, start.as_deref(), Some(&message)).await?;
                    Ok(format!("\u{9}{name}"))
                })
            }
            (RefAction::Checkout, Some(Ref::Branch(name)), _) => self.checkout(name),
            (RefAction::Checkout, Some(Ref::Tag(name)), _) => self.act(move |repo| async move {
                history::checkout_ref(&repo, &name).await?;
                Ok(String::new())
            }),
            (RefAction::Merge, Some(Ref::Branch(name)), _) => self.act(move |repo| async move {
                crate::git::merge_branch(&repo, &name, merge_mode).await?;
                Ok(String::new())
            }),
            (RefAction::Rebase, Some(Ref::Branch(name)), _) => self.act(move |repo| async move {
                crate::git::rebase_onto(&repo, &name).await?;
                Ok(String::new())
            }),
            (RefAction::Pull, _, picked) => {
                // A remote branch that is picked is what comes in.
                let from = picked.filter(|b| b.remote).and_then(|b| {
                    b.name
                        .split_once('/')
                        .map(|(r, n)| (r.to_string(), n.to_string()))
                });
                self.act(move |repo| async move {
                    let from = from.as_ref().map(|(r, n)| (r.as_str(), n.as_str()));
                    crate::git::pull(&repo, pull_mode, from).await?;
                    Ok("\u{6}pulled".to_string())
                })
            }
            (RefAction::Push | RefAction::ForcePush, _, Some(b)) => {
                let force = action == RefAction::ForcePush;
                self.act(move |repo| async move {
                    history::push_branch(&repo, &b.name, b.upstream.as_deref(), force).await?;
                    Ok("\u{3}pushed".to_string())
                })
            }
            (RefAction::Delete, Some(Ref::Tag(name)), _) => {
                self.branches.selected = None;
                self.act(move |repo| async move {
                    history::delete_tag(&repo, &name).await?;
                    Ok(String::new())
                })
            }
            (RefAction::Delete, _, Some(b)) => {
                self.branches.selected = None;
                self.act(move |repo| async move {
                    if b.remote {
                        history::delete_remote_branch(&repo, &b.name).await?;
                    } else {
                        history::delete_branch(&repo, &b.name).await?;
                    }
                    Ok(String::new())
                })
            }
            (RefAction::Create, selected, _) => {
                let name = branch_name(&self.branches.name);
                let start = selected.map(|r| match r {
                    Ref::Branch(n) | Ref::Tag(n) => n,
                });
                self.branches.name.clear();
                self.act(move |repo| async move {
                    history::create_branch(&repo, &name, start.as_deref()).await?;
                    Ok(format!("\u{4}{name}"))
                })
            }
            _ => Task::none(),
        }
    }

    /// A button in two parts: the action (with the mode it will use shown
    /// beside it) and a chevron that opens the modes to choose from.
    fn split_button(&self, label: String, which: Split, dark: bool) -> Element<'_, Message> {
        let (action, mode) = match which {
            Split::Pull => (RefAction::Pull, short_pull(self.pull_mode)),
            Split::Merge => (RefAction::Merge, short_merge(self.merge_mode)),
        };
        let p = se_theme::palette(dark);
        let open = self.branches.menu.is_some_and(|(w, _)| w == which);
        let mut main = button(
            row![
                text(label)
                    .size(12.5)
                    .width(Length::Fill)
                    .wrapping(iced::widget::text::Wrapping::None),
                text(mode).size(11.5).color(p.muted),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .padding([4, 12])
        .width(Length::Fill)
        .style(
            (se_theme::GhostButton {
                dark,
                active: false,
            })
            .style(),
        );
        if self.can(action) {
            main = main.on_press(Message::Git(GitMsg::RefAction(action)));
        }
        // The chevron is a button that only looks it: the area over it hears
        // the press and where it was (the modes open there). Open, a click
        // anywhere else closes them and the chevron must not reopen them.
        let chevron = button(text(if open { "▴" } else { "▾" }).size(11))
            .padding([4, 8])
            .on_press(Message::Git(GitMsg::Noop))
            .style((se_theme::GhostButton { dark, active: open }).style());
        let chevron = crate::pointer_area::pointer_area(chevron)
            .watching()
            .on_press_in_window(move |at| {
                Message::Git(if open {
                    GitMsg::CloseSplit
                } else {
                    GitMsg::OpenSplit(which, at)
                })
            });
        row![main, chevron].spacing(2).into()
    }

    /// The modes of the open split button, and where they go in the window
    /// (a layer over everything, so they push nothing aside).
    pub fn split_overlay(&self, dark: bool) -> Option<(iced::Point, Element<'_, Message>)> {
        let (which, at) = self.branches.menu?;
        Some((at, self.split_options(which, dark)))
    }

    /// The modes of an open split button: choosing one makes it the mode of
    /// the button (and the default of the Preferences); nothing is run yet.
    fn split_options(&self, which: Split, dark: bool) -> Element<'_, Message> {
        let p = se_theme::palette(dark);
        let option = |label: String, chosen: bool, msg: Message| {
            button(
                row![
                    text(if chosen { "✓" } else { " " })
                        .size(11)
                        .color(p.accent)
                        .width(14),
                    text(label).size(12.5),
                ]
                .align_y(Alignment::Center),
            )
            .padding([3, 10])
            .width(Length::Fill)
            .on_press(msg)
            .style(
                (se_theme::GhostButton {
                    dark,
                    active: chosen,
                })
                .style(),
            )
        };
        let mut list = column![].spacing(1);
        match which {
            Split::Pull => {
                for mode in git::PullMode::ALL {
                    list = list.push(option(
                        mode.to_string(),
                        mode == self.pull_mode,
                        Message::SetPullMode(mode),
                    ));
                }
            }
            Split::Merge => {
                for mode in git::MergeMode::ALL {
                    list = list.push(option(
                        mode.to_string(),
                        mode == self.merge_mode,
                        Message::SetMergeMode(mode),
                    ));
                }
            }
        }
        container(list)
            .padding([4, 4])
            .width(Length::Fixed(SPLIT_OPTIONS_WIDTH))
            .style(se_theme::card(dark))
            .into()
    }

    pub(super) fn branches_tab<'a>(&'a self, dark: bool) -> Element<'a, Message> {
        let p = se_theme::palette(dark);
        let current = self
            .status
            .as_ref()
            .map(|s| s.branch.clone())
            .unwrap_or_default();

        let item = |label: Element<'a, Message>, selected: bool, msg: GitMsg| {
            button(label)
                .padding([3, 8])
                .width(Length::Fill)
                .on_press(Message::Git(msg))
                .style(
                    (se_theme::GhostButton {
                        dark,
                        active: selected,
                    })
                    .style(),
                )
        };
        let title = |label: String| {
            container(
                text(label)
                    .size(11.5)
                    .font(se_theme::UI_SEMIBOLD)
                    .color(p.muted),
            )
            .padding([8, 8])
        };

        let mut list = column![].spacing(1);
        if let Some(refs) = &self.log.refs {
            for (remote, heading) in [
                (false, t!("git.branches_local")),
                (true, t!("git.branches_remote")),
            ] {
                let mut group = refs
                    .branches
                    .iter()
                    .filter(|b| b.remote == remote)
                    .peekable();
                if group.peek().is_none() {
                    continue;
                }
                list = list.push(title(heading.to_string()));
                for b in group {
                    let selected = self.branches.selected == Some(Ref::Branch(b.name.clone()));
                    let mut line = row![
                        text(if b.current { "●" } else { " " })
                            .size(9)
                            .color(p.accent)
                            .width(10),
                        text(b.name.as_str()).size(12.5),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center);
                    if let Some(up) = &b.upstream {
                        line = line.push(text(up.as_str()).size(11.5).color(p.muted));
                    }
                    if b.ahead > 0 {
                        line = line.push(text(format!("↑{}", b.ahead)).size(11).color(p.accent));
                    }
                    if b.behind > 0 {
                        line = line.push(text(format!("↓{}", b.behind)).size(11).color(p.muted));
                    }
                    list = list.push(item(
                        line.into(),
                        selected,
                        GitMsg::SelectRef(Ref::Branch(b.name.clone())),
                    ));
                }
            }
            if !refs.tags.is_empty() {
                list = list.push(title(t!("git.branches_tags").to_string()));
                for tag in &refs.tags {
                    let selected = self.branches.selected == Some(Ref::Tag(tag.clone()));
                    list = list.push(item(
                        row![Space::new().width(16), text(tag.as_str()).size(12.5)].into(),
                        selected,
                        GitMsg::SelectRef(Ref::Tag(tag.clone())),
                    ));
                }
            }
        }

        let act = |label: String, action: RefAction, active: bool| {
            let mut b = button(text(label).size(12.5))
                .padding([4, 12])
                .width(Length::Fill);
            if self.can(action) {
                b = b.on_press(Message::Git(GitMsg::RefAction(action)));
            }
            b.style((se_theme::GhostButton { dark, active }).style())
        };
        let act_msg = |label: String, msg: GitMsg| {
            button(text(label).size(12.5))
                .padding([4, 12])
                .width(Length::Fill)
                .on_press(Message::Git(msg))
                .style(
                    (se_theme::GhostButton {
                        dark,
                        active: false,
                    })
                    .style(),
                )
        };
        let confirming = |action: RefAction| self.branches.confirm == Some(action);
        let picked = match &self.branches.selected {
            Some(Ref::Branch(n) | Ref::Tag(n)) => n.clone(),
            None => String::new(),
        };
        let picked_from = picked.clone();
        let heading = if picked.is_empty() {
            t!("git.branch_pick").to_string()
        } else {
            picked.clone()
        };
        let create_label = if picked.is_empty() {
            t!("git.new_branch_here").to_string()
        } else {
            t!("git.new_branch_from", from = picked.as_str()).to_string()
        };
        let tag_label = if picked.is_empty() {
            t!("git.new_tag_here").to_string()
        } else {
            t!("git.new_tag_from", from = picked.as_str()).to_string()
        };
        let notice: Element<Message> = match &self.notice {
            Some((message, is_error)) => {
                button(text(message.as_str()).size(12).color(if *is_error {
                    se_theme::error_color(dark)
                } else {
                    p.muted
                }))
                .padding(0)
                .on_press(Message::Git(GitMsg::DismissNotice))
                .style(|_: &iced::Theme, _| iced::widget::button::Style::default())
                .into()
            }
            None => Space::new().into(),
        };
        let confirm_or = |action: RefAction, label: String| {
            if confirming(action) {
                t!("git.branch_confirm").to_string()
            } else {
                label
            }
        };
        let pull_label = match &self.branches.selected {
            Some(Ref::Branch(n)) if self.picked_branch().is_some_and(|b| b.remote) => {
                t!("git.pull_from", name = n.as_str()).to_string()
            }
            _ => t!("git.pull").to_string(),
        };
        let actions = column![
            text(heading)
                .size(13)
                .font(se_theme::UI_SEMIBOLD)
                .color(p.text),
            act(
                t!("git.branch_checkout").to_string(),
                RefAction::Checkout,
                false
            ),
            act(t!("git.fetch").to_string(), RefAction::Fetch, false),
            self.split_button(pull_label, Split::Pull, dark),
            self.split_button(
                t!("git.branch_merge", current = current.as_str()).to_string(),
                Split::Merge,
                dark
            ),
            act(
                t!("git.branch_rebase", current = current.as_str()).to_string(),
                RefAction::Rebase,
                false
            ),
            act(t!("git.push").to_string(), RefAction::Push, false),
            act(
                if confirming(RefAction::ForcePush) {
                    t!("git.force_push_confirm").to_string()
                } else {
                    t!("git.force_push").to_string()
                },
                RefAction::ForcePush,
                confirming(RefAction::ForcePush)
            ),
            act(
                confirm_or(RefAction::Delete, t!("git.branch_delete").to_string()),
                RefAction::Delete,
                confirming(RefAction::Delete)
            ),
            act(t!("git.push_tag").to_string(), RefAction::PushTag, false),
            act(
                confirm_or(
                    RefAction::DeleteRemoteTag,
                    t!("git.delete_remote_tag").to_string()
                ),
                RefAction::DeleteRemoteTag,
                confirming(RefAction::DeleteRemoteTag)
            ),
            container(Space::new().height(1))
                .width(Length::Fill)
                .style(se_theme::rule(dark)),
            act_msg(create_label, GitMsg::RefDialog(Some(RefDialog::Branch))),
            act_msg(tag_label, GitMsg::RefDialog(Some(RefDialog::Tag))),
            notice,
        ]
        .spacing(4)
        .width(Length::Fixed(280.0));

        let page: Element<Message> = row![
            scrollable(container(list).padding([2, 6]))
                .style(se_theme::Scrollbar { dark }.style())
                .width(Length::Fill)
                .height(Length::Fill),
            container(Space::new().width(1).height(Length::Fill)).style(se_theme::rule(dark)),
            scrollable(container(actions).padding([10, 12]))
                .style(se_theme::Scrollbar { dark }.style())
                .height(Length::Fill),
        ]
        .into();
        match self.branches.dialog {
            Some(kind) => {
                iced::widget::stack![page, self.ref_dialog(kind, picked_from, dark)].into()
            }
            None => page,
        }
    }

    /// The popin to name a new branch or tag.
    fn ref_dialog(&self, kind: RefDialog, from: String, dark: bool) -> Element<'_, Message> {
        let p = se_theme::palette(dark);
        let field = |placeholder: String, value: &str, on_input: fn(String) -> GitMsg| {
            text_input(&placeholder, value)
                .on_input(move |t| Message::Git(on_input(t)))
                .on_submit(Message::Git(GitMsg::RefDialogOk))
                .size(13)
                .padding([6, 10])
                .style(se_theme::Field::new(dark).style())
        };
        let (title, fields): (String, Element<Message>) = match kind {
            RefDialog::Branch => (
                if from.is_empty() {
                    t!("git.new_branch_here").to_string()
                } else {
                    t!("git.new_branch_from", from = from.as_str()).to_string()
                },
                field(
                    t!("git.new_branch_name").to_string(),
                    &self.branches.name,
                    GitMsg::RefName,
                )
                .id(ref_dialog_field())
                .into(),
            ),
            RefDialog::Tag => (
                if from.is_empty() {
                    t!("git.new_tag_here").to_string()
                } else {
                    t!("git.new_tag_from", from = from.as_str()).to_string()
                },
                column![
                    field(
                        t!("git.new_tag_name").to_string(),
                        &self.branches.tag_name,
                        GitMsg::RefTagName
                    )
                    .id(ref_dialog_field()),
                    field(
                        t!("git.new_tag_message").to_string(),
                        &self.branches.tag_message,
                        GitMsg::RefTagMessage
                    ),
                ]
                .spacing(8)
                .into(),
            ),
        };
        let ok = match kind {
            RefDialog::Branch => RefAction::Create,
            RefDialog::Tag => RefAction::CreateTag,
        };
        let form = column![
            text(title)
                .size(13)
                .font(se_theme::UI_SEMIBOLD)
                .color(p.text),
            fields,
            row![
                button(text(t!("git.create").to_string()).size(12.5))
                    .padding([5, 14])
                    .on_press_maybe(self.can(ok).then_some(Message::Git(GitMsg::RefDialogOk)))
                    .style((se_theme::Primary { dark }).style()),
                button(text(t!("git.cancel").to_string()).size(12.5))
                    .padding([5, 14])
                    .on_press(Message::Git(GitMsg::RefDialog(None)))
                    .style(
                        (se_theme::GhostButton {
                            dark,
                            active: false
                        })
                        .style()
                    ),
            ]
            .spacing(8),
        ]
        .spacing(10)
        .width(Length::Fixed(320.0));
        let card = container(form).padding(16).style(se_theme::card(dark));
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
        .on_press(Message::Git(GitMsg::RefDialog(None)))
        .into()
    }
}

/// The id of the first field of the popin (focused when it opens).
pub fn ref_dialog_field() -> iced::widget::Id {
    iced::widget::Id::new("ref-dialog-field")
}
