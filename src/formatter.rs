use std::io::Write;
use std::process::{Command, Stdio};

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

pub async fn format(content: String, ext: String) -> Result<String, String> {
    let (cmd, args) = command_for(&ext)
        .ok_or_else(|| "No formatter available for this file type.".to_string())?;

    // Try the direct command first, then fall back to npx (for prettier & friends).
    let mut child = Command::new(&cmd)
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .or_else(|_| {
            // Fallback: run via `npx --yes <cmd> <args>` (works if Node/npm is installed)
            let mut npx_args = vec!["--yes".to_string(), cmd.clone()];
            npx_args.extend(args.iter().cloned());
            Command::new("npx")
                .args(&npx_args)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
        })
        .map_err(|_| {
            format!(
                "'{}' not found. Install it or ensure Node/npm is available.",
                cmd
            )
        })?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(content.as_bytes())
            .map_err(|e| e.to_string())?;
    }

    let output = child.wait_with_output().map_err(|e| e.to_string())?;

    if output.status.success() {
        String::from_utf8(output.stdout).map_err(|e| e.to_string())
    } else {
        let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(if err.is_empty() {
            format!("{} failed.", cmd)
        } else {
            let first = err.lines().next().unwrap_or(&err);
            // Strip tool prefixes like "[error] f.html: " to keep the message short
            let msg = first
                .trim_start_matches("[error] ")
                .trim_start_matches("[warn] ");
            let msg = if let Some(pos) = msg.find(": ") {
                &msg[pos + 2..]
            } else {
                msg
            };
            // Cap at 120 chars so it fits the status bar
            if msg.len() > 120 {
                format!("{}…", &msg[..120])
            } else {
                msg.to_string()
            }
        })
    }
}
