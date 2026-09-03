//! Wraps the syntax highlighter with a second pass that recolors whatever
//! the current search query matches, so occurrences are visible without
//! losing syntax colors.
//!
//! `iced`'s text-highlighting API can only recolor/re-font a range of text —
//! there is no per-range background, so a match reads as bold, accent-
//! colored text rather than a classic highlighter-pen box. The *current*
//! match still gets a real highlighted background: the search module
//! additionally selects it in the editor, and selection highlighting is a
//! real background the editor already draws.

use std::ops::Range;

use iced::advanced::text::{self, highlighter::Format};
use iced::{highlighter as syntax, Font};
use regex::Regex;

/// Settings for the combined highlighter: the underlying syntax settings,
/// plus the live search query driving the second pass.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub syntax: syntax::Settings,
    pub query: String,
    pub case_sensitive: bool,
    pub use_regex: bool,
}

/// One highlighted span: the syntax format underneath, and whether a search
/// match covers it.
#[derive(Debug, Clone, Copy)]
pub struct Highlight {
    format: Format<Font>,
    is_match: bool,
}

/// The `to_format` callback `text_editor::highlight` expects — a bare `fn`,
/// so a search match's styling (bold + accent) has to be hardcoded here
/// rather than threaded through as data.
pub fn to_format(highlight: &Highlight, _theme: &iced::Theme) -> Format<Font> {
    if highlight.is_match {
        Format {
            color: Some(crate::theme::accent_color()),
            font: Some(Font {
                weight: iced::font::Weight::Bold,
                ..Font::DEFAULT
            }),
        }
    } else {
        highlight.format
    }
}

pub struct Highlighter {
    inner: syntax::Highlighter,
    syntax_settings: syntax::Settings,
    query: String,
    case_sensitive: bool,
    use_regex: bool,
    /// Compiled once per `update`, not per line — `None` when the query
    /// isn't in use, is empty, or (regex mode) doesn't compile.
    regex: Option<Regex>,
}

impl Highlighter {
    fn line_matches(&self, line: &str) -> Vec<Range<usize>> {
        if self.query.is_empty() {
            return Vec::new();
        }
        if self.use_regex {
            match &self.regex {
                Some(re) => re.find_iter(line).map(|m| m.start()..m.end()).collect(),
                None => Vec::new(),
            }
        } else if self.case_sensitive {
            line.match_indices(self.query.as_str())
                .map(|(i, m)| i..i + m.len())
                .collect()
        } else {
            let lower_line = line.to_lowercase();
            let lower_query = self.query.to_lowercase();
            lower_line
                .match_indices(lower_query.as_str())
                .map(|(i, _)| i..i + lower_query.len())
                .collect()
        }
    }
}

fn compile_regex(settings: &Settings) -> Option<Regex> {
    if !settings.use_regex || settings.query.is_empty() {
        return None;
    }
    let flags = if settings.case_sensitive { "" } else { "(?i)" };
    Regex::new(&format!("{flags}{}", settings.query)).ok()
}

impl text::Highlighter for Highlighter {
    type Settings = Settings;
    type Highlight = Highlight;
    type Iterator<'a> = std::vec::IntoIter<(Range<usize>, Highlight)>;

    fn new(settings: &Self::Settings) -> Self {
        Self {
            inner: <syntax::Highlighter as text::Highlighter>::new(&settings.syntax),
            syntax_settings: settings.syntax.clone(),
            query: settings.query.clone(),
            case_sensitive: settings.case_sensitive,
            use_regex: settings.use_regex,
            regex: compile_regex(settings),
        }
    }

    fn update(&mut self, settings: &Self::Settings) {
        // Re-parsing from scratch is only needed when the syntax itself
        // changed — a keystroke in the search field shouldn't pay for that.
        if settings.syntax != self.syntax_settings {
            self.inner.update(&settings.syntax);
            self.syntax_settings = settings.syntax.clone();
        }
        self.query = settings.query.clone();
        self.case_sensitive = settings.case_sensitive;
        self.use_regex = settings.use_regex;
        self.regex = compile_regex(settings);
    }

    fn change_line(&mut self, line: usize) {
        self.inner.change_line(line);
    }

    fn highlight_line(&mut self, line: &str) -> Self::Iterator<'_> {
        let base: Vec<(Range<usize>, Format<Font>)> = self
            .inner
            .highlight_line(line)
            .map(|(range, highlight)| (range, highlight.to_format()))
            .collect();

        let matches = self.line_matches(line);
        overlay_matches(base, &matches)
            .into_iter()
            .map(|(range, format, is_match)| (range, Highlight { format, is_match }))
            .collect::<Vec<_>>()
            .into_iter()
    }

    fn current_line(&self) -> usize {
        self.inner.current_line()
    }
}

/// Splits `base` (ranges tiling a line, each carrying a `T`) at every
/// boundary in `matches` (non-overlapping ranges into the same line),
/// producing the minimal set of sub-ranges needed to tag each one with both
/// its underlying `T` and whether a match covers it.
fn overlay_matches<T: Copy>(
    base: Vec<(Range<usize>, T)>,
    matches: &[Range<usize>],
) -> Vec<(Range<usize>, T, bool)> {
    if matches.is_empty() {
        return base
            .into_iter()
            .map(|(range, t)| (range, t, false))
            .collect();
    }

    let mut cuts: Vec<usize> = base.iter().flat_map(|(r, _)| [r.start, r.end]).collect();
    cuts.extend(matches.iter().flat_map(|r| [r.start, r.end]));
    cuts.sort_unstable();
    cuts.dedup();

    let mut out = Vec::with_capacity(cuts.len());
    for pair in cuts.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if a >= b {
            continue;
        }
        let Some(&(_, t)) = base.iter().find(|(r, _)| r.start <= a && a < r.end) else {
            continue;
        };
        let is_match = matches.iter().any(|r| r.start <= a && a < r.end);
        out.push((a..b, t, is_match));
    }
    out
}

#[cfg(test)]
#[allow(clippy::single_range_in_vec_init)]
mod tests {
    use super::*;

    fn fmt(n: u8) -> Format<Font> {
        Format {
            color: Some(iced::Color::from_rgb8(n, n, n)),
            font: None,
        }
    }

    #[test]
    fn no_matches_leaves_base_ranges_untouched() {
        let base = vec![(0..3, fmt(1)), (3..6, fmt(2))];
        let out = overlay_matches(base.clone(), &[]);
        assert_eq!(out, vec![(0..3, fmt(1), false), (3..6, fmt(2), false)]);
    }

    #[test]
    fn a_match_splits_the_base_range_it_falls_within() {
        // "let x = 1" — one base range (plain text, no real syntax spans),
        // with a match on "x" at 4..5.
        let base = vec![(0..9, fmt(1))];
        let out = overlay_matches(base, &[4..5]);
        assert_eq!(
            out,
            vec![
                (0..4, fmt(1), false),
                (4..5, fmt(1), true),
                (5..9, fmt(1), false)
            ]
        );
    }

    #[test]
    fn a_match_spanning_two_base_ranges_splits_both() {
        let base = vec![(0..3, fmt(1)), (3..6, fmt(2))];
        let out = overlay_matches(base, &[2..4]);
        assert_eq!(
            out,
            vec![
                (0..2, fmt(1), false),
                (2..3, fmt(1), true),
                (3..4, fmt(2), true),
                (4..6, fmt(2), false)
            ]
        );
    }

    #[test]
    fn a_match_exactly_covering_a_base_range_needs_no_split() {
        let base = vec![(0..3, fmt(1)), (3..6, fmt(2))];
        let out = overlay_matches(base, &[3..6]);
        assert_eq!(out, vec![(0..3, fmt(1), false), (3..6, fmt(2), true)]);
    }

    #[test]
    fn compile_regex_is_none_outside_regex_mode_or_when_empty() {
        let s = |query: &str, use_regex: bool| Settings {
            syntax: syntax::Settings {
                theme: syntax::Theme::SolarizedDark,
                extension: String::new(),
            },
            query: query.to_string(),
            case_sensitive: false,
            use_regex,
        };
        assert!(compile_regex(&s("abc", false)).is_none());
        assert!(compile_regex(&s("", true)).is_none());
        assert!(compile_regex(&s(r"\d+", true)).is_some());
        assert!(compile_regex(&s(r"[invalid", true)).is_none());
    }
}
