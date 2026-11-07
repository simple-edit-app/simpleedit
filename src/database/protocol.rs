//! What the app and the PostgreSQL driver tell each other.
//!
//! This file is compiled into both programs (the driver includes it with
//! `#[path]`), so the two can't drift apart. It must stay free of anything
//! else from the app.

use serde::{Deserialize, Serialize};

/// Bumped when a request or an answer changes; the app only uses a driver
/// that answers `--protocol` with the same number.
pub const PROTOCOL: u32 = 2;

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Schema {
    pub name: String,
    pub tables: Vec<Table>,
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
}

/// A page of a table; a value is `None` for NULL.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Page {
    pub columns: Vec<Column>,
    pub rows: Vec<Vec<Option<String>>>,
    pub has_more: bool,
    #[serde(default, skip_serializing)]
    pub offset: u64,
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
