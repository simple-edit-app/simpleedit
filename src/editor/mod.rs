pub mod highlighter;

/// All (token, display_name) pairs recognized by the syntax highlighter.
pub const SUPPORTED_LANGUAGES: &[(&str, &str)] = &[
    ("rs", "Rust"),
    ("py", "Python"),
    ("js", "JavaScript"),
    ("ts", "TypeScript"),
    ("jsx", "JSX"),
    ("tsx", "TSX"),
    ("html", "HTML"),
    ("css", "CSS"),
    ("json", "JSON"),
    ("toml", "TOML"),
    ("yaml", "YAML"),
    ("md", "Markdown"),
    ("sh", "Shell"),
    ("c", "C"),
    ("cpp", "C++"),
    ("java", "Java"),
    ("go", "Go"),
    ("rb", "Ruby"),
    ("php", "PHP"),
    ("swift", "Swift"),
    ("kt", "Kotlin"),
    ("xml", "XML"),
    ("sql", "SQL"),
    ("lua", "Lua"),
    ("r", "R"),
];

use iced::highlighter as hl;
use iced::{
    widget::{container, row, text, text_editor},
    Element, Font, Length,
};

use crate::{app::Message, config::Config};

pub struct EditorState {
    pub content: text_editor::Content,
    pub language: Option<String>,
}

impl EditorState {
    pub fn new() -> Self {
        Self {
            content: text_editor::Content::new(),
            language: None,
        }
    }

    pub fn from_content(text: &str) -> Self {
        Self {
            content: text_editor::Content::with_text(text),
            language: None,
        }
    }

    pub fn set_language_by_extension(&mut self, ext: &str) {
        self.language = extension_to_token(ext).map(|s| s.to_string());
    }

    pub fn view(&self, config: &Config) -> Element<'_, Message> {
        let hl_theme = if config.dark_mode {
            hl::Theme::SolarizedDark
        } else {
            hl::Theme::InspiredGitHub
        };
        let extension = self.language.clone().unwrap_or_default();

        let editor = text_editor(&self.content)
            .on_action(Message::EditorAction)
            .highlight::<hl::Highlighter>(
                hl::Settings {
                    theme: hl_theme,
                    extension,
                },
                |highlight, _theme| highlight.to_format(),
            )
            .style(iced::theme::TextEditor::Custom(Box::new(
                crate::theme::EditorStyle {
                    dark: config.dark_mode,
                },
            )))
            .height(Length::Fill);

        if config.show_line_numbers {
            let raw = self.content.text();
            let base = raw.lines().count().max(1);
            // text_editor shows an implicit empty line after a trailing '\n'
            let line_count = if raw.ends_with('\n') { base + 1 } else { base };

            let numbers: String = (1..=line_count)
                .map(|n| format!("{:>4}", n))
                .collect::<Vec<_>>()
                .join("\n");

            // text_editor internal top padding = 5px, container top = 8px → total = 13px.
            // Use line_height 1.3 to match cosmic-text default used by text_editor.
            let gutter = container(
                text(numbers)
                    .size(16.0)
                    .line_height(1.3_f32)
                    .font(Font::MONOSPACE)
                    .style(crate::theme::muted_text(config.dark_mode)),
            )
            .padding(iced::Padding {
                top: 13.0,
                right: 6.0,
                bottom: 8.0,
                left: 8.0,
            })
            .height(Length::Fill)
            .style(crate::theme::gutter(config.dark_mode));

            row![
                gutter,
                container(editor).width(Length::Fill).padding([8, 14]),
            ]
            .height(Length::Fill)
            .into()
        } else {
            container(editor)
                .width(Length::Fill)
                .height(Length::Fill)
                .padding([8, 14])
                .into()
        }
    }
}

fn extension_to_token(ext: &str) -> Option<&'static str> {
    match ext {
        "rs" => Some("rs"),
        "py" => Some("py"),
        "js" | "jsx" => Some("js"),
        "ts" | "tsx" => Some("ts"),
        "html" | "htm" => Some("html"),
        "css" => Some("css"),
        "json" => Some("json"),
        "toml" => Some("toml"),
        "yaml" | "yml" => Some("yaml"),
        "md" => Some("md"),
        "sh" | "bash" => Some("sh"),
        "c" | "h" => Some("c"),
        "cpp" | "cc" | "cxx" | "hpp" => Some("cpp"),
        "java" => Some("java"),
        "go" => Some("go"),
        "rb" => Some("rb"),
        "php" => Some("php"),
        "swift" => Some("swift"),
        "kt" => Some("kt"),
        "xml" => Some("xml"),
        "sql" => Some("sql"),
        "lua" => Some("lua"),
        "r" => Some("r"),
        _ => None,
    }
}

pub mod statusbar {
    use iced::{
        widget::{container, row, text, Space},
        Element, Length,
    };

    use crate::app::Message;
    use crate::theme;
    use std::path::PathBuf;

    #[allow(clippy::too_many_arguments)]
    pub fn view<'a>(
        status: &'a str,
        current_file: &'a Option<PathBuf>,
        is_dirty: bool,
        dark: bool,
        cursor: (usize, usize),
        is_error: bool,
        language: Option<&'a str>,
        selection_info: Option<String>,
        file_size: Option<u64>,
    ) -> Element<'a, Message> {
        let file_info = current_file
            .as_ref()
            .and_then(|p| p.to_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| t!("status.no_file").to_string());

        let dirty_indicator = if is_dirty { " •" } else { "" };
        let muted = theme::muted_text(dark);

        let (line, col) = cursor;
        let cursor_text = format!("Ln {}, Col {}", line + 1, col + 1);

        let status_color = if is_error {
            iced::Color::from_rgb(0.88, 0.27, 0.18)
        } else {
            muted
        };

        // Resolve language token → display name
        let lang_label = language
            .and_then(|tok| {
                crate::editor::SUPPORTED_LANGUAGES
                    .iter()
                    .find(|(t, _)| *t == tok)
                    .map(|(_, name)| *name)
            })
            .unwrap_or("");

        let sel_text = selection_info
            .map(|s| format!("{}  ", s))
            .unwrap_or_default();

        let size_text = file_size
            .map(|b| {
                if b < 1024 {
                    format!("  {}B", b)
                } else if b < 1024 * 1024 {
                    format!("  {}KB", b / 1024)
                } else {
                    format!("  {}MB", b / (1024 * 1024))
                }
            })
            .unwrap_or_default();

        let bar = row![
            text(status).size(11).style(status_color),
            Space::with_width(Length::Fill),
            text(sel_text).size(11).style(muted),
            text(lang_label).size(11).style(muted),
            text(format!("  {}", cursor_text)).size(11).style(muted),
            text(size_text).size(11).style(muted),
            text(format!("  {}{}", file_info, dirty_indicator))
                .size(11)
                .style(if is_dirty {
                    theme::accent_color()
                } else {
                    muted
                }),
        ]
        .padding([5, 10]);

        container(bar)
            .width(Length::Fill)
            .style(theme::bar(dark))
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_to_token_rust() {
        assert_eq!(extension_to_token("rs"), Some("rs"));
    }

    #[test]
    fn extension_to_token_unknown() {
        assert_eq!(extension_to_token("xyz"), None);
    }

    #[test]
    fn extension_to_token_all_common() {
        let exts = [
            "py", "js", "ts", "html", "css", "json", "md", "sh", "c", "cpp",
        ];
        for ext in exts {
            assert!(
                extension_to_token(ext).is_some(),
                "Missing token for .{}",
                ext
            );
        }
    }

    #[test]
    fn editor_state_new_is_empty() {
        let state = EditorState::new();
        // iced initializes Content::new() with a single newline
        assert!(state.content.text().trim().is_empty());
        assert!(state.language.is_none());
    }

    #[test]
    fn editor_state_from_content() {
        let state = EditorState::from_content("hello world");
        assert_eq!(state.content.text().trim(), "hello world");
    }

    #[test]
    fn set_language_by_extension_known() {
        let mut state = EditorState::new();
        state.set_language_by_extension("rs");
        assert_eq!(state.language, Some("rs".to_string()));
    }

    #[test]
    fn set_language_by_extension_unknown() {
        let mut state = EditorState::new();
        state.set_language_by_extension("unknown_ext");
        assert!(state.language.is_none());
    }
}
