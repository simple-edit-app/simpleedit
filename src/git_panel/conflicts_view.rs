//! The conflicts of an operation, and the choices for each file; the banner
//! of the Commit tab that leads here.

use iced::widget::{button, column, container, row, scrollable, text, Space};
use iced::{Alignment, Element, Length};

use super::{GitMsg, Panel};
use crate::app::Message;
use crate::git::Operation;
use crate::theme as se_theme;

pub fn operation_label(operation: Operation) -> String {
    match operation {
        Operation::Merge => t!("git.op_merge").to_string(),
        Operation::Rebase => t!("git.op_rebase").to_string(),
        Operation::CherryPick => t!("git.op_cherry_pick").to_string(),
        Operation::Revert => t!("git.op_revert").to_string(),
    }
}

impl Panel {
    /// What the pop-up that announces conflicts says (none: no conflicts).
    pub fn conflict_summary(&self) -> Option<String> {
        let status = self.status.as_ref()?;
        let n = status.conflicts().count();
        if n == 0 {
            return None;
        }
        Some(match status.operation {
            Some(op) => t!("git.op_conflicts", op = operation_label(op), n = n).to_string(),
            None => t!("git.conflicts_n", n = n).to_string(),
        })
    }

    /// The files in conflict, if there are any, or the operation to go on
    /// with: shown above the file list of the Commit tab.
    pub(super) fn conflict_banner(&self, dark: bool) -> Option<Element<'_, Message>> {
        let status = self.status.as_ref()?;
        let conflicts = status.conflicts().count();
        if conflicts == 0 && status.operation.is_none() {
            return None;
        }
        let p = se_theme::palette(dark);
        let label = match (status.operation, conflicts) {
            (Some(op), 0) => t!("git.op_ready", op = operation_label(op)).to_string(),
            (Some(op), n) => t!("git.op_conflicts", op = operation_label(op), n = n).to_string(),
            (None, n) => t!("git.conflicts_n", n = n).to_string(),
        };
        let mut line = row![].spacing(6).align_y(Alignment::Center);
        if conflicts > 0 {
            line = line.push(
                button(text(t!("git.resolve").to_string()).size(12))
                    .padding([3, 10])
                    .on_press(Message::OpenConflicts)
                    .style((se_theme::Primary { dark }).style()),
            );
        }
        if status.operation.is_some() {
            let mut next = button(text(t!("git.continue").to_string()).size(12))
                .padding([3, 10])
                .style(
                    (se_theme::GhostButton {
                        dark,
                        active: false,
                    })
                    .style(),
                );
            if conflicts == 0 && !self.busy {
                next = next.on_press(Message::Git(GitMsg::ContinueOperation));
            }
            line = line.push(next);
            // A commit that became empty once settled can be left out.
            if conflicts == 0 && status.operation != Some(Operation::Merge) && !self.busy {
                line = line.push(
                    button(text(t!("git.skip").to_string()).size(12))
                        .padding([3, 10])
                        .on_press(Message::Git(GitMsg::SkipOperation))
                        .style(
                            (se_theme::GhostButton {
                                dark,
                                active: false,
                            })
                            .style(),
                        ),
                );
            }
            line = line.push(
                button(text(t!("git.abort").to_string()).size(12))
                    .padding([3, 10])
                    .on_press(Message::Git(GitMsg::AbortOperation))
                    .style(
                        (se_theme::GhostButton {
                            dark,
                            active: false,
                        })
                        .style(),
                    ),
            );
        }
        let line = column![
            text(label).size(12).color(if conflicts > 0 {
                se_theme::error_color(dark)
            } else {
                p.muted
            }),
            line,
        ]
        .spacing(4);
        Some(container(line).padding([4, 10]).into())
    }

    /// The card that lists the conflicted files.
    pub fn conflicts_card(&self, dark: bool) -> Element<'_, Message> {
        let p = se_theme::palette(dark);
        let status = self.status.as_ref();
        let files: Vec<&crate::git::Change> =
            status.map(|s| s.conflicts().collect()).unwrap_or_default();
        let mut list = column![].spacing(2);
        for change in &files {
            let (dir, name) = match change.path.rsplit_once('/') {
                Some((dir, name)) => (dir, name),
                None => ("", change.path.as_str()),
            };
            let act = |label: String, msg: GitMsg| {
                button(text(label).size(12.5))
                    .padding([4, 10])
                    .on_press(Message::Git(msg))
                    .style((se_theme::Outline { dark }).style())
            };
            list = list.push(
                row![
                    row![
                        crate::file_icons::file(std::path::Path::new(name), dark),
                        text(name).size(13),
                        text(dir)
                            .size(12)
                            .color(p.muted)
                            .wrapping(iced::widget::text::Wrapping::None),
                    ]
                    .spacing(8)
                    .width(Length::Fill),
                    act(
                        t!("git.accept_yours").to_string(),
                        GitMsg::AcceptSide(change.path.clone(), true)
                    ),
                    act(
                        t!("git.accept_theirs").to_string(),
                        GitMsg::AcceptSide(change.path.clone(), false)
                    ),
                    act(
                        t!("git.merge_manually").to_string(),
                        GitMsg::OpenMerge(change.path.clone())
                    ),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            );
        }
        let body: Element<Message> = if files.is_empty() {
            text(t!("git.all_resolved").to_string())
                .size(13)
                .color(p.muted)
                .into()
        } else {
            scrollable(list)
                .style(se_theme::Scrollbar { dark }.style())
                .height(Length::Fixed(220.0))
                .into()
        };
        let mut footer = row![Space::new().width(Length::Fill)].spacing(8);
        if self.status.as_ref().is_some_and(|s| s.operation.is_some()) && files.is_empty() {
            footer = footer.push(
                button(text(t!("git.continue").to_string()).size(12.5))
                    .padding([5, 14])
                    .on_press(Message::Git(GitMsg::ContinueOperation))
                    .style((se_theme::Primary { dark }).style()),
            );
        }
        footer = footer.push(
            button(text(t!("git.close").to_string()).size(12.5))
                .padding([5, 14])
                .on_press(Message::CloseOverlay)
                .style(
                    (se_theme::GhostButton {
                        dark,
                        active: false,
                    })
                    .style(),
                ),
        );
        container(
            column![
                text(t!("git.conflicts_title", n = files.len()).to_string())
                    .size(15)
                    .font(se_theme::UI_SEMIBOLD),
                body,
                footer,
            ]
            .spacing(14),
        )
        .padding(20)
        .width(Length::Fixed(760.0))
        .style(se_theme::card(dark))
        .into()
    }
}
