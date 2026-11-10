//! Spelling: the words of a text (or of the comments of a program) that the
//! dictionary of the language does not know, with suggestions. The
//! dictionaries are the Hunspell ones of the system (`fr.dic` and `fr.aff`).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// A dictionary found on the machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Language {
    /// `fr`, `en_US`…
    pub code: String,
    aff: PathBuf,
    dic: PathBuf,
}

/// Where Hunspell dictionaries are kept.
fn search_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(data) = crate::fsutil::data_dir() {
        dirs.push(data.join("simpleedit").join("dictionaries"));
    }
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join(".local/share/hunspell"));
        dirs.push(home.join("Library/Spelling"));
    }
    for fixed in [
        "/usr/share/hunspell",
        "/usr/share/myspell",
        "/usr/share/myspell/dicts",
        "/usr/local/share/hunspell",
        "/opt/homebrew/share/hunspell",
        "/Library/Spelling",
        "/app/share/hunspell",
        "/snap/core22/current/usr/share/hunspell",
    ] {
        dirs.push(PathBuf::from(fixed));
    }
    dirs
}

/// The dictionaries of `dirs`: a `.dic` with its `.aff`. The first folder
/// that has a language gives it.
pub fn languages_in(dirs: &[PathBuf]) -> Vec<Language> {
    let mut found: Vec<Language> = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let dic = entry.path();
            if dic.extension().is_none_or(|e| e != "dic") {
                continue;
            }
            let aff = dic.with_extension("aff");
            let Some(code) = dic.file_stem().map(|s| s.to_string_lossy().into_owned()) else {
                continue;
            };
            if aff.is_file() && !found.iter().any(|l| l.code == code) {
                found.push(Language { code, aff, dic });
            }
        }
    }
    found.sort_by(|a, b| a.code.cmp(&b.code));
    found
}

/// The dictionaries of the machine.
pub fn languages() -> Vec<Language> {
    languages_in(&search_dirs())
}

/// The language that goes with `wanted` (`fr`, `fr_CA`, or empty for the
/// language of the interface): the same code, else one that starts the same.
pub fn pick_language<'a>(
    wanted: &str,
    interface: &str,
    available: &'a [Language],
) -> Option<&'a Language> {
    let wanted = if wanted.is_empty() { interface } else { wanted };
    let wanted = wanted.replace('-', "_");
    available.iter().find(|l| l.code == wanted).or_else(|| {
        let base = wanted.split('_').next().unwrap_or("");
        available.iter().find(|l| l.code == base).or_else(|| {
            available
                .iter()
                .find(|l| l.code.split('_').next() == Some(base))
        })
    })
}

/// Cheap to clone (the dictionary is shared): a copy checks a text in the
/// background.
#[derive(Clone)]
pub struct Speller {
    dictionary: std::sync::Arc<spellbook::Dictionary>,
    /// Words the user added, in lower case.
    personal: HashSet<String>,
    /// Words left alone until the app closes.
    ignored: HashSet<String>,
}

impl Speller {
    pub fn from_text(aff: &str, dic: &str) -> Result<Speller, String> {
        let dictionary = spellbook::Dictionary::new(aff, dic).map_err(|e| e.to_string())?;
        Ok(Speller {
            dictionary: std::sync::Arc::new(dictionary),
            personal: HashSet::new(),
            ignored: HashSet::new(),
        })
    }

    pub fn load(language: &Language) -> Result<Speller, String> {
        let read = |path: &Path| {
            std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
        };
        Speller::from_text(&read(&language.aff)?, &read(&language.dic)?)
    }

    pub fn add_personal(&mut self, word: &str) {
        self.personal.insert(word.to_lowercase());
    }

    pub fn ignore(&mut self, word: &str) {
        self.ignored.insert(word.to_lowercase());
    }

    pub fn knows(&self, word: &str) -> bool {
        let lower = word.to_lowercase();
        self.personal.contains(&lower)
            || self.ignored.contains(&lower)
            || self.dictionary.check(word)
            // A curly apostrophe is the straight one.
            || (word.contains('’') && self.dictionary.check(&word.replace('’', "'")))
    }

    pub fn suggest(&self, word: &str) -> Vec<String> {
        let mut out = Vec::new();
        self.dictionary.suggest(word, &mut out);
        out.truncate(8);
        out
    }
}

/// The words of `text` worth checking, with their byte offset: letters
/// (an apostrophe between two letters stays), not the names of code
/// (`camelCase`, `snake_case`, `v2`), the acronyms, the paths and addresses.
pub fn words(text: &str) -> Vec<(usize, &str)> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut found = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if !chars[i].1.is_alphabetic() {
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() {
            let c = chars[i].1;
            let inner_apostrophe = (c == '\'' || c == '’')
                && i > start
                && chars.get(i + 1).is_some_and(|n| n.1.is_alphabetic());
            if c.is_alphabetic() || inner_apostrophe {
                i += 1;
            } else {
                break;
            }
        }
        let from = chars[start].0;
        let to = chars.get(i).map_or(text.len(), |c| c.0);
        let word = &text[from..to];
        let before = start.checked_sub(1).map(|p| chars[p].1);
        let after = chars.get(i).map(|c| c.1);
        let glued =
            |c: Option<char>, set: &str| c.is_some_and(|c| c.is_ascii_digit() || set.contains(c));
        if word.chars().count() < 3
            || glued(before, "_/\\@#$%.~:")
            || glued(after, "_/\\@#$%~")
            || (after == Some('.') && chars.get(i + 1).is_some_and(|n| n.1.is_alphanumeric()))
        {
            continue;
        }
        let letters: Vec<char> = word.chars().filter(|c| c.is_alphabetic()).collect();
        let camel = letters
            .windows(2)
            .any(|w| w[0].is_lowercase() && w[1].is_uppercase());
        let acronym = letters.len() > 1 && letters.iter().all(|c| c.is_uppercase());
        if camel || acronym {
            continue;
        }
        found.push((from, word));
    }
    found
}

/// What the text of a file is: prose is checked all through, a program only
/// in its comments.
fn is_prose(ext: &str) -> bool {
    matches!(
        ext,
        "" | "txt" | "md" | "markdown" | "rst" | "tex" | "adoc" | "org" | "text"
    )
}

/// The parts of a Markdown text that are not prose: fenced blocks and
/// `inline code`.
fn markdown_code(text: &str) -> Vec<(usize, usize)> {
    let mut parts = Vec::new();
    let mut fence: Option<usize> = None;
    let mut at = 0;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            match fence.take() {
                Some(start) => parts.push((start, at + line.len())),
                None => fence = Some(at),
            }
        } else if fence.is_none() {
            let mut open: Option<usize> = None;
            for (i, c) in line.char_indices() {
                if c == '`' {
                    match open.take() {
                        Some(s) => parts.push((at + s, at + i + 1)),
                        None => open = Some(i),
                    }
                }
            }
        }
        at += line.len();
    }
    if let Some(start) = fence {
        parts.push((start, text.len()));
    }
    parts
}

/// The misspelled words of `text` (the content of a file of the language
/// `ext`) as byte ranges.
pub fn misspelled(speller: &Speller, text: &str, ext: &str) -> Vec<(usize, usize)> {
    let skip: Vec<(usize, usize)>;
    let only: Option<Vec<(usize, usize)>>;
    if is_prose(ext) {
        skip = if matches!(ext, "md" | "markdown") {
            markdown_code(text)
        } else {
            Vec::new()
        };
        only = None;
    } else {
        skip = Vec::new();
        only = Some(
            crate::find_replace::regions(ext, text)
                .into_iter()
                .filter(|(_, _, region)| *region == crate::find_replace::Region::Comment)
                .map(|(s, e, _)| (s, e))
                .collect(),
        );
        if only.as_ref().is_some_and(Vec::is_empty) {
            return Vec::new();
        }
    }
    // The parts are in order and apart: the one a word could be in is found
    // by halves, not by going through them all for each word.
    let sorted = |mut list: Vec<(usize, usize)>| {
        list.sort_unstable();
        list
    };
    let (skip, only) = (sorted(skip), only.map(sorted));
    let inside = |at: usize, list: &[(usize, usize)]| {
        let i = list.partition_point(|(s, _)| *s <= at);
        i > 0 && at < list[i - 1].1
    };
    words(text)
        .into_iter()
        .filter(|(at, _)| only.as_ref().is_none_or(|o| inside(*at, o)))
        .filter(|(at, _)| !inside(*at, &skip))
        .filter(|(_, word)| !speller.knows(word))
        .map(|(at, word)| (at, at + word.len()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn speller() -> Speller {
        Speller::from_text(
            "SET UTF-8\nTRY esianrtolcdugmphbyfvkwzxjq\n",
            "11\nhello\nworld\ncafé\nthe\nis\nnice\ndon't\ncomment\nand\nsee\nme\n",
        )
        .unwrap()
    }

    fn found(text: &str, ext: &str) -> Vec<String> {
        misspelled(&speller(), text, ext)
            .into_iter()
            .map(|(s, e)| text[s..e].to_string())
            .collect()
    }

    #[test]
    fn words_of_prose_are_checked_and_names_of_code_are_not() {
        assert_eq!(found("hello wrold, café nice", "txt"), vec!["wrold"]);
        assert_eq!(
            found("the fooBar and snake_case and v2 and HTTP", "txt"),
            Vec::<String>::new()
        );
        assert_eq!(
            found("see /usr/share/zork and me@zork.com", "txt"),
            Vec::<String>::new()
        );
        assert_eq!(found("don't dont ab", "txt"), vec!["dont"]);
        assert_eq!(
            found("it’s DON’T", "txt").len(),
            1,
            "a curly apostrophe is an apostrophe"
        );
    }

    #[test]
    fn only_the_comments_of_a_program_are_checked() {
        let text = "let wrold = 1; // the wrold is nice\nlet s = \"wrold\";\n";
        assert_eq!(found(text, "rs"), vec!["wrold"]);
        assert_eq!(found("let wrold = 1;\n", "rs"), Vec::<String>::new());
    }

    #[test]
    fn markdown_code_is_left_alone() {
        let text = "hello `wrold` and\n```\nwrold wrold\n```\nwrold again\n";
        assert_eq!(found(text, "md"), vec!["wrold", "again"]);
    }

    #[test]
    fn a_word_is_added_or_ignored_and_suggestions_come() {
        let mut s = speller();
        assert!(!s.knows("zork"));
        s.add_personal("Zork");
        assert!(s.knows("zork") && s.knows("ZORK"));
        assert!(!s.knows("blarg"));
        s.ignore("blarg");
        assert!(s.knows("Blarg"));
        assert!(s.suggest("helo").contains(&"hello".to_string()));
    }

    #[test]
    fn dictionaries_are_found_by_pairs_and_chosen_by_the_language() {
        let dir = tempfile::tempdir().unwrap();
        for name in [
            "fr.dic",
            "fr.aff",
            "en_US.dic",
            "en_US.aff",
            "de.dic",
            "x.aff",
        ] {
            std::fs::write(dir.path().join(name), "").unwrap();
        }
        let list = languages_in(&[dir.path().to_path_buf()]);
        let codes: Vec<&str> = list.iter().map(|l| l.code.as_str()).collect();
        assert_eq!(
            codes,
            vec!["en_US", "fr"],
            "a dictionary needs its affix file"
        );
        let code =
            |wanted: &str, ui: &str| pick_language(wanted, ui, &list).map(|l| l.code.as_str());
        assert_eq!(code("", "fr"), Some("fr"));
        assert_eq!(code("", "en"), Some("en_US"));
        assert_eq!(code("fr_CA", "en"), Some("fr"));
        assert_eq!(code("en-US", "fr"), Some("en_US"));
        assert_eq!(code("", "de"), None);
    }
}
