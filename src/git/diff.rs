//! Unified diffs (`git diff`, `git show`) parsed into hunks of lines, and the
//! ways the viewer lays them out.

/// What a line of a diff is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tag {
    Context,
    Added,
    Removed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub tag: Tag,
    pub text: String,
    /// The line ended with a carriage return (CRLF file); it is not in `text`
    /// but a patch has to give it back.
    pub cr: bool,
    /// Git said `\ No newline at end of file` after it.
    pub no_eol: bool,
    /// Line numbers in the old and the new file.
    pub old_no: Option<u32>,
    pub new_no: Option<u32>,
    /// Where it is in the diff: (hunk, line of the hunk).
    pub pos: (usize, usize),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Hunk {
    /// The `@@ … @@` line.
    pub header: String,
    pub lines: Vec<Line>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileDiff {
    pub path: String,
    pub old_path: Option<String>,
    pub binary: bool,
    pub hunks: Vec<Hunk>,
}

/// A block of consecutive changed lines, as the file stands now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    /// 0-based index, in the current file, of the first line it covers (or
    /// of the place where removed lines used to be).
    pub at: usize,
    /// The new lines it holds.
    pub added: Vec<String>,
    /// The old lines it replaces.
    pub removed: Vec<String>,
    /// The last old line had no end of line (the end of a file).
    pub removed_no_eol: bool,
}

/// A row of the unified layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row<'a> {
    Hunk(&'a str),
    Line(&'a Line),
}

/// A row of the side-by-side layout: the old file on the left, the new one
/// on the right; a changed block lines up its removals with its additions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SideRow<'a> {
    Hunk(&'a str),
    Pair(Option<&'a Line>, Option<&'a Line>),
}

impl FileDiff {
    pub fn added(&self) -> usize {
        self.count(Tag::Added)
    }

    pub fn removed(&self) -> usize {
        self.count(Tag::Removed)
    }

    fn count(&self, tag: Tag) -> usize {
        self.hunks
            .iter()
            .flat_map(|h| &h.lines)
            .filter(|l| l.tag == tag)
            .count()
    }

    pub fn is_empty(&self) -> bool {
        self.hunks.is_empty()
    }

    pub fn rows(&self) -> Vec<Row<'_>> {
        let mut rows = Vec::new();
        for hunk in &self.hunks {
            rows.push(Row::Hunk(&hunk.header));
            rows.extend(hunk.lines.iter().map(Row::Line));
        }
        rows
    }

    pub fn side_rows(&self) -> Vec<SideRow<'_>> {
        fn flush<'a>(
            rows: &mut Vec<SideRow<'a>>,
            removed: &mut Vec<&'a Line>,
            added: &mut Vec<&'a Line>,
        ) {
            for i in 0..removed.len().max(added.len()) {
                rows.push(SideRow::Pair(
                    removed.get(i).copied(),
                    added.get(i).copied(),
                ));
            }
            removed.clear();
            added.clear();
        }
        let mut rows = Vec::new();
        for hunk in &self.hunks {
            rows.push(SideRow::Hunk(&hunk.header));
            let mut removed: Vec<&Line> = Vec::new();
            let mut added: Vec<&Line> = Vec::new();
            for line in &hunk.lines {
                match line.tag {
                    Tag::Context => {
                        flush(&mut rows, &mut removed, &mut added);
                        rows.push(SideRow::Pair(Some(line), Some(line)));
                    }
                    Tag::Removed => removed.push(line),
                    Tag::Added => added.push(line),
                }
            }
            flush(&mut rows, &mut removed, &mut added);
        }
        rows
    }

    /// Where each block of changes starts, as an index into [`rows`](Self::rows).
    pub fn change_rows(&self) -> Vec<usize> {
        let mut starts = Vec::new();
        let mut in_change = false;
        for (i, row) in self.rows().iter().enumerate() {
            let changed = matches!(row, Row::Line(l) if l.tag != Tag::Context);
            if changed && !in_change {
                starts.push(i);
            }
            in_change = changed;
        }
        starts
    }

    /// The same, as an index into [`side_rows`](Self::side_rows).
    pub fn change_side_rows(&self) -> Vec<usize> {
        let mut starts = Vec::new();
        let mut in_change = false;
        for (i, row) in self.side_rows().iter().enumerate() {
            let changed = matches!(row, SideRow::Pair(l, r)
                if l.is_none_or(|l| l.tag != Tag::Context) || r.is_none_or(|r| r.tag != Tag::Context));
            if changed && !in_change {
                starts.push(i);
            }
            in_change = changed;
        }
        starts
    }

    /// The blocks of changes, in file order (the same ones the arrows visit).
    pub fn blocks(&self) -> Vec<Block> {
        let mut blocks = Vec::new();
        for hunk in &self.hunks {
            // `@@ -a,b +c,d @@`: where the new side starts.
            let start = hunk
                .header
                .split(' ')
                .find_map(|p| p.strip_prefix('+'))
                .and_then(|n| n.split(',').next())
                .and_then(|n| n.parse::<usize>().ok())
                .unwrap_or(1);
            let mut at = start.saturating_sub(1);
            let mut current: Option<Block> = None;
            for line in &hunk.lines {
                match line.tag {
                    Tag::Context => {
                        blocks.extend(current.take());
                        at += 1;
                    }
                    Tag::Added => {
                        current
                            .get_or_insert_with(|| Block {
                                at,
                                added: Vec::new(),
                                removed: Vec::new(),
                                removed_no_eol: false,
                            })
                            .added
                            .push(line.text.clone());
                        at += 1;
                    }
                    Tag::Removed => {
                        let block = current.get_or_insert_with(|| Block {
                            at,
                            added: Vec::new(),
                            removed: Vec::new(),
                            removed_no_eol: false,
                        });
                        block.removed.push(line.text.clone());
                        block.removed_no_eol |= line.no_eol;
                    }
                }
            }
            blocks.extend(current);
        }
        blocks
    }

    /// For each row of [`rows`](Self::rows): the block of changes it is in.
    pub fn row_blocks(&self) -> Vec<Option<usize>> {
        let mut blocks = Vec::new();
        let (mut in_change, mut n) = (false, 0usize);
        for row in self.rows() {
            let changed = matches!(row, Row::Line(l) if l.tag != Tag::Context);
            if changed && !in_change {
                n += 1;
            }
            in_change = changed;
            blocks.push(changed.then(|| n - 1));
        }
        blocks
    }

    /// The same for [`side_rows`](Self::side_rows).
    pub fn side_row_blocks(&self) -> Vec<Option<usize>> {
        let mut blocks = Vec::new();
        let (mut in_change, mut n) = (false, 0usize);
        for row in self.side_rows() {
            let changed = matches!(row, SideRow::Pair(l, r)
                if l.is_none_or(|l| l.tag != Tag::Context) || r.is_none_or(|r| r.tag != Tag::Context));
            if changed && !in_change {
                n += 1;
            }
            in_change = changed;
            blocks.push(changed.then(|| n - 1));
        }
        blocks
    }

    pub fn changes(&self) -> usize {
        self.change_rows().len()
    }
}

impl FileDiff {
    /// Which block of changes each line of each hunk belongs to (`None` for
    /// context), numbered as [`blocks`](Self::blocks) does.
    fn block_ids(&self) -> Vec<Vec<Option<usize>>> {
        let mut next = 0usize;
        self.hunks
            .iter()
            .map(|hunk| {
                let mut open = false;
                hunk.lines
                    .iter()
                    .map(|line| {
                        if line.tag == Tag::Context {
                            open = false;
                            None
                        } else {
                            if !open {
                                open = true;
                                next += 1;
                            }
                            Some(next - 1)
                        }
                    })
                    .collect()
            })
            .collect()
    }

    /// A patch that holds only the changes `keep` says yes to, for
    /// `git apply --cached`: what is left out stays as it is. Reversed
    /// (`git apply --cached -R`, to take changes out of the index) it is
    /// what is *kept* that is taken out, and what is left out stays in.
    fn patch_of(&self, reverse: bool, keep: impl Fn(usize, usize) -> bool) -> Option<String> {
        let mut out = String::new();
        for (h, hunk) in self.hunks.iter().enumerate() {
            let kept = |l: usize| keep(h, l);
            if !(0..hunk.lines.len()).any(|l| hunk.lines[l].tag != Tag::Context && kept(l)) {
                continue;
            }
            let start = |sign: char| -> usize {
                hunk.header
                    .split(' ')
                    .find_map(|p| p.strip_prefix(sign))
                    .and_then(|n| n.split(',').next())
                    .and_then(|n| n.parse().ok())
                    .unwrap_or(1)
            };
            let (mut old_len, mut new_len) = (0usize, 0usize);
            let mut body = String::new();
            for (l, line) in hunk.lines.iter().enumerate() {
                let (sign, in_old, in_new) = match (line.tag, kept(l), reverse) {
                    (Tag::Context, _, _) => (' ', true, true),
                    (Tag::Added, true, _) => ('+', false, true),
                    (Tag::Removed, true, _) => ('-', true, false),
                    // Left out: the line is as it already is where the patch
                    // is applied: not there (apply) or there (reverse).
                    (Tag::Added, false, false) | (Tag::Removed, false, true) => continue,
                    (Tag::Removed, false, false) | (Tag::Added, false, true) => (' ', true, true),
                };
                old_len += usize::from(in_old);
                new_len += usize::from(in_new);
                body.push(sign);
                body.push_str(&line.text);
                if line.cr {
                    body.push('\r');
                }
                body.push('\n');
                if line.no_eol {
                    body.push_str("\\ No newline at end of file\n");
                }
            }
            if out.is_empty() {
                let old = self.old_path.as_deref().unwrap_or(&self.path);
                out.push_str(&format!(
                    "diff --git a/{old} b/{new}\n--- a/{old}\n+++ b/{new}\n",
                    new = self.path
                ));
            }
            // (`git apply --recount` takes the counts from the body.)
            let (old_start, new_start) = (start('-'), start('+'));
            out.push_str(&format!(
                "@@ -{old_start},{old_len} +{new_start},{new_len} @@\n{body}"
            ));
        }
        (!out.is_empty()).then_some(out)
    }

    /// The patch of one block of changes (an index of [`blocks`](Self::blocks)).
    pub fn block_patch(&self, block: usize, reverse: bool) -> Option<String> {
        let ids = self.block_ids();
        self.patch_of(reverse, |h, l| ids[h][l] == Some(block))
    }

    /// The patch of the changed lines picked (hunk, line).
    pub fn lines_patch(
        &self,
        picked: &std::collections::HashSet<(usize, usize)>,
        reverse: bool,
    ) -> Option<String> {
        self.patch_of(reverse, |h, l| picked.contains(&(h, l)))
    }
}

/// Parses the output of `git diff` / `git show` (one or several files).
pub fn parse_diff(text: &str) -> Vec<FileDiff> {
    let mut files: Vec<FileDiff> = Vec::new();
    let mut old_no = 0u32;
    let mut new_no = 0u32;
    // Split on `\n` only: a `\r` ends a line of a CRLF file, not a line of the
    // diff, and a patch made of the lines must give it back.
    let text = text.strip_suffix('\n').unwrap_or(text);
    for line in text.split('\n') {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            // `a/path b/path` (the paths may hold spaces: split in the middle).
            let path = rest
                .split_once(" b/")
                .map_or_else(|| rest.to_string(), |(_, b)| b.to_string());
            files.push(FileDiff {
                path,
                ..FileDiff::default()
            });
            continue;
        }
        let Some(file) = files.last_mut() else {
            // `--no-index` diffs start with `diff --git` too; anything else
            // before the first file is not part of a diff.
            continue;
        };
        // The headers come before the first hunk; after it, a line that
        // reads `--- …` is a removed line whose text starts with `-- `.
        let in_header = file.hunks.is_empty();
        // (git adds a tab after a name that holds a space.)
        if let (true, Some(path)) = (
            in_header,
            line.strip_prefix("--- ").map(|p| p.trim_end_matches('\t')),
        ) {
            if let Some(old) = path.strip_prefix("a/") {
                if old != file.path {
                    file.old_path = Some(old.to_string());
                }
            }
        } else if let (true, Some(path)) = (
            in_header,
            line.strip_prefix("+++ ").map(|p| p.trim_end_matches('\t')),
        ) {
            // The file's own name (the `diff --git` line is cut in the middle
            // when a path holds " b/").
            if let Some(new) = path.strip_prefix("b/") {
                file.path = new.to_string();
                if file.old_path.as_deref() == Some(new) {
                    file.old_path = None;
                }
            }
        } else if let (true, Some(path)) = (in_header, line.strip_prefix("rename from ")) {
            file.old_path = Some(path.to_string());
        } else if in_header
            && (line.starts_with("Binary files ") || line.starts_with("GIT binary patch"))
        {
            file.binary = true;
        } else if let Some(rest) = line.strip_prefix("@@ ") {
            // @@ -old[,n] +new[,n] @@ section
            let header = format!("@@ {rest}");
            let numbers = rest.split(" @@").next().unwrap_or("");
            let mut parts = numbers.split(' ');
            let start = |part: Option<&str>, sign: char| -> u32 {
                part.and_then(|p| p.strip_prefix(sign))
                    .and_then(|p| p.split(',').next())
                    .and_then(|n| n.parse().ok())
                    .unwrap_or(1)
            };
            old_no = start(parts.next(), '-');
            new_no = start(parts.next(), '+');
            file.hunks.push(Hunk {
                header,
                lines: Vec::new(),
            });
        } else if let Some(hunk) = file.hunks.len().checked_sub(1).map(|h| (h, h)) {
            let (hunk_index, _) = hunk;
            let hunk = &mut file.hunks[hunk_index];
            if line.starts_with('\\') {
                // `\ No newline at end of file`: about the line before.
                if let Some(last) = hunk.lines.last_mut() {
                    last.no_eol = true;
                }
                continue;
            }
            let (line, cr) = match line.strip_suffix('\r') {
                Some(stripped) => (stripped, true),
                None => (line, false),
            };
            let (tag, text) = match line.as_bytes().first() {
                Some(b'+') => (Tag::Added, &line[1..]),
                Some(b'-') => (Tag::Removed, &line[1..]),
                Some(b' ') => (Tag::Context, &line[1..]),
                // An empty context line.
                None => (Tag::Context, ""),
                _ => continue,
            };
            let (old, new) = match tag {
                Tag::Context => (Some(old_no), Some(new_no)),
                Tag::Removed => (Some(old_no), None),
                Tag::Added => (None, Some(new_no)),
            };
            if old.is_some() {
                old_no += 1;
            }
            if new.is_some() {
                new_no += 1;
            }
            hunk.lines.push(Line {
                tag,
                text: text.to_string(),
                cr,
                no_eol: false,
                old_no: old,
                new_no: new,
                pos: (hunk_index, hunk.lines.len()),
            });
        }
    }
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIFF: &str = "\
diff --git a/src/a.rs b/src/a.rs
index 111..222 100644
--- a/src/a.rs
+++ b/src/a.rs
@@ -1,5 +1,6 @@ fn main() {
 keep one
-old two
+new two
+extra
 keep three
@@ -20,3 +21,3 @@ fn other() {
 keep
-gone
+here
 end
diff --git a/b.bin b/b.bin
Binary files a/b.bin and b/b.bin differ
";

    #[test]
    fn a_diff_is_split_into_files_hunks_and_numbered_lines() {
        let files = parse_diff(DIFF);
        assert_eq!(files.len(), 2);
        let a = &files[0];
        assert_eq!(a.path, "src/a.rs");
        assert_eq!(a.hunks.len(), 2);
        assert_eq!((a.added(), a.removed()), (3, 2));
        let removed = &a.hunks[0].lines[1];
        assert_eq!(
            (removed.tag, removed.old_no, removed.new_no),
            (Tag::Removed, Some(2), None)
        );
        let added = &a.hunks[0].lines[3];
        assert_eq!(
            (added.tag, added.old_no, added.new_no),
            (Tag::Added, None, Some(3))
        );
        let second = &a.hunks[1].lines[0];
        assert_eq!((second.old_no, second.new_no), (Some(20), Some(21)));
        assert!(files[1].binary);
    }

    #[test]
    fn changes_are_blocks_of_consecutive_changed_lines() {
        let a = &parse_diff(DIFF)[0];
        // "-old two +new two +extra" and "-gone +here": two blocks.
        assert_eq!(a.changes(), 2);
        let rows = a.rows();
        assert!(matches!(rows[0], Row::Hunk(_)));
        assert_eq!(a.change_rows(), vec![2, 8]);
    }

    #[test]
    fn every_row_knows_the_block_of_changes_it_belongs_to() {
        let a = &parse_diff(DIFF)[0];
        let blocks = a.row_blocks();
        assert_eq!(blocks.len(), a.rows().len());
        assert_eq!(blocks[0], None);
        assert_eq!(&blocks[2..5], &[Some(0), Some(0), Some(0)]);
        assert_eq!(blocks[8], Some(1));
        let side = a.side_row_blocks();
        assert_eq!(side.len(), a.side_rows().len());
        assert_eq!(side.iter().flatten().max(), Some(&1));
    }

    #[test]
    fn blocks_know_where_they_are_and_what_they_replace() {
        let a = &parse_diff(DIFF)[0];
        let blocks = a.blocks();
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].at, 1);
        assert_eq!(blocks[0].added, vec!["new two", "extra"]);
        assert_eq!(blocks[0].removed, vec!["old two"]);
        assert_eq!(blocks[1].at, 21);
        assert_eq!(blocks[1].removed, vec!["gone"]);
        // A removal on its own sits before the line that follows it.
        let only = &parse_diff(
            "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1,3 +1,2 @@\n keep\n-gone\n tail\n",
        )[0];
        let b = only.blocks();
        assert_eq!(b[0].at, 1);
        assert!(b[0].added.is_empty());
        assert_eq!(b[0].removed, vec!["gone".to_string()]);
    }

    #[test]
    fn side_by_side_lines_up_removals_with_additions() {
        let a = &parse_diff(DIFF)[0];
        let rows = a.side_rows();
        // hunk, keep one, (old two | new two), (· | extra), keep three, hunk…
        assert!(matches!(rows[2], SideRow::Pair(Some(l), Some(r))
            if l.text == "old two" && r.text == "new two"));
        assert!(matches!(rows[3], SideRow::Pair(None, Some(r)) if r.text == "extra"));
        assert_eq!(a.change_side_rows().len(), 2);
    }

    #[test]
    fn an_empty_output_is_no_file() {
        assert!(parse_diff("").is_empty());
    }

    #[test]
    fn a_new_file_diff_numbers_only_the_new_side() {
        let files = parse_diff(
            "diff --git a/n.txt b/n.txt\nnew file mode 100644\n--- /dev/null\n+++ b/n.txt\n@@ -0,0 +1,2 @@\n+a\n+b\n",
        );
        let lines = &files[0].hunks[0].lines;
        assert_eq!(lines[0].new_no, Some(1));
        assert_eq!(lines[1].new_no, Some(2));
        assert_eq!(lines[0].old_no, None);
    }

    #[test]
    fn a_removed_line_that_starts_with_dashes_stays_a_line_of_the_diff() {
        let text = "diff --git a/q.sql b/q.sql\nindex 1..2 100644\n--- a/q.sql\n+++ b/q.sql\n@@ -1,3 +1,2 @@\n SELECT 1;\n--- a comment\n SELECT 2;\n";
        let file = parse_diff(text).remove(0);
        assert_eq!(file.removed(), 1);
        assert_eq!(file.hunks[0].lines[1].text, "-- a comment");
        assert_eq!(file.blocks().len(), 1);
        assert_eq!(file.path, "q.sql");
    }

    #[test]
    fn the_path_comes_from_the_plus_line_even_when_it_holds_a_b_folder() {
        let text = "diff --git a/dir b/x.txt b/dir b/x.txt\n--- a/dir b/x.txt\n+++ b/dir b/x.txt\n@@ -1 +1 @@\n-a\n+b\n";
        let file = parse_diff(text).remove(0);
        assert_eq!(file.path, "dir b/x.txt");
        assert_eq!(file.old_path, None);
    }

    const TWO_BLOCKS: &str = "\
diff --git a/f.txt b/f.txt
--- a/f.txt
+++ b/f.txt
@@ -1,7 +1,7 @@
 a
-b
+B
 c
 d
 e
-f
+F
 g
";

    #[test]
    fn a_block_has_a_patch_of_its_own_with_the_other_blocks_left_as_they_are() {
        let file = parse_diff(TWO_BLOCKS).remove(0);
        let patch = file.block_patch(1, false).unwrap();
        // The second block only: the first one's lines are context.
        assert!(patch.contains("\n-f\n+F\n"));
        assert!(patch.contains("\n b\n"), "{patch}");
        assert!(!patch.contains("+B") && !patch.contains("-b"));
        assert!(patch.contains("@@ -1,7 +1,7 @@"));
        // Reversed, to take it out of the index: the first block's new line
        // stays in, and the first block's old line is not there.
        let reversed = file.block_patch(1, true).unwrap();
        assert!(reversed.contains("\n B\n") && !reversed.contains("\n b\n"));
        assert!(file.block_patch(5, false).is_none());
    }

    #[test]
    fn some_lines_have_a_patch_too() {
        let file = parse_diff(TWO_BLOCKS).remove(0);
        let mut picked = std::collections::HashSet::new();
        picked.insert((0usize, 1usize)); // -b only
        let patch = file.lines_patch(&picked, false).unwrap();
        assert!(patch.contains("\n-b\n") && !patch.contains("+B"));
        assert!(patch.contains("@@ -1,7 +1,6 @@"), "{patch}");
        assert!(file.lines_patch(&Default::default(), false).is_none());
    }
}
