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

pub use crate::encoding::Encoding;

/// How a file stores its text; the editor itself only ever holds `\n`-joined
/// lines, so this is applied again when saving.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextFormat {
    pub ending: LineEnding,
    /// Whether the file ended with a line break.
    pub final_newline: bool,
    pub encoding: Encoding,
    /// The file mixes line endings (or has a lone `\r`): a save keeps the
    /// ending of the lines that were not touched, until one is chosen.
    pub mixed: bool,
}

impl Default for TextFormat {
    fn default() -> Self {
        Self {
            ending: LineEnding::Lf,
            final_newline: true,
            encoding: Encoding::Utf8,
            mixed: false,
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
        let lone_cr = raw.replace("\r\n", "").contains('\r');
        let mixed = lone_cr || (crlf > 0 && lf > 0);
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
                encoding: Encoding::Utf8,
                mixed,
            },
        )
    }

    /// Like `encode_bytes`, for a file that mixes line endings: the lines
    /// that are as the file on disk has them keep their own ending, the
    /// others (typed, changed) take the dominant one.
    pub fn encode_bytes_for(self, text: &str, path: &Path) -> Option<Vec<u8>> {
        if self.mixed {
            if let Some(original) = std::fs::read(path)
                .ok()
                .and_then(|bytes| self.encoding.decode(&bytes))
            {
                return self.encoding.encode(&self.encode_keeping(text, &original));
            }
        }
        self.encode_bytes(text)
    }

    /// The text laid out as `encode` does, but every line the same in
    /// `original` (as read from disk) keeps the ending it had there.
    pub fn encode_keeping(self, text: &str, original: &str) -> String {
        let dominant = match self.ending {
            LineEnding::Lf => "\n",
            LineEnding::Crlf => "\r\n",
        };
        // The lines of the original, each with its ending.
        let mut old: Vec<(&str, &str)> = Vec::new();
        let mut rest = original;
        while !rest.is_empty() {
            let end = rest.find(['\r', '\n']).unwrap_or(rest.len());
            let line = &rest[..end];
            let tail = &rest[end..];
            let term = if tail.starts_with("\r\n") {
                "\r\n"
            } else if tail.starts_with('\r') {
                "\r"
            } else if tail.starts_with('\n') {
                "\n"
            } else {
                ""
            };
            old.push((line, term));
            rest = &tail[term.len()..];
        }
        let body = text.strip_suffix('\n').unwrap_or(text);
        if text.is_empty() {
            return String::new();
        }
        let new: Vec<&str> = body.split('\n').collect();
        let old_lines: Vec<&str> = old.iter().map(|(l, _)| *l).collect();
        let mut terms: Vec<&str> = vec![dominant; new.len()];
        for op in similar::capture_diff_slices(similar::Algorithm::Myers, &old_lines, &new) {
            if let similar::DiffOp::Equal {
                old_index,
                new_index,
                len,
            } = op
            {
                for k in 0..len {
                    // The last line of the original may have had none.
                    if !old[old_index + k].1.is_empty() {
                        terms[new_index + k] = old[old_index + k].1;
                    }
                }
            }
        }
        let mut out = String::with_capacity(body.len() + new.len());
        for (i, line) in new.iter().enumerate() {
            out.push_str(line);
            // The last line has an ending only if the file ends with one.
            if i + 1 < new.len() || self.final_newline {
                out.push_str(terms[i]);
            }
        }
        out
    }

    /// The bytes to write: the text laid out as the file stores it, in its
    /// encoding. `None` when a character has no place in that encoding.
    pub fn encode_bytes(self, text: &str) -> Option<Vec<u8>> {
        self.encoding.encode(&self.encode(text))
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

#[cfg(test)]
mod encoding_tests {
    use super::*;

    #[test]
    fn a_byte_order_mark_or_the_bytes_tell_the_encoding() {
        let (t, e) = Encoding::detect(b"\xef\xbb\xbfhi");
        assert_eq!((t.as_str(), e), ("hi", Encoding::Utf8Bom));
        let (t, e) = Encoding::detect(b"\xff\xfeh\0i\0");
        assert_eq!((t.as_str(), e), ("hi", Encoding::Utf16Le));
        let (t, e) = Encoding::detect(b"\xfe\xff\0h\0i");
        assert_eq!((t.as_str(), e), ("hi", Encoding::Utf16Be));
        assert_eq!(Encoding::detect("é".as_bytes()).1, Encoding::Utf8);
        let (t, e) = Encoding::detect(b"caf\xe9 \x80");
        assert_eq!((t.as_str(), e), ("café €", Encoding::Windows1252));
    }

    #[test]
    fn every_encoding_writes_back_what_it_read() {
        for e in Encoding::ALL {
            let text = "abc é\n";
            let bytes = e
                .encode(text)
                .unwrap_or_else(|| panic!("{e:?} cannot write it"));
            assert_eq!(e.decode(&bytes).as_deref(), Some(text), "{e:?}");
        }
    }

    #[test]
    fn a_character_without_a_place_is_not_written() {
        assert!(Encoding::Latin1.encode("€").is_none());
        assert_eq!(Encoding::Latin9.encode("€"), Some(vec![0xa4]));
        assert_eq!(Encoding::Windows1252.encode("€"), Some(vec![0x80]));
        assert!(Encoding::Windows1252.encode("日").is_none());
        assert!(Encoding::Utf16Le.encode("日").is_some());
    }

    #[test]
    fn utf8_refuses_bytes_that_are_not_text_and_utf16_an_odd_count() {
        assert!(Encoding::Utf8.decode(&[0xe9]).is_none());
        assert!(Encoding::Utf16Le.decode(&[0x61]).is_none());
    }

    #[test]
    fn a_mixed_file_keeps_the_ending_of_the_lines_that_were_not_touched() {
        let raw = "a\r\nb\nc\rd\r\n";
        let (text, format) = TextFormat::decode(raw);
        assert!(format.mixed);
        assert_eq!(text, "a\nb\nc\nd\n");
        // Nothing changed: the same bytes.
        assert_eq!(format.encode_keeping(&text, raw), raw);
        // One line changed, one added: only those take the dominant ending.
        let edited = "a\nB\nc\nnew\nd\n";
        assert_eq!(
            format.encode_keeping(edited, raw),
            "a\r\nB\r\nc\rnew\r\nd\r\n"
        );
        // A file with one ending is not mixed.
        assert!(!TextFormat::decode("a\r\nb\r\n").1.mixed);
        assert!(!TextFormat::decode("a\nb\n").1.mixed);
        assert!(TextFormat::decode("a\rb").1.mixed);
    }

    #[test]
    fn a_format_writes_the_layout_then_the_encoding() {
        let format = TextFormat {
            ending: LineEnding::Crlf,
            final_newline: true,
            encoding: Encoding::Latin1,
            mixed: false,
        };
        assert_eq!(format.encode_bytes("é\n"), Some(vec![0xe9, b'\r', b'\n']));
    }
}
