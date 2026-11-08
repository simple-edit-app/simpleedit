//! The SQL console at the bottom of the window: a log of every command run,
//! and a place to write queries and run them.

use std::path::PathBuf;

use chrono::Local;
use iced::widget::text_editor;

use super::protocol::Executed;

/// Most entries kept in the log.
const MAX_LOG: usize = 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsoleTab {
    Log,
    Queries,
}

/// A command that was run, as the log shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct LogEntry {
    /// The time it ended, `HH:MM:SS`.
    pub time: String,
    /// The connection it ran on.
    pub connection: String,
    pub sql: String,
    pub ms: u64,
    /// Rows read or touched.
    pub rows: Option<u64>,
    pub error: Option<String>,
    /// Run by the driver to know the database, not asked for.
    pub internal: bool,
}

impl LogEntry {
    /// The entry as text, to copy: its head, then its SQL, then its error.
    pub fn text(&self) -> String {
        let mut head = format!(
            "{} {} {} {} ms",
            self.time,
            self.connection,
            if self.error.is_none() { "✓" } else { "✗" },
            self.ms
        );
        if let Some(rows) = self.rows {
            head.push_str(&format!(" {rows} rows"));
        }
        let mut out = format!("{head}\n{}", self.sql);
        if let Some(error) = &self.error {
            out.push('\n');
            out.push_str(error);
        }
        out
    }
}

pub struct Console {
    pub open: bool,
    pub tab: ConsoleTab,
    pub log: Vec<LogEntry>,
    pub editor: text_editor::Content,
    /// The connection the queries run on.
    pub connection: Option<u64>,
    /// The database of the server they run in (the connection's own when empty).
    pub database: String,
    /// A transaction is open on the session of the console.
    pub in_transaction: bool,
    pub running: bool,
    /// Stops what runs: the task is dropped, and the driver with it.
    pub abort: Option<iced::task::Handle>,
    /// What the last run said: whether it went well, and how.
    pub last: Option<(bool, String)>,
    /// The log lists what the driver runs to know the database too.
    pub show_internal: bool,
    /// The list of proposals while writing a query.
    pub completion: Option<Completion>,
    /// The entry of the log that was clicked: what is copied.
    pub selected_log: Option<usize>,
    /// Statements that wait for a yes: they are risky.
    pub guard: Option<Guard>,
    /// What is typed to find an entry of the log.
    pub log_filter: String,
}

/// What waits for a confirmation, and why.
#[derive(Debug, Clone, PartialEq)]
pub struct Guard {
    pub conn: u64,
    pub statements: Vec<String>,
    pub risk: super::protocol::Risk,
}

/// The proposals shown under the caret.
#[derive(Debug, Clone, PartialEq)]
pub struct Completion {
    pub suggestions: super::completion::Suggestions,
    pub selected: usize,
    /// Where the fragment typed starts: its line and its column in characters.
    pub line: usize,
    pub column: usize,
    /// The fragment typed, in characters (what an accepted item replaces).
    pub typed: usize,
}

fn queries_file() -> Option<PathBuf> {
    Some(
        crate::fsutil::data_dir()?
            .join("simpleedit")
            .join("queries.sql"),
    )
}

impl Console {
    /// Whether an entry of the log is what the filter looks for (its SQL, its
    /// connection or its error, whatever the case).
    pub fn matches(&self, entry: &LogEntry) -> bool {
        let needle = self.log_filter.trim().to_lowercase();
        needle.is_empty()
            || entry.sql.to_lowercase().contains(&needle)
            || entry.connection.to_lowercase().contains(&needle)
            || entry
                .error
                .as_ref()
                .is_some_and(|e| e.to_lowercase().contains(&needle))
    }

    /// Adds a statement at the end of the queries, on its own lines.
    pub fn append_query(&mut self, sql: &str) {
        use text_editor::{Action, Edit, Motion};
        self.editor.perform(Action::Move(Motion::DocumentEnd));
        let written = self.editor.text();
        // A blank line between statements, and a `;` to end the last one.
        let separator = if written.trim().is_empty() {
            String::new()
        } else {
            let closed = written.trim_end().ends_with(';');
            let newlines = written.len() - written.trim_end_matches('\n').len();
            format!(
                "{}{}",
                if closed { "" } else { ";" },
                "\n".repeat(2usize.saturating_sub(newlines))
            )
        };
        let text = format!("{separator}{};\n", sql.trim().trim_end_matches(';'));
        self.editor
            .perform(Action::Edit(Edit::Paste(std::sync::Arc::new(text))));
    }
}

impl Console {
    /// The queries written last time are there again.
    pub fn load() -> Self {
        let text = queries_file()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .unwrap_or_default();
        Console {
            open: false,
            tab: ConsoleTab::Queries,
            log: Vec::new(),
            editor: text_editor::Content::with_text(&text),
            connection: None,
            database: String::new(),
            in_transaction: false,
            running: false,
            abort: None,
            last: None,
            show_internal: true,
            completion: None,
            selected_log: None,
            guard: None,
            log_filter: String::new(),
        }
    }

    /// Keeps what is written for the next time.
    pub fn save(&self) {
        let Some(path) = queries_file() else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let text = self.editor.text();
        let _ = crate::fsutil::write_atomic(&path, text.as_bytes(), true);
    }

    pub fn push(&mut self, connection: &str, executed: &[Executed]) {
        let time = Local::now().format("%H:%M:%S").to_string();
        for command in executed {
            self.log.push(LogEntry {
                time: time.clone(),
                connection: connection.to_string(),
                sql: command.sql.clone(),
                ms: command.ms,
                rows: command.rows,
                error: command.error.clone(),
                internal: command.internal,
            });
        }
        if self.log.len() > MAX_LOG {
            let extra = self.log.len() - MAX_LOG;
            self.log.drain(..extra);
            // The entries moved up.
            self.selected_log = self.selected_log.and_then(|at| at.checked_sub(extra));
        }
    }

    /// What to run: all the statements, or the selection, or the statement
    /// the caret is in.
    pub fn statements(&self, all: bool) -> Vec<String> {
        let text = self.editor.text();
        let texts = |from: &str| -> Vec<String> {
            split_statements(from)
                .into_iter()
                .map(|span| from[span.start..span.end].to_string())
                .collect()
        };
        if all {
            return texts(&text);
        }
        if let Some(selection) = self.editor.selection().filter(|s| !s.trim().is_empty()) {
            return texts(&selection);
        }
        let cursor = self.editor.cursor().position;
        let caret = caret_offset(&text, cursor.line, cursor.column);
        statement_at(&text, caret)
            .map(|span| vec![text[span.start..span.end].to_string()])
            .unwrap_or_default()
    }
}

impl Console {
    /// The first and last line of the statement "Run" would run, to frame it:
    /// the one the caret is in, unless something is selected (then that runs).
    pub fn statement_lines(&self) -> Option<(usize, usize)> {
        if self
            .editor
            .selection()
            .is_some_and(|s| !s.trim().is_empty())
        {
            return None;
        }
        let text = self.editor.text();
        let at = self.editor.cursor().position;
        let span = statement_at(&text, caret_offset(&text, at.line, at.column))?;
        let lines = |offset: usize| text[..offset].matches('\n').count();
        Some((lines(span.start), lines(span.end)))
    }
}

// ------------------------------------------------------------ the statements

/// A statement in a text: its place, without the `;` and the blanks around.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    /// Where it stops counting for the caret: after its `;`.
    pub reach: usize,
}

/// Cuts a text into statements at the `;` that are not in a string, a quoted
/// name, a `$$` block or a comment. Blank statements, and the ones that are
/// only comments, are left out.
pub fn split_statements(text: &str) -> Vec<Span> {
    let bytes = text.as_bytes();
    let mut spans = Vec::new();
    let mut start = 0;
    let mut i = 0;
    // What the text is in at `i`.
    while i < bytes.len() {
        match bytes[i] {
            b'\'' | b'"' => {
                let quote = bytes[i];
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == quote {
                        // A doubled quote is a quote in the text.
                        if bytes.get(i + 1) == Some(&quote) {
                            i += 2;
                            continue;
                        }
                        break;
                    }
                    i += 1;
                }
                i += 1;
            }
            b'-' if bytes.get(i + 1) == Some(&b'-') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
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
            }
            b'$' => {
                // `$$` or `$tag$`, up to the same again.
                let tag_end = text[i + 1..].find('$').filter(|n| {
                    text[i + 1..i + 1 + n]
                        .chars()
                        .all(|c| c.is_alphanumeric() || c == '_')
                });
                match tag_end {
                    Some(n) => {
                        let tag = &text[i..i + n + 2];
                        i += n + 2;
                        match text[i..].find(tag) {
                            Some(close) => i += close + tag.len(),
                            None => i = bytes.len(),
                        }
                    }
                    None => i += 1,
                }
            }
            b';' => {
                push_span(text, start, i, i + 1, &mut spans);
                i += 1;
                start = i;
            }
            _ => i += 1,
        }
    }
    push_span(text, start, bytes.len(), bytes.len(), &mut spans);
    spans
}

/// Adds the statement of `text[start..end]`, trimmed, if it says anything.
fn push_span(text: &str, start: usize, end: usize, reach: usize, spans: &mut Vec<Span>) {
    let end = end.min(text.len());
    let slice = &text[start..end];
    let lead = slice.len() - slice.trim_start().len();
    let trimmed = slice.trim();
    if trimmed.is_empty() || only_comments(trimmed) {
        return;
    }
    let from = start + lead;
    spans.push(Span {
        start: from,
        end: from + trimmed.len(),
        reach,
    });
}

/// Whether a text has nothing in it but comments.
fn only_comments(text: &str) -> bool {
    let mut rest = text.trim_start();
    loop {
        if let Some(after) = rest.strip_prefix("--") {
            rest = after.split_once('\n').map_or("", |(_, r)| r).trim_start();
        } else if let Some(after) = rest.strip_prefix("/*") {
            match after.find("*/") {
                Some(close) => rest = after[close + 2..].trim_start(),
                None => return true,
            }
        } else {
            return rest.is_empty();
        }
    }
}

/// The statement the caret is in; between two, the next one; after the last,
/// the last.
pub fn statement_at(text: &str, caret: usize) -> Option<Span> {
    let spans = split_statements(text);
    spans
        .iter()
        .find(|s| caret <= s.reach && caret >= s.start)
        .or_else(|| spans.iter().find(|s| s.start >= caret))
        .or_else(|| spans.last())
        .copied()
}

/// The byte offset of a place of the editor (a line, and a column in it).
pub fn caret_offset(text: &str, line: usize, column: usize) -> usize {
    let mut offset = 0;
    for (n, l) in text.split('\n').enumerate() {
        if n == line {
            let mut at = column.min(l.len());
            while !l.is_char_boundary(at) {
                at -= 1;
            }
            return offset + at;
        }
        offset += l.len() + 1;
    }
    text.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn statements(text: &str) -> Vec<&str> {
        split_statements(text)
            .into_iter()
            .map(|s| &text[s.start..s.end])
            .collect()
    }

    #[test]
    fn a_text_is_cut_at_the_semicolons() {
        assert_eq!(
            statements("SELECT 1;\n\n  UPDATE t SET a = 2 ;\nDELETE FROM t"),
            vec!["SELECT 1", "UPDATE t SET a = 2", "DELETE FROM t"]
        );
        assert!(statements("  ;; \n ; ").is_empty());
        assert!(statements("").is_empty());
    }

    #[test]
    fn a_semicolon_in_a_string_a_name_or_a_comment_does_not_cut() {
        assert_eq!(
            statements("SELECT 'a;b', \"c;d\"; SELECT 2"),
            vec!["SELECT 'a;b', \"c;d\"", "SELECT 2"]
        );
        assert_eq!(
            statements("SELECT 'it''s; ok'; SELECT 3"),
            vec!["SELECT 'it''s; ok'", "SELECT 3"]
        );
        assert_eq!(
            statements("SELECT 1 -- not; here\n; SELECT 2 /* nor; here /* nested; */ still */"),
            vec![
                "SELECT 1 -- not; here",
                "SELECT 2 /* nor; here /* nested; */ still */"
            ]
        );
    }

    #[test]
    fn a_dollar_quoted_body_is_one_piece() {
        let text = "CREATE FUNCTION f() RETURNS int AS $$ BEGIN RETURN 1; END; $$ LANGUAGE plpgsql; SELECT f()";
        let all = statements(text);
        assert_eq!(all.len(), 2);
        assert!(all[0].ends_with("LANGUAGE plpgsql"));
        let tagged = "DO $body$ BEGIN PERFORM 1; END $body$; SELECT 2";
        assert_eq!(statements(tagged).len(), 2);
        // A price is not a quote.
        assert_eq!(statements("SELECT $1; SELECT 2").len(), 2);
    }

    #[test]
    fn comments_alone_are_not_statements() {
        assert_eq!(
            statements("-- a comment\nSELECT 1;\n/* only this */;\n-- end"),
            vec!["-- a comment\nSELECT 1"]
        );
    }

    #[test]
    fn the_statement_at_the_caret_is_the_one_it_is_in() {
        let text = "SELECT 1;\nSELECT 2;\nSELECT 3";
        let at = |caret: usize| {
            &text[statement_at(text, caret).unwrap().start..statement_at(text, caret).unwrap().end]
        };
        assert_eq!(at(0), "SELECT 1");
        assert_eq!(at(5), "SELECT 1");
        // Right after the `;`, still that statement.
        assert_eq!(at(9), "SELECT 1");
        assert_eq!(at(10), "SELECT 2");
        assert_eq!(at(25), "SELECT 3");
        assert_eq!(at(text.len()), "SELECT 3");
        assert!(statement_at("   ", 1).is_none());
    }

    #[test]
    fn between_two_statements_the_next_one_is_taken() {
        let text = "SELECT 1;\n\n\nSELECT 2;";
        let span = statement_at(text, 11).unwrap();
        assert_eq!(&text[span.start..span.end], "SELECT 2");
        // Past the last one, the last.
        let span = statement_at("SELECT 1;\n\n", 11).unwrap();
        assert_eq!(&"SELECT 1;\n\n"[span.start..span.end], "SELECT 1");
    }

    #[test]
    fn a_place_of_the_editor_is_a_byte_offset() {
        let text = "ab\ncdé\nf";
        assert_eq!(caret_offset(text, 0, 0), 0);
        assert_eq!(caret_offset(text, 1, 2), 5);
        // The accent takes two bytes; a column inside it goes back.
        assert_eq!(caret_offset(text, 1, 4), 7);
        assert_eq!(caret_offset(text, 2, 1), 9);
        assert_eq!(caret_offset(text, 9, 0), text.len());
        assert_eq!(caret_offset(text, 0, 99), 2);
    }

    #[test]
    fn the_statement_that_would_run_is_the_one_framed() {
        use text_editor::{Action, Motion};
        let mut console = Console::load();
        console.editor =
            text_editor::Content::with_text("SELECT 1;\n\nSELECT a,\n  b\nFROM t;\nSELECT 3");
        // The caret starts on the first statement.
        assert_eq!(console.statement_lines(), Some((0, 0)));
        // Down to the multi-line one: all its lines.
        for _ in 0..2 {
            console.editor.perform(Action::Move(Motion::Down));
        }
        assert_eq!(console.statement_lines(), Some((2, 4)));
        console.editor.perform(Action::Move(Motion::Down));
        assert_eq!(console.statement_lines(), Some((2, 4)));
        // On the last one.
        console.editor.perform(Action::Move(Motion::DocumentEnd));
        assert_eq!(console.statement_lines(), Some((5, 5)));
        // A selection runs instead: nothing is framed.
        console.editor.perform(Action::SelectAll);
        assert_eq!(console.statement_lines(), None);
        // Nothing written, nothing framed.
        console.editor = text_editor::Content::new();
        assert_eq!(console.statement_lines(), None);
    }

    #[test]
    fn an_entry_is_copied_as_text_with_its_sql_and_its_error() {
        let ok = LogEntry {
            time: "10:00:00".into(),
            connection: "shop".into(),
            sql: "SELECT 1".into(),
            ms: 3,
            rows: Some(1),
            error: None,
            internal: false,
        };
        assert_eq!(ok.text(), "10:00:00 shop ✓ 3 ms 1 rows\nSELECT 1");
        let failed = LogEntry {
            error: Some("boom".into()),
            rows: None,
            ..ok
        };
        assert_eq!(failed.text(), "10:00:00 shop ✗ 3 ms\nSELECT 1\nboom");
    }

    #[test]
    fn the_selected_entry_follows_the_entries_when_the_oldest_are_dropped() {
        let mut console = Console::load();
        console.log.clear();
        let executed = |n: usize| Executed {
            sql: format!("SELECT {n}"),
            ms: 1,
            rows: None,
            error: None,
            internal: false,
        };
        let first: Vec<Executed> = (0..MAX_LOG).map(executed).collect();
        console.push("shop", &first);
        console.selected_log = Some(MAX_LOG - 1);
        console.push("shop", &[executed(9000), executed(9001)]);
        // Two went; the one that was last is two places nearer the start.
        assert_eq!(console.selected_log, Some(MAX_LOG - 3));
        assert_eq!(
            console.log[MAX_LOG - 3].sql,
            format!("SELECT {}", MAX_LOG - 1)
        );
        // One of the dropped ones is not selected any more.
        console.selected_log = Some(0);
        console.push("shop", &[executed(9002)]);
        assert_eq!(console.selected_log, None);
    }

    #[test]
    fn the_log_keeps_the_last_entries_only() {
        let mut console = Console::load();
        console.log.clear();
        let executed = |n: usize| Executed {
            sql: format!("SELECT {n}"),
            ms: 1,
            rows: Some(1),
            error: None,
            internal: false,
        };
        let batch: Vec<Executed> = (0..MAX_LOG + 20).map(executed).collect();
        console.push("shop", &batch);
        assert_eq!(console.log.len(), MAX_LOG);
        assert_eq!(console.log[0].sql, "SELECT 20");
        assert_eq!(console.log[0].connection, "shop");
    }

    #[test]
    fn what_runs_is_all_the_text_or_the_selection_or_the_caret_statement() {
        let mut console = Console::load();
        console.editor = text_editor::Content::with_text("SELECT 1;\nSELECT 2;\nSELECT 3;");
        assert_eq!(console.statements(true).len(), 3);
        // The caret is at the start: the first one.
        assert_eq!(console.statements(false), vec!["SELECT 1"]);
        // Down to the second line: the second one.
        console
            .editor
            .perform(text_editor::Action::Move(text_editor::Motion::Down));
        assert_eq!(console.statements(false), vec!["SELECT 2"]);
        // A selection of two lines: both.
        console
            .editor
            .perform(text_editor::Action::Select(text_editor::Motion::Down));
        let two = console.statements(false);
        assert!(!two.is_empty() && two.len() <= 2, "{two:?}");
        // Nothing written: nothing to run.
        console.editor = text_editor::Content::new();
        assert!(console.statements(false).is_empty());
        assert!(console.statements(true).is_empty());
    }

    #[test]
    fn the_log_is_searched_by_sql_connection_or_error() {
        let mut console = Console::load();
        console.log.clear();
        console.push(
            "shop",
            &[Executed {
                sql: "SELECT * FROM Orders".into(),
                ms: 1,
                rows: Some(1),
                error: Some("Boom".into()),
                internal: false,
            }],
        );
        let entry = console.log[0].clone();
        assert!(console.matches(&entry));
        for needle in ["orders", "SHOP", "boom"] {
            console.log_filter = needle.into();
            assert!(console.matches(&entry), "{needle}");
        }
        console.log_filter = "invoices".into();
        assert!(!console.matches(&entry));
    }

    #[test]
    fn an_entry_goes_back_into_the_queries_on_its_own_lines() {
        let mut console = Console::load();
        console.editor = text_editor::Content::new();
        console.append_query("SELECT 1;");
        assert_eq!(console.editor.text().trim_end(), "SELECT 1;");
        console.append_query("SELECT 2");
        assert_eq!(console.editor.text().trim_end(), "SELECT 1;\n\nSELECT 2;");
        console.editor = text_editor::Content::with_text("SELECT 0");
        console.append_query("SELECT 1");
        assert_eq!(console.editor.text().trim_end(), "SELECT 0;\n\nSELECT 1;");
    }
}
