use std::ops::Range;

use iced::advanced::widget::{operation, Id as WidgetId, Operation};
use iced::{
    widget::{button, container, row, text, text_input, tooltip, Space},
    Command, Element, Length,
};
use regex::Regex;

use crate::app::Message;
use crate::theme;

pub fn find_field_id() -> text_input::Id {
    text_input::Id::new("search-find")
}

pub fn replace_field_id() -> text_input::Id {
    text_input::Id::new("search-replace")
}

/// Resolves to whether one of the search bar's text fields has keyboard
/// focus, so editor shortcuts (Ctrl+Z, Ctrl+A…) can leave it alone.
pub fn search_field_focused() -> impl Operation<bool> {
    struct Focused {
        ids: [WidgetId; 2],
        found: bool,
    }
    impl Operation<bool> for Focused {
        fn container(
            &mut self,
            _id: Option<&WidgetId>,
            _bounds: iced::Rectangle,
            operate_on_children: &mut dyn FnMut(&mut dyn Operation<bool>),
        ) {
            operate_on_children(self);
        }

        fn focusable(&mut self, state: &mut dyn operation::Focusable, id: Option<&WidgetId>) {
            if state.is_focused() && id.is_some_and(|id| self.ids.contains(id)) {
                self.found = true;
            }
        }

        fn finish(&self) -> operation::Outcome<bool> {
            operation::Outcome::Some(self.found)
        }
    }
    Focused {
        ids: [find_field_id().into(), replace_field_id().into()],
        found: false,
    }
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

    /// Forgets all matches; called when the buffer is replaced by another file.
    pub fn reset(&mut self) {
        self.matches.clear();
        self.current = None;
        self.current_line = None;
        self.committed = false;
        self.match_count = 0;
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
        content: &mut iced::widget::text_editor::Content,
    ) -> Command<Message> {
        match msg {
            SearchMessage::QueryChanged(q) => {
                self.query = q;
                self.last_error = None;
                self.matches.clear();
                self.current = None;
                self.current_line = None;
                self.committed = false;
                Command::none()
            }
            SearchMessage::ReplaceChanged(r) => {
                self.replacement = r;
                Command::none()
            }
            SearchMessage::ToggleRegex => {
                self.use_regex = !self.use_regex;
                self.matches.clear();
                self.current = None;
                self.current_line = None;
                self.committed = false;
                Command::none()
            }
            SearchMessage::ToggleCaseSensitive => {
                self.case_sensitive = !self.case_sensitive;
                self.matches.clear();
                self.current = None;
                self.current_line = None;
                self.committed = false;
                Command::none()
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
                Command::none()
            }
            SearchMessage::Find => {
                self.jump(content, true);
                Command::none()
            }
            SearchMessage::FindPrevious => {
                self.jump(content, false);
                Command::none()
            }
            SearchMessage::Replace => self.replace_current(content),
        }
    }

    /// Whether the editor's selection is exactly the text of the current match.
    fn current_is_selected(
        &self,
        content: &iced::widget::text_editor::Content,
        text: &str,
    ) -> bool {
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
    fn replace_current(
        &mut self,
        content: &mut iced::widget::text_editor::Content,
    ) -> Command<Message> {
        let text = content.text();
        if self.matches.is_empty() || !self.current_is_selected(content, &text) {
            self.current = None;
            self.jump(content, true);
            return Command::none();
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
        Command::perform(async {}, |_| Message::Search(SearchMessage::Find))
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
    fn jump(&mut self, content: &mut iced::widget::text_editor::Content, forward: bool) {
        let text = content.text();
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
        let case_sensitive_label = t!("search.case_sensitive").to_string();
        let regex_label = t!("search.regex").to_string();

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
        // A toggle button with its full label as a hover tooltip — same
        // pattern as the preview pane's mode-switcher icons.
        let toggle = |label: &'static str, hint: String, active: bool, message: Message| {
            let btn = button(text(label).size(12))
                .padding([4, 8])
                .on_press(message)
                .style(iced::theme::Button::custom(theme::GhostButton {
                    dark,
                    active,
                }));
            tooltip(btn, text(hint).size(11), tooltip::Position::Bottom)
                .gap(4)
                .padding(6)
                .style(theme::card(dark))
        };

        let bar = row![
            text_input(find_placeholder.as_str(), &self.query)
                .id(find_field_id())
                .on_input(|v| Message::Search(SearchMessage::QueryChanged(v)))
                .on_submit(Message::Search(SearchMessage::Find))
                .size(13)
                .width(220),
            toggle(
                "Aa",
                case_sensitive_label,
                self.case_sensitive,
                Message::Search(SearchMessage::ToggleCaseSensitive),
            ),
            toggle(
                ".*",
                regex_label,
                self.use_regex,
                Message::Search(SearchMessage::ToggleRegex),
            ),
            nav("↑", Message::Search(SearchMessage::FindPrevious)),
            nav("↓", Message::Search(SearchMessage::Find)),
            text_input(replace_placeholder.as_str(), &self.replacement)
                .id(replace_field_id())
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

/// Compiles a user query as a regex: multi-line (`^`/`$` match per line, as
/// the editor's per-line highlighter does) and optionally case-insensitive.
pub fn build_regex(query: &str, case_sensitive: bool) -> Result<Regex, regex::Error> {
    let flags = if case_sensitive { "(?m)" } else { "(?mi)" };
    Regex::new(&format!("{flags}{query}"))
}

/// Case-insensitive literal search returning byte ranges *into `text`
/// itself*. Lowercasing the whole text first would shift offsets whenever a
/// character changes length (e.g. `İ`), so characters are compared one by
/// one instead.
pub fn find_literal_ci(text: &str, query: &str) -> Vec<Range<usize>> {
    let q: Vec<char> = query.chars().flat_map(char::to_lowercase).collect();
    let mut out = Vec::new();
    if q.is_empty() {
        return out;
    }
    let starts: Vec<(usize, char)> = text.char_indices().collect();
    let mut s = 0;
    while s < starts.len() {
        let mut k = 0;
        let mut j = s;
        let mut ok = true;
        while k < q.len() {
            let Some(&(_, c)) = starts.get(j) else {
                ok = false;
                break;
            };
            for lc in c.to_lowercase() {
                if k >= q.len() || q[k] != lc {
                    ok = false;
                    break;
                }
                k += 1;
            }
            if !ok {
                break;
            }
            j += 1;
        }
        if ok {
            let end = starts.get(j).map_or(text.len(), |&(i, _)| i);
            out.push(starts[s].0..end);
            s = j;
        } else {
            s += 1;
        }
    }
    out
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

/// Moves the cursor to the start of logical line `target`.
///
/// `Motion::Down` steps one *visual* row, which differs from a logical line
/// when long lines wrap, so progress is read back from the cursor instead of
/// counted. Stops early if the cursor stops moving (e.g. an unshaped buffer).
pub fn goto_line_start(content: &mut iced::widget::text_editor::Content, target: usize) {
    use iced::widget::text_editor::{Action, Motion};

    content.perform(Action::Move(Motion::DocumentStart));
    let mut last = content.cursor_position();
    while last.0 < target {
        content.perform(Action::Move(Motion::Down));
        let now = content.cursor_position();
        if now == last {
            break;
        }
        last = now;
    }
    content.perform(Action::Move(Motion::Home));
}

/// Selects `range` in the editor. `text_editor` has no "select this byte
/// range" action, only relative motions — same approach as jumping to a
/// line in the Go to Line dialog.
fn select_range(content: &mut iced::widget::text_editor::Content, text: &str, range: Range<usize>) {
    use iced::widget::text_editor::{Action, Motion};

    let (line, col, len) = byte_range_to_line_col(text, &range);
    goto_line_start(content, line);
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
        let mut content = iced::widget::text_editor::Content::with_text("nothing here");
        let mut s = make_search("xyz", "", false, true);
        s.jump(&mut content, true);
        assert_eq!(s.current, None);
        assert_eq!(s.match_count, 0);
    }
}
