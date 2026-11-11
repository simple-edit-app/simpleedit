//! Code completion: what a language server proposes, as the editor uses it.
//!
//! The server knows the code (types, scopes, imports); this module must not
//! lose any of that. It reads the server's items exactly as the Language
//! Server Protocol defines them (edit ranges, snippets, edits elsewhere in the
//! file such as imports, defaults shared by the list), keeps or drops them by
//! what was typed since, orders them by the server's own ranking, and works
//! out the exact text an accepted item gives.
//!
//! Positions are (line, byte column) in the text the server was asked about.

use serde_json::Value;
use std::collections::HashMap;

/// What an item is, as the server says (LSP `CompletionItemKind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Text,
    Method,
    Function,
    Constructor,
    Field,
    Variable,
    Class,
    Interface,
    Module,
    Property,
    Unit,
    Value,
    Enum,
    Keyword,
    Snippet,
    Color,
    File,
    Reference,
    Folder,
    EnumMember,
    Constant,
    Struct,
    Event,
    Operator,
    TypeParameter,
}

impl Kind {
    fn from_lsp(n: u64) -> Option<Kind> {
        use Kind::*;
        Some(match n {
            1 => Text,
            2 => Method,
            3 => Function,
            4 => Constructor,
            5 => Field,
            6 => Variable,
            7 => Class,
            8 => Interface,
            9 => Module,
            10 => Property,
            11 => Unit,
            12 => Value,
            13 => Enum,
            14 => Keyword,
            15 => Snippet,
            16 => Color,
            17 => File,
            18 => Reference,
            19 => Folder,
            20 => EnumMember,
            21 => Constant,
            22 => Struct,
            23 => Event,
            24 => Operator,
            25 => TypeParameter,
            _ => return None,
        })
    }

    /// Its icon (a codicon of the Nerd Font) and the icon's color.
    pub fn icon(self) -> (char, (u8, u8, u8)) {
        use Kind::*;
        const PURPLE: (u8, u8, u8) = (0xb1, 0x80, 0xd7);
        const BLUE: (u8, u8, u8) = (0x75, 0xbe, 0xff);
        const ORANGE: (u8, u8, u8) = (0xee, 0x9d, 0x28);
        const GREY: (u8, u8, u8) = (0x9a, 0xa0, 0xad);
        let (code, color) = match self {
            Method | Function | Constructor => (0xea8c, PURPLE),
            Field => (0xeb5f, BLUE),
            Variable | Reference => (0xea88, BLUE),
            Class => (0xeb5b, ORANGE),
            Interface => (0xeb61, BLUE),
            Module => (0xea8b, GREY),
            Property => (0xeb65, GREY),
            Unit => (0xea96, GREY),
            Value => (0xea90, BLUE),
            Enum => (0xea95, ORANGE),
            EnumMember => (0xeb5e, BLUE),
            Keyword => (0xeb62, GREY),
            Snippet => (0xeb66, GREY),
            Color => (0xeb5c, GREY),
            File | Folder => (0xeb60, GREY),
            Constant => (0xeb5d, BLUE),
            Struct => (0xea91, ORANGE),
            Event => (0xea86, ORANGE),
            Operator => (0xeb64, GREY),
            TypeParameter => (0xea92, ORANGE),
            Text => (0xeb8d, GREY),
        };
        (char::from_u32(code).unwrap_or(' '), color)
    }
}

/// A place in the text: (line, byte column).
pub type Pos = (usize, usize);

/// A stretch of the text, start included, end excluded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Range {
    pub start: Pos,
    pub end: Pos,
}

/// A replacement of a stretch of the text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEdit {
    pub range: Range,
    pub text: String,
}

/// An item's documentation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Doc {
    pub text: String,
    pub markdown: bool,
}

/// One proposal of the server.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub label: String,
    /// Shown right after the label, dimmed (`(…)`, `(use std::fmt)`).
    pub label_detail: Option<String>,
    /// Shown on the right (a type, a signature, a module).
    pub description: Option<String>,
    pub kind: Option<Kind>,
    pub detail: Option<String>,
    pub doc: Option<Doc>,
    pub deprecated: bool,
    pub preselect: bool,
    pub sort_text: String,
    pub filter_text: String,
    /// The text it puts in (a snippet if `snippet`).
    pub new_text: String,
    pub snippet: bool,
    /// Indent the lines it adds like the line it goes on.
    pub adjust_indent: bool,
    /// What it replaces when accepted with Enter (insert) or Tab (replace);
    /// `None`: the word before the cursor.
    pub insert: Option<Range>,
    pub replace: Option<Range>,
    /// Edits elsewhere in the file (an import), made with it.
    pub additional: Vec<TextEdit>,
    pub command: Option<String>,
    /// The item as the server sent it, to ask it for the rest (`resolve`).
    pub raw: Value,
    /// The server has filled in the rest already.
    pub resolved: bool,
}

/// What the item still needs from the server before it is complete.
impl Item {
    pub fn can_resolve(&self) -> bool {
        !self.resolved && self.raw.get("data").is_some_and(|d| !d.is_null())
    }
}

// ─── Reading the server's answer ───────────────────────────────────────────

/// The byte column of UTF-16 column `utf16` in `line`.
fn byte_col(line: &str, utf16: usize) -> usize {
    let mut units = 0;
    for (i, c) in line.char_indices() {
        if units >= utf16 {
            return i;
        }
        units += c.len_utf16();
    }
    line.len()
}

fn pos_of(value: &Value, lines: &[&str]) -> Option<Pos> {
    let line = value.get("line")?.as_u64()? as usize;
    let character = value.get("character")?.as_u64()? as usize;
    let text = lines.get(line).copied().unwrap_or("");
    Some((line, byte_col(text, character)))
}

fn range_of(value: &Value, lines: &[&str]) -> Option<Range> {
    Some(Range {
        start: pos_of(value.get("start")?, lines)?,
        end: pos_of(value.get("end")?, lines)?,
    })
}

fn edits_of(value: Option<&Value>, lines: &[&str]) -> Vec<TextEdit> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|e| {
            Some(TextEdit {
                range: range_of(e.get("range")?, lines)?,
                text: e.get("newText")?.as_str()?.to_string(),
            })
        })
        .collect()
}

fn doc_of(value: Option<&Value>) -> Option<Doc> {
    match value? {
        Value::String(s) if !s.trim().is_empty() => Some(Doc {
            text: s.clone(),
            markdown: false,
        }),
        Value::Object(o) => {
            let text = o.get("value")?.as_str()?.to_string();
            (!text.trim().is_empty()).then(|| Doc {
                markdown: o.get("kind").and_then(Value::as_str) == Some("markdown"),
                text,
            })
        }
        _ => None,
    }
}

/// The text's lines, `\n` separated (a final empty line included).
fn lines_of(text: &str) -> Vec<&str> {
    text.split('\n').collect()
}

/// The items of an answer to `textDocument/completion` about `text`, and
/// whether the list is incomplete (to be asked for again as one types).
pub fn parse_list(value: &Value, text: &str) -> (Vec<Item>, bool) {
    let lines = lines_of(text);
    let (items, incomplete, defaults) = match value {
        Value::Array(items) => (items.as_slice(), false, None),
        Value::Object(list) => (
            list.get("items")
                .and_then(Value::as_array)
                .map_or(&[][..], Vec::as_slice),
            list.get("isIncomplete")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            list.get("itemDefaults"),
        ),
        _ => (&[][..], false, None),
    };
    let items = items
        .iter()
        .filter_map(|item| parse_item(item, defaults, &lines))
        .collect();
    (items, incomplete)
}

fn parse_item(value: &Value, defaults: Option<&Value>, lines: &[&str]) -> Option<Item> {
    let label = value.get("label")?.as_str()?.to_string();
    let get = |key: &str| value.get(key).filter(|v| !v.is_null());
    let default = |key: &str| defaults.and_then(|d| d.get(key)).filter(|v| !v.is_null());
    let string = |key: &str| get(key).and_then(Value::as_str).map(str::to_string);
    let format = get("insertTextFormat")
        .or_else(|| default("insertTextFormat"))
        .and_then(Value::as_u64);
    let mode = get("insertTextMode")
        .or_else(|| default("insertTextMode"))
        .and_then(Value::as_u64);

    // What it puts in and where: its own edit, the list's default range, or
    // the word before the cursor.
    let (new_text, insert, replace) = match get("textEdit") {
        Some(edit) => {
            let new_text = edit.get("newText")?.as_str()?.to_string();
            match edit.get("range") {
                Some(range) => {
                    let range = range_of(range, lines)?;
                    (new_text, Some(range), Some(range))
                }
                None => (
                    new_text,
                    Some(range_of(edit.get("insert")?, lines)?),
                    Some(range_of(edit.get("replace")?, lines)?),
                ),
            }
        }
        None => {
            let new_text = string("textEditText")
                .or_else(|| string("insertText"))
                .unwrap_or_else(|| label.clone());
            match default("editRange") {
                Some(range) if range.get("insert").is_some() => (
                    new_text,
                    range_of(&range["insert"], lines),
                    range_of(&range["replace"], lines),
                ),
                Some(range) => {
                    let range = range_of(range, lines);
                    (new_text, range, range)
                }
                None => (new_text, None, None),
            }
        }
    };

    let label_details = get("labelDetails");
    let label_detail = label_details
        .and_then(|d| d.get("detail"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let description = label_details
        .and_then(|d| d.get("description"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let deprecated = get("deprecated").and_then(Value::as_bool) == Some(true)
        || get("tags")
            .and_then(Value::as_array)
            .is_some_and(|t| t.iter().any(|t| t.as_u64() == Some(1)));

    // The list's default data belongs to every item: the server needs it
    // back to resolve one.
    let mut raw = value.clone();
    if get("data").is_none() {
        if let (Some(data), Some(object)) = (default("data"), raw.as_object_mut()) {
            object.insert("data".to_string(), data.clone());
        }
    }

    Some(Item {
        label_detail,
        description,
        kind: get("kind").and_then(Value::as_u64).and_then(Kind::from_lsp),
        detail: string("detail").filter(|d| !d.trim().is_empty()),
        doc: doc_of(get("documentation")),
        deprecated,
        preselect: get("preselect").and_then(Value::as_bool) == Some(true),
        sort_text: string("sortText").unwrap_or_else(|| label.clone()),
        filter_text: string("filterText").unwrap_or_else(|| label.clone()),
        new_text,
        snippet: format == Some(2),
        adjust_indent: mode != Some(1),
        insert,
        replace,
        additional: edits_of(get("additionalTextEdits"), lines),
        command: get("command")
            .and_then(|c| c.get("command"))
            .and_then(Value::as_str)
            .map(str::to_string),
        raw,
        resolved: false,
        label,
    })
}

/// Fills `item` in with what `completionItem/resolve` answered (about the
/// same `text` as the list).
pub fn merge_resolved(item: &mut Item, resolved: &Value, text: &str) {
    item.resolved = true;
    let lines = lines_of(text);
    if let Some(detail) = resolved
        .get("detail")
        .and_then(Value::as_str)
        .filter(|d| !d.trim().is_empty())
    {
        item.detail = Some(detail.to_string());
    }
    if let Some(doc) = doc_of(resolved.get("documentation")) {
        item.doc = Some(doc);
    }
    if resolved.get("additionalTextEdits").is_some() {
        item.additional = edits_of(resolved.get("additionalTextEdits"), &lines);
    }
    if let Some(edit) = resolved.get("textEdit").filter(|e| !e.is_null()) {
        if let Some(new_text) = edit.get("newText").and_then(Value::as_str) {
            let ranges = match edit.get("range") {
                Some(range) => range_of(range, &lines).map(|r| (r, r)),
                None => edit
                    .get("insert")
                    .and_then(|r| range_of(r, &lines))
                    .zip(edit.get("replace").and_then(|r| range_of(r, &lines))),
            };
            if let Some((insert, replace)) = ranges {
                item.new_text = new_text.to_string();
                item.insert = Some(insert);
                item.replace = Some(replace);
            }
        }
    }
    if let Some(command) = resolved
        .get("command")
        .and_then(|c| c.get("command"))
        .and_then(Value::as_str)
    {
        item.command = Some(command.to_string());
    }
}

// ─── The parameters of a call ──────────────────────────────────────────────

/// One way to call a function: its label, the byte ranges of its
/// parameters in the label, their documentation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signature {
    pub label: String,
    pub doc: Option<Doc>,
    pub params: Vec<(usize, usize)>,
    pub param_docs: Vec<Option<Doc>>,
    /// Its own active parameter, when the server says it per signature.
    pub active_param: Option<usize>,
}

/// What the server tells of a call: its signatures (several for an
/// overloaded function), the one that fits and the parameter being typed.
#[derive(Debug, Clone, PartialEq)]
pub struct Signatures {
    pub list: Vec<Signature>,
    pub active: usize,
    pub active_param: Option<usize>,
    /// The answer as it came: sent back while the help stays open.
    pub raw: Value,
}

impl Signatures {
    /// The parameter being typed in signature `index`.
    pub fn param_of(&self, index: usize) -> Option<usize> {
        let signature = self.list.get(index)?;
        signature
            .active_param
            .or(self.active_param)
            .filter(|p| *p < signature.params.len())
    }
}

/// Reads an answer to `textDocument/signatureHelp`; `None` when it holds no
/// signature.
pub fn parse_signatures(value: &Value) -> Option<Signatures> {
    let list: Vec<Signature> = value
        .get("signatures")?
        .as_array()?
        .iter()
        .filter_map(|s| {
            let label = s.get("label")?.as_str()?.to_string();
            let mut params = Vec::new();
            let mut param_docs = Vec::new();
            let mut from = 0;
            for p in s
                .get("parameters")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let range = match p.get("label") {
                    // The parameter's place in the label (UTF-16 offsets).
                    Some(Value::Array(ends)) => {
                        let at = |i: usize| {
                            ends.get(i)
                                .and_then(Value::as_u64)
                                .map(|u| byte_col(&label, u as usize))
                        };
                        at(0).zip(at(1))
                    }
                    // Its text, found in the label after the previous one.
                    Some(Value::String(text)) if !text.is_empty() => label
                        .get(from..)
                        .and_then(|rest| rest.find(text.as_str()))
                        .map(|i| (from + i, from + i + text.len())),
                    _ => None,
                };
                let range = range.unwrap_or((label.len(), label.len()));
                from = range.1.max(from);
                params.push(range);
                param_docs.push(doc_of(p.get("documentation")));
            }
            Some(Signature {
                doc: doc_of(s.get("documentation")),
                active_param: s
                    .get("activeParameter")
                    .and_then(Value::as_u64)
                    .map(|n| n as usize),
                label,
                params,
                param_docs,
            })
        })
        .collect();
    if list.is_empty() {
        return None;
    }
    let active = value
        .get("activeSignature")
        .and_then(Value::as_u64)
        .map_or(0, |n| n as usize)
        .min(list.len() - 1);
    Some(Signatures {
        active,
        active_param: value
            .get("activeParameter")
            .and_then(Value::as_u64)
            .map(|n| n as usize),
        list,
        raw: value.clone(),
    })
}

// ─── Matching what was typed ───────────────────────────────────────────────

/// How well what was typed matches an item: lower tiers first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Score {
    /// 0: starts with it, same case; 1: starts with it, other case; 2: its
    /// letters start the item's words (`fb` for `fooBar`, `gfb` for
    /// `get_foo_bar`) or follow each other there.
    pub tier: u8,
    /// How many of the matched letters have the same case.
    pub same_case: usize,
    /// The matched characters (indices of chars in the item's text).
    pub positions: Vec<usize>,
}

/// Whether the character at `i` starts a word of `w`: the first one, one
/// after a separator, an upper case letter after a lower case one or a
/// digit, the last capital of an acronym before a lower case letter, the
/// first digit of a number.
fn word_start(w: &[char], i: usize) -> bool {
    if i == 0 {
        return true;
    }
    let (prev, c) = (w[i - 1], w[i]);
    !prev.is_alphanumeric()
        || (c.is_uppercase() && (prev.is_lowercase() || prev.is_ascii_digit()))
        || (c.is_uppercase()
            && prev.is_uppercase()
            && w.get(i + 1).is_some_and(|n| n.is_lowercase()))
        || (c.is_ascii_digit() && !prev.is_ascii_digit())
}

fn same(a: char, b: char) -> bool {
    a == b || a.to_lowercase().eq(b.to_lowercase())
}

/// Whether `pattern` (what was typed) matches `word` (an item's filter
/// text), and how well. Every typed character must be found, in order; the
/// first one starts a word of `word`, each next one follows the previous
/// match or starts another word. Case is ignored for the match and counts
/// in the ranking.
pub fn fuzzy(pattern: &str, word: &str) -> Option<Score> {
    let p: Vec<char> = pattern.chars().collect();
    let w: Vec<char> = word.chars().collect();
    if p.len() > w.len() {
        return None;
    }
    let same_case = |positions: &[usize]| {
        positions
            .iter()
            .zip(&p)
            .filter(|(&i, &c)| w[i] == c)
            .count()
    };
    if p.iter().zip(&w).all(|(a, b)| same(*a, *b)) {
        let positions: Vec<usize> = (0..p.len()).collect();
        let exact = p.iter().zip(&w).all(|(a, b)| a == b);
        return Some(Score {
            tier: if exact { 0 } else { 1 },
            same_case: same_case(&positions),
            positions,
        });
    }
    // Depth first, preferring a match right after the previous one, then the
    // earliest word start; dead ends are remembered.
    fn search(
        p: &[char],
        w: &[char],
        pi: usize,
        from: usize,
        contiguous: bool,
        dead: &mut std::collections::HashSet<(usize, usize, bool)>,
        out: &mut Vec<usize>,
    ) -> bool {
        if pi == p.len() {
            return true;
        }
        if dead.contains(&(pi, from, contiguous)) {
            return false;
        }
        for wi in from..w.len() {
            if w.len() - wi < p.len() - pi {
                break;
            }
            let allowed = (contiguous && wi == from) || word_start(w, wi);
            if allowed && same(w[wi], p[pi]) {
                out.push(wi);
                if search(p, w, pi + 1, wi + 1, true, dead, out) {
                    return true;
                }
                out.pop();
            }
        }
        dead.insert((pi, from, contiguous));
        false
    }
    let mut positions = Vec::with_capacity(p.len());
    let mut dead = std::collections::HashSet::new();
    search(&p, &w, 0, 0, false, &mut dead, &mut positions).then(|| Score {
        tier: 2,
        same_case: same_case(&positions),
        positions,
    })
}

/// An item kept for what was typed, with its score.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ranked {
    pub index: usize,
    pub score: Score,
}

/// The characters a word is made of (what the cursor's word is, when the
/// server gives no range).
pub fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

/// The snippets of the user that begin like `prefix` (nothing typed: all), for
/// a file of this extension, as proposals.
pub fn snippet_items(
    snippets: &[crate::config::UserSnippet],
    ext: &str,
    prefix: &str,
) -> Vec<Item> {
    let wanted = prefix.to_lowercase();
    let list: Vec<Value> = snippets
        .iter()
        .filter(|s| {
            s.languages.is_empty() || s.languages.iter().any(|l| l.eq_ignore_ascii_case(ext))
        })
        .filter(|s| !s.trigger.is_empty() && s.trigger.to_lowercase().starts_with(&wanted))
        .map(|s| {
            serde_json::json!({
                "label": s.trigger,
                "kind": 15,
                "detail": "snippet",
                "insertText": s.body,
                "insertTextFormat": 2,
            })
        })
        .collect();
    parse_list(&Value::Array(list), "").0
}

/// The words of a text as proposals (no language server): each one once, the
/// short ones and the one being typed (`prefix`) left out.
pub fn buffer_words(text: &str, prefix: &str) -> Vec<Item> {
    const MAX_WORDS: usize = 5_000;
    let mut seen = std::collections::HashSet::new();
    let mut labels = Vec::new();
    for word in text.split(|c: char| !is_word_char(c)) {
        if word.chars().count() < 3
            || word == prefix
            || word.chars().next().is_some_and(|c| c.is_ascii_digit())
            || !seen.insert(word)
        {
            continue;
        }
        labels.push(serde_json::json!({ "label": word, "kind": 1 }));
        if labels.len() >= MAX_WORDS {
            break;
        }
    }
    parse_list(&Value::Array(labels), "").0
}

/// Where the word that ends at byte `col` of `line` starts.
pub fn word_start_col(line: &str, col: usize) -> usize {
    let col = col.min(line.len());
    line[..col]
        .char_indices()
        .rev()
        .take_while(|(_, c)| is_word_char(*c))
        .last()
        .map_or(col, |(i, _)| i)
}

/// Where the word that starts at byte `col` of `line` ends.
fn word_end_col(line: &str, col: usize) -> usize {
    let col = col.min(line.len());
    line[col..]
        .char_indices()
        .find(|(_, c)| !is_word_char(*c))
        .map_or(line.len(), |(i, _)| col + i)
}

/// The text the server was asked about and where the cursor was then.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Base {
    pub text: String,
    pub origin: Pos,
}

impl Base {
    /// Where the word being completed starts, when an item gives no range.
    pub fn word_start(&self) -> usize {
        let line = self.text.split('\n').nth(self.origin.0).unwrap_or("");
        word_start_col(line, self.origin.1)
    }

    /// A position of the asked text in the text of now, the cursor having
    /// gone from the origin to `caret` on the same line by typing there.
    /// The start of a range at the origin stays before what was typed; any
    /// other position at or after it moves with it.
    fn shift(&self, pos: Pos, caret: Pos, is_start: bool) -> Pos {
        let (line, col) = self.origin;
        if pos.0 != line || pos.1 < col || (pos.1 == col && is_start) {
            return pos;
        }
        let moved = pos.1 as isize + caret.1 as isize - col as isize;
        (pos.0, moved.max(0) as usize)
    }

    /// Whether `now` is the asked text with only some typing at the origin
    /// (characters added or removed there), the cursor now at `caret`.
    pub fn typed_since(&self, now: &str, caret: Pos) -> bool {
        let (line, col) = self.origin;
        if caret.0 != line {
            return false;
        }
        let delta = now.len() as isize - self.text.len() as isize;
        if delta != caret.1 as isize - col as isize {
            return false;
        }
        let start = line_start(&self.text, line) + col.min(caret.1);
        let now_start = line_start(now, line) + col.min(caret.1);
        let after_old = line_start(&self.text, line) + col;
        let after_now = line_start(now, line) + caret.1;
        self.text.get(..start) == now.get(..now_start)
            && self.text.get(after_old..) == now.get(after_now..)
    }
}

/// The byte offset where line `line` starts.
fn line_start(text: &str, line: usize) -> usize {
    if line == 0 {
        return 0;
    }
    text.match_indices('\n')
        .nth(line - 1)
        .map_or(text.len(), |(i, _)| i + 1)
}

fn offset(text: &str, pos: Pos) -> usize {
    let start = line_start(text, pos.0);
    let end = text[start..].find('\n').map_or(text.len(), |i| start + i);
    (start + pos.1).min(end)
}

fn pos_at(text: &str, offset: usize) -> Pos {
    let before = &text[..offset.min(text.len())];
    let line = before.matches('\n').count();
    let start = before.rfind('\n').map_or(0, |i| i + 1);
    (line, before.len() - start)
}

/// What has been typed for `item`: from where its range starts to the
/// cursor, on the cursor's line of now (`line`). `None` when the cursor is
/// before that start (or on another line): the item no longer applies.
/// `default_start` is where an item without a range starts.
fn typed_for<'l>(
    item: &Item,
    base: &Base,
    default_start: Pos,
    line: &'l str,
    caret: Pos,
) -> Option<&'l str> {
    let start = item
        .insert
        .or(item.replace)
        .map_or(default_start, |r| r.start);
    let start = base.shift(start, caret, true);
    if start.0 != caret.0 || start.1 > caret.1 {
        return None;
    }
    line.get(start.1..caret.1)
}

/// The items that match what was typed since `base` (the cursor now at
/// `caret`, on the line `line`), best first: by how they match, then by the
/// server's own order (`sortText`), then by name.
pub fn rank(items: &[Item], base: &Base, line: &str, caret: Pos) -> Vec<Ranked> {
    let default_start = (base.origin.0, base.word_start());
    let mut kept: Vec<Ranked> = items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            let typed = typed_for(item, base, default_start, line, caret)?;
            let score = fuzzy(typed, &item.filter_text)?;
            Some(Ranked { index, score })
        })
        .collect();
    kept.sort_by(|a, b| {
        let (x, y) = (&items[a.index], &items[b.index]);
        a.score
            .tier
            .cmp(&b.score.tier)
            .then(b.score.same_case.cmp(&a.score.same_case))
            .then_with(|| x.sort_text.cmp(&y.sort_text))
            .then_with(|| x.label.cmp(&y.label))
    });
    kept
}

/// Which of the ranked items to select first: one the server preselects, if
/// it matches as well as the best one; else the best one.
pub fn first_selected(items: &[Item], ranked: &[Ranked]) -> usize {
    let Some(best) = ranked.first() else {
        return 0;
    };
    ranked
        .iter()
        .take_while(|r| r.score.tier == best.score.tier)
        .position(|r| items[r.index].preselect)
        .unwrap_or(0)
}

// ─── Snippets ──────────────────────────────────────────────────────────────

/// A place to fill in a snippet: `$1`, `${1:default}`, `${1|a,b|}`, `$0`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stop {
    pub index: u32,
    /// Byte range of its text in the snippet's text.
    pub start: usize,
    pub end: usize,
    pub choices: Vec<String>,
    /// Markers read before it opened, and before it closed: a marker read
    /// in between is inside it.
    pub seq: usize,
    pub close: usize,
}

/// A snippet once its markers are read: the text it puts in, and the places
/// to fill, in the order the Tab key goes through them (`$0` last); a number
/// written several times is one place with copies, the first one written
/// before its copies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snippet {
    pub text: String,
    pub stops: Vec<Stop>,
}

struct SnippetParser<'a> {
    chars: Vec<char>,
    i: usize,
    out: String,
    stops: Vec<Stop>,
    variable: &'a dyn Fn(&str) -> Option<String>,
    /// Markers opened so far.
    opened: usize,
}

impl SnippetParser<'_> {
    fn open(&mut self) -> usize {
        self.opened += 1;
        self.opened - 1
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.i).copied()
    }

    fn number(&mut self) -> Option<u32> {
        let start = self.i;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.i += 1;
        }
        (self.i > start)
            .then(|| self.chars[start..self.i].iter().collect::<String>())
            .and_then(|s| s.parse().ok())
    }

    fn name(&mut self) -> Option<String> {
        let start = self.i;
        if !self
            .peek()
            .is_some_and(|c| c == '_' || c.is_ascii_alphabetic())
        {
            return None;
        }
        while self
            .peek()
            .is_some_and(|c| c == '_' || c.is_ascii_alphanumeric())
        {
            self.i += 1;
        }
        Some(self.chars[start..self.i].iter().collect())
    }

    /// Reads until `close` (not consumed) at this level; `}` ends a nested
    /// placeholder.
    fn parse_until(&mut self, nested: bool) {
        while let Some(c) = self.peek() {
            match c {
                '\\' => {
                    let next = self.chars.get(self.i + 1).copied();
                    match next {
                        Some(n @ ('$' | '}' | '\\')) => {
                            self.out.push(n);
                            self.i += 2;
                        }
                        _ => {
                            self.out.push('\\');
                            self.i += 1;
                        }
                    }
                }
                '}' if nested => return,
                '$' => {
                    let at = self.i;
                    if !self.dollar() {
                        // Not a marker: a plain dollar sign.
                        self.i = at + 1;
                        self.out.push('$');
                    }
                }
                _ => {
                    self.out.push(c);
                    self.i += 1;
                }
            }
        }
    }

    /// A marker after `$`; false if it isn't one (nothing consumed then).
    fn dollar(&mut self) -> bool {
        let at = self.i;
        self.i += 1;
        if let Some(index) = self.number() {
            let here = self.out.len();
            self.stop(index, here, here, Vec::new());
            return true;
        }
        if let Some(name) = self.name() {
            let value = (self.variable)(&name).unwrap_or_default();
            self.out.push_str(&value);
            return true;
        }
        if self.peek() != Some('{') {
            self.i = at;
            return false;
        }
        self.i += 1;
        if let Some(index) = self.number() {
            match self.peek() {
                Some('}') => {
                    self.i += 1;
                    let here = self.out.len();
                    self.stop(index, here, here, Vec::new());
                    return true;
                }
                Some(':') => {
                    self.i += 1;
                    let start = self.out.len();
                    let seq = self.open();
                    self.parse_until(true);
                    if self.peek() == Some('}') {
                        self.i += 1;
                    }
                    let end = self.out.len();
                    self.stop_at(seq, index, start, end, Vec::new());
                    return true;
                }
                Some('|') => {
                    self.i += 1;
                    let mut choices = vec![String::new()];
                    while let Some(c) = self.peek() {
                        self.i += 1;
                        match c {
                            '\\' => {
                                if let Some(n) = self.peek() {
                                    choices.last_mut().unwrap().push(n);
                                    self.i += 1;
                                }
                            }
                            ',' => choices.push(String::new()),
                            '|' if self.peek() == Some('}') => {
                                self.i += 1;
                                break;
                            }
                            _ => choices.last_mut().unwrap().push(c),
                        }
                    }
                    let start = self.out.len();
                    self.out.push_str(&choices[0]);
                    let end = self.out.len();
                    self.stop(index, start, end, choices);
                    return true;
                }
                _ => {
                    self.i = at;
                    return false;
                }
            }
        }
        if let Some(name) = self.name() {
            match self.peek() {
                Some('}') => {
                    self.i += 1;
                    let value = (self.variable)(&name).unwrap_or_default();
                    self.out.push_str(&value);
                    return true;
                }
                Some(':') => {
                    self.i += 1;
                    let start = self.out.len();
                    let stops = self.stops.len();
                    self.parse_until(true);
                    if self.peek() == Some('}') {
                        self.i += 1;
                    }
                    // A known variable replaces its default.
                    if let Some(value) = (self.variable)(&name) {
                        self.out.truncate(start);
                        self.stops.truncate(stops);
                        self.out.push_str(&value);
                    }
                    return true;
                }
                Some('/') => {
                    // A transform: the variable's value, untransformed.
                    let mut slashes = 0;
                    while let Some(c) = self.peek() {
                        self.i += 1;
                        match c {
                            '\\' => self.i += 1,
                            '/' => slashes += 1,
                            '}' if slashes >= 3 => break,
                            _ => {}
                        }
                    }
                    let value = (self.variable)(&name).unwrap_or_default();
                    self.out.push_str(&value);
                    return true;
                }
                _ => {}
            }
        }
        self.i = at;
        false
    }

    fn stop(&mut self, index: u32, start: usize, end: usize, choices: Vec<String>) {
        let seq = self.open();
        self.stop_at(seq, index, start, end, choices);
    }

    fn stop_at(&mut self, seq: usize, index: u32, start: usize, end: usize, choices: Vec<String>) {
        let close = self.opened;
        self.stops.push(Stop {
            index,
            start,
            end,
            choices,
            seq,
            close,
        });
    }
}

/// Reads a snippet; `variable` gives the value of `$TM_FILENAME` and the
/// like (`None`: unknown, its default or nothing is used).
pub fn parse_snippet(snippet: &str, variable: &dyn Fn(&str) -> Option<String>) -> Snippet {
    let mut parser = SnippetParser {
        chars: snippet.chars().collect(),
        i: 0,
        out: String::new(),
        stops: Vec::new(),
        variable,
        opened: 0,
    };
    parser.parse_until(false);
    let SnippetParser {
        mut out, mut stops, ..
    } = parser;
    // The copies of a number take the text of its place: the first one
    // written with a default, else the first one written.
    stops.sort_by_key(|s| s.seq);
    let mut primary: HashMap<u32, usize> = HashMap::new();
    for (i, s) in stops.iter().enumerate() {
        let better = primary
            .get(&s.index)
            .is_none_or(|&p| stops[p].start == stops[p].end && s.start < s.end);
        if better {
            primary.insert(s.index, i);
        }
    }
    let mut copies: Vec<usize> = (0..stops.len())
        .filter(|i| stops[*i].index != 0 && primary.get(&stops[*i].index) != Some(i))
        .collect();
    copies.sort_by_key(|&i| std::cmp::Reverse(stops[i].seq));
    for c in copies {
        let p = primary[&stops[c].index];
        let filled = out[stops[p].start..stops[p].end].to_string();
        let (at, end, seq) = (stops[c].start, stops[c].end, stops[c].seq);
        out.replace_range(at..end, &filled);
        let delta = filled.len() as isize - (end - at) as isize;
        let moved = |x: usize| (x as isize + delta) as usize;
        for (i, s) in stops.iter_mut().enumerate() {
            if i == c {
                continue;
            }
            if s.seq < seq && seq < s.close {
                // The copy is inside it.
                s.end = moved(s.end);
            } else if s.seq > seq {
                // Written after the copy.
                s.start = moved(s.start);
                s.end = moved(s.end);
            }
        }
        stops[c].end = at + filled.len();
    }
    // Tab goes through the places by number, `$0` (where the cursor ends)
    // last; each place's own text first, then its copies.
    let order: Vec<(u32, bool, usize)> = stops
        .iter()
        .enumerate()
        .map(|(i, s)| {
            (
                if s.index == 0 { u32::MAX } else { s.index },
                primary.get(&s.index) != Some(&i),
                s.seq,
            )
        })
        .collect();
    let mut sorted: Vec<(usize, Stop)> = stops.into_iter().enumerate().collect();
    sorted.sort_by_key(|(i, _)| order[*i]);
    Snippet {
        text: out,
        stops: sorted.into_iter().map(|(_, s)| s).collect(),
    }
}

/// The places of a snippet in Tab order, each with its copies after it.
pub fn groups(stops: &[Stop]) -> Vec<Vec<Stretch>> {
    let mut groups: Vec<(u32, Vec<Stretch>)> = Vec::new();
    for s in stops {
        match groups.last_mut() {
            Some((index, group)) if *index == s.index => group.push((s.start, s.end)),
            _ => groups.push((s.index, vec![(s.start, s.end)])),
        }
    }
    groups.into_iter().map(|(_, g)| g).collect()
}

// ─── Accepting an item ─────────────────────────────────────────────────────

/// The text once an item is accepted, and where the cursor goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub text: String,
    /// The cursor, or the start of the selected first place of a snippet.
    pub caret: Pos,
    /// Bytes selected from `caret` (the default text of a snippet's place).
    pub select: usize,
    /// The copies of that first place, which follow what is typed in it.
    pub copies: Vec<Stretch>,
    /// The other places of a snippet, in Tab order, each with its copies:
    /// byte ranges of `text`.
    pub stops: Vec<Vec<Stretch>>,
}

/// Works out what accepting `item` does to `now` (the text of now, the
/// cursor at `caret`): its text replaces the typed word (up to the cursor
/// for Enter, the whole word for Tab), its other edits (imports) are made
/// too, and a snippet's first place is selected.
pub fn plan(
    item: &Item,
    base: &Base,
    now: &str,
    caret: Pos,
    replace_mode: bool,
    variable: &dyn Fn(&str) -> Option<String>,
) -> Option<Plan> {
    if !base.typed_since(now, caret) {
        return None;
    }
    let base_line = base.text.split('\n').nth(base.origin.0).unwrap_or("");
    let range = match (replace_mode, item.insert, item.replace) {
        (true, _, Some(r)) | (false, Some(r), _) | (_, None, Some(r)) | (true, Some(r), None) => r,
        (_, None, None) => {
            let start = (base.origin.0, base.word_start());
            let end = if replace_mode {
                (base.origin.0, word_end_col(base_line, base.origin.1))
            } else {
                base.origin
            };
            Range { start, end }
        }
    };
    let start = base.shift(range.start, caret, true);
    let end = base.shift(range.end, caret, false);
    // Accepted with Enter, what is after the cursor stays.
    let end = if replace_mode || end.0 != caret.0 {
        end
    } else {
        (end.0, end.1.max(caret.1))
    };
    if (start.0, start.1) > (end.0, end.1) {
        return None;
    }

    // The text it puts in, its snippet's places, its lines indented.
    let (mut text, mut stops) = if item.snippet {
        let snippet = parse_snippet(&item.new_text, variable);
        let stops = groups(&snippet.stops);
        (snippet.text, stops)
    } else {
        (item.new_text.clone(), Vec::new())
    };
    if item.adjust_indent && text.contains('\n') {
        let line = now.split('\n').nth(start.0).unwrap_or("");
        let indent: String = line
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .collect();
        if !indent.is_empty() {
            let mut indented = String::with_capacity(text.len());
            let mut map = Vec::with_capacity(text.len() + 1);
            for c in text.chars() {
                for _ in 0..c.len_utf8() {
                    map.push(indented.len());
                }
                indented.push(c);
                if c == '\n' {
                    indented.push_str(&indent);
                }
            }
            map.push(indented.len());
            for stop in stops.iter_mut().flatten() {
                stop.0 = map[stop.0];
                stop.1 = map[stop.1];
            }
            text = indented;
        }
    }

    // Every edit, as byte offsets of the text of now; the other edits must
    // not touch the item's own.
    let main = (offset(now, start), offset(now, end));
    let mut edits: Vec<(usize, usize, String, bool)> = vec![(main.0, main.1, text.clone(), true)];
    for edit in &item.additional {
        let from = offset(now, base.shift(edit.range.start, caret, true));
        let to = offset(now, base.shift(edit.range.end, caret, false));
        let overlaps =
            from < main.1 && to > main.0 || (from == to && from > main.0 && from < main.1);
        if from <= to && !overlaps {
            edits.push((from, to, edit.text.clone(), false));
        }
    }
    // At the same place, an insertion of another edit goes before the item's.
    edits.sort_by_key(|e| (e.0, e.3));
    let mut result = String::with_capacity(now.len() + text.len());
    let mut at = 0;
    let mut main_start = 0;
    for (from, to, new, is_main) in &edits {
        if *from < at {
            continue;
        }
        result.push_str(&now[at..*from]);
        if *is_main {
            main_start = result.len();
        }
        result.push_str(new);
        at = *to;
    }
    result.push_str(&now[at..]);

    let mut stops: Vec<Vec<Stretch>> = stops
        .into_iter()
        .map(|g| {
            g.into_iter()
                .map(|(s, e)| (main_start + s, main_start + e))
                .collect()
        })
        .collect();
    let (caret, select, copies) = if stops.is_empty() {
        (pos_at(&result, main_start + text.len()), 0, Vec::new())
    } else {
        let mut first = stops.remove(0);
        let (s, e) = first.remove(0);
        (pos_at(&result, s), e - s, first)
    };
    Some(Plan {
        text: result,
        caret,
        select,
        copies,
        stops,
    })
}

/// A stretch of a text: byte offsets, start and end.
pub type Stretch = (usize, usize);

/// A snippet's places once an edit is followed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Followed {
    /// The text with the copies of the current place written again.
    pub text: String,
    /// The current place first, then its copies.
    pub current: Vec<Stretch>,
    pub stops: Vec<Vec<Stretch>>,
    /// The current place in `new`, before its copies were written again.
    pub typed: Stretch,
}

/// Moves the places of a snippet after an edit of the text (`old` became
/// `new`): places after the edit move with it, the one it happens in grows
/// or shrinks, and its copies get its new text. `None` when the edit is not
/// in the current place: the snippet is over.
pub fn follow_edit(
    current: &[Stretch],
    stops: &[Vec<Stretch>],
    old: &str,
    new: &str,
) -> Option<Followed> {
    let (&primary, _) = current.split_first()?;
    let prefix = old
        .bytes()
        .zip(new.bytes())
        .take_while(|(a, b)| a == b)
        .count();
    let max_suffix = old.len().min(new.len()) - prefix;
    let suffix = old
        .bytes()
        .rev()
        .zip(new.bytes().rev())
        .take(max_suffix)
        .take_while(|(a, b)| a == b)
        .count();
    let (from, to) = (prefix, old.len() - suffix);
    if old != new && (from < primary.0 || to > primary.1) {
        return None;
    }
    let delta = new.len() as isize - old.len() as isize;
    // Every place, flat: (group, its rank in it, range); group 0 is the
    // current one.
    let mut all: Vec<(usize, usize, Stretch)> = std::iter::once(current)
        .chain(stops.iter().map(Vec::as_slice))
        .enumerate()
        .flat_map(|(g, group)| group.iter().enumerate().map(move |(k, &r)| (g, k, r)))
        .collect();
    let shift = |r: &mut Stretch, at: usize, end: usize, d: isize| {
        let moved = |x: usize| (x as isize + d) as usize;
        if r.0 >= end && !(r.0 == at && at == end && r.1 == r.0 && d == 0) {
            *r = (moved(r.0), moved(r.1));
        } else if r.0 <= at && r.1 >= end {
            r.1 = moved(r.1);
        }
    };
    if old != new {
        for (_, _, r) in &mut all {
            shift(r, from, to, delta);
        }
    }
    // The copies of the current place take its text, last one first.
    let mut text = new.to_string();
    let main = all.iter().find(|(g, k, _)| *g == 0 && *k == 0)?.2;
    let typed = main;
    let written = text.get(main.0..main.1)?.to_string();
    let mut copies: Vec<usize> = (0..all.len())
        .filter(|&i| all[i].0 == 0 && all[i].1 > 0)
        .collect();
    copies.sort_by_key(|&i| std::cmp::Reverse(all[i].2 .0));
    for c in copies {
        let copy = all[c].2;
        if text.get(copy.0..copy.1) == Some(written.as_str()) {
            continue;
        }
        text.replace_range(copy.0..copy.1, &written);
        let d = written.len() as isize - (copy.1 - copy.0) as isize;
        for (i, entry) in all.iter_mut().enumerate() {
            if i != c {
                shift(&mut entry.2, copy.0, copy.1, d);
            }
        }
        all[c].2 .1 = copy.0 + written.len();
    }
    let mut groups: Vec<Vec<Stretch>> = vec![Vec::new(); stops.len() + 1];
    for (g, _, r) in all {
        groups[g].push(r);
    }
    let current = groups.remove(0);
    Some(Followed {
        text,
        current,
        stops: groups,
        typed,
    })
}

pub fn offset_of(text: &str, pos: Pos) -> usize {
    offset(text, pos)
}

pub fn pos_of_offset(text: &str, offset: usize) -> Pos {
    pos_at(text, offset)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn no_vars(_: &str) -> Option<String> {
        None
    }

    fn base(text: &str, origin: Pos) -> Base {
        Base {
            text: text.to_string(),
            origin,
        }
    }

    // ── Reading ──

    #[test]
    fn a_typescript_member_list_is_read_with_its_ranges() {
        // As typescript-language-server answers after `user.`.
        let text = "const user = 1;\nuser.\n";
        let answer = json!({ "isIncomplete": false, "items": [
            { "label": "age", "kind": 5, "sortText": "11", "data": { "cacheId": 1 },
              "filterText": ".age",
              "textEdit": { "range": { "start": { "line": 1, "character": 4 },
                                        "end": { "line": 1, "character": 5 } },
                            "newText": ".age" } },
        ]});
        let (items, incomplete) = parse_list(&answer, text);
        assert!(!incomplete);
        let item = &items[0];
        assert_eq!(item.kind, Some(Kind::Field));
        assert_eq!(item.filter_text, ".age");
        assert_eq!(
            item.insert,
            Some(Range {
                start: (1, 4),
                end: (1, 5)
            })
        );
        assert!(item.can_resolve());
    }

    #[test]
    fn rust_analyzer_items_keep_snippets_details_and_insert_replace_ranges() {
        let text = "fn f() {\n    u.na\n}\n";
        let answer = json!({ "isIncomplete": true, "items": [
            { "label": "greet(…)", "labelDetails": { "description": "fn(&self, bool) -> String" },
              "kind": 2, "preselect": true, "sortText": "7fffffff", "filterText": "greet",
              "insertTextFormat": 2,
              "textEdit": { "newText": "greet(${1:loud})$0",
                "insert": { "start": { "line": 1, "character": 6 }, "end": { "line": 1, "character": 8 } },
                "replace": { "start": { "line": 1, "character": 6 }, "end": { "line": 1, "character": 8 } } } },
        ]});
        let (items, incomplete) = parse_list(&answer, text);
        assert!(incomplete);
        let item = &items[0];
        assert!(item.snippet && item.preselect);
        assert_eq!(
            item.description.as_deref(),
            Some("fn(&self, bool) -> String")
        );
        assert_eq!(item.insert.unwrap().start, (1, 6));
    }

    #[test]
    fn the_list_defaults_apply_to_every_item() {
        let text = "ab\n";
        let answer = json!({ "isIncomplete": false,
            "itemDefaults": {
                "editRange": { "insert": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 2 } },
                               "replace": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 2 } } },
                "insertTextFormat": 2,
                "data": { "id": 7 } },
            "items": [ { "label": "abc", "textEditText": "abc($1)" }, { "label": "abd" } ] });
        let (items, _) = parse_list(&answer, text);
        assert_eq!(items[0].new_text, "abc($1)");
        assert!(items[0].snippet && items[1].snippet);
        assert_eq!(items[1].new_text, "abd");
        assert_eq!(items[1].insert.unwrap().end, (0, 2));
        assert_eq!(
            items[1].raw["data"]["id"], 7,
            "the default data goes back to the server"
        );
    }

    #[test]
    fn a_plain_array_and_utf16_columns() {
        // "é" is one UTF-16 unit and two bytes; "😀" two units and four bytes.
        let text = "é😀x\n";
        let answer = json!([{ "label": "xyz", "textEdit": {
            "range": { "start": { "line": 0, "character": 3 }, "end": { "line": 0, "character": 4 } },
            "newText": "xyz" } }]);
        let (items, incomplete) = parse_list(&answer, text);
        assert!(!incomplete);
        assert_eq!(
            items[0].insert,
            Some(Range {
                start: (0, 6),
                end: (0, 7)
            })
        );
    }

    #[test]
    fn deprecation_and_documentation_in_both_forms() {
        let text = "\n";
        let answer = json!([
            { "label": "a", "deprecated": true, "documentation": "plain" },
            { "label": "b", "tags": [1], "documentation": { "kind": "markdown", "value": "**md**" } },
            { "label": "c", "documentation": "  " },
        ]);
        let (items, _) = parse_list(&answer, text);
        assert!(items[0].deprecated && items[1].deprecated && !items[2].deprecated);
        assert_eq!(
            items[0].doc,
            Some(Doc {
                text: "plain".into(),
                markdown: false
            })
        );
        assert!(items[1].doc.as_ref().unwrap().markdown);
        assert_eq!(items[2].doc, None);
    }

    #[test]
    fn resolving_adds_the_detail_the_documentation_and_the_import() {
        let text = "formatD\n";
        let (mut items, _) = parse_list(
            &json!([{ "label": "formatDate", "data": { "cacheId": 9 } }]),
            text,
        );
        merge_resolved(
            &mut items[0],
            &json!({ "label": "formatDate",
                "detail": "Auto import from './b'",
                "documentation": { "kind": "markdown", "value": "Formats." },
                "additionalTextEdits": [ { "range": { "start": { "line": 0, "character": 0 },
                    "end": { "line": 0, "character": 0 } }, "newText": "import { formatDate } from \"./b\";\n\n" } ] }),
            text,
        );
        let item = &items[0];
        assert!(item.resolved && !item.can_resolve());
        assert_eq!(item.detail.as_deref(), Some("Auto import from './b'"));
        assert_eq!(item.additional.len(), 1);
    }

    // ── Parameters ──

    #[test]
    fn signatures_find_their_parameters_by_text_or_by_offsets() {
        // rust-analyzer gives offsets; "é" counts one UTF-16 unit, two bytes.
        let help = json!({ "signatures": [
            { "label": "fn greet(&self, é: bool, n: u32) -> String",
              "parameters": [ { "label": [9, 14] }, { "label": [16, 23] }, { "label": [25, 31] } ] }
        ], "activeSignature": 0, "activeParameter": 1 });
        let s = parse_signatures(&help).unwrap();
        let sig = &s.list[0];
        assert_eq!(&sig.label[sig.params[1].0..sig.params[1].1], "é: bool");
        assert_eq!(&sig.label[sig.params[2].0..sig.params[2].1], "n: u32");
        assert_eq!(s.param_of(0), Some(1));
        // TypeScript gives texts, and overloads with their own active one.
        let help = json!({ "signatures": [
            { "label": "f(a: string): void", "parameters": [ { "label": "a: string",
               "documentation": { "kind": "markdown", "value": "The **a**." } } ] },
            { "label": "f(a: number, b: number): void", "activeParameter": 1,
              "parameters": [ { "label": "a: number" }, { "label": "b: number" } ] }
        ], "activeSignature": 1, "activeParameter": 0 });
        let s = parse_signatures(&help).unwrap();
        assert_eq!(s.active, 1);
        assert_eq!(s.param_of(1), Some(1), "its own active parameter wins");
        assert_eq!(s.param_of(0), Some(0));
        assert!(s.list[0].param_docs[0].as_ref().unwrap().markdown);
        // A parameter beyond the signature's: none is marked.
        let help =
            json!({ "signatures": [ { "label": "g()", "parameters": [] } ], "activeParameter": 2 });
        assert_eq!(parse_signatures(&help).unwrap().param_of(0), None);
        assert!(parse_signatures(&json!({ "signatures": [] })).is_none());
        assert!(parse_signatures(&Value::Null).is_none());
    }

    // ── Matching ──

    #[test]
    fn a_prefix_ranks_before_a_match_of_word_starts() {
        assert_eq!(fuzzy("str", "String").unwrap().tier, 1);
        assert_eq!(fuzzy("Str", "String").unwrap().tier, 0);
        let humps = fuzzy("fb", "fooBar").unwrap();
        assert_eq!(humps.tier, 2);
        assert_eq!(humps.positions, vec![0, 3]);
        assert_eq!(
            fuzzy("gfb", "get_foo_bar").unwrap().positions,
            vec![0, 4, 8]
        );
        assert_eq!(fuzzy("strlen", "str_len").unwrap().tier, 2);
        assert_eq!(fuzzy("parser", "HTMLParser").unwrap().positions[0], 4);
        assert_eq!(fuzzy("", "anything").unwrap().tier, 0);
    }

    #[test]
    fn letters_in_the_middle_of_a_word_do_not_match() {
        // Nothing that would surprise: no letter found in the middle of a word.
        assert!(fuzzy("ma", "takelimit").is_none());
        assert!(fuzzy("ction", "function").is_none());
        assert!(fuzzy("xyz", "abc").is_none());
        assert!(fuzzy("long", "lon").is_none());
        // But a word that starts later does.
        assert!(fuzzy("map", "filter_map").is_some());
        assert!(fuzzy("ma", "max_by").is_some());
    }

    #[test]
    fn the_search_backtracks_to_a_later_word_start() {
        // A greedy choice of the first `b` would fail on `c`.
        let score = fuzzy("abc", "aB_bCd").unwrap();
        assert_eq!(score.positions, vec![0, 1, 4]);
    }

    fn item(label: &str, sort: &str) -> Item {
        Item {
            label: label.into(),
            label_detail: None,
            description: None,
            kind: None,
            detail: None,
            doc: None,
            deprecated: false,
            preselect: false,
            sort_text: sort.into(),
            filter_text: label.into(),
            new_text: label.into(),
            snippet: false,
            adjust_indent: true,
            insert: None,
            replace: None,
            additional: Vec::new(),
            command: None,
            raw: Value::Null,
            resolved: false,
        }
    }

    #[test]
    fn ranking_keeps_the_server_order_among_equal_matches() {
        let items = vec![
            item("max", "2"),
            item("map", "1"),
            item("filter_map", "0"),
            item("Max", "3"),
            item("take", "0"),
        ];
        let b = base("ma\n", (0, 2));
        let ranked = rank(&items, &b, "ma", (0, 2));
        let labels: Vec<&str> = ranked
            .iter()
            .map(|r| items[r.index].label.as_str())
            .collect();
        assert_eq!(labels, vec!["map", "max", "Max", "filter_map"]);
    }

    #[test]
    fn typing_after_the_request_filters_the_same_list() {
        let items = vec![item("name", "1"), item("age", "1"), item("nap", "2")];
        // Asked at `u.|`, then "na" typed.
        let b = base("u.\n", (0, 2));
        let ranked = rank(&items, &b, "u.na", (0, 4));
        let labels: Vec<&str> = ranked
            .iter()
            .map(|r| items[r.index].label.as_str())
            .collect();
        assert_eq!(labels, vec!["name", "nap"]);
        // Erasing back before where the word starts: nothing applies.
        assert!(rank(&items, &b, "u", (0, 1)).is_empty());
    }

    #[test]
    fn a_preselected_item_is_selected_if_it_matches_as_well() {
        let mut items = vec![item("abc", "1"), item("abd", "2")];
        items[1].preselect = true;
        let b = base("a\n", (0, 1));
        let ranked = rank(&items, &b, "a", (0, 1));
        assert_eq!(
            items[ranked[first_selected(&items, &ranked)].index].label,
            "abd"
        );
    }

    // ── Snippets ──

    #[test]
    fn snippet_places_choices_variables_and_escapes() {
        let s = parse_snippet("greet(${1:loud})$0", &no_vars);
        assert_eq!(s.text, "greet(loud)");
        let places: Vec<(u32, usize, usize)> =
            s.stops.iter().map(|s| (s.index, s.start, s.end)).collect();
        assert_eq!(places, vec![(1, 6, 10), (0, 11, 11)]);
        let s = parse_snippet("${2:b} ${1|x,y|} \\$5 \\} $ ${TM_FILENAME}", &|n| {
            (n == "TM_FILENAME").then(|| "a.rs".to_string())
        });
        assert_eq!(s.text, "b x $5 } $ a.rs");
        assert_eq!(s.stops[0].index, 1);
        assert_eq!(s.stops[0].choices, vec!["x", "y"]);
        assert_eq!(s.stops[1].index, 2);
        // Nested places; a repeated number is a copy with the place's text.
        let s = parse_snippet("${1:a ${2:b}} $1", &no_vars);
        assert_eq!(s.text, "a b a b");
        let places = groups(&s.stops);
        assert_eq!(places, vec![vec![(0, 3), (4, 7)], vec![(2, 3)]]);
        // An unknown variable gives its default.
        assert_eq!(parse_snippet("${NOPE:dflt}x", &no_vars).text, "dfltx");
    }

    // ── Accepting ──

    #[test]
    fn enter_puts_the_text_in_and_keeps_what_follows() {
        // TypeScript: `s.toU|pper` with insert and replace ranges.
        let text = "s.toUpper\n";
        let mut it = item("toUpperCase", "1");
        it.insert = Some(Range {
            start: (0, 2),
            end: (0, 5),
        });
        it.replace = Some(Range {
            start: (0, 2),
            end: (0, 9),
        });
        let b = base(text, (0, 5));
        let insert = plan(&it, &b, text, (0, 5), false, &no_vars).unwrap();
        assert_eq!(insert.text, "s.toUpperCasepper\n");
        assert_eq!(insert.caret, (0, 13));
        let replace = plan(&it, &b, text, (0, 5), true, &no_vars).unwrap();
        assert_eq!(replace.text, "s.toUpperCase\n");
    }

    #[test]
    fn characters_typed_while_the_server_answered_are_replaced_too() {
        // Asked at `u.|` (rust-analyzer's empty range), "na" typed since.
        let mut it = item("name", "1");
        it.insert = Some(Range {
            start: (0, 2),
            end: (0, 2),
        });
        it.replace = it.insert;
        let b = base("u.\n", (0, 2));
        let done = plan(&it, &b, "u.na\n", (0, 4), false, &no_vars).unwrap();
        assert_eq!(done.text, "u.name\n");
        assert_eq!(done.caret, (0, 6));
    }

    #[test]
    fn without_a_range_the_word_before_the_cursor_is_replaced() {
        let it = item("println", "1");
        let b = base("    pri\n", (0, 7));
        let done = plan(&it, &b, "    print\n", (0, 9), false, &no_vars).unwrap();
        assert_eq!(done.text, "    println\n");
        // Tab replaces the rest of the word too.
        let b = base("    prixyz\n", (0, 7));
        let done = plan(&it, &b, "    prixyz\n", (0, 7), true, &no_vars).unwrap();
        assert_eq!(done.text, "    println\n");
    }

    #[test]
    fn a_snippet_selects_its_first_place_and_keeps_the_others() {
        let mut it = item("greet(…)", "1");
        it.new_text = "greet(${1:loud}, ${2:n})$0".into();
        it.snippet = true;
        it.insert = Some(Range {
            start: (1, 6),
            end: (1, 8),
        });
        it.replace = it.insert;
        let text = "fn f() {\n    u.gr\n}\n";
        let b = base(text, (1, 8));
        let done = plan(&it, &b, text, (1, 8), false, &no_vars).unwrap();
        assert_eq!(done.text, "fn f() {\n    u.greet(loud, n)\n}\n");
        assert_eq!(done.caret, (1, 12));
        assert_eq!(done.select, 4);
        // `n`, then the end.
        let n = done.text.find(", n").unwrap() + 2;
        assert_eq!(done.stops, vec![vec![(n, n + 1)], vec![(n + 2, n + 2)]]);
    }

    #[test]
    fn an_import_at_the_top_is_made_with_the_item() {
        let text = "const x = 1;\nformatD\n";
        let mut it = item("formatDate", "1");
        it.insert = Some(Range {
            start: (1, 0),
            end: (1, 7),
        });
        it.replace = it.insert;
        it.additional = vec![TextEdit {
            range: Range {
                start: (0, 0),
                end: (0, 0),
            },
            text: "import { formatDate } from \"./b\";\n\n".into(),
        }];
        let b = base(text, (1, 7));
        let done = plan(&it, &b, text, (1, 7), false, &no_vars).unwrap();
        assert_eq!(
            done.text,
            "import { formatDate } from \"./b\";\n\nconst x = 1;\nformatDate\n"
        );
        // The cursor is after the word, three lines down now.
        assert_eq!(done.caret, (3, 10));
    }

    #[test]
    fn a_postfix_item_removes_what_is_before_on_the_same_line() {
        // rust-analyzer's `u.ref` → `&u`: the receiver goes with an
        // additional edit, the main one puts `&u` in.
        let text = "    u.re\n";
        let mut it = item("ref", "1");
        it.new_text = "&u".into();
        it.insert = Some(Range {
            start: (0, 6),
            end: (0, 8),
        });
        it.replace = it.insert;
        it.additional = vec![TextEdit {
            range: Range {
                start: (0, 4),
                end: (0, 6),
            },
            text: String::new(),
        }];
        let b = base(text, (0, 8));
        let done = plan(&it, &b, text, (0, 8), false, &no_vars).unwrap();
        assert_eq!(done.text, "    &u\n");
        assert_eq!(done.caret, (0, 6));
    }

    #[test]
    fn the_lines_of_a_snippet_follow_the_indentation() {
        let mut it = item("if", "1");
        it.new_text = "if ${1:cond} {\n\t$0\n}".into();
        it.snippet = true;
        let text = "    i\n";
        let b = base(text, (0, 5));
        let done = plan(&it, &b, text, (0, 5), false, &no_vars).unwrap();
        assert_eq!(done.text, "    if cond {\n    \t\n    }\n");
        assert_eq!(done.caret, (0, 7));
        assert_eq!(done.select, 4);
        let end = done.text.find("\t\n").unwrap() + 1;
        assert_eq!(done.stops, vec![vec![(end, end)]]);
    }

    #[test]
    fn an_unrelated_change_since_the_request_refuses_the_plan() {
        let it = item("name", "1");
        let b = base("ab\ncd\n", (1, 2));
        // The first line changed: positions can't be trusted.
        assert!(plan(&it, &b, "xb\ncd\n", (1, 2), false, &no_vars).is_none());
    }

    #[test]
    fn snippet_places_follow_the_typing() {
        // `greet(loud, n)` with `loud` (6..10) the current place.
        let old = "greet(loud, n)";
        let stops = vec![vec![(12, 13)], vec![(14, 14)]];
        let f = follow_edit(&[(6, 10)], &stops, old, "greet(true, n)").unwrap();
        assert_eq!(f.current, vec![(6, 10)]);
        assert_eq!(f.stops, stops);
        let f = follow_edit(&[(6, 10)], &stops, old, "greet(x, n)").unwrap();
        assert_eq!(f.current, vec![(6, 7)]);
        assert_eq!(f.stops, vec![vec![(9, 10)], vec![(11, 11)]]);
        // An edit outside the current place ends the snippet.
        assert!(follow_edit(&[(6, 10)], &stops, old, "xgreet(loud, n)").is_none());
    }

    #[test]
    fn the_copies_of_a_place_get_what_is_typed_in_it() {
        // `${1:i} < ${1}` with a later place: typing in the first `i`.
        let s = parse_snippet("for ${1:i} < $1 { $2 }$0", &no_vars);
        assert_eq!(s.text, "for i < i {  }");
        let mut places = groups(&s.stops);
        let current = places.remove(0);
        assert_eq!(current, vec![(4, 5), (8, 9)]);
        // `i` selected and replaced by `idx`.
        let f = follow_edit(&current, &places, &s.text, "for idx < i {  }").unwrap();
        assert_eq!(f.text, "for idx < idx {  }");
        assert_eq!(f.current, vec![(4, 7), (10, 13)]);
        // The later places moved by both changes.
        assert_eq!(f.stops, vec![vec![(16, 16)], vec![(18, 18)]]);
        // Erased to nothing: the copy empties too.
        let f2 = follow_edit(&f.current, &f.stops, &f.text, "for  < idx {  }").unwrap();
        assert_eq!(f2.text, "for  <  {  }");
    }

    #[test]
    fn word_boundaries() {
        assert_eq!(word_start_col("  foo_bar", 9), 2);
        assert_eq!(word_start_col("a.b", 3), 2);
        assert_eq!(word_start_col("a.", 2), 2);
        assert_eq!(word_end_col("abc def", 1), 3);
        assert_eq!(word_start_col("été", "été".len()), 0);
    }
}
