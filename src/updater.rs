use std::path::{Path, PathBuf};
use std::time::Duration;

use sha2::{Digest, Sha256};

const REPO: &str = "simple-edit-app/simpleedit";
const RELEASES_API: &str =
    "https://api.github.com/repos/simple-edit-app/simpleedit/releases/latest";
/// Public half of the release signing key (Ed25519). `SHA256SUMS` must carry
/// a valid signature from it, so a tampered release can't vouch for itself.
const SIGNING_PUBLIC_KEY: [u8; 32] = [
    0x99, 0x9a, 0xef, 0x9a, 0x8e, 0x99, 0xcd, 0x86, 0xd9, 0x0f, 0x6e, 0x6e, 0xed, 0xd3, 0x2c, 0xc7,
    0xa7, 0x66, 0xf4, 0x77, 0x32, 0xad, 0x6f, 0xe2, 0x8e, 0x0c, 0x15, 0x6f, 0x8b, 0xe3, 0x7a, 0x4b,
];
/// Refuse absurdly large downloads.
const MAX_DOWNLOAD: usize = 300 * 1024 * 1024;

/// Strict `MAJOR.MINOR.PATCH`. Anything else — pre-releases, path
/// separators, garbage from a tampered API response — is rejected, so a
/// version string can't be used to build a hostile URL or file name.
fn parse_version(s: &str) -> Option<(u32, u32, u32)> {
    let mut parts = s.split('.');
    let mut next = || {
        let p = parts.next()?;
        (!p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
            .then(|| p.parse().ok())
            .flatten()
    };
    let v = (next()?, next()?, next()?);
    parts.next().is_none().then_some(v)
}

fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

fn client(timeout: Duration) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent("simpleedit-updater")
        .timeout(timeout)
        .build()
        .map_err(|e| e.to_string())
}

pub async fn check_for_update() -> Result<Option<String>, String> {
    let json: serde_json::Value = client(Duration::from_secs(15))?
        .get(RELEASES_API)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    let tag = json["tag_name"]
        .as_str()
        .ok_or_else(|| t!("update.err_api").to_string())?;

    let latest = tag.trim_start_matches('v');
    if parse_version(latest).is_none() {
        return Err(t!("update.err_api").to_string());
    }

    if is_newer(latest, env!("CARGO_PKG_VERSION")) {
        Ok(Some(latest.to_string()))
    } else {
        Ok(None)
    }
}

#[cfg(target_os = "linux")]
fn asset_name(version: &str) -> String {
    format!("simpleedit_{}-1_amd64.deb", version)
}

#[cfg(target_os = "macos")]
fn asset_name(version: &str) -> String {
    format!("simpleedit-v{}-aarch64-apple-darwin.tar.gz", version)
}

/// In-app install only works for the package this binary came from: a .deb
/// in /usr/bin on Linux, the copy in /usr/local/bin on macOS. Anything else
/// (snap, AppImage, Homebrew cask, another architecture) would have the
/// update installed somewhere the running app isn't.
#[cfg(target_os = "linux")]
fn install_supported() -> Result<(), String> {
    let ok = cfg!(target_arch = "x86_64")
        && Path::new("/usr/bin/dpkg").exists()
        && std::env::current_exe()
            .map(|p| p.starts_with("/usr/bin"))
            .unwrap_or(false);
    if ok {
        Ok(())
    } else {
        Err(t!("update.err_unsupported").to_string())
    }
}

#[cfg(target_os = "macos")]
fn install_supported() -> Result<(), String> {
    let ok = cfg!(target_arch = "aarch64")
        && std::env::current_exe()
            .map(|p| p == Path::new("/usr/local/bin/simpleedit"))
            .unwrap_or(false);
    if ok {
        Ok(())
    } else {
        Err(t!("update.err_unsupported").to_string())
    }
}

/// The hash `SHA256SUMS` lists for `asset` (`<hash>  <name>` or
/// `<hash> *<name>` lines, as `sha256sum` writes them).
fn expected_hash(sums: &str, asset: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (hash, name) = line.split_once(char::is_whitespace)?;
        (name.trim().trim_start_matches('*') == asset).then(|| hash.to_ascii_lowercase())
    })
}

fn verify_signature(message: &[u8], signature: &[u8]) -> bool {
    use ed25519_dalek::{Signature, VerifyingKey};
    let (Ok(key), Ok(sig)) = (
        VerifyingKey::from_bytes(&SIGNING_PUBLIC_KEY),
        <[u8; 64]>::try_from(signature).map(|b| Signature::from_bytes(&b)),
    ) else {
        return false;
    };
    key.verify_strict(message, &sig).is_ok()
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Downloads the release asset into a fresh private directory and checks it
/// against the release's `SHA256SUMS`. Returns the path of the verified file.
pub async fn download_update(version: String) -> Result<PathBuf, String> {
    if parse_version(&version).is_none() {
        return Err(t!("update.err_api").to_string());
    }
    install_supported()?;

    let asset = asset_name(&version);
    let base = format!("https://github.com/{REPO}/releases/download/v{version}");
    let client = client(Duration::from_secs(300))?;

    let sums = client
        .get(format!("{base}/SHA256SUMS"))
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| e.to_string())?
        .text()
        .await
        .map_err(|e| e.to_string())?;
    let sig = client
        .get(format!("{base}/SHA256SUMS.sig"))
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| e.to_string())?
        .bytes()
        .await
        .map_err(|e| e.to_string())?;
    if !verify_signature(sums.as_bytes(), &sig) {
        return Err(t!("update.err_signature").to_string());
    }
    let expected =
        expected_hash(&sums, &asset).ok_or_else(|| t!("update.err_checksum").to_string())?;

    let bytes = client
        .get(format!("{base}/{asset}"))
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| e.to_string())?
        .bytes()
        .await
        .map_err(|e| e.to_string())?;
    if bytes.len() > MAX_DOWNLOAD || sha256_hex(&bytes) != expected {
        return Err(t!("update.err_checksum").to_string());
    }

    // A fresh 0700 directory: nobody else can pre-create or swap the file
    // between this write and the privileged install.
    let dir = tempfile::Builder::new()
        .prefix("simpleedit-update-")
        .tempdir()
        .map_err(|e| e.to_string())?
        .keep();
    let dest = dir.join(&asset);
    std::fs::write(&dest, &bytes).map_err(|e| e.to_string())?;
    Ok(dest)
}

pub async fn install_update(path: PathBuf) -> Result<(), String> {
    let result = tokio::task::spawn_blocking({
        let path = path.clone();
        move || install_blocking(&path)
    })
    .await
    .map_err(|e| e.to_string())?;
    if let Some(dir) = path.parent() {
        let _ = std::fs::remove_dir_all(dir);
    }
    result
}

#[cfg(target_os = "linux")]
fn install_blocking(path: &Path) -> Result<(), String> {
    let status = std::process::Command::new("pkexec")
        .arg("dpkg")
        .arg("-i")
        .arg(path)
        .status()
        .map_err(|e| e.to_string())?;

    if status.success() {
        Ok(())
    } else {
        Err(t!("update.err_install", tool = "pkexec dpkg").to_string())
    }
}

#[cfg(target_os = "macos")]
fn install_blocking(path: &Path) -> Result<(), String> {
    let tmp_dir = path
        .parent()
        .map(|d| d.join("extract"))
        .ok_or_else(|| t!("update.err_extract").to_string())?;
    std::fs::create_dir_all(&tmp_dir).map_err(|e| e.to_string())?;

    let ok = std::process::Command::new("tar")
        .arg("-xzf")
        .arg(path)
        .arg("-C")
        .arg(&tmp_dir)
        .status()
        .map_err(|e| e.to_string())?
        .success();

    if !ok {
        return Err(t!("update.err_extract").to_string());
    }

    // The binary's path goes in as an AppleScript argument and is quoted by
    // `quoted form of`, never spliced into the script text.
    let ok = std::process::Command::new("osascript")
        .args([
            "-e",
            "on run argv",
            "-e",
            "do shell script \"cp \" & quoted form of (item 1 of argv) & \" /usr/local/bin/simpleedit && chmod +x /usr/local/bin/simpleedit\" with administrator privileges",
            "-e",
            "end run",
        ])
        .arg(tmp_dir.join("simpleedit"))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_parsed_strictly() {
        assert_eq!(parse_version("1.5.0"), Some((1, 5, 0)));
        for bad in [
            "1.5",
            "1.5.0.1",
            "1.5.0-rc1",
            "../1.0.0",
            "1..0",
            "v1.0.0",
            "",
        ] {
            assert_eq!(parse_version(bad), None, "{bad}");
        }
    }

    #[test]
    fn newer_means_strictly_greater_and_valid() {
        assert!(is_newer("1.10.0", "1.9.9"));
        assert!(!is_newer("1.5.0", "1.5.0"));
        assert!(!is_newer("1.6.0-beta", "1.5.0"));
        assert!(!is_newer("garbage", "1.5.0"));
    }

    #[test]
    fn expected_hash_finds_the_asset_line() {
        let sums = "ABC123  other.deb\nDEF456 *simpleedit.deb\n";
        assert_eq!(
            expected_hash(sums, "simpleedit.deb").as_deref(),
            Some("def456")
        );
        assert_eq!(expected_hash(sums, "missing.deb"), None);
    }

    #[test]
    fn signature_verifies_only_for_the_signed_message() {
        let sig: Vec<u8> = (0..128)
            .step_by(2)
            .map(|i| u8::from_str_radix(&"4263c802c9311a3da734c16666f6f474d81360c9bbe75cdcee330b4bfd58c694931d9933178d6e899f3e7c27d90e7e2a5ddf85dc2cdebcc1fea68c7426af9405"[i..i + 2], 16).unwrap())
            .collect();
        assert!(verify_signature(b"simpleedit-test-message", &sig));
        assert!(!verify_signature(b"simpleedit-test-messagE", &sig));
        assert!(!verify_signature(b"simpleedit-test-message", &sig[..63]));
    }

    #[test]
    fn sha256_matches_a_known_digest() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
