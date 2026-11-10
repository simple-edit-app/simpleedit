//! The passwords of the data sources, in the keychain of the system (the
//! Keychain of macOS, the Secret Service of Linux) instead of the file of the
//! connections. Where there is no keychain, the caller keeps them in the file.

#[cfg_attr(test, allow(dead_code))]
const SERVICE: &str = "SimpleEdit data source";

fn account(id: u64, kind: &str) -> String {
    format!("{id}/{kind}")
}

#[cfg(all(not(test), any(target_os = "macos", target_os = "linux")))]
mod store {
    use super::{account, SERVICE};

    fn entry(id: u64, kind: &str) -> Option<keyring::Entry> {
        keyring::Entry::new(SERVICE, &account(id, kind)).ok()
    }

    pub fn set(id: u64, kind: &str, secret: &str) -> bool {
        entry(id, kind).is_some_and(|e| e.set_password(secret).is_ok())
    }

    pub fn get(id: u64, kind: &str) -> super::Lookup {
        let Some(entry) = entry(id, kind) else {
            return super::Lookup::Unavailable;
        };
        match entry.get_password() {
            Ok(secret) => super::Lookup::Found(secret),
            Err(keyring::Error::NoEntry) => super::Lookup::Missing,
            // Locked, no service running, access refused…: the secret may
            // well be there.
            Err(_) => super::Lookup::Unavailable,
        }
    }

    pub fn delete(id: u64, kind: &str) {
        if let Some(e) = entry(id, kind) {
            let _ = e.delete_credential();
        }
    }
}

/// Tests (and systems without a keychain) use a memory that is gone with the
/// process: the keychain of the person running the tests stays as it is.
#[cfg(any(test, not(any(target_os = "macos", target_os = "linux"))))]
mod store {
    use super::account;
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};

    fn memory() -> &'static Mutex<HashMap<String, String>> {
        static MEMORY: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
        MEMORY.get_or_init(Mutex::default)
    }

    pub fn set(id: u64, kind: &str, secret: &str) -> bool {
        // Without a keychain nothing is kept: the file holds the secret.
        if cfg!(test) {
            memory()
                .lock()
                .unwrap()
                .insert(account(id, kind), secret.to_string());
            true
        } else {
            false
        }
    }

    pub fn get(id: u64, kind: &str) -> super::Lookup {
        match memory().lock().unwrap().get(&account(id, kind)).cloned() {
            Some(secret) => super::Lookup::Found(secret),
            None => super::Lookup::Missing,
        }
    }

    pub fn delete(id: u64, kind: &str) {
        memory().lock().unwrap().remove(&account(id, kind));
    }
}

pub const PASSWORD: &str = "password";
pub const SSH: &str = "ssh";

/// What the keychain said about a secret.
pub enum Lookup {
    Found(String),
    /// Nothing is kept under that name.
    Missing,
    /// The keychain could not be asked (locked, not running): the secret may
    /// exist, and must not be taken for an empty one.
    #[cfg_attr(test, allow(dead_code))]
    Unavailable,
}

/// The secrets that could not be read: writing "empty" over them would erase
/// them from the keychain.
fn unreadable() -> &'static std::sync::Mutex<std::collections::HashSet<String>> {
    static SET: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> =
        std::sync::OnceLock::new();
    SET.get_or_init(Default::default)
}

fn mark(id: u64, kind: &str, on: bool) {
    if let Ok(mut set) = unreadable().lock() {
        if on {
            set.insert(account(id, kind));
        } else {
            set.remove(&account(id, kind));
        }
    }
}

pub(crate) fn is_unreadable(id: u64, kind: &str) -> bool {
    unreadable()
        .lock()
        .is_ok_and(|set| set.contains(&account(id, kind)))
}

/// Puts a secret in the keychain; `false` if there is none to take it.
pub fn save(id: u64, kind: &str, secret: &str) -> bool {
    if secret.is_empty() {
        // An empty value that only means "could not be read" keeps what is
        // there.
        if !is_unreadable(id, kind) {
            store::delete(id, kind);
        }
        return true;
    }
    let kept = store::set(id, kind, secret);
    if kept {
        mark(id, kind, false);
    }
    kept
}

pub fn load(id: u64, kind: &str) -> Option<String> {
    match store::get(id, kind) {
        Lookup::Found(secret) => {
            mark(id, kind, false);
            Some(secret)
        }
        Lookup::Missing => {
            mark(id, kind, false);
            None
        }
        Lookup::Unavailable => {
            mark(id, kind, true);
            None
        }
    }
}

/// Forgets what the keychain holds for a data source.
pub fn forget(id: u64) {
    mark(id, PASSWORD, false);
    mark(id, SSH, false);
    store::delete(id, PASSWORD);
    store::delete(id, SSH);
}

#[cfg(test)]
pub(crate) fn pretend_unreadable(id: u64, kind: &str) {
    mark(id, kind, true);
}

#[cfg(test)]
impl Lookup {
    fn found(self) -> Option<String> {
        match self {
            Lookup::Found(s) => Some(s),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_secret_is_kept_read_and_forgotten() {
        // An id that no other test uses.
        let id = 9_000_001;
        assert_eq!(load(id, PASSWORD), None);
        assert!(save(id, PASSWORD, "s3cret"));
        assert!(save(id, SSH, "phrase"));
        assert_eq!(load(id, PASSWORD).as_deref(), Some("s3cret"));
        assert_eq!(load(id, SSH).as_deref(), Some("phrase"));
        // An empty secret is no secret.
        assert!(save(id, PASSWORD, ""));
        assert_eq!(load(id, PASSWORD), None);
        forget(id);
        assert_eq!(load(id, SSH), None);
    }

    #[test]
    fn a_secret_that_could_not_be_read_is_not_erased_by_an_empty_save() {
        let id = 9_000_002;
        assert!(save(id, PASSWORD, "s3cret"));
        // The keychain was locked when the connections were read.
        pretend_unreadable(id, PASSWORD);
        assert!(save(id, PASSWORD, ""));
        assert_eq!(store::get(id, PASSWORD).found().as_deref(), Some("s3cret"));
        // Typing a new one replaces it, and an empty one then really erases.
        assert!(save(id, PASSWORD, "new"));
        assert!(save(id, PASSWORD, ""));
        assert_eq!(load(id, PASSWORD), None);
    }
}
