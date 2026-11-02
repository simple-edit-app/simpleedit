//! Crash-safe file writes.

use std::io::Write;
use std::path::{Path, PathBuf};

/// Where the app keeps its data (session, bookmarks). Tests get a throwaway
/// folder so they never touch the user's files.
pub fn data_dir() -> Option<PathBuf> {
    #[cfg(test)]
    {
        Some(test_dir().join("data"))
    }
    #[cfg(not(test))]
    {
        dirs::data_dir()
    }
}

/// Where the app keeps its settings.
pub fn config_dir() -> Option<PathBuf> {
    #[cfg(test)]
    {
        Some(test_dir().join("config"))
    }
    #[cfg(not(test))]
    {
        dirs::config_dir()
    }
}

#[cfg(test)]
fn test_dir() -> PathBuf {
    std::env::temp_dir().join(format!("simpleedit-test-{}", std::process::id()))
}

/// Writes `data` to `path` through a temporary sibling file and a `rename`,
/// so a crash mid-write never leaves a truncated target. Symlinks are
/// followed (the link target is replaced, not the link) and an existing
/// file keeps its permissions. With `private`, the file is created `0600`.
///
/// The temporary file is created owner-only and takes the target's
/// permissions before any data is written. A target that the rename would
/// change for others (a hard link, a file of another owner) is rewritten in
/// place instead, and a read-only file is refused. If the directory isn't
/// writable, falls back to a direct write.
pub fn write_atomic(path: &Path, data: &[u8], private: bool) -> std::io::Result<()> {
    let target = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let existing = std::fs::metadata(&target).ok();
    if !private
        && existing
            .as_ref()
            .is_some_and(|m| m.permissions().readonly())
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "read-only file",
        ));
    }
    #[cfg(unix)]
    if !private && existing.as_ref().is_some_and(is_shared_inode) {
        return write_in_place(&target, data);
    }

    let dir = target.parent().unwrap_or_else(|| Path::new("."));
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = dir.join(format!(".{name}.{}.{nonce}.tmp", std::process::id()));

    let result = (|| {
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            // A brand-new file gets the usual permissions (minus umask); a
            // replaced one starts owner-only until it takes the old file's.
            opts.mode(if private || existing.is_some() {
                0o600
            } else {
                0o666
            });
        }
        let mut file = opts.open(&tmp)?;
        if !private {
            if let Some(meta) = &existing {
                #[cfg(unix)]
                if !same_owner(&file.metadata()?, meta) {
                    drop(file);
                    let _ = std::fs::remove_file(&tmp);
                    return write_in_place(&target, data);
                }
                file.set_permissions(meta.permissions())?;
            }
        }
        file.write_all(data)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&tmp, &target)?;
        sync_dir(dir);
        Ok(())
    })();

    match result {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            if !private && e.kind() == std::io::ErrorKind::PermissionDenied {
                std::fs::write(&target, data)
            } else {
                Err(e)
            }
        }
    }
}

/// Truncates and rewrites the file itself (keeps its inode, owner and links).
fn write_in_place(target: &Path, data: &[u8]) -> std::io::Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(target)?;
    file.write_all(data)?;
    file.sync_all()
}

#[cfg(unix)]
fn is_shared_inode(meta: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    meta.nlink() > 1
}

#[cfg(unix)]
fn same_owner(ours: &std::fs::Metadata, theirs: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    ours.uid() == theirs.uid() && ours.gid() == theirs.gid()
}

/// Makes the rename itself durable (best effort; not possible everywhere).
fn sync_dir(dir: &Path) {
    #[cfg(unix)]
    if let Ok(handle) = std::fs::File::open(dir) {
        let _ = handle.sync_all();
    }
    #[cfg(not(unix))]
    let _ = dir;
}

/// Moves an unreadable data file aside (`name.bak-<unix time>`) so the next
/// save doesn't overwrite what may still be recoverable. Returns the new path.
pub fn set_aside(path: &Path) -> Option<PathBuf> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let name = path.file_name()?.to_string_lossy().into_owned();
    let aside = path.with_file_name(format!("{name}.bak-{stamp}"));
    std::fs::rename(path, &aside).ok().map(|()| aside)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_content_and_leaves_no_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        std::fs::write(&path, "old").unwrap();
        write_atomic(&path, b"new", false).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn private_files_are_owner_only_and_symlinks_are_followed() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.json");
        write_atomic(&path, b"{}", true).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);

        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        write_atomic(&link, b"[]", true).unwrap();
        assert!(std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[]");
    }

    #[cfg(unix)]
    #[test]
    fn read_only_files_are_refused_and_left_alone() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ro.txt");
        std::fs::write(&path, "keep").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).unwrap();
        let err = write_atomic(&path, b"new", false).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "keep");
    }

    #[cfg(unix)]
    #[test]
    fn hard_links_see_the_new_content() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        std::fs::write(&a, "old").unwrap();
        std::fs::hard_link(&a, &b).unwrap();
        write_atomic(&a, b"new", false).unwrap();
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "new");
    }

    #[cfg(unix)]
    #[test]
    fn a_private_target_keeps_its_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".env");
        std::fs::write(&path, "SECRET=1").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        write_atomic(&path, b"SECRET=2", false).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "SECRET=2");
    }

    #[test]
    fn set_aside_keeps_the_unreadable_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.json");
        std::fs::write(&path, "{ broken").unwrap();
        let aside = set_aside(&path).unwrap();
        assert!(!path.exists());
        assert_eq!(std::fs::read_to_string(aside).unwrap(), "{ broken");
    }
}
