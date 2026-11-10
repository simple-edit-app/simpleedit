//! The rows of an export, in the formats the driver does not write: SQL
//! `INSERT` statements, a Markdown table, an Excel sheet. The driver writes
//! a CSV (a NULL is an empty value with no quotes, an empty text is `""`);
//! this reads it and writes the other formats.

use std::path::Path;

use super::protocol::Dialect;

/// The rows of a CSV text as `Some(text)` or `None` for a NULL. The first
/// row is the header.
pub fn parse_csv(text: &str) -> Vec<Vec<Option<String>>> {
    let mut rows: Vec<Vec<Option<String>>> = Vec::new();
    let mut row: Vec<Option<String>> = Vec::new();
    let mut value = String::new();
    let mut quoted = false;
    let mut in_quotes = false;
    let mut any = false;
    let mut chars = text.chars().peekable();
    let finish = |value: &mut String, quoted: &mut bool, row: &mut Vec<Option<String>>| {
        let taken = std::mem::take(value);
        row.push(if taken.is_empty() && !*quoted {
            None
        } else {
            Some(taken)
        });
        *quoted = false;
    };
    while let Some(c) = chars.next() {
        any = true;
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    value.push('"');
                    chars.next();
                } else {
                    in_quotes = false;
                }
            } else {
                value.push(c);
            }
            continue;
        }
        match c {
            '"' => {
                in_quotes = true;
                quoted = true;
            }
            ',' => finish(&mut value, &mut quoted, &mut row),
            '\r' => {}
            '\n' => {
                finish(&mut value, &mut quoted, &mut row);
                rows.push(std::mem::take(&mut row));
                any = false;
            }
            other => value.push(other),
        }
    }
    if any {
        finish(&mut value, &mut quoted, &mut row);
        rows.push(row);
    }
    rows
}

fn sql_literal(value: &Option<String>, dialect: Dialect) -> String {
    match value {
        None => "NULL".to_string(),
        Some(text) => super::protocol::quote_literal_in(dialect, text),
    }
}

/// The records of a CSV, one at a time: a record goes on over the lines
/// while a quote is open.
pub fn csv_records<R: std::io::BufRead>(
    mut reader: R,
) -> impl Iterator<Item = std::io::Result<Vec<Option<String>>>> {
    std::iter::from_fn(move || {
        let mut text = String::new();
        loop {
            match reader.read_line(&mut text) {
                Err(e) => return Some(Err(e)),
                Ok(0) => break,
                Ok(_) => {
                    if text.matches('"').count().is_multiple_of(2) {
                        break;
                    }
                }
            }
        }
        if text.is_empty() {
            return None;
        }
        Some(Ok(parse_csv(&text).into_iter().next().unwrap_or_default()))
    })
}

type Records<'a> = &'a mut dyn Iterator<Item = std::io::Result<Vec<Option<String>>>>;

/// `INSERT` statements for the records (the first names the columns), a
/// hundred rows to a statement, written as they come.
pub fn write_inserts(
    out: &mut impl std::io::Write,
    table: &str,
    rows: Records,
    dialect: Dialect,
) -> std::io::Result<()> {
    let Some(header) = rows.next().transpose()? else {
        return Ok(());
    };
    let columns = header
        .iter()
        .map(|c| super::protocol::quote_ident_in(dialect, c.as_deref().unwrap_or("")))
        .collect::<Vec<_>>()
        .join(", ");
    let mut lines: Vec<String> = Vec::new();
    let mut flush = |lines: &mut Vec<String>| -> std::io::Result<()> {
        if lines.is_empty() {
            return Ok(());
        }
        write!(
            out,
            "INSERT INTO {table} ({columns}) VALUES\n{};\n",
            lines.join(",\n")
        )?;
        lines.clear();
        Ok(())
    };
    for row in rows {
        let values: Vec<String> = row?.iter().map(|v| sql_literal(v, dialect)).collect();
        lines.push(format!("  ({})", values.join(", ")));
        if lines.len() == 100 {
            flush(&mut lines)?;
        }
    }
    flush(&mut lines)
}

/// `INSERT` statements for the rows (the first of `rows` names the
/// columns), a hundred rows to a statement.
#[cfg(test)]
pub fn to_inserts(table: &str, rows: &[Vec<Option<String>>], dialect: Dialect) -> String {
    let mut out = Vec::new();
    let mut records = rows.iter().cloned().map(Ok);
    let _ = write_inserts(&mut out, table, &mut records, dialect);
    String::from_utf8_lossy(&out).into_owned()
}

/// A Markdown table: `|` and line breaks inside a value are escaped.
pub fn write_markdown(out: &mut impl std::io::Write, rows: Records) -> std::io::Result<()> {
    let Some(header) = rows.next().transpose()? else {
        return Ok(());
    };
    let cell = |v: &Option<String>| match v {
        None => "*NULL*".to_string(),
        Some(t) => t.replace('|', "\\|").replace(['\n', '\r'], " "),
    };
    let line = |row: &Vec<Option<String>>| {
        format!(
            "| {} |\n",
            row.iter().map(cell).collect::<Vec<_>>().join(" | ")
        )
    };
    out.write_all(line(&header).as_bytes())?;
    writeln!(out, "|{}", " --- |".repeat(header.len()))?;
    for row in rows {
        out.write_all(line(&row?).as_bytes())?;
    }
    Ok(())
}

#[cfg(test)]
pub fn to_markdown(rows: &[Vec<Option<String>>]) -> String {
    let mut out = Vec::new();
    let mut records = rows.iter().cloned().map(Ok);
    let _ = write_markdown(&mut out, &mut records);
    String::from_utf8_lossy(&out).into_owned()
}

/// Whether a text is a number a sheet should hold as one (no leading zero:
/// a code such as `01234` stays text).
fn is_number(text: &str) -> bool {
    let digits = text.strip_prefix('-').unwrap_or(text);
    let plain = !digits.is_empty()
        && digits.chars().all(|c| c.is_ascii_digit() || c == '.')
        && digits.matches('.').count() <= 1
        && !digits.starts_with('.')
        && !digits.ends_with('.');
    let zero_led = digits.len() > 1 && digits.starts_with('0') && !digits.starts_with("0.");
    plain && !zero_led && digits.len() < 16
}

/// An Excel file with one sheet, the first row (the names of the columns)
/// in bold, numbers as numbers.
pub fn write_xlsx(path: &Path, rows: &[Vec<Option<String>>]) -> Result<(), String> {
    use rust_xlsxwriter::{Format, Workbook};
    let mut book = Workbook::new();
    let sheet = book.add_worksheet();
    let bold = Format::new().set_bold();
    for (r, row) in rows.iter().enumerate() {
        for (c, value) in row.iter().enumerate() {
            let (r, c) = (r as u32, c as u16);
            let wrote = match value {
                None => continue,
                Some(text) if r == 0 => sheet.write_string_with_format(r, c, text, &bold),
                Some(text) if is_number(text) => match text.parse::<f64>() {
                    Ok(n) => sheet.write_number(r, c, n),
                    Err(_) => sheet.write_string(r, c, text),
                },
                Some(text) => sheet.write_string(r, c, text),
            };
            wrote.map_err(|e| e.to_string())?;
        }
    }
    book.save(path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn some(s: &str) -> Option<String> {
        Some(s.to_string())
    }

    #[test]
    fn a_csv_is_read_with_quotes_nulls_and_line_breaks() {
        let rows =
            parse_csv("id,name,note\n1,\"Smith, J\",\n2,,\"\"\n3,\"say \"\"hi\"\"\nnow\",x\n");
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[1], vec![some("1"), some("Smith, J"), None]);
        assert_eq!(
            rows[2],
            vec![some("2"), None, some("")],
            "NULL and empty text differ"
        );
        assert_eq!(rows[3][1], some("say \"hi\"\nnow"));
        // No final line break, or Windows ones.
        assert_eq!(parse_csv("a,b\r\n1,2").len(), 2);
        assert!(parse_csv("").is_empty());
    }

    #[test]
    fn rows_become_inserts_in_batches() {
        let mut rows = vec![vec![some("id"), some("na\"me")]];
        for i in 0..101 {
            rows.push(vec![
                some(&i.to_string()),
                if i == 1 { None } else { some("it's") },
            ]);
        }
        let sql = to_inserts("\"public\".\"t\"", &rows, Dialect::Postgres);
        assert!(sql.starts_with("INSERT INTO \"public\".\"t\" (\"id\", \"na\"\"me\") VALUES\n  ('0', 'it''s'),\n  ('1', NULL)"));
        assert_eq!(
            sql.matches("INSERT INTO").count(),
            2,
            "a hundred rows to a statement"
        );
        assert!(sql.trim_end().ends_with("('100', 'it''s');"));
        assert_eq!(to_inserts("t", &[], Dialect::Postgres), "");
        assert_eq!(
            to_inserts(
                "`t`",
                &[vec![Some("a".into())], vec![Some("C:\\x".into())]],
                Dialect::Mysql
            ),
            "INSERT INTO `t` (`a`) VALUES\n  ('C:\\\\x');\n"
        );
    }

    #[test]
    fn rows_become_a_markdown_table() {
        let rows = vec![
            vec![some("a"), some("b")],
            vec![some("x|y"), None],
            vec![some("1\n2"), some("")],
        ];
        assert_eq!(
            to_markdown(&rows),
            "| a | b |\n| --- | --- |\n| x\\|y | *NULL* |\n| 1 2 |  |\n"
        );
    }

    #[test]
    fn a_sheet_holds_numbers_as_numbers_and_codes_as_text() {
        assert!(is_number("42") && is_number("-3.5") && is_number("0.5"));
        assert!(!is_number("01234") && !is_number("1.2.3") && !is_number("") && !is_number("."));
        assert!(!is_number("12345678901234567890"));
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.xlsx");
        let rows = vec![
            vec![some("id"), some("zip")],
            vec![some("1"), some("01234")],
            vec![some("2"), None],
        ];
        write_xlsx(&file, &rows).unwrap();
        let bytes = std::fs::read(&file).unwrap();
        assert_eq!(&bytes[..2], b"PK", "a zip archive, as an xlsx is");
        assert!(bytes.len() > 500);
    }
}
