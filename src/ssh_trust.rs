//! Trusting the host key of an SSH server: the tunnel of a data source does
//! not accept a host it does not know. The key is shown by its fingerprint,
//! and `known_hosts` takes it once the user says so.

use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

/// A key the server offered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostKey {
    /// The line of `known_hosts` it would be.
    pub line: String,
    /// `SHA256:…` and the kind of key, as `ssh-keygen -l` says them.
    pub fingerprint: String,
}

/// Whether an error of the tunnel is a host key that is not known (or that
/// changed, which this is not: that one stays an error).
pub fn is_unknown_host(error: &str) -> bool {
    let e = error.to_lowercase();
    (e.contains("host key verification failed") || e.contains("no ed25519 host key is known"))
        && !e.contains("remote host identification has changed")
        && !e.contains("man-in-the-middle")
}

/// The keys `host:port` offers, with their fingerprints.
pub async fn scan(host: &str, port: u16) -> Result<Vec<HostKey>, String> {
    if host.is_empty() || host.starts_with('-') || host.chars().any(char::is_whitespace) {
        return Err("bad host".to_string());
    }
    let out = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        Command::new("ssh-keyscan")
            .args(["-T", "5", "-p", &port.to_string(), "--", host])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output(),
    )
    .await
    .map_err(|_| "timeout".to_string())?
    .map_err(|e| format!("ssh-keyscan: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let mut keys = Vec::new();
    for line in text
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
    {
        let Some(fingerprint) = fingerprint_of(line).await else {
            continue;
        };
        keys.push(HostKey {
            line: line.to_string(),
            fingerprint,
        });
    }
    if keys.is_empty() {
        return Err(format!("{host}:{port}: no host key"));
    }
    Ok(keys)
}

/// `256 SHA256:… host (ED25519)` as `ssh-keygen` says it for a key line.
async fn fingerprint_of(line: &str) -> Option<String> {
    let mut child = Command::new("ssh-keygen")
        .args(["-l", "-f", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdin = child.stdin.take()?;
    stdin.write_all(line.as_bytes()).await.ok()?;
    stdin.write_all(b"\n").await.ok()?;
    drop(stdin);
    let out = child.wait_with_output().await.ok()?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    // The size, the fingerprint, the host and the kind: the host is left out.
    let mut words = text.split_whitespace();
    let (_, print) = (words.next()?, words.next()?);
    let kind = text.rsplit(' ').next().unwrap_or("");
    Some(format!("{print} {kind}"))
}

/// Where the keys the user trusts are kept.
fn known_hosts() -> Option<PathBuf> {
    Some(dirs::home_dir()?.join(".ssh").join("known_hosts"))
}

/// Adds the keys to `~/.ssh/known_hosts` (made owner-only if it is new).
pub fn trust(keys: &[HostKey]) -> Result<(), String> {
    let path = known_hosts().ok_or("no home folder")?;
    trust_in(&path, keys)
}

fn trust_in(path: &std::path::Path, keys: &[HostKey]) -> Result<(), String> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
        }
    }
    let mut options = std::fs::OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    // A file that does not end with a line break would glue to the next key.
    if std::fs::read(path).is_ok_and(|b| !b.is_empty() && !b.ends_with(b"\n")) {
        writeln!(file).map_err(|e| e.to_string())?;
    }
    for key in keys {
        writeln!(file, "{}", key.line).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_host_that_is_not_known_asks_for_trust() {
        assert!(is_unknown_host("Host key verification failed."));
        assert!(!is_unknown_host(
            "@@@ WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED! @@@\nHost key verification failed."
        ));
        assert!(!is_unknown_host("Permission denied (publickey)."));
    }

    #[test]
    fn keys_are_appended_on_their_own_lines_to_a_private_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(".ssh/known_hosts");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "old ssh-ed25519 AAAA").unwrap();
        let key = HostKey {
            line: "example.org ssh-ed25519 BBBB".into(),
            fingerprint: String::new(),
        };
        trust_in(&file, &[key]).unwrap();
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "old ssh-ed25519 AAAA\nexample.org ssh-ed25519 BBBB\n"
        );
    }

    #[test]
    fn a_bad_host_is_not_scanned() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        assert!(runtime.block_on(scan("-oProxyCommand=x", 22)).is_err());
        assert!(runtime.block_on(scan("a b", 22)).is_err());
    }

    /// Against a real server: `SE_SSH_TEST=127.0.0.1:2222 cargo test live_ssh -- --ignored`.
    #[test]
    #[ignore]
    fn live_ssh_server_keys_have_fingerprints() {
        let Ok(spec) = std::env::var("SE_SSH_TEST") else {
            return;
        };
        let (host, port) = spec.rsplit_once(':').unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let keys = runtime.block_on(scan(host, port.parse().unwrap())).unwrap();
        assert!(
            keys.iter().all(|k| k.fingerprint.starts_with("SHA256:")),
            "{keys:?}"
        );
    }
}
