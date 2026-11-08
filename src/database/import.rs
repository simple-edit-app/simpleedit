//! Reading a file to put into a table: its fields, and which column each
//! one goes to.

use super::protocol::{ImportColumn, ImportSpec};

/// Most of the file that is read to show it and to count its columns.
pub const SAMPLE_BYTES: usize = 256 * 1024;
/// Most lines shown in the preview.
pub const PREVIEW_ROWS: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Csv,
    Json,
}

impl Format {
    /// What a file name says it is.
    pub fn of(name: &str) -> Format {
        if name.to_lowercase().ends_with(".json") {
            Format::Json
        } else {
            Format::Csv
        }
    }
}

/// The delimiters that are offered.
pub const DELIMITERS: [char; 4] = [',', ';', '\t', '|'];

pub fn delimiter_name(delimiter: char) -> String {
    match delimiter {
        '\t' => "Tab".to_string(),
        other => other.to_string(),
    }
}

/// The delimiter a sample of text uses: the one that gives the same number of
/// fields on its first lines, and the most.
pub fn detect_delimiter(sample: &str) -> char {
    let lines: Vec<&str> = sample.lines().filter(|l| !l.is_empty()).take(5).collect();
    let mut best = (',', 0usize);
    for delimiter in DELIMITERS {
        let counts: Vec<usize> = lines
            .iter()
            .map(|l| parse_csv(l, delimiter).first().map_or(0, Vec::len))
            .collect();
        let first = counts.first().copied().unwrap_or(0);
        if first > 1 && counts.iter().all(|c| *c == first) && first > best.1 {
            best = (delimiter, first);
        }
    }
    best.0
}

/// The records of a CSV: a field in quotes may hold the delimiter, a line
/// break and a doubled quote.
pub fn parse_csv(text: &str, delimiter: char) -> Vec<Vec<String>> {
    let mut records: Vec<Vec<String>> = Vec::new();
    let mut record: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut touched = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    field.push('"');
                    chars.next();
                } else {
                    quoted = false;
                }
            } else {
                field.push(c);
            }
            continue;
        }
        match c {
            '"' => {
                quoted = true;
                touched = true;
            }
            c if c == delimiter => {
                record.push(std::mem::take(&mut field));
                touched = true;
            }
            '\r' => {}
            '\n' => {
                if touched || !field.is_empty() || !record.is_empty() {
                    record.push(std::mem::take(&mut field));
                    records.push(std::mem::take(&mut record));
                }
                touched = false;
            }
            c => {
                field.push(c);
                touched = true;
            }
        }
    }
    if touched || !field.is_empty() || !record.is_empty() {
        record.push(field);
        records.push(record);
    }
    records
}

/// What is read of a file: the names of its fields, and its first lines.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Sample {
    pub names: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

/// A CSV: with a header, the first line names the fields; without, they are
/// `1`, `2`, ….
pub fn csv_sample(text: &str, delimiter: char, header: bool) -> Sample {
    let mut records = parse_csv(text, delimiter);
    let width = records.iter().map(Vec::len).max().unwrap_or(0);
    let names = if header && !records.is_empty() {
        let first = records.remove(0);
        (0..width)
            .map(|i| match first.get(i) {
                Some(n) if !n.trim().is_empty() => n.trim().to_string(),
                _ => format!("{}", i + 1),
            })
            .collect()
    } else {
        (1..=width).map(|i| i.to_string()).collect()
    };
    records.truncate(PREVIEW_ROWS);
    Sample {
        names,
        rows: records,
    }
}

/// A JSON array of objects (or one object): the keys, in the order they come,
/// and the first objects as lines.
pub fn json_sample(text: &str) -> Result<Sample, String> {
    let value: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let objects: Vec<&serde_json::Map<String, serde_json::Value>> = match &value {
        serde_json::Value::Array(items) => items
            .iter()
            .map(|v| {
                v.as_object()
                    .ok_or_else(|| "not an array of objects".to_string())
            })
            .collect::<Result<_, _>>()?,
        serde_json::Value::Object(map) => vec![map],
        _ => return Err("not an array of objects".to_string()),
    };
    let mut names: Vec<String> = Vec::new();
    for object in &objects {
        for key in object.keys() {
            if !names.contains(key) {
                names.push(key.clone());
            }
        }
    }
    let show = |v: Option<&serde_json::Value>| match v {
        None | Some(serde_json::Value::Null) => String::new(),
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    };
    let rows = objects
        .iter()
        .take(PREVIEW_ROWS)
        .map(|o| names.iter().map(|n| show(o.get(n))).collect())
        .collect();
    Ok(Sample { names, rows })
}

/// A column of the table that a file can fill.
#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    pub name: String,
    pub kind: String,
}

/// For each field of the file, the column it goes to: the one of the same
/// name (whatever the case, `_` or `-` or a space), and no column twice. A
/// file without names goes by place.
pub fn auto_map(names: &[String], named: bool, targets: &[Target]) -> Vec<Option<String>> {
    let plain = |s: &str| {
        s.chars()
            .filter(|c| c.is_alphanumeric())
            .collect::<String>()
            .to_lowercase()
    };
    let mut taken: Vec<&str> = Vec::new();
    names
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let found = if named {
                targets
                    .iter()
                    .find(|t| plain(&t.name) == plain(name) && !taken.contains(&t.name.as_str()))
            } else {
                targets.get(i)
            };
            found.map(|t| {
                taken.push(t.name.as_str());
                t.name.clone()
            })
        })
        .collect()
}

/// What the driver is asked: the fields that go somewhere, as the column's
/// type wants them.
pub fn spec(
    format: Format,
    delimiter: char,
    header: bool,
    empty_null: bool,
    names: &[String],
    mapping: &[Option<String>],
    targets: &[Target],
) -> ImportSpec {
    let columns = names
        .iter()
        .zip(mapping)
        .enumerate()
        .filter_map(|(index, (name, to))| {
            let target = targets.iter().find(|t| Some(&t.name) == to.as_ref())?;
            Some(ImportColumn {
                index,
                key: name.clone(),
                column: target.name.clone(),
                kind: target.kind.clone(),
            })
        })
        .collect();
    ImportSpec {
        format: if format == Format::Json {
            "json"
        } else {
            "csv"
        }
        .to_string(),
        delimiter: delimiter.to_string(),
        header,
        file_columns: names.len(),
        empty_null,
        mapping: columns,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_csv_is_cut_into_records_and_fields() {
        assert_eq!(
            parse_csv("a,b\n1,2\n", ','),
            vec![vec!["a", "b"], vec!["1", "2"]]
        );
        // Quotes hold the delimiter, a line break and a quote.
        assert_eq!(
            parse_csv("x;\"a;b\";\"l1\nl2\";\"say \"\"hi\"\"\"\r\n", ';'),
            vec![vec!["x", "a;b", "l1\nl2", "say \"hi\""]]
        );
        // An empty field is a field; a blank line is not a record.
        assert_eq!(
            parse_csv("a,,c\n\n,\n", ','),
            vec![vec!["a", "", "c"], vec!["", ""]]
        );
        // No line break at the end.
        assert_eq!(parse_csv("a,b", ','), vec![vec!["a", "b"]]);
        assert!(parse_csv("", ',').is_empty());
    }

    #[test]
    fn the_delimiter_is_the_one_every_line_agrees_on() {
        assert_eq!(detect_delimiter("a;b;c\n1;2;3\n"), ';');
        assert_eq!(detect_delimiter("a\tb\n1\t2\n"), '\t');
        assert_eq!(detect_delimiter("a,b\n1,2\n"), ',');
        // A comma inside quotes does not count.
        assert_eq!(detect_delimiter("a;\"x,y\"\n1;2\n"), ';');
        // Nothing to go by: the comma.
        assert_eq!(detect_delimiter("single\nlines\n"), ',');
    }

    #[test]
    fn with_a_header_the_first_line_names_the_fields() {
        let s = csv_sample("id,name,\n1,Ann,x\n2,Bob\n", ',', true);
        assert_eq!(s.names, vec!["id", "name", "3"]);
        assert_eq!(s.rows.len(), 2);
        let s = csv_sample("1,Ann\n2,Bob\n", ',', false);
        assert_eq!(s.names, vec!["1", "2"]);
        assert_eq!(s.rows.len(), 2);
        // The preview is cut.
        let many: String = (0..20).map(|i| format!("{i},x\n")).collect();
        assert_eq!(csv_sample(&many, ',', false).rows.len(), PREVIEW_ROWS);
    }

    #[test]
    fn a_json_array_gives_the_keys_in_order_of_appearance() {
        let s =
            json_sample(r#"[{"id":1,"name":"Ann"},{"id":2,"extra":{"a":1},"name":null}]"#).unwrap();
        assert_eq!(s.names, vec!["id", "name", "extra"]);
        assert_eq!(s.rows[0], vec!["1", "Ann", ""]);
        assert_eq!(s.rows[1], vec!["2", "", "{\"a\":1}"]);
        assert_eq!(json_sample(r#"{"a":1}"#).unwrap().names, vec!["a"]);
        assert!(json_sample("[1,2]").is_err());
        assert!(json_sample("nope").is_err());
        assert!(json_sample("\"text\"").is_err());
    }

    fn targets() -> Vec<Target> {
        ["id", "first_name", "born"]
            .iter()
            .map(|n| Target {
                name: n.to_string(),
                kind: "text".into(),
            })
            .collect()
    }

    #[test]
    fn fields_go_to_the_column_of_their_name() {
        let names: Vec<String> = ["ID", "First Name", "unknown", "born"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(
            auto_map(&names, true, &targets()),
            vec![
                Some("id".to_string()),
                Some("first_name".to_string()),
                None,
                Some("born".to_string())
            ]
        );
        // A column is filled once.
        let twice: Vec<String> = vec!["id".into(), "id".into()];
        assert_eq!(
            auto_map(&twice, true, &targets()),
            vec![Some("id".to_string()), None]
        );
        // Without names, by place.
        let by_place = auto_map(&["1".into(), "2".into()], false, &targets());
        assert_eq!(
            by_place,
            vec![Some("id".to_string()), Some("first_name".to_string())]
        );
    }

    #[test]
    fn the_request_keeps_the_fields_that_go_somewhere_with_their_type() {
        let mut t = targets();
        t[0].kind = "integer".into();
        let names: Vec<String> = vec!["a".into(), "b".into(), "c".into()];
        let mapping = vec![Some("id".to_string()), None, Some("born".to_string())];
        let s = spec(Format::Csv, ';', true, true, &names, &mapping, &t);
        assert_eq!(
            (s.format.as_str(), s.delimiter.as_str(), s.file_columns),
            ("csv", ";", 3)
        );
        assert_eq!(s.mapping.len(), 2);
        assert_eq!(
            (
                s.mapping[0].index,
                s.mapping[0].column.as_str(),
                s.mapping[0].kind.as_str()
            ),
            (0, "id", "integer")
        );
        assert_eq!((s.mapping[1].index, s.mapping[1].key.as_str()), (2, "c"));
    }

    #[test]
    fn the_format_follows_the_name_of_the_file() {
        assert_eq!(Format::of("rows.JSON"), Format::Json);
        assert_eq!(Format::of("rows.csv"), Format::Csv);
        assert_eq!(Format::of("rows.tsv"), Format::Csv);
    }
}
