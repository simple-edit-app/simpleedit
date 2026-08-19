use iced::{
    widget::{button, checkbox, column, container, row, scrollable, slider, text, Space},
    Element, Length,
};

use crate::{app::Message, config::Config, theme};

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum PreferencesMessage {
    FontSizeChanged(f32),
    TabWidthChanged(usize),
    UseSpacesToggled(bool),
    ShowLineNumbersToggled(bool),
    WordWrapToggled(bool),
    AutoIndentToggled(bool),
    AutocompleteBracketsToggled(bool),
    AutocompleteQuotesToggled(bool),
    ShowPageGuideToggled(bool),
    PageGuideColumnChanged(usize),
    HighlightCurrentLineToggled(bool),
    LocaleChanged(String),
}

pub struct PreferencesState {
    pub font_size: f32,
    pub tab_width: usize,
    pub use_spaces: bool,
    pub show_line_numbers: bool,
    pub word_wrap: bool,
    pub auto_indent: bool,
    pub autocomplete_brackets: bool,
    pub autocomplete_quotes: bool,
    pub show_page_guide: bool,
    pub page_guide_column: usize,
    pub highlight_current_line: bool,
    pub locale: String,
}

impl PreferencesState {
    pub fn from_config(config: &Config) -> Self {
        Self {
            font_size: config.font_size,
            tab_width: config.tab_width,
            use_spaces: config.use_spaces,
            show_line_numbers: config.show_line_numbers,
            word_wrap: config.word_wrap,
            auto_indent: config.auto_indent,
            autocomplete_brackets: config.autocomplete_brackets,
            autocomplete_quotes: config.autocomplete_quotes,
            show_page_guide: config.show_page_guide,
            page_guide_column: config.page_guide_column,
            highlight_current_line: config.highlight_current_line,
            locale: config.locale.clone(),
        }
    }

    pub fn update(&mut self, msg: PreferencesMessage, config: &mut Config) {
        match msg {
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
            PreferencesMessage::WordWrapToggled(v) => {
                self.word_wrap = v;
                config.word_wrap = v;
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
            PreferencesMessage::ShowPageGuideToggled(v) => {
                self.show_page_guide = v;
                config.show_page_guide = v;
            }
            PreferencesMessage::PageGuideColumnChanged(v) => {
                self.page_guide_column = v;
                config.page_guide_column = v;
            }
            PreferencesMessage::HighlightCurrentLineToggled(v) => {
                self.highlight_current_line = v;
                config.highlight_current_line = v;
            }
            PreferencesMessage::LocaleChanged(v) => {
                self.locale = v.clone();
                config.locale = v.clone();
                rust_i18n::set_locale(&v);
            }
        }
    }

    pub fn view(&self, dark: bool) -> Element<'_, Message> {
        let muted = theme::muted_text(dark);
        let section = |label: String| text(label).size(11).style(muted);

        // Segmented control: the language names stay in their own language.
        let lang_button = |code: &'static str, label: &'static str| {
            button(text(label).size(13))
                .padding([4, 12])
                .on_press(Message::Preferences(PreferencesMessage::LocaleChanged(
                    code.to_string(),
                )))
                .style(iced::theme::Button::custom(theme::GhostButton {
                    dark,
                    active: self.locale == code,
                }))
        };

        let close = button(text("✕").size(12))
            .padding([4, 8])
            .on_press(Message::TogglePreferences)
            .style(iced::theme::Button::custom(theme::GhostButton {
                dark,
                active: false,
            }));

        let header = row![
            text(t!("prefs.editor").to_string()).size(14),
            Space::with_width(Length::Fill),
            close,
        ];

        container(scrollable(
            column![
                header,
                section(t!("prefs.editor").to_string()),
                row![
                    text(t!("prefs.font_size")).size(12),
                    slider(8.0..=32.0, self.font_size, |v| {
                        Message::Preferences(PreferencesMessage::FontSizeChanged(v))
                    }),
                    text(format!("{:.0}px", self.font_size)).size(12),
                ]
                .spacing(8),
                checkbox(t!("prefs.show_line_numbers"), self.show_line_numbers)
                    .text_size(13)
                    .on_toggle(|v| Message::Preferences(
                        PreferencesMessage::ShowLineNumbersToggled(v)
                    )),
                checkbox(t!("prefs.auto_indent"), self.auto_indent)
                    .text_size(13)
                    .on_toggle(|v| Message::Preferences(PreferencesMessage::AutoIndentToggled(v))),
                checkbox(
                    t!("prefs.autocomplete_brackets"),
                    self.autocomplete_brackets
                )
                .text_size(13)
                .on_toggle(|v| Message::Preferences(
                    PreferencesMessage::AutocompleteBracketsToggled(v)
                )),
                checkbox(t!("prefs.autocomplete_quotes"), self.autocomplete_quotes)
                    .text_size(13)
                    .on_toggle(|v| Message::Preferences(
                        PreferencesMessage::AutocompleteQuotesToggled(v)
                    )),
                section(t!("prefs.language").to_string()),
                row![lang_button("en", "English"), lang_button("fr", "Français")].spacing(6),
            ]
            .spacing(14)
            .padding(18)
            .width(Length::Fill),
        ))
        .width(300)
        .height(Length::Fill)
        .style(theme::panel(dark))
        .into()
    }
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
    }

    #[test]
    fn update_font_size() {
        let mut config = Config::default();
        let mut prefs = PreferencesState::from_config(&config);
        prefs.update(PreferencesMessage::FontSizeChanged(18.0), &mut config);
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
    fn update_word_wrap() {
        let mut config = Config::default();
        let mut prefs = PreferencesState::from_config(&config);
        prefs.update(PreferencesMessage::WordWrapToggled(false), &mut config);
        assert!(!prefs.word_wrap);
        assert!(!config.word_wrap);
    }
}
