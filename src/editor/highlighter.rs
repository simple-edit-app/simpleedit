//! Syntax highlighting with the app's own palette.
//!
//! This is `iced_highlighter` re-done on top of `syntect` with a theme built
//! from the design tokens (keyword, string, function, number, comment, type),
//! instead of one of syntect's bundled themes. Search matches are not styled
//! here: they get a background, which the editor widget draws itself.

use std::ops::Range;
use std::str::FromStr;

use iced::advanced::text::{self, highlighter::Format};
use iced::{Color, Font};
use once_cell::sync::Lazy;
use syntect::highlighting::{
    self, Color as SynColor, ScopeSelectors, StyleModifier, ThemeItem, ThemeSettings,
};
use syntect::parsing;

static SYNTAXES: Lazy<parsing::SyntaxSet> = Lazy::new(parsing::SyntaxSet::load_defaults_nonewlines);

static DARK: Lazy<highlighting::Theme> = Lazy::new(|| build_theme(true));
static LIGHT: Lazy<highlighting::Theme> = Lazy::new(|| build_theme(false));

const LINES_PER_SNAPSHOT: usize = 50;

fn syn_color(c: Color) -> SynColor {
    let byte = |v: f32| (v * 255.0).round().clamp(0.0, 255.0) as u8;
    SynColor {
        r: byte(c.r),
        g: byte(c.g),
        b: byte(c.b),
        a: 255,
    }
}

/// The syntect theme for a mode: scope selectors mapped to the design's six
/// syntax colors. Later, more specific selectors win.
fn build_theme(dark: bool) -> highlighting::Theme {
    let s = crate::theme::syntax(dark);
    let rule = |selectors: &str, color: Color| ThemeItem {
        scope: ScopeSelectors::from_str(selectors).expect("valid scope selectors"),
        style: StyleModifier {
            foreground: Some(syn_color(color)),
            background: None,
            font_style: None,
        },
    };
    highlighting::Theme {
        name: Some(if dark { "Nocturne" } else { "Nocturne Light" }.to_string()),
        author: None,
        settings: ThemeSettings::default(),
        scopes: vec![
            rule("keyword, storage, entity.name.tag, markup.heading, punctuation.definition.heading", s.keyword),
            rule("string, markup.bold, markup.inline.raw, punctuation.definition.string", s.string),
            rule("entity.name.function, support.function, variable.function, meta.function-call, markup.list punctuation.definition, punctuation.definition.list_item", s.function),
            rule("constant.numeric, constant.language, constant.character.escape, support.constant", s.number),
            rule("comment, punctuation.definition.comment, punctuation.definition.raw, constant.other.language-name", s.comment),
            rule("entity.name.type, entity.name.class, entity.name.struct, entity.name.enum, entity.name.trait, entity.other.inherited-class, support.type, support.class, storage.type.primitive, storage.type.rust, storage.type.js, entity.other.attribute-name", s.type_),
        ],
    }
}

/// Settings of the highlighter: which grammar, and which mode's colors.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub dark: bool,
    pub extension: String,
}

/// A highlighted span: just its text color (`None` = the default text color).
pub struct Highlight(Option<Color>);

/// The `to_format` callback `text_editor::highlight` expects.
pub fn to_format(highlight: &Highlight, _theme: &iced::Theme) -> Format<Font> {
    Format {
        color: highlight.0,
        font: None,
    }
}

pub struct Highlighter {
    /// Whether to also color what the grammar leaves unscoped (see
    /// `refine`): PascalCase names as types, `name(` as calls.
    refine: bool,
    function: Color,
    type_: Color,
    syntax: &'static parsing::SyntaxReference,
    highlighter: highlighting::Highlighter<'static>,
    caches: Vec<(parsing::ParseState, parsing::ScopeStack)>,
    current_line: usize,
}

fn syntax_for(extension: &str) -> &'static parsing::SyntaxReference {
    SYNTAXES
        .find_syntax_by_token(extension)
        .unwrap_or_else(|| SYNTAXES.find_syntax_plain_text())
}

fn theme_for(dark: bool) -> &'static highlighting::Theme {
    if dark {
        &DARK
    } else {
        &LIGHT
    }
}

impl text::Highlighter for Highlighter {
    type Settings = Settings;
    type Highlight = Highlight;

    type Iterator<'a> = Box<dyn Iterator<Item = (Range<usize>, Self::Highlight)> + 'a>;

    fn new(settings: &Self::Settings) -> Self {
        let syntax = syntax_for(&settings.extension);

        let colors = crate::theme::syntax(settings.dark);
        Highlighter {
            refine: refines(&settings.extension),
            function: colors.function,
            type_: colors.type_,
            syntax,
            highlighter: highlighting::Highlighter::new(theme_for(settings.dark)),
            caches: vec![(parsing::ParseState::new(syntax), parsing::ScopeStack::new())],
            current_line: 0,
        }
    }

    fn update(&mut self, new_settings: &Self::Settings) {
        self.syntax = syntax_for(&new_settings.extension);
        self.highlighter = highlighting::Highlighter::new(theme_for(new_settings.dark));
        let colors = crate::theme::syntax(new_settings.dark);
        self.refine = refines(&new_settings.extension);
        self.function = colors.function;
        self.type_ = colors.type_;

        // Restart the highlighter
        self.change_line(0);
    }

    fn change_line(&mut self, line: usize) {
        let snapshot = line / LINES_PER_SNAPSHOT;

        if snapshot <= self.caches.len() {
            self.caches.truncate(snapshot);
            self.current_line = snapshot * LINES_PER_SNAPSHOT;
        } else {
            self.caches.truncate(1);
            self.current_line = 0;
        }

        let (parser, stack) = self.caches.last().cloned().unwrap_or_else(|| {
            (
                parsing::ParseState::new(self.syntax),
                parsing::ScopeStack::new(),
            )
        });

        self.caches.push((parser, stack));
    }

    fn highlight_line(&mut self, line: &str) -> Self::Iterator<'_> {
        if self.current_line / LINES_PER_SNAPSHOT >= self.caches.len() {
            let (parser, stack) = self.caches.last().expect("Caches must not be empty");

            self.caches.push((parser.clone(), stack.clone()));
        }

        self.current_line += 1;

        let (parser, stack) = self.caches.last_mut().expect("Caches must not be empty");

        let ops = parser.parse_line(line, &SYNTAXES).unwrap_or_default();

        let highlighter = &self.highlighter;

        let spans: Vec<(Range<usize>, Highlight)> = ScopeRangeIterator {
            ops,
            line_length: line.len(),
            index: 0,
            last_str_index: 0,
        }
        .filter_map(|(range, scope)| {
            let _ = stack.apply(&scope);

            if range.is_empty() {
                None
            } else {
                let color = highlighter
                    .style_mod_for_stack(&stack.scopes)
                    .foreground
                    .map(|c| Color::from_rgb8(c.r, c.g, c.b));
                Some((range, Highlight(color)))
            }
        })
        .collect();

        if self.refine {
            Box::new(refine(line, spans, self.function, self.type_).into_iter())
        } else {
            Box::new(spans.into_iter())
        }
    }

    fn current_line(&self) -> usize {
        self.current_line
    }
}

/// Whether a language gets the `refine` pass: programming languages whose
/// grammars leave type names and calls unscoped. Markup, data and shell
/// formats don't.
fn refines(extension: &str) -> bool {
    !matches!(
        extension,
        "" | "md"
            | "markdown"
            | "json"
            | "yaml"
            | "yml"
            | "toml"
            | "xml"
            | "html"
            | "css"
            | "sql"
            | "sh"
            | "bash"
            | "txt"
    )
}

/// Colors what the grammar left plain: a PascalCase word (at least one
/// lowercase letter, so constants like `ACCENT` stay as they are) is a type,
/// and a word right before `(` is a call.
fn refine(
    line: &str,
    spans: Vec<(Range<usize>, Highlight)>,
    function: Color,
    type_: Color,
) -> Vec<(Range<usize>, Highlight)> {
    let mut out = Vec::with_capacity(spans.len());
    for (range, highlight) in spans {
        if highlight.0.is_some() {
            out.push((range, highlight));
            continue;
        }
        let text = &line[range.clone()];
        let mut last = 0;
        let mut word_start: Option<usize> = None;
        let push = |from: usize, to: usize, color: Option<Color>, out: &mut Vec<_>| {
            if from < to {
                out.push((range.start + from..range.start + to, Highlight(color)));
            }
        };
        let is_word = |c: char| c.is_alphanumeric() || c == '_';
        for (i, c) in text
            .char_indices()
            .chain(std::iter::once((text.len(), ' ')))
        {
            match (word_start, is_word(c)) {
                (None, true) => word_start = Some(i),
                (Some(start), false) => {
                    let word = &text[start..i];
                    let first = word.chars().next().unwrap_or(' ');
                    let color = if first.is_uppercase() && word.chars().any(char::is_lowercase) {
                        Some(type_)
                    } else if first.is_alphabetic() || first == '_' {
                        // A call: the next character *in the line* is `(`.
                        let after = line[range.start + i..].chars().next();
                        (after == Some('(')).then_some(function)
                    } else {
                        None
                    };
                    if color.is_some() {
                        push(last, start, None, &mut out);
                        push(start, i, color, &mut out);
                        last = i;
                    }
                    word_start = None;
                }
                _ => {}
            }
        }
        push(last, text.len(), None, &mut out);
    }
    out
}

/// Turns syntect's scope-stack operations into (byte range, op) pairs that
/// tile the line.
struct ScopeRangeIterator {
    ops: Vec<(usize, parsing::ScopeStackOp)>,
    line_length: usize,
    index: usize,
    last_str_index: usize,
}

impl Iterator for ScopeRangeIterator {
    type Item = (Range<usize>, parsing::ScopeStackOp);

    fn next(&mut self) -> Option<Self::Item> {
        if self.index > self.ops.len() {
            return None;
        }

        let next_str_i = if self.index == self.ops.len() {
            self.line_length
        } else {
            self.ops[self.index].0
        };

        let range = self.last_str_index..next_str_i;
        self.last_str_index = next_str_i;

        let op = if self.index == 0 {
            parsing::ScopeStackOp::Noop
        } else {
            self.ops[self.index - 1].1.clone()
        };

        self.index += 1;
        Some((range, op))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use text::Highlighter as _;

    fn colors(ext: &str, line: &str, dark: bool) -> Vec<(String, Option<Color>)> {
        let mut h = Highlighter::new(&Settings {
            dark,
            extension: ext.to_string(),
        });
        h.highlight_line(line)
            .map(|(r, hl)| (line[r].to_string(), hl.0))
            .collect()
    }

    fn color_of(spans: &[(String, Option<Color>)], needle: &str) -> Option<Color> {
        spans
            .iter()
            .find(|(t, _)| t.contains(needle))
            .and_then(|(_, c)| *c)
    }

    #[test]
    fn rust_tokens_use_the_design_palette() {
        let s = crate::theme::syntax(true);
        let kw = colors("rs", "pub struct Palette {", true);
        assert_eq!(color_of(&kw, "pub"), Some(s.keyword));
        assert_eq!(color_of(&kw, "struct"), Some(s.keyword));
        assert_eq!(color_of(&kw, "Palette"), Some(s.type_));

        let field = colors("rs", "    pub bg: Color,", true);
        assert_eq!(color_of(&field, "Color"), Some(s.type_));

        let sig = colors("rs", "pub fn palette(dark: bool) -> Palette {", true);
        assert_eq!(color_of(&sig, "palette"), Some(s.function));
        assert_eq!(color_of(&sig, "bool"), Some(s.type_));
        assert_eq!(color_of(&sig, "Palette"), Some(s.type_));

        let call = colors("rs", "let x = rgb(0x14, \"abc\");", true);
        assert_eq!(color_of(&call, "rgb"), Some(s.function));
        assert_eq!(color_of(&call, "0x14"), Some(s.number));
        assert_eq!(color_of(&call, "abc"), Some(s.string));

        let comment = colors("rs", "// Palette de couleurs", true);
        assert_eq!(color_of(&comment, "Palette"), Some(s.comment));
    }

    #[test]
    fn constants_and_plain_words_stay_plain() {
        let spans = colors("rs", "    accent: ACCENT,", true);
        assert_eq!(color_of(&spans, "ACCENT"), None);
        assert_eq!(color_of(&spans, "accent"), None);
    }

    #[test]
    fn markdown_is_not_refined_but_uses_the_palette() {
        let s = crate::theme::syntax(true);
        let heading = colors("md", "# SimpleEdit", true);
        assert_eq!(color_of(&heading, "SimpleEdit"), Some(s.keyword));
        let bold = colors("md", "Un texte **rapide**, propre", true);
        assert_eq!(color_of(&bold, "rapide"), Some(s.string));
        // PascalCase in prose is not a type.
        let prose = colors("md", "Voir SimpleEdit ici", true);
        assert_eq!(color_of(&prose, "SimpleEdit"), None);
    }

    #[test]
    fn light_mode_uses_light_colors() {
        let d = colors("rs", "// c", true);
        let l = colors("rs", "// c", false);
        assert_ne!(color_of(&d, "c"), color_of(&l, "c"));
    }

    #[test]
    fn refined_spans_still_tile_the_line() {
        let line = "    let v = Vec::new(Foo, bar(1));";
        let mut h = Highlighter::new(&Settings {
            dark: true,
            extension: "rs".to_string(),
        });
        let mut at = 0;
        for (r, _) in h.highlight_line(line) {
            assert_eq!(r.start, at, "gap before {r:?}");
            at = r.end;
        }
        assert_eq!(at, line.len());
    }
}
