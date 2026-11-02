//! Undo/redo stacks that store changes, not copies of the whole buffer.
//!
//! Only the newest entry holds its full text; each older one keeps just the
//! lines that differ from the entry above it, and gets its text back when
//! the ones above are popped. A step on a large file then costs the size of
//! the edit, not the size of the file.

use std::collections::VecDeque;

use crate::editor::line_replacement;

/// A buffer state with its cursor (line, byte column).
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub text: String,
    pub cursor: (usize, usize),
}

#[derive(Debug, Clone)]
enum Entry {
    Full(Snapshot),
    /// The state that precedes the entry above: its `old` lines stand in for
    /// the `new_lines` lines found at `first_line` in the text above.
    Delta {
        cursor: (usize, usize),
        first_line: usize,
        new_lines: usize,
        old: String,
    },
}

impl Entry {
    fn bytes(&self) -> usize {
        match self {
            Entry::Full(s) => s.text.len(),
            Entry::Delta { old, .. } => old.len() + 32,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct History {
    entries: VecDeque<Entry>,
    bytes: usize,
}

impl History {
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.bytes = 0;
    }

    /// Memory held by the stack, approximately.
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    pub fn push_back(&mut self, snapshot: Snapshot) {
        if let Some(Entry::Full(below)) = self.entries.back() {
            if let Some(delta) = compress(below, &snapshot.text) {
                let last = self.entries.len() - 1;
                self.bytes -= self.entries[last].bytes();
                self.bytes += delta.bytes();
                self.entries[last] = delta;
            }
        }
        self.bytes += snapshot.text.len();
        self.entries.push_back(Entry::Full(snapshot));
    }

    pub fn pop_back(&mut self) -> Option<Snapshot> {
        let Entry::Full(top) = self.entries.pop_back()? else {
            unreachable!("the newest entry always holds its text");
        };
        self.bytes -= top.text.len();
        if let Some(Entry::Delta { .. }) = self.entries.back() {
            let last = self.entries.len() - 1;
            let delta = std::mem::replace(
                &mut self.entries[last],
                Entry::Full(Snapshot {
                    text: String::new(),
                    cursor: (0, 0),
                }),
            );
            self.bytes -= delta.bytes();
            let full = expand(&delta, &top.text);
            self.bytes += full.text.len();
            self.entries[last] = Entry::Full(full);
        }
        Some(top)
    }

    /// Drops the oldest entries beyond `max_steps`, or beyond `max_bytes`
    /// (always keeping one).
    pub fn trim(&mut self, max_steps: usize, max_bytes: usize) {
        while self.entries.len() > max_steps || (self.bytes > max_bytes && self.entries.len() > 1) {
            match self.entries.pop_front() {
                Some(old) => self.bytes -= old.bytes(),
                None => break,
            }
        }
    }
}

/// `below` as a delta against `above`, when that is exact and smaller.
fn compress(below: &Snapshot, above: &str) -> Option<Entry> {
    // The line diff ignores one final newline; both sides must agree on it.
    if below.text.ends_with('\n') != above.ends_with('\n') {
        return None;
    }
    let change = line_replacement(&below.text, above)?;
    if change.old_text.len() >= below.text.len() / 2 {
        return None;
    }
    Some(Entry::Delta {
        cursor: below.cursor,
        first_line: change.first_line,
        new_lines: change.new_text.split('\n').count(),
        old: change.old_text,
    })
}

fn expand(delta: &Entry, above: &str) -> Snapshot {
    let Entry::Delta {
        cursor,
        first_line,
        new_lines,
        old,
    } = delta
    else {
        unreachable!("only deltas are expanded");
    };
    let had_newline = above.ends_with('\n');
    let body = above.strip_suffix('\n').unwrap_or(above);
    let lines: Vec<&str> = body.split('\n').collect();
    let mut text = String::with_capacity(above.len());
    text.push_str(&lines[..*first_line].join("\n"));
    if *first_line > 0 {
        text.push('\n');
    }
    text.push_str(old);
    let rest = &lines[first_line + new_lines..];
    if !rest.is_empty() {
        text.push('\n');
        text.push_str(&rest.join("\n"));
    }
    if had_newline {
        text.push('\n');
    }
    Snapshot {
        text,
        cursor: *cursor,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(text: &str) -> Snapshot {
        Snapshot {
            text: text.to_string(),
            cursor: (0, 0),
        }
    }

    fn big(n: usize) -> String {
        (0..n).map(|i| format!("line {i}\n")).collect()
    }

    #[test]
    fn states_come_back_exactly_after_being_compressed() {
        let base = big(200);
        let states = vec![
            base.clone(),
            base.replacen("line 5\n", "five\nand more\n", 1),
            base.replacen("line 5\n", "", 1),
            format!("first\n{base}"),
            format!("{base}last\n"),
            base.replace("line 100\n", "\n"),
        ];
        let mut history = History::default();
        for s in &states {
            history.push_back(snap(s));
        }
        for s in states.iter().rev() {
            assert_eq!(history.pop_back().unwrap().text, *s);
        }
        assert!(history.pop_back().is_none());
        assert_eq!(history.bytes(), 0);
    }

    #[test]
    fn only_the_changed_lines_are_kept() {
        let base = big(10_000);
        let mut history = History::default();
        history.push_back(snap(&base));
        history.push_back(snap(&base.replacen("line 7\n", "seven\n", 1)));
        assert!(history.bytes() < base.len() + 200, "{}", history.bytes());
    }

    #[test]
    fn random_edits_round_trip() {
        let mut seed = 12345u64;
        let mut next = move |n: usize| {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((seed >> 33) as usize) % n
        };
        let mut text = big(50);
        let mut history = History::default();
        let mut states = vec![];
        for _ in 0..300 {
            states.push(text.clone());
            history.push_back(snap(&text));
            let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
            match next(3) {
                0 if !lines.is_empty() => {
                    lines.remove(next(lines.len()));
                }
                1 => lines.insert(next(lines.len() + 1), format!("new {}", next(99))),
                _ if !lines.is_empty() => {
                    let i = next(lines.len());
                    lines[i].push('x');
                }
                _ => {}
            }
            text = lines.iter().map(|l| format!("{l}\n")).collect();
        }
        while let Some(s) = history.pop_back() {
            assert_eq!(s.text, states.pop().unwrap());
        }
    }

    #[test]
    fn trimming_drops_the_oldest() {
        let mut history = History::default();
        for i in 0..10 {
            history.push_back(snap(&format!("{i}\n")));
        }
        history.trim(3, usize::MAX);
        assert_eq!(history.len(), 3);
        assert_eq!(history.pop_back().unwrap().text, "9\n");
        assert_eq!(history.pop_back().unwrap().text, "8\n");
    }
}
