//! A minimal Language Server Protocol client, for "go to definition".
//!
//! A definition is only trusted when a language server (rust-analyzer,
//! pyright, gopls, clangd, …) says where it is, as in VS Code: variables,
//! classes, interfaces, methods and imports are then resolved exactly. With
//! no server for the language, or nothing found, the answer is "none": the
//! editor never guesses.
//!
//! Servers are started on first use, one per (server, project), and kept for
//! the life of the program.

use crate::definition::Hit;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::{oneshot, Mutex};

/// A use of a name, with its line for the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    pub hit: Hit,
    pub preview: String,
}

/// What is asked of the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Definition,
    References,
}

/// Most references kept: a list longer than that is no use to read.
const MAX_REFERENCES: usize = 500;

/// What a search for a definition came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Found(Hit),
    /// The server knows of no definition here.
    NotFound,
    /// The cursor already is on the definition.
    AtDefinition,
    /// Everywhere the name is used (asked from its definition).
    References(Vec<Reference>),
    /// No server is known for this language (the file extension).
    Unsupported(String),
    /// A server exists for this language but isn't installed: the extension
    /// and the programs that would do.
    NoServer(String, String),
    /// The server failed or didn't answer.
    Failed(String),
}

/// The programs that serve a language, best first, with their arguments.
type Candidates = &'static [(&'static str, &'static [&'static str])];

/// The LSP language id and the servers for a file extension.
pub(crate) fn spec_for(ext: &str) -> Option<(&'static str, Candidates)> {
    Some(match ext {
        "rs" => ("rust", &[("rust-analyzer", &[])]),
        "py" | "pyi" => (
            "python",
            &[
                ("pyright-langserver", &["--stdio"]),
                ("basedpyright-langserver", &["--stdio"]),
                ("pylsp", &[]),
                ("jedi-language-server", &[]),
            ],
        ),
        "ts" | "mts" | "cts" => ("typescript", TYPESCRIPT),
        "tsx" => ("typescriptreact", TYPESCRIPT),
        "js" | "mjs" | "cjs" => ("javascript", TYPESCRIPT),
        "jsx" => ("javascriptreact", TYPESCRIPT),
        "go" => ("go", &[("gopls", &[])]),
        "c" | "h" => ("c", &[("clangd", &[])]),
        "cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx" => ("cpp", &[("clangd", &[])]),
        "java" => ("java", &[("jdtls", &[])]),
        "lua" => ("lua", &[("lua-language-server", &[])]),
        "sh" | "bash" => ("shellscript", &[("bash-language-server", &["start"])]),
        "php" => ("php", &[("intelephense", &["--stdio"])]),
        "rb" => ("ruby", &[("ruby-lsp", &[]), ("solargraph", &["stdio"])]),
        "zig" => ("zig", &[("zls", &[])]),
        "cs" => ("csharp", &[("csharp-ls", &[]), ("OmniSharp", &["-lsp"])]),
        "kt" | "kts" => ("kotlin", &[("kotlin-language-server", &[])]),
        "swift" => ("swift", &[("sourcekit-lsp", &[])]),
        "dart" => ("dart", &[("dart", &["language-server"])]),
        "hs" => (
            "haskell",
            &[("haskell-language-server-wrapper", &["--lsp"])],
        ),
        _ => return None,
    })
}

// ---- Completion settings, per language ------------------------------------

/// The completion settings the servers are started with (`language.option`
/// → on); a missing one is on.
static OPTIONS: OnceLock<StdMutex<std::collections::BTreeMap<String, bool>>> = OnceLock::new();

/// The settings of the Preferences, for the servers started from now on.
pub fn set_completion_options(options: &std::collections::BTreeMap<String, bool>) {
    let store = OPTIONS.get_or_init(Default::default);
    if let Ok(mut current) = store.lock() {
        *current = options.clone();
    }
}

fn option(key: &str) -> bool {
    OPTIONS
        .get()
        .and_then(|o| o.lock().ok().and_then(|o| o.get(key).copied()))
        .unwrap_or(true)
}

/// Which family a server program is of, for its settings.
fn family(exe: &Path) -> &'static str {
    let name = exe
        .file_stem()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match name.as_str() {
        "typescript-language-server" | "vtsls" => "typescript",
        "rust-analyzer" => "rust",
        "pyright-langserver" | "basedpyright-langserver" | "pylsp" | "jedi-language-server" => {
            "python"
        }
        _ => "",
    }
}

/// rust-analyzer's completion settings.
fn rust_settings(trusted: bool) -> Value {
    json!({
        // Build scripts and procedural macros are programs of the project: they
        // run only in a folder that is trusted.
        "cargo": { "buildScripts": { "enable": trusted } },
        "procMacro": { "enable": trusted },
        "completion": {
        "autoimport": { "enable": option("rust.auto_import") },
        "postfix": { "enable": option("rust.postfix") },
        "callable": { "snippets": if option("rust.call_arguments") {
            "fill_arguments"
        } else {
            "add_parentheses"
        } },
    } })
}

/// What a server asks with `workspace/configuration`, for one section.
fn configuration(family: &str, section: &str, trusted: bool) -> Value {
    let auto_import = option("python.auto_import");
    match (family, section) {
        ("rust", "rust-analyzer") => rust_settings(trusted),
        ("python", "python") => json!({ "analysis": { "autoImportCompletions": auto_import } }),
        ("python", "python.analysis") => json!({ "autoImportCompletions": auto_import }),
        _ => Value::Null,
    }
}

/// Tells the servers that already have `path` open that it was saved
/// (`didSave`), which is when rust-analyzer runs its check. Nothing is
/// started for it.
pub async fn did_save(path: PathBuf, text: String) {
    let Some(servers) = SERVERS.get() else {
        return;
    };
    let running: Vec<Arc<Server>> = servers
        .lock()
        .await
        .values()
        .filter_map(|cell| cell.get().cloned())
        .filter(|s| s.alive.load(Ordering::SeqCst))
        .collect();
    let uri = path_to_uri(&path);
    for server in running {
        if !server.docs.lock().await.contains_key(&uri) {
            continue;
        }
        // The server saved what is on disk: it must have been told that.
        if server.sync(&uri, "", &text).await.is_err() {
            continue;
        }
        let mut params = json!({ "textDocument": { "uri": uri } });
        if server.capability(&["textDocumentSync", "save", "includeText"]) == json!(true) {
            params["text"] = json!(text);
        }
        let _ = server
            .send(&json!({
                "jsonrpc": "2.0", "method": "textDocument/didSave", "params": params,
            }))
            .await;
    }
}

/// How long the servers are given to answer `shutdown` when the program
/// ends.
const SHUTDOWN_WAIT: Duration = Duration::from_millis(700);

/// Asks every server to end (`shutdown`, then `exit`), then stops what is
/// left. Waits for at most [`SHUTDOWN_WAIT`]: closing the program must not
/// wait for a server that does not answer.
pub fn stop_servers() {
    let (Some(runtime), Some(servers)) = (RUNTIME.get(), SERVERS.get()) else {
        return;
    };
    let (done, finished) = std::sync::mpsc::channel();
    runtime.spawn(async move {
        let running: Vec<Arc<Server>> = servers
            .lock()
            .await
            .values()
            .filter_map(|cell| cell.get().cloned())
            .collect();
        let ends = running.iter().map(|server| async move {
            if server.alive.load(Ordering::SeqCst) {
                let _ =
                    tokio::time::timeout(SHUTDOWN_WAIT, server.request("shutdown", Value::Null))
                        .await;
                let _ = server
                    .send(&json!({ "jsonrpc": "2.0", "method": "exit" }))
                    .await;
            }
            server.kill();
        });
        iced::futures::future::join_all(ends).await;
        let _ = done.send(());
    });
    // A little more than they are given: the rest is killed by the exit.
    let _ = finished.recv_timeout(SHUTDOWN_WAIT + Duration::from_millis(200));
}

/// Stops every server: they start again, with the settings of now, when
/// next needed.
pub async fn restart_servers() {
    forget_usable();
    let Some(servers) = SERVERS.get() else {
        return;
    };
    let mut servers = servers.lock().await;
    for server in servers.values().filter_map(|cell| cell.get()) {
        server.kill();
    }
    servers.clear();
}

/// What a server is started with: rust-analyzer's settings; for
/// TypeScript, its completion preferences and its installation (the
/// project's own `node_modules/typescript` wins, else the copy installed
/// next to the server from the Preferences).
fn init_options(exe: &Path, root: &Path) -> Value {
    match family(exe) {
        "typescript" => typescript_options(root),
        "rust" => rust_settings(crate::trust::is_trusted(root)),
        _ => json!({}),
    }
}

fn typescript_options(root: &Path) -> Value {
    let project = root.join("node_modules/typescript/lib/tsserver.js");
    let managed = crate::lsp_install::managed_dir()
        .map(|d| d.join("lib/node_modules/typescript/lib/tsserver.js"))
        .filter(|p| p.is_file());
    // Completion as complete as the editor of TypeScript's own makes it:
    // names of other modules (imported on accept), snippets, `?.`.
    let preferences = json!({
        "includeCompletionsForModuleExports": option("typescript.auto_import"),
        "includeCompletionsForImportStatements": option("typescript.auto_import"),
        "includeCompletionsWithInsertText": true,
        "includeCompletionsWithSnippetText": true,
        "includeAutomaticOptionalChainCompletions": true,
        "includeCompletionsWithClassMemberSnippets": true,
        "includeCompletionsWithObjectLiteralMethodSnippets": true,
        "useLabelDetailsInCompletionEntries": true,
    });
    match managed {
        Some(path) if !project.is_file() => {
            json!({ "tsserver": { "path": path }, "preferences": preferences })
        }
        _ => json!({ "preferences": preferences }),
    }
}

const TYPESCRIPT: Candidates = &[
    ("typescript-language-server", &["--stdio"]),
    ("vtsls", &["--stdio"]),
];

/// Folders where language servers are often installed but which a program
/// started from a launcher may not have in its `PATH`.
fn extra_bin_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = dirs::home_dir() {
        for sub in [".cargo/bin", ".local/bin", "go/bin", ".npm-global/bin"] {
            dirs.push(home.join(sub));
        }
    }
    for fixed in ["/opt/homebrew/bin", "/usr/local/bin", "/snap/bin"] {
        dirs.push(PathBuf::from(fixed));
    }
    dirs
}

pub(crate) fn find_executable(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let suffixes: &[&str] = if cfg!(windows) {
        &["", ".exe", ".cmd", ".bat"]
    } else {
        &[""]
    };
    crate::lsp_install::managed_bin_dir()
        .into_iter()
        .chain(std::env::split_paths(&path))
        .chain(extra_bin_dirs())
        .flat_map(|dir| suffixes.iter().map(move |s| dir.join(format!("{name}{s}"))))
        .find(|candidate| candidate.is_file())
}

/// Whether a server program can really be run: `rustup` leaves a
/// `rust-analyzer` proxy that fails until the component is added.
pub(crate) fn usable(name: &str) -> bool {
    let Some(path) = find_executable(name) else {
        return false;
    };
    name != "rust-analyzer" || runs(&path)
}

/// What [`runs`] found, by program: running it again at each completion,
/// hover or lint (on the UI thread for the last) costs a process each time.
static RUNS: OnceLock<StdMutex<HashMap<PathBuf, bool>>> = OnceLock::new();

/// Whether `path --version` succeeds, asked once until [`forget_usable`].
fn runs(path: &Path) -> bool {
    let store = RUNS.get_or_init(Default::default);
    if let Some(known) = store.lock().ok().and_then(|m| m.get(path).copied()) {
        return known;
    }
    let ok = std::process::Command::new(path)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if let Ok(mut known) = store.lock() {
        known.insert(path.to_path_buf(), ok);
    }
    ok
}

/// Asks [`usable`] again from scratch: after a server is installed or
/// removed, or the servers are restarted.
pub(crate) fn forget_usable() {
    if let Some(Ok(mut known)) = RUNS.get().map(|m| m.lock()) {
        known.clear();
    }
}

/// Longest wait for an answer, indexing included.
const TOTAL_TIMEOUT: Duration = Duration::from_secs(60);
/// A fresh server may answer "nothing" while it still loads the project.
const SETTLE: Duration = if cfg!(test) {
    Duration::from_millis(300)
} else {
    Duration::from_secs(5)
};
const RETRY_EVERY: Duration = Duration::from_millis(500);

/// Where the definition of the name at (`line`, byte `col`) of `text`, the
/// content of `path`, is. `language` is the file extension.
pub async fn definition(
    language: &str,
    path: &Path,
    text: &str,
    line: usize,
    col: usize,
) -> Outcome {
    ask(Kind::Definition, language, path, text, line, col).await
}

/// Where the name at (`line`, `col`), which is a definition, is used.
pub async fn references(
    language: &str,
    path: &Path,
    text: &str,
    line: usize,
    col: usize,
) -> Outcome {
    ask(Kind::References, language, path, text, line, col).await
}

async fn ask(
    kind: Kind,
    language: &str,
    path: &Path,
    text: &str,
    line: usize,
    col: usize,
) -> Outcome {
    let ext = language.to_ascii_lowercase();
    let Some((language_id, candidates)) = spec_for(&ext) else {
        return Outcome::Unsupported(ext);
    };
    let Some((exe, args)) = candidates.iter().find_map(|(name, args)| {
        usable(name)
            .then(|| find_executable(name))
            .flatten()
            .map(|exe| (exe, *args))
    }) else {
        let names: Vec<_> = candidates.iter().map(|c| c.0).collect();
        return Outcome::NoServer(ext, names.join(" / "));
    };
    definition_with(kind, &exe, args, language_id, path, text, line, col).await
}

#[allow(clippy::too_many_arguments)]
async fn definition_with(
    kind: Kind,
    exe: &Path,
    args: &[&str],
    language_id: &str,
    path: &Path,
    text: &str,
    line: usize,
    col: usize,
) -> Outcome {
    let query = query(kind, exe, args, language_id, path, text, line, col);
    match tokio::time::timeout(TOTAL_TIMEOUT, query).await {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(error)) => Outcome::Failed(error),
        Err(_) => Outcome::Failed("timeout".to_string()),
    }
}

#[allow(clippy::too_many_arguments)]
async fn query(
    kind: Kind,
    exe: &Path,
    args: &[&str],
    language_id: &str,
    path: &Path,
    text: &str,
    line: usize,
    col: usize,
) -> Result<Outcome, String> {
    let root = crate::definition::project_root(path)
        .or_else(|| path.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("/"));
    let server = server_for(exe, args, &root).await.map_err(|e| {
        let name = exe
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        format!("{name}: {e}")
    })?;
    let uri = path_to_uri(path);
    server.sync(&uri, language_id, text).await?;
    let mut params = json!({
        "textDocument": { "uri": uri },
        "position": { "line": line, "character": utf16_col(text, line, col) },
    });
    let method = match kind {
        Kind::Definition => "textDocument/definition",
        Kind::References => {
            params["context"] = json!({ "includeDeclaration": false });
            "textDocument/references"
        }
    };
    loop {
        match server.request(method, params.clone()).await {
            // Still working on it: ask again.
            Err(e) if e.code == CONTENT_MODIFIED || e.code == SERVER_CANCELLED => {}
            Err(e) => return Err(e.message),
            Ok(value) => {
                let found = targets(&value);
                if kind == Kind::References && !found.is_empty() {
                    return Ok(Outcome::References(references_of(&found, path, text)));
                }
                if let Some(target) = found
                    .into_iter()
                    .next()
                    .filter(|_| kind == Kind::Definition)
                {
                    return Ok(resolve(&target, path, text, line, col));
                }
                let settling = server.is_busy() || server.started.elapsed() < SETTLE;
                if !settling {
                    return Ok(Outcome::NotFound);
                }
            }
        }
        tokio::time::sleep(RETRY_EVERY).await;
    }
}

const CONTENT_MODIFIED: i64 = -32801;
const SERVER_CANCELLED: i64 = -32802;

/// A place a server answered with: LSP positions (UTF-16 columns).
#[derive(Debug, PartialEq, Eq)]
struct Target {
    uri: String,
    line: usize,
    character: usize,
    end_line: usize,
    end_character: usize,
}

/// The places of an answer: a `Location`, a list of them, or `LocationLink`s.
fn targets(value: &Value) -> Vec<Target> {
    let items = match value {
        Value::Array(items) => items.iter().collect(),
        Value::Object(_) => vec![value],
        _ => Vec::new(),
    };
    let number = |v: &Value, key: &str| v.get(key).and_then(Value::as_u64).map(|n| n as usize);
    items
        .into_iter()
        .filter_map(|item| {
            let (uri, range) = match item.get("targetUri") {
                Some(uri) => (
                    uri,
                    item.get("targetSelectionRange")
                        .or_else(|| item.get("targetRange"))?,
                ),
                None => (item.get("uri")?, item.get("range")?),
            };
            let (start, end) = (range.get("start")?, range.get("end")?);
            Some(Target {
                uri: uri.as_str()?.to_string(),
                line: number(start, "line")?,
                character: number(start, "character")?,
                end_line: number(end, "line")?,
                end_character: number(end, "character")?,
            })
        })
        .collect()
}

/// Turns a server's place into a [`Hit`] (byte columns), or says that the
/// cursor already is there.
fn resolve(target: &Target, path: &Path, text: &str, line: usize, col: usize) -> Outcome {
    let Some(target_path) = uri_to_path(&target.uri) else {
        return Outcome::Failed(format!("unsupported location: {}", target.uri));
    };
    let on_disk;
    let target_text = if target_path == path {
        text
    } else {
        on_disk = std::fs::read_to_string(&target_path).unwrap_or_default();
        &on_disk
    };
    let hit = hit_of(target, target_path, target_text);
    if hit.path == path && hit.line == line && (hit.col..=hit.col + hit.len).contains(&col) {
        Outcome::AtDefinition
    } else {
        Outcome::Found(hit)
    }
}

/// The uses a server listed, in file and line order, with their lines.
fn references_of(found: &[Target], path: &Path, text: &str) -> Vec<Reference> {
    let mut texts: HashMap<PathBuf, String> = HashMap::new();
    let mut list: Vec<Reference> = found
        .iter()
        .filter_map(|target| {
            let target_path = uri_to_path(&target.uri)?;
            let target_text = if target_path == path {
                text
            } else {
                texts
                    .entry(target_path.clone())
                    .or_insert_with(|| std::fs::read_to_string(&target_path).unwrap_or_default())
            };
            let preview: String = target_text
                .lines()
                .nth(target.line)
                .unwrap_or("")
                .trim()
                .chars()
                .take(160)
                .collect();
            Some(Reference {
                hit: hit_of(target, target_path, target_text),
                preview,
            })
        })
        .collect();
    list.sort_by(|a, b| {
        (&a.hit.path, a.hit.line, a.hit.col).cmp(&(&b.hit.path, b.hit.line, b.hit.col))
    });
    list.dedup();
    list.truncate(MAX_REFERENCES);
    list
}

/// A server's place as a [`Hit`] (byte columns) in `target_text`.
fn hit_of(target: &Target, target_path: PathBuf, target_text: &str) -> Hit {
    let line_text = |n: usize| target_text.lines().nth(n).unwrap_or("");
    let start = byte_col(line_text(target.line), target.character);
    let len = if target.end_line == target.line {
        byte_col(line_text(target.line), target.end_character).saturating_sub(start)
    } else {
        0
    };
    Hit {
        path: target_path,
        line: target.line,
        col: start,
        len,
    }
}

/// The UTF-16 column of byte `col` of line `line`.
fn utf16_col(text: &str, line: usize, col: usize) -> usize {
    text.lines()
        .nth(line)
        .and_then(|l| l.get(..col))
        .map_or(col, |prefix| prefix.encode_utf16().count())
}

/// The byte offset of UTF-16 column `utf16` in `line`.
fn byte_col(line: &str, utf16: usize) -> usize {
    let mut units = 0;
    for (i, c) in line.char_indices() {
        if units >= utf16 {
            return i;
        }
        units += c.len_utf16();
    }
    line.len()
}

fn path_to_uri(path: &Path) -> String {
    let mut uri = String::from("file://");
    for byte in path.to_string_lossy().bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                uri.push(byte as char)
            }
            _ => uri.push_str(&format!("%{byte:02X}")),
        }
    }
    uri
}

fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let mut bytes = Vec::with_capacity(rest.len());
    let raw = rest.as_bytes();
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'%' {
            let hex = rest.get(i + 1..i + 3)?;
            bytes.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            bytes.push(raw[i]);
            i += 1;
        }
    }
    Some(PathBuf::from(String::from_utf8(bytes).ok()?))
}

fn same_uri(a: &str, b: &str) -> bool {
    uri_key(a) == uri_key(b)
}

/// What identifies a document in what a server says: its decoded path. The
/// same file has several spellings as a URI (`+`, `(` or `@` escaped or
/// not), and servers do not encode them as `path_to_uri` does.
fn uri_key(uri: &str) -> String {
    uri_to_path(uri).map_or_else(|| uri.to_string(), |p| p.to_string_lossy().into_owned())
}

// ---- Problems and fixes ---------------------------------------------------

/// The main program that serves a file extension (to tell servers apart).
pub fn server_of(ext: &str) -> Option<&'static str> {
    spec_for(&ext.to_ascii_lowercase()).and_then(|(_, c)| c.first().map(|c| c.0))
}

/// Whether a server for the language of `ext` is installed.
pub fn installed(ext: &str) -> bool {
    spec_for(&ext.to_ascii_lowercase())
        .is_some_and(|(_, candidates)| candidates.iter().any(|(name, _)| usable(name)))
}

/// The running server for a file, with the language id and its name.
async fn connect(ext: &str, path: &Path) -> Result<(Arc<Server>, &'static str, String), String> {
    let ext = ext.to_ascii_lowercase();
    let (language_id, candidates) =
        spec_for(&ext).ok_or_else(|| format!("no language server for .{ext}"))?;
    let (exe, args) = candidates
        .iter()
        .find_map(|(name, args)| {
            usable(name)
                .then(|| find_executable(name))
                .flatten()
                .map(|exe| (exe, *args))
        })
        .ok_or_else(|| {
            let names: Vec<_> = candidates.iter().map(|c| c.0).collect();
            format!("no language server installed ({})", names.join(" / "))
        })?;
    let root = crate::definition::project_root(path)
        .or_else(|| path.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("/"));
    let name = exe
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let server = server_for(&exe, args, &root)
        .await
        .map_err(|e| format!("{name}: {e}"))?;
    Ok((server, language_id, name))
}

/// How long to wait for a server to publish its problems.
const DIAGNOSTICS_WAIT: Duration = if cfg!(test) {
    Duration::from_secs(3)
} else {
    Duration::from_secs(20)
};

/// The problems the language server of `ext` finds in `text`, the content of
/// `path`.
pub async fn diagnostics(
    ext: &str,
    path: &Path,
    text: &str,
) -> Result<Vec<crate::lint::Diagnostic>, String> {
    let (server, language_id, name) = connect(ext, path).await?;
    let uri = path_to_uri(path);
    let seen = server.published_count(&uri);
    let changed = server.sync(&uri, language_id, text).await?;
    let started = Instant::now();
    loop {
        let (count, _) = server.published_of(&uri);
        let fresh = if changed { count > seen } else { count > 0 };
        if fresh {
            // The server may publish again right after (syntax, then types):
            // the answer is what it said last, once it has been quiet.
            let mut last = count;
            let quiet_since = Instant::now();
            let mut quiet = quiet_since;
            while quiet.elapsed() < Duration::from_millis(700)
                && quiet_since.elapsed() < Duration::from_secs(8)
            {
                tokio::time::sleep(Duration::from_millis(100)).await;
                let now = server.published_count(&uri);
                if now != last {
                    last = now;
                    quiet = Instant::now();
                }
            }
            let (_, list) = server.published_of(&uri);
            return Ok(list
                .iter()
                .filter_map(|d| lint_of(d, text, &name))
                .collect());
        }
        if started.elapsed() > DIAGNOSTICS_WAIT {
            return Ok(Vec::new());
        }
        if !server.alive.load(Ordering::SeqCst) {
            return Err("language server stopped".to_string());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

impl Server {
    fn published_of(&self, uri: &str) -> (u64, Vec<Value>) {
        self.published
            .lock()
            .ok()
            .and_then(|p| p.get(&uri_key(uri)).cloned())
            .unwrap_or_default()
    }

    fn published_count(&self, uri: &str) -> u64 {
        self.published_of(uri).0
    }
}

/// A problem as a server said it, in our terms (byte columns).
fn lint_of(raw: &Value, text: &str, server: &str) -> Option<crate::lint::Diagnostic> {
    use crate::lint::{Diagnostic, Severity};
    let range = raw.get("range")?;
    let pos = |side: &str| {
        let p = range.get(side)?;
        let line = p.get("line")?.as_u64()? as usize;
        let character = p.get("character")?.as_u64()? as usize;
        let line_text = text.split('\n').nth(line).unwrap_or("");
        Some((line, byte_col(line_text, character)))
    };
    let (start, end) = (pos("start")?, pos("end")?);
    let severity = match raw.get("severity").and_then(Value::as_u64) {
        Some(1) | None => Severity::Error,
        Some(2) => Severity::Warning,
        Some(3) => Severity::Info,
        _ => Severity::Hint,
    };
    let code = raw.get("code").map(|c| match c {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    });
    let source = raw
        .get("source")
        .and_then(Value::as_str)
        .map_or_else(|| server.to_string(), str::to_string);
    // TypeScript's problems are known by their number.
    let code = match (source.as_str(), code) {
        ("ts" | "typescript", Some(c)) => Some(format!("TS{c}")),
        (_, c) => c,
    };
    Some(Diagnostic {
        line: start.0,
        col: start.1,
        end_line: end.0,
        end_col: end.1.max(if end.0 == start.0 && end.1 == start.1 {
            end.1
        } else {
            0
        }),
        severity,
        message: raw.get("message")?.as_str()?.trim().to_string(),
        code,
        source: if matches!(source.as_str(), "ts" | "typescript") {
            "TypeScript".to_string()
        } else {
            source
        },
        fixes: Vec::new(),
        raw: Some(raw.clone()),
    })
}

/// The edits of a workspace edit that are for `uri`, in byte columns.
fn edits_for(edit: &Value, uri: &str, text: &str) -> Vec<crate::lint::Edit> {
    let mut lists: Vec<&Value> = Vec::new();
    if let Some(changes) = edit.get("changes").and_then(Value::as_object) {
        lists.extend(
            changes
                .iter()
                .filter(|(u, _)| same_uri(u, uri))
                .map(|(_, l)| l),
        );
    }
    for change in edit
        .get("documentChanges")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if change["textDocument"]["uri"]
            .as_str()
            .is_some_and(|u| same_uri(u, uri))
        {
            if let Some(list) = change.get("edits") {
                lists.push(list);
            }
        }
    }
    lists
        .into_iter()
        .filter_map(Value::as_array)
        .flatten()
        .filter_map(|e| {
            let range = e.get("range")?;
            let at = |side: &str| {
                let p = range.get(side)?;
                let line = p.get("line")?.as_u64()? as usize;
                let character = p.get("character")?.as_u64()? as usize;
                let line_text = text.split('\n').nth(line).unwrap_or("");
                Some((line, byte_col(line_text, character)))
            };
            let (from, to) = (at("start")?, at("end")?);
            Some(crate::lint::Edit {
                line: from.0,
                col: from.1,
                end_line: to.0,
                end_col: to.1,
                text: e.get("newText")?.as_str()?.to_string(),
            })
        })
        .collect()
}

/// The code actions of a request, as fixes: those that edit this file.
async fn fixes_of(
    server: &Server,
    uri: &str,
    text: &str,
    params: Value,
) -> Result<Vec<crate::lint::Fix>, String> {
    let actions = server
        .request("textDocument/codeAction", params)
        .await
        .map_err(|e| e.message)?;
    let mut fixes = Vec::new();
    for action in actions.as_array().into_iter().flatten() {
        let mut action = action.clone();
        // Some servers send the edit only when asked for it.
        if action.get("edit").is_none() && action.get("data").is_some() {
            if let Ok(resolved) = server.request("codeAction/resolve", action.clone()).await {
                action = resolved;
            }
        }
        let Some(edit) = action.get("edit") else {
            continue;
        };
        let edits = edits_for(edit, uri, text);
        if edits.is_empty() {
            continue;
        }
        fixes.push(crate::lint::Fix {
            title: action["title"].as_str().unwrap_or("Fix").to_string(),
            edits,
            safe: true,
        });
    }
    Ok(fixes)
}

/// The fixes the language server offers for one problem.
pub async fn code_actions(
    ext: &str,
    path: &Path,
    text: &str,
    problem: &crate::lint::Diagnostic,
) -> Result<Vec<crate::lint::Fix>, String> {
    let (server, language_id, _) = connect(ext, path).await?;
    let uri = path_to_uri(path);
    server.sync(&uri, language_id, text).await?;
    let range =
        |line: usize, col: usize| json!({ "line": line, "character": utf16_col(text, line, col) });
    let params = json!({
        "textDocument": { "uri": uri },
        "range": { "start": range(problem.line, problem.col), "end": range(problem.end_line, problem.end_col) },
        "context": {
            "diagnostics": problem.raw.iter().collect::<Vec<_>>(),
            "only": ["quickfix"],
        },
    });
    fixes_of(&server, &uri, text, params).await
}

// ---- Rename, hover, formatting -----------------------------------------------

/// What a rename changes in one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEdits {
    pub path: PathBuf,
    pub edits: Vec<crate::lint::Edit>,
}

/// Text edits (`TextEdit[]`) of a file, as edits in bytes.
fn text_edit_list(list: &[Value], text: &str) -> Vec<crate::lint::Edit> {
    list.iter()
        .filter_map(|e| {
            let range = e.get("range")?;
            let at = |side: &str| {
                let p = range.get(side)?;
                let line = p.get("line")?.as_u64()? as usize;
                let character = p.get("character")?.as_u64()? as usize;
                let line_text = text.split('\n').nth(line).unwrap_or("");
                Some((line, byte_col(line_text, character)))
            };
            let (from, to) = (at("start")?, at("end")?);
            Some(crate::lint::Edit {
                line: from.0,
                col: from.1,
                end_line: to.0,
                end_col: to.1,
                text: e.get("newText")?.as_str()?.to_string(),
            })
        })
        .collect()
}

/// The files a `WorkspaceEdit` changes, with their edits. The text of
/// `uri` is `text` (what the server was told), the others are read from
/// disk. Creating, renaming or deleting files is not supported.
fn workspace_files(edit: &Value, uri: &str, text: &str) -> Result<Vec<FileEdits>, String> {
    let mut by_uri: Vec<(String, Vec<Value>)> = Vec::new();
    if let Some(changes) = edit.get("changes").and_then(Value::as_object) {
        for (u, list) in changes {
            by_uri.push((u.clone(), list.as_array().cloned().unwrap_or_default()));
        }
    }
    for change in edit
        .get("documentChanges")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if change.get("kind").is_some() {
            return Err("the language server also asks to create, rename or delete files".into());
        }
        if let (Some(u), Some(list)) = (
            change["textDocument"]["uri"].as_str(),
            change.get("edits").and_then(Value::as_array),
        ) {
            by_uri.push((u.to_string(), list.clone()));
        }
    }
    let mut files = Vec::new();
    for (u, list) in by_uri {
        let Some(path) = uri_to_path(&u) else {
            continue;
        };
        let file_text = if same_uri(&u, uri) {
            text.to_string()
        } else {
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?
        };
        let edits = text_edit_list(&list, &file_text);
        if !edits.is_empty() {
            files.push(FileEdits { path, edits });
        }
    }
    Ok(files)
}

/// A request retried while the server says its answer would be stale.
async fn request_retrying(server: &Server, method: &str, params: Value) -> Result<Value, String> {
    let mut tries = 0;
    loop {
        match server.request(method, params.clone()).await {
            Ok(value) => return Ok(value),
            Err(e) if (e.code == CONTENT_MODIFIED || e.code == SERVER_CANCELLED) && tries < 5 => {
                tries += 1;
                tokio::time::sleep(Duration::from_millis(60)).await;
            }
            Err(e) => return Err(e.message),
        }
    }
}

fn position_params(uri: &str, text: &str, line: usize, col: usize) -> Value {
    json!({
        "textDocument": { "uri": uri },
        "position": { "line": line, "character": utf16_col(text, line, col) },
    })
}

/// Renames the name at (`line`, byte `col`) of `text`, the content of
/// `path`, to `new_name` everywhere: the edits of every file.
pub async fn rename(
    ext: &str,
    path: &Path,
    text: &str,
    line: usize,
    col: usize,
    new_name: &str,
) -> Result<Vec<FileEdits>, String> {
    let (server, language_id, _) = connect(ext, path).await?;
    rename_on(&server, language_id, path, text, line, col, new_name).await
}

async fn rename_on(
    server: &Server,
    language_id: &str,
    path: &Path,
    text: &str,
    line: usize,
    col: usize,
    new_name: &str,
) -> Result<Vec<FileEdits>, String> {
    let provider = server.capability(&["renameProvider"]);
    if provider.is_null() || provider == json!(false) {
        return Err("this language server cannot rename".into());
    }
    let uri = path_to_uri(path);
    server.close_others(&uri).await?;
    server.sync(&uri, language_id, text).await?;
    let mut params = position_params(&uri, text, line, col);
    params["newName"] = json!(new_name);
    let answer = request_retrying(server, "textDocument/rename", params).await?;
    if answer.is_null() {
        return Err("there is no name to rename here".into());
    }
    workspace_files(&answer, &uri, text)
}

/// The text of a hover answer (`contents`: a string, a `MarkedString`, a
/// `MarkupContent` or a list of those), without the code fences.
fn hover_text(answer: &Value) -> String {
    fn piece(v: &Value) -> String {
        match v {
            Value::String(s) => s.clone(),
            Value::Object(o) => o
                .get("value")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            Value::Array(list) => list.iter().map(piece).collect::<Vec<_>>().join("\n\n"),
            _ => String::new(),
        }
    }
    piece(&answer["contents"])
        .lines()
        .filter(|l| !l.trim_start().starts_with("```"))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

/// What the language server says about the name at (`line`, byte `col`):
/// its type and its documentation. `None` when it says nothing.
pub async fn hover(
    ext: &str,
    path: &Path,
    text: &str,
    line: usize,
    col: usize,
) -> Result<Option<String>, String> {
    let (server, language_id, _) = connect(ext, path).await?;
    hover_on(&server, language_id, path, text, line, col).await
}

async fn hover_on(
    server: &Server,
    language_id: &str,
    path: &Path,
    text: &str,
    line: usize,
    col: usize,
) -> Result<Option<String>, String> {
    let provider = server.capability(&["hoverProvider"]);
    if provider.is_null() || provider == json!(false) {
        return Ok(None);
    }
    let uri = path_to_uri(path);
    server.sync(&uri, language_id, text).await?;
    let answer = request_retrying(
        server,
        "textDocument/hover",
        position_params(&uri, text, line, col),
    )
    .await?;
    let shown = hover_text(&answer);
    Ok((!shown.is_empty()).then_some(shown))
}

/// `text`, the content of `path`, laid out by the language server.
pub async fn format(
    ext: &str,
    path: &Path,
    text: &str,
    spaces: bool,
    width: usize,
) -> Result<String, String> {
    let (server, language_id, _) = connect(ext, path).await?;
    format_on(&server, language_id, path, text, spaces, width).await
}

async fn format_on(
    server: &Server,
    language_id: &str,
    path: &Path,
    text: &str,
    spaces: bool,
    width: usize,
) -> Result<String, String> {
    let provider = server.capability(&["documentFormattingProvider"]);
    if provider.is_null() || provider == json!(false) {
        return Err("this language server cannot format".into());
    }
    let uri = path_to_uri(path);
    server.sync(&uri, language_id, text).await?;
    let params = json!({
        "textDocument": { "uri": uri },
        "options": {
            "tabSize": width,
            "insertSpaces": spaces,
            "trimTrailingWhitespace": true,
            "insertFinalNewline": true,
        },
    });
    let answer = request_retrying(server, "textDocument/formatting", params).await?;
    let Some(list) = answer.as_array() else {
        return Ok(text.to_string());
    };
    let fix = crate::lint::Fix {
        title: String::new(),
        edits: text_edit_list(list, text),
        safe: true,
    };
    Ok(crate::lint::apply(text, &[&fix]).0)
}

// ---- Completion -----------------------------------------------------------

/// What a server proposes at a place of a file.
#[derive(Debug, Clone, Default)]
pub struct Completions {
    pub items: Vec<crate::completion::Item>,
    /// Not everything: to be asked again as one types.
    pub incomplete: bool,
    /// The server is still loading the project: the list may lack items.
    pub busy: bool,
}

/// What a server tells about its completion.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CompletionInfo {
    pub supported: bool,
    /// The characters that ask for it (`.`, `::`, …) besides a name.
    pub triggers: Vec<String>,
    pub busy: bool,
    /// The server tells the parameters of a call; the characters that open
    /// that help (`(`, `,`) and those that update it once open.
    pub signatures: bool,
    pub signature_triggers: Vec<String>,
    pub signature_retriggers: Vec<String>,
}

/// Why completion is asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Trigger {
    /// A name typed, or the shortcut.
    Invoked,
    /// One of the server's characters typed.
    Character(String),
    /// The server said its last list was not everything.
    Incomplete,
}

impl Server {
    fn completion_info(&self) -> CompletionInfo {
        let provider = self.capability(&["completionProvider"]);
        let signature = self.capability(&["signatureHelpProvider"]);
        let strings = |v: &Value| -> Vec<String> {
            v.as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        };
        CompletionInfo {
            signatures: !signature.is_null(),
            signature_triggers: strings(&signature["triggerCharacters"]),
            signature_retriggers: strings(&signature["retriggerCharacters"]),
            supported: !provider.is_null(),
            triggers: provider["triggerCharacters"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect(),
            busy: self.loading(),
        }
    }

    /// Still loading the project: reporting progress, or just started (the
    /// first report may not have come yet).
    fn loading(&self) -> bool {
        self.is_busy() || self.started.elapsed() < SETTLE
    }
}

/// Starts the server of the file's language and tells it about the file, so
/// that it loads the project in the background before completion is asked.
pub async fn prepare_completion(
    ext: &str,
    path: &Path,
    text: &str,
) -> Result<CompletionInfo, String> {
    let (server, language_id, _) = connect(ext, path).await?;
    server.sync(&path_to_uri(path), language_id, text).await?;
    Ok(server.completion_info())
}

/// Whether the server of the file's language is still loading the project.
pub async fn completion_busy(ext: &str, path: &Path) -> bool {
    match connect(ext, path).await {
        Ok((server, ..)) => server.loading(),
        Err(_) => false,
    }
}

/// What the server proposes at (`line`, byte `col`) of `text`, the content of
/// `path`.
pub async fn complete(
    ext: &str,
    path: &Path,
    text: &str,
    line: usize,
    col: usize,
    trigger: Trigger,
) -> Result<Completions, String> {
    let (server, language_id, _) = connect(ext, path).await?;
    complete_on(&server, language_id, path, text, line, col, trigger).await
}

#[allow(clippy::too_many_arguments)]
async fn complete_on(
    server: &Server,
    language_id: &str,
    path: &Path,
    text: &str,
    line: usize,
    col: usize,
    trigger: Trigger,
) -> Result<Completions, String> {
    if !server.completion_info().supported {
        return Ok(Completions::default());
    }
    let uri = path_to_uri(path);
    server.sync(&uri, language_id, text).await?;
    let context = match &trigger {
        Trigger::Invoked => json!({ "triggerKind": 1 }),
        Trigger::Character(c) => json!({ "triggerKind": 2, "triggerCharacter": c }),
        Trigger::Incomplete => json!({ "triggerKind": 3 }),
    };
    let params = json!({
        "textDocument": { "uri": uri },
        "position": { "line": line, "character": utf16_col(text, line, col) },
        "context": context,
    });
    let mut tries = 0;
    loop {
        match server
            .request_completion("textDocument/completion", params.clone())
            .await
        {
            Ok(value) => {
                let (items, incomplete) = crate::completion::parse_list(&value, text);
                return Ok(Completions {
                    items,
                    incomplete,
                    busy: server.loading(),
                });
            }
            // The server was busy with the text: ask again, a few times.
            Err(e) if (e.code == CONTENT_MODIFIED || e.code == SERVER_CANCELLED) && tries < 5 => {
                tries += 1;
                tokio::time::sleep(Duration::from_millis(60)).await;
            }
            Err(e) => return Err(e.message),
        }
    }
}

/// Why the parameters of a call are asked.
#[derive(Debug, Clone, PartialEq)]
pub struct SignatureTrigger {
    /// 1: asked (shortcut, an item accepted), 2: a character typed, 3: the
    /// text or the cursor moved while the help is open.
    pub kind: u8,
    pub character: Option<String>,
    /// The help is open: the answer it shows, sent back to the server.
    pub open: Option<Value>,
}

/// The parameters of the call at (`line`, byte `col`) of `text`, as the
/// server answers `textDocument/signatureHelp` (`None`: not in a call).
pub async fn signature_help(
    ext: &str,
    path: &Path,
    text: &str,
    line: usize,
    col: usize,
    trigger: SignatureTrigger,
) -> Result<Option<Value>, String> {
    let (server, language_id, _) = connect(ext, path).await?;
    signature_help_on(&server, language_id, path, text, line, col, trigger).await
}

#[allow(clippy::too_many_arguments)]
async fn signature_help_on(
    server: &Server,
    language_id: &str,
    path: &Path,
    text: &str,
    line: usize,
    col: usize,
    trigger: SignatureTrigger,
) -> Result<Option<Value>, String> {
    if !server.completion_info().signatures {
        return Ok(None);
    }
    let uri = path_to_uri(path);
    server.sync(&uri, language_id, text).await?;
    let mut context = json!({
        "triggerKind": trigger.kind,
        "isRetrigger": trigger.open.is_some(),
    });
    if let Some(character) = trigger.character {
        context["triggerCharacter"] = json!(character);
    }
    if let Some(open) = trigger.open {
        context["activeSignatureHelp"] = open;
    }
    let params = json!({
        "textDocument": { "uri": uri },
        "position": { "line": line, "character": utf16_col(text, line, col) },
        "context": context,
    });
    let mut tries = 0;
    loop {
        match server
            .request("textDocument/signatureHelp", params.clone())
            .await
        {
            Ok(Value::Null) => return Ok(None),
            Ok(value) => return Ok(Some(value)),
            Err(e) if (e.code == CONTENT_MODIFIED || e.code == SERVER_CANCELLED) && tries < 5 => {
                tries += 1;
                tokio::time::sleep(Duration::from_millis(60)).await;
            }
            Err(e) => return Err(e.message),
        }
    }
}

// ---- Symbols of the projects (search everywhere) ---------------------------

/// A name a server knows in a project: a class, a function, a field…
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolHit {
    pub name: String,
    /// LSP `SymbolKind` (5 class, 10 enum, 11 interface, 23 struct, …).
    pub kind: u64,
    pub container: Option<String>,
    pub path: PathBuf,
    /// From 0; the column in bytes.
    pub line: usize,
    pub col: usize,
}

/// What the servers found, and whether one is still loading its project.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Symbols {
    pub hits: Vec<SymbolHit>,
    pub busy: bool,
}

/// Classes, interfaces, structs and enums: what the "Classes" tab lists.
pub fn is_class_kind(kind: u64) -> bool {
    matches!(kind, 5 | 10 | 11 | 23)
}

/// The names of an answer to `workspace/symbol`, with byte columns (the
/// files read once each).
fn symbols_of(value: &Value, texts: &mut HashMap<PathBuf, String>) -> Vec<SymbolHit> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| {
            let location = s.get("location")?;
            let path = uri_to_path(location.get("uri")?.as_str()?)?;
            let start = location.get("range").and_then(|r| r.get("start"));
            let line = start
                .and_then(|p| p.get("line"))
                .and_then(Value::as_u64)
                .unwrap_or(0) as usize;
            let character = start
                .and_then(|p| p.get("character"))
                .and_then(Value::as_u64)
                .unwrap_or(0) as usize;
            let text = texts
                .entry(path.clone())
                .or_insert_with(|| std::fs::read_to_string(&path).unwrap_or_default());
            let col = byte_col(text.lines().nth(line).unwrap_or(""), character);
            Some(SymbolHit {
                name: s.get("name")?.as_str()?.to_string(),
                kind: s.get("kind").and_then(Value::as_u64).unwrap_or(0),
                container: s
                    .get("containerName")
                    .and_then(Value::as_str)
                    .filter(|c| !c.is_empty())
                    .map(str::to_string),
                path,
                line,
                col,
            })
        })
        .collect()
}

/// The names matching `query` in the projects, from their servers: each is
/// given as (language, a file of the project in that language). `all` asks
/// for every kind of name (else the servers may give only types);
/// `dependencies` looks in the libraries too.
pub async fn workspace_symbols(
    servers: Vec<(String, PathBuf)>,
    query: String,
    all: bool,
    dependencies: bool,
) -> Symbols {
    let mut found = Symbols::default();
    let mut texts = HashMap::new();
    for (ext, file) in servers {
        let Ok((server, language_id, _)) = connect(&ext, &file).await else {
            continue;
        };
        // TypeScript needs a file of the project open to know it.
        if let Ok(text) = std::fs::read_to_string(&file) {
            let _ = server.sync(&path_to_uri(&file), language_id, &text).await;
        }
        found.busy |= server.loading();
        // rust-analyzer reads `#` as "every kind", `*` as "libraries too".
        let mut asked = query.clone();
        if server.family == "rust" {
            if all {
                asked.push('#');
            }
            if dependencies {
                asked.push('*');
            }
        }
        let answer = tokio::time::timeout(
            Duration::from_secs(8),
            server.request("workspace/symbol", json!({ "query": asked })),
        )
        .await;
        if let Ok(Ok(value)) = answer {
            found.hits.extend(symbols_of(&value, &mut texts));
        }
    }
    found
        .hits
        .sort_by(|a, b| (&a.name, &a.path, a.line).cmp(&(&b.name, &b.path, b.line)));
    found
        .hits
        .dedup_by(|a, b| a.name == b.name && a.path == b.path && a.line == b.line);
    found
}

/// The rest of an item (documentation, detail, the import it needs), as the
/// server sends it on demand; `raw` is the item as it came.
pub async fn resolve_completion(ext: &str, path: &Path, raw: Value) -> Result<Value, String> {
    let (server, ..) = connect(ext, path).await?;
    resolve_on(&server, raw).await
}

async fn resolve_on(server: &Server, raw: Value) -> Result<Value, String> {
    if server.capability(&["completionProvider", "resolveProvider"]) != Value::Bool(true) {
        return Ok(raw);
    }
    server
        .request("completionItem/resolve", raw)
        .await
        .map_err(|e| e.message)
}

// ---- The connection to one server -----------------------------------------

#[derive(Debug)]
struct RpcError {
    code: i64,
    message: String,
}

type Pending = StdMutex<HashMap<i64, oneshot::Sender<Result<Value, RpcError>>>>;

struct Server {
    stdin: Mutex<ChildStdin>,
    pending: Pending,
    next_id: AtomicI64,
    alive: AtomicBool,
    started: Instant,
    /// Tokens of the progress reports (indexing, …) that are running.
    progress: StdMutex<HashSet<String>>,
    /// Documents the server was told about: uri → (version, text).
    docs: Mutex<HashMap<String, (i64, String)>>,
    /// What the server last published as problems, per document: how many
    /// times it did, and the problems as it said them.
    published: StdMutex<HashMap<String, (u64, Vec<Value>)>>,
    /// What the server said it can do (its `initialize` answer).
    capabilities: StdMutex<Value>,
    /// The completion request still running, cancelled by the next one.
    completing: AtomicI64,
    /// Which family of server it is, for its settings.
    family: &'static str,
    /// The folder was trusted when the server started.
    trusted: bool,
    /// Kept so the process dies with the program.
    _child: StdMutex<Child>,
}

/// The server of each program and project, once it has started.
type Servers = Mutex<HashMap<(PathBuf, PathBuf), Arc<tokio::sync::OnceCell<Arc<Server>>>>>;

static SERVERS: OnceLock<Servers> = OnceLock::new();

/// The runtime the servers run on, to reach them when the program ends.
static RUNTIME: OnceLock<tokio::runtime::Handle> = OnceLock::new();

/// How long an answer is waited for: a server that never gives one must not
/// leave what asked (Format, a rename, a completion…) waiting for ever.
const ANSWER_WAIT: Duration = Duration::from_secs(30);
/// Starting takes longer: some servers read the whole project first.
const START_WAIT: Duration = Duration::from_secs(120);

/// The running server for this program and project, started if need be.
/// The table is only held to find the server's place: a slow start keeps
/// waiting only those who want that server, not the other languages.
async fn server_for(exe: &Path, args: &[&str], root: &Path) -> Result<Arc<Server>, String> {
    let cell = {
        let mut servers = SERVERS.get_or_init(Default::default).lock().await;
        let key = (exe.to_path_buf(), root.to_path_buf());
        let cell = servers.entry(key).or_default();
        // One that died is started again.
        if cell.get().is_some_and(|s| !s.alive.load(Ordering::SeqCst)) {
            *cell = Default::default();
        }
        cell.clone()
    };
    cell.get_or_try_init(|| Server::start(exe, args, root))
        .await
        .cloned()
}

/// Stops a server whose start did not go to the end (it failed, or whoever
/// waited for it gave up): nothing would ever stop it otherwise, since the
/// task reading its answers keeps it.
struct StartGuard(Option<Arc<Server>>);

impl Drop for StartGuard {
    fn drop(&mut self) {
        if let Some(server) = self.0.take() {
            server.kill();
        }
    }
}

impl Server {
    async fn start(exe: &Path, args: &[&str], root: &Path) -> Result<Arc<Server>, String> {
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            let _ = RUNTIME.set(handle);
        }
        let mut child = Command::new(exe)
            .args(args)
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("{}: {e}", exe.display()))?;
        let stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let server = Arc::new(Server {
            stdin: Mutex::new(stdin),
            pending: Default::default(),
            next_id: AtomicI64::new(1),
            alive: AtomicBool::new(true),
            started: Instant::now(),
            progress: Default::default(),
            docs: Default::default(),
            published: Default::default(),
            capabilities: StdMutex::new(Value::Null),
            completing: AtomicI64::new(0),
            family: family(exe),
            trusted: crate::trust::is_trusted(root),
            _child: StdMutex::new(child),
        });
        tokio::spawn(read_loop(server.clone(), stdout));
        let mut guard = StartGuard(Some(server.clone()));

        let root_uri = path_to_uri(root);
        let name = root.file_name().map_or_else(
            || "project".to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        let params = json!({
            "processId": std::process::id(),
            "clientInfo": { "name": "SimpleEdit" },
            "initializationOptions": init_options(exe, root),
            "rootUri": root_uri,
            "workspaceFolders": [{ "uri": root_uri, "name": name }],
            "capabilities": {
                "general": { "positionEncodings": ["utf-16"] },
                "window": { "workDoneProgress": true },
                "workspace": { "configuration": true, "workspaceFolders": true },
                "textDocument": {
                    "synchronization": { "didSave": false },
                    "signatureHelp": {
                        "contextSupport": true,
                        "signatureInformation": {
                            "documentationFormat": ["markdown", "plaintext"],
                            "parameterInformation": { "labelOffsetSupport": true },
                            "activeParameterSupport": true,
                        },
                    },
                    "completion": {
                        "contextSupport": true,
                        "completionItem": {
                            "snippetSupport": true,
                            "commitCharactersSupport": false,
                            "documentationFormat": ["markdown", "plaintext"],
                            "deprecatedSupport": true,
                            "preselectSupport": true,
                            "insertReplaceSupport": true,
                            "labelDetailsSupport": true,
                            "tagSupport": { "valueSet": [1] },
                            "resolveSupport": { "properties": [
                                "documentation", "detail", "additionalTextEdits"
                            ] },
                            "insertTextModeSupport": { "valueSet": [1, 2] },
                        },
                        "completionItemKind": { "valueSet": (1..=25).collect::<Vec<u8>>() },
                        "completionList": { "itemDefaults": [
                            "commitCharacters", "editRange", "insertTextFormat",
                            "insertTextMode", "data"
                        ] },
                    },
                    "definition": { "linkSupport": true },
                    "rename": { "prepareSupport": false },
                    "hover": { "contentFormat": ["markdown", "plaintext"] },
                    "formatting": {},
                    "publishDiagnostics": { "versionSupport": true },
                    "codeAction": {
                        "codeActionLiteralSupport": { "codeActionKind": { "valueSet": [
                            "quickfix", "source", "source.fixAll"
                        ] } },
                        "resolveSupport": { "properties": ["edit"] },
                    },
                },
            },
        });
        let answer = server
            .request("initialize", params)
            .await
            .map_err(|e| e.message)?;
        if let Ok(mut capabilities) = server.capabilities.lock() {
            *capabilities = answer.get("capabilities").cloned().unwrap_or(Value::Null);
        }
        server
            .send(&json!({
                "jsonrpc": "2.0", "method": "initialized", "params": {}
            }))
            .await?;
        guard.0 = None;
        Ok(server)
    }

    /// The process is stopped, and the server no longer used.
    fn kill(&self) {
        self.alive.store(false, Ordering::SeqCst);
        if let Ok(mut child) = self._child.lock() {
            let _ = child.start_kill();
        }
    }

    /// The answer to request `id`, or an error once `wait` has passed (the
    /// server is then told to give up the request).
    async fn answer(
        &self,
        id: i64,
        rx: oneshot::Receiver<Result<Value, RpcError>>,
        wait: Duration,
    ) -> Result<Value, RpcError> {
        match tokio::time::timeout(wait, rx).await {
            Ok(answer) => answer.unwrap_or_else(|_| Err(stopped())),
            Err(_) => {
                if let Ok(mut pending) = self.pending.lock() {
                    pending.remove(&id);
                }
                let _ = self
                    .send(&json!({
                        "jsonrpc": "2.0", "method": "$/cancelRequest", "params": { "id": id }
                    }))
                    .await;
                Err(RpcError {
                    code: -1,
                    message: t!("lsp.no_answer").to_string(),
                })
            }
        }
    }

    fn is_busy(&self) -> bool {
        !self.progress.lock().map_or(true, |p| p.is_empty())
    }

    async fn send(&self, message: &Value) -> Result<(), String> {
        let body = message.to_string();
        let mut stdin = self.stdin.lock().await;
        let framed = format!("Content-Length: {}\r\n\r\n{body}", body.len());
        stdin
            .write_all(framed.as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        stdin.flush().await.map_err(|e| e.to_string())
    }

    async fn request(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        if !self.alive.load(Ordering::SeqCst) {
            return Err(stopped());
        }
        if let Ok(mut pending) = self.pending.lock() {
            pending.insert(id, tx);
        }
        let message = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        if let Err(message) = self.send(&message).await {
            return Err(RpcError { code: -1, message });
        }
        let wait = if method == "initialize" {
            START_WAIT
        } else {
            ANSWER_WAIT
        };
        self.answer(id, rx, wait).await
    }

    /// A completion request; the one still running before it is cancelled:
    /// its answer would be for a text that is no more.
    async fn request_completion(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let previous = self.completing.swap(id, Ordering::SeqCst);
        let running = self.pending.lock().is_ok_and(|p| p.contains_key(&previous));
        if previous != 0 && running {
            let _ = self
                .send(&json!({
                    "jsonrpc": "2.0", "method": "$/cancelRequest", "params": { "id": previous }
                }))
                .await;
        }
        let (tx, rx) = oneshot::channel();
        if !self.alive.load(Ordering::SeqCst) {
            return Err(stopped());
        }
        if let Ok(mut pending) = self.pending.lock() {
            pending.insert(id, tx);
        }
        let message = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        if let Err(message) = self.send(&message).await {
            return Err(RpcError { code: -1, message });
        }
        self.answer(id, rx, ANSWER_WAIT).await
    }

    /// How the server wants changes: 2 for the changed part only, else the
    /// whole text.
    fn sync_kind(&self) -> u64 {
        let capabilities = self
            .capabilities
            .lock()
            .map(|c| c.clone())
            .unwrap_or_default();
        match &capabilities["textDocumentSync"] {
            Value::Number(n) => n.as_u64().unwrap_or(1),
            Value::Object(o) => o.get("change").and_then(Value::as_u64).unwrap_or(1),
            _ => 1,
        }
    }

    fn capability(&self, path: &[&str]) -> Value {
        let capabilities = self
            .capabilities
            .lock()
            .map(|c| c.clone())
            .unwrap_or_default();
        path.iter().fold(&capabilities, |v, key| &v[*key]).clone()
    }

    /// Tells the server the current text of a document.
    async fn sync(&self, uri: &str, language_id: &str, text: &str) -> Result<bool, String> {
        let mut docs = self.docs.lock().await;
        let message = match docs.get_mut(uri) {
            None => {
                docs.insert(uri.to_string(), (1, text.to_string()));
                json!({
                    "jsonrpc": "2.0", "method": "textDocument/didOpen",
                    "params": { "textDocument": {
                        "uri": uri, "languageId": language_id, "version": 1, "text": text,
                    } },
                })
            }
            Some((_, known)) if known == text => return Ok(false),
            Some((version, known)) => {
                *version += 1;
                // Only what changed, if the server takes that: a keystroke
                // in a long file sends a character, not the file.
                let change = if self.sync_kind() == 2 {
                    text_change(known, text)
                } else {
                    json!({ "text": text })
                };
                *known = text.to_string();
                json!({
                    "jsonrpc": "2.0", "method": "textDocument/didChange",
                    "params": {
                        "textDocument": { "uri": uri, "version": *version },
                        "contentChanges": [change],
                    },
                })
            }
        };
        self.send(&message).await?;
        Ok(true)
    }

    /// Closes every document but `keep` (`didClose`): the server then reads
    /// those from the disk again, instead of from the text it was told long
    /// ago, which is what a multi-file edit (a rename) is computed on.
    async fn close_others(&self, keep: &str) -> Result<(), String> {
        let mut docs = self.docs.lock().await;
        let stale: Vec<String> = docs
            .keys()
            .filter(|u| u.as_str() != keep)
            .cloned()
            .collect();
        for uri in stale {
            docs.remove(&uri);
            self.send(&json!({
                "jsonrpc": "2.0", "method": "textDocument/didClose",
                "params": { "textDocument": { "uri": uri } },
            }))
            .await?;
        }
        Ok(())
    }

    /// Reacts to a message of the server.
    async fn handle(&self, message: Value) {
        let id = message.get("id").cloned();
        let method = message.get("method").and_then(Value::as_str);
        match (method, id) {
            // The answer to one of our requests.
            (None, Some(id)) => {
                let Some(id) = id.as_i64() else { return };
                let sender = self.pending.lock().ok().and_then(|mut p| p.remove(&id));
                let Some(sender) = sender else { return };
                let answer = match message.get("error") {
                    Some(error) => Err(RpcError {
                        code: error.get("code").and_then(Value::as_i64).unwrap_or(0),
                        message: error
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("error")
                            .to_string(),
                    }),
                    None => Ok(message.get("result").cloned().unwrap_or(Value::Null)),
                };
                let _ = sender.send(answer);
            }
            // The server asks us something: nothing to tell it.
            (Some(method), Some(id)) => {
                let result = if method == "workspace/configuration" {
                    let items = message["params"]["items"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default();
                    Value::Array(
                        items
                            .iter()
                            .map(|i| {
                                configuration(
                                    self.family,
                                    i["section"].as_str().unwrap_or(""),
                                    self.trusted,
                                )
                            })
                            .collect(),
                    )
                } else {
                    Value::Null
                };
                let _ = self
                    .send(&json!({ "jsonrpc": "2.0", "id": id, "result": result }))
                    .await;
            }
            (Some("textDocument/publishDiagnostics"), None) => {
                let params = &message["params"];
                let Some(uri) = params["uri"].as_str() else {
                    return;
                };
                // Problems computed for a text older than ours are stale.
                if let Some(v) = params["version"].as_i64() {
                    if let Some((current, _)) = self.docs.lock().await.get(uri) {
                        if v < *current {
                            return;
                        }
                    }
                }
                let list = params["diagnostics"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                if let Ok(mut published) = self.published.lock() {
                    let entry = published.entry(uri_key(uri)).or_default();
                    entry.0 += 1;
                    entry.1 = list;
                }
            }
            (Some("$/progress"), None) => {
                let token = message["params"]["token"].to_string();
                if let Ok(mut progress) = self.progress.lock() {
                    match message["params"]["value"]["kind"].as_str() {
                        Some("begin") => {
                            progress.insert(token);
                        }
                        Some("end") => {
                            progress.remove(&token);
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
}

/// The one change that turns `old` into `new`: the stretch between what they
/// start and end with in common, as LSP positions of `old`, and its new text.
fn text_change(old: &str, new: &str) -> Value {
    let mut prefix = old
        .bytes()
        .zip(new.bytes())
        .take_while(|(a, b)| a == b)
        .count();
    while !old.is_char_boundary(prefix) {
        prefix -= 1;
    }
    let most = old.len().min(new.len()) - prefix;
    let mut suffix = old
        .bytes()
        .rev()
        .zip(new.bytes().rev())
        .take(most)
        .take_while(|(a, b)| a == b)
        .count();
    while !old.is_char_boundary(old.len() - suffix) || !new.is_char_boundary(new.len() - suffix) {
        suffix -= 1;
    }
    let position = |at: usize| {
        let before = &old[..at];
        let line = before.matches('\n').count();
        let start = before.rfind('\n').map_or(0, |i| i + 1);
        json!({ "line": line, "character": before[start..].encode_utf16().count() })
    };
    json!({
        "range": { "start": position(prefix), "end": position(old.len() - suffix) },
        "text": &new[prefix..new.len() - suffix],
    })
}

fn stopped() -> RpcError {
    RpcError {
        code: -1,
        message: "language server stopped".to_string(),
    }
}

async fn read_loop(server: Arc<Server>, stdout: ChildStdout) {
    let mut reader = BufReader::new(stdout);
    while let Some(message) = read_message(&mut reader).await {
        // The answer to a request of the server waits for the stdin lock: apart,
        // so that reading goes on (the server may be blocked writing to us
        // while a large message is being written to it).
        if message.get("method").is_some() && message.get("id").is_some() {
            let server = server.clone();
            tokio::spawn(async move { server.handle(message).await });
        } else {
            server.handle(message).await;
        }
    }
    server.alive.store(false, Ordering::SeqCst);
    // Whoever waits for an answer won't get one.
    if let Ok(mut pending) = server.pending.lock() {
        pending.clear();
    }
}

/// The next framed JSON message (`Content-Length` header, then the body).
async fn read_message<R: AsyncBufRead + Unpin>(reader: &mut R) -> Option<Value> {
    let mut length = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).await.ok()? == 0 {
            return None;
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some(value) = line.strip_prefix("Content-Length:") {
            length = value.trim().parse::<usize>().ok();
        }
    }
    let mut body = vec![0; length?];
    reader.read_exact(&mut body).await.ok()?;
    // A body that isn't JSON is skipped, not fatal.
    Some(serde_json::from_slice(&body).unwrap_or(Value::Null))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uris_round_trip() {
        let path = Path::new("/tmp/mon dossier/é#1.rs");
        let uri = path_to_uri(path);
        assert_eq!(uri, "file:///tmp/mon%20dossier/%C3%A9%231.rs");
        assert_eq!(uri_to_path(&uri).as_deref(), Some(path));
        assert_eq!(uri_to_path("http://x"), None);
    }

    #[cfg(unix)]
    #[test]
    fn spellings_of_a_uri_are_the_same_document() {
        // `+` and `(` are written plainly by some servers, escaped by us.
        let path = Path::new("/home/u/c++/Projet (copie)/x.rs");
        let ours = path_to_uri(path);
        let theirs = "file:///home/u/c++/Projet%20(copie)/x.rs";
        assert_ne!(ours, theirs);
        assert!(same_uri(&ours, theirs));
        let edit = json!({ "changes": { theirs: [{
            "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 1 } },
            "newText": "y" }] } });
        assert_eq!(edits_for(&edit, &ours, "x").len(), 1);
    }

    #[test]
    fn a_program_is_run_once_to_know_whether_it_works_until_forgotten() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let count = dir.path().join("count");
        let program = dir.path().join("fake-server");
        std::fs::write(
            &program,
            format!("#!/bin/sh\necho run >> '{}'\n", count.display()),
        )
        .unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        let runs_so_far = || {
            std::fs::read_to_string(&count)
                .unwrap_or_default()
                .lines()
                .count()
        };
        assert!(runs(&program));
        assert!(runs(&program));
        assert_eq!(runs_so_far(), 1, "the second answer comes from the cache");
        forget_usable();
        assert!(runs(&program));
        assert_eq!(runs_so_far(), 2, "forgotten, it is asked again");
    }

    #[test]
    fn columns_convert_between_utf16_and_bytes() {
        // '😀' is 4 bytes, 2 UTF-16 units; 'é' 2 bytes, 1 unit.
        let line = "😀é x";
        assert_eq!(byte_col(line, 0), 0);
        assert_eq!(byte_col(line, 2), 4);
        assert_eq!(byte_col(line, 3), 6);
        assert_eq!(byte_col(line, 99), line.len());
        assert_eq!(utf16_col("ab\n😀é x", 1, 6), 3);
        assert_eq!(utf16_col("ab", 5, 1), 1);
    }

    #[test]
    fn every_shape_of_answer_is_read() {
        let range =
            json!({ "start": { "line": 3, "character": 4 }, "end": { "line": 3, "character": 9 } });
        let location = json!({ "uri": "file:///a.rs", "range": range });
        let link = json!({
            "targetUri": "file:///b.rs",
            "targetRange": { "start": { "line": 0, "character": 0 }, "end": { "line": 9, "character": 0 } },
            "targetSelectionRange": range,
        });
        let expected = |uri: &str| Target {
            uri: uri.to_string(),
            line: 3,
            character: 4,
            end_line: 3,
            end_character: 9,
        };
        assert_eq!(targets(&location), vec![expected("file:///a.rs")]);
        assert_eq!(targets(&json!([link])), vec![expected("file:///b.rs")]);
        assert_eq!(targets(&json!([location.clone(), location])).len(), 2);
        assert!(targets(&Value::Null).is_empty());
        assert!(targets(&json!([])).is_empty());
    }

    #[test]
    fn a_target_becomes_a_hit_or_the_cursor_place() {
        let dir = tempfile::tempdir().unwrap();
        let here = dir.path().join("a.rs");
        let there = dir.path().join("b.rs");
        std::fs::write(&there, "// x\nfn été() {}\n").unwrap();
        let target = |path: &Path, line, character, end| Target {
            uri: path_to_uri(path),
            line,
            character,
            end_line: line,
            end_character: end,
        };
        let text = "été();";
        let hit = resolve(&target(&there, 1, 3, 6), &here, text, 0, 0);
        assert_eq!(
            hit,
            Outcome::Found(Hit {
                path: there,
                line: 1,
                col: 3,
                len: 5
            })
        );
        // The definition is the place being clicked.
        let at = resolve(&target(&here, 0, 0, 3), &here, text, 0, 2);
        assert_eq!(at, Outcome::AtDefinition);
    }

    #[test]
    fn languages_without_a_server_are_unsupported() {
        assert!(spec_for("rs").is_some());
        assert!(spec_for("tsx").is_some());
        assert!(spec_for("txt").is_none());
        let outcome = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(definition("txt", Path::new("/x.txt"), "a", 0, 0));
        assert_eq!(outcome, Outcome::Unsupported("txt".into()));
    }

    /// A scripted server speaking the protocol, in Python.
    const FAKE_SERVER: &str = r#"
import sys, json
def read():
    n = 0
    while True:
        h = sys.stdin.buffer.readline()
        if not h: return None
        h = h.strip()
        if not h: break
        if h.lower().startswith(b"content-length:"): n = int(h.split(b":")[1])
    return json.loads(sys.stdin.buffer.read(n))
def send(m):
    b = json.dumps(m).encode()
    sys.stdout.buffer.write(b"Content-Length: %d\r\n\r\n" % len(b) + b)
    sys.stdout.buffer.flush()
opened = {}
tries = 0
while True:
    m = read()
    if m is None: break
    meth = m.get("method")
    if meth == "initialize":
        send({"jsonrpc": "2.0", "id": m["id"], "result": {"capabilities": {}}})
    elif meth == "textDocument/didOpen":
        d = m["params"]["textDocument"]; opened[d["uri"]] = d["text"]
    elif meth == "textDocument/didChange":
        opened[m["params"]["textDocument"]["uri"]] = m["params"]["contentChanges"][0]["text"]
    elif meth == "textDocument/references":
        uri = m["params"]["textDocument"]["uri"]
        r = lambda l: {"uri": uri, "range": {"start": {"line": l, "character": 4}, "end": {"line": l, "character": 10}}}
        res = [r(1), r(0)] if m["params"]["context"]["includeDeclaration"] is False else None
        send({"jsonrpc": "2.0", "id": m["id"], "result": res})
    elif meth == "textDocument/definition":
        p = m["params"]; uri = p["textDocument"]["uri"]
        lines = opened[uri].split("\n")
        tries += 1
        if tries == 1:
            send({"jsonrpc": "2.0", "id": m["id"], "error": {"code": -32801, "message": "busy"}})
        elif "def target" in lines[0] and p["position"]["line"] > 0:
            send({"jsonrpc": "2.0", "id": m["id"], "result": [{"uri": uri, "range": {
                "start": {"line": 0, "character": 4}, "end": {"line": 0, "character": 10}}}]})
        else:
            send({"jsonrpc": "2.0", "id": m["id"], "result": None})
"#;

    /// The interpreter that runs the fake server (`python3` isn't a command on Windows).
    const PYTHON: &str = if cfg!(windows) { "python" } else { "python3" };

    #[cfg(target_os = "linux")]
    #[test]
    fn a_server_that_fails_to_start_does_not_stay_behind() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("pid");
        // It refuses `initialize`, then would wait for ever.
        let script = FAKE_SERVER
            .replace(
                "import sys, json",
                &format!("import sys, json, os\nopen({:?}, 'w').write(str(os.getpid()))", pid_file),
            )
            .replace(
                r#"send({"jsonrpc": "2.0", "id": m["id"], "result": {"capabilities": {}}})"#,
                r#"send({"jsonrpc": "2.0", "id": m["id"], "error": {"code": -32603, "message": "no"}})"#,
            );
        let path = dir.path().join("fake.py");
        std::fs::write(&path, script).unwrap();
        let path = path.to_string_lossy().into_owned();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let started = runtime.block_on(server_for(Path::new(PYTHON), &[path.as_str()], dir.path()));
        assert!(started.is_err());
        let pid = std::fs::read_to_string(&pid_file).unwrap();
        // Gone, or a zombie waiting to be reaped: it no longer runs.
        let running = || {
            std::fs::read_to_string(format!("/proc/{pid}/status"))
                .unwrap_or_default()
                .lines()
                .any(|l| l.starts_with("State:") && !l.contains('Z'))
        };
        for _ in 0..50 {
            if !running() {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!("the server process {pid} still runs");
    }

    #[test]
    fn a_server_answers_a_definition_request() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "").unwrap();
        let script = dir.path().join("fake.py");
        std::fs::write(&script, FAKE_SERVER).unwrap();
        let file = dir.path().join("m.py");
        let text = "def target():\n    target()\n";
        let script = script.to_string_lossy().into_owned();
        let args = [script.as_str()];
        // One runtime: the server's reader task lives in it.
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let run = |text: &str, line, col| {
            runtime.block_on(definition_with(
                Kind::Definition,
                Path::new(PYTHON),
                &args,
                "python",
                &file,
                text,
                line,
                col,
            ))
        };
        // The first request is retried after "busy"; the click is on the call.
        assert_eq!(
            run(text, 1, 5),
            Outcome::Found(Hit {
                path: file.clone(),
                line: 0,
                col: 4,
                len: 6
            })
        );
        // On the definition, the server lists the uses (sorted here).
        let refs = runtime.block_on(definition_with(
            Kind::References,
            Path::new(PYTHON),
            &args,
            "python",
            &file,
            text,
            0,
            5,
        ));
        let Outcome::References(list) = refs else {
            panic!("{refs:?}");
        };
        let lines: Vec<_> = list
            .iter()
            .map(|r| (r.hit.line, r.preview.as_str()))
            .collect();
        assert_eq!(lines, vec![(0, "def target():"), (1, "target()")]);
        // Another click on a place the server doesn't know: no answer, not a guess.
        assert_eq!(run("x = 1\n", 0, 0), Outcome::NotFound);
    }

    /// Against a real rust-analyzer: `cargo test -- --ignored real_rust`.
    #[test]
    #[ignore]
    fn real_rust_analyzer_resolves_types_and_variables() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/app.rs");
        let text = std::fs::read_to_string(&path).unwrap();
        let find = |needle: &str, nth: usize| {
            let (line, l) = text
                .lines()
                .enumerate()
                .filter(|(_, l)| l.contains(needle))
                .nth(nth)
                .unwrap();
            (line, l.find(needle).unwrap())
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let go = |line, col| runtime.block_on(definition("rs", &path, &text, line, col));

        // A type from another file.
        let (line, col) = find("hit: crate::definition::Hit", 0);
        let Outcome::Found(hit) = go(line, col + "hit: crate::definition::".len() + 1) else {
            panic!("no type definition");
        };
        assert!(hit.path.ends_with("src/definition.rs"), "{hit:?}");
        // A local variable, used on a later line.
        let (decl, _) = find("let len = hit.len;", 0);
        let (line, col) = find("hit.col, len);", 0);
        let Outcome::Found(hit) = go(line, col + "hit.col, ".len() + 1) else {
            panic!("no variable definition");
        };
        assert_eq!(
            (hit.path.as_path(), hit.line),
            (path.as_path(), decl),
            "{hit:?}"
        );
        // On a definition: where it is used.
        let (line, col) = find("fn goto_line_id()", 0);
        let Outcome::References(list) =
            runtime.block_on(references("rs", &path, &text, line, col + 5))
        else {
            panic!("no references");
        };
        assert!(list.len() >= 2, "{list:?}");
        assert!(
            list.iter().all(|r| r.preview.contains("goto_line_id")),
            "{list:?}"
        );
        // A string literal is no name.
        let (line, col) = find("\"status.no_definition\"", 0);
        assert_eq!(go(line, col + 3), Outcome::NotFound);
    }

    #[test]
    fn each_server_gets_the_completion_settings_of_its_language() {
        let mut options = std::collections::BTreeMap::new();
        options.insert("rust.postfix".to_string(), false);
        options.insert("python.auto_import".to_string(), false);
        options.insert("typescript.auto_import".to_string(), false);
        set_completion_options(&options);
        let rust = init_options(Path::new("/x/rust-analyzer"), Path::new("/p"));
        assert_eq!(rust["completion"]["postfix"]["enable"], false);
        assert_eq!(
            rust["completion"]["autoimport"]["enable"], true,
            "missing: on"
        );
        assert_eq!(rust["completion"]["callable"]["snippets"], "fill_arguments");
        let ts = init_options(Path::new("/x/typescript-language-server"), Path::new("/p"));
        assert_eq!(
            ts["preferences"]["includeCompletionsForModuleExports"],
            false
        );
        assert_eq!(
            configuration("python", "python.analysis", false)["autoImportCompletions"],
            false
        );
        assert_eq!(configuration("rust", "rust-analyzer", false), rust);
        assert_eq!(configuration("", "anything", false), Value::Null);
        assert_eq!(
            init_options(Path::new("/x/gopls"), Path::new("/p")),
            json!({})
        );
        set_completion_options(&std::collections::BTreeMap::new());
    }

    #[test]
    fn workspace_symbols_are_read_in_both_shapes() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.rs");
        std::fs::write(&file, "x\n  é struct User {}\n").unwrap();
        let uri = path_to_uri(&file);
        let answer = json!([
            { "name": "User", "kind": 23, "containerName": "",
              "location": { "uri": uri, "range": { "start": { "line": 1, "character": 11 }, "end": { "line": 1, "character": 15 } } } },
            { "name": "lazy", "kind": 12, "location": { "uri": uri } },
            { "name": "bad" }
        ]);
        let mut texts = HashMap::new();
        let hits = symbols_of(&answer, &mut texts);
        assert_eq!(hits.len(), 2);
        assert_eq!((hits[0].line, hits[0].col), (1, 12), "after the two-byte é");
        assert_eq!(hits[0].container, None);
        assert_eq!((hits[1].line, hits[1].col), (0, 0));
        assert!(is_class_kind(23) && is_class_kind(5) && !is_class_kind(12));
    }

    #[test]
    fn a_change_is_sent_as_the_part_that_changed() {
        let change = text_change("ab\ncd\n", "ab\ncXd\n");
        assert_eq!(
            change["range"]["start"],
            json!({ "line": 1, "character": 1 })
        );
        assert_eq!(change["range"]["end"], json!({ "line": 1, "character": 1 }));
        assert_eq!(change["text"], "X");
        // A deletion across lines, after a character of two UTF-16 units.
        let change = text_change("😀ab\ncd", "😀d");
        assert_eq!(
            change["range"]["start"],
            json!({ "line": 0, "character": 2 })
        );
        assert_eq!(change["range"]["end"], json!({ "line": 1, "character": 1 }));
        assert_eq!(change["text"], "");
        // Multi-byte characters that share their first bytes stay whole.
        let change = text_change("é", "è");
        assert_eq!(change["range"]["end"], json!({ "line": 0, "character": 1 }));
        assert_eq!(change["text"], "è");
        // Repeated letters: the change is within the common prefix/suffix.
        let change = text_change("aaa", "aa");
        assert_eq!(change["text"], "");
    }

    /// A server that applies changes as they come (only the changed part)
    /// and proposes what it sees left of the cursor: what it proposes shows
    /// whether its copy of the text is right.
    const FAKE_COMPLETION_SERVER: &str = r#"
import json, sys
def read():
    n = 0
    while True:
        h = sys.stdin.buffer.readline()
        if not h: return None
        h = h.strip()
        if not h: break
        if h.lower().startswith(b"content-length:"): n = int(h.split(b":")[1])
    return json.loads(sys.stdin.buffer.read(n))
def send(m):
    b = json.dumps(m).encode()
    sys.stdout.buffer.write(b"Content-Length: %d\r\n\r\n" % len(b) + b)
    sys.stdout.buffer.flush()
def offset(text, pos):
    lines = text.split("\n")
    before = sum(len(l) + 1 for l in lines[:pos["line"]])
    return before + pos["character"]
docs = {}
while True:
    m = read()
    if m is None: break
    meth = m.get("method")
    if meth == "initialize":
        send({"jsonrpc": "2.0", "id": m["id"], "result": {"capabilities": {
            "textDocumentSync": {"openClose": True, "change": 2},
            "completionProvider": {"triggerCharacters": [".", "::"], "resolveProvider": True}}}})
    elif meth == "textDocument/didOpen":
        d = m["params"]["textDocument"]; docs[d["uri"]] = d["text"]
    elif meth == "textDocument/didChange":
        uri = m["params"]["textDocument"]["uri"]
        for c in m["params"]["contentChanges"]:
            t = docs[uri]
            if "range" in c:
                a, b = offset(t, c["range"]["start"]), offset(t, c["range"]["end"])
                docs[uri] = t[:a] + c["text"] + t[b:]
            else:
                docs[uri] = c["text"]
    elif meth == "textDocument/completion":
        p = m["params"]; t = docs[p["textDocument"]["uri"]]
        line = t.split("\n")[p["position"]["line"]][:p["position"]["character"]]
        kind = p["context"]["triggerKind"]
        send({"jsonrpc": "2.0", "id": m["id"], "result": {"isIncomplete": True, "items": [
            {"label": "seen:" + line, "kind": 6, "data": {"n": 1}},
            {"label": "kind:%d" % kind, "kind": 14}]}})
    elif meth == "completionItem/resolve":
        item = m["params"]; item["documentation"] = {"kind": "markdown", "value": "resolved"}
        send({"jsonrpc": "2.0", "id": m["id"], "result": item})
"#;

    #[test]
    fn completion_sees_the_text_as_it_is_after_every_change() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("fake.py");
        std::fs::write(&script, FAKE_COMPLETION_SERVER).unwrap();
        let file = dir.path().join("m.txt");
        let script = script.to_string_lossy().into_owned();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let server = server_for(Path::new(PYTHON), &[script.as_str()], dir.path())
                .await
                .unwrap();
            let info = server.completion_info();
            assert!(info.supported);
            assert_eq!(info.triggers, vec![".", "::"]);
            let ask = |text: &'static str, line, col, trigger| {
                let server = server.clone();
                let file = file.clone();
                async move {
                    complete_on(&server, "plaintext", &file, text, line, col, trigger)
                        .await
                        .unwrap()
                }
            };
            let first = ask("ab\ncd", 1, 2, Trigger::Invoked).await;
            assert!(first.incomplete);
            assert_eq!(first.items[0].label, "seen:cd");
            assert_eq!(first.items[1].label, "kind:1");
            // Typed in the middle, then at the end, accents included.
            let next = ask("ab\ncéd.", 1, 5, Trigger::Character(".".into())).await;
            assert_eq!(next.items[0].label, "seen:céd.");
            assert_eq!(next.items[1].label, "kind:2");
            let next = ask("ab\ncéd.x", 1, 6, Trigger::Incomplete).await;
            assert_eq!(next.items[0].label, "seen:céd.x");
            assert_eq!(next.items[1].label, "kind:3");
            // Lines joined.
            let next = ask("abcéd.x", 0, 8, Trigger::Invoked).await;
            assert_eq!(next.items[0].label, "seen:abcéd.x");
            // The rest of an item, on demand.
            let resolved = resolve_on(&server, next.items[0].raw.clone())
                .await
                .unwrap();
            assert_eq!(resolved["documentation"]["value"], "resolved");
        });
    }

    /// A server that renames, hovers and formats.
    const FAKE_EDIT_SERVER: &str = r#"
import json, sys
def read():
    n = 0
    while True:
        h = sys.stdin.buffer.readline()
        if not h: return None
        h = h.strip()
        if not h: break
        if h.lower().startswith(b"content-length:"): n = int(h.split(b":")[1])
    return json.loads(sys.stdin.buffer.read(n))
def send(m):
    b = json.dumps(m).encode()
    sys.stdout.buffer.write(b"Content-Length: %d\r\n\r\n" % len(b) + b)
    sys.stdout.buffer.flush()
docs = {}
while True:
    m = read()
    if m is None: break
    meth = m.get("method")
    if meth == "initialize":
        send({"jsonrpc": "2.0", "id": m["id"], "result": {"capabilities": {
            "renameProvider": True, "hoverProvider": True, "documentFormattingProvider": True}}})
    elif meth == "textDocument/didOpen":
        d = m["params"]["textDocument"]; docs[d["uri"]] = d["text"]
    elif meth == "textDocument/didChange":
        docs[m["params"]["textDocument"]["uri"]] = m["params"]["contentChanges"][0]["text"]
    elif meth == "textDocument/rename":
        p = m["params"]; uri = p["textDocument"]["uri"]
        other = uri.replace("m.txt", "o.txt")
        edit = lambda l, a, b: {"range": {"start": {"line": l, "character": a}, "end": {"line": l, "character": b}}, "newText": p["newName"]}
        if p["position"]["line"] == 9:
            send({"jsonrpc": "2.0", "id": m["id"], "result": None})
        else:
            send({"jsonrpc": "2.0", "id": m["id"], "result": {"changes": {
                uri: [edit(0, 4, 7), edit(1, 0, 3)], other: [edit(0, 0, 3)]}}})
    elif meth == "textDocument/hover":
        send({"jsonrpc": "2.0", "id": m["id"], "result": {"contents": {"kind": "markdown",
            "value": "```rust\nfn foo()\n```\n\nDoes foo."}}})
    elif meth == "textDocument/formatting":
        uri = m["params"]["textDocument"]["uri"]
        lines = docs[uri].split("\n")
        send({"jsonrpc": "2.0", "id": m["id"], "result": [
            {"range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": len(lines[0])}},
             "newText": lines[0].strip()}]})
"#;

    #[test]
    fn a_server_renames_hovers_and_formats() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("fake.py");
        std::fs::write(&script, FAKE_EDIT_SERVER).unwrap();
        let file = dir.path().join("m.txt");
        let other = dir.path().join("o.txt");
        std::fs::write(&other, "foo = 1\n").unwrap();
        let script = script.to_string_lossy().into_owned();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let server = server_for(Path::new(PYTHON), &[script.as_str()], dir.path())
                .await
                .unwrap();
            let text = "let foo = 1\nfoo + 1\n";
            let files = rename_on(&server, "plaintext", &file, text, 0, 4, "bar")
                .await
                .unwrap();
            assert_eq!(files.len(), 2);
            assert_eq!(files[0].edits.len() + files[1].edits.len(), 3);
            let here = files.iter().find(|f| f.path == file).unwrap();
            let fix = crate::lint::Fix {
                title: String::new(),
                edits: here.edits.clone(),
                safe: true,
            };
            assert_eq!(
                crate::lint::apply(text, &[&fix]).0,
                "let bar = 1\nbar + 1\n"
            );
            // Nothing to rename on that line.
            let none = rename_on(&server, "plaintext", &file, text, 9, 0, "bar").await;
            assert!(none.is_err());
            let said = hover_on(&server, "plaintext", &file, text, 0, 4)
                .await
                .unwrap();
            assert_eq!(said.as_deref(), Some("fn foo()\n\nDoes foo."));
            let laid_out = format_on(&server, "plaintext", &file, "   x   \ny\n", true, 4)
                .await
                .unwrap();
            assert_eq!(laid_out, "x\ny\n");
        });
    }

    #[test]
    fn hover_answers_come_in_every_shape() {
        assert_eq!(hover_text(&json!({"contents": "plain"})), "plain");
        assert_eq!(
            hover_text(&json!({"contents": {"language": "rs", "value": "```rs\nx\n```"}})),
            "x"
        );
        assert_eq!(
            hover_text(&json!({"contents": ["a", {"value": "b"}]})),
            "a\n\nb"
        );
        assert_eq!(hover_text(&json!({"contents": ""})), "");
    }

    /// Waits until the server has loaded the project, then asks.
    async fn complete_when_ready(
        server: &Server,
        language_id: &str,
        path: &Path,
        text: &str,
        line: usize,
        col: usize,
    ) -> Completions {
        let started = Instant::now();
        loop {
            let found = complete_on(server, language_id, path, text, line, col, Trigger::Invoked)
                .await
                .unwrap();
            if (!found.busy && !found.items.is_empty())
                || started.elapsed() > Duration::from_secs(90)
            {
                return found;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    fn labels(found: &Completions, typed: &str, text: &str, origin: (usize, usize)) -> Vec<String> {
        let base = crate::completion::Base {
            text: text.to_string(),
            origin,
        };
        let line = text.split('\n').nth(origin.0).unwrap_or("");
        crate::completion::rank(&found.items, &base, line, origin)
            .iter()
            .map(|r| found.items[r.index].filter_text.clone())
            .filter(|l| l.starts_with(typed) || typed.is_empty())
            .collect()
    }

    /// Against a real typescript-language-server, in a project prepared by
    /// hand: `COMPLETION_TS=<dir> cargo test -- --ignored real_typescript`,
    /// `<dir>` holding `node_modules/.bin/typescript-language-server` and
    /// TypeScript.
    #[test]
    #[ignore]
    fn real_typescript_completion_is_exact_and_imports() {
        let Some(root) = std::env::var_os("COMPLETION_TS").map(PathBuf::from) else {
            return;
        };
        let exe = root.join("node_modules/.bin/typescript-language-server");
        let dir = root.join("src");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            root.join("tsconfig.json"),
            r#"{ "compilerOptions": { "strict": true, "module": "esnext", "target": "es2022", "moduleResolution": "bundler" }, "include": ["src"] }"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("b.ts"),
            "export function formatDate(d: Date): string { return d.toISOString(); }\n",
        )
        .unwrap();
        let path = dir.join("c.ts");
        // `user.` last: on the line before a name, it would read `user.name`.
        let text = "interface User { name: string; age: number }\nconst user: User = { name: \"x\", age: 1 };\nformatD\nuser.\n";
        std::fs::write(&path, text).unwrap();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let server = server_for(&exe, &["--stdio"], &root).await.unwrap();
            // After `user.`: its two fields, nothing else.
            let found = complete_when_ready(&server, "typescript", &path, text, 3, 5).await;
            let members = labels(&found, "", text, (3, 5));
            assert_eq!(members, vec![".age", ".name"], "{members:?}");
            // A function of another file, imported when accepted.
            let found = complete_when_ready(&server, "typescript", &path, text, 2, 7).await;
            let base = crate::completion::Base {
                text: text.to_string(),
                origin: (2, 7),
            };
            let ranked = crate::completion::rank(&found.items, &base, "formatD", (2, 7));
            let index = ranked
                .iter()
                .map(|r| r.index)
                .find(|&i| found.items[i].label == "formatDate")
                .expect("formatDate is proposed");
            let mut item = found.items[index].clone();
            let resolved = resolve_on(&server, item.raw.clone()).await.unwrap();
            crate::completion::merge_resolved(&mut item, &resolved, text);
            let plan =
                crate::completion::plan(&item, &base, text, (2, 7), false, &|_| None).unwrap();
            assert!(
                plan.text
                    .starts_with("import { formatDate } from \"./b\";\n"),
                "{}",
                plan.text
            );
            assert!(plan.text.contains("\nformatDate\n"), "{}", plan.text);
            // The parameters of a call to it.
            let call = "import { formatDate } from \"./b\";\nformatDate(\n";
            let help = signature_help_on(
                &server,
                "typescript",
                &path,
                call,
                1,
                11,
                SignatureTrigger {
                    kind: 2,
                    character: Some("(".into()),
                    open: None,
                },
            )
            .await
            .unwrap()
            .expect("a call");
            let signatures = crate::completion::parse_signatures(&help).unwrap();
            let sig = &signatures.list[signatures.active];
            assert!(sig.label.contains("d: Date"), "{}", sig.label);
            assert_eq!(signatures.param_of(signatures.active), Some(0));
        });
    }

    /// Against a real rust-analyzer, on a project made for the test:
    /// `cargo test -- --ignored real_rust_analyzer_completion`.
    #[test]
    #[ignore]
    fn real_rust_analyzer_completion_is_exact() {
        let Some(exe) = find_executable("rust-analyzer").filter(|_| usable("rust-analyzer")) else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"probe\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        let path = dir.path().join("src/main.rs");
        let text = "struct User { name: String, age: u32 }\nimpl User {\n    fn greet(&self, loud: bool) -> String { self.name.clone() }\n}\nfn main() {\n    let u = User { name: \"x\".into(), age: 1 };\n    u.\n    let v = vec![1, 2, 3];\n    v.iter().ma\n}\n";
        std::fs::write(&path, text).unwrap();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let server = server_for(&exe, &[], dir.path()).await.unwrap();
            // After `u.`: its fields and methods first, as the server ranks.
            let found = complete_when_ready(&server, "rust", &path, text, 6, 6).await;
            let all = labels(&found, "", text, (6, 6));
            for wanted in ["name", "age", "greet"] {
                assert!(all.iter().any(|l| l == wanted), "{wanted} in {all:?}");
            }
            assert!(
                all.iter().position(|l| l == "name") < all.iter().position(|l| l == "into"),
                "{all:?}"
            );
            // `v.iter().ma`: every kept item has a word starting with `ma`.
            let found = complete_when_ready(&server, "rust", &path, text, 8, 15).await;
            let kept = labels(&found, "", text, (8, 15));
            assert!(kept.iter().any(|l| l == "map"), "{kept:?}");
            assert!(kept.iter().any(|l| l == "max"), "{kept:?}");
            assert!(
                kept.iter()
                    .all(|l| l.starts_with("ma") || l.contains("_ma") || l.contains("Ma")),
                "{kept:?}"
            );
            // Accepting `greet` gives its call with the argument selected.
            let found = complete_when_ready(&server, "rust", &path, text, 6, 6).await;
            let greet = found
                .items
                .iter()
                .find(|i| i.filter_text == "greet")
                .unwrap();
            let base = crate::completion::Base {
                text: text.to_string(),
                origin: (6, 6),
            };
            let plan =
                crate::completion::plan(greet, &base, text, (6, 6), false, &|_| None).unwrap();
            assert!(plan.text.contains("    u.greet(loud)\n"), "{}", plan.text);
            assert_eq!((plan.caret, plan.select), ((6, 12), 4));
        });
    }

    /// Against a real rust-analyzer: the parameters of a call, and the
    /// settings of the Preferences reaching it (`cargo test -- --ignored
    /// real_rust_analyzer_parameters`).
    #[test]
    #[ignore]
    fn real_rust_analyzer_parameters_and_settings() {
        let Some(exe) = find_executable("rust-analyzer").filter(|_| usable("rust-analyzer")) else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"probe\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        let path = dir.path().join("src/main.rs");
        // `x.` last: before another line, it would read `x.greet(…)`.
        let text = "fn greet(loud: bool, n: u32) -> u32 { n }\nfn main() {\n    greet(true, 2);\n    let x = 1;\n    x.\n}\n";
        std::fs::write(&path, text).unwrap();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let has_dbg = |found: &Completions| found.items.iter().any(|i| i.filter_text == "dbg");
            let mut options = std::collections::BTreeMap::new();
            options.insert("rust.postfix".to_string(), false);
            set_completion_options(&options);
            let server = server_for(&exe, &[], dir.path()).await.unwrap();
            let found = complete_when_ready(&server, "rust", &path, text, 4, 6).await;
            assert!(!found.items.is_empty());
            assert!(!has_dbg(&found), "postfix completions are off");
            // The parameter being typed: the second one, after the comma.
            let help = signature_help_on(
                &server,
                "rust",
                &path,
                text,
                2,
                16,
                SignatureTrigger {
                    kind: 1,
                    character: None,
                    open: None,
                },
            )
            .await
            .unwrap()
            .expect("a call");
            let signatures = crate::completion::parse_signatures(&help).unwrap();
            let sig = &signatures.list[signatures.active];
            let param = signatures.param_of(signatures.active).unwrap();
            assert_eq!(
                &sig.label[sig.params[param].0..sig.params[param].1],
                "n: u32"
            );
            // Out of any call: nothing.
            let none = signature_help_on(
                &server,
                "rust",
                &path,
                text,
                3,
                4,
                SignatureTrigger {
                    kind: 1,
                    character: None,
                    open: None,
                },
            )
            .await
            .unwrap();
            assert!(none.is_none_or(|v| crate::completion::parse_signatures(&v).is_none()));
            // Turned on again: the server starts again with it.
            restart_servers().await;
            options.insert("rust.postfix".to_string(), true);
            set_completion_options(&options);
            let server = server_for(&exe, &[], dir.path()).await.unwrap();
            let found = complete_when_ready(&server, "rust", &path, text, 4, 6).await;
            assert!(has_dbg(&found), "postfix completions are back");
            set_completion_options(&std::collections::BTreeMap::new());
        });
    }

    /// Against a real rust-analyzer: the names of a project, every kind or
    /// only the types (`cargo test -- --ignored real_rust_analyzer_symbols`).
    #[test]
    #[ignore]
    fn real_rust_analyzer_symbols() {
        if find_executable("rust-analyzer")
            .filter(|_| usable("rust-analyzer"))
            .is_none()
        {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"probe\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        let path = dir.path().join("src/main.rs");
        std::fs::write(
            &path,
            "struct UserAccount { name: String }\nfn user_name(u: &UserAccount) -> &str { &u.name }\nfn main() {}\n",
        )
        .unwrap();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let servers = vec![("rs".to_string(), path.clone())];
            let started = Instant::now();
            let all = loop {
                let found = workspace_symbols(servers.clone(), "user".into(), true, false).await;
                if (!found.busy && !found.hits.is_empty())
                    || started.elapsed() > Duration::from_secs(90)
                {
                    break found;
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            };
            let names: Vec<&str> = all.hits.iter().map(|h| h.name.as_str()).collect();
            assert!(
                names.contains(&"UserAccount") && names.contains(&"user_name"),
                "{names:?}"
            );
            let account = all.hits.iter().find(|h| h.name == "UserAccount").unwrap();
            assert_eq!((account.line, account.col), (0, 7));
            assert!(is_class_kind(account.kind));
            let types = workspace_symbols(servers, "user".into(), false, false).await;
            assert!(
                types.hits.iter().all(|h| h.name != "user_name"),
                "{:?}",
                types.hits
            );
        });
    }
}
