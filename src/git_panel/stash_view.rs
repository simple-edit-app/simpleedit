//! The Stash tab on screen.

use iced::widget::{button, column, container, row, scrollable, text, text_input, Space};
use iced::{Alignment, Element, Length};

use super::diff_view::{self, Nav};
use super::stash::format_time;
use super::{GitMsg, Panel};
use crate::app::Message;
use crate::theme as se_theme;

impl Panel {
    pub(super) fn stash_tab(&self, dark: bool) -> Element<'_, Message> {
        let p = se_theme::palette(dark);
        let state = &self.stash;

        let mut list = column![].spacing(1);
        for stash in &state.list {
            let selected = state.selected == Some(stash.index);
            let line = row![
                text(stash.message.as_str()).size(12.5).width(Length::Fill),
                text(stash.branch.as_str()).size(11.5).color(p.accent),
                text(format_time(stash.time)).size(11.5).color(p.muted),
            ]
            .spacing(14)
            .align_y(Alignment::Center);
            let target = button(line)
                .padding([4, 8])
                .width(Length::Fill)
                .on_press(Message::Git(GitMsg::SelectStash(stash.index)))
                .style(
                    (se_theme::GhostButton {
                        dark,
                        active: selected,
                    })
                    .style(),
                );
            list = list.push(self.stash_menu(target.into(), stash.index, dark));
        }
        if state.list.is_empty() {
            list = list.push(
                container(
                    text(t!("git.no_stash").to_string())
                        .size(12.5)
                        .color(p.muted),
                )
                .padding([10, 12]),
            );
        }

        let mut files = column![].spacing(1);
        for (i, change) in state.files.iter().enumerate() {
            let selected = state.viewer.index == Some(i);
            let (dir, name) = match change.path.rsplit_once('/') {
                Some((dir, name)) => (dir, name),
                None => ("", change.path.as_str()),
            };
            files = files.push(
                button(
                    row![
                        text(change.kind.letter())
                            .size(11)
                            .font(se_theme::MONO)
                            .color(super::view::kind_color(change.kind, dark))
                            .width(12),
                        crate::file_icons::file(std::path::Path::new(name), dark),
                        text(name).size(12.5),
                        text(dir)
                            .size(11.5)
                            .color(p.muted)
                            .wrapping(iced::widget::text::Wrapping::None),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                )
                .padding([3, 8])
                .width(Length::Fill)
                .on_press(Message::Git(GitMsg::SelectFile(i)))
                .style(
                    (se_theme::GhostButton {
                        dark,
                        active: selected,
                    })
                    .style(),
                ),
            );
        }

        let chosen = state.selected;
        let act = |label: String, msg: Option<GitMsg>| {
            let mut b = button(text(label).size(12.5)).padding([4, 12]);
            if let Some(m) = msg {
                b = b.on_press(Message::Git(m));
            }
            b.style(
                (se_theme::GhostButton {
                    dark,
                    active: false,
                })
                .style(),
            )
        };
        let controls = row![
            act(
                t!("git.stash_apply").to_string(),
                chosen.map(GitMsg::StashApply),
            ),
            act(
                t!("git.stash_pop").to_string(),
                chosen.map(GitMsg::StashPop)
            ),
            Space::new().width(Length::Fill),
            text_input(&t!("git.stash_message"), &state.message)
                .on_input(|t| Message::Git(GitMsg::StashMessage(t)))
                .on_submit(Message::Git(GitMsg::StashPush))
                .size(12.5)
                .padding([4, 8])
                .width(Length::Fixed(150.0))
                .style(se_theme::Field::new(dark).style()),
            act(t!("git.stash_push").to_string(), Some(GitMsg::StashPush)),
        ]
        .spacing(6)
        .align_y(Alignment::Center);

        let left = column![
            scrollable(container(list).padding([2, 6]))
                .style(se_theme::Scrollbar { dark }.style())
                .height(Length::FillPortion(3)),
            container(Space::new().height(1))
                .width(Length::Fill)
                .style(se_theme::rule(dark)),
            scrollable(container(files).padding([2, 6]))
                .style(se_theme::Scrollbar { dark }.style())
                .height(Length::FillPortion(3)),
            container(controls).padding([6, 10]),
        ]
        .width(Length::Fixed(430.0));

        row![
            left,
            container(Space::new().width(1).height(Length::Fill)).style(se_theme::rule(dark)),
            diff_view::view(
                &self.stash.viewer,
                &Nav::panel(),
                dark,
                t!("git.select_file").to_string(),
            ),
        ]
        .into()
    }

    fn stash_menu<'a>(
        &'a self,
        target: Element<'a, Message>,
        index: usize,
        dark: bool,
    ) -> Element<'a, Message> {
        let apply = t!("git.stash_apply").to_string();
        let pop = t!("git.stash_pop").to_string();
        let drop = t!("git.stash_drop").to_string();
        crate::pointer_area::context_menu(target, move || {
            let item = |label: String, msg: GitMsg| -> Element<'static, Message> {
                button(text(label).size(13))
                    .padding([7, 10])
                    .width(Length::Fixed(190.0))
                    .on_press(Message::Git(msg))
                    .style(
                        (se_theme::MenuItem {
                            dark,
                            radius: se_theme::R_SM,
                            highlighted: false,
                        })
                        .style(),
                    )
                    .into()
            };
            container(
                column![
                    item(apply.clone(), GitMsg::StashApply(index)),
                    item(pop.clone(), GitMsg::StashPop(index)),
                    item(drop.clone(), GitMsg::StashDrop(index)),
                ]
                .spacing(1)
                .padding(6),
            )
            .style(se_theme::card(dark))
            .into()
        })
    }
}
