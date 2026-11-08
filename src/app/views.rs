//! The overlay cards and the other views of the app.

use super::*;
use iced::widget::{column, row};

// ─── Overlay views ──────────────────────────────────────────────────────────

const REFERENCE_ROW: f32 = 28.0;

fn references_list_id() -> iced::widget::Id {
    iced::widget::Id::new("references-list")
}

/// Keeps the highlighted usage in view while the arrows move it.
pub(super) fn scroll_references_to(selected: usize) -> Task<Message> {
    iced::widget::operation::scroll_to(
        references_list_id(),
        iced::widget::scrollable::AbsoluteOffset {
            x: 0.0,
            y: ((selected as f32 - 4.0) * (REFERENCE_ROW + 1.0)).max(0.0),
        },
    )
}

impl SimpleEditApp {
    /// Whether the language can be set by hand. Always: a recognized file
    /// may still deserve another language (a `.h` header as C++).
    pub(super) fn language_picker_enabled(&self) -> bool {
        true
    }

    pub(super) fn view_about_overlay(&self, dark: bool) -> Element<'_, Message> {
        let p = se_theme::palette(dark);
        let version = env!("CARGO_PKG_VERSION");
        container(
            column![
                text("SimpleEdit").size(24).color(p.accent),
                text(format!("v{}", version)).size(15).color(p.text),
                text(t!("about.tagline").to_string())
                    .size(12)
                    .color(p.muted),
                button(text(t!("about.close").to_string()).size(13))
                    .padding([8, 24])
                    .on_press(Message::CloseOverlay)
                    .style((se_theme::GhostButton { dark, active: true }).style()),
            ]
            .spacing(12)
            .align_x(Alignment::Center)
            .padding(36)
            .width(Length::Fill),
        )
        .width(320)
        .style(se_theme::card(dark))
        .into()
    }

    pub(super) fn view_update_overlay(&self, dark: bool) -> Element<'_, Message> {
        let p = se_theme::palette(dark);

        let content: Element<'_, Message> = match &self.overlay {
            ActiveOverlay::UpdateChecking => column![text(t!("update.checking").to_string())
                .size(14)
                .color(p.text),]
            .spacing(12)
            .align_x(Alignment::Center)
            .padding(36)
            .width(Length::Fill)
            .into(),

            ActiveOverlay::UpdateUpToDate => column![
                text(t!("update.up_to_date").to_string())
                    .size(16)
                    .color(p.accent),
                button(text(t!("about.close").to_string()).size(13))
                    .padding([8, 24])
                    .on_press(Message::CloseOverlay)
                    .style((se_theme::GhostButton { dark, active: true }).style()),
            ]
            .spacing(12)
            .align_x(Alignment::Center)
            .padding(36)
            .width(Length::Fill)
            .into(),

            ActiveOverlay::UpdateAvailable(version, notes) => {
                let version = version.clone();
                let mut body =
                    column![
                        text(t!("update.available", version = version.as_str()).to_string())
                            .size(16)
                            .color(p.accent)
                    ]
                    .spacing(16)
                    .align_x(Alignment::Center);
                if !notes.is_empty() {
                    body = body.push(
                        scrollable(changelog_list(dark, notes))
                            .height(Length::Fixed(260.0))
                            .width(Length::Fill),
                    );
                }
                body.push(
                    row![
                        button(text(t!("about.close").to_string()).size(13))
                            .padding([8, 20])
                            .on_press(Message::CloseOverlay)
                            .style(
                                (se_theme::GhostButton {
                                    dark,
                                    active: false,
                                })
                                .style()
                            ),
                        button(text(t!("update.download_install").to_string()).size(13))
                            .padding([8, 20])
                            .on_press(Message::ConfirmInstallUpdate(version))
                            .style((se_theme::GhostButton { dark, active: true }).style()),
                    ]
                    .spacing(12),
                )
                .padding(28)
                .width(Length::Fill)
                .into()
            }

            ActiveOverlay::UpdateDownloading => column![text(t!("update.downloading").to_string())
                .size(14)
                .color(p.text),]
            .spacing(12)
            .align_x(Alignment::Center)
            .padding(36)
            .width(Length::Fill)
            .into(),

            ActiveOverlay::UpdateInstalling => column![text(t!("update.installing").to_string())
                .size(14)
                .color(p.text),]
            .spacing(12)
            .align_x(Alignment::Center)
            .padding(36)
            .width(Length::Fill)
            .into(),

            ActiveOverlay::UpdateError(e) => column![
                text(t!("update.error").to_string())
                    .size(14)
                    .color(p.accent),
                text(e.clone())
                    .size(12)
                    .color(p.muted)
                    .shaping(iced::widget::text::Shaping::Advanced),
                button(text(t!("about.close").to_string()).size(13))
                    .padding([8, 24])
                    .on_press(Message::CloseOverlay)
                    .style(
                        (se_theme::GhostButton {
                            dark,
                            active: false,
                        })
                        .style()
                    ),
            ]
            .spacing(12)
            .align_x(Alignment::Center)
            .padding(36)
            .width(Length::Fill)
            .into(),

            // Shown by their own views (`view`).
            ActiveOverlay::None
            | ActiveOverlay::About
            | ActiveOverlay::Changelog
            | ActiveOverlay::WhatsNew(_)
            | ActiveOverlay::GotoLine
            | ActiveOverlay::LanguagePicker
            | ActiveOverlay::References(_)
            | ActiveOverlay::Shortcuts
            | ActiveOverlay::Conflicts
            | ActiveOverlay::Everywhere
            | ActiveOverlay::LocalHistory(_)
            | ActiveOverlay::HistoryLabel(_)
            | ActiveOverlay::ConfirmClose(_)
            | ActiveOverlay::ConfirmCloseMany(..)
            | ActiveOverlay::ConfirmQuit(_)
            | ActiveOverlay::ConfirmScratchDelete(_)
            | ActiveOverlay::ConfirmFileDelete(..)
            | ActiveOverlay::ProjectName(_)
            | ActiveOverlay::FindInFiles
            | ActiveOverlay::ConfirmFolderDelete(_)
            | ActiveOverlay::ConfirmMove(..)
            | ActiveOverlay::BranchSwitch(_)
            | ActiveOverlay::GitFile(_)
            | ActiveOverlay::GitPick(_)
            | ActiveOverlay::ConfirmOverwrite(_) => Space::new().height(0).into(),
        };

        let width = if matches!(&self.overlay, ActiveOverlay::UpdateAvailable(_, n) if !n.is_empty())
        {
            480
        } else {
            360
        };
        container(content)
            .width(width)
            .style(se_theme::card(dark))
            .into()
    }

    /// Records the running version; right after an update, opens the notes
    /// of what it brought.
    pub(super) fn announce_new_version(&mut self) {
        let current = env!("CARGO_PKG_VERSION");
        let previous = std::mem::replace(&mut self.config.last_version, current.to_string());
        if previous == current {
            return;
        }
        self.config_dirty = true;
        let upgraded = !previous.is_empty()
            && !crate::changelog::between(crate::changelog::embedded(), &previous, current)
                .is_empty();
        if upgraded && self.overlay == ActiveOverlay::None {
            self.overlay = ActiveOverlay::WhatsNew(previous);
        }
    }

    /// The changelog in a card; `since` limits it to the releases after that
    /// version.
    pub(super) fn changelog_card(
        &self,
        dark: bool,
        title: String,
        since: Option<&str>,
    ) -> Element<'_, Message> {
        let p = se_theme::palette(dark);
        let mut entries = crate::changelog::embedded();
        if let Some(since) = since {
            entries = crate::changelog::between(entries, since, env!("CARGO_PKG_VERSION"));
        }
        container(
            column![
                text(title).size(18).color(p.accent),
                scrollable(changelog_list(dark, &entries))
                    .height(Length::Fixed(380.0))
                    .width(Length::Fill),
                button(text(t!("about.close").to_string()).size(13))
                    .padding([8, 24])
                    .on_press(Message::CloseOverlay)
                    .style((se_theme::GhostButton { dark, active: true }).style()),
            ]
            .spacing(16)
            .align_x(Alignment::Center)
            .padding(28)
            .width(Length::Fill),
        )
        .width(520)
        .style(se_theme::card(dark))
        .into()
    }

    pub(super) fn view_confirm_overlay(&self, dark: bool, path: &Path) -> Element<'_, Message> {
        let name = file_label(path);
        confirm_card(
            dark,
            t!("confirm.title").to_string(),
            t!("confirm.message", name = name.as_str()).to_string(),
            t!("confirm.discard").to_string(),
            Message::ConfirmDiscard(path.to_path_buf()),
        )
    }

    pub(super) fn view_confirm_many_overlay(
        &self,
        dark: bool,
        paths: &[PathBuf],
        modified: usize,
    ) -> Element<'_, Message> {
        confirm_card(
            dark,
            t!("confirm.title").to_string(),
            t!("confirm.many", n = modified).to_string(),
            t!("confirm.discard").to_string(),
            Message::ConfirmDiscardMany(paths.to_vec()),
        )
    }

    pub(super) fn view_overwrite_overlay(&self, dark: bool, path: &Path) -> Element<'_, Message> {
        let name = file_label(path);
        confirm_card(
            dark,
            t!("overwrite.title").to_string(),
            t!("overwrite.message", name = name.as_str()).to_string(),
            t!("overwrite.confirm").to_string(),
            Message::ConfirmOverwrite,
        )
    }

    pub(super) fn view_goto_line_overlay(&self, dark: bool) -> Element<'_, Message> {
        let p = se_theme::palette(dark);
        container(
            column![
                text(t!("goto_line.title").to_string())
                    .size(14)
                    .color(p.text),
                text_input(t!("goto_line.placeholder").as_ref(), &self.goto_line_input)
                    .id(goto_line_id())
                    .on_input(Message::GotoLineInputChanged)
                    .on_submit(Message::GotoLineSubmit)
                    .padding([8, 10])
                    .size(14)
                    .style(se_theme::Field::new(dark).style()),
                row![
                    button(text(t!("goto_line.cancel").to_string()).size(13))
                        .padding([7, 16])
                        .on_press(Message::CloseOverlay)
                        .style(
                            (se_theme::GhostButton {
                                dark,
                                active: false,
                            })
                            .style()
                        ),
                    Space::new().width(Length::Fill),
                    button(text(t!("goto_line.go").to_string()).size(13))
                        .padding([7, 16])
                        .on_press(Message::GotoLineSubmit)
                        .style((se_theme::GhostButton { dark, active: true }).style()),
                ]
                .align_y(Alignment::Center),
            ]
            .spacing(14)
            .padding(24),
        )
        .width(320)
        .style(se_theme::card(dark))
        .into()
    }

    /// The scratch files: a form to make one, then the list.
    pub(super) fn scratch_panel(&self, dark: bool) -> Element<'_, Message> {
        let p = se_theme::palette(dark);
        let scratch = &self.scratch;
        let ghost = move |active: bool| (se_theme::GhostButton { dark, active }).style();
        // Left empty, the name is the next free `scratchN`, shown as a hint.
        let suggestion = crate::scratch::next_name(&scratch.dir, scratch.ext.as_str());
        let create = button(text(t!("scratch.create").to_string()).size(12.5))
            .padding([5, 12])
            .on_press(Message::ScratchCreate)
            .style((se_theme::Primary { dark }).style());
        let header = row![
            text(t!("scratch.title").to_string())
                .size(12)
                .font(se_theme::UI_SEMIBOLD)
                .color(p.text),
            Space::new().width(8),
            text_input(&suggestion, &scratch.name)
                .on_input(Message::ScratchName)
                .on_submit(Message::ScratchCreate)
                .padding([5, 8])
                .size(13)
                .width(200)
                .style(se_theme::Field::new(dark).style()),
            iced::widget::pick_list(
                crate::scratch::Ext::all(),
                Some(scratch.ext),
                Message::ScratchExt
            )
            .padding([5, 10])
            .width(150)
            .text_size(13)
            .style(se_theme::Select { dark }.style())
            .menu_style(se_theme::Select { dark }.menu_style()),
            create,
            Space::new().width(Length::Fill),
            button(text("—").size(12))
                .padding([1, 8])
                .on_press(Message::ToggleScratch)
                .style(ghost(false)),
        ]
        .spacing(6)
        .align_y(Alignment::Center);

        let current = self.current_file.as_ref();
        let mut list = column![].spacing(1);
        if scratch.files.is_empty() {
            list = list.push(
                text(t!("scratch.empty").to_string())
                    .size(12.5)
                    .color(p.muted),
            );
        }
        for path in &scratch.files {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let open = button(
                row![
                    crate::file_icons::file(path, dark),
                    text(name)
                        .size(13)
                        .shaping(iced::widget::text::Shaping::Advanced),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            )
            .padding([4, 8])
            .width(Length::Fill)
            .on_press(Message::ScratchOpen(path.clone()))
            .style(
                (se_theme::SidebarRow {
                    dark,
                    selected: current == Some(path),
                })
                .style(),
            );
            let delete_label = t!("scratch.delete").to_string();
            let target = path.clone();
            let menu = crate::pointer_area::context_menu(open, move || {
                container(
                    button(text(delete_label.clone()).size(13))
                        .padding([7, 10])
                        .width(Length::Fixed(160.0))
                        .on_press(Message::ScratchAskDelete(target.clone()))
                        .style(
                            (se_theme::MenuItem {
                                dark,
                                radius: se_theme::R_SM,
                                highlighted: false,
                            })
                            .style(),
                        ),
                )
                .padding(6)
                .style(se_theme::card(dark))
                .into()
            });
            list = list.push(menu);
        }
        let mut body = column![header].spacing(8);
        if let Some(error) = &scratch.error {
            body = body.push(
                text(error.as_str())
                    .size(12)
                    .color(se_theme::error_color(dark)),
            );
        }
        body = body.push(
            scrollable(container(list).padding([0, 4]).width(Length::Fill))
                .style(se_theme::Scrollbar { dark }.style())
                .height(Length::Fill),
        );
        column![
            resize_edge(dark),
            container(body).padding([8, 12]).height(Length::Fill),
        ]
        .height(Length::Fixed(self.panel_height))
        .into()
    }

    /// The terminals: a row of tabs, then the active one.
    pub(super) fn terminal_panel(&self, dark: bool) -> Element<'_, Message> {
        let p = se_theme::palette(dark);
        let mut header = row![text(t!("terminal.title").to_string())
            .size(12)
            .font(se_theme::UI_SEMIBOLD)
            .color(p.text)]
        .spacing(8)
        .align_y(Alignment::Center);
        for (index, tab) in self.terminal.tabs.iter().enumerate() {
            let active = index == self.terminal.active;
            header = header.push(
                button(
                    row![
                        text(tab.title.as_str()).size(12),
                        button(text("×").size(13))
                            .padding([0, 4])
                            .on_press(Message::CloseTerminal(index))
                            .style(
                                (se_theme::GhostButton {
                                    dark,
                                    active: false,
                                })
                                .style(),
                            ),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center),
                )
                .padding([3, 8])
                .on_press(Message::SelectTerminal(index))
                .style((se_theme::GhostButton { dark, active }).style()),
            );
        }
        header = header
            .push(
                button(text("+").size(14))
                    .padding([1, 8])
                    .on_press(Message::NewTerminal)
                    .style(
                        (se_theme::GhostButton {
                            dark,
                            active: false,
                        })
                        .style(),
                    ),
            )
            .push(Space::new().width(Length::Fill))
            .push(
                button(text("—").size(12))
                    .padding([1, 8])
                    .on_press(Message::ToggleTerminal)
                    .style(
                        (se_theme::GhostButton {
                            dark,
                            active: false,
                        })
                        .style(),
                    ),
            );
        let body: Element<Message> = match self.terminal.active_session() {
            Some(session) => crate::terminal::view(session, dark, self.terminal.focused),
            None => Space::new().into(),
        };
        column![
            resize_edge(dark),
            container(header).padding([4, 12]),
            container(body).width(Length::Fill).height(Length::Fill),
        ]
        .height(Length::Fixed(self.panel_height))
        .into()
    }

    /// Where a name is used, as a compact list under the top edge: file and
    /// line, the line itself, and the full path of the highlighted one.
    pub(super) fn view_references_overlay<'a>(
        &'a self,
        dark: bool,
        list: &'a super::RefList,
    ) -> Element<'a, Message> {
        let p = se_theme::palette(dark);
        let base = self
            .current_file
            .as_deref()
            .and_then(crate::definition::project_root);
        let relative = |path: &std::path::Path| {
            base.as_deref()
                .and_then(|b| path.strip_prefix(b).ok())
                .unwrap_or(path)
                .display()
                .to_string()
        };
        let items: Vec<Element<Message>> = list
            .items
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let name = r
                    .hit
                    .path
                    .file_name()
                    .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                button(
                    row![
                        text(format!("{name}:{}", r.hit.line + 1))
                            .size(13)
                            .color(p.accent),
                        text(r.preview.trim())
                            .size(13)
                            .font(se_theme::MONO)
                            .wrapping(iced::widget::text::Wrapping::None),
                    ]
                    .spacing(12)
                    .align_y(Alignment::Center),
                )
                .padding([0, 10])
                .height(REFERENCE_ROW)
                .width(Length::Fill)
                .on_press(Message::OpenReference(i))
                .style(
                    (se_theme::GhostButton {
                        dark,
                        active: i == list.selected,
                    })
                    .style(),
                )
                .into()
            })
            .collect();
        let footer = list
            .items
            .get(list.selected)
            .map(|r| relative(&r.hit.path))
            .unwrap_or_default();

        let body: Element<Message> = match &list.state {
            super::RefState::Ready => column![
                container(
                    scrollable(column(items).spacing(1))
                        .id(references_list_id())
                        .style(se_theme::Scrollbar { dark }.style())
                )
                .max_height(REFERENCE_ROW * 10.0 + 9.0),
                container(Space::new().height(1))
                    .width(Length::Fill)
                    .style(se_theme::rule(dark)),
                text(footer)
                    .size(11)
                    .color(p.muted)
                    .wrapping(iced::widget::text::Wrapping::None),
            ]
            .spacing(8)
            .into(),
            super::RefState::Searching => container(
                text(t!("references.searching").to_string())
                    .size(13)
                    .color(p.muted),
            )
            .padding([8, 6])
            .into(),
            super::RefState::Empty(message) => {
                container(text(message.as_str()).size(13).color(p.muted))
                    .padding([8, 6])
                    .into()
            }
        };

        container(
            column![
                row![
                    text(list.word.as_str())
                        .size(14)
                        .font(se_theme::MONO)
                        .color(p.text),
                    Space::new().width(Length::Fill),
                    text(match list.state {
                        super::RefState::Ready =>
                            t!("references.count", count = list.items.len()).to_string(),
                        _ => String::new(),
                    })
                    .size(12)
                    .color(p.muted),
                    button(text("×").size(16))
                        .padding([0, 7])
                        .on_press(Message::CloseOverlay)
                        .style(
                            (se_theme::GhostButton {
                                dark,
                                active: false
                            })
                            .style()
                        ),
                ]
                .spacing(10)
                .padding([2, 6])
                .align_y(Alignment::Center),
                body,
            ]
            .spacing(8),
        )
        .padding(10)
        .width(640)
        .style(se_theme::card(dark))
        .into()
    }

    pub(super) fn view_language_picker_overlay(&self, dark: bool) -> Element<'_, Message> {
        let p = se_theme::palette(dark);
        let current_lang = self.editor.language.as_deref().unwrap_or("");

        let mut items: Vec<Element<Message>> = vec![
            // "None / auto" row
            button(
                row![
                    text(t!("language.auto").to_string()).size(13),
                    Space::new().width(Length::Fill),
                    if current_lang.is_empty() {
                        text("✓")
                            .size(13)
                            .color(p.accent)
                            .shaping(iced::widget::text::Shaping::Advanced)
                    } else {
                        text("").size(13)
                    },
                ]
                .align_y(Alignment::Center),
            )
            .padding([6, 12])
            .width(Length::Fill)
            .on_press(Message::SetLanguage(String::new()))
            .style(
                (se_theme::GhostButton {
                    dark,
                    active: current_lang.is_empty(),
                })
                .style(),
            )
            .into(),
        ];

        for (token, name) in crate::editor::SUPPORTED_LANGUAGES {
            let is_current = current_lang == *token;
            items.push(
                button(
                    row![
                        text(*name).size(13),
                        Space::new().width(Length::Fill),
                        if is_current {
                            text("✓")
                                .size(13)
                                .color(p.accent)
                                .shaping(iced::widget::text::Shaping::Advanced)
                        } else {
                            text("").size(13)
                        },
                    ]
                    .align_y(Alignment::Center),
                )
                .padding([6, 12])
                .width(Length::Fill)
                .on_press(Message::SetLanguage(token.to_string()))
                .style(
                    (se_theme::GhostButton {
                        dark,
                        active: is_current,
                    })
                    .style(),
                )
                .into(),
            );
        }

        container(
            column![
                text(t!("language.title").to_string())
                    .size(14)
                    .color(p.text),
                scrollable(column(items).spacing(2).padding(4)).height(Length::Fixed(320.0)),
            ]
            .spacing(10)
            .padding(16),
        )
        .width(260)
        .style(se_theme::card(dark))
        .into()
    }

    /// Help > Configure shortcuts: the same list as the Preferences page.
    pub(super) fn view_shortcuts_overlay(&self, dark: bool) -> Element<'_, Message> {
        let p = se_theme::palette(dark);

        container(
            column![
                row![
                    text(t!("shortcuts.title").to_string())
                        .size(14)
                        .font(se_theme::UI_SEMIBOLD)
                        .color(p.text),
                    Space::new().width(Length::Fill),
                    button(text("×").size(17))
                        .padding([4, 8])
                        .on_press(Message::CloseOverlay)
                        .style(
                            (se_theme::GhostButton {
                                dark,
                                active: false,
                            })
                            .style()
                        ),
                ]
                .align_y(Alignment::Center),
                scrollable(crate::preferences::shortcuts_list(
                    dark,
                    &self.config,
                    self.capturing,
                ))
                .style(se_theme::Scrollbar { dark }.style())
                .height(Length::Fixed(420.0)),
            ]
            .spacing(12)
            .padding(20),
        )
        .width(460)
        .style(se_theme::card(dark))
        .into()
    }

    // ─── Session persistence ─────────────────────────────────────────────────

    /// The session as it stands: every tab, with the text of the ones that
    /// can't be read back from disk (untitled or modified).
    pub(super) fn build_session(&self) -> crate::session::Session {
        use crate::session::{FileSession, Session};
        let files: Vec<FileSession> = self
            .sidebar
            .files()
            .iter()
            .filter(|path| !is_virtual(path))
            .map(|path| {
                let current = self.current_file.as_ref() == Some(path);
                let cached = self.file_cache.get(path);
                let (language, dirty, format, stamp) = if current {
                    (
                        self.editor.language.clone(),
                        self.is_dirty,
                        self.format,
                        self.disk_stamp,
                    )
                } else if let Some(c) = cached {
                    (c.language.clone(), c.dirty, c.format, c.stamp)
                } else {
                    (None, false, TextFormat::default(), None)
                };
                // Only what the disk can't give back.
                let content = (is_untitled(path) || dirty)
                    .then(|| {
                        if current {
                            Some(crate::editor::buffer_text(&self.editor.content))
                        } else {
                            cached.and_then(CachedBuffer::text)
                        }
                    })
                    .flatten();
                let mut entry = FileSession {
                    path: String::new(),
                    path_raw: None,
                    content,
                    language,
                    dirty,
                    crlf: format.ending == crate::tabs::LineEnding::Crlf,
                    final_newline: format.final_newline,
                    stamp,
                };
                entry.set_path(path);
                entry
            })
            .collect();

        Session {
            files,
            active_file: self
                .current_file
                .as_ref()
                .filter(|p| !self.is_scratch(p) && !is_virtual(p))
                .map(|p| p.to_string_lossy().into_owned()),
            untitled_counter: self.untitled_counter,
        }
    }

    /// Writes the session now, and reports a failure: closing the app relies
    /// on it to keep unsaved buffers.
    pub(super) fn save_session_now(&mut self) -> Result<(), String> {
        // Closing the app also writes the scratch file being typed in.
        self.flush_scratch();
        if !self.session_owner {
            return Ok(());
        }
        self.bookmarks.save();
        let json = self
            .build_session()
            .to_json()
            .ok_or_else(|| "session could not be serialized".to_string())?;
        crate::session::Session::write(&json).map_err(|e| e.to_string())?;
        self.last_session = fingerprint(&json);
        self.session_rev = self.rev;
        Ok(())
    }

    /// The autosave: the session is built here but written in the background.
    /// `None` when it didn't change since the last write.
    pub(super) fn session_write_job(
        &mut self,
    ) -> Option<impl std::future::Future<Output = Result<(), String>> + Send + 'static> {
        if !self.session_owner {
            return None;
        }
        self.bookmarks.save();
        self.session_rev = self.rev;
        let json = self.build_session().to_json()?;
        let print = fingerprint(&json);
        if print == self.last_session {
            return None;
        }
        self.last_session = print;
        Some(async move {
            tokio::task::spawn_blocking(move || {
                crate::session::Session::write(&json).map_err(|e| e.to_string())
            })
            .await
            .map_err(|e| e.to_string())?
        })
    }

    /// Rebuilds the tabs of the last run. Only the active tab is read from
    /// disk (when it was saved); the others wait until they are opened.
    pub(super) fn restore_session(&mut self, session: crate::session::Session) -> Task<Message> {
        use crate::tabs::LineEnding;
        self.untitled_counter = session.untitled_counter.max(1);

        for fs in &session.files {
            let path = fs.path();
            let format = TextFormat {
                ending: if fs.crlf {
                    LineEnding::Crlf
                } else {
                    LineEnding::Lf
                },
                final_newline: fs.final_newline,
            };
            let buffer = match &fs.content {
                Some(text) => CachedBuffer {
                    content: Some(crate::code_editor::Content::with_text(text)),
                    language: fs.language.clone(),
                    dirty: fs.dirty,
                    stamp: fs.stamp,
                    format,
                    saved_fp: (!fs.dirty).then(|| fingerprint(text)),
                    view_mode: ViewMode::Raw,
                },
                // A file deleted since the last run is dropped, not shown empty.
                None if !is_untitled(&path) => {
                    if !path.exists() {
                        continue;
                    }
                    CachedBuffer {
                        content: None,
                        language: fs.language.clone(),
                        dirty: false,
                        stamp: None,
                        format,
                        saved_fp: None,
                        view_mode: ViewMode::Raw,
                    }
                }
                None => CachedBuffer::blank(),
            };
            self.sidebar.add_file(path.clone());
            self.file_cache.insert(path, buffer);
        }

        if self.sidebar.files().is_empty() {
            // First launch (or nothing restorable): a blank untitled file
            let path = untitled_path(1);
            self.sidebar.add_file(path.clone());
            self.current_file = Some(path);
            self.untitled_counter = 1;
            self.saved_fp = Some(fingerprint(""));
            return Task::none();
        }

        // Activate the previously active file, or the first one left
        let active_path = session
            .active_file
            .as_deref()
            .map(PathBuf::from)
            .filter(|p| self.file_cache.contains_key(p))
            .or_else(|| self.sidebar.files().first().cloned());

        let Some(path) = active_path else {
            return Task::none();
        };
        match self.file_cache.remove(&path) {
            Some(buffer) if buffer.content.is_some() => {
                // Modified here and there while the app was closed?
                let changed =
                    buffer.dirty && buffer.stamp.is_some() && DiskStamp::of(&path) != buffer.stamp;
                self.load_buffer(path, buffer);
                if changed {
                    self.external_warned = true;
                    self.set_status(t!("status.changed_on_disk"), true);
                }
                Task::none()
            }
            Some(buffer) => {
                self.file_cache.insert(path.clone(), buffer);
                self.pending_open = Some(path.clone());
                Task::perform(read_file(path, false), Message::TabRead)
            }
            None => Task::none(),
        }
    }
}

/// The top edge of a bottom panel: a rule that can be dragged to resize it.
/// The height of a tab, and of their bar.
pub(super) const TAB_HEIGHT: f32 = 34.0;

pub(crate) fn resize_edge<'a>(dark: bool) -> Element<'a, Message> {
    iced::widget::mouse_area(
        container(Space::new().height(1))
            .width(Length::Fill)
            .padding(iced::Padding {
                top: 2.0,
                bottom: 2.0,
                ..Default::default()
            })
            .style(se_theme::rule(dark)),
    )
    .on_press(Message::PanelResizeStart)
    .interaction(iced::mouse::Interaction::ResizingVertically)
    .into()
}

impl SimpleEditApp {
    /// The narrow bar at the left: a button for each panel of the sidebar.
    pub(super) fn activity_bar(&self, dark: bool) -> Element<'_, Message> {
        let p = se_theme::palette(dark);
        let tool = |icon: Element<'static, Message>,
                    open: bool,
                    tip: String,
                    msg: Message|
         -> Element<'_, Message> {
            let button = button(container(icon).center_x(Length::Fill))
                .padding([7, 0])
                .width(Length::Fill)
                .on_press(msg)
                .style(move |theme: &iced::Theme, status| {
                    if open {
                        (se_theme::GhostButton { dark, active: true }).style()(theme, status)
                    } else {
                        (se_theme::Outline { dark }).style()(theme, status)
                    }
                });
            iced::widget::tooltip(
                button,
                container(text(tip).size(12))
                    .padding([4, 8])
                    .style(se_theme::tooltip(dark)),
                iced::widget::tooltip::Position::Right,
            )
            .gap(6)
            .delay(std::time::Duration::from_millis(400))
            .into()
        };
        let shown = |on: bool| if on { p.accent } else { p.text };
        // The panels of the sidebar at the top, the tools that open under the
        // editor at the bottom.
        container(
            column![
                tool(
                    crate::sidebar::icons::files(shown(self.panels.files)),
                    self.panels.files,
                    t!("sidebar.files").to_string(),
                    Message::TogglePanel(crate::sidebar::Panel::Files),
                ),
                tool(
                    crate::sidebar::icons::folder(shown(self.panels.projects)),
                    self.panels.projects,
                    t!("sidebar.projects").to_string(),
                    Message::TogglePanel(crate::sidebar::Panel::Projects),
                ),
                tool(
                    crate::sidebar::icons::database(shown(self.panels.database)),
                    self.panels.database,
                    t!("sidebar.database").to_string(),
                    Message::TogglePanel(crate::sidebar::Panel::Database),
                ),
                tool(
                    crate::sidebar::icons::api(shown(self.panels.api)),
                    self.panels.api,
                    t!("sidebar.api").to_string(),
                    Message::TogglePanel(crate::sidebar::Panel::Api),
                ),
                Space::new().height(Length::Fill),
                tool(
                    text(">_")
                        .size(13)
                        .font(se_theme::MONO)
                        .color(shown(self.terminal.open))
                        .into(),
                    self.terminal.open,
                    t!("terminal.title").to_string(),
                    Message::ToggleTerminal,
                ),
                tool(
                    crate::scratch::icon(shown(self.scratch.open)),
                    self.scratch.open,
                    t!("scratch.title").to_string(),
                    Message::ToggleScratch,
                ),
                tool(
                    super::bookmark_panel::ribbon(shown(self.bookmark_panel.open), false),
                    self.bookmark_panel.open,
                    t!("bookmarks.title").to_string(),
                    Message::ToggleBookmarkPanel,
                ),
                tool(
                    crate::git_panel::icon::branch(shown(self.git.open)),
                    self.git.open,
                    t!("git.title").to_string(),
                    Message::ToggleGit,
                ),
            ]
            .extend((!self.results.tabs.is_empty()).then(|| {
                let open = self.results.open;
                tool(
                    text('\u{ea6d}')
                        .font(iced_fonts::NERD_FONT)
                        .size(15)
                        .color(shown(open))
                        .into(),
                    open,
                    t!("results.panel").to_string(),
                    Message::Results(super::results::ResultsMsg::Hide),
                )
            }))
            .spacing(6),
        )
        .padding([8, 6])
        .width(Length::Fixed(super::ACTIVITY_WIDTH))
        .height(Length::Fill)
        .style(se_theme::panel(dark))
        .into()
    }

    /// The open files as tabs above the editor.
    pub(super) fn tab_bar(&self, dark: bool) -> Element<'_, Message> {
        use crate::sidebar::SidebarMessage;
        let p = se_theme::palette(dark);
        let files = self.sidebar.files();
        let dirty = self.dirty_files();
        let groups = self.tab_groups();
        // Told apart inside their group: its label says the project.
        let mut labels = vec![String::new(); files.len()];
        for group in &groups {
            let names: Vec<PathBuf> = group.tabs.iter().map(|t| files[*t].clone()).collect();
            for (t, name) in group.tabs.iter().zip(unique_labels(&names)) {
                labels[*t] = name;
            }
        }
        let slots = tab_groups::slots(&groups, &self.folded_tabs);
        let tabs: Vec<Element<Message>> = slots
            .iter()
            .map(|slot| {
                let index = match slot {
                    tab_groups::Slot::Label(g) => return self.tab_group_label(&groups[*g], dark),
                    tab_groups::Slot::Tab(t) => *t,
                };
                let (path, name) = (&files[index], labels[index].clone());
                let active = self.current_file.as_ref() == Some(path);
                let preview = self.preview_tab.as_ref() == Some(path);
                let mut label = row![
                    // Its kind, as the list of files shows it.
                    crate::file_icons::file(path, dark),
                    text(name)
                        .size(12.5)
                        .shaping(iced::widget::text::Shaping::Advanced)
                        // The preview tab, in italics: the next file opened
                        // from the tree takes its place.
                        .font(if preview {
                            se_theme::UI_ITALIC
                        } else {
                            iced::Font::DEFAULT
                        })
                        // How it stands in Git, as in the tree.
                        .color(
                            crate::sidebar::state_color(self.file_marks.git(path, false), dark)
                                .unwrap_or(if active { p.text } else { p.muted })
                        )
                ]
                .spacing(6)
                .align_y(Alignment::Center);
                if self.file_marks.has_errors(path, false) {
                    label = label.push(crate::sidebar::error_dot(dark));
                }
                if dirty.contains(path) {
                    label = label.push(
                        container(Space::new().width(6).height(6)).style(se_theme::dot(p.accent)),
                    );
                }
                label = label.push(
                    button(text("×").size(13).color(p.muted))
                        .padding([0, 4])
                        .on_press(Message::Sidebar(SidebarMessage::RemoveFile(path.clone())))
                        .style(
                            (se_theme::GhostButton {
                                dark,
                                active: false,
                            })
                            .style(),
                        ),
                );
                let tab = button(container(label).center_y(Length::Fixed(TAB_HEIGHT)))
                    .padding([0, 12])
                    .on_press(Message::Sidebar(SidebarMessage::OpenFile(path.clone())))
                    .style(se_theme::tab(dark, active));
                let menu_path = path.clone();
                let labels = (
                    t!("sidebar.close").to_string(),
                    t!("menu.file_close_others").to_string(),
                    t!("menu.file_close_all").to_string(),
                    t!("sidebar.show_in_tree").to_string(),
                    t!("sidebar.copy_path").to_string(),
                    t!("tabs.keep").to_string(),
                );
                let in_project = self.projects.roots.iter().any(|r| path.starts_with(r));
                let untitled = is_untitled(path);
                let with_menu = crate::pointer_area::context_menu(tab, move || {
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
                    container(
                        column![
                            item(
                                labels.0.clone(),
                                Some(Message::Sidebar(SidebarMessage::RemoveFile(
                                    menu_path.clone()
                                )))
                            ),
                            item(
                                labels.1.clone(),
                                Some(Message::CloseTabs(Some(menu_path.clone())))
                            ),
                            item(labels.2.clone(), Some(Message::CloseTabs(None))),
                            item(
                                labels.3.clone(),
                                in_project.then(|| Message::ShowInProjects(menu_path.clone()))
                            ),
                            item(
                                labels.4.clone(),
                                (!untitled).then(|| Message::CopyPath(menu_path.clone()))
                            ),
                            item(labels.5.clone(), preview.then_some(Message::KeepTab)),
                        ]
                        .spacing(1)
                        .padding(6),
                    )
                    .style(se_theme::card(dark))
                    .into()
                });
                // The open tab has the editor's color, over the bar's bottom
                // hairline: it joins the editor. A hairline splits the tabs.
                let framed = container(with_menu)
                    .height(Length::Fixed(TAB_HEIGHT))
                    .style(se_theme::tab_frame(dark, active));
                row![
                    self.with_project_line(framed.into(), path),
                    container(Space::new().width(1).height(Length::Fixed(TAB_HEIGHT)))
                        .style(se_theme::rule(dark)),
                ]
                .into()
            })
            .collect();
        // The bar scrolls with the wheel and its tabs are dragged to reorder
        // them, all inside the widget so that it follows the pointer.
        let active = slots.iter().position(|slot| {
            matches!(slot, tab_groups::Slot::Tab(t)
                if self.current_file.as_ref() == files.get(*t))
        });
        let strip = crate::tab_strip::tab_strip(
            tabs,
            active,
            Message::TabMoved,
            p.tabs,
            iced::Color { a: 0.45, ..p.muted },
        )
        .on_close(Message::CloseTabAt)
        .on_double_click(Message::TabDoubleClick);
        // Every tab in a list, for when they don't all fit.
        let list = crate::pointer_area::pointer_area(
            iced::widget::mouse_area(
                container(text("⌄").size(14).color(p.muted))
                    .padding([0, 10])
                    .center_y(Length::Fixed(TAB_HEIGHT)),
            )
            .interaction(iced::mouse::Interaction::Pointer),
        )
        .on_press_in_window(Message::OpenTabList);
        // Under the tabs, the bar's bottom hairline, that the open tab hides.
        let bottom = column![
            Space::new().width(Length::Fill).height(TAB_HEIGHT - 1.0),
            container(Space::new().height(1))
                .width(Length::Fill)
                .style(se_theme::rule(dark)),
        ];
        container(iced::widget::stack![
            bottom,
            row![strip, list].align_y(Alignment::Center)
        ])
        .style(se_theme::tab_bar(dark))
        .into()
    }
}

impl SimpleEditApp {
    /// The list of every open tab, under its button.
    pub(super) fn tab_list_view(&self, dark: bool) -> Element<'_, Message> {
        let p = se_theme::palette(dark);
        let files = self.sidebar.files();
        let labels = unique_labels(files);
        let dirty = self.dirty_files();
        let rows: Vec<Element<Message>> = files
            .iter()
            .zip(labels)
            .map(|(path, name)| {
                let current = self.current_file.as_ref() == Some(path);
                let folder = path
                    .parent()
                    .filter(|_| !is_untitled(path))
                    .map(short_dir)
                    .unwrap_or_default();
                let color = self
                    .tab_key(path)
                    .map_or(iced::Color::TRANSPARENT, |r| self.project_color(&r));
                let mut line = row![
                    container(Space::new().width(8).height(8)).style(se_theme::dot(color)),
                    crate::file_icons::file(path, dark),
                    text(name)
                        .size(13)
                        .shaping(iced::widget::text::Shaping::Advanced)
                        .color(if current { p.accent } else { p.text }),
                    text(folder)
                        .size(11.5)
                        .color(p.muted)
                        .shaping(iced::widget::text::Shaping::Advanced),
                ]
                .spacing(8)
                .align_y(Alignment::Center);
                if dirty.contains(path) {
                    line = line.push(
                        container(Space::new().width(6).height(6)).style(se_theme::dot(p.accent)),
                    );
                }
                button(line)
                    .padding([6, 10])
                    .width(Length::Fill)
                    .on_press(Message::PickTab(path.clone()))
                    .style(
                        (se_theme::MenuItem {
                            dark,
                            radius: se_theme::R_SM,
                            highlighted: current,
                        })
                        .style(),
                    )
                    .into()
            })
            .collect();
        container(
            scrollable(column(rows).spacing(1).padding(6))
                .style(se_theme::Scrollbar { dark }.style()),
        )
        .width(Length::Fixed(TAB_LIST_WIDTH))
        .max_height(420)
        .style(se_theme::card(dark))
        .into()
    }
}

/// Width of the list of open tabs.
pub(super) const TAB_LIST_WIDTH: f32 = 340.0;
