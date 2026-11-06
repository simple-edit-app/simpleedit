//! Finding in the projects: their files by name ("go to file"), and a text
//! in all of them ("find in files").

use std::path::{Path, PathBuf};

/// Folders never looked into when the files are listed without Git.
const HEAVY: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    ".venv",
    "venv",
    "__pycache__",
    ".next",
    ".cache",
];

/// Most files listed, to stay quick in a huge tree.
const MAX_FILES: usize = 200_000;

/// Whether `name` (of a folder if `is_dir`) matches one of the patterns the
/// user left out: `*` stands for anything, a trailing `/` for folders only.
pub fn excluded(name: &str, is_dir: bool, patterns: &[String]) -> bool {
    patterns.iter().any(|pattern| {
        let pattern = pattern.trim();
        if pattern.is_empty() {
            return false;
        }
        match pattern.strip_suffix('/') {
            Some(dir_pattern) => is_dir && glob(dir_pattern, name),
            None => glob(pattern, name),
        }
    })
}

/// `*` matches any run of characters, `?` one character.
fn glob(pattern: &str, name: &str) -> bool {
    let (p, n): (Vec<char>, Vec<char>) = (pattern.chars().collect(), name.chars().collect());
    let (mut pi, mut ni) = (0, 0);
    let (mut star, mut mark) = (None, 0);
    while ni < n.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == n[ni]) {
            pi += 1;
            ni += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ni;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ni = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// The files of a folder, heavy folders left out: in a repository, what Git
/// lists, plus with `ignored` the files it ignores one by one (a `.env`),
/// never the folders it ignores as a whole (`node_modules`, `dist`); every
/// file otherwise.
pub fn list_files(root: &Path, exclude: &[String], ignored: bool) -> Vec<PathBuf> {
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .ok()
            .filter(|o| o.status.success())
    };
    let from_git = git(&[
        "ls-files",
        "-z",
        "--cached",
        "--others",
        "--exclude-standard",
    ]);
    // `--directory`: a folder ignored as a whole is one entry, dropped below.
    let ignored = ignored
        .then(|| {
            git(&[
                "ls-files",
                "-z",
                "--others",
                "--ignored",
                "--exclude-standard",
                "--directory",
            ])
        })
        .flatten();
    let mut files: Vec<PathBuf> = match from_git {
        Some(output) => output
            .stdout
            .split(|b| *b == 0)
            .chain(ignored.iter().flat_map(|o| o.stdout.split(|b| *b == 0)))
            .filter(|p| !p.is_empty())
            .map(|p| root.join(String::from_utf8_lossy(p).as_ref()))
            // A file deleted but not committed is still listed by Git.
            .filter(|p| p.is_file())
            .take(MAX_FILES)
            .collect(),
        None => {
            let mut found = Vec::new();
            walk(root, exclude, &mut found);
            found
        }
    };
    if !exclude.is_empty() {
        files.retain(|f| {
            f.strip_prefix(root).is_ok_and(|rel| {
                let parts: Vec<_> = rel.components().collect();
                parts.iter().enumerate().all(|(i, c)| {
                    let name = c.as_os_str().to_string_lossy();
                    !excluded(&name, i + 1 < parts.len(), exclude)
                })
            })
        });
    }
    files.sort();
    files.dedup();
    files
}

/// Every file of a folder, what Git ignores and the heavy folders included
/// (the libraries in `node_modules`, `target`…): "non-project items".
pub fn list_all_files(root: &Path) -> Vec<PathBuf> {
    fn walk_all(dir: &Path, found: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.filter_map(Result::ok) {
            if found.len() >= MAX_FILES {
                return;
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                if entry.file_name() != ".git" {
                    walk_all(&entry.path(), found);
                }
            } else if kind.is_file() {
                found.push(entry.path());
            }
        }
    }
    let mut found = Vec::new();
    walk_all(root, &mut found);
    found.sort();
    found
}

fn walk(dir: &Path, exclude: &[String], found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        if found.len() >= MAX_FILES {
            return;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            if !HEAVY.contains(&name.as_str()) && !excluded(&name, true, exclude) {
                walk(&entry.path(), exclude, found);
            }
        } else if kind.is_file() && !excluded(&name, false, exclude) {
            found.push(entry.path());
        }
    }
}

/// How well `query` matches `path` (a path shown relative to its project):
/// its letters in order, better when they are in the file's name, follow
/// each other, or start words. `None`: they are not all there.
pub fn score(query: &str, path: &str) -> Option<i64> {
    let query: Vec<char> = query
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect();
    if query.is_empty() {
        return Some(0);
    }
    let lower: Vec<char> = path.chars().flat_map(char::to_lowercase).collect();
    let original: Vec<char> = path.chars().collect();
    if lower.len() != original.len() {
        // A character that lowercases to several: compare as is.
        return score_chars(&query, &original, &original);
    }
    score_chars(&query, &lower, &original)
}

fn score_chars(query: &[char], lower: &[char], original: &[char]) -> Option<i64> {
    let name_start = lower
        .iter()
        .rposition(|c| *c == '/' || *c == '\\')
        .map_or(0, |i| i + 1);
    let starts_word = |i: usize| {
        i == 0
            || matches!(lower[i - 1], '/' | '\\' | '_' | '-' | '.' | ' ')
            || (original[i].is_uppercase() && original[i - 1].is_lowercase())
    };
    // Matched from the end, so that the letters land in the file's name when
    // they can, then each one as early as possible after that.
    let mut positions = Vec::with_capacity(query.len());
    let mut at = lower.len();
    for q in query.iter().rev() {
        let found = lower[..at].iter().rposition(|c| c == q)?;
        positions.push(found);
        at = found;
    }
    positions.reverse();
    let mut score: i64 = 0;
    let mut previous: Option<usize> = None;
    for &i in &positions {
        score += 10;
        if i >= name_start {
            score += 8;
        }
        if starts_word(i) {
            score += 12;
        }
        if previous.is_some_and(|p| p + 1 == i) {
            score += 15;
        }
        previous = Some(i);
    }
    let name: String = lower[name_start..].iter().collect();
    let typed: String = query.iter().collect();
    if name == typed || name.split('.').next() == Some(typed.as_str()) {
        score += 200;
    } else if name.starts_with(&typed) {
        score += 80;
    }
    // Shorter paths first, all else equal.
    score -= lower.len() as i64 / 4;
    Some(score)
}

/// A place where the text was found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextHit {
    pub path: PathBuf,
    /// From 0; the column and the length in bytes.
    pub line: usize,
    pub col: usize,
    pub len: usize,
    /// The line, trimmed, with where the match is in it.
    pub preview: String,
    pub preview_at: (usize, usize),
}

/// Most places listed.
pub const MAX_HITS: usize = 2000;
/// Files bigger than this are not looked into.
const MAX_SIZE: u64 = 2 * 1024 * 1024;

/// Every match of a search in `files`, with what each would be replaced by.
/// The text of a file open in the editor is `open`'s (its unsaved changes
/// count), else the file's; a big or binary file is left out.
pub fn find_occurrences(
    files: &[PathBuf],
    open: &std::collections::HashMap<PathBuf, String>,
    options: &crate::find_replace::Options,
    replace: Option<&crate::find_replace::Replace>,
) -> Result<Vec<(TextHit, Option<String>)>, String> {
    if options.query.is_empty() {
        return Ok(Vec::new());
    }
    let re = crate::find_replace::compile(options)?;
    let mut hits = Vec::new();
    for path in files {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if let Some(mask) = &options.mask {
            if !crate::find_replace::mask_matches(mask, &name) {
                continue;
            }
        }
        let read;
        let text: &str = match open.get(path) {
            Some(text) => text,
            None => {
                if std::fs::metadata(path).map_or(true, |m| m.len() > MAX_SIZE) {
                    continue;
                }
                let Ok(bytes) = std::fs::read(path) else {
                    continue;
                };
                if bytes.iter().take(8000).any(|b| *b == 0) {
                    continue;
                }
                read = String::from_utf8_lossy(&bytes).into_owned();
                &read
            }
        };
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        for found in
            crate::find_replace::find_in(text, &ext, &re, options.context, options.regex, replace)
        {
            let line_start = found.start - found.col;
            let line_end = text[line_start..]
                .find('\n')
                .map_or(text.len(), |n| line_start + n);
            let line = text[line_start..line_end].trim_end_matches('\r');
            let trimmed = line.trim_start();
            let cut = line.len() - trimmed.len();
            let len = found.end.min(line_start + line.len()) - found.start;
            let (preview, at) = excerpt(trimmed, found.col.saturating_sub(cut), len);
            hits.push((
                TextHit {
                    path: path.clone(),
                    line: found.line,
                    col: found.col,
                    len: found.end - found.start,
                    preview,
                    preview_at: at,
                },
                found.replacement,
            ));
            if hits.len() >= MAX_HITS {
                return Ok(hits);
            }
        }
    }
    Ok(hits)
}

/// Every place `query` is in `files` (a regular expression if `regex`).
pub fn search(
    files: &[PathBuf],
    query: &str,
    case_sensitive: bool,
    regex: bool,
) -> Result<Vec<TextHit>, String> {
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let pattern = if regex {
        query.to_string()
    } else {
        regex::escape(query)
    };
    let re = regex::RegexBuilder::new(&pattern)
        .case_insensitive(!case_sensitive)
        .build()
        .map_err(|e| e.to_string())?;
    let mut hits = Vec::new();
    for path in files {
        if std::fs::metadata(path).map_or(true, |m| m.len() > MAX_SIZE) {
            continue;
        }
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        // Not text.
        if bytes.iter().take(8000).any(|b| *b == 0) {
            continue;
        }
        let text = String::from_utf8_lossy(&bytes);
        for (n, line) in text.lines().enumerate() {
            for m in re.find_iter(line) {
                if m.start() == m.end() {
                    continue;
                }
                let trimmed = line.trim_start();
                let cut = line.len() - trimmed.len();
                // A long line is shown around the match.
                let (preview, at) = excerpt(trimmed, m.start() - cut.min(m.start()), m.len());
                hits.push(TextHit {
                    path: path.clone(),
                    line: n,
                    col: m.start(),
                    len: m.len(),
                    preview,
                    preview_at: at,
                });
                if hits.len() >= MAX_HITS {
                    return Ok(hits);
                }
            }
        }
    }
    Ok(hits)
}

/// At most about 160 characters of `line` around the match at `start`.
fn excerpt(line: &str, start: usize, len: usize) -> (String, (usize, usize)) {
    const ROOM: usize = 160;
    if line.len() <= ROOM {
        return (line.to_string(), (start, start + len));
    }
    let mut from = start.saturating_sub(40);
    while !line.is_char_boundary(from) {
        from -= 1;
    }
    let mut to = (from + ROOM).min(line.len());
    while !line.is_char_boundary(to) {
        to -= 1;
    }
    let lead = if from > 0 { "…" } else { "" };
    let text = format!("{lead}{}", &line[from..to]);
    let shift = lead.len() + start - from;
    (text, (shift, (shift + len).min(lead.len() + to - from)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The best `limit` of `paths` for `query`, best first.
    fn rank(query: &str, paths: &[String], limit: usize) -> Vec<usize> {
        let mut scored: Vec<(i64, usize)> = paths
            .iter()
            .enumerate()
            .filter_map(|(i, p)| score(query, p).map(|s| (s, i)))
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        scored.into_iter().take(limit).map(|(_, i)| i).collect()
    }

    #[test]
    fn patterns_leave_out_names_and_folders() {
        let p = vec!["*.log".to_string(), "dist/".to_string(), "tmp?".to_string()];
        assert!(excluded("app.log", false, &p));
        assert!(!excluded("app.logs", false, &p));
        assert!(excluded("dist", true, &p));
        assert!(
            !excluded("dist", false, &p),
            "a file named like the folder stays"
        );
        assert!(excluded("tmp1", false, &p) && !excluded("tmp12", false, &p));
        assert!(!excluded("main.rs", false, &[String::new()]));
        assert!(glob("*", "anything") && glob("a*b*c", "aXXbYc") && !glob("a*b", "ac"));
    }

    #[test]
    fn the_files_git_ignores_are_listed_but_not_the_folders_it_ignores() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let git = |args: &[&str]| {
            std::process::Command::new("git")
                .arg("-C")
                .arg(root)
                .args(args)
                .output()
                .unwrap();
        };
        git(&["init", "-q"]);
        std::fs::create_dir_all(root.join("dist")).unwrap();
        std::fs::create_dir_all(root.join("app")).unwrap();
        std::fs::write(root.join(".gitignore"), ".env\ndist/\n").unwrap();
        std::fs::write(root.join("a.rs"), "").unwrap();
        std::fs::write(root.join(".env"), "# NEON\n").unwrap();
        std::fs::write(root.join("app/.env"), "").unwrap();
        std::fs::write(root.join("dist/out.js"), "").unwrap();
        let files: Vec<String> = list_files(root, &[], true)
            .iter()
            .map(|f| f.strip_prefix(root).unwrap().display().to_string())
            .collect();
        assert_eq!(files, [".env", ".gitignore", "a.rs", "app/.env"]);
        let without: Vec<PathBuf> = list_files(root, &[], false);
        assert_eq!(without, vec![root.join(".gitignore"), root.join("a.rs")]);
        // Looked into like the others.
        let hits = search(&list_files(root, &[], true), "neon", false, false).unwrap();
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn the_name_of_the_file_counts_most() {
        let paths: Vec<String> = [
            "src/components/app-sidebar.tsx",
            "src/app.rs",
            "docs/application/notes.md",
            "src/sidebar/mod.rs",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(rank("app.rs", &paths, 10)[0], 1, "the exact name wins");
        assert_eq!(
            rank("appside", &paths, 10)[0],
            0,
            "letters in the name, in order"
        );
        assert_eq!(rank("sidebar", &paths, 10)[0], 0);
        assert_eq!(
            rank("sbmod", &paths, 10)[0],
            3,
            "across the folder and the name"
        );
        assert!(rank("zzz", &paths, 10).is_empty());
        assert_eq!(rank("", &paths, 2).len(), 2, "nothing typed: everything");
        // Upper case typed or not: the same.
        assert_eq!(score("APP", "src/app.rs"), score("app", "src/app.rs"));
    }

    #[test]
    fn files_are_listed_with_git_or_by_walking_without_the_heavy_folders() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        std::fs::create_dir_all(p.join("src")).unwrap();
        std::fs::create_dir_all(p.join("node_modules/x")).unwrap();
        std::fs::write(p.join("src/main.rs"), "").unwrap();
        std::fs::write(p.join("node_modules/x/i.js"), "").unwrap();
        std::fs::write(p.join("debug.log"), "").unwrap();
        // Not a repository: walked.
        let walked = list_files(p, &[], true);
        assert_eq!(walked, vec![p.join("debug.log"), p.join("src/main.rs")]);
        assert_eq!(
            list_files(p, &["*.log".into()], true),
            vec![p.join("src/main.rs")]
        );
        // A repository: what Git knows of and ignored files, not ignored folders.
        let git = |args: &[&str]| {
            std::process::Command::new("git")
                .arg("-C")
                .arg(p)
                .args(args)
                .output()
                .unwrap();
        };
        git(&["init", "-q"]);
        std::fs::write(p.join(".gitignore"), "*.log\nnode_modules/\n").unwrap();
        let listed = list_files(p, &[], true);
        assert!(listed.contains(&p.join("src/main.rs")) && listed.contains(&p.join(".gitignore")));
        assert!(listed.contains(&p.join("debug.log")));
        assert!(!listed.iter().any(|f| f.starts_with(p.join("node_modules"))));
        assert_eq!(
            list_files(p, &["src/".into()], true),
            vec![p.join(".gitignore"), p.join("debug.log")],
            "a folder pattern leaves out what is in it"
        );
    }

    #[test]
    fn a_text_is_found_in_every_file_with_its_line() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.rs");
        let b = dir.path().join("b.txt");
        let bin = dir.path().join("c.bin");
        std::fs::write(&a, "fn main() {\n    let Total = 1;\n}\n").unwrap();
        std::fs::write(&b, "total: 3\nnothing\ntotal again total\n").unwrap();
        std::fs::write(&bin, b"total\0\0").unwrap();
        let files = vec![a.clone(), b.clone(), bin];
        let hits = search(&files, "total", false, false).unwrap();
        assert_eq!(hits.len(), 4, "{hits:?}");
        assert_eq!(
            (hits[0].path.clone(), hits[0].line, hits[0].col),
            (a.clone(), 1, 8)
        );
        assert_eq!(hits[0].preview, "let Total = 1;");
        assert_eq!(
            &hits[0].preview[hits[0].preview_at.0..hits[0].preview_at.1],
            "Total"
        );
        // The case, when it counts.
        assert_eq!(search(&files, "Total", true, false).unwrap().len(), 1);
        // A regular expression, and one that is not.
        assert_eq!(
            search(&files, r"total\s+\w+", false, true).unwrap().len(),
            1
        );
        assert!(search(&files, "(", false, true).is_err());
        assert_eq!(search(&files, "(", false, false).unwrap().len(), 1);
        assert!(search(&files, "", false, false).unwrap().is_empty());
    }

    #[test]
    fn a_long_line_is_shown_around_the_match() {
        let line = format!("{}needle{}", "a".repeat(300), "b".repeat(300));
        let (text, (from, to)) = excerpt(&line, 300, 6);
        assert!(text.len() < 200 && text.starts_with('…'));
        assert_eq!(&text[from..to], "needle");
    }

    #[test]
    fn non_project_items_include_what_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("node_modules/lib")).unwrap();
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        std::fs::write(dir.path().join("node_modules/lib/index.js"), "").unwrap();
        std::fs::write(dir.path().join(".git/HEAD"), "").unwrap();
        std::fs::write(dir.path().join("a.ts"), "").unwrap();
        let all = list_all_files(dir.path());
        assert!(all.contains(&dir.path().join("node_modules/lib/index.js")));
        assert!(all.contains(&dir.path().join("a.ts")));
        assert!(!all.iter().any(|p| p.starts_with(dir.path().join(".git"))));
        assert!(!list_files(dir.path(), &[], true)
            .contains(&dir.path().join("node_modules/lib/index.js")));
    }
}
