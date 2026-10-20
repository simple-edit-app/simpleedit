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

/// Every `t!("key")` in the source, with the file it is in.
fn used_keys() -> Vec<(String, String)> {
    fn walk(dir: &std::path::Path, out: &mut Vec<(String, String)>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let source = std::fs::read_to_string(&path).unwrap();
                let mut rest = source.as_str();
                while let Some(at) = rest.find("t!(\"") {
                    // `format!("…")` ends in `t!(` too: only a bare macro counts.
                    let bare = !rest[..at]
                        .chars()
                        .next_back()
                        .is_some_and(|c| c.is_alphanumeric() || c == '_');
                    rest = &rest[at + 4..];
                    if !bare {
                        continue;
                    }
                    if let Some(end) = rest.find('"') {
                        // Only literal keys: `t!("a.b"` or `t!("a.b", x = …`.
                        let key = &rest[..end];
                        if key.contains('.') && !key.contains(' ') {
                            out.push((key.to_string(), path.display().to_string()));
                        }
                    }
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut out,
    );
    out
}

#[test]
fn every_key_the_code_asks_for_is_translated() {
    let en = keys("en.json");
    let missing: Vec<_> = used_keys()
        .into_iter()
        .filter(|(key, _)| !en.contains(key))
        .collect();
    assert!(
        missing.is_empty(),
        "keys without a translation: {missing:?}"
    );
}

#[test]
fn no_translation_is_left_unused() {
    let used: BTreeSet<String> = used_keys().into_iter().map(|(k, _)| k).collect();
    // Keys built at run time (`format!("shortcuts.{name}")`, …) can't be seen
    // above; their prefixes are listed here.
    let dynamic: [&str; 0] = [];
    let unused: Vec<_> = keys("en.json")
        .into_iter()
        .filter(|k| !used.contains(k) && !dynamic.iter().any(|p| k.starts_with(p)))
        .collect();
    assert!(unused.is_empty(), "unused keys: {unused:?}");
}
