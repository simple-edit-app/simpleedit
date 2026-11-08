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

    pub fn get(id: u64, kind: &str) -> Option<String> {
        entry(id, kind)?.get_password().ok()
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

    pub fn get(id: u64, kind: &str) -> Option<String> {
        memory().lock().unwrap().get(&account(id, kind)).cloned()
    }

    pub fn delete(id: u64, kind: &str) {
        memory().lock().unwrap().remove(&account(id, kind));
    }
}

pub const PASSWORD: &str = "password";
pub const SSH: &str = "ssh";

/// Puts a secret in the keychain; `false` if there is none to take it.
pub fn save(id: u64, kind: &str, secret: &str) -> bool {
    if secret.is_empty() {
        store::delete(id, kind);
        return true;
    }
    store::set(id, kind, secret)
}

pub fn load(id: u64, kind: &str) -> Option<String> {
    store::get(id, kind)
}

/// Forgets what the keychain holds for a data source.
pub fn forget(id: u64) {
    store::delete(id, PASSWORD);
    store::delete(id, SSH);
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
}
