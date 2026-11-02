//! "Go to definition" (Ctrl/Cmd+click) without a language server: the way
//! used until one is installed for the language (see `lsp`).
//!
//! The editor knows no language grammar, so a definition is recognised by how
//! declarations look across languages (`fn name`, `def name`, `class Name`,
//! `let name =`, `int name(…) {`, …). The search goes from the nearest place
//! outwards: the current buffer, the other open files, then the project
//! (the folders around the file). It is a heuristic: it can miss a
//! definition or pick a look-alike; a language server is exact.

use regex::Regex;
use std::path::{Path, PathBuf};

/// A file that may hold a definition: its text if it is open with unsaved
/// edits, otherwise read from disk.
pub type Source = (PathBuf, Option<String>);

/// Where a name is defined.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub path: PathBuf,
    pub line: usize,
    /// Byte offset of the name within its line.
    pub col: usize,
    /// Byte length of the name when it is on one line, else 0.
    pub len: usize,
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The identifier under byte offset `col` of `line` (or touching it).
pub fn word_at(line: &str, col: usize) -> Option<&str> {
    let col = col.min(line.len());
    if !line.is_char_boundary(col) {
        return None;
    }
    let start = line[..col]
        .char_indices()
        .rev()
        .take_while(|&(_, c)| is_word_char(c))
        .last()
        .map_or(col, |(i, _)| i);
    let end = line[col..]
        .char_indices()
        .find(|&(_, c)| !is_word_char(c))
        .map_or(line.len(), |(i, _)| col + i);
    let word = &line[start..end];
    // Numbers (`42`, `0xff`) are not names.
    (!word.is_empty() && !word.starts_with(|c: char| c.is_ascii_digit())).then_some(word)
}

const PROJECT_MARKERS: &[&str] = &[
    ".git",
    "Cargo.toml",
    "package.json",
    "go.mod",
    "pyproject.toml",
    "setup.py",
    "pom.xml",
    "build.gradle",
    "build.gradle.kts",
    "composer.json",
    "Gemfile",
    "CMakeLists.txt",
    "Makefile",
    "mix.exs",
    "pubspec.yaml",
    "Package.swift",
    "deno.json",
    "tsconfig.json",
];

/// The folder that holds the project of `file`: the nearest parent with a
/// project marker, or the file's own folder.
pub fn project_root(file: &Path) -> Option<PathBuf> {
    let home = dirs::home_dir();
    let start = file.parent()?;
    let marked = start
        .ancestors()
        .take_while(|dir| Some(*dir) != home.as_deref() && dir.parent().is_some())
        .find(|dir| PROJECT_MARKERS.iter().any(|m| dir.join(m).exists()));
    Some(marked.unwrap_or(start).to_path_buf())
}

/// Declaration keywords of a function, type or module.
const DECLARATION: &str = "fn|func|function|def|defn|defun|defmacro|defstruct|defrecord|\
defprotocol|defmulti|defmethod|defgeneric|class|struct|enum|trait|interface|type|typedef|\
module|mod|namespace|protocol|record|union|object|sub|proc|fun|macro_rules!|macro|message|\
service|rpc|define|data|newtype|template|subroutine|program|procedure|entity|package|\
extension|actor|contract|event|modifier|typealias|alias|component";

/// Declaration keywords of a variable or constant.
const VARIABLE: &str =
    "let|var|val|const|local|my|our|readonly|auto|global|static|final|def|defvar|defparameter";

/// How a name is declared; the lower, the surer.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Tier {
    /// `fn name`, `class Name`, …
    Keyword,
    /// `let name`, `const name`, `#define NAME`, …
    Variable,
    /// `int name(…) {`, `name(…) {`, `name = function`, `name = () =>`.
    Signature,
    /// `name = value` at the start of a line.
    Assignment,
}

/// Words that precede a call, never a return type.
const NOT_A_TYPE: &[&str] = &[
    "return", "else", "new", "delete", "throw", "case", "await", "yield", "typeof", "in", "and",
    "or", "not", "if", "while", "for", "do", "switch", "echo", "print", "goto", "elif", "puts",
];

struct Patterns {
    keyword: Regex,
    go_method: Regex,
    variable: Regex,
    define: Regex,
    typed_signature: Regex,
    method: Regex,
    function_value: Regex,
    assignment: Regex,
}

impl Patterns {
    fn new(word: &str) -> Option<Self> {
        let w = regex::escape(word);
        let build = |pattern: String| Regex::new(&pattern).ok();
        Some(Self {
            keyword: build(format!(
                r"\b(?:{DECLARATION})\s+(?:\(\s*)?(?:mut\s+)?(?:self\.)?(?P<n>{w})\b"
            ))?,
            go_method: build(format!(r"\bfunc\s*\([^)]*\)\s*(?P<n>{w})\b"))?,
            variable: build(format!(
                r"\b(?:{VARIABLE})\s+(?:mut\s+|readonly\s+)?[\$@%]?(?P<n>{w})\b"
            ))?,
            define: build(format!(r"#\s*(?:define|macro)\s+(?P<n>{w})\b"))?,
            typed_signature: build(format!(
                r"^\s*(?P<prefix>[A-Za-z_][\w:<>,\[\]\*&\s]*?(?:[\s\*&]|::))(?P<n>{w})\s*\([^;]*$"
            ))?,
            method: build(format!(
                r"^\s*(?:(?:pub|public|private|protected|internal|static|async|override|virtual|final|export|abstract|get|set)\s+)*(?P<n>{w})\s*\([^)]*\)\s*(?:->[^{{]*|:[^{{]*)?\{{\s*$"
            ))?,
            function_value: build(format!(
                r"\b(?P<n>{w})\s*[:=]\s*(?:async\s*)?(?:function\b|\([^)]*\)\s*=>|\w+\s*=>)"
            ))?,
            assignment: build(format!(
                r"^\s*(?:export\s+|global\s+)?\$?(?P<n>{w})\s*(?::=|=)[^=]"
            ))?,
        })
    }

    /// The strongest way `line` declares the name, with its offset.
    fn declaration(&self, line: &str) -> Option<(Tier, usize)> {
        let col = |re: &Regex| {
            re.captures(line)
                .and_then(|c| c.name("n"))
                .map(|m| m.start())
        };
        if let Some(c) = col(&self.keyword).or_else(|| col(&self.go_method)) {
            return Some((Tier::Keyword, c));
        }
        if let Some(c) = col(&self.variable).or_else(|| col(&self.define)) {
            return Some((Tier::Variable, c));
        }
        if let Some(caps) = self.typed_signature.captures(line) {
            let first = caps["prefix"].split_whitespace().next().unwrap_or("");
            // A line ending in `,` is an argument list of a call.
            if !NOT_A_TYPE.contains(&first) && !line.trim_end().ends_with(',') {
                return Some((Tier::Signature, caps.name("n")?.start()));
            }
        }
        if let Some(c) = col(&self.method).or_else(|| col(&self.function_value)) {
            return Some((Tier::Signature, c));
        }
        col(&self.assignment).map(|c| (Tier::Assignment, c))
    }
}

/// The best definition of `word` in `text`: the surest kind, then the
/// nearest to line `near`. `skip` is the place being clicked on.
fn best_in(
    patterns: &Patterns,
    word: &str,
    text: &str,
    near: usize,
    skip: Option<(usize, usize)>,
) -> Option<(usize, usize)> {
    text.lines()
        .enumerate()
        .filter(|(_, line)| line.contains(word))
        .filter_map(|(i, line)| {
            let (tier, col) = patterns.declaration(line)?;
            Some((i, col, tier))
        })
        .filter(|&(line, col, _)| skip != Some((line, col)))
        .min_by_key(|&(line, _, tier)| (tier, line.abs_diff(near)))
        .map(|(line, col, _)| (line, col))
}

/// Finds where the identifier under the cursor of `current` is defined.
/// Looks in `current` first, then in `open`, then in the project files.
pub fn locate(
    word: &str,
    current: &Source,
    cursor_line: usize,
    cursor_col: usize,
    open: &[Source],
) -> Option<Hit> {
    let patterns = Patterns::new(word)?;
    let hit = |path: &Path, (line, col): (usize, usize)| Hit {
        path: path.to_path_buf(),
        line,
        col,
        len: word.len(),
    };

    // The definition being clicked on is not "the" answer.
    let clicked = word_start(current, cursor_line, cursor_col);
    let current_text = current.1.clone().or_else(|| read(&current.0))?;
    if let Some(found) = best_in(&patterns, word, &current_text, cursor_line, clicked) {
        return Some(hit(&current.0, found));
    }

    let mut seen = vec![current.0.clone()];
    let search = |path: &Path, text: Option<&String>| -> Option<Hit> {
        let owned;
        let text = match text {
            Some(text) => text,
            None => {
                owned = read(path)?;
                &owned
            }
        };
        best_in(&patterns, word, text, 0, None).map(|found| hit(path, found))
    };

    for (path, text) in open {
        if seen.contains(path) {
            continue;
        }
        seen.push(path.clone());
        if let Some(found) = search(path, text.as_ref()) {
            return Some(found);
        }
    }

    for path in project_files(&current.0) {
        if seen.contains(&path) {
            continue;
        }
        if let Some(found) = search(&path, None) {
            return Some(found);
        }
    }
    None
}

/// The (line, col) of the word under the cursor.
fn word_start(current: &Source, line: usize, col: usize) -> Option<(usize, usize)> {
    let text = current.1.as_ref()?;
    let text_line = text.lines().nth(line)?;
    let word = word_at(text_line, col)?;
    let start = word.as_ptr() as usize - text_line.as_ptr() as usize;
    Some((line, start))
}

const MAX_FILE_BYTES: u64 = 1 << 20;
const MAX_ENTRIES: usize = 20_000;

const SKIPPED_DIRS: &[&str] = &[
    "node_modules",
    "target",
    "vendor",
    "dist",
    "build",
    "out",
    "__pycache__",
    "venv",
    "env",
    "site-packages",
    "bower_components",
    "Pods",
];

const BINARY_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "bmp", "ico", "webp", "svg", "pdf", "zip", "gz", "tar", "xz",
    "bz2", "7z", "rar", "so", "o", "a", "dll", "dylib", "exe", "class", "jar", "pyc", "wasm",
    "ttf", "otf", "woff", "woff2", "mp3", "mp4", "mov", "avi", "mkv", "lock", "map", "min", "db",
    "sqlite", "bin", "iso", "deb", "rpm", "snap",
];

/// The text files of the project of `file`, those with its extension first.
fn project_files(file: &Path) -> Vec<PathBuf> {
    let Some(root) = project_root(file) else {
        return Vec::new();
    };
    let mut files = Vec::new();
    let mut dirs = vec![root];
    let mut visited = 0;
    while let Some(dir) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            visited += 1;
            if visited > MAX_ENTRIES {
                break;
            }
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if name.starts_with('.') || kind.is_symlink() {
                continue;
            }
            let path = entry.path();
            if kind.is_dir() {
                if !SKIPPED_DIRS.contains(&&*name) {
                    dirs.push(path);
                }
            } else if kind.is_file() && is_candidate(&path) {
                files.push(path);
            }
        }
    }
    let wanted = file.extension();
    files.sort_by_key(|p| (p.extension() != wanted, p.clone()));
    files
}

fn is_candidate(path: &Path) -> bool {
    let binary = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| BINARY_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()));
    !binary && std::fs::metadata(path).is_ok_and(|m| m.len() <= MAX_FILE_BYTES)
}

/// The text of a file, `None` when it can't be read as text.
fn read(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_under_the_cursor() {
        assert_eq!(word_at("let foo_bar = 1;", 6), Some("foo_bar"));
        assert_eq!(word_at("let foo_bar = 1;", 4), Some("foo_bar"));
        assert_eq!(word_at("let foo_bar = 1;", 11), Some("foo_bar"));
        assert_eq!(word_at("a + b", 2), None);
        assert_eq!(word_at("x = 42", 5), None);
        assert_eq!(word_at("été = 1", 2), Some("été"));
        // Inside a multi-byte character: not a cursor position.
        assert_eq!(word_at("été = 1", 1), None);
        assert_eq!(word_at("", 0), None);
    }

    #[test]
    fn the_project_is_the_folder_with_a_marker() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "").unwrap();
        std::fs::create_dir(dir.path().join("src")).unwrap();
        let file = dir.path().join("src/main.rs");
        assert_eq!(project_root(&file).as_deref(), Some(dir.path()));
    }

    fn find(word: &str, text: &str) -> Option<(usize, usize)> {
        let patterns = Patterns::new(word).unwrap();
        best_in(&patterns, word, text, 0, None)
    }

    #[test]
    fn keyword_declarations_in_many_languages() {
        let cases = [
            ("pub async fn run(x: u32) {", "run"),
            ("export function run() {", "run"),
            ("def run(self):", "run"),
            ("class Run extends Base {", "Run"),
            ("func (s *Server) Run() error {", "Run"),
            ("(defun run (x)", "run"),
            ("struct Run {", "Run"),
            ("type Run struct {", "Run"),
            ("sub run {", "run"),
            ("  fun run() {", "run"),
            ("def self.run", "run"),
        ];
        for (line, word) in cases {
            let col = find(word, line).unwrap_or_else(|| panic!("{line}")).1;
            assert_eq!(&line[col..col + word.len()], word, "{line}");
        }
    }

    #[test]
    fn variables_signatures_and_assignments() {
        assert!(find("count", "let mut count = 0;").is_some());
        assert!(find("MAX", "#define MAX 10").is_some());
        assert!(find("run", "static int run(int a, int b)\n{").is_some());
        assert!(find("run", "void Foo::run() {").is_some());
        assert!(find("run", "  run(a, b) {").is_some());
        assert!(find("run", "const run = (a) => a;").is_some());
        assert!(find("run", "run = function() {}").is_some());
        assert!(find("run", "run = compute(3)").is_some());
        assert!(find("run", "$run = 3;").is_some());
    }

    #[test]
    fn calls_and_comparisons_are_not_definitions() {
        assert_eq!(find("run", "    run(a, b);"), None);
        assert_eq!(find("run", "    return run(a, b);"), None);
        assert_eq!(find("run", "    let x = run(a);"), None);
        assert_eq!(find("run", "if run == 3 {"), None);
        assert_eq!(find("run", "    foo(a, run(b),"), None);
        assert_eq!(find("run", "// nothing here"), None);
        assert_eq!(find("run", "running = 3"), None);
    }

    #[test]
    fn surest_kind_then_nearest() {
        let text = "run = 1\nfn run() {}\nuse(run)";
        assert_eq!(find("run", text), Some((1, 3)));

        let patterns = Patterns::new("x").unwrap();
        let text = "let x = 1;\nfoo();\nlet x = 2;\nbar();\nlet x = 3;";
        assert_eq!(best_in(&patterns, "x", text, 3, None), Some((2, 4)));
        assert_eq!(best_in(&patterns, "x", text, 4, None), Some((4, 4)));
    }

    #[test]
    fn the_clicked_declaration_is_skipped() {
        let patterns = Patterns::new("run").unwrap();
        let text = "fn run() {}\nfn other() { run() }";
        assert_eq!(best_in(&patterns, "run", text, 0, None), Some((0, 3)));
        assert_eq!(best_in(&patterns, "run", text, 0, Some((0, 3))), None);
    }

    #[test]
    fn locates_across_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "").unwrap();
        let main = dir.path().join("main.rs");
        let lib = dir.path().join("lib.rs");
        let other = dir.path().join("other.rs");
        std::fs::write(&main, "fn main() { helper(); }").unwrap();
        std::fs::write(&lib, "// lib\npub fn helper() {}\n").unwrap();
        std::fs::write(&other, "fn elsewhere() {}\n").unwrap();
        let current: Source = (main.clone(), None);
        let at = |path: &PathBuf, line, col| Hit {
            path: path.clone(),
            line,
            col,
            len: 6,
        };

        // From the project folder.
        assert_eq!(locate("helper", &current, 0, 12, &[]), Some(at(&lib, 1, 7)));

        // From an open file, with its unsaved text.
        let open = vec![(other.clone(), Some("fn helper() {}".to_string()))];
        assert_eq!(
            locate("helper", &current, 0, 12, &open),
            Some(at(&other, 0, 3))
        );

        // In the current buffer, whose text may differ from the disk.
        let current: Source = (main.clone(), Some("fn helper() {}\nhelper();".into()));
        assert_eq!(locate("helper", &current, 1, 2, &[]), Some(at(&main, 0, 3)));

        assert_eq!(locate("nowhere", &current, 1, 2, &[]), None);
    }
}
