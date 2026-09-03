use std::ops::Range;

use iced::{
    widget::{button, container, row, text, text_input, Space},
    Element, Length,
};
use regex::Regex;

use crate::app::Message;
use crate::theme;

#[derive(Debug, Clone)]
pub enum SearchMessage {
    QueryChanged(String),
    ReplaceChanged(String),
    /// Enter in the query field, or the "Find" button: (re)runs the search
    /// against the current buffer and jumps to the next match.
    Find,
    /// Shift+Enter, or the up-arrow button.
    FindPrevious,
    /// Replaces the current match only, then advances to the next one.
    Replace,
    ReplaceAll,
    // Not wired to any UI control yet, but supported end-to-end.
    #[allow(dead_code)]
    ToggleRegex,
    #[allow(dead_code)]
    ToggleCaseSensitive,
}

pub struct SearchState {
    pub query: String,
    pub replacement: String,
    pub use_regex: bool,
    pub case_sensitive: bool,
    pub match_count: usize,
    pub last_error: Option<String>,
    /// Byte ranges of every match found by the last `Find`/`FindPrevious`.
    matches: Vec<Range<usize>>,
    /// Index into `matches` of the one currently selected in the editor.
    current: Option<usize>,
    /// (line index, byte range within that line) of the current match —
    /// `text_editor` only draws its own selection highlight while focused,
    /// which the search field holds instead, so the editor's highlighter
    /// needs this to mark the current match some other way. Kept alongside
    /// `current` rather than recomputed from it, since that needs the
    /// buffer text this struct doesn't otherwise hold onto.
    current_line: Option<(usize, Range<usize>)>,
    /// Whether `query`/`case_sensitive`/`use_regex` reflect a search that
    /// actually ran (a `Find`/`FindPrevious`), as opposed to being mid-edit.
    /// The editor's highlighter only sees the query once this is true, so
    /// typing in the field doesn't force a full re-highlight of the buffer
    /// on every keystroke — only committing the search does.
    committed: bool,
}

impl SearchState {
    pub fn new() -> Self {
        Self {
            query: String::new(),
            replacement: String::new(),
            use_regex: false,
            case_sensitive: false,
            match_count: 0,
            last_error: None,
            matches: Vec::new(),
            current: None,
            current_line: None,
            committed: false,
        }
    }

    /// The current match's (line index, byte range within that line), for
    /// the editor's highlighter to style apart from the other matches.
    pub fn current_match_line(&self) -> Option<(usize, Range<usize>)> {
        self.current_line.clone()
    }

    /// The query the editor's highlighter should actually match against —
    /// `None` while the field is being edited but no search has run yet, so
    /// typing doesn't force a full re-highlight of the buffer on every
    /// keystroke.
    pub fn highlight_query(&self) -> Option<(String, bool, bool)> {
        self.committed
            .then(|| (self.query.clone(), self.case_sensitive, self.use_regex))
    }

    pub fn update(&mut self, msg: SearchMessage, content: &mut iced::widget::text_editor::Content) {
        match msg {
            SearchMessage::QueryChanged(q) => {
                self.query = q;
                self.last_error = None;
                self.matches.clear();
                self.current = None;
                self.current_line = None;
                self.committed = false;
            }
            SearchMessage::ReplaceChanged(r) => self.replacement = r,
            SearchMessage::ToggleRegex => {
                self.use_regex = !self.use_regex;
                self.matches.clear();
                self.current = None;
                self.current_line = None;
                self.committed = false;
            }
            SearchMessage::ToggleCaseSensitive => {
                self.case_sensitive = !self.case_sensitive;
                self.matches.clear();
                self.current = None;
                self.current_line = None;
                self.committed = false;
            }
            SearchMessage::ReplaceAll => {
                let text = content.text();
                match self.replace_all(&text) {
                    Ok(new_text) => {
                        *content = iced::widget::text_editor::Content::with_text(&new_text);
                    }
                    Err(e) => self.last_error = Some(e),
                }
                self.matches.clear();
                self.current = None;
                self.current_line = None;
                self.committed = false;
            }
            SearchMessage::Find => self.jump(content, true),
            SearchMessage::FindPrevious => self.jump(content, false),
            SearchMessage::Replace => self.replace_current(content),
        }
    }

    /// Replaces the current match — the one already selected in the editor
    /// by a prior `Find`/`FindPrevious` — with the replacement text, then
    /// jumps to whatever match now follows it.
    ///
    /// With no current match yet (the field was just opened, or the buffer
    /// changed since), this is the same as a first `Find`: it only jumps,
    /// same as pressing Enter — replacing nothing was ever selected would be
    /// a surprise, not a convenience.
    fn replace_current(&mut self, content: &mut iced::widget::text_editor::Content) {
        if self.current.is_none() || self.matches.is_empty() {
            self.jump(content, true);
            return;
        }

        // The current match is already selected in the editor (that's how
        // it got highlighted); pasting over a selection replaces it, same
        // as the context menu's own Cut/Paste.
        content.perform(iced::widget::text_editor::Action::Edit(
            iced::widget::text_editor::Edit::Paste(std::sync::Arc::new(self.replacement.clone())),
        ));

        // The buffer just changed, so the old byte ranges no longer apply —
        // re-find from the cursor, which now sits right after the
        // replacement.
        self.current = None;
        self.current_line = None;
        self.committed = false;
        self.jump(content, true);
    }

    /// (Re)finds every match, then selects the next one (`forward`) or the
    /// previous one, wrapping around either end. The very first jump after a
    /// fresh search starts from whichever match is nearest the cursor.
    fn jump(&mut self, content: &mut iced::widget::text_editor::Content, forward: bool) {
        let text = content.text();
        match find_matches(&text, &self.query, self.case_sensitive, self.use_regex) {
            Ok(matches) => {
                self.matches = matches;
                self.match_count = self.matches.len();
                self.last_error = None;
            }
            Err(e) => {
                self.matches.clear();
                self.match_count = 0;
                self.current = None;
                self.current_line = None;
                self.committed = false;
                self.last_error = Some(e);
                return;
            }
        }

        if self.matches.is_empty() {
            self.current = None;
            self.current_line = None;
            self.committed = false;
            return;
        }

        let next = match self.current {
            Some(i) => advance_index(i, self.matches.len(), forward),
            None => {
                let cursor = line_col_to_byte(&text, content.cursor_position());
                nearest_match_index(&self.matches, cursor, forward)
            }
        };

        self.current = Some(next);
        self.current_line = Some(byte_range_to_line_local(&text, &self.matches[next]));
        self.committed = true;
        select_range(content, &text, self.matches[next].clone());
    }

    pub fn replace_all(&self, text: &str) -> Result<String, String> {
        if self.query.is_empty() {
            return Ok(text.to_string());
        }
        if self.use_regex {
            let flags = if self.case_sensitive { "" } else { "(?i)" };
            let re = Regex::new(&format!("{}{}", flags, self.query)).map_err(|e| e.to_string())?;
            Ok(re.replace_all(text, self.replacement.as_str()).into_owned())
        } else if self.case_sensitive {
            Ok(text.replace(&self.query, &self.replacement))
        } else {
            // Case-insensitive literal replace
            let mut result = String::with_capacity(text.len());
            let lower_text = text.to_lowercase();
            let lower_query = self.query.to_lowercase();
            let mut last = 0;
            for (start, _) in lower_text.match_indices(lower_query.as_str()) {
                result.push_str(&text[last..start]);
                result.push_str(&self.replacement);
                last = start + self.query.len();
            }
            result.push_str(&text[last..]);
            Ok(result)
        }
    }

    pub fn view(&self, dark: bool) -> Element<'_, Message> {
        let count_label = if let Some(ref e) = self.last_error {
            e.clone()
        } else if !self.query.is_empty() {
            match self.current {
                Some(i) => format!("{} / {}", i + 1, self.match_count),
                None => format!("{} {}", self.match_count, t!("search.matches")),
            }
        } else {
            String::new()
        };
        let find_placeholder = t!("search.find_placeholder").to_string();
        let replace_placeholder = t!("search.replace_placeholder").to_string();
        let find_label = t!("search.find").to_string();
        let replace_label = t!("search.replace").to_string();
        let replace_all_label = t!("search.replace_all").to_string();

        let action = |label: String, message: Message| {
            button(text(label).size(12))
                .padding([4, 10])
                .on_press(message)
                .style(iced::theme::Button::custom(theme::GhostButton {
                    dark,
                    active: false,
                }))
        };
        let nav = |label: &str, message: Message| {
            button(text(label.to_string()).size(12))
                .padding([4, 8])
                .on_press(message)
                .style(iced::theme::Button::custom(theme::GhostButton {
                    dark,
                    active: false,
                }))
        };

        let bar = row![
            text_input(find_placeholder.as_str(), &self.query)
                .on_input(|v| Message::Search(SearchMessage::QueryChanged(v)))
                .on_submit(Message::Search(SearchMessage::Find))
                .size(13)
                .width(220),
            nav("↑", Message::Search(SearchMessage::FindPrevious)),
            nav("↓", Message::Search(SearchMessage::Find)),
            text_input(replace_placeholder.as_str(), &self.replacement)
                .on_input(|v| Message::Search(SearchMessage::ReplaceChanged(v)))
                .size(13)
                .width(220),
            action(find_label, Message::Search(SearchMessage::Find)),
            action(replace_label, Message::Search(SearchMessage::Replace)),
            action(
                replace_all_label,
                Message::Search(SearchMessage::ReplaceAll)
            ),
            text(count_label).size(12).style(theme::muted_text(dark)),
            Space::with_width(Length::Fill),
            action("✕".to_string(), Message::ToggleSearch),
        ]
        .spacing(8)
        .padding([6, 10]);

        container(bar)
            .width(Length::Fill)
            .style(theme::bar(dark))
            .into()
    }
}

/// The next match index, wrapping at either end of a non-empty `matches`.
fn advance_index(current: usize, len: usize, forward: bool) -> usize {
    if forward {
        (current + 1) % len
    } else {
        (current + len - 1) % len
    }
}

/// The index of whichever match in `matches` (sorted by position, non-empty)
/// sits closest to `cursor` in the search direction, wrapping to the far end
/// if the cursor is past the last match (searching forward) or before the
/// first (searching backward).
fn nearest_match_index(matches: &[Range<usize>], cursor: usize, forward: bool) -> usize {
    if forward {
        matches.iter().position(|m| m.start >= cursor).unwrap_or(0)
    } else {
        matches
            .iter()
            .rposition(|m| m.start <= cursor)
            .unwrap_or(matches.len() - 1)
    }
}

/// Every match of `query` in `text`, as byte ranges. `Err` only for an
/// invalid regex; an empty query yields no matches rather than an error.
fn find_matches(
    text: &str,
    query: &str,
    case_sensitive: bool,
    use_regex: bool,
) -> Result<Vec<Range<usize>>, String> {
    if query.is_empty() {
        return Ok(Vec::new());
    }
    if use_regex {
        let flags = if case_sensitive { "" } else { "(?i)" };
        let re = Regex::new(&format!("{flags}{query}")).map_err(|e| e.to_string())?;
        Ok(re.find_iter(text).map(|m| m.start()..m.end()).collect())
    } else if case_sensitive {
        Ok(text
            .match_indices(query)
            .map(|(i, m)| i..i + m.len())
            .collect())
    } else {
        let lower_text = text.to_lowercase();
        let lower_query = query.to_lowercase();
        Ok(lower_text
            .match_indices(lower_query.as_str())
            .map(|(i, _)| i..i + lower_query.len())
            .collect())
    }
}

/// Converts a `text_editor` cursor position (line, column — both in chars)
/// to a byte offset into `text`.
fn line_col_to_byte(text: &str, (line, col): (usize, usize)) -> usize {
    let mut offset = 0;
    for (i, l) in text.split('\n').enumerate() {
        if i == line {
            let take: usize = l.chars().take(col).map(char::len_utf8).sum();
            return offset + take;
        }
        offset += l.len() + 1;
    }
    offset
}

/// Converts a byte range into (line, start column, length) — all in chars,
/// what `text_editor`'s `Motion::Right` steps over.
fn byte_range_to_line_col(text: &str, range: &Range<usize>) -> (usize, usize, usize) {
    let mut offset = 0;
    for (line_idx, l) in text.split('\n').enumerate() {
        let line_end = offset + l.len();
        if range.start >= offset && range.start <= line_end {
            let col = l[..range.start - offset].chars().count();
            let len = text[range.start..range.end].chars().count();
            return (line_idx, col, len);
        }
        offset = line_end + 1;
    }
    (0, 0, 0)
}

/// Converts a byte range into (line index, byte range within that line) —
/// what the editor's highlighter matches its own per-line, byte-based
/// search results against.
fn byte_range_to_line_local(text: &str, range: &Range<usize>) -> (usize, Range<usize>) {
    let mut offset = 0;
    for (line_idx, l) in text.split('\n').enumerate() {
        let line_end = offset + l.len();
        if range.start >= offset && range.start <= line_end {
            return (line_idx, range.start - offset..range.end - offset);
        }
        offset = line_end + 1;
    }
    (0, 0..0)
}

/// Selects `range` in the editor. `text_editor` has no "select this byte
/// range" action, only relative motions — same approach as jumping to a
/// line in the Go to Line dialog.
fn select_range(content: &mut iced::widget::text_editor::Content, text: &str, range: Range<usize>) {
    use iced::widget::text_editor::{Action, Motion};

    let (line, col, len) = byte_range_to_line_col(text, &range);
    content.perform(Action::Move(Motion::DocumentStart));
    for _ in 0..line {
        content.perform(Action::Move(Motion::Down));
    }
    content.perform(Action::Move(Motion::Home));
    for _ in 0..col {
        content.perform(Action::Move(Motion::Right));
    }
    for _ in 0..len {
        content.perform(Action::Select(Motion::Right));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_search(
        query: &str,
        replacement: &str,
        use_regex: bool,
        case_sensitive: bool,
    ) -> SearchState {
        SearchState {
            query: query.to_string(),
            replacement: replacement.to_string(),
            use_regex,
            case_sensitive,
            match_count: 0,
            last_error: None,
            matches: Vec::new(),
            current: None,
            current_line: None,
            committed: false,
        }
    }

    #[test]
    fn count_matches_literal() {
        let s = make_search("hello", "", false, true);
        assert_eq!(
            find_matches("hello world hello", &s.query, s.case_sensitive, s.use_regex)
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn count_matches_case_insensitive() {
        let s = make_search("hello", "", false, false);
        assert_eq!(
            find_matches("Hello HELLO hello", &s.query, s.case_sensitive, s.use_regex)
                .unwrap()
                .len(),
            3
        );
    }

    #[test]
    fn count_matches_regex() {
        let s = make_search(r"\d+", "", true, false);
        assert_eq!(
            find_matches("abc 123 def 456", &s.query, s.case_sensitive, s.use_regex)
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn count_matches_empty_query() {
        let s = make_search("", "", false, false);
        assert_eq!(
            find_matches("anything", &s.query, s.case_sensitive, s.use_regex)
                .unwrap()
                .len(),
            0
        );
    }

    #[test]
    fn replace_all_literal() {
        let s = make_search("foo", "bar", false, true);
        assert_eq!(s.replace_all("foo baz foo").unwrap(), "bar baz bar");
    }

    #[test]
    fn replace_all_case_insensitive() {
        let s = make_search("hello", "hi", false, false);
        let result = s.replace_all("Hello HELLO hello").unwrap();
        assert_eq!(result, "hi hi hi");
    }

    #[test]
    fn replace_all_regex() {
        let s = make_search(r"\d+", "NUM", true, false);
        assert_eq!(s.replace_all("abc 123 def 456").unwrap(), "abc NUM def NUM");
    }

    #[test]
    fn replace_all_invalid_regex_returns_error() {
        let s = make_search(r"[invalid", "x", true, false);
        assert!(s.replace_all("text").is_err());
    }

    #[test]
    fn find_matches_reports_byte_ranges() {
        let m = find_matches("ab ab", "ab", true, false).unwrap();
        assert_eq!(m, vec![0..2, 3..5]);
    }

    #[test]
    fn find_matches_invalid_regex_is_an_error() {
        assert!(find_matches("text", "[invalid", false, true).is_err());
    }

    #[test]
    fn line_col_to_byte_handles_multibyte_lines() {
        // '\u{e9}' (é) is 2 bytes, precomposed — spelled out to not depend
        // on how this source file's own encoding normalizes an "é" literal.
        let text = format!("caf{}\nworld", '\u{e9}');
        let text = text.as_str();
        // 4 chars ("caf" + é) but 5 bytes; column 4 is right after it. Line 1
        // starts one byte further still, past the '\n'.
        assert_eq!(line_col_to_byte(text, (0, 4)), 5);
        assert_eq!(line_col_to_byte(text, (1, 0)), 6);
        assert_eq!(line_col_to_byte(text, (1, 2)), 8);
    }

    #[test]
    fn byte_range_to_line_col_round_trips_with_line_col_to_byte() {
        let text = format!("one two\nthree caf{} four", '\u{e9}');
        let text = text.as_str();
        let start = line_col_to_byte(text, (1, 6)); // start of "café"
        let len_bytes = "caf\u{e9}".len();
        let (line, col, len) = byte_range_to_line_col(text, &(start..start + len_bytes));
        assert_eq!((line, col, len), (1, 6, 4));
    }

    #[test]
    fn advance_index_wraps_at_both_ends() {
        assert_eq!(advance_index(0, 3, true), 1);
        assert_eq!(advance_index(2, 3, true), 0); // wraps forward
        assert_eq!(advance_index(0, 3, false), 2); // wraps backward
        assert_eq!(advance_index(1, 3, false), 0);
    }

    #[test]
    fn nearest_match_index_picks_the_closest_match_in_direction() {
        let matches = [2..3, 4..5, 8..9];
        assert_eq!(nearest_match_index(&matches, 0, true), 0);
        assert_eq!(nearest_match_index(&matches, 3, true), 1);
        assert_eq!(nearest_match_index(&matches, 9, true), 0); // past the end, wraps
        assert_eq!(nearest_match_index(&matches, 9, false), 2);
        assert_eq!(nearest_match_index(&matches, 0, false), 2); // before the start, wraps
    }

    #[test]
    fn jump_with_no_matches_clears_current() {
        let mut content = iced::widget::text_editor::Content::with_text("nothing here");
        let mut s = make_search("xyz", "", false, true);
        s.jump(&mut content, true);
        assert_eq!(s.current, None);
        assert_eq!(s.match_count, 0);
    }
}
