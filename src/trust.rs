//! The folders whose own programs may run by themselves. A project cloned
//! from elsewhere can ship `node_modules/.bin/eslint` or `.venv/bin/ruff`:
//! they run on opening a file (the lint) only once the folder is trusted.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Where a project keeps its own programs.
pub const LOCAL_BINS: [&str; 4] = ["node_modules/.bin", ".venv/bin", "venv/bin", "vendor/bin"];

static TRUSTED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// The trusted folders, as the preferences keep them.
pub fn set_all(roots: &[String]) {
    if let Ok(mut list) = TRUSTED.lock() {
        *list = roots.iter().map(PathBuf::from).collect();
    }
}

pub fn add(root: &Path) {
    if let Ok(mut list) = TRUSTED.lock() {
        if !list.iter().any(|r| r == root) {
            list.push(root.to_path_buf());
        }
    }
}

/// Whether `near` is in (or is) a trusted folder.
pub fn is_trusted(near: &Path) -> bool {
    TRUSTED
        .lock()
        .is_ok_and(|list| list.iter().any(|root| near.starts_with(root)))
}

/// What a project can carry that runs code once a tool reads it: a
/// Prettier configuration written in JavaScript, tasks of its own, a Cargo
/// build script.
const LOCAL_CODE: [&str; 11] = [
    "prettier.config.js",
    "prettier.config.cjs",
    "prettier.config.mjs",
    "prettier.config.ts",
    ".prettierrc.js",
    ".prettierrc.cjs",
    ".prettierrc.mjs",
    ".prettierrc.ts",
    ".simpleedit/tasks.json",
    "build.rs",
    ".cargo/config.toml",
];

/// Whether the folder holds programs or code of its own to run.
fn holds_code(dir: &Path) -> bool {
    LOCAL_BINS.iter().any(|sub| dir.join(sub).is_dir())
        || LOCAL_CODE.iter().any(|f| dir.join(f).is_file())
}

/// The folder above `near` that holds programs or code of its own, when it
/// is not trusted (yet).
pub fn untrusted_root(near: &Path) -> Option<PathBuf> {
    if is_trusted(near) {
        return None;
    }
    near.ancestors()
        .skip(1)
        .take(12)
        .find(|dir| holds_code(dir))
        .map(Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_with_programs_of_its_own_asks_until_it_is_trusted() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("node_modules/.bin")).unwrap();
        let file = dir.path().join("src/a.ts");
        // A script configuration alone is enough to ask.
        let other = tempfile::tempdir().unwrap();
        std::fs::write(other.path().join("prettier.config.js"), "").unwrap();
        assert_eq!(
            untrusted_root(&other.path().join("a.ts")).as_deref(),
            Some(other.path())
        );
        assert_eq!(untrusted_root(&file).as_deref(), Some(dir.path()));
        add(dir.path());
        assert!(is_trusted(&file));
        assert_eq!(untrusted_root(&file), None);
        // Another folder is not trusted for that.
        assert!(!is_trusted(Path::new("/definitely/elsewhere/a.ts")));
    }
}
