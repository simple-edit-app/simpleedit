// Guards the locale files against drifting apart: a key added to one language
// but not the other silently falls back to the raw key name in the UI.

use std::collections::{BTreeMap, BTreeSet};

fn entries(name: &str) -> BTreeMap<String, String> {
    let path = format!("{}/src/i18n/{name}", env!("CARGO_MANIFEST_DIR"));
    let raw = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let map: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(&raw).unwrap_or_else(|e| panic!("{path}: {e}"));
    map.into_iter()
        .map(|(k, v)| {
            let s = v
                .as_str()
                .unwrap_or_else(|| panic!("{path}: {k} is not a string"))
                .to_string();
            (k, s)
        })
        .collect()
}

fn keys(name: &str) -> BTreeSet<String> {
    entries(name).into_keys().collect()
}

/// The `%{name}` placeholders a translation expects, e.g. "Version %{version}".
fn placeholders(value: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let mut rest = value;
    while let Some(start) = rest.find("%{") {
        rest = &rest[start + 2..];
        match rest.find('}') {
            Some(end) => {
                found.insert(rest[..end].to_string());
                rest = &rest[end + 1..];
            }
            None => break,
        }
    }
    found
}

#[test]
fn locales_define_the_same_keys() {
    let en = keys("en.json");
    let fr = keys("fr.json");

    assert!(!en.is_empty(), "en.json is empty");

    let missing_fr: Vec<_> = en.difference(&fr).collect();
    let missing_en: Vec<_> = fr.difference(&en).collect();

    assert!(
        missing_fr.is_empty(),
        "missing from fr.json: {missing_fr:?}"
    );
    assert!(
        missing_en.is_empty(),
        "missing from en.json: {missing_en:?}"
    );
}

#[test]
fn language_setting_key_is_translated() {
    for name in ["en.json", "fr.json"] {
        assert!(
            keys(name).contains("prefs.language"),
            "{name} has no prefs.language key"
        );
    }
}

#[test]
fn translations_agree_on_placeholders() {
    let en = entries("en.json");
    let fr = entries("fr.json");

    for (key, en_value) in &en {
        let Some(fr_value) = fr.get(key) else {
            continue;
        };
        assert_eq!(
            placeholders(en_value),
            placeholders(fr_value),
            "{key}: placeholders differ between en.json and fr.json"
        );
    }
}

#[test]
fn placeholder_parsing_is_sane() {
    assert!(placeholders("no args here").is_empty());
    assert_eq!(
        placeholders("Version %{version} is available"),
        BTreeSet::from(["version".to_string()])
    );
    assert_eq!(
        placeholders("%{a} then %{b}"),
        BTreeSet::from(["a".to_string(), "b".to_string()])
    );
}
