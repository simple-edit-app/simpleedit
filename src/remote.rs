//! Files on another machine, over SSH: the system's `ssh` does the work (so
//! keys, the agent and `~/.ssh/config` apply). A remote file is copied to a
//! local folder, edited there, and sent back each time it is saved.

use std::path::PathBuf;

/// A file on a machine: `host` is what `ssh` accepts (`user@host`, or a
/// name of `~/.ssh/config`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteFile {
    pub host: String,
    pub path: String,
}

/// An entry of a remote folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub is_dir: bool,
}

/// What `ssh` is run as (`SIMPLEEDIT_SSH` says another program, for tests).
fn program() -> String {
    std::env::var("SIMPLEEDIT_SSH").unwrap_or_else(|_| "ssh".to_string())
}

/// Whether `host` can be given to `ssh` without being taken for an option.
pub fn valid_host(host: &str) -> bool {
    !host.is_empty()
        && !host.starts_with('-')
        && !host.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// `host:/path`, `user@host:path` or `ssh://user@host/path`.
pub fn parse(spec: &str) -> Option<RemoteFile> {
    let spec = spec.trim();
    let (host, path) = if let Some(rest) = spec.strip_prefix("ssh://") {
        let (host, path) = rest.split_once('/')?;
        // ssh takes `ssh://user@host:port` as a destination itself.
        if host.contains(':') {
            let host = format!("ssh://{host}");
            return valid_host(&host).then(|| RemoteFile {
                host,
                path: format!("/{path}"),
            });
        }
        (host, format!("/{path}"))
    } else {
        let (host, path) = spec.split_once(':')?;
        (host, path.to_string())
    };
    (valid_host(host) && !path.is_empty()).then(|| RemoteFile {
        host: host.to_string(),
        path,
    })
}

/// A word for a remote shell: between quotes, the quotes inside escaped.
pub fn quote(word: &str) -> String {
    format!("'{}'", word.replace('\'', r"'\''"))
}

/// The folder above a path, `/` at the top.
pub fn parent(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    match trimmed.rsplit_once('/') {
        Some(("", _)) | None => "/".to_string(),
        Some((parent, _)) => parent.to_string(),
    }
}

/// `dir` and `name` joined with one slash.
pub fn join(dir: &str, name: &str) -> String {
    format!("{}/{name}", dir.trim_end_matches('/'))
}

/// Where the copy of a remote file is kept.
pub fn local_copy(file: &RemoteFile) -> Option<PathBuf> {
    let mut path = crate::fsutil::data_dir()?
        .join("simpleedit")
        .join("remote")
        .join(file.host.replace(['/', '\\', ':'], "_"));
    // A path from the home folder is not the one of the same name from `/`.
    if !file.path.starts_with('/') {
        path.push("~");
    }
    for part in file.path.split('/').filter(|p| !p.is_empty() && *p != "..") {
        path.push(part);
    }
    Some(path)
}

async fn run(
    host: &str,
    command: &str,
    input: Option<Vec<u8>>,
    limit: usize,
) -> Result<Vec<u8>, String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    if !valid_host(host) {
        return Err(format!("{host}: not a valid host"));
    }
    let mut child = tokio::process::Command::new(program())
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=10",
            "-o",
            "ServerAliveInterval=15",
            host,
            command,
        ])
        .stdin(if input.is_some() {
            std::process::Stdio::piped()
        } else {
            std::process::Stdio::null()
        })
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("ssh: {e}"))?;
    let mut stdout = child.stdout.take().ok_or("no output")?;
    let mut stderr = child.stderr.take().ok_or("no output")?;
    let mut stdin = child.stdin.take();
    let write_in = async {
        if let (Some(bytes), Some(stdin)) = (input, stdin.as_mut()) {
            stdin.write_all(&bytes).await?;
        }
        drop(stdin.take());
        Ok::<(), std::io::Error>(())
    };
    let read_out = async {
        let mut out = Vec::new();
        (&mut stdout)
            .take(limit as u64 + 1)
            .read_to_end(&mut out)
            .await
            .map(|_| out)
    };
    let read_err = async {
        let mut err = Vec::new();
        stderr.read_to_end(&mut err).await.map(|_| err)
    };
    let (sent, out, err) = tokio::time::timeout(std::time::Duration::from_secs(120), async {
        tokio::join!(write_in, read_out, read_err)
    })
    .await
    .map_err(|_| "the server did not answer in time".to_string())?;
    sent.map_err(|e| format!("ssh: {e}"))?;
    let (out, err) = (out.map_err(|e| e.to_string())?, err.unwrap_or_default());
    if out.len() > limit {
        return Err("the file is too large".to_string());
    }
    let status = child.wait().await.map_err(|e| e.to_string())?;
    if status.success() {
        Ok(out)
    } else {
        let said = String::from_utf8_lossy(&err).trim().to_string();
        Err(if said.is_empty() {
            format!("ssh exited with {status}")
        } else {
            said
        })
    }
}

/// The folder (as the machine spells it: `.` becomes the full path) and its
/// entries, folders first.
pub async fn list(host: &str, dir: &str) -> Result<(String, Vec<Entry>), String> {
    let out = run(
        host,
        &format!("cd -- {} && pwd && ls -1Ap", quote(dir)),
        None,
        8 << 20,
    )
    .await?;
    let text = String::from_utf8_lossy(&out).into_owned();
    let mut lines = text.lines();
    let resolved = lines.next().unwrap_or("/").to_string();
    let mut entries: Vec<Entry> = lines
        .filter(|l| !l.is_empty())
        .map(|l| match l.strip_suffix('/') {
            Some(name) => Entry {
                name: name.to_string(),
                is_dir: true,
            },
            None => Entry {
                name: l.to_string(),
                is_dir: false,
            },
        })
        .collect();
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok((resolved, entries))
}

/// The bytes of a file, and when it was last changed.
pub async fn download(file: &RemoteFile, limit: usize) -> Result<(Vec<u8>, Option<u64>), String> {
    let bytes = run(
        &file.host,
        &format!("cat -- {}", quote(&file.path)),
        None,
        limit,
    )
    .await?;
    Ok((bytes, mtime(file).await.ok()))
}

/// When the file was last changed (seconds), on a GNU or a BSD system.
pub async fn mtime(file: &RemoteFile) -> Result<u64, String> {
    let path = quote(&file.path);
    let out = run(
        &file.host,
        &format!("stat -c %Y -- {path} 2>/dev/null || stat -f %m -- {path}"),
        None,
        64,
    )
    .await?;
    String::from_utf8_lossy(&out)
        .trim()
        .parse()
        .map_err(|_| "no date for the file".to_string())
}

/// Sends the bytes to the file, unless it changed on the machine since
/// `known` (the date at the last copy). Returns the new date.
pub async fn upload(
    file: &RemoteFile,
    bytes: Vec<u8>,
    known: Option<u64>,
) -> Result<Option<u64>, String> {
    if let (Some(known), Ok(now)) = (known, mtime(file).await) {
        if now != known {
            return Err("the file changed on the server since it was opened".to_string());
        }
    }
    run(
        &file.host,
        &format!(
            "t={tmp}; p={path}; cat > \"$t\" && {{ chmod --reference=\"$p\" \"$t\" 2>/dev/null || chmod $(stat -f %Lp \"$p\") \"$t\" 2>/dev/null; true; }} && mv -f -- \"$t\" \"$p\" || {{ rm -f -- \"$t\"; exit 1; }}",
            tmp = quote(&format!("{}.simpleedit-tmp", file.path)),
            path = quote(&file.path)
        ),
        Some(bytes),
        64,
    )
    .await?;
    Ok(mtime(file).await.ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// The tests that stand in for `ssh` share the variable that says which.
    static SSH: Mutex<()> = Mutex::new(());

    /// A program that runs the command it is given on this machine, as an
    /// `ssh` would on the other.
    fn fake_ssh(dir: &std::path::Path) -> PathBuf {
        let script = dir.join("fake-ssh");
        std::fs::write(
            &script,
            "#!/bin/sh\n# ssh -o a -o b host command\nshift 6\nhost=\"$1\"\nshift\nif [ \"$host\" = down ]; then echo 'no route' >&2; exit 255; fi\nexec sh -c \"$1\"\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        script
    }

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    #[test]
    fn a_remote_file_is_named_in_the_ways_ssh_is() {
        let f = |s: &str| parse(s).map(|r| (r.host, r.path));
        assert_eq!(
            f("me@box:/srv/a.txt"),
            Some(("me@box".into(), "/srv/a.txt".into()))
        );
        assert_eq!(f("box:rel/a.txt"), Some(("box".into(), "rel/a.txt".into())));
        assert_eq!(
            f("ssh://me@box/srv/a.txt"),
            Some(("me@box".into(), "/srv/a.txt".into()))
        );
        assert_eq!(f("-oProxyCommand=x:/a"), None, "an option is not a host");
        assert_eq!(
            f("ssh://me@box:2222/srv/a.txt"),
            Some(("ssh://me@box:2222".into(), "/srv/a.txt".into()))
        );
        assert_eq!(f("no path"), None);
        assert_eq!(f("box:"), None);
    }

    #[test]
    fn paths_are_quoted_joined_and_climbed() {
        assert_eq!(quote("a b"), "'a b'");
        assert_eq!(quote("it's"), r"'it'\''s'");
        assert_eq!(parent("/srv/app/a.txt"), "/srv/app");
        assert_eq!(parent("/srv"), "/");
        assert_eq!(parent("/"), "/");
        assert_eq!(join("/srv/", "a"), "/srv/a");
        let file = RemoteFile {
            host: "me@box".into(),
            path: "/srv/../etc/a.txt".into(),
        };
        let copy = local_copy(&file).unwrap();
        assert!(copy.ends_with("me@box/srv/etc/a.txt"), "{copy:?}");
        let rel = |path: &str| RemoteFile {
            host: "me@box".into(),
            path: path.into(),
        };
        assert_ne!(local_copy(&rel("a.txt")), local_copy(&rel("/a.txt")));
    }

    #[cfg(unix)]
    #[test]
    fn folders_are_listed_files_read_and_written_and_a_change_is_noticed() {
        let _guard = SSH.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("SIMPLEEDIT_SSH", fake_ssh(dir.path()));
        let site = dir.path().join("site");
        std::fs::create_dir_all(site.join("sub dir")).unwrap();
        std::fs::write(site.join("b.txt"), "bee\n").unwrap();
        std::fs::write(site.join(".hidden"), "").unwrap();
        std::fs::write(site.join("it's.txt"), "quote\n").unwrap();
        let host = "me@box";
        let remote = |name: &str| RemoteFile {
            host: host.into(),
            path: site.join(name).to_string_lossy().into_owned(),
        };
        runtime().block_on(async {
            let (resolved, entries) = list(host, &site.to_string_lossy()).await.unwrap();
            assert_eq!(
                std::fs::canonicalize(&resolved).unwrap(),
                std::fs::canonicalize(&site).unwrap()
            );
            let names: Vec<(&str, bool)> = entries
                .iter()
                .map(|e| (e.name.as_str(), e.is_dir))
                .collect();
            assert_eq!(
                names,
                vec![
                    ("sub dir", true),
                    (".hidden", false),
                    ("b.txt", false),
                    ("it's.txt", false)
                ]
            );
            let (bytes, date) = download(&remote("b.txt"), 1 << 20).await.unwrap();
            assert_eq!(bytes, b"bee\n");
            assert!(date.is_some());
            assert_eq!(
                download(&remote("it's.txt"), 1 << 20).await.unwrap().0,
                b"quote\n"
            );
            // Writing goes through when nobody else changed the file.
            let newer = upload(&remote("b.txt"), b"bee!\n".to_vec(), date)
                .await
                .unwrap();
            assert_eq!(
                std::fs::read_to_string(site.join("b.txt")).unwrap(),
                "bee!\n"
            );
            assert!(newer.is_some());
            // Someone changed it: nothing is overwritten.
            let stale = Some(date.unwrap() - 100);
            let refused = upload(&remote("b.txt"), b"mine\n".to_vec(), stale).await;
            assert!(refused.unwrap_err().contains("changed on the server"));
            assert_eq!(
                std::fs::read_to_string(site.join("b.txt")).unwrap(),
                "bee!\n"
            );
            // A file that is too large is refused, a host that is down says why.
            assert!(download(&remote("b.txt"), 2)
                .await
                .unwrap_err()
                .contains("too large"));
            let down = list("down", "/").await.unwrap_err();
            assert!(down.contains("no route"), "{down}");
        });
        std::env::remove_var("SIMPLEEDIT_SSH");
    }
}
