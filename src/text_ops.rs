//! Whole-buffer text transformations behind the line commands (duplicate,
//! move, delete, comment, indent). They work on the text as `buffer_text`
//! gives it: lines joined with `\n`, ending in one newline unless empty.

/// The line-comment markers for a language token: (prefix, suffix). `None`
/// for formats without comments and for plain text, where a marker would be
/// a guess.
pub fn comment_style(language: Option<&str>) -> Option<(&'static str, &'static str)> {
    match language? {
        "json" | "diff" => None,
        "html" | "xml" | "md" => Some(("<!-- ", " -->")),
        "css" => Some(("/* ", " */")),
        "py" | "rb" | "sh" | "r" | "toml" | "yaml" | "make" | "pl" => Some(("# ", "")),
        "sql" | "lua" | "hs" => Some(("-- ", "")),
        "tex" => Some(("% ", "")),
        _ => Some(("// ", "")),
    }
}

/// The lines of `text` and whether it ended with a line break. An empty text
/// is one empty line.
fn split(text: &str) -> (Vec<&str>, bool) {
    match text.strip_suffix('\n') {
        Some(body) => (body.split('\n').collect(), true),
        None => (text.split('\n').collect(), false),
    }
}

fn join<S: AsRef<str>>(lines: &[S], final_newline: bool) -> String {
    let mut out = lines
        .iter()
        .map(AsRef::as_ref)
        .collect::<Vec<_>>()
        .join("\n");
    if final_newline {
        out.push('\n');
    }
    out
}

/// Duplicates lines `first..=last` right below themselves.
pub fn duplicate_lines(text: &str, first: usize, last: usize) -> String {
    let (lines, nl) = split(text);
    let last = last.min(lines.len() - 1);
    if first > last {
        return text.to_string();
    }
    let mut out: Vec<&str> = lines[..=last].to_vec();
    out.extend_from_slice(&lines[first..=last]);
    out.extend_from_slice(&lines[last + 1..]);
    join(&out, nl)
}

/// Moves lines `first..=last` one line up or down. Returns the new text and
/// the new index of `first`; both unchanged at the edge of the buffer.
pub fn move_lines(text: &str, first: usize, last: usize, up: bool) -> (String, usize) {
    let (mut lines, nl) = split(text);
    let last = last.min(lines.len() - 1);
    if first > last {
        return (text.to_string(), first);
    }
    if up {
        if first == 0 {
            return (text.to_string(), first);
        }
        let above = lines.remove(first - 1);
        lines.insert(last, above);
        (join(&lines, nl), first - 1)
    } else {
        if last + 1 >= lines.len() {
            return (text.to_string(), first);
        }
        let below = lines.remove(last + 1);
        lines.insert(first, below);
        (join(&lines, nl), first + 1)
    }
}

/// Deletes lines `first..=last`. Returns the new text and the line the
/// cursor should land on.
pub fn delete_lines(text: &str, first: usize, last: usize) -> (String, usize) {
    let (mut lines, nl) = split(text);
    let last = last.min(lines.len() - 1);
    if first > last {
        return (text.to_string(), first);
    }
    lines.drain(first..=last);
    if lines.is_empty() {
        return (String::new(), 0);
    }
    let new_line = first.min(lines.len() - 1);
    (join(&lines, nl), new_line)
}

/// Whether `rest` (a line without its indentation) starts with the comment
/// marker as a marker: followed by a space or the end of the line, so
/// `/// doc`, `//! doc` and `#!/bin/sh` are not mistaken for commented code.
fn starts_with_marker(rest: &str, prefix: &str) -> bool {
    let marker = prefix.trim_end();
    rest.strip_prefix(marker)
        .is_some_and(|after| after.is_empty() || after.starts_with([' ', '\t']))
}

fn is_commented(rest: &str, prefix: &str, suffix: &str) -> bool {
    starts_with_marker(rest, prefix)
        && (suffix.is_empty() || rest.trim_end().ends_with(suffix.trim_start()))
}

/// Comments lines `first..=last`, or uncomments them if every non-blank one
/// already is. Trailing whitespace is left alone.
pub fn toggle_comment_lines(
    text: &str,
    first: usize,
    last: usize,
    prefix: &str,
    suffix: &str,
) -> String {
    let (lines, nl) = split(text);
    let mut lines: Vec<String> = lines.into_iter().map(str::to_string).collect();
    let last = last.min(lines.len() - 1);
    if first > last {
        return text.to_string();
    }
    let range = first..=last;
    let is_blank = |l: &str| l.trim().is_empty();
    let all_commented = lines[range.clone()].iter().any(|l| !is_blank(l))
        && lines[range.clone()]
            .iter()
            .filter(|l| !is_blank(l))
            .all(|l| is_commented(l.trim_start(), prefix, suffix));

    for line in &mut lines[range] {
        if is_blank(line) {
            continue;
        }
        let rest = line.trim_start();
        let indent = &line[..line.len() - rest.len()];
        *line = if all_commented {
            let mut inner = rest.strip_prefix(prefix.trim_end()).unwrap_or(rest);
            inner = inner.strip_prefix(' ').unwrap_or(inner);
            if !suffix.is_empty() {
                let body = inner.trim_end();
                let trailing = &inner[body.len()..];
                let body = body.strip_suffix(suffix.trim_start()).unwrap_or(body);
                let body = body.strip_suffix(' ').unwrap_or(body);
                format!("{indent}{body}{trailing}")
            } else {
                format!("{indent}{inner}")
            }
        } else if suffix.is_empty() {
            format!("{indent}{prefix}{rest}")
        } else {
            let body = rest.trim_end();
            let trailing = &rest[body.len()..];
            format!("{indent}{prefix}{body}{suffix}{trailing}")
        };
    }
    join(&lines, nl)
}

/// Adds one `indent` to every non-empty line in `first..=last`.
pub fn indent_lines(text: &str, first: usize, last: usize, indent: &str) -> String {
    let (lines, nl) = split(text);
    let out: Vec<String> = lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            if (first..=last).contains(&i) && !line.is_empty() {
                format!("{indent}{line}")
            } else {
                line.to_string()
            }
        })
        .collect();
    join(&out, nl)
}

/// Removes one level of indentation from the start of a line: `indent`
/// itself, else a tab, else up to `width` spaces.
fn dedent_one(line: &str, indent: &str, width: usize) -> String {
    if let Some(stripped) = line.strip_prefix(indent) {
        stripped.to_string()
    } else if let Some(stripped) = line.strip_prefix('\t') {
        stripped.to_string()
    } else {
        let n = line.chars().take(width).take_while(|c| *c == ' ').count();
        line[n..].to_string()
    }
}

/// Removes one level of indentation from every line in `first..=last`.
pub fn dedent_lines(text: &str, first: usize, last: usize, indent: &str, width: usize) -> String {
    let (lines, nl) = split(text);
    let out: Vec<String> = lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            if (first..=last).contains(&i) {
                dedent_one(line, indent, width)
            } else {
                line.to_string()
            }
        })
        .collect();
    join(&out, nl)
}

/// Spaces to type from the end of `before` (the text left of the cursor on
/// its line) to the next tab stop.
pub fn spaces_to_tab_stop(before: &str, width: usize) -> usize {
    let width = width.max(1);
    let mut col = 0;
    for c in before.chars() {
        if c == '\t' {
            col += width - col % width;
        } else {
            col += 1;
        }
    }
    width - col % width
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_lines_middle_and_range() {
        assert_eq!(duplicate_lines("l1\nl2\nl3\n", 1, 1), "l1\nl2\nl2\nl3\n");
        assert_eq!(duplicate_lines("a\nb\nc\n", 0, 1), "a\nb\na\nb\nc\n");
        assert_eq!(duplicate_lines("a\nb\n", 1, 1), "a\nb\nb\n");
    }

    #[test]
    fn move_lines_basic() {
        assert_eq!(move_lines("a\nb\nc\n", 1, 1, true), ("b\na\nc\n".into(), 0));
        assert_eq!(
            move_lines("a\nb\nc\n", 0, 0, false),
            ("b\na\nc\n".into(), 1)
        );
        assert_eq!(
            move_lines("a\nb\nc\nd\n", 1, 2, true),
            ("b\nc\na\nd\n".into(), 0)
        );
        assert_eq!(
            move_lines("a\nb\nc\nd\n", 1, 2, false),
            ("a\nd\nb\nc\n".into(), 2)
        );
    }

    #[test]
    fn move_lines_is_a_no_op_at_the_edges() {
        assert_eq!(move_lines("a\nb\n", 0, 0, true), ("a\nb\n".into(), 0));
        // The buffer text ends in a newline: that is not a line to swap with.
        assert_eq!(move_lines("a\nb\n", 1, 1, false), ("a\nb\n".into(), 1));
        assert_eq!(move_lines("a\nb", 1, 1, false), ("a\nb".into(), 1));
    }

    #[test]
    fn delete_lines_cases() {
        assert_eq!(delete_lines("a\nb\nc\n", 1, 1), ("a\nc\n".into(), 1));
        assert_eq!(delete_lines("a\nb\n", 1, 1), ("a\n".into(), 0));
        assert_eq!(delete_lines("a\nb\nc\n", 0, 1), ("c\n".into(), 0));
        assert_eq!(delete_lines("a\n", 0, 0), (String::new(), 0));
    }

    #[test]
    fn toggle_comment_add_and_remove_roundtrip() {
        let text = "    let x = 1;\n";
        let on = toggle_comment_lines(text, 0, 0, "// ", "");
        assert_eq!(on, "    // let x = 1;\n");
        assert_eq!(toggle_comment_lines(&on, 0, 0, "// ", ""), text);
    }

    #[test]
    fn documentation_comments_and_shebangs_are_not_uncommented() {
        assert_eq!(
            toggle_comment_lines("/// doc\n", 0, 0, "// ", ""),
            "// /// doc\n"
        );
        assert_eq!(
            toggle_comment_lines("//! doc\n", 0, 0, "// ", ""),
            "// //! doc\n"
        );
        assert_eq!(
            toggle_comment_lines("#!/bin/sh\n", 0, 0, "# ", ""),
            "# #!/bin/sh\n"
        );
        // A bare marker is still a comment.
        assert_eq!(toggle_comment_lines("//\n", 0, 0, "// ", ""), "\n");
    }

    #[test]
    fn trailing_whitespace_survives_commenting() {
        let on = toggle_comment_lines("let x = 1;  \n", 0, 0, "// ", "");
        assert_eq!(on, "// let x = 1;  \n");
        assert_eq!(toggle_comment_lines(&on, 0, 0, "// ", ""), "let x = 1;  \n");
    }

    #[test]
    fn toggle_comment_covers_a_line_range_and_skips_blank_lines() {
        let text = "a\n\nb\nc\n";
        let on = toggle_comment_lines(text, 0, 2, "// ", "");
        assert_eq!(on, "// a\n\n// b\nc\n");
        assert_eq!(toggle_comment_lines(&on, 0, 2, "// ", ""), text);
        // A mixed selection comments everything first.
        assert_eq!(
            toggle_comment_lines("// a\nb\n", 0, 1, "// ", ""),
            "// // a\n// b\n"
        );
    }

    #[test]
    fn block_comment_styles_wrap_the_line() {
        let on = toggle_comment_lines("p { }\n", 0, 0, "/* ", " */");
        assert_eq!(on, "/* p { } */\n");
        assert_eq!(toggle_comment_lines(&on, 0, 0, "/* ", " */"), "p { }\n");
    }

    #[test]
    fn indent_lines_only_touches_the_range() {
        assert_eq!(indent_lines("a\n\nb\nc\n", 0, 2, "  "), "  a\n\n  b\nc\n");
    }

    #[test]
    fn dedent_lines_basic_and_partial() {
        assert_eq!(dedent_lines("    a\n    b\n", 0, 1, "    ", 4), "a\nb\n");
        assert_eq!(dedent_lines("  a\nb\n\tc\n", 0, 2, "    ", 4), "a\nb\nc\n");
        assert_eq!(
            dedent_lines("    a\n    b\n", 0, 0, "    ", 4),
            "a\n    b\n"
        );
    }

    #[test]
    fn dedent_in_tab_mode_removes_a_whole_space_indent() {
        assert_eq!(dedent_lines("    a\n", 0, 0, "\t", 4), "a\n");
        assert_eq!(dedent_lines("  a\n", 0, 0, "\t", 4), "a\n");
    }

    #[test]
    fn comment_style_by_language() {
        assert_eq!(comment_style(Some("rs")), Some(("// ", "")));
        assert_eq!(comment_style(Some("py")), Some(("# ", "")));
        assert_eq!(comment_style(Some("make")), Some(("# ", "")));
        assert_eq!(comment_style(Some("sql")), Some(("-- ", "")));
        assert_eq!(comment_style(Some("css")), Some(("/* ", " */")));
        assert_eq!(comment_style(Some("html")), Some(("<!-- ", " -->")));
        assert_eq!(comment_style(Some("json")), None);
        // Plain text has no comments.
        assert_eq!(comment_style(None), None);
    }

    #[test]
    fn tab_stops() {
        assert_eq!(spaces_to_tab_stop("", 4), 4);
        assert_eq!(spaces_to_tab_stop("ab", 4), 2);
        assert_eq!(spaces_to_tab_stop("abcd", 4), 4);
        assert_eq!(spaces_to_tab_stop("\ta", 4), 3);
        assert_eq!(spaces_to_tab_stop("x", 0), 1);
    }
}
