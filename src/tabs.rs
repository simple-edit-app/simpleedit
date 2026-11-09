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

/// The character encoding of a file; the editor holds Unicode, this is
/// applied again when saving.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Encoding {
    #[default]
    Utf8,
    Utf8Bom,
    Utf16Le,
    Utf16Be,
    Windows1252,
    Latin1,
    Latin9,
}

/// What Windows-1252 puts at 0x80..=0x9F (the controls where it has no
/// character).
const CP1252_HIGH: [char; 32] = [
    '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž', '\u{8f}',
    '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}', 'ž', 'Ÿ',
];

impl Encoding {
    pub const ALL: [Encoding; 7] = [
        Encoding::Utf8,
        Encoding::Utf8Bom,
        Encoding::Utf16Le,
        Encoding::Utf16Be,
        Encoding::Windows1252,
        Encoding::Latin1,
        Encoding::Latin9,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Encoding::Utf8 => "UTF-8",
            Encoding::Utf8Bom => "UTF-8 BOM",
            Encoding::Utf16Le => "UTF-16 LE",
            Encoding::Utf16Be => "UTF-16 BE",
            Encoding::Windows1252 => "Windows-1252",
            Encoding::Latin1 => "ISO-8859-1",
            Encoding::Latin9 => "ISO-8859-15",
        }
    }

    /// The character a byte stands for in a one-byte encoding.
    fn single(self, byte: u8) -> char {
        match (self, byte) {
            (Encoding::Windows1252, 0x80..=0x9f) => CP1252_HIGH[(byte - 0x80) as usize],
            (Encoding::Latin9, 0xa4) => '€',
            (Encoding::Latin9, 0xa6) => 'Š',
            (Encoding::Latin9, 0xa8) => 'š',
            (Encoding::Latin9, 0xb4) => 'Ž',
            (Encoding::Latin9, 0xb8) => 'ž',
            (Encoding::Latin9, 0xbc) => 'Œ',
            (Encoding::Latin9, 0xbd) => 'œ',
            (Encoding::Latin9, 0xbe) => 'Ÿ',
            _ => byte as char,
        }
    }

    /// The text of `bytes` read as this encoding; `None` if they are not
    /// valid in it (only UTF-8 and UTF-16 can be wrong).
    pub fn decode(self, bytes: &[u8]) -> Option<String> {
        match self {
            Encoding::Utf8 => String::from_utf8(bytes.to_vec()).ok(),
            Encoding::Utf8Bom => {
                let body = bytes.strip_prefix(&[0xef, 0xbb, 0xbf][..]).unwrap_or(bytes);
                String::from_utf8(body.to_vec()).ok()
            }
            Encoding::Utf16Le | Encoding::Utf16Be => {
                let big = self == Encoding::Utf16Be;
                let body = bytes
                    .strip_prefix(if big {
                        &[0xfe, 0xff][..]
                    } else {
                        &[0xff, 0xfe][..]
                    })
                    .unwrap_or(bytes);
                if body.len() % 2 != 0 {
                    return None;
                }
                let units: Vec<u16> = body
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|c| {
                        if big {
                            u16::from_be_bytes([c[0], c[1]])
                        } else {
                            u16::from_le_bytes([c[0], c[1]])
                        }
                    })
                    .collect();
                String::from_utf16(&units).ok()
            }
            _ => Some(bytes.iter().map(|b| self.single(*b)).collect()),
        }
    }

    /// The encoding a file seems to be in, and its text: a byte order mark
    /// says, else UTF-8 if it is valid, else Windows-1252.
    pub fn detect(bytes: &[u8]) -> (String, Encoding) {
        let by_mark = if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
            Some(Encoding::Utf8Bom)
        } else if bytes.starts_with(&[0xff, 0xfe]) {
            Some(Encoding::Utf16Le)
        } else if bytes.starts_with(&[0xfe, 0xff]) {
            Some(Encoding::Utf16Be)
        } else {
            None
        };
        if let Some(text) = by_mark.and_then(|e| e.decode(bytes).map(|t| (t, e))) {
            return text;
        }
        match String::from_utf8(bytes.to_vec()) {
            Ok(text) => (text, Encoding::Utf8),
            Err(_) => (
                Encoding::Windows1252.decode(bytes).unwrap_or_default(),
                Encoding::Windows1252,
            ),
        }
    }

    /// The bytes of `text` in this encoding; `None` if a character has none
    /// in it.
    pub fn encode(self, text: &str) -> Option<Vec<u8>> {
        match self {
            Encoding::Utf8 => Some(text.as_bytes().to_vec()),
            Encoding::Utf8Bom => {
                let mut out = vec![0xef, 0xbb, 0xbf];
                out.extend_from_slice(text.as_bytes());
                Some(out)
            }
            Encoding::Utf16Le | Encoding::Utf16Be => {
                let big = self == Encoding::Utf16Be;
                let mut out = if big {
                    vec![0xfe, 0xff]
                } else {
                    vec![0xff, 0xfe]
                };
                for unit in text.encode_utf16() {
                    out.extend_from_slice(&if big {
                        unit.to_be_bytes()
                    } else {
                        unit.to_le_bytes()
                    });
                }
                Some(out)
            }
            _ => {
                let table: std::collections::HashMap<char, u8> =
                    (0..=255u8).map(|b| (self.single(b), b)).collect();
                text.chars().map(|c| table.get(&c).copied()).collect()
            }
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
    pub encoding: Encoding,
}

impl Default for TextFormat {
    fn default() -> Self {
        Self {
            ending: LineEnding::Lf,
            final_newline: true,
            encoding: Encoding::Utf8,
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
                encoding: Encoding::Utf8,
            },
        )
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
    fn a_format_writes_the_layout_then_the_encoding() {
        let format = TextFormat {
            ending: LineEnding::Crlf,
            final_newline: true,
            encoding: Encoding::Latin1,
        };
        assert_eq!(format.encode_bytes("é\n"), Some(vec![0xe9, b'\r', b'\n']));
    }
}
