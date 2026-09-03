//! Wraps the syntax highlighter with a second pass that recolors whatever
//! the current search query matches, so occurrences are visible without
//! losing syntax colors.
//!
//! `iced`'s text-highlighting API can only recolor/re-font a range of text —
//! there is no per-range background — and, separately, `text_editor` only
//! draws its selection highlight while it holds keyboard focus, which the
//! search field does instead while searching. So neither the "all matches"
//! nor the "current match" indication can lean on the editor's own
//! selection; both go through this highlighter, driven by data (the query,
//! and the current match's position) rather than focus or a real
//! background.

use std::ops::Range;

use iced::advanced::text::{self, highlighter::Format};
use iced::{highlighter as syntax, Color, Font};
use regex::Regex;

/// Settings for the combined highlighter: the underlying syntax settings,
/// the live search query driving the second pass, and — if a search is
/// active and has a current match — that match's position, so it can be
/// styled apart from the others.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub syntax: syntax::Settings,
    pub query: String,
    pub case_sensitive: bool,
    pub use_regex: bool,
    /// (line index, byte range within that line) of the current match.
    pub current: Option<(usize, Range<usize>)>,
}

/// Whether a highlighted span is a search match, and if so, whether it's the
/// one the search bar is currently on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MatchKind {
    None,
    Match,
    Current,
}

/// One highlighted span: the syntax format underneath, and whether/how a
/// search match covers it.
#[derive(Debug, Clone, Copy)]
pub struct Highlight {
    format: Format<Font>,
    kind: MatchKind,
}

/// Bold + a saturated "highlighter" color is the most `Format` (color and
/// font, nothing else) can do to stand in for a real highlighter-pen
/// background — picked bright enough, and different enough from the app's
/// own violet accent used all over the rest of the UI, to actually read as
/// "this is a search match" at a glance. Two tones per role: `Theme::palette`
/// gives `to_format` enough to tell dark from light without Settings having
/// to carry it separately.
fn match_color(dark: bool) -> Color {
    if dark {
        Color::from_rgb(0.95, 0.80, 0.20) // warm gold
    } else {
        Color::from_rgb(0.62, 0.46, 0.02) // deep amber, readable on pale paper
    }
}

/// The current match's color — distinct from every other match's gold, not
/// just bolder.
fn current_match_color(dark: bool) -> Color {
    if dark {
        Color::from_rgb(1.0, 0.42, 0.30) // vivid coral
    } else {
        Color::from_rgb(0.78, 0.20, 0.10) // brick red
    }
}

fn is_dark(theme: &iced::Theme) -> bool {
    let bg = theme.palette().background;
    let luminance = 0.299 * bg.r + 0.587 * bg.g + 0.114 * bg.b;
    luminance < 0.5
}

fn bold() -> Font {
    Font {
        weight: iced::font::Weight::Bold,
        ..Font::DEFAULT
    }
}

/// The `to_format` callback `text_editor::highlight` expects — a bare `fn`,
/// so this styling has to be hardcoded here rather than threaded through as
/// data.
pub fn to_format(highlight: &Highlight, theme: &iced::Theme) -> Format<Font> {
    let dark = is_dark(theme);
    match highlight.kind {
        MatchKind::Current => Format {
            color: Some(current_match_color(dark)),
            font: Some(bold()),
        },
        MatchKind::Match => Format {
            color: Some(match_color(dark)),
            font: Some(bold()),
        },
        MatchKind::None => highlight.format,
    }
}

pub struct Highlighter {
    inner: syntax::Highlighter,
    syntax_settings: syntax::Settings,
    query: String,
    case_sensitive: bool,
    use_regex: bool,
    current: Option<(usize, Range<usize>)>,
    /// Compiled once per `update`, not per line — `None` when the query
    /// isn't in use, is empty, or (regex mode) doesn't compile.
    regex: Option<Regex>,
}

impl Highlighter {
    /// Every match on `line`, each flagged with whether it's the current one
    /// — `line_idx` is which line this is, matched against `self.current`.
    fn line_matches(&self, line: &str, line_idx: usize) -> Vec<(Range<usize>, bool)> {
        if self.query.is_empty() {
            return Vec::new();
        }
        let ranges: Vec<Range<usize>> = if self.use_regex {
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
        };

        let current = self
            .current
            .as_ref()
            .filter(|(l, _)| *l == line_idx)
            .map(|(_, r)| r.clone());

        ranges
            .into_iter()
            .map(|r| {
                let is_current = current.as_ref() == Some(&r);
                (r, is_current)
            })
            .collect()
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
            current: settings.current.clone(),
            regex: compile_regex(settings),
        }
    }

    fn update(&mut self, settings: &Self::Settings) {
        // `update` is only called when `Settings` actually changed (the
        // widget compares by PartialEq), and — since `SearchState` only
        // hands the query over once a search has actually run, not on every
        // keystroke — that's now a real action (Find/Next/Previous/Replace,
        // or a file/theme switch), not something that fires on every
        // keystroke. So there's no need to guard the inner update: even
        // when only the search overlay changed, every already-shaped line
        // needs telling its *effective* highlight may have changed, and
        // `iced_highlighter::Highlighter::update` is what does that (it
        // resets to line 0 internally) — re-deriving the same syntax/theme
        // it already has is a rounding error next to that.
        self.inner.update(&settings.syntax);
        self.syntax_settings = settings.syntax.clone();
        self.query = settings.query.clone();
        self.case_sensitive = settings.case_sensitive;
        self.use_regex = settings.use_regex;
        self.current = settings.current.clone();
        self.regex = compile_regex(settings);
    }

    fn change_line(&mut self, line: usize) {
        self.inner.change_line(line);
    }

    fn highlight_line(&mut self, line: &str) -> Self::Iterator<'_> {
        // current_line() is the index of the line about to be processed —
        // highlight_line() below advances it, so this has to be read first.
        let line_idx = self.inner.current_line();

        let base: Vec<(Range<usize>, Format<Font>)> = self
            .inner
            .highlight_line(line)
            .map(|(range, highlight)| (range, highlight.to_format()))
            .collect();

        let matches = self.line_matches(line, line_idx);
        overlay_matches(base, &matches)
            .into_iter()
            .map(|(range, format, kind)| (range, Highlight { format, kind }))
            .collect::<Vec<_>>()
            .into_iter()
    }

    fn current_line(&self) -> usize {
        self.inner.current_line()
    }
}

/// Splits `base` (ranges tiling a line, each carrying a `T`) at every
/// boundary in `matches` (non-overlapping ranges into the same line, each
/// flagged with whether it's the current match), producing the minimal set
/// of sub-ranges needed to tag each one with its underlying `T` and its
/// [`MatchKind`].
fn overlay_matches<T: Copy>(
    base: Vec<(Range<usize>, T)>,
    matches: &[(Range<usize>, bool)],
) -> Vec<(Range<usize>, T, MatchKind)> {
    if matches.is_empty() {
        return base
            .into_iter()
            .map(|(range, t)| (range, t, MatchKind::None))
            .collect();
    }

    let mut cuts: Vec<usize> = base.iter().flat_map(|(r, _)| [r.start, r.end]).collect();
    cuts.extend(matches.iter().flat_map(|(r, _)| [r.start, r.end]));
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
        let kind = match matches.iter().find(|(r, _)| r.start <= a && a < r.end) {
            Some((_, true)) => MatchKind::Current,
            Some((_, false)) => MatchKind::Match,
            None => MatchKind::None,
        };
        out.push((a..b, t, kind));
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
        assert_eq!(
            out,
            vec![
                (0..3, fmt(1), MatchKind::None),
                (3..6, fmt(2), MatchKind::None)
            ]
        );
    }

    #[test]
    fn a_match_splits_the_base_range_it_falls_within() {
        // "let x = 1" — one base range (plain text, no real syntax spans),
        // with a match on "x" at 4..5.
        let base = vec![(0..9, fmt(1))];
        let out = overlay_matches(base, &[(4..5, false)]);
        assert_eq!(
            out,
            vec![
                (0..4, fmt(1), MatchKind::None),
                (4..5, fmt(1), MatchKind::Match),
                (5..9, fmt(1), MatchKind::None)
            ]
        );
    }

    #[test]
    fn a_match_spanning_two_base_ranges_splits_both() {
        let base = vec![(0..3, fmt(1)), (3..6, fmt(2))];
        let out = overlay_matches(base, &[(2..4, false)]);
        assert_eq!(
            out,
            vec![
                (0..2, fmt(1), MatchKind::None),
                (2..3, fmt(1), MatchKind::Match),
                (3..4, fmt(2), MatchKind::Match),
                (4..6, fmt(2), MatchKind::None)
            ]
        );
    }

    #[test]
    fn a_match_exactly_covering_a_base_range_needs_no_split() {
        let base = vec![(0..3, fmt(1)), (3..6, fmt(2))];
        let out = overlay_matches(base, &[(3..6, false)]);
        assert_eq!(
            out,
            vec![
                (0..3, fmt(1), MatchKind::None),
                (3..6, fmt(2), MatchKind::Match)
            ]
        );
    }

    #[test]
    fn the_current_match_is_tagged_separately_from_the_others() {
        let base = vec![(0..9, fmt(1))];
        let out = overlay_matches(base, &[(0..1, false), (4..5, true)]);
        assert_eq!(
            out,
            vec![
                (0..1, fmt(1), MatchKind::Match),
                (1..4, fmt(1), MatchKind::None),
                (4..5, fmt(1), MatchKind::Current),
                (5..9, fmt(1), MatchKind::None),
            ]
        );
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
            current: None,
        };
        assert!(compile_regex(&s("abc", false)).is_none());
        assert!(compile_regex(&s("", true)).is_none());
        assert!(compile_regex(&s(r"\d+", true)).is_some());
        assert!(compile_regex(&s(r"[invalid", true)).is_none());
    }
}
