//! What the SQLite and the MySQL drivers have in common: the protocol with
//! the app (one request per line, one answer per line, a session that
//! stays), and the operations written once over a small `Engine`: the rows
//! of a table or a query, the changes, the statements of the console, the
//! export.
//!
//! The PostgreSQL driver has its own, richer, program; this is the part
//! that does not depend on the system.

use std::cell::RefCell;
use std::io::{BufRead, Read, Write};
use std::time::Instant;

use serde_json::{json, Value};

use crate::protocol::{
    self, Change, ChangeKind, Column, Database, Executed, ForeignKey, QueryResult, Request,
    TableColumn, MAX_RESULT_ROWS, PROTOCOL,
};

/// A row, every value as text, `None` for NULL.
pub type Row = Vec<Option<String>>;

/// Told each row of an export with the names of the columns and which
/// values are numbers.
pub type Sink<'a> = dyn FnMut(&[String], &[bool], Row) -> Result<(), String> + 'a;

/// A table as the driver reads it.
pub struct Shape {
    pub columns: Vec<Column>,
    pub primary_key: Vec<String>,
    /// A table, not a view.
    pub is_table: bool,
    /// What finds a row of a table that has no primary key (`rowid`).
    pub row_id: Option<&'static str>,
}

/// One database system.
pub trait Engine {
    /// A name between the quotes the system uses.
    fn quote(&self, name: &str) -> String;
    /// A text as a literal of an SQL statement.
    fn literal(&self, text: &str) -> String {
        format!("'{}'", text.replace('\'', "''"))
    }
    /// A text as the literal for a column of type `kind` (a hex dump as
    /// bytes, for the binary ones).
    fn literal_for(&self, _kind: &str, text: &str) -> String {
        self.literal(text)
    }
    /// `column` equals the value `literal`, for finding a row by its key.
    fn key_equals(&self, column: &str, literal: &str) -> String {
        format!("{column} = {literal}")
    }
    /// `column` equals `literal`, NULLs included.
    fn same(&self, column: &str, literal: &str) -> String;
    /// A request is about to be answered (an engine with a time limit of its
    /// own starts its clock).
    fn start_request(&mut self) {}
    fn version(&mut self) -> Result<String, String>;
    fn tree(&mut self, request: &Request) -> Result<Vec<Database>, String>;
    fn shape(&mut self, schema: &str, table: &str) -> Result<Shape, String>;
    /// The rows a statement gives, at most `limit`: the names of the
    /// columns, the rows, and whether there were more.
    fn fetch(&mut self, sql: &str, limit: usize) -> Result<(Vec<String>, Vec<Row>, bool), String>;
    /// Every row, one at a time, with whether each value is a number.
    fn each(&mut self, sql: &str, sink: &mut Sink<'_>) -> Result<(), String>;
    /// Runs a statement that gives no rows: how many it touched.
    fn execute(&mut self, sql: &str) -> Result<u64, String>;
    /// The columns a statement gives, `None` if it gives no rows.
    fn columns_of(&mut self, sql: &str) -> Result<Option<Vec<String>>, String>;
    fn begin(&mut self) -> Result<(), String>;
    fn commit(&mut self) -> Result<(), String>;
    fn rollback(&mut self) -> Result<(), String>;
    fn in_transaction(&mut self) -> bool;
    fn structure(&mut self, schema: &str, table: &str) -> Result<protocol::Structure, String>;
    fn definition(&mut self, request: &Request) -> Result<String, String>;
    fn catalog(&mut self, request: &Request)
        -> Result<(Vec<TableColumn>, Vec<ForeignKey>), String>;
    /// Whether the server is still there: a session left idle may have been
    /// closed by it (MySQL's `wait_timeout`, a restart).
    fn alive(&mut self) -> bool {
        true
    }
    /// What the server calls the session, for stopping its statement from
    /// another connection.
    fn session_id(&mut self) -> Option<i64> {
        None
    }
    /// Stops the statement the session `id` runs: whether there was one.
    fn cancel(&mut self, _id: i64) -> Result<bool, String> {
        Ok(false)
    }
}

thread_local! {
    /// The commands run so far, for the log of the app.
    static LOG: RefCell<Vec<Executed>> = const { RefCell::new(Vec::new()) };
}

pub fn record(sql: &str, started: Instant, rows: Option<u64>, error: Option<String>) {
    log(sql, started, rows, error, false);
}

pub fn record_internal(sql: &str, started: Instant, rows: Option<u64>, error: Option<String>) {
    log(sql, started, rows, error, true);
}

fn log(sql: &str, started: Instant, rows: Option<u64>, error: Option<String>, internal: bool) {
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

/// The answer line: what was run goes back with it, well or not.
pub fn answer_line(answer: Result<Value, String>) -> Value {
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

/// `main` of a driver: `--protocol`, `--serve`, or one request on the
/// standard input.
pub fn main_loop(open: impl Fn(&Request) -> Result<Box<dyn Engine>, String>) {
    match std::env::args().nth(1).as_deref() {
        Some("--protocol") => println!("{PROTOCOL}"),
        Some("--serve") => {
            let mut slot: Option<(String, Box<dyn Engine>)> = None;
            let stdin = std::io::stdin();
            let mut out = std::io::stdout();
            for line in stdin.lock().lines() {
                let Ok(line) = line else { break };
                if line.trim().is_empty() {
                    continue;
                }
                let answer = run(&line, &mut slot, &open, true);
                if writeln!(out, "{}", answer_line(answer)).is_err() || out.flush().is_err() {
                    break;
                }
            }
        }
        _ => {
            let mut input = String::new();
            let answer = match std::io::stdin().read_to_string(&mut input) {
                Ok(_) => run(&input, &mut None, &open, false),
                Err(e) => Err(e.to_string()),
            };
            println!("{}", answer_line(answer));
        }
    }
}

/// One request: the session it is about (kept in `slot` while the target is
/// the same), then the operation.
pub fn run(
    input: &str,
    slot: &mut Option<(String, Box<dyn Engine>)>,
    open: &impl Fn(&Request) -> Result<Box<dyn Engine>, String>,
    serving: bool,
) -> Result<Value, String> {
    let request: Request = serde_json::from_str(input).map_err(|e| e.to_string())?;
    let key = serde_json::to_string(&request.target).unwrap_or_default();
    if let Some((k, engine)) = slot {
        if *k == key && !engine.alive() {
            // A transaction of the session is gone with it: going on in a new
            // session would write what follows at once, and a ROLLBACK would
            // undo nothing. The next request opens another.
            let lost = engine.in_transaction();
            *slot = None;
            if lost {
                return Err(
                    "the connection to the server was lost: its open transaction was rolled back"
                        .to_string(),
                );
            }
        }
    }
    if !matches!(slot, Some((k, _)) if *k == key) {
        *slot = None;
        *slot = Some((key, open(&request)?));
        if serving {
            // Said at once, before the request is answered, as the PostgreSQL
            // driver does: whoever wants to stop a long statement needs it
            // while the statement runs.
            if let Some(id) = slot.as_mut().and_then(|(_, engine)| engine.session_id()) {
                let mut out = std::io::stdout();
                let _ = writeln!(out, "{}", json!({ "backend_pid": id }));
                let _ = out.flush();
            }
        }
    }
    let engine = slot.as_mut().expect("just opened").1.as_mut();
    engine.start_request();
    let answer = answer(engine, &request);
    if serving {
        return answer.map(|mut value| {
            if request.op == "query" {
                value["in_transaction"] = json!(engine.in_transaction());
            }
            value
        });
    }
    answer
}

fn answer(engine: &mut dyn Engine, request: &Request) -> Result<Value, String> {
    match request.op.as_str() {
        "test" => {
            let started = Instant::now();
            let version = engine.version();
            record(
                "SELECT version()",
                started,
                Some(1),
                version.as_ref().err().cloned(),
            );
            Ok(json!({ "version": version? }))
        }
        "tree" => Ok(json!({ "databases": engine.tree(request)? })),
        "rows" => rows(engine, request),
        "count" => count(engine, request),
        "export" => export(engine, request),
        "apply" => {
            if request.target.read_only {
                return Err("this data source is read-only".to_string());
            }
            apply(engine, request)
        }
        "query" => query(engine, request),
        "columns" => {
            let (columns, foreign_keys) = engine.catalog(request)?;
            Ok(json!({ "columns": columns, "foreign_keys": foreign_keys }))
        }
        "choices" => Ok(json!({ "choices": Vec::<protocol::Choices>::new() })),
        "structure" => {
            let structure = engine.structure(&request.schema, &request.table)?;
            Ok(json!({ "structure": structure }))
        }
        "definition" => Ok(json!({ "definition": engine.definition(request)? })),
        "cancel" => Ok(json!({ "stopped": engine.cancel(request.pid)? })),
        "import" => {
            if request.target.read_only {
                return Err("this data source is read-only".to_string());
            }
            import(engine, request)
        }
        "dump" => dump(engine, request),
        "restore" => {
            if request.target.read_only {
                return Err("this data source is read-only".to_string());
            }
            restore(engine, request)
        }
        "health" => Err(format!(
            "{} is not available for this kind of data source",
            request.op
        )),
        other => Err(format!("unknown operation: {other}")),
    }
}

/// ` WHERE (…)` and ` ORDER BY …`: the user's own condition on its own
/// lines (a comment in it can't swallow what follows), the sort on known
/// columns, then what makes the order total.
fn clauses(
    engine: &dyn Engine,
    request: &Request,
    names: &[&str],
    tiebreak: &[String],
) -> Result<String, String> {
    let mut sql = String::new();
    if protocol::has_semicolon(&request.filter, request.target.dialect) {
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
                "{} {}",
                engine.quote(&sort.column),
                if sort.desc { "DESC" } else { "ASC" }
            ));
        }
        for name in tiebreak {
            if !request.order.iter().any(|s| s.column == *name) {
                parts.push(if name == "rowid" {
                    name.clone()
                } else {
                    engine.quote(name)
                });
            }
        }
        sql.push_str(&format!(" ORDER BY {}", parts.join(", ")));
    }
    Ok(sql)
}

fn table_name(engine: &dyn Engine, schema: &str, table: &str) -> String {
    if schema.is_empty() {
        engine.quote(table)
    } else {
        format!("{}.{}", engine.quote(schema), engine.quote(table))
    }
}

/// A fetch that is logged.
fn fetch_logged(
    engine: &mut dyn Engine,
    sql: &str,
    shown: &str,
    limit: usize,
) -> Result<(Vec<String>, Vec<Row>, bool), String> {
    let started = Instant::now();
    match engine.fetch(sql, limit) {
        Ok(found) => {
            record(shown, started, Some(found.1.len().min(limit) as u64), None);
            Ok(found)
        }
        Err(message) => {
            record(shown, started, None, Some(message.clone()));
            Err(message)
        }
    }
}

/// How many rows a table (or a query) has under the condition.
fn count(engine: &mut dyn Engine, request: &Request) -> Result<Value, String> {
    let mut plain = request.clone();
    plain.order.clear();
    let tail = clauses(engine, &plain, &[], &[])?;
    let from = if request.sql.trim().is_empty() {
        table_name(engine, &request.schema, &request.table)
    } else {
        let sql = request.sql.trim().trim_end_matches(';').trim().to_string();
        if !protocol::wrappable(&sql, request.target.dialect) {
            return Err("this query cannot be counted".to_string());
        }
        format!("(\n{sql}\n) AS {}", engine.quote("q"))
    };
    let sql = format!("SELECT COUNT(*) FROM {from}{tail}");
    let (_, data, _) = fetch_logged(engine, &sql, &sql, 1)?;
    let total = data
        .first()
        .and_then(|row| row.first())
        .and_then(|v| v.as_deref())
        .and_then(|v| v.trim().parse::<u64>().ok())
        .ok_or_else(|| "the count is not a number".to_string())?;
    Ok(json!({ "total": total }))
}

fn rows(engine: &mut dyn Engine, request: &Request) -> Result<Value, String> {
    if !request.sql.trim().is_empty() {
        return result_rows(engine, request);
    }
    let (schema, table) = (request.schema.as_str(), request.table.as_str());
    let limit = request.limit.max(1) as usize;
    let shape = engine.shape(schema, table)?;
    let names: Vec<&str> = shape.columns.iter().map(|c| c.name.as_str()).collect();
    let keyless = shape.is_table && shape.primary_key.is_empty() && shape.row_id.is_some();
    let tiebreak: Vec<String> = if !shape.is_table {
        Vec::new()
    } else if keyless {
        vec![shape.row_id.unwrap_or("rowid").to_string()]
    } else {
        shape.primary_key.clone()
    };
    let target = table_name(engine, schema, table);
    let tail = clauses(engine, request, &names, &tiebreak)?;
    let columns = if keyless {
        format!("*, {}", shape.row_id.unwrap_or("rowid"))
    } else {
        "*".to_string()
    };
    let sql = format!(
        "SELECT {columns} FROM {target}{tail} LIMIT {} OFFSET {}",
        limit + 1,
        request.offset
    );
    let shown = format!(
        "SELECT * FROM {target}{tail} LIMIT {limit} OFFSET {}",
        request.offset
    );
    let (_, mut data, more) = fetch_logged(engine, &sql, &shown, limit)?;
    let has_more = more || data.len() > limit;
    data.truncate(limit);
    let mut ctids = Vec::new();
    if keyless {
        for row in &mut data {
            ctids.push(row.pop().flatten().unwrap_or_default());
        }
    }
    let editable = shape.is_table
        && !request.target.read_only
        && (!shape.primary_key.is_empty() || shape.row_id.is_some());
    let truncated = protocol::cap_page(&mut data);
    Ok(json!({
        "columns": shape.columns,
        "rows": data,
        "truncated": truncated,
        "has_more": has_more,
        "primary_key": shape.primary_key,
        "ctids": ctids,
        "editable": editable,
    }))
}

/// The rows of a query, sorted, filtered and paged by wrapping it.
fn result_rows(engine: &mut dyn Engine, request: &Request) -> Result<Value, String> {
    let sql = request.sql.trim().trim_end_matches(';').trim().to_string();
    if !protocol::wrappable(&sql, request.target.dialect) {
        return Err("this query cannot be paged".to_string());
    }
    let limit = request.limit.max(1) as usize;
    let names = engine
        .columns_of(&sql)?
        .ok_or_else(|| "this statement gives no rows".to_string())?;
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let unchanged = request.filter.trim().is_empty() && request.order.is_empty();
    let (mut data, more) = if unchanged && request.target.dialect == protocol::Dialect::Mysql {
        // MariaDB drops the ORDER BY of a derived table, and MySQL refuses a
        // join whose columns share a name: the query runs as it is, and the
        // page is cut from its rows.
        let skip = request.offset as usize;
        let (_, mut all, more) = fetch_logged(engine, &sql, &sql, skip + limit)?;
        all.drain(..skip.min(all.len()));
        (all, more)
    } else {
        let wrapped = format!(
            "SELECT * FROM (\n{sql}\n) AS {}{} LIMIT {} OFFSET {}",
            engine.quote("q"),
            clauses(engine, request, &refs, &[])?,
            limit + 1,
            request.offset
        );
        let (_, data, more) = fetch_logged(engine, &wrapped, &wrapped, limit)?;
        (data, more)
    };
    let has_more = more || data.len() > limit;
    data.truncate(limit);
    let columns: Vec<Column> = names
        .iter()
        .map(|name| Column {
            name: name.clone(),
            kind: String::new(),
            nullable: true,
        })
        .collect();
    let truncated = protocol::cap_page(&mut data);
    Ok(json!({
        "columns": columns,
        "rows": data,
        "truncated": truncated,
        "has_more": has_more,
        "primary_key": Vec::<String>::new(),
        "editable": false,
        "source": Value::Null,
    }))
}

// ------------------------------------------------------------------ changes

fn change_sql(
    engine: &dyn Engine,
    change: &Change,
    target: &str,
    shape: &Shape,
) -> Result<String, String> {
    let known = |name: &str| -> Result<(), String> {
        shape
            .columns
            .iter()
            .any(|c| c.name == name)
            .then_some(())
            .ok_or_else(|| format!("no column {name}"))
    };
    let kind_of = |name: &str| -> &str {
        shape
            .columns
            .iter()
            .find(|c| c.name == name)
            .map_or("", |c| c.kind.as_str())
    };
    let value = |column: &str, v: &Option<String>| match v {
        Some(text) => engine.literal_for(kind_of(column), text),
        None => "NULL".to_string(),
    };
    if change.kind == ChangeKind::Insert {
        let mut columns = Vec::new();
        let mut values = Vec::new();
        for (column, v) in &change.set {
            known(column)?;
            columns.push(engine.quote(column));
            values.push(value(column, v));
        }
        return Ok(if columns.is_empty() {
            format!("INSERT INTO {target} DEFAULT VALUES")
        } else {
            format!(
                "INSERT INTO {target} ({}) VALUES ({})",
                columns.join(", "),
                values.join(", ")
            )
        });
    }
    let mut conditions = Vec::new();
    if shape.primary_key.is_empty() {
        let id = shape
            .row_id
            .ok_or_else(|| "the row cannot be found: the table has no primary key".to_string())?;
        if change.key.len() != 1 || change.key[0].0 != id {
            return Err("the row cannot be found: it has no place to be found by".to_string());
        }
        conditions.push(format!("{id} = {}", engine.literal(&change.key[0].1)));
        for (column, v) in &change.original {
            known(column)?;
            conditions.push(match v {
                Some(text) => engine.same(
                    &engine.quote(column),
                    &engine.literal_for(kind_of(column), text),
                ),
                None => format!("{} IS NULL", engine.quote(column)),
            });
        }
    } else {
        let mut given: Vec<&str> = change.key.iter().map(|(c, _)| c.as_str()).collect();
        given.sort_unstable();
        let mut expected: Vec<&str> = shape.primary_key.iter().map(String::as_str).collect();
        expected.sort_unstable();
        if given != expected {
            return Err("the row cannot be found: the table has no usable primary key".to_string());
        }
        for (column, v) in &change.key {
            conditions.push(engine.key_equals(
                &engine.quote(column),
                &engine.literal_for(kind_of(column), v),
            ));
        }
    }
    match change.kind {
        ChangeKind::Insert => Err("a new row has no key".to_string()),
        ChangeKind::Update => {
            if change.set.is_empty() {
                return Err("nothing to change".to_string());
            }
            let mut sets = Vec::new();
            for (column, v) in &change.set {
                known(column)?;
                sets.push(format!("{} = {}", engine.quote(column), value(column, v)));
            }
            Ok(format!(
                "UPDATE {target} SET {} WHERE {}",
                sets.join(", "),
                conditions.join(" AND ")
            ))
        }
        ChangeKind::Delete => Ok(format!(
            "DELETE FROM {target} WHERE {}",
            conditions.join(" AND ")
        )),
    }
}

/// The changes in one transaction: if one fails, or does not touch exactly
/// one row, none is made.
/// A backup as SQL: for each table of the scope (a table, a schema, or the
/// whole source), its definition and then its rows as INSERT statements.
fn dump(engine: &mut dyn Engine, request: &Request) -> Result<Value, String> {
    use std::io::Write;
    if request.path.is_empty() {
        return Err("no file to write".to_string());
    }
    let spec = request.tool.clone().unwrap_or_default();
    let tree = engine.tree(request)?;
    let mut tables: Vec<(String, String)> = Vec::new();
    for database in &tree {
        for schema in &database.schemas {
            for table in &schema.tables {
                let wanted = (request.table.is_empty() || table.name == request.table)
                    && (request.schema.is_empty() || schema.name == request.schema);
                if wanted && table.kind == protocol::Kind::Table {
                    tables.push((schema.name.clone(), table.name.clone()));
                }
            }
        }
    }
    if tables.is_empty() {
        return Err("nothing to back up".to_string());
    }
    let file =
        std::fs::File::create(&request.path).map_err(|e| format!("{}: {e}", request.path))?;
    let mut out = std::io::BufWriter::new(file);
    let write_err = |e: std::io::Error| e.to_string();
    writeln!(out, "-- SimpleEdit SQL backup").map_err(write_err)?;
    // The tables refer to each other in any order: the checks wait for the end.
    let (off, on) = match request.target.dialect {
        protocol::Dialect::Sqlite => ("PRAGMA foreign_keys = OFF;", "PRAGMA foreign_keys = ON;"),
        protocol::Dialect::Mysql => ("SET FOREIGN_KEY_CHECKS = 0;", "SET FOREIGN_KEY_CHECKS = 1;"),
        protocol::Dialect::Postgres => ("", ""),
    };
    if !off.is_empty() {
        writeln!(out, "{off}").map_err(write_err)?;
    }
    for (schema, table) in &tables {
        let mut one = request.clone();
        one.schema = schema.clone();
        one.table = table.clone();
        if !spec.data_only {
            let definition = engine.definition(&one)?;
            writeln!(out, "\n{}", definition.trim_end()).map_err(write_err)?;
        }
        if spec.schema_only {
            continue;
        }
        let source = table_name(engine, schema, table);
        // Written without the schema, as the definition is: the file goes
        // into whatever database it is restored to.
        let target = engine.quote(table);
        let mut batch: Vec<String> = Vec::new();
        let mut names: Vec<String> = Vec::new();
        let literal = |engine: &dyn Engine, v: &Option<String>, number: bool| match v {
            None => "NULL".to_string(),
            Some(t) if number => t.clone(),
            Some(t) => engine.literal(t),
        };
        let mut lines: Vec<String> = Vec::new();
        let quote_names = |engine: &dyn Engine, names: &[String]| {
            names
                .iter()
                .map(|n| engine.quote(n))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let sql = format!("SELECT * FROM {source}");
        let mut rows: Vec<(Vec<bool>, Row)> = Vec::new();
        engine.each(&sql, &mut |cols: &[String], numbers: &[bool], row: Row| {
            if names.is_empty() {
                names = cols.to_vec();
            }
            rows.push((numbers.to_vec(), row));
            Ok(())
        })?;
        for (numbers, row) in &rows {
            let cells: Vec<String> = row
                .iter()
                .zip(numbers.iter().chain(std::iter::repeat(&false)))
                .map(|(v, n)| literal(&*engine, v, *n))
                .collect();
            batch.push(format!("({})", cells.join(", ")));
            if batch.len() >= IMPORT_BATCH {
                lines.push(format!(
                    "INSERT INTO {target} ({}) VALUES {};",
                    quote_names(&*engine, &names),
                    batch.join(", ")
                ));
                batch.clear();
            }
            if lines.len() >= 64 {
                for l in lines.drain(..) {
                    writeln!(out, "{l}").map_err(write_err)?;
                }
            }
        }
        if !batch.is_empty() {
            lines.push(format!(
                "INSERT INTO {target} ({}) VALUES {};",
                quote_names(&*engine, &names),
                batch.join(", ")
            ));
        }
        for l in lines {
            writeln!(out, "{l}").map_err(write_err)?;
        }
    }
    if !on.is_empty() {
        writeln!(out, "\n{on}").map_err(write_err)?;
    }
    out.flush().map_err(write_err)?;
    drop(out);
    let size = std::fs::metadata(&request.path).map_or(0, |m| m.len());
    Ok(json!({ "path": request.path, "tool": "SimpleEdit SQL", "size": size, "said": "" }))
}

/// Runs the statements of a backup file, one after the other, in one
/// transaction where the system has one for them.
fn restore(engine: &mut dyn Engine, request: &Request) -> Result<Value, String> {
    if request.path.is_empty() {
        return Err("no file to read".to_string());
    }
    let text =
        std::fs::read_to_string(&request.path).map_err(|e| format!("{}: {e}", request.path))?;
    let dialect = request.target.dialect;
    let mut statements: Vec<&str> = Vec::new();
    let mut start = 0;
    for token in protocol::sql_tokens(&text, dialect) {
        if token.kind == protocol::TokenKind::Punct && &text[token.start..token.end] == ";" {
            statements.push(&text[start..token.start]);
            start = token.end;
        }
    }
    statements.push(&text[start..]);
    let mut ran = 0u64;
    for (i, sql) in statements.iter().enumerate() {
        // Only comments are not a statement.
        let body: String = protocol::sql_tokens(sql, dialect)
            .iter()
            .filter(|t| t.kind != protocol::TokenKind::Comment)
            .map(|t| &sql[t.start..t.end])
            .collect::<Vec<_>>()
            .join(" ");
        if body.trim().is_empty() {
            continue;
        }
        let started = Instant::now();
        match engine.execute(sql.trim()) {
            Ok(n) => {
                record(&sql_head(sql.trim()), started, Some(n), None);
                ran += 1;
            }
            Err(message) => {
                record(&sql_head(sql.trim()), started, None, Some(message.clone()));
                return Err(format!("statement {}: {message}", i + 1));
            }
        }
    }
    Ok(json!({ "tool": "SimpleEdit SQL", "said": format!("{ran} statements") }))
}

/// Rows per INSERT when a file is imported.
const IMPORT_BATCH: usize = 500;

/// Puts the rows of a CSV or JSON file into a table: INSERT statements, in
/// one transaction (all of the file or none).
fn import(engine: &mut dyn Engine, request: &Request) -> Result<Value, String> {
    let spec = request
        .import
        .as_ref()
        .ok_or_else(|| "nothing to import".to_string())?;
    if spec.mapping.is_empty() {
        return Err("no column to fill".to_string());
    }
    let text =
        std::fs::read_to_string(&request.path).map_err(|e| format!("{}: {e}", request.path))?;
    // One entry per row, one value per mapped column: `None` is NULL.
    let rows: Vec<Vec<Option<String>>> = if spec.format == "json" {
        let value: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        let objects = match value {
            Value::Array(list) => list,
            other => vec![other],
        };
        objects
            .iter()
            .map(|object| {
                spec.mapping
                    .iter()
                    .map(|m| match object.get(&m.key) {
                        None | Some(Value::Null) => None,
                        Some(Value::String(s)) => Some(s.clone()),
                        Some(other) => Some(other.to_string()),
                    })
                    .collect()
            })
            .collect()
    } else {
        let delimiter = spec.delimiter.chars().next().unwrap_or(',');
        let mut records = crate::import::parse_csv(&text, delimiter);
        if spec.header && !records.is_empty() {
            records.remove(0);
        }
        records
            .iter()
            .filter(|r| !(r.len() == 1 && r[0].is_empty()))
            .map(|record| {
                spec.mapping
                    .iter()
                    .map(|m| {
                        let value = record.get(m.index).cloned().unwrap_or_default();
                        (!(spec.empty_null && value.is_empty())).then_some(value)
                    })
                    .collect()
            })
            .collect()
    };
    let target = table_name(engine, &request.schema, &request.table);
    let columns: Vec<String> = spec
        .mapping
        .iter()
        .map(|m| engine.quote(&m.column))
        .collect();
    engine.begin()?;
    let mut imported = 0u64;
    for batch in rows.chunks(IMPORT_BATCH) {
        let values: Vec<String> = batch
            .iter()
            .map(|row| {
                let cells: Vec<String> = row
                    .iter()
                    .zip(&spec.mapping)
                    .map(|(value, m)| match value {
                        Some(text) => engine.literal_for(&m.kind, text),
                        None => "NULL".to_string(),
                    })
                    .collect();
                format!("({})", cells.join(", "))
            })
            .collect();
        let sql = format!(
            "INSERT INTO {target} ({}) VALUES {}",
            columns.join(", "),
            values.join(", ")
        );
        let started = Instant::now();
        match engine.execute(&sql) {
            Ok(n) => {
                record(&sql_head(&sql), started, Some(n), None);
                imported += n;
            }
            Err(message) => {
                record(&sql_head(&sql), started, None, Some(message.clone()));
                let _ = engine.rollback();
                return Err(message);
            }
        }
    }
    if let Err(message) = engine.commit() {
        let _ = engine.rollback();
        return Err(message);
    }
    Ok(json!({ "imported": imported }))
}

/// The start of a long statement, for the log.
fn sql_head(sql: &str) -> String {
    match sql.char_indices().nth(200) {
        Some((at, _)) => format!("{}…", &sql[..at]),
        None => sql.to_string(),
    }
}

fn apply(engine: &mut dyn Engine, request: &Request) -> Result<Value, String> {
    let (schema, table) = (request.schema.as_str(), request.table.as_str());
    let shape = engine.shape(schema, table)?;
    if !shape.is_table {
        return Err("only the rows of a table can be changed".to_string());
    }
    let target = table_name(engine, schema, table);
    engine.begin()?;
    let mut done = 0u64;
    for (i, change) in request.changes.iter().enumerate() {
        let sql = match change_sql(engine, change, &target, &shape) {
            Ok(sql) => sql,
            Err(e) => {
                let _ = engine.rollback();
                return Err(e);
            }
        };
        let started = Instant::now();
        match engine.execute(&sql) {
            Ok(touched) => {
                record(&sql, started, Some(touched), None);
                if touched != 1 {
                    let _ = engine.rollback();
                    return Err(format!(
                        "change {}: {touched} rows touched instead of 1 (the row was changed or removed by someone else)",
                        i + 1
                    ));
                }
                done += touched;
            }
            Err(message) => {
                record(&sql, started, None, Some(message.clone()));
                let _ = engine.rollback();
                return Err(format!("change {}: {message}", i + 1));
            }
        }
    }
    // A commit that fails (a lock waited for in vain) can leave the
    // transaction open: replayed in it, the changes would be made twice.
    if let Err(message) = engine.commit() {
        let _ = engine.rollback();
        return Err(message);
    }
    Ok(json!({ "applied": done }))
}

// ------------------------------------------------------------------ queries

/// Whether one of the functions named may change something when it runs: in
/// MySQL, a stored function of one's own (SQLite has none but the app's).
/// When the server can't say, it may.
fn may_write(engine: &mut dyn Engine, dialect: protocol::Dialect, names: &[String]) -> bool {
    if dialect != protocol::Dialect::Mysql {
        return false;
    }
    let list: Vec<String> = names.iter().map(|n| engine.literal(n)).collect();
    let sql = format!(
        "SELECT 1 FROM information_schema.ROUTINES \
         WHERE ROUTINE_TYPE = 'FUNCTION' AND ROUTINE_NAME IN ({}) LIMIT 1",
        list.join(", ")
    );
    engine
        .fetch(&sql, 1)
        .map_or(true, |(_, rows, _)| !rows.is_empty())
}

/// The statements of the console, one after the other in one session.
fn query(engine: &mut dyn Engine, request: &Request) -> Result<Value, String> {
    let mut results: Vec<QueryResult> = Vec::new();
    let mut failed: Option<Value> = None;
    let wrapped = request.transaction && !request.target.read_only;
    if wrapped {
        engine.begin()?;
    }
    for (index, sql) in request.statements.iter().enumerate() {
        if request.target.read_only && !protocol::reads_only(sql, request.target.dialect) {
            let message = "this data source is read-only".to_string();
            record(sql, Instant::now(), None, Some(message.clone()));
            failed = Some(json!({ "index": index, "error": message }));
            break;
        }
        let started = Instant::now();
        // Several commands in one text can't be described, only run.
        let columns = if protocol::is_multiple(sql, request.target.dialect) {
            None
        } else {
            engine.columns_of(sql).unwrap_or(None)
        };
        let deferred = columns.is_some()
            && protocol::deferrable(
                &request.statements,
                index,
                request.target.dialect,
                |names| may_write(engine, request.target.dialect, names),
            );
        if let Some(columns) = columns.filter(|_| deferred) {
            results.push(QueryResult {
                sql: sql.clone(),
                ms: 0,
                has_rows: true,
                columns,
                rows: Vec::new(),
                affected: 0,
                truncated: false,
                deferred: true,
            });
            continue;
        }
        let looks_like_rows = engine
            .columns_of(sql)
            .ok()
            .flatten()
            .filter(|_| !protocol::is_multiple(sql, request.target.dialect));
        let mut result = QueryResult {
            sql: sql.clone(),
            ms: 0,
            has_rows: false,
            columns: Vec::new(),
            rows: Vec::new(),
            affected: 0,
            truncated: false,
            deferred: false,
        };
        let outcome = if looks_like_rows.is_some() {
            engine
                .fetch(sql, MAX_RESULT_ROWS)
                .map(|(columns, rows, more)| {
                    result.has_rows = true;
                    result.columns = columns;
                    result.truncated = more;
                    result.rows = rows;
                    result.rows.len() as u64
                })
        } else {
            engine.execute(sql).inspect(|&n| {
                result.affected = n;
            })
        };
        match outcome {
            Ok(count) => {
                result.ms = started.elapsed().as_millis() as u64;
                record(sql, started, Some(count), None);
                results.push(result);
            }
            Err(message) => {
                record(sql, started, None, Some(message.clone()));
                failed = Some(json!({ "index": index, "error": message }));
                break;
            }
        }
    }
    if wrapped {
        if failed.is_some() {
            engine.rollback()?;
        } else if let Err(message) = engine.commit() {
            // Not left open by a commit that failed, as in `apply`.
            let _ = engine.rollback();
            return Err(message);
        }
    }
    Ok(json!({ "results": results, "failed": failed }))
}

// ------------------------------------------------------------------- export

/// A value of a CSV file: a NULL is empty without quotes, an empty text is
/// `""`, so that the two stay apart.
pub fn csv_field(value: Option<&str>) -> String {
    match value {
        None => String::new(),
        Some("") => "\"\"".to_string(),
        Some(text) if text.contains([',', '"', '\n', '\r']) => {
            format!("\"{}\"", text.replace('"', "\"\""))
        }
        Some(text) => text.to_string(),
    }
}

fn export(engine: &mut dyn Engine, request: &Request) -> Result<Value, String> {
    let select = if request.sql.trim().is_empty() {
        let (schema, table) = (request.schema.as_str(), request.table.as_str());
        let shape = engine.shape(schema, table)?;
        let names: Vec<&str> = shape.columns.iter().map(|c| c.name.as_str()).collect();
        let tiebreak: Vec<String> = if shape.is_table && !shape.primary_key.is_empty() {
            shape.primary_key.clone()
        } else {
            Vec::new()
        };
        format!(
            "SELECT * FROM {}{}",
            table_name(engine, schema, table),
            clauses(engine, request, &names, &tiebreak)?
        )
    } else {
        let sql = request.sql.trim().trim_end_matches(';').trim().to_string();
        if !protocol::wrappable(&sql, request.target.dialect) {
            return Err("this query cannot be exported".to_string());
        }
        let names = engine.columns_of(&sql)?.unwrap_or_default();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        format!(
            "SELECT * FROM (\n{sql}\n) AS {}{}",
            engine.quote("q"),
            clauses(engine, request, &refs, &[])?
        )
    };
    let json_format = request.format == "json";
    let started = Instant::now();
    let done = (|| -> Result<u64, String> {
        let file =
            std::fs::File::create(&request.path).map_err(|e| format!("{}: {e}", request.path))?;
        let mut out = std::io::BufWriter::new(file);
        let mut rows = 0u64;
        if json_format {
            out.write_all(b"[").map_err(|e| e.to_string())?;
        }
        let mut wrote_header = false;
        engine.each(&select, &mut |names, numeric, row| {
            let io = |e: std::io::Error| e.to_string();
            if json_format {
                let object: serde_json::Map<String, Value> = names
                    .iter()
                    .zip(row.iter())
                    .zip(numeric)
                    .map(|((name, value), number)| {
                        let v = match value {
                            None => Value::Null,
                            Some(text) if *number => serde_json::from_str::<Value>(text)
                                .ok()
                                .filter(Value::is_number)
                                .unwrap_or_else(|| Value::String(text.clone())),
                            Some(text) => Value::String(text.clone()),
                        };
                        (name.clone(), v)
                    })
                    .collect();
                out.write_all(if rows == 0 { b"\n  " } else { b",\n  " })
                    .map_err(io)?;
                out.write_all(Value::Object(object).to_string().as_bytes())
                    .map_err(io)?;
            } else {
                if !wrote_header {
                    let header: Vec<String> =
                        names.iter().map(|n| csv_field(Some(n.as_str()))).collect();
                    writeln!(out, "{}", header.join(",")).map_err(io)?;
                    wrote_header = true;
                }
                let line: Vec<String> = row.iter().map(|v| csv_field(v.as_deref())).collect();
                writeln!(out, "{}", line.join(",")).map_err(io)?;
            }
            rows += 1;
            Ok(())
        })?;
        if !json_format && !wrote_header {
            // No row: the header still says what the columns are.
            if let Some(names) = engine.columns_of(&select)? {
                let header: Vec<String> =
                    names.iter().map(|n| csv_field(Some(n.as_str()))).collect();
                writeln!(out, "{}", header.join(",")).map_err(|e| e.to_string())?;
            }
        }
        if json_format {
            out.write_all(if rows == 0 { b"]\n" } else { b"\n]\n" })
                .map_err(|e| e.to_string())?;
        }
        out.flush().map_err(|e| e.to_string())?;
        Ok(rows)
    })();
    match done {
        Ok(rows) => {
            record(&select, started, Some(rows), None);
            Ok(json!({ "exported": rows, "path": request.path }))
        }
        Err(message) => {
            record(&select, started, None, Some(message.clone()));
            let _ = std::fs::remove_file(&request.path);
            Err(message)
        }
    }
}
