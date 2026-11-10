//! What is kept between two runs: the collections (a file each, so they can
//! be put under version control), the environments, the history. The secret
//! variables go to the keychain of the system, not to a file.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::model::{Collection, Environment, Item, Request};

/// Most requests kept in the history.
pub const MAX_HISTORY: usize = 200;

/// Most of a body text kept in the history file.
const MAX_HISTORY_BODY: usize = 64 * 1024;

/// A request that was sent.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HistoryEntry {
    /// `YYYY-MM-DD HH:MM:SS`.
    pub time: String,
    pub request: Request,
    pub status: Option<u16>,
    pub ms: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct EnvironmentsFile {
    active: Option<u64>,
    next_id: u64,
    environments: Vec<Environment>,
}

/// Everything that was kept.
#[derive(Debug, Clone, Default)]
pub struct Stored {
    pub collections: Vec<Collection>,
    pub environments: Vec<Environment>,
    pub active_env: Option<u64>,
    pub next_id: u64,
    pub history: Vec<HistoryEntry>,
}

pub fn dir() -> Option<PathBuf> {
    Some(crate::fsutil::data_dir()?.join("simpleedit").join("api"))
}

fn collections_dir() -> Option<PathBuf> {
    Some(dir()?.join("collections"))
}

fn write(path: &std::path::Path, text: &str) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = crate::fsutil::write_atomic(path, text.as_bytes(), true);
}

/// What the keychain is known to hold, by owner and name: every call to it
/// is a synchronous D-Bus round trip (and a locked keychain asks a question),
/// so a secret that is already as it should be is not asked about again.
fn keychain_known() -> &'static std::sync::Mutex<std::collections::HashMap<(u64, String), String>> {
    static KNOWN: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<(u64, String), String>>,
    > = std::sync::OnceLock::new();
    KNOWN.get_or_init(Default::default)
}

/// `secrets::save`, skipped when the keychain already holds that (an empty
/// value is "nothing kept").
fn keychain_save(id: u64, name: &str, value: &str) -> bool {
    let key = (id, name.to_string());
    if keychain_known()
        .lock()
        .is_ok_and(|known| known.get(&key).is_some_and(|v| v == value))
    {
        return true;
    }
    let kept = crate::database::secrets::save(id, name, value);
    // An empty write over an unreadable secret kept it: that is not known.
    if kept && !(value.is_empty() && crate::database::secrets::is_unreadable(id, name)) {
        if let Ok(mut known) = keychain_known().lock() {
            known.insert(key, value.to_string());
        }
    }
    kept
}

/// `secrets::load`, asking the keychain once for what it is known to hold.
fn keychain_load(id: u64, name: &str) -> Option<String> {
    let key = (id, name.to_string());
    if let Some(value) = keychain_known()
        .lock()
        .ok()
        .and_then(|known| known.get(&key).cloned())
    {
        return (!value.is_empty()).then_some(value);
    }
    let found = crate::database::secrets::load(id, name);
    if let (Some(value), Ok(mut known)) = (&found, keychain_known().lock()) {
        known.insert(key, value.clone());
    }
    found
}

/// The name a secret goes by in the keychain.
fn secret_name(key: &str) -> String {
    format!("api:{key}")
}

/// The credentials of a request's Auth, by the name they have in the
/// keychain.
fn auth_secrets(request: &mut Request) -> [(&'static str, &mut String); 3] {
    [
        ("apiauth:token", &mut request.auth.token),
        ("apiauth:password", &mut request.auth.password),
        ("apiauth:key", &mut request.auth.key_value),
    ]
}

/// A value that is only variables (`{{token}}`) holds no secret by itself.
fn only_variables(value: &str) -> bool {
    let rest = value.trim();
    !rest.is_empty()
        && rest.split("{{").enumerate().all(|(i, part)| {
            (i == 0 && part.is_empty()) || part.matches("}}").count() == 1 && part.ends_with("}}")
        })
}

fn each_request(items: &mut [Item], f: &mut impl FnMut(&mut Request)) {
    for item in items {
        match item {
            Item::Request(r) => f(r),
            Item::Folder(folder) => each_request(&mut folder.items, f),
        }
    }
}

/// Takes the Auth credentials of the requests out of a collection about to
/// be written (the file is meant to be shared, even versioned): they go to
/// the keychain, and stay in the file only where there is none.
fn move_auth_to_keychain(collection: &mut Collection) {
    each_request(&mut collection.items, &mut |request| {
        let id = request.id;
        if id == 0 {
            return;
        }
        for (name, value) in auth_secrets(request) {
            if value.is_empty() || only_variables(value) {
                // Nothing to hide; an older secret is not to come back.
                let _ = keychain_save(id, name, "");
            } else if keychain_save(id, name, value) {
                value.clear();
            }
        }
    });
}

/// The reverse, for a collection just read.
fn restore_auth_from_keychain(collection: &mut Collection) {
    each_request(&mut collection.items, &mut |request| {
        let id = request.id;
        for (name, value) in auth_secrets(request) {
            if id != 0 && value.is_empty() {
                if let Some(secret) = keychain_load(id, name) {
                    *value = secret;
                }
            }
        }
    });
}

pub fn load() -> Stored {
    let mut stored = Stored::default();
    let Some(root) = dir() else {
        return stored;
    };
    let mut unreadable_id = 0;
    if let Ok(entries) = std::fs::read_dir(root.join("collections")) {
        let mut files: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "json"))
            .collect();
        files.sort();
        for path in files {
            if let Some(mut collection) = std::fs::read_to_string(&path)
                .ok()
                .and_then(|t| serde_json::from_str::<Collection>(&t).ok())
            {
                restore_auth_from_keychain(&mut collection);
                stored.collections.push(collection);
            } else {
                // Unreadable: kept aside, and its number stays taken.
                if let Some(id) = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .and_then(|s| s.parse::<u64>().ok())
                {
                    unreadable_id = unreadable_id.max(id);
                }
                let _ = crate::fsutil::set_aside(&path);
            }
        }
    }
    if let Some(file) = std::fs::read_to_string(root.join("environments.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<EnvironmentsFile>(&t).ok())
    {
        stored.environments = file.environments;
        stored.active_env = file.active;
        stored.next_id = file.next_id;
        // The secrets come back from the keychain.
        for env in &mut stored.environments {
            for var in env
                .vars
                .iter_mut()
                .filter(|v| v.secret && v.value.is_empty())
            {
                if let Some(secret) = keychain_load(env.id, &secret_name(&var.key)) {
                    var.value = secret;
                }
            }
        }
    }
    if let Some(history) = std::fs::read_to_string(root.join("history.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<Vec<HistoryEntry>>(&t).ok())
    {
        stored.history = history;
    }
    // The next number is after every number in use.
    let used = stored
        .collections
        .iter()
        .map(Collection::max_id)
        .chain(stored.environments.iter().map(|e| e.id))
        .max()
        .unwrap_or(0)
        .max(unreadable_id);
    stored.next_id = stored.next_id.max(used);
    stored
}

pub fn save_collection(collection: &Collection) {
    if let Some(dir) = collections_dir() {
        let mut kept = collection.clone();
        move_auth_to_keychain(&mut kept);
        if let Ok(text) = serde_json::to_string_pretty(&kept) {
            write(&dir.join(format!("{}.json", collection.id)), &text);
        }
    }
}

pub fn delete_collection(id: u64) {
    if let Some(dir) = collections_dir() {
        // Its credentials leave the keychain with it.
        if let Some(mut collection) = std::fs::read_to_string(dir.join(format!("{id}.json")))
            .ok()
            .and_then(|t| serde_json::from_str::<Collection>(&t).ok())
        {
            each_request(&mut collection.items, &mut |request| {
                let rid = request.id;
                for (name, _) in auth_secrets(request) {
                    let _ = keychain_save(rid, name, "");
                }
            });
        }
        let _ = std::fs::remove_file(dir.join(format!("{id}.json")));
    }
}

pub fn save_environments(environments: &[Environment], active: Option<u64>, next_id: u64) {
    let Some(root) = dir() else { return };
    // A secret goes to the keychain; where there is none, the file keeps it.
    let kept: Vec<Environment> = environments
        .iter()
        .map(|env| {
            let mut env = env.clone();
            for var in env.vars.iter_mut().filter(|v| v.secret) {
                if keychain_save(env.id, &secret_name(&var.key), &var.value) {
                    var.value.clear();
                }
            }
            env
        })
        .collect();
    let file = EnvironmentsFile {
        active,
        next_id,
        environments: kept,
    };
    if let Ok(text) = serde_json::to_string_pretty(&file) {
        write(&root.join("environments.json"), &text);
    }
}

/// Forgets one secret variable's entry in the keychain.
pub fn forget_secret(env_id: u64, key: &str) {
    let _ = keychain_save(env_id, &secret_name(key), "");
}

/// Forgets the secrets of an environment that was deleted.
pub fn forget_secrets(env: &Environment) {
    for var in env.vars.iter().filter(|v| v.secret) {
        let _ = keychain_save(env.id, &secret_name(&var.key), "");
    }
}

pub fn save_history(history: &[HistoryEntry]) {
    let Some(root) = dir() else { return };
    // The history is a record of what was sent, not a place for credentials.
    let mut kept: Vec<HistoryEntry> = history[..history.len().min(MAX_HISTORY)].to_vec();
    for entry in &mut kept {
        let text = &mut entry.request.body.text;
        if text.len() > MAX_HISTORY_BODY {
            let mut end = MAX_HISTORY_BODY;
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            text.truncate(end);
            text.push_str("\n[... truncated]");
        }
        for (_, value) in auth_secrets(&mut entry.request) {
            if !only_variables(value) {
                value.clear();
            }
        }
    }
    if let Ok(text) = serde_json::to_string(&kept) {
        write(&root.join("history.json"), &text);
    }
}

/// The files of the tests live in a folder of their own, shared: one test at
/// a time uses it, and it starts empty.
#[cfg(test)]
pub fn test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(root) = dir() {
        let _ = std::fs::remove_dir_all(root);
    }
    guard
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::model::{Item, Method, Variable};

    use super::test_lock as lock;

    #[test]
    fn collections_environments_and_history_come_back_as_they_were_kept() {
        let _guard = lock();
        let mut request = Request::new("list");
        request.id = 3;
        request.method = Method::Post;
        let collection = Collection {
            id: 2,
            name: "shop".into(),
            items: vec![Item::Request(request.clone())],
        };
        save_collection(&collection);
        let env = Environment {
            id: 5,
            name: "dev".into(),
            vars: vec![Variable {
                key: "host".into(),
                value: "localhost".into(),
                ..Variable::default()
            }],
        };
        save_environments(std::slice::from_ref(&env), Some(5), 9);
        save_history(&[HistoryEntry {
            time: "2026-11-09 10:00:00".into(),
            request,
            status: Some(200),
            ms: 12,
            error: None,
        }]);
        let back = load();
        assert_eq!(back.collections, vec![collection]);
        assert_eq!(back.environments, vec![env]);
        assert_eq!(back.active_env, Some(5));
        assert_eq!(back.next_id, 9);
        assert_eq!(back.history.len(), 1);
        assert_eq!(back.history[0].status, Some(200));
    }

    #[test]
    fn a_secret_is_not_written_in_the_file_but_comes_back() {
        let _guard = lock();
        let env = Environment {
            id: 7,
            name: "prod".into(),
            vars: vec![
                Variable {
                    key: "token".into(),
                    value: "s3cret".into(),
                    secret: true,
                    ..Variable::default()
                },
                Variable {
                    key: "host".into(),
                    value: "api.test".into(),
                    ..Variable::default()
                },
            ],
        };
        save_environments(std::slice::from_ref(&env), None, 7);
        let text = std::fs::read_to_string(dir().unwrap().join("environments.json")).unwrap();
        assert!(!text.contains("s3cret"), "{text}");
        assert!(text.contains("api.test"));
        assert_eq!(load().environments, vec![env.clone()]);
        // Forgotten with its environment.
        forget_secrets(&env);
        let gone = crate::database::secrets::load(7, "api:token");
        assert!(gone.is_none() || gone.as_deref() == Some(""));
    }

    #[test]
    fn the_credentials_of_a_request_stay_out_of_the_files() {
        let _guard = lock();
        let mut request = Request::new("login");
        request.id = 77;
        request.auth.kind = crate::api::model::AuthKind::Bearer;
        request.auth.token = "tok-s3cret".into();
        request.auth.password = "{{pass}}".into();
        let collection = Collection {
            id: 8,
            name: "auth".into(),
            items: vec![Item::Request(request.clone())],
        };
        save_collection(&collection);
        save_history(&[HistoryEntry {
            request,
            ..HistoryEntry::default()
        }]);
        let root = dir().unwrap();
        let file = std::fs::read_to_string(root.join("collections/8.json")).unwrap();
        let history = std::fs::read_to_string(root.join("history.json")).unwrap();
        assert!(!file.contains("tok-s3cret") && !history.contains("tok-s3cret"));
        assert!(file.contains("{{pass}}") && history.contains("{{pass}}"));
        // It comes back from the keychain.
        assert_eq!(load().collections, vec![collection]);
        delete_collection(8);
        assert!(crate::database::secrets::load(77, "apiauth:token").is_none());
    }

    #[test]
    fn a_deleted_collection_is_gone_and_the_numbers_go_on_after_the_biggest() {
        let _guard = lock();
        let mut request = Request::new("r");
        request.id = 40;
        for id in [1, 2] {
            save_collection(&Collection {
                id,
                name: format!("c{id}"),
                items: if id == 2 {
                    vec![Item::Request(request.clone())]
                } else {
                    Vec::new()
                },
            });
        }
        delete_collection(1);
        let back = load();
        assert_eq!(back.collections.len(), 1);
        assert_eq!(back.next_id, 40);
    }

    #[test]
    fn nothing_kept_gives_nothing_not_an_error() {
        let _guard = lock();
        let back = load();
        assert!(
            back.collections.is_empty() && back.environments.is_empty() && back.history.is_empty()
        );
        // A file that is not what it should be is left aside.
        let root = dir().unwrap();
        std::fs::create_dir_all(root.join("collections")).unwrap();
        std::fs::write(root.join("collections").join("9.json"), "not json").unwrap();
        assert!(load().collections.is_empty());
    }
}
