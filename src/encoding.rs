//! The character encodings a file can be in. Standalone: the database
//! drivers (separate programs) include this file too.

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

    /// Like `detect`, for the start of a file that goes on (`cut`): the end
    /// may fall in the middle of a character, which is no sign of another
    /// encoding.
    pub fn detect_prefix(bytes: &[u8], cut: bool) -> (String, Encoding) {
        let mut end = bytes.len();
        if let Err(e) = std::str::from_utf8(bytes) {
            if cut && e.error_len().is_none() {
                end = e.valid_up_to();
            }
        }
        let (text, encoding) = Encoding::detect(&bytes[..end]);
        if end < bytes.len() && encoding != Encoding::Utf8 {
            // Not UTF-8 after all: all the bytes are read in the other one.
            return Encoding::detect(bytes);
        }
        (text, encoding)
    }

    /// UTF-16 with no byte order mark: text of mostly Latin letters has a NUL
    /// every other byte (and nothing else a NUL), on the odd places in
    /// little-endian, on the even ones in big-endian.
    pub fn utf16_without_mark(bytes: &[u8]) -> Option<Encoding> {
        let sample = &bytes[..bytes.len().min(8192)];
        let sample = &sample[..sample.len() - sample.len() % 2];
        if sample.len() < 4 {
            return None;
        }
        let pairs = sample.len() / 2;
        let (mut even, mut odd) = (0usize, 0usize);
        for pair in sample.as_chunks::<2>().0 {
            even += usize::from(pair[0] == 0);
            odd += usize::from(pair[1] == 0);
        }
        let mostly = |n: usize| n * 10 >= pairs * 9;
        let hardly = |n: usize| n * 20 <= pairs;
        if mostly(odd) && hardly(even) {
            Some(Encoding::Utf16Le)
        } else if mostly(even) && hardly(odd) {
            Some(Encoding::Utf16Be)
        } else {
            None
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
        if let Some(e) = Encoding::utf16_without_mark(bytes) {
            if let Some(text) = e.decode(bytes) {
                return (text, e);
            }
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
