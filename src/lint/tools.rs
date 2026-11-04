//! The linters known for each language, and how each one is run.

use std::path::{Path, PathBuf};

/// How a tool reports, so that its output is read the right way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Eslint,
    Ruff,
    Flake8,
    Pylint,
    Mypy,
    ShellCheck,
    Stylelint,
    Rubocop,
    YamlLint,
    MarkdownLint,
    Cppcheck,
    GoVet,
    GolangCi,
    Php,
    Luacheck,
    Ktlint,
    Swiftlint,
    SqlFluff,
    Hlint,
    Xmllint,
    Clippy,
    /// Checked by the program itself.
    Json,
}

pub struct Tool {
    pub id: &'static str,
    pub name: &'static str,
    pub program: &'static str,
    /// `{file}` is the file's path, `{shell}` the shell of a script.
    pub args: &'static [&'static str],
    /// The text as typed is piped in (otherwise the saved file is read).
    pub stdin: bool,
    pub format: Format,
    /// Run from the folder of the file (otherwise from the project's root).
    pub in_file_dir: bool,
    /// Files, one of which must be in the project for the tool to be used
    /// without being asked (`file#key` looks for a key of a JSON file).
    pub config: &'static [&'static str],
    /// Used when nothing is chosen by hand.
    pub auto: bool,
    /// Arguments that make the tool print the file with every fix applied.
    pub fix_all: Option<&'static [&'static str]>,
    /// Longest the tool may run, in seconds.
    pub timeout: u64,
}

const fn tool(
    id: &'static str,
    name: &'static str,
    program: &'static str,
    args: &'static [&'static str],
    format: Format,
) -> Tool {
    Tool {
        id,
        name,
        program,
        args,
        stdin: true,
        format,
        in_file_dir: false,
        config: &[],
        auto: true,
        fix_all: None,
        timeout: 60,
    }
}

pub const TOOLS: &[Tool] = &[
    Tool {
        config: &[
            "eslint.config.js",
            "eslint.config.mjs",
            "eslint.config.cjs",
            "eslint.config.ts",
            "eslint.config.mts",
            "eslint.config.cts",
            ".eslintrc",
            ".eslintrc.js",
            ".eslintrc.cjs",
            ".eslintrc.json",
            ".eslintrc.yaml",
            ".eslintrc.yml",
            "package.json#eslintConfig",
        ],
        ..tool(
            "eslint",
            "ESLint",
            "eslint",
            &["-f", "json", "--stdin", "--stdin-filename", "{file}"],
            Format::Eslint,
        )
    },
    tool(
        "ruff",
        "Ruff",
        "ruff",
        &[
            "check",
            "--output-format",
            "json",
            "--no-cache",
            "--stdin-filename",
            "{file}",
            "-",
        ],
        Format::Ruff,
    ),
    Tool {
        auto: false,
        ..tool(
            "flake8",
            "Flake8",
            "flake8",
            &[
                "--format=%(row)d:%(col)d:%(code)s:%(text)s",
                "--stdin-display-name",
                "{file}",
                "-",
            ],
            Format::Flake8,
        )
    },
    Tool {
        auto: false,
        ..tool(
            "pycodestyle",
            "pycodestyle",
            "pycodestyle",
            &["--format=%(row)d:%(col)d:%(code)s:%(text)s", "-"],
            Format::Flake8,
        )
    },
    Tool {
        auto: false,
        ..tool(
            "pylint",
            "Pylint",
            "pylint",
            &["--output-format=json", "--from-stdin", "{file}"],
            Format::Pylint,
        )
    },
    Tool {
        auto: false,
        stdin: false,
        timeout: 120,
        ..tool(
            "mypy",
            "mypy",
            "mypy",
            &[
                "--show-column-numbers",
                "--no-error-summary",
                "--no-pretty",
                "--no-color-output",
                "{file}",
            ],
            Format::Mypy,
        )
    },
    Tool {
        auto: false,
        stdin: false,
        timeout: 300,
        ..tool(
            "clippy",
            "Clippy",
            "cargo",
            &["clippy", "--message-format=json", "--quiet"],
            Format::Clippy,
        )
    },
    Tool {
        auto: false,
        stdin: false,
        in_file_dir: true,
        ..tool("go-vet", "go vet", "go", &["vet", "."], Format::GoVet)
    },
    Tool {
        auto: false,
        stdin: false,
        in_file_dir: true,
        timeout: 120,
        ..tool(
            "golangci-lint",
            "golangci-lint",
            "golangci-lint",
            &["run", "--out-format", "json", "{file}"],
            Format::GolangCi,
        )
    },
    Tool {
        stdin: false,
        ..tool(
            "cppcheck",
            "cppcheck",
            "cppcheck",
            &[
                "--enable=warning,style,performance,portability",
                "--template={line}:{column}:{severity}:{id}:{message}",
                "--quiet",
                "{file}",
            ],
            Format::Cppcheck,
        )
    },
    tool(
        "shellcheck",
        "ShellCheck",
        "shellcheck",
        &["-f", "json1", "-s", "{shell}", "-"],
        Format::ShellCheck,
    ),
    Tool {
        config: &[
            ".stylelintrc",
            ".stylelintrc.json",
            ".stylelintrc.yaml",
            ".stylelintrc.yml",
            ".stylelintrc.js",
            ".stylelintrc.cjs",
            ".stylelintrc.mjs",
            "stylelint.config.js",
            "stylelint.config.cjs",
            "stylelint.config.mjs",
            "package.json#stylelint",
        ],
        fix_all: Some(&["--fix", "--stdin", "--stdin-filename", "{file}"]),
        ..tool(
            "stylelint",
            "Stylelint",
            "stylelint",
            &[
                "--formatter",
                "json",
                "--stdin",
                "--stdin-filename",
                "{file}",
            ],
            Format::Stylelint,
        )
    },
    tool(
        "yamllint",
        "yamllint",
        "yamllint",
        &["-f", "parsable", "-"],
        Format::YamlLint,
    ),
    tool(
        "markdownlint",
        "markdownlint",
        "markdownlint",
        &["--stdin"],
        Format::MarkdownLint,
    ),
    Tool {
        fix_all: Some(&["-a", "--format", "quiet", "--stdin", "{file}"]),
        ..tool(
            "rubocop",
            "RuboCop",
            "rubocop",
            &["--format", "json", "--stdin", "{file}"],
            Format::Rubocop,
        )
    },
    tool(
        "php",
        "php -l",
        "php",
        &["-l", "-d", "display_errors=1"],
        Format::Php,
    ),
    tool(
        "luacheck",
        "luacheck",
        "luacheck",
        &[
            "--formatter",
            "plain",
            "--codes",
            "--ranges",
            "--filename",
            "{file}",
            "-",
        ],
        Format::Luacheck,
    ),
    tool(
        "ktlint",
        "ktlint",
        "ktlint",
        &["--reporter=json", "--stdin"],
        Format::Ktlint,
    ),
    tool(
        "swiftlint",
        "SwiftLint",
        "swiftlint",
        &["lint", "--reporter", "json", "--use-stdin", "--quiet"],
        Format::Swiftlint,
    ),
    tool(
        "sqlfluff",
        "SQLFluff",
        "sqlfluff",
        &["lint", "--format", "json", "--dialect", "ansi", "-"],
        Format::SqlFluff,
    ),
    tool("hlint", "HLint", "hlint", &["--json", "-"], Format::Hlint),
    tool(
        "xmllint",
        "xmllint",
        "xmllint",
        &["--noout", "-"],
        Format::Xmllint,
    ),
    tool("json", "JSON", "", &[], Format::Json),
];

/// Language servers give their problems too: the tool with this id.
pub const LSP: &str = "lsp";

/// The languages the tools are chosen for.
pub struct Group {
    pub id: &'static str,
    pub name: &'static str,
    pub exts: &'static [&'static str],
    pub tools: &'static [&'static str],
}

pub const GROUPS: &[Group] = &[
    Group {
        id: "js",
        name: "JavaScript / TypeScript",
        exts: &["js", "jsx", "mjs", "cjs", "ts", "tsx", "mts", "cts"],
        tools: &["eslint"],
    },
    Group {
        id: "py",
        name: "Python",
        exts: &["py", "pyi"],
        tools: &["ruff", "flake8", "pycodestyle", "pylint", "mypy"],
    },
    Group {
        id: "rs",
        name: "Rust",
        exts: &["rs"],
        tools: &["clippy"],
    },
    Group {
        id: "go",
        name: "Go",
        exts: &["go"],
        tools: &["go-vet", "golangci-lint"],
    },
    Group {
        id: "c",
        name: "C / C++",
        exts: &["c", "h", "cpp", "cc", "cxx", "hpp", "hh", "hxx"],
        tools: &["cppcheck"],
    },
    Group {
        id: "sh",
        name: "Shell",
        exts: &["sh", "bash"],
        tools: &["shellcheck"],
    },
    Group {
        id: "css",
        name: "CSS / SCSS / Less",
        exts: &["css", "scss", "less"],
        tools: &["stylelint"],
    },
    Group {
        id: "json",
        name: "JSON",
        exts: &["json"],
        tools: &["json"],
    },
    Group {
        id: "yaml",
        name: "YAML",
        exts: &["yaml", "yml"],
        tools: &["yamllint"],
    },
    Group {
        id: "md",
        name: "Markdown",
        exts: &["md", "markdown"],
        tools: &["markdownlint"],
    },
    Group {
        id: "rb",
        name: "Ruby",
        exts: &["rb"],
        tools: &["rubocop"],
    },
    Group {
        id: "php",
        name: "PHP",
        exts: &["php"],
        tools: &["php"],
    },
    Group {
        id: "lua",
        name: "Lua",
        exts: &["lua"],
        tools: &["luacheck"],
    },
    Group {
        id: "kt",
        name: "Kotlin",
        exts: &["kt", "kts"],
        tools: &["ktlint"],
    },
    Group {
        id: "swift",
        name: "Swift",
        exts: &["swift"],
        tools: &["swiftlint"],
    },
    Group {
        id: "sql",
        name: "SQL",
        exts: &["sql"],
        tools: &["sqlfluff"],
    },
    Group {
        id: "hs",
        name: "Haskell",
        exts: &["hs"],
        tools: &["hlint"],
    },
    Group {
        id: "xml",
        name: "XML",
        exts: &["xml"],
        tools: &["xmllint"],
    },
    Group {
        id: "java",
        name: "Java",
        exts: &["java"],
        tools: &[],
    },
    Group {
        id: "cs",
        name: "C#",
        exts: &["cs"],
        tools: &[],
    },
    Group {
        id: "dart",
        name: "Dart / Zig",
        exts: &["dart", "zig"],
        tools: &[],
    },
];

pub fn tool_by_id(id: &str) -> Option<&'static Tool> {
    TOOLS.iter().find(|t| t.id == id)
}

/// The group a file extension belongs to.
pub fn group_of(ext: &str) -> Option<&'static Group> {
    let ext = ext.to_ascii_lowercase();
    GROUPS.iter().find(|g| g.exts.contains(&ext.as_str()))
}

/// Whether a language server may serve this extension.
pub fn has_server(ext: &str) -> bool {
    crate::lsp::spec_for(&ext.to_ascii_lowercase()).is_some()
}

/// Whether one is installed too.
fn server_ready(ext: &str) -> bool {
    has_server(ext) && crate::lsp::installed(ext)
}

/// Whether `tool` finds what it needs near `path`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Availability {
    Ready(PathBuf),
    /// The program is not installed.
    NoProgram,
    /// It is, but the project has no configuration for it.
    NoConfig(PathBuf),
}

/// Where the program of a tool is: the project's own copy first
/// (`node_modules/.bin`, a virtualenv), then the system's.
pub fn program_path(tool: &Tool, near: &Path, chosen: Option<&str>) -> Option<PathBuf> {
    if tool.program.is_empty() {
        return Some(PathBuf::new());
    }
    // A path chosen by the user wins, and is the only one tried.
    if let Some(path) = chosen.map(str::trim).filter(|p| !p.is_empty()) {
        let path = PathBuf::from(path);
        return path.is_file().then_some(path);
    }
    let dirs = near
        .ancestors()
        .skip(1)
        .take(12)
        .flat_map(|d| {
            ["node_modules/.bin", ".venv/bin", "venv/bin", "vendor/bin"]
                .into_iter()
                .map(move |sub| d.join(sub).join(tool.program))
        })
        .collect::<Vec<_>>();
    dirs.into_iter()
        .find(|p| p.is_file())
        .or_else(|| crate::lsp::find_executable(tool.program))
}

/// The nearest folder above `near` that holds a configuration of the tool.
pub fn config_dir(tool: &Tool, near: &Path) -> Option<PathBuf> {
    near.ancestors()
        .skip(1)
        .take(12)
        .find(|dir| {
            tool.config.iter().any(|entry| match entry.split_once('#') {
                Some((file, key)) => std::fs::read_to_string(dir.join(file))
                    .is_ok_and(|text| text.contains(&format!("\"{key}\""))),
                None => dir.join(entry).exists(),
            })
        })
        .map(Path::to_path_buf)
}

fn has_config(tool: &Tool, near: &Path) -> bool {
    tool.config.is_empty() || config_dir(tool, near).is_some()
}

pub fn availability(tool: &Tool, near: &Path, chosen: Option<&str>) -> Availability {
    match program_path(tool, near, chosen) {
        None => Availability::NoProgram,
        Some(program) if has_config(tool, near) => Availability::Ready(program),
        Some(program) => Availability::NoConfig(program),
    }
}

/// What to run for a file: the tools, and whether the language server is
/// asked too. `manual` is the choice made in the Preferences for the
/// language (none: everything that is ready by itself).
pub struct Plan {
    pub tools: Vec<(&'static Tool, PathBuf)>,
    pub lsp: bool,
}

pub fn plan(
    path: &Path,
    ext: &str,
    manual: Option<&[String]>,
    programs: &std::collections::BTreeMap<String, String>,
) -> Plan {
    let Some(group) = group_of(ext) else {
        return Plan {
            tools: Vec::new(),
            lsp: manual.is_none_or(|m| m.iter().any(|id| id == LSP)) && server_ready(ext),
        };
    };
    let mut tools = Vec::new();
    for id in group.tools {
        let Some(tool) = tool_by_id(id) else {
            continue;
        };
        let chosen = match manual {
            Some(list) => list.iter().any(|m| m == id),
            None => tool.auto,
        };
        if !chosen {
            continue;
        }
        match availability(tool, path, programs.get(*id).map(String::as_str)) {
            Availability::Ready(program) => tools.push((tool, program)),
            // Asked for by hand: it runs although the project has no setup.
            Availability::NoConfig(program) if manual.is_some() => tools.push((tool, program)),
            _ => {}
        }
    }
    let lsp = manual.is_none_or(|m| m.iter().any(|id| id == LSP)) && server_ready(ext);
    Plan { tools, lsp }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_extension_belongs_to_one_group_and_every_tool_is_known() {
        let mut seen = std::collections::HashSet::new();
        for group in GROUPS {
            for ext in group.exts {
                assert!(seen.insert(*ext), "{ext} is in two groups");
            }
            for id in group.tools {
                assert!(tool_by_id(id).is_some(), "{id} is not a tool");
            }
        }
        let mut ids = std::collections::HashSet::new();
        assert!(TOOLS.iter().all(|t| ids.insert(t.id)));
        assert_eq!(group_of("TSX").map(|g| g.id), Some("js"));
        assert!(group_of("zzz").is_none());
        assert!(has_server("ts") && !has_server("json"));
    }

    #[test]
    fn a_tool_needing_a_setup_waits_for_it() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("node_modules/.bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("eslint"), "").unwrap();
        let file = dir.path().join("src/a.ts");
        let eslint = tool_by_id("eslint").unwrap();
        let none = std::collections::BTreeMap::new();
        // The project's own program is found, but it has no configuration.
        assert!(matches!(
            availability(eslint, &file, None),
            Availability::NoConfig(_)
        ));
        let auto = plan(&file, "ts", None, &none);
        assert!(auto.tools.is_empty());
        // By hand it runs anyway.
        let by_hand = plan(&file, "ts", Some(&["eslint".to_string()]), &none);
        assert_eq!(by_hand.tools.len(), 1);
        assert!(!by_hand.lsp, "the server was not asked for");
        std::fs::write(dir.path().join("eslint.config.js"), "").unwrap();
        assert!(matches!(
            availability(eslint, &file, None),
            Availability::Ready(_)
        ));
        assert_eq!(plan(&file, "ts", None, &none).tools.len(), 1);
        // A key of package.json counts as a configuration too.
        std::fs::remove_file(dir.path().join("eslint.config.js")).unwrap();
        std::fs::write(dir.path().join("package.json"), r#"{"eslintConfig":{}}"#).unwrap();
        assert!(matches!(
            availability(eslint, &file, None),
            Availability::Ready(_)
        ));
    }

    #[test]
    fn a_path_chosen_by_the_user_wins_over_the_search() {
        let dir = tempfile::tempdir().unwrap();
        let mine = dir.path().join("my-flake8");
        std::fs::write(&mine, "").unwrap();
        let flake8 = tool_by_id("flake8").unwrap();
        let near = Path::new("/tmp/none/a.py");
        let chosen = mine.to_string_lossy().into_owned();
        assert_eq!(
            program_path(flake8, near, Some(&chosen)),
            Some(mine.clone())
        );
        // A wrong path is not replaced by another one: the tool is missing.
        assert_eq!(program_path(flake8, near, Some("/nowhere/flake8")), None);
        let mut programs = std::collections::BTreeMap::new();
        programs.insert("flake8".to_string(), chosen);
        let by_hand = plan(near, "py", Some(&["flake8".to_string()]), &programs);
        assert_eq!(by_hand.tools[0].1, mine);
    }

    #[test]
    fn json_is_always_checked_and_other_tools_wait_to_be_asked() {
        let file = Path::new("/tmp/none/a.json");
        assert_eq!(plan(file, "json", None, &Default::default()).tools.len(), 1);
        assert!(plan(file, "json", Some(&[]), &Default::default())
            .tools
            .is_empty());
        // Python's alternatives to ruff are not run unless chosen.
        let py = plan(Path::new("/tmp/none/a.py"), "py", None, &Default::default());
        assert!(py.tools.iter().all(|(t, _)| t.id == "ruff"));
    }
}
