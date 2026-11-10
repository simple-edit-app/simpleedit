//! Tasks of a project (build, test, run…): found in its files or written
//! by hand in `.simpleedit/tasks.json`, run with their output streamed, and
//! the places an output names (`src/a.rs:12:5`) found in it.

use std::path::{Path, PathBuf};

/// A command of a project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskSpec {
    pub name: String,
    pub command: String,
    pub dir: PathBuf,
    /// Written in a project's own `tasks.json`: the project, which must be
    /// trusted for it to run (not its `cwd`, which the file chooses).
    pub custom: Option<PathBuf>,
}

/// A place an output line names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    pub path: PathBuf,
    /// From 1.
    pub line: usize,
    /// From 1; 1 when the line gives none.
    pub col: usize,
}

/// What a run says as it goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Line(String),
    /// Lines that came together (the stream sends them in packets).
    Lines(Vec<String>),
    /// The exit code (`None`: stopped by a signal, or never started).
    Done(Option<i32>),
}

fn spec(name: &str, command: &str, dir: &Path) -> TaskSpec {
    TaskSpec {
        name: name.to_string(),
        command: command.to_string(),
        dir: dir.to_path_buf(),
        custom: None,
    }
}

/// The targets of a Makefile that are not patterns, variables or hidden.
fn make_targets(text: &str) -> Vec<String> {
    let mut targets: Vec<String> = Vec::new();
    for line in text.lines() {
        if line.starts_with([' ', '\t', '#', '.']) {
            continue;
        }
        let Some((head, tail)) = line.split_once(':') else {
            continue;
        };
        // `a := 1` and `a::` are not targets of their own.
        if tail.starts_with('=') || head.contains(['=', '%', '$']) {
            continue;
        }
        for name in head.split_whitespace() {
            if !targets.iter().any(|t| t == name) {
                targets.push(name.to_string());
            }
        }
    }
    targets
}

/// The tasks of the project at `root`: the ones written in
/// `.simpleedit/tasks.json` (`[{"name", "command", "cwd"}]`) first, then
/// those its files imply (Cargo, npm scripts, Make, Go).
pub fn detect(root: &Path) -> Vec<TaskSpec> {
    let mut tasks = Vec::new();
    if let Ok(raw) = std::fs::read_to_string(root.join(".simpleedit").join("tasks.json")) {
        if let Ok(list) = serde_json::from_str::<Vec<serde_json::Value>>(&raw) {
            for item in list {
                let (Some(name), Some(command)) = (item["name"].as_str(), item["command"].as_str())
                else {
                    continue;
                };
                let dir = item["cwd"]
                    .as_str()
                    .map_or_else(|| root.to_path_buf(), |c| root.join(c));
                tasks.push(TaskSpec {
                    custom: Some(root.to_path_buf()),
                    ..spec(name, command, &dir)
                });
            }
        }
    }
    if root.join("Cargo.toml").is_file() {
        for (name, command) in [
            ("cargo build", "cargo build"),
            ("cargo run", "cargo run"),
            ("cargo test", "cargo test"),
            ("cargo check", "cargo check"),
            ("cargo clippy", "cargo clippy"),
            ("cargo fmt", "cargo fmt"),
        ] {
            tasks.push(spec(name, command, root));
        }
    }
    if let Ok(raw) = std::fs::read_to_string(root.join("package.json")) {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&raw) {
            if let Some(scripts) = json["scripts"].as_object() {
                for name in scripts.keys() {
                    tasks.push(spec(
                        &format!("npm run {name}"),
                        &format!("npm run {}", crate::remote::quote(name)),
                        root,
                    ));
                }
            }
        }
    }
    for name in ["Makefile", "makefile", "GNUmakefile"] {
        if let Ok(text) = std::fs::read_to_string(root.join(name)) {
            for target in make_targets(&text) {
                tasks.push(spec(
                    &format!("make {target}"),
                    &format!("make -- {}", crate::remote::quote(&target)),
                    root,
                ));
            }
            break;
        }
    }
    if root.join("go.mod").is_file() {
        tasks.push(spec("go build", "go build ./...", root));
        tasks.push(spec("go test", "go test ./...", root));
        tasks.push(spec("go vet", "go vet ./...", root));
    }
    tasks
}

/// The first place a line of output names that is a file: `path:12:5`,
/// `path:12`, `path(12,5)` (TypeScript), `--> path:12:5` (Rust) or
/// `File "path", line 12` (Python). Relative paths are from `dir`.
#[cfg(test)]
pub fn location(line: &str, dir: &Path) -> Option<Location> {
    location_with(line, dir, &mut |p| p.is_file())
}

/// [`location`], asking `is_file` whether a path is a file (so that a caller
/// reading many lines can remember the answers).
pub fn location_with(
    line: &str,
    dir: &Path,
    is_file: &mut dyn FnMut(&Path) -> bool,
) -> Option<Location> {
    use std::sync::OnceLock;
    static PYTHON: OnceLock<regex::Regex> = OnceLock::new();
    static PLAIN: OnceLock<regex::Regex> = OnceLock::new();
    let python = PYTHON.get_or_init(|| {
        regex::Regex::new(r#"File "([^"]+)", line (\d+)"#).expect("a valid pattern")
    });
    let plain = PLAIN.get_or_init(|| {
        regex::Regex::new(r#"([^\s:()"'<>|]+\.[A-Za-z0-9_]+)(?::|\()(\d+)(?:[:,](\d+))?"#)
            .expect("a valid pattern")
    });
    static SPACED: OnceLock<regex::Regex> = OnceLock::new();
    let spaced = SPACED.get_or_init(|| {
        regex::Regex::new(r#"(/[^:()"'<>|]+\.[A-Za-z0-9_]+)(?::|\()(\d+)(?:[:,](\d+))?"#)
            .expect("a valid pattern")
    });
    let mut resolve = |path: &str| {
        let path = PathBuf::from(path);
        let full = if path.is_absolute() {
            path
        } else {
            dir.join(path)
        };
        is_file(&full).then_some(full)
    };
    if let Some(c) = python.captures(line) {
        if let Some(path) = resolve(&c[1]) {
            return Some(Location {
                path,
                line: c[2].parse().ok()?,
                col: 1,
            });
        }
    }
    for c in plain.captures_iter(line) {
        let Some(path) = resolve(&c[1]) else { continue };
        return Some(Location {
            path,
            line: c[2].parse().ok()?,
            col: c.get(3).and_then(|m| m.as_str().parse().ok()).unwrap_or(1),
        });
    }
    // An absolute path with spaces: the longest tail that is a file.
    for c in spaced.captures_iter(line) {
        let whole = &c[1];
        let starts = whole.match_indices('/').map(|(i, _)| i);
        for start in starts {
            if let Some(path) = resolve(&whole[start..]) {
                return Some(Location {
                    path,
                    line: c[2].parse().ok()?,
                    col: c.get(3).and_then(|m| m.as_str().parse().ok()).unwrap_or(1),
                });
            }
        }
    }
    None
}

/// A line without the escape sequences that color or move the cursor.
pub fn strip_ansi(line: &str) -> String {
    use std::sync::OnceLock;
    static ESCAPES: OnceLock<regex::Regex> = OnceLock::new();
    let escapes = ESCAPES.get_or_init(|| {
        regex::Regex::new(r"\x1b\[[0-9;?]*[ -/]*[@-~]|\r").expect("a valid pattern")
    });
    escapes.replace_all(line, "").into_owned()
}

/// Runs the task, its output (both streams) line by line, then the exit
/// code. Dropping the stream stops the command.
pub fn run(task: TaskSpec) -> impl iced::futures::Stream<Item = Event> {
    use iced::futures::SinkExt;
    use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};

    fn forward<R: AsyncRead + Unpin + Send + 'static>(
        reader: R,
        to: tokio::sync::mpsc::UnboundedSender<String>,
    ) {
        tokio::spawn(async move {
            // Bytes, not `lines()`: an invalid UTF-8 byte must not end the
            // output (and close the pipe under the command).
            let mut reader = BufReader::new(reader);
            let mut buf = Vec::new();
            loop {
                buf.clear();
                if !matches!(reader.read_until(b'\n', &mut buf).await, Ok(n) if n > 0) {
                    break;
                }
                if buf.last() == Some(&b'\n') {
                    buf.pop();
                    if buf.last() == Some(&b'\r') {
                        buf.pop();
                    }
                }
                if to.send(String::from_utf8_lossy(&buf).into_owned()).is_err() {
                    break;
                }
            }
        });
    }

    iced::stream::channel(256, async move |mut output| {
        let mut command = if cfg!(windows) {
            let mut c = tokio::process::Command::new("cmd");
            c.arg("/C");
            c
        } else {
            let mut c = tokio::process::Command::new("sh");
            c.arg("-c");
            c
        };
        // In a group of its own, so that a stop reaches what the shell
        // started too (a dev server, the program `cargo run` built).
        #[cfg(unix)]
        command.process_group(0);
        let spawned = command
            .arg(&task.command)
            .current_dir(&task.dir)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn();
        let mut child = match spawned {
            Ok(child) => child,
            Err(e) => {
                let _ = output
                    .send(Event::Line(format!("{}: {e}", task.command)))
                    .await;
                let _ = output.send(Event::Done(None)).await;
                return;
            }
        };
        // Dropped with the stream (a stop), it ends the whole group.
        #[cfg(unix)]
        let mut group = Group(child.id().map(|pid| pid as i32));
        #[cfg(unix)]
        group.watch();
        let (to, mut from) = tokio::sync::mpsc::unbounded_channel();
        if let Some(out) = child.stdout.take() {
            forward(out, to.clone());
        }
        if let Some(err) = child.stderr.take() {
            forward(err, to.clone());
        }
        drop(to);
        // What is already there goes in one packet: a verbose build would
        // otherwise cost the window an update per line.
        while let Some(line) = from.recv().await {
            let mut packet = vec![line];
            while packet.len() < PACKET {
                match from.try_recv() {
                    Ok(line) => packet.push(line),
                    Err(_) => break,
                }
            }
            if output.send(Event::Lines(packet)).await.is_err() {
                return;
            }
        }
        let code = child.wait().await.ok().and_then(|s| s.code());
        // Ended by itself: what it left running on purpose stays.
        #[cfg(unix)]
        group.disarm();
        let _ = output.send(Event::Done(code)).await;
    })
}

/// The most lines sent in one packet.
const PACKET: usize = 200;

/// How long a stopped task has to end on SIGTERM before SIGKILL.
#[cfg(unix)]
const GRACE: std::time::Duration = std::time::Duration::from_secs(1);

/// The process groups of the tasks that run, for [`stop_all`].
#[cfg(unix)]
static RUNNING: std::sync::Mutex<Vec<i32>> = std::sync::Mutex::new(Vec::new());

/// The process group of a task, ended when dropped.
#[cfg(unix)]
struct Group(Option<i32>);

#[cfg(unix)]
impl Group {
    fn watch(&self) {
        if let (Some(id), Ok(mut running)) = (self.0, RUNNING.lock()) {
            running.push(id);
        }
    }

    fn disarm(&mut self) {
        if let (Some(id), Ok(mut running)) = (self.0.take(), RUNNING.lock()) {
            running.retain(|g| *g != id);
        }
    }
}

#[cfg(unix)]
impl Drop for Group {
    fn drop(&mut self) {
        let Some(id) = self.0 else {
            return;
        };
        self.disarm();
        // Waiting for it here would hold up the runtime that drops the task.
        std::thread::spawn(move || end_group(id, GRACE));
    }
}

/// Asks a process group to end, then forces it after `grace`.
#[cfg(unix)]
fn end_group(id: i32, grace: std::time::Duration) {
    let alive = || unsafe { libc::killpg(id, 0) } == 0;
    unsafe { libc::killpg(id, libc::SIGTERM) };
    let start = std::time::Instant::now();
    while alive() && start.elapsed() < grace {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    if alive() {
        unsafe { libc::killpg(id, libc::SIGKILL) };
    }
}

/// Ends the tasks that still run, for the app that quits: its exit would
/// leave them running.
pub fn stop_all() {
    #[cfg(unix)]
    {
        let groups = RUNNING.lock().map(|mut r| std::mem::take(&mut *r));
        for id in groups.unwrap_or_default() {
            end_group(id, std::time::Duration::from_millis(300));
        }
    }
}

/// The byte column of the `col`-th character (1-based, as compilers count)
/// of `line` (1-based) in the file; `col` itself when the file is unreadable.
pub fn byte_col(path: &Path, line: usize, col: usize) -> usize {
    let Ok(text) = std::fs::read_to_string(path) else {
        return col;
    };
    let Some(text) = text.lines().nth(line.saturating_sub(1)) else {
        return col;
    };
    match text.char_indices().nth(col.saturating_sub(1)) {
        Some((i, _)) => i + 1,
        None => text.len() + 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::futures::StreamExt;

    #[test]
    fn a_path_with_spaces_and_a_character_column() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("Mon Projet");
        std::fs::create_dir_all(&sub).unwrap();
        let file = sub.join("a.rs");
        std::fs::write(&file, "é = 1;\n").unwrap();
        let line = format!("--> {}:1:3", file.display());
        let at = location(&line, dir.path()).unwrap();
        assert_eq!(at.path, file);
        assert_eq!(byte_col(&at.path, 1, at.col), 4);
    }

    #[test]
    fn the_tasks_of_a_project_come_from_its_files_and_its_own_list() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("Cargo.toml"), "").unwrap();
        std::fs::write(
            root.join("package.json"),
            r#"{"scripts": {"build": "x", "lint": "y"}}"#,
        )
        .unwrap();
        std::fs::write(
            root.join("Makefile"),
            "CC := gcc\n.PHONY: all\nall: a.o\n\tcc\nclean:\n\trm\n%.o: %.c\n\tcc\ninstall test: all\n",
        )
        .unwrap();
        std::fs::create_dir(root.join(".simpleedit")).unwrap();
        std::fs::write(
            root.join(".simpleedit/tasks.json"),
            r#"[{"name": "serve", "command": "python -m http.server", "cwd": "web"}, {"name": "no command"}]"#,
        )
        .unwrap();
        let names: Vec<String> = detect(root).into_iter().map(|t| t.name).collect();
        assert_eq!(names[0], "serve");
        for wanted in [
            "cargo test",
            "npm run build",
            "npm run lint",
            "make all",
            "make clean",
            "make install",
            "make test",
        ] {
            assert!(names.contains(&wanted.to_string()), "{wanted}: {names:?}");
        }
        assert!(!names.iter().any(|n| n.contains('%') || n.contains("CC")));
        assert_eq!(detect(root)[0].dir, root.join("web"));
        // What must be trusted is the project, whatever folder it runs in.
        assert_eq!(detect(root)[0].custom.as_deref(), Some(root));
        assert!(detect(root).iter().skip(1).all(|t| t.custom.is_none()));
        assert!(detect(&root.join("empty")).is_empty());
    }

    #[test]
    fn the_places_an_output_names_are_found_when_the_file_exists() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir(root.join("src")).unwrap();
        std::fs::write(root.join("src/a.rs"), "").unwrap();
        std::fs::write(root.join("b.py"), "").unwrap();
        let at = |line: &str| location(line, root).map(|l| (l.path, l.line, l.col));
        let a = root.join("src/a.rs");
        assert_eq!(at("  --> src/a.rs:12:5"), Some((a.clone(), 12, 5)));
        assert_eq!(at("src/a.rs:7: error: x"), Some((a.clone(), 7, 1)));
        assert_eq!(at("src/a.rs(3,9): error TS1"), Some((a.clone(), 3, 9)));
        assert_eq!(
            at(r#"  File "b.py", line 4, in <module>"#),
            Some((root.join("b.py"), 4, 1))
        );
        // A name that is not a file, or a time, is no place.
        assert_eq!(at("src/none.rs:12:5"), None);
        assert_eq!(at("took 12:30:45"), None);
        assert_eq!(at("nothing here"), None);
    }

    #[test]
    fn colors_and_carriage_returns_are_taken_out_of_a_line() {
        assert_eq!(strip_ansi("\x1b[1;31merror\x1b[0m: x\r"), "error: x");
        assert_eq!(strip_ansi("plain"), "plain");
    }

    fn flat(events: &[Event]) -> Vec<String> {
        events
            .iter()
            .flat_map(|e| match e {
                Event::Line(l) => vec![l.clone()],
                Event::Lines(l) => l.clone(),
                _ => Vec::new(),
            })
            .collect()
    }

    #[test]
    fn a_burst_of_lines_comes_in_packets() {
        if cfg!(windows) {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let events: Vec<Event> =
            runtime.block_on(async { run(spec("t", "seq 1 1000", dir.path())).collect().await });
        assert_eq!(flat(&events).len(), 1000);
        let packets = events
            .iter()
            .filter(|e| matches!(e, Event::Lines(_)))
            .count();
        assert!(packets < 1000, "{packets} packets");
        assert!(events
            .iter()
            .all(|e| !matches!(e, Event::Lines(l) if l.len() > PACKET)));
    }

    #[test]
    fn a_run_streams_both_outputs_then_the_exit_code() {
        if cfg!(windows) {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let events: Vec<Event> = runtime.block_on(async {
            run(spec("t", "echo out; echo err 1>&2; exit 3", dir.path()))
                .collect()
                .await
        });
        let flat_lines = flat(&events);
        let lines: Vec<&str> = flat_lines.iter().map(String::as_str).collect();
        assert!(
            lines.contains(&"out") && lines.contains(&"err"),
            "{lines:?}"
        );
        assert_eq!(events.last(), Some(&Event::Done(Some(3))));
    }

    #[cfg(unix)]
    #[test]
    fn a_byte_that_is_not_utf8_does_not_end_the_output() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let events: Vec<Event> = runtime.block_on(async {
            run(spec("t", "printf 'a\\351b\\nafter\\n'", dir.path()))
                .collect()
                .await
        });
        let lines = flat(&events);
        assert!(lines.contains(&"a\u{FFFD}b".to_string()), "{events:?}");
        assert!(lines.contains(&"after".to_string()));
        assert_eq!(events.last(), Some(&Event::Done(Some(0))));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_stop_ends_what_the_command_started_too() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        // The shell prints the pid of a child it left in the background.
        let pid: String = runtime.block_on(async {
            let mut events = Box::pin(run(spec("t", "sleep 30 & echo $!; wait", dir.path())));
            match events.next().await {
                Some(Event::Lines(mut lines)) => lines.swap_remove(0),
                other => panic!("{other:?}"),
            }
            // The stream dropped here is the Stop button.
        });
        let gone = || {
            std::fs::read_to_string(format!("/proc/{pid}/stat"))
                // A zombie has ended too, it only waits to be reaped.
                .map_or(true, |stat| stat.contains(") Z "))
        };
        let start = std::time::Instant::now();
        while !gone() && start.elapsed() < std::time::Duration::from_secs(5) {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(gone(), "the background child {pid} still runs");
    }

    #[test]
    fn a_command_that_cannot_start_says_so() {
        if cfg!(windows) {
            return;
        }
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let events: Vec<Event> = runtime.block_on(async {
            run(spec("t", "x", Path::new("/no/such/folder")))
                .collect()
                .await
        });
        assert_eq!(events.last(), Some(&Event::Done(None)));
        assert!(matches!(events.first(), Some(Event::Line(_))));
    }
}
