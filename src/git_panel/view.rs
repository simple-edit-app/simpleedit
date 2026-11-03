//! The Git panel on screen.

use iced::widget::{
    button, checkbox, column, container, row, scrollable, text, text_editor, Space,
};
use iced::{Alignment, Color, Element, Length};

use super::{diff_view, GitMsg, Panel, Tab};
use crate::app::Message;
use crate::git::{Change, Kind};
use crate::theme as se_theme;

/// The height of the panel.
pub const HEIGHT: f32 = 380.0;

/// The color of a kind of change.
pub(super) fn kind_color(kind: Kind, dark: bool) -> Color {
    let p = se_theme::palette(dark);
    match kind {
        Kind::Added | Kind::Untracked => {
            if dark {
                Color::from_rgb8(0x8f, 0xd1, 0x8f)
            } else {
                Color::from_rgb8(0x2e, 0x7d, 0x32)
            }
        }
        Kind::Deleted | Kind::Conflict => se_theme::error_color(dark),
        Kind::Modified | Kind::Renamed => p.accent,
    }
}

fn plain_button<'a>(
    label: String,
    msg: Option<Message>,
    active: bool,
    dark: bool,
) -> Element<'a, Message> {
    let mut b = button(text(label).size(12.5)).padding([4, 10]);
    if let Some(msg) = msg {
        b = b.on_press(msg);
    }
    b.style((se_theme::GhostButton { dark, active }).style())
        .into()
}

fn filled_button<'a>(label: String, msg: Option<Message>, dark: bool) -> Element<'a, Message> {
    let mut b = button(text(label).size(12.5)).padding([5, 14]);
    if let Some(msg) = msg {
        b = b.on_press(msg);
    }
    b.style((se_theme::Primary { dark }).style()).into()
}

impl Panel {
    /// The whole panel: tabs on top, then the tab.
    pub fn view(&self, dark: bool) -> Element<'_, Message> {
        let p = se_theme::palette(dark);
        let tab = |label: String, tab: Tab| {
            plain_button(
                label,
                Some(Message::Git(GitMsg::SelectTab(tab))),
                self.tab == tab,
                dark,
            )
        };
        let mut header = row![
            tab(t!("git.tab_commit").to_string(), Tab::Commit),
            tab(t!("git.tab_stash").to_string(), Tab::Stash),
            tab(t!("git.tab_log").to_string(), Tab::Log),
            Space::new().width(Length::Fill),
        ]
        .spacing(4)
        .align_y(Alignment::Center);
        if let Some(status) = &self.status {
            header = header.push(
                text(branch_label(&status.branch, status.ahead, status.behind))
                    .size(12)
                    .color(p.muted),
            );
        }
        header = header.push(
            button(text("—").size(12))
                .padding([1, 8])
                .on_press(Message::ToggleGit)
                .style(
                    (se_theme::GhostButton {
                        dark,
                        active: false,
                    })
                    .style(),
                ),
        );

        let content: Element<Message> = match (&self.repo, self.searched) {
            (None, true) => self.no_repository(dark),
            (None, false) => Space::new().into(),
            (Some(_), _) => match self.tab {
                Tab::Commit => self.commit_tab(dark),
                Tab::Stash => self.stash_tab(dark),
                Tab::Log => self.log_tab(dark),
            },
        };

        column![
            container(Space::new().height(1))
                .width(Length::Fill)
                .style(se_theme::rule(dark)),
            container(header).padding([4, 10]),
            container(Space::new().height(1))
                .width(Length::Fill)
                .style(se_theme::rule(dark)),
            content,
        ]
        .height(Length::Fixed(HEIGHT))
        .into()
    }

    fn no_repository(&self, dark: bool) -> Element<'_, Message> {
        container(
            column![
                text(t!("git.no_repo").to_string())
                    .size(13)
                    .color(se_theme::muted_text(dark)),
                plain_button(
                    t!("git.init").to_string(),
                    (!self.busy).then_some(Message::Git(GitMsg::Init)),
                    true,
                    dark,
                ),
            ]
            .spacing(10)
            .align_x(Alignment::Center),
        )
        .center(Length::Fill)
        .into()
    }

    /// Changed files on the left, the diff on the right, the message below.
    fn commit_tab(&self, dark: bool) -> Element<'_, Message> {
        let p = se_theme::palette(dark);
        let Some(status) = &self.status else {
            return Space::new().into();
        };
        let staged = status.staged.len();
        let unstaged = status.unstaged.len();

        let mut list = column![].spacing(1);
        list = list.push(self.section(
            t!("git.staged", n = staged).to_string(),
            staged > 0,
            !status.staged.is_empty(),
            false,
            dark,
        ));
        for (i, change) in status.staged.iter().enumerate() {
            list = list.push(self.file_row(i, change, true, dark));
        }
        list = list.push(self.section(
            t!("git.changes", n = unstaged).to_string(),
            unstaged > 0,
            false,
            true,
            dark,
        ));
        for (i, change) in status.unstaged.iter().enumerate() {
            list = list.push(self.file_row(staged + i, change, false, dark));
        }
        if staged + unstaged == 0 {
            list = list.push(
                container(
                    text(t!("git.nothing_to_commit").to_string())
                        .size(12.5)
                        .color(p.muted),
                )
                .padding([10, 12]),
            );
        }

        let can_commit =
            !self.busy && (staged > 0 || self.amend) && !self.message.text().trim().is_empty();
        let notice: Element<Message> = match &self.notice {
            Some((text_value, is_error)) => button(
                text(text_value.as_str())
                    .size(12)
                    .color(if *is_error {
                        se_theme::error_color(dark)
                    } else {
                        p.muted
                    })
                    .wrapping(iced::widget::text::Wrapping::None),
            )
            .padding(0)
            .on_press(Message::Git(GitMsg::DismissNotice))
            .style(|_: &iced::Theme, _| iced::widget::button::Style::default())
            .into(),
            None => Space::new().into(),
        };
        let editor = iced::widget::mouse_area(
            text_editor(&self.message)
                .placeholder(t!("git.message_placeholder").to_string())
                .on_action(|a| Message::Git(GitMsg::MessageAction(a)))
                .size(13)
                .height(Length::Fixed(78.0)),
        )
        .on_press(Message::Git(GitMsg::MessageFocused));

        let banner = self.conflict_banner(dark);
        let left = column![
            banner.unwrap_or_else(|| Space::new().into()),
            scrollable(container(list).padding([2, 6]))
                .style(se_theme::Scrollbar { dark }.style())
                .height(Length::Fill),
            container(
                column![
                    checkbox(self.amend)
                        .label(t!("git.amend").to_string())
                        .size(14)
                        .text_size(12.5)
                        .on_toggle(|on| Message::Git(GitMsg::ToggleAmend(on))),
                    editor,
                    row![
                        filled_button(
                            t!("git.commit").to_string(),
                            can_commit.then_some(Message::Git(GitMsg::Commit(false))),
                            dark,
                        ),
                        plain_button(
                            t!("git.commit_push").to_string(),
                            can_commit.then_some(Message::Git(GitMsg::Commit(true))),
                            false,
                            dark,
                        ),
                        notice,
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                ]
                .spacing(6),
            )
            .padding([6, 10]),
        ]
        .width(Length::Fixed(380.0));

        row![
            left,
            container(Space::new().width(1).height(Length::Fill)).style(se_theme::rule(dark)),
            diff_view::view(
                &self.viewer,
                &diff_view::Nav::panel(),
                dark,
                t!("git.select_file").to_string(),
            ),
        ]
        .into()
    }

    /// A heading of the list, with the checkbox that stages or unstages all.
    fn section(
        &self,
        label: String,
        enabled: bool,
        checked: bool,
        stage_all: bool,
        dark: bool,
    ) -> Element<'_, Message> {
        let p = se_theme::palette(dark);
        let mut check = checkbox(checked).size(14);
        if enabled {
            check = check.on_toggle(move |_| Message::Git(GitMsg::StageAll(stage_all)));
        }
        container(
            row![
                check,
                text(label)
                    .size(12)
                    .font(se_theme::UI_SEMIBOLD)
                    .color(p.muted)
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .padding([6, 6])
        .into()
    }

    /// One changed file: its checkbox (staged or not), a letter, name, folder.
    fn file_row<'a>(
        &'a self,
        index: usize,
        change: &'a Change,
        staged: bool,
        dark: bool,
    ) -> Element<'a, Message> {
        let p = se_theme::palette(dark);
        let selected = self.viewer.index == Some(index);
        let (dir, name) = match change.path.rsplit_once('/') {
            Some((dir, name)) => (dir, name),
            None => ("", change.path.as_str()),
        };
        let line = row![
            checkbox(staged)
                .size(14)
                .on_toggle(move |_| Message::Git(GitMsg::ToggleStaged(index))),
            text(change.kind.letter())
                .size(11)
                .font(se_theme::MONO)
                .color(kind_color(change.kind, dark))
                .width(12),
            text(name).size(12.5).color(p.text),
            text(dir)
                .size(11.5)
                .color(p.muted)
                .wrapping(iced::widget::text::Wrapping::None),
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        let row_button = button(line)
            .padding([3, 6])
            .width(Length::Fill)
            .on_press(Message::Git(GitMsg::SelectFile(index)))
            .style(
                (se_theme::GhostButton {
                    dark,
                    active: selected,
                })
                .style(),
            );
        self.file_menu(row_button.into(), index, change, staged, dark)
    }

    /// The right-click menu of a file.
    fn file_menu<'a>(
        &'a self,
        target: Element<'a, Message>,
        index: usize,
        change: &Change,
        staged: bool,
        dark: bool,
    ) -> Element<'a, Message> {
        let kind = change.kind;
        let toggle_label = if staged {
            t!("git.unstage").to_string()
        } else {
            t!("git.stage").to_string()
        };
        let rollback_label = t!("git.rollback").to_string();
        let jump_label = t!("git.jump_source").to_string();
        let diff_label = t!("git.diff_editor").to_string();
        let delete_label = t!("git.delete").to_string();
        iced_aw::ContextMenu::new(target, move || {
            let item = |label: String, msg: Option<Message>| -> Element<'static, Message> {
                let mut b = button(text(label).size(13))
                    .padding([7, 10])
                    .width(Length::Fixed(190.0))
                    .style(
                        (se_theme::MenuItem {
                            dark,
                            radius: se_theme::R_SM,
                            highlighted: false,
                        })
                        .style(),
                    );
                if let Some(m) = msg {
                    b = b.on_press(m);
                }
                b.into()
            };
            let exists = kind != Kind::Deleted;
            container(
                column![
                    item(
                        rollback_label.clone(),
                        Some(Message::Git(GitMsg::Rollback(index))),
                    ),
                    item(
                        toggle_label.clone(),
                        Some(Message::Git(GitMsg::ToggleStaged(index))),
                    ),
                    item(
                        diff_label.clone(),
                        Some(Message::Git(GitMsg::DiffInEditor(index))),
                    ),
                    item(
                        jump_label.clone(),
                        exists.then_some(Message::Git(GitMsg::JumpToSource(index))),
                    ),
                    item(
                        delete_label.clone(),
                        exists.then_some(Message::Git(GitMsg::Delete(index))),
                    ),
                ]
                .spacing(1)
                .padding(6),
            )
            .style(se_theme::card(dark))
            .into()
        })
        .into()
    }
}

/// `main ↑2 ↓1`
fn branch_label(branch: &str, ahead: u32, behind: u32) -> String {
    let mut label = branch.to_string();
    if ahead > 0 {
        label.push_str(&format!(" ↑{ahead}"));
    }
    if behind > 0 {
        label.push_str(&format!(" ↓{behind}"));
    }
    label
}
