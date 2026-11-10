//! The MySQL and MariaDB driver of SimpleEdit's Database panel: a separate
//! program, as the PostgreSQL one, speaking the same protocol. A MySQL
//! "database" is what the tree calls a schema: the server is the one node
//! above them.

#[allow(dead_code)]
#[path = "../../src/database/protocol.rs"]
mod protocol;

#[path = "../common/mod.rs"]
mod common;
#[path = "../common/tunnel.rs"]
mod tunnel;

use common::{Engine, Row, Shape};
use mysql::consts::{CapabilityFlags, ColumnType};
use mysql::prelude::Queryable;
use mysql::{Conn, OptsBuilder, SslOpts, Value};
use protocol::{
    Auth, Column, ColumnDef, ConstraintDef, ConstraintKind, Database, ForeignKey, IndexDef, Kind,
    Reference, Request, Routine, Schema, SslMode, Structure, Table, TableColumn,
};

fn main() {
    common::main_loop(|request| Session::open(request).map(|s| Box::new(s) as Box<dyn Engine>));
}

/// The schemas that belong to the server itself.
const SYSTEM: &str = "'information_schema', 'mysql', 'performance_schema', 'sys'";

struct Session {
    conn: Conn,
    /// The name of the node above the schemas.
    server: String,
    /// The database the connection names, if it names one.
    current: String,
    /// A transaction is open (the driver's own, or one the user began).
    in_tx: bool,
    _tunnel: Option<tunnel::Tunnel>,
}

/// Whether a transaction is open once `sql` has run: the client cannot read
/// the server's flag, so the statement says. A DDL statement commits
/// implicitly, and `SET autocommit = 0` opens a transaction that stays open.
fn tx_after(open: bool, sql: &str) -> bool {
    let lower = sql.trim_start().to_lowercase();
    let squashed: String = lower.split_whitespace().collect::<Vec<_>>().join(" ");
    let first = squashed.split([' ', '(', ';']).next().unwrap_or("");
    if first == "begin" || squashed.starts_with("start transaction") {
        true
    } else if first == "commit" || (first == "rollback" && !squashed.contains("savepoint")) {
        false
    } else if first == "set" {
        let compact = squashed.replace(' ', "");
        if compact.contains("autocommit=0") || compact.contains("autocommit=off") {
            true
        } else if compact.contains("autocommit=1") || compact.contains("autocommit=on") {
            false
        } else {
            open
        }
    } else if matches!(
        first,
        "create"
            | "alter"
            | "drop"
            | "truncate"
            | "rename"
            | "grant"
            | "revoke"
            | "analyze"
            | "optimize"
            | "repair"
            | "lock"
            | "unlock"
    ) {
        false
    } else {
        open
    }
}

fn clean(error: mysql::Error) -> String {
    match error {
        mysql::Error::MySqlError(e) => e.message,
        other => other.to_string(),
    }
}

impl Session {
    fn open(request: &Request) -> Result<Session, String> {
        let target = &request.target;
        if target.host.trim().is_empty() {
            return Err("no host".to_string());
        }
        let tunnel = if target.ssh.enabled {
            Some(tunnel::Tunnel::open(target)?)
        } else {
            None
        };
        let (host, port) = match &tunnel {
            Some(t) => ("127.0.0.1".to_string(), t.port),
            None => (target.host.trim().to_string(), target.port),
        };
        let password = match target.auth {
            Auth::UserPassword if !target.password.is_empty() => Some(target.password.clone()),
            Auth::Pgpass => return Err("~/.pgpass is for PostgreSQL".to_string()),
            _ => None,
        };
        let mut options = OptsBuilder::new()
            .ip_or_hostname(Some(host))
            .tcp_port(port)
            .user(Some(target.user.clone()).filter(|u| !u.is_empty()))
            .pass(password)
            .db_name(Some(target.database.clone()).filter(|d| !d.is_empty()))
            .prefer_socket(false)
            .tcp_connect_timeout(Some(std::time::Duration::from_secs(15)))
            // The rows a change finds, not only those whose value differs:
            // `2.50` saved again as `2.5` touches its row, it is not taken for
            // a row changed by someone else.
            .additional_capabilities(CapabilityFlags::CLIENT_FOUND_ROWS);
        let ssl = &target.ssl;
        if ssl.mode != SslMode::Disable {
            let mut opts = SslOpts::default();
            match ssl.mode {
                SslMode::VerifyFull | SslMode::VerifyCa => {
                    if !ssl.ca_file.trim().is_empty() {
                        opts = opts.with_root_cert_path(Some(std::path::PathBuf::from(
                            ssl.ca_file.trim(),
                        )));
                    }
                    // Through a tunnel the connection goes to 127.0.0.1, a
                    // name no certificate has: the chain is still checked.
                    if ssl.mode == SslMode::VerifyCa || tunnel.is_some() {
                        opts = opts.with_danger_skip_domain_validation(true);
                    }
                }
                _ => {
                    opts = opts
                        .with_danger_accept_invalid_certs(true)
                        .with_danger_skip_domain_validation(true);
                }
            }
            if !ssl.client_cert.trim().is_empty() && !ssl.client_key.trim().is_empty() {
                opts = opts.with_client_identity(Some(mysql::ClientIdentity::new(
                    std::path::PathBuf::from(ssl.client_cert.trim()),
                    std::path::PathBuf::from(ssl.client_key.trim()),
                )));
            }
            options = options.ssl_opts(Some(opts));
        }
        let mut conn = match Conn::new(options.clone()) {
            Ok(conn) => conn,
            // `prefer`: plain when the server does not do TLS.
            Err(_) if ssl.mode == SslMode::Prefer => {
                Conn::new(options.ssl_opts(None::<SslOpts>)).map_err(clean)?
            }
            Err(e) => return Err(clean(e)),
        };
        let mut settings = Vec::new();
        if !target.time_zone.trim().is_empty() {
            settings.push(format!(
                "SET time_zone = '{}'",
                target.time_zone.replace('\'', "''")
            ));
        }
        if target.read_only {
            settings.push("SET SESSION TRANSACTION READ ONLY".to_string());
        }
        for sql in settings {
            conn.query_drop(&sql).map_err(clean)?;
        }
        if target.query_timeout > 0 {
            // MySQL counts milliseconds, MariaDB seconds; each ignores the other's.
            let _ = conn.query_drop(format!(
                "SET SESSION max_execution_time = {}",
                target.query_timeout * 1000
            ));
            let _ = conn.query_drop(format!(
                "SET SESSION max_statement_time = {}",
                target.query_timeout
            ));
        }
        if !target.startup_script.trim().is_empty() {
            conn.query_drop(&target.startup_script).map_err(clean)?;
        }
        Ok(Session {
            conn,
            server: target.host.trim().to_string(),
            current: target.database.trim().to_string(),
            in_tx: false,
            _tunnel: tunnel,
        })
    }

    /// The rows a statement gives, as text, with the columns that are numbers.
    fn rows_of(
        &mut self,
        sql: &str,
        limit: Option<usize>,
        mut each: impl FnMut(&[String], &[bool], Row) -> Result<(), String>,
    ) -> Result<(Vec<String>, bool), String> {
        let mut result = self.conn.query_iter(sql).map_err(clean)?;
        let mut names: Vec<String> = Vec::new();
        let numbers: Vec<bool>;
        let mut count = 0usize;
        let mut more = false;
        if let Some(set) = result.iter() {
            let columns = set.columns();
            names = columns
                .as_ref()
                .iter()
                .map(|c| c.name_str().into_owned())
                .collect();
            numbers = columns
                .as_ref()
                .iter()
                .map(|c| is_number(c.column_type()))
                .collect();
            for row in set {
                let row = row.map_err(clean)?;
                if limit.is_some_and(|l| count > l) {
                    more = true;
                    break;
                }
                count += 1;
                let cells: Row = row.unwrap().iter().map(text_of).collect();
                each(&names, &numbers, cells)?;
            }
        }
        Ok((names, more))
    }

    /// The rows of an internal query, as text.
    fn internal(&mut self, sql: &str) -> Result<Vec<Row>, String> {
        let started = std::time::Instant::now();
        let mut out = Vec::new();
        let done = self.rows_of(sql, None, |_, _, row| {
            out.push(row);
            Ok(())
        });
        common::record_internal(
            sql,
            started,
            Some(out.len() as u64),
            done.as_ref().err().cloned(),
        );
        done.map(|_| out)
    }
}

fn is_number(kind: ColumnType) -> bool {
    use ColumnType::*;
    matches!(
        kind,
        MYSQL_TYPE_TINY
            | MYSQL_TYPE_SHORT
            | MYSQL_TYPE_LONG
            | MYSQL_TYPE_LONGLONG
            | MYSQL_TYPE_INT24
            | MYSQL_TYPE_FLOAT
            | MYSQL_TYPE_DOUBLE
            | MYSQL_TYPE_DECIMAL
            | MYSQL_TYPE_NEWDECIMAL
            | MYSQL_TYPE_YEAR
    )
}

/// A value as text, NULL apart; bytes that are not text are shown in hex.
fn text_of(value: &Value) -> Option<String> {
    Some(match value {
        Value::NULL => return None,
        Value::Bytes(bytes) => match String::from_utf8(bytes.clone()) {
            Ok(text) => text,
            Err(_) => format!(
                "0x{}",
                bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
            ),
        },
        Value::Int(n) => n.to_string(),
        Value::UInt(n) => n.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Double(f) => f.to_string(),
        Value::Date(y, m, d, 0, 0, 0, 0) => format!("{y:04}-{m:02}-{d:02}"),
        Value::Date(y, m, d, h, i, s, 0) => format!("{y:04}-{m:02}-{d:02} {h:02}:{i:02}:{s:02}"),
        Value::Date(y, m, d, h, i, s, us) => {
            format!("{y:04}-{m:02}-{d:02} {h:02}:{i:02}:{s:02}.{us:06}")
        }
        Value::Time(neg, days, h, i, s, us) => {
            let hours = u32::from(*h) + days * 24;
            let sign = if *neg { "-" } else { "" };
            if *us == 0 {
                format!("{sign}{hours:02}:{i:02}:{s:02}")
            } else {
                format!("{sign}{hours:02}:{i:02}:{s:02}.{us:06}")
            }
        }
    })
}

fn quote(name: &str) -> String {
    protocol::Dialect::Mysql.quote(name)
}

/// A text as a MySQL literal: the backslash escapes too.
fn literal(text: &str) -> String {
    format!("'{}'", text.replace('\\', "\\\\").replace('\'', "''"))
}

fn cell(row: &Row, i: usize) -> String {
    row.get(i).cloned().flatten().unwrap_or_default()
}

impl Engine for Session {
    fn quote(&self, name: &str) -> String {
        quote(name)
    }

    fn literal(&self, text: &str) -> String {
        literal(text)
    }

    fn literal_for(&self, kind: &str, text: &str) -> String {
        // Bytes that are not text are shown as `0x…`: they go back as bytes.
        let kind = kind.to_lowercase();
        let binary = kind.contains("binary") || kind.contains("blob");
        match text.strip_prefix("0x") {
            Some(hex)
                if binary
                    && !hex.is_empty()
                    && hex.len() % 2 == 0
                    && hex.bytes().all(|b| b.is_ascii_hexdigit()) =>
            {
                format!("X'{hex}'")
            }
            _ => literal(text),
        }
    }

    fn same(&self, column: &str, literal: &str) -> String {
        format!("{column} <=> {literal}")
    }

    fn version(&mut self) -> Result<String, String> {
        let rows = self.internal("SELECT VERSION()")?;
        let version = cell(&rows[0], 0);
        Ok(if version.to_lowercase().contains("mariadb") {
            format!("MariaDB {version}")
        } else {
            format!("MySQL {version}")
        })
    }

    fn tree(&mut self, request: &Request) -> Result<Vec<Database>, String> {
        let filter = if request.scope.system_schemas {
            String::new()
        } else {
            format!(" WHERE table_schema NOT IN ({SYSTEM})")
        };
        let only = if request.scope.single_database && !self.current.is_empty() {
            let word = if filter.is_empty() { " WHERE" } else { " AND" };
            format!("{word} table_schema = {}", literal(&self.current))
        } else {
            String::new()
        };
        let names: Vec<String> = self
            .internal(&format!(
                "SELECT schema_name FROM information_schema.schemata{} ORDER BY schema_name",
                filter.replace("table_schema", "schema_name")
                    + &only.replace("table_schema", "schema_name")
            ))?
            .iter()
            .map(|r| cell(r, 0))
            .collect();
        let tables = self.internal(&format!(
            "SELECT table_schema, table_name, table_type FROM information_schema.tables{filter}{only} \
             ORDER BY table_schema, table_name"
        ))?;
        let routines = self.internal(&format!(
            "SELECT routine_schema, routine_name, routine_type FROM information_schema.routines{} \
             ORDER BY routine_schema, routine_name",
            (filter.replace("table_schema", "routine_schema")
                + &only.replace("table_schema", "routine_schema"))
        ))?;
        // Grouped by schema once, not searched for each schema.
        let mut tables_of: std::collections::HashMap<String, Vec<Table>> =
            std::collections::HashMap::new();
        for t in &tables {
            tables_of.entry(cell(t, 0)).or_default().push(Table {
                name: cell(t, 1),
                kind: if cell(t, 2).contains("VIEW") {
                    Kind::View
                } else {
                    Kind::Table
                },
            });
        }
        let mut routines_of: std::collections::HashMap<String, Vec<Routine>> =
            std::collections::HashMap::new();
        for r in &routines {
            routines_of.entry(cell(r, 0)).or_default().push(Routine {
                name: cell(r, 1),
                args: String::new(),
                procedure: cell(r, 2) == "PROCEDURE",
            });
        }
        let schemas: Vec<Schema> = names
            .into_iter()
            .map(|name| Schema {
                tables: tables_of.remove(&name).unwrap_or_default(),
                routines: routines_of.remove(&name).unwrap_or_default(),
                name,
                ..Schema::default()
            })
            .collect();
        Ok(vec![Database {
            name: if self.current.is_empty() {
                self.server.clone()
            } else {
                self.current.clone()
            },
            current: true,
            schemas,
        }])
    }

    fn shape(&mut self, schema: &str, table: &str) -> Result<Shape, String> {
        let (s, t) = (literal(schema), literal(table));
        let columns = self.internal(&format!(
            "SELECT column_name, column_type, is_nullable FROM information_schema.columns \
             WHERE table_schema = {s} AND table_name = {t} ORDER BY ordinal_position"
        ))?;
        if columns.is_empty() {
            return Err(format!("{schema}.{table}: no such table"));
        }
        let keys = self.internal(&format!(
            "SELECT column_name FROM information_schema.statistics \
             WHERE table_schema = {s} AND table_name = {t} AND index_name = 'PRIMARY' \
             ORDER BY seq_in_index"
        ))?;
        let kind = self.internal(&format!(
            "SELECT table_type FROM information_schema.tables WHERE table_schema = {s} AND table_name = {t}"
        ))?;
        Ok(Shape {
            columns: columns
                .iter()
                .map(|r| Column {
                    name: cell(r, 0),
                    kind: cell(r, 1),
                    nullable: cell(r, 2) == "YES",
                })
                .collect(),
            primary_key: keys.iter().map(|r| cell(r, 0)).collect(),
            is_table: kind.first().is_some_and(|r| cell(r, 0) == "BASE TABLE"),
            row_id: None,
        })
    }

    fn fetch(&mut self, sql: &str, limit: usize) -> Result<(Vec<String>, Vec<Row>, bool), String> {
        let mut data = Vec::new();
        let (names, more) = self.rows_of(sql, Some(limit), |_, _, row| {
            data.push(row);
            Ok(())
        })?;
        Ok((names, data, more))
    }

    fn each(&mut self, sql: &str, sink: &mut common::Sink<'_>) -> Result<(), String> {
        self.rows_of(sql, None, sink).map(|_| ())
    }

    fn execute(&mut self, sql: &str) -> Result<u64, String> {
        self.conn.query_drop(sql).map_err(clean)?;
        let affected = self.conn.affected_rows();
        self.in_tx = tx_after(self.in_tx, sql);
        Ok(affected)
    }

    fn columns_of(&mut self, sql: &str) -> Result<Option<Vec<String>>, String> {
        // What cannot be prepared (a `SET`, a `BEGIN`) gives no rows: running
        // it says the rest, if it is wrong.
        let Ok(statement) = self.conn.prep(sql) else {
            return Ok(None);
        };
        let columns = statement.columns();
        Ok((!columns.is_empty())
            .then(|| columns.iter().map(|c| c.name_str().into_owned()).collect()))
    }

    fn begin(&mut self) -> Result<(), String> {
        if !self.in_tx {
            self.conn.query_drop("START TRANSACTION").map_err(clean)?;
            self.in_tx = true;
        }
        Ok(())
    }

    fn commit(&mut self) -> Result<(), String> {
        self.in_tx = false;
        self.conn.query_drop("COMMIT").map_err(clean)
    }

    fn rollback(&mut self) -> Result<(), String> {
        self.in_tx = false;
        self.conn.query_drop("ROLLBACK").map_err(clean)
    }

    fn in_transaction(&mut self) -> bool {
        self.in_tx
    }

    fn alive(&mut self) -> bool {
        self.conn.ping().is_ok()
    }

    fn session_id(&mut self) -> Option<i64> {
        Some(i64::from(self.conn.connection_id()))
    }

    fn cancel(&mut self, id: i64) -> Result<bool, String> {
        // From another connection: the driver of the session is stopped by
        // the app, but MySQL would finish the statement and keep what it did.
        match self.conn.query_drop(format!("KILL QUERY {id}")) {
            Ok(()) => Ok(true),
            // No such session: there is nothing left to stop.
            Err(mysql::Error::MySqlError(e)) if e.code == 1094 => Ok(false),
            Err(e) => Err(clean(e)),
        }
    }

    fn structure(&mut self, schema: &str, table: &str) -> Result<Structure, String> {
        let name = format!("{}.{}", quote(schema), quote(table));
        let (s, t) = (literal(schema), literal(table));
        let columns: Vec<ColumnDef> = self
            .internal(&format!("SHOW FULL COLUMNS FROM {name}"))?
            .iter()
            .map(|r| {
                let extra = cell(r, 6).to_lowercase();
                ColumnDef {
                    name: cell(r, 0),
                    kind: cell(r, 1),
                    nullable: cell(r, 3) == "YES",
                    default: r.get(5).cloned().flatten(),
                    identity: if extra.contains("auto_increment") {
                        "d".into()
                    } else {
                        String::new()
                    },
                    generated: extra.contains("generated"),
                    comment: cell(r, 8),
                }
            })
            .collect();
        let index_rows = self.internal(&format!("SHOW INDEX FROM {name}"))?;
        let mut names: Vec<String> = Vec::new();
        for r in &index_rows {
            let key = cell(r, 2);
            if !names.contains(&key) {
                names.push(key);
            }
        }
        let mut constraints = Vec::new();
        let mut indexes = Vec::new();
        for key in names {
            let mine: Vec<&Row> = index_rows.iter().filter(|r| cell(r, 2) == key).collect();
            let list = mine
                .iter()
                .map(|r| quote(&cell(r, 4)))
                .collect::<Vec<_>>()
                .join(", ");
            let unique = cell(mine[0], 1) == "0";
            if key == "PRIMARY" {
                constraints.push(ConstraintDef {
                    name: key,
                    kind: ConstraintKind::Primary,
                    definition: format!("PRIMARY KEY ({list})"),
                });
            } else {
                indexes.push(IndexDef {
                    definition: format!(
                        "CREATE {}INDEX {} ON {name} ({list})",
                        if unique { "UNIQUE " } else { "" },
                        quote(&key)
                    ),
                    name: key,
                    unique,
                });
            }
        }
        let mut foreign = |condition: String| -> Result<Vec<Row>, String> {
            self.internal(&format!(
                "SELECT k.constraint_name, k.table_schema, k.table_name, k.column_name, \
                        k.referenced_table_schema, k.referenced_table_name, k.referenced_column_name \
                 FROM information_schema.key_column_usage k \
                 WHERE k.referenced_table_name IS NOT NULL AND {condition} \
                 ORDER BY k.constraint_name, k.ordinal_position"
            ))
        };
        let describe = |rows: &[&Row]| {
            format!(
                "FOREIGN KEY ({}) REFERENCES {}.{} ({})",
                rows.iter()
                    .map(|r| quote(&cell(r, 3)))
                    .collect::<Vec<_>>()
                    .join(", "),
                quote(&cell(rows[0], 4)),
                quote(&cell(rows[0], 5)),
                rows.iter()
                    .map(|r| quote(&cell(r, 6)))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        let own = foreign(format!("k.table_schema = {s} AND k.table_name = {t}"))?;
        let mut seen: Vec<String> = Vec::new();
        for r in &own {
            let id = cell(r, 0);
            if seen.contains(&id) {
                continue;
            }
            seen.push(id.clone());
            let mine: Vec<&Row> = own.iter().filter(|o| cell(o, 0) == id).collect();
            constraints.push(ConstraintDef {
                name: id,
                kind: ConstraintKind::Foreign,
                definition: describe(&mine),
            });
        }
        let pointing = foreign(format!(
            "k.referenced_table_schema = {s} AND k.referenced_table_name = {t}"
        ))?;
        let mut referenced_by = Vec::new();
        let mut seen: Vec<(String, String)> = Vec::new();
        for r in &pointing {
            let id = (cell(r, 0), cell(r, 2));
            if seen.contains(&id) {
                continue;
            }
            seen.push(id.clone());
            let mine: Vec<&Row> = pointing
                .iter()
                .filter(|o| (cell(o, 0), cell(o, 2)) == id)
                .collect();
            referenced_by.push(Reference {
                name: id.0,
                schema: cell(r, 1),
                table: id.1,
                definition: describe(&mine),
            });
        }
        let comment = self
            .internal(&format!(
                "SELECT table_comment FROM information_schema.tables WHERE table_schema = {s} AND table_name = {t}"
            ))?
            .first()
            .map(|r| cell(r, 0))
            .unwrap_or_default();
        Ok(Structure {
            columns,
            constraints,
            indexes,
            comment,
            referenced_by,
            extra: Box::default(),
        })
    }

    fn definition(&mut self, request: &Request) -> Result<String, String> {
        let name = format!("{}.{}", quote(&request.schema), quote(&request.table));
        let (command, column) = match request.object.as_str() {
            "view" | "matview" => (format!("SHOW CREATE VIEW {name}"), 1),
            "function" => {
                let function = self.internal(&format!("SHOW CREATE FUNCTION {name}"));
                match function {
                    Ok(rows) => return Ok(format!("{};\n", cell(&rows[0], 2).trim_end())),
                    Err(_) => (format!("SHOW CREATE PROCEDURE {name}"), 2),
                }
            }
            _ => (format!("SHOW CREATE TABLE {name}"), 1),
        };
        let rows = self.internal(&command)?;
        let row = rows
            .first()
            .ok_or_else(|| format!("{name}: no definition"))?;
        Ok(format!(
            "{};\n",
            cell(row, column).trim_end().trim_end_matches(';')
        ))
    }

    fn catalog(
        &mut self,
        request: &Request,
    ) -> Result<(Vec<TableColumn>, Vec<ForeignKey>), String> {
        let filter = if request.scope.system_schemas {
            "1 = 1".to_string()
        } else {
            format!("table_schema NOT IN ({SYSTEM})")
        };
        let columns = self
            .internal(&format!(
                "SELECT table_schema, table_name, column_name, column_type \
                 FROM information_schema.columns WHERE {filter} \
                 ORDER BY table_schema, table_name, ordinal_position LIMIT {}",
                protocol::MAX_COMPLETION_COLUMNS
            ))?
            .iter()
            .map(|r| TableColumn {
                schema: cell(r, 0),
                table: cell(r, 1),
                name: cell(r, 2),
                kind: cell(r, 3),
            })
            .collect();
        let rows = self.internal(&format!(
            "SELECT table_schema, table_name, constraint_name, column_name, \
                    referenced_table_schema, referenced_table_name, referenced_column_name \
             FROM information_schema.key_column_usage \
             WHERE referenced_table_name IS NOT NULL AND {filter} \
             ORDER BY table_schema, table_name, constraint_name, ordinal_position LIMIT 20000"
        ))?;
        let mut keys: Vec<ForeignKey> = Vec::new();
        let mut last: Option<(String, String, String)> = None;
        for r in &rows {
            let id = (cell(r, 0), cell(r, 1), cell(r, 2));
            if last.as_ref() != Some(&id) {
                keys.push(ForeignKey {
                    schema: id.0.clone(),
                    table: id.1.clone(),
                    columns: Vec::new(),
                    ref_schema: cell(r, 4),
                    ref_table: cell(r, 5),
                    ref_columns: Vec::new(),
                });
                last = Some(id);
            }
            if let Some(key) = keys.last_mut() {
                key.columns.push(cell(r, 3));
                key.ref_columns.push(cell(r, 6));
            }
        }
        Ok((columns, keys))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value as Json};

    #[test]
    fn the_transaction_flag_follows_the_statements() {
        assert!(tx_after(false, "BEGIN"));
        assert!(tx_after(false, "set autocommit = 0"));
        assert!(!tx_after(true, "SET autocommit=1"));
        assert!(!tx_after(true, "CREATE TABLE t (a int)"));
        assert!(!tx_after(true, "rollback"));
        assert!(tx_after(true, "ROLLBACK TO SAVEPOINT s"));
        assert!(tx_after(true, "insert into t values (1)"));
        assert!(!tx_after(false, "select 1"));
    }

    #[test]
    fn values_are_shown_as_text_with_nulls_apart() {
        assert_eq!(text_of(&Value::NULL), None);
        assert_eq!(
            text_of(&Value::Bytes(b"caf\xc3\xa9".to_vec())).as_deref(),
            Some("café")
        );
        assert_eq!(
            text_of(&Value::Bytes(vec![0xff, 0x00])).as_deref(),
            Some("0xff00")
        );
        assert_eq!(text_of(&Value::Int(-5)).as_deref(), Some("-5"));
        assert_eq!(
            text_of(&Value::Date(2024, 1, 2, 0, 0, 0, 0)).as_deref(),
            Some("2024-01-02")
        );
        assert_eq!(
            text_of(&Value::Date(2024, 1, 2, 3, 4, 5, 6)).as_deref(),
            Some("2024-01-02 03:04:05.000006")
        );
        assert_eq!(
            text_of(&Value::Time(true, 1, 2, 3, 4, 0)).as_deref(),
            Some("-26:03:04")
        );
    }

    #[test]
    fn names_and_texts_are_quoted_for_mysql() {
        assert_eq!(quote("a`b"), "`a``b`");
        assert_eq!(literal("it's a\\b"), "'it''s a\\\\b'");
    }

    /// Against a real server: `SIMPLEEDIT_TEST_MYSQL=host:port:user:password:database`
    /// (a MariaDB or MySQL; it creates and drops a table of its own).
    #[test]
    fn a_real_server_is_read_changed_and_exported() {
        let Ok(spec) = std::env::var("SIMPLEEDIT_TEST_MYSQL") else {
            eprintln!("no SIMPLEEDIT_TEST_MYSQL: skipped");
            return;
        };
        let parts: Vec<&str> = spec.split(':').collect();
        let target = json!({
            "dialect": "mysql", "host": parts[0], "port": parts[1].parse::<u16>().unwrap(),
            "user": parts[2], "password": parts[3], "database": parts[4],
        });
        let open = |r: &Request| Session::open(r).map(|s| Box::new(s) as Box<dyn Engine>);
        let mut slot = None;
        let mut call = |request: Json| {
            let mut request = request;
            request["target"] = target.clone();
            common::run(&request.to_string(), &mut slot, &open, true)
        };
        let version = call(json!({"op": "test"})).unwrap();
        assert!(
            version["version"].as_str().unwrap().contains("Maria")
                || version["version"].as_str().unwrap().starts_with("MySQL")
        );
        let db = parts[4];
        call(json!({"op": "query", "statements": [
            "DROP TABLE IF EXISTS se_items", "DROP TABLE IF EXISTS se_people",
            "CREATE TABLE se_people (id INT PRIMARY KEY AUTO_INCREMENT, name VARCHAR(40) NOT NULL, note TEXT)",
            "CREATE TABLE se_items (id INT PRIMARY KEY, person_id INT, qty DECIMAL(8,2), CONSTRAINT fk_person FOREIGN KEY (person_id) REFERENCES se_people (id))",
            "CREATE INDEX se_items_person ON se_items (person_id)",
            "INSERT INTO se_people (name, note) VALUES ('Ann', NULL), ('Bob', ''), ('C\\'d', 'x')",
            "INSERT INTO se_items VALUES (1, 1, 2.50)",
        ]}))
        .unwrap();
        let tree = call(json!({"op": "tree"})).unwrap();
        let databases: Vec<Database> = serde_json::from_value(tree["databases"].clone()).unwrap();
        let schema = databases[0].schemas.iter().find(|s| s.name == db).unwrap();
        assert!(schema.tables.iter().any(|t| t.name == "se_people"));
        let page =
            call(json!({"op": "rows", "schema": db, "table": "se_people", "limit": 2})).unwrap();
        assert_eq!(page["rows"], json!([["1", "Ann", null], ["2", "Bob", ""]]));
        assert_eq!(page["has_more"], true);
        assert_eq!(page["editable"], true);
        let done = call(
            json!({"op": "apply", "schema": db, "table": "se_people", "changes": [
                {"kind": "update", "key": [["id", "1"]], "set": [["name", "A\\nn'"]]},
                {"kind": "insert", "key": [], "set": [["name", "Zed"]]},
                {"kind": "delete", "key": [["id", "2"]], "set": []},
            ]}),
        )
        .unwrap();
        assert_eq!(done["applied"], 3);
        let page = call(
            json!({"op": "rows", "schema": db, "table": "se_people", "limit": 10,
            "order": [{"column": "id", "desc": false}]}),
        )
        .unwrap();
        // A backslash and a quote in a value stay what they are.
        assert_eq!(page["rows"][0][1], "A\\nn'");
        // A value saved again as it was (2.50 as 2.5) still finds its row.
        let same = call(
            json!({"op": "apply", "schema": db, "table": "se_items", "changes": [
            {"kind": "update", "key": [["id", "1"]], "set": [["qty", "2.5"]]}]}),
        )
        .unwrap();
        assert_eq!(same["applied"], 1);
        assert!(call(
            json!({"op": "apply", "schema": db, "table": "se_people", "changes": [
            {"kind": "update", "key": [["id", "404"]], "set": [["name", "x"]]}]})
        )
        .is_err());
        let structure =
            call(json!({"op": "structure", "schema": db, "table": "se_items"})).unwrap();
        let structure: Structure = serde_json::from_value(structure["structure"].clone()).unwrap();
        assert_eq!(structure.columns[2].kind, "decimal(8,2)");
        assert!(structure
            .constraints
            .iter()
            .any(|c| c.kind == ConstraintKind::Foreign && c.name == "fk_person"));
        assert!(structure
            .indexes
            .iter()
            .any(|i| i.name == "se_items_person"));
        let people = call(json!({"op": "structure", "schema": db, "table": "se_people"})).unwrap();
        let people: Structure = serde_json::from_value(people["structure"].clone()).unwrap();
        assert_eq!(people.referenced_by[0].table, "se_items");
        assert_eq!(people.columns[0].identity, "d");
        let catalog = call(json!({"op": "columns"})).unwrap();
        let keys: Vec<ForeignKey> =
            serde_json::from_value(catalog["foreign_keys"].clone()).unwrap();
        assert!(keys
            .iter()
            .any(|k| k.table == "se_items" && k.ref_table == "se_people"));
        let d = call(json!({"op": "definition", "schema": db, "table": "se_people"})).unwrap();
        assert!(d["definition"]
            .as_str()
            .unwrap()
            .starts_with("CREATE TABLE `se_people`"));
        // The console: a reading followed by a writing is read at once, a
        // writing is counted, a last reading is left to the tab, a
        // transaction stays open.
        let q = call(json!({"op": "query", "statements": [
            "SELECT 1", "UPDATE se_people SET note = 'n'", "SELECT 2"
        ]}))
        .unwrap();
        assert_eq!(q["results"][0]["deferred"], false);
        assert_eq!(q["results"][1]["affected"], 3);
        assert_eq!(q["results"][2]["deferred"], true);
        let open_tx = call(
            json!({"op": "query", "statements": ["START TRANSACTION", "DELETE FROM se_items"]}),
        )
        .unwrap();
        assert_eq!(open_tx["in_transaction"], true);
        let closed = call(json!({"op": "query", "statements": ["ROLLBACK"]})).unwrap();
        assert_eq!(closed["in_transaction"], false);
        let paged = call(
            json!({"op": "rows", "sql": "SELECT id, name FROM se_people", "limit": 1,
            "order": [{"column": "id", "desc": true}]}),
        )
        .unwrap();
        assert_eq!(paged["has_more"], true);
        let out = std::env::temp_dir().join(format!("se-mysql-{}.csv", std::process::id()));
        let exported = call(
            json!({"op": "export", "schema": db, "table": "se_people", "format": "csv",
            "path": out.to_string_lossy()}),
        )
        .unwrap();
        assert_eq!(exported["exported"], 3);
        let csv = std::fs::read_to_string(&out).unwrap();
        assert!(csv.starts_with("id,name,note\n"));
        let _ = std::fs::remove_file(out);
        call(json!({"op": "query", "statements": ["DROP TABLE se_items", "DROP TABLE se_people"]}))
            .unwrap();
    }

    /// Against a real server, as above: a session the server closed, and a
    /// statement stopped from another connection.
    #[test]
    fn a_lost_session_is_opened_again_and_a_long_statement_is_stopped() {
        let Ok(spec) = std::env::var("SIMPLEEDIT_TEST_MYSQL") else {
            eprintln!("no SIMPLEEDIT_TEST_MYSQL: skipped");
            return;
        };
        let parts: Vec<&str> = spec.split(':').collect();
        let target = json!({
            "dialect": "mysql", "host": parts[0], "port": parts[1].parse::<u16>().unwrap(),
            "user": parts[2], "password": parts[3], "database": parts[4],
        });
        let open = |r: &Request| Session::open(r).map(|s| Box::new(s) as Box<dyn Engine>);
        let ask = |slot: &mut Option<(String, Box<dyn Engine>)>, request: Json| {
            let mut request = request;
            request["target"] = target.clone();
            common::run(&request.to_string(), slot, &open, true)
        };
        let id_of = |slot: &mut Option<(String, Box<dyn Engine>)>| {
            slot.as_mut().unwrap().1.session_id().unwrap()
        };
        let mut session = None;
        let mut other = None;
        let kill = |other: &mut Option<(String, Box<dyn Engine>)>, id: i64| {
            ask(
                other,
                json!({"op": "query", "statements": [format!("KILL {id}")]}),
            )
            .unwrap();
        };
        // Closed while idle: the next request opens another session.
        ask(&mut session, json!({"op": "test"})).unwrap();
        let first = id_of(&mut session);
        kill(&mut other, first);
        ask(&mut session, json!({"op": "test"})).unwrap();
        assert_ne!(id_of(&mut session), first);
        // Closed in a transaction: that is said, once.
        let began = ask(
            &mut session,
            json!({"op": "query", "statements": ["START TRANSACTION"]}),
        )
        .unwrap();
        assert_eq!(began["in_transaction"], true);
        kill(&mut other, id_of(&mut session));
        let lost = ask(
            &mut session,
            json!({"op": "query", "statements": ["SELECT 1"]}),
        );
        assert!(lost.unwrap_err().contains("transaction was rolled back"));
        let again = ask(
            &mut session,
            json!({"op": "query", "statements": ["SELECT 1"]}),
        )
        .unwrap();
        assert_eq!(again["in_transaction"], false);
        // Stopped from another connection, as the app does.
        let started = std::time::Instant::now();
        let sleeping = std::thread::spawn({
            let target = target.clone();
            move || {
                let open = |r: &Request| Session::open(r).map(|s| Box::new(s) as Box<dyn Engine>);
                let mut slot = None;
                let request = json!({"op": "query", "target": target,
                    "statements": ["DO SLEEP(30)"]});
                common::run(&request.to_string(), &mut slot, &open, true)
            }
        });
        std::thread::sleep(std::time::Duration::from_secs(1));
        let list = ask(
            &mut other,
            json!({"op": "rows", "limit": 10,
            "sql": "SELECT id FROM information_schema.processlist WHERE info = 'DO SLEEP(30)'"}),
        )
        .unwrap();
        let id: i64 = list["rows"][0][0].as_str().unwrap().parse().unwrap();
        let stopped = ask(&mut None, json!({"op": "cancel", "pid": id})).unwrap();
        assert_eq!(stopped["stopped"], true);
        sleeping.join().unwrap().unwrap();
        assert!(started.elapsed() < std::time::Duration::from_secs(20));
    }
}
