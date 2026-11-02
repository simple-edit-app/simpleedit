//! The changelog, shown in the Help menu and after an update.
//!
//! `CHANGELOG.md` and `CHANGELOG.fr.md` follow Keep a Changelog: one
//! `## [x.y.z] - date` heading per release, `### Section` per kind of
//! change, `- ` per change.
//!
//! Both are compiled in; the update dialog also parses the copy published
//! with the new release.

use crate::updater::parse_version;

const EN: &str = include_str!("../CHANGELOG.md");
const FR: &str = include_str!("../CHANGELOG.fr.md");

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub version: String,
    pub date: String,
    /// `(section title, changes)`.
    pub sections: Vec<(String, Vec<String>)>,
}

/// The releases a changelog describes, newest first as written. Lines that
/// don't fit the format are ignored.
pub fn parse(src: &str) -> Vec<Entry> {
    let mut entries: Vec<Entry> = Vec::new();
    for line in src.lines() {
        let line = line.trim_end();
        if let Some(head) = line.strip_prefix("## ") {
            let (version, date) = head.split_once(" - ").unwrap_or((head, ""));
            let version = version.trim_matches(|c| c == '[' || c == ']' || c == ' ');
            if parse_version(version).is_some() {
                entries.push(Entry {
                    version: version.to_string(),
                    date: date.trim().to_string(),
                    sections: Vec::new(),
                });
            }
        } else if let Some(title) = line.strip_prefix("### ") {
            if let Some(entry) = entries.last_mut() {
                entry.sections.push((title.trim().to_string(), Vec::new()));
            }
        } else if let Some(change) = line.strip_prefix("- ") {
            if let Some((_, changes)) = entries.last_mut().and_then(|e| e.sections.last_mut()) {
                changes.push(change.trim().to_string());
            }
        }
    }
    entries
}

/// File name of the changelog for the interface language.
pub fn file_name(locale: &str) -> &'static str {
    if locale.starts_with("fr") {
        "CHANGELOG.fr.md"
    } else {
        "CHANGELOG.md"
    }
}

static EN_ENTRIES: once_cell::sync::Lazy<Vec<Entry>> = once_cell::sync::Lazy::new(|| parse(EN));
static FR_ENTRIES: once_cell::sync::Lazy<Vec<Entry>> = once_cell::sync::Lazy::new(|| parse(FR));

/// The compiled-in changelog, in the interface language (parsed once).
pub fn embedded() -> Vec<Entry> {
    if rust_i18n::locale().starts_with("fr") {
        FR_ENTRIES.clone()
    } else {
        EN_ENTRIES.clone()
    }
}

/// The releases after `current` up to and including `latest`.
pub fn between(entries: Vec<Entry>, current: &str, latest: &str) -> Vec<Entry> {
    let (Some(current), Some(latest)) = (parse_version(current), parse_version(latest)) else {
        return Vec::new();
    };
    entries
        .into_iter()
        .filter(|e| parse_version(&e.version).is_some_and(|v| v > current && v <= latest))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "# Changelog\n\n## [1.1.0] - 2026-02-01\n\n### Added\n- One\n- Two\n\n### Fixed\n- Three\n\n## [1.0.0] - 2026-01-01\n\n### Added\n- First\n";

    #[test]
    fn parses_releases_sections_and_changes() {
        let e = parse(SAMPLE);
        assert_eq!(e.len(), 2);
        assert_eq!(e[0].version, "1.1.0");
        assert_eq!(e[0].date, "2026-02-01");
        assert_eq!(e[0].sections.len(), 2);
        assert_eq!(
            e[0].sections[0],
            ("Added".into(), vec!["One".into(), "Two".into()])
        );
        assert_eq!(e[1].sections[0].1, vec!["First".to_string()]);
    }

    #[test]
    fn ignores_headings_that_are_not_versions() {
        assert!(parse("## Unreleased\n### Added\n- x\n").is_empty());
    }

    #[test]
    fn between_keeps_only_the_releases_after_the_current_one() {
        let v: Vec<_> = between(parse(SAMPLE), "1.0.0", "1.1.0")
            .into_iter()
            .map(|e| e.version)
            .collect();
        assert_eq!(v, ["1.1.0"]);
        assert!(between(parse(SAMPLE), "1.1.0", "1.1.0").is_empty());
        assert!(between(parse(SAMPLE), "x", "1.1.0").is_empty());
    }

    #[test]
    fn both_shipped_changelogs_cover_the_current_version_with_the_same_releases() {
        let (en, fr) = (parse(EN), parse(FR));
        assert_eq!(
            en.first().map(|e| e.version.as_str()),
            Some(env!("CARGO_PKG_VERSION"))
        );
        let versions = |v: &[Entry]| v.iter().map(|e| e.version.clone()).collect::<Vec<_>>();
        assert_eq!(versions(&en), versions(&fr));
        for (a, b) in en.iter().zip(&fr) {
            assert_eq!(a.sections.len(), b.sections.len(), "{}", a.version);
        }
    }

    #[test]
    fn file_name_follows_the_locale() {
        assert_eq!(file_name("fr"), "CHANGELOG.fr.md");
        assert_eq!(file_name("en"), "CHANGELOG.md");
    }
}
