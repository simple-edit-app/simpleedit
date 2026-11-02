//! Reading and writing the files of the tabs.

use super::*;

/// Reads a text file, refusing ones too large to edit comfortably, devices
/// and pipes (which have no length and may never end), and anything that
/// isn't UTF-8.
pub(super) fn read_text(path: &Path) -> Result<String, String> {
    use std::io::Read;

    let meta = std::fs::metadata(path).map_err(read_error)?;
    if !meta.is_file() {
        return Err(t!("status.err_not_a_file").to_string());
    }
    if meta.len() > MAX_OPEN_BYTES {
        return Err(t!("status.err_too_large", mb = MAX_OPEN_BYTES / (1024 * 1024)).to_string());
    }
    let mut bytes = Vec::with_capacity(meta.len() as usize);
    std::fs::File::open(path)
        .and_then(|f| f.take(MAX_OPEN_BYTES + 1).read_to_end(&mut bytes))
        .map_err(read_error)?;
    if bytes.len() as u64 > MAX_OPEN_BYTES {
        return Err(t!("status.err_too_large", mb = MAX_OPEN_BYTES / (1024 * 1024)).to_string());
    }
    String::from_utf8(bytes).map_err(|_| t!("status.err_not_utf8").to_string())
}

/// A filesystem error in the interface language.
pub(super) fn read_error(e: std::io::Error) -> String {
    use std::io::ErrorKind::*;
    match e.kind() {
        NotFound => t!("status.err_not_found").to_string(),
        PermissionDenied => t!("status.err_permission").to_string(),
        _ => e.to_string(),
    }
}

/// A path as an absolute one, so a file opened from the command line keeps
/// meaning the same file after the app is started from another folder.
pub(super) fn absolute(path: PathBuf) -> PathBuf {
    std::fs::canonicalize(&path)
        .or_else(|_| std::env::current_dir().map(|dir| dir.join(&path)))
        .unwrap_or(path)
}

pub(super) async fn open_file() -> Result<Loaded, String> {
    // No "all files" filter: the portal turns `*` into `*.*`, which hides
    // files without an extension (Makefile, LICENSE…). Without a filter,
    // everything is listed.
    let handle = rfd::AsyncFileDialog::new()
        .set_title(t!("dialog.open_title").to_string())
        .pick_file()
        .await
        .ok_or_else(|| CANCELLED.to_string())?;

    read_file(handle.path().to_path_buf(), false).await
}

/// Reads a file for a tab. With `missing_ok`, a file that doesn't exist yet
/// opens as an empty one that the first save creates (`simpleedit new.txt`).
pub(super) async fn read_file(path: PathBuf, missing_ok: bool) -> Result<Loaded, String> {
    let path = absolute(path);
    tokio::task::spawn_blocking(move || {
        // Taken first: a change during the read then shows up as newer.
        let stamp = DiskStamp::of(&path);
        match read_text(&path) {
            Ok(raw) => {
                let (text, format) = TextFormat::decode(&raw);
                Ok(Loaded {
                    path,
                    text,
                    format,
                    stamp,
                })
            }
            Err(_) if missing_ok && !path.exists() => Ok(Loaded {
                path,
                text: String::new(),
                format: TextFormat::default(),
                stamp: None,
            }),
            Err(e) => Err(e),
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Writes the buffer text as the file stores it (line endings, final
/// newline); returns the path and the buffer text that was saved.
pub(super) async fn save_file(
    path: PathBuf,
    content: String,
    format: TextFormat,
) -> Result<(PathBuf, String), String> {
    tokio::task::spawn_blocking(move || {
        let bytes = format.encode(&content);
        crate::fsutil::write_atomic(&path, bytes.as_bytes(), false)
            .map(|()| (path, content))
            .map_err(|e| match e.kind() {
                std::io::ErrorKind::PermissionDenied => t!("status.err_permission").to_string(),
                _ => e.to_string(),
            })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The "Save as" dialog, starting from the file's folder and name.
pub(super) async fn pick_save_path(
    current: Option<PathBuf>,
    name: Option<String>,
) -> Result<PathBuf, String> {
    let mut dialog = rfd::AsyncFileDialog::new().set_title(t!("dialog.save_title").to_string());
    if let Some(dir) = current.as_deref().and_then(Path::parent) {
        dialog = dialog.set_directory(dir);
    }
    if let Some(name) = name {
        dialog = dialog.set_file_name(name);
    }
    let handle = dialog
        .save_file()
        .await
        .ok_or_else(|| CANCELLED.to_string())?;
    Ok(handle.path().to_path_buf())
}

/// Runs a save and reports it together with the tab it started from.
pub(super) fn save_command(
    origin: Option<PathBuf>,
    save: impl std::future::Future<Output = Result<(PathBuf, String), String>> + Send + 'static,
) -> Task<Message> {
    Task::perform(save, move |result| {
        Message::FileSaved(origin.clone(), result)
    })
}
