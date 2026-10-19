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

use iced::{Element, Length};

use crate::{app::Message, config::Config, search::SearchState};

/// The buffer's text, exactly. `Content::text()` always ends in a newline but
/// drops a final empty line; this keeps every line and ends in one newline
/// (none for an empty buffer).
pub fn buffer_text(content: &crate::code_editor::Content) -> String {
    let mut text = content
        .lines()
        .enumerate()
        .fold(String::new(), |mut out, (i, line)| {
            if i > 0 {
                out.push('\n');
            }
            out.push_str(&line);
            out
        });
    if !text.is_empty() {
        text.push('\n');
    }
    text
}

/// The smallest run of whole lines that differs between two buffer texts.
#[derive(Debug, PartialEq, Eq)]
pub struct LineReplacement {
    /// Index of the first differing line.
    pub first_line: usize,
    /// The old lines, joined with `\n` (what must be selected).
    pub old_text: String,
    /// The lines that replace them, joined with `\n`.
    pub new_text: String,
}

/// Diffs two buffer texts by whole lines: common leading and trailing lines
/// are skipped. Both sides always keep at least one line, so a pure insertion
/// or deletion still has something to select and replace. `None` if equal.
pub fn line_replacement(old: &str, new: &str) -> Option<LineReplacement> {
    if old == new {
        return None;
    }
    let o: Vec<&str> = old.strip_suffix('\n').unwrap_or(old).split('\n').collect();
    let n: Vec<&str> = new.strip_suffix('\n').unwrap_or(new).split('\n').collect();
    let mut prefix = o.iter().zip(&n).take_while(|(a, b)| a == b).count();
    let max_suffix = o.len().min(n.len()) - prefix;
    let mut suffix = o
        .iter()
        .rev()
        .zip(n.iter().rev())
        .take(max_suffix)
        .take_while(|(a, b)| a == b)
        .count();
    while o.len() - prefix - suffix == 0 || n.len() - prefix - suffix == 0 {
        if prefix > 0 {
            prefix -= 1;
        } else if suffix > 0 {
            suffix -= 1;
        } else {
            break;
        }
    }
    Some(LineReplacement {
        first_line: prefix,
        old_text: o[prefix..o.len() - suffix].join("\n"),
        new_text: n[prefix..n.len() - suffix].join("\n"),
    })
}

/// Makes the buffer read `new_text`. Only the lines that differ are replaced,
/// through ordinary editor actions, so the scroll position survives; the whole
/// buffer is rebuilt only for a huge change.
///
/// Must only be called once the editor has laid out its current content:
/// cosmic-text panics ("layout not found") when a motion crosses a line that
/// was edited since the last layout. The app guarantees that by deferring
/// such operations until after a redraw (see `layout_stale` in `app.rs`).
pub fn set_buffer_text(content: &mut crate::code_editor::Content, new_text: &str) {
    use iced::widget::text_editor::{Action, Edit, Motion};

    let old = buffer_text(content);
    let Some(change) = line_replacement(&old, new_text) else {
        return;
    };
    let len = change.old_text.chars().count();
    if len <= 20_000 {
        crate::search::goto_line_start(content, change.first_line);
        for _ in 0..len {
            content.perform(Action::Select(Motion::Right));
        }
        let last_line = change.first_line + change.old_text.matches('\n').count();
        // Belt and braces: never paste unless exactly the old lines are selected.
        let positioned = content.cursor_position().0 == last_line
            && content.selection().unwrap_or_default() == change.old_text;
        if positioned {
            content.perform(Action::Edit(Edit::Paste(std::sync::Arc::new(
                change.new_text,
            ))));
            return;
        }
    }
    *content = crate::code_editor::Content::with_text(new_text);
}

pub struct EditorState {
    pub content: crate::code_editor::Content,
    pub language: Option<String>,
}

impl EditorState {
    pub fn new() -> Self {
        Self {
            content: crate::code_editor::Content::new(),
            language: None,
        }
    }

    pub fn from_content(text: &str) -> Self {
        Self {
            content: crate::code_editor::Content::with_text(text),
            language: None,
        }
    }

    /// Like `set_language_by_extension`, but keeps the current language when
    /// the extension isn't recognized (e.g. a manual override survives a save).
    pub fn adopt_extension_language(&mut self, ext: &str) {
        if let Some(token) = extension_to_token(ext) {
            self.language = Some(token.to_string());
        }
    }

    /// Restores a language remembered for a buffer, falling back to the file
    /// extension: sessions and the file cache predate the language override and
    /// may carry `None` for a file that was never the active editor.
    pub fn restore_language(&mut self, cached: Option<String>, path: &std::path::Path) {
        self.language = cached.or_else(|| {
            path.extension()
                .and_then(|e| e.to_str())
                .and_then(extension_to_token)
                .map(str::to_string)
        });
    }

    pub fn view<'a>(
        &'a self,
        config: &Config,
        search: Option<&SearchState>,
        bookmarks: &[usize],
    ) -> Element<'a, Message> {
        let dark = config.dark_mode;
        let p = crate::theme::palette(dark);
        let extension = self.language.clone().unwrap_or_default();

        let mut editor = crate::code_editor::CodeEditor::new(&self.content)
            .size(config.font_size.clamp(8.0, 32.0))
            .font(crate::theme::MONO)
            .line_height(iced::advanced::text::LineHeight::Relative(LINE_HEIGHT))
            .on_action(Message::EditorAction)
            .highlight::<highlighter::Highlighter>(
                highlighter::Settings { dark, extension },
                highlighter::to_format,
            )
            .style(iced::theme::TextEditor::Custom(Box::new(
                crate::theme::EditorStyle { dark },
            )))
            .height(Length::Fill);

        if let Some((query, case_sensitive, use_regex)) =
            search.and_then(SearchState::highlight_query)
        {
            editor = editor.search(crate::code_editor::SearchPaint {
                query,
                case_sensitive,
                use_regex,
                current: search.and_then(SearchState::current_match_line),
                color: p.search_match,
                current_color: iced::Color {
                    a: (p.search_match.a * 1.7).min(0.85),
                    ..p.search_match
                },
                current_border: p.accent,
            });
        }
        if config.highlight_current_line {
            editor = editor.line_highlight(crate::code_editor::LineHighlight {
                background: p.current_line,
                bar: p.accent,
            });
        }
        if config.show_page_guide {
            editor = editor.page_guide(config.page_guide_column, p.border);
        }

        if !bookmarks.is_empty() {
            editor = editor.bookmarks(crate::code_editor::BookmarkPaint {
                lines: bookmarks.to_vec(),
                color: crate::theme::syntax(dark).function,
            });
        }
        if config.show_minimap {
            let with_alpha = |color: iced::Color, a: f32| iced::Color { a, ..color };
            editor = editor.minimap(crate::code_editor::Minimap {
                bar: with_alpha(p.muted, 0.45),
                viewport: with_alpha(p.accent, 0.16),
                viewport_border: with_alpha(p.accent, 0.75),
                marker: p.accent,
            });
        }

        // The gutter is drawn by the editor itself, from its own row layout,
        // so numbers stay aligned with wrapped lines and while scrolling.
        if config.show_line_numbers {
            editor = editor.gutter(crate::code_editor::Gutter {
                text: p.gutter,
                current: p.text,
            });
        }
        editor.padding(padding(config)).into()
    }
}

/// The editor's own padding (the gutter and the minimap come on top).
fn padding(config: &Config) -> iced::Padding {
    iced::Padding {
        top: 10.0,
        right: 14.0,
        bottom: 8.0,
        // The line numbers give the text its left margin; without them it
        // needs one of its own.
        left: if config.show_line_numbers { 0.0 } else { 16.0 },
    }
}

/// Where the text starts inside the editor widget (top-left corner of the
/// first character), to turn a position over the widget into a click on
/// the text.
pub fn text_origin(config: &Config, lines: usize) -> iced::Vector {
    let size = config.font_size.clamp(8.0, 32.0);
    crate::code_editor::text_origin(size, lines, config.show_line_numbers, padding(config))
}

/// Row height relative to the text size (the design: 22px for 12.5px code).
const LINE_HEIGHT: f32 = 1.7;

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
        widget::{column, container, row, text, Space},
        Alignment, Element, Length,
    };

    use crate::app::Message;
    use crate::theme;

    /// What the status bar shows.
    pub struct Info<'a> {
        pub status: &'a str,
        pub is_error: bool,
        pub dark: bool,
        /// Cursor as (line, byte column).
        pub cursor: (usize, usize),
        pub language: Option<&'a str>,
        pub selection: Option<String>,
        pub file_size: Option<u64>,
        pub use_spaces: bool,
        pub tab_width: usize,
    }

    fn size_label(b: u64) -> String {
        if b < 1024 {
            format!("{}B", b)
        } else if b < 1024 * 1024 {
            format!("{}KB", b / 1024)
        } else {
            format!("{}MB", b / (1024 * 1024))
        }
    }

    /// A hairline, then a 30px row: state dot and message on the left,
    /// position, indentation, encoding and language on the right.
    pub fn view<'a>(info: Info<'a>) -> Element<'a, Message> {
        let dark = info.dark;
        let p = theme::palette(dark);
        let muted = theme::muted_text(dark);

        let (line, col) = info.cursor;

        // Resolve language token → display name
        let lang_label = info
            .language
            .and_then(|tok| {
                crate::editor::SUPPORTED_LANGUAGES
                    .iter()
                    .find(|(t, _)| *t == tok)
                    .map(|(_, name)| name.to_string())
            })
            .unwrap_or_else(|| t!("status.plain_text").to_string());

        let indent = if info.use_spaces {
            t!("status.spaces", n = info.tab_width).to_string()
        } else {
            t!("status.tabs", n = info.tab_width).to_string()
        };

        let dot_color = if info.is_error {
            theme::error_color(dark)
        } else {
            p.accent
        };

        let mut right = row![].spacing(18).align_items(Alignment::Center);
        if let Some(sel) = info.selection {
            right = right.push(text(sel).size(11.5).style(muted));
        }
        if let Some(bytes) = info.file_size {
            right = right.push(text(size_label(bytes)).size(11.5).style(muted));
        }
        right = right
            .push(
                text(t!("status.cursor", line = line + 1, col = col + 1).to_string())
                    .size(11.5)
                    .style(muted),
            )
            .push(text(indent).size(11.5).style(muted))
            .push(text("UTF-8").size(11.5).style(muted))
            .push(text(lang_label).size(11.5).style(p.text));

        let bar = row![
            container(Space::new(6, 6)).style(theme::dot(dot_color)),
            text(info.status)
                .size(11.5)
                .style(if info.is_error { dot_color } else { muted }),
            Space::with_width(Length::Fill),
            right,
        ]
        .spacing(6)
        .padding([0, 14])
        .height(30)
        .align_items(Alignment::Center);

        column![
            container(Space::with_height(1))
                .width(Length::Fill)
                .style(theme::rule(dark)),
            bar
        ]
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
    fn line_count_matches_visible_lines() {
        // The gutter numbers rely on the buffer's own line count, which
        // includes the empty line after a trailing newline.
        assert_eq!(EditorState::from_content("a").content.line_count(), 1);
        assert_eq!(EditorState::from_content("a\nb").content.line_count(), 2);
        // A single trailing newline is absorbed, a second one is a real line.
        assert_eq!(EditorState::from_content("a\n").content.line_count(), 1);
        assert_eq!(EditorState::from_content("a\n\n").content.line_count(), 2);
        assert_eq!(EditorState::new().content.line_count(), 1);
    }

    #[test]
    fn line_replacement_isolates_the_changed_lines() {
        let r = line_replacement("a\nb\nc\n", "a\nB\nc\n").unwrap();
        assert_eq!(
            (r.first_line, r.old_text.as_str(), r.new_text.as_str()),
            (1, "b", "B")
        );
        assert_eq!(line_replacement("a\nb\n", "a\nb\n"), None);
    }

    #[test]
    fn line_replacement_handles_pure_insertions_and_deletions() {
        // Duplicate a line: the replaced run keeps one old line to select.
        let r = line_replacement("a\nb\n", "a\na\nb\n").unwrap();
        assert_eq!(
            (r.first_line, r.old_text.as_str(), r.new_text.as_str()),
            (0, "a", "a\na")
        );
        // Delete the last line.
        let r = line_replacement("a\nb\n", "a\n").unwrap();
        assert_eq!(
            (r.first_line, r.old_text.as_str(), r.new_text.as_str()),
            (0, "a\nb", "a")
        );
        // Delete everything.
        let r = line_replacement("a\n", "").unwrap();
        assert_eq!((r.old_text.as_str(), r.new_text.as_str()), ("a", ""));
    }

    #[test]
    fn buffer_text_keeps_trailing_blank_lines() {
        // `Content::text()` drops the last line when it is empty, which would
        // silently shorten a file ending in a blank line on every save.
        for text in ["a\n", "a\n\n", "a\nb\n", "a\n\n\n"] {
            let c = EditorState::from_content(text).content;
            assert_eq!(buffer_text(&c), text);
        }
        assert_eq!(buffer_text(&EditorState::new().content), "");
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
        state.adopt_extension_language("rs");
        assert_eq!(state.language, Some("rs".to_string()));
    }

    #[test]
    fn restore_language_prefers_the_cached_value() {
        let mut state = EditorState::new();
        state.restore_language(Some("md".to_string()), std::path::Path::new("/a/b.json"));
        assert_eq!(state.language, Some("md".to_string()));
    }

    #[test]
    fn restore_language_falls_back_to_the_extension() {
        let mut state = EditorState::new();
        state.restore_language(None, std::path::Path::new("/a/b.json"));
        assert_eq!(state.language, Some("json".to_string()));

        state.restore_language(None, std::path::Path::new("/a/b.unknown"));
        assert!(state.language.is_none());
    }

    #[test]
    fn unknown_extension_keeps_a_manual_language_override() {
        let mut state = EditorState::new();
        state.language = Some("py".to_string());
        state.adopt_extension_language("unknown_ext");
        assert_eq!(state.language, Some("py".to_string()));
    }
}
