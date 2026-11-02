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
    /// Line numbers in the old and the new file.
    pub old_no: Option<u32>,
    pub new_no: Option<u32>,
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

/// Parses the output of `git diff` / `git show` (one or several files).
pub fn parse_diff(text: &str) -> Vec<FileDiff> {
    let mut files: Vec<FileDiff> = Vec::new();
    let mut old_no = 0u32;
    let mut new_no = 0u32;
    for line in text.lines() {
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
        if let Some(path) = line.strip_prefix("--- ") {
            if let Some(old) = path.strip_prefix("a/") {
                if old != file.path {
                    file.old_path = Some(old.to_string());
                }
            }
        } else if let Some(path) = line.strip_prefix("rename from ") {
            file.old_path = Some(path.to_string());
        } else if line.starts_with("Binary files ") || line.starts_with("GIT binary patch") {
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
        } else if let Some(hunk) = file.hunks.last_mut() {
            let (tag, text) = match line.as_bytes().first() {
                Some(b'+') => (Tag::Added, &line[1..]),
                Some(b'-') => (Tag::Removed, &line[1..]),
                Some(b' ') => (Tag::Context, &line[1..]),
                // `\ No newline at end of file`
                Some(b'\\') => continue,
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
                old_no: old,
                new_no: new,
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
}
