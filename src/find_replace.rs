//! Find and replace in files, as IntelliJ IDEA has it: what is looked for
//! (case, whole words, regular expression), where (a file mask, a search
//! context: in or except comments and string literals), and what a match is
//! replaced by (`$1`, `${name}`, `\l \u \L \U \E`, keeping the case).
//!
//! Positions are byte offsets in a file's text.

use regex::{Captures, Regex};

/// Where in the code a match may be.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Context {
    #[default]
    Anywhere,
    InComments,
    InStrings,
    ExceptComments,
    ExceptStrings,
    ExceptBoth,
}

impl Context {
    pub const ALL: [Context; 6] = [
        Context::Anywhere,
        Context::InComments,
        Context::InStrings,
        Context::ExceptComments,
        Context::ExceptStrings,
        Context::ExceptBoth,
    ];

    /// Whether a match in `region` (none: plain code) is kept.
    pub fn keeps(self, region: Option<Region>) -> bool {
        match self {
            Context::Anywhere => true,
            Context::InComments => region == Some(Region::Comment),
            Context::InStrings => region == Some(Region::Str),
            Context::ExceptComments => region != Some(Region::Comment),
            Context::ExceptStrings => region != Some(Region::Str),
            Context::ExceptBoth => region.is_none(),
        }
    }
}

/// A part of the code that is not code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Region {
    Comment,
    Str,
}

/// How a language writes comments and strings.
struct Syntax {
    line: &'static [&'static str],
    block: &'static [(&'static str, &'static str)],
    /// Delimiters of strings, longest first (`"""` before `"`).
    strings: &'static [&'static str],
    /// Strings that go on past the end of a line.
    multiline: &'static [&'static str],
    /// `\` escapes the next character in strings.
    escapes: bool,
    /// Rust: raw strings `r#"…"#`, and `'` is a lifetime unless it closes
    /// a character.
    rust: bool,
    /// HTML, XML: strings only inside tags.
    markup: bool,
}

const C_LIKE: Syntax = Syntax {
    line: &["//"],
    block: &[("/*", "*/")],
    strings: &["\"", "'"],
    multiline: &[],
    escapes: true,
    rust: false,
    markup: false,
};

fn syntax_for(ext: &str) -> Option<Syntax> {
    Some(match ext {
        "rs" => Syntax {
            strings: &["\""],
            multiline: &["\""],
            rust: true,
            ..C_LIKE
        },
        "js" | "mjs" | "cjs" | "jsx" | "ts" | "mts" | "cts" | "tsx" | "vue" | "svelte" => Syntax {
            strings: &["`", "\"", "'"],
            multiline: &["`"],
            ..C_LIKE
        },
        "go" => Syntax {
            strings: &["`", "\"", "'"],
            multiline: &["`"],
            ..C_LIKE
        },
        "c" | "h" | "cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx" | "cs" | "java" | "kt" | "kts"
        | "swift" | "dart" | "scala" | "php" | "groovy" | "gradle" | "zig" => C_LIKE,
        "css" | "scss" | "less" => Syntax {
            line: if ext == "css" { &[] } else { &["//"] },
            ..C_LIKE
        },
        "json" | "jsonc" | "json5" => Syntax {
            strings: &["\""],
            ..C_LIKE
        },
        "py" | "pyi" | "pyw" => Syntax {
            line: &["#"],
            block: &[],
            strings: &["\"\"\"", "'''", "\"", "'"],
            multiline: &["\"\"\"", "'''"],
            ..C_LIKE
        },
        "sh" | "bash" | "zsh" | "fish" | "rb" | "pl" | "pm" | "r" | "yaml" | "yml" | "toml"
        | "mk" | "dockerfile" | "conf" | "nix" | "ex" | "exs" | "jl" | "tf" => Syntax {
            line: &["#"],
            block: &[],
            strings: &["\"", "'"],
            multiline: &[],
            ..C_LIKE
        },
        "ini" | "cfg" | "properties" => Syntax {
            line: &["#", ";"],
            block: &[],
            strings: &[],
            ..C_LIKE
        },
        "sql" => Syntax {
            line: &["--"],
            strings: &["'", "\""],
            ..C_LIKE
        },
        "lua" => Syntax {
            line: &["--"],
            block: &[("--[[", "]]")],
            strings: &["\"", "'"],
            ..C_LIKE
        },
        "hs" => Syntax {
            line: &["--"],
            block: &[("{-", "-}")],
            strings: &["\""],
            ..C_LIKE
        },
        "html" | "htm" | "xml" | "xhtml" | "svg" | "plist" | "xsd" | "xsl" => Syntax {
            line: &[],
            block: &[("<!--", "-->")],
            strings: &["\"", "'"],
            multiline: &["\"", "'"],
            escapes: false,
            rust: false,
            markup: true,
        },
        _ => return None,
    })
}

/// The comments and string literals of `text`, a file of extension `ext`,
/// as byte ranges in order. A language the editor doesn't know has none.
pub fn regions(ext: &str, text: &str) -> Vec<(usize, usize, Region)> {
    let Some(syntax) = syntax_for(&ext.to_ascii_lowercase()) else {
        return Vec::new();
    };
    let bytes = text.as_bytes();
    let at = |i: usize, s: &str| bytes[i..].starts_with(s.as_bytes());
    let mut found = Vec::new();
    let mut in_tag = false;
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if syntax.markup {
            if c == b'<' && !at(i, "<!--") {
                in_tag = true;
            } else if c == b'>' {
                in_tag = false;
            }
        }
        if let Some(start) = syntax.line.iter().find(|s| at(i, s)) {
            let _ = start;
            let end = text[i..].find('\n').map_or(bytes.len(), |n| i + n);
            found.push((i, end, Region::Comment));
            i = end;
            continue;
        }
        if let Some((open, close)) = syntax.block.iter().find(|(o, _)| at(i, o)) {
            let from = i + open.len();
            let end = text[from..]
                .find(close)
                .map_or(bytes.len(), |n| from + n + close.len());
            found.push((i, end, Region::Comment));
            i = end;
            continue;
        }
        // Rust: raw strings, and a `'` that is a character, not a lifetime.
        if syntax.rust {
            if c == b'r' && (i == 0 || !is_ident(bytes[i - 1])) {
                let hashes = bytes[i + 1..].iter().take_while(|b| **b == b'#').count();
                if bytes.get(i + 1 + hashes) == Some(&b'"') {
                    let close = format!("\"{}", "#".repeat(hashes));
                    let from = i + 2 + hashes;
                    let end = text[from..]
                        .find(&close)
                        .map_or(bytes.len(), |n| from + n + close.len());
                    found.push((i, end, Region::Str));
                    i = end;
                    continue;
                }
            }
            if c == b'\'' {
                let rest = &text[i + 1..];
                let mut chars = rest.char_indices();
                let close = match chars.next() {
                    Some((_, '\\')) => rest[1..].find('\'').map(|n| n + 2),
                    Some((_, ch)) => {
                        let after = ch.len_utf8();
                        (rest[after..].starts_with('\'')).then_some(after + 1)
                    }
                    None => None,
                };
                if let Some(len) = close.filter(|l| *l <= 12) {
                    found.push((i, i + 1 + len, Region::Str));
                    i += 1 + len;
                } else {
                    i += 1;
                }
                continue;
            }
        }
        if !syntax.markup || in_tag {
            if let Some(delim) = syntax.strings.iter().find(|s| at(i, s)) {
                let multiline = syntax.multiline.contains(delim);
                let mut j = i + delim.len();
                let end = loop {
                    if j >= bytes.len() {
                        break bytes.len();
                    }
                    if syntax.escapes && bytes[j] == b'\\' {
                        j += 2;
                        continue;
                    }
                    if at(j, delim) {
                        break j + delim.len();
                    }
                    if bytes[j] == b'\n' && !multiline {
                        break j;
                    }
                    j += 1;
                };
                found.push((i, end.min(bytes.len()), Region::Str));
                i = end.min(bytes.len()).max(i + 1);
                continue;
            }
        }
        i += 1;
    }
    found
}

fn is_ident(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// The region `offset` is in, if any (`regions` in order).
pub fn region_at(regions: &[(usize, usize, Region)], offset: usize) -> Option<Region> {
    let i = regions.partition_point(|r| r.1 <= offset);
    regions
        .get(i)
        .filter(|r| r.0 <= offset && offset < r.1)
        .map(|r| r.2)
}

// ─── What is looked for ────────────────────────────────────────────────────

/// What is looked for and how.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Options {
    pub query: String,
    pub case_sensitive: bool,
    pub words: bool,
    pub regex: bool,
    pub context: Context,
    /// File mask (`*.js, !test*.js`), when used.
    pub mask: Option<String>,
}

/// The expression for `options`: the text as it is (or the regular
/// expression), between word boundaries for whole words; `^` and `$` at
/// the ends of lines.
pub fn compile(options: &Options) -> Result<Regex, String> {
    let body = if options.regex {
        options.query.clone()
    } else {
        regex::escape(&options.query)
    };
    let body = if options.words {
        format!(r"\b(?:{body})\b")
    } else {
        body
    };
    regex::RegexBuilder::new(&body)
        .case_insensitive(!options.case_sensitive)
        .multi_line(true)
        .crlf(true)
        .build()
        .map_err(|e| e.to_string())
}

fn glob(pattern: &[u8], name: &[u8]) -> bool {
    match pattern.split_first() {
        None => name.is_empty(),
        Some((b'*', rest)) => (0..=name.len()).any(|i| glob(rest, &name[i..])),
        Some((b'?', rest)) => !name.is_empty() && glob(rest, &name[1..]),
        Some((c, rest)) => {
            name.first().is_some_and(|n| n.eq_ignore_ascii_case(c)) && glob(rest, &name[1..])
        }
    }
}

/// Whether a file name passes a mask: comma separated patterns (`*`, `?`);
/// one starting with `!` leaves out what it matches. Only exclusions: every
/// other file passes.
pub fn mask_matches(mask: &str, name: &str) -> bool {
    let patterns: Vec<&str> = mask
        .split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    if patterns.is_empty() {
        return true;
    }
    let (out, keep): (Vec<&str>, Vec<&str>) = patterns.iter().partition(|p| p.starts_with('!'));
    let matches = |p: &str| glob(p.as_bytes(), name.as_bytes());
    if out.iter().any(|p| matches(&p[1..])) {
        return false;
    }
    keep.is_empty() || keep.iter().any(|p| matches(p))
}

// ─── Replacing ─────────────────────────────────────────────────────────────

/// The text a match is replaced by: with a regular expression, `$0`, `$1`,
/// `${name}` give the groups, `\n` and `\t` a new line and a tab, `\x` the
/// character x, and `\l \u` change the case of the next character, `\L \U`
/// of what follows until `\E`. Without one, the text as written.
pub fn expand(caps: &Captures, template: &str, regex: bool) -> String {
    if !regex {
        return template.to_string();
    }
    #[derive(Clone, Copy, PartialEq)]
    enum Case {
        Lower,
        Upper,
    }
    let mut out = String::new();
    let mut span: Option<Case> = None;
    let mut next: Option<Case> = None;
    let push = |out: &mut String, s: &str, next: &mut Option<Case>, span: Option<Case>| {
        for c in s.chars() {
            let case = next.take().or(span);
            match case {
                Some(Case::Lower) => out.extend(c.to_lowercase()),
                Some(Case::Upper) => out.extend(c.to_uppercase()),
                None => out.push(c),
            }
        }
    };
    let chars: Vec<char> = template.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '\\' if i + 1 < chars.len() => {
                i += 1;
                match chars[i] {
                    'l' => next = Some(Case::Lower),
                    'u' => next = Some(Case::Upper),
                    'L' => span = Some(Case::Lower),
                    'U' => span = Some(Case::Upper),
                    'E' => span = None,
                    'n' => push(&mut out, "\n", &mut next, span),
                    't' => push(&mut out, "\t", &mut next, span),
                    c => push(&mut out, &c.to_string(), &mut next, span),
                }
                i += 1;
            }
            '$' if i + 1 < chars.len() && chars[i + 1] == '{' => {
                let close = chars[i + 2..].iter().position(|c| *c == '}');
                match close {
                    Some(n) => {
                        let name: String = chars[i + 2..i + 2 + n].iter().collect();
                        let value = name
                            .parse::<usize>()
                            .ok()
                            .and_then(|k| caps.get(k))
                            .or_else(|| caps.name(&name))
                            .map_or("", |m| m.as_str());
                        push(&mut out, value, &mut next, span);
                        i += 3 + n;
                    }
                    None => {
                        push(&mut out, "$", &mut next, span);
                        i += 1;
                    }
                }
            }
            '$' if i + 1 < chars.len() && chars[i + 1].is_ascii_digit() => {
                // As many digits as make a group that exists.
                let mut j = i + 1;
                let mut group = None;
                while j < chars.len() && chars[j].is_ascii_digit() {
                    let n: String = chars[i + 1..=j].iter().collect();
                    match n.parse::<usize>() {
                        Ok(k) if k < caps.len() => {
                            group = Some((k, j));
                            j += 1;
                        }
                        _ => break,
                    }
                }
                match group {
                    Some((k, last)) => {
                        let value = caps.get(k).map_or("", |m| m.as_str());
                        push(&mut out, value, &mut next, span);
                        i = last + 1;
                    }
                    None => {
                        push(&mut out, "$", &mut next, span);
                        i += 1;
                    }
                }
            }
            c => {
                push(&mut out, &c.to_string(), &mut next, span);
                i += 1;
            }
        }
    }
    out
}

/// The replacement written in the case of what it replaces: all capitals,
/// a capital first, or a small letter first.
pub fn preserve_case(found: &str, replacement: &str) -> String {
    let letters: Vec<char> = found.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.is_empty() {
        return replacement.to_string();
    }
    if letters.len() > 1 && letters.iter().all(|c| c.is_uppercase()) {
        return replacement.to_uppercase();
    }
    let mut chars = replacement.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    let rest: String = chars.collect();
    if letters[0].is_uppercase() {
        first.to_uppercase().collect::<String>() + &rest
    } else {
        first.to_lowercase().collect::<String>() + &rest
    }
}

/// What a replacement is made with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replace {
    pub template: String,
    pub preserve_case: bool,
}

/// A match in a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// Byte offsets in the text.
    pub start: usize,
    pub end: usize,
    /// From 0; the column in bytes.
    pub line: usize,
    pub col: usize,
    /// What it would be replaced by, when replacing.
    pub replacement: Option<String>,
}

/// Every match of `re` in `text` (a file of extension `ext`) in `context`,
/// with what each would be replaced by.
pub fn find_in(
    text: &str,
    ext: &str,
    re: &Regex,
    context: Context,
    regex: bool,
    replace: Option<&Replace>,
) -> Vec<Found> {
    let regions = if context == Context::Anywhere {
        Vec::new()
    } else {
        regions(ext, text)
    };
    let mut line = 0;
    let mut line_start = 0;
    let mut counted = 0;
    let mut found = Vec::new();
    for caps in re.captures_iter(text) {
        let m = caps.get(0).expect("the whole match");
        // An empty match is only worth something to replace (`^` → `// `).
        if m.start() == m.end() && !(regex && replace.is_some()) {
            continue;
        }
        if !context.keeps(region_at(&regions, m.start())) {
            continue;
        }
        // Lines counted from the last match on.
        for (i, b) in text.as_bytes()[counted..m.start()].iter().enumerate() {
            if *b == b'\n' {
                line += 1;
                line_start = counted + i + 1;
            }
        }
        counted = m.start();
        let replacement = replace.map(|r| {
            let new = expand(&caps, &r.template, regex);
            if r.preserve_case {
                preserve_case(m.as_str(), &new)
            } else {
                new
            }
        });
        found.push(Found {
            start: m.start(),
            end: m.end(),
            line,
            col: m.start() - line_start,
            replacement,
        });
    }
    found
}

/// `text` with the matches replaced (in order, not overlapping).
pub fn apply(text: &str, found: &[Found]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for f in found {
        let Some(new) = &f.replacement else {
            continue;
        };
        if f.start < at {
            continue;
        }
        out.push_str(&text[at..f.start]);
        out.push_str(new);
        at = f.end;
    }
    out.push_str(&text[at..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dollar_matches_before_a_crlf() {
        let re = compile(&Options {
            query: "foo(.*)$".into(),
            regex: true,
            ..Options::default()
        })
        .unwrap();
        let caps = re.captures("a foo!\r\nb").unwrap();
        assert_eq!(&caps[1], "!");
    }

    fn kinds(ext: &str, text: &str) -> Vec<(String, Region)> {
        regions(ext, text)
            .into_iter()
            .map(|(a, b, r)| (text[a..b].to_string(), r))
            .collect()
    }

    #[test]
    fn comments_and_strings_of_c_like_languages() {
        let text = "let a = \"x // no\"; // yes\n/* b\n */ f('c', `t\nu`);";
        assert_eq!(
            kinds("ts", text),
            vec![
                ("\"x // no\"".to_string(), Region::Str),
                ("// yes".to_string(), Region::Comment),
                ("/* b\n */".to_string(), Region::Comment),
                ("'c'".to_string(), Region::Str),
                ("`t\nu`".to_string(), Region::Str),
            ]
        );
        // An escaped quote stays in the string.
        assert_eq!(kinds("js", r#""a\"b" x"#)[0].0, r#""a\"b""#);
    }

    #[test]
    fn rust_raw_strings_characters_and_lifetimes() {
        let text = "fn f<'a>(x: &'a str) { let c = 'z'; let r = r#\"say \"hi\"\"#; // end\n}";
        let found = kinds("rs", text);
        assert_eq!(
            found,
            vec![
                ("'z'".to_string(), Region::Str),
                ("r#\"say \"hi\"\"#".to_string(), Region::Str),
                ("// end".to_string(), Region::Comment),
            ]
        );
    }

    #[test]
    fn python_docstrings_and_hash_comments() {
        let text = "\"\"\"doc\n# not\"\"\"\nx = 'a' # c";
        assert_eq!(
            kinds("py", text),
            vec![
                ("\"\"\"doc\n# not\"\"\"".to_string(), Region::Str),
                ("'a'".to_string(), Region::Str),
                ("# c".to_string(), Region::Comment),
            ]
        );
    }

    #[test]
    fn markup_strings_only_in_tags() {
        let text = "<a href=\"x\">it's \"text\"</a><!-- c -->";
        assert_eq!(
            kinds("html", text),
            vec![
                ("\"x\"".to_string(), Region::Str),
                ("<!-- c -->".to_string(), Region::Comment),
            ]
        );
        assert!(regions("unknown", "// x").is_empty());
    }

    fn options(query: &str) -> Options {
        Options {
            query: query.into(),
            ..Options::default()
        }
    }

    fn found_texts(text: &str, ext: &str, o: &Options) -> Vec<String> {
        let re = compile(o).unwrap();
        find_in(text, ext, &re, o.context, o.regex, None)
            .iter()
            .map(|f| text[f.start..f.end].to_string())
            .collect()
    }

    #[test]
    fn search_contexts_keep_or_leave_out_comments_and_strings() {
        let text = "foo(); // foo\nlet s = \"foo\";";
        let mut o = options("foo");
        assert_eq!(found_texts(text, "js", &o).len(), 3);
        o.context = Context::InComments;
        assert_eq!(found_texts(text, "js", &o).len(), 1);
        o.context = Context::InStrings;
        assert_eq!(found_texts(text, "js", &o).len(), 1);
        o.context = Context::ExceptComments;
        assert_eq!(found_texts(text, "js", &o).len(), 2);
        o.context = Context::ExceptStrings;
        assert_eq!(found_texts(text, "js", &o).len(), 2);
        o.context = Context::ExceptBoth;
        let re = compile(&o).unwrap();
        let found = find_in(text, "js", &re, o.context, false, None);
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].line, found[0].col), (0, 0));
    }

    #[test]
    fn case_words_regex_and_lines() {
        let text = "Foo foo food\nfoo";
        let mut o = options("foo");
        assert_eq!(found_texts(text, "txt", &o).len(), 4);
        o.case_sensitive = true;
        assert_eq!(found_texts(text, "txt", &o).len(), 3);
        o.words = true;
        assert_eq!(found_texts(text, "txt", &o), vec!["foo", "foo"]);
        let re = compile(&o).unwrap();
        let last = find_in(text, "txt", &re, Context::Anywhere, false, None);
        assert_eq!((last[1].line, last[1].col), (1, 0));
        // A regular expression across lines; a plain text with its specials.
        let o = Options {
            query: r"food\nfoo".into(),
            regex: true,
            ..Options::default()
        };
        assert_eq!(found_texts(text, "txt", &o), vec!["food\nfoo"]);
        assert_eq!(found_texts("a.b axb", "txt", &options("a.b")), vec!["a.b"]);
        assert!(compile(&Options {
            query: "(".into(),
            regex: true,
            ..Options::default()
        })
        .is_err());
    }

    #[test]
    fn file_masks() {
        assert!(mask_matches("", "a.rs"));
        assert!(mask_matches("*.js", "app.js"));
        assert!(!mask_matches("*.js", "app.ts"));
        assert!(mask_matches("*.js,????.ts", "main.ts"));
        assert!(!mask_matches("*.js,!test*.js", "test_a.js"));
        assert!(mask_matches("*.js,!test*.js", "a.js"));
        assert!(
            mask_matches("!*.min.js", "a.ts"),
            "only exclusions: the rest passes"
        );
        assert!(mask_matches("*.JS", "a.js"), "case does not matter");
    }

    fn replaced(text: &str, query: &str, template: &str, regex: bool, preserve: bool) -> String {
        let o = Options {
            query: query.into(),
            regex,
            ..Options::default()
        };
        let re = compile(&o).unwrap();
        let replace = Replace {
            template: template.into(),
            preserve_case: preserve,
        };
        apply(
            text,
            &find_in(text, "txt", &re, Context::Anywhere, regex, Some(&replace)),
        )
    }

    #[test]
    fn an_empty_match_can_be_replaced_with_a_regex() {
        assert_eq!(replaced("a\nb", "^", "// ", true, false), "// a\n// b");
        assert_eq!(replaced("a\nb", "$", ";", true, false), "a;\nb;");
        // Not as a plain text, and not when only searching.
        assert_eq!(replaced("a", "", "x", false, false), "a");
        assert!(found_texts(
            "a\nb",
            "txt",
            &Options {
                query: "^".into(),
                regex: true,
                ..Options::default()
            }
        )
        .is_empty());
    }

    #[test]
    fn replacements_with_groups_and_case_changes() {
        assert_eq!(
            replaced(
                r#" title="Hi" />"#,
                r#"\stitle="(.*)?"\s*(/>*)"#,
                "$2<title>$1</title>",
                true,
                false
            ),
            "/><title>Hi</title>"
        );
        assert_eq!(
            replaced("ab", "(?<x>a)(b)", "${x}-$2-$0", true, false),
            "a-b-ab"
        );
        assert_eq!(replaced("bar", "(bar)", r"\u$1", true, false), "Bar");
        assert_eq!(replaced("Bar", "(Bar)", r"\l$1", true, false), "bar");
        assert_eq!(
            replaced("bar baz", "(bar) (baz)", r"\U$1\E $2", true, false),
            "BAR baz"
        );
        assert_eq!(replaced("BAR", "(BAR)", r"\L$1", true, false), "bar");
        assert_eq!(replaced("a", "a", r"x\ny\$1\\", true, false), "x\ny$1\\");
        // `$12` when there is no twelfth group: group 1, then a 2.
        assert_eq!(replaced("q", "(q)", "$12", true, false), "q2");
        // Without a regular expression, the replacement is as written.
        assert_eq!(replaced("a.b", "a.b", "$1\\n", false, false), "$1\\n");
    }

    #[test]
    fn preserving_the_case() {
        assert_eq!(
            replaced("foo Foo FOO", "foo", "bar", false, true),
            "bar Bar BAR"
        );
        assert_eq!(preserve_case("x", "Bar"), "bar");
        assert_eq!(preserve_case("42", "bar"), "bar");
    }
}
