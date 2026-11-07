//! The Database panel: the connections, the PostgreSQL driver they go
//! through, and the tables opened as tabs.
//!
//! The driver is a separate program (`drivers/postgres`), downloaded on
//! demand like the JDBC drivers of a JetBrains IDE: the app talks to it in
//! JSON, one process per operation (see `protocol` for what is said).

pub mod protocol;
pub mod view;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use iced::widget::text_editor;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub use protocol::{
    Auth, Database, Kind, Page, Scope, Ssh, SshAuth, Ssl, SslMode, Target, DEFAULT_PORT,
};

/// The protocol the app speaks with the driver; the driver's own number
/// (`--protocol`) must be the same.
pub const DRIVER_PROTOCOL: u32 = protocol::PROTOCOL;
/// Rows in a page of a table, as in a JetBrains IDE.
pub const PAGE_SIZE: u64 = 500;

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
    Tested(Result<String, String>),
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
    TreeLoaded(u64, Result<Vec<Database>, String>),
    /// A table: connection, database, schema, table.
    OpenTable(u64, String, String, String),
    /// A page of a table: its tab, then the page.
    Rows(PathBuf, Result<Page, String>),
    /// Moves a table tab by this many pages.
    Turn(PathBuf, i64),
    Refresh(PathBuf),
    /// The grid of a tab scrolled: offset and height of what is seen.
    Scrolled(PathBuf, f32, f32),
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
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn save_stored(connections: &[Connection], driver_path: &str) {
    let Some(path) = connections_file() else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    // A password that is not to be kept never reaches the disk.
    let kept: Vec<Connection> = connections
        .iter()
        .map(|c| {
            if c.remember == Remember::Forever {
                c.clone()
            } else {
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

/// One operation of the driver: the request on its standard input, its
/// answer on its standard output.
async fn call(picked: String, request: protocol::Request) -> Result<serde_json::Value, String> {
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
    let out = tokio::time::timeout(CALL_TIMEOUT, work)
        .await
        .map_err(|_| t!("db.err_timeout").to_string())??;
    let answer: serde_json::Value =
        serde_json::from_str(out.trim()).map_err(|_| t!("db.err_driver_answer").to_string())?;
    if answer["ok"].as_bool() == Some(true) {
        Ok(answer)
    } else {
        Err(answer["error"]
            .as_str()
            .unwrap_or("unknown error")
            .to_string())
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
pub async fn test(params: Params) -> Result<String, String> {
    let answer = call(params.driver.clone(), params.request("test")).await?;
    Ok(answer["version"]
        .as_str()
        .unwrap_or_default()
        .split(" on ")
        .next()
        .unwrap_or_default()
        .to_string())
}

pub async fn tree(params: Params) -> Result<Vec<Database>, String> {
    let answer = call(params.driver.clone(), params.request("tree")).await?;
    serde_json::from_value(answer["databases"].clone()).map_err(|e| e.to_string())
}

pub async fn rows(
    params: Params,
    database: String,
    schema: String,
    table: String,
    offset: u64,
) -> Result<Page, String> {
    let mut request = params.request("rows");
    request.database = database;
    request.schema = schema;
    request.table = table;
    request.limit = PAGE_SIZE;
    request.offset = offset;
    let answer = call(params.driver.clone(), request).await?;
    let mut page: Page = serde_json::from_value(answer).map_err(|e| e.to_string())?;
    page.offset = offset;
    Ok(page)
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
}

/// Height of a row of the grid.
pub const ROW_HEIGHT: f32 = 26.0;

impl TableTab {
    pub fn new(conn: u64, database: String, schema: String, table: String) -> Self {
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
                (longest as f32 * 7.4 + 28.0).clamp(80.0, 420.0)
            })
            .collect();
        self.page = Some(page);
        self.loading = false;
        self.error = None;
        self.scroll.0 = 0.0;
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
}

impl State {
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
        }
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
        save_stored(&self.connections, &self.driver_path);
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
        self.passwords.remove(&id);
        self.once.remove(&id);
        self.trees.remove(&id);
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
        let mut tab = TableTab::new(1, "shop".into(), "public".into(), "t".into());
        tab.set_page(Page {
            columns: vec![
                Column {
                    name: "id".into(),
                    kind: "integer".into(),
                },
                Column {
                    name: "bio".into(),
                    kind: "text".into(),
                },
            ],
            rows: vec![vec![Some("1".into()), Some("x".repeat(500))]],
            has_more: false,
            offset: 0,
        });
        assert_eq!(tab.widths.len(), 2);
        assert_eq!(tab.widths[0], 80.0);
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
            let version = test(params.clone()).await.unwrap();
            assert!(version.starts_with("PostgreSQL"), "{version}");
            let databases = tree(params.clone()).await.unwrap();
            assert_eq!(databases.len(), 1);
            let schemas = &databases[0].schemas;
            let public = schemas.iter().find(|s| s.name == "public").unwrap();
            assert!(public.tables.iter().any(|t| t.name == "customers"));
            let sales = schemas.iter().find(|s| s.name == "sales").unwrap();
            assert_eq!(sales.tables[0].name, "Order Items");
            // Every database, then a filter on the tables.
            let mut everything = params.clone();
            everything.scope.pattern = "*:*".into();
            assert!(tree(everything).await.unwrap().len() >= 2);
            let mut filtered = params.clone();
            filtered.scope.object_filter = "table:-cust.*".into();
            let shown = tree(filtered).await.unwrap();
            let public = shown[0]
                .schemas
                .iter()
                .find(|s| s.name == "public")
                .unwrap();
            assert!(public.tables.iter().all(|t| t.name != "customers"));
            let page = rows(
                params.clone(),
                "shop".into(),
                "public".into(),
                "customers".into(),
                0,
            )
            .await
            .unwrap();
            assert_eq!(page.rows.len() as u64, PAGE_SIZE);
            assert!(page.has_more);
            assert_eq!(page.columns[0].name, "id");
            let last = rows(
                params.clone(),
                "shop".into(),
                "public".into(),
                "customers".into(),
                1000,
            )
            .await
            .unwrap();
            assert_eq!(last.rows.len(), 200);
            assert!(!last.has_more);
            assert_eq!(last.rows[0][0].as_deref(), Some("1001"));
            // Names that need quoting.
            let odd = rows(
                params.clone(),
                "shop".into(),
                "sales".into(),
                "Order Items".into(),
                0,
            )
            .await
            .unwrap();
            assert_eq!(odd.rows.len(), 2);
            let missing = rows(
                params.clone(),
                "shop".into(),
                "public".into(),
                "nope".into(),
                0,
            )
            .await;
            assert!(missing.unwrap_err().contains("no such table"));
            let mut bad = params.clone();
            bad.target.password = "wrong".into();
            assert!(test(bad).await.unwrap_err().contains("password"));
            // Read only, a time zone and a script that fails.
            let mut set = params.clone();
            set.target.read_only = true;
            set.target.time_zone = "Europe/Paris".into();
            set.target.startup_script = "SELECT 1".into();
            assert!(test(set.clone()).await.is_ok());
            set.target.startup_script = "SELEKT nope".into();
            assert!(test(set).await.unwrap_err().contains("startup script"));
            // TLS asked of a server that has none.
            let mut tls = params;
            tls.target.ssl.mode = SslMode::Require;
            assert!(test(tls).await.is_err());
        });
    }
}
