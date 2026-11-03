//! Which lines of a file differ from what HEAD has: the bars in the editor's
//! gutter and the ticks beside its scrollbar.

use similar::{DiffOp, TextDiff};

/// 0-based line numbers of the buffer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChangeMarks {
    /// New lines.
    pub added: Vec<usize>,
    /// Lines that replace others.
    pub modified: Vec<usize>,
    /// Lines that were removed: the line they used to sit before (the
    /// marker is drawn at its top).
    pub deleted: Vec<usize>,
}

/// Files longer than this are not compared (the diff is quadratic in the
/// worst case).
pub const MAX_LINES: usize = 30_000;

/// What `current` changes against `base`: `None` is a file git does not
/// know yet, which is added as a whole.
pub fn compute(base: Option<&str>, current: &str) -> ChangeMarks {
    let mut marks = ChangeMarks::default();
    let Some(base) = base else {
        marks.added = (0..current.lines().count()).collect();
        return marks;
    };
    if current.lines().count() > MAX_LINES || base.lines().count() > MAX_LINES {
        return marks;
    }
    // The editor's text always ends in a line break and uses `\n`: so does
    // what it is compared with.
    let mut base = base.replace("\r\n", "\n");
    if !base.is_empty() && !base.ends_with('\n') {
        base.push('\n');
    }
    let mut current = current.to_string();
    if !current.is_empty() && !current.ends_with('\n') {
        current.push('\n');
    }
    let diff = TextDiff::from_lines(base.as_str(), current.as_str());
    for op in diff.ops() {
        match *op {
            DiffOp::Equal { .. } => {}
            DiffOp::Insert {
                new_index, new_len, ..
            } => marks.added.extend(new_index..new_index + new_len),
            DiffOp::Delete { new_index, .. } => marks.deleted.push(new_index),
            // Replaced: as many modified lines as there were old ones, the
            // others are new (or gone).
            DiffOp::Replace {
                old_len,
                new_index,
                new_len,
                ..
            } => {
                let shared = old_len.min(new_len);
                marks.modified.extend(new_index..new_index + shared);
                if new_len > shared {
                    marks.added.extend(new_index + shared..new_index + new_len);
                } else if old_len > shared {
                    marks.deleted.push(new_index + new_len);
                }
            }
        }
    }
    marks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_text_has_no_marks() {
        assert_eq!(compute(Some("a\nb\n"), "a\nb\n"), ChangeMarks::default());
    }

    #[test]
    fn added_modified_and_deleted_lines_are_told_apart() {
        let base = "one\ntwo\nthree\nfour\nfive\n";
        let current = "one\nTWO\nthree\nnew\nfive\nsix\n";
        let marks = compute(Some(base), current);
        assert_eq!(marks.modified, vec![1, 3]); // two → TWO, four → new
        assert_eq!(marks.added, vec![5]); // six
        assert!(marks.deleted.is_empty());

        let marks = compute(Some("a\nb\nc\n"), "a\nc\n");
        assert_eq!(marks.deleted, vec![1]);
        assert!(marks.added.is_empty() && marks.modified.is_empty());
    }

    #[test]
    fn a_replacement_with_more_lines_adds_the_extra_ones() {
        let marks = compute(Some("a\nb\nc\n"), "a\nX\nY\nZ\nc\n");
        assert_eq!(marks.modified, vec![1]);
        assert_eq!(marks.added, vec![2, 3]);
    }

    #[test]
    fn a_replacement_with_fewer_lines_marks_where_lines_went() {
        let marks = compute(Some("a\nb\nc\nd\n"), "a\nX\nd\n");
        assert_eq!(marks.modified, vec![1]);
        assert_eq!(marks.deleted, vec![2]);
    }

    #[test]
    fn line_endings_and_the_final_newline_are_not_changes() {
        assert_eq!(compute(Some("a\r\nb"), "a\nb\n"), ChangeMarks::default());
        assert_eq!(compute(Some("a\nb\n"), "a\nb\n"), ChangeMarks::default());
        // A real extra empty line is one.
        assert_eq!(compute(Some("a\n"), "a\n\n").added, vec![1]);
    }

    #[test]
    fn a_file_git_does_not_know_is_all_added() {
        assert_eq!(compute(None, "a\nb\nc").added, vec![0, 1, 2]);
        assert!(compute(None, "").added.is_empty());
    }
}
