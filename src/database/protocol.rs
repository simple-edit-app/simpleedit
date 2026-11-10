//! What the app and the PostgreSQL driver tell each other.
//!
//! This file is compiled into both programs (the driver includes it with
//! `#[path]`), so the two can't drift apart. It must stay free of anything
//! else from the app.

use serde::{Deserialize, Serialize};

/// Bumped when a request or an answer changes; the app only uses a driver
/// that answers `--protocol` with the same number.
pub const PROTOCOL: u32 = 19;

pub const DEFAULT_PORT: u16 = 5432;
pub const DEFAULT_MYSQL_PORT: u16 = 3306;
pub const DEFAULT_SSH_PORT: u16 = 22;

/// The database system a data source is on: each has its own driver.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Dialect {
    #[default]
    Postgres,
    Sqlite,
    /// MySQL and MariaDB.
    Mysql,
}

impl Dialect {
    pub const ALL: [Dialect; 3] = [Dialect::Postgres, Dialect::Mysql, Dialect::Sqlite];

    pub fn label(self) -> &'static str {
        match self {
            Dialect::Postgres => "PostgreSQL",
            Dialect::Sqlite => "SQLite",
            Dialect::Mysql => "MySQL / MariaDB",
        }
    }

    /// The name of its driver program.
    pub fn driver_name(self) -> &'static str {
        match self {
            Dialect::Postgres => "simpleedit-db-postgres",
            Dialect::Sqlite => "simpleedit-db-sqlite",
            Dialect::Mysql => "simpleedit-db-mysql",
        }
    }

    pub fn default_port(self) -> u16 {
        match self {
            Dialect::Postgres => DEFAULT_PORT,
            Dialect::Mysql => DEFAULT_MYSQL_PORT,
            Dialect::Sqlite => 0,
        }
    }

    /// A name between the quotes the system uses for them.
    pub fn quote(self, name: &str) -> String {
        match self {
            Dialect::Mysql => format!("`{}`", name.replace('`', "``")),
            _ => quote_ident(name),
        }
    }
}

/// How the user proves who they are to the database.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(clippy::enum_variant_names)]
pub enum Auth {
    #[default]
    UserPassword,
    NoAuth,
    /// The password is read from `~/.pgpass`.
    Pgpass,
}

impl Auth {
    pub const ALL: [Auth; 3] = [Auth::UserPassword, Auth::Pgpass, Auth::NoAuth];
}

/// Whether, and how strictly, the connection is encrypted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SslMode {
    #[default]
    Disable,
    /// Encrypted if the server can, plain otherwise.
    Prefer,
    /// Encrypted, whoever the server is.
    Require,
    /// Encrypted, and the server's certificate comes from the given CA.
    VerifyCa,
    /// As above, and the certificate is for the host asked for.
    VerifyFull,
}

impl SslMode {
    pub const ALL: [SslMode; 5] = [
        SslMode::Disable,
        SslMode::Prefer,
        SslMode::Require,
        SslMode::VerifyCa,
        SslMode::VerifyFull,
    ];
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Ssl {
    pub mode: SslMode,
    /// The authority that signed the server's certificate (PEM).
    pub ca_file: String,
    /// The client's certificate and key (PEM), if the server asks for one.
    pub client_cert: String,
    pub client_key: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SshAuth {
    #[default]
    Password,
    /// A private key file, with its passphrase if it has one.
    KeyPair,
    /// The keys of the running ssh agent and the user's OpenSSH config.
    Agent,
}

impl SshAuth {
    pub const ALL: [SshAuth; 3] = [SshAuth::Password, SshAuth::KeyPair, SshAuth::Agent];
}

/// A tunnel through a machine that can reach the database.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Ssh {
    pub enabled: bool,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub auth: SshAuth,
    pub key_file: String,
    /// The local port of the tunnel; empty to let the system choose.
    pub local_port: String,
}

impl Default for Ssh {
    fn default() -> Self {
        Ssh {
            enabled: false,
            host: String::new(),
            port: DEFAULT_SSH_PORT,
            user: String::new(),
            auth: SshAuth::default(),
            key_file: String::new(),
            local_port: String::new(),
        }
    }
}

/// Everything the driver needs to connect.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Target {
    pub dialect: Dialect,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub user: String,
    pub auth: Auth,
    pub password: String,
    pub ssl: Ssl,
    pub ssh: Ssh,
    /// The password of the ssh user, or the passphrase of the key.
    pub ssh_secret: String,
    /// The session can't change anything.
    pub read_only: bool,
    /// Run on every connection, before anything else.
    pub startup_script: String,
    pub time_zone: String,
    /// Further connection parameters (`application_name`, `options`, …).
    pub props: Vec<(String, String)>,
    /// The longest a statement may run, in seconds (0: no limit).
    pub query_timeout: u64,
}

/// What of the server the tree shows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Scope {
    /// Databases and schemas to introspect: `database:schema,schema`
    /// entries separated by `|`; `*` is all, `@` is the current one.
    pub pattern: String,
    /// `type:[-]regex` entries separated by `|` (types: table, view, mview,
    /// ftable); a `-` hides what matches.
    pub object_filter: String,
    pub system_schemas: bool,
    pub template_databases: bool,
    /// Only the database the connection names.
    pub single_database: bool,
}

pub const DEFAULT_PATTERN: &str = "@:*";

impl Default for Scope {
    fn default() -> Self {
        Scope {
            pattern: DEFAULT_PATTERN.to_string(),
            object_filter: String::new(),
            system_schemas: false,
            template_databases: false,
            single_database: false,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Request {
    /// `test`, `tree` or `rows`.
    pub op: String,
    pub target: Target,
    pub scope: Scope,
    /// For `rows`: the table and the page.
    pub database: String,
    pub schema: String,
    pub table: String,
    pub limit: u64,
    pub offset: u64,
    /// For `rows`: a condition, as it would follow `WHERE`.
    pub filter: String,
    /// For `rows`: the columns to sort by, in order.
    pub order: Vec<Sort>,
    /// For `apply`: what to change in the table, all or nothing.
    pub changes: Vec<Change>,
    /// For `query`: the statements to run, one after the other in one session.
    pub statements: Vec<String>,
    /// For `rows`: a query whose rows are wanted, in place of a table's.
    pub sql: String,
    /// For `query`: the statements are one transaction, all or none.
    pub transaction: bool,
    /// For `definition`: `view`, `matview` or `function` (its arguments in
    /// `sql`), named by `schema` and `table`.
    pub object: String,
    /// For `cancel`: the backend whose statement is stopped.
    pub pid: i64,
    /// For `export`: the file to write, and its format (`csv` or `json`).
    pub path: String,
    pub format: String,
    /// For `import`: how the file is read and where its values go.
    pub import: Option<ImportSpec>,
    /// For `dump` and `restore`: what the tool is asked for.
    pub tool: Option<ToolSpec>,
}

/// What `pg_dump`, `pg_restore` and `psql` are asked for.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ToolSpec {
    /// For a dump: `custom` (restored with `pg_restore`, compressed) or
    /// `plain` (SQL).
    pub format: String,
    pub schema_only: bool,
    pub data_only: bool,
    pub no_owner: bool,
    pub no_privileges: bool,
    /// For a restore: drop what is there first (`--clean --if-exists`).
    pub clean: bool,
    /// For a restore of SQL or of a custom file: all or none.
    pub single_transaction: bool,
}

/// A file of rows to put into a table.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ImportSpec {
    /// `csv` or `json`.
    pub format: String,
    /// The character between the fields of a CSV.
    pub delimiter: String,
    /// The first line of a CSV holds the names of the columns.
    pub header: bool,
    /// How many columns each line of the CSV has.
    pub file_columns: usize,
    /// An empty value is NULL, not an empty text.
    pub empty_null: bool,
    pub mapping: Vec<ImportColumn>,
}

/// Where a value of the file goes.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ImportColumn {
    /// The place of the field in a line of CSV.
    pub index: usize,
    /// The key in an object of JSON.
    pub key: String,
    /// The column of the table, and its type (to convert the text).
    pub column: String,
    pub kind: String,
}

/// Most columns the driver gives for the completion; as many means there
/// were more.
pub const MAX_COMPLETION_COLUMNS: usize = 100_000;

/// Most rows kept of the result of a statement.
pub const MAX_RESULT_ROWS: usize = 1000;

/// A column of a table or a view, for the completion of the console.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TableColumn {
    pub schema: String,
    pub table: String,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
}

/// A foreign key: the columns of a table that point to the columns of another.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForeignKey {
    pub schema: String,
    pub table: String,
    pub columns: Vec<String>,
    pub ref_schema: String,
    pub ref_table: String,
    pub ref_columns: Vec<String>,
}

/// The values a column can take, when the database says so: the labels of an
/// enumeration, or the keys of the table a foreign key points to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Choices {
    pub column: String,
    /// `enum` or `fk`.
    pub kind: String,
    pub values: Vec<String>,
    /// There were more than the list holds.
    #[serde(default)]
    pub more: bool,
}

/// Most values of a foreign key that are listed.
pub const MAX_CHOICES: usize = 200;

/// How a table of the database is doing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TableHealth {
    pub schema: String,
    pub name: String,
    /// The server's estimate of the rows.
    pub rows: i64,
    /// Bytes: the table alone, its indexes, and all of it with what is kept
    /// aside (TOAST).
    pub table_size: i64,
    pub indexes_size: i64,
    pub total_size: i64,
    /// Rows that were deleted or replaced and not yet cleaned.
    pub dead_rows: i64,
    /// `YYYY-MM-DD HH:MM`, empty when it never happened.
    pub last_vacuum: String,
    pub last_analyze: String,
}

/// An index, and how much it is used.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexHealth {
    pub schema: String,
    pub table: String,
    pub name: String,
    pub size: i64,
    /// How many times the server read through it since the statistics began.
    pub scans: i64,
    pub unique: bool,
    pub primary: bool,
}

impl IndexHealth {
    /// Never used, and not one that keeps the data right (a key or a unique
    /// index does that without being read).
    pub fn unused(&self) -> bool {
        self.scans == 0 && !self.unique && !self.primary
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Health {
    pub database_size: i64,
    pub tables: Vec<TableHealth>,
    pub indexes: Vec<IndexHealth>,
}

/// A size in bytes, as a person reads it.
pub fn human_size(bytes: i64) -> String {
    let mut value = bytes.max(0) as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < 4 {
        value /= 1024.0;
        unit += 1;
    }
    let units = ["B", "KB", "MB", "GB", "TB"];
    if unit == 0 {
        format!("{} B", bytes.max(0))
    } else {
        format!("{value:.1} {}", units[unit])
    }
}

/// What the completion knows of a database besides its tree.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Catalog {
    pub columns: Vec<TableColumn>,
    #[serde(default)]
    pub foreign_keys: Vec<ForeignKey>,
}

/// A column of a table, as the table designer shows and changes it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ColumnDef {
    pub name: String,
    /// The type as PostgreSQL writes it: `integer`, `numeric(8,2)`, `text[]`.
    #[serde(rename = "type")]
    pub kind: String,
    pub nullable: bool,
    /// The default expression, if there is one.
    pub default: Option<String>,
    /// `a` (always) or `d` (by default) for an identity column.
    #[serde(default)]
    pub identity: String,
    /// A generated column: its expression is the default.
    #[serde(default)]
    pub generated: bool,
    #[serde(default)]
    pub comment: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConstraintKind {
    Primary,
    Unique,
    Foreign,
    Check,
    /// Another one (an exclusion constraint…): shown, and dropped.
    Other,
}

/// A constraint of a table, as PostgreSQL writes it (`PRIMARY KEY (id)`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConstraintDef {
    pub name: String,
    pub kind: ConstraintKind,
    pub definition: String,
}

/// An index that is not the one of a constraint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexDef {
    pub name: String,
    /// The `CREATE INDEX` statement of it.
    pub definition: String,
    pub unique: bool,
}

/// What a table is made of.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Structure {
    pub columns: Vec<ColumnDef>,
    pub constraints: Vec<ConstraintDef>,
    pub indexes: Vec<IndexDef>,
    pub comment: String,
    /// The foreign keys of other tables that point to this one.
    #[serde(default)]
    pub referenced_by: Vec<Reference>,
    /// What only some tables have (boxed: the structure is carried in many
    /// messages).
    #[serde(default)]
    pub extra: Box<TableExtra>,
}

/// Partitioning, triggers and the like.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TableExtra {
    /// `RANGE (created)`, `LIST (country)`…: the table is partitioned by it.
    pub partition_key: String,
    /// The table is a partition of another one.
    pub partition_of: Option<PartitionOf>,
    /// The definitions of the triggers (`CREATE TRIGGER …`).
    pub triggers: Vec<String>,
    pub unlogged: bool,
}

/// The table a partition belongs to, and the values it holds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PartitionOf {
    pub schema: String,
    pub table: String,
    /// `FOR VALUES FROM (…) TO (…)`, or `DEFAULT`.
    pub bound: String,
}

/// A foreign key, in another table, that points to the table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reference {
    pub name: String,
    pub schema: String,
    pub table: String,
    pub definition: String,
}

/// A command the driver ran, for the log of the console.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Executed {
    pub sql: String,
    pub ms: u64,
    /// Rows read, or rows touched.
    pub rows: Option<u64>,
    pub error: Option<String>,
    /// The driver ran it to know the database (the tree, a table's shape),
    /// not because the user asked.
    #[serde(default)]
    pub internal: bool,
}

/// What a statement of a query gave.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueryResult {
    pub sql: String,
    pub ms: u64,
    /// The statement gave a set of rows (a SELECT), not just a count.
    pub has_rows: bool,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Option<String>>>,
    /// Rows touched by an INSERT, UPDATE or DELETE.
    pub affected: u64,
    /// There were more rows than `MAX_RESULT_ROWS`.
    pub truncated: bool,
    /// The statement gives rows and can be paged: they were not read, a
    /// `rows` request for its `sql` does.
    #[serde(default)]
    pub deferred: bool,
}

/// The table the rows of a query come from, when all of them do.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Source {
    pub schema: String,
    pub table: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sort {
    pub column: String,
    pub desc: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Update,
    Delete,
    /// A new row: `set` gives the columns that are not left to their default.
    Insert,
}

/// A row to change, found by its primary key.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Change {
    pub kind: ChangeKind,
    /// The primary key of the row as it was read: column and value.
    pub key: Vec<(String, String)>,
    /// For an update: the columns and what they become (`None` is NULL).
    pub set: Vec<(String, Option<String>)>,
    /// For a table without a primary key, the key is the row's `ctid`: the
    /// row must also still have all the values it was read with.
    #[serde(default)]
    pub original: Vec<(String, Option<String>)>,
}

// ------------------------------------------------------------ the answers

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Table {
    pub name: String,
    pub kind: Kind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Table,
    View,
    Matview,
    Foreign,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Schema {
    pub name: String,
    pub tables: Vec<Table>,
    /// Functions and procedures.
    #[serde(default)]
    pub routines: Vec<Routine>,
    #[serde(default)]
    pub sequences: Vec<String>,
    /// Enumerations, domains and composite types.
    #[serde(default)]
    pub types: Vec<String>,
}

/// A function or a procedure of a schema.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Routine {
    pub name: String,
    /// The types of its arguments, as `pg_get_function_identity_arguments` says.
    pub args: String,
    pub procedure: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Database {
    pub name: String,
    pub current: bool,
    pub schemas: Vec<Schema>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Column {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    /// The column accepts NULL.
    #[serde(default = "yes")]
    pub nullable: bool,
}

fn yes() -> bool {
    true
}

/// A page of a table; a value is `None` for NULL.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Page {
    pub columns: Vec<Column>,
    pub rows: Vec<Vec<Option<String>>>,
    pub has_more: bool,
    /// The columns of the primary key.
    #[serde(default)]
    pub primary_key: Vec<String>,
    /// For a table without a primary key: the `ctid` of each row, which
    /// finds it again (with its values).
    #[serde(default)]
    pub ctids: Vec<String>,
    /// Rows can be changed: a table with a primary key, on a session that
    /// is not read-only.
    #[serde(default)]
    pub editable: bool,
    /// For the rows of a query that come from one table: which.
    #[serde(default)]
    pub source: Option<Source>,
    /// The query can be sorted, filtered and paged (its rows were asked with
    /// `rows`); otherwise it gave all it has, at most `MAX_RESULT_ROWS`.
    #[serde(default, skip_serializing)]
    pub offset: u64,
}

// -------------------------------------------------------------- the display

/// A name between double quotes, quotes doubled.
pub fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// A text value as an SQL literal.
pub fn quote_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// A change as plain SQL with its values in place, for the log and the
/// preview (what is run takes them as parameters).
pub fn display_change(change: &Change, schema: &str, table: &str) -> String {
    let target = format!("{}.{}", quote_ident(schema), quote_ident(table));
    let mut key: Vec<String> = change
        .key
        .iter()
        .map(|(c, v)| format!("{} = {}", quote_ident(c), quote_literal(v)))
        .collect();
    // A row found by its place: and it must still be what was read.
    for (column, value) in &change.original {
        key.push(match value {
            Some(v) => format!(
                "{}::text IS NOT DISTINCT FROM {}",
                quote_ident(column),
                quote_literal(v)
            ),
            None => format!("{} IS NULL", quote_ident(column)),
        });
    }
    match change.kind {
        ChangeKind::Update => {
            let set: Vec<String> = change
                .set
                .iter()
                .map(|(c, v)| match v {
                    Some(v) => format!("{} = {}", quote_ident(c), quote_literal(v)),
                    None => format!("{} = NULL", quote_ident(c)),
                })
                .collect();
            format!(
                "UPDATE {target} SET {} WHERE {}",
                set.join(", "),
                key.join(" AND ")
            )
        }
        ChangeKind::Delete => format!("DELETE FROM {target} WHERE {}", key.join(" AND ")),
        ChangeKind::Insert if change.set.is_empty() => {
            format!("INSERT INTO {target} DEFAULT VALUES")
        }
        ChangeKind::Insert => {
            let columns: Vec<String> = change.set.iter().map(|(c, _)| quote_ident(c)).collect();
            let values: Vec<String> = change
                .set
                .iter()
                .map(|(_, v)| v.as_deref().map_or("NULL".to_string(), quote_literal))
                .collect();
            format!(
                "INSERT INTO {target} ({}) VALUES ({})",
                columns.join(", "),
                values.join(", ")
            )
        }
    }
}

// ------------------------------------------------------------------- sql

/// What a piece of SQL is made of, as far as the app and the driver need.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    /// A keyword or a name, without quotes.
    Word,
    /// A name between double quotes.
    Quoted,
    /// A string between single quotes, or a `$$` block.
    Text,
    Number,
    Comment,
    /// Anything else, one character: `(`, `,`, `.`, `;`, an operator…
    Punct,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub start: usize,
    pub end: usize,
}

/// Whether the quote at `at` opens an `E'…'` string (where `\` escapes).
fn is_escape_string(bytes: &[u8], at: usize) -> bool {
    at > 0
        && matches!(bytes[at - 1], b'e' | b'E')
        && (at < 2 || !(bytes[at - 2].is_ascii_alphanumeric() || bytes[at - 2] == b'_'))
}

/// Cuts SQL into tokens, in order, with their places (blanks are left out).
/// A string, a quoted name, a comment or a `$tag$` block is one token, even
/// if it holds a `;` or a keyword.
pub fn sql_tokens(sql: &str, dialect: Dialect) -> Vec<Token> {
    let bytes = sql.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;
    let word_char = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'$' || b >= 0x80;
    while i < bytes.len() {
        let start = i;
        let b = bytes[i];
        let kind = match b {
            b if b.is_ascii_whitespace() => {
                i += 1;
                continue;
            }
            b'\'' | b'"' | b'`' if b != b'`' || dialect != Dialect::Postgres => {
                // A backslash escapes the next character in MySQL's strings
                // and in PostgreSQL's `E'…'`.
                let escapes = match dialect {
                    Dialect::Mysql => b != b'`',
                    Dialect::Postgres => b == b'\'' && is_escape_string(bytes, start),
                    Dialect::Sqlite => false,
                };
                i += 1;
                while i < bytes.len() {
                    if escapes && bytes[i] == b'\\' {
                        i += 2;
                        continue;
                    }
                    if bytes[i] == b {
                        // A doubled quote is a quote in the text.
                        if bytes.get(i + 1) == Some(&b) {
                            i += 2;
                            continue;
                        }
                        i += 1;
                        break;
                    }
                    i += 1;
                }
                if b == b'\'' {
                    TokenKind::Text
                } else {
                    TokenKind::Quoted
                }
            }
            b'[' if dialect == Dialect::Sqlite => {
                while i < bytes.len() && bytes[i] != b']' {
                    i += 1;
                }
                i += 1;
                TokenKind::Quoted
            }
            b'#' if dialect == Dialect::Mysql => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                TokenKind::Comment
            }
            // MySQL wants a blank after the two dashes.
            b'-' if bytes.get(i + 1) == Some(&b'-')
                && (dialect != Dialect::Mysql
                    || bytes
                        .get(i + 2)
                        .is_none_or(|c| c.is_ascii_whitespace() || c.is_ascii_control())) =>
            {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                TokenKind::Comment
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                // They nest in PostgreSQL.
                let mut depth = 1;
                i += 2;
                while i < bytes.len() && depth > 0 {
                    if dialect == Dialect::Postgres
                        && bytes[i] == b'/'
                        && bytes.get(i + 1) == Some(&b'*')
                    {
                        depth += 1;
                        i += 2;
                    } else if bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/') {
                        depth -= 1;
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                TokenKind::Comment
            }
            b'$' if dialect == Dialect::Postgres => {
                // `$$` or `$tag$`, up to the same again; else a parameter.
                let tag_len = sql[i + 1..].find('$').filter(|n| {
                    sql[i + 1..i + 1 + n]
                        .chars()
                        .all(|c| c.is_alphanumeric() || c == '_')
                        && !sql[i + 1..i + 1 + n].starts_with(|c: char| c.is_ascii_digit())
                });
                match tag_len {
                    Some(n) => {
                        let tag = sql[i..i + n + 2].to_string();
                        i += n + 2;
                        match sql[i..].find(&tag) {
                            Some(close) => i += close + tag.len(),
                            None => i = bytes.len(),
                        }
                        TokenKind::Text
                    }
                    None => {
                        i += 1;
                        while i < bytes.len() && bytes[i].is_ascii_digit() {
                            i += 1;
                        }
                        TokenKind::Number
                    }
                }
            }
            b if b.is_ascii_digit() => {
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'.') {
                    i += 1;
                }
                TokenKind::Number
            }
            b if word_char(b) => {
                while i < bytes.len() && word_char(bytes[i]) {
                    i += 1;
                }
                TokenKind::Word
            }
            _ => {
                // One character, whole if it is more than a byte.
                i += 1;
                while i < bytes.len() && !sql.is_char_boundary(i) {
                    i += 1;
                }
                TokenKind::Punct
            }
        };
        tokens.push(Token {
            kind,
            start,
            end: i.min(bytes.len()),
        });
    }
    tokens
}

/// The keywords and names of a statement, in capitals, without what is in a
/// string, a quoted name or a comment.
pub fn sql_words(sql: &str, dialect: Dialect) -> Vec<String> {
    sql_tokens(sql, dialect)
        .into_iter()
        .filter(|t| t.kind == TokenKind::Word)
        .map(|t| sql[t.start..t.end].to_uppercase())
        .collect()
}

/// Why a statement is worth a second look before it runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Risk {
    /// An `UPDATE` or a `DELETE` with no `WHERE`: it touches every row.
    NoWhere,
    /// It changes data or structure, on a source marked as production.
    Writes,
}

/// What is risky in a statement. `production`: the source is one where every
/// change is asked about.
pub fn risk_of(sql: &str, production: bool, dialect: Dialect) -> Option<Risk> {
    let words = sql_words(sql, dialect);
    let first = words.first().map(String::as_str)?;
    // `EXPLAIN` without `ANALYZE` and the like do not run what they describe.
    if first == "EXPLAIN" && !words.iter().any(|w| w == "ANALYZE") {
        return None;
    }
    let touches_all = matches!(first, "UPDATE" | "DELETE")
        && !words.iter().any(|w| w == "WHERE" || w == "CURRENT");
    if touches_all {
        return Some(Risk::NoWhere);
    }
    // Controlling a transaction or the session is not a change of the data.
    let control = matches!(
        first,
        "BEGIN"
            | "START"
            | "COMMIT"
            | "END"
            | "ROLLBACK"
            | "SAVEPOINT"
            | "RELEASE"
            | "SET"
            | "RESET"
    );
    (production && !control && !reads_only(sql, dialect)).then_some(Risk::Writes)
}

/// A text as the inside of a `LIKE`: what it holds of `%`, `_` and `\` is
/// taken as itself.
fn like_text(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

/// A name between the quotes of `dialect`.
pub fn quote_ident_in(dialect: Dialect, name: &str) -> String {
    dialect.quote(name)
}

/// A text as a literal of `dialect`: MySQL also reads `\` as an escape.
pub fn quote_literal_in(dialect: Dialect, value: &str) -> String {
    match dialect {
        Dialect::Mysql => format!("'{}'", value.replace('\\', "\\\\").replace('\'', "''")),
        _ => quote_literal(value),
    }
}

/// `name` as text, for a search inside the values of any type.
fn as_text(dialect: Dialect, name: &str) -> String {
    match dialect {
        Dialect::Postgres => format!("{name}::text"),
        Dialect::Mysql => format!("CAST({name} AS CHAR)"),
        Dialect::Sqlite => format!("CAST({name} AS TEXT)"),
    }
}

/// `LIKE` of the dialect, with the `\` this code escapes with.
fn like_clause(dialect: Dialect, name: &str, pattern: &str) -> String {
    let pattern = quote_literal_in(dialect, pattern);
    match dialect {
        Dialect::Postgres => format!("{} ILIKE {pattern}", as_text(dialect, name)),
        Dialect::Mysql => format!("{} LIKE {pattern}", as_text(dialect, name)),
        // SQLite has no escape character unless told.
        Dialect::Sqlite => format!("{} LIKE {pattern} ESCAPE '\\'", as_text(dialect, name)),
    }
}

/// The condition a text typed under a column makes: `> 10`, `= abc`, `<> x`,
/// `~ regex`, `null`, `!null`; anything else is looked for inside the value,
/// whatever the case.
pub fn column_condition(dialect: Dialect, column: &str, text: &str) -> Option<String> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let name = quote_ident_in(dialect, column);
    match text.to_lowercase().as_str() {
        "null" | "is null" => return Some(format!("{name} IS NULL")),
        "!null" | "not null" | "is not null" => return Some(format!("{name} IS NOT NULL")),
        _ => {}
    }
    for (prefix, operator) in [
        (">=", ">="),
        ("<=", "<="),
        ("<>", "<>"),
        ("!=", "<>"),
        ("=", "="),
        (">", ">"),
        ("<", "<"),
    ] {
        if let Some(rest) = text.strip_prefix(prefix) {
            let value = rest.trim();
            if value.is_empty() {
                return None;
            }
            return Some(format!(
                "{name} {operator} {}",
                quote_literal_in(dialect, value)
            ));
        }
    }
    // SQLite has no regular expression operator of its own.
    if let (Some(rest), true) = (text.strip_prefix('~'), dialect != Dialect::Sqlite) {
        let pattern = rest.trim();
        if pattern.is_empty() {
            return None;
        }
        let operator = if dialect == Dialect::Mysql {
            "REGEXP"
        } else {
            "~"
        };
        return Some(format!(
            "{} {operator} {}",
            as_text(dialect, &name),
            quote_literal_in(dialect, pattern)
        ));
    }
    Some(like_clause(
        dialect,
        &name,
        &format!("%{}%", like_text(text)),
    ))
}

/// The condition that keeps the rows with a text in any of the columns.
pub fn search_condition(dialect: Dialect, columns: &[&str], text: &str) -> Option<String> {
    let text = text.trim();
    if text.is_empty() || columns.is_empty() {
        return None;
    }
    let pattern = format!("%{}%", like_text(text));
    let parts: Vec<String> = columns
        .iter()
        .map(|c| like_clause(dialect, &quote_ident_in(dialect, c), &pattern))
        .collect();
    Some(parts.join(" OR "))
}

/// Whether a statement may run on a read-only data source: it only reads. The
/// session is also read-only on the server's side, but a session can undo
/// that, so the statements that could are refused (transaction control, `SET`,
/// `RESET`, and anything that is not a plain reading statement).
pub fn reads_only(sql: &str, dialect: Dialect) -> bool {
    // MySQL runs what is in `/*! … */`: a comment that is code.
    if dialect == Dialect::Mysql && (sql.contains("/*!") || sql.contains("/*M!")) {
        return false;
    }
    let words = sql_words(sql, dialect);
    let Some(first) = words.first().map(String::as_str) else {
        return true;
    };
    match first {
        "SELECT" | "WITH" | "VALUES" | "TABLE" | "SHOW" => !words.iter().any(|w| {
            matches!(
                w.as_str(),
                "INSERT"
                    | "UPDATE"
                    | "DELETE"
                    | "MERGE"
                    | "INTO"
                    | "COPY"
                    | "SET_CONFIG"
                    | "COMMIT"
                    | "ROLLBACK"
                    | "BEGIN"
                    | "END"
            )
        }),
        // Plans are fine, but ANALYZE runs the statement.
        "EXPLAIN" => {
            !words.iter().any(|w| w == "ANALYZE" || w == "ANALYSE")
                && reads_only(&explained(sql, dialect), dialect)
        }
        _ => false,
    }
}

/// The statement an `EXPLAIN` is about.
fn explained(sql: &str, dialect: Dialect) -> String {
    let tokens = sql_tokens(sql, dialect);
    // Skip `EXPLAIN` and an option list in parentheses.
    let mut i = 1;
    if tokens.get(i).is_some_and(|t| &sql[t.start..t.end] == "(") {
        let mut depth = 0;
        while let Some(t) = tokens.get(i) {
            match &sql[t.start..t.end] {
                "(" => depth += 1,
                ")" => depth -= 1,
                _ => {}
            }
            i += 1;
            if depth == 0 {
                break;
            }
        }
    }
    tokens
        .get(i)
        .map(|t| sql[t.start..].to_string())
        .unwrap_or_default()
}

/// Whether the text holds several commands (a `;` followed by more than
/// comments): the server cannot prepare it, it can only run it.
pub fn is_multiple(sql: &str, dialect: Dialect) -> bool {
    let tokens = sql_tokens(sql, dialect);
    tokens.iter().enumerate().any(|(i, t)| {
        t.kind == TokenKind::Punct
            && &sql[t.start..t.end] == ";"
            && tokens[i + 1..]
                .iter()
                .any(|n| n.kind != TokenKind::Comment && &sql[n.start..n.end] != ";")
    })
}

/// Whether a condition typed in a filter holds a `;` of its own: it would
/// start another statement.
pub fn has_semicolon(sql: &str, dialect: Dialect) -> bool {
    sql_tokens(sql, dialect)
        .iter()
        .any(|t| t.kind == TokenKind::Punct && &sql[t.start..t.end] == ";")
}

/// Whether a statement can be the inside of `SELECT * FROM (…) AS q`, so that
/// its rows can be sorted, filtered and paged: it only reads (a SELECT, a
/// WITH that only reads, VALUES or TABLE).
pub fn wrappable(sql: &str, dialect: Dialect) -> bool {
    if dialect == Dialect::Mysql && (sql.contains("/*!") || sql.contains("/*M!")) {
        return false;
    }
    let words = sql_words(sql, dialect);
    if !matches!(
        words.first().map(String::as_str),
        Some("SELECT" | "WITH" | "VALUES" | "TABLE")
    ) {
        return false;
    }
    // Writes (also in a WITH), SELECT … INTO, FOR UPDATE, RETURNING, and
    // functions that change the state of the session.
    !words.iter().any(|w| {
        matches!(
            w.as_str(),
            "INSERT" | "UPDATE" | "DELETE" | "MERGE" | "INTO" | "RETURNING" | "COPY"
                // Functions that change state: running them again for each page
                // would change it again.
                | "NEXTVAL" | "SETVAL" | "SET_CONFIG"
        )
    })
}

/// The names of the functions a statement calls (a name followed by `(`), as
/// the server knows them: unquoted names in lower case, quoted ones as they
/// are. Keywords that take parentheses are left out.
pub fn called_functions(sql: &str, dialect: Dialect) -> Vec<String> {
    let tokens = sql_tokens(sql, dialect);
    let mut names: Vec<String> = Vec::new();
    for (i, t) in tokens.iter().enumerate() {
        let opens = tokens
            .get(i + 1)
            .is_some_and(|n| &sql[n.start..n.end] == "(");
        if !opens {
            continue;
        }
        let text = &sql[t.start..t.end];
        let name = match t.kind {
            TokenKind::Word => {
                let upper = text.to_uppercase();
                if matches!(
                    upper.as_str(),
                    "IN" | "EXISTS"
                        | "VALUES"
                        | "AS"
                        | "ANY"
                        | "ALL"
                        | "SOME"
                        | "OVER"
                        | "FILTER"
                        | "USING"
                        | "ON"
                        | "FROM"
                        | "JOIN"
                        | "AND"
                        | "OR"
                        | "NOT"
                        | "WHERE"
                        | "SELECT"
                        | "WITH"
                ) {
                    continue;
                }
                text.to_lowercase()
            }
            TokenKind::Quoted if text.len() >= 2 => {
                let quote = &text[..1];
                text[1..text.len() - 1].replace(&quote.repeat(2), quote)
            }
            _ => continue,
        };
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

/// Whether a reading statement of a batch may be left to the tab that pages
/// it, which runs it again for each page, sort and refresh: not when a later
/// statement of the batch writes (the rows must be read before it), and not
/// when it calls a function that may write (`may_write`, asked of the server).
pub fn deferrable(
    statements: &[String],
    index: usize,
    dialect: Dialect,
    may_write: impl FnOnce(&[String]) -> bool,
) -> bool {
    let sql = &statements[index];
    if !wrappable(sql, dialect) {
        return false;
    }
    if statements[index + 1..]
        .iter()
        .any(|later| !reads_only(later, dialect))
    {
        return false;
    }
    let names = called_functions(sql, dialect);
    names.is_empty() || !may_write(&names)
}

// ------------------------------------------------------------------- urls

/// The JDBC URL of a connection, as a JetBrains IDE shows it.
pub fn jdbc_url(host: &str, port: u16, database: &str) -> String {
    let database = database.trim();
    if database.is_empty() {
        format!("jdbc:postgresql://{host}:{port}/")
    } else {
        format!("jdbc:postgresql://{host}:{port}/{database}")
    }
}

/// What a URL says.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParsedUrl {
    pub host: String,
    pub port: u16,
    pub database: String,
    /// From `user:password@`, or from the `user` and `password` parameters.
    pub user: Option<String>,
    pub password: Option<String>,
    /// The other `?key=value` parameters.
    pub params: Vec<(String, String)>,
}

/// `%40` becomes `@`; what is not a valid escape stays as it is.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
            if let Some(byte) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The host, port, database, user and parameters of a
/// `jdbc:postgresql://…`, `postgresql://…` or `postgres://…` URL.
pub fn parse_url(url: &str) -> Option<ParsedUrl> {
    let url = url.trim();
    let rest = url
        .strip_prefix("jdbc:postgresql://")
        .or_else(|| url.strip_prefix("jdbc:postgres://"))
        .or_else(|| url.strip_prefix("postgresql://"))
        .or_else(|| url.strip_prefix("postgres://"))?;
    let (location, query) = match rest.split_once('?') {
        Some((location, query)) => (location, query),
        None => (rest, ""),
    };
    let (authority, database) = match location.split_once('/') {
        Some((authority, database)) => (authority, database),
        None => (location, ""),
    };
    let (userinfo, hostport) = match authority.rsplit_once('@') {
        Some((userinfo, hostport)) => (Some(userinfo), hostport),
        None => (None, authority),
    };
    let (mut user, mut password) = match userinfo {
        Some(info) => match info.split_once(':') {
            Some((u, p)) => (Some(percent_decode(u)), Some(percent_decode(p))),
            None => (Some(percent_decode(info)), None),
        },
        None => (None, None),
    };
    let (host, port) = if let Some(rest) = hostport.strip_prefix('[') {
        // [::1]:5432
        let (host, after) = rest.split_once(']')?;
        (host.to_string(), after.strip_prefix(':').unwrap_or(""))
    } else {
        match hostport.rsplit_once(':') {
            Some((host, port)) => (host.to_string(), port),
            None => (hostport.to_string(), ""),
        }
    };
    if host.is_empty() {
        return None;
    }
    let port = if port.is_empty() {
        DEFAULT_PORT
    } else {
        port.parse().ok().filter(|p| *p > 0)?
    };
    let mut params = Vec::new();
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let (key, value) = match pair.split_once('=') {
            Some((k, v)) => (percent_decode(k), percent_decode(v)),
            None => (percent_decode(pair), String::new()),
        };
        // The JDBC way to name the user.
        match key.as_str() {
            "user" if user.is_none() => user = Some(value),
            "password" if password.is_none() => password = Some(value),
            _ => params.push((key, value)),
        }
    }
    Some(ParsedUrl {
        host,
        port,
        database: percent_decode(database),
        user,
        password,
        params,
    })
}

/// The SSL mode a `sslmode` (or JDBC `ssl`) parameter asks for.
pub fn ssl_mode_of(key: &str, value: &str) -> Option<SslMode> {
    match (key, value) {
        ("sslmode", "disable") => Some(SslMode::Disable),
        ("sslmode", "allow" | "prefer") => Some(SslMode::Prefer),
        ("sslmode", "require") => Some(SslMode::Require),
        ("sslmode", "verify-ca") => Some(SslMode::VerifyCa),
        ("sslmode", "verify-full") => Some(SslMode::VerifyFull),
        ("ssl", "true") => Some(SslMode::Require),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_change_is_shown_as_plain_sql() {
        let update = Change {
            kind: ChangeKind::Update,
            original: Vec::new(),
            key: vec![("id".into(), "7".into())],
            set: vec![("name".into(), Some("it's".into())), ("due".into(), None)],
        };
        assert_eq!(
            display_change(&update, "public", "Order Items"),
            "UPDATE \"public\".\"Order Items\" SET \"name\" = 'it''s', \"due\" = NULL WHERE \"id\" = '7'"
        );
        let delete = Change {
            kind: ChangeKind::Delete,
            original: Vec::new(),
            key: vec![("a".into(), "1".into()), ("b".into(), "x".into())],
            set: vec![],
        };
        assert_eq!(
            display_change(&delete, "s", "t"),
            "DELETE FROM \"s\".\"t\" WHERE \"a\" = '1' AND \"b\" = 'x'"
        );
    }

    #[test]
    fn a_new_row_is_shown_as_an_insert() {
        let insert = Change {
            kind: ChangeKind::Insert,
            original: Vec::new(),
            key: vec![],
            set: vec![("label".into(), Some("it's".into())), ("due".into(), None)],
        };
        assert_eq!(
            display_change(&insert, "public", "items"),
            "INSERT INTO \"public\".\"items\" (\"label\", \"due\") VALUES ('it''s', NULL)"
        );
        let empty = Change {
            kind: ChangeKind::Insert,
            original: Vec::new(),
            key: vec![],
            set: vec![],
        };
        assert_eq!(
            display_change(&empty, "s", "t"),
            "INSERT INTO \"s\".\"t\" DEFAULT VALUES"
        );
    }

    #[test]
    fn sql_is_cut_into_tokens() {
        let sql = "SELECT 'a;b', \"Col\" -- note\nFROM t WHERE x = $1 AND y > 2.5";
        let kinds: Vec<(TokenKind, &str)> = sql_tokens(sql, Dialect::Postgres)
            .into_iter()
            .map(|t| (t.kind, &sql[t.start..t.end]))
            .collect();
        assert_eq!(kinds[0], (TokenKind::Word, "SELECT"));
        assert_eq!(kinds[1], (TokenKind::Text, "'a;b'"));
        assert_eq!(kinds[2], (TokenKind::Punct, ","));
        assert_eq!(kinds[3], (TokenKind::Quoted, "\"Col\""));
        assert_eq!(kinds[4], (TokenKind::Comment, "-- note"));
        assert!(kinds.contains(&(TokenKind::Number, "$1")));
        assert!(kinds.contains(&(TokenKind::Number, "2.5")));
        // A dollar block is one piece, with what is inside.
        let block = "DO $$ BEGIN PERFORM 1; END $$";
        assert_eq!(sql_words(block, Dialect::Postgres), vec!["DO"]);
        assert_eq!(
            sql_words("select a.b from \"X\" /* from */", Dialect::Postgres),
            vec!["SELECT", "A", "B", "FROM"]
        );
    }

    #[test]
    fn a_reading_statement_is_deferred_only_when_nothing_after_it_writes() {
        let batch = |sqls: &[&str]| sqls.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let never = |_: &[String]| false;
        // Read later, the rows would be the ones after the UPDATE.
        let statements = batch(&[
            "SELECT * FROM jobs WHERE status = 'pending'",
            "UPDATE jobs SET status = 'done' WHERE status = 'pending'",
        ]);
        assert!(!deferrable(&statements, 0, Dialect::Postgres, never));
        // The last one, or one followed by reads only, is paged.
        assert!(deferrable(&statements[..1], 0, Dialect::Postgres, never));
        let statements = batch(&["SELECT 1", "SELECT 2"]);
        assert!(deferrable(&statements, 0, Dialect::Mysql, never));
        // A function the server says may write is run once, at once.
        let statements = batch(&["SELECT \"Log\"(id), count(*) FROM t WHERE id IN (1)"]);
        let mut asked = Vec::new();
        assert!(!deferrable(&statements, 0, Dialect::Postgres, |names| {
            asked = names.to_vec();
            true
        }));
        assert_eq!(asked, ["Log", "count"]);
        // No call, no question.
        let statements = batch(&["SELECT * FROM t"]);
        assert!(deferrable(&statements, 0, Dialect::Sqlite, |_| panic!()));
    }

    #[test]
    fn only_a_reading_statement_can_be_wrapped() {
        assert!(wrappable(
            "SELECT * FROM t WHERE a = 'delete'",
            Dialect::Postgres
        ));
        assert!(wrappable(
            "with x as (select 1) select * from x",
            Dialect::Postgres
        ));
        assert!(wrappable("VALUES (1), (2)", Dialect::Postgres));
        assert!(wrappable("TABLE t", Dialect::Postgres));
        assert!(!wrappable("SELECT * INTO u FROM t", Dialect::Postgres));
        assert!(!wrappable("SELECT * FROM t FOR UPDATE", Dialect::Postgres));
        assert!(!wrappable(
            "WITH d AS (DELETE FROM t RETURNING *) SELECT * FROM d",
            Dialect::Postgres,
        ));
        assert!(!wrappable(
            "INSERT INTO t VALUES (1) RETURNING id",
            Dialect::Postgres
        ));
        assert!(!wrappable("EXPLAIN SELECT 1", Dialect::Postgres));
        assert!(!wrappable("SHOW search_path", Dialect::Postgres));
        assert!(!wrappable("", Dialect::Postgres));
        assert!(!wrappable("SELECT nextval('s')", Dialect::Postgres));
    }

    #[test]
    fn a_read_only_source_refuses_what_could_write_or_undo_it() {
        assert!(reads_only("SELECT 1", Dialect::Postgres));
        assert!(reads_only(
            "  with x as (select 1) select * from x",
            Dialect::Postgres
        ));
        assert!(reads_only("SHOW search_path", Dialect::Postgres));
        assert!(reads_only("EXPLAIN SELECT 1", Dialect::Postgres));
        assert!(reads_only(
            "EXPLAIN (COSTS off) SELECT 1",
            Dialect::Postgres
        ));
        assert!(!reads_only(
            "EXPLAIN ANALYZE DELETE FROM t",
            Dialect::Postgres
        ));
        assert!(!reads_only("EXPLAIN (ANALYZE) SELECT 1", Dialect::Postgres));
        assert!(!reads_only(
            "SET default_transaction_read_only = off",
            Dialect::Postgres
        ));
        assert!(!reads_only("BEGIN READ WRITE", Dialect::Postgres));
        assert!(!reads_only("COMMIT", Dialect::Postgres));
        assert!(!reads_only("DELETE FROM t", Dialect::Postgres));
        assert!(!reads_only("SELECT * INTO u FROM t", Dialect::Postgres));
        assert!(!reads_only(
            "SELECT set_config('default_transaction_read_only','off',false)",
            Dialect::Postgres,
        ));
        assert!(!reads_only("CREATE TABLE t()", Dialect::Postgres));
    }

    #[test]
    fn several_commands_are_told_from_one() {
        assert!(is_multiple("DROP TABLE a; DROP TABLE b", Dialect::Postgres));
        assert!(!is_multiple("SELECT 1;", Dialect::Postgres));
        assert!(!is_multiple("SELECT 'a;b' -- x; y", Dialect::Postgres));
        assert!(!is_multiple("SELECT 1; -- done", Dialect::Postgres));
    }

    #[test]
    fn a_semicolon_outside_a_string_is_found() {
        assert!(has_semicolon(
            "a = 1); DELETE FROM t; --",
            Dialect::Postgres
        ));
        assert!(!has_semicolon(
            "a = 'x;y' AND b = \"c;d\"",
            Dialect::Postgres
        ));
    }

    #[test]
    fn a_url_is_taken_apart() {
        let u = parse_url("jdbc:postgresql://127.0.0.1:5433/myDatabase?ssl=true&x").unwrap();
        assert_eq!(
            (u.host.as_str(), u.port, u.database.as_str()),
            ("127.0.0.1", 5433, "myDatabase")
        );
        assert_eq!(u.user, None);
        assert_eq!(
            u.params,
            vec![("ssl".into(), "true".into()), ("x".into(), "".into())]
        );
        assert_eq!(
            parse_url("jdbc:postgresql://[::1]:6000/").unwrap().host,
            "::1"
        );
        assert_eq!(parse_url("mysql://h/db"), None);
        assert_eq!(parse_url("jdbc:postgresql://h:abc/db"), None);
        assert_eq!(parse_url("postgresql:///db"), None);
    }

    #[test]
    fn a_url_with_a_user_gives_it_with_its_password() {
        let u = parse_url(
            "postgresql://neondb_owner:p%40ss%2Fw@ep-x-pooler.c-3.eu-central-1.aws.neon.tech/neondb?sslmode=require&channel_binding=require",
        )
        .unwrap();
        assert_eq!(u.host, "ep-x-pooler.c-3.eu-central-1.aws.neon.tech");
        assert_eq!(u.port, 5432);
        assert_eq!(u.database, "neondb");
        assert_eq!(u.user.as_deref(), Some("neondb_owner"));
        assert_eq!(u.password.as_deref(), Some("p@ss/w"));
        assert_eq!(
            u.params,
            vec![
                ("sslmode".into(), "require".into()),
                ("channel_binding".into(), "require".into())
            ]
        );
        // A user without a password, and the JDBC way.
        assert_eq!(parse_url("postgres://me@h/d").unwrap().password, None);
        let j = parse_url("jdbc:postgresql://h/d?user=me&password=pw&x=1").unwrap();
        assert_eq!(
            (j.user.as_deref(), j.password.as_deref()),
            (Some("me"), Some("pw"))
        );
        assert_eq!(j.params, vec![("x".into(), "1".into())]);
    }

    #[test]
    fn the_ssl_parameters_name_a_mode() {
        assert_eq!(ssl_mode_of("sslmode", "require"), Some(SslMode::Require));
        assert_eq!(
            ssl_mode_of("sslmode", "verify-full"),
            Some(SslMode::VerifyFull)
        );
        assert_eq!(ssl_mode_of("sslmode", "allow"), Some(SslMode::Prefer));
        assert_eq!(ssl_mode_of("ssl", "true"), Some(SslMode::Require));
        assert_eq!(ssl_mode_of("application_name", "x"), None);
    }

    #[test]
    fn a_url_is_built_from_its_parts() {
        assert_eq!(
            jdbc_url("localhost", 5432, "shop"),
            "jdbc:postgresql://localhost:5432/shop"
        );
        assert_eq!(jdbc_url("h", 1, " "), "jdbc:postgresql://h:1/");
    }

    #[test]
    fn an_update_or_a_delete_without_where_is_risky() {
        assert_eq!(
            risk_of("UPDATE t SET a = 1", false, Dialect::Postgres),
            Some(Risk::NoWhere)
        );
        assert_eq!(
            risk_of("delete from t", false, Dialect::Postgres),
            Some(Risk::NoWhere)
        );
        assert_eq!(
            risk_of("DELETE FROM t WHERE id = 1", false, Dialect::Postgres),
            None
        );
        // A `WHERE` in a string or a comment is not one.
        assert_eq!(
            risk_of("DELETE FROM t -- WHERE id = 1", false, Dialect::Postgres),
            Some(Risk::NoWhere)
        );
        assert_eq!(risk_of("SELECT * FROM t", false, Dialect::Postgres), None);
        assert_eq!(
            risk_of("EXPLAIN DELETE FROM t", false, Dialect::Postgres),
            None
        );
    }

    #[test]
    fn on_a_production_source_every_change_is_risky() {
        assert_eq!(
            risk_of("INSERT INTO t VALUES (1)", true, Dialect::Postgres),
            Some(Risk::Writes)
        );
        assert_eq!(
            risk_of("DROP TABLE t", true, Dialect::Postgres),
            Some(Risk::Writes)
        );
        assert_eq!(
            risk_of("UPDATE t SET a = 1 WHERE b", true, Dialect::Postgres),
            Some(Risk::Writes)
        );
        assert_eq!(risk_of("SELECT 1", true, Dialect::Postgres), None);
        assert_eq!(risk_of("BEGIN", true, Dialect::Postgres), None);
        assert_eq!(
            risk_of("SET search_path = a", true, Dialect::Postgres),
            None
        );
        assert_eq!(
            risk_of("INSERT INTO t VALUES (1)", false, Dialect::Postgres),
            None
        );
    }

    #[test]
    fn a_column_filter_reads_an_operator_or_looks_inside() {
        let c = |t: &str| column_condition(Dialect::Postgres, "age", t);
        assert_eq!(c(""), None);
        assert_eq!(c("  "), None);
        assert_eq!(c("> 30").as_deref(), Some("\"age\" > '30'"));
        assert_eq!(c(">=30").as_deref(), Some("\"age\" >= '30'"));
        assert_eq!(c("!= 5").as_deref(), Some("\"age\" <> '5'"));
        assert_eq!(c("= it's").as_deref(), Some("\"age\" = 'it''s'"));
        assert_eq!(c(">"), None);
        assert_eq!(c("NULL").as_deref(), Some("\"age\" IS NULL"));
        assert_eq!(c("!null").as_deref(), Some("\"age\" IS NOT NULL"));
        assert_eq!(c("~ ^a.*z$").as_deref(), Some("\"age\"::text ~ '^a.*z$'"));
        assert_eq!(c("ab").as_deref(), Some("\"age\"::text ILIKE '%ab%'"));
        // What `LIKE` reads as a joker is taken as itself.
        assert_eq!(
            c("50%_").as_deref(),
            Some("\"age\"::text ILIKE '%50\\%\\_%'")
        );
        // A column with a quote in its name stays one name.
        assert_eq!(
            column_condition(Dialect::Postgres, "a\"b", "x").as_deref(),
            Some("\"a\"\"b\"::text ILIKE '%x%'")
        );
    }

    #[test]
    fn a_search_looks_in_every_column() {
        assert_eq!(search_condition(Dialect::Postgres, &["a", "b"], ""), None);
        assert_eq!(search_condition(Dialect::Postgres, &[], "x"), None);
        assert_eq!(
            search_condition(Dialect::Postgres, &["a", "b c"], " Foo ").as_deref(),
            Some("\"a\"::text ILIKE '%Foo%' OR \"b c\"::text ILIKE '%Foo%'")
        );
    }

    #[test]
    fn a_size_is_read_in_the_unit_that_fits() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(1023), "1023 B");
        assert_eq!(human_size(1536), "1.5 KB");
        assert_eq!(human_size(5 * 1024 * 1024), "5.0 MB");
        assert_eq!(human_size(-4), "0 B");
    }

    #[test]
    fn an_index_that_keeps_the_data_right_is_never_unused() {
        let index = |scans, unique, primary| IndexHealth {
            schema: "public".into(),
            table: "t".into(),
            name: "i".into(),
            size: 8192,
            scans,
            unique,
            primary,
        };
        assert!(index(0, false, false).unused());
        assert!(!index(3, false, false).unused());
        assert!(!index(0, true, false).unused());
        assert!(!index(0, false, true).unused());
    }

    #[test]
    fn mysql_reads_its_backslashes_its_hash_comments_and_its_backticks() {
        let my = Dialect::Mysql;
        // `\'` does not end the string: the `;` is inside it.
        assert!(!is_multiple("SELECT 'it\\'s; fine'", my));
        assert!(is_multiple("SELECT 'it\\'s; fine'", Dialect::Postgres));
        // A hash comment hides what follows it, a backtick name is one piece.
        assert_eq!(sql_words("SELECT 1 # DELETE FROM t", my), vec!["SELECT"]);
        assert_eq!(sql_words("SELECT `from;x`", my), vec!["SELECT"]);
        // The trick that would pass a write for a read.
        assert!(reads_only("SELECT 1", my));
        assert!(
            !reads_only("SELECT 1; DELETE FROM t", my)
                || is_multiple("SELECT 1; DELETE FROM t", my)
        );
        assert!(!reads_only("SELECT 1 /*! INTO OUTFILE '/tmp/x' */", my));
        assert_eq!(risk_of("DELETE FROM t", false, my), Some(Risk::NoWhere));
        assert_eq!(risk_of("DELETE FROM t -- c\n WHERE a = 1", false, my), None);
        // Two dashes without a blank are not a comment in MySQL.
        assert_eq!(sql_words("SELECT 1--1", my), vec!["SELECT"]);
        assert!(!has_semicolon("a = 'x\\'; y'", my));
    }

    #[test]
    fn the_filters_speak_the_sql_of_their_connection() {
        let my = Dialect::Mysql;
        assert_eq!(
            column_condition(my, "age", "> 30").as_deref(),
            Some("`age` > '30'")
        );
        assert_eq!(
            column_condition(my, "name", "o'b\\").as_deref(),
            Some("CAST(`name` AS CHAR) LIKE '%o''b\\\\\\\\%'")
        );
        assert_eq!(
            column_condition(my, "a", "~ ^x").as_deref(),
            Some("CAST(`a` AS CHAR) REGEXP '^x'")
        );
        assert_eq!(
            search_condition(Dialect::Sqlite, &["a"], "50%").as_deref(),
            Some("CAST(\"a\" AS TEXT) LIKE '%50\\%%' ESCAPE '\\'")
        );
        // No regular expression operator in SQLite: it is a text search.
        assert!(column_condition(Dialect::Sqlite, "a", "~ x")
            .unwrap()
            .contains("LIKE"));
    }
}
