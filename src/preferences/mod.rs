//! The Preferences window: a navigation column on the left (Editor,
//! Appearance, Shortcuts, Language, Updates) and settings cards on the right.

use iced::{
    widget::{button, column, container, pick_list, row, scrollable, slider, text, Space},
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
    LocaleChanged(String),
}

/// The pages of the navigation column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefsPage {
    Editor,
    Appearance,
    Shortcuts,
    Language,
    Updates,
}

impl PrefsPage {
    const ALL: [PrefsPage; 5] = [
        PrefsPage::Editor,
        PrefsPage::Appearance,
        PrefsPage::Shortcuts,
        PrefsPage::Language,
        PrefsPage::Updates,
    ];

    fn label(self) -> String {
        match self {
            PrefsPage::Editor => t!("prefs.nav_editor"),
            PrefsPage::Appearance => t!("prefs.nav_appearance"),
            PrefsPage::Shortcuts => t!("prefs.nav_shortcuts"),
            PrefsPage::Language => t!("prefs.language"),
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

pub struct PreferencesState {
    pub page: PrefsPage,
    pub font_size: f32,
    pub tab_width: usize,
    pub use_spaces: bool,
    pub show_line_numbers: bool,
    pub auto_indent: bool,
    pub autocomplete_brackets: bool,
    pub autocomplete_quotes: bool,
    pub highlight_current_line: bool,
    pub show_page_guide: bool,
    pub locale: String,
}

impl PreferencesState {
    pub fn from_config(config: &Config) -> Self {
        Self {
            page: PrefsPage::Editor,
            font_size: config.font_size,
            tab_width: config.tab_width,
            use_spaces: config.use_spaces,
            show_line_numbers: config.show_line_numbers,
            auto_indent: config.auto_indent,
            autocomplete_brackets: config.autocomplete_brackets,
            autocomplete_quotes: config.autocomplete_quotes,
            highlight_current_line: config.highlight_current_line,
            show_page_guide: config.show_page_guide,
            locale: config.locale.clone(),
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
                self.font_size = v;
                config.font_size = v;
            }
            PreferencesMessage::TabWidthChanged(v) => {
                self.tab_width = v;
                config.tab_width = v;
            }
            PreferencesMessage::UseSpacesToggled(v) => {
                self.use_spaces = v;
                config.use_spaces = v;
            }
            PreferencesMessage::ShowLineNumbersToggled(v) => {
                self.show_line_numbers = v;
                config.show_line_numbers = v;
            }
            PreferencesMessage::AutoIndentToggled(v) => {
                self.auto_indent = v;
                config.auto_indent = v;
            }
            PreferencesMessage::AutocompleteBracketsToggled(v) => {
                self.autocomplete_brackets = v;
                config.autocomplete_brackets = v;
            }
            PreferencesMessage::AutocompleteQuotesToggled(v) => {
                self.autocomplete_quotes = v;
                config.autocomplete_quotes = v;
            }
            PreferencesMessage::HighlightCurrentLineToggled(v) => {
                self.highlight_current_line = v;
                config.highlight_current_line = v;
            }
            PreferencesMessage::ShowPageGuideToggled(v) => {
                self.show_page_guide = v;
                config.show_page_guide = v;
            }
            PreferencesMessage::LocaleChanged(v) => {
                self.locale = v.clone();
                config.locale = v.clone();
                rust_i18n::set_locale(&v);
            }
        }
        true
    }

    /// The whole window: title row, navigation column, page.
    pub fn view(&self, config: &Config, capturing: Option<ShortcutTarget>) -> Element<'_, Message> {
        let dark = config.dark_mode;
        let p = theme::palette(dark);

        let close = button(text("×").size(17))
            .padding([4, 8])
            .on_press(Message::TogglePreferences)
            .style(iced::theme::Button::custom(theme::GhostButton {
                dark,
                active: false,
            }));

        let title_row = row![
            Space::with_width(40),
            text(t!("prefs.window_title").to_string())
                .size(13)
                .font(theme::UI_SEMIBOLD)
                .width(Length::Fill)
                .horizontal_alignment(iced::alignment::Horizontal::Center),
            container(close)
                .width(40)
                .align_x(iced::alignment::Horizontal::Right),
        ]
        .padding([0, 16])
        .height(46)
        .align_items(Alignment::Center);

        let nav_items: Vec<Element<Message>> = PrefsPage::ALL
            .iter()
            .map(|page| {
                button(text(page.label()).size(13))
                    .padding([8, 12])
                    .width(Length::Fill)
                    .on_press(Message::Preferences(PreferencesMessage::PageSelected(
                        *page,
                    )))
                    .style(iced::theme::Button::custom(theme::SidebarRow {
                        dark,
                        selected: self.page == *page,
                    }))
                    .into()
            })
            .collect();

        let nav = container(column(nav_items).spacing(2))
            .padding([16, 10])
            .width(210)
            .height(Length::Fill)
            .style(move |_: &iced::Theme| container::Appearance {
                text_color: Some(p.text),
                background: Some(iced::Background::Color(p.surface)),
                border: iced::Border {
                    color: iced::Color::TRANSPARENT,
                    width: 0.0,
                    radius: [0.0, 0.0, 0.0, theme::R_PALETTE].into(),
                },
                shadow: iced::Shadow::default(),
            });

        let page: Element<Message> = match self.page {
            PrefsPage::Editor => column![self.editor_card(dark), self.language_card(dark)]
                .spacing(14)
                .into(),
            PrefsPage::Appearance => self.appearance_card(dark),
            PrefsPage::Shortcuts => shortcuts_list(dark, config, capturing),
            PrefsPage::Language => self.language_card(dark),
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
        .style(iced::theme::Scrollable::Custom(Box::new(
            theme::Scrollbar { dark },
        )))
        .width(Length::Fill)
        .height(Length::Fill);

        container(column![
            title_row,
            container(Space::with_height(1))
                .width(Length::Fill)
                .style(theme::rule(dark)),
            row![nav, content].height(Length::Fill),
        ])
        .style(move |_: &iced::Theme| container::Appearance {
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
        })
        .into()
    }

    fn editor_card(&self, dark: bool) -> Element<'_, Message> {
        let font_size = row![
            slider(8.0..=32.0, self.font_size, |v| {
                Message::Preferences(PreferencesMessage::FontSizeChanged(v.round()))
            })
            .step(1.0)
            .width(140)
            .style(iced::theme::Slider::Custom(Box::new(theme::Slider {
                dark
            }))),
            text(format!("{:.0} px", self.font_size))
                .size(13)
                .font(theme::MONO)
                .width(48)
                .horizontal_alignment(iced::alignment::Horizontal::Right),
        ]
        .spacing(10)
        .align_items(Alignment::Center);

        let tab_width = {
            let seg = |width: usize| {
                button(text(width.to_string()).size(12).font(theme::MONO))
                    .padding([3, 12])
                    .on_press(Message::Preferences(PreferencesMessage::TabWidthChanged(
                        width,
                    )))
                    .style(iced::theme::Button::custom(theme::Segment {
                        dark,
                        active: self.tab_width == width,
                        radius: theme::R_SM,
                    }))
            };
            container(row![seg(2), seg(4), seg(8)].spacing(2))
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
                self.use_spaces,
                PreferencesMessage::UseSpacesToggled,
            ),
            toggle(
                t!("prefs.show_line_numbers").to_string(),
                self.show_line_numbers,
                PreferencesMessage::ShowLineNumbersToggled,
            ),
            toggle(
                t!("prefs.auto_indent").to_string(),
                self.auto_indent,
                PreferencesMessage::AutoIndentToggled,
            ),
            toggle(
                t!("prefs.autocomplete_brackets").to_string(),
                self.autocomplete_brackets,
                PreferencesMessage::AutocompleteBracketsToggled,
            ),
            toggle(
                t!("prefs.autocomplete_quotes").to_string(),
                self.autocomplete_quotes,
                PreferencesMessage::AutocompleteQuotesToggled,
            ),
            toggle(
                t!("prefs.highlight_current_line").to_string(),
                self.highlight_current_line,
                PreferencesMessage::HighlightCurrentLineToggled,
            ),
            toggle(
                t!("prefs.show_page_guide").to_string(),
                self.show_page_guide,
                PreferencesMessage::ShowPageGuideToggled,
            ),
        ];

        card_of_rows(dark, rows)
    }

    fn appearance_card(&self, dark: bool) -> Element<'_, Message> {
        let seg = |label: String, is_dark: bool| {
            button(text(label).size(12))
                .padding([3, 12])
                .on_press(Message::ThemeChanged(is_dark))
                .style(iced::theme::Button::custom(theme::Segment {
                    dark,
                    active: dark == is_dark,
                    radius: theme::R_SM,
                }))
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

    fn language_card(&self, dark: bool) -> Element<'_, Message> {
        let selected = LOCALES.iter().copied().find(|l| l.code == self.locale);
        let select = pick_list(LOCALES, selected, |choice| {
            Message::Preferences(PreferencesMessage::LocaleChanged(choice.code.to_string()))
        })
        .padding([6, 12])
        .width(150)
        .text_size(13)
        .style(iced::theme::PickList::Custom(
            std::rc::Rc::new(theme::Select { dark }),
            std::rc::Rc::new(theme::Select { dark }),
        ));

        card_of_rows(
            dark,
            vec![setting_row(t!("prefs.language").to_string(), select.into())],
        )
    }

    fn updates_card(&self, dark: bool) -> Element<'_, Message> {
        let version = text(format!("v{}", env!("CARGO_PKG_VERSION")))
            .size(13)
            .font(theme::MONO);
        let check = button(text(t!("menu.help_check_updates").to_string()).size(12))
            .padding([6, 12])
            .on_press(Message::CheckForUpdate)
            .style(iced::theme::Button::custom(theme::Outline { dark }));

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
        .align_items(Alignment::Center)
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
                container(Space::with_height(1))
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
    let knob = container(Space::new(14, 14)).style(theme::switch_knob(dark, on));
    button(row![Space::with_width(if on { 17.0 } else { 0.0 }), knob])
        .padding(3)
        .width(36)
        .height(20)
        .on_press(message)
        .style(iced::theme::Button::custom(theme::SwitchTrack { dark, on }))
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
                    .style(if is_capturing { p.accent } else { p.text }),
            )
            .padding([4, 10])
            .on_press(Message::StartCaptureShortcut(target))
            .style(iced::theme::Button::custom(theme::Segment {
                dark,
                active: is_capturing,
                radius: theme::R_SM,
            }));
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
        .style(p.muted),
    ]
    .spacing(10)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferences_from_default_config() {
        let config = Config::default();
        let prefs = PreferencesState::from_config(&config);
        assert_eq!(prefs.font_size, 14.0);
        assert_eq!(prefs.tab_width, 4);
        assert!(prefs.use_spaces);
        assert_eq!(prefs.locale, "en");
        assert!(prefs.highlight_current_line);
        assert!(!prefs.show_page_guide);
    }

    #[test]
    fn update_font_size() {
        let mut config = Config::default();
        let mut prefs = PreferencesState::from_config(&config);
        assert!(prefs.update(PreferencesMessage::FontSizeChanged(18.0), &mut config));
        assert_eq!(prefs.font_size, 18.0);
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
        assert_eq!(prefs.locale, "fr");
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
        assert_eq!(config.tab_width, 2);
        assert!(!config.use_spaces);
        assert!(!config.highlight_current_line);
        assert!(config.show_page_guide);
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
