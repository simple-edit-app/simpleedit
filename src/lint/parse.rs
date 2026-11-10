//! The reports of the linters, turned into diagnostics.

use super::{Diagnostic, Edit, Fix, Severity};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// The text a report talks about, to turn the columns the tools count in
/// (characters, UTF-16 units) into bytes.
pub struct Doc<'a> {
    text: &'a str,
    starts: Vec<usize>,
}

/// What a column of a report counts.
#[derive(Clone, Copy)]
pub enum Unit {
    Bytes,
    Chars,
    Utf16,
}

impl<'a> Doc<'a> {
    pub fn new(text: &'a str) -> Self {
        let mut starts = vec![0];
        starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
        Doc { text, starts }
    }

    fn line_text(&self, line: usize) -> &'a str {
        let Some(&start) = self.starts.get(line) else {
            return "";
        };
        let end = self.starts.get(line + 1).map_or(self.text.len(), |e| e - 1);
        &self.text[start..end.max(start)]
    }

    /// The byte column of `col` (counted from 0 in `unit`) on `line` (from 0).
    pub fn col(&self, line: usize, col: usize, unit: Unit) -> usize {
        let text = self.line_text(line);
        match unit {
            Unit::Bytes => col.min(text.len()),
            Unit::Chars => text.char_indices().nth(col).map_or(text.len(), |(i, _)| i),
            Unit::Utf16 => {
                let mut units = 0;
                for (i, c) in text.char_indices() {
                    if units >= col {
                        return i;
                    }
                    units += c.len_utf16();
                }
                text.len()
            }
        }
    }

    /// The (line, byte column) of a position of a report: `line` and `col`
    /// count from 1.
    pub fn at(&self, line: usize, col: usize, unit: Unit) -> (usize, usize) {
        let line = line.saturating_sub(1).min(self.starts.len() - 1);
        (line, self.col(line, col.saturating_sub(1), unit))
    }

    /// The (line, byte column) of a byte offset.
    pub fn at_offset(&self, offset: usize) -> (usize, usize) {
        super::position_of(self.text, offset)
    }

    /// The byte offset of a UTF-16 offset (what JavaScript tools give).
    pub fn utf16_offset(&self, offset: usize) -> usize {
        let mut units = 0;
        for (i, c) in self.text.char_indices() {
            if units >= offset {
                return i;
            }
            units += c.len_utf16();
        }
        self.text.len()
    }

    /// The end of the word that starts at (`line`, `col`): where a problem
    /// with no end is underlined to.
    pub fn word_end(&self, line: usize, col: usize) -> usize {
        let text = self.line_text(line);
        let rest = text.get(col..).unwrap_or("");
        let mut len = 0;
        for c in rest.chars() {
            if !(c.is_alphanumeric() || c == '_') {
                break;
            }
            len += c.len_utf8();
        }
        col + len.max(rest.chars().next().map_or(0, char::len_utf8))
    }
}

fn diagnostic(
    source: &str,
    start: (usize, usize),
    end: Option<(usize, usize)>,
    severity: Severity,
    message: &str,
    code: Option<String>,
    doc: &Doc,
) -> Diagnostic {
    let end = end
        .filter(|e| *e > start)
        .unwrap_or((start.0, doc.word_end(start.0, start.1)));
    Diagnostic {
        line: start.0,
        col: start.1,
        end_line: end.0,
        end_col: end.1,
        severity,
        message: message.trim().to_string(),
        code: code.filter(|c| !c.is_empty()),
        source: source.to_string(),
        fixes: Vec::new(),
        raw: None,
    }
}

fn num(value: &Value, key: &str) -> Option<usize> {
    value.get(key).and_then(Value::as_u64).map(|n| n as usize)
}

fn string(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_string)
}

/// ESLint, `-f json`: columns are UTF-16 units from 1, fixes are ranges of
/// UTF-16 offsets.
pub fn eslint(out: &str, text: &str, source: &str) -> Option<Vec<Diagnostic>> {
    let doc = Doc::new(text);
    let files: Value = serde_json::from_str(out).ok()?;
    let mut found = Vec::new();
    for message in files.get(0)?.get("messages")?.as_array()? {
        let rule = string(message, "ruleId");
        let text_of = string(message, "message").unwrap_or_default();
        // A file the configuration leaves out is not a problem of the file.
        if rule.is_none() && text_of.starts_with("File ignored") {
            continue;
        }
        let start = doc.at(
            num(message, "line").unwrap_or(1),
            num(message, "column").unwrap_or(1),
            Unit::Utf16,
        );
        let end = num(message, "endLine")
            .zip(num(message, "endColumn"))
            .map(|(l, c)| doc.at(l, c, Unit::Utf16));
        let severity = if num(message, "severity") == Some(2) {
            Severity::Error
        } else {
            Severity::Warning
        };
        let mut d = diagnostic(source, start, end, severity, &text_of, rule.clone(), &doc);
        let fix_of = |fix: &Value, title: String| -> Option<Fix> {
            let range = fix.get("range")?.as_array()?;
            let (a, b) = (
                range.first()?.as_u64()? as usize,
                range.get(1)?.as_u64()? as usize,
            );
            let (a, b) = (doc.utf16_offset(a), doc.utf16_offset(b));
            let (from, to) = (doc.at_offset(a), doc.at_offset(b));
            Some(Fix {
                title,
                safe: true,
                edits: vec![Edit {
                    line: from.0,
                    col: from.1,
                    end_line: to.0,
                    end_col: to.1,
                    text: string(fix, "text").unwrap_or_default(),
                }],
            })
        };
        if let Some(fix) = message.get("fix") {
            let title = rule
                .as_deref()
                .map_or_else(|| "ESLint".to_string(), |r| format!("Fix '{r}'"));
            d.fixes.extend(fix_of(fix, title));
        }
        for suggestion in message
            .get("suggestions")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let title = string(suggestion, "desc").unwrap_or_default();
            if let Some(fix) = suggestion.get("fix") {
                d.fixes.extend(fix_of(fix, title));
            }
        }
        found.push(d);
    }
    Some(found)
}

/// ruff, `--output-format json`: rows from 1, columns in characters.
pub fn ruff(out: &str, text: &str, source: &str) -> Option<Vec<Diagnostic>> {
    let doc = Doc::new(text);
    let items: Value = serde_json::from_str(out).ok()?;
    let at = |v: &Value| {
        doc.at(
            num(v, "row").unwrap_or(1),
            num(v, "column").unwrap_or(1),
            Unit::Chars,
        )
    };
    let mut found = Vec::new();
    for item in items.as_array()? {
        let code = string(item, "code");
        let start = at(item.get("location")?);
        let end = item.get("end_location").map(at);
        let severity = match code.as_deref() {
            Some(c) if c.starts_with("E9") || c == "F821" || c.starts_with("F63") => {
                Severity::Error
            }
            None => Severity::Error,
            _ => Severity::Warning,
        };
        let mut d = diagnostic(
            source,
            start,
            end,
            severity,
            &string(item, "message").unwrap_or_default(),
            code.clone(),
            &doc,
        );
        if let Some(fix) = item.get("fix").filter(|f| !f.is_null()) {
            let edits: Vec<Edit> = fix
                .get("edits")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|e| {
                    let (from, to) = (at(e.get("location")?), at(e.get("end_location")?));
                    Some(Edit {
                        line: from.0,
                        col: from.1,
                        end_line: to.0,
                        end_col: to.1,
                        text: string(e, "content").unwrap_or_default(),
                    })
                })
                .collect();
            let title = string(fix, "message")
                .or_else(|| code.as_ref().map(|c| format!("Fix {c}")))
                .unwrap_or_default();
            let safe = fix.get("applicability").and_then(Value::as_str) != Some("unsafe");
            if !edits.is_empty() {
                d.fixes.push(Fix { title, edits, safe });
            }
        }
        found.push(d);
    }
    Some(found)
}

/// The `file:line:col: message` of many tools (gcc style), with `severity`
/// found by `level` in the text that follows the position.
fn located(out: &str, text: &str, source: &str, pattern: &regex::Regex) -> Vec<Diagnostic> {
    let doc = Doc::new(text);
    out.lines()
        .filter_map(|line| {
            let c = pattern.captures(line)?;
            let row: usize = c.name("line")?.as_str().parse().ok()?;
            let col: usize = c
                .name("col")
                .and_then(|m| m.as_str().parse().ok())
                .unwrap_or(1);
            let level = c.name("level").map_or("error", |m| m.as_str());
            let severity = severity_of(level);
            let start = doc.at(row, col, Unit::Chars);
            let end = c
                .name("endcol")
                .and_then(|m| m.as_str().parse::<usize>().ok())
                .map(|e| doc.at(row, e + 1, Unit::Chars));
            Some(diagnostic(
                source,
                start,
                end,
                severity,
                c.name("msg")?.as_str(),
                c.name("code").map(|m| m.as_str().to_string()),
                &doc,
            ))
        })
        .collect()
}

fn severity_of(level: &str) -> Severity {
    let level = level.to_ascii_lowercase();
    // `parser error`, `validity warning`…
    let level = level.rsplit(' ').next().unwrap_or("");
    match level {
        "error" | "e" | "fatal" | "err" | "high" | "critical" => Severity::Error,
        "warning" | "warn" | "w" | "medium" | "style" | "performance" | "portability" => {
            Severity::Warning
        }
        "info" | "information" | "note" | "convention" | "refactor" | "i" | "c" | "r" => {
            Severity::Info
        }
        _ => Severity::Hint,
    }
}

fn regex(pattern: &str) -> regex::Regex {
    regex::Regex::new(pattern).expect("a valid pattern")
}

/// flake8 with `--format=%(row)d:%(col)d:%(code)s:%(text)s`.
pub fn flake8(out: &str, text: &str, source: &str) -> Vec<Diagnostic> {
    let doc = Doc::new(text);
    out.lines()
        .filter_map(|line| {
            let mut parts = line.splitn(4, ':');
            let (row, col, code, msg) = (
                parts.next()?.trim().parse::<usize>().ok()?,
                parts.next()?.trim().parse::<usize>().ok()?,
                parts.next()?.trim(),
                parts.next()?,
            );
            // Syntax errors and undefined names are errors, style is not.
            let severity = if code.starts_with("E9") || code.starts_with("F82") {
                Severity::Error
            } else {
                Severity::Warning
            };
            Some(diagnostic(
                source,
                doc.at(row, col, Unit::Chars),
                None,
                severity,
                msg,
                Some(code.to_string()),
                &doc,
            ))
        })
        .collect()
}

/// pylint, `--output-format=json`.
pub fn pylint(out: &str, text: &str, source: &str) -> Option<Vec<Diagnostic>> {
    let doc = Doc::new(text);
    let items: Value = serde_json::from_str(out).ok()?;
    let found = items
        .as_array()?
        .iter()
        .filter_map(|item| {
            let row = num(item, "line")?;
            let col = num(item, "column").unwrap_or(0) + 1;
            let start = doc.at(row, col, Unit::Chars);
            let end = num(item, "endLine")
                .zip(num(item, "endColumn"))
                .map(|(l, c)| doc.at(l, c + 1, Unit::Chars));
            let severity = match item.get("type").and_then(Value::as_str) {
                Some("error" | "fatal") => Severity::Error,
                Some("warning") => Severity::Warning,
                _ => Severity::Info,
            };
            let code = string(item, "symbol").or_else(|| string(item, "message-id"));
            Some(diagnostic(
                source,
                start,
                end,
                severity,
                &string(item, "message")?,
                code,
                &doc,
            ))
        })
        .collect();
    Some(found)
}

/// Whether a path a tool printed (relative to where it ran, or absolute) is
/// `path`, compared by path components, not by characters.
fn same_file(path: &Path, printed: &str) -> bool {
    let printed = Path::new(printed);
    if printed.is_absolute() {
        path == printed
    } else {
        path.ends_with(printed.strip_prefix(".").unwrap_or(printed))
    }
}

/// mypy, `--show-column-numbers`: `file:line:col: error: message  [code]`.
pub fn mypy(out: &str, text: &str, source: &str, path: &Path) -> Vec<Diagnostic> {
    // Only the lines about this file: mypy follows imports and reports on
    // the other files too.
    let file = regex(r"^(?P<file>[^:]+):\d+:");
    let mine: String = out
        .lines()
        .filter(|l| {
            file.captures(l)
                .is_some_and(|c| same_file(path, c["file"].trim()))
        })
        .fold(String::new(), |acc, l| acc + l + "\n");
    let out = mine.as_str();
    let pattern = regex(
        r"^[^:]+:(?P<line>\d+):(?:(?P<col>\d+):)? (?P<level>error|warning|note): (?P<msg>.*?)(?:  \[(?P<code>[\w-]+)\])?$",
    );
    located(out, text, source, &pattern)
}

/// ShellCheck, `-f json1`: columns in characters from 1; fixes are
/// replacements of ranges.
pub fn shellcheck(out: &str, text: &str, source: &str) -> Option<Vec<Diagnostic>> {
    let doc = Doc::new(text);
    let value: Value = serde_json::from_str(out).ok()?;
    let items = value.get("comments").unwrap_or(&value).as_array()?;
    let mut found = Vec::new();
    for item in items {
        let at = |l: &str, c: &str| {
            doc.at(
                num(item, l).unwrap_or(1),
                num(item, c).unwrap_or(1),
                Unit::Chars,
            )
        };
        let severity = match item.get("level").and_then(Value::as_str) {
            Some("error") => Severity::Error,
            Some("warning") => Severity::Warning,
            _ => Severity::Info,
        };
        let code = num(item, "code").map(|c| format!("SC{c}"));
        let mut d = diagnostic(
            source,
            at("line", "column"),
            Some(at("endLine", "endColumn")),
            severity,
            &string(item, "message").unwrap_or_default(),
            code.clone(),
            &doc,
        );
        if let Some(reps) = item
            .get("fix")
            .and_then(|f| f.get("replacements"))
            .and_then(Value::as_array)
        {
            let edits: Vec<Edit> = reps
                .iter()
                .map(|r| {
                    let at = |l: &str, c: &str| {
                        doc.at(num(r, l).unwrap_or(1), num(r, c).unwrap_or(1), Unit::Chars)
                    };
                    let (from, to) = (at("line", "column"), at("endLine", "endColumn"));
                    Edit {
                        line: from.0,
                        col: from.1,
                        end_line: to.0,
                        end_col: to.1,
                        text: string(r, "replacement").unwrap_or_default(),
                    }
                })
                .collect();
            if !edits.is_empty() {
                d.fixes.push(Fix {
                    title: code.map_or_else(|| "ShellCheck".into(), |c| format!("Fix {c}")),
                    edits,
                    safe: true,
                });
            }
        }
        found.push(d);
    }
    Some(found)
}

/// stylelint, `--formatter json`.
pub fn stylelint(out: &str, text: &str, source: &str) -> Option<Vec<Diagnostic>> {
    let doc = Doc::new(text);
    let files: Value = serde_json::from_str(out).ok()?;
    let found = files
        .get(0)?
        .get("warnings")?
        .as_array()?
        .iter()
        .filter_map(|w| {
            let start = doc.at(num(w, "line")?, num(w, "column").unwrap_or(1), Unit::Chars);
            let end = num(w, "endLine")
                .zip(num(w, "endColumn"))
                .map(|(l, c)| doc.at(l, c, Unit::Chars));
            let severity = if w.get("severity").and_then(Value::as_str) == Some("error") {
                Severity::Error
            } else {
                Severity::Warning
            };
            let message = string(w, "text")?;
            // The rule's name is at the end of the text, in brackets.
            let rule = string(w, "rule");
            let message = rule
                .as_deref()
                .and_then(|r| message.strip_suffix(&format!(" ({r})")))
                .unwrap_or(&message)
                .to_string();
            Some(diagnostic(
                source, start, end, severity, &message, rule, &doc,
            ))
        })
        .collect();
    Some(found)
}

/// RuboCop, `--format json`.
pub fn rubocop(out: &str, text: &str, source: &str) -> Option<Vec<Diagnostic>> {
    let doc = Doc::new(text);
    let report: Value = serde_json::from_str(out).ok()?;
    let found = report
        .get("files")?
        .get(0)?
        .get("offenses")?
        .as_array()?
        .iter()
        .filter_map(|o| {
            let loc = o.get("location")?;
            let start = doc.at(
                num(loc, "start_line")?,
                num(loc, "start_column").unwrap_or(1),
                Unit::Chars,
            );
            let end = num(loc, "last_line")
                .zip(num(loc, "last_column"))
                .map(|(l, c)| doc.at(l, c + 1, Unit::Chars));
            let severity = match o.get("severity").and_then(Value::as_str) {
                Some("error" | "fatal") => Severity::Error,
                Some("warning") => Severity::Warning,
                _ => Severity::Info,
            };
            Some(diagnostic(
                source,
                start,
                end,
                severity,
                &string(o, "message")?,
                string(o, "cop_name"),
                &doc,
            ))
        })
        .collect();
    Some(found)
}

/// yamllint, `-f parsable`: `file:line:col: [level] message (rule)`.
pub fn yamllint(out: &str, text: &str, source: &str) -> Vec<Diagnostic> {
    let pattern = regex(
        r"^[^:]*:(?P<line>\d+):(?P<col>\d+): \[(?P<level>\w+)\] (?P<msg>.*?)(?: \((?P<code>[\w-]+)\))?$",
    );
    located(out, text, source, &pattern)
}

/// markdownlint: `file:line[:col] MD001/rule-name message [Context: …]`.
pub fn markdownlint(out: &str, text: &str, source: &str) -> Vec<Diagnostic> {
    let pattern = regex(
        r"^[^:]+:(?P<line>\d+)(?::(?P<col>\d+))? (?P<code>MD\d+(?:/[\w-]+)?) (?P<msg>.*?)(?: \[(?:Context|Expected)[^\]]*\])?$",
    );
    located(out, text, source, &pattern)
        .into_iter()
        .map(|mut d| {
            d.severity = Severity::Warning;
            d
        })
        .collect()
}

/// cppcheck with `--template={line}:{column}:{severity}:{id}:{message}`.
pub fn cppcheck(out: &str, text: &str, source: &str) -> Vec<Diagnostic> {
    let pattern =
        regex(r"^(?P<line>\d+):(?P<col>\d+):(?P<level>\w+):(?P<code>[\w.]+):(?P<msg>.*)$");
    located(out, text, source, &pattern)
}

/// `go vet`: `./file.go:3:5: message` (lines starting with `#` are headers).
pub fn govet(out: &str, text: &str, source: &str, file_name: &str) -> Vec<Diagnostic> {
    let pattern = regex(&format!(
        r"^(?:\./)?{}:(?P<line>\d+):(?:(?P<col>\d+):)? (?P<msg>.*)$",
        regex::escape(file_name)
    ));
    located(out, text, source, &pattern)
        .into_iter()
        .map(|mut d| {
            d.severity = Severity::Warning;
            d
        })
        .collect()
}

/// golangci-lint, `--out-format json`.
pub fn golangci(out: &str, text: &str, source: &str, file_name: &str) -> Option<Vec<Diagnostic>> {
    let doc = Doc::new(text);
    let report: Value = serde_json::from_str(out).ok()?;
    let found = report
        .get("Issues")?
        .as_array()?
        .iter()
        .filter_map(|i| {
            let pos = i.get("Pos")?;
            let name = string(pos, "Filename")?;
            if !name.ends_with(file_name) {
                return None;
            }
            let start = doc.at(
                num(pos, "Line")?,
                num(pos, "Column").unwrap_or(1),
                Unit::Chars,
            );
            let severity = match i.get("Severity").and_then(Value::as_str) {
                Some("error") => Severity::Error,
                _ => Severity::Warning,
            };
            Some(diagnostic(
                source,
                start,
                None,
                severity,
                &string(i, "Text")?,
                string(i, "FromLinter"),
                &doc,
            ))
        })
        .collect();
    Some(found)
}

/// `php -l`: `PHP Parse error: syntax error, … in Standard input code on line 3`.
pub fn php(out: &str, text: &str, source: &str) -> Vec<Diagnostic> {
    let pattern = regex(
        r"^(?:PHP )?(?P<level>Parse error|Fatal error|Warning|Deprecated):\s+(?P<msg>.*?) in .* on line (?P<line>\d+)$",
    );
    located(out, text, source, &pattern)
        .into_iter()
        .map(|mut d| {
            d.severity = if d.message.is_empty() {
                Severity::Hint
            } else {
                Severity::Error
            };
            d
        })
        .collect()
}

/// luacheck, `--formatter plain --codes --ranges`: `stdin:3:5-8: (W113) message`.
pub fn luacheck(out: &str, text: &str, source: &str) -> Vec<Diagnostic> {
    let pattern = regex(
        r"^[^:]+:(?P<line>\d+):(?P<col>\d+)(?:-(?P<endcol>\d+))?: \((?P<level>[EW])(?P<code>\d+)\) (?P<msg>.*)$",
    );
    located(out, text, source, &pattern)
}

/// ktlint, `--reporter=json`.
pub fn ktlint(out: &str, text: &str, source: &str) -> Option<Vec<Diagnostic>> {
    let doc = Doc::new(text);
    let files: Value = serde_json::from_str(out).ok()?;
    let found = files
        .get(0)?
        .get("errors")?
        .as_array()?
        .iter()
        .filter_map(|e| {
            let start = doc.at(num(e, "line")?, num(e, "column").unwrap_or(1), Unit::Chars);
            Some(diagnostic(
                source,
                start,
                None,
                Severity::Warning,
                &string(e, "message")?,
                string(e, "rule"),
                &doc,
            ))
        })
        .collect();
    Some(found)
}

/// SwiftLint, `--reporter json`.
pub fn swiftlint(out: &str, text: &str, source: &str) -> Option<Vec<Diagnostic>> {
    let doc = Doc::new(text);
    let items: Value = serde_json::from_str(out).ok()?;
    let found = items
        .as_array()?
        .iter()
        .filter_map(|i| {
            let start = doc.at(
                num(i, "line")?,
                num(i, "character").unwrap_or(1),
                Unit::Chars,
            );
            let severity = if i.get("severity").and_then(Value::as_str) == Some("Error") {
                Severity::Error
            } else {
                Severity::Warning
            };
            Some(diagnostic(
                source,
                start,
                None,
                severity,
                &string(i, "reason")?,
                string(i, "rule_id"),
                &doc,
            ))
        })
        .collect();
    Some(found)
}

/// sqlfluff, `--format json`.
pub fn sqlfluff(out: &str, text: &str, source: &str) -> Option<Vec<Diagnostic>> {
    let doc = Doc::new(text);
    let files: Value = serde_json::from_str(out).ok()?;
    let found = files
        .get(0)?
        .get("violations")?
        .as_array()?
        .iter()
        .filter_map(|v| {
            let start = doc.at(
                num(v, "start_line_no").or_else(|| num(v, "line_no"))?,
                num(v, "start_line_pos")
                    .or_else(|| num(v, "line_pos"))
                    .unwrap_or(1),
                Unit::Chars,
            );
            let end = num(v, "end_line_no")
                .zip(num(v, "end_line_pos"))
                .map(|(l, c)| doc.at(l, c, Unit::Chars));
            Some(diagnostic(
                source,
                start,
                end,
                Severity::Warning,
                &string(v, "description")?,
                string(v, "code"),
                &doc,
            ))
        })
        .collect();
    Some(found)
}

/// hlint, `--json`.
pub fn hlint(out: &str, text: &str, source: &str) -> Option<Vec<Diagnostic>> {
    let doc = Doc::new(text);
    let items: Value = serde_json::from_str(out).ok()?;
    let found = items
        .as_array()?
        .iter()
        .filter_map(|i| {
            let start = doc.at(
                num(i, "startLine")?,
                num(i, "startColumn").unwrap_or(1),
                Unit::Chars,
            );
            let end = num(i, "endLine")
                .zip(num(i, "endColumn"))
                .map(|(l, c)| doc.at(l, c, Unit::Chars));
            let severity = match i.get("severity").and_then(Value::as_str) {
                Some("Error") => Severity::Error,
                Some("Warning") => Severity::Warning,
                _ => Severity::Info,
            };
            let message = match (string(i, "from"), string(i, "to")) {
                (Some(from), Some(to)) => format!("{}: {from} → {to}", string(i, "hint")?),
                _ => string(i, "hint")?,
            };
            Some(diagnostic(
                source, start, end, severity, &message, None, &doc,
            ))
        })
        .collect();
    Some(found)
}

/// xmllint: `-:3: parser error : message`.
pub fn xmllint(out: &str, text: &str, source: &str) -> Vec<Diagnostic> {
    let pattern = regex(r"^[^:]*:(?P<line>\d+): (?P<level>[\w ]+?) ?: (?P<msg>.*)$");
    located(out, text, source, &pattern)
}

/// JSON read by the program itself.
pub fn json(text: &str, source: &str) -> Vec<Diagnostic> {
    let doc = Doc::new(text);
    match serde_json::from_str::<Value>(text) {
        Ok(_) => Vec::new(),
        Err(e) => {
            // The parser counts columns in bytes from 1.
            let line = e.line().max(1);
            let start = doc.at(line, e.column().max(1), Unit::Bytes);
            // A problem at the very end is shown on the last character.
            let start = if start.1 > 0 && doc.line_text(start.0).len() == start.1 {
                (
                    start.0,
                    doc.line_text(start.0)
                        .char_indices()
                        .last()
                        .map_or(0, |(i, _)| i),
                )
            } else {
                start
            };
            let message = e.to_string();
            let message = message
                .rsplit_once(" at line ")
                .map_or(message.as_str(), |(m, _)| m);
            vec![diagnostic(
                source,
                start,
                None,
                Severity::Error,
                message,
                None,
                &doc,
            )]
        }
    }
}

/// `cargo clippy --message-format json`: the compiler messages of `file`,
/// with the suggestions that are safe to apply as fixes (byte offsets).
pub fn clippy(out: &str, text: &str, source: &str, path: &Path) -> Vec<Diagnostic> {
    let doc = Doc::new(text);
    let mut found = Vec::new();
    for line in out.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if value.get("reason").and_then(Value::as_str) != Some("compiler-message") {
            continue;
        }
        let Some(message) = value.get("message") else {
            continue;
        };
        let level = message.get("level").and_then(Value::as_str).unwrap_or("");
        if !matches!(level, "error" | "warning" | "note" | "help") {
            continue;
        }
        // Paths are relative to the workspace root, which is the folder of
        // the package or one above it: `src/lib.rs` of another package of
        // the workspace is not this file.
        let manifest = string(&value, "manifest_path").map(PathBuf::from);
        let mine = |f: &str| clippy_file(path, manifest.as_deref(), f);
        let spans = message.get("spans").and_then(Value::as_array);
        let Some(primary) = spans.and_then(|s| {
            s.iter().find(|s| {
                s.get("is_primary").and_then(Value::as_bool) == Some(true)
                    && string(s, "file_name").is_some_and(|f| mine(&f))
            })
        }) else {
            continue;
        };
        let start = doc.at(
            num(primary, "line_start").unwrap_or(1),
            num(primary, "column_start").unwrap_or(1),
            Unit::Chars,
        );
        let end = doc.at(
            num(primary, "line_end").unwrap_or(1),
            num(primary, "column_end").unwrap_or(1),
            Unit::Chars,
        );
        let code = message.get("code").and_then(|c| string(c, "code"));
        let mut d = diagnostic(
            source,
            start,
            Some(end),
            severity_of(level),
            &string(message, "message").unwrap_or_default(),
            code,
            &doc,
        );
        let mut edits = Vec::new();
        let machine = collect_suggestions(message, &mine, &doc, &mut edits);
        if !edits.is_empty() {
            d.fixes.push(Fix {
                title: "Apply the suggestion".into(),
                edits,
                safe: machine,
            });
        }
        found.push(d);
    }
    found
}

/// Whether the file of a clippy span is `path`.
fn clippy_file(path: &Path, manifest: Option<&Path>, file: &str) -> bool {
    let file = Path::new(file);
    if file.is_absolute() {
        return path == file;
    }
    let Some(package) = manifest.and_then(Path::parent) else {
        return path.ends_with(file);
    };
    package.ancestors().any(|root| root.join(file) == path)
}

/// Gathers the suggestions of a message; true when all of them are certain.
fn collect_suggestions(
    message: &Value,
    mine: &dyn Fn(&str) -> bool,
    doc: &Doc,
    edits: &mut Vec<Edit>,
) -> bool {
    let mut certain = true;
    let children = message.get("children").and_then(Value::as_array);
    let own = message.get("spans").and_then(Value::as_array);
    for span in own
        .into_iter()
        .flatten()
        .chain(children.into_iter().flatten().flat_map(|c| {
            c.get("spans")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        }))
    {
        let Some(replacement) = string(span, "suggested_replacement") else {
            continue;
        };
        if !string(span, "file_name").is_some_and(|f| mine(&f)) {
            continue;
        }
        match span.get("suggestion_applicability").and_then(Value::as_str) {
            Some("MachineApplicable") => {}
            Some("MaybeIncorrect") => certain = false,
            _ => continue,
        }
        let (Some(a), Some(b)) = (num(span, "byte_start"), num(span, "byte_end")) else {
            continue;
        };
        let (from, to) = (doc.at_offset(a), doc.at_offset(b));
        edits.push(Edit {
            line: from.0,
            col: from.1,
            end_line: to.0,
            end_col: to.1,
            text: replacement,
        });
    }
    certain
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "const a = [b,c];\nlet é = 1;\n";

    #[test]
    fn eslint_gives_the_position_the_rule_and_the_fix() {
        let out = r#"[{"filePath":"/x/a.ts","messages":[
          {"ruleId":"comma-spacing","severity":2,"message":"A space is required after ','.","line":1,"column":13,"nodeType":"Punctuator","endLine":1,"endColumn":14,"fix":{"range":[13,13],"text":" "}},
          {"ruleId":null,"severity":1,"message":"File ignored because of a matching ignore pattern."},
          {"ruleId":"no-unused-vars","severity":1,"message":"'é' is assigned a value but never used.","line":2,"column":5,"endLine":2,"endColumn":6}
        ]}]"#;
        let found = eslint(out, TEXT, "ESLint").unwrap();
        assert_eq!(found.len(), 2, "the ignored-file notice is no problem");
        let d = &found[0];
        assert_eq!((d.line, d.col, d.end_col), (0, 12, 13));
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.code.as_deref(), Some("comma-spacing"));
        assert_eq!(d.fixes[0].title, "Fix 'comma-spacing'");
        let (fixed, n) = super::super::apply(TEXT, &[&d.fixes[0]]);
        assert_eq!(n, 1);
        assert_eq!(fixed, "const a = [b, c];\nlet é = 1;\n");
        // Columns after a two-byte character still land on it.
        let e = &found[1];
        assert_eq!((e.line, e.col, e.end_col), (1, 4, 6));
        assert_eq!(e.severity, Severity::Warning);
    }

    #[test]
    fn eslint_offsets_count_utf16_units() {
        // `😀` is two UTF-16 units and four bytes.
        let text = "x = '😀' ,1;\n";
        let out = r#"[{"messages":[{"ruleId":"comma-spacing","severity":1,"message":"m","line":1,"column":9,"fix":{"range":[8,9],"text":""}}]}]"#;
        let found = eslint(out, text, "ESLint").unwrap();
        let (fixed, _) = super::super::apply(text, &[&found[0].fixes[0]]);
        assert_eq!(fixed, "x = '😀',1;\n");
    }

    #[test]
    fn ruff_reports_with_a_fix() {
        let text = "import os\nx = 1\n";
        let out = r#"[{"cell":null,"code":"F401","end_location":{"column":10,"row":1},"filename":"-","fix":{"applicability":"safe","edits":[{"content":"","end_location":{"column":1,"row":2},"location":{"column":1,"row":1}}],"message":"Remove unused import: `os`"},"location":{"column":8,"row":1},"message":"`os` imported but unused","noqa_row":1,"url":"x"}]"#;
        let found = ruff(out, text, "ruff").unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].code.as_deref(), Some("F401"));
        assert_eq!((found[0].line, found[0].col, found[0].end_col), (0, 7, 9));
        assert_eq!(found[0].fixes[0].title, "Remove unused import: `os`");
        let (fixed, _) = super::super::apply(text, &[&found[0].fixes[0]]);
        assert_eq!(fixed, "x = 1\n");
    }

    #[test]
    fn text_reports_are_read_line_by_line() {
        let text = "a\nb\nc\n";
        let f = flake8(
            "2:1:E302:expected 2 blank lines\n3:5:F821:undefined name 'x'\n",
            text,
            "flake8",
        );
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].code.as_deref(), Some("E302"));
        assert_eq!(f[1].severity, Severity::Error);
        let y = yamllint("stdin:2:3: [warning] too many spaces (indentation)\nstdin:3:1: [error] syntax error: found character '\\t' (syntax)\n", text, "yamllint");
        assert_eq!(y[0].severity, Severity::Warning);
        assert_eq!(y[0].code.as_deref(), Some("indentation"));
        assert_eq!(y[1].severity, Severity::Error);
        let m = markdownlint("stdin:2 MD012/no-multiple-blanks Multiple consecutive blank lines [Expected: 1; Actual: 2]\nstdin:3:5 MD009/no-trailing-spaces Trailing spaces\n", text, "markdownlint");
        assert_eq!((m[0].line, m[1].line), (1, 2));
        assert_eq!(m[0].code.as_deref(), Some("MD012/no-multiple-blanks"));
        let l = luacheck(
            "stdin:1:1-1: (W113) accessing undefined variable a\n",
            text,
            "luacheck",
        );
        assert_eq!((l[0].col, l[0].end_col), (0, 1));
        let c = cppcheck(
            "3:7:error:nullPointer:Null pointer dereference\n",
            text,
            "cppcheck",
        );
        assert_eq!((c[0].severity, c[0].line), (Severity::Error, 2));
        let g = govet(
            "# pkg\n./main.go:2:3: unreachable code\n./other.go:1:1: no\n",
            text,
            "go vet",
            "main.go",
        );
        assert_eq!(g.len(), 1);
        let x = xmllint(
            "-:2: parser error : Opening and ending tag mismatch\n",
            text,
            "xmllint",
        );
        assert_eq!((x[0].line, x[0].severity), (1, Severity::Error));
        let p = php(
            "PHP Parse error:  syntax error, unexpected '}' in Standard input code on line 3\n",
            text,
            "php",
        );
        assert_eq!((p[0].line, p[0].severity), (2, Severity::Error));
        let t = mypy(
            "a.py:2:5: error: Incompatible types  [assignment]\na.py:3: note: See docs\n",
            text,
            "mypy",
            Path::new("/p/a.py"),
        );
        assert_eq!(t[0].code.as_deref(), Some("assignment"));
        assert_eq!(t[1].severity, Severity::Info);
        let other = mypy(
            "b.py:10: error: x\n/p/a.py:2: error: y\n",
            text,
            "mypy",
            Path::new("/p/a.py"),
        );
        assert_eq!(other.len(), 1);
    }

    #[test]
    fn json_reports_are_read() {
        let text = "echo $a\nls\n";
        let s = shellcheck(r#"{"comments":[{"file":"-","line":1,"endLine":1,"column":6,"endColumn":8,"level":"info","code":2086,"message":"Double quote to prevent globbing.","fix":{"replacements":[{"line":1,"endLine":1,"precedence":7,"insertionPoint":"afterEnd","column":6,"endColumn":6,"replacement":"\""},{"line":1,"endLine":1,"precedence":7,"insertionPoint":"beforeStart","column":8,"endColumn":8,"replacement":"\""}]}}]}"#, text, "ShellCheck").unwrap();
        assert_eq!(s[0].code.as_deref(), Some("SC2086"));
        let (fixed, _) = super::super::apply(text, &[&s[0].fixes[0]]);
        assert_eq!(fixed, "echo \"$a\"\nls\n");
        let sl = stylelint(r#"[{"source":"a.css","warnings":[{"line":2,"column":3,"endLine":2,"endColumn":8,"rule":"color-no-invalid-hex","severity":"error","text":"Unexpected invalid hex color (color-no-invalid-hex)"}]}]"#, text, "stylelint").unwrap();
        assert_eq!(sl[0].message, "Unexpected invalid hex color");
        assert_eq!(sl[0].severity, Severity::Error);
        let r = rubocop(r#"{"files":[{"path":"a.rb","offenses":[{"severity":"convention","message":"Use single quotes","cop_name":"Style/StringLiterals","corrected":false,"location":{"start_line":2,"start_column":1,"last_line":2,"last_column":2}}]}]}"#, text, "RuboCop").unwrap();
        assert_eq!((r[0].line, r[0].severity), (1, Severity::Info));
        let pl = pylint(r#"[{"type":"warning","line":1,"column":0,"endLine":1,"endColumn":4,"symbol":"unused-import","message":"Unused import os","message-id":"W0611"}]"#, text, "pylint").unwrap();
        assert_eq!(pl[0].end_col, 4);
        let k = ktlint(r#"[{"file":"-","errors":[{"line":1,"column":2,"message":"Unnecessary semicolon","rule":"no-semi"}]}]"#, text, "ktlint").unwrap();
        assert_eq!(k[0].code.as_deref(), Some("no-semi"));
        let sw = swiftlint(r#"[{"line":2,"character":1,"severity":"Warning","reason":"Line too long","rule_id":"line_length"}]"#, text, "SwiftLint").unwrap();
        assert_eq!(sw[0].line, 1);
        let sq = sqlfluff(r#"[{"filepath":"stdin","violations":[{"start_line_no":1,"start_line_pos":1,"end_line_no":1,"end_line_pos":5,"code":"CP01","description":"Keywords must be consistently upper case."}]}]"#, text, "SQLFluff").unwrap();
        assert_eq!(sq[0].end_col, 4);
        let h = hlint(r#"[{"severity":"Suggestion","hint":"Use head","startLine":1,"startColumn":1,"endLine":1,"endColumn":5,"from":"x !! 0","to":"head x"}]"#, text, "hlint").unwrap();
        assert!(h[0].message.contains("→"));
        let gl = golangci(r#"{"Issues":[{"FromLinter":"errcheck","Text":"Error return value is not checked","Severity":"","Pos":{"Filename":"pkg/main.go","Line":2,"Column":3}}]}"#, text, "golangci-lint", "main.go").unwrap();
        assert_eq!(gl[0].code.as_deref(), Some("errcheck"));
    }

    #[test]
    fn json_syntax_errors_are_found_by_the_program() {
        assert!(json("{\"a\": [1, 2]}", "JSON").is_empty());
        let found = json("{\n  \"a\": 1,\n  \"b\": ,\n}\n", "JSON");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].line, 2);
        assert_eq!(found[0].severity, Severity::Error);
        // An unfinished document points at its end, not past it.
        let found = json("{\"a\": ", "JSON");
        assert_eq!(found[0].line, 0);
    }

    #[test]
    fn clippy_suggestions_become_fixes() {
        let text = "fn main() {\n    let x = vec![1].len() == 0;\n}\n";
        let line = r#"{"reason":"compiler-message","message":{"level":"warning","message":"length comparison to zero","code":{"code":"clippy::len_zero"},"spans":[{"file_name":"src/main.rs","byte_start":24,"byte_end":42,"line_start":2,"line_end":2,"column_start":13,"column_end":31,"is_primary":true,"suggested_replacement":null}],"children":[{"level":"help","message":"using `is_empty` is clearer","spans":[{"file_name":"src/main.rs","byte_start":24,"byte_end":42,"line_start":2,"line_end":2,"column_start":13,"column_end":31,"is_primary":true,"suggested_replacement":"vec![1].is_empty()","suggestion_applicability":"MachineApplicable"}],"children":[]}]}}
{"reason":"build-finished","success":true}"#;
        let found = clippy(line, text, "clippy", Path::new("/p/src/main.rs"));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].code.as_deref(), Some("clippy::len_zero"));
        let (fixed, _) = super::super::apply(text, &[&found[0].fixes[0]]);
        assert_eq!(fixed, "fn main() {\n    let x = vec![1].is_empty();\n}\n");
        assert!(clippy(line, text, "clippy", Path::new("/p/src/other.rs")).is_empty());
        // In a workspace, paths are from its root: not the root package's file.
        let member = line.replace(
            "{\"reason\":\"compiler-message\",",
            "{\"reason\":\"compiler-message\",\"manifest_path\":\"/w/crates/b/Cargo.toml\",",
        );
        let member = member.replace("\"src/main.rs\"", "\"crates/b/src/main.rs\"");
        assert_eq!(
            clippy(
                &member,
                text,
                "clippy",
                Path::new("/w/crates/b/src/main.rs")
            )
            .len(),
            1
        );
        let root = line.replace(
            "{\"reason\":\"compiler-message\",",
            "{\"reason\":\"compiler-message\",\"manifest_path\":\"/w/Cargo.toml\",",
        );
        assert!(clippy(&root, text, "clippy", Path::new("/w/crates/b/src/main.rs")).is_empty());
    }
}
