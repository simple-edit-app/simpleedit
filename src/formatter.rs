use std::process::Stdio;

fn command_for(ext: &str) -> Option<(String, Vec<String>)> {
    match ext {
        "js" | "jsx" | "ts" | "tsx" | "css" | "scss" | "less" | "html" | "htm" | "json"
        | "yaml" | "yml" | "md" | "markdown" => Some((
            "prettier".into(),
            vec!["--stdin-filepath".into(), format!("f.{}", ext)],
        )),
        "rs" => Some(("rustfmt".into(), vec!["--edition".into(), "2021".into()])),
        "go" => Some(("gofmt".into(), vec![])),
        "py" => Some(("black".into(), vec!["-".into(), "--quiet".into()])),
        "sh" | "bash" | "zsh" | "fish" => Some(("shfmt".into(), vec![])),
        "c" | "h" => Some((
            "clang-format".into(),
            vec!["--assume-filename=file.c".into()],
        )),
        "cpp" | "cc" | "cxx" | "hpp" | "hh" => Some((
            "clang-format".into(),
            vec!["--assume-filename=file.cpp".into()],
        )),
        "java" => Some((
            "clang-format".into(),
            vec!["--assume-filename=file.java".into()],
        )),
        "xml" | "svg" | "plist" => Some(("xmllint".into(), vec!["--format".into(), "-".into()])),
        "toml" => Some(("taplo".into(), vec!["fmt".into(), "-".into()])),
        "lua" => Some(("stylua".into(), vec!["-".into()])),
        "kt" | "kts" => Some(("ktlint".into(), vec!["--stdin".into()])),
        _ => None,
    }
}

/// Returns true if a formatter is defined for this extension.
/// Does NOT check whether the binary is installed — errors surface at format time.
pub fn has_formatter(ext: &str) -> bool {
    command_for(ext).is_some()
}

/// Longest a formatter may run before it is killed.
const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

fn spawn(program: &str, args: &[String]) -> std::io::Result<tokio::process::Child> {
    tokio::process::Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
}

pub async fn format(content: String, ext: String) -> Result<String, String> {
    let (cmd, args) = command_for(&ext).ok_or_else(|| t!("format.no_formatter").to_string())?;

    // Only prettier may fall back to npx. For any other tool, `npx --yes
    // <name>` would download whatever npm package happens to carry that
    // name and run it.
    let mut child = match spawn(&cmd, &args) {
        Ok(child) => child,
        Err(_) if cmd == "prettier" => {
            let mut npx_args = vec!["--yes".to_string(), cmd.clone()];
            npx_args.extend(args.iter().cloned());
            spawn("npx", &npx_args)
                .map_err(|_| t!("format.not_found", tool = cmd.as_str()).to_string())?
        }
        Err(_) => return Err(t!("format.not_found", tool = cmd.as_str()).to_string()),
    };

    // Feed stdin from its own task: writing it all before reading stdout
    // deadlocks once the formatter's output pipe fills up.
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
        String::from_utf8(output.stdout).map_err(|e| e.to_string())
    } else {
        let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(if err.is_empty() {
            t!("format.failed", tool = cmd.as_str()).to_string()
        } else {
            short_error(&err)
        })
    }
}

/// First line of a formatter's stderr, minus tool prefixes like
/// "[error] f.html: ", capped to fit the status bar.
fn short_error(err: &str) -> String {
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
    }
}
