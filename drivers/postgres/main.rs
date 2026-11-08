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

use std::cell::RefCell;
use std::io::{BufRead, Read, Write};
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
    Auth, Change, ChangeKind, Column, Database, Executed, Kind, QueryResult, Request, Schema,
    Scope, SshAuth, SslMode, Table, Target, MAX_RESULT_ROWS, PROTOCOL,
};

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("--protocol") => println!("{PROTOCOL}"),
        // A session that stays: one request per line, one answer per line,
        // the connection (and what it holds: a transaction, temporary
        // tables, `SET`s) kept from one request to the next.
        Some("--serve") => serve(),
        _ => {
            let mut input = String::new();
            let answer = match std::io::stdin().read_to_string(&mut input) {
                Ok(_) => run(&input, &mut None, false),
                Err(e) => Err(e.to_string()),
            };
            println!("{}", answer_line(answer));
        }
    }
}

/// The answer line: what was run goes back with it, whether it went well or
/// not.
fn answer_line(answer: Result<Value, String>) -> Value {
    let executed = LOG.with(|log| std::mem::take(&mut *log.borrow_mut()));
    match answer {
        Ok(mut value) => {
            value["ok"] = json!(true);
            value["executed"] = json!(executed);
            value
        }
        Err(error) => json!({ "ok": false, "error": error, "executed": executed }),
    }
}

fn serve() {
    let mut slot: Option<(String, Session)> = None;
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let answer = run(&line, &mut slot, true);
        if writeln!(out, "{}", answer_line(answer)).is_err() || out.flush().is_err() {
            break;
        }
    }
}

thread_local! {
    /// The commands run so far, for the log of the app.
    static LOG: RefCell<Vec<Executed>> = const { RefCell::new(Vec::new()) };
}

fn record(sql: &str, started: Instant, rows: Option<u64>, error: Option<String>) {
    log(sql, started, rows, error, false);
}

/// A command the driver runs for its own needs.
fn record_internal(sql: &str, started: Instant, rows: Option<u64>, error: Option<String>) {
    log(sql, started, rows, error, true);
}

fn log(sql: &str, started: Instant, rows: Option<u64>, error: Option<String>, internal: bool) {
    // On one line, as the log shows it.
    let sql = sql.split_whitespace().collect::<Vec<_>>().join(" ");
    LOG.with(|log| {
        log.borrow_mut().push(Executed {
            sql,
            ms: started.elapsed().as_millis() as u64,
            rows,
            error,
            internal,
        })
    });
}

/// Runs a query of the driver's own, and logs it.
fn internal_query(
    client: &mut Client,
    sql: &str,
    params: &[&(dyn postgres::types::ToSql + Sync)],
) -> Result<Vec<postgres::Row>, postgres::Error> {
    let started = Instant::now();
    match client.query(sql, params) {
        Ok(rows) => {
            record_internal(sql, started, Some(rows.len() as u64), None);
            Ok(rows)
        }
        Err(e) => {
            record_internal(sql, started, None, Some(e.to_string()));
            Err(e)
        }
    }
}

/// The connection a request is about, kept in `slot` when the driver serves
/// several requests: it is reused while the target and the database are the
/// same and the server has not hung up.
fn run(input: &str, slot: &mut Option<(String, Session)>, serving: bool) -> Result<Value, String> {
    let request: Request = serde_json::from_str(input).map_err(|e| e.to_string())?;
    // The database named or, when none is, the connection's own: the same
    // session either way.
    let database = if request.database.is_empty() {
        &request.target.database
    } else {
        &request.database
    };
    let key = serde_json::to_string(&(&request.target, database)).unwrap_or_default();
    let usable = matches!(slot, Some((k, s)) if *k == key && !s.client.is_closed());
    if !usable {
        *slot = None;
        let opened = Session::open(&request.target, &request.database)?;
        *slot = Some((key, opened));
        if serving {
            // Said at once, before the request is answered: whoever wants to
            // stop a long statement needs it while the statement runs.
            if let Some((_, session)) = slot.as_mut() {
                if let Ok(row) = session.client.query_one("SELECT pg_backend_pid()", &[]) {
                    let mut out = std::io::stdout();
                    let _ = writeln!(out, "{}", json!({ "backend_pid": row.get::<_, i32>(0) }));
                    let _ = out.flush();
                }
            }
        }
    }
    let session = &mut slot.as_mut().expect("just opened").1;
    let tunnel_port = session.tunnel_port;
    let client = &mut session.client;
    match request.op.as_str() {
        "test" => {
            let started = Instant::now();
            let row = match client.query_one("SELECT version()", &[]) {
                Ok(row) => row,
                Err(e) => {
                    let message = clean(e);
                    record("SELECT version()", started, None, Some(message.clone()));
                    return Err(message);
                }
            };
            record("SELECT version()", started, Some(1), None);
            Ok(json!({ "version": row.get::<_, String>(0) }))
        }
        "tree" => tree(session, &request),
        "rows" => {
            limit(client, &request.target)?;
            guarded(client, &request.target, |c| rows(c, &request))
        }
        "export" => {
            limit(client, &request.target)?;
            guarded(client, &request.target, |c| export(c, &request))
        }
        "apply" => {
            if request.target.read_only {
                return Err("this data source is read-only".to_string());
            }
            limit(client, &request.target)?;
            apply(client, &request)
        }
        "dump" => dump(&request, tunnel_port),
        "restore" => {
            if request.target.read_only {
                return Err("this data source is read-only".to_string());
            }
            restore(&request, tunnel_port)
        }
        "import" => {
            if request.target.read_only {
                return Err("this data source is read-only".to_string());
            }
            limit(client, &request.target)?;
            import(client, &request)
        }
        "query" => {
            limit(client, &request.target)?;
            let mut answer = guarded(client, &request.target, |c| query(c, &request))?;
            if serving {
                answer["in_transaction"] = json!(in_transaction(client));
            }
            Ok(answer)
        }
        "columns" => columns(client, &request),
        "choices" => choices(client, &request),
        "health" => health(client, &request),
        "structure" => structure(client, &request),
        "definition" => definition(client, &request),
        "cancel" => {
            // From another connection: the statement stops on the server, which
            // is the only way on servers that don't watch their clients.
            let started = Instant::now();
            let sql = "SELECT pg_cancel_backend($1)";
            let stopped = internal_query(client, sql, &[&(request.pid as i32)]).map_err(clean)?;
            let _ = started;
            Ok(json!({ "stopped": stopped.first().is_some_and(|r| r.get::<_, bool>(0)) }))
        }
        other => Err(format!("unknown operation: {other}")),
    }
}

/// Whether a transaction is open on the session (an aborted one too).
fn in_transaction(client: &mut Client) -> bool {
    // In a transaction of its own, a statement starts it: the two clocks
    // read the same.
    match client.simple_query("SELECT transaction_timestamp() <> statement_timestamp()") {
        Ok(messages) => messages
            .iter()
            .any(|m| matches!(m, postgres::SimpleQueryMessage::Row(r) if r.get(0) == Some("t"))),
        Err(_) => true,
    }
}

/// On a read-only source, what runs is inside a read-only transaction that is
/// never kept: nothing that changes data can go through, a function that
/// writes included.
fn guarded<T>(
    client: &mut Client,
    target: &Target,
    f: impl FnOnce(&mut Client) -> Result<T, String>,
) -> Result<T, String> {
    if !target.read_only {
        return f(client);
    }
    client.batch_execute("BEGIN READ ONLY").map_err(clean)?;
    let out = f(client);
    let _ = client.batch_execute("ROLLBACK");
    out
}

/// The longest a statement may run, and the server watching that the client
/// is still there: when the app gives up on a query (the user cancelled it),
/// the driver is gone and the server stops the query within a second
/// (PostgreSQL 14 and later; older servers ignore the setting).
fn limit(client: &mut Client, target: &Target) -> Result<(), String> {
    if target.query_timeout > 0 {
        client
            .batch_execute(&format!(
                "SET statement_timeout = {}",
                target.query_timeout.saturating_mul(1000)
            ))
            .map_err(clean)?;
    }
    let _ = client.batch_execute("SET client_connection_check_interval = 1000");
    Ok(())
}

// -------------------------------------------------------------- connection

/// A connection, and the tunnel it goes through, if any.
struct Session {
    client: Client,
    _tunnel: Option<Tunnel>,
    /// The local end of the tunnel the connection goes through, if any.
    tunnel_port: Option<u16>,
}

impl Session {
    /// Connects to `database` (the target's own when empty).
    fn open(target: &Target, database: &str) -> Result<Session, String> {
        Session::open_via(target, database, None)
    }

    /// The same, through the tunnel that is already up on `tunnel_port`, if
    /// one is (a fixed local port can't be taken twice).
    fn open_via(
        target: &Target,
        database: &str,
        tunnel_port: Option<u16>,
    ) -> Result<Session, String> {
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
        let (port, tunnel) = if !target.ssh.enabled {
            (target.port, None)
        } else if let Some(port) = tunnel_port {
            (port, None)
        } else {
            let tunnel = Tunnel::open(target)?;
            (tunnel.port, Some(tunnel))
        };
        let tunnel_port = target.ssh.enabled.then_some(port);
        // Through a tunnel the name stays the server's (the certificate is
        // checked against it); only the place to knock at changes.
        let mut config = config(
            target,
            port,
            tunnel_port.map(|_| "127.0.0.1"),
            password.as_deref(),
            database,
        )?;
        let mut client = connect(&mut config, target)?;
        // The session is set up the way the connection asks.
        if target.read_only {
            let started = Instant::now();
            client
                .batch_execute("SET default_transaction_read_only = on")
                .map_err(clean)?;
            record_internal(
                "SET default_transaction_read_only = on",
                started,
                None,
                None,
            );
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
            let started = Instant::now();
            client
                .batch_execute(&target.startup_script)
                .map_err(|e| format!("startup script: {}", clean(e)))?;
            record_internal(&target.startup_script, started, None, None);
        }
        Ok(Session {
            client,
            _tunnel: tunnel,
            tunnel_port,
        })
    }
}

/// The text of a value in a libpq connection string.
fn conninfo_value(value: &str) -> String {
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"))
}

fn config(
    target: &Target,
    port: u16,
    hostaddr: Option<&str>,
    password: Option<&str>,
    database: &str,
) -> Result<Config, String> {
    let mut info = String::new();
    let mut add = |key: &str, value: &str| {
        info.push_str(&format!("{key}={} ", conninfo_value(value)));
    };
    add("host", &target.host);
    if let Some(addr) = hostaddr {
        add("hostaddr", addr);
    }
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

/// Where in the statement the server found the error, counted in characters
/// from 1 (a syntax error, an unknown column).
fn position_of(error: &postgres::Error) -> Option<u32> {
    match error.as_db_error()?.position()? {
        postgres::error::ErrorPosition::Original(at) => Some(*at),
        postgres::error::ErrorPosition::Internal { .. } => None,
    }
}

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

fn tree(session: &mut Session, request: &Request) -> Result<Value, String> {
    let scope: &Scope = &request.scope;
    let filters = Filters::parse(&scope.object_filter)?;
    let current: String = internal_query(&mut session.client, "SELECT current_database()", &[])
        .map_err(clean)?[0]
        .get(0);
    let current_schema: Option<String> =
        internal_query(&mut session.client, "SELECT current_schema()", &[]).map_err(clean)?[0]
            .get(0);
    let all: Vec<String> = if scope.single_database {
        vec![current.clone()]
    } else {
        internal_query(
            &mut session.client,
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
            let mut other = Session::open_via(&request.target, &name, session.tunnel_port)?;
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
    let schemas = internal_query(
        client,
        &format!("SELECT nspname FROM pg_namespace WHERE ($1 OR NOT ({system})) ORDER BY nspname"),
        &[&scope.system_schemas],
    )
    .map_err(clean)?;
    let tables = internal_query(
        client,
        "SELECT n.nspname, c.relname, c.relkind::text \
         FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
         WHERE c.relkind IN ('r', 'p', 'v', 'm', 'f') \
         ORDER BY n.nspname, c.relname",
        &[],
    )
    .map_err(clean)?;
    let extension = |alias: &str, class: &str| {
        format!(
            "NOT EXISTS (SELECT 1 FROM pg_depend d WHERE d.objid = {alias}.oid \
             AND d.classid = '{class}'::regclass AND d.deptype = 'e')"
        )
    };
    let routines = internal_query(
        client,
        &format!(
            "SELECT n.nspname::text, p.proname::text, pg_get_function_identity_arguments(p.oid), \
                    p.prokind::text \
             FROM pg_proc p JOIN pg_namespace n ON n.oid = p.pronamespace \
             WHERE p.prokind IN ('f', 'p') AND {} ORDER BY n.nspname, p.proname, 3",
            extension("p", "pg_proc")
        ),
        &[],
    )
    .map_err(clean)?;
    // The sequences that a column owns (serial, identity) are part of it.
    let sequences = internal_query(
        client,
        "SELECT n.nspname::text, c.relname::text \
         FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
         WHERE c.relkind = 'S' AND NOT EXISTS (SELECT 1 FROM pg_depend d \
               WHERE d.objid = c.oid AND d.classid = 'pg_class'::regclass \
                 AND d.deptype IN ('a', 'i')) \
         ORDER BY n.nspname, c.relname",
        &[],
    )
    .map_err(clean)?;
    let types = internal_query(
        client,
        &format!(
            "SELECT n.nspname::text, t.typname::text \
             FROM pg_type t JOIN pg_namespace n ON n.oid = t.typnamespace \
             WHERE (t.typtype IN ('e', 'd') OR (t.typtype = 'c' AND EXISTS \
                    (SELECT 1 FROM pg_class c WHERE c.oid = t.typrelid AND c.relkind = 'c'))) \
               AND {} ORDER BY n.nspname, t.typname",
            extension("t", "pg_type")
        ),
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
            let in_schema = |rows: &[postgres::Row]| -> Vec<String> {
                rows.iter()
                    .filter(|r| r.get::<_, String>(0) == name)
                    .map(|r| r.get::<_, String>(1))
                    .collect()
            };
            Schema {
                tables: mine,
                routines: routines
                    .iter()
                    .filter(|r| r.get::<_, String>(0) == name)
                    .map(|r| protocol::Routine {
                        name: r.get(1),
                        args: r.get(2),
                        procedure: r.get::<_, String>(3) == "p",
                    })
                    .collect(),
                sequences: in_schema(&sequences),
                types: in_schema(&types),
                name,
            }
        })
        .collect())
}

/// A name between double quotes, quotes doubled.
fn quote(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// What a table is made of: its columns, its primary key, and whether
/// rows can be changed in it.
struct Shape {
    columns: Vec<Column>,
    primary_key: Vec<String>,
    /// An ordinary or partitioned table (not a view).
    is_table: bool,
}

fn shape(client: &mut Client, schema: &str, table: &str) -> Result<Shape, String> {
    let found = internal_query(
        client,
        "SELECT a.attname::text, format_type(a.atttypid, a.atttypmod), NOT a.attnotnull, \
                    c.relkind::text, \
                    COALESCE(a.attnum = ANY (i.indkey::int2[]), false) \
             FROM pg_attribute a \
             JOIN pg_class c ON c.oid = a.attrelid \
             JOIN pg_namespace n ON n.oid = c.relnamespace \
             LEFT JOIN pg_index i ON i.indrelid = c.oid AND i.indisprimary \
             WHERE n.nspname = $1 AND c.relname = $2 \
               AND a.attnum > 0 AND NOT a.attisdropped \
             ORDER BY a.attnum",
        &[&schema, &table],
    )
    .map_err(clean)?;
    if found.is_empty() {
        return Err(format!("{schema}.{table}: no such table"));
    }
    let mut columns = Vec::new();
    let mut primary_key = Vec::new();
    for row in &found {
        let name: String = row.get(0);
        if row.get::<_, bool>(4) {
            primary_key.push(name.clone());
        }
        columns.push(Column {
            name,
            kind: row.get(1),
            nullable: row.get(2),
        });
    }
    let relkind: String = found[0].get(3);
    Ok(Shape {
        columns,
        primary_key,
        is_table: matches!(relkind.as_str(), "r" | "p"),
    })
}

/// ` WHERE (…)` and ` ORDER BY …` for a request; the parts are known columns
/// and the user's own condition, on its own lines so that a comment in it
/// can't swallow what follows.
///
/// `qualifier` (`"schema"."table".`, or empty) puts the sorted columns in the
/// table: a bare name would be the column of the list, which is cast to text
/// (and would sort `10` before `2`).
///
/// `tiebreak` names the columns that make the order total (the primary key,
/// or `ctid`): they follow the user's sort, so that a row is on one page only.
fn clauses(
    request: &Request,
    names: &[&str],
    tiebreak: &[String],
    qualifier: &str,
) -> Result<String, String> {
    let mut sql = String::new();
    if protocol::has_semicolon(&request.filter) {
        return Err("the condition cannot hold a `;`".to_string());
    }
    if !request.filter.trim().is_empty() {
        sql.push_str(&format!(" WHERE (\n{}\n)", request.filter.trim()));
    }
    if !request.order.is_empty() || !tiebreak.is_empty() {
        let mut parts = Vec::new();
        for sort in &request.order {
            if !names.contains(&sort.column.as_str()) {
                return Err(format!("no column {}", sort.column));
            }
            parts.push(format!(
                "{qualifier}{} {}",
                quote(&sort.column),
                if sort.desc { "DESC" } else { "ASC" }
            ));
        }
        for name in tiebreak {
            if !request.order.iter().any(|s| s.column == *name) {
                parts.push(if name == "ctid" {
                    format!("{qualifier}ctid")
                } else {
                    format!("{qualifier}{}", quote(name))
                });
            }
        }
        sql.push_str(&format!(" ORDER BY {}", parts.join(", ")));
    }
    Ok(sql)
}

fn rows(client: &mut Client, request: &Request) -> Result<Value, String> {
    // The rows of a query, not of a table.
    if !request.sql.trim().is_empty() {
        return result_rows(client, request);
    }
    let (schema, table) = (request.schema.as_str(), request.table.as_str());
    let limit = request.limit.max(1);
    let shape = shape(client, schema, table)?;
    let names: Vec<&str> = shape.columns.iter().map(|c| c.name.as_str()).collect();
    let mut select: Vec<String> = names
        .iter()
        .map(|n| format!("{}::text", quote(n)))
        .collect();
    // Without a primary key, a row is found again by its place.
    let keyless = shape.is_table && shape.primary_key.is_empty();
    let qualifier = format!("{}.{}.", quote(schema), quote(table));
    // Without a stable order the pages of a table could overlap or skip rows.
    let tiebreak: Vec<String> = if !shape.is_table {
        Vec::new()
    } else if keyless {
        vec!["ctid".to_string()]
    } else {
        shape.primary_key.clone()
    };
    if keyless {
        select.push("ctid::text".to_string());
    }
    // One more than asked, to know whether there is a next page.
    let sql = format!(
        "SELECT {} FROM {}.{}{} LIMIT {} OFFSET {}",
        select.join(", "),
        quote(schema),
        quote(table),
        clauses(request, &names, &tiebreak, &qualifier)?,
        limit + 1,
        request.offset
    );
    // What the log shows is the plain query, without the casts to text.
    let shown = format!(
        "SELECT * FROM {}.{}{} LIMIT {} OFFSET {}",
        quote(schema),
        quote(table),
        clauses(request, &names, &tiebreak, &qualifier)?,
        limit,
        request.offset
    );
    let started = Instant::now();
    let fetched = match client.query(&sql, &[]) {
        Ok(rows) => rows,
        Err(e) => {
            let message = clean(e);
            record(&shown, started, None, Some(message.clone()));
            return Err(message);
        }
    };
    record(
        &shown,
        started,
        Some(fetched.len().min(limit as usize) as u64),
        None,
    );
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
    let ctids: Vec<String> = if keyless {
        fetched
            .iter()
            .take(limit as usize)
            .map(|row| row.get::<_, String>(names.len()))
            .collect()
    } else {
        Vec::new()
    };
    let editable = shape.is_table && !request.target.read_only;
    Ok(json!({
        "columns": shape.columns,
        "rows": data,
        "has_more": has_more,
        "primary_key": shape.primary_key,
        "ctids": ctids,
        "editable": editable,
    }))
}

/// The rows of a query, sorted, filtered and paged by wrapping it:
/// `SELECT * FROM (query) AS "q" …`. When all its columns are columns of one
/// table, with the names they have there, and its primary key is among them,
/// the rows can be changed.
fn result_rows(client: &mut Client, request: &Request) -> Result<Value, String> {
    let sql = request.sql.trim().trim_end_matches(';').trim().to_string();
    if !protocol::wrappable(&sql) {
        return Err("this query cannot be paged".to_string());
    }
    let limit = request.limit.max(1);
    // What the query is made of, without running it.
    let statement = client.prepare(&sql).map_err(clean)?;
    let columns: Vec<Column> = statement
        .columns()
        .iter()
        .map(|c| Column {
            name: c.name().to_string(),
            kind: c.type_().name().to_string(),
            nullable: true,
        })
        .collect();
    let names: Vec<&str> = columns.iter().map(|c| c.name.as_str()).collect();
    let wrapped = format!(
        "SELECT * FROM (\n{sql}\n) AS \"q\"{} LIMIT {} OFFSET {}",
        clauses(request, &names, &[], "")?,
        limit + 1,
        request.offset
    );
    let started = Instant::now();
    let fetched = match client.simple_query(&wrapped) {
        Ok(messages) => messages,
        Err(e) => {
            let message = clean(e);
            record(&wrapped, started, None, Some(message.clone()));
            return Err(message);
        }
    };
    let mut data: Vec<Value> = Vec::new();
    for message in fetched {
        if let postgres::SimpleQueryMessage::Row(row) = message {
            data.push(Value::Array(
                (0..row.len())
                    .map(|i| match row.get(i) {
                        Some(text) => Value::String(text.to_string()),
                        None => Value::Null,
                    })
                    .collect(),
            ));
        }
    }
    let has_more = data.len() as u64 > limit;
    data.truncate(limit as usize);
    record(&wrapped, started, Some(data.len() as u64), None);

    let (primary_key, editable, source) = if request.target.read_only {
        (Vec::new(), false, None)
    } else {
        source_of(client, statement.columns()).unwrap_or_default()
    };
    Ok(json!({
        "columns": columns,
        "rows": data,
        "has_more": has_more,
        "primary_key": primary_key,
        "editable": editable,
        "source": source,
    }))
}

/// The table all the columns of a query come from, if there is one, they keep
/// their names, and its primary key is among them: then its primary key, and
/// `true` for editable.
fn source_of(
    client: &mut Client,
    columns: &[postgres::Column],
) -> Option<(Vec<String>, bool, Option<protocol::Source>)> {
    let oid = columns.first()?.table_oid().filter(|oid| *oid != 0)?;
    if columns.iter().any(|c| c.table_oid() != Some(oid)) {
        return None;
    }
    let found = internal_query(
        client,
        "SELECT n.nspname, c.relname, c.relkind::text FROM pg_class c \
         JOIN pg_namespace n ON n.oid = c.relnamespace WHERE c.oid = $1",
        &[&oid],
    )
    .ok()?;
    let row = found.first()?;
    let (schema, table, kind): (String, String, String) = (row.get(0), row.get(1), row.get(2));
    if !matches!(kind.as_str(), "r" | "p") {
        return None;
    }
    // Each column has to be the one of the table it seems to be.
    let attributes = internal_query(
        client,
        "SELECT attnum, attname::text FROM pg_attribute WHERE attrelid = $1 AND attnum > 0",
        &[&oid],
    )
    .ok()?;
    for column in columns {
        let id = column.column_id()?;
        let real = attributes
            .iter()
            .find(|a| a.get::<_, i16>(0) == id)
            .map(|a| a.get::<_, String>(1))?;
        if real != column.name() {
            return None;
        }
    }
    let shape = shape(client, &schema, &table).ok()?;
    let present = |name: &String| columns.iter().any(|c| c.name() == name);
    if shape.primary_key.is_empty() || !shape.primary_key.iter().all(present) {
        return None;
    }
    Some((
        shape.primary_key,
        true,
        Some(protocol::Source { schema, table }),
    ))
}

/// The SQL of a change: a statement and its text parameters. A value is cast
/// from text to the type of its column, so any type can be given as text.
fn statement(
    change: &Change,
    schema: &str,
    table: &str,
    shape: &Shape,
) -> Result<(String, Vec<Option<String>>), String> {
    let kind_of = |name: &str| -> Result<&str, String> {
        shape
            .columns
            .iter()
            .find(|c| c.name == name)
            .map(|c| c.kind.as_str())
            .ok_or_else(|| format!("no column {name}"))
    };
    let mut params: Vec<Option<String>> = Vec::new();
    let target = format!("{}.{}", quote(schema), quote(table));
    // A new row: the columns that were given, the others keep their default.
    if change.kind == ChangeKind::Insert {
        let mut columns = Vec::new();
        let mut values = Vec::new();
        for (column, value) in &change.set {
            let kind = kind_of(column)?;
            columns.push(quote(column));
            match value {
                Some(text) => {
                    params.push(Some(text.clone()));
                    values.push(format!("(${}::text)::{kind}", params.len()));
                }
                None => values.push("NULL".to_string()),
            }
        }
        let sql = if columns.is_empty() {
            format!("INSERT INTO {target} DEFAULT VALUES")
        } else {
            format!(
                "INSERT INTO {target} ({}) VALUES ({})",
                columns.join(", "),
                values.join(", ")
            )
        };
        return Ok((sql, params));
    }
    let mut sets = Vec::new();
    for (column, value) in &change.set {
        match value {
            Some(text) => {
                params.push(Some(text.clone()));
                let n = params.len();
                sets.push(format!(
                    "{} = (${n}::text)::{}",
                    quote(column),
                    kind_of(column)?
                ));
            }
            None => {
                kind_of(column)?;
                sets.push(format!("{} = NULL", quote(column)));
            }
        }
    }
    let mut conditions = Vec::new();
    if shape.primary_key.is_empty() {
        // No primary key: the row's place, and all it was read with.
        if change.key.len() != 1 || change.key[0].0 != "ctid" {
            return Err("the row cannot be found: it has no place to be found by".to_string());
        }
        params.push(Some(change.key[0].1.clone()));
        conditions.push(format!("ctid = (${}::text)::tid", params.len()));
        for (column, value) in &change.original {
            kind_of(column)?;
            match value {
                Some(text) => {
                    params.push(Some(text.clone()));
                    conditions.push(format!(
                        "{}::text IS NOT DISTINCT FROM ${}::text",
                        quote(column),
                        params.len()
                    ));
                }
                None => conditions.push(format!("{} IS NULL", quote(column))),
            }
        }
    } else {
        // The key must be the table's primary key, whole.
        let mut key_columns: Vec<&str> = change.key.iter().map(|(c, _)| c.as_str()).collect();
        key_columns.sort_unstable();
        let mut expected: Vec<&str> = shape.primary_key.iter().map(String::as_str).collect();
        expected.sort_unstable();
        if key_columns != expected {
            return Err("the row cannot be found: the table has no usable primary key".to_string());
        }
        for (column, value) in &change.key {
            params.push(Some(value.clone()));
            conditions.push(format!(
                "{} = (${}::text)::{}",
                quote(column),
                params.len(),
                kind_of(column)?
            ));
        }
    }
    let sql = match change.kind {
        ChangeKind::Insert => return Err("a new row has no key".to_string()),
        ChangeKind::Update => {
            if sets.is_empty() {
                return Err("nothing to change".to_string());
            }
            format!(
                "UPDATE {target} SET {} WHERE {}",
                sets.join(", "),
                conditions.join(" AND ")
            )
        }
        ChangeKind::Delete => format!("DELETE FROM {target} WHERE {}", conditions.join(" AND ")),
    };
    Ok((sql, params))
}

/// Applies the changes of the request in one transaction: if one fails, or
/// does not touch exactly one row, none is made.
fn apply(client: &mut Client, request: &Request) -> Result<Value, String> {
    let (schema, table) = (request.schema.as_str(), request.table.as_str());
    let shape = shape(client, schema, table)?;
    if !shape.is_table {
        return Err("only the rows of a table can be changed".to_string());
    }
    let mut transaction = client.transaction().map_err(clean)?;
    let mut done = 0u64;
    for (i, change) in request.changes.iter().enumerate() {
        let (sql, params) = statement(change, schema, table, &shape)?;
        let refs: Vec<&(dyn postgres::types::ToSql + Sync)> = params
            .iter()
            .map(|p| p as &(dyn postgres::types::ToSql + Sync))
            .collect();
        let shown = protocol::display_change(change, schema, table);
        let started = Instant::now();
        let touched = match transaction.execute(&sql, &refs) {
            Ok(touched) => touched,
            Err(e) => {
                let message = clean(e);
                record(&shown, started, None, Some(message.clone()));
                return Err(format!("change {}: {message}", i + 1));
            }
        };
        record(&shown, started, Some(touched), None);
        if touched != 1 {
            return Err(format!(
                "change {}: {touched} rows touched instead of 1 (the row was changed or removed by someone else)",
                i + 1
            ));
        }
        done += touched;
    }
    transaction.commit().map_err(clean)?;
    Ok(json!({ "applied": done }))
}

/// What a table is made of: its columns with their types, defaults and
/// comments, its constraints and its indexes, for the table designer.
fn structure(client: &mut Client, request: &Request) -> Result<Value, String> {
    let name = format!("{}.{}", quote(&request.schema), quote(&request.table));
    let columns = internal_query(
        client,
        "SELECT a.attname::text, format_type(a.atttypid, a.atttypmod), NOT a.attnotnull, \
                pg_get_expr(d.adbin, d.adrelid), a.attidentity::text, a.attgenerated::text, \
                col_description(a.attrelid, a.attnum) \
         FROM pg_attribute a \
         LEFT JOIN pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum \
         WHERE a.attrelid = $1::text::regclass AND a.attnum > 0 AND NOT a.attisdropped \
         ORDER BY a.attnum",
        &[&name],
    )
    .map_err(clean)?;
    let constraints = internal_query(
        client,
        "SELECT conname::text, contype::text, pg_get_constraintdef(oid) \
         FROM pg_constraint WHERE conrelid = $1::text::regclass ORDER BY contype, conname",
        &[&name],
    )
    .map_err(clean)?;
    let indexes = internal_query(
        client,
        "SELECT ic.relname::text, pg_get_indexdef(i.indexrelid), i.indisunique \
         FROM pg_index i JOIN pg_class ic ON ic.oid = i.indexrelid \
         WHERE i.indrelid = $1::text::regclass \
           AND NOT EXISTS (SELECT 1 FROM pg_constraint c WHERE c.conindid = i.indexrelid) \
         ORDER BY ic.relname",
        &[&name],
    )
    .map_err(clean)?;
    let comment = internal_query(
        client,
        "SELECT obj_description($1::text::regclass, 'pg_class')",
        &[&name],
    )
    .map_err(clean)?
    .first()
    .and_then(|r| r.get::<_, Option<String>>(0))
    .unwrap_or_default();
    let referenced_by = internal_query(
        client,
        "SELECT c.conname::text, n.nspname::text, r.relname::text, pg_get_constraintdef(c.oid) \
         FROM pg_constraint c JOIN pg_class r ON r.oid = c.conrelid \
         JOIN pg_namespace n ON n.oid = r.relnamespace \
         WHERE c.contype = 'f' AND c.confrelid = $1::text::regclass \
         ORDER BY n.nspname, r.relname, c.conname",
        &[&name],
    )
    .map_err(clean)?;
    let (partition_key, unlogged) = {
        let found = internal_query(
            client,
            "SELECT pg_get_partkeydef(c.oid), c.relpersistence::text FROM pg_class c \
             WHERE c.oid = $1::text::regclass",
            &[&name],
        )
        .map_err(clean)?;
        match found.first() {
            Some(r) => (
                r.get::<_, Option<String>>(0).unwrap_or_default(),
                r.get::<_, String>(1) == "u",
            ),
            None => (String::new(), false),
        }
    };
    let partition_of = internal_query(
        client,
        "SELECT pn.nspname::text, pc.relname::text, pg_get_expr(c.relpartbound, c.oid) \
         FROM pg_class c JOIN pg_inherits i ON i.inhrelid = c.oid \
         JOIN pg_class pc ON pc.oid = i.inhparent \
         JOIN pg_namespace pn ON pn.oid = pc.relnamespace \
         WHERE c.oid = $1::text::regclass AND c.relispartition",
        &[&name],
    )
    .map_err(clean)?
    .first()
    .map(|r| protocol::PartitionOf {
        schema: r.get(0),
        table: r.get(1),
        bound: r.get::<_, Option<String>>(2).unwrap_or_default(),
    });
    let triggers: Vec<String> = internal_query(
        client,
        "SELECT pg_get_triggerdef(oid) FROM pg_trigger \
         WHERE tgrelid = $1::text::regclass AND NOT tgisinternal ORDER BY tgname",
        &[&name],
    )
    .map_err(clean)?
    .iter()
    .map(|r| r.get(0))
    .collect();
    let structure = protocol::Structure {
        extra: Box::new(protocol::TableExtra {
            partition_key,
            partition_of,
            triggers,
            unlogged,
        }),
        referenced_by: referenced_by
            .iter()
            .map(|r| protocol::Reference {
                name: r.get(0),
                schema: r.get(1),
                table: r.get(2),
                definition: r.get(3),
            })
            .collect(),
        columns: columns
            .iter()
            .map(|r| protocol::ColumnDef {
                name: r.get(0),
                kind: r.get(1),
                nullable: r.get(2),
                default: r.get(3),
                identity: r.get(4),
                generated: !r.get::<_, String>(5).is_empty(),
                comment: r.get::<_, Option<String>>(6).unwrap_or_default(),
            })
            .collect(),
        constraints: constraints
            .iter()
            .map(|r| protocol::ConstraintDef {
                name: r.get(0),
                kind: match r.get::<_, String>(1).as_str() {
                    "p" => protocol::ConstraintKind::Primary,
                    "u" => protocol::ConstraintKind::Unique,
                    "f" => protocol::ConstraintKind::Foreign,
                    "c" => protocol::ConstraintKind::Check,
                    _ => protocol::ConstraintKind::Other,
                },
                definition: r.get(2),
            })
            .collect(),
        indexes: indexes
            .iter()
            .map(|r| protocol::IndexDef {
                name: r.get(0),
                definition: r.get(1),
                unique: r.get(2),
            })
            .collect(),
        comment,
    };
    Ok(json!({ "structure": structure }))
}

/// The SQL that makes a view or a materialized view.
fn definition(client: &mut Client, request: &Request) -> Result<Value, String> {
    if request.object == "function" {
        let rows = internal_query(
            client,
            "SELECT pg_get_functiondef(p.oid) FROM pg_proc p \
             JOIN pg_namespace n ON n.oid = p.pronamespace \
             WHERE n.nspname = $1 AND p.proname = $2 \
               AND pg_get_function_identity_arguments(p.oid) = $3 \
               AND p.prokind IN ('f', 'p')",
            &[&request.schema, &request.table, &request.sql],
        )
        .map_err(clean)?;
        let text: String = rows
            .first()
            .and_then(|r| r.get::<_, Option<String>>(0))
            .ok_or_else(|| format!("{}.{}: no such function", request.schema, request.table))?;
        return Ok(json!({ "definition": format!("{};\n", text.trim_end()) }));
    }
    let name = format!("{}.{}", quote(&request.schema), quote(&request.table));
    let rows = internal_query(
        client,
        "SELECT pg_get_viewdef($1::text::regclass, true)",
        &[&name],
    )
    .map_err(clean)?;
    let body: String = rows
        .first()
        .and_then(|r| r.get::<_, Option<String>>(0))
        .ok_or_else(|| format!("{name}: no such view"))?;
    let body = body.trim_end().trim_end_matches(';');
    let text = if request.object == "matview" {
        format!("CREATE MATERIALIZED VIEW {name} AS\n{body}\nWITH DATA;\n")
    } else {
        format!("CREATE OR REPLACE VIEW {name} AS\n{body};\n")
    };
    Ok(json!({ "definition": text }))
}

/// Every column of every table and view of the database, for the completion
/// (at most a hundred thousand, those of the schemas in use first).
fn columns(client: &mut Client, request: &Request) -> Result<Value, String> {
    let rows = internal_query(
        client,
        "SELECT n.nspname::text, c.relname::text, a.attname::text, \
                format_type(a.atttypid, a.atttypmod) \
         FROM pg_attribute a \
         JOIN pg_class c ON c.oid = a.attrelid \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         WHERE a.attnum > 0 AND NOT a.attisdropped AND c.relkind IN ('r', 'p', 'v', 'm', 'f') \
           AND ($1 OR (n.nspname NOT IN ('pg_catalog', 'information_schema') \
                       AND n.nspname NOT LIKE 'pg\\_toast%' AND n.nspname NOT LIKE 'pg\\_temp%')) \
         ORDER BY (n.nspname = ANY (current_schemas(false))) DESC, n.nspname, c.relname, a.attnum \
         LIMIT $2",
        &[
            &request.scope.system_schemas,
            &(protocol::MAX_COMPLETION_COLUMNS as i64),
        ],
    )
    .map_err(clean)?;
    // The schemas the session looks into come first, so that what is cut, if
    // anything is, is what is far away.
    let list: Vec<protocol::TableColumn> = rows
        .iter()
        .map(|r| protocol::TableColumn {
            schema: r.get(0),
            table: r.get(1),
            name: r.get(2),
            kind: r.get(3),
        })
        .collect();
    // The foreign keys, for the `JOIN … ON` the completion proposes.
    let keys = internal_query(
        client,
        "SELECT n.nspname::text, c.relname::text, \
                (SELECT array_agg(a.attname::text ORDER BY k.ord) \
                   FROM unnest(con.conkey) WITH ORDINALITY k(num, ord) \
                   JOIN pg_attribute a ON a.attrelid = con.conrelid AND a.attnum = k.num), \
                rn.nspname::text, rc.relname::text, \
                (SELECT array_agg(a.attname::text ORDER BY k.ord) \
                   FROM unnest(con.confkey) WITH ORDINALITY k(num, ord) \
                   JOIN pg_attribute a ON a.attrelid = con.confrelid AND a.attnum = k.num) \
         FROM pg_constraint con \
         JOIN pg_class c ON c.oid = con.conrelid \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         JOIN pg_class rc ON rc.oid = con.confrelid \
         JOIN pg_namespace rn ON rn.oid = rc.relnamespace \
         WHERE con.contype = 'f' \
           AND ($1 OR (n.nspname NOT IN ('pg_catalog', 'information_schema') \
                       AND n.nspname NOT LIKE 'pg\\_toast%' AND n.nspname NOT LIKE 'pg\\_temp%')) \
         ORDER BY n.nspname, c.relname, con.conname \
         LIMIT 20000",
        &[&request.scope.system_schemas],
    )
    .map_err(clean)?;
    let foreign_keys: Vec<protocol::ForeignKey> = keys
        .iter()
        .map(|r| protocol::ForeignKey {
            schema: r.get(0),
            table: r.get(1),
            columns: r.get::<_, Option<Vec<String>>>(2).unwrap_or_default(),
            ref_schema: r.get(3),
            ref_table: r.get(4),
            ref_columns: r.get::<_, Option<Vec<String>>>(5).unwrap_or_default(),
        })
        .collect();
    Ok(json!({ "columns": list, "foreign_keys": foreign_keys }))
}

/// Puts the rows of a file into a table, all or none. A CSV goes through a
/// temporary table of texts (`COPY` reads it as it is), then each value is
/// converted to the type of its column; a JSON array of objects is read by the
/// server. What is not mapped is left to the column's default.
fn import(client: &mut Client, request: &Request) -> Result<Value, String> {
    use std::io::Read;
    let spec = request
        .import
        .as_ref()
        .ok_or_else(|| "nothing to import".to_string())?;
    if spec.mapping.is_empty() {
        return Err("no column to fill".to_string());
    }
    let target = format!(
        "{}.{}",
        protocol::quote_ident(&request.schema),
        protocol::quote_ident(&request.table)
    );
    let columns: Vec<String> = spec
        .mapping
        .iter()
        .map(|m| protocol::quote_ident(&m.column))
        .collect();
    let mut file =
        std::fs::File::open(&request.path).map_err(|e| format!("{}: {e}", request.path))?;
    let mut transaction = client.transaction().map_err(clean)?;
    let started = Instant::now();
    let insert = if spec.format == "json" {
        let mut text = String::new();
        file.read_to_string(&mut text).map_err(|e| e.to_string())?;
        let values: Vec<String> = spec
            .mapping
            .iter()
            .map(|m| {
                let json = matches!(m.kind.as_str(), "json" | "jsonb");
                format!(
                    "(e.j {} {})::{}",
                    if json { "->" } else { "->>" },
                    protocol::quote_literal(&m.key),
                    m.kind
                )
            })
            .collect();
        let sql = format!(
            "INSERT INTO {target} ({}) SELECT {} FROM jsonb_array_elements($1::text::jsonb) AS e(j)",
            columns.join(", "),
            values.join(", ")
        );
        match transaction.execute(&sql, &[&text]) {
            Ok(n) => {
                record(&sql, started, Some(n), None);
                n
            }
            Err(e) => {
                let message = clean(e);
                record(&sql, started, None, Some(message.clone()));
                return Err(message);
            }
        }
    } else {
        let width = spec.file_columns.max(1);
        let names: Vec<String> = (0..width).map(|i| format!("c{i}")).collect();
        let create = format!(
            "CREATE TEMP TABLE _se_import ({}) ON COMMIT DROP",
            names
                .iter()
                .map(|n| format!("{n} text"))
                .collect::<Vec<_>>()
                .join(", ")
        );
        transaction.batch_execute(&create).map_err(clean)?;
        record_internal(&create, started, None, None);
        let delimiter = spec.delimiter.chars().next().unwrap_or(',');
        let copy = format!(
            "COPY _se_import FROM STDIN WITH (FORMAT csv, DELIMITER {}, HEADER {})",
            protocol::quote_literal(&delimiter.to_string()),
            if spec.header { "true" } else { "false" }
        );
        let started = Instant::now();
        let loaded = (|| -> Result<u64, String> {
            let mut writer = transaction.copy_in(copy.as_str()).map_err(clean)?;
            std::io::copy(&mut file, &mut writer).map_err(|e| clean_copy(&e))?;
            writer.finish().map_err(clean)
        })();
        match &loaded {
            Ok(n) => record(&copy, started, Some(*n), None),
            Err(e) => record(&copy, started, None, Some(e.clone())),
        }
        loaded?;
        let values: Vec<String> = spec
            .mapping
            .iter()
            .map(|m| {
                let name = format!("c{}", m.index);
                if spec.empty_null {
                    format!("NULLIF({name}, '')::{}", m.kind)
                } else {
                    format!("{name}::{}", m.kind)
                }
            })
            .collect();
        let sql = format!(
            "INSERT INTO {target} ({}) SELECT {} FROM _se_import",
            columns.join(", "),
            values.join(", ")
        );
        let started = Instant::now();
        match transaction.execute(&sql, &[]) {
            Ok(n) => {
                record(&sql, started, Some(n), None);
                n
            }
            Err(e) => {
                let message = clean(e);
                record(&sql, started, None, Some(message.clone()));
                return Err(message);
            }
        }
    };
    transaction.commit().map_err(clean)?;
    Ok(json!({ "imported": insert }))
}

/// The text of an error met while copying a file in.
fn clean_copy(error: &std::io::Error) -> String {
    let mut message = error.to_string();
    let mut source = std::error::Error::source(error);
    while let Some(inner) = source {
        message = inner.to_string();
        source = inner.source();
    }
    message
}

/// The directories where the PostgreSQL tools may be, besides the `PATH`.
fn tool_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    for fixed in [
        "/opt/homebrew/bin",
        "/usr/local/bin",
        "/usr/bin",
        "/Applications/Postgres.app/Contents/Versions/latest/bin",
    ] {
        dirs.push(PathBuf::from(fixed));
    }
    // Homebrew keeps one directory per version, and Debian too.
    for (parent, prefix) in [
        ("/opt/homebrew/opt", "postgresql@"),
        ("/usr/local/opt", "postgresql@"),
        ("/usr/lib/postgresql", ""),
    ] {
        if let Ok(entries) = std::fs::read_dir(parent) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with(prefix) {
                    dirs.push(entry.path().join("bin"));
                }
            }
        }
    }
    dirs
}

/// The newest of the installed copies of a tool: one older than the server
/// refuses to work with it.
fn find_tool(name: &str) -> Result<(PathBuf, String), String> {
    let mut best: Option<(Vec<u32>, PathBuf, String)> = None;
    for dir in tool_dirs() {
        let path = dir.join(name);
        if !path.is_file() {
            continue;
        }
        let Ok(output) = Command::new(&path).arg("--version").output() else {
            continue;
        };
        let said = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let number: Vec<u32> = said
            .split_whitespace()
            .last()
            .unwrap_or("")
            .split(|c: char| !c.is_ascii_digit())
            .filter_map(|n| n.parse().ok())
            .collect();
        if best.as_ref().is_none_or(|(b, _, _)| number > *b) {
            best = Some((number, path, said));
        }
    }
    best.map(|(_, path, said)| (path, said)).ok_or_else(|| {
        format!(
            "{name} was not found: install the PostgreSQL client tools (for example `brew install libpq`)"
        )
    })
}

/// What a tool needs to reach the server, in its environment (the password
/// never goes on a command line).
fn tool_environment(
    target: &Target,
    database: &str,
    tunnel_port: Option<u16>,
) -> Vec<(String, String)> {
    let mut env = vec![
        ("PGHOST".to_string(), target.host.clone()),
        ("PGPORT".to_string(), target.port.to_string()),
        (
            "PGDATABASE".to_string(),
            if database.is_empty() {
                target.database.clone()
            } else {
                database.to_string()
            },
        ),
        ("PGCONNECT_TIMEOUT".to_string(), "20".to_string()),
    ];
    if !target.user.is_empty() {
        env.push(("PGUSER".to_string(), target.user.clone()));
    }
    // The tunnel: the address to knock at changes, the name that the
    // certificate is checked against does not.
    if let Some(port) = tunnel_port {
        env.push(("PGHOSTADDR".to_string(), "127.0.0.1".to_string()));
        env.push(("PGPORT".to_string(), port.to_string()));
    }
    if target.auth == Auth::UserPassword && !target.password.is_empty() {
        env.push(("PGPASSWORD".to_string(), target.password.clone()));
    }
    let mode = match target.ssl.mode {
        SslMode::Disable => "disable",
        SslMode::Prefer => "prefer",
        SslMode::Require => "require",
        SslMode::VerifyCa => "verify-ca",
        SslMode::VerifyFull => "verify-full",
    };
    env.push(("PGSSLMODE".to_string(), mode.to_string()));
    for (key, value) in [
        ("PGSSLROOTCERT", &target.ssl.ca_file),
        ("PGSSLCERT", &target.ssl.client_cert),
        ("PGSSLKEY", &target.ssl.client_key),
    ] {
        if !value.is_empty() {
            env.push((key.to_string(), value.clone()));
        }
    }
    env
}

/// What a tool said that matters: its lines of complaint, or else its last
/// lines (the first ones are progress, and a `Command was:` is the SQL it ran).
fn said_of(said: &str) -> String {
    let complaints: Vec<&str> = said
        .lines()
        .filter(|l| {
            let l = l.to_lowercase();
            l.contains("error") || l.contains("fatal") || l.contains("could not")
        })
        .take(4)
        .collect();
    if !complaints.is_empty() {
        return complaints.join("\n");
    }
    let lines: Vec<&str> = said.lines().collect();
    lines[lines.len().saturating_sub(8)..].join("\n")
}

/// Runs a tool and says how it ended: its last lines of complaint when it
/// failed.
fn run_tool(program: &Path, args: &[String], env: &[(String, String)]) -> Result<String, String> {
    let shown = format!(
        "{} {}",
        program
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().to_string()),
        args.join(" ")
    );
    let started = Instant::now();
    let output = Command::new(program)
        .args(args)
        .envs(env.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("{}: {e}", program.display()))?;
    let said = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let tail = said_of(&said);
    if output.status.success() {
        record(&shown, started, None, None);
        Ok(tail)
    } else {
        record(&shown, started, None, Some(tail.clone()));
        Err(if tail.is_empty() {
            format!("{} failed", program.display())
        } else {
            tail
        })
    }
}

/// Writes a copy of the database (or of a schema, or of a table) to a file.
fn dump(request: &Request, tunnel_port: Option<u16>) -> Result<Value, String> {
    let spec = request.tool.clone().unwrap_or_default();
    if request.path.is_empty() {
        return Err("no file to write".to_string());
    }
    let (program, version) = find_tool("pg_dump")?;
    let mut args: Vec<String> = vec![
        "--no-password".into(),
        "--file".into(),
        request.path.clone(),
        if spec.format == "plain" {
            "--format=plain"
        } else {
            "--format=custom"
        }
        .into(),
    ];
    for (on, flag) in [
        (spec.schema_only, "--schema-only"),
        (spec.data_only, "--data-only"),
        (spec.no_owner, "--no-owner"),
        (spec.no_privileges, "--no-privileges"),
        (spec.clean && spec.format == "plain", "--clean"),
    ] {
        if on {
            args.push(flag.into());
        }
    }
    if !request.table.is_empty() {
        args.push("--table".into());
        args.push(format!(
            "{}.{}",
            protocol::quote_ident(&request.schema),
            protocol::quote_ident(&request.table)
        ));
    } else if !request.schema.is_empty() {
        args.push("--schema".into());
        args.push(protocol::quote_ident(&request.schema));
    }
    let env = tool_environment(&request.target, &request.database, tunnel_port);
    let said = run_tool(&program, &args, &env)?;
    let size = std::fs::metadata(&request.path).map_or(0, |m| m.len());
    Ok(json!({ "path": request.path, "tool": version, "size": size, "said": said }))
}

/// How a backup file is made: `pg_restore` reads the custom and tar formats,
/// `psql` the SQL ones.
fn backup_kind(path: &str) -> Result<&'static str, String> {
    let mut file = std::fs::File::open(path).map_err(|e| format!("{path}: {e}"))?;
    let mut head = [0u8; 512];
    let read = file.read(&mut head).map_err(|e| e.to_string())?;
    if head[..read.min(5)] == *b"PGDMP" {
        Ok("custom")
    } else if read >= 262 && &head[257..262] == b"ustar" {
        Ok("tar")
    } else {
        Ok("sql")
    }
}

/// Puts a backup back into the database.
fn restore(request: &Request, tunnel_port: Option<u16>) -> Result<Value, String> {
    let spec = request.tool.clone().unwrap_or_default();
    if request.path.is_empty() {
        return Err("no file to read".to_string());
    }
    let kind = backup_kind(&request.path)?;
    let env = tool_environment(&request.target, &request.database, tunnel_port);
    let (program, version, args) = if kind == "sql" {
        let (program, version) = find_tool("psql")?;
        let mut args: Vec<String> = vec![
            "--no-password".into(),
            "--quiet".into(),
            "-v".into(),
            "ON_ERROR_STOP=1".into(),
        ];
        if spec.single_transaction {
            args.push("--single-transaction".into());
        }
        args.push("--file".into());
        args.push(request.path.clone());
        (program, version, args)
    } else {
        let (program, version) = find_tool("pg_restore")?;
        let mut args: Vec<String> = vec![
            "--no-password".into(),
            "--dbname".into(),
            if request.database.is_empty() {
                request.target.database.clone()
            } else {
                request.database.clone()
            },
            "--exit-on-error".into(),
        ];
        for (on, flag) in [
            (spec.schema_only, "--schema-only"),
            (spec.data_only, "--data-only"),
            (spec.no_owner, "--no-owner"),
            (spec.no_privileges, "--no-privileges"),
            (spec.clean, "--clean"),
            (spec.clean, "--if-exists"),
            (spec.single_transaction, "--single-transaction"),
        ] {
            if on {
                args.push(flag.into());
            }
        }
        args.push(request.path.clone());
        (program, version, args)
    };
    let said = run_tool(&program, &args, &env)?;
    Ok(json!({ "path": request.path, "tool": version, "kind": kind, "said": said }))
}

/// How the database is doing: the size of its tables and indexes, what waits
/// to be cleaned, and the indexes nobody reads.
fn health(client: &mut Client, request: &Request) -> Result<Value, String> {
    let user_schemas = "($1 OR (n.nspname NOT IN ('pg_catalog', 'information_schema') \
                         AND n.nspname NOT LIKE 'pg\\_toast%' AND n.nspname NOT LIKE 'pg\\_temp%'))";
    let size = internal_query(client, "SELECT pg_database_size(current_database())", &[])
        .map_err(clean)?;
    let database_size: i64 = size.first().map_or(0, |r| r.get(0));
    let tables = internal_query(
        client,
        &format!(
            "SELECT n.nspname::text, c.relname::text, GREATEST(c.reltuples, 0)::bigint, \
                    pg_table_size(c.oid), pg_indexes_size(c.oid), pg_total_relation_size(c.oid), \
                    COALESCE(s.n_dead_tup, 0)::bigint, \
                    COALESCE(to_char(GREATEST(s.last_vacuum, s.last_autovacuum), 'YYYY-MM-DD HH24:MI'), ''), \
                    COALESCE(to_char(GREATEST(s.last_analyze, s.last_autoanalyze), 'YYYY-MM-DD HH24:MI'), '') \
             FROM pg_class c \
             JOIN pg_namespace n ON n.oid = c.relnamespace \
             LEFT JOIN pg_stat_all_tables s ON s.relid = c.oid \
             WHERE c.relkind IN ('r', 'p', 'm') AND {user_schemas} \
             ORDER BY pg_total_relation_size(c.oid) DESC, n.nspname, c.relname \
             LIMIT 500"
        ),
        &[&request.scope.system_schemas],
    )
    .map_err(clean)?;
    let indexes = internal_query(
        client,
        &format!(
            "SELECT n.nspname::text, t.relname::text, i.relname::text, pg_relation_size(i.oid), \
                    COALESCE(s.idx_scan, 0)::bigint, x.indisunique, x.indisprimary \
             FROM pg_index x \
             JOIN pg_class i ON i.oid = x.indexrelid \
             JOIN pg_class t ON t.oid = x.indrelid \
             JOIN pg_namespace n ON n.oid = t.relnamespace \
             LEFT JOIN pg_stat_all_indexes s ON s.indexrelid = i.oid \
             WHERE {user_schemas} \
             ORDER BY pg_relation_size(i.oid) DESC, n.nspname, i.relname \
             LIMIT 500"
        ),
        &[&request.scope.system_schemas],
    )
    .map_err(clean)?;
    let health = protocol::Health {
        database_size,
        tables: tables
            .iter()
            .map(|r| protocol::TableHealth {
                schema: r.get(0),
                name: r.get(1),
                rows: r.get(2),
                table_size: r.get(3),
                indexes_size: r.get(4),
                total_size: r.get(5),
                dead_rows: r.get(6),
                last_vacuum: r.get(7),
                last_analyze: r.get(8),
            })
            .collect(),
        indexes: indexes
            .iter()
            .map(|r| protocol::IndexHealth {
                schema: r.get(0),
                table: r.get(1),
                name: r.get(2),
                size: r.get(3),
                scans: r.get(4),
                unique: r.get(5),
                primary: r.get(6),
            })
            .collect(),
    };
    Ok(json!({ "health": health }))
}

/// The columns of a table that take their values from a list: the labels of
/// an enumeration, or the keys of the table a one-column foreign key points to.
fn choices(client: &mut Client, request: &Request) -> Result<Value, String> {
    let (schema, table) = (request.schema.as_str(), request.table.as_str());
    let mut out: Vec<protocol::Choices> = Vec::new();
    let enums = internal_query(
        client,
        "SELECT a.attname::text, array_agg(e.enumlabel::text ORDER BY e.enumsortorder) \
         FROM pg_attribute a \
         JOIN pg_enum e ON e.enumtypid = a.atttypid \
         JOIN pg_class c ON c.oid = a.attrelid \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         WHERE n.nspname = $1 AND c.relname = $2 AND a.attnum > 0 AND NOT a.attisdropped \
         GROUP BY a.attnum, a.attname ORDER BY a.attnum",
        &[&schema, &table],
    )
    .map_err(clean)?;
    for row in &enums {
        out.push(protocol::Choices {
            column: row.get(0),
            kind: "enum".into(),
            values: row.get::<_, Vec<String>>(1),
            more: false,
        });
    }
    let keys = internal_query(
        client,
        "SELECT a.attname::text, n2.nspname::text, c2.relname::text, a2.attname::text \
         FROM pg_constraint con \
         JOIN pg_class c ON c.oid = con.conrelid \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         JOIN pg_attribute a ON a.attrelid = con.conrelid AND a.attnum = con.conkey[1] \
         JOIN pg_class c2 ON c2.oid = con.confrelid \
         JOIN pg_namespace n2 ON n2.oid = c2.relnamespace \
         JOIN pg_attribute a2 ON a2.attrelid = con.confrelid AND a2.attnum = con.confkey[1] \
         WHERE con.contype = 'f' AND array_length(con.conkey, 1) = 1 \
           AND n.nspname = $1 AND c.relname = $2 \
         ORDER BY con.conname",
        &[&schema, &table],
    )
    .map_err(clean)?;
    for row in &keys {
        let column: String = row.get(0);
        if out.iter().any(|c| c.column == column) {
            continue;
        }
        let (ref_schema, ref_table, ref_column): (String, String, String) =
            (row.get(1), row.get(2), row.get(3));
        let select = format!(
            "SELECT DISTINCT {col}::text FROM {sch}.{tbl} WHERE {col} IS NOT NULL ORDER BY 1 LIMIT {limit}",
            col = protocol::quote_ident(&ref_column),
            sch = protocol::quote_ident(&ref_schema),
            tbl = protocol::quote_ident(&ref_table),
            limit = protocol::MAX_CHOICES + 1,
        );
        // A table that cannot be read gives no list, not an error.
        let Ok(found) = internal_query(client, &select, &[]) else {
            continue;
        };
        let mut values: Vec<String> = found.iter().map(|r| r.get(0)).collect();
        let more = values.len() > protocol::MAX_CHOICES;
        values.truncate(protocol::MAX_CHOICES);
        out.push(protocol::Choices {
            column,
            kind: "fk".into(),
            values,
            more,
        });
    }
    Ok(json!({ "choices": out }))
}

/// Runs the statements one after the other, in one session: the rows they
/// give (up to a limit), or how many rows they touched. Stops at the first
/// that fails and says which.
fn query(client: &mut Client, request: &Request) -> Result<Value, String> {
    let mut results: Vec<QueryResult> = Vec::new();
    let mut failed: Option<Value> = None;
    // All or none: the statements are one transaction of their own.
    let wrapped = request.transaction && !request.target.read_only;
    if wrapped {
        let started = Instant::now();
        client.batch_execute("BEGIN").map_err(clean)?;
        record_internal("BEGIN", started, None, None);
    }
    for (index, sql) in request.statements.iter().enumerate() {
        if request.target.read_only && !protocol::reads_only(sql) {
            let message = "this data source is read-only".to_string();
            record(sql, Instant::now(), None, Some(message.clone()));
            failed = Some(json!({ "index": index, "error": message }));
            break;
        }
        // What the statement gives, without running it: a reading statement
        // is left to the tab that pages it. A statement the server refuses
        // to prepare is refused here, with its own error (after one, an open
        // transaction would only answer that it is aborted).
        let started = Instant::now();
        // Several commands in one text can't be prepared, only run.
        let statement = if protocol::is_multiple(sql) {
            None
        } else {
            match client.prepare(sql) {
                Ok(statement) => Some(statement),
                Err(e) => {
                    let position = position_of(&e);
                    let message = clean(e);
                    record(sql, started, None, Some(message.clone()));
                    failed =
                        Some(json!({ "index": index, "error": message, "position": position }));
                    break;
                }
            }
        };
        if let Some(statement) =
            statement.filter(|s| !s.columns().is_empty() && protocol::wrappable(sql))
        {
            results.push(QueryResult {
                sql: sql.clone(),
                ms: 0,
                has_rows: true,
                columns: statement
                    .columns()
                    .iter()
                    .map(|c| c.name().to_string())
                    .collect(),
                rows: Vec::new(),
                affected: 0,
                truncated: false,
                deferred: true,
            });
            continue;
        }
        let started = Instant::now();
        let messages = match client.simple_query(sql) {
            Ok(messages) => messages,
            Err(e) => {
                let position = position_of(&e);
                let message = clean(e);
                record(sql, started, None, Some(message.clone()));
                failed = Some(json!({ "index": index, "error": message, "position": position }));
                break;
            }
        };
        let mut result = QueryResult {
            sql: sql.clone(),
            ms: started.elapsed().as_millis() as u64,
            has_rows: false,
            columns: Vec::new(),
            rows: Vec::new(),
            affected: 0,
            truncated: false,
            deferred: false,
        };
        let mut seen = 0usize;
        for message in messages {
            match message {
                postgres::SimpleQueryMessage::RowDescription(columns) => {
                    result.has_rows = true;
                    result.columns = columns.iter().map(|c| c.name().to_string()).collect();
                }
                postgres::SimpleQueryMessage::Row(row) => {
                    result.has_rows = true;
                    if result.columns.is_empty() {
                        result.columns =
                            row.columns().iter().map(|c| c.name().to_string()).collect();
                    }
                    seen += 1;
                    if result.rows.len() < MAX_RESULT_ROWS {
                        result.rows.push(
                            (0..row.len())
                                .map(|i| row.get(i).map(str::to_string))
                                .collect(),
                        );
                    } else {
                        result.truncated = true;
                    }
                }
                postgres::SimpleQueryMessage::CommandComplete(n) => result.affected = n,
                _ => {}
            }
        }
        let count = if result.has_rows {
            seen as u64
        } else {
            result.affected
        };
        record(sql, started, Some(count), None);
        results.push(result);
    }
    if wrapped {
        let started = Instant::now();
        let end = if failed.is_some() {
            "ROLLBACK"
        } else {
            "COMMIT"
        };
        // The commit itself can fail (a deferred constraint): then nothing was
        // made, whatever the statements said.
        match client.batch_execute(end) {
            Ok(()) => record_internal(end, started, None, None),
            Err(e) => {
                let message = clean(e);
                record_internal(end, started, None, Some(message.clone()));
                return Err(message);
            }
        }
    }
    Ok(json!({ "results": results, "failed": failed }))
}

/// Writes the rows of a table or a query (all of them, with the filter and
/// the sort of the tab) to a file, as CSV or as a JSON array. The server
/// streams them, so nothing big is held in memory.
fn export(client: &mut Client, request: &Request) -> Result<Value, String> {
    let select = if request.sql.trim().is_empty() {
        let (schema, table) = (request.schema.as_str(), request.table.as_str());
        let shape = shape(client, schema, table)?;
        let names: Vec<&str> = shape.columns.iter().map(|c| c.name.as_str()).collect();
        let tiebreak: Vec<String> = if shape.is_table && !shape.primary_key.is_empty() {
            shape.primary_key.clone()
        } else {
            Vec::new()
        };
        format!(
            "SELECT * FROM {}.{}{}",
            quote(schema),
            quote(table),
            clauses(request, &names, &tiebreak, "")?
        )
    } else {
        let sql = request.sql.trim().trim_end_matches(';').trim().to_string();
        if !protocol::wrappable(&sql) {
            return Err("this query cannot be exported".to_string());
        }
        let statement = client.prepare(&sql).map_err(clean)?;
        let names: Vec<&str> = statement.columns().iter().map(|c| c.name()).collect();
        format!(
            "SELECT * FROM (\n{sql}\n) AS \"q\"{}",
            clauses(request, &names, &[], "")?
        )
    };
    let json = request.format == "json";
    let copy = if json {
        // One JSON object per line, between markers that JSON never holds.
        format!(
            "COPY (SELECT to_json(\"e\")::text FROM ({select}) AS \"e\") TO STDOUT \
             WITH (FORMAT csv, DELIMITER E'\\x02', QUOTE E'\\x01')"
        )
    } else {
        format!("COPY ({select}) TO STDOUT WITH (FORMAT csv, HEADER true)")
    };
    let started = Instant::now();
    let done = (|| -> Result<u64, String> {
        let file =
            std::fs::File::create(&request.path).map_err(|e| format!("{}: {e}", request.path))?;
        let mut out = std::io::BufWriter::new(file);
        let mut reader = client.copy_out(copy.as_str()).map_err(clean)?;
        let mut rows = 0u64;
        if json {
            out.write_all(b"[").map_err(|e| e.to_string())?;
            let mut lines = std::io::BufReader::new(&mut reader);
            let mut line = String::new();
            loop {
                line.clear();
                if lines.read_line(&mut line).map_err(|e| e.to_string())? == 0 {
                    break;
                }
                let text = line.trim_end_matches(['\n', '\r']);
                out.write_all(if rows == 0 { b"\n  " } else { b",\n  " })
                    .and_then(|_| out.write_all(text.as_bytes()))
                    .map_err(|e| e.to_string())?;
                rows += 1;
            }
            out.write_all(if rows == 0 { b"]\n" } else { b"\n]\n" })
                .map_err(|e| e.to_string())?;
        } else {
            std::io::copy(&mut reader, &mut out).map_err(|e| e.to_string())?;
        }
        out.flush().map_err(|e| e.to_string())?;
        Ok(rows)
    })();
    match done {
        Ok(rows) => {
            record(&select, started, json.then_some(rows), None);
            Ok(json!({ "exported": rows, "path": request.path }))
        }
        Err(message) => {
            record(&select, started, None, Some(message.clone()));
            // A file half written is of no use.
            let _ = std::fs::remove_file(&request.path);
            Err(message)
        }
    }
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
