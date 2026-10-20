pub mod highlighter;

/// All (token, display_name) pairs recognized by the syntax highlighter.
/// The token is what formatters, comment markers and previews key on; the
/// grammar used for coloring may be a close relative (see `highlighter`).
pub const SUPPORTED_LANGUAGES: &[(&str, &str)] = &[
    ("rs", "Rust"),
    ("py", "Python"),
    ("js", "JavaScript"),
    ("ts", "TypeScript"),
    ("jsx", "JSX"),
    ("tsx", "TSX"),
    ("html", "HTML"),
    ("css", "CSS"),
    ("scss", "SCSS"),
    ("less", "Less"),
    ("json", "JSON"),
    ("toml", "TOML"),
    ("yaml", "YAML"),
    ("md", "Markdown"),
    ("sh", "Shell"),
    ("make", "Makefile"),
    ("c", "C"),
    ("cpp", "C++"),
    ("cs", "C#"),
    ("java", "Java"),
    ("go", "Go"),
    ("rb", "Ruby"),
    ("php", "PHP"),
    ("pl", "Perl"),
    ("hs", "Haskell"),
    ("scala", "Scala"),
    ("swift", "Swift"),
    ("kt", "Kotlin"),
    ("xml", "XML"),
    ("sql", "SQL"),
    ("lua", "Lua"),
    ("r", "R"),
    ("tex", "LaTeX"),
    ("diff", "Diff"),
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
/// was edited since the last layout. `Content::settle` takes care of that.
pub fn set_buffer_text(content: &mut crate::code_editor::Content, new_text: &str) {
    let old = buffer_text(content);
    replace_text(content, &old, new_text);
}

/// `set_buffer_text` for a caller that already holds the current text
/// (`old`), saving a full copy of the buffer.
pub fn replace_text(content: &mut crate::code_editor::Content, old: &str, new_text: &str) {
    use iced::widget::text_editor::{Action, Edit};

    let Some(change) = line_replacement(old, new_text) else {
        return;
    };
    if change.old_text.len() <= 100_000 {
        content.select_bytes(change.first_line, 0, change.old_text.len());
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

    #[cfg(test)]
    pub fn from_content(text: &str) -> Self {
        Self {
            content: crate::code_editor::Content::with_text(text),
            language: None,
        }
    }

    /// Like `set_language_by_extension`, but keeps the current language when
    /// the extension isn't recognized (e.g. a manual override survives a save).
    pub fn adopt_path_language(&mut self, path: &std::path::Path, first_line: &str) {
        if let Some(token) = detect_language(path, first_line) {
            self.language = Some(token.to_string());
        }
    }

    /// Restores a language remembered for a buffer, falling back to the file
    /// extension: sessions and the file cache predate the language override and
    /// may carry `None` for a file that was never the active editor.
    pub fn restore_language(
        &mut self,
        cached: Option<String>,
        path: &std::path::Path,
        first_line: &str,
    ) {
        self.language = cached.or_else(|| detect_language(path, first_line).map(str::to_string));
    }

    pub fn view<'a>(
        &'a self,
        config: &Config,
        search: Option<&SearchState>,
        bookmarks: &[usize],
    ) -> Element<'a, Message> {
        let dark = config.dark_mode;
        self.content.set_wrapping(config.word_wrap);
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
            .style(crate::theme::editor_style(dark))
            .height(Length::Fill);

        if let Some((query, case_sensitive, use_regex)) =
            search.and_then(SearchState::highlight_query)
        {
            editor = editor.search(crate::code_editor::SearchPaint {
                query,
                case_sensitive,
                use_regex,
                regex: search.and_then(SearchState::compiled_regex),
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

        editor = editor.folding(
            crate::code_editor::FoldPaint {
                kind: crate::folding::Kind::of(self.language.as_deref()),
                chevron: p.muted,
                folded: p.accent,
                marker_background: p.accent_soft,
                tooltip: fold_tooltip(config),
                tooltip_text: p.text,
                tooltip_background: p.elevated,
                tooltip_border: p.border_strong,
            },
            Message::FoldClick,
        );

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

/// What a fold chevron does, and with which keys.
fn fold_tooltip(config: &Config) -> Vec<String> {
    let ctrl = if cfg!(target_os = "macos") {
        "⌘"
    } else {
        "Ctrl"
    };
    let shortcut = config.shortcuts.toggle_fold.display();
    vec![
        t!("fold.tip_click").to_string(),
        t!("fold.tip_siblings", key = ctrl).to_string(),
        t!("fold.tip_subtree").to_string(),
        t!("fold.tip_cursor", shortcut = shortcut).to_string(),
    ]
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
    match ext.to_ascii_lowercase().as_str() {
        "rs" => Some("rs"),
        "py" | "pyw" => Some("py"),
        "js" | "mjs" | "cjs" => Some("js"),
        "jsx" => Some("jsx"),
        "ts" | "mts" | "cts" => Some("ts"),
        "tsx" => Some("tsx"),
        "html" | "htm" | "xhtml" => Some("html"),
        "css" => Some("css"),
        "scss" => Some("scss"),
        "less" => Some("less"),
        "json" => Some("json"),
        "toml" => Some("toml"),
        "yaml" | "yml" => Some("yaml"),
        "md" | "markdown" => Some("md"),
        "sh" | "bash" | "zsh" => Some("sh"),
        "mk" | "mak" => Some("make"),
        "c" | "h" => Some("c"),
        "cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx" => Some("cpp"),
        "cs" => Some("cs"),
        "java" => Some("java"),
        "go" => Some("go"),
        "rb" => Some("rb"),
        "php" => Some("php"),
        "pl" | "pm" => Some("pl"),
        "hs" => Some("hs"),
        "scala" => Some("scala"),
        "swift" => Some("swift"),
        "kt" | "kts" => Some("kt"),
        "xml" | "svg" | "plist" => Some("xml"),
        "sql" => Some("sql"),
        "lua" => Some("lua"),
        "r" => Some("r"),
        "tex" => Some("tex"),
        "diff" | "patch" => Some("diff"),
        _ => None,
    }
}

/// Files recognized by their whole name.
fn filename_to_token(name: &str) -> Option<&'static str> {
    match name {
        "Makefile" | "makefile" | "GNUmakefile" => Some("make"),
        ".bashrc" | ".bash_profile" | ".bash_aliases" | ".profile" | ".zshrc" | ".zprofile"
        | ".zshenv" => Some("sh"),
        _ => None,
    }
}

/// The language named by a `#!` first line.
fn shebang_to_token(first_line: &str) -> Option<&'static str> {
    let rest = first_line.strip_prefix("#!")?;
    let mut words = rest.split_whitespace();
    let mut program = words.next()?.rsplit('/').next()?;
    if program == "env" {
        program = words.find(|w| !w.starts_with('-'))?;
    }
    let program = program.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.');
    match program {
        "sh" | "bash" | "zsh" | "dash" | "ash" => Some("sh"),
        "python" => Some("py"),
        "node" | "nodejs" | "deno" => Some("js"),
        "ruby" => Some("rb"),
        "perl" => Some("pl"),
        "php" => Some("php"),
        "lua" => Some("lua"),
        _ => None,
    }
}

/// The language of a file: by extension, then by name, then by shebang.
pub fn detect_language(path: &std::path::Path, first_line: &str) -> Option<&'static str> {
    path.extension()
        .and_then(|e| e.to_str())
        .and_then(extension_to_token)
        .or_else(|| {
            path.file_name()
                .and_then(|n| n.to_str())
                .and_then(filename_to_token)
        })
        .or_else(|| shebang_to_token(first_line))
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
        /// Cursor as (line, column in characters), both from 0.
        pub cursor: (usize, usize),
        pub line_ending: &'a str,
        pub language: Option<&'a str>,
        pub selection: Option<String>,
        pub file_size: Option<u64>,
        pub use_spaces: bool,
        pub tab_width: usize,
    }

    pub(super) fn size_label(b: u64) -> String {
        const KB: f64 = 1024.0;
        let value = b as f64;
        if b < 1024 {
            t!("status.size_b", n = b).to_string()
        } else if value < KB * KB {
            t!("status.size_kb", n = format!("{:.1}", value / KB)).to_string()
        } else {
            t!("status.size_mb", n = format!("{:.1}", value / (KB * KB))).to_string()
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

        let mut right = row![].spacing(18).align_y(Alignment::Center);
        if let Some(sel) = info.selection {
            right = right.push(text(sel).size(11.5).color(muted));
        }
        if let Some(bytes) = info.file_size {
            right = right.push(text(size_label(bytes)).size(11.5).color(muted));
        }
        right = right
            .push(
                text(t!("status.cursor", line = line + 1, col = col + 1).to_string())
                    .size(11.5)
                    .color(muted),
            )
            .push(text(indent).size(11.5).color(muted))
            .push(text(info.line_ending).size(11.5).color(muted))
            .push(text("UTF-8").size(11.5).color(muted))
            .push(text(lang_label).size(11.5).color(p.text));

        let bar = row![
            container(Space::new().width(6).height(6)).style(theme::dot(dot_color)),
            text(info.status)
                .size(11.5)
                .shaping(iced::widget::text::Shaping::Advanced)
                .color(if info.is_error { dot_color } else { muted }),
            Space::new().width(Length::Fill),
            right,
        ]
        .spacing(6)
        .padding([0, 14])
        .height(30)
        .align_y(Alignment::Center);

        column![
            container(Space::new().height(1))
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
    fn tsx_and_jsx_keep_their_own_token_and_common_extensions_are_known() {
        for (ext, token) in [
            ("tsx", "tsx"),
            ("jsx", "jsx"),
            ("mjs", "js"),
            ("markdown", "md"),
            ("scss", "scss"),
            ("kts", "kt"),
            ("HTM", "html"),
        ] {
            assert_eq!(extension_to_token(ext), Some(token), ".{ext}");
        }
    }

    #[test]
    fn languages_are_found_by_name_and_by_shebang() {
        let p = std::path::Path::new;
        assert_eq!(detect_language(p("/x/Makefile"), ""), Some("make"));
        assert_eq!(detect_language(p("/x/.bashrc"), ""), Some("sh"));
        assert_eq!(
            detect_language(p("/x/run"), "#!/usr/bin/env python3"),
            Some("py")
        );
        assert_eq!(detect_language(p("/x/run"), "#!/bin/bash"), Some("sh"));
        assert_eq!(detect_language(p("/x/run"), "plain"), None);
        // The extension wins over a shebang.
        assert_eq!(detect_language(p("/x/a.rb"), "#!/bin/sh"), Some("rb"));
    }

    #[test]
    fn every_listed_language_has_an_extension_or_a_name() {
        // The language picker offers these tokens; detection must be able to
        // produce each of them.
        for (token, _) in SUPPORTED_LANGUAGES {
            let produced = [
                "rs", "py", "js", "jsx", "ts", "tsx", "html", "css", "scss", "less", "json",
                "toml", "yaml", "md", "sh", "mk", "c", "cpp", "cs", "java", "go", "rb", "php",
                "pl", "hs", "scala", "swift", "kt", "xml", "sql", "lua", "r", "tex", "diff",
            ]
            .iter()
            .any(|ext| extension_to_token(ext) == Some(*token));
            assert!(produced, "no extension maps to {token}");
        }
    }

    #[test]
    fn positioning_the_cursor_in_a_big_file_is_instant() {
        let text = "let x = 1;\n".repeat(100_000);
        let mut content = crate::code_editor::Content::with_text(&text);
        let started = std::time::Instant::now();
        crate::code_editor::goto_line_start(&mut content, 50_000);
        assert_eq!(content.cursor_position(), (50_000, 0));
        content.select_bytes(99_990, 4, 8);
        assert_eq!(content.selection().as_deref(), Some("x = 1;\nl"));
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
    }

    #[test]
    fn selection_counts_match_the_selected_text() {
        let mut content = crate::code_editor::Content::with_text("héllo\nwörld\nend\n");
        content.select_bytes(0, 1, 11);
        let text = content.selection().unwrap();
        assert_eq!(
            content.selection_counts(),
            Some((text.chars().count(), text.lines().count()))
        );
        content.select_bytes(1, 0, 0);
        assert_eq!(content.selection_counts(), None);
    }

    #[test]
    fn wrapping_can_be_switched_without_touching_the_text() {
        let content = crate::code_editor::Content::with_text("a long line\n");
        content.set_wrapping(false);
        content.set_wrapping(false);
        content.set_wrapping(true);
        assert_eq!(buffer_text(&content), "a long line\n");
    }

    #[test]
    fn set_buffer_text_keeps_trailing_tabs() {
        // cosmic-text strips a trailing control character from pasted text.
        let mut content = crate::code_editor::Content::with_text("x\n");
        set_buffer_text(&mut content, "a\t\n");
        assert_eq!(buffer_text(&content), "a\t\n");
    }

    #[test]
    fn pasting_a_tab_after_text_inserts_it() {
        use iced::widget::text_editor::{Action, Edit, Motion};
        let mut content = crate::code_editor::Content::with_text("ab\n");
        content.perform(Action::Move(Motion::DocumentEnd));
        content.perform(Action::Edit(Edit::Paste(std::sync::Arc::new(
            "\t\t".into(),
        ))));
        assert_eq!(buffer_text(&content), "ab\t\t\n");
    }

    #[test]
    fn pasted_windows_line_breaks_are_normalized() {
        use iced::widget::text_editor::{Action, Edit};
        let mut content = crate::code_editor::Content::with_text("");
        content.perform(Action::Edit(Edit::Paste(std::sync::Arc::new(
            "x\r\ny".into(),
        ))));
        assert_eq!(buffer_text(&content), "x\ny\n");
    }

    #[test]
    fn size_labels_use_one_decimal() {
        // (The unit depends on the interface language other tests may set.)
        assert!(statusbar::size_label(1_992_294).starts_with("1.9"));
        assert!(statusbar::size_label(512).starts_with("512"));
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
        state.adopt_path_language(std::path::Path::new("a.rs"), "");
        assert_eq!(state.language, Some("rs".to_string()));
    }

    #[test]
    fn restore_language_prefers_the_cached_value() {
        let mut state = EditorState::new();
        state.restore_language(
            Some("md".to_string()),
            std::path::Path::new("/a/b.json"),
            "",
        );
        assert_eq!(state.language, Some("md".to_string()));
    }

    #[test]
    fn restore_language_falls_back_to_the_extension() {
        let mut state = EditorState::new();
        state.restore_language(None, std::path::Path::new("/a/b.json"), "");
        assert_eq!(state.language, Some("json".to_string()));

        state.restore_language(None, std::path::Path::new("/a/b.unknown"), "");
        assert!(state.language.is_none());
    }

    #[test]
    fn unknown_extension_keeps_a_manual_language_override() {
        let mut state = EditorState::new();
        state.language = Some("py".to_string());
        state.adopt_path_language(std::path::Path::new("a.unknown_ext"), "");
        assert_eq!(state.language, Some("py".to_string()));
    }
}
