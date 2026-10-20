//! The overlay cards and the other views of the app.

use super::*;
use iced::widget::{column, row};

// ─── Overlay views ──────────────────────────────────────────────────────────

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
            | ActiveOverlay::Shortcuts
            | ActiveOverlay::Palette
            | ActiveOverlay::ConfirmClose(_)
            | ActiveOverlay::ConfirmQuit(_)
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
                .map(|p| p.to_string_lossy().into_owned()),
            untitled_counter: self.untitled_counter,
        }
    }

    /// Writes the session now, and reports a failure: closing the app relies
    /// on it to keep unsaved buffers.
    pub(super) fn save_session_now(&mut self) -> Result<(), String> {
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
