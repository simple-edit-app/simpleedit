//! One window for the files: `simpleedit file.rs:12` hands the file (and the
//! line) to the window that runs instead of opening another one, unless
//! `--new-window` says so.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// A file named on the command line, with the place to go to (`file:12:5`,
/// line and column counted from 1).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Target {
    pub path: PathBuf,
    pub line: Option<usize>,
    pub col: Option<usize>,
}

impl Target {
    pub fn file(path: impl Into<PathBuf>) -> Target {
        Target {
            path: path.into(),
            ..Target::default()
        }
    }

    /// A command line argument: a path, or `path:line` or `path:line:col`
    /// (when that whole name is not a file itself).
    pub fn parse(arg: &std::ffi::OsStr) -> Target {
        let whole = PathBuf::from(arg);
        let Some(text) = arg.to_str().filter(|_| !whole.exists()) else {
            return Target::file(whole);
        };
        let mut rest = text;
        let mut numbers: Vec<usize> = Vec::new();
        while numbers.len() < 2 {
            match rest.rsplit_once(':') {
                Some((head, tail)) if !head.is_empty() => match tail.parse::<usize>() {
                    Ok(n) => {
                        numbers.push(n);
                        rest = head;
                    }
                    Err(_) => break,
                },
                _ => break,
            }
        }
        numbers.reverse();
        Target {
            path: PathBuf::from(rest),
            line: numbers.first().copied(),
            col: numbers.get(1).copied(),
        }
    }
}

/// What other instances sent and the window has not taken yet.
static INBOX: Mutex<Vec<Vec<Target>>> = Mutex::new(Vec::new());

/// Everything received since the last call.
pub fn take() -> Vec<Vec<Target>> {
    INBOX
        .lock()
        .map(|mut inbox| std::mem::take(&mut *inbox))
        .unwrap_or_default()
}

fn encode(targets: &[Target]) -> String {
    let rows: Vec<(String, Option<usize>, Option<usize>)> = targets
        .iter()
        .map(|t| {
            let path = std::path::absolute(&t.path).unwrap_or_else(|_| t.path.clone());
            (path.to_string_lossy().into_owned(), t.line, t.col)
        })
        .collect();
    serde_json::to_string(&rows).unwrap_or_default()
}

fn decode(text: &str) -> Vec<Target> {
    serde_json::from_str::<Vec<(String, Option<usize>, Option<usize>)>>(text)
        .unwrap_or_default()
        .into_iter()
        .map(|(path, line, col)| Target {
            path: PathBuf::from(path),
            line,
            col,
        })
        .collect()
}

fn socket_path() -> Option<PathBuf> {
    crate::fsutil::data_dir().map(|d| d.join("simpleedit").join("instance.sock"))
}

/// Starts listening, if this window owns the session. `false` when the
/// system can't (the files then open in a window of their own).
pub fn listen() -> bool {
    socket_path().is_some_and(|path| listen_at(&path))
}

/// Gives the files to the window that runs: `false` when there is none.
pub fn send(targets: &[Target]) -> bool {
    socket_path().is_some_and(|path| send_to(&path, targets))
}

#[cfg(unix)]
fn listen_at(path: &Path) -> bool {
    use std::io::Read;
    use std::os::unix::net::UnixListener;
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    // The lock of the session says this window is the one: what is left
    // of an earlier one goes.
    let _ = std::fs::remove_file(path);
    let Ok(listener) = UnixListener::bind(path) else {
        return false;
    };
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));
            let mut text = String::new();
            if stream.read_to_string(&mut text).is_ok() {
                let targets = decode(&text);
                if !targets.is_empty() {
                    if let Ok(mut inbox) = INBOX.lock() {
                        inbox.push(targets);
                    }
                }
            }
        }
    });
    true
}

#[cfg(unix)]
fn send_to(path: &Path, targets: &[Target]) -> bool {
    use std::io::Write;
    use std::os::unix::net::UnixStream;
    let Ok(mut stream) = UnixStream::connect(path) else {
        return false;
    };
    let _ = stream.set_write_timeout(Some(std::time::Duration::from_secs(2)));
    stream.write_all(encode(targets).as_bytes()).is_ok()
}

#[cfg(not(unix))]
fn listen_at(_: &Path) -> bool {
    false
}

#[cfg(not(unix))]
fn send_to(_: &Path, _: &[Target]) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    fn parsed(arg: &str) -> (String, Option<usize>, Option<usize>) {
        let t = Target::parse(OsStr::new(arg));
        (t.path.to_string_lossy().into_owned(), t.line, t.col)
    }

    #[test]
    fn a_line_and_a_column_follow_the_name() {
        assert_eq!(parsed("a/b.rs:12"), ("a/b.rs".into(), Some(12), None));
        assert_eq!(parsed("a/b.rs:12:5"), ("a/b.rs".into(), Some(12), Some(5)));
        assert_eq!(parsed("b.rs"), ("b.rs".into(), None, None));
        assert_eq!(parsed("b.rs:x"), ("b.rs:x".into(), None, None));
        assert_eq!(parsed(":12"), (":12".into(), None, None));
    }

    #[test]
    fn a_file_whose_name_has_a_colon_is_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let odd = dir.path().join("a:12");
        std::fs::write(&odd, "").unwrap();
        let t = Target::parse(odd.as_os_str());
        assert_eq!((t.path, t.line), (odd, None));
    }

    #[cfg(unix)]
    #[test]
    fn a_window_receives_what_a_second_command_sends() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("i.sock");
        assert!(
            !send_to(&socket, &[Target::file("x")]),
            "nobody listens yet"
        );
        assert!(listen_at(&socket));
        let target = Target {
            path: dir.path().join("a.rs"),
            line: Some(3),
            col: Some(2),
        };
        assert!(send_to(&socket, std::slice::from_ref(&target)));
        let mut got = Vec::new();
        for _ in 0..100 {
            got = take();
            if !got.is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert_eq!(got, vec![vec![target]]);
    }
}
