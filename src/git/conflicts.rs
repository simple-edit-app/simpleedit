//! A file in conflict: the text git left with its `<<<<<<<` markers, split
//! into what is common and what each side wants, and put back together once
//! each conflict has been decided.

/// Which side of a conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// What the user has (the left pane).
    Yours,
    /// What comes from elsewhere (the right pane).
    Theirs,
}

/// What was decided about one side of a conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pick {
    Pending,
    /// Its lines go into the result.
    Taken,
    /// Its lines are left out.
    Ignored,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    pub yours: Vec<String>,
    pub theirs: Vec<String>,
    /// What both started from, when git recorded it (`diff3` style).
    pub base: Option<Vec<String>>,
    pub yours_pick: Pick,
    pub theirs_pick: Pick,
    /// What was typed for the result, instead of what the sides give.
    pub text: Option<Vec<String>>,
}

/// How a conflict settles by itself.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Simple {
    Yours,
    Theirs,
    Text(Vec<String>),
}

impl Chunk {
    pub fn pick(&self, side: Side) -> Pick {
        match side {
            Side::Yours => self.yours_pick,
            Side::Theirs => self.theirs_pick,
        }
    }

    pub fn set(&mut self, side: Side, pick: Pick) {
        match side {
            Side::Yours => self.yours_pick = pick,
            Side::Theirs => self.theirs_pick = pick,
        }
    }

    pub fn resolved(&self) -> bool {
        self.yours_pick != Pick::Pending && self.theirs_pick != Pick::Pending
    }

    /// Settles the conflict with these lines, whatever the sides say.
    pub fn set_text(&mut self, lines: Vec<String>) {
        self.text = Some(lines);
        for pick in [&mut self.yours_pick, &mut self.theirs_pick] {
            if *pick == Pick::Pending {
                *pick = Pick::Ignored;
            }
        }
    }

    /// Back to what the sides give.
    pub fn clear_text(&mut self) {
        self.text = None;
    }

    /// A conflict that needs no human: one side did nothing, both did the
    /// same, they only differ by spaces, or each changed other parts of the
    /// same line.
    fn simple(&self) -> Option<Simple> {
        if self.yours == self.theirs {
            return Some(Simple::Yours);
        }
        if let Some(base) = &self.base {
            if self.yours == *base {
                return Some(Simple::Theirs);
            }
            if self.theirs == *base {
                return Some(Simple::Yours);
            }
        }
        let squash = |lines: &[String]| -> Vec<String> {
            lines
                .iter()
                .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
                .collect()
        };
        if squash(&self.yours) == squash(&self.theirs) {
            return Some(Simple::Yours);
        }
        if let Some(base) = &self.base {
            // Lines changed on both sides, in different places of the line.
            if base.len() == self.yours.len() && base.len() == self.theirs.len() {
                let merged: Option<Vec<String>> = base
                    .iter()
                    .zip(self.yours.iter().zip(&self.theirs))
                    .map(|(b, (y, t))| {
                        if y == t {
                            Some(y.clone())
                        } else {
                            merge_line(b, y, t)
                        }
                    })
                    .collect();
                if let Some(lines) = merged {
                    return Some(Simple::Text(lines));
                }
            }
        }
        None
    }

    /// The lines of the result for this chunk, as decided so far.
    pub fn result(&self) -> Vec<&str> {
        if let Some(text) = &self.text {
            return text.iter().map(String::as_str).collect();
        }
        let mut lines = Vec::new();
        if self.yours_pick == Pick::Taken {
            lines.extend(self.yours.iter().map(String::as_str));
        }
        if self.theirs_pick == Pick::Taken {
            lines.extend(self.theirs.iter().map(String::as_str));
        }
        lines
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
    /// Lines both sides agree on (shared, so every step of the undo history
    /// can keep the whole file without copying it).
    Text(std::sync::Arc<Vec<String>>),
    Conflict(Chunk),
}

/// A conflicted file, parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Merge {
    pub segments: Vec<Segment>,
    /// `\n` or `\r\n`, as the file had.
    pub eol: &'static str,
    pub final_newline: bool,
}

impl Merge {
    pub fn conflicts(&self) -> impl Iterator<Item = &Chunk> {
        self.segments.iter().filter_map(|s| match s {
            Segment::Conflict(c) => Some(c),
            Segment::Text(_) => None,
        })
    }

    pub fn conflicts_mut(&mut self) -> impl Iterator<Item = &mut Chunk> {
        self.segments.iter_mut().filter_map(|s| match s {
            Segment::Conflict(c) => Some(c),
            Segment::Text(_) => None,
        })
    }

    pub fn unresolved(&self) -> usize {
        self.conflicts().filter(|c| !c.resolved()).count()
    }

    /// Settles every conflict that needs no human (see [`Chunk::simple`]);
    /// how many were.
    pub fn auto_resolve(&mut self) -> usize {
        let mut settled = 0;
        for chunk in self.conflicts_mut().filter(|c| !c.resolved()) {
            match chunk.simple() {
                Some(Simple::Yours) => {
                    chunk.set(Side::Yours, Pick::Taken);
                    chunk.set(Side::Theirs, Pick::Ignored);
                }
                Some(Simple::Theirs) => {
                    chunk.set(Side::Theirs, Pick::Taken);
                    chunk.set(Side::Yours, Pick::Ignored);
                }
                Some(Simple::Text(lines)) => chunk.set_text(lines),
                None => continue,
            }
            settled += 1;
        }
        settled
    }

    /// The file as it stands: undecided conflicts keep their markers.
    pub fn render(&self) -> String {
        let mut lines: Vec<String> = Vec::new();
        for segment in &self.segments {
            match segment {
                Segment::Text(text) => lines.extend(text.iter().cloned()),
                Segment::Conflict(c) if c.resolved() => {
                    lines.extend(c.result().into_iter().map(str::to_string))
                }
                Segment::Conflict(c) => {
                    lines.push("<<<<<<< yours".to_string());
                    lines.extend(c.yours.iter().cloned());
                    lines.push("=======".to_string());
                    lines.extend(c.theirs.iter().cloned());
                    lines.push(">>>>>>> theirs".to_string());
                }
            }
        }
        let mut out = lines.join(self.eol);
        if self.final_newline && !lines.is_empty() {
            out.push_str(self.eol);
        }
        out
    }
}

/// Three-way merge of one line by characters: both sides' edits of the base,
/// when they do not touch each other.
fn merge_line(base: &str, yours: &str, theirs: &str) -> Option<String> {
    use similar::{ChangeTag, TextDiff};
    // The edits of one side: (start, end) in the base, and what replaces it.
    let edits = |side: &str| -> Vec<(usize, usize, String)> {
        let diff = TextDiff::from_chars(base, side);
        let mut edits: Vec<(usize, usize, String)> = Vec::new();
        let (mut at, mut open) = (0usize, None::<(usize, String)>);
        for change in diff.iter_all_changes() {
            let c = change.value().to_string();
            match change.tag() {
                ChangeTag::Equal => {
                    if let Some((start, text)) = open.take() {
                        edits.push((start, at, text));
                    }
                    at += c.chars().count();
                }
                ChangeTag::Delete => {
                    let entry = open.get_or_insert((at, String::new()));
                    let _ = entry;
                    at += c.chars().count();
                }
                ChangeTag::Insert => {
                    open.get_or_insert((at, String::new())).1.push_str(&c);
                }
            }
        }
        if let Some((start, text)) = open {
            edits.push((start, at, text));
        }
        edits
    };
    let (mine, others) = (edits(yours), edits(theirs));
    // Touching or overlapping edits (two insertions at one point too) clash.
    for (s1, e1, _) in &mine {
        for (s2, e2, _) in &others {
            if s1 <= e2 && s2 <= e1 {
                return None;
            }
        }
    }
    let mut all: Vec<&(usize, usize, String)> = mine.iter().chain(others.iter()).collect();
    all.sort_by_key(|e| e.0);
    let chars: Vec<char> = base.chars().collect();
    let mut out = String::new();
    let mut at = 0;
    for (start, end, text) in all {
        out.extend(&chars[at..*start]);
        out.push_str(text);
        at = *end;
    }
    out.extend(&chars[at..]);
    Some(out)
}

/// Reads a file with conflict markers. `swap`: the markers' "ours" is the
/// other side's work (during a rebase git calls the branch being rebased onto
/// "ours"), so the two are exchanged. `None` when there is no conflict or the
/// markers are broken.
pub fn parse(text: &str, swap: bool) -> Option<Merge> {
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let final_newline = text.ends_with('\n');
    let body = text.strip_suffix('\n').unwrap_or(text);
    let mut segments: Vec<Segment> = Vec::new();
    let mut common: Vec<String> = Vec::new();
    #[derive(PartialEq)]
    enum In {
        Common,
        Ours,
        Base,
        Theirs,
    }
    let (mut at, mut ours, mut base, mut theirs) =
        (In::Common, vec![], None::<Vec<String>>, vec![]);
    for raw in body.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        match at {
            In::Common if line.starts_with("<<<<<<< ") || line == "<<<<<<<" => {
                if !common.is_empty() {
                    segments.push(Segment::Text(std::sync::Arc::new(std::mem::take(
                        &mut common,
                    ))));
                }
                at = In::Ours;
            }
            In::Common => common.push(line.to_string()),
            In::Ours if line.starts_with("||||||| ") || line == "|||||||" => {
                base = Some(Vec::new());
                at = In::Base;
            }
            In::Ours if line == "=======" => at = In::Theirs,
            In::Ours => ours.push(line.to_string()),
            In::Base if line == "=======" => at = In::Theirs,
            In::Base => base.get_or_insert_with(Vec::new).push(line.to_string()),
            In::Theirs if line.starts_with(">>>>>>> ") || line == ">>>>>>>" => {
                let (a, b) = (std::mem::take(&mut ours), std::mem::take(&mut theirs));
                let (yours, theirs) = if swap { (b, a) } else { (a, b) };
                segments.push(Segment::Conflict(Chunk {
                    yours,
                    theirs,
                    base: base.take(),
                    yours_pick: Pick::Pending,
                    theirs_pick: Pick::Pending,
                    text: None,
                }));
                at = In::Common;
            }
            In::Theirs => theirs.push(line.to_string()),
        }
    }
    // A conflict left open at the end: not a file git wrote.
    if at != In::Common {
        return None;
    }
    if !common.is_empty() {
        segments.push(Segment::Text(std::sync::Arc::new(common)));
    }
    let merge = Merge {
        segments,
        eol,
        final_newline,
    };
    let has_conflict = merge.conflicts().next().is_some();
    has_conflict.then_some(merge)
}

/// Why no conflict could be offered for a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unavailable {
    /// The file holds no conflict markers any more: it was settled by hand.
    Settled,
    /// Neither text could be read as a conflicted file.
    NotMergeable,
}

/// The conflicts to settle for a file: rebuilt from the three versions git
/// keeps (so each knows its base), unless the file on disk has fewer of
/// them, which means some were already settled by hand and that work must
/// not be thrown away.
pub fn choose(rebuilt: Option<&str>, disk: Option<&str>, swap: bool) -> Result<Merge, Unavailable> {
    let from_rebuilt = rebuilt.and_then(|t| parse(t, swap));
    let from_disk = disk.and_then(|t| parse(t, swap));
    match (from_rebuilt, from_disk) {
        (Some(r), Some(d)) if d.conflicts().count() < r.conflicts().count() => Ok(d),
        (Some(r), _) if disk.is_none_or(|t| t.lines().any(|l| l.starts_with("<<<<<<<"))) => Ok(r),
        // Markers gone: the file was settled by hand.
        (Some(_), None) => Err(Unavailable::Settled),
        (Some(r), Some(_)) => Ok(r),
        (None, Some(d)) => Ok(d),
        (None, None) => Err(Unavailable::NotMergeable),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "\
start
<<<<<<< HEAD
our line
our second
=======
their line
>>>>>>> feature
middle
<<<<<<< HEAD
only ours
=======
>>>>>>> feature
end
";

    #[test]
    fn a_conflicted_file_is_split_into_common_text_and_chunks() {
        let merge = parse(FILE, false).unwrap();
        assert_eq!(merge.segments.len(), 5);
        assert_eq!(merge.conflicts().count(), 2);
        let first = merge.conflicts().next().unwrap();
        assert_eq!(first.yours, vec!["our line", "our second"]);
        assert_eq!(first.theirs, vec!["their line"]);
        assert_eq!(merge.unresolved(), 2);
        assert!(merge.final_newline);
    }

    #[test]
    fn during_a_rebase_the_two_sides_swap() {
        let merge = parse(FILE, true).unwrap();
        let first = merge.conflicts().next().unwrap();
        assert_eq!(first.yours, vec!["their line"]);
        assert_eq!(first.theirs, vec!["our line", "our second"]);
    }

    #[test]
    fn the_result_follows_what_was_taken_and_ignored() {
        let mut merge = parse(FILE, false).unwrap();
        for (i, chunk) in merge.conflicts_mut().enumerate() {
            if i == 0 {
                chunk.set(Side::Yours, Pick::Taken);
                chunk.set(Side::Theirs, Pick::Taken);
            } else {
                chunk.set(Side::Yours, Pick::Ignored);
                chunk.set(Side::Theirs, Pick::Ignored);
            }
        }
        assert_eq!(merge.unresolved(), 0);
        assert_eq!(
            merge.render(),
            "start\nour line\nour second\ntheir line\nmiddle\nend\n"
        );
    }

    #[test]
    fn a_half_decided_chunk_keeps_its_markers_in_the_rendered_file() {
        let mut merge = parse(FILE, false).unwrap();
        merge
            .conflicts_mut()
            .next()
            .unwrap()
            .set(Side::Yours, Pick::Taken);
        assert_eq!(merge.unresolved(), 2);
        assert!(merge.render().contains("<<<<<<<"));
    }

    #[test]
    fn line_endings_and_the_base_section_are_kept() {
        let text = "a\r\n<<<<<<< HEAD\r\nx\r\n||||||| base\r\nb\r\n=======\r\ny\r\n>>>>>>> t\r\nz";
        let mut merge = parse(text, false).unwrap();
        assert_eq!(merge.eol, "\r\n");
        assert!(!merge.final_newline);
        let chunk = merge.conflicts_mut().next().unwrap();
        assert_eq!(chunk.base.as_deref(), Some(&["b".to_string()][..]));
        chunk.set(Side::Yours, Pick::Taken);
        chunk.set(Side::Theirs, Pick::Ignored);
        assert_eq!(merge.render(), "a\r\nx\r\nz");
    }

    fn diff3(base: &str, yours: &str, theirs: &str) -> Merge {
        let text = format!("<<<<<<< a\n{yours}\n||||||| b\n{base}\n=======\n{theirs}\n>>>>>>> c\n");
        parse(&text, false).unwrap()
    }

    #[test]
    fn a_side_that_did_nothing_gives_way_to_the_other() {
        let mut merge = diff3("same", "same", "changed");
        assert_eq!(merge.auto_resolve(), 1);
        assert_eq!(merge.render(), "changed\n");
        let mut merge = diff3("same", "mine", "same");
        merge.auto_resolve();
        assert_eq!(merge.render(), "mine\n");
    }

    #[test]
    fn identical_or_whitespace_only_differences_settle_by_themselves() {
        let mut merge = diff3("old", "new thing", "new thing");
        assert_eq!(merge.auto_resolve(), 1);
        assert_eq!(merge.render(), "new thing\n");
        let mut merge = diff3("old", "a  b", "a b");
        assert_eq!(merge.auto_resolve(), 1);
    }

    #[test]
    fn edits_in_different_places_of_a_line_are_both_kept() {
        let mut merge = diff3(
            "let a = 1; let b = 2;",
            "let A = 1; let b = 2;",
            "let a = 1; let b = 3;",
        );
        assert_eq!(merge.auto_resolve(), 1);
        assert_eq!(merge.render(), "let A = 1; let b = 3;\n");
        // Both on the same word: a real conflict.
        let mut clash = diff3("value 1", "value 2", "value 3");
        assert_eq!(clash.auto_resolve(), 0);
        assert_eq!(clash.unresolved(), 1);
    }

    #[test]
    fn typed_text_replaces_what_the_sides_give() {
        let mut merge = diff3("b", "y", "t");
        let chunk = merge.conflicts_mut().next().unwrap();
        chunk.set_text(vec!["typed".into(), "two lines".into()]);
        assert!(chunk.resolved());
        assert_eq!(merge.render(), "typed\ntwo lines\n");
    }

    #[test]
    fn broken_or_absent_markers_are_not_a_conflict() {
        assert!(parse("just text\n", false).is_none());
        assert!(parse("a\n<<<<<<< HEAD\nx\n=======\ny\n", false).is_none());
    }

    const TWO: &str =
        "a\n<<<<<<< HEAD\nx\n=======\ny\n>>>>>>> t\nm\n<<<<<<< HEAD\np\n=======\nq\n>>>>>>> t\nz\n";
    const ONE_LEFT: &str = "a\nsettled by hand\nm\n<<<<<<< HEAD\np\n=======\nq\n>>>>>>> t\nz\n";

    #[test]
    fn what_was_settled_by_hand_is_not_given_back_as_a_conflict() {
        // Nothing settled yet: the rebuilt text (it knows the base).
        assert_eq!(
            choose(Some(TWO), Some(TWO), false)
                .unwrap()
                .conflicts()
                .count(),
            2
        );
        // One conflict settled in the editor: the file on disk wins.
        assert_eq!(
            choose(Some(TWO), Some(ONE_LEFT), false)
                .unwrap()
                .conflicts()
                .count(),
            1
        );
        // All of them: no markers, nothing to open.
        assert_eq!(
            choose(Some(TWO), Some("a\nall done\n"), false).unwrap_err(),
            Unavailable::Settled
        );
        // No rebuilt text (a file deleted on one side): the disk's.
        assert_eq!(
            choose(None, Some(TWO), false).unwrap().conflicts().count(),
            2
        );
        assert_eq!(
            choose(None, None, false).unwrap_err(),
            Unavailable::NotMergeable
        );
        // The disk can't be read: the rebuilt text.
        assert!(choose(Some(TWO), None, false).is_ok());
    }
}
