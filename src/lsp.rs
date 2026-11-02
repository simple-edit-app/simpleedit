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

/// `typescript-language-server` needs a TypeScript installation. The
/// project's own (`node_modules/typescript`) wins; otherwise point it to the
/// copy installed next to the server from the Preferences.
fn init_options(root: &Path) -> Value {
    let project = root.join("node_modules/typescript/lib/tsserver.js");
    let managed = crate::lsp_install::managed_dir()
        .map(|d| d.join("lib/node_modules/typescript/lib/tsserver.js"))
        .filter(|p| p.is_file());
    match managed {
        Some(path) if !project.is_file() => json!({ "tsserver": { "path": path } }),
        _ => json!({}),
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
    for fixed in ["/usr/local/bin", "/snap/bin"] {
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
    name != "rust-analyzer"
        || std::process::Command::new(path)
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
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
    /// Kept so the process dies with the program.
    _child: StdMutex<Child>,
}

type Servers = Mutex<HashMap<(PathBuf, PathBuf), Arc<Server>>>;

/// The running server for this program and project, started if need be.
async fn server_for(exe: &Path, args: &[&str], root: &Path) -> Result<Arc<Server>, String> {
    static SERVERS: OnceLock<Servers> = OnceLock::new();
    let mut servers = SERVERS.get_or_init(Default::default).lock().await;
    let key = (exe.to_path_buf(), root.to_path_buf());
    if let Some(server) = servers.get(&key).filter(|s| s.alive.load(Ordering::SeqCst)) {
        return Ok(server.clone());
    }
    let server = Server::start(exe, args, root).await?;
    servers.insert(key, server.clone());
    Ok(server)
}

impl Server {
    async fn start(exe: &Path, args: &[&str], root: &Path) -> Result<Arc<Server>, String> {
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
            _child: StdMutex::new(child),
        });
        tokio::spawn(read_loop(server.clone(), stdout));

        let root_uri = path_to_uri(root);
        let name = root.file_name().map_or_else(
            || "project".to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        let params = json!({
            "processId": std::process::id(),
            "clientInfo": { "name": "SimpleEdit" },
            "initializationOptions": init_options(root),
            "rootUri": root_uri,
            "workspaceFolders": [{ "uri": root_uri, "name": name }],
            "capabilities": {
                "general": { "positionEncodings": ["utf-16"] },
                "window": { "workDoneProgress": true },
                "workspace": { "configuration": true, "workspaceFolders": true },
                "textDocument": {
                    "synchronization": { "didSave": false },
                    "definition": { "linkSupport": true },
                },
            },
        });
        server
            .request("initialize", params)
            .await
            .map_err(|e| e.message)?;
        server
            .send(&json!({
                "jsonrpc": "2.0", "method": "initialized", "params": {}
            }))
            .await?;
        Ok(server)
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
        rx.await.unwrap_or_else(|_| Err(stopped()))
    }

    /// Tells the server the current text of a document.
    async fn sync(&self, uri: &str, language_id: &str, text: &str) -> Result<(), String> {
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
            Some((_, known)) if known == text => return Ok(()),
            Some((version, known)) => {
                *version += 1;
                *known = text.to_string();
                json!({
                    "jsonrpc": "2.0", "method": "textDocument/didChange",
                    "params": {
                        "textDocument": { "uri": uri, "version": *version },
                        "contentChanges": [{ "text": text }],
                    },
                })
            }
        };
        self.send(&message).await
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
                    let count = message["params"]["items"].as_array().map_or(0, Vec::len);
                    Value::Array(vec![Value::Null; count])
                } else {
                    Value::Null
                };
                let _ = self
                    .send(&json!({ "jsonrpc": "2.0", "id": id, "result": result }))
                    .await;
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

fn stopped() -> RpcError {
    RpcError {
        code: -1,
        message: "language server stopped".to_string(),
    }
}

async fn read_loop(server: Arc<Server>, stdout: ChildStdout) {
    let mut reader = BufReader::new(stdout);
    while let Some(message) = read_message(&mut reader).await {
        server.handle(message).await;
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
}
