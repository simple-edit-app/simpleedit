//! The Database panel: the connections, the PostgreSQL driver they go
//! through, and the tables opened as tabs.
//!
//! The driver is a separate program (`drivers/postgres`), downloaded on
//! demand like the JDBC drivers of a JetBrains IDE: the app talks to it in
//! JSON, one process per operation (see `protocol` for what is said).

pub mod admin_view;
pub mod completion;
pub mod console;
pub mod ddl;
pub mod designer;
pub mod grid;
// The driver uses parts of it that the app does not.
#[allow(dead_code)]
pub mod protocol;
pub mod secrets;
pub mod view;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use console::Console;
use grid::{Editing, Pending, Picker};
use iced::widget::text_editor;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub use protocol::{
    Auth, Database, Executed, Kind, Page, QueryResult, Scope, Ssh, SshAuth, Ssl, SslMode, Target,
    DEFAULT_PORT,
};

/// The protocol the app speaks with the driver; the driver's own number
/// (`--protocol`) must be the same.
pub const DRIVER_PROTOCOL: u32 = protocol::PROTOCOL;
/// Rows in a page of a table, as in a JetBrains IDE, and the other sizes
/// the user can choose.
pub const DEFAULT_PAGE_SIZE: u64 = 500;
pub const PAGE_SIZES: [u64; 6] = [50, 100, 200, 500, 1000, 5000];

const DRIVER_NAME: &str = "simpleedit-db-postgres";
/// Longest wait for the driver to answer (a tunnel takes a while to come up).
const CALL_TIMEOUT: Duration = Duration::from_secs(45);
/// The driver is a few megabytes.
const MAX_DRIVER: usize = 64 * 1024 * 1024;

// ---------------------------------------------------------------- messages

#[derive(Debug, Clone)]
pub enum DbMsg {
    /// Opens the dialog: a new connection, or the one with this id.
    OpenDialog(Option<u64>),
    CloseDialog,
    Tab(DialogTab),
    Text(Field, String),
    Check(Flag, bool),
    Choose(Choice),
    /// The startup script was edited.
    Script(text_editor::Action),
    PropKey(usize, String),
    PropValue(usize, String),
    PropAdd,
    PropRemove(usize),
    /// Asks for a file for this field.
    Browse(Field),
    Browsed(Field, Option<PathBuf>),
    Test,
    Tested(Done<String>),
    Save,
    /// Makes a copy of the connection and opens it.
    Duplicate(u64),
    /// Removes the connection being edited.
    Delete,
    DriverProbed(Driver),
    DownloadDriver,
    DriverDownloaded(Result<(), String>),
    /// Forgets the driver file picked by hand.
    UseDownloadedDriver,
    /// Opens or folds a node of the tree.
    Toggle(String),
    Reload(u64),
    TreeLoaded(u64, Done<Vec<Database>>),
    /// A table: connection, database, schema, table.
    OpenTable(u64, String, String, String),
    /// A page of a table: its tab, then the page.
    Rows(PathBuf, Done<Page>),
    /// Moves a table tab by this many pages.
    Turn(PathBuf, i64),
    Refresh(PathBuf),
    /// The WHERE field of a table tab was typed in, and was validated.
    Filter(PathBuf, String),
    ApplyFilter(PathBuf),
    ClearFilter(PathBuf),
    PageSize(PathBuf, u64),
    /// A column header was clicked: ascending, descending, then none.
    Sort(PathBuf, usize),
    /// A cell: start typing in it, type, validate (Escape gives up).
    Edit(PathBuf, usize, usize),
    EditInput(PathBuf, String),
    EditCommit(PathBuf),
    /// A cell made NULL, a row marked for deletion, taken back.
    SetNull(PathBuf, usize, usize),
    DeleteRow(PathBuf, usize),
    RevertRow(PathBuf, usize),
    RevertAll(PathBuf),
    /// Sends what is pending to the database.
    Commit(PathBuf),
    /// Shows, or hides, the SQL that Commit would run.
    Preview(PathBuf, bool),
    Committed(PathBuf, Done<u64>),
    /// A row is added after the others, and a cell of it left to its default.
    AddRow(PathBuf),
    SetDefault(PathBuf, usize, usize),
    /// A cell was clicked: it has the focus.
    Select(PathBuf, usize, usize),
    /// The console at the bottom: shown or hidden, its tab, its connection.
    ToggleConsole,
    ConsoleTab(console::ConsoleTab),
    ConsoleConnection(u64),
    /// The database of the server the console runs in (empty: the connection's own).
    ConsoleDatabase(String),
    /// The plan of the statement at the caret; with `true`, run for real
    /// (only a reading statement is).
    Explain(bool),
    /// Ends the transaction the console left open.
    EndTransaction(bool),
    /// The rows of a tab, all of them, written to a file: `csv` or `json`.
    Export(PathBuf, &'static str),
    /// `None`: the file dialog was closed.
    Exported(PathBuf, Option<Done<String>>),
    ColumnsLoaded(u64, Done<Vec<protocol::TableColumn>>),
    /// The completion list of the queries: asked for, moved in, taken,
    /// picked with the mouse, closed.
    Complete,
    CompleteMove(i32),
    CompleteAccept,
    CompletePick(usize),
    CompleteClose,
    /// A table's menu: ask before dropping it, or deleting all its rows;
    /// open its designer.
    AskDrop(u64, String, String, String, Kind),
    AskEmpty(u64, String, String, String),
    ConfirmCascade(bool),
    ConfirmRun,
    ConfirmClose,
    OpenDesigner(u64, String, String, String),
    Designer(DesignerMsg),
    /// What an operation on a table gave.
    Altered(Box<Altered>),
    /// A schema's menu: a new table, a new schema in its database.
    NewTable(u64, String, String),
    NewSchema(u64, String),
    SchemaName(String),
    SchemaCreate,
    SchemaClose,
    /// A table of the tree opens or folds (its columns, keys and indexes).
    ToggleTable(u64, String, String, String),
    /// The structure of a table was read: for the tree, or for its DDL.
    StructureRead(TableKey, bool, Done<protocol::Structure>),
    /// The DDL of a table is shown.
    ShowDdl(u64, String, String, String),
    /// What is typed in the filter of the tree.
    TreeFilter(String),
    /// A value is shown whole, a row or a value is copied.
    ViewValue(PathBuf, usize, usize),
    CopyRow(PathBuf, usize),
    TextCopy,
    TextClose,
    CopyCell(PathBuf, usize, usize),
    /// Opens a table with a condition on it (the rows another one points
    /// to, or the rows that point to this one).
    OpenRelated(u64, String, String, String, String),
    /// A column of the grid is dragged to another width.
    ResizeStart(PathBuf, usize),
    ResizeMove(f32),
    ResizeEnd,
    /// Stops the query that runs.
    Cancel,
    ClearLog,
    /// An entry of the log was clicked, or is copied.
    SelectLog(usize),
    CopyLog(usize, LogCopy),
    /// Shows or hides what the driver runs to know the database.
    ShowInternal(bool),
    /// The queries were edited; run the statement of the caret (or the
    /// selection), or all of them.
    Sql(text_editor::Action),
    Run(bool),
    Ran(u64, Done<Ran>),
    /// The date picker of a cell.
    OpenPicker(PathBuf, usize, usize),
    PickerShift(PathBuf, i32),
    PickerDay(PathBuf, u32),
    PickerTime(PathBuf, grid::TimePart, String),
    PickerToday(PathBuf),
    PickerApply(PathBuf),
    PickerClose(PathBuf),
    /// Hides what the last action said.
    DismissNotice(PathBuf),
    /// The grid of a tab scrolled: offset and height of what is seen.
    Scrolled(PathBuf, f32, f32),
}

/// A table: connection, database, schema, table.
pub type TableKey = (u64, String, String, String);

/// A text shown whole, to read and copy: a long value, a DDL.
#[derive(Debug, Clone)]
pub struct TextView {
    pub title: String,
    pub text: String,
}

/// The question to make a schema.
#[derive(Debug, Clone)]
pub struct NewSchema {
    pub conn: u64,
    pub database: String,
    pub name: String,
    pub running: bool,
    pub error: Option<String>,
}

/// A column of the grid that is being dragged: the pointer's first place and
/// the width the column had.
#[derive(Debug, Clone)]
pub struct Resizing {
    pub path: PathBuf,
    pub col: usize,
    pub anchor: Option<(f32, f32)>,
}

/// What an operation on a table was, and what it gave.
#[derive(Debug, Clone)]
pub struct Altered {
    pub what: AlterKind,
    pub conn: u64,
    pub database: String,
    pub schema: String,
    pub table: String,
    pub done: Done<Ran>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AlterKind {
    Drop,
    Empty,
    /// The table was modified; its new name if it was renamed.
    Modify(Option<String>),
    /// A schema was made.
    Schema,
}

/// A drop or a deletion that waits for the user to say yes.
#[derive(Debug, Clone)]
pub struct Confirm {
    pub conn: u64,
    pub database: String,
    pub schema: String,
    pub table: String,
    pub kind: Kind,
    pub action: ConfirmAction,
    /// For a drop: what depends on it goes too.
    pub cascade: bool,
    pub running: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmAction {
    Drop,
    Empty,
}

impl Confirm {
    /// The SQL that is run when the user says yes.
    pub fn statement(&self) -> String {
        let name = format!(
            "{}.{}",
            protocol::quote_ident(&self.schema),
            protocol::quote_ident(&self.table)
        );
        match self.action {
            ConfirmAction::Empty => format!("DELETE FROM {name}"),
            ConfirmAction::Drop => {
                let what = match self.kind {
                    Kind::Table => "TABLE",
                    Kind::View => "VIEW",
                    Kind::Matview => "MATERIALIZED VIEW",
                    Kind::Foreign => "FOREIGN TABLE",
                };
                format!(
                    "DROP {what} {name}{}",
                    if self.cascade { " CASCADE" } else { "" }
                )
            }
        }
    }
}

/// Which text of the table designer is edited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColField {
    Name,
    Type,
    Default,
    Comment,
    /// How the old values become the new ones, for a change of type.
    Using,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConField {
    Name,
    Columns,
    Reference,
    RefColumns,
    OnDelete,
    Expr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdxField {
    Name,
    Columns,
}

/// What is done in the table designer.
#[derive(Debug, Clone)]
pub enum DesignerMsg {
    Loaded(Done<protocol::Structure>),
    Tab(designer::DesignerTab),
    TableName(String),
    TableSchema(String),
    TableComment(String),
    Col(usize, ColField, String),
    ColNull(usize, bool),
    /// Drops a column that exists, or takes back the drop; one that is new
    /// is removed.
    ColDrop(usize, bool),
    ColAdd,
    Con(usize, ConField, String),
    ConDrop(usize, bool),
    ConAdd(protocol::ConstraintKind),
    Idx(usize, IdxField, String),
    IdxUnique(usize, bool),
    IdxDrop(usize, bool),
    IdxAdd,
    Apply,
    Close,
}

/// What is copied of an entry of the log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogCopy {
    Sql,
    Error,
    Entry,
}

/// The tabs of the dialog, as in the Data Sources window of a JetBrains IDE.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogTab {
    General,
    Options,
    SshSsl,
    Schemas,
    Advanced,
    Driver,
}

impl DialogTab {
    pub const ALL: [DialogTab; 6] = [
        DialogTab::General,
        DialogTab::Options,
        DialogTab::SshSsl,
        DialogTab::Schemas,
        DialogTab::Advanced,
        DialogTab::Driver,
    ];
}

/// A line of text of the dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Name,
    Comment,
    Host,
    Port,
    Database,
    Url,
    User,
    Password,
    TimeZone,
    QueryTimeout,
    SshHost,
    SshPort,
    SshUser,
    SshKey,
    SshSecret,
    SshLocalPort,
    SslCa,
    SslCert,
    SslKey,
    SchemaPattern,
    ObjectFilter,
    DriverFile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flag {
    ReadOnly,
    SingleDatabase,
    SystemSchemas,
    TemplateDatabases,
    SshEnabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    /// `true`: only the URL is asked for.
    UrlOnly(bool),
    Auth(Auth),
    Remember(Remember),
    SslMode(SslMode),
    SshAuth(SshAuth),
}

/// What is done with the password once it is typed, as in the
/// "Save" list of a JetBrains IDE.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Remember {
    /// Asked again at every connection.
    Never,
    /// Kept in memory until the app is closed.
    #[default]
    UntilRestart,
    /// Kept in the connections file.
    Forever,
}

impl Remember {
    pub const ALL: [Remember; 3] = [Remember::Never, Remember::UntilRestart, Remember::Forever];
}

// ------------------------------------------------------------- connections

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Connection {
    pub id: u64,
    pub name: String,
    pub comment: String,
    /// Only the URL is given (host, port and database come out of it).
    pub url_only: bool,
    pub url: String,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub user: String,
    pub auth: Auth,
    pub remember: Remember,
    /// Kept in the file only when `remember` is `Forever`.
    pub password: String,
    pub ssl: Ssl,
    pub ssh: Ssh,
    /// The ssh password or key passphrase, kept like the password.
    pub ssh_secret: String,
    pub read_only: bool,
    pub startup_script: String,
    pub time_zone: String,
    pub props: Vec<(String, String)>,
    pub scope: Scope,
    /// The longest a statement may run, in seconds (0: no limit).
    pub query_timeout: u64,
}

impl Default for Connection {
    fn default() -> Self {
        Connection {
            id: 0,
            name: String::new(),
            comment: String::new(),
            url_only: false,
            url: String::new(),
            host: "localhost".to_string(),
            port: DEFAULT_PORT,
            database: String::new(),
            user: String::new(),
            auth: Auth::default(),
            remember: Remember::default(),
            password: String::new(),
            ssl: Ssl::default(),
            ssh: Ssh::default(),
            ssh_secret: String::new(),
            read_only: false,
            startup_script: String::new(),
            time_zone: String::new(),
            props: Vec::new(),
            scope: Scope::default(),
            query_timeout: 0,
        }
    }
}

impl Connection {
    /// What the sidebar calls it: the name, or where it points to.
    pub fn label(&self) -> String {
        if !self.name.trim().is_empty() {
            self.name.trim().to_string()
        } else if !self.database.is_empty() {
            format!("{}@{}", self.database, self.host)
        } else {
            self.host.clone()
        }
    }

    /// The JDBC URL the settings make.
    pub fn jdbc_url(&self) -> String {
        protocol::jdbc_url(&self.host, self.port, &self.database)
    }

    /// Whether the connection can't be made without asking for a password.
    fn needs_password(&self) -> bool {
        self.auth == Auth::UserPassword
    }

    /// What the driver is given; the secrets come from outside.
    fn target(&self, password: String, ssh_secret: String) -> Target {
        Target {
            host: self.host.clone(),
            port: self.port,
            database: self.database.clone(),
            user: self.user.clone(),
            auth: self.auth,
            password,
            ssl: self.ssl.clone(),
            ssh: self.ssh.clone(),
            ssh_secret,
            read_only: self.read_only,
            startup_script: self.startup_script.clone(),
            time_zone: self.time_zone.clone(),
            props: self.props.clone(),
            query_timeout: self.query_timeout,
        }
    }

    /// `Forever` keeps the secrets in the file: the one typed goes in.
    fn remember_secrets(&mut self, password: &str) {
        if self.remember == Remember::Forever {
            self.password = password.to_string();
        } else {
            self.password.clear();
        }
    }
}

/// What is stored in the connections file.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct Stored {
    connections: Vec<Connection>,
    /// A driver file picked by hand, in place of the downloaded one.
    driver_path: String,
    /// Rows in a page of a table.
    page_size: u64,
}

fn connections_file() -> Option<PathBuf> {
    Some(
        crate::fsutil::config_dir()?
            .join("simpleedit")
            .join("databases.json"),
    )
}

fn load_stored() -> Stored {
    let Some(path) = connections_file() else {
        return Stored::default();
    };
    let mut stored: Stored = std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    // What the keychain holds goes back into the connections.
    for c in stored
        .connections
        .iter_mut()
        .filter(|c| c.remember == Remember::Forever)
    {
        if c.password.is_empty() {
            c.password = secrets::load(c.id, secrets::PASSWORD).unwrap_or_default();
        }
        if c.ssh_secret.is_empty() {
            c.ssh_secret = secrets::load(c.id, secrets::SSH).unwrap_or_default();
        }
    }
    stored
}

fn save_stored(connections: &[Connection], driver_path: &str, page_size: u64) {
    let Some(path) = connections_file() else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    // A password that is not to be kept never reaches the disk; one that is
    // kept goes to the keychain, and to the file only where there is none.
    let kept: Vec<Connection> = connections
        .iter()
        .map(|c| {
            if c.remember == Remember::Forever {
                let mut c = c.clone();
                if secrets::save(c.id, secrets::PASSWORD, &c.password) {
                    c.password.clear();
                }
                if secrets::save(c.id, secrets::SSH, &c.ssh_secret) {
                    c.ssh_secret.clear();
                }
                c
            } else {
                secrets::forget(c.id);
                Connection {
                    password: String::new(),
                    ssh_secret: String::new(),
                    ..c.clone()
                }
            }
        })
        .collect();
    let stored = Stored {
        connections: kept,
        driver_path: driver_path.to_string(),
        page_size,
    };
    if let Ok(json) = serde_json::to_vec_pretty(&stored) {
        // Owner only: it may hold passwords.
        let _ = crate::fsutil::write_atomic(&path, &json, true);
    }
}

// ------------------------------------------------------------------ driver

#[derive(Debug, Clone, PartialEq)]
pub enum Driver {
    /// Not looked at yet.
    Unknown,
    Missing,
    Ready,
    Downloading,
    Failed(String),
}

/// Where the driver is looked for: the file picked by hand, a developer's
/// override, next to the app (a build from source), then the folder the app
/// downloads it to.
fn driver_candidates(picked: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    if !picked.trim().is_empty() {
        found.push(PathBuf::from(picked.trim()));
    }
    if let Some(path) = std::env::var_os("SIMPLEEDIT_DB_DRIVER") {
        found.push(PathBuf::from(path));
    }
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
    {
        found.push(dir.join(DRIVER_NAME));
    }
    found.push(downloaded_driver());
    found
}

/// Where a driver is downloaded to.
pub fn downloaded_driver() -> PathBuf {
    crate::fsutil::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("simpleedit")
        .join("drivers")
        .join(DRIVER_NAME)
}

/// The asset of a release that holds the driver for this system.
fn driver_asset(version: &str) -> Option<String> {
    let target = if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "aarch64-apple-darwin"
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        "x86_64-linux"
    } else {
        return None;
    };
    Some(format!("{DRIVER_NAME}-v{version}-{target}"))
}

/// The first driver that is there and speaks the right protocol.
pub async fn find_driver(picked: &str) -> Option<PathBuf> {
    for path in driver_candidates(picked) {
        if !path.is_file() {
            continue;
        }
        let answer = tokio::time::timeout(
            Duration::from_secs(5),
            tokio::process::Command::new(&path)
                .arg("--protocol")
                .kill_on_drop(true)
                .output(),
        )
        .await;
        if let Ok(Ok(out)) = answer {
            let said = String::from_utf8_lossy(&out.stdout);
            if said.trim().parse::<u32>() == Ok(DRIVER_PROTOCOL) {
                return Some(path);
            }
        }
    }
    None
}

pub async fn probe_driver(picked: String) -> Driver {
    if find_driver(&picked).await.is_some() {
        Driver::Ready
    } else {
        Driver::Missing
    }
}

/// Downloads the driver of this version of the app, checked against the
/// signed `SHA256SUMS` of its release, and keeps it in the data folder.
pub async fn download_driver() -> Result<(), String> {
    let version = env!("CARGO_PKG_VERSION");
    let asset = driver_asset(version).ok_or_else(|| t!("db.err_platform").to_string())?;
    let bytes =
        crate::updater::download_verified(version, &asset, MAX_DRIVER, Duration::from_secs(120))
            .await?;
    let dest = downloaded_driver();
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        if let Some(dir) = dest.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        crate::fsutil::write_atomic(&dest, &bytes, true).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o700))
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// What a call to the driver gave, and the commands it ran (for the log,
/// whether it went well or not).
#[derive(Debug, Clone)]
pub struct Done<T> {
    pub result: Result<T, String>,
    pub executed: Vec<Executed>,
}

impl<T> Done<T> {
    /// A value, without anything run (for what is made by hand).
    #[cfg(test)]
    pub fn ok(value: T) -> Self {
        Done {
            result: Ok(value),
            executed: Vec::new(),
        }
    }

    #[cfg(test)]
    pub fn err(error: impl Into<String>) -> Self {
        Done::failed(error.into())
    }

    pub fn failed(error: String) -> Self {
        Done {
            result: Err(error),
            executed: Vec::new(),
        }
    }

    /// The same, with its value made into another.
    fn map<U>(self, f: impl FnOnce(T) -> Result<U, String>) -> Done<U> {
        Done {
            result: self.result.and_then(f),
            executed: self.executed,
        }
    }
}

/// One operation of the driver: the request on its standard input, its
/// answer on its standard output.
async fn call(picked: String, request: protocol::Request) -> Done<serde_json::Value> {
    call_with(None, picked, request).await
}

/// The same, in the session that stays when there is one (see `Live`).
async fn call_with(
    live: Option<Live>,
    picked: String,
    request: protocol::Request,
) -> Done<serde_json::Value> {
    let result = match live {
        Some(live) => call_live(live, picked, request).await,
        None => call_inner(picked, request).await,
    };
    match result {
        Ok(done) => done,
        Err(error) => Done::failed(error),
    }
}

/// How long a request may take before the app gives up on the driver.
fn wait_limit(request: &protocol::Request) -> Option<Duration> {
    // What reads or changes data waits as long as the connection allows (the
    // server stops it), the rest does not wait for long.
    match request.op.as_str() {
        "query" | "rows" | "apply" | "export" => match request.target.query_timeout {
            0 => None,
            seconds => Some(Duration::from_secs(seconds) + CALL_TIMEOUT),
        },
        _ => Some(CALL_TIMEOUT),
    }
}

/// What the driver answered, as a `Done`.
fn read_answer(out: &str) -> Result<Done<serde_json::Value>, String> {
    let answer: serde_json::Value =
        serde_json::from_str(out.trim()).map_err(|_| t!("db.err_driver_answer").to_string())?;
    let executed: Vec<Executed> =
        serde_json::from_value(answer["executed"].clone()).unwrap_or_default();
    let result = if answer["ok"].as_bool() == Some(true) {
        Ok(answer)
    } else {
        Err(answer["error"]
            .as_str()
            .unwrap_or("unknown error")
            .to_string())
    };
    Ok(Done { result, executed })
}

async fn call_inner(
    picked: String,
    request: protocol::Request,
) -> Result<Done<serde_json::Value>, String> {
    let path = find_driver(&picked)
        .await
        .ok_or_else(|| t!("db.err_driver_missing").to_string())?;
    let mut child = tokio::process::Command::new(path)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| e.to_string())?;
    let mut stdin = child.stdin.take().ok_or("no stdin")?;
    let mut stdout = child.stdout.take().ok_or("no stdout")?;
    let body = serde_json::to_vec(&request).map_err(|e| e.to_string())?;
    let work = async {
        stdin.write_all(&body).await.map_err(|e| e.to_string())?;
        drop(stdin);
        let mut out = String::new();
        stdout
            .read_to_string(&mut out)
            .await
            .map_err(|e| e.to_string())?;
        Ok::<String, String>(out)
    };
    let out = match wait_limit(&request) {
        Some(limit) => tokio::time::timeout(limit, work)
            .await
            .map_err(|_| t!("db.err_timeout").to_string())??,
        None => work.await?,
    };
    read_answer(&out)
}

/// A driver process that stays, with the connection it holds: a transaction
/// that was begun, temporary tables and `SET`s last from one request to the
/// next, as in a console of a JetBrains IDE. One per data source, for the
/// console and the result tabs it opened.
struct LiveProcess {
    _child: tokio::process::Child,
    stdin: tokio::process::ChildStdin,
    stdout: tokio::io::BufReader<tokio::process::ChildStdout>,
}

#[derive(Clone, Default)]
pub struct Live(std::sync::Arc<tokio::sync::Mutex<Option<LiveProcess>>>);

impl std::fmt::Debug for Live {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Live")
    }
}

async fn call_live(
    live: Live,
    picked: String,
    request: protocol::Request,
) -> Result<Done<serde_json::Value>, String> {
    use tokio::io::AsyncBufReadExt;
    let path = find_driver(&picked)
        .await
        .ok_or_else(|| t!("db.err_driver_missing").to_string())?;
    let mut guard = live.0.lock().await;
    let mut line = serde_json::to_vec(&request).map_err(|e| e.to_string())?;
    line.push(b'\n');
    let work = async {
        if guard.is_none() {
            let mut child = tokio::process::Command::new(path)
                .arg("--serve")
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null())
                .kill_on_drop(true)
                .spawn()
                .map_err(|e| e.to_string())?;
            let stdin = child.stdin.take().ok_or("no stdin")?;
            let stdout = child.stdout.take().ok_or("no stdout")?;
            *guard = Some(LiveProcess {
                _child: child,
                stdin,
                stdout: tokio::io::BufReader::new(stdout),
            });
        }
        let process = guard.as_mut().ok_or("no driver")?;
        process
            .stdin
            .write_all(&line)
            .await
            .map_err(|e| e.to_string())?;
        process.stdin.flush().await.map_err(|e| e.to_string())?;
        let mut out = String::new();
        let read = process
            .stdout
            .read_line(&mut out)
            .await
            .map_err(|e| e.to_string())?;
        if read == 0 {
            return Err(t!("db.err_driver_answer").to_string());
        }
        Ok::<String, String>(out)
    };
    let out = match wait_limit(&request) {
        Some(limit) => tokio::time::timeout(limit, work)
            .await
            .map_err(|_| t!("db.err_timeout").to_string()),
        None => Ok(work.await),
    };
    match out {
        Ok(Ok(out)) => read_answer(&out),
        // The process can't be trusted any more (it may still be at work, or
        // gone): the next request starts another.
        Ok(Err(error)) | Err(error) => {
            *guard = None;
            Err(error)
        }
    }
}

/// What a call needs: the connection's settings with their secrets, the
/// scope of the tree, and where the driver is.
#[derive(Debug, Clone)]
pub struct Params {
    pub target: Target,
    pub scope: Scope,
    pub driver: String,
}

impl Params {
    fn request(&self, op: &str) -> protocol::Request {
        protocol::Request {
            op: op.to_string(),
            target: self.target.clone(),
            scope: self.scope.clone(),
            ..Default::default()
        }
    }
}

/// Connects and returns the server's version.
pub async fn test(params: Params) -> Done<String> {
    call(params.driver.clone(), params.request("test"))
        .await
        .map(|answer| {
            Ok(answer["version"]
                .as_str()
                .unwrap_or_default()
                .split(" on ")
                .next()
                .unwrap_or_default()
                .to_string())
        })
}

pub async fn tree(params: Params) -> Done<Vec<Database>> {
    call(params.driver.clone(), params.request("tree"))
        .await
        .map(|answer| {
            serde_json::from_value(answer["databases"].clone()).map_err(|e| e.to_string())
        })
}

/// Which rows of a table, and how.
#[derive(Debug, Clone, Default)]
pub struct Query {
    pub offset: u64,
    pub limit: u64,
    /// A condition, as it follows `WHERE`.
    pub filter: String,
    /// Columns and whether each is descending.
    pub order: Vec<(String, bool)>,
    /// The rows of a query, in place of a table's.
    pub sql: Option<String>,
}

/// The table a page or a change is about.
#[derive(Debug, Clone)]
pub struct TableRef {
    pub database: String,
    pub schema: String,
    pub table: String,
}

fn table_request(params: &Params, op: &str, table: &TableRef) -> protocol::Request {
    let mut request = params.request(op);
    request.database = table.database.clone();
    request.schema = table.schema.clone();
    request.table = table.table.clone();
    request
}

#[cfg(test)]
pub async fn rows(params: Params, table: TableRef, query: Query) -> Done<Page> {
    rows_with(None, params, table, query).await
}

/// The same, in the session of the console when it is given.
pub async fn rows_with(
    live: Option<Live>,
    params: Params,
    table: TableRef,
    query: Query,
) -> Done<Page> {
    let mut request = table_request(&params, "rows", &table);
    request.limit = query.limit;
    request.offset = query.offset;
    request.filter = query.filter;
    request.sql = query.sql.unwrap_or_default();
    request.order = query
        .order
        .into_iter()
        .map(|(column, desc)| protocol::Sort { column, desc })
        .collect();
    let offset = query.offset;
    call_with(live, params.driver.clone(), request)
        .await
        .map(|answer| {
            let mut page: Page = serde_json::from_value(answer).map_err(|e| e.to_string())?;
            page.offset = offset;
            Ok(page)
        })
}

/// Makes the changes in the table, all or none; how many rows were touched.
pub async fn apply(params: Params, table: TableRef, changes: Vec<protocol::Change>) -> Done<u64> {
    let mut request = table_request(&params, "apply", &table);
    request.changes = changes;
    call(params.driver.clone(), request)
        .await
        .map(|answer| Ok(answer["applied"].as_u64().unwrap_or(0)))
}

/// The columns of every table of the database, for the completion.
pub async fn table_columns(params: Params) -> Done<Vec<protocol::TableColumn>> {
    call(params.driver.clone(), params.request("columns"))
        .await
        .map(|answer| serde_json::from_value(answer["columns"].clone()).map_err(|e| e.to_string()))
}

/// What running statements gave: a result for each that went well, and the
/// one that failed, if one did (the ones after it did not run).
#[derive(Debug, Clone)]
pub struct Ran {
    pub results: Vec<QueryResult>,
    pub failed: Option<(usize, String)>,
    /// A transaction is open on the session when the statements ended.
    pub in_transaction: bool,
}

/// What a table is made of.
pub async fn structure(params: Params, table: TableRef) -> Done<protocol::Structure> {
    let request = table_request(&params, "structure", &table);
    call(params.driver.clone(), request).await.map(|answer| {
        serde_json::from_value(answer["structure"].clone()).map_err(|e| e.to_string())
    })
}

/// Runs statements one after the other in one session.
#[cfg(test)]
pub async fn run_statements(params: Params, statements: Vec<String>) -> Done<Ran> {
    run_statements_in(params, String::new(), statements).await
}

/// The same, in a database of the server (the connection's own when empty).
pub async fn run_statements_in(
    params: Params,
    database: String,
    statements: Vec<String>,
) -> Done<Ran> {
    run_with(None, params, database, statements, false).await
}

/// Runs statements as one transaction: if one fails, none is made.
pub async fn run_transaction(
    params: Params,
    database: String,
    statements: Vec<String>,
) -> Done<Ran> {
    run_with(None, params, database, statements, true).await
}

/// Runs statements in the session that stays, if one is given: what an
/// earlier run left (a transaction, a temporary table) is still there.
pub async fn run_with(
    live: Option<Live>,
    params: Params,
    database: String,
    statements: Vec<String>,
    transaction: bool,
) -> Done<Ran> {
    let mut request = params.request("query");
    request.database = database;
    request.statements = statements;
    request.transaction = transaction;
    call_with(live, params.driver.clone(), request)
        .await
        .map(|answer| {
            let results: Vec<QueryResult> =
                serde_json::from_value(answer["results"].clone()).map_err(|e| e.to_string())?;
            let failed = answer["failed"].as_object().map(|f| {
                (
                    f["index"].as_u64().unwrap_or(0) as usize,
                    f["error"].as_str().unwrap_or("").to_string(),
                )
            });
            Ok(Ran {
                results,
                failed,
                in_transaction: answer["in_transaction"].as_bool().unwrap_or(false),
            })
        })
}

/// Writes all the rows of a table (with its filter and sort) or of a query to
/// a file; how many there were (CSV counts none).
pub async fn export(
    live: Option<Live>,
    params: Params,
    table: TableRef,
    query: Query,
    path: String,
    format: &str,
) -> Done<String> {
    let mut request = table_request(&params, "export", &table);
    request.filter = query.filter;
    request.sql = query.sql.unwrap_or_default();
    request.order = query
        .order
        .into_iter()
        .map(|(column, desc)| protocol::Sort { column, desc })
        .collect();
    request.path = path;
    request.format = format.to_string();
    call_with(live, params.driver.clone(), request)
        .await
        .map(|answer| Ok(answer["path"].as_str().unwrap_or_default().to_string()))
}

/// The rows a statement gave, as a page; its place is the first one.
pub fn result_page(result: &QueryResult) -> Page {
    Page {
        columns: result
            .columns
            .iter()
            .map(|name| protocol::Column {
                name: name.clone(),
                kind: String::new(),
                nullable: true,
            })
            .collect(),
        rows: result.rows.clone(),
        has_more: result.truncated,
        primary_key: Vec::new(),
        ctids: Vec::new(),
        editable: false,
        source: None,
        offset: 0,
    }
}

// -------------------------------------------------------------------- tree

#[derive(Debug, Clone, PartialEq)]
pub enum Tree {
    Loading,
    Loaded(Vec<Database>),
    Failed(String),
}

// ------------------------------------------------------------------- pages

/// A table opened in a tab.
#[derive(Debug, Clone)]
pub struct TableTab {
    pub conn: u64,
    pub database: String,
    pub schema: String,
    pub table: String,
    pub page: Option<Page>,
    pub loading: bool,
    pub error: Option<String>,
    /// What the grid shows: the offset and the height of its viewport.
    pub scroll: (f32, f32),
    /// The width of each column, worked out when a page arrives.
    pub widths: Vec<f32>,
    /// What is typed in the WHERE field, and what was applied of it.
    pub filter_text: String,
    pub filter: String,
    /// The sort that was applied: columns, and whether each is descending.
    pub order: Vec<(String, bool)>,
    pub page_size: u64,
    /// The edits and deletions that wait for Commit.
    pub pending: Pending,
    /// The cell being typed in, and the date picker, if any.
    pub editing: Option<Editing>,
    pub picker: Option<Picker>,
    /// What the last action said (a failure of the commit, mostly).
    pub notice: Option<String>,
    pub committing: bool,
    /// The SQL of what is pending is on show.
    pub preview: bool,
    /// The cell that has the focus, by row of the page and column.
    pub selected: Option<(usize, usize)>,
    /// The SQL of a result tab: it shows what a query gave, not a table.
    pub query: Option<String>,
    /// What a result tab is called.
    pub title: String,
    /// The query can be sorted, filtered and paged (it only reads).
    pub paged: bool,
    /// The widths the user gave to columns, by name: kept when a page comes.
    pub custom_widths: HashMap<String, f32>,
    /// What the table is made of, once read: for the links of its rows.
    pub structure: Option<protocol::Structure>,
    /// The tab shows a query of the console: its rows are asked for in the
    /// session of the console, which may hold what the query needs.
    pub live: bool,
}

/// What stands for a data source among the projects of the tabs: the tabs of
/// its tables are grouped under it, with its color.
pub fn source_root(conn: u64) -> PathBuf {
    PathBuf::from(format!("db-source://{conn}"))
}

/// The data source a `source_root` stands for.
pub fn source_id(root: &Path) -> Option<u64> {
    root.to_str()?.strip_prefix("db-source://")?.parse().ok()
}

/// The text field of the cell being edited, to give it the focus.
pub fn edit_id() -> iced::widget::Id {
    iced::widget::Id::new("db-cell-edit")
}

/// The rows of the table in front, to scroll them to the focused cell.
pub fn grid_id() -> iced::widget::Id {
    iced::widget::Id::new("db-grid")
}

/// The log of the console, to scroll it to its end.
pub fn log_id() -> iced::widget::Id {
    iced::widget::Id::new("db-log")
}

/// Height of a row of the grid.
pub const ROW_HEIGHT: f32 = 26.0;

impl TableTab {
    pub fn new(conn: u64, database: String, schema: String, table: String, page_size: u64) -> Self {
        TableTab {
            conn,
            database,
            schema,
            table,
            page: None,
            loading: true,
            error: None,
            scroll: (0.0, 900.0),
            widths: Vec::new(),
            custom_widths: HashMap::new(),
            structure: None,
            filter_text: String::new(),
            filter: String::new(),
            order: Vec::new(),
            page_size,
            pending: Pending::default(),
            editing: None,
            picker: None,
            notice: None,
            committing: false,
            preview: false,
            selected: None,
            query: None,
            title: String::new(),
            paged: false,
            live: false,
        }
    }

    /// The table that rows are read from and changed in: the one a query
    /// comes from, if it does from one.
    pub fn table_ref(&self) -> TableRef {
        match self.page.as_ref().and_then(|p| p.source.as_ref()) {
            Some(source) => TableRef {
                database: self.database.clone(),
                schema: source.schema.clone(),
                table: source.table.clone(),
            },
            None => TableRef {
                database: self.database.clone(),
                schema: self.schema.clone(),
                table: self.table.clone(),
            },
        }
    }

    /// A tab of a query that gave all it has, with no sort, filter or page.
    pub fn is_static_result(&self) -> bool {
        self.query.is_some() && !self.paged
    }

    /// The query that gives the page at `offset`.
    pub fn query(&self, offset: u64) -> Query {
        Query {
            offset,
            limit: self.page_size,
            filter: self.filter.clone(),
            order: self.order.clone(),
            sql: self.query.clone().filter(|_| self.paged),
        }
    }

    /// Whether rows can be changed here.
    pub fn editable(&self) -> bool {
        self.page.as_ref().is_some_and(|p| p.editable)
    }

    /// The new order after a click on a column: ascending, descending, none.
    pub fn sorted_by(&self, column: &str, add: bool) -> Vec<(String, bool)> {
        if add {
            // Stacked: this column is added, turned over, then taken out.
            let mut order = self.order.clone();
            match order.iter().position(|(name, _)| name == column) {
                Some(at) if !order[at].1 => order[at].1 = true,
                Some(at) => {
                    order.remove(at);
                }
                None => order.push((column.to_string(), false)),
            }
            return order;
        }
        match self.order.first() {
            Some((name, false)) if name == column && self.order.len() == 1 => {
                vec![(column.to_string(), true)]
            }
            Some((name, true)) if name == column && self.order.len() == 1 => Vec::new(),
            _ => vec![(column.to_string(), false)],
        }
    }

    /// The offset of the page that is shown (0 before any arrived).
    pub fn offset(&self) -> u64 {
        self.page.as_ref().map_or(0, |p| p.offset)
    }

    pub fn set_page(&mut self, page: Page) {
        self.widths = page
            .columns
            .iter()
            .enumerate()
            .map(|(i, column)| {
                // 7.4 px a character, from the longest of the header and the
                // first rows.
                let longest = page
                    .rows
                    .iter()
                    .take(200)
                    .filter_map(|row| row[i].as_deref())
                    .map(|v| v.chars().take(60).count())
                    .chain([column.name.chars().count(), column.kind.chars().count()])
                    .max()
                    .unwrap_or(4);
                // Room for the sort arrow beside the name and the type.
                let worked = (longest as f32 * 7.4 + 44.0).clamp(80.0, 420.0);
                self.custom_widths
                    .get(&column.name)
                    .copied()
                    .unwrap_or(worked)
            })
            .collect();
        self.page = Some(page);
        self.loading = false;
        self.error = None;
        self.scroll.0 = 0.0;
        self.pending = Pending::default();
        self.editing = None;
        self.picker = None;
        self.selected = None;
    }
}

// ------------------------------------------------------------------ dialog

/// The form to add or change a connection: a copy of it that is only kept
/// when the user says OK.
pub struct Dialog {
    pub editing: Option<u64>,
    pub tab: DialogTab,
    pub draft: Connection,
    /// The port as typed, until it is a number.
    pub port: String,
    pub ssh_port: String,
    pub password: String,
    pub script: text_editor::Content,
    /// The password was asked for: this connection has none to use yet.
    pub needs_password: bool,
    pub test: TestState,
    /// The driver file, as typed.
    pub driver_file: String,
    /// The first click on Delete was made: the next one deletes.
    pub confirm_delete: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TestState {
    Idle,
    Running,
    Ok(String),
    Failed(String),
}

impl Dialog {
    pub fn new() -> Self {
        Dialog::from(Connection::default(), String::new(), String::new())
    }

    fn from(draft: Connection, password: String, driver_file: String) -> Self {
        Dialog {
            editing: None,
            tab: DialogTab::General,
            port: draft.port.to_string(),
            ssh_port: draft.ssh.port.to_string(),
            script: text_editor::Content::with_text(&draft.startup_script),
            draft,
            password,
            needs_password: false,
            test: TestState::Idle,
            driver_file,
            confirm_delete: false,
        }
    }

    pub fn edit(connection: &Connection, password: String, driver_file: String) -> Self {
        let mut dialog = Dialog::from(connection.clone(), password, driver_file);
        dialog.editing = Some(connection.id);
        dialog
    }

    /// What a URL says goes into the form: where to connect, who as, and the
    /// encryption it asks for. With `keep_params`, its other parameters
    /// become connection properties (otherwise they are read at save time).
    fn apply_url(&mut self, url: &str, keep_params: bool) {
        let Some(parsed) = protocol::parse_url(url) else {
            return;
        };
        let d = &mut self.draft;
        d.host = parsed.host;
        d.port = parsed.port;
        self.port = parsed.port.to_string();
        d.database = parsed.database;
        if let Some(user) = parsed.user {
            d.user = user;
        }
        if let Some(password) = parsed.password {
            self.password = password;
        }
        for (key, value) in &parsed.params {
            if let Some(mode) = protocol::ssl_mode_of(key, value) {
                d.ssl.mode = mode;
            }
        }
        if keep_params {
            for (key, value) in parsed.params {
                if protocol::ssl_mode_of(&key, &value).is_none()
                    && !d.props.iter().any(|(k, _)| *k == key)
                {
                    d.props.push((key, value));
                }
            }
        }
    }

    /// Sets what a line of text holds.
    pub fn set(&mut self, field: Field, value: String) {
        // A URL fills the form, wherever it was typed.
        match field {
            Field::Host if value.contains("://") && protocol::parse_url(&value).is_some() => {
                self.apply_url(&value, true);
                self.test = TestState::Idle;
                return;
            }
            Field::Url => {
                self.apply_url(&value, false);
                self.draft.url = value;
                self.test = TestState::Idle;
                return;
            }
            _ => {}
        }
        let d = &mut self.draft;
        match field {
            Field::Name => d.name = value,
            Field::Comment => d.comment = value,
            Field::Host => d.host = value,
            Field::Port => {
                if let Some(port) = value.trim().parse::<u16>().ok().filter(|p| *p > 0) {
                    d.port = port;
                }
                self.port = value;
            }
            Field::Database => d.database = value,
            Field::Url => {
                self.apply_url(&value, false);
                self.draft.url = value;
            }
            Field::User => d.user = value,
            Field::Password => self.password = value,
            Field::TimeZone => d.time_zone = value,
            Field::QueryTimeout => {
                if value.trim().is_empty() {
                    d.query_timeout = 0;
                } else if let Ok(seconds) = value.trim().parse::<u64>() {
                    d.query_timeout = seconds;
                }
            }
            Field::SshHost => d.ssh.host = value,
            Field::SshPort => {
                if let Some(port) = value.trim().parse::<u16>().ok().filter(|p| *p > 0) {
                    d.ssh.port = port;
                }
                self.ssh_port = value;
            }
            Field::SshUser => d.ssh.user = value,
            Field::SshKey => d.ssh.key_file = value,
            Field::SshSecret => d.ssh_secret = value,
            Field::SshLocalPort => d.ssh.local_port = value,
            Field::SslCa => d.ssl.ca_file = value,
            Field::SslCert => d.ssl.client_cert = value,
            Field::SslKey => d.ssl.client_key = value,
            Field::SchemaPattern => d.scope.pattern = value,
            Field::ObjectFilter => d.scope.object_filter = value,
            Field::DriverFile => self.driver_file = value,
        }
        self.test = TestState::Idle;
    }

    pub fn check(&mut self, flag: Flag, on: bool) {
        let d = &mut self.draft;
        match flag {
            Flag::ReadOnly => d.read_only = on,
            Flag::SingleDatabase => d.scope.single_database = on,
            Flag::SystemSchemas => d.scope.system_schemas = on,
            Flag::TemplateDatabases => d.scope.template_databases = on,
            Flag::SshEnabled => d.ssh.enabled = on,
        }
        self.test = TestState::Idle;
    }

    pub fn choose(&mut self, choice: Choice) {
        let d = &mut self.draft;
        match choice {
            Choice::UrlOnly(on) => {
                d.url_only = on;
                // The URL starts as what the fields said.
                if on && d.url.trim().is_empty() {
                    d.url = d.jdbc_url();
                }
            }
            Choice::Auth(auth) => d.auth = auth,
            Choice::Remember(remember) => d.remember = remember,
            Choice::SslMode(mode) => d.ssl.mode = mode,
            Choice::SshAuth(auth) => d.ssh.auth = auth,
        }
        self.test = TestState::Idle;
    }

    /// What the script of the Options tab says.
    fn script_text(&self) -> String {
        let text = self.script.text();
        text.strip_suffix('\n').unwrap_or(&text).to_string()
    }

    /// What it takes to connect.
    pub fn valid(&self) -> bool {
        let d = &self.draft;
        let url_ok = !d.url_only || protocol::parse_url(&d.url).is_some();
        !d.host.trim().is_empty()
            && self.port.trim().parse::<u16>().is_ok_and(|p| p > 0)
            && url_ok
            && (!d.ssh.enabled
                || (!d.ssh.host.trim().is_empty()
                    && self.ssh_port.trim().parse::<u16>().is_ok_and(|p| p > 0)))
    }

    /// The connection the form describes, with its script and the
    /// parameters its URL carries.
    pub fn connection(&self, id: u64) -> Connection {
        let mut c = self.draft.clone();
        c.id = id;
        c.name = c.name.trim().to_string();
        c.host = c.host.trim().to_string();
        c.database = c.database.trim().to_string();
        c.user = c.user.trim().to_string();
        c.startup_script = self.script_text();
        if c.url_only {
            // The parameters of the URL are connection parameters too (the
            // encryption it names is already in the SSL settings); the ones
            // of the Advanced tab win.
            if let Some(parsed) = protocol::parse_url(&c.url) {
                for (key, value) in parsed.params {
                    if protocol::ssl_mode_of(&key, &value).is_none()
                        && !c.props.iter().any(|(k, _)| *k == key)
                    {
                        c.props.push((key, value));
                    }
                }
            }
        }
        c.props.retain(|(k, _)| !k.trim().is_empty());
        c.remember_secrets(&self.password);
        c
    }

    /// What the driver gets to test the form as it is.
    pub fn target(&self) -> Target {
        let c = self.connection(0);
        c.target(self.password.clone(), c.ssh_secret.clone())
    }
}

// ------------------------------------------------------------------- state

pub struct State {
    pub connections: Vec<Connection>,
    /// Passwords typed but not kept in the file: `(password, ssh secret)`.
    pub passwords: HashMap<u64, (String, String)>,
    /// Passwords of connections that ask every time: used once.
    pub once: HashMap<u64, (String, String)>,
    pub trees: HashMap<u64, Tree>,
    /// The open nodes of the tree: `<id>`, `<id>/<database>` and
    /// `<id>/<database>/<schema>`.
    pub open: HashSet<String>,
    pub dialog: Option<Dialog>,
    pub driver: Driver,
    /// A driver file picked by hand.
    pub driver_path: String,
    /// Rows in a page of a table, for the tables that are opened.
    pub page_size: u64,
    /// The console at the bottom of the window.
    pub console: Console,
    /// The columns of the tables of each connection, once read.
    pub columns: HashMap<u64, Vec<protocol::TableColumn>>,
    /// The table being modified, and a drop or deletion waiting for a yes.
    pub designer: Option<designer::Designer>,
    pub confirm: Option<Confirm>,
    /// What each table is made of, read when a table of the tree opens.
    pub structures: HashMap<TableKey, protocol::Structure>,
    /// What is typed to find a table in the tree.
    pub tree_filter: String,
    /// A text shown whole, and the question to make a schema.
    pub text: Option<TextView>,
    pub new_schema: Option<NewSchema>,
    pub resizing: Option<Resizing>,
    /// The sessions that stay, one per data source the console ran on.
    pub live: HashMap<u64, Live>,
}

impl State {
    /// The session of a data source, made when there is none.
    pub fn live_of(&mut self, id: u64) -> Live {
        self.live.entry(id).or_default().clone()
    }

    /// Ends the session of a data source (its transaction is rolled back).
    pub fn end_live(&mut self, id: u64) {
        self.live.remove(&id);
    }

    pub fn load() -> Self {
        let stored = load_stored();
        State {
            connections: stored.connections,
            passwords: HashMap::new(),
            once: HashMap::new(),
            trees: HashMap::new(),
            open: HashSet::new(),
            dialog: None,
            driver: Driver::Unknown,
            driver_path: stored.driver_path,
            page_size: if stored.page_size == 0 {
                DEFAULT_PAGE_SIZE
            } else {
                stored.page_size
            },
            console: Console::load(),
            columns: HashMap::new(),
            designer: None,
            confirm: None,
            structures: HashMap::new(),
            tree_filter: String::new(),
            text: None,
            new_schema: None,
            resizing: None,
            live: HashMap::new(),
        }
    }

    /// What the completion knows of a connection: its tables, and the
    /// columns when they have been read.
    pub fn known(&self, id: u64) -> completion::Known {
        let mut known = completion::Known::default();
        if let Some(Tree::Loaded(databases)) = self.trees.get(&id) {
            // The tables of the database the connection is on.
            let current = databases.iter().find(|d| d.current).or(databases.first());
            for schema in current.into_iter().flat_map(|d| &d.schemas) {
                for table in &schema.tables {
                    known
                        .tables
                        .push((schema.name.clone(), table.name.clone(), table.kind));
                }
            }
        }
        for column in self.columns.get(&id).into_iter().flatten() {
            known
                .columns
                .entry((column.schema.clone(), column.table.clone()))
                .or_default()
                .push((column.name.clone(), column.kind.clone()));
        }
        // A table the tree leaves out (a filter) is known by its columns.
        for (schema, table) in known.columns.keys().cloned().collect::<Vec<_>>() {
            if !known.tables.iter().any(|t| t.0 == schema && t.1 == table) {
                known.tables.push((schema, table, Kind::Table));
            }
        }
        known
    }

    pub fn connection(&self, id: u64) -> Option<&Connection> {
        self.connections.iter().find(|c| c.id == id)
    }

    /// The password and the ssh secret to connect with, if they are known.
    pub fn secrets(&self, connection: &Connection) -> Option<(String, String)> {
        if !connection.needs_password() {
            return Some((String::new(), connection.ssh_secret.clone()));
        }
        match connection.remember {
            Remember::Forever => Some((connection.password.clone(), connection.ssh_secret.clone())),
            Remember::UntilRestart => self.passwords.get(&connection.id).cloned(),
            Remember::Never => self.once.get(&connection.id).cloned(),
        }
    }

    pub fn params(&self, id: u64) -> Option<Params> {
        let c = self.connection(id)?;
        let (password, ssh_secret) = self.secrets(c)?;
        Some(Params {
            target: c.target(password, ssh_secret),
            scope: c.scope.clone(),
            driver: self.driver_path.clone(),
        })
    }

    pub fn persist(&self) {
        save_stored(&self.connections, &self.driver_path, self.page_size);
    }

    /// Adds or replaces the connection the dialog describes; its id.
    pub fn apply(&mut self, dialog: &Dialog) -> Option<u64> {
        if !dialog.valid() {
            return None;
        }
        let id = dialog.editing.unwrap_or_else(|| self.next_id());
        let connection = dialog.connection(id);
        let secrets = (dialog.password.clone(), connection.ssh_secret.clone());
        match connection.remember {
            Remember::Forever => {
                self.passwords.remove(&id);
                self.once.remove(&id);
            }
            Remember::UntilRestart => {
                self.passwords.insert(id, secrets);
                self.once.remove(&id);
            }
            Remember::Never => {
                self.passwords.remove(&id);
                self.once.insert(id, secrets);
            }
        }
        match self.connections.iter_mut().find(|c| c.id == id) {
            Some(existing) => *existing = connection,
            None => self.connections.push(connection),
        }
        self.trees.remove(&id);
        // The settings may have changed: the next run connects afresh.
        self.live.remove(&id);
        self.persist();
        Some(id)
    }

    fn next_id(&self) -> u64 {
        self.connections.iter().map(|c| c.id).max().unwrap_or(0) + 1
    }

    /// A copy of the connection, with its secrets; its id.
    pub fn duplicate(&mut self, id: u64) -> Option<u64> {
        let mut copy = self.connection(id)?.clone();
        copy.id = self.next_id();
        copy.name = format!("{} (2)", copy.label());
        let secrets = self.passwords.get(&id).cloned();
        let new = copy.id;
        if let Some(secrets) = secrets {
            self.passwords.insert(new, secrets);
        }
        self.connections.push(copy);
        self.persist();
        Some(new)
    }

    pub fn remove(&mut self, id: u64) {
        self.connections.retain(|c| c.id != id);
        secrets::forget(id);
        self.passwords.remove(&id);
        self.once.remove(&id);
        self.trees.remove(&id);
        self.live.remove(&id);
        let prefix = format!("{id}/");
        self.open
            .retain(|k| *k != id.to_string() && !k.starts_with(&prefix));
        self.persist();
    }
}

#[cfg(test)]
mod tests {
    use super::protocol::Column;
    use super::*;

    fn filled() -> Dialog {
        let mut d = Dialog::new();
        d.set(Field::Host, " db.example.com ".into());
        d.set(Field::Database, "shop".into());
        d.set(Field::User, "dev".into());
        d.set(Field::Password, "secret".into());
        d
    }

    fn fresh() -> State {
        let mut state = State::load();
        state.connections.clear();
        state
    }

    #[test]
    fn a_new_dialog_points_to_a_local_postgres() {
        let d = Dialog::new();
        assert_eq!(d.draft.host, "localhost");
        assert_eq!(d.draft.port, 5432);
        assert_eq!(d.draft.scope.pattern, "@:*");
        assert!(d.valid());
    }

    #[test]
    fn a_bad_port_or_an_empty_host_is_not_valid() {
        let mut d = Dialog::new();
        for bad in ["abc", "0", "70000", ""] {
            d.set(Field::Port, bad.into());
            assert!(!d.valid(), "{bad}");
        }
        d.set(Field::Port, "5432".into());
        assert!(d.valid());
        d.set(Field::Host, "  ".into());
        assert!(!d.valid());
    }

    #[test]
    fn a_tunnel_needs_its_own_host_and_port() {
        let mut d = Dialog::new();
        d.check(Flag::SshEnabled, true);
        assert!(!d.valid());
        d.set(Field::SshHost, "bastion".into());
        assert!(d.valid());
        d.set(Field::SshPort, "x".into());
        assert!(!d.valid());
    }

    #[test]
    fn the_url_gives_the_host_port_and_database() {
        let mut d = Dialog::new();
        d.choose(Choice::UrlOnly(true));
        assert_eq!(d.draft.url, "jdbc:postgresql://localhost:5432/");
        d.set(
            Field::Url,
            "jdbc:postgresql://10.0.0.5:6543/shop?application_name=se".into(),
        );
        assert_eq!(d.draft.host, "10.0.0.5");
        assert_eq!(d.port, "6543");
        assert_eq!(d.draft.database, "shop");
        let c = d.connection(1);
        assert_eq!(c.props, vec![("application_name".into(), "se".into())]);
        // A broken URL is not a connection.
        d.set(Field::Url, "nonsense".into());
        assert!(!d.valid());
    }

    const NEON: &str = "postgresql://neondb_owner:s3cr%40t@ep-x-pooler.c-3.eu-central-1.aws.neon.tech/neondb?sslmode=require&channel_binding=require";

    #[test]
    fn a_pasted_url_fills_the_user_the_password_and_the_encryption() {
        let mut d = Dialog::new();
        d.choose(Choice::UrlOnly(true));
        d.set(Field::Url, NEON.into());
        assert_eq!(d.draft.host, "ep-x-pooler.c-3.eu-central-1.aws.neon.tech");
        assert_eq!(d.draft.port, 5432);
        assert_eq!(d.draft.database, "neondb");
        assert_eq!(d.draft.user, "neondb_owner");
        assert_eq!(d.password, "s3cr@t");
        assert_eq!(d.draft.ssl.mode, SslMode::Require);
        assert!(d.valid());
        let c = d.connection(1);
        // The encryption is a setting, the rest stays a parameter.
        assert_eq!(c.props, vec![("channel_binding".into(), "require".into())]);
        assert_eq!(c.ssl.mode, SslMode::Require);
        assert_eq!(c.user, "neondb_owner");
    }

    #[test]
    fn a_url_pasted_in_the_host_field_fills_the_form_too() {
        let mut d = Dialog::new();
        d.set(Field::Host, NEON.into());
        assert_eq!(d.draft.host, "ep-x-pooler.c-3.eu-central-1.aws.neon.tech");
        assert_eq!(d.draft.database, "neondb");
        assert_eq!(d.draft.user, "neondb_owner");
        assert_eq!(d.draft.ssl.mode, SslMode::Require);
        let c = d.connection(1);
        assert_eq!(c.props, vec![("channel_binding".into(), "require".into())]);
        // The URL is for the form: the type stays the one chosen.
        assert!(!c.url_only);
        // Typing a plain host still works.
        d.set(Field::Host, "db.example.com".into());
        assert_eq!(d.draft.host, "db.example.com");
    }

    #[test]
    fn the_advanced_tab_wins_over_the_parameters_of_the_url() {
        let mut d = Dialog::new();
        d.choose(Choice::UrlOnly(true));
        d.set(
            Field::Url,
            "jdbc:postgresql://h/db?application_name=url".into(),
        );
        d.draft
            .props
            .push(("application_name".into(), "advanced".into()));
        d.draft.props.push((" ".into(), "ignored".into()));
        let c = d.connection(1);
        assert_eq!(
            c.props,
            vec![("application_name".into(), "advanced".into())]
        );
    }

    #[test]
    fn the_connection_is_named_after_what_it_points_to() {
        let mut state = fresh();
        let id = state.apply(&filled()).unwrap();
        let c = state.connection(id).unwrap().clone();
        assert_eq!(c.label(), "shop@db.example.com");
        assert_eq!(c.host, "db.example.com");
        let mut named = filled();
        named.editing = Some(id);
        named.set(Field::Name, "Prod".into());
        state.apply(&named);
        assert_eq!(state.connections.len(), 1);
        assert_eq!(state.connection(id).unwrap().label(), "Prod");
        state.connections.clear();
    }

    #[test]
    fn a_password_kept_until_restart_stays_in_memory_only() {
        let mut state = fresh();
        let id = state.apply(&filled()).unwrap();
        let c = state.connection(id).unwrap().clone();
        assert_eq!(c.password, "");
        assert_eq!(state.secrets(&c).unwrap().0, "secret");
        // A fresh run has none.
        state.passwords.clear();
        assert_eq!(state.secrets(&c), None);
        state.connections.clear();
    }

    #[test]
    fn a_password_kept_forever_is_in_the_file_and_used() {
        let mut state = fresh();
        let mut d = filled();
        d.choose(Choice::Remember(Remember::Forever));
        let id = state.apply(&d).unwrap();
        let c = state.connection(id).unwrap().clone();
        assert_eq!(c.password, "secret");
        state.passwords.clear();
        assert_eq!(state.secrets(&c).unwrap().0, "secret");
        state.connections.clear();
    }

    #[test]
    fn a_password_never_kept_is_asked_again() {
        let mut state = fresh();
        let mut d = filled();
        d.choose(Choice::Remember(Remember::Never));
        let id = state.apply(&d).unwrap();
        let c = state.connection(id).unwrap().clone();
        assert_eq!(c.password, "");
        // The one typed serves once.
        assert!(state.secrets(&c).is_some());
        state.once.clear();
        assert_eq!(state.secrets(&c), None);
        state.connections.clear();
    }

    #[test]
    fn without_a_password_to_type_nothing_is_asked() {
        let mut state = fresh();
        let mut d = filled();
        d.choose(Choice::Auth(Auth::NoAuth));
        let id = state.apply(&d).unwrap();
        state.passwords.clear();
        let c = state.connection(id).unwrap().clone();
        assert!(state.secrets(&c).is_some());
        d.choose(Choice::Auth(Auth::Pgpass));
        state.apply(&d);
        let c = state.connection(id).unwrap().clone();
        assert!(state.secrets(&c).is_some());
        state.connections.clear();
    }

    #[test]
    fn the_settings_reach_the_driver() {
        let mut state = fresh();
        let mut d = filled();
        d.check(Flag::ReadOnly, true);
        d.set(Field::TimeZone, "Europe/Paris".into());
        d.set(Field::SchemaPattern, "*:*".into());
        d.set(Field::ObjectFilter, "table:-tmp_.*".into());
        d.choose(Choice::SslMode(SslMode::VerifyFull));
        d.set(Field::SslCa, "/ca.pem".into());
        d.check(Flag::SshEnabled, true);
        d.set(Field::SshHost, "bastion".into());
        d.choose(Choice::SshAuth(SshAuth::KeyPair));
        d.script = text_editor::Content::with_text("SET search_path TO shop;\n");
        let id = state.apply(&d).unwrap();
        let p = state.params(id).unwrap();
        assert!(p.target.read_only);
        assert_eq!(p.target.time_zone, "Europe/Paris");
        assert_eq!(p.target.ssl.mode, SslMode::VerifyFull);
        assert_eq!(p.target.ssl.ca_file, "/ca.pem");
        assert!(p.target.ssh.enabled);
        assert_eq!(p.target.ssh.host, "bastion");
        assert_eq!(p.target.startup_script, "SET search_path TO shop;");
        assert_eq!(p.target.password, "secret");
        assert_eq!(p.scope.pattern, "*:*");
        assert_eq!(p.scope.object_filter, "table:-tmp_.*");
        state.connections.clear();
    }

    #[test]
    fn a_duplicate_keeps_the_settings_under_a_new_name() {
        let mut state = fresh();
        let id = state.apply(&filled()).unwrap();
        let copy = state.duplicate(id).unwrap();
        assert_ne!(copy, id);
        let c = state.connection(copy).unwrap().clone();
        assert_eq!(c.host, "db.example.com");
        assert_eq!(c.name, "shop@db.example.com (2)");
        assert_eq!(state.secrets(&c).unwrap().0, "secret");
        state.connections.clear();
    }

    #[test]
    fn removing_a_connection_forgets_its_tree_and_open_nodes() {
        let mut state = fresh();
        let id = state.apply(&filled()).unwrap();
        state.trees.insert(id, Tree::Loading);
        state.open.insert(id.to_string());
        state.open.insert(format!("{id}/shop/public"));
        state.open.insert("99/shop/public".into());
        state.remove(id);
        assert!(state.connections.is_empty());
        assert!(state.trees.is_empty());
        assert_eq!(state.open.len(), 1);
    }

    #[test]
    fn a_page_sizes_its_columns_from_its_content() {
        let mut tab = TableTab::new(1, "shop".into(), "public".into(), "t".into(), 500);
        tab.set_page(Page {
            columns: vec![
                Column {
                    name: "id".into(),
                    kind: "integer".into(),
                    nullable: false,
                },
                Column {
                    name: "bio".into(),
                    kind: "text".into(),
                    nullable: true,
                },
            ],
            rows: vec![vec![Some("1".into()), Some("x".repeat(500))]],
            has_more: false,
            primary_key: vec![],
            ctids: Vec::new(),
            editable: false,
            source: None,
            offset: 0,
        });
        assert_eq!(tab.widths.len(), 2);
        // The type ("integer") is the longest: 7 characters, and the arrow.
        assert!((tab.widths[0] - 95.8).abs() < 0.01);
        // Long values are capped.
        assert_eq!(tab.widths[1], 420.0);
        assert!(!tab.loading);
    }

    #[test]
    fn the_driver_answers_are_read() {
        let page: Page = serde_json::from_value(serde_json::json!({
            "columns": [{"name": "id", "type": "integer"}],
            "rows": [["1"], [null]],
            "has_more": true,
        }))
        .unwrap();
        assert_eq!(page.rows[1][0], None);
        assert!(page.has_more);
        let databases: Vec<Database> = serde_json::from_value(serde_json::json!([
            {"name": "shop", "current": true,
             "schemas": [{"name": "public", "tables": [{"name": "t", "kind": "matview"}]}]}
        ]))
        .unwrap();
        assert_eq!(databases[0].schemas[0].tables[0].kind, Kind::Matview);
    }

    #[test]
    fn an_old_file_without_the_new_settings_still_loads() {
        let c: Connection = serde_json::from_str(
            r#"{"id":3,"name":"x","host":"h","port":5433,"database":"d","user":"u"}"#,
        )
        .unwrap();
        assert_eq!(c.port, 5433);
        assert_eq!(c.ssl.mode, SslMode::Disable);
        assert_eq!(c.remember, Remember::UntilRestart);
        assert_eq!(c.scope.pattern, "@:*");
    }

    #[test]
    fn the_asset_is_named_after_the_version_and_the_system() {
        if let Some(asset) = driver_asset("1.18.0") {
            assert!(asset.starts_with("simpleedit-db-postgres-v1.18.0-"));
        }
    }

    fn tref(schema: &str, table: &str) -> TableRef {
        TableRef {
            database: "shop".into(),
            schema: schema.into(),
            table: table.into(),
        }
    }

    fn at(offset: u64) -> Query {
        Query {
            offset,
            limit: DEFAULT_PAGE_SIZE,
            ..Query::default()
        }
    }

    #[test]
    fn a_click_on_a_header_goes_ascending_then_descending_then_off() {
        let mut tab = TableTab::new(1, "shop".into(), "public".into(), "t".into(), 500);
        tab.order = tab.sorted_by("name", false);
        assert_eq!(tab.order, vec![("name".to_string(), false)]);
        tab.order = tab.sorted_by("name", false);
        assert_eq!(tab.order, vec![("name".to_string(), true)]);
        tab.order = tab.sorted_by("name", false);
        assert!(tab.order.is_empty());
        // Another column starts again, replacing the first.
        tab.order = tab.sorted_by("a", false);
        tab.order = tab.sorted_by("b", false);
        assert_eq!(tab.order, vec![("b".to_string(), false)]);
    }

    #[test]
    fn alt_click_stacks_the_sort_on_more_columns() {
        let mut tab = TableTab::new(1, "shop".into(), "public".into(), "t".into(), 500);
        tab.order = tab.sorted_by("a", false);
        tab.order = tab.sorted_by("b", true);
        assert_eq!(
            tab.order,
            vec![("a".to_string(), false), ("b".to_string(), false)]
        );
        tab.order = tab.sorted_by("a", true);
        assert_eq!(
            tab.order,
            vec![("a".to_string(), true), ("b".to_string(), false)]
        );
        tab.order = tab.sorted_by("a", true);
        assert_eq!(tab.order, vec![("b".to_string(), false)]);
        // A plain click starts again.
        tab.order = tab.sorted_by("c", false);
        assert_eq!(tab.order, vec![("c".to_string(), false)]);
    }

    #[test]
    fn the_query_carries_the_page_the_filter_and_the_sort() {
        let mut tab = TableTab::new(1, "shop".into(), "public".into(), "t".into(), 100);
        tab.filter = "score > 2".into();
        tab.order = vec![("score".into(), true)];
        let q = tab.query(300);
        assert_eq!((q.offset, q.limit), (300, 100));
        assert_eq!(q.filter, "score > 2");
        assert_eq!(q.order, vec![("score".to_string(), true)]);
    }

    #[test]
    fn a_new_page_throws_away_what_was_pending() {
        use grid::Pending;
        let mut tab = TableTab::new(1, "shop".into(), "public".into(), "t".into(), 500);
        tab.pending = Pending::default();
        tab.pending.deletes.insert(0);
        tab.editing = Some(Editing {
            row: 0,
            col: 0,
            text: "x".into(),
        });
        tab.set_page(Page {
            columns: vec![],
            rows: vec![],
            has_more: false,
            primary_key: vec![],
            ctids: Vec::new(),
            editable: false,
            source: None,
            offset: 0,
        });
        assert!(tab.pending.is_empty() && tab.editing.is_none());
    }

    /// The session that stays, the transaction of a change, a read-only
    /// source and the export, against a real server (same variables as
    /// `live_postgres_is_read_through_the_driver`).
    #[test]
    #[ignore]
    fn live_session_transaction_read_only_and_export() {
        let Ok(spec) = std::env::var("SE_PG_TEST") else {
            return;
        };
        let p: Vec<&str> = spec.split(':').collect();
        let target = Target {
            host: p[0].into(),
            port: p[1].parse().unwrap(),
            database: p[2].into(),
            user: p[3].into(),
            password: p[4].into(),
            ..Target::default()
        };
        let params = Params {
            target,
            scope: Scope::default(),
            driver: String::new(),
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let sql = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
            // One session: a transaction and a temporary table last from one
            // run to the next, and the rows of a query are read inside them.
            let live = Live::default();
            let begun = run_with(
                Some(live.clone()),
                params.clone(),
                String::new(),
                sql(&[
                    "BEGIN",
                    "CREATE TEMP TABLE live_tmp (a int)",
                    "INSERT INTO live_tmp VALUES (1), (2)",
                ]),
                false,
            )
            .await
            .result
            .unwrap();
            assert!(begun.failed.is_none() && begun.in_transaction);
            let query = Query {
                limit: 10,
                sql: Some("SELECT a FROM live_tmp".into()),
                ..Query::default()
            };
            let page = rows_with(Some(live.clone()), params.clone(), tref("", ""), query)
                .await
                .result
                .unwrap();
            assert_eq!(page.rows.len(), 2);
            let ended = run_with(
                Some(live.clone()),
                params.clone(),
                String::new(),
                sql(&["ROLLBACK"]),
                false,
            )
            .await
            .result
            .unwrap();
            assert!(!ended.in_transaction);
            let gone = run_with(
                Some(live.clone()),
                params.clone(),
                String::new(),
                sql(&["SELECT a FROM live_tmp"]),
                false,
            )
            .await
            .result
            .unwrap();
            assert!(gone.failed.is_some());

            // A change is all or none.
            let broken = run_transaction(
                params.clone(),
                String::new(),
                sql(&["CREATE TABLE live_atomic (a int)", "SELEKT 1"]),
            )
            .await
            .result
            .unwrap();
            assert_eq!(broken.failed.as_ref().map(|f| f.0), Some(1));
            let absent = run_statements(params.clone(), sql(&["SELECT 1 FROM live_atomic"]))
                .await
                .result
                .unwrap();
            assert!(absent.failed.is_some());

            // A read-only source stays read-only, whatever is typed.
            let mut locked = params.clone();
            locked.target.read_only = true;
            for attempt in [
                "SET default_transaction_read_only = off",
                "BEGIN READ WRITE",
                "CREATE TABLE live_nope (a int)",
                "COMMIT",
            ] {
                let ran = run_statements(locked.clone(), sql(&[attempt]))
                    .await
                    .result
                    .unwrap();
                assert!(ran.failed.is_some(), "{attempt}");
            }
            let semicolon = Query {
                limit: 10,
                filter: "true); CREATE TABLE live_nope (a int); --".into(),
                ..Query::default()
            };
            assert!(
                rows(locked.clone(), tref("public", "live_export"), semicolon)
                    .await
                    .result
                    .is_err()
            );

            // The pages of a table follow its key, and all the rows go to a file.
            run_transaction(
                params.clone(),
                String::new(),
                sql(&[
                    "DROP TABLE IF EXISTS live_export",
                    "CREATE TABLE live_export (id int PRIMARY KEY, label text)",
                    "INSERT INTO live_export SELECT g, 'row \"' || g || E'\\n' FROM generate_series(1, 30) g",
                ]),
            )
            .await
            .result
            .unwrap();
            let first = Query {
                limit: 10,
                ..Query::default()
            };
            let second = Query {
                offset: 10,
                limit: 10,
                ..Query::default()
            };
            let a = rows(params.clone(), tref("public", "live_export"), first)
                .await
                .result
                .unwrap();
            let b = rows(params.clone(), tref("public", "live_export"), second)
                .await
                .result
                .unwrap();
            assert_eq!(a.rows[0][0].as_deref(), Some("1"));
            assert_eq!(b.rows[0][0].as_deref(), Some("11"));
            let dir = tempfile::tempdir().unwrap();
            for (format, ext) in [("csv", "csv"), ("json", "json")] {
                let file = dir.path().join(format!("out.{ext}"));
                export(
                    None,
                    params.clone(),
                    tref("public", "live_export"),
                    Query::default(),
                    file.to_string_lossy().to_string(),
                    format,
                )
                .await
                .result
                .unwrap();
                let text = std::fs::read_to_string(&file).unwrap();
                if format == "csv" {
                    // The header, then one record per row (a label holds a line break).
                    assert!(text.starts_with("id,label\n1,"));
                } else {
                    let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
                    assert_eq!(parsed.as_array().unwrap().len(), 30);
                    assert_eq!(parsed[0]["id"], 1);
                }
            }
            run_statements(
                params.clone(),
                sql(&["DROP TABLE live_export"]),
            )
            .await
            .result
            .unwrap();
        });
    }

    /// Against a real server, with the driver built next to the app:
    /// `SIMPLEEDIT_DB_DRIVER=target/debug/simpleedit-db-postgres
    /// SE_PG_TEST=127.0.0.1:55432:shop:dev:secret cargo test live_postgres
    /// -- --ignored`.
    #[test]
    #[ignore]
    fn live_postgres_is_read_through_the_driver() {
        let Ok(spec) = std::env::var("SE_PG_TEST") else {
            return;
        };
        let p: Vec<&str> = spec.split(':').collect();
        let target = Target {
            host: p[0].into(),
            port: p[1].parse().unwrap(),
            database: p[2].into(),
            user: p[3].into(),
            password: p[4].into(),
            ..Target::default()
        };
        let params = Params {
            target,
            scope: Scope::default(),
            driver: String::new(),
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            assert_eq!(probe_driver(String::new()).await, Driver::Ready);
            let version = test(params.clone()).await.result.unwrap();
            assert!(version.starts_with("PostgreSQL"), "{version}");
            let databases = tree(params.clone()).await.result.unwrap();
            assert_eq!(databases.len(), 1);
            let schemas = &databases[0].schemas;
            let public = schemas.iter().find(|s| s.name == "public").unwrap();
            assert!(public.tables.iter().any(|t| t.name == "customers"));
            let sales = schemas.iter().find(|s| s.name == "sales").unwrap();
            assert_eq!(sales.tables[0].name, "Order Items");
            // Every database, then a filter on the tables.
            let mut everything = params.clone();
            everything.scope.pattern = "*:*".into();
            assert!(tree(everything).await.result.unwrap().len() >= 2);
            let mut filtered = params.clone();
            filtered.scope.object_filter = "table:-cust.*".into();
            let shown = tree(filtered).await.result.unwrap();
            let public = shown[0]
                .schemas
                .iter()
                .find(|s| s.name == "public")
                .unwrap();
            assert!(public.tables.iter().all(|t| t.name != "customers"));
            let page = rows(params.clone(), tref("public", "customers"), at(0))
            .await
                .result
                .unwrap();
            assert_eq!(page.rows.len() as u64, DEFAULT_PAGE_SIZE);
            assert!(page.has_more);
            assert_eq!(page.columns[0].name, "id");
            let last = rows(params.clone(), tref("public", "customers"), at(1000))
            .await
                .result
                .unwrap();
            assert_eq!(last.rows.len(), 200);
            assert!(!last.has_more);
            assert_eq!(last.rows[0][0].as_deref(), Some("1001"));
            // Names that need quoting.
            let odd = rows(params.clone(), tref("sales", "Order Items"), at(0))
            .await
                .result
                .unwrap();
            assert_eq!(odd.rows.len(), 2);
            let missing = rows(params.clone(), tref("public", "nope"), at(0))
            .await;
            assert!(missing.result.unwrap_err().contains("no such table"));
            let mut bad = params.clone();
            bad.target.password = "wrong".into();
            assert!(test(bad).await.result.unwrap_err().contains("password"));
            // Read only, a time zone and a script that fails.
            let mut set = params.clone();
            set.target.read_only = true;
            set.target.time_zone = "Europe/Paris".into();
            set.target.startup_script = "SELECT 1".into();
            assert!(test(set.clone()).await.result.is_ok());
            set.target.startup_script = "SELEKT nope".into();
            assert!(test(set).await.result.unwrap_err().contains("startup script"));
            // Filter, sort, and changes in a table of its own (made through
            // the startup script, which can run any SQL).
            let mut setup = params.clone();
            setup.target.startup_script = "DROP TABLE IF EXISTS live_items; \
                CREATE TABLE live_items (id int primary key, label text, due date, score numeric(8,2)); \
                INSERT INTO live_items VALUES (1,'a','2026-01-05',1.5),(2,'b','2026-02-10',2.5),(3,'c',NULL,9.99);"
                .into();
            test(setup).await.result.unwrap();
            let table = tref("public", "live_items");
            let query = Query {
                limit: 10,
                filter: "score > 2".into(),
                order: vec![("score".into(), true)],
                ..Query::default()
            };
            let filtered = rows(params.clone(), table.clone(), query).await
                .result
                .unwrap();
            assert_eq!(filtered.rows.len(), 2);
            assert_eq!(filtered.rows[0][0].as_deref(), Some("3"));
            assert_eq!(filtered.primary_key, vec!["id".to_string()]);
            assert!(filtered.editable);
            assert!(!filtered.columns[0].nullable && filtered.columns[1].nullable);
            let bad_filter = Query {
                limit: 10,
                filter: "nope = 1".into(),
                ..Query::default()
            };
            assert!(rows(params.clone(), table.clone(), bad_filter)
                .await
                .result
                .is_err());
            // A pending edit, a NULL and a date, then a deletion.
            let mut pending = grid::Pending::default();
            pending.set(&filtered, 1, 1, Some("B".into()));
            pending.set(&filtered, 1, 2, Some("2026-12-25".into()));
            pending.set(&filtered, 0, 3, None);
            let changes = pending.changes(&filtered);
            assert_eq!(
                apply(params.clone(), table.clone(), changes).await.result,
                Ok(2)
            );
            let all = Query {
                limit: 10,
                order: vec![("id".into(), false)],
                ..Query::default()
            };
            let after = rows(params.clone(), table.clone(), all.clone())
                .await
                .result
                .unwrap();
            assert_eq!(after.rows[1][1].as_deref(), Some("B"));
            assert_eq!(after.rows[1][2].as_deref(), Some("2026-12-25"));
            assert_eq!(after.rows[2][3], None);
            let mut gone = grid::Pending::default();
            gone.delete(&after, 0);
            let removed = apply(params.clone(), table.clone(), gone.changes(&after))
                .await
                .result;
            assert_eq!(removed, Ok(1));
            // The same row again: nothing to delete, and nothing is done.
            let again = apply(params.clone(), table.clone(), gone.changes(&after))
                .await
                .result;
            assert!(again.unwrap_err().contains("0 rows"));
            let left = rows(params.clone(), table.clone(), all.clone()).await
                .result
                .unwrap();
            assert_eq!(left.rows.len(), 2);
            // Rows that are added: what is given, the rest left to the defaults,
            // all or none.
            let before = rows(params.clone(), table.clone(), all.clone())
                .await
                .result
                .unwrap();
            let mut added = grid::Pending::default();
            let first = added.add_row(&before);
            added.set(&before, first, 0, Some("10".into()));
            added.set(&before, first, 1, Some("ten".into()));
            added.set(&before, first, 2, Some("2026-10-10".into()));
            let second = added.add_row(&before);
            added.set(&before, second, 0, Some("11".into()));
            added.set(&before, second, 1, None);
            let done = apply(params.clone(), table.clone(), added.changes(&before)).await;
            assert_eq!(done.result, Ok(2));
            let after_add = rows(params.clone(), table.clone(), all.clone())
                .await
                .result
                .unwrap();
            assert_eq!(after_add.rows.len(), before.rows.len() + 2);
            let ten = after_add
                .rows
                .iter()
                .find(|r| r[0].as_deref() == Some("10"))
                .unwrap();
            assert_eq!(ten[1].as_deref(), Some("ten"));
            assert_eq!(ten[2].as_deref(), Some("2026-10-10"));
            // The second row failed to be NULL? It was given NULL: it is there.
            assert!(after_add
                .rows
                .iter()
                .any(|r| r[0].as_deref() == Some("11") && r[1].is_none()));
            // One of the two breaks a rule (the same key): none is added.
            let mut clash = grid::Pending::default();
            let fresh = clash.add_row(&after_add);
            clash.set(&after_add, fresh, 0, Some("12".into()));
            let again = clash.add_row(&after_add);
            clash.set(&after_add, again, 0, Some("10".into()));
            let refused = apply(params.clone(), table.clone(), clash.changes(&after_add)).await;
            assert!(refused.result.is_err());
            let still = rows(params.clone(), table.clone(), all.clone())
                .await
                .result
                .unwrap();
            assert_eq!(still.rows.len(), after_add.rows.len());
            // A table with no key: its rows are found by their place, and must
            // still have the values they were read with.
            let mut nokey = params.clone();
            nokey.target.startup_script = "DROP TABLE IF EXISTS live_nokey; \
                CREATE TABLE live_nokey (a int, b text); \
                INSERT INTO live_nokey VALUES (1,'x'),(1,'x'),(2,NULL)"
                .into();
            test(nokey).await.result.unwrap();
            let loose = tref("public", "live_nokey");
            let page = rows(params.clone(), loose.clone(), at(0))
                .await
                .result
                .unwrap();
            assert!(page.editable && page.primary_key.is_empty());
            assert_eq!(page.ctids.len(), 3);
            // One of two identical rows is changed, the other left alone.
            let mut pending = grid::Pending::default();
            pending.set(&page, 0, 1, Some("changed".into()));
            pending.delete(&page, 2);
            let done = apply(params.clone(), loose.clone(), pending.changes(&page)).await;
            assert_eq!(done.result, Ok(2));
            assert!(done.executed.iter().any(|e| e.sql.contains("ctid")));
            let after = rows(params.clone(), loose.clone(), at(0))
                .await
                .result
                .unwrap();
            let mut seen: Vec<(Option<String>, Option<String>)> = after
                .rows
                .iter()
                .map(|r| (r[0].clone(), r[1].clone()))
                .collect();
            seen.sort();
            assert_eq!(
                seen,
                vec![
                    (Some("1".into()), Some("changed".into())),
                    (Some("1".into()), Some("x".into()))
                ]
            );
            // The row was changed by someone else since it was read: not touched.
            let mut stale = grid::Pending::default();
            stale.set(&after, 0, 1, Some("mine".into()));
            let mut other = params.clone();
            other.target.startup_script = "UPDATE live_nokey SET b = 'theirs'".into();
            test(other).await.result.unwrap();
            let refused = apply(params.clone(), loose.clone(), stale.changes(&after)).await;
            assert!(refused.result.unwrap_err().contains("0 rows"));
            // A row added to a table that has no key.
            let current = rows(params.clone(), loose.clone(), at(0))
                .await
                .result
                .unwrap();
            let mut added = grid::Pending::default();
            let new = added.add_row(&current);
            added.set(&current, new, 0, Some("7".into()));
            assert_eq!(
                apply(params.clone(), loose.clone(), added.changes(&current))
                    .await
                    .result,
                Ok(1)
            );
            // The table designer: what is edited becomes statements that, run in
            // one transaction, bring the table there.
            let mut ddl = params.clone();
            ddl.target.startup_script = "DROP TABLE IF EXISTS live_ddl2; DROP TABLE IF EXISTS live_ddl; \
                CREATE TABLE live_ddl (id serial PRIMARY KEY, label text NOT NULL, score numeric(8,2), flag boolean DEFAULT true); \
                INSERT INTO live_ddl (label, score) VALUES ('a', 1.5), ('b', 2)"
                .into();
            test(ddl).await.result.unwrap();
            let before = structure(params.clone(), tref("public", "live_ddl"))
                .await
                .result
                .unwrap();
            assert_eq!(before.columns.len(), 4);
            assert_eq!(before.constraints[0].kind, protocol::ConstraintKind::Primary);
            let mut designer =
                designer::Designer::new(1, "shop".into(), "public".into(), "live_ddl".into());
            designer.load(before);
            designer.columns[2].name = "points".into();
            designer.columns[2].kind = "integer".into();
            designer.columns[1].nullable = true;
            // Retyped with a default: the default goes and comes back.
            designer.columns[3].kind = "text".into();
            designer.columns[3].default = "'yes'".into();
            designer.add_column();
            let note = designer.columns.last_mut().unwrap();
            note.name = "note".into();
            note.kind = "varchar(20)".into();
            note.default = "'none'".into();
            note.comment = "a note".into();
            designer.add_constraint(protocol::ConstraintKind::Unique);
            designer.constraints.last_mut().unwrap().columns = "label".into();
            designer.add_constraint(protocol::ConstraintKind::Check);
            designer.constraints.last_mut().unwrap().expr = "points >= 0".into();
            designer.add_index();
            let index = designer.indexes.last_mut().unwrap();
            index.columns = "points, label".into();
            index.name = "live_ddl_points_idx".into();
            designer.comment = "ddl test".into();
            designer.name = "live_ddl2".into();
            assert_eq!(designer.problem(), None);
            let mut all = vec!["BEGIN".to_string()];
            all.extend(designer.statements());
            all.push("COMMIT".to_string());
            let ran = run_statements_in(params.clone(), String::new(), all).await;
            let ran = ran.result.unwrap();
            assert_eq!(ran.failed, None, "{:?}", designer.statements());
            let after = structure(params.clone(), tref("public", "live_ddl2"))
                .await
                .result
                .unwrap();
            let names: Vec<&str> = after.columns.iter().map(|c| c.name.as_str()).collect();
            assert_eq!(names, vec!["id", "label", "points", "flag", "note"]);
            assert_eq!(after.columns[2].kind, "integer");
            assert!(after.columns[1].nullable);
            assert_eq!(after.columns[3].kind, "text");
            assert_eq!(after.columns[3].default.as_deref(), Some("'yes'::text"));
            assert_eq!(after.columns[4].comment, "a note");
            assert_eq!(after.comment, "ddl test");
            assert!(after
                .constraints
                .iter()
                .any(|c| c.kind == protocol::ConstraintKind::Unique));
            assert!(after
                .constraints
                .iter()
                .any(|c| c.kind == protocol::ConstraintKind::Check));
            assert_eq!(after.indexes.len(), 1);
            // The data went along, converted.
            let moved = rows(params.clone(), tref("public", "live_ddl2"), at(0))
                .await
                .result
                .unwrap();
            assert_eq!(moved.rows.len(), 2);
            // One statement fails: nothing of the transaction stays.
            let failed = run_statements_in(
                params.clone(),
                String::new(),
                vec![
                    "BEGIN".into(),
                    "ALTER TABLE live_ddl2 ADD COLUMN extra int".into(),
                    "ALTER TABLE live_ddl2 ADD COLUMN label text".into(),
                    "COMMIT".into(),
                ],
            )
            .await
            .result
            .unwrap();
            assert_eq!(failed.failed.as_ref().map(|f| f.0), Some(2));
            let still = structure(params.clone(), tref("public", "live_ddl2"))
                .await
                .result
                .unwrap();
            assert!(still.columns.iter().all(|c| c.name != "extra"));
            // Constraints and indexes are edited in place, a type is converted with
            // an expression, a table moves to another schema.
            let mut setup = params.clone();
            setup.target.startup_script = "DROP SCHEMA IF EXISTS live_arch CASCADE; \
                DROP TABLE IF EXISTS live_child; DROP TABLE IF EXISTS live_parent; \
                DROP FUNCTION IF EXISTS live_fn(int); DROP TYPE IF EXISTS live_mood; \
                DROP SEQUENCE IF EXISTS live_seq; CREATE SCHEMA live_arch; \
                CREATE TABLE live_parent (id int PRIMARY KEY); \
                CREATE TABLE live_child (id serial PRIMARY KEY, parent_id int, qty text, \
                    CONSTRAINT live_child_fk FOREIGN KEY (parent_id) REFERENCES live_parent(id) \
                        ON UPDATE CASCADE DEFERRABLE, \
                    CONSTRAINT live_child_qty CHECK (qty <> '')); \
                CREATE INDEX live_child_idx ON live_child (parent_id); \
                INSERT INTO live_parent VALUES (1); \
                INSERT INTO live_child (parent_id, qty) VALUES (1, '12'); \
                CREATE FUNCTION live_fn(a int) RETURNS int LANGUAGE sql AS 'SELECT a'; \
                CREATE TYPE live_mood AS ENUM ('ok', 'ko'); CREATE SEQUENCE live_seq"
                .into();
            test(setup).await.result.unwrap();
            let child = structure(params.clone(), tref("public", "live_child"))
                .await
                .result
                .unwrap();
            let parent = structure(params.clone(), tref("public", "live_parent"))
                .await
                .result
                .unwrap();
            assert_eq!(parent.referenced_by.len(), 1);
            assert_eq!(parent.referenced_by[0].table, "live_child");
            let mut edit =
                designer::Designer::new(1, "shop".into(), "public".into(), "live_child".into());
            edit.load(child.clone());
            assert!(edit.constraints.iter().all(designer::EditConstraint::editable));
            let fk = edit
                .constraints
                .iter_mut()
                .find(|c| c.kind == protocol::ConstraintKind::Foreign)
                .unwrap();
            // The part of the definition that is not edited stays.
            assert_eq!(fk.extra, "ON UPDATE CASCADE DEFERRABLE");
            fk.on_delete = "CASCADE".into();
            let check = edit
                .constraints
                .iter_mut()
                .find(|c| c.kind == protocol::ConstraintKind::Check)
                .unwrap();
            check.expr = "qty <> 0".into();
            edit.indexes[0].name = "live_child_parent_idx".into();
            edit.columns[2].kind = "integer".into();
            edit.columns[2].using = "qty::integer * 2".into();
            edit.schema_to = "live_arch".into();
            let mut all = vec!["BEGIN".to_string()];
            all.extend(edit.statements());
            all.push("COMMIT".to_string());
            let ran = run_statements_in(params.clone(), String::new(), all)
                .await
                .result
                .unwrap();
            assert_eq!(ran.failed, None, "{:?}", edit.statements());
            let moved = structure(params.clone(), tref("live_arch", "live_child"))
                .await
                .result
                .unwrap();
            let fk = moved
                .constraints
                .iter()
                .find(|c| c.kind == protocol::ConstraintKind::Foreign)
                .unwrap();
            assert!(fk.definition.contains("ON DELETE CASCADE"), "{}", fk.definition);
            assert!(fk.definition.contains("ON UPDATE CASCADE"), "{}", fk.definition);
            assert!(fk.definition.contains("DEFERRABLE"), "{}", fk.definition);
            assert_eq!(moved.indexes[0].name, "live_child_parent_idx");
            assert_eq!(moved.columns[2].kind, "integer");
            let converted = rows(params.clone(), tref("live_arch", "live_child"), at(0))
                .await
                .result
                .unwrap();
            assert_eq!(converted.rows[0][2].as_deref(), Some("24"));
            // What the DDL says makes the table again.
            let ddl_text = ddl::table_ddl("live_arch", "live_child", &moved);
            let recreated = run_statements_in(
                params.clone(),
                String::new(),
                vec!["DROP TABLE live_arch.live_child".into(), ddl_text],
            )
            .await
            .result
            .unwrap();
            assert_eq!(recreated.failed, None);
            let again = structure(params.clone(), tref("live_arch", "live_child"))
                .await
                .result
                .unwrap();
            assert_eq!(again.columns, moved.columns);
            assert_eq!(again.constraints, moved.constraints);
            assert_eq!(again.indexes, moved.indexes);
            // The tree knows the functions, the sequences and the types.
            let databases = tree(params.clone()).await.result.unwrap();
            let public = databases[0]
                .schemas
                .iter()
                .find(|s| s.name == "public")
                .unwrap();
            assert!(public.routines.iter().any(|r| r.name == "live_fn" && r.args == "a integer"));
            assert!(public.types.iter().any(|t| t == "live_mood"));
            assert!(public.sequences.iter().any(|n| n == "live_seq"));
            // The sequence of a serial column belongs to its table.
            assert!(public.sequences.iter().all(|n| !n.ends_with("_id_seq")));
            // A table is made in the new schema from the designer.
            let mut fresh = designer::Designer::for_new_table(1, "shop".into(), "live_arch".into());
            fresh.name = "live_made".into();
            fresh.add_column();
            let c = fresh.columns.last_mut().unwrap();
            c.name = "total".into();
            c.kind = "numeric(10,2)".into();
            c.default = "0".into();
            c.comment = "what is due".into();
            fresh.add_index();
            fresh.indexes.last_mut().unwrap().columns = "total".into();
            assert_eq!(fresh.problem(), None);
            let mut all = vec!["BEGIN".to_string()];
            all.extend(fresh.statements());
            all.push("COMMIT".to_string());
            let ran = run_statements_in(params.clone(), String::new(), all)
                .await
                .result
                .unwrap();
            assert_eq!(ran.failed, None, "{:?}", fresh.statements());
            let made = structure(params.clone(), tref("live_arch", "live_made"))
                .await
                .result
                .unwrap();
            assert_eq!(made.columns[0].name, "id");
            assert_eq!(made.columns[1].comment, "what is due");
            assert_eq!(made.indexes.len(), 1);
            // A statement that runs too long is stopped by the server.
            let mut slow = params.clone();
            slow.target.query_timeout = 1;
            let stopped = run_statements_in(slow, String::new(), vec!["DO $$ BEGIN PERFORM pg_sleep(5); END $$".into()])
                .await
                .result
                .unwrap();
            assert!(
                stopped.failed.as_ref().is_some_and(|f| f.1.contains("timeout")),
                "{:?}",
                stopped.failed
            );
            let mut cleanup = params.clone();
            cleanup.target.startup_script = "DROP SCHEMA IF EXISTS live_arch CASCADE; \
                DROP TABLE IF EXISTS live_parent; DROP FUNCTION IF EXISTS live_fn(int); \
                DROP TYPE IF EXISTS live_mood; DROP SEQUENCE IF EXISTS live_seq"
                .into();
            test(cleanup).await.result.unwrap();
            // All the rows go, the table stays; then the table is dropped.
            let confirm = |action, cascade| Confirm {
                conn: 1,
                database: String::new(),
                schema: "public".into(),
                table: "live_ddl2".into(),
                kind: Kind::Table,
                action,
                cascade,
                running: false,
                error: None,
            };
            let emptied = run_statements_in(
                params.clone(),
                String::new(),
                vec![confirm(ConfirmAction::Empty, false).statement()],
            )
            .await
            .result
            .unwrap();
            assert_eq!(emptied.results[0].affected, 2);
            let none = rows(params.clone(), tref("public", "live_ddl2"), at(0))
                .await
                .result
                .unwrap();
            assert!(none.rows.is_empty());
            assert_eq!(
                confirm(ConfirmAction::Drop, true).statement(),
                "DROP TABLE \"public\".\"live_ddl2\" CASCADE"
            );
            let dropped = run_statements_in(
                params.clone(),
                String::new(),
                vec![confirm(ConfirmAction::Drop, false).statement()],
            )
            .await
            .result
            .unwrap();
            assert_eq!(dropped.failed, None);
            assert!(structure(params.clone(), tref("public", "live_ddl2"))
                .await
                .result
                .is_err());
            let mut cleanup = params.clone();
            cleanup.target.startup_script =
                "DROP TABLE live_items; DROP TABLE live_nokey".into();
            test(cleanup).await.result.unwrap();
            // Queries: a select, an update, a failure; one session.
            let ran = run_statements(
                params.clone(),
                vec![
                    "CREATE TEMP TABLE live_tmp (a int, b text)".into(),
                    "INSERT INTO live_tmp VALUES (1,'x'),(2,NULL)".into(),
                    "SELECT a, b FROM live_tmp ORDER BY a".into(),
                    "UPDATE live_tmp SET b = 'y' WHERE a = 2".into(),
                    "SELEKT 1".into(),
                    "SELECT 99".into(),
                ],
            )
            .await;
            // What ran is in the log, the failure too; a SELECT is left to the
            // tab that pages it, so it is not run here.
            assert_eq!(ran.executed.len(), 4);
            assert!(ran.executed[3].error.is_some());
            assert_eq!(ran.executed[1].rows, Some(2));
            let ran = ran.result.unwrap();
            assert_eq!(ran.results.len(), 4);
            assert_eq!(ran.failed.as_ref().map(|f| f.0), Some(4));
            let select = &ran.results[2];
            assert!(select.has_rows && select.deferred && select.rows.is_empty());
            assert_eq!(select.columns, vec!["a".to_string(), "b".to_string()]);
            assert!(!ran.results[3].has_rows);
            assert_eq!(ran.results[3].affected, 1);
            // The rows of a table are in the log too, as plain SQL (what the
            // driver runs to know the table is marked).
            let shown = rows(params.clone(), tref("public", "customers"), at(0)).await;
            let user: Vec<&Executed> = shown.executed.iter().filter(|e| !e.internal).collect();
            assert_eq!(user.len(), 1);
            assert!(user[0].sql.starts_with("SELECT * FROM \"public\".\"customers\""));
            assert_eq!(user[0].rows, Some(DEFAULT_PAGE_SIZE));
            assert!(shown.executed.iter().any(|e| e.internal));
            // A query's rows: paged, sorted, filtered, and editable when they
            // come from one table with its key.
            let query = |filter: &str, order: Vec<(String, bool)>, sql: &str| Query {
                limit: 2,
                filter: filter.into(),
                order,
                sql: Some(sql.into()),
                ..Query::default()
            };
            let result = rows(
                params.clone(),
                tref("", ""),
                query(
                    "id > 3",
                    vec![("score".into(), true)],
                    "SELECT id, name, score FROM customers",
                ),
            )
            .await
            .result
            .unwrap();
            assert_eq!(result.rows.len(), 2);
            assert!(result.has_more && result.editable);
            assert_eq!(result.primary_key, vec!["id".to_string()]);
            assert_eq!(result.source.as_ref().map(|s| s.table.as_str()), Some("customers"));
            // A join, an alias or an aggregate give rows that cannot be changed.
            for sql in [
                "SELECT id AS ident, name FROM customers",
                "SELECT count(*) FROM customers",
            ] {
                let page = rows(params.clone(), tref("", ""), query("", vec![], sql))
                    .await
                    .result
                    .unwrap();
                assert!(!page.editable, "{sql}");
            }
            // The columns of the tables, for the completion.
            let columns = table_columns(params.clone()).await;
            assert!(columns.executed.iter().all(|e| e.internal));
            let columns = columns.result.unwrap();
            assert!(columns
                .iter()
                .any(|c| c.table == "customers" && c.name == "email" && c.schema == "public"));
            // TLS asked of a server that has none.
            let mut tls = params;
            tls.target.ssl.mode = SslMode::Require;
            assert!(test(tls).await.result.is_err());
        });
    }
}
