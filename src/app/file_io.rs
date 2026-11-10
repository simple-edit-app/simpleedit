//! Reading and writing the files of the tabs.

use super::*;
use crate::tabs::Encoding;

/// Reads a text file, refusing ones too large to edit comfortably, devices
/// and pipes (which have no length and may never end), and binary files.
pub(super) fn read_text(path: &Path) -> Result<String, String> {
    read_decoded(path).map(|(text, _)| text)
}

/// Like `read_text`, with the encoding the file is in (UTF-8, a byte order
/// mark, or Windows-1252 when it is not valid UTF-8).
pub(super) fn read_decoded(path: &Path) -> Result<(String, Encoding), String> {
    read_bytes(path).map(|bytes| Encoding::detect(&bytes))
}

/// The bytes of a text file, with the checks of `read_text`.
pub(super) fn read_bytes(path: &Path) -> Result<Vec<u8>, String> {
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
    if is_binary(&bytes) {
        return Err(t!("status.err_binary").to_string());
    }
    Ok(bytes)
}

/// Whether bytes are those of a binary file (an image, an archive…): a NUL in
/// the first 8 KiB, which text never holds, except UTF-16 with its byte
/// order mark. Opened as text, the first save would change its bytes.
pub(super) fn is_binary(bytes: &[u8]) -> bool {
    let utf16 = bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]);
    !utf16 && bytes[..bytes.len().min(8192)].contains(&0)
}

/// Part of a file too large to open whole: the lines from about `from` to
/// about `from + window` bytes (the first part starts at the beginning, the
/// others at the first line start after `from`), as `(text, start, end)`
/// with the byte range the text covers in the file.
pub(super) fn read_window(
    path: &Path,
    from: u64,
    window: u64,
) -> Result<(String, u64, u64), String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path).map_err(read_error)?;
    let size = file.metadata().map_err(read_error)?.len();
    let from = from.min(size);
    // The byte before the part tells whether it starts on a line.
    let lead = u64::from(from > 0);
    file.seek(SeekFrom::Start(from - lead))
        .map_err(read_error)?;
    let mut bytes = Vec::new();
    file.take(window + lead)
        .read_to_end(&mut bytes)
        .map_err(read_error)?;
    let mut start = from;
    let mut body = &bytes[lead as usize..];
    if lead == 1 && bytes[0] != b'\n' {
        // The line that was cut belongs to the part before.
        match body.iter().position(|b| *b == b'\n') {
            Some(i) => {
                body = &body[i + 1..];
                start += i as u64 + 1;
            }
            None => {
                start += body.len() as u64;
                body = &[];
            }
        }
    }
    if from + (body.len() as u64) + (start - from) < size {
        // And the last one to the part after.
        if let Some(i) = body.iter().rposition(|b| *b == b'\n') {
            body = &body[..=i];
        }
    }
    let end = start + body.len() as u64;
    let text = TextFormat::decode(&String::from_utf8_lossy(body)).0;
    Ok((text, start, end))
}

/// The part of a file that ends at `end` (a line start) and is about
/// `window` bytes long.
pub(super) fn read_window_before(
    path: &Path,
    end: u64,
    window: u64,
) -> Result<(String, u64, u64), String> {
    let from = end.saturating_sub(window);
    let (text, start, _) = read_window(path, from, end - from)?;
    // `read_window` cut at the last line break before `end`: that is `end`.
    let end = start + text.len() as u64;
    Ok((text, start, end))
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

/// Asks for a folder.
pub(super) async fn pick_folder() -> Option<PathBuf> {
    rfd::AsyncFileDialog::new()
        .set_title(t!("dialog.add_project_title").to_string())
        .pick_folder()
        .await
        .map(|h| h.path().to_path_buf())
}

/// Reads a file for a tab. With `missing_ok`, a file that doesn't exist yet
/// opens as an empty one that the first save creates (`simpleedit new.txt`).
pub(super) async fn read_file(path: PathBuf, missing_ok: bool) -> Result<Loaded, String> {
    let path = absolute(path);
    tokio::task::spawn_blocking(move || {
        // Taken first: a change during the read then shows up as newer.
        let stamp = DiskStamp::of(&path);
        // Too large to open whole: the first part, to read.
        if std::fs::metadata(&path).is_ok_and(|m| m.is_file() && m.len() > MAX_OPEN_BYTES) {
            let (text, _, _) = read_window(&path, 0, LARGE_WINDOW)?;
            return Ok(Loaded {
                path,
                text,
                format: TextFormat::default(),
                stamp,
            });
        }
        match read_decoded(&path) {
            Ok((raw, encoding)) => {
                let (text, mut format) = TextFormat::decode(&raw);
                format.encoding = encoding;
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
        let Some(bytes) = format.encode_bytes(&content) else {
            return Err(t!("status.err_encoding", name = format.encoding.label()).to_string());
        };
        crate::fsutil::write_atomic(&path, &bytes, false)
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
