//! The SQL console at the bottom of the window: a log of every command run,
//! and a place to write queries and run them.

use std::path::PathBuf;

use chrono::Local;
use iced::widget::text_editor;

use super::protocol::{sql_tokens, Dialect, Executed, TokenKind};

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
    /// The SQL of that connection: how the text is cut into statements.
    pub dialect: super::protocol::Dialect,
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
    /// The consoles: the first is the one whose queries are kept for the next
    /// time, the others last until the app is closed. The text of the active
    /// one is in `editor`, not in its entry here.
    pub docs: Vec<Doc>,
    pub active: usize,
    /// The statements of the run that is going, to find the one that failed.
    pub running_sql: Vec<String>,
    /// The word of the queries the server blamed, to underline.
    pub error_mark: Option<ErrorMark>,
    /// The number the next console is given.
    pub next_doc: usize,
}

/// A place of the queries: a line, a column and a length, in characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ErrorMark {
    pub line: usize,
    pub column: usize,
    pub len: usize,
}

/// A console, as its tab says.
pub struct Doc {
    pub title: String,
    pub content: text_editor::Content,
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

/// A text of SQL, laid out: keywords in capitals, a clause by line, the
/// statements one blank line apart.
pub fn format_sql(sql: &str) -> String {
    let options = sqlformat::FormatOptions {
        indent: sqlformat::Indent::Spaces(2),
        uppercase: Some(true),
        lines_between_queries: 2,
        ..sqlformat::FormatOptions::default()
    };
    sqlformat::format(sql, &sqlformat::QueryParams::None, &options)
}

impl Console {
    /// Marks the word the server blamed in a statement of the queries:
    /// `position` counts characters of the statement, from 1.
    pub fn mark_error(&mut self, statement: &str, position: usize) {
        self.error_mark = None;
        let text = self.editor.text();
        let Some(start) = text.find(statement) else {
            return;
        };
        let at = start
            + statement
                .char_indices()
                .nth(position.saturating_sub(1))
                .map_or(statement.len(), |(i, _)| i);
        let before = &text[..at];
        let line = before.matches('\n').count();
        let line_start = before.rfind('\n').map_or(0, |i| i + 1);
        let column = text[line_start..at].chars().count();
        let rest = &text[at..];
        let word = rest
            .chars()
            .take_while(|ch| ch.is_alphanumeric() || *ch == '_' || *ch == '.')
            .count();
        self.error_mark = Some(ErrorMark {
            line,
            column,
            len: word.max(1),
        });
    }

    /// Opens another console and goes to it.
    pub fn new_doc(&mut self) {
        let title = t!("db.console_title", n = self.next_doc).to_string();
        self.next_doc += 1;
        self.docs.push(Doc {
            title,
            content: text_editor::Content::new(),
        });
        let last = self.docs.len() - 1;
        self.switch_doc(last);
    }

    /// Goes to a console: what is written stays where it was.
    pub fn switch_doc(&mut self, to: usize) {
        if to >= self.docs.len() || to == self.active {
            return;
        }
        self.completion = None;
        let current = std::mem::take(&mut self.editor);
        self.docs[self.active].content = current;
        self.editor = std::mem::take(&mut self.docs[to].content);
        self.active = to;
    }

    /// Closes a console; the first one stays, empty, when it is the last.
    pub fn close_doc(&mut self, at: usize) {
        if at >= self.docs.len() {
            return;
        }
        self.completion = None;
        if self.docs.len() == 1 {
            self.editor = text_editor::Content::new();
            return;
        }
        // Its text goes with it; the one next to it takes the editor.
        if at == self.active {
            let next = if at + 1 < self.docs.len() {
                at + 1
            } else {
                at - 1
            };
            let current = std::mem::take(&mut self.editor);
            self.docs[self.active].content = current;
            self.editor = std::mem::take(&mut self.docs[next].content);
            self.docs.remove(at);
            self.active = if next > at { next - 1 } else { next };
        } else {
            self.docs.remove(at);
            if at < self.active {
                self.active -= 1;
            }
        }
    }

    /// Lays out the selection, or all the queries when nothing is selected.
    pub fn format(&mut self) {
        use text_editor::{Action, Edit};
        match self.editor.selection().filter(|s| !s.trim().is_empty()) {
            Some(selection) => {
                let text = format_sql(&selection);
                self.editor
                    .perform(Action::Edit(Edit::Paste(std::sync::Arc::new(text))));
            }
            None => {
                let text = self.editor.text();
                if !text.trim().is_empty() {
                    self.editor = text_editor::Content::with_text(format_sql(&text).trim_end());
                }
            }
        }
    }

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
            dialect: super::protocol::Dialect::default(),
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
            docs: vec![Doc {
                title: t!("db.console_title", n = 1).to_string(),
                content: text_editor::Content::new(),
            }],
            active: 0,
            running_sql: Vec::new(),
            error_mark: None,
            next_doc: 2,
        }
    }

    /// Keeps what is written for the next time.
    pub fn save(&self) {
        let Some(path) = queries_file() else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let text = if self.active == 0 {
            self.editor.text()
        } else {
            self.docs[0].content.text()
        };
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
            split_statements(from, self.dialect)
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
        statement_at(&text, caret, self.dialect)
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
        let span = statement_at(&text, caret_offset(&text, at.line, at.column), self.dialect)?;
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
pub fn split_statements(text: &str, dialect: Dialect) -> Vec<Span> {
    // `DELIMITER //` lines (a command of the mysql client, in every dump of
    // routines) change what ends a statement.
    if dialect == Dialect::Mysql && text.lines().any(|l| is_delimiter_line(l).is_some()) {
        return split_with_delimiters(text, dialect);
    }
    split_at_semicolons(text, 0, text.len(), dialect)
}

/// The new delimiter a `DELIMITER xx` line sets.
fn is_delimiter_line(line: &str) -> Option<String> {
    let line = line.trim();
    let word = line.get(.."DELIMITER".len())?;
    if !word.eq_ignore_ascii_case("DELIMITER") {
        return None;
    }
    let rest = line["DELIMITER".len()..].trim();
    (line.len() == "DELIMITER".len() || line[word.len()..].starts_with(char::is_whitespace)).then(
        || {
            if rest.is_empty() {
                ";".into()
            } else {
                rest.to_string()
            }
        },
    )
}

fn split_with_delimiters(text: &str, dialect: Dialect) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut delimiter = ";".to_string();
    let (mut seg_start, mut pos) = (0, 0);
    let mut flush = |from: usize, to: usize, delimiter: &str| {
        if delimiter == ";" {
            spans.extend(split_at_semicolons(text, from, to, dialect));
            return;
        }
        let mut start = from;
        while let Some(at) = text[start..to].find(delimiter) {
            let at = start + at;
            push_span(text, start, at, at + delimiter.len(), dialect, &mut spans);
            start = at + delimiter.len();
        }
        push_span(text, start, to, to, dialect, &mut spans);
    };
    for line in text.split_inclusive('\n') {
        let line_start = pos;
        pos += line.len();
        if let Some(next) = is_delimiter_line(line) {
            flush(seg_start, line_start, &delimiter);
            delimiter = next;
            seg_start = pos;
        }
    }
    flush(seg_start, text.len(), &delimiter);
    spans
}

/// The statements of `text[from..to]`, cut at the `;` that are tokens of
/// their own. A trigger, a procedure or a function keeps the `;` of its
/// `BEGIN … END` body.
fn split_at_semicolons(text: &str, from: usize, to: usize, dialect: Dialect) -> Vec<Span> {
    let part = &text[from..to];
    let mut spans = Vec::new();
    let mut start = from;
    let mut depth = 0usize;
    let mut words: Vec<String> = Vec::new();
    let tokens = sql_tokens(part, dialect);
    for (n, token) in tokens.iter().enumerate() {
        let (a, b) = (from + token.start, from + token.end);
        match token.kind {
            TokenKind::Punct if &text[a..b] == ";" && depth == 0 => {
                push_span(text, start, a, b, dialect, &mut spans);
                start = b;
                words.clear();
            }
            TokenKind::Word if dialect != Dialect::Postgres => {
                let word = text[a..b].to_uppercase();
                let routine = words.first().is_some_and(|w| w == "CREATE")
                    && words.iter().take(10).any(|w| {
                        matches!(w.as_str(), "TRIGGER" | "PROCEDURE" | "FUNCTION" | "EVENT")
                    });
                let next = tokens
                    .iter()
                    .skip(n + 1)
                    .find(|t| t.kind != TokenKind::Comment)
                    .map(|t| part[t.start..t.end].to_uppercase());
                match word.as_str() {
                    "BEGIN" | "CASE" if routine => depth += 1,
                    // `END IF`, `END LOOP`… close what was not counted.
                    "END"
                        if depth > 0
                            && !matches!(
                                next.as_deref(),
                                Some("IF" | "LOOP" | "WHILE" | "REPEAT")
                            ) =>
                    {
                        depth -= 1;
                    }
                    _ => {}
                }
                if words.len() < 16 {
                    words.push(word);
                }
            }
            _ => {}
        }
    }
    push_span(text, start, to, to, dialect, &mut spans);
    spans
}

/// Adds the statement of `text[start..end]`, trimmed, if it says anything.
fn push_span(
    text: &str,
    start: usize,
    end: usize,
    reach: usize,
    dialect: Dialect,
    spans: &mut Vec<Span>,
) {
    let end = end.min(text.len());
    let slice = &text[start..end];
    let lead = slice.len() - slice.trim_start().len();
    let trimmed = slice.trim();
    if trimmed.is_empty() || only_comments(trimmed, dialect) {
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
fn only_comments(text: &str, dialect: Dialect) -> bool {
    sql_tokens(text, dialect)
        .iter()
        .all(|t| t.kind == TokenKind::Comment)
}

/// The statement the caret is in; between two, the next one; after the last,
/// the last.
pub fn statement_at(text: &str, caret: usize, dialect: Dialect) -> Option<Span> {
    let spans = split_statements(text, dialect);
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
        split_statements(text, Dialect::Postgres)
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
            &text[statement_at(text, caret, Dialect::Postgres).unwrap().start
                ..statement_at(text, caret, Dialect::Postgres).unwrap().end]
        };
        assert_eq!(at(0), "SELECT 1");
        assert_eq!(at(5), "SELECT 1");
        // Right after the `;`, still that statement.
        assert_eq!(at(9), "SELECT 1");
        assert_eq!(at(10), "SELECT 2");
        assert_eq!(at(25), "SELECT 3");
        assert_eq!(at(text.len()), "SELECT 3");
        assert!(statement_at("   ", 1, Dialect::Postgres).is_none());
    }

    #[test]
    fn between_two_statements_the_next_one_is_taken() {
        let text = "SELECT 1;\n\n\nSELECT 2;";
        let span = statement_at(text, 11, Dialect::Postgres).unwrap();
        assert_eq!(&text[span.start..span.end], "SELECT 2");
        // Past the last one, the last.
        let span = statement_at("SELECT 1;\n\n", 11, Dialect::Postgres).unwrap();
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
    fn the_sql_is_laid_out_with_capitals_and_a_clause_by_line() {
        let out = format_sql("select a,b from t where a=1 and b<2; select 2");
        assert!(out.contains("SELECT\n  a,\n  b\nFROM\n  t\nWHERE"), "{out}");
        assert!(out.contains(";\n\nSELECT"), "{out}");
        // A text already laid out stays as it is.
        assert_eq!(format_sql(&out).trim_end(), out.trim_end());
    }

    #[test]
    fn formatting_works_on_the_selection_or_on_all() {
        use text_editor::{Action, Motion};
        let mut console = Console::load();
        console.editor = text_editor::Content::with_text("select 1;\nselect 2");
        console.format();
        assert!(console.editor.text().starts_with("SELECT\n  1;"));
        console.editor = text_editor::Content::with_text("select 1;\nselect 2");
        console.editor.perform(Action::Select(Motion::Right));
        console.editor.perform(Action::Select(Motion::Right));
        // Only `se` is selected: it is laid out, the rest is left alone.
        console.format();
        assert!(console.editor.text().ends_with("select 2"));
    }

    #[test]
    fn consoles_keep_their_own_text() {
        let mut console = Console::load();
        console.editor = text_editor::Content::with_text("SELECT 1");
        console.new_doc();
        assert_eq!((console.docs.len(), console.active), (2, 1));
        assert_eq!(console.editor.text().trim(), "");
        console.editor = text_editor::Content::with_text("SELECT 2");
        console.switch_doc(0);
        assert_eq!(console.editor.text().trim(), "SELECT 1");
        console.switch_doc(1);
        assert_eq!(console.editor.text().trim(), "SELECT 2");
        // Closing the active one goes to its neighbour, text intact.
        console.close_doc(1);
        assert_eq!((console.docs.len(), console.active), (1, 0));
        assert_eq!(console.editor.text().trim(), "SELECT 1");
        // The last one is emptied, not removed.
        console.close_doc(0);
        assert_eq!(console.docs.len(), 1);
        assert_eq!(console.editor.text(), "");
    }

    #[test]
    fn closing_a_console_before_the_active_one_keeps_the_active_one() {
        let mut console = Console::load();
        console.editor = text_editor::Content::with_text("a");
        console.new_doc();
        console.editor = text_editor::Content::with_text("b");
        console.new_doc();
        console.editor = text_editor::Content::with_text("c");
        console.close_doc(0);
        assert_eq!(console.active, 1);
        assert_eq!(console.editor.text().trim(), "c");
        console.switch_doc(0);
        assert_eq!(console.editor.text().trim(), "b");
    }

    #[test]
    fn the_word_the_server_blamed_is_marked() {
        let mut console = Console::load();
        console.editor = text_editor::Content::with_text("SELECT 1;\nSELECT nme FROM t");
        // Character 8 of the second statement: `nme`.
        console.mark_error("SELECT nme FROM t", 8);
        assert_eq!(
            console.error_mark,
            Some(ErrorMark {
                line: 1,
                column: 7,
                len: 3
            })
        );
        // A statement that is not there leaves nothing marked.
        console.mark_error("SELECT other", 3);
        assert_eq!(console.error_mark, None);
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

#[cfg(test)]
mod dialect_split_tests {
    use super::*;

    fn pieces(text: &str, dialect: Dialect) -> Vec<&str> {
        split_statements(text, dialect)
            .into_iter()
            .map(|s| &text[s.start..s.end])
            .collect()
    }

    #[test]
    fn a_mysql_dump_is_cut_where_mysql_cuts_it() {
        let dump = "INSERT INTO t VALUES ('it\\'s; ok'); SELECT 1; # a; comment\nSELECT `a;b`";
        assert_eq!(
            pieces(dump, Dialect::Mysql),
            vec![
                "INSERT INTO t VALUES ('it\\'s; ok')",
                "SELECT 1",
                "# a; comment\nSELECT `a;b`"
            ]
        );
    }

    #[test]
    fn a_trigger_body_is_one_statement() {
        let text = "CREATE TRIGGER x AFTER INSERT ON t BEGIN UPDATE u SET a = 1; DELETE FROM v; END; SELECT 2";
        let all = pieces(text, Dialect::Sqlite);
        assert_eq!(all.len(), 2, "{all:?}");
        assert!(all[0].ends_with("END"));
        // A transaction's BEGIN is not a block.
        assert_eq!(pieces("BEGIN; SELECT 1; COMMIT", Dialect::Sqlite).len(), 3);
    }

    #[test]
    fn mysql_delimiter_lines_change_what_ends_a_statement() {
        let text = "DELIMITER //\nCREATE PROCEDURE p() BEGIN SELECT 1; SELECT 2; END//\nDELIMITER ;\nSELECT 3;";
        let all = pieces(text, Dialect::Mysql);
        assert_eq!(
            all,
            vec![
                "CREATE PROCEDURE p() BEGIN SELECT 1; SELECT 2; END",
                "SELECT 3"
            ]
        );
    }
}
