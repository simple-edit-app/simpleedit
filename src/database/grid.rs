//! What can be done to the rows of a table opened in a tab: edits and
//! deletions that wait for the Commit button, the cell being typed in, and
//! the date picker.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{Datelike, Local, NaiveDate, Timelike};

use super::protocol::{Change, ChangeKind, Page};

// -------------------------------------------------------------- selection

/// How a block of cells is written to the clipboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyFormat {
    /// Tab-separated, no header: what a spreadsheet pastes.
    Tsv,
    Csv,
    /// One `INSERT` for each row.
    Insert,
    Markdown,
}

/// The block between two cells, whichever corner each is: top left, bottom
/// right.
pub fn block(a: (usize, usize), b: (usize, usize)) -> ((usize, usize), (usize, usize)) {
    ((a.0.min(b.0), a.1.min(b.1)), (a.0.max(b.0), a.1.max(b.1)))
}

/// The cells of a block as text. `table` is the name `INSERT` writes to.
pub fn block_text(
    page: &Page,
    pending: &Pending,
    from: (usize, usize),
    to: (usize, usize),
    format: CopyFormat,
    table: &str,
    dialect: super::protocol::Dialect,
) -> String {
    let (from, to) = block(from, to);
    let last_col = to.1.min(page.columns.len().saturating_sub(1));
    let last_row = to.0.min(pending.rows(page).saturating_sub(1));
    let names: Vec<&str> = (from.1..=last_col)
        .map(|c| page.columns[c].name.as_str())
        .collect();
    let rows: Vec<Vec<Cell>> = (from.0..=last_row)
        .map(|r| {
            (from.1..=last_col)
                .map(|c| pending.cell(page, r, c))
                .collect()
        })
        .collect();
    let plain = |cell: &Cell| match cell {
        Cell::Value(v) => v.to_string(),
        _ => String::new(),
    };
    match format {
        CopyFormat::Tsv => rows
            .iter()
            .map(|row| row.iter().map(plain).collect::<Vec<_>>().join("\t"))
            .collect::<Vec<_>>()
            .join("\n"),
        CopyFormat::Csv => {
            let quote = |v: String| {
                if v.contains([',', '"', '\n', '\r']) {
                    format!("\"{}\"", v.replace('"', "\"\""))
                } else {
                    v
                }
            };
            let mut lines = vec![names
                .iter()
                .map(|n| quote(n.to_string()))
                .collect::<Vec<_>>()
                .join(",")];
            lines.extend(rows.iter().map(|row| {
                row.iter()
                    .map(|c| quote(plain(c)))
                    .collect::<Vec<_>>()
                    .join(",")
            }));
            lines.join("\n")
        }
        CopyFormat::Markdown => {
            let clean = |v: String| v.replace('|', "\\|").replace(['\n', '\r'], " ");
            let mut lines = vec![
                format!(
                    "| {} |",
                    names
                        .iter()
                        .map(|n| clean(n.to_string()))
                        .collect::<Vec<_>>()
                        .join(" | ")
                ),
                format!("|{}|", vec![" --- "; names.len()].join("|")),
            ];
            lines.extend(rows.iter().map(|row| {
                format!(
                    "| {} |",
                    row.iter()
                        .map(|c| clean(plain(c)))
                        .collect::<Vec<_>>()
                        .join(" | ")
                )
            }));
            lines.join("\n")
        }
        CopyFormat::Insert => {
            let columns: Vec<String> = names
                .iter()
                .map(|n| super::protocol::quote_ident_in(dialect, n))
                .collect();
            rows.iter()
                .map(|row| {
                    let values: Vec<String> = row
                        .iter()
                        .map(|c| match c {
                            Cell::Value(v) => super::protocol::quote_literal_in(dialect, v),
                            Cell::Null => "NULL".to_string(),
                            Cell::Default => "DEFAULT".to_string(),
                        })
                        .collect();
                    format!(
                        "INSERT INTO {table} ({}) VALUES ({});",
                        columns.join(", "),
                        values.join(", ")
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        }
    }
}

// ----------------------------------------------------------------- pending

/// A cell of a row that is added.
#[derive(Debug, Clone, PartialEq)]
pub enum NewCell {
    /// Not given: the column's default is used.
    Default,
    Null,
    Text(String),
}

/// What a cell shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cell<'a> {
    Value(&'a str),
    Null,
    /// A new row's cell that is left to the column's default.
    Default,
}

/// What the user asked for and the database has not heard of yet.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Pending {
    /// The new value of a cell, by row of the page and column; `None` is NULL.
    pub edits: BTreeMap<(usize, usize), Option<String>>,
    /// The rows to delete.
    pub deletes: BTreeSet<usize>,
    /// The rows to add: after the rows of the page, a cell for each column.
    pub inserts: Vec<Vec<NewCell>>,
}

impl Pending {
    pub fn is_empty(&self) -> bool {
        self.edits.is_empty() && self.deletes.is_empty() && self.inserts.is_empty()
    }

    /// How many rows are touched or added.
    pub fn count(&self) -> usize {
        let edited: BTreeSet<usize> = self.edits.keys().map(|(row, _)| *row).collect();
        edited.union(&self.deletes).count() + self.inserts.len()
    }

    pub fn is_deleted(&self, row: usize) -> bool {
        self.deletes.contains(&row)
    }

    pub fn is_edited(&self, row: usize, col: usize) -> bool {
        self.edits.contains_key(&(row, col))
    }

    /// The row is one that is added (it comes after the rows of the page).
    pub fn is_new(&self, page: &Page, row: usize) -> bool {
        row >= page.rows.len() && row - page.rows.len() < self.inserts.len()
    }

    /// How many rows there are to show: those of the page, and those added.
    pub fn rows(&self, page: &Page) -> usize {
        page.rows.len() + self.inserts.len()
    }

    /// Adds a row after the others, every cell left to its default; its place.
    pub fn add_row(&mut self, page: &Page) -> usize {
        self.inserts
            .push(vec![NewCell::Default; page.columns.len()]);
        page.rows.len() + self.inserts.len() - 1
    }

    /// Makes a cell hold `value`; asking for what the cell holds already
    /// takes the edit back. In a row that is added, an empty text is the
    /// column's default.
    pub fn set(&mut self, page: &Page, row: usize, col: usize, value: Option<String>) {
        if self.is_new(page, row) {
            if let Some(cell) = self
                .inserts
                .get_mut(row - page.rows.len())
                .and_then(|r| r.get_mut(col))
            {
                *cell = match value {
                    None => NewCell::Null,
                    Some(text) if text.is_empty() => NewCell::Default,
                    Some(text) => NewCell::Text(text),
                };
            }
            return;
        }
        let Some(original) = page.rows.get(row).and_then(|r| r.get(col)) else {
            return;
        };
        if *original == value {
            self.edits.remove(&(row, col));
        } else {
            self.edits.insert((row, col), value);
        }
    }

    /// Types into a cell as a form does, letter by letter: an empty text in a
    /// cell that was NULL is still NULL, not an empty text.
    pub fn type_into(&mut self, page: &Page, row: usize, col: usize, text: String) {
        let was_null = page
            .rows
            .get(row)
            .and_then(|r| r.get(col))
            .is_some_and(|v| v.is_none());
        let value = if text.is_empty() && (was_null || self.is_new(page, row)) {
            None
        } else {
            Some(text)
        };
        // In a row that is added, empty is the column's default.
        if self.is_new(page, row) && value.is_none() {
            self.set(page, row, col, Some(String::new()));
        } else {
            self.set(page, row, col, value);
        }
    }

    /// A cell of a row that is added goes back to the column's default.
    pub fn set_default(&mut self, page: &Page, row: usize, col: usize) {
        if self.is_new(page, row) {
            if let Some(cell) = self
                .inserts
                .get_mut(row - page.rows.len())
                .and_then(|r| r.get_mut(col))
            {
                *cell = NewCell::Default;
            }
        }
    }

    /// Deletes a row: one that is added is simply taken out.
    pub fn delete(&mut self, page: &Page, row: usize) {
        if self.is_new(page, row) {
            self.inserts.remove(row - page.rows.len());
        } else {
            self.deletes.insert(row);
        }
    }

    /// Takes back what was asked of a row.
    pub fn revert_row(&mut self, page: &Page, row: usize) {
        if self.is_new(page, row) {
            self.inserts.remove(row - page.rows.len());
            return;
        }
        self.deletes.remove(&row);
        self.edits.retain(|(r, _), _| *r != row);
    }

    /// What a cell holds: the value being set, or the one that was read.
    pub fn cell<'a>(&'a self, page: &'a Page, row: usize, col: usize) -> Cell<'a> {
        if row >= page.rows.len() {
            return match self
                .inserts
                .get(row - page.rows.len())
                .and_then(|r| r.get(col))
            {
                Some(NewCell::Text(text)) => Cell::Value(text),
                Some(NewCell::Null) => Cell::Null,
                _ => Cell::Default,
            };
        }
        let shown = match self.edits.get(&(row, col)) {
            Some(value) => value.as_deref(),
            None => page.rows[row][col].as_deref(),
        };
        shown.map_or(Cell::Null, Cell::Value)
    }

    /// The text of a cell, none for NULL (and a default).
    pub fn value<'a>(&'a self, page: &'a Page, row: usize, col: usize) -> Option<&'a str> {
        match self.cell(page, row, col) {
            Cell::Value(text) => Some(text),
            _ => None,
        }
    }

    /// What to tell the database: the updates, the deletions, then the rows
    /// that are added (after the deletions: a key can be used again). A row is
    /// found by the primary key it had when it was read.
    pub fn changes(&self, page: &Page) -> Vec<Change> {
        // How a row is found again: its primary key, or else its place.
        let key_of = |row: usize| -> Option<Vec<(String, String)>> {
            if page.primary_key.is_empty() {
                return Some(vec![("ctid".to_string(), page.ctids.get(row)?.clone())]);
            }
            page.primary_key
                .iter()
                .map(|name| {
                    let col = page.columns.iter().position(|c| c.name == *name)?;
                    Some((name.clone(), page.rows.get(row)?.get(col)?.clone()?))
                })
                .collect()
        };
        // Without a key, the row must also have all it was read with.
        let original_of = |row: usize| -> Vec<(String, Option<String>)> {
            if !page.primary_key.is_empty() {
                return Vec::new();
            }
            page.columns
                .iter()
                .zip(page.rows.get(row).into_iter().flatten())
                .map(|(c, v)| (c.name.clone(), v.clone()))
                .collect()
        };
        let mut out = Vec::new();
        let edited: BTreeSet<usize> = self.edits.keys().map(|(row, _)| *row).collect();
        for row in edited.difference(&self.deletes) {
            let Some(key) = key_of(*row) else { continue };
            let set = self
                .edits
                .iter()
                .filter(|((r, _), _)| r == row)
                .map(|((_, col), value)| (page.columns[*col].name.clone(), value.clone()))
                .collect();
            out.push(Change {
                kind: ChangeKind::Update,
                key,
                set,
                original: original_of(*row),
            });
        }
        for row in &self.deletes {
            if let Some(key) = key_of(*row) {
                out.push(Change {
                    kind: ChangeKind::Delete,
                    key,
                    set: Vec::new(),
                    original: original_of(*row),
                });
            }
        }
        for cells in &self.inserts {
            // What was not given is left out: the column's default applies.
            let set = cells
                .iter()
                .enumerate()
                .filter_map(|(col, cell)| {
                    let name = page.columns.get(col)?.name.clone();
                    match cell {
                        NewCell::Default => None,
                        NewCell::Null => Some((name, None)),
                        NewCell::Text(text) => Some((name, Some(text.clone()))),
                    }
                })
                .collect();
            out.push(Change {
                kind: ChangeKind::Insert,
                key: Vec::new(),
                set,
                original: Vec::new(),
            });
        }
        out
    }
}

// ----------------------------------------------------------------- editing

/// The cell being typed in.
#[derive(Debug, Clone, PartialEq)]
pub struct Editing {
    pub row: usize,
    pub col: usize,
    pub text: String,
}

/// The most characters a cell of the grid shows.
pub const CELL_CHARS: usize = 200;

/// What a cell of the grid shows of its value: one line, cut after
/// `CELL_CHARS` characters. Laying out a value of 100 KB for every row that
/// scrolls into view makes the grid stutter; the whole value stays in the
/// page for copying, editing, the preview and the exports.
pub fn shown(value: &str) -> std::borrow::Cow<'_, str> {
    let long = value.len() > CELL_CHARS && value.chars().nth(CELL_CHARS).is_some();
    if !long && !value.contains(char::is_control) {
        return std::borrow::Cow::Borrowed(value);
    }
    let mut line: String = value
        .chars()
        .take(CELL_CHARS)
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    if long {
        line.push('…');
    }
    std::borrow::Cow::Owned(line)
}

// -------------------------------------------------------------------- date

/// What a date picker picks, from the type of the column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickKind {
    Date,
    Time,
    DateTime,
}

impl PickKind {
    /// The kind of a column type (`date`, `time without time zone`,
    /// `timestamp with time zone`, …), if it is one of the temporal ones.
    pub fn of(column_type: &str) -> Option<PickKind> {
        let t = column_type.trim().to_lowercase();
        // An array of dates (`date[]`, or `_date` as PostgreSQL names it)
        // holds a list, which one picked value would replace.
        if t.ends_with("[]") || t.starts_with('_') {
            return None;
        }
        // `datetime` (MySQL, SQLite) starts like `date` but keeps a time.
        if t.starts_with("timestamp") || t.starts_with("datetime") {
            Some(PickKind::DateTime)
        } else if t.starts_with("date") {
            Some(PickKind::Date)
        } else if t.starts_with("time") {
            Some(PickKind::Time)
        } else {
            None
        }
    }

    pub fn has_date(self) -> bool {
        self != PickKind::Time
    }

    pub fn has_time(self) -> bool {
        self != PickKind::Date
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimePart {
    Hour,
    Minute,
    Second,
}

/// The date picker of a cell: a month to look at, a day chosen, and a time.
#[derive(Debug, Clone, PartialEq)]
pub struct Picker {
    pub row: usize,
    pub col: usize,
    pub kind: PickKind,
    pub year: i32,
    pub month: u32,
    pub day: Option<u32>,
    pub hour: String,
    pub minute: String,
    pub second: String,
}

impl Picker {
    /// Starts from the value the cell has, if it can be read; else today.
    pub fn new(row: usize, col: usize, kind: PickKind, current: Option<&str>) -> Picker {
        let current = current.unwrap_or("").trim();
        let date = current
            .get(..10)
            .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok());
        let time_text = match kind {
            PickKind::Time => current,
            _ => current.get(11..).unwrap_or(""),
        };
        let digits: Vec<&str> = time_text
            .get(..8)
            .unwrap_or("")
            .split(':')
            .filter(|p| p.len() == 2)
            .collect();
        let (hour, minute, second) = match digits.as_slice() {
            [h, m, s] => (h.to_string(), m.to_string(), s.to_string()),
            _ => ("00".to_string(), "00".to_string(), "00".to_string()),
        };
        let shown = date.unwrap_or_else(|| Local::now().date_naive());
        Picker {
            row,
            col,
            kind,
            year: shown.year(),
            month: shown.month(),
            day: date.map(|d| d.day()),
            hour,
            minute,
            second,
        }
    }

    /// Looks at another month: `months` before (negative) or after.
    pub fn shift(&mut self, months: i32) {
        let total = self.year * 12 + (self.month as i32 - 1) + months;
        self.year = total.div_euclid(12);
        self.month = total.rem_euclid(12) as u32 + 1;
        // A day that the new month does not have is dropped.
        if self.day.is_some_and(|d| d > self.days_in_month()) {
            self.day = None;
        }
    }

    pub fn days_in_month(&self) -> u32 {
        let (y, m) = if self.month == 12 {
            (self.year + 1, 1)
        } else {
            (self.year, self.month + 1)
        };
        NaiveDate::from_ymd_opt(y, m, 1)
            .and_then(|next| next.pred_opt())
            .map_or(30, |last| last.day())
    }

    /// The weekday of the first day of the month, Monday being 0.
    pub fn first_weekday(&self) -> u32 {
        NaiveDate::from_ymd_opt(self.year, self.month, 1)
            .map_or(0, |d| d.weekday().num_days_from_monday())
    }

    pub fn today(&mut self) {
        let now = Local::now();
        let date = now.date_naive();
        self.year = date.year();
        self.month = date.month();
        self.day = Some(date.day());
        if self.kind.has_time() {
            self.hour = format!("{:02}", now.hour());
            self.minute = format!("{:02}", now.minute());
            self.second = format!("{:02}", now.second());
        }
    }

    pub fn set_time(&mut self, part: TimePart, text: String) {
        // Two digits at most.
        let text: String = text.chars().filter(char::is_ascii_digit).take(2).collect();
        match part {
            TimePart::Hour => self.hour = text,
            TimePart::Minute => self.minute = text,
            TimePart::Second => self.second = text,
        }
    }

    fn time_text(&self) -> Option<String> {
        let part = |text: &str, most: u32| -> Option<u32> {
            text.parse::<u32>().ok().filter(|n| *n <= most)
        };
        Some(format!(
            "{:02}:{:02}:{:02}",
            part(&self.hour, 23)?,
            part(&self.minute, 59)?,
            part(&self.second, 59)?
        ))
    }

    /// The value to give the cell, if what is picked makes one.
    pub fn value(&self) -> Option<String> {
        let date = || {
            let day = self.day?;
            NaiveDate::from_ymd_opt(self.year, self.month, day)
                .map(|d| d.format("%Y-%m-%d").to_string())
        };
        match self.kind {
            PickKind::Date => date(),
            PickKind::Time => self.time_text(),
            PickKind::DateTime => Some(format!("{} {}", date()?, self.time_text()?)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::protocol::Column;

    fn page() -> Page {
        let column = |name: &str| Column {
            name: name.into(),
            kind: "text".into(),
            nullable: true,
        };
        Page {
            columns: vec![column("id"), column("name"), column("note")],
            rows: vec![
                vec![Some("1".into()), Some("a".into()), None],
                vec![Some("2".into()), Some("b".into()), Some("x".into())],
                vec![Some("3".into()), Some("c".into()), None],
            ],
            has_more: false,
            primary_key: vec!["id".into()],
            ctids: Vec::new(),
            editable: true,
            source: None,
            offset: 0,
        }
    }

    #[test]
    fn an_edit_waits_and_shows_instead_of_the_old_value() {
        let page = page();
        let mut p = Pending::default();
        p.set(&page, 1, 1, Some("B".into()));
        assert_eq!(p.value(&page, 1, 1), Some("B"));
        assert_eq!(p.value(&page, 0, 1), Some("a"));
        assert!(p.is_edited(1, 1) && !p.is_edited(0, 1));
        assert_eq!(p.count(), 1);
        // NULL is a value too.
        p.set(&page, 1, 2, None);
        assert_eq!(p.value(&page, 1, 2), None);
        assert_eq!(p.count(), 1);
    }

    #[test]
    fn putting_back_what_was_read_takes_the_edit_back() {
        let page = page();
        let mut p = Pending::default();
        p.set(&page, 0, 1, Some("z".into()));
        p.set(&page, 0, 1, Some("a".into()));
        assert!(p.is_empty());
        // A NULL cell set to NULL is no change either.
        p.set(&page, 0, 2, None);
        assert!(p.is_empty());
    }

    #[test]
    fn a_row_is_reverted_with_its_edits_and_its_deletion() {
        let page = page();
        let mut p = Pending::default();
        p.set(&page, 0, 1, Some("z".into()));
        p.delete(&page, 0);
        p.delete(&page, 2);
        assert_eq!(p.count(), 2);
        p.revert_row(&page, 0);
        assert!(!p.is_deleted(0) && !p.is_edited(0, 1) && p.is_deleted(2));
    }

    #[test]
    fn the_changes_find_rows_by_the_key_they_had() {
        let page = page();
        let mut p = Pending::default();
        p.set(&page, 1, 1, Some("B".into()));
        p.set(&page, 1, 2, None);
        p.set(&page, 0, 1, Some("ignored".into()));
        p.delete(&page, 0);
        p.delete(&page, 2);
        let changes = p.changes(&page);
        // One update (the row 0 is deleted: its edit does not go), then deletes.
        assert_eq!(changes.len(), 3);
        assert_eq!(changes[0].kind, ChangeKind::Update);
        assert_eq!(changes[0].key, vec![("id".to_string(), "2".to_string())]);
        assert_eq!(
            changes[0].set,
            vec![
                ("name".to_string(), Some("B".to_string())),
                ("note".to_string(), None)
            ]
        );
        assert_eq!(changes[1].kind, ChangeKind::Delete);
        assert_eq!(changes[1].key[0].1, "1");
        assert_eq!(changes[2].key[0].1, "3");
    }

    #[test]
    fn a_table_without_a_key_makes_no_changes() {
        let mut page = page();
        page.primary_key.clear();
        let mut p = Pending::default();
        p.delete(&page, 0);
        p.set(&page, 1, 1, Some("B".into()));
        assert!(p.changes(&page).is_empty());
    }

    #[test]
    fn a_row_is_added_after_the_others_with_every_cell_left_to_its_default() {
        let page = page();
        let mut p = Pending::default();
        assert_eq!(p.rows(&page), 3);
        let row = p.add_row(&page);
        assert_eq!(row, 3);
        let second = p.add_row(&page);
        assert_eq!(second, 4);
        assert_eq!((p.rows(&page), p.count()), (5, 2));
        assert!(p.is_new(&page, 3) && p.is_new(&page, 4) && !p.is_new(&page, 2));
        assert!(!p.is_empty());
        assert_eq!(p.cell(&page, 3, 1), Cell::Default);
        p.set(&page, 3, 1, Some("hello".into()));
        p.set(&page, 3, 2, None);
        assert_eq!(p.cell(&page, 3, 1), Cell::Value("hello"));
        assert_eq!(p.cell(&page, 3, 2), Cell::Null);
        assert_eq!(p.value(&page, 3, 1), Some("hello"));
        // An empty text is the default again; so does asking for it.
        p.set(&page, 3, 1, Some(String::new()));
        assert_eq!(p.cell(&page, 3, 1), Cell::Default);
        p.set(&page, 3, 2, Some("x".into()));
        p.set_default(&page, 3, 2);
        assert_eq!(p.cell(&page, 3, 2), Cell::Default);
    }

    #[test]
    fn the_added_rows_become_inserts_with_only_what_was_given() {
        let page = page();
        let mut p = Pending::default();
        let row = p.add_row(&page);
        p.set(&page, row, 1, Some("new".into()));
        p.set(&page, row, 2, None);
        p.add_row(&page);
        p.delete(&page, 0);
        let changes = p.changes(&page);
        // The deletion, then the two rows.
        assert_eq!(changes.len(), 3);
        assert_eq!(changes[0].kind, ChangeKind::Delete);
        assert_eq!(changes[1].kind, ChangeKind::Insert);
        assert!(changes[1].key.is_empty());
        assert_eq!(
            changes[1].set,
            vec![
                ("name".to_string(), Some("new".to_string())),
                ("note".to_string(), None)
            ]
        );
        // Nothing given: the database fills the row in.
        assert_eq!(changes[2].kind, ChangeKind::Insert);
        assert!(changes[2].set.is_empty());
    }

    #[test]
    fn a_row_that_is_added_is_removed_not_marked_and_the_others_move_up() {
        let page = page();
        let mut p = Pending::default();
        let first = p.add_row(&page);
        let second = p.add_row(&page);
        p.set(&page, second, 1, Some("kept".into()));
        p.delete(&page, first);
        assert!(p.deletes.is_empty());
        assert_eq!(p.inserts.len(), 1);
        // What was the second row is now where the first was.
        assert_eq!(p.cell(&page, first, 1), Cell::Value("kept"));
        p.revert_row(&page, first);
        assert!(p.is_empty());
        // A row of the page is only marked.
        p.delete(&page, 1);
        assert!(p.is_deleted(1) && p.inserts.is_empty());
    }

    #[test]
    fn a_table_without_a_key_finds_its_rows_by_place_and_values() {
        let mut loose = page();
        loose.primary_key.clear();
        loose.ctids = vec!["(0,1)".into(), "(0,2)".into(), "(0,3)".into()];
        let mut p = Pending::default();
        p.set(&loose, 1, 1, Some("B".into()));
        p.delete(&loose, 2);
        let changes = p.changes(&loose);
        assert_eq!(changes.len(), 2);
        // The place is the key, and every value of the row is kept to check.
        assert_eq!(
            changes[0].key,
            vec![("ctid".to_string(), "(0,2)".to_string())]
        );
        assert_eq!(
            changes[0].original,
            vec![
                ("id".to_string(), Some("2".to_string())),
                ("name".to_string(), Some("b".to_string())),
                ("note".to_string(), Some("x".to_string())),
            ]
        );
        assert_eq!(changes[1].key[0].1, "(0,3)");
        assert_eq!(changes[1].original[2], ("note".to_string(), None));
        // With a key, nothing more is needed.
        let with_key = page();
        let mut q = Pending::default();
        q.delete(&with_key, 0);
        assert!(q.changes(&with_key)[0].original.is_empty());
    }

    #[test]
    fn a_long_value_is_shown_on_one_line_and_cut_short() {
        assert_eq!(shown("short"), "short");
        assert_eq!(shown("two\nlines\r\n"), "two lines  ");
        let long = "é".repeat(100_000);
        let line = shown(&long);
        assert_eq!(line.chars().count(), CELL_CHARS + 1);
        assert!(line.ends_with('…'));
        // Exactly as long as the limit: nothing is cut.
        let full = "a".repeat(CELL_CHARS);
        assert_eq!(shown(&full), full.as_str());
    }

    #[test]
    fn the_column_type_says_which_picker() {
        assert_eq!(PickKind::of("date"), Some(PickKind::Date));
        assert_eq!(PickKind::of("time without time zone"), Some(PickKind::Time));
        assert_eq!(PickKind::of("time with time zone"), Some(PickKind::Time));
        assert_eq!(
            PickKind::of("timestamp without time zone"),
            Some(PickKind::DateTime)
        );
        assert_eq!(
            PickKind::of("timestamp(3) with time zone"),
            Some(PickKind::DateTime)
        );
        assert_eq!(PickKind::of("text"), None);
        assert_eq!(PickKind::of("integer"), None);
    }

    #[test]
    fn a_datetime_column_keeps_its_time_and_an_array_gets_no_picker() {
        assert_eq!(PickKind::of("datetime"), Some(PickKind::DateTime));
        assert_eq!(PickKind::of("DATETIME(6)"), Some(PickKind::DateTime));
        assert_eq!(PickKind::of("datetime2"), Some(PickKind::DateTime));
        assert_eq!(PickKind::of("date[]"), None);
        assert_eq!(PickKind::of("_date"), None);
        assert_eq!(PickKind::of("timestamp with time zone[]"), None);
        // Picking a day in a datetime cell keeps the time it had.
        let mut p = Picker::new(
            0,
            0,
            PickKind::of("datetime").unwrap(),
            Some("2026-02-10 14:30:00"),
        );
        p.day = Some(15);
        assert_eq!(p.value().as_deref(), Some("2026-02-15 14:30:00"));
    }

    #[test]
    fn the_picker_starts_from_the_value_of_the_cell() {
        let p = Picker::new(0, 0, PickKind::DateTime, Some("2026-02-10 12:34:56.789+01"));
        assert_eq!((p.year, p.month, p.day), (2026, 2, Some(10)));
        assert_eq!(
            (p.hour.as_str(), p.minute.as_str(), p.second.as_str()),
            ("12", "34", "56")
        );
        assert_eq!(p.value().as_deref(), Some("2026-02-10 12:34:56"));
        let t = Picker::new(0, 0, PickKind::Time, Some("07:05:09"));
        assert_eq!(t.value().as_deref(), Some("07:05:09"));
        let d = Picker::new(0, 0, PickKind::Date, Some("2026-03-01"));
        assert_eq!(d.value().as_deref(), Some("2026-03-01"));
        // Nothing to read: no day is picked yet.
        let empty = Picker::new(0, 0, PickKind::Date, None);
        assert_eq!(empty.day, None);
        assert_eq!(empty.value(), None);
    }

    #[test]
    fn the_picker_moves_between_months_and_years() {
        let mut p = Picker::new(0, 0, PickKind::Date, Some("2026-01-31"));
        p.shift(1);
        // February has no 31st.
        assert_eq!((p.year, p.month, p.day), (2026, 2, None));
        p.shift(-2);
        assert_eq!((p.year, p.month), (2025, 12));
        p.shift(12);
        assert_eq!((p.year, p.month), (2026, 12));
        p.shift(1);
        assert_eq!((p.year, p.month), (2027, 1));
    }

    #[test]
    fn a_month_knows_its_length_and_where_it_starts() {
        let mut p = Picker::new(0, 0, PickKind::Date, Some("2024-02-10"));
        assert_eq!(p.days_in_month(), 29);
        // 1 February 2024 was a Thursday.
        assert_eq!(p.first_weekday(), 3);
        p.shift(1);
        assert_eq!(p.days_in_month(), 31);
        p.shift(9);
        assert_eq!((p.year, p.month, p.days_in_month()), (2024, 12, 31));
    }

    #[test]
    fn a_time_that_does_not_exist_gives_no_value() {
        let mut p = Picker::new(0, 0, PickKind::Time, Some("07:05:09"));
        p.set_time(TimePart::Hour, "24".into());
        assert_eq!(p.value(), None);
        p.set_time(TimePart::Hour, "2a3".into());
        assert_eq!(p.hour, "23");
        p.set_time(TimePart::Minute, "".into());
        assert_eq!(p.value(), None);
        p.set_time(TimePart::Minute, "7".into());
        assert_eq!(p.value().as_deref(), Some("23:07:09"));
    }

    #[test]
    fn a_date_and_time_needs_both() {
        let mut p = Picker::new(0, 0, PickKind::DateTime, None);
        assert_eq!(p.value(), None);
        p.today();
        assert!(p.value().is_some());
        assert_eq!(p.value().unwrap().len(), 19);
    }

    #[test]
    fn a_block_is_the_same_from_any_corner() {
        assert_eq!(block((2, 1), (0, 3)), ((0, 1), (2, 3)));
        assert_eq!(block((1, 1), (1, 1)), ((1, 1), (1, 1)));
    }

    #[test]
    fn a_block_is_copied_in_the_four_formats() {
        let page = page();
        let mut p = Pending::default();
        p.set(&page, 0, 1, Some("a,\"q\"".into()));
        let text = |format| {
            block_text(
                &page,
                &p,
                (0, 1),
                (1, 2),
                format,
                "\"public\".\"t\"",
                crate::database::protocol::Dialect::Postgres,
            )
        };
        // NULL is an empty cell for a spreadsheet.
        assert_eq!(text(CopyFormat::Tsv), "a,\"q\"\t\nb\tx");
        assert_eq!(text(CopyFormat::Csv), "name,note\n\"a,\"\"q\"\"\",\nb,x");
        assert_eq!(
            text(CopyFormat::Markdown),
            "| name | note |\n| --- | --- |\n| a,\"q\" |  |\n| b | x |"
        );
        // INSERT keeps NULL as NULL, and quotes what it writes.
        assert_eq!(
            text(CopyFormat::Insert),
            "INSERT INTO \"public\".\"t\" (\"name\", \"note\") VALUES ('a,\"q\"', NULL);\n\
             INSERT INTO \"public\".\"t\" (\"name\", \"note\") VALUES ('b', 'x');"
        );
        // A block past the rows and columns is cut to what exists.
        assert_eq!(
            block_text(
                &page,
                &p,
                (2, 2),
                (9, 9),
                CopyFormat::Tsv,
                "t",
                crate::database::protocol::Dialect::Postgres
            ),
            "".to_string()
        );
    }

    #[test]
    fn typing_in_a_form_keeps_null_until_something_is_typed() {
        let page = page();
        let mut p = Pending::default();
        // The note of the first row is NULL.
        p.type_into(&page, 0, 2, "a".into());
        assert_eq!(p.value(&page, 0, 2), Some("a"));
        // Back to nothing: it is NULL again, not an empty text, and no edit.
        p.type_into(&page, 0, 2, String::new());
        assert!(!p.is_edited(0, 2));
        assert_eq!(p.cell(&page, 0, 2), Cell::Null);
        // A cell that held a text and is emptied holds an empty text.
        p.type_into(&page, 1, 2, String::new());
        assert_eq!(p.value(&page, 1, 2), Some(""));
    }
}
