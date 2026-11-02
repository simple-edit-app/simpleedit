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

static SYNTAXES: Lazy<parsing::SyntaxSet> = Lazy::new(two_face::syntax::extra_newlines);

static DARK: Lazy<highlighting::Theme> = Lazy::new(|| build_theme(true));
static LIGHT: Lazy<highlighting::Theme> = Lazy::new(|| build_theme(false));

const LINES_PER_SNAPSHOT: usize = 50;

/// Lines longer than this (in bytes) are not colored.
const MAX_HIGHLIGHTED_LINE: usize = 10_000;

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

/// The grammar that colors a language token (the extended set of `two-face`
/// has TypeScript, TSX / JSX, Kotlin, Swift, SCSS, Less and TOML of its own).
fn syntax_for(token: &str) -> &'static parsing::SyntaxReference {
    // The Babel grammar loses itself in JSX (a `/` of a closing tag read as
    // the start of a regular expression); TSX reads the same files soundly.
    let token = if token == "jsx" { "tsx" } else { token };
    SYNTAXES
        .find_syntax_by_token(token)
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

        // Coloring a minified file's single line takes seconds on each
        // keystroke; such a line stays plain.
        if line.len() > MAX_HIGHLIGHTED_LINE {
            return Box::new(std::iter::empty());
        }

        let (parser, stack) = self.caches.last_mut().expect("Caches must not be empty");

        // The grammars are written for lines that end with a newline (a tag
        // name last on its line, a `$` anchor…): give them one, and keep
        // only what lies inside the line.
        let terminated = format!("{line}\n");
        let ops = parser
            .parse_line(&terminated, &SYNTAXES)
            .unwrap_or_default();

        let highlighter = &self.highlighter;
        let line_len = line.len();

        let spans: Vec<(Range<usize>, Highlight)> = ScopeRangeIterator {
            ops,
            line_length: terminated.len(),
            index: 0,
            last_str_index: 0,
        }
        .filter_map(|(range, scope)| {
            let _ = stack.apply(&scope);
            let range = range.start.min(line_len)..range.end.min(line_len);

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
            | "scss"
            | "less"
            | "make"
            | "diff"
            | "tex"
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
mod samples;

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

    /// Colors of each line of a snippet, one highlighter for all of them.
    fn colors_of_lines(ext: &str, text: &str) -> Vec<Vec<(String, Option<Color>)>> {
        let mut h = Highlighter::new(&Settings {
            dark: true,
            extension: ext.to_string(),
        });
        text.lines()
            .map(|line| {
                h.highlight_line(line)
                    .map(|(r, hl)| (line[r].to_string(), hl.0))
                    .collect()
            })
            .collect()
    }

    const SCREENSHOT_JS: &str = r#"  if (parsedJson?.elements.length > 1) {
    let thumbnailUrl = null;
    if (parsedJson?.elements[1].thumbnail) {
      thumbnailUrl = parsedJson?.elements[1].thumbnail;
    } else {
      const ImagesOfSequence1 = parsedJson?.elements[1].elements.filter(
        (element) => element.type === IMAGE_TYPE,
      );
      if (ImagesOfSequence1.length > 0) {
        thumbnailUrl = `${ImagesOfSequence1[0].source}/width=200,height=200`;
      }
    }

    if (thumbnailUrl) {
      await updateVideoThumbnailData(videoId, thumbnailUrl);
    }
  }

  const currentDomain = currentVideo?.domain;
  if (currentDomain.name !== parsedJson?.advertiser) {
    await updateDomainNameData(currentVideo.domainId, parsedJson?.advertiser);
  }
"#;

    #[test]
    fn code_after_a_template_literal_is_not_colored_as_a_string() {
        let s = crate::theme::syntax(true);
        for ext in ["js", "jsx", "ts", "tsx"] {
            let lines = colors_of_lines(ext, SCREENSHOT_JS);
            let last = lines.len() - 1;
            for (n, line) in lines.iter().enumerate().skip(12) {
                for (text, color) in line {
                    if text.trim().is_empty() || text.starts_with('"') {
                        continue;
                    }
                    assert_ne!(
                        *color,
                        Some(s.string),
                        "{ext}: line {n}: {text:?} in {line:?}"
                    );
                }
            }
            assert!(last > 12);
        }
    }

    const SCREENSHOT_TSX: &str = r#"export const EditorButton = ({
  children,
  type = 'default',
}: EditorButtonProps) => {
  const primaryClassName = `text-white ${!disabled && 'hover:bg-gray-800'}`;
  return (
    <button
      title={title ?? ''}
      className={cn(
        `relative w-[120px] ${!disabled && 'hover:bg-gray-100'}`,
        type === 'primary' ? primaryClassName : '',
        className,
      )}
      onClick={onClick}
    >
      {children}
    </button>
  );
};

const next = 1;
"#;

    #[test]
    fn jsx_tags_are_colored_alike_and_strings_stop_at_their_end() {
        let s = crate::theme::syntax(true);
        for ext in ["tsx", "jsx"] {
            let lines = colors_of_lines(ext, SCREENSHOT_TSX);
            let at = |needle: &str| {
                SCREENSHOT_TSX
                    .lines()
                    .position(|l| l.contains(needle))
                    .unwrap()
            };
            let tag = |line: usize| color_of(&lines[line], "button");
            let (open, close) = (tag(at("<button")), tag(at("</button>")));
            assert_eq!(open, close, "{ext}: opening and closing tag differ");
            assert!(
                open.is_some(),
                "{ext}: the tag is not colored: {:?} / {:?}",
                lines[at("<button")],
                lines[at("</button>")]
            );
            let next = &lines[lines.len() - 1];
            assert_eq!(
                color_of(next, "const"),
                color_of(&lines[4], "const"),
                "{ext}"
            );
            assert_ne!(color_of(next, "next"), Some(s.string), "{ext}");
            assert_eq!(color_of(next, "1"), Some(s.number), "{ext}");
        }
    }

    /// Runs the highlighter over real code and reports the files that end
    /// inside a string, a comment or a block: in valid source everything is
    /// closed by the last line, so an open scope there means the grammar lost
    /// its way: `HIGHLIGHT_CORPUS=/path/a:/path/b cargo test highlight_corpus -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn highlight_corpus_ends_every_file_outside_any_scope() {
        use std::collections::BTreeMap;
        let Ok(roots) = std::env::var("HIGHLIGHT_CORPUS") else {
            return;
        };
        const SKIPPED: [&str; 9] = [
            "node_modules",
            "target",
            ".git",
            "vendor",
            "dist",
            "build",
            ".next",
            ".venv",
            "__pycache__",
        ];
        fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().into_owned();
                if path.is_dir() {
                    if !SKIPPED.contains(&name.as_str()) {
                        walk(&path, out);
                    }
                } else {
                    out.push(path);
                }
            }
        }
        let mut files = Vec::new();
        for root in roots.split(':') {
            walk(std::path::Path::new(root), &mut files);
        }
        let mut scanned: BTreeMap<&str, usize> = BTreeMap::new();
        let mut open_at_end: BTreeMap<&str, Vec<String>> = BTreeMap::new();
        for path in files {
            let Some(token) = path
                .extension()
                .and_then(|e| e.to_str())
                .and_then(crate::editor::extension_to_token)
            else {
                continue;
            };
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            if text.len() > 300_000 {
                continue;
            }
            *scanned.entry(token).or_default() += 1;
            let mut h = Highlighter::new(&Settings {
                dark: true,
                extension: token.to_string(),
            });
            for line in text.lines() {
                h.highlight_line(line).count();
            }
            let stack = &h.caches.last().expect("a cache").1;
            // Open blocks and groups at the end are harmless (a header cut
            // short, a Markdown paragraph); what hurts is a string, a comment
            // or a regular expression that never ends.
            let swallowing = |scope: &parsing::Scope| {
                let name = scope.build_string();
                ["string.", "comment.", "regexp"]
                    .iter()
                    .any(|k| name.contains(k))
            };
            if stack.scopes.iter().any(swallowing) {
                open_at_end
                    .entry(token)
                    .or_default()
                    .push(format!("{} => {stack}", path.display()));
            }
        }
        eprintln!("SCANNED {scanned:?}");
        for (token, list) in &open_at_end {
            eprintln!("OPEN {token}: {} of {}", list.len(), scanned[token]);
            for f in list.iter().take(8) {
                eprintln!("  {f}");
            }
        }
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

    #[test]
    fn every_listed_language_is_colored_by_some_grammar() {
        for (token, name) in crate::editor::SUPPORTED_LANGUAGES {
            let syntax = super::syntax_for(token);
            assert_ne!(syntax.name, "Plain Text", "{name} ({token}) has no grammar");
        }
    }

    #[test]
    fn a_very_long_line_is_left_plain() {
        use iced::advanced::text::Highlighter as _;
        let mut h = super::Highlighter::new(&super::Settings {
            dark: true,
            extension: "json".into(),
        });
        let long = format!("[{}]", "1,".repeat(super::MAX_HIGHLIGHTED_LINE));
        assert_eq!(h.highlight_line(&long).count(), 0);
        assert!(h.highlight_line("[1]").count() > 0);
    }

    #[test]
    fn every_language_has_its_own_grammar_and_unknown_ones_stay_plain() {
        assert_eq!(super::syntax_for("tsx").name, "TypeScriptReact");
        assert_eq!(super::syntax_for("ts").name, "TypeScript");
        assert_eq!(super::syntax_for("jsx").name, "TypeScriptReact");
        assert_eq!(super::syntax_for("scss").name, "SCSS");
        assert_eq!(super::syntax_for("kt").name, "Kotlin");
        assert_eq!(super::syntax_for("unknown").name, "Plain Text");
    }
}
