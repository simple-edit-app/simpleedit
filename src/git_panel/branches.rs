//! The Branches tab: local and remote branches and tags, and what can be
//! done to them: create, switch, merge, rebase, push, delete.

use iced::widget::{button, column, container, row, scrollable, text, text_input, Space};
use iced::{Alignment, Element, Length, Task};

use super::{GitMsg, Panel};
use crate::app::Message;
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
    Push,
    ForcePush,
    Delete,
    Create,
}

impl RefAction {
    /// Those that can't be undone ask for a second click.
    fn asks_twice(self) -> bool {
        matches!(self, RefAction::ForcePush | RefAction::Delete)
    }
}

#[derive(Debug, Default)]
pub struct State {
    pub selected: Option<Ref>,
    /// The name typed for a new branch.
    pub name: String,
    /// The action waiting for its second click.
    pub confirm: Option<RefAction>,
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
            (RefAction::Checkout, Some(Ref::Branch(name)), _) => self.checkout(name),
            (RefAction::Checkout, Some(Ref::Tag(name)), _) => self.act(move |repo| async move {
                history::checkout_ref(&repo, &name).await?;
                Ok(String::new())
            }),
            (RefAction::Merge, Some(Ref::Branch(name)), _) => self.act(move |repo| async move {
                crate::git::merge_branch(&repo, &name).await?;
                Ok(String::new())
            }),
            (RefAction::Rebase, Some(Ref::Branch(name)), _) => self.act(move |repo| async move {
                crate::git::rebase_onto(&repo, &name).await?;
                Ok(String::new())
            }),
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
        let confirming = |action: RefAction| self.branches.confirm == Some(action);
        let picked = match &self.branches.selected {
            Some(Ref::Branch(n) | Ref::Tag(n)) => n.clone(),
            None => String::new(),
        };
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
            act(
                t!("git.branch_merge", current = current.as_str()).to_string(),
                RefAction::Merge,
                false
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
            container(Space::new().height(1))
                .width(Length::Fill)
                .style(se_theme::rule(dark)),
            text(create_label).size(11.5).color(p.muted),
            row![
                text_input(&t!("git.new_branch_name"), &self.branches.name)
                    .on_input(|t| Message::Git(GitMsg::RefName(t)))
                    .on_submit(Message::Git(GitMsg::RefAction(RefAction::Create)))
                    .size(12.5)
                    .padding([4, 8])
                    .style(se_theme::Field::new(dark).style()),
                act(t!("git.create").to_string(), RefAction::Create, true).width(Length::Shrink),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
            notice,
        ]
        .spacing(4)
        .width(Length::Fixed(280.0));

        row![
            scrollable(container(list).padding([2, 6]))
                .style(se_theme::Scrollbar { dark }.style())
                .width(Length::Fill)
                .height(Length::Fill),
            container(Space::new().width(1).height(Length::Fill)).style(se_theme::rule(dark)),
            scrollable(container(actions).padding([10, 12]))
                .style(se_theme::Scrollbar { dark }.style())
                .height(Length::Fill),
        ]
        .into()
    }
}
