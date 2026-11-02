//! Code folding: which blocks of a file can be folded, and which are.
//!
//! A [`Region`] is a header line followed by lines that fold away with it: a
//! bracketed block (`{ … }`, `[ … ]`, `( … )`), a block or run of comments, an
//! indented block, a Markdown section or a fenced code block, depending on
//! the language (see [`Kind`]). [`Regions`] finds them in a text; [`Folds`]
//! remembers which headers are folded.

use std::collections::BTreeSet;
use std::ops::{Deref, Range};

/// Beyond this many lines a file isn't scanned for regions: the scan would
/// run after every edit.
pub const MAX_LINES: usize = 100_000;

/// What a language folds on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Kind {
    /// `{ }`, `[ ]` and `( )` blocks.
    brackets: bool,
    /// Blocks of more indented lines.
    indent: bool,
    /// `#` sections and ``` fences.
    markdown: bool,
    /// Starts a comment that runs to the end of the line.
    line_comment: &'static str,
    /// (start, end) of comments (or strings) that may span lines.
    blocks: &'static [(&'static str, &'static str)],
}

const C_BLOCK: &[(&str, &str)] = &[("/*", "*/")];
const HTML_BLOCK: &[(&str, &str)] = &[("<!--", "-->")];
const PY_BLOCK: &[(&str, &str)] = &[("\"\"\"", "\"\"\""), ("'''", "'''")];

impl Kind {
    /// The kind for a language token (the file extension, see `editor`);
    /// `None` is plain text.
    pub fn of(language: Option<&str>) -> Self {
        let code = |line_comment, blocks| Kind {
            brackets: true,
            indent: false,
            markdown: false,
            line_comment,
            blocks,
        };
        match language {
            None => Kind {
                brackets: false,
                indent: true,
                markdown: false,
                line_comment: "",
                blocks: &[],
            },
            Some("json") => code("", &[]),
            Some("md") => Kind {
                brackets: false,
                indent: false,
                markdown: true,
                line_comment: "",
                blocks: HTML_BLOCK,
            },
            Some("html" | "xml") => Kind {
                brackets: false,
                indent: true,
                markdown: false,
                line_comment: "",
                blocks: HTML_BLOCK,
            },
            Some("css") => code("", C_BLOCK),
            Some("py") => Kind {
                indent: true,
                ..code("#", PY_BLOCK)
            },
            Some("yaml") => Kind {
                brackets: false,
                indent: true,
                ..code("#", &[])
            },
            Some("rb" | "make") => Kind {
                indent: true,
                ..code("#", &[])
            },
            Some("sh" | "r" | "toml" | "pl") => code("#", &[]),
            Some("sql" | "lua") => code("--", &[]),
            Some("hs") => Kind {
                indent: true,
                ..code("--", &[])
            },
            Some("tex") => code("%", &[]),
            Some(_) => code("//", C_BLOCK),
        }
    }
}

/// A block that can be folded: `start` stays visible, the lines after it up
/// to `end` (included) are hidden.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region {
    pub start: usize,
    pub end: usize,
}

impl Region {
    /// The lines folding hides.
    pub fn hidden(self) -> Range<usize> {
        self.start + 1..self.end + 1
    }
}

/// The foldable blocks of a text, outermost first, one per header line.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Regions {
    items: Vec<Region>,
    /// The index of the block each one sits in.
    parents: Vec<Option<usize>>,
}

/// How far a fold click reaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Just this block.
    One,
    /// This block and the others at its level.
    Siblings,
    /// This block and everything nested in it.
    Subtree,
}

impl Regions {
    pub fn find<L: Deref<Target = str>>(lines: impl Iterator<Item = L>, kind: Kind) -> Self {
        let lines: Vec<L> = lines.collect();
        let mut found = Vec::new();
        if kind.brackets || !kind.blocks.is_empty() || !kind.line_comment.is_empty() {
            scan_code(&lines, kind, &mut found);
        }
        if !kind.line_comment.is_empty() {
            comment_runs(&lines, kind.line_comment, &mut found);
        }
        if kind.indent {
            by_indent(&lines, &mut found);
        }
        if kind.markdown {
            markdown(&lines, &mut found);
        }
        Self::from_candidates(found)
    }

    fn from_candidates(mut found: Vec<Region>) -> Self {
        // One block per header: the longest.
        found.retain(|r| r.end > r.start);
        found.sort_by_key(|r| (r.start, std::cmp::Reverse(r.end)));
        found.dedup_by_key(|r| r.start);

        let mut parents = Vec::with_capacity(found.len());
        let mut open: Vec<usize> = Vec::new();
        for (i, region) in found.iter().enumerate() {
            while open
                .last()
                .is_some_and(|&top| found[top].end < region.start)
            {
                let _ = open.pop();
            }
            parents.push(open.last().copied());
            open.push(i);
        }
        Regions {
            items: found,
            parents,
        }
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The index of the block headed by `line`.
    pub fn at(&self, line: usize) -> Option<usize> {
        self.items.binary_search_by_key(&line, |r| r.start).ok()
    }

    pub fn region(&self, index: usize) -> Region {
        self.items[index]
    }

    /// The innermost block `line` is in: its header, one of its hidden lines
    /// or the line closing it.
    pub fn innermost(&self, line: usize) -> Option<usize> {
        let upto = self.items.partition_point(|r| r.start <= line);
        (0..upto).rev().find(|&i| line <= self.items[i].end + 1)
    }

    fn siblings(&self, index: usize) -> Vec<usize> {
        (0..self.items.len())
            .filter(|&i| self.parents[i] == self.parents[index])
            .collect()
    }

    fn subtree(&self, index: usize) -> Vec<usize> {
        let mut out = vec![index];
        for i in index + 1..self.items.len() {
            if self.items[i].start > self.items[index].end {
                break;
            }
            if self.is_under(i, index) {
                out.push(i);
            }
        }
        out
    }

    fn is_under(&self, mut index: usize, ancestor: usize) -> bool {
        while let Some(parent) = self.parents[index] {
            if parent == ancestor {
                return true;
            }
            index = parent;
        }
        false
    }
}

/// Brackets and block comments, which need the state of the whole text: a
/// comment or a bracket may stay open across lines.
fn scan_code<L: Deref<Target = str>>(lines: &[L], kind: Kind, found: &mut Vec<Region>) {
    let mut open: Vec<(char, usize)> = Vec::new();
    // The end marker and the line a multi-line block started on.
    let mut block: Option<(&str, usize)> = None;
    for (n, line) in lines.iter().enumerate() {
        let line: &str = line;
        let mut i = 0;
        if let Some((end, from)) = block {
            match line.find(end) {
                Some(at) => {
                    found.push(Region {
                        start: from,
                        end: n,
                    });
                    block = None;
                    i = at + end.len();
                }
                None => continue,
            }
        }
        'line: while i < line.len() {
            let rest = &line[i..];
            let Some(c) = rest.chars().next() else { break };
            if !kind.line_comment.is_empty() && rest.starts_with(kind.line_comment) {
                break;
            }
            for (start, end) in kind.blocks {
                if rest.starts_with(start) {
                    let after = i + start.len();
                    match line[after..].find(end) {
                        Some(at) => {
                            i = after + at + end.len();
                            continue 'line;
                        }
                        None => {
                            block = Some((end, n));
                            break 'line;
                        }
                    }
                }
            }
            i += c.len_utf8();
            match c {
                '"' => i = end_of_string(line, i),
                '\'' => i = end_of_char(line, i),
                '{' | '[' | '(' if kind.brackets => open.push((c, n)),
                '}' | ']' | ')' if kind.brackets => {
                    let opener = match c {
                        '}' => '{',
                        ']' => '[',
                        _ => '(',
                    };
                    if let Some(depth) = open.iter().rposition(|(o, _)| *o == opener) {
                        let (_, from) = open[depth];
                        open.truncate(depth);
                        // The closing line stays visible.
                        found.push(Region {
                            start: from,
                            end: n.saturating_sub(1),
                        });
                    }
                }
                _ => {}
            }
        }
    }
}

/// Where a string literal that started before `from` ends: after its closing
/// quote, or at the end of the line.
fn end_of_string(line: &str, from: usize) -> usize {
    let mut escaped = false;
    for (at, c) in line[from..].char_indices() {
        match c {
            _ if escaped => escaped = false,
            '\\' => escaped = true,
            '"' => return from + at + 1,
            _ => {}
        }
    }
    line.len()
}

/// Where a character literal (`'{'`, `'\n'`) that started before `from`
/// ends; a lone apostrophe (a lifetime, a contraction) skips nothing.
fn end_of_char(line: &str, from: usize) -> usize {
    let mut chars = line[from..].char_indices();
    let end = match chars.next() {
        Some((_, '\\')) => chars.nth(1),
        Some(_) => chars.next(),
        None => None,
    };
    match end {
        Some((at, '\'')) => from + at + 1,
        _ => from,
    }
}

/// Two or more comment lines in a row.
fn comment_runs<L: Deref<Target = str>>(lines: &[L], marker: &str, found: &mut Vec<Region>) {
    let mut run_start: Option<usize> = None;
    for n in 0..=lines.len() {
        let is_comment = lines
            .get(n)
            .is_some_and(|l| l.trim_start().starts_with(marker));
        match (is_comment, run_start) {
            (true, None) => run_start = Some(n),
            (false, Some(start)) => {
                found.push(Region { start, end: n - 1 });
                run_start = None;
            }
            _ => {}
        }
    }
}

/// The width of a line's indentation (a tab counts for 4), or `None` for a
/// blank line.
fn indent_of(line: &str) -> Option<usize> {
    let mut width = 0;
    for c in line.chars() {
        match c {
            ' ' => width += 1,
            '\t' => width += 4,
            c if c.is_whitespace() => {}
            _ => return Some(width),
        }
    }
    None
}

/// A line followed by more indented ones.
fn by_indent<L: Deref<Target = str>>(lines: &[L], found: &mut Vec<Region>) {
    let mut open: Vec<(usize, usize)> = Vec::new();
    let mut last_text = 0;
    let close = |open: &mut Vec<(usize, usize)>,
                 below: usize,
                 last_text: usize,
                 found: &mut Vec<Region>| {
        while let Some(&(start, width)) = open.last() {
            if width < below {
                break;
            }
            let _ = open.pop();
            found.push(Region {
                start,
                end: last_text,
            });
        }
    };
    for (n, line) in lines.iter().enumerate() {
        let Some(width) = indent_of(line) else {
            continue;
        };
        close(&mut open, width, last_text, found);
        open.push((n, width));
        last_text = n;
    }
    close(&mut open, 0, last_text, found);
}

/// `#` sections (up to the next heading of the same or a higher level) and
/// fenced code blocks.
fn markdown<L: Deref<Target = str>>(lines: &[L], found: &mut Vec<Region>) {
    let mut sections: Vec<(usize, usize)> = Vec::new();
    let mut fence: Option<usize> = None;
    let mut last_text = 0;
    let close = |sections: &mut Vec<(usize, usize)>,
                 level: usize,
                 last_text: usize,
                 found: &mut Vec<Region>| {
        while let Some(&(start, open_level)) = sections.last() {
            if open_level < level {
                break;
            }
            let _ = sections.pop();
            found.push(Region {
                start,
                end: last_text,
            });
        }
    };
    for (n, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            match fence.take() {
                Some(start) => found.push(Region { start, end: n }),
                None => fence = Some(n),
            }
        } else if fence.is_none() {
            let level = trimmed.chars().take_while(|&c| c == '#').count();
            if (1..=6).contains(&level) && trimmed[level..].starts_with(' ') {
                close(&mut sections, level, last_text, found);
                sections.push((n, level));
            }
        }
        if !line.trim().is_empty() {
            last_text = n;
        }
    }
    close(&mut sections, 0, last_text, found);
}

/// The folded blocks, by header line.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Folds {
    folded: BTreeSet<usize>,
}

impl Folds {
    pub fn is_empty(&self) -> bool {
        self.folded.is_empty()
    }

    pub fn is_folded(&self, line: usize) -> bool {
        self.folded.contains(&line)
    }

    /// The lines folds hide, sorted and merged.
    pub fn hidden(&self, regions: &Regions) -> Vec<Range<usize>> {
        let mut out: Vec<Range<usize>> = Vec::new();
        for &line in &self.folded {
            let Some(i) = regions.at(line) else { continue };
            let range = regions.region(i).hidden();
            match out.last_mut() {
                Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
                _ => out.push(range),
            }
        }
        out
    }

    /// Forgets the folds that no longer head a block.
    pub fn prune(&mut self, regions: &Regions) {
        self.folded.retain(|&line| regions.at(line).is_some());
    }

    /// Moves the folds below `top` after `delta` lines were added (or, when
    /// negative, removed) under it.
    pub fn shift(&mut self, top: usize, delta: isize) {
        if delta == 0 || self.folded.is_empty() {
            return;
        }
        self.folded = self
            .folded
            .iter()
            .filter_map(|&line| {
                if line <= top {
                    Some(line)
                } else {
                    let moved = line as isize + delta;
                    (moved > top as isize).then_some(moved as usize)
                }
            })
            .collect();
    }

    /// Folds the block `index`, or unfolds it when it is folded, along with
    /// the blocks `scope` adds.
    pub fn toggle(&mut self, regions: &Regions, index: usize, scope: Scope) {
        let fold = !self.is_folded(regions.region(index).start);
        let targets = match scope {
            Scope::One => vec![index],
            Scope::Siblings => regions.siblings(index),
            Scope::Subtree => regions.subtree(index),
        };
        for target in targets {
            let line = regions.region(target).start;
            if fold {
                let _ = self.folded.insert(line);
            } else {
                let _ = self.folded.remove(&line);
            }
        }
    }

    /// Folds every block, or unfolds them all.
    pub fn set_all(&mut self, regions: &Regions, fold: bool) {
        self.folded.clear();
        if fold {
            self.folded
                .extend((0..regions.items.len()).map(|i| regions.region(i).start));
        }
    }

    /// Unfolds what hides `line`; whether something was.
    pub fn reveal(&mut self, regions: &Regions, line: usize) -> bool {
        let before = self.folded.len();
        self.folded.retain(|&header| {
            regions
                .at(header)
                .is_none_or(|i| !regions.region(i).hidden().contains(&line))
        });
        self.folded.len() != before
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find(text: &str, language: Option<&str>) -> Regions {
        Regions::find(text.lines(), Kind::of(language))
    }

    fn spans(regions: &Regions) -> Vec<(usize, usize)> {
        (0..regions.items.len())
            .map(|i| (regions.region(i).start, regions.region(i).end))
            .collect()
    }

    #[test]
    fn a_bracket_block_folds_to_the_line_before_its_closer() {
        let r = find("fn f() {\n    a;\n    b;\n}\n", Some("rs"));
        assert_eq!(spans(&r), vec![(0, 2)]);
    }

    #[test]
    fn a_block_of_two_lines_has_nothing_to_fold() {
        assert!(find("{\n}\n", Some("json")).is_empty());
        assert!(find("fn f() {}\n", Some("rs")).is_empty());
    }

    #[test]
    fn json_objects_and_arrays_nest() {
        let r = find(
            "{\n  \"a\": [\n    1,\n    2\n  ],\n  \"b\": {\n    \"c\": 1\n  }\n}\n",
            Some("json"),
        );
        assert_eq!(spans(&r), vec![(0, 7), (1, 3), (5, 6)]);
        assert_eq!(r.parents, vec![None, Some(0), Some(0)]);
    }

    #[test]
    fn brackets_in_strings_chars_and_comments_are_ignored() {
        let text = "let s = \"{\";\nlet c = '{';\n// {\nfn f<'a>() {\n    x;\n}\n";
        assert_eq!(spans(&find(text, Some("rs"))), vec![(3, 4)]);
    }

    #[test]
    fn an_else_line_closes_one_block_and_opens_the_next() {
        let r = find("if a {\n  x;\n  y;\n} else {\n  z;\n  w;\n}\n", Some("rs"));
        assert_eq!(spans(&r), vec![(0, 2), (3, 5)]);
        assert_eq!(r.parents, vec![None, None]);
    }

    #[test]
    fn block_comments_fold_down_to_their_end() {
        let r = find("/**\n * doc\n */\nfn f() {}\n", Some("rs"));
        assert_eq!(spans(&r), vec![(0, 2)]);
    }

    #[test]
    fn a_run_of_line_comments_folds() {
        let r = find("// a\n// b\n// c\ncode();\n// lone\n", Some("rs"));
        assert_eq!(spans(&r), vec![(0, 2)]);
    }

    #[test]
    fn python_folds_by_indentation_and_docstrings() {
        let text = "def f(x):\n    \"\"\"doc\n    more\n    \"\"\"\n    if x:\n        return 1\n    return 2\n\nprint(f(1))\n";
        let r = find(text, Some("py"));
        assert_eq!(spans(&r), vec![(0, 6), (1, 3), (4, 5)]);
    }

    #[test]
    fn yaml_and_plain_text_fold_by_indentation() {
        let r = find("a:\n  b: 1\n  c:\n    d: 2\ne: 3\n", Some("yaml"));
        assert_eq!(spans(&r), vec![(0, 3), (2, 3)]);
        assert_eq!(spans(&find("title\n  one\n  two\n", None)), vec![(0, 2)]);
    }

    #[test]
    fn markdown_sections_and_fences_fold() {
        let text = "# A\ntext\n## B\nmore\n\n# C\n```rust\nfn x() {}\n```\n";
        let r = find(text, Some("md"));
        assert_eq!(spans(&r), vec![(0, 3), (2, 3), (5, 8), (6, 8)]);
    }

    #[test]
    fn a_hash_in_a_markdown_fence_is_not_a_heading() {
        let r = find("```\n# not\ntext\n```\n", Some("md"));
        assert_eq!(spans(&r), vec![(0, 3)]);
    }

    #[test]
    fn innermost_finds_the_block_around_a_line() {
        let r = find(
            "{\n  \"a\": [\n    1,\n    2\n  ],\n  \"b\": 1\n}\n",
            Some("json"),
        );
        assert_eq!(r.innermost(2), Some(1));
        assert_eq!(r.innermost(5), Some(0));
        assert_eq!(r.innermost(6), Some(0));
    }

    fn folds_of(lines: &[usize]) -> Folds {
        Folds {
            folded: lines.iter().copied().collect(),
        }
    }

    #[test]
    fn folded_blocks_hide_their_lines_and_merge() {
        let r = find(
            "{\n  \"a\": [\n    1,\n    2\n  ],\n  \"b\": 1\n}\n",
            Some("json"),
        );
        assert_eq!(folds_of(&[1]).hidden(&r), vec![2..4]);
        assert_eq!(folds_of(&[0, 1]).hidden(&r), vec![1..6]);
    }

    #[test]
    fn toggling_reaches_siblings_or_the_whole_subtree() {
        let text = "{\n  \"a\": {\n    \"x\": [\n      1,\n      2\n    ],\n    \"y\": 1\n  },\n  \"b\": {\n    \"z\": 1,\n    \"w\": 2\n  }\n}\n";
        let r = find(text, Some("json"));
        assert_eq!(spans(&r), vec![(0, 11), (1, 6), (2, 4), (8, 10)]);

        let mut f = Folds::default();
        f.toggle(&r, 1, Scope::Siblings);
        assert_eq!(f.folded, [1, 8].into());
        f.toggle(&r, 1, Scope::Siblings);
        assert!(f.is_empty());

        f.toggle(&r, 1, Scope::Subtree);
        assert_eq!(f.folded, [1, 2].into());
        f.toggle(&r, 1, Scope::Subtree);
        assert!(f.is_empty());

        f.toggle(&r, 2, Scope::One);
        assert_eq!(f.folded, [2].into());
    }

    #[test]
    fn fold_all_and_unfold_all() {
        let r = find(
            "{\n  \"a\": [\n    1,\n    2\n  ],\n  \"b\": 1\n}\n",
            Some("json"),
        );
        let mut f = Folds::default();
        f.set_all(&r, true);
        assert_eq!(f.folded, [0, 1].into());
        f.set_all(&r, false);
        assert!(f.is_empty());
    }

    #[test]
    fn folds_follow_the_lines_added_or_removed_above_them() {
        let mut f = folds_of(&[2, 10]);
        f.shift(5, 3);
        assert_eq!(f.folded, [2, 13].into());
        f.shift(5, -3);
        assert_eq!(f.folded, [2, 10].into());
        f.shift(8, -5);
        assert_eq!(f.folded, [2].into());
    }

    #[test]
    fn revealing_a_line_unfolds_the_blocks_hiding_it() {
        let r = find(
            "{\n  \"a\": [\n    1,\n    2\n  ],\n  \"b\": 1\n}\n",
            Some("json"),
        );
        let mut f = folds_of(&[0, 1]);
        assert!(f.reveal(&r, 2));
        assert!(f.is_empty());
        assert!(!f.reveal(&r, 2));
    }

    #[test]
    fn folds_that_lost_their_block_are_dropped() {
        let r = find("{\n  1,\n  2\n}\n", Some("json"));
        let mut f = folds_of(&[0, 7]);
        f.prune(&r);
        assert_eq!(f.folded, [0].into());
    }
}
