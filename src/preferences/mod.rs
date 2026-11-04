//! The Preferences window: a navigation column on the left (Editor,
//! Appearance, Shortcuts, Language, Updates) and settings cards on the right.

use iced::{
    widget::{
        button, column, container, pick_list, row, scrollable, slider, text, text_input, Space,
    },
    Alignment, Element, Length, Padding,
};

use crate::{
    app::Message,
    config::{Config, ShortcutTarget},
    theme,
};

#[derive(Debug, Clone)]
pub enum PreferencesMessage {
    PageSelected(PrefsPage),
    FontSizeChanged(f32),
    TabWidthChanged(usize),
    UseSpacesToggled(bool),
    ShowLineNumbersToggled(bool),
    AutoIndentToggled(bool),
    AutocompleteBracketsToggled(bool),
    AutocompleteQuotesToggled(bool),
    HighlightCurrentLineToggled(bool),
    ShowPageGuideToggled(bool),
    AutoSaveToggled(bool),
    PageGuideColumnChanged(usize),
    ShowMinimapToggled(bool),
    WordWrapToggled(bool),
    LocaleChanged(String),
    /// The scratch folder being typed (applied on Enter).
    ScratchDirTyped(String),
}

/// The pages of the navigation column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefsPage {
    Editor,
    Appearance,
    Shortcuts,
    Language,
    Git,
    Lint,
    Servers,
    Updates,
}

impl PrefsPage {
    const ALL: [PrefsPage; 8] = [
        PrefsPage::Editor,
        PrefsPage::Appearance,
        PrefsPage::Shortcuts,
        PrefsPage::Language,
        PrefsPage::Git,
        PrefsPage::Lint,
        PrefsPage::Servers,
        PrefsPage::Updates,
    ];

    fn label(self) -> String {
        match self {
            PrefsPage::Editor => t!("prefs.nav_editor"),
            PrefsPage::Appearance => t!("prefs.nav_appearance"),
            PrefsPage::Shortcuts => t!("prefs.nav_shortcuts"),
            PrefsPage::Language => t!("prefs.language"),
            PrefsPage::Git => t!("prefs.nav_git"),
            PrefsPage::Lint => t!("prefs.nav_lint"),
            PrefsPage::Servers => t!("prefs.nav_servers"),
            PrefsPage::Updates => t!("prefs.nav_updates"),
        }
        .to_string()
    }

    fn title(self) -> String {
        match self {
            PrefsPage::Editor => t!("prefs.editor").to_string(),
            other => other.label(),
        }
    }
}

/// How often the Git panel fetches by itself, in minutes (0: never).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AutoFetch(u32);

impl AutoFetch {
    const ALL: [AutoFetch; 5] = [
        AutoFetch(0),
        AutoFetch(1),
        AutoFetch(5),
        AutoFetch(10),
        AutoFetch(30),
    ];
}

impl std::fmt::Display for AutoFetch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0 {
            0 => f.write_str(&t!("prefs.git_fetch_never")),
            n => f.write_str(&t!("prefs.git_fetch_every", n = n)),
        }
    }
}

/// An entry of the language select.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LocaleChoice {
    code: &'static str,
    name: &'static str,
}

impl std::fmt::Display for LocaleChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name)
    }
}

// The language names stay in their own language.
const LOCALES: [LocaleChoice; 2] = [
    LocaleChoice {
        code: "en",
        name: "English",
    },
    LocaleChoice {
        code: "fr",
        name: "Français",
    },
];

/// What the Servers page shows.
#[derive(Debug, Default)]
pub struct ServerStatus {
    /// Programs of the language servers that can be run.
    pub ready: std::collections::HashSet<&'static str>,
    /// Of those, the ones SimpleEdit installed and can remove.
    pub removable: std::collections::HashSet<&'static str>,
    /// Programs being installed.
    pub installing: std::collections::HashSet<&'static str>,
    /// Programs being removed.
    pub removing: std::collections::HashSet<&'static str>,
}

pub struct PreferencesState {
    pub page: PrefsPage,
    pub scratch_draft: String,
}

impl PreferencesState {
    pub fn from_config(config: &Config) -> Self {
        Self {
            page: PrefsPage::Editor,
            scratch_draft: config.scratch_dir.clone(),
        }
    }

    /// Applies a change. Returns whether the config changed (page
    /// navigation doesn't touch it).
    pub fn update(&mut self, msg: PreferencesMessage, config: &mut Config) -> bool {
        match msg {
            PreferencesMessage::PageSelected(page) => {
                self.page = page;
                return false;
            }
            PreferencesMessage::FontSizeChanged(v) => {
                config.font_size = v;
            }
            PreferencesMessage::TabWidthChanged(v) => {
                config.tab_width = v;
            }
            PreferencesMessage::UseSpacesToggled(v) => {
                config.use_spaces = v;
            }
            PreferencesMessage::ShowLineNumbersToggled(v) => {
                config.show_line_numbers = v;
            }
            PreferencesMessage::AutoIndentToggled(v) => {
                config.auto_indent = v;
            }
            PreferencesMessage::AutocompleteBracketsToggled(v) => {
                config.autocomplete_brackets = v;
            }
            PreferencesMessage::AutocompleteQuotesToggled(v) => {
                config.autocomplete_quotes = v;
            }
            PreferencesMessage::HighlightCurrentLineToggled(v) => {
                config.highlight_current_line = v;
            }
            PreferencesMessage::PageGuideColumnChanged(v) => {
                config.page_guide_column = v;
            }
            PreferencesMessage::ShowPageGuideToggled(v) => {
                config.show_page_guide = v;
            }
            PreferencesMessage::AutoSaveToggled(v) => {
                config.auto_save = v;
            }
            PreferencesMessage::ShowMinimapToggled(v) => {
                config.show_minimap = v;
            }
            PreferencesMessage::WordWrapToggled(v) => {
                config.word_wrap = v;
            }
            PreferencesMessage::ScratchDirTyped(v) => {
                self.scratch_draft = v;
                return false;
            }
            PreferencesMessage::LocaleChanged(v) => {
                config.locale = v.clone();
                rust_i18n::set_locale(&v);
            }
        }
        true
    }

    /// The whole window: title row, navigation column, page.
    pub fn view(
        &self,
        config: &Config,
        capturing: Option<ShortcutTarget>,
        servers: &ServerStatus,
        near: Option<&std::path::Path>,
    ) -> Element<'_, Message> {
        let dark = config.dark_mode;
        let p = theme::palette(dark);

        let close = button(text("×").size(17))
            .padding([4, 8])
            .on_press(Message::TogglePreferences)
            .style(
                (theme::GhostButton {
                    dark,
                    active: false,
                })
                .style(),
            );

        let title_row = row![
            Space::new().width(40),
            text(t!("prefs.window_title").to_string())
                .size(13)
                .font(theme::UI_SEMIBOLD)
                .width(Length::Fill)
                .align_x(iced::alignment::Horizontal::Center),
            container(close)
                .width(40)
                .align_x(iced::alignment::Horizontal::Right),
        ]
        .padding([0, 16])
        .height(46)
        .align_y(Alignment::Center);

        let nav_items: Vec<Element<Message>> = PrefsPage::ALL
            .iter()
            .map(|page| {
                button(text(page.label()).size(13))
                    .padding([8, 12])
                    .width(Length::Fill)
                    .on_press(Message::Preferences(PreferencesMessage::PageSelected(
                        *page,
                    )))
                    .style(
                        (theme::SidebarRow {
                            dark,
                            selected: self.page == *page,
                        })
                        .style(),
                    )
                    .into()
            })
            .collect();

        let nav = container(column(nav_items).spacing(2))
            .padding([16, 10])
            .width(210)
            .height(Length::Fill)
            .style(move |_: &iced::Theme| container::Style {
                text_color: Some(p.text),
                background: Some(iced::Background::Color(p.surface)),
                border: iced::Border {
                    color: iced::Color::TRANSPARENT,
                    width: 0.0,
                    radius: iced::border::Radius::default().bottom_left(theme::R_PALETTE),
                },
                shadow: iced::Shadow::default(),
                ..container::Style::default()
            });

        let page: Element<Message> = match self.page {
            PrefsPage::Editor => column![
                self.editor_card(config, dark),
                self.scratch_card(dark),
                self.language_card(config, dark)
            ]
            .spacing(14)
            .into(),
            PrefsPage::Appearance => self.appearance_card(dark),
            PrefsPage::Shortcuts => shortcuts_list(dark, config, capturing),
            PrefsPage::Language => self.language_card(config, dark),
            PrefsPage::Git => self.git_card(config, dark),
            PrefsPage::Lint => lint_page(config, dark, near),
            PrefsPage::Servers => self.servers_page(dark, servers),
            PrefsPage::Updates => self.updates_card(dark),
        };

        let content = scrollable(
            column![text(self.page.title()).size(22).font(theme::UI_BOLD), page,]
                .spacing(18)
                .padding(Padding {
                    top: 26.0,
                    right: 40.0,
                    bottom: 26.0,
                    left: 40.0,
                })
                .width(Length::Fill),
        )
        .style(theme::Scrollbar { dark }.style())
        .width(Length::Fill)
        .height(Length::Fill);

        container(column![
            title_row,
            container(Space::new().height(1))
                .width(Length::Fill)
                .style(theme::rule(dark)),
            row![nav, content].height(Length::Fill),
        ])
        .style(move |_: &iced::Theme| container::Style {
            text_color: Some(p.text),
            background: Some(iced::Background::Color(p.bg)),
            border: iced::Border {
                color: p.border_strong,
                width: 1.0,
                radius: theme::R_PALETTE.into(),
            },
            shadow: iced::Shadow {
                color: iced::Color {
                    a: 0.5,
                    ..iced::Color::BLACK
                },
                offset: iced::Vector::new(0.0, 18.0),
                blur_radius: 50.0,
            },
            ..container::Style::default()
        })
        .into()
    }

    fn editor_card(&self, config: &Config, dark: bool) -> Element<'_, Message> {
        let font_size = row![
            slider(8.0..=32.0, config.font_size, |v| {
                Message::Preferences(PreferencesMessage::FontSizeChanged(v.round()))
            })
            .step(1.0_f32)
            .width(140)
            .style((theme::Slider { dark }).style()),
            text(format!("{:.0} px", config.font_size))
                .size(13)
                .font(theme::MONO)
                .width(48)
                .align_x(iced::alignment::Horizontal::Right),
        ]
        .spacing(10)
        .align_y(Alignment::Center);

        let tab_width = {
            let seg = |width: usize| {
                button(text(width.to_string()).size(12).font(theme::MONO))
                    .padding([3, 12])
                    .on_press(Message::Preferences(PreferencesMessage::TabWidthChanged(
                        width,
                    )))
                    .style(
                        (theme::Segment {
                            dark,
                            active: config.tab_width == width,
                            radius: theme::R_SM,
                        })
                        .style(),
                    )
            };
            container(row![seg(2), seg(4), seg(8)].spacing(2))
                .padding(2)
                .style(theme::field(dark, false, theme::R_ROW))
        };

        let guide_column = {
            let seg = |column: usize| {
                button(text(column.to_string()).size(12).font(theme::MONO))
                    .padding([3, 10])
                    .on_press(Message::Preferences(
                        PreferencesMessage::PageGuideColumnChanged(column),
                    ))
                    .style(
                        (theme::Segment {
                            dark,
                            active: config.page_guide_column == column,
                            radius: theme::R_SM,
                        })
                        .style(),
                    )
            };
            container(row![seg(72), seg(80), seg(100), seg(120)].spacing(2))
                .padding(2)
                .style(theme::field(dark, false, theme::R_ROW))
        };

        let toggle = |label: String, on: bool, msg: fn(bool) -> PreferencesMessage| {
            setting_row(label, switch(dark, on, Message::Preferences(msg(!on))))
        };

        let rows: Vec<Element<Message>> = vec![
            setting_row(t!("prefs.font_size").to_string(), font_size.into()),
            setting_row(t!("prefs.tab_width").to_string(), tab_width.into()),
            toggle(
                t!("prefs.use_spaces").to_string(),
                config.use_spaces,
                PreferencesMessage::UseSpacesToggled,
            ),
            toggle(
                t!("prefs.show_line_numbers").to_string(),
                config.show_line_numbers,
                PreferencesMessage::ShowLineNumbersToggled,
            ),
            toggle(
                t!("prefs.auto_indent").to_string(),
                config.auto_indent,
                PreferencesMessage::AutoIndentToggled,
            ),
            toggle(
                t!("prefs.autocomplete_brackets").to_string(),
                config.autocomplete_brackets,
                PreferencesMessage::AutocompleteBracketsToggled,
            ),
            toggle(
                t!("prefs.autocomplete_quotes").to_string(),
                config.autocomplete_quotes,
                PreferencesMessage::AutocompleteQuotesToggled,
            ),
            toggle(
                t!("prefs.highlight_current_line").to_string(),
                config.highlight_current_line,
                PreferencesMessage::HighlightCurrentLineToggled,
            ),
            toggle(
                t!("prefs.auto_save").to_string(),
                config.auto_save,
                PreferencesMessage::AutoSaveToggled,
            ),
            toggle(
                t!("prefs.show_page_guide").to_string(),
                config.show_page_guide,
                PreferencesMessage::ShowPageGuideToggled,
            ),
            setting_row(
                t!("prefs.page_guide_column").to_string(),
                guide_column.into(),
            ),
            toggle(
                t!("prefs.show_minimap").to_string(),
                config.show_minimap,
                PreferencesMessage::ShowMinimapToggled,
            ),
            toggle(
                t!("prefs.word_wrap").to_string(),
                config.word_wrap,
                PreferencesMessage::WordWrapToggled,
            ),
        ];

        card_of_rows(dark, rows)
    }

    fn appearance_card(&self, dark: bool) -> Element<'_, Message> {
        let seg = |label: String, is_dark: bool| {
            button(text(label).size(12))
                .padding([3, 12])
                .on_press(Message::ThemeChanged(is_dark))
                .style(
                    (theme::Segment {
                        dark,
                        active: dark == is_dark,
                        radius: theme::R_SM,
                    })
                    .style(),
                )
        };
        let control = container(
            row![
                seg(t!("toolbar.dark").to_string(), true),
                seg(t!("toolbar.light").to_string(), false)
            ]
            .spacing(2),
        )
        .padding(2)
        .style(theme::field(dark, false, theme::R_ROW));

        card_of_rows(
            dark,
            vec![setting_row(t!("prefs.theme").to_string(), control.into())],
        )
    }

    /// Where scratch files are kept.
    fn scratch_card(&self, dark: bool) -> Element<'_, Message> {
        let field = text_input(
            crate::scratch::default_dir().to_string_lossy().as_ref(),
            &self.scratch_draft,
        )
        .on_input(|v| Message::Preferences(PreferencesMessage::ScratchDirTyped(v)))
        .on_submit(Message::SetScratchDir(self.scratch_draft.clone()))
        .padding([6, 10])
        .size(13)
        .width(300)
        .style(theme::Field::new(dark).style());
        let small = |label: String, msg: Message| {
            button(text(label).size(12))
                .padding([6, 12])
                .on_press(msg)
                .style((theme::Outline { dark }).style())
        };
        let control = row![
            field,
            small(
                t!("prefs.scratch_browse").to_string(),
                Message::PickScratchDir
            ),
            small(
                t!("prefs.scratch_reset").to_string(),
                Message::SetScratchDir(String::new())
            ),
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        card_of_rows(
            dark,
            vec![setting_row(
                t!("prefs.scratch_dir").to_string(),
                control.into(),
            )],
        )
    }

    /// How a pull and a merge are made when the Git panel asks for one.
    fn git_card(&self, config: &Config, dark: bool) -> Element<'_, Message> {
        let pull = pick_list(
            crate::git::PullMode::ALL,
            Some(config.git_pull_mode),
            Message::SetPullMode,
        )
        .padding([6, 12])
        .width(220)
        .text_size(13)
        .style(theme::Select { dark }.style())
        .menu_style(theme::Select { dark }.menu_style());
        let merge = pick_list(
            crate::git::MergeMode::ALL,
            Some(config.git_merge_mode),
            Message::SetMergeMode,
        )
        .padding([6, 12])
        .width(220)
        .text_size(13)
        .style(theme::Select { dark }.style())
        .menu_style(theme::Select { dark }.menu_style());
        let fetch = pick_list(
            AutoFetch::ALL,
            Some(AutoFetch(config.git_auto_fetch_minutes)),
            |choice| Message::SetAutoFetch(choice.0),
        )
        .padding([6, 12])
        .width(220)
        .text_size(13)
        .style(theme::Select { dark }.style())
        .menu_style(theme::Select { dark }.menu_style());
        card_of_rows(
            dark,
            vec![
                setting_row(t!("prefs.git_pull").to_string(), pull.into()),
                setting_row(t!("prefs.git_merge").to_string(), merge.into()),
                setting_row(t!("prefs.git_auto_fetch").to_string(), fetch.into()),
            ],
        )
    }

    fn language_card(&self, config: &Config, dark: bool) -> Element<'_, Message> {
        let selected = LOCALES.iter().copied().find(|l| l.code == config.locale);
        let select = pick_list(LOCALES, selected, |choice| {
            Message::Preferences(PreferencesMessage::LocaleChanged(choice.code.to_string()))
        })
        .padding([6, 12])
        .width(150)
        .text_size(13)
        .style(theme::Select { dark }.style())
        .menu_style(theme::Select { dark }.menu_style());

        card_of_rows(
            dark,
            vec![setting_row(t!("prefs.language").to_string(), select.into())],
        )
    }

    /// The language servers: installed ones give exact answers to Ctrl+click
    /// on a name; without one the editor searches by patterns.
    fn servers_page(&self, dark: bool, status: &ServerStatus) -> Element<'_, Message> {
        use crate::lsp_install::{Installer, SERVERS};
        let p = theme::palette(dark);
        let note = |label: String| text(label).size(12).color(p.muted);
        let rows = SERVERS
            .iter()
            .map(|info| {
                let name = format!("{} ({})", info.label, info.extensions.join(" "));
                let control: Element<Message> = if status.removing.contains(info.binary) {
                    note(t!("prefs.server_uninstalling").to_string()).into()
                } else if status.ready.contains(info.binary) {
                    let installed = note(t!("prefs.server_installed").to_string());
                    if status.removable.contains(info.binary) {
                        row![
                            installed,
                            button(text(t!("prefs.server_uninstall").to_string()).size(12))
                                .padding([6, 12])
                                .on_press(Message::UninstallServer(info.binary))
                                .style((theme::Outline { dark }).style()),
                        ]
                        .spacing(12)
                        .align_y(Alignment::Center)
                        .into()
                    } else {
                        installed.into()
                    }
                } else if status.installing.contains(info.binary) {
                    note(t!("prefs.server_installing").to_string()).into()
                } else if let Installer::Manual(hint) = info.installer {
                    note(hint.to_string()).font(theme::MONO).into()
                } else {
                    button(text(t!("prefs.server_install").to_string()).size(12))
                        .padding([6, 12])
                        .on_press(Message::InstallServer(info.binary))
                        .style((theme::Outline { dark }).style())
                        .into()
                };
                setting_row(name, control)
            })
            .collect();
        column![
            text(t!("prefs.servers_intro").to_string())
                .size(12)
                .color(p.muted),
            card_of_rows(dark, rows),
        ]
        .spacing(12)
        .into()
    }

    fn updates_card(&self, dark: bool) -> Element<'_, Message> {
        let version = text(format!("v{}", env!("CARGO_PKG_VERSION")))
            .size(13)
            .font(theme::MONO);
        let check = button(text(t!("menu.help_check_updates").to_string()).size(12))
            .padding([6, 12])
            .on_press(Message::CheckForUpdate)
            .style((theme::Outline { dark }).style());

        card_of_rows(
            dark,
            vec![
                setting_row(t!("prefs.current_version").to_string(), version.into()),
                setting_row(t!("prefs.updates").to_string(), check.into()),
            ],
        )
    }
}

/// A label on the left, its control on the right.
fn setting_row<'a>(label: String, control: Element<'a, Message>) -> Element<'a, Message> {
    row![text(label).size(13).width(Length::Fill), control]
        .spacing(16)
        .padding([12, 16])
        .align_y(Alignment::Center)
        .into()
}

/// Rows on a bordered card, separated by hairlines.
fn card_of_rows<'a>(dark: bool, rows: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    let mut col = column![];
    let last = rows.len().saturating_sub(1);
    for (i, row_el) in rows.into_iter().enumerate() {
        col = col.push(row_el);
        if i != last {
            col = col.push(
                container(Space::new().height(1))
                    .width(Length::Fill)
                    .style(theme::rule(dark)),
            );
        }
    }
    container(col)
        .width(Length::Fill)
        .style(theme::settings_card(dark))
        .into()
}

/// A 36×20 switch with a 14px knob.
pub fn switch(dark: bool, on: bool, message: Message) -> Element<'static, Message> {
    let knob = container(Space::new().width(14).height(14)).style(theme::switch_knob(dark, on));
    button(row![Space::new().width(if on { 17.0 } else { 0.0 }), knob])
        .padding(3)
        .width(36)
        .height(20)
        .on_press(message)
        .style((theme::SwitchTrack { dark, on }).style())
        .into()
}

/// The configurable shortcuts: one row per action, click a key to rebind it.
/// Used by the Preferences page and by Help > Configure shortcuts.
pub fn shortcuts_list(
    dark: bool,
    config: &Config,
    capturing: Option<ShortcutTarget>,
) -> Element<'static, Message> {
    let p = theme::palette(dark);
    let sc = &config.shortcuts;

    let entries: Vec<(ShortcutTarget, String, String)> = vec![
        (
            ShortcutTarget::Undo,
            t!("shortcuts.undo").to_string(),
            sc.undo.display(),
        ),
        (
            ShortcutTarget::Redo,
            t!("shortcuts.redo").to_string(),
            sc.redo.display(),
        ),
        (
            ShortcutTarget::DuplicateLine,
            t!("shortcuts.duplicate_line").to_string(),
            sc.duplicate_line.display(),
        ),
        (
            ShortcutTarget::MoveLineUp,
            t!("shortcuts.move_line_up").to_string(),
            sc.move_line_up.display(),
        ),
        (
            ShortcutTarget::MoveLineDown,
            t!("shortcuts.move_line_down").to_string(),
            sc.move_line_down.display(),
        ),
        (
            ShortcutTarget::ToggleComment,
            t!("shortcuts.toggle_comment").to_string(),
            sc.toggle_comment.display(),
        ),
        (
            ShortcutTarget::DeleteLine,
            t!("shortcuts.delete_line").to_string(),
            sc.delete_line.display(),
        ),
        (
            ShortcutTarget::ToggleBookmark,
            t!("shortcuts.toggle_bookmark").to_string(),
            sc.toggle_bookmark.display(),
        ),
        (
            ShortcutTarget::NextBookmark,
            t!("shortcuts.next_bookmark").to_string(),
            sc.next_bookmark.display(),
        ),
        (
            ShortcutTarget::PrevBookmark,
            t!("shortcuts.prev_bookmark").to_string(),
            sc.prev_bookmark.display(),
        ),
        (
            ShortcutTarget::NavBack,
            t!("shortcuts.nav_back").to_string(),
            sc.nav_back.display(),
        ),
        (
            ShortcutTarget::NavForward,
            t!("shortcuts.nav_forward").to_string(),
            sc.nav_forward.display(),
        ),
        (
            ShortcutTarget::ToggleTerminal,
            t!("shortcuts.toggle_terminal").to_string(),
            sc.toggle_terminal.display(),
        ),
        (
            ShortcutTarget::ToggleGit,
            t!("shortcuts.toggle_git").to_string(),
            sc.toggle_git.display(),
        ),
        (
            ShortcutTarget::GitCommit,
            t!("shortcuts.git_commit").to_string(),
            sc.git_commit.display(),
        ),
        (
            ShortcutTarget::GitPush,
            t!("shortcuts.git_push").to_string(),
            sc.git_push.display(),
        ),
        (
            ShortcutTarget::GitPull,
            t!("shortcuts.git_pull").to_string(),
            sc.git_pull.display(),
        ),
        (
            ShortcutTarget::GitBranches,
            t!("shortcuts.git_branches").to_string(),
            sc.git_branches.display(),
        ),
        (
            ShortcutTarget::BookmarkPanel,
            t!("shortcuts.bookmark_panel").to_string(),
            sc.bookmark_panel.display(),
        ),
        (
            ShortcutTarget::ToggleFold,
            t!("shortcuts.toggle_fold").to_string(),
            sc.toggle_fold.display(),
        ),
        (
            ShortcutTarget::NewFile,
            t!("shortcuts.new_file").to_string(),
            sc.new_file.display(),
        ),
        (
            ShortcutTarget::OpenFile,
            t!("shortcuts.open_file").to_string(),
            sc.open_file.display(),
        ),
        (
            ShortcutTarget::SaveFile,
            t!("shortcuts.save_file").to_string(),
            sc.save_file.display(),
        ),
        (
            ShortcutTarget::SaveAs,
            t!("shortcuts.save_as").to_string(),
            sc.save_as.display(),
        ),
        (
            ShortcutTarget::CloseFile,
            t!("shortcuts.close_file").to_string(),
            sc.close_file.display(),
        ),
        (
            ShortcutTarget::Find,
            t!("shortcuts.find").to_string(),
            sc.find.display(),
        ),
        (
            ShortcutTarget::SelectAll,
            t!("shortcuts.select_all").to_string(),
            sc.select_all.display(),
        ),
        (
            ShortcutTarget::FormatCode,
            t!("shortcuts.format_code").to_string(),
            sc.format_code.display(),
        ),
        (
            ShortcutTarget::GotoLine,
            t!("shortcuts.goto_line").to_string(),
            sc.goto_line.display(),
        ),
        (
            ShortcutTarget::CommandPalette,
            t!("shortcuts.command_palette").to_string(),
            sc.command_palette.display(),
        ),
        (
            ShortcutTarget::ToggleSidebar,
            t!("shortcuts.toggle_sidebar").to_string(),
            sc.toggle_sidebar.display(),
        ),
        (
            ShortcutTarget::Quit,
            t!("shortcuts.quit").to_string(),
            sc.quit.display(),
        ),
    ];

    let rows: Vec<Element<'static, Message>> = entries
        .into_iter()
        .map(|(target, label, key)| {
            let is_capturing = capturing == Some(target);
            let key_label = if is_capturing {
                t!("shortcuts.capturing").to_string()
            } else {
                key
            };
            let key_button = button(
                text(key_label)
                    .size(12)
                    .font(theme::MONO)
                    .color(if is_capturing { p.accent } else { p.text }),
            )
            .padding([4, 10])
            .on_press(Message::StartCaptureShortcut(target))
            .style(
                (theme::Segment {
                    dark,
                    active: is_capturing,
                    radius: theme::R_SM,
                })
                .style(),
            );
            setting_row(label, key_button.into())
        })
        .collect();

    column![
        card_of_rows(dark, rows),
        text(if capturing.is_some() {
            t!("shortcuts.capturing").to_string()
        } else {
            t!("shortcuts.click_to_rebind").to_string()
        })
        .size(11.5)
        .color(p.muted),
    ]
    .spacing(10)
    .into()
}

/// The Lint page: whether and how problems are shown, then the tools of
/// each language.
fn lint_page<'a>(
    config: &Config,
    dark: bool,
    near: Option<&std::path::Path>,
) -> Element<'a, Message> {
    use crate::lint::tools::{availability, Availability, GROUPS, LSP};
    use crate::lint::Setting;
    let p = theme::palette(dark);
    let near = near.unwrap_or(std::path::Path::new("/"));
    let general = card_of_rows(
        dark,
        vec![
            setting_row(
                t!("lint.enabled").to_string(),
                switch(
                    dark,
                    config.lint_enabled,
                    Message::Lint(Setting::Enabled(!config.lint_enabled)),
                ),
            ),
            setting_row(
                t!("lint.highlight").to_string(),
                switch(
                    dark,
                    config.lint_highlight,
                    Message::Lint(Setting::Highlight(!config.lint_highlight)),
                ),
            ),
            setting_row(
                t!("lint.on_open").to_string(),
                switch(
                    dark,
                    config.lint_on_open,
                    Message::Lint(Setting::OnOpen(!config.lint_on_open)),
                ),
            ),
            setting_row(
                t!("lint.trigger").to_string(),
                pick_list(crate::lint::Trigger::ALL, Some(config.lint_trigger), |t| {
                    Message::Lint(Setting::Trigger(t))
                })
                .padding([6, 12])
                .width(260)
                .text_size(13)
                .style(theme::Select { dark }.style())
                .menu_style(theme::Select { dark }.menu_style())
                .into(),
            ),
            setting_row(
                t!("lint.level").to_string(),
                pick_list(crate::lint::Level::ALL, Some(config.lint_level), |l| {
                    Message::Lint(Setting::Level(l))
                })
                .padding([6, 12])
                .width(260)
                .text_size(13)
                .style(theme::Select { dark }.style())
                .menu_style(theme::Select { dark }.menu_style())
                .into(),
            ),
            setting_row(
                t!("lint.fix_level").to_string(),
                pick_list(crate::lint::Level::ALL, Some(config.lint_fix_level), |l| {
                    Message::Lint(Setting::FixLevel(l))
                })
                .padding([6, 12])
                .width(260)
                .text_size(13)
                .style(theme::Select { dark }.style())
                .menu_style(theme::Select { dark }.menu_style())
                .into(),
            ),
            setting_row(
                t!("lint.fix_on_save").to_string(),
                switch(
                    dark,
                    config.lint_fix_on_save,
                    Message::Lint(Setting::FixOnSave(!config.lint_fix_on_save)),
                ),
            ),
        ],
    );
    let note = |label: String| text(label).size(12).color(p.muted);
    let colors = card_of_rows(
        dark,
        [
            (crate::lint::Severity::Error, t!("lint.color_error")),
            (crate::lint::Severity::Warning, t!("lint.color_warning")),
            (crate::lint::Severity::Info, t!("lint.color_info")),
            (crate::lint::Severity::Hint, t!("lint.color_hint")),
        ]
        .into_iter()
        .map(|(severity, label)| {
            let shown = crate::lint::color(config, severity);
            let typed = config
                .lint_colors
                .get(crate::lint::color_key(severity))
                .cloned()
                .unwrap_or_default();
            let default = crate::lint::default_color(severity, config.dark_mode);
            let placeholder = format!(
                "#{:02x}{:02x}{:02x}",
                (default.r * 255.0).round() as u8,
                (default.g * 255.0).round() as u8,
                (default.b * 255.0).round() as u8
            );
            setting_row(
                label.to_string(),
                row![
                    pick_list(
                        crate::lint::Marking::ALL,
                        Some(crate::lint::marking(config, severity)),
                        move |m| Message::Lint(Setting::Marking(severity, m)),
                    )
                    .padding([5, 10])
                    .width(210)
                    .text_size(12.5)
                    .style(theme::Select { dark }.style())
                    .menu_style(theme::Select { dark }.menu_style()),
                    container(Space::new().width(18).height(18)).style(move |_: &iced::Theme| {
                        container::Style {
                            background: Some(iced::Background::Color(shown)),
                            border: iced::Border {
                                radius: 4.0.into(),
                                ..iced::Border::default()
                            },
                            ..container::Style::default()
                        }
                    }),
                    iced::widget::text_input(&placeholder, &typed)
                        .on_input(move |t| Message::Lint(Setting::Color(severity, t)))
                        .size(12.5)
                        .padding([5, 8])
                        .width(120)
                        .style(theme::Field::new(dark).style()),
                ]
                .spacing(10)
                .align_y(Alignment::Center)
                .into(),
            )
        })
        .collect(),
    );
    let mut blocks = column![
        general,
        colors,
        text(t!("lint.languages_hint").to_string())
            .size(12.5)
            .color(p.muted)
    ]
    .spacing(14);
    for group in GROUPS {
        let on = config.lint_languages.contains(group.id);
        let by_hand = config.lint_tools.get(group.id).filter(|_| on);
        let mut rows = vec![setting_row(
            group.name.to_string(),
            switch(dark, on, Message::Lint(Setting::Language(group.id, !on))),
        )];
        if on {
            rows.push(setting_row(
                format!("    {}", t!("lint.automatic")),
                switch(
                    dark,
                    by_hand.is_none(),
                    Message::Lint(Setting::Auto(group.id, by_hand.is_some())),
                ),
            ));
        }
        if let Some(chosen) = by_hand {
            let mut entries: Vec<(&'static str, String, String)> = group
                .tools
                .iter()
                .filter_map(|id| crate::lint::tools::tool_by_id(id))
                .map(|tool| {
                    let chosen = config.lint_programs.get(tool.id).map(String::as_str);
                    let state = match availability(tool, near, chosen) {
                        Availability::Ready(_) => t!("lint.status_ready"),
                        Availability::NoConfig(_) => t!("lint.status_no_config"),
                        Availability::NoProgram => t!("lint.status_missing"),
                    };
                    (tool.id, tool.name.to_string(), state.to_string())
                })
                .collect();
            if group.exts.iter().any(|e| crate::lint::tools::has_server(e)) {
                let state = if group.exts.iter().any(|e| crate::lsp::installed(e)) {
                    t!("lint.status_ready")
                } else {
                    t!("lint.status_missing")
                };
                entries.push((
                    LSP,
                    t!("lint.language_server").to_string(),
                    state.to_string(),
                ));
            }
            for (id, name, state) in entries {
                // The path of the tool, when it is not where it is looked for.
                let program: Element<Message> =
                    match crate::lint::tools::tool_by_id(id).filter(|t| !t.program.is_empty()) {
                        Some(tool) => {
                            let value = config.lint_programs.get(id).cloned().unwrap_or_default();
                            let placeholder = crate::lint::tools::program_path(tool, near, None)
                                .map_or_else(
                                    || t!("lint.path_placeholder").to_string(),
                                    |p| p.display().to_string(),
                                );
                            iced::widget::text_input(&placeholder, &value)
                                .on_input(move |t| Message::Lint(Setting::Program(id, t)))
                                .size(12)
                                .padding([5, 8])
                                .width(260)
                                .style(theme::Field::new(dark).style())
                                .into()
                        }
                        None => Space::new().width(0).into(),
                    };
                rows.push(setting_row(
                    format!("    {name}"),
                    row![
                        program,
                        note(state),
                        switch(
                            dark,
                            chosen.iter().any(|c| c == id),
                            Message::Lint(Setting::Tool(
                                group.id,
                                id,
                                !chosen.iter().any(|c| c == id),
                            )),
                        ),
                    ]
                    .spacing(10)
                    .align_y(Alignment::Center)
                    .into(),
                ));
            }
        }
        blocks = blocks.push(card_of_rows(dark, rows));
    }
    blocks.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferences_from_default_config() {
        let config = Config::default();
        let prefs = PreferencesState::from_config(&config);
        assert!(matches!(prefs.page, PrefsPage::Editor));
        assert_eq!(config.font_size, 14.0);
        assert_eq!(config.tab_width, 4);
        assert!(config.use_spaces);
        assert_eq!(config.locale, "en");
        assert!(config.highlight_current_line);
        assert!(!config.show_page_guide);
        assert!(config.show_minimap);
    }

    #[test]
    fn update_font_size() {
        let mut config = Config::default();
        let mut prefs = PreferencesState::from_config(&config);
        assert!(prefs.update(PreferencesMessage::FontSizeChanged(18.0), &mut config));
        assert_eq!(config.font_size, 18.0);
        assert_eq!(config.font_size, 18.0);
    }

    #[test]
    fn update_locale_persists_to_config() {
        let mut config = Config::default();
        let mut prefs = PreferencesState::from_config(&config);
        prefs.update(
            PreferencesMessage::LocaleChanged("fr".to_string()),
            &mut config,
        );
        assert_eq!(config.locale, "fr");
        assert_eq!(config.locale, "fr");
        assert_eq!(rust_i18n::locale().to_string(), "fr");

        prefs.update(
            PreferencesMessage::LocaleChanged("en".to_string()),
            &mut config,
        );
        assert_eq!(config.locale, "en");
        assert_eq!(rust_i18n::locale().to_string(), "en");
    }

    #[test]
    fn update_tab_settings_and_editor_toggles() {
        let mut config = Config::default();
        let mut prefs = PreferencesState::from_config(&config);
        prefs.update(PreferencesMessage::TabWidthChanged(2), &mut config);
        prefs.update(PreferencesMessage::UseSpacesToggled(false), &mut config);
        prefs.update(
            PreferencesMessage::HighlightCurrentLineToggled(false),
            &mut config,
        );
        prefs.update(PreferencesMessage::ShowPageGuideToggled(true), &mut config);
        prefs.update(PreferencesMessage::ShowMinimapToggled(false), &mut config);
        assert_eq!(config.tab_width, 2);
        assert!(!config.use_spaces);
        assert!(!config.highlight_current_line);
        assert!(config.show_page_guide);
        assert!(!config.show_minimap);
        prefs.update(PreferencesMessage::WordWrapToggled(false), &mut config);
        assert!(!config.word_wrap);
    }

    #[test]
    fn page_navigation_does_not_touch_the_config() {
        let mut config = Config::default();
        let mut prefs = PreferencesState::from_config(&config);
        assert!(!prefs.update(
            PreferencesMessage::PageSelected(PrefsPage::Shortcuts),
            &mut config
        ));
        assert_eq!(prefs.page, PrefsPage::Shortcuts);
    }
}
