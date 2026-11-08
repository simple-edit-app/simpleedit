//! What the app and the PostgreSQL driver tell each other.
//!
//! This file is compiled into both programs (the driver includes it with
//! `#[path]`), so the two can't drift apart. It must stay free of anything
//! else from the app.

use serde::{Deserialize, Serialize};

/// Bumped when a request or an answer changes; the app only uses a driver
/// that answers `--protocol` with the same number.
pub const PROTOCOL: u32 = 10;

pub const DEFAULT_PORT: u16 = 5432;
pub const DEFAULT_SSH_PORT: u16 = 22;

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
}

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

/// Cuts SQL into tokens, in order, with their places (blanks are left out).
/// A string, a quoted name, a comment or a `$tag$` block is one token, even
/// if it holds a `;` or a keyword.
pub fn sql_tokens(sql: &str) -> Vec<Token> {
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
            b'\'' | b'"' => {
                i += 1;
                while i < bytes.len() {
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
                if b == b'"' {
                    TokenKind::Quoted
                } else {
                    TokenKind::Text
                }
            }
            b'-' if bytes.get(i + 1) == Some(&b'-') => {
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
                    if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
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
            b'$' => {
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
pub fn sql_words(sql: &str) -> Vec<String> {
    sql_tokens(sql)
        .into_iter()
        .filter(|t| t.kind == TokenKind::Word)
        .map(|t| sql[t.start..t.end].to_uppercase())
        .collect()
}

/// Whether a statement can be the inside of `SELECT * FROM (…) AS q`, so that
/// its rows can be sorted, filtered and paged: it only reads (a SELECT, a
/// WITH that only reads, VALUES or TABLE).
pub fn wrappable(sql: &str) -> bool {
    let words = sql_words(sql);
    if !matches!(
        words.first().map(String::as_str),
        Some("SELECT" | "WITH" | "VALUES" | "TABLE")
    ) {
        return false;
    }
    // Writes (also in a WITH), SELECT … INTO, FOR UPDATE, RETURNING.
    !words.iter().any(|w| {
        matches!(
            w.as_str(),
            "INSERT" | "UPDATE" | "DELETE" | "MERGE" | "INTO" | "RETURNING" | "COPY"
        )
    })
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
        let kinds: Vec<(TokenKind, &str)> = sql_tokens(sql)
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
        assert_eq!(sql_words(block), vec!["DO"]);
        assert_eq!(
            sql_words("select a.b from \"X\" /* from */"),
            vec!["SELECT", "A", "B", "FROM"]
        );
    }

    #[test]
    fn only_a_reading_statement_can_be_wrapped() {
        assert!(wrappable("SELECT * FROM t WHERE a = 'delete'"));
        assert!(wrappable("with x as (select 1) select * from x"));
        assert!(wrappable("VALUES (1), (2)"));
        assert!(wrappable("TABLE t"));
        assert!(!wrappable("SELECT * INTO u FROM t"));
        assert!(!wrappable("SELECT * FROM t FOR UPDATE"));
        assert!(!wrappable(
            "WITH d AS (DELETE FROM t RETURNING *) SELECT * FROM d"
        ));
        assert!(!wrappable("INSERT INTO t VALUES (1) RETURNING id"));
        assert!(!wrappable("EXPLAIN SELECT 1"));
        assert!(!wrappable("SHOW search_path"));
        assert!(!wrappable(""));
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
}
