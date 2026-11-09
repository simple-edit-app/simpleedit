//! The SQLite and MySQL drivers as programs: what the app does, one request
//! on the standard input and one answer line on the standard output, and a
//! session that stays (`--serve`).

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use serde_json::{json, Value};

const SQLITE: &str = env!("CARGO_BIN_EXE_simpleedit-db-sqlite");
const MYSQL: &str = env!("CARGO_BIN_EXE_simpleedit-db-mysql");

/// The protocol number the app speaks (the same file is compiled in both).
fn protocol_of(program: &str) -> String {
    let out = Command::new(program).arg("--protocol").output().unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn once(program: &str, request: &Value) -> Value {
    let mut child = Command::new(program)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(request.to_string().as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    serde_json::from_slice(&out.stdout).unwrap()
}

fn sqlite_file() -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.db").to_string_lossy().into_owned();
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch(
        "CREATE TABLE pets (id INTEGER PRIMARY KEY, name TEXT); INSERT INTO pets VALUES (1, 'Rex'), (2, 'Tom');",
    )
    .unwrap();
    (dir, path)
}

#[test]
fn the_drivers_speak_the_protocol_the_app_does() {
    assert_eq!(protocol_of(SQLITE), protocol_of(MYSQL));
    assert!(protocol_of(SQLITE).parse::<u32>().unwrap() >= 19);
}

#[test]
fn the_sqlite_driver_answers_one_request_per_process() {
    let (_dir, path) = sqlite_file();
    let target = json!({"dialect": "sqlite", "database": path});
    let test = once(SQLITE, &json!({"op": "test", "target": target}));
    assert_eq!(test["ok"], true, "{test}");
    assert!(test["version"].as_str().unwrap().starts_with("SQLite"));
    // What was run goes back with the answer, for the log of the app.
    assert_eq!(test["executed"][0]["sql"], "SELECT version()");
    let rows = once(
        SQLITE,
        &json!({"op": "rows", "target": target, "table": "pets", "limit": 10}),
    );
    assert_eq!(rows["rows"], json!([["1", "Rex"], ["2", "Tom"]]));
    let missing = once(
        SQLITE,
        &json!({"op": "test", "target": {"dialect": "sqlite", "database": "/no/such.db"}}),
    );
    assert_eq!(missing["ok"], false);
    assert!(missing["error"].as_str().unwrap().contains("no such file"));
}

#[test]
fn the_sqlite_driver_keeps_a_session_between_lines() {
    let (_dir, path) = sqlite_file();
    let target = json!({"dialect": "sqlite", "database": path});
    let mut child = Command::new(SQLITE)
        .arg("--serve")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let mut ask = |statements: Value| -> Value {
        let request = json!({"op": "query", "target": target, "statements": statements});
        writeln!(stdin, "{request}").unwrap();
        serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap()
    };
    let begun = ask(json!(["BEGIN", "DELETE FROM pets"]));
    assert_eq!(begun["in_transaction"], true);
    let ended = ask(json!(["ROLLBACK"]));
    assert_eq!(ended["in_transaction"], false);
    drop(stdin);
    let _ = child.wait();
}

/// Against a server: `SIMPLEEDIT_TEST_MYSQL=host:port:user:password:database`.
#[test]
fn the_mysql_driver_connects_when_a_server_is_given() {
    let Ok(spec) = std::env::var("SIMPLEEDIT_TEST_MYSQL") else {
        eprintln!("no SIMPLEEDIT_TEST_MYSQL: skipped");
        return;
    };
    let p: Vec<&str> = spec.split(':').collect();
    let target = json!({"dialect": "mysql", "host": p[0], "port": p[1].parse::<u16>().unwrap(),
        "user": p[2], "password": p[3], "database": p[4]});
    let test = once(MYSQL, &json!({"op": "test", "target": target}));
    assert_eq!(test["ok"], true, "{test}");
    // A wrong password is said, not hidden.
    let mut wrong = target.clone();
    wrong["password"] = json!("not-the-password");
    let refused = once(MYSQL, &json!({"op": "test", "target": wrong}));
    assert_eq!(refused["ok"], false);
    assert!(!refused["error"].as_str().unwrap().is_empty());
}
