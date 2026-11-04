//! Syntax colors for the lines of a diff: each hunk is read twice, as the old
//! file had it and as the new one has it, so a line is colored in the state
//! of the file it belongs to.

use std::ops::Range;

use iced::advanced::text::Highlighter as _;
use iced::Color;

use crate::editor::highlighter::{Highlighter, Settings};
use crate::git::diff::Tag;
use crate::git::FileDiff;

/// The colored spans of each line of each hunk: `[hunk][line]`, byte ranges
/// of the line's text.
pub type Spans = Vec<Vec<Vec<(Range<usize>, Option<Color>)>>>;

/// Only this many lines of a diff are colored (the rest stay plain): reading
/// a huge diff must not stop the window.
const MAX_LINES: usize = 4000;

pub fn highlight(diff: &FileDiff, path: &str, dark: bool) -> Spans {
    let extension = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    let settings = Settings { dark, extension };
    let mut budget = MAX_LINES;
    diff.hunks
        .iter()
        .map(|hunk| {
            let mut old_side = Highlighter::new(&settings);
            let mut new_side = Highlighter::new(&settings);
            old_side.change_line(0);
            new_side.change_line(0);
            hunk.lines
                .iter()
                .map(|line| {
                    if budget == 0 {
                        return Vec::new();
                    }
                    budget -= 1;
                    let colored = |side: &mut Highlighter| -> Vec<(Range<usize>, Option<Color>)> {
                        side.highlight_line(&line.text)
                            .map(|(range, highlight)| (range, highlight.color()))
                            .collect()
                    };
                    match line.tag {
                        Tag::Removed => colored(&mut old_side),
                        Tag::Added => colored(&mut new_side),
                        // Both sides read it, to stay in step.
                        Tag::Context => {
                            let _ = colored(&mut old_side);
                            colored(&mut new_side)
                        }
                    }
                })
                .collect()
        })
        .collect()
}

/// The color of each character of the line as it is shown (tabs as four
/// spaces): `None` where the grammar leaves the default text color.
pub fn colors_of(text: &str, spans: &[(Range<usize>, Option<Color>)]) -> Vec<Option<Color>> {
    let mut colors = Vec::new();
    for (byte, c) in text.char_indices() {
        let color = spans
            .iter()
            .find(|(range, _)| range.contains(&byte))
            .and_then(|(_, color)| *color);
        let width = if c == '\t' { 4 } else { 1 };
        colors.extend(std::iter::repeat_n(color, width));
    }
    colors
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::diff::parse_diff;

    const RUST: &str = "diff --git a/f.rs b/f.rs\n--- a/f.rs\n+++ b/f.rs\n@@ -1,3 +1,3 @@\n fn main() {\n-    let x = 1;\n+    let x = \"one\";\n }\n";

    #[test]
    fn the_lines_of_a_diff_are_colored_as_the_file_they_belong_to() {
        let diff = parse_diff(RUST).remove(0);
        let spans = highlight(&diff, "src/f.rs", true);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].len(), 4);
        // `fn` is a keyword: colored; the string of the added line too.
        let fn_line = &spans[0][0];
        assert!(fn_line.iter().any(|(r, c)| r.start == 0 && c.is_some()));
        let added = &spans[0][2];
        let quote = diff.hunks[0].lines[2].text.find('"').unwrap();
        assert!(added.iter().any(|(r, c)| r.contains(&quote) && c.is_some()));
        // A language nobody knows is read all the same.
        assert_eq!(highlight(&diff, "notes.zzz", true).len(), 1);
    }

    #[test]
    fn a_tab_takes_four_cells_in_the_colors_of_a_line() {
        let spans = vec![(1..4, Some(Color::from_rgb(1.0, 0.0, 0.0)))];
        let colors = colors_of("\tfoo", &spans);
        assert_eq!(colors.len(), 4 + 3);
        assert!(colors[..4].iter().all(Option::is_none));
        assert!(colors[4..].iter().all(Option::is_some));
    }
}
