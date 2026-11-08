//! What is kept between two runs: the collections (a file each, so they can
//! be put under version control), the environments, the history. The secret
//! variables go to the keychain of the system, not to a file.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::model::{Collection, Environment, Request};

/// Most requests kept in the history.
pub const MAX_HISTORY: usize = 200;

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

/// The name a secret goes by in the keychain.
fn secret_name(key: &str) -> String {
    format!("api:{key}")
}

pub fn load() -> Stored {
    let mut stored = Stored::default();
    let Some(root) = dir() else {
        return stored;
    };
    if let Ok(entries) = std::fs::read_dir(root.join("collections")) {
        let mut files: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "json"))
            .collect();
        files.sort();
        for path in files {
            if let Some(collection) = std::fs::read_to_string(&path)
                .ok()
                .and_then(|t| serde_json::from_str::<Collection>(&t).ok())
            {
                stored.collections.push(collection);
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
                if let Some(secret) = crate::database::secrets::load(env.id, &secret_name(&var.key))
                {
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
        .unwrap_or(0);
    stored.next_id = stored.next_id.max(used);
    stored
}

pub fn save_collection(collection: &Collection) {
    if let Some(dir) = collections_dir() {
        if let Ok(text) = serde_json::to_string_pretty(collection) {
            write(&dir.join(format!("{}.json", collection.id)), &text);
        }
    }
}

pub fn delete_collection(id: u64) {
    if let Some(dir) = collections_dir() {
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
                if crate::database::secrets::save(env.id, &secret_name(&var.key), &var.value) {
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

/// Forgets the secrets of an environment that was deleted.
pub fn forget_secrets(env: &Environment) {
    for var in env.vars.iter().filter(|v| v.secret) {
        let _ = crate::database::secrets::save(env.id, &secret_name(&var.key), "");
    }
}

pub fn save_history(history: &[HistoryEntry]) {
    let Some(root) = dir() else { return };
    if let Ok(text) = serde_json::to_string(&history[..history.len().min(MAX_HISTORY)]) {
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
