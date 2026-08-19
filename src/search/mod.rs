use iced::{
    widget::{button, container, row, text, text_input, Space},
    Element, Length,
};
use regex::Regex;

use crate::app::Message;
use crate::theme;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum SearchMessage {
    QueryChanged(String),
    ReplaceChanged(String),
    Find,
    ReplaceNext,
    ReplaceAll,
    ToggleRegex,
    ToggleCaseSensitive,
    Close,
}

pub struct SearchState {
    pub query: String,
    pub replacement: String,
    pub use_regex: bool,
    pub case_sensitive: bool,
    pub match_count: usize,
    pub last_error: Option<String>,
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
        }
    }

    pub fn update(&mut self, msg: SearchMessage, content: &mut iced::widget::text_editor::Content) {
        match msg {
            SearchMessage::QueryChanged(q) => {
                self.query = q;
                self.last_error = None;
            }
            SearchMessage::ReplaceChanged(r) => self.replacement = r,
            SearchMessage::ToggleRegex => self.use_regex = !self.use_regex,
            SearchMessage::ToggleCaseSensitive => self.case_sensitive = !self.case_sensitive,
            SearchMessage::ReplaceAll => {
                let text = content.text().to_string();
                match self.replace_all(&text) {
                    Ok(new_text) => {
                        *content = iced::widget::text_editor::Content::with_text(&new_text);
                    }
                    Err(e) => self.last_error = Some(e),
                }
            }
            SearchMessage::Find => {
                let text = content.text().to_string();
                self.match_count = self.count_matches(&text);
            }
            _ => {}
        }
    }

    pub fn count_matches(&self, text: &str) -> usize {
        if self.query.is_empty() {
            return 0;
        }
        if self.use_regex {
            let flags = if self.case_sensitive { "" } else { "(?i)" };
            Regex::new(&format!("{}{}", flags, self.query))
                .map(|re| re.find_iter(text).count())
                .unwrap_or(0)
        } else if self.case_sensitive {
            text.matches(&self.query).count()
        } else {
            let q = self.query.to_lowercase();
            let t = text.to_lowercase();
            t.matches(q.as_str()).count()
        }
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
        let count_label = if self.match_count > 0 {
            format!("{} {}", self.match_count, t!("search.matches"))
        } else if let Some(ref e) = self.last_error {
            e.clone()
        } else {
            String::new()
        };
        let find_placeholder = t!("search.find_placeholder").to_string();
        let replace_placeholder = t!("search.replace_placeholder").to_string();
        let find_label = t!("search.find").to_string();
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

        let bar = row![
            text_input(find_placeholder.as_str(), &self.query)
                .on_input(|v| Message::Search(SearchMessage::QueryChanged(v)))
                .size(13)
                .width(220),
            text_input(replace_placeholder.as_str(), &self.replacement)
                .on_input(|v| Message::Search(SearchMessage::ReplaceChanged(v)))
                .size(13)
                .width(220),
            action(find_label, Message::Search(SearchMessage::Find)),
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
        }
    }

    #[test]
    fn count_matches_literal() {
        let s = make_search("hello", "", false, true);
        assert_eq!(s.count_matches("hello world hello"), 2);
    }

    #[test]
    fn count_matches_case_insensitive() {
        let s = make_search("hello", "", false, false);
        assert_eq!(s.count_matches("Hello HELLO hello"), 3);
    }

    #[test]
    fn count_matches_regex() {
        let s = make_search(r"\d+", "", true, false);
        assert_eq!(s.count_matches("abc 123 def 456"), 2);
    }

    #[test]
    fn count_matches_empty_query() {
        let s = make_search("", "", false, false);
        assert_eq!(s.count_matches("anything"), 0);
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
}
