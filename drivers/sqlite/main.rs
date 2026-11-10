//! The SQLite driver of SimpleEdit's Database panel: a separate program, as
//! the PostgreSQL one, speaking the same protocol. The "database" of a data
//! source is the path of the file.

#[allow(dead_code)]
#[path = "../../src/database/protocol.rs"]
mod protocol;

#[path = "../common/mod.rs"]
mod common;

use common::{Engine, Row, Shape};
use protocol::{
    Column, ColumnDef, ConstraintDef, ConstraintKind, Database, ForeignKey, IndexDef, Kind,
    Reference, Request, Schema, Structure, Table, TableColumn,
};
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags};

fn main() {
    common::main_loop(|request| Session::open(request).map(|s| Box::new(s) as Box<dyn Engine>));
}

struct Session {
    conn: Connection,
    /// The name of the file, for the tree.
    name: String,
    /// A transaction begun by this driver (not one the user ran).
    begun: bool,
}

impl Session {
    fn open(request: &Request) -> Result<Session, String> {
        let target = &request.target;
        let path = target.database.trim();
        if path.is_empty() {
            return Err("no file: give the path of the SQLite database".to_string());
        }
        if path != ":memory:" && !std::path::Path::new(path).is_file() {
            return Err(format!("{path}: no such file"));
        }
        let flags = if target.read_only {
            OpenFlags::SQLITE_OPEN_READ_ONLY
        } else {
            OpenFlags::SQLITE_OPEN_READ_WRITE
        } | OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let conn = Connection::open_with_flags(path, flags).map_err(clean)?;
        let wait = if target.query_timeout == 0 {
            5000
        } else {
            target.query_timeout * 1000
        };
        conn.busy_timeout(std::time::Duration::from_millis(wait))
            .map_err(clean)?;
        conn.execute_batch("PRAGMA foreign_keys = ON")
            .map_err(clean)?;
        if !target.startup_script.trim().is_empty() {
            conn.execute_batch(&target.startup_script).map_err(clean)?;
        }
        let name = std::path::Path::new(path)
            .file_name()
            .map_or_else(|| path.to_string(), |n| n.to_string_lossy().into_owned());
        Ok(Session {
            conn,
            name,
            begun: false,
        })
    }
}

fn clean(error: rusqlite::Error) -> String {
    error.to_string()
}

fn quote(name: &str) -> String {
    protocol::quote_ident(name)
}

/// A value as the driver says it: text, with NULL apart.
fn text_of(value: ValueRef<'_>) -> (Option<String>, bool) {
    match value {
        ValueRef::Null => (None, false),
        ValueRef::Integer(n) => (Some(n.to_string()), true),
        ValueRef::Real(f) => (Some(f.to_string()), true),
        ValueRef::Text(bytes) => (Some(String::from_utf8_lossy(bytes).into_owned()), false),
        ValueRef::Blob(bytes) => (
            Some(format!(
                "x'{}'",
                bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
            )),
            false,
        ),
    }
}

impl Engine for Session {
    fn quote(&self, name: &str) -> String {
        quote(name)
    }

    fn same(&self, column: &str, literal: &str) -> String {
        // The text is what the grid shows: a number in a column without
        // affinity, or a blob, is not equal to it as such.
        format!(
            "({column} IS {literal} \
             OR (typeof({column}) = 'integer' AND CAST({column} AS TEXT) = {literal}) \
             OR (typeof({column}) = 'real' AND {column} = CAST({literal} AS REAL)) \
             OR (typeof({column}) = 'blob' AND 'x''' || lower(hex({column})) || '''' = {literal}))"
        )
    }

    fn key_equals(&self, column: &str, literal: &str) -> String {
        self.same(column, literal)
    }

    fn version(&mut self) -> Result<String, String> {
        let version: String = self
            .conn
            .query_row("SELECT sqlite_version()", [], |r| r.get(0))
            .map_err(clean)?;
        Ok(format!("SQLite {version}"))
    }

    fn tree(&mut self, request: &Request) -> Result<Vec<Database>, String> {
        let started = std::time::Instant::now();
        let sql = "SELECT name, type FROM sqlite_master \
                   WHERE type IN ('table', 'view') AND (?1 OR name NOT LIKE 'sqlite\\_%' ESCAPE '\\') \
                   ORDER BY name";
        let mut statement = self.conn.prepare(sql).map_err(clean)?;
        let tables: Vec<Table> = statement
            .query_map([request.scope.system_schemas], |r| {
                let kind: String = r.get(1)?;
                Ok(Table {
                    name: r.get(0)?,
                    kind: if kind == "view" {
                        Kind::View
                    } else {
                        Kind::Table
                    },
                })
            })
            .map_err(clean)?
            .collect::<Result<_, _>>()
            .map_err(clean)?;
        common::record_internal(sql, started, Some(tables.len() as u64), None);
        Ok(vec![Database {
            name: self.name.clone(),
            current: true,
            schemas: vec![Schema {
                name: "main".to_string(),
                tables,
                ..Schema::default()
            }],
        }])
    }

    fn shape(&mut self, _schema: &str, table: &str) -> Result<Shape, String> {
        let started = std::time::Instant::now();
        let mut statement = self
            .conn
            .prepare(&format!("PRAGMA table_info({})", quote(table)))
            .map_err(clean)?;
        let found: Vec<(String, String, bool, i64)> = statement
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)? == 0,
                    r.get::<_, i64>(5)?,
                ))
            })
            .map_err(clean)?
            .collect::<Result<_, _>>()
            .map_err(clean)?;
        common::record_internal(
            &format!("PRAGMA table_info({})", quote(table)),
            started,
            Some(found.len() as u64),
            None,
        );
        if found.is_empty() {
            return Err(format!("{table}: no such table"));
        }
        let mut keyed: Vec<&(String, String, bool, i64)> =
            found.iter().filter(|c| c.3 > 0).collect();
        keyed.sort_by_key(|c| c.3);
        let kind: String = self
            .conn
            .query_row(
                "SELECT type FROM sqlite_master WHERE name = ?1",
                [table],
                |r| r.get(0),
            )
            .unwrap_or_else(|_| "table".into());
        Ok(Shape {
            primary_key: keyed.iter().map(|c| c.0.clone()).collect(),
            columns: found
                .iter()
                .map(|(name, kind, nullable, _)| Column {
                    name: name.clone(),
                    kind: kind.clone(),
                    nullable: *nullable,
                })
                .collect(),
            is_table: kind == "table",
            row_id: Some("rowid"),
        })
    }

    fn fetch(&mut self, sql: &str, limit: usize) -> Result<(Vec<String>, Vec<Row>, bool), String> {
        let mut statement = self.conn.prepare(sql).map_err(clean)?;
        let names: Vec<String> = statement.column_names().map(|n| n.to_string()).collect();
        let mut rows = statement.query([]).map_err(clean)?;
        let mut data: Vec<Row> = Vec::new();
        let mut more = false;
        while let Some(row) = rows.next().map_err(clean)? {
            if data.len() > limit {
                more = true;
                break;
            }
            data.push(
                (0..names.len())
                    .map(|i| text_of(row.get_ref_unwrap(i)).0)
                    .collect(),
            );
        }
        Ok((names, data, more))
    }

    fn each(&mut self, sql: &str, sink: &mut common::Sink<'_>) -> Result<(), String> {
        let mut statement = self.conn.prepare(sql).map_err(clean)?;
        let names: Vec<String> = statement.column_names().map(|n| n.to_string()).collect();
        let mut rows = statement.query([]).map_err(clean)?;
        while let Some(row) = rows.next().map_err(clean)? {
            let cells: Vec<(Option<String>, bool)> = (0..names.len())
                .map(|i| text_of(row.get_ref_unwrap(i)))
                .collect();
            let numeric: Vec<bool> = cells.iter().map(|c| c.1).collect();
            sink(&names, &numeric, cells.into_iter().map(|c| c.0).collect())?;
        }
        Ok(())
    }

    fn execute(&mut self, sql: &str) -> Result<u64, String> {
        self.conn.execute_batch(sql).map_err(clean)?;
        Ok(self.conn.changes())
    }

    fn columns_of(&mut self, sql: &str) -> Result<Option<Vec<String>>, String> {
        let statement = self.conn.prepare(sql).map_err(clean)?;
        Ok((statement.column_count() > 0)
            .then(|| statement.column_names().map(|n| n.to_string()).collect()))
    }

    fn begin(&mut self) -> Result<(), String> {
        if self.conn.is_autocommit() {
            self.conn.execute_batch("BEGIN").map_err(clean)?;
            self.begun = true;
        }
        Ok(())
    }

    fn commit(&mut self) -> Result<(), String> {
        if !self.conn.is_autocommit() && self.begun {
            self.conn.execute_batch("COMMIT").map_err(clean)?;
        }
        self.begun = false;
        Ok(())
    }

    fn rollback(&mut self) -> Result<(), String> {
        if !self.conn.is_autocommit() {
            self.conn.execute_batch("ROLLBACK").map_err(clean)?;
        }
        self.begun = false;
        Ok(())
    }

    fn in_transaction(&mut self) -> bool {
        !self.conn.is_autocommit()
    }

    fn structure(&mut self, _schema: &str, table: &str) -> Result<Structure, String> {
        let infos: Vec<(String, String, bool, Option<String>, i64)> = {
            let mut s = self
                .conn
                .prepare(&format!("PRAGMA table_xinfo({})", quote(table)))
                .map_err(clean)?;
            let found: Vec<_> = s
                .query_map([], |r| {
                    Ok((
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, i64>(3)? == 0,
                        r.get::<_, Option<String>>(4)?,
                        r.get::<_, i64>(6)?,
                    ))
                })
                .map_err(clean)?
                .collect::<Result<_, _>>()
                .map_err(clean)?;
            found
        };
        if infos.is_empty() {
            return Err(format!("{table}: no such table"));
        }
        let mut keyed: Vec<(i64, String)> = {
            let mut s = self
                .conn
                .prepare(&format!("PRAGMA table_info({})", quote(table)))
                .map_err(clean)?;
            let found: Vec<(i64, String)> = s
                .query_map([], |r| Ok((r.get::<_, i64>(5)?, r.get::<_, String>(1)?)))
                .map_err(clean)?
                .collect::<Result<_, _>>()
                .map_err(clean)?;
            found.into_iter().filter(|(pk, _)| *pk > 0).collect()
        };
        keyed.sort();
        let columns: Vec<ColumnDef> = infos
            .iter()
            .map(|(name, kind, nullable, default, hidden)| ColumnDef {
                name: name.clone(),
                kind: kind.clone(),
                nullable: *nullable,
                default: default.clone(),
                identity: String::new(),
                generated: *hidden >= 2,
                comment: String::new(),
            })
            .collect();
        let mut constraints = Vec::new();
        if !keyed.is_empty() {
            constraints.push(ConstraintDef {
                name: "PRIMARY".into(),
                kind: ConstraintKind::Primary,
                definition: format!(
                    "PRIMARY KEY ({})",
                    keyed
                        .iter()
                        .map(|(_, n)| quote(n))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            });
        }
        // The foreign keys of the table.
        {
            let mut s = self
                .conn
                .prepare(&format!("PRAGMA foreign_key_list({})", quote(table)))
                .map_err(clean)?;
            let list: Vec<(i64, String, String, Option<String>)> = s
                .query_map([], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, Option<String>>(4)?,
                    ))
                })
                .map_err(clean)?
                .collect::<Result<_, _>>()
                .map_err(clean)?;
            let mut ids: Vec<i64> = list.iter().map(|f| f.0).collect();
            ids.dedup();
            for id in ids {
                let mine: Vec<&(i64, String, String, Option<String>)> =
                    list.iter().filter(|f| f.0 == id).collect();
                let target = &mine[0].1;
                constraints.push(ConstraintDef {
                    name: format!("fk_{id}"),
                    kind: ConstraintKind::Foreign,
                    definition: format!(
                        "FOREIGN KEY ({}) REFERENCES {}({})",
                        mine.iter()
                            .map(|f| quote(&f.2))
                            .collect::<Vec<_>>()
                            .join(", "),
                        quote(target),
                        mine.iter()
                            .map(|f| quote(f.3.as_deref().unwrap_or("")))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                });
            }
        }
        // Indexes: those of a UNIQUE constraint are constraints.
        let mut indexes = Vec::new();
        {
            let mut s = self
                .conn
                .prepare(&format!("PRAGMA index_list({})", quote(table)))
                .map_err(clean)?;
            let list: Vec<(String, bool, String)> = s
                .query_map([], |r| {
                    Ok((
                        r.get::<_, String>(1)?,
                        r.get::<_, i64>(2)? != 0,
                        r.get::<_, String>(3)?,
                    ))
                })
                .map_err(clean)?
                .collect::<Result<_, _>>()
                .map_err(clean)?;
            for (name, unique, origin) in list {
                if origin == "pk" {
                    continue;
                }
                let sql: Option<String> = self
                    .conn
                    .query_row(
                        "SELECT sql FROM sqlite_master WHERE name = ?1",
                        [&name],
                        |r| r.get(0),
                    )
                    .ok()
                    .flatten();
                if origin == "u" {
                    let mut s = self
                        .conn
                        .prepare(&format!("PRAGMA index_info({})", quote(&name)))
                        .map_err(clean)?;
                    let names: Vec<String> = s
                        .query_map([], |r| r.get::<_, String>(2))
                        .map_err(clean)?
                        .collect::<Result<_, _>>()
                        .map_err(clean)?;
                    constraints.push(ConstraintDef {
                        name,
                        kind: ConstraintKind::Unique,
                        definition: format!(
                            "UNIQUE ({})",
                            names
                                .iter()
                                .map(|n| quote(n))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    });
                } else {
                    indexes.push(IndexDef {
                        definition: sql.unwrap_or_default(),
                        name,
                        unique,
                    });
                }
            }
        }
        // The foreign keys of other tables that point to this one.
        let mut referenced_by = Vec::new();
        let others: Vec<String> = {
            let mut s = self
                .conn
                .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\'")
                .map_err(clean)?;
            let found: Vec<String> = s
                .query_map([], |r| r.get::<_, String>(0))
                .map_err(clean)?
                .collect::<Result<_, _>>()
                .map_err(clean)?;
            found
        };
        for other in others {
            let mut s = self
                .conn
                .prepare(&format!("PRAGMA foreign_key_list({})", quote(&other)))
                .map_err(clean)?;
            let list: Vec<(i64, String, String, Option<String>)> = s
                .query_map([], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, Option<String>>(4)?,
                    ))
                })
                .map_err(clean)?
                .collect::<Result<_, _>>()
                .map_err(clean)?;
            let mut ids: Vec<i64> = list.iter().filter(|f| f.1 == table).map(|f| f.0).collect();
            ids.dedup();
            for id in ids {
                let mine: Vec<_> = list.iter().filter(|f| f.0 == id).collect();
                referenced_by.push(Reference {
                    name: format!("fk_{id}"),
                    schema: "main".into(),
                    table: other.clone(),
                    definition: format!(
                        "FOREIGN KEY ({}) REFERENCES {}({})",
                        mine.iter()
                            .map(|f| quote(&f.2))
                            .collect::<Vec<_>>()
                            .join(", "),
                        quote(table),
                        mine.iter()
                            .map(|f| quote(f.3.as_deref().unwrap_or("")))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                });
            }
        }
        Ok(Structure {
            columns,
            constraints,
            indexes,
            comment: String::new(),
            referenced_by,
            extra: Box::default(),
        })
    }

    fn definition(&mut self, request: &Request) -> Result<String, String> {
        let name = &request.table;
        let sql: Option<String> = self
            .conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE name = ?1",
                [name],
                |r| r.get(0),
            )
            .map_err(|_| format!("{name}: no such table or view"))?;
        let sql = sql.ok_or_else(|| format!("{name}: no definition"))?;
        Ok(format!("{};\n", sql.trim_end().trim_end_matches(';')))
    }

    fn catalog(
        &mut self,
        request: &Request,
    ) -> Result<(Vec<TableColumn>, Vec<ForeignKey>), String> {
        let tables = self.tree(request)?.remove(0).schemas.remove(0).tables;
        let mut columns = Vec::new();
        let mut keys = Vec::new();
        for table in tables {
            let mut s = self
                .conn
                .prepare(&format!("PRAGMA table_info({})", quote(&table.name)))
                .map_err(clean)?;
            let found: Vec<(String, String)> = s
                .query_map([], |r| Ok((r.get::<_, String>(1)?, r.get::<_, String>(2)?)))
                .map_err(clean)?
                .collect::<Result<_, _>>()
                .map_err(clean)?;
            columns.extend(found.into_iter().map(|(name, kind)| TableColumn {
                schema: "main".into(),
                table: table.name.clone(),
                name,
                kind,
            }));
            let mut s = self
                .conn
                .prepare(&format!("PRAGMA foreign_key_list({})", quote(&table.name)))
                .map_err(clean)?;
            let list: Vec<(i64, String, String, Option<String>)> = s
                .query_map([], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, Option<String>>(4)?,
                    ))
                })
                .map_err(clean)?
                .collect::<Result<_, _>>()
                .map_err(clean)?;
            let mut ids: Vec<i64> = list.iter().map(|f| f.0).collect();
            ids.dedup();
            for id in ids {
                let mine: Vec<_> = list.iter().filter(|f| f.0 == id).collect();
                keys.push(ForeignKey {
                    schema: "main".into(),
                    table: table.name.clone(),
                    columns: mine.iter().map(|f| f.2.clone()).collect(),
                    ref_schema: "main".into(),
                    ref_table: mine[0].1.clone(),
                    ref_columns: mine
                        .iter()
                        .map(|f| f.3.clone().unwrap_or_default())
                        .collect(),
                });
            }
        }
        Ok((columns, keys))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    /// A database file with a few tables.
    fn database() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shop.db").to_string_lossy().into_owned();
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT NOT NULL, note TEXT);
             CREATE TABLE \"Order Items\" (id INTEGER PRIMARY KEY, customer_id INTEGER REFERENCES customers(id), qty REAL);
             CREATE TABLE log (msg TEXT, at TEXT);
             CREATE UNIQUE INDEX customers_name ON customers (name);
             CREATE INDEX items_customer ON \"Order Items\" (customer_id);
             CREATE VIEW named AS SELECT name FROM customers;
             INSERT INTO customers (id, name, note) VALUES (1, 'Ann', NULL), (2, 'Bob', ''), (3, 'Cé, \"d\"', 'x');
             INSERT INTO \"Order Items\" VALUES (1, 1, 2.5), (2, 1, 3.0);
             INSERT INTO log VALUES ('a', NULL), ('b', 'now');",
        )
        .unwrap();
        (dir, path)
    }

    fn call(path: &str, request: Value) -> Result<Value, String> {
        let mut request = request;
        request["target"] = json!({ "database": path });
        let open = |r: &Request| Session::open(r).map(|s| Box::new(s) as Box<dyn Engine>);
        common::run(&request.to_string(), &mut None, &open, false)
    }

    #[test]
    fn the_version_and_the_tree_are_read() {
        let (_dir, path) = database();
        let version = call(&path, json!({"op": "test"})).unwrap();
        assert!(version["version"]
            .as_str()
            .unwrap()
            .starts_with("SQLite 3."));
        let tree = call(&path, json!({"op": "tree"})).unwrap();
        let databases: Vec<Database> = serde_json::from_value(tree["databases"].clone()).unwrap();
        assert_eq!(databases[0].name, "shop.db");
        let tables = &databases[0].schemas[0].tables;
        let names: Vec<&str> = tables.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, vec!["Order Items", "customers", "log", "named"]);
        assert_eq!(tables[3].kind, Kind::View);
        assert!(call("/no/such/file.db", json!({"op": "test"}))
            .unwrap_err()
            .contains("no such file"));
        assert!(call("", json!({"op": "test"})).is_err());
    }

    #[test]
    fn rows_are_counted_under_a_filter() {
        let (_dir, path) = database();
        let all = call(&path, json!({"op": "count", "table": "customers"})).unwrap();
        assert_eq!(all["total"], 3);
        let some = call(
            &path,
            json!({"op": "count", "table": "customers", "filter": "name LIKE 'B%'",
                   "order": [{"column": "zzz", "desc": false}]}),
        )
        .unwrap();
        assert_eq!(some["total"], 1);
        let query = call(
            &path,
            json!({"op": "count", "sql": "SELECT id FROM customers WHERE id > 1"}),
        )
        .unwrap();
        assert_eq!(query["total"], 2);
        assert!(call(
            &path,
            json!({"op": "count", "table": "customers", "filter": "1; SELECT 2"})
        )
        .is_err());
    }

    #[test]
    fn a_table_is_read_by_pages_sorted_and_filtered() {
        let (_dir, path) = database();
        let page = |extra: Value| {
            let mut r = json!({"op": "rows", "schema": "main", "table": "customers", "limit": 2});
            for (k, v) in extra.as_object().unwrap() {
                r[k] = v.clone();
            }
            call(&path, r).unwrap()
        };
        let first = page(json!({}));
        assert_eq!(first["columns"][0]["name"], "id");
        assert_eq!(first["rows"], json!([["1", "Ann", null], ["2", "Bob", ""]]));
        assert_eq!(first["has_more"], true);
        assert_eq!(first["primary_key"], json!(["id"]));
        assert_eq!(first["editable"], true);
        let second = page(json!({"offset": 2}));
        assert_eq!(second["rows"].as_array().unwrap().len(), 1);
        assert_eq!(second["has_more"], false);
        let sorted = page(json!({"order": [{"column": "name", "desc": true}]}));
        assert_eq!(sorted["rows"][0][1], "Cé, \"d\"");
        let filtered = page(json!({"filter": "name LIKE 'B%'"}));
        assert_eq!(filtered["rows"], json!([["2", "Bob", ""]]));
        assert!(call(&path, json!({"op": "rows", "table": "customers", "limit": 5, "order": [{"column": "zzz", "desc": false}]})).is_err());
        assert!(call(
            &path,
            json!({"op": "rows", "table": "customers", "limit": 5, "filter": "1; DROP TABLE log"})
        )
        .is_err());
        // A table named with a space, a view, a table without a key.
        let items = call(
            &path,
            json!({"op": "rows", "table": "Order Items", "limit": 10}),
        )
        .unwrap();
        assert_eq!(items["rows"][0], json!(["1", "1", "2.5"]));
        let view = call(&path, json!({"op": "rows", "table": "named", "limit": 10})).unwrap();
        assert_eq!(view["editable"], false);
        let log = call(&path, json!({"op": "rows", "table": "log", "limit": 10})).unwrap();
        assert_eq!(log["ctids"], json!(["1", "2"]));
        assert_eq!(
            log["rows"][0],
            json!(["a", null]),
            "the row id is not a column"
        );
        assert_eq!(log["editable"], true);
    }

    #[test]
    fn changes_are_all_or_none_and_a_changed_row_is_refused() {
        let (_dir, path) = database();
        let change = |changes: Value| {
            call(
                &path,
                json!({"op": "apply", "schema": "main", "table": "customers", "changes": changes}),
            )
        };
        let done = change(json!([
            {"kind": "update", "key": [["id", "1"]], "set": [["name", "Anna"], ["note", null]]},
            {"kind": "insert", "key": [], "set": [["id", "9"], ["name", "Zed"]]},
            {"kind": "delete", "key": [["id", "2"]], "set": []},
        ]))
        .unwrap();
        assert_eq!(done["applied"], 3);
        let page = call(
            &path,
            json!({"op": "rows", "table": "customers", "limit": 10}),
        )
        .unwrap();
        assert_eq!(page["rows"][0], json!(["1", "Anna", null]));
        assert_eq!(page["rows"].as_array().unwrap().len(), 3);
        // One of them is wrong: nothing is made.
        let refused = change(json!([
            {"kind": "update", "key": [["id", "1"]], "set": [["name", "Again"]]},
            {"kind": "update", "key": [["id", "404"]], "set": [["name", "Nobody"]]},
        ]));
        assert!(refused.unwrap_err().contains("0 rows touched"));
        let page = call(
            &path,
            json!({"op": "rows", "table": "customers", "limit": 10}),
        )
        .unwrap();
        assert_eq!(page["rows"][0][1], "Anna");
        // A quote in a value is a value.
        change(json!([{"kind": "update", "key": [["id", "1"]], "set": [["name", "O'Hara"]]}]))
            .unwrap();
        // A table without a key: the row's place and what it was read with.
        let by_place = |original: Value| {
            call(
                &path,
                json!({"op": "apply", "table": "log", "changes": [
                {"kind": "update", "key": [["rowid", "1"]], "set": [["msg", "A"]], "original": original}]}),
            )
        };
        assert!(by_place(json!([["msg", "changed meanwhile"]])).is_err());
        assert_eq!(
            by_place(json!([["msg", "a"], ["at", null]])).unwrap()["applied"],
            1
        );
        // Read only.
        let mut request = json!({"op": "apply", "table": "customers", "changes": []});
        request["target"] = json!({"database": path, "read_only": true});
        let open = |r: &Request| Session::open(r).map(|s| Box::new(s) as Box<dyn Engine>);
        let refused = common::run(&request.to_string(), &mut None, &open, false);
        assert!(refused.unwrap_err().contains("read-only"));
    }

    #[test]
    fn rows_without_type_affinity_or_with_blobs_are_found_by_what_is_shown() {
        let (_dir, path) = database();
        Connection::open(&path)
            .unwrap()
            .execute_batch(
                "CREATE TABLE loose (a, b);
                 INSERT INTO loose VALUES (5, x'9f00'), (2.0, 'z');
                 CREATE TABLE untyped_key (k PRIMARY KEY, v);
                 INSERT INTO untyped_key VALUES (7, 'q');",
            )
            .unwrap();
        let apply = |table: &str, change: Value| {
            call(
                &path,
                json!({"op": "apply", "table": table, "changes": [change]}),
            )
        };
        let done = apply(
            "loose",
            json!({"kind": "update", "key": [["rowid", "1"]], "set": [["a", "6"]],
                   "original": [["a", "5"], ["b", "x'9f00'"]]}),
        )
        .unwrap();
        assert_eq!(done["applied"], 1);
        let done = apply(
            "loose",
            json!({"kind": "delete", "key": [["rowid", "2"]], "set": [],
                   "original": [["a", "2"], ["b", "z"]]}),
        )
        .unwrap();
        assert_eq!(done["applied"], 1);
        let done = apply(
            "untyped_key",
            json!({"kind": "delete", "key": [["k", "7"]], "set": []}),
        )
        .unwrap();
        assert_eq!(done["applied"], 1);
    }

    #[test]
    fn the_console_runs_statements_in_order_and_stops_at_the_first_error() {
        let (_dir, path) = database();
        let answer = call(
            &path,
            json!({"op": "query", "statements": [
                "SELECT id FROM customers",
                "INSERT INTO log VALUES ('c', 'x'); INSERT INTO log VALUES ('d', 'y')",
                "UPDATE customers SET note = 'n' WHERE id < 3",
                "SELEKT 1",
                "SELECT 2",
            ]}),
        )
        .unwrap();
        let results = answer["results"].as_array().unwrap();
        assert_eq!(results.len(), 3);
        // Writes come after it: read at once, as the rows were before them.
        assert_eq!(results[0]["deferred"], false);
        assert!(!results[0]["rows"].as_array().unwrap().is_empty());
        assert_eq!(results[2]["affected"], 2);
        assert_eq!(answer["failed"]["index"], 3);
        assert!(answer["failed"]["error"]
            .as_str()
            .unwrap()
            .contains("syntax"));
        // Nothing written after it: a reading is left to the tab.
        let answer = call(
            &path,
            json!({"op": "query", "statements": [
                "UPDATE customers SET note = 'm' WHERE id < 2",
                "SELECT id FROM customers",
            ]}),
        )
        .unwrap();
        assert_eq!(answer["results"][1]["deferred"], true);
        // All or none.
        let answer = call(
            &path,
            json!({"op": "query", "transaction": true, "statements": [
            "DELETE FROM log", "BAD"]}),
        )
        .unwrap();
        assert!(answer["failed"].is_object());
        let log = call(&path, json!({"op": "rows", "table": "log", "limit": 10})).unwrap();
        assert!(
            log["rows"].as_array().unwrap().len() >= 2,
            "the delete was undone"
        );
        // A query is paged by wrapping it.
        let page = call(
            &path,
            json!({"op": "rows", "sql": "SELECT name, id FROM customers", "limit": 2,
            "order": [{"column": "id", "desc": true}]}),
        )
        .unwrap();
        assert_eq!(page["rows"][0][1], "3");
        assert_eq!(page["has_more"], true);
        assert_eq!(page["editable"], false);
    }

    #[test]
    fn a_session_that_stays_keeps_its_transaction() {
        let (_dir, path) = database();
        let open = |r: &Request| Session::open(r).map(|s| Box::new(s) as Box<dyn Engine>);
        let mut slot = None;
        let mut ask = |statements: Value| {
            let request =
                json!({"op": "query", "target": {"database": path}, "statements": statements});
            common::run(&request.to_string(), &mut slot, &open, true).unwrap()
        };
        let began = ask(json!(["BEGIN", "DELETE FROM log"]));
        assert_eq!(began["in_transaction"], true);
        let ended = ask(json!(["ROLLBACK"]));
        assert_eq!(ended["in_transaction"], false);
        let count = ask(json!(["SELECT count(*) FROM log"]));
        assert_eq!(count["results"][0]["deferred"], true);
    }

    #[test]
    fn a_commit_that_fails_leaves_no_transaction_and_a_retry_writes_once() {
        let (_dir, path) = database();
        let open = |r: &Request| Session::open(r).map(|s| Box::new(s) as Box<dyn Engine>);
        let mut slot = None;
        let mut ask = |request: Value| {
            let mut request = request;
            request["target"] = json!({"database": path, "query_timeout": 1});
            common::run(&request.to_string(), &mut slot, &open, true)
        };
        let insert = json!({"op": "apply", "table": "log", "changes": [
            {"kind": "insert", "key": [], "set": [["msg", "once"]]}]});
        // A reader holds the file: the commit waits for it, then gives up.
        let reader = Connection::open(&path).unwrap();
        reader.execute_batch("BEGIN").unwrap();
        let _: i64 = reader
            .query_row("SELECT count(*) FROM log", [], |r| r.get(0))
            .unwrap();
        assert!(ask(insert.clone()).is_err());
        let console = json!({"op": "query", "transaction": true, "statements": [
            "INSERT INTO log VALUES ('console', NULL)"]});
        assert!(ask(console).is_err());
        reader.execute_batch("COMMIT").unwrap();
        let ended = ask(json!({"op": "query", "statements": ["SELECT 1"]})).unwrap();
        assert_eq!(ended["in_transaction"], false);
        assert_eq!(ask(insert).unwrap()["applied"], 1);
        let count: i64 = reader
            .query_row(
                "SELECT count(*) FROM log WHERE msg IN ('once', 'console')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn the_rows_are_exported_as_csv_and_json_with_nulls_apart() {
        let (dir, path) = database();
        let csv = dir.path().join("out.csv").to_string_lossy().into_owned();
        let done = call(
            &path,
            json!({"op": "export", "table": "customers", "format": "csv", "path": csv}),
        )
        .unwrap();
        assert_eq!(done["exported"], 3);
        assert_eq!(
            std::fs::read_to_string(&csv).unwrap(),
            "id,name,note\n1,Ann,\n2,Bob,\"\"\n3,\"Cé, \"\"d\"\"\",x\n"
        );
        let json_path = dir.path().join("out.json").to_string_lossy().into_owned();
        call(&path, json!({"op": "export", "sql": "SELECT id, note FROM customers WHERE id < 3", "format": "json", "path": json_path})).unwrap();
        let written: Value =
            serde_json::from_str(&std::fs::read_to_string(&json_path).unwrap()).unwrap();
        assert_eq!(
            written,
            json!([{"id": 1, "note": null}, {"id": 2, "note": ""}])
        );
        // A file that cannot be written leaves nothing and says why.
        assert!(call(
            &path,
            json!({"op": "export", "table": "log", "format": "csv", "path": "/no/such/dir/x.csv"})
        )
        .is_err());
    }

    #[test]
    fn the_structure_the_definition_and_the_catalog_are_read() {
        let (_dir, path) = database();
        let s = call(&path, json!({"op": "structure", "table": "Order Items"})).unwrap();
        let structure: Structure = serde_json::from_value(s["structure"].clone()).unwrap();
        assert_eq!(structure.columns[1].name, "customer_id");
        assert_eq!(structure.columns[2].kind, "REAL");
        assert!(structure
            .constraints
            .iter()
            .any(|c| c.kind == ConstraintKind::Primary));
        assert!(structure
            .constraints
            .iter()
            .any(|c| c.kind == ConstraintKind::Foreign
                && c.definition.contains("REFERENCES \"customers\"")));
        assert_eq!(structure.indexes[0].name, "items_customer");
        let c = call(&path, json!({"op": "structure", "table": "customers"})).unwrap();
        let customers: Structure = serde_json::from_value(c["structure"].clone()).unwrap();
        assert!(customers
            .indexes
            .iter()
            .any(|i| i.unique && i.name == "customers_name"));
        assert_eq!(customers.referenced_by[0].table, "Order Items");
        assert!(!customers.columns[1].nullable);
        let d = call(
            &path,
            json!({"op": "definition", "table": "named", "object": "view"}),
        )
        .unwrap();
        assert!(d["definition"]
            .as_str()
            .unwrap()
            .starts_with("CREATE VIEW named AS SELECT name"));
        let catalog = call(&path, json!({"op": "columns"})).unwrap();
        let keys: Vec<ForeignKey> =
            serde_json::from_value(catalog["foreign_keys"].clone()).unwrap();
        assert_eq!(
            (keys[0].table.as_str(), keys[0].ref_table.as_str()),
            ("Order Items", "customers")
        );
        let columns: Vec<TableColumn> = serde_json::from_value(catalog["columns"].clone()).unwrap();
        assert!(columns
            .iter()
            .any(|c| c.table == "customers" && c.name == "note"));
        // What this kind of source does not have is said, not guessed.
        assert!(call(&path, json!({"op": "dump"}))
            .unwrap_err()
            .contains("not available"));
    }
}
