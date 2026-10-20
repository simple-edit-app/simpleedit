use std::process::Stdio;

/// The tool and arguments for a language token. `name` is the file name the
/// tool should believe it formats (it picks the parser and the project
/// configuration from it).
fn command_for(token: &str, name: &str) -> Option<(String, Vec<String>)> {
    match token {
        "js" | "jsx" | "ts" | "tsx" | "css" | "scss" | "less" | "html" | "json" | "yaml" | "md" => {
            Some((
                "prettier".into(),
                vec!["--stdin-filepath".into(), name.to_string()],
            ))
        }
        "rs" => Some(("rustfmt".into(), vec!["--edition".into(), "2021".into()])),
        "go" => Some(("gofmt".into(), vec![])),
        "py" => Some(("black".into(), vec!["-".into(), "--quiet".into()])),
        "sh" => Some(("shfmt".into(), vec![])),
        "c" | "cpp" | "java" | "cs" => Some((
            "clang-format".into(),
            vec![format!("--assume-filename={name}")],
        )),
        "xml" => Some(("xmllint".into(), vec!["--format".into(), "-".into()])),
        "toml" => Some(("taplo".into(), vec!["fmt".into(), "-".into()])),
        "lua" => Some(("stylua".into(), vec!["-".into()])),
        "kt" => Some((
            "ktlint".into(),
            vec![
                "--format".into(),
                "--stdin".into(),
                "--log-level=error".into(),
            ],
        )),
        _ => None,
    }
}

/// The extension a formatter should be told for a language token.
fn extension_for(token: &str) -> &str {
    match token {
        "kt" => "kt",
        "make" => "mk",
        other => other,
    }
}

/// Returns true if a formatter is defined for this extension.
/// Does NOT check whether the binary is installed — errors surface at format time.
pub fn has_formatter(token: &str) -> bool {
    command_for(token, "file").is_some()
}

/// Longest a formatter may run before it is killed.
const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

fn spawn(
    program: &str,
    args: &[String],
    dir: Option<&std::path::Path>,
) -> std::io::Result<tokio::process::Child> {
    let mut command = tokio::process::Command::new(program);
    command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    // The tools look for the project's configuration from where they run.
    if let Some(dir) = dir {
        command.current_dir(dir);
    }
    command.spawn()
}

/// Formats `content` written in language `token`. `target` is the file being
/// edited: the formatter runs in its folder, under its name, so it finds the
/// project's own settings (as editors that trust the workspace do).
///
/// prettier is only run if installed; nothing is downloaded.
pub async fn format(
    content: String,
    token: String,
    target: Option<std::path::PathBuf>,
) -> Result<String, String> {
    let dir = target
        .as_deref()
        .and_then(std::path::Path::parent)
        .filter(|d| d.is_dir())
        .map(std::path::Path::to_path_buf);
    let stem = target
        .as_deref()
        .and_then(|p| p.file_stem())
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".to_string());
    let name = format!("{stem}.{}", extension_for(&token));
    let (cmd, args) =
        command_for(&token, &name).ok_or_else(|| t!("format.no_formatter").to_string())?;

    let mut child = spawn(&cmd, &args, dir.as_deref())
        .map_err(|_| t!("format.not_found", tool = cmd.as_str()).to_string())?;

    // Feed stdin from its own task: writing it all before reading stdout
    // deadlocks once the formatter's output pipe fills up.
    let sent = content.len();
    if let Some(mut stdin) = child.stdin.take() {
        tokio::spawn(async move {
            use tokio::io::AsyncWriteExt;
            // A formatter that exits early closes the pipe; its stderr is
            // the useful error, not "broken pipe".
            let _ = stdin.write_all(content.as_bytes()).await;
        });
    }

    let output = tokio::time::timeout(TIMEOUT, child.wait_with_output())
        .await
        .map_err(|_| t!("format.timeout", tool = cmd.as_str()).to_string())?
        .map_err(|e| e.to_string())?;

    if output.status.success() {
        let formatted = String::from_utf8(output.stdout).map_err(|e| e.to_string())?;
        accept_output(sent, formatted, &cmd)
    } else {
        let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(if err.is_empty() {
            t!("format.failed", tool = cmd.as_str()).to_string()
        } else {
            // The whole message: it goes to the error panel.
            err.chars().take(4000).collect()
        })
    }
}

/// An empty result for a non-empty input is a formatter that printed nothing
/// (it checked instead of formatting, or failed quietly): applying it would
/// wipe the buffer.
fn accept_output(sent: usize, formatted: String, tool: &str) -> Result<String, String> {
    if sent > 0 && formatted.trim().is_empty() {
        Err(t!("format.failed", tool = tool).to_string())
    } else {
        Ok(formatted)
    }
}

/// First line of a formatter's stderr, minus tool prefixes like
/// "[error] f.html: ", capped to fit the status bar.
pub fn short_error(err: &str) -> String {
    let first = err.lines().next().unwrap_or(err);
    let msg = first
        .trim_start_matches("[error] ")
        .trim_start_matches("[warn] ");
    let msg = match msg.find(": ") {
        Some(pos) => &msg[pos + 2..],
        None => msg,
    };
    if msg.chars().count() > 120 {
        let cut: String = msg.chars().take(120).collect();
        format!("{cut}…")
    } else {
        msg.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ktlint_is_asked_to_format_not_just_to_check() {
        let (_, args) = command_for("kt", "a.kt").unwrap();
        assert!(args.iter().any(|a| a == "--format"));
    }

    #[test]
    fn tsx_and_jsx_reach_prettier_under_their_own_name() {
        let (tool, args) = command_for("tsx", "App.tsx").unwrap();
        assert_eq!(tool, "prettier");
        assert_eq!(args, ["--stdin-filepath", "App.tsx"]);
        assert!(has_formatter("tsx") && has_formatter("jsx"));
    }

    #[test]
    fn an_empty_result_for_real_input_is_an_error() {
        assert!(accept_output(10, String::new(), "ktlint").is_err());
        assert!(accept_output(10, "\n".into(), "ktlint").is_err());
        assert_eq!(accept_output(0, String::new(), "x").unwrap(), "");
        assert_eq!(accept_output(3, "a\n".into(), "x").unwrap(), "a\n");
    }

    #[test]
    fn short_error_strips_prefixes_and_truncates_on_char_boundaries() {
        assert_eq!(
            short_error("[error] f.html: Unexpected token"),
            "Unexpected token"
        );
        // 150 two-byte chars: a byte slice at 120 would land mid-character.
        let long = "é".repeat(150);
        let out = short_error(&long);
        assert_eq!(out.chars().count(), 121);
        assert!(out.ends_with('…'));
    }

    #[test]
    fn has_formatter_matches_known_extensions() {
        assert!(has_formatter("rs"));
        assert!(!has_formatter("txt"));
        assert!(!has_formatter("make"));
    }
}
