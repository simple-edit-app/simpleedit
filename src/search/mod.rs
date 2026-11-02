use std::ops::Range;

use iced::advanced::widget::{operation, Id as WidgetId, Operation};
use iced::{
    widget::{button, column, container, row, text, text_input, tooltip},
    Alignment, Element, Length, Task,
};
use regex::Regex;

use crate::app::Message;
use crate::theme;

pub fn find_field_id() -> iced::widget::Id {
    iced::widget::Id::new("search-find")
}

pub fn replace_field_id() -> iced::widget::Id {
    iced::widget::Id::new("search-replace")
}

/// Resolves to whether one of the search bar's text fields has keyboard
/// focus, so editor shortcuts (Ctrl+Z, Ctrl+A, Tab…) can leave it alone.
pub fn search_field_focused() -> impl Operation<bool> {
    field_focused(vec![find_field_id(), replace_field_id()])
}

/// Resolves to whether the find field has keyboard focus (Enter there goes
/// to the next match).
pub fn find_field_focused() -> impl Operation<bool> {
    field_focused(vec![find_field_id()])
}

fn field_focused(ids: Vec<WidgetId>) -> impl Operation<bool> {
    struct Focused {
        ids: Vec<WidgetId>,
        found: bool,
    }
    impl Operation<bool> for Focused {
        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<bool>)) {
            operate(self);
        }

        fn focusable(
            &mut self,
            id: Option<&WidgetId>,
            _bounds: iced::Rectangle,
            state: &mut dyn operation::Focusable,
        ) {
            if state.is_focused() && id.is_some_and(|id| self.ids.contains(id)) {
                self.found = true;
            }
        }

        fn finish(&self) -> operation::Outcome<bool> {
            operation::Outcome::Some(self.found)
        }
    }
    Focused { ids, found: false }
}

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
    ToggleRegex,
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
    /// The query as a regex when matching needs one (regex mode, or a
    /// case-insensitive literal), compiled once per search.
    compiled: Option<std::sync::Arc<Regex>>,
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
            compiled: None,
        }
    }

    /// Forgets all matches; called when the buffer is replaced by another file.
    pub fn reset(&mut self) {
        self.reset_matches();
        self.match_count = 0;
    }

    /// Forgets the matches (the text they pointed into changed).
    pub fn reset_matches(&mut self) {
        self.matches.clear();
        self.current = None;
        self.current_line = None;
        self.committed = false;
        self.compiled = None;
    }

    pub fn set_error(&mut self, error: String) {
        self.last_error = Some(one_line(&error));
        self.reset_matches();
    }

    /// The compiled query for the editor's match highlighting.
    pub fn compiled_regex(&self) -> Option<std::sync::Arc<Regex>> {
        self.compiled.clone()
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

    pub fn update(
        &mut self,
        msg: SearchMessage,
        content: &mut crate::code_editor::Content,
    ) -> Task<Message> {
        match msg {
            SearchMessage::QueryChanged(q) => {
                self.query = q;
                self.last_error = None;
                self.matches.clear();
                self.current = None;
                self.current_line = None;
                self.committed = false;
                Task::none()
            }
            SearchMessage::ReplaceChanged(r) => {
                self.replacement = r;
                Task::none()
            }
            SearchMessage::ToggleRegex => {
                self.use_regex = !self.use_regex;
                self.matches.clear();
                self.current = None;
                self.current_line = None;
                self.committed = false;
                Task::none()
            }
            SearchMessage::ToggleCaseSensitive => {
                self.case_sensitive = !self.case_sensitive;
                self.matches.clear();
                self.current = None;
                self.current_line = None;
                self.committed = false;
                Task::none()
            }
            // Rewrites the whole buffer, so the app does it (undo, bookmarks).
            SearchMessage::ReplaceAll => Task::none(),
            SearchMessage::Find => {
                self.jump(content, true);
                Task::none()
            }
            SearchMessage::FindPrevious => {
                self.jump(content, false);
                Task::none()
            }
            SearchMessage::Replace => self.replace_current(content),
        }
    }

    /// Whether the editor's selection is exactly the text of the current match.
    fn current_is_selected(&self, content: &crate::code_editor::Content, text: &str) -> bool {
        match (self.current, content.selection()) {
            (Some(i), Some(sel)) => {
                self.matches.get(i).and_then(|r| text.get(r.clone())) == Some(sel.as_str())
            }
            _ => false,
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
    fn replace_current(&mut self, content: &mut crate::code_editor::Content) -> Task<Message> {
        let text = crate::editor::buffer_text(content);
        if self.matches.is_empty() || !self.current_is_selected(content, &text) {
            self.current = None;
            self.jump(content, true);
            return Task::none();
        }

        // The current match is selected in the editor; pasting over a
        // selection replaces it, same as the context menu's own Cut/Paste.
        let replacement = self.expanded_replacement(&text);
        content.perform(iced::widget::text_editor::Action::Edit(
            iced::widget::text_editor::Edit::Paste(std::sync::Arc::new(replacement)),
        ));
        self.current = None;
        self.current_line = None;
        self.committed = false;

        // cosmic-text needs a layout pass to reshape the buffer before any
        // cursor/selection query is safe to make (the same reasoning as
        // MoveCursorLeft after bracket auto-close in app.rs — calling
        // content.cursor_position() synchronously right after this edit
        // panics: "layout not found"). So finding the next match happens on
        // the next update cycle instead of immediately here.
        Task::perform(async {}, |_| Message::Search(SearchMessage::Find))
    }

    /// The replacement for the current match: in regex mode `$1`/`${name}`
    /// are expanded from the match's capture groups, like Replace All does.
    fn expanded_replacement(&self, text: &str) -> String {
        if self.use_regex {
            if let (Some(i), Ok(re)) = (self.current, build_regex(&self.query, self.case_sensitive))
            {
                let range = &self.matches[i];
                if let Some(caps) = re.captures_at(text, range.start) {
                    if caps.get(0).map(|m| m.range()) == Some(range.clone()) {
                        let mut out = String::new();
                        caps.expand(&self.replacement, &mut out);
                        return out;
                    }
                }
            }
        }
        self.replacement.clone()
    }

    /// (Re)finds every match, then selects the next one (`forward`) or the
    /// previous one, wrapping around either end. The very first jump after a
    /// fresh search starts from whichever match is nearest the cursor.
    fn jump(&mut self, content: &mut crate::code_editor::Content, forward: bool) {
        let text = crate::editor::buffer_text(content);
        // `current` only counts while the editor still has that match
        // selected; a click elsewhere or an edit makes it stale.
        if !self.current_is_selected(content, &text) {
            self.current = None;
        }
        match find_matches(&text, &self.query, self.case_sensitive, self.use_regex) {
            Ok(matches) => {
                self.matches = matches;
                self.match_count = self.matches.len();
                self.last_error = None;
            }
            Err(e) => {
                self.reset_matches();
                self.match_count = 0;
                self.last_error = Some(one_line(&e));
                return;
            }
        }

        if self.matches.is_empty() {
            self.reset_matches();
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
        self.compiled = if self.use_regex {
            build_regex(&self.query, self.case_sensitive)
                .ok()
                .map(Into::into)
        } else if !self.case_sensitive {
            literal_ci_regex(&self.query).map(Into::into)
        } else {
            None
        };
        select_range(content, &text, self.matches[next].clone());
    }

    pub fn replace_all(&self, text: &str) -> Result<String, String> {
        if self.query.is_empty() {
            return Ok(text.to_string());
        }
        if self.use_regex {
            let re = build_regex(&self.query, self.case_sensitive).map_err(|e| e.to_string())?;
            Ok(re.replace_all(text, self.replacement.as_str()).into_owned())
        } else if self.case_sensitive {
            Ok(text.replace(&self.query, &self.replacement))
        } else {
            let mut result = String::with_capacity(text.len());
            let mut last = 0;
            for r in find_literal_ci(text, &self.query) {
                result.push_str(&text[last..r.start]);
                result.push_str(&self.replacement);
                last = r.end;
            }
            result.push_str(&text[last..]);
            Ok(result)
        }
    }

    /// The floating search card: find field with its match counter and the
    /// regex / case chips, then the replace field with its two actions.
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
        let p = theme::palette(dark);
        let find_placeholder = t!("search.find_placeholder").to_string();
        let replace_placeholder = t!("search.replace_placeholder").to_string();
        let replace_label = t!("search.replace").to_string();
        let replace_all_label = t!("search.replace_all").to_string();
        let case_sensitive_label = t!("search.case_sensitive").to_string();
        let regex_label = t!("search.regex").to_string();
        let prev_label = t!("search.previous").to_string();
        let next_label = t!("search.next").to_string();

        // A toggle chip with its full label as a hover tooltip.
        let chip = |label: &'static str, hint: String, active: bool, message: Message| {
            let btn = button(text(label).size(11.5).font(theme::MONO))
                .padding([5, 8])
                .on_press(message)
                .style((theme::Chip { dark, active }).style());
            tooltip(btn, text(hint).size(11), tooltip::Position::Bottom)
                .gap(4)
                .padding(6)
                .style(theme::tooltip(dark))
        };

        // The find field is always drawn focused (accent border): it is the
        // card's primary input. Its counter sits inside, on the right.
        let find_field = container(
            row![
                text_input(find_placeholder.as_str(), &self.query)
                    .id(find_field_id())
                    .on_input(|v| Message::Search(SearchMessage::QueryChanged(v)))
                    .size(13)
                    .padding(0)
                    .style((theme::BareField { dark }).style()),
                text(count_label).size(11.5).color(p.muted),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .padding([0, 10])
        .width(Length::Fill)
        .height(30)
        .align_y(iced::alignment::Vertical::Center)
        .style(theme::field(dark, true, theme::R_ROW));

        // Previous / next match (Shift+Enter and Enter do the same).
        let step = |arrow: &'static str, hint: String, message: SearchMessage| {
            let btn = button(
                text(arrow)
                    .size(12)
                    .shaping(iced::widget::text::Shaping::Advanced),
            )
            .padding([5, 8])
            .on_press(Message::Search(message))
            .style(
                (theme::Chip {
                    dark,
                    active: false,
                })
                .style(),
            );
            tooltip(btn, text(hint).size(11), tooltip::Position::Bottom)
                .gap(4)
                .padding(6)
                .style(theme::tooltip(dark))
        };

        let find_row = row![
            find_field,
            step("↑", prev_label, SearchMessage::FindPrevious),
            step("↓", next_label, SearchMessage::Find),
            chip(
                ".*",
                regex_label,
                self.use_regex,
                Message::Search(SearchMessage::ToggleRegex),
            ),
            chip(
                "Aa",
                case_sensitive_label,
                self.case_sensitive,
                Message::Search(SearchMessage::ToggleCaseSensitive),
            ),
        ]
        .spacing(6)
        .align_y(Alignment::Center);

        let replace_field = container(
            text_input(replace_placeholder.as_str(), &self.replacement)
                .id(replace_field_id())
                .on_input(|v| Message::Search(SearchMessage::ReplaceChanged(v)))
                .size(13)
                .padding(0)
                .style((theme::BareField { dark }).style()),
        )
        .padding([0, 10])
        .width(Length::Fill)
        .height(30)
        .align_y(iced::alignment::Vertical::Center)
        .style(theme::field(dark, false, theme::R_ROW));

        let replace_row = row![
            replace_field,
            button(text(replace_label).size(12))
                .padding([6, 10])
                .on_press(Message::Search(SearchMessage::Replace))
                .style((theme::Outline { dark }).style()),
            button(text(replace_all_label).size(12).font(theme::UI_SEMIBOLD))
                .padding([6, 10])
                .on_press(Message::Search(SearchMessage::ReplaceAll))
                .style((theme::Primary { dark }).style()),
        ]
        .spacing(6)
        .align_y(Alignment::Center);

        container(column![find_row, replace_row].spacing(8))
            .padding(10)
            .width(432)
            .style(theme::card(dark))
            .into()
    }
}

/// Compiles a user query as a regex: multi-line (`^`/`$` match per line, as
/// the editor's per-line highlighter does) and optionally case-insensitive.
pub fn build_regex(query: &str, case_sensitive: bool) -> Result<Regex, regex::Error> {
    let flags = if case_sensitive { "(?m)" } else { "(?mi)" };
    Regex::new(&format!("{flags}{query}"))
}

/// The first line of an error message, cut to fit the little counter of the
/// search card (the `regex` crate's own messages run over several lines).
fn one_line(message: &str) -> String {
    let line = message
        .lines()
        .rev()
        .find(|l| l.trim_start().starts_with("error:"))
        .or_else(|| message.lines().next())
        .unwrap_or("")
        .trim()
        .trim_start_matches("error:")
        .trim();
    let cut: String = line.chars().take(48).collect();
    if cut.len() < line.len() {
        format!("{cut}…")
    } else {
        cut
    }
}

/// `query` as a case-insensitive literal.
fn literal_ci_regex(query: &str) -> Option<Regex> {
    if query.is_empty() {
        return None;
    }
    regex::RegexBuilder::new(&regex::escape(query))
        .case_insensitive(true)
        .build()
        .ok()
}

/// Case-insensitive literal search returning byte ranges *into `text`
/// itself* (lowercasing the whole text first would shift offsets whenever a
/// character changes length, e.g. `İ`). Linear in the text.
pub fn find_literal_ci(text: &str, query: &str) -> Vec<Range<usize>> {
    literal_ci_regex(query)
        .map(|re| re.find_iter(text).map(|m| m.range()).collect())
        .unwrap_or_default()
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
        let re = build_regex(query, case_sensitive).map_err(|e| e.to_string())?;
        // Empty matches (`a*`, `^`) can't be selected or highlighted.
        Ok(re
            .find_iter(text)
            .filter(|m| !m.is_empty())
            .map(|m| m.start()..m.end())
            .collect())
    } else if case_sensitive {
        Ok(text
            .match_indices(query)
            .map(|(i, m)| i..i + m.len())
            .collect())
    } else {
        Ok(find_literal_ci(text, query))
    }
}

/// Converts a `text_editor` cursor position to a byte offset into `text`.
/// The editor's column is already a byte index into its line (cosmic-text's
/// `Cursor::index`), so it only needs clamping to the line.
fn line_col_to_byte(text: &str, (line, col): (usize, usize)) -> usize {
    let mut offset = 0;
    for (i, l) in text.split('\n').enumerate() {
        if i == line {
            return offset + col.min(l.len());
        }
        offset += l.len() + 1;
    }
    offset
}

/// Converts a byte range into (line, start byte column, length in bytes).
fn byte_range_to_line_col(text: &str, range: &Range<usize>) -> (usize, usize, usize) {
    let mut offset = 0;
    for (line_idx, l) in text.split('\n').enumerate() {
        let line_end = offset + l.len();
        if range.start >= offset && range.start <= line_end {
            return (line_idx, range.start - offset, range.end - range.start);
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

pub use crate::code_editor::goto_line_start;

/// Selects `range` (byte offsets into `text`) in the editor.
fn select_range(content: &mut crate::code_editor::Content, text: &str, range: Range<usize>) {
    let (line, col, _) = byte_range_to_line_col(text, &range);
    content.select_bytes(line, col, range.end - range.start);
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
            compiled: None,
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
        // The editor's column is a byte index: 5 is right after the é, and
        // line 1 starts one byte further still, past the '\n'.
        assert_eq!(line_col_to_byte(text, (0, 5)), 5);
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
        assert_eq!((line, col, len), (1, 6, len_bytes));
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
    fn case_insensitive_offsets_survive_length_changing_lowercase() {
        // 'İ' (2 bytes) lowercases to "i\u{307}" (3 bytes): offsets taken from
        // the lowercased text would be wrong for everything after it.
        let text = "İİ abc ABC";
        let m = find_literal_ci(text, "abc");
        assert_eq!(m.len(), 2);
        for r in &m {
            assert!(text[r.clone()].eq_ignore_ascii_case("abc"));
        }
        let s = make_search("abc", "x", false, false);
        assert_eq!(s.replace_all(text).unwrap(), "İİ x x");
    }

    #[test]
    fn regex_anchors_work_per_line_and_empty_matches_are_dropped() {
        let m = find_matches("foo\nfoo", "^foo", false, true).unwrap();
        assert_eq!(m, vec![0..3, 4..7]);
        assert!(find_matches("abc", "x*", false, true).unwrap().is_empty());
    }

    #[test]
    fn regex_replacement_expands_captures_for_the_current_match() {
        let mut s = make_search(r"(\w+)@", "$1 at ", true, false);
        s.matches = std::iter::once(0..4).collect();
        s.current = Some(0);
        assert_eq!(s.expanded_replacement("bob@x"), "bob at ");
    }

    #[test]
    fn jump_with_no_matches_clears_current() {
        let mut content = crate::code_editor::Content::with_text("nothing here");
        let mut s = make_search("xyz", "", false, true);
        s.jump(&mut content, true);
        assert_eq!(s.current, None);
        assert_eq!(s.match_count, 0);
    }

    #[test]
    fn the_match_is_selected_after_emoji_accents_and_right_to_left_text() {
        for text in [
            "\u{1F44D}\u{1F3FD} foo",
            "\u{1F1EB}\u{1F1F7} foo",
            "cafe\u{301} foo",
            "\u{5E9}\u{5DC}\u{5D5}\u{5DD} foo",
            "plain foo",
        ] {
            let mut content = crate::code_editor::Content::with_text(&format!("{text}\n"));
            let mut s = make_search("foo", "", false, true);
            s.jump(&mut content, true);
            assert_eq!(content.selection().as_deref(), Some("foo"), "in {text:?}");
        }
    }

    #[test]
    fn a_match_spanning_lines_is_selected_whole() {
        let mut content = crate::code_editor::Content::with_text("ab\ncd\nef\n");
        let mut s = make_search("b\ncd", "", false, true);
        s.jump(&mut content, true);
        assert_eq!(content.selection().as_deref(), Some("b\ncd"));
    }

    #[test]
    fn long_regex_errors_fit_the_counter() {
        let mut s = make_search("(", "", true, false);
        let mut content = crate::code_editor::Content::with_text("x\n");
        s.jump(&mut content, true);
        let shown = s.last_error.clone().unwrap();
        assert!(!shown.contains('\n'));
        assert!(shown.chars().count() <= 49);
    }
}
