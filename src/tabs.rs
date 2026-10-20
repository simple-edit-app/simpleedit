//! What a tab remembers about its file: how the text is stored on disk (line
//! endings, final newline), the disk state it was read from, and the parked
//! buffer of a tab that isn't the active one.

use std::path::Path;
use std::time::SystemTime;

use crate::code_editor::Content;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LineEnding {
    #[default]
    Lf,
    Crlf,
}

impl LineEnding {
    pub fn label(self) -> &'static str {
        match self {
            LineEnding::Lf => "LF",
            LineEnding::Crlf => "CRLF",
        }
    }
}

/// How a file stores its text; the editor itself only ever holds `\n`-joined
/// lines, so this is applied again when saving.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextFormat {
    pub ending: LineEnding,
    /// Whether the file ended with a line break.
    pub final_newline: bool,
}

impl Default for TextFormat {
    fn default() -> Self {
        Self {
            ending: LineEnding::Lf,
            final_newline: true,
        }
    }
}

impl TextFormat {
    /// Reads the line-ending style of `raw` and returns the text with `\n`
    /// only. The dominant style wins in a mixed file; a lone `\r` counts as a
    /// line break (the editor's text engine would split on it anyway).
    pub fn decode(raw: &str) -> (String, TextFormat) {
        let crlf = raw.matches("\r\n").count();
        let lf = raw.matches('\n').count() - crlf;
        let ending = if crlf > 0 && crlf >= lf {
            LineEnding::Crlf
        } else {
            LineEnding::Lf
        };
        let text = if raw.contains('\r') {
            raw.replace("\r\n", "\n").replace('\r', "\n")
        } else {
            raw.to_string()
        };
        let final_newline = text.is_empty() || text.ends_with('\n');
        (
            text,
            TextFormat {
                ending,
                final_newline,
            },
        )
    }

    /// The bytes to write for a buffer text (`buffer_text`, which always ends
    /// in a newline unless empty).
    pub fn encode(self, text: &str) -> String {
        let text = if self.final_newline {
            text
        } else {
            text.strip_suffix('\n').unwrap_or(text)
        };
        match self.ending {
            LineEnding::Lf => text.to_string(),
            LineEnding::Crlf => text.replace('\n', "\r\n"),
        }
    }
}

/// The state of a file on disk when the editor last read or wrote it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DiskStamp {
    modified: SystemTime,
    len: u64,
}

impl DiskStamp {
    /// `None` when the file can't be inspected (missing, untitled…).
    pub fn of(path: &Path) -> Option<DiskStamp> {
        let meta = std::fs::metadata(path).ok()?;
        Some(DiskStamp {
            modified: meta.modified().ok()?,
            len: meta.len(),
        })
    }
}

/// A file read from disk, ready for a tab.
#[derive(Debug, Clone)]
pub struct Loaded {
    pub path: std::path::PathBuf,
    /// `\n` line breaks only.
    pub text: String,
    pub format: TextFormat,
    /// Taken *before* the read: a change during it shows up as stale later.
    pub stamp: Option<DiskStamp>,
}

/// A tab that isn't the active one.
pub struct CachedBuffer {
    /// `None` for a saved file not read yet (restored from the last session).
    pub content: Option<Content>,
    pub language: Option<String>,
    pub dirty: bool,
    pub stamp: Option<DiskStamp>,
    pub format: TextFormat,
    /// Fingerprint of the text last read or saved, if known.
    pub saved_fp: Option<u64>,
    pub view_mode: crate::preview::ViewMode,
}

impl CachedBuffer {
    /// A buffer that reads as `loaded` says.
    pub fn from_loaded(loaded: Loaded, language: Option<String>) -> Self {
        Self {
            saved_fp: Some(fingerprint(&loaded.text)),
            content: Some(Content::with_text(&loaded.text)),
            language,
            dirty: false,
            stamp: loaded.stamp,
            format: loaded.format,
            view_mode: crate::preview::ViewMode::Raw,
        }
    }

    /// An empty, unsaved buffer.
    pub fn blank() -> Self {
        Self {
            content: Some(Content::new()),
            language: None,
            dirty: false,
            stamp: None,
            format: TextFormat::default(),
            saved_fp: Some(fingerprint("")),
            view_mode: crate::preview::ViewMode::Raw,
        }
    }

    /// The buffer text, when it is loaded.
    pub fn text(&self) -> Option<String> {
        self.content.as_ref().map(crate::editor::buffer_text)
    }
}

/// A fast fingerprint of a text, to tell whether a buffer is back to the
/// state last saved without keeping a second copy of it.
pub fn fingerprint(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crlf_files_round_trip_byte_for_byte() {
        let raw = "a\r\nb\r\n";
        let (text, format) = TextFormat::decode(raw);
        assert_eq!(text, "a\nb\n");
        assert_eq!(format.ending, LineEnding::Crlf);
        assert_eq!(format.encode(&text), raw);
    }

    #[test]
    fn a_missing_final_newline_stays_missing() {
        let (text, format) = TextFormat::decode("a\nb");
        assert!(!format.final_newline);
        // The buffer text always ends in a newline.
        assert_eq!(format.encode("a\nb\n"), "a\nb");
        assert_eq!(text, "a\nb");
    }

    #[test]
    fn the_dominant_style_wins_in_a_mixed_file() {
        assert_eq!(
            TextFormat::decode("a\r\nb\r\nc\n").1.ending,
            LineEnding::Crlf
        );
        assert_eq!(TextFormat::decode("a\nb\nc\r\n").1.ending, LineEnding::Lf);
    }

    #[test]
    fn lone_carriage_returns_become_line_breaks() {
        assert_eq!(TextFormat::decode("a\rb").0, "a\nb");
    }

    #[test]
    fn empty_and_default_formats() {
        assert!(TextFormat::decode("").1.final_newline);
        assert_eq!(TextFormat::default().encode("x\n"), "x\n");
    }

    #[test]
    fn crlf_file_opened_in_the_editor_has_no_blank_lines() {
        let (text, format) = TextFormat::decode("a\r\nb\r\n");
        let content = Content::with_text(&text);
        assert_eq!(content.line_count(), 2);
        assert_eq!(
            format.encode(&crate::editor::buffer_text(&content)),
            "a\r\nb\r\n"
        );
    }

    #[test]
    fn fingerprints_differ_for_different_texts() {
        assert_eq!(fingerprint("a"), fingerprint("a"));
        assert_ne!(fingerprint("a"), fingerprint("b"));
    }
}
