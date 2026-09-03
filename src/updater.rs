use std::path::PathBuf;

const RELEASES_API: &str =
    "https://api.github.com/repos/simple-edit-app/simpleedit/releases/latest";

fn is_newer(latest: &str, current: &str) -> bool {
    let parse = |s: &str| -> (u32, u32, u32) {
        let mut p = s.split('.');
        let a = p.next().and_then(|x| x.parse().ok()).unwrap_or(0);
        let b = p.next().and_then(|x| x.parse().ok()).unwrap_or(0);
        let c = p.next().and_then(|x| x.parse().ok()).unwrap_or(0);
        (a, b, c)
    };
    parse(latest) > parse(current)
}

pub async fn check_for_update() -> Result<Option<String>, String> {
    let client = reqwest::Client::builder()
        .user_agent("simpleedit-updater")
        .build()
        .map_err(|e| e.to_string())?;

    let json: serde_json::Value = client
        .get(RELEASES_API)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    let tag = json["tag_name"]
        .as_str()
        .ok_or_else(|| t!("update.err_api").to_string())?;

    let latest = tag.trim_start_matches('v');
    let current = env!("CARGO_PKG_VERSION");

    if is_newer(latest, current) {
        Ok(Some(latest.to_string()))
    } else {
        Ok(None)
    }
}

#[cfg(target_os = "linux")]
fn download_url(version: &str) -> String {
    format!(
        "https://github.com/simple-edit-app/simpleedit/releases/download/v{}/simpleedit_{}-1_amd64.deb",
        version, version
    )
}

#[cfg(target_os = "macos")]
fn download_url(version: &str) -> String {
    format!(
        "https://github.com/simple-edit-app/simpleedit/releases/download/v{}/simpleedit-v{}-aarch64-apple-darwin.tar.gz",
        version, version
    )
}

#[cfg(target_os = "linux")]
fn tmp_path(version: &str) -> PathBuf {
    std::env::temp_dir().join(format!("simpleedit_{}-1_amd64.deb", version))
}

#[cfg(target_os = "macos")]
fn tmp_path(version: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "simpleedit-v{}-aarch64-apple-darwin.tar.gz",
        version
    ))
}

pub async fn download_update(version: String) -> Result<PathBuf, String> {
    let url = download_url(&version);
    let dest = tmp_path(&version);

    let bytes = reqwest::get(&url)
        .await
        .map_err(|e| e.to_string())?
        .bytes()
        .await
        .map_err(|e| e.to_string())?;

    std::fs::write(&dest, &bytes).map_err(|e| e.to_string())?;
    Ok(dest)
}

pub async fn install_update(path: PathBuf) -> Result<(), String> {
    tokio::task::spawn_blocking(move || install_blocking(&path))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(target_os = "linux")]
fn install_blocking(path: &std::path::Path) -> Result<(), String> {
    let status = std::process::Command::new("pkexec")
        .args(["dpkg", "-i", path.to_str().unwrap_or_default()])
        .status()
        .map_err(|e| e.to_string())?;

    if status.success() {
        Ok(())
    } else {
        Err(t!("update.err_install", tool = "pkexec dpkg").to_string())
    }
}

#[cfg(target_os = "macos")]
fn install_blocking(path: &std::path::Path) -> Result<(), String> {
    let tmp_dir = std::env::temp_dir().join("simpleedit_update_extract");
    let _ = std::fs::create_dir_all(&tmp_dir);

    let ok = std::process::Command::new("tar")
        .args([
            "-xzf",
            path.to_str().unwrap_or_default(),
            "-C",
            tmp_dir.to_str().unwrap_or_default(),
        ])
        .status()
        .map_err(|e| e.to_string())?
        .success();

    if !ok {
        return Err(t!("update.err_extract").to_string());
    }

    let binary = tmp_dir.join("simpleedit");
    let script = format!(
        "cp '{}' /usr/local/bin/simpleedit && chmod +x /usr/local/bin/simpleedit",
        binary.display()
    );

    let ok = std::process::Command::new("osascript")
        .args([
            "-e",
            &format!(
                "do shell script \"{}\" with administrator privileges",
                script
            ),
        ])
        .status()
        .map_err(|e| e.to_string())?
        .success();

    if ok {
        Ok(())
    } else {
        Err(t!("update.err_install", tool = "osascript").to_string())
    }
}

/// Re-executes the just-installed binary and exits this process.
///
/// The `Ok` case never actually returns — a successful respawn calls
/// `process::exit` immediately. The `Err` case lets the caller show the
/// user something instead of the app just vanishing, which is what used to
/// happen: the spawn failure was silently swallowed.
pub fn restart() -> Result<(), String> {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("simpleedit"));

    // A package manager (dpkg, etc.) replaces the binary file in place while
    // this process is still running it. On Linux that makes current_exe()
    // report something like "/usr/bin/simpleedit (deleted)": the kernel's
    // way of saying the path this process was loaded from no longer has a
    // directory entry, even though a freshly installed file now sits at
    // that same path. Command::new() on that literal string can't find
    // anything there, so fall back to the bare name and let PATH resolve
    // the new file instead.
    let exe = if exe.exists() {
        exe
    } else {
        PathBuf::from("simpleedit")
    };

    std::process::Command::new(&exe)
        .spawn()
        .map_err(|e| t!("update.err_restart", error = e.to_string()).to_string())?;
    std::process::exit(0);
}
