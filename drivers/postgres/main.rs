//! The PostgreSQL driver of SimpleEdit's Database panel.
//!
//! A separate program, downloaded on demand by the app (like the JDBC
//! drivers of a JetBrains IDE). The app starts it once per operation and
//! talks to it in JSON: one request on its standard input, one answer line
//! on its standard output. Passwords travel on the standard input only,
//! never on the command line.
//!
//! Operations: `test` (connect and say the server version), `tree` (the
//! databases, schemas and tables the scope selects) and `rows` (a page of a
//! table, every value as text, NULL as `null`).

// The app uses parts of the file that the driver does not.
#[allow(dead_code)]
#[path = "../../src/database/protocol.rs"]
mod protocol;

use std::io::Read;
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use postgres::config::SslMode as PgSslMode;
use postgres::{Client, Config, NoTls};
use regex::Regex;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::client::WebPkiServerVerifier;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime};
use rustls::{CertificateError, DigitallySignedStruct, RootCertStore, SignatureScheme};
use serde_json::{json, Value};
use tokio_postgres_rustls::MakeRustlsConnect;

use protocol::{
    Auth, Column, Database, Kind, Request, Schema, Scope, SshAuth, SslMode, Table, Target, PROTOCOL,
};

fn main() {
    if std::env::args().nth(1).as_deref() == Some("--protocol") {
        println!("{PROTOCOL}");
        return;
    }
    let mut input = String::new();
    let answer = match std::io::stdin().read_to_string(&mut input) {
        Ok(_) => run(&input),
        Err(e) => Err(e.to_string()),
    };
    let line = match answer {
        Ok(mut value) => {
            value["ok"] = json!(true);
            value
        }
        Err(error) => json!({ "ok": false, "error": error }),
    };
    println!("{line}");
}

fn run(input: &str) -> Result<Value, String> {
    let request: Request = serde_json::from_str(input).map_err(|e| e.to_string())?;
    let session = Session::open(&request.target, &request.database)?;
    match request.op.as_str() {
        "test" => {
            let mut session = session;
            let row = session
                .client
                .query_one("SELECT version()", &[])
                .map_err(clean)?;
            Ok(json!({ "version": row.get::<_, String>(0) }))
        }
        "tree" => tree(session, &request),
        "rows" => {
            let mut session = session;
            rows(
                &mut session.client,
                &request.schema,
                &request.table,
                request.limit.max(1),
                request.offset,
            )
        }
        other => Err(format!("unknown operation: {other}")),
    }
}

// -------------------------------------------------------------- connection

/// A connection, and the tunnel it goes through, if any.
struct Session {
    client: Client,
    _tunnel: Option<Tunnel>,
}

impl Session {
    /// Connects to `database` (the target's own when empty).
    fn open(target: &Target, database: &str) -> Result<Session, String> {
        let password = match target.auth {
            Auth::UserPassword => Some(target.password.clone()).filter(|p| !p.is_empty()),
            Auth::NoAuth => None,
            Auth::Pgpass => pgpass(
                &target.host,
                target.port,
                if database.is_empty() {
                    &target.database
                } else {
                    database
                },
                &target.user,
            ),
        };
        let (host, port, tunnel) = if target.ssh.enabled {
            let tunnel = Tunnel::open(target)?;
            ("127.0.0.1".to_string(), tunnel.port, Some(tunnel))
        } else {
            (target.host.clone(), target.port, None)
        };
        let mut config = config(target, &host, port, password.as_deref(), database)?;
        let mut client = connect(&mut config, target)?;
        // The session is set up the way the connection asks.
        if target.read_only {
            client
                .batch_execute("SET default_transaction_read_only = on")
                .map_err(clean)?;
        }
        if !target.time_zone.trim().is_empty() {
            client
                .batch_execute(&format!(
                    "SET TIME ZONE '{}'",
                    target.time_zone.trim().replace('\'', "''")
                ))
                .map_err(clean)?;
        }
        if !target.startup_script.trim().is_empty() {
            client
                .batch_execute(&target.startup_script)
                .map_err(|e| format!("startup script: {}", clean(e)))?;
        }
        Ok(Session {
            client,
            _tunnel: tunnel,
        })
    }
}

/// The text of a value in a libpq connection string.
fn conninfo_value(value: &str) -> String {
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"))
}

fn config(
    target: &Target,
    host: &str,
    port: u16,
    password: Option<&str>,
    database: &str,
) -> Result<Config, String> {
    let mut info = String::new();
    let mut add = |key: &str, value: &str| {
        info.push_str(&format!("{key}={} ", conninfo_value(value)));
    };
    add("host", host);
    add("port", &port.to_string());
    if !target.user.trim().is_empty() {
        add("user", target.user.trim());
    }
    let database = if database.is_empty() {
        target.database.trim()
    } else {
        database
    };
    if !database.is_empty() {
        add("dbname", database);
    }
    if let Some(password) = password {
        add("password", password);
    }
    if !target.props.iter().any(|(k, _)| k == "connect_timeout") {
        add("connect_timeout", "10");
    }
    for (key, value) in &target.props {
        if !key.trim().is_empty() {
            add(key.trim(), value);
        }
    }
    info.parse::<Config>().map_err(|e| e.to_string())
}

fn connect(config: &mut Config, target: &Target) -> Result<Client, String> {
    match target.ssl.mode {
        SslMode::Disable => {
            config.ssl_mode(PgSslMode::Disable);
            config.connect(NoTls).map_err(clean)
        }
        mode => {
            config.ssl_mode(if mode == SslMode::Prefer {
                PgSslMode::Prefer
            } else {
                PgSslMode::Require
            });
            let tls = tls_config(target)?;
            config.connect(MakeRustlsConnect::new(tls)).map_err(clean)
        }
    }
}

// --------------------------------------------------------------------- TLS

/// Accepts any certificate: encryption without knowing who is there.
#[derive(Debug)]
struct AcceptAny(Arc<rustls::crypto::CryptoProvider>);

impl ServerCertVerifier for AcceptAny {
    fn verify_server_cert(
        &self,
        _: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

/// The chain must lead to a trusted authority; the name is not looked at.
#[derive(Debug)]
struct ChainOnly(Arc<WebPkiServerVerifier>);

impl ServerCertVerifier for ChainOnly {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        match self
            .0
            .verify_server_cert(end_entity, intermediates, server_name, ocsp, now)
        {
            Err(rustls::Error::InvalidCertificate(CertificateError::NotValidForName))
            | Err(rustls::Error::InvalidCertificate(CertificateError::NotValidForNameContext {
                ..
            })) => Ok(ServerCertVerified::assertion()),
            other => other,
        }
    }
    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.0.verify_tls12_signature(message, cert, dss)
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.0.verify_tls13_signature(message, cert, dss)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.supported_verify_schemes()
    }
}

fn certificates(path: &str) -> Result<Vec<CertificateDer<'static>>, String> {
    CertificateDer::pem_file_iter(path)
        .map_err(|e| format!("{path}: {e}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("{path}: {e}"))
}

fn tls_config(target: &Target) -> Result<rustls::ClientConfig, String> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let builder = rustls::ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?;
    let ssl = &target.ssl;
    // The authorities: the given file, or the system's.
    let roots = || -> Result<RootCertStore, String> {
        let mut roots = RootCertStore::empty();
        if ssl.ca_file.trim().is_empty() {
            for cert in rustls_native_certs::load_native_certs().certs {
                let _ = roots.add(cert);
            }
        } else {
            for cert in certificates(ssl.ca_file.trim())? {
                roots.add(cert).map_err(|e| e.to_string())?;
            }
        }
        Ok(roots)
    };
    let builder = match ssl.mode {
        SslMode::VerifyFull => builder.with_root_certificates(roots()?),
        SslMode::VerifyCa => {
            let inner = WebPkiServerVerifier::builder_with_provider(Arc::new(roots()?), provider)
                .build()
                .map_err(|e| e.to_string())?;
            builder
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(ChainOnly(inner)))
        }
        _ => builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(AcceptAny(provider))),
    };
    if ssl.client_cert.trim().is_empty() {
        return Ok(builder.with_no_client_auth());
    }
    let chain = certificates(ssl.client_cert.trim())?;
    let key = PrivateKeyDer::from_pem_file(ssl.client_key.trim())
        .map_err(|e| format!("{}: {e}", ssl.client_key.trim()))?;
    builder
        .with_client_auth_cert(chain, key)
        .map_err(|e| e.to_string())
}

// --------------------------------------------------------------------- SSH

/// `ssh -L`, started for the length of the operation.
struct Tunnel {
    child: Child,
    port: u16,
    askpass: Option<PathBuf>,
}

impl Tunnel {
    fn open(target: &Target) -> Result<Tunnel, String> {
        let ssh = &target.ssh;
        if ssh.host.trim().is_empty() {
            return Err("SSH: no host".to_string());
        }
        let local = match ssh.local_port.trim() {
            "" => free_port()?,
            port => port
                .parse::<u16>()
                .map_err(|_| format!("SSH: bad local port {port}"))?,
        };
        let destination = if ssh.user.trim().is_empty() {
            ssh.host.trim().to_string()
        } else {
            format!("{}@{}", ssh.user.trim(), ssh.host.trim())
        };
        let mut command = Command::new("ssh");
        command
            .arg("-N")
            .args([
                "-L",
                &format!("127.0.0.1:{local}:{}:{}", target.host, target.port),
            ])
            .args(["-p", &ssh.port.to_string()])
            .args(["-o", "ExitOnForwardFailure=yes"])
            .args(["-o", "ConnectTimeout=10"])
            .args(["-o", "StrictHostKeyChecking=accept-new"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        let mut askpass = None;
        match ssh.auth {
            SshAuth::Agent => {
                command.args(["-o", "BatchMode=yes"]);
            }
            SshAuth::KeyPair => {
                if !ssh.key_file.trim().is_empty() {
                    command.args(["-i", ssh.key_file.trim(), "-o", "IdentitiesOnly=yes"]);
                }
                askpass = Some(secret_script(&mut command, &target.ssh_secret)?);
            }
            SshAuth::Password => {
                command.args([
                    "-o",
                    "PreferredAuthentications=password,keyboard-interactive",
                    "-o",
                    "PubkeyAuthentication=no",
                ]);
                askpass = Some(secret_script(&mut command, &target.ssh_secret)?);
            }
        }
        command.arg(&destination);
        let child = command
            .spawn()
            .map_err(|e| format!("SSH: cannot run ssh: {e}"))?;
        let mut tunnel = Tunnel {
            child,
            port: local,
            askpass,
        };
        // Wait until the local end accepts connections.
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(status) = tunnel.child.try_wait().map_err(|e| e.to_string())? {
                let mut said = String::new();
                if let Some(mut err) = tunnel.child.stderr.take() {
                    let _ = err.read_to_string(&mut said);
                }
                let said = said.trim().lines().last().unwrap_or("").to_string();
                return Err(format!("SSH: {status}: {said}"));
            }
            if TcpStream::connect_timeout(
                &([127, 0, 0, 1], local).into(),
                Duration::from_millis(200),
            )
            .is_ok()
            {
                return Ok(tunnel);
            }
            if Instant::now() > deadline {
                return Err("SSH: the tunnel did not come up in time".to_string());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

/// A script `ssh` runs to ask for the password: it prints the secret, which
/// reaches it through the environment (never the command line).
fn secret_script(command: &mut Command, secret: &str) -> Result<PathBuf, String> {
    let path = std::env::temp_dir().join(format!("simpleedit-askpass-{}", std::process::id()));
    std::fs::write(
        &path,
        "#!/bin/sh\nprintf '%s\\n' \"$SIMPLEEDIT_SSH_SECRET\"\n",
    )
    .map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| e.to_string())?;
    }
    command
        .env("SSH_ASKPASS", &path)
        .env("SSH_ASKPASS_REQUIRE", "force")
        .env("SIMPLEEDIT_SSH_SECRET", secret)
        .env(
            "DISPLAY",
            std::env::var("DISPLAY").unwrap_or_else(|_| ":0".into()),
        );
    Ok(path)
}

fn free_port() -> Result<u16, String> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|e| e.to_string())?;
    listener
        .local_addr()
        .map(|a| a.port())
        .map_err(|e| e.to_string())
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(path) = &self.askpass {
            let _ = std::fs::remove_file(path);
        }
    }
}

// ------------------------------------------------------------------ pgpass

/// The password `~/.pgpass` has for these (host:port:database:user lines,
/// `*` matches anything, `\:` and `\\` escape).
fn pgpass(host: &str, port: u16, database: &str, user: &str) -> Option<String> {
    let path = std::env::var_os("PGPASSFILE")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".pgpass")))?;
    let text = std::fs::read_to_string(path).ok()?;
    pgpass_lookup(&text, host, port, database, user)
}

fn pgpass_lookup(text: &str, host: &str, port: u16, database: &str, user: &str) -> Option<String> {
    let wanted = [
        host.to_string(),
        port.to_string(),
        database.to_string(),
        user.to_string(),
    ];
    text.lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .find_map(|line| {
            let mut fields = vec![String::new()];
            let mut escaped = false;
            for c in line.chars() {
                match (escaped, c) {
                    (true, c) => {
                        fields.last_mut()?.push(c);
                        escaped = false;
                    }
                    (false, '\\') => escaped = true,
                    (false, ':') if fields.len() < 5 => fields.push(String::new()),
                    (false, c) => fields.last_mut()?.push(c),
                }
            }
            if fields.len() != 5 {
                return None;
            }
            let same = fields[..4]
                .iter()
                .zip(&wanted)
                .all(|(f, w)| f == "*" || f == w || (f == "localhost" && w == "127.0.0.1"));
            same.then(|| fields[4].clone())
        })
}

// -------------------------------------------------------------------- tree

/// The text of an error, without the driver's wrapping noise.
fn clean(error: postgres::Error) -> String {
    match error.as_db_error() {
        Some(db) => db.message().to_string(),
        None => {
            let mut message = error.to_string();
            let mut source = std::error::Error::source(&error);
            while let Some(inner) = source {
                message = inner.to_string();
                source = inner.source();
            }
            message
        }
    }
}

/// The `type:[-]regex` filters of a scope.
struct Filters(Vec<(String, bool, Regex)>);

impl Filters {
    fn parse(spec: &str) -> Result<Filters, String> {
        let mut filters = Vec::new();
        for entry in spec.split('|').map(str::trim).filter(|e| !e.is_empty()) {
            let (kind, pattern) = entry
                .split_once(':')
                .ok_or_else(|| format!("object filter: {entry}"))?;
            let (hide, pattern) = match pattern.strip_prefix('-') {
                Some(rest) => (true, rest),
                None => (false, pattern),
            };
            let regex = Regex::new(&format!("^(?:{pattern})$"))
                .map_err(|e| format!("object filter {entry}: {e}"))?;
            filters.push((kind.trim().to_lowercase(), hide, regex));
        }
        Ok(Filters(filters))
    }

    fn keeps(&self, kind: Kind, name: &str) -> bool {
        let label = match kind {
            Kind::Table => "table",
            Kind::View => "view",
            Kind::Matview => "mview",
            Kind::Foreign => "ftable",
        };
        let mut shown = true;
        let mut asked = false;
        for (what, hide, regex) in &self.0 {
            if what != label {
                continue;
            }
            if *hide {
                if regex.is_match(name) {
                    return false;
                }
            } else {
                asked = true;
                shown = regex.is_match(name);
                if shown {
                    return true;
                }
            }
        }
        !asked || shown
    }
}

/// Which databases the pattern selects, and for each the schemas it names
/// (`None`: all of them).
fn selected(
    pattern: &str,
    current: &str,
    databases: &[String],
) -> Vec<(String, Option<Vec<String>>)> {
    let mut out: Vec<(String, Option<Vec<String>>)> = Vec::new();
    for entry in pattern.split('|').map(str::trim).filter(|e| !e.is_empty()) {
        let (db, schemas) = entry.split_once(':').unwrap_or((entry, "*"));
        let names: Vec<String> = match db.trim() {
            "*" => databases.to_vec(),
            "@" => vec![current.to_string()],
            name => vec![name.to_string()],
        };
        let schemas: Option<Vec<String>> = match schemas.trim() {
            "*" | "" => None,
            list => Some(list.split(',').map(|s| s.trim().to_string()).collect()),
        };
        for name in names {
            if !databases.contains(&name) {
                continue;
            }
            match out.iter_mut().find(|(n, _)| *n == name) {
                Some((_, existing)) => match (&mut *existing, &schemas) {
                    (Some(have), Some(more)) => have.extend(more.iter().cloned()),
                    _ => *existing = None,
                },
                None => out.push((name, schemas.clone())),
            }
        }
    }
    out
}

fn tree(mut session: Session, request: &Request) -> Result<Value, String> {
    let scope: &Scope = &request.scope;
    let filters = Filters::parse(&scope.object_filter)?;
    let current: String = session
        .client
        .query_one("SELECT current_database()", &[])
        .map_err(clean)?
        .get(0);
    let current_schema: Option<String> = session
        .client
        .query_one("SELECT current_schema()", &[])
        .map_err(clean)?
        .get(0);
    let all: Vec<String> = if scope.single_database {
        vec![current.clone()]
    } else {
        session
            .client
            .query(
                "SELECT datname FROM pg_database \
                 WHERE datallowconn AND (NOT datistemplate OR $1) ORDER BY datname",
                &[&scope.template_databases],
            )
            .map_err(clean)?
            .iter()
            .map(|r| r.get::<_, String>(0))
            .collect()
    };
    let mut databases = Vec::new();
    for (name, wanted) in selected(&scope.pattern, &current, &all) {
        let wanted: Option<Vec<String>> = wanted.map(|list| {
            list.into_iter()
                .map(|s| {
                    if s == "@" {
                        current_schema.clone().unwrap_or_else(|| "public".into())
                    } else {
                        s
                    }
                })
                .collect()
        });
        let schemas = if name == current {
            schemas_of(&mut session.client, scope, &filters, wanted.as_deref())?
        } else {
            let mut other = Session::open(&request.target, &name)?;
            schemas_of(&mut other.client, scope, &filters, wanted.as_deref())?
        };
        databases.push(Database {
            current: name == current,
            name,
            schemas,
        });
    }
    Ok(json!({ "databases": databases }))
}

fn schemas_of(
    client: &mut Client,
    scope: &Scope,
    filters: &Filters,
    wanted: Option<&[String]>,
) -> Result<Vec<Schema>, String> {
    let system = "nspname IN ('pg_catalog', 'information_schema') \
                  OR nspname LIKE 'pg\\_toast%' OR nspname LIKE 'pg\\_temp%'";
    let schemas = client
        .query(
            &format!(
                "SELECT nspname FROM pg_namespace WHERE ($1 OR NOT ({system})) ORDER BY nspname"
            ),
            &[&scope.system_schemas],
        )
        .map_err(clean)?;
    let tables = client
        .query(
            "SELECT n.nspname, c.relname, c.relkind::text \
             FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
             WHERE c.relkind IN ('r', 'p', 'v', 'm', 'f') \
             ORDER BY n.nspname, c.relname",
            &[],
        )
        .map_err(clean)?;
    Ok(schemas
        .iter()
        .map(|r| r.get::<_, String>(0))
        .filter(|name| wanted.is_none_or(|w| w.contains(name)))
        .map(|name| {
            let mine = tables
                .iter()
                .filter(|t| t.get::<_, String>(0) == name)
                .filter_map(|t| {
                    let kind = match t.get::<_, String>(2).as_str() {
                        "v" => Kind::View,
                        "m" => Kind::Matview,
                        "f" => Kind::Foreign,
                        _ => Kind::Table,
                    };
                    let table = t.get::<_, String>(1);
                    filters
                        .keeps(kind, &table)
                        .then_some(Table { name: table, kind })
                })
                .collect();
            Schema { name, tables: mine }
        })
        .collect())
}

/// A name between double quotes, quotes doubled.
fn quote(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn rows(
    client: &mut Client,
    schema: &str,
    table: &str,
    limit: u64,
    offset: u64,
) -> Result<Value, String> {
    let columns = client
        .query(
            "SELECT a.attname::text, format_type(a.atttypid, a.atttypmod) \
             FROM pg_attribute a \
             JOIN pg_class c ON c.oid = a.attrelid \
             JOIN pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = $1 AND c.relname = $2 \
               AND a.attnum > 0 AND NOT a.attisdropped \
             ORDER BY a.attnum",
            &[&schema, &table],
        )
        .map_err(clean)?;
    if columns.is_empty() {
        return Err(format!("{schema}.{table}: no such table"));
    }
    let names: Vec<String> = columns.iter().map(|c| c.get::<_, String>(0)).collect();
    let select: Vec<String> = names
        .iter()
        .map(|n| format!("{}::text", quote(n)))
        .collect();
    // One more than asked, to know whether there is a next page.
    let sql = format!(
        "SELECT {} FROM {}.{} LIMIT {} OFFSET {}",
        select.join(", "),
        quote(schema),
        quote(table),
        limit + 1,
        offset
    );
    let fetched = client.query(&sql, &[]).map_err(clean)?;
    let has_more = fetched.len() as u64 > limit;
    let data: Vec<Value> = fetched
        .iter()
        .take(limit as usize)
        .map(|row| {
            Value::Array(
                (0..names.len())
                    .map(|i| match row.get::<_, Option<String>>(i) {
                        Some(text) => Value::String(text),
                        None => Value::Null,
                    })
                    .collect(),
            )
        })
        .collect();
    let columns: Vec<Column> = columns
        .iter()
        .map(|c| Column {
            name: c.get::<_, String>(0),
            kind: c.get::<_, String>(1),
        })
        .collect();
    Ok(json!({ "columns": columns, "rows": data, "has_more": has_more }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pgpass_line_that_matches_gives_its_password() {
        let file = "# a comment\n*:5432:other:me:no\nlocalhost:5432:shop:dev:s\\:ecret\n";
        assert_eq!(
            pgpass_lookup(file, "127.0.0.1", 5432, "shop", "dev").as_deref(),
            Some("s:ecret")
        );
        assert_eq!(pgpass_lookup(file, "h", 5432, "shop", "dev"), None);
        assert_eq!(
            pgpass_lookup("*:*:*:me:pw", "x", 1, "y", "me").as_deref(),
            Some("pw")
        );
    }

    #[test]
    fn the_pattern_picks_databases_and_schemas() {
        let all = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        assert_eq!(selected("@:*", "b", &all), vec![("b".into(), None)]);
        assert_eq!(selected("*:*", "b", &all).len(), 3);
        assert_eq!(
            selected("a:s1,s2|c", "b", &all),
            vec![
                ("a".into(), Some(vec!["s1".into(), "s2".into()])),
                ("c".into(), None)
            ]
        );
        assert!(selected("zzz:*", "b", &all).is_empty());
    }

    #[test]
    fn the_object_filter_shows_and_hides_by_type() {
        let f = Filters::parse("table:-audit_.*|view:v_.*").unwrap();
        assert!(!f.keeps(Kind::Table, "audit_log"));
        assert!(f.keeps(Kind::Table, "users"));
        assert!(f.keeps(Kind::View, "v_users"));
        assert!(!f.keeps(Kind::View, "other"));
        assert!(Filters::parse("nonsense").is_err());
    }

    #[test]
    fn conninfo_values_are_quoted() {
        assert_eq!(conninfo_value("it's"), "'it\\'s'");
    }
}
