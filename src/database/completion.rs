//! The completion of SQL in the console: keywords and functions, the tables
//! and views, and their columns when the statement says which it is about.

use std::collections::HashMap;

use super::protocol::{sql_tokens, ForeignKey, Kind, Token, TokenKind};

/// What a connection is known to hold, for the completion.
#[derive(Debug, Clone, Default)]
pub struct Known {
    /// Schema, table or view name, and which.
    pub tables: Vec<(String, String, Kind)>,
    /// The columns of each table: `(schema, table)` to name and type.
    pub columns: HashMap<(String, String), Vec<(String, String)>>,
    /// The foreign keys: what a `JOIN` is made on.
    pub foreign_keys: Vec<ForeignKey>,
    /// The functions and procedures of the schemas: schema, name, arguments.
    pub routines: Vec<(String, String, String)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    Keyword,
    Function,
    /// A condition to join two tables on.
    Join,
    Schema,
    Table,
    View,
    Column,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// What the list shows.
    pub label: String,
    /// What replaces the fragment typed.
    pub insert: String,
    /// What the label is about: the table of a column and its type, the schema.
    pub detail: String,
    pub kind: ItemKind,
}

/// A list of proposals for the fragment that ends at the caret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestions {
    /// Where the fragment typed starts, in the text.
    pub start: usize,
    pub items: Vec<Item>,
}

const KEYWORDS: &[&str] = &[
    "ADD",
    "ALL",
    "ALTER",
    "AND",
    "ANY",
    "AS",
    "ASC",
    "BEGIN",
    "BETWEEN",
    "BY",
    "CASCADE",
    "CASE",
    "CHECK",
    "COLUMN",
    "COMMENT",
    "COMMIT",
    "CONSTRAINT",
    "COPY",
    "CREATE",
    "CROSS",
    "CURRENT_DATE",
    "CURRENT_TIMESTAMP",
    "DATABASE",
    "DEFAULT",
    "DELETE",
    "DESC",
    "DISTINCT",
    "DROP",
    "ELSE",
    "END",
    "EXCEPT",
    "EXISTS",
    "EXPLAIN",
    "FALSE",
    "FETCH",
    "FILTER",
    "FIRST",
    "FOREIGN",
    "FROM",
    "FULL",
    "GRANT",
    "GROUP",
    "HAVING",
    "ILIKE",
    "IN",
    "INDEX",
    "INNER",
    "INSERT",
    "INTERSECT",
    "INTO",
    "IS",
    "JOIN",
    "KEY",
    "LAST",
    "LATERAL",
    "LEFT",
    "LIKE",
    "LIMIT",
    "NATURAL",
    "NOT",
    "NULL",
    "NULLS",
    "OFFSET",
    "ON",
    "OR",
    "ORDER",
    "OUTER",
    "OVER",
    "PARTITION",
    "PRIMARY",
    "RECURSIVE",
    "REFERENCES",
    "RETURNING",
    "REVOKE",
    "RIGHT",
    "ROLLBACK",
    "ROW",
    "ROWS",
    "SCHEMA",
    "SELECT",
    "SEQUENCE",
    "SET",
    "SHOW",
    "TABLE",
    "TEMP",
    "TEMPORARY",
    "THEN",
    "TRANSACTION",
    "TRIGGER",
    "TRUE",
    "TRUNCATE",
    "UNION",
    "UNIQUE",
    "UPDATE",
    "USING",
    "VACUUM",
    "VALUES",
    "VIEW",
    "WHEN",
    "WHERE",
    "WINDOW",
    "WITH",
];

const FUNCTIONS: &[&str] = &[
    "ABS",
    "ARRAY_AGG",
    "AVG",
    "BOOL_AND",
    "BOOL_OR",
    "CAST",
    "CEIL",
    "COALESCE",
    "CONCAT",
    "COUNT",
    "DATE_TRUNC",
    "DENSE_RANK",
    "EXTRACT",
    "FIRST_VALUE",
    "FLOOR",
    "GENERATE_SERIES",
    "GREATEST",
    "JSON_AGG",
    "JSONB_AGG",
    "LAG",
    "LAST_VALUE",
    "LEAD",
    "LEAST",
    "LENGTH",
    "LOWER",
    "LTRIM",
    "MAX",
    "MIN",
    "NOW",
    "NULLIF",
    "RANK",
    "REGEXP_REPLACE",
    "REPLACE",
    "ROUND",
    "ROW_NUMBER",
    "RTRIM",
    "SPLIT_PART",
    "STRING_AGG",
    "SUBSTRING",
    "SUM",
    "TO_CHAR",
    "TO_DATE",
    "TO_TIMESTAMP",
    "TRIM",
    "UPPER",
];

/// Words after which a table is expected.
const TABLE_CLAUSES: &[&str] = &[
    "FROM", "JOIN", "INTO", "UPDATE", "TABLE", "TRUNCATE", "ONLY",
];

/// Words that open a part of a statement: what comes next depends on the
/// last one.
const CLAUSES: &[&str] = &[
    "SELECT",
    "FROM",
    "WHERE",
    "GROUP",
    "ORDER",
    "HAVING",
    "JOIN",
    "ON",
    "SET",
    "INTO",
    "UPDATE",
    "VALUES",
    "LIMIT",
    "TABLE",
    "TRUNCATE",
    "USING",
    "RETURNING",
    "AND",
    "OR",
    "BY",
    "WHEN",
    "THEN",
    "ELSE",
    "CASE",
    "DELETE",
    "INSERT",
    "WITH",
];

/// Whether a name has to be between quotes to be read back as it is.
pub fn needs_quotes(name: &str) -> bool {
    let mut chars = name.chars();
    let plain_start = chars
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c == '_');
    let plain = plain_start
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '$');
    !plain || KEYWORDS.contains(&name.to_uppercase().as_str())
}

pub fn quoted(name: &str) -> String {
    if needs_quotes(name) {
        super::protocol::quote_ident(name)
    } else {
        name.to_string()
    }
}

/// What the text at the caret could be completed with. Without `force`, there
/// has to be something typed (or a `.` just before).
pub fn suggest(text: &str, caret: usize, known: &Known, force: bool) -> Option<Suggestions> {
    let caret = caret.min(text.len());
    if !text.is_char_boundary(caret) {
        return None;
    }
    let tokens = sql_tokens(text);
    // Not in a string, a comment or a quoted name.
    if tokens.iter().any(|t| {
        matches!(
            t.kind,
            TokenKind::Text | TokenKind::Comment | TokenKind::Quoted
        ) && t.start < caret
            && caret <= t.end
    }) {
        // The end of a quoted name or a string is the end of it: nothing to add.
        return None;
    }
    // The fragment: the name being typed, and what qualifies it.
    let bytes = text.as_bytes();
    let mut start = caret;
    while start > 0 && is_word(bytes[start - 1]) {
        start -= 1;
    }
    let fragment = &text[start..caret];
    let qualifier = qualifier_before(text, start);
    if fragment.is_empty() && qualifier.is_none() && !force {
        return None;
    }

    // The statement the caret is in, up to the fragment.
    let statement_start = statement_start(text, &tokens, start);
    let before: Vec<Token> = tokens
        .iter()
        .copied()
        .filter(|t| t.start >= statement_start && t.end <= start)
        .collect();
    let referenced = referenced_tables(text, &tokens, statement_start, known);

    let mut items: Vec<Item> = Vec::new();
    let lower = !fragment.is_empty() && !fragment.chars().any(|c| c.is_uppercase());
    let wanted = fragment.to_lowercase();
    let matches = |label: &str| label.to_lowercase().starts_with(&wanted);

    if let Some(prefix) = &qualifier {
        // `x.`: the columns of a table or alias, or the tables of a schema.
        let tables_named = resolve(prefix, &referenced, known);
        for key in &tables_named {
            for (name, kind) in known.columns.get(key).into_iter().flatten() {
                if matches(name) {
                    items.push(Item {
                        label: name.clone(),
                        insert: quoted(name),
                        detail: format!("{} · {kind}", key.1),
                        kind: ItemKind::Column,
                    });
                }
            }
        }
        if tables_named.is_empty() {
            let schema = prefix.last().map(|s| s.to_lowercase()).unwrap_or_default();
            for (s, table, kind) in &known.tables {
                if s.to_lowercase() == schema && matches(table) {
                    items.push(table_item(s, table, *kind, false));
                }
            }
        }
    } else {
        let after = last_clause(text, &before);
        let tables_first = after.as_deref().is_some_and(|w| TABLE_CLAUSES.contains(&w))
            || in_from_list(text, &before);
        let tables = || -> Vec<Item> {
            let mut out = Vec::new();
            for (schema, table, kind) in &known.tables {
                if matches(table) {
                    out.push(table_item(schema, table, *kind, true));
                }
            }
            let mut schemas: Vec<&String> = known.tables.iter().map(|t| &t.0).collect();
            schemas.sort();
            schemas.dedup();
            for schema in schemas {
                if matches(schema) {
                    out.push(Item {
                        label: schema.clone(),
                        insert: quoted(schema),
                        detail: String::new(),
                        kind: ItemKind::Schema,
                    });
                }
            }
            out
        };
        let columns = || -> Vec<Item> {
            let mut out = Vec::new();
            let from: Vec<(String, String)> = if referenced.is_empty() {
                // Nothing said yet which table: all of them, with the table named.
                known.columns.keys().cloned().collect()
            } else {
                referenced.iter().map(|r| r.key.clone()).collect()
            };
            for key in from {
                for (name, kind) in known.columns.get(&key).into_iter().flatten() {
                    if matches(name) {
                        out.push(Item {
                            label: name.clone(),
                            insert: quoted(name),
                            detail: format!("{} · {kind}", key.1),
                            kind: ItemKind::Column,
                        });
                    }
                }
            }
            out
        };
        // After `ON`: the conditions the foreign keys give between the table
        // that was just joined and the others.
        let joins = || -> Vec<Item> {
            join_conditions(&referenced, known)
                .into_iter()
                .filter(|c| matches(c))
                .map(|c| Item {
                    insert: c.clone(),
                    label: c,
                    detail: t!("db.completion_fk").to_string(),
                    kind: ItemKind::Join,
                })
                .collect()
        };
        let routines = || -> Vec<Item> {
            known
                .routines
                .iter()
                .filter(|(_, name, _)| matches(name))
                .map(|(schema, name, args)| Item {
                    label: name.clone(),
                    insert: format!("{}(", quoted(name)),
                    detail: format!("{schema} · {args}"),
                    kind: ItemKind::Function,
                })
                .collect()
        };
        let words = |list: &[&str], kind: ItemKind| -> Vec<Item> {
            list.iter()
                .filter(|w| matches(w))
                .map(|w| {
                    let word = if lower {
                        w.to_lowercase()
                    } else {
                        w.to_string()
                    };
                    Item {
                        label: w.to_string(),
                        insert: if kind == ItemKind::Function {
                            format!("{word}(")
                        } else {
                            word
                        },
                        detail: String::new(),
                        kind,
                    }
                })
                .collect()
        };
        if after.as_deref() == Some("ON") {
            items.extend(joins());
            items.extend(columns());
            items.extend(words(KEYWORDS, ItemKind::Keyword));
        } else if tables_first {
            let mut listed = tables();
            // After `JOIN`: the tables a foreign key links to the ones of the
            // statement come first.
            if after.as_deref() == Some("JOIN") {
                let linked = linked_tables(&referenced, known);
                let (mut first, rest): (Vec<Item>, Vec<Item>) = listed
                    .into_iter()
                    .partition(|i| i.kind != ItemKind::Schema && linked.contains(&i.label));
                for item in &mut first {
                    item.detail = format!("{} · {}", item.detail, t!("db.completion_fk"));
                }
                first.extend(rest);
                listed = first;
            }
            items.extend(listed);
            items.extend(words(KEYWORDS, ItemKind::Keyword));
        } else if before.is_empty() {
            items.extend(words(KEYWORDS, ItemKind::Keyword));
        } else {
            items.extend(columns());
            items.extend(words(FUNCTIONS, ItemKind::Function));
            items.extend(routines());
            items.extend(words(KEYWORDS, ItemKind::Keyword));
            items.extend(tables());
        }
    }

    // One item that is what is typed already: nothing left to say.
    if items.len() == 1 && !fragment.is_empty() && items[0].label.eq_ignore_ascii_case(fragment) {
        return None;
    }
    dedup(&mut items);
    items.truncate(40);
    (!items.is_empty()).then_some(Suggestions { start, items })
}

fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$' || b >= 0x80
}

fn table_item(schema: &str, table: &str, kind: Kind, qualify: bool) -> Item {
    // Out of the usual schema, the schema is part of what is inserted.
    let insert = if qualify && schema != "public" {
        format!("{}.{}", quoted(schema), quoted(table))
    } else {
        quoted(table)
    };
    Item {
        label: table.to_string(),
        insert,
        detail: schema.to_string(),
        kind: if kind == Kind::Table {
            ItemKind::Table
        } else {
            ItemKind::View
        },
    }
}

fn dedup(items: &mut Vec<Item>) {
    let mut seen: Vec<(String, ItemKind, String)> = Vec::new();
    items.retain(|i| {
        let key = (i.insert.clone(), i.kind, i.detail.clone());
        if seen.contains(&key) {
            false
        } else {
            seen.push(key);
            true
        }
    });
}

/// The names before a `.` just in front of `start`: `a.b.` gives `a`, `b`.
fn qualifier_before(text: &str, start: usize) -> Option<Vec<String>> {
    let bytes = text.as_bytes();
    let mut names = Vec::new();
    let mut at = start;
    while at > 0 && bytes[at - 1] == b'.' {
        let end = at - 1;
        let mut from = end;
        if from > 0 && bytes[from - 1] == b'"' {
            // "Quoted name".
            let close = from - 1;
            let open = text[..close].rfind('"')?;
            names.push(text[open + 1..close].to_string());
            at = open;
            continue;
        }
        while from > 0 && is_word(bytes[from - 1]) {
            from -= 1;
        }
        if from == end {
            return None;
        }
        names.push(text[from..end].to_string());
        at = from;
    }
    names.reverse();
    (!names.is_empty()).then_some(names)
}

/// Where the statement the caret is in begins: after the last `;` before it.
fn statement_start(text: &str, tokens: &[Token], before: usize) -> usize {
    tokens
        .iter()
        .rev()
        .find(|t| t.kind == TokenKind::Punct && t.end <= before && &text[t.start..t.end] == ";")
        .map_or(0, |t| t.end)
}

/// The last word that opens a part of the statement, in capitals.
fn last_clause(text: &str, before: &[Token]) -> Option<String> {
    before
        .iter()
        .rev()
        .filter(|t| t.kind == TokenKind::Word)
        .map(|t| text[t.start..t.end].to_uppercase())
        .find(|w| CLAUSES.contains(&w.as_str()))
}

/// Whether the caret is in a list of tables after `FROM` (`FROM a, b, `).
fn in_from_list(text: &str, before: &[Token]) -> bool {
    let mut depth = 0i32;
    for t in before.iter().rev() {
        let s = &text[t.start..t.end];
        match (t.kind, s) {
            (TokenKind::Punct, ")") => depth += 1,
            (TokenKind::Punct, "(") => depth -= 1,
            (TokenKind::Punct, ",") if depth == 0 => {
                // A comma: the list is the one of the clause before it.
                return last_clause_before_comma(text, before, t.start)
                    .is_some_and(|w| w == "FROM" || w == "JOIN");
            }
            (TokenKind::Word, _) if depth == 0 && CLAUSES.contains(&s.to_uppercase().as_str()) => {
                return false;
            }
            _ => {}
        }
    }
    false
}

fn last_clause_before_comma(text: &str, before: &[Token], comma: usize) -> Option<String> {
    last_clause(
        text,
        &before
            .iter()
            .copied()
            .filter(|t| t.end <= comma)
            .collect::<Vec<_>>(),
    )
}

/// A table a statement is about, with the name it goes by in it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Referenced {
    key: (String, String),
    /// The alias, or else the table's name.
    name: String,
    table: String,
}

/// The tables named after `FROM`, `JOIN`, `UPDATE`, `INTO`… in the statement
/// (the ones that are known), with their aliases.
fn referenced_tables(text: &str, tokens: &[Token], from: usize, known: &Known) -> Vec<Referenced> {
    let words: Vec<(usize, &Token)> = tokens
        .iter()
        .enumerate()
        .filter(|(_, t)| t.start >= from && t.kind != TokenKind::Comment)
        .collect();
    let mut out = Vec::new();
    let mut i = 0;
    let slice = |t: &Token| &text[t.start..t.end];
    let name_of = |t: &Token| -> String {
        let s = slice(t);
        if t.kind == TokenKind::Quoted {
            s.trim_matches('"').replace("\"\"", "\"")
        } else {
            s.to_string()
        }
    };
    while i < words.len() {
        let (_, t) = words[i];
        let upper = slice(t).to_uppercase();
        let opens = t.kind == TokenKind::Word
            && matches!(
                upper.as_str(),
                "FROM" | "JOIN" | "UPDATE" | "INTO" | "TABLE"
            );
        i += 1;
        if !opens {
            continue;
        }
        // A list: `a, b x, c AS y`.
        loop {
            let mut parts: Vec<String> = Vec::new();
            while i < words.len() {
                let (_, t) = words[i];
                let named = matches!(t.kind, TokenKind::Word | TokenKind::Quoted);
                if !named {
                    break;
                }
                if slice(t).eq_ignore_ascii_case("ONLY") {
                    i += 1;
                    continue;
                }
                parts.push(name_of(t));
                i += 1;
                let dot = words
                    .get(i)
                    .is_some_and(|(_, t)| t.kind == TokenKind::Punct && slice(t) == ".");
                if dot {
                    i += 1;
                } else {
                    break;
                }
            }
            if parts.is_empty() {
                break;
            }
            // An alias, with or without `AS`.
            let mut alias = None;
            if let Some((_, t)) = words.get(i) {
                if t.kind == TokenKind::Word && slice(t).eq_ignore_ascii_case("AS") {
                    i += 1;
                    if let Some((_, a)) = words.get(i) {
                        alias = Some(name_of(a));
                        i += 1;
                    }
                } else if matches!(t.kind, TokenKind::Word | TokenKind::Quoted)
                    && !CLAUSES.contains(&slice(t).to_uppercase().as_str())
                    && !matches!(
                        slice(t).to_uppercase().as_str(),
                        "INNER"
                            | "LEFT"
                            | "RIGHT"
                            | "FULL"
                            | "CROSS"
                            | "NATURAL"
                            | "LATERAL"
                            | "OUTER"
                            | "USING"
                            | "OFFSET"
                            | "FETCH"
                            | "UNION"
                            | "EXCEPT"
                            | "INTERSECT"
                            | "WINDOW"
                            | "FOR"
                            | "NOT"
                            | "IS"
                            | "NULL"
                    )
                {
                    alias = Some(name_of(t));
                    i += 1;
                }
            }
            let table = parts.pop().unwrap_or_default();
            let schema = parts.pop();
            if let Some(key) = find_table(known, schema.as_deref(), &table) {
                out.push(Referenced {
                    name: alias.unwrap_or_else(|| table.clone()),
                    table: key.1.clone(),
                    key,
                });
            }
            // More after a comma.
            let comma = words
                .get(i)
                .is_some_and(|(_, t)| t.kind == TokenKind::Punct && slice(t) == ",");
            if comma {
                i += 1;
            } else {
                break;
            }
        }
    }
    out
}

/// The names of the tables a foreign key links to a table of the statement.
fn linked_tables(referenced: &[Referenced], known: &Known) -> Vec<String> {
    let mut out = Vec::new();
    for fk in &known.foreign_keys {
        for r in referenced {
            if r.key == (fk.schema.clone(), fk.table.clone()) {
                out.push(fk.ref_table.clone());
            }
            if r.key == (fk.ref_schema.clone(), fk.ref_table.clone()) {
                out.push(fk.table.clone());
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// The conditions a foreign key gives between the table joined last and the
/// other tables of the statement: `o.customer_id = c.id`.
fn join_conditions(referenced: &[Referenced], known: &Known) -> Vec<String> {
    let Some((joined, others)) = referenced.split_last() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for other in others {
        for fk in &known.foreign_keys {
            let from = (fk.schema.clone(), fk.table.clone());
            let to = (fk.ref_schema.clone(), fk.ref_table.clone());
            let pairs =
                |a: &Referenced, a_columns: &[String], b: &Referenced, b_columns: &[String]| {
                    a_columns
                        .iter()
                        .zip(b_columns)
                        .map(|(x, y)| {
                            format!(
                                "{}.{} = {}.{}",
                                quoted(&a.name),
                                quoted(x),
                                quoted(&b.name),
                                quoted(y)
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(" AND ")
                };
            if joined.key == from && other.key == to {
                out.push(pairs(joined, &fk.columns, other, &fk.ref_columns));
            } else if joined.key == to && other.key == from {
                out.push(pairs(other, &fk.columns, joined, &fk.ref_columns));
            }
        }
    }
    out.dedup();
    out
}

/// The known table of this name; in the usual schema first.
fn find_table(known: &Known, schema: Option<&str>, table: &str) -> Option<(String, String)> {
    let mut found: Option<(String, String)> = None;
    for (s, t, _) in &known.tables {
        if !t.eq_ignore_ascii_case(table) {
            continue;
        }
        if let Some(wanted) = schema {
            if !s.eq_ignore_ascii_case(wanted) {
                continue;
            }
        }
        if s == "public" || found.is_none() {
            found = Some((s.clone(), t.clone()));
        }
    }
    found
}

/// The tables a qualifier names: an alias or a table of the statement, or a
/// table of the database (with its schema, if given).
fn resolve(
    qualifier: &[String],
    referenced: &[Referenced],
    known: &Known,
) -> Vec<(String, String)> {
    let Some(last) = qualifier.last() else {
        return Vec::new();
    };
    if qualifier.len() == 1 {
        let by_alias: Vec<(String, String)> = referenced
            .iter()
            .filter(|r| r.name.eq_ignore_ascii_case(last) || r.table.eq_ignore_ascii_case(last))
            .map(|r| r.key.clone())
            .collect();
        if !by_alias.is_empty() {
            return by_alias;
        }
        return find_table(known, None, last).into_iter().collect();
    }
    let schema = &qualifier[qualifier.len() - 2];
    find_table(known, Some(schema), last).into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn known() -> Known {
        let mut known = Known::default();
        let col = |n: &str, t: &str| (n.to_string(), t.to_string());
        for (schema, table, kind, columns) in [
            (
                "public",
                "customers",
                Kind::Table,
                vec![
                    col("id", "integer"),
                    col("name", "text"),
                    col("email", "text"),
                ],
            ),
            (
                "public",
                "orders",
                Kind::Table,
                vec![
                    col("id", "integer"),
                    col("customer_id", "integer"),
                    col("total", "numeric"),
                ],
            ),
            (
                "public",
                "customer_names",
                Kind::View,
                vec![col("name", "text")],
            ),
            (
                "sales",
                "Order Items",
                Kind::Table,
                vec![col("Id", "integer"), col("qty", "integer")],
            ),
        ] {
            known.tables.push((schema.into(), table.into(), kind));
            known.columns.insert((schema.into(), table.into()), columns);
        }
        known.foreign_keys.push(ForeignKey {
            schema: "public".into(),
            table: "orders".into(),
            columns: vec!["customer_id".into()],
            ref_schema: "public".into(),
            ref_table: "customers".into(),
            ref_columns: vec!["id".into()],
        });
        known.routines.push((
            "public".into(),
            "total_of".into(),
            "order_id integer".into(),
        ));
        known
    }

    /// The suggestions with the caret at the end of `text`.
    fn at_end(text: &str) -> Option<Suggestions> {
        suggest(text, text.len(), &known(), false)
    }

    fn labels(s: &Option<Suggestions>) -> Vec<&str> {
        s.as_ref()
            .map(|s| s.items.iter().map(|i| i.label.as_str()).collect())
            .unwrap_or_default()
    }

    #[test]
    fn keywords_are_proposed_from_what_is_typed() {
        let s = at_end("sel");
        assert_eq!(labels(&s), vec!["SELECT"]);
        // The case follows what was typed.
        assert_eq!(s.as_ref().unwrap().items[0].insert, "select");
        assert_eq!(at_end("SEL").unwrap().items[0].insert, "SELECT");
        assert_eq!(s.unwrap().start, 0);
        assert!(labels(&at_end("ORD")).contains(&"ORDER"));
        // Nothing typed: nothing proposed, unless asked.
        assert!(at_end("SELECT ").is_none());
        assert!(suggest("SELECT ", 7, &known(), true).is_some());
    }

    #[test]
    fn after_from_the_tables_come_first() {
        let s = at_end("SELECT * FROM cu");
        assert_eq!(labels(&s)[..2], ["customers", "customer_names"]);
        let items = s.unwrap().items;
        assert_eq!(items[0].kind, ItemKind::Table);
        assert_eq!(items[1].kind, ItemKind::View);
        // A table outside the usual schema comes with its schema, quoted if need be.
        let s = at_end("SELECT * FROM ord");
        let inserts: Vec<&str> = s
            .as_ref()
            .unwrap()
            .items
            .iter()
            .map(|i| i.insert.as_str())
            .collect();
        assert!(inserts.contains(&"orders") && inserts.contains(&"sales.\"Order Items\""));
        assert!(labels(&at_end("UPDATE or")).contains(&"orders"));
        assert!(labels(&at_end("INSERT INTO cust")).contains(&"customers"));
    }

    #[test]
    fn a_list_of_tables_goes_on_after_a_comma() {
        let s = at_end("SELECT * FROM customers c, ord");
        assert!(labels(&s).contains(&"orders"));
        assert_eq!(s.unwrap().items[0].kind, ItemKind::Table);
    }

    #[test]
    fn the_columns_are_those_of_the_tables_of_the_statement() {
        let s = at_end("SELECT na FROM customers");
        // The caret is at the end of the text here: put it before FROM.
        assert!(s.is_none() || !labels(&s).contains(&"total"));
        let text = "SELECT na FROM customers";
        let s = suggest(text, 9, &known(), false);
        assert_eq!(labels(&s)[0], "name");
        assert!(!labels(&s).contains(&"total"));
        // Another table of the statement adds its columns.
        let text = "SELECT cu FROM customers c JOIN orders o ON o.customer_id = c.id";
        let s = suggest(text, 9, &known(), false);
        assert!(labels(&s).contains(&"customer_id") && labels(&s).contains(&"customers"));
        // After WHERE too.
        let s = at_end("SELECT * FROM orders WHERE to");
        assert_eq!(labels(&s)[0], "total");
        assert_eq!(s.unwrap().items[0].detail, "orders · numeric");
    }

    #[test]
    fn an_alias_or_a_table_before_a_dot_gives_its_columns() {
        let text = "SELECT c.na FROM customers c";
        let s = suggest(text, 11, &known(), false);
        assert_eq!(labels(&s), vec!["name"]);
        assert_eq!(s.unwrap().start, 9);
        let text = "SELECT o. FROM customers c, orders o";
        let s = suggest(text, 9, &known(), false);
        assert_eq!(labels(&s), vec!["id", "customer_id", "total"]);
        // Without an alias, the table's own name; AS aliases too.
        let text = "SELECT orders. FROM orders";
        assert_eq!(labels(&suggest(text, 14, &known(), false))[0], "id");
        let text = "SELECT x.tot FROM orders AS x";
        assert_eq!(labels(&suggest(text, 12, &known(), false)), vec!["total"]);
    }

    #[test]
    fn a_schema_before_a_dot_gives_its_tables_and_a_table_its_columns() {
        let s = at_end("SELECT * FROM sales.");
        assert_eq!(labels(&s), vec!["Order Items"]);
        assert_eq!(s.as_ref().unwrap().items[0].insert, "\"Order Items\"");
        let text = "SELECT sales.\"Order Items\".q FROM sales.\"Order Items\"";
        let s = suggest(text, 28, &known(), false);
        assert_eq!(labels(&s), vec!["qty"]);
    }

    #[test]
    fn nothing_is_proposed_in_a_string_or_a_comment() {
        assert!(at_end("SELECT 'sel").is_none());
        assert!(at_end("SELECT 1 -- sel").is_none());
        assert!(at_end("SELECT \"cu").is_none());
        // After the string is over, it works again.
        assert!(at_end("SELECT 'a' FRO").is_some());
    }

    #[test]
    fn the_statement_before_a_semicolon_is_not_the_context() {
        let text = "SELECT * FROM orders;\nSELECT na";
        let s = at_end(text);
        // No FROM in the second statement: the names of every table's columns.
        assert!(labels(&s).contains(&"name"));
        assert!(!labels(&s).is_empty());
        let s = at_end("SELECT 1; sel");
        assert_eq!(labels(&s), vec!["SELECT"]);
    }

    #[test]
    fn functions_come_after_the_columns() {
        let s = at_end("SELECT COU");
        assert_eq!(labels(&s)[0], "COUNT");
        assert_eq!(s.unwrap().items[0].insert, "COUNT(");
        // In lowercase when it is typed so.
        assert_eq!(at_end("select cou").unwrap().items[0].insert, "count(");
    }

    #[test]
    fn a_name_is_quoted_when_it_has_to_be() {
        assert!(!needs_quotes("customers"));
        assert!(needs_quotes("Order Items"));
        assert!(needs_quotes("Id"));
        assert!(needs_quotes("select"));
        assert!(needs_quotes("1abc"));
        assert_eq!(quoted("user_id"), "user_id");
        assert_eq!(quoted("a\"b"), "\"a\"\"b\"");
    }

    #[test]
    fn what_is_complete_already_is_left_alone() {
        assert!(at_end("SELECT * FROM customer_names").is_none());
    }

    #[test]
    fn after_join_the_tables_a_foreign_key_links_come_first() {
        let s = suggest("SELECT * FROM customers c JOIN ", 31, &known(), true);
        let items = &s.unwrap().items;
        assert_eq!(items[0].label, "orders");
        assert!(
            items[0].detail.contains("foreign key"),
            "{}",
            items[0].detail
        );
        // From the other side too.
        let text = "SELECT * FROM orders o JOIN ";
        let s = suggest(text, text.len(), &known(), true).unwrap();
        assert_eq!(s.items[0].label, "customers");
    }

    #[test]
    fn after_on_the_condition_of_the_foreign_key_is_proposed() {
        let text = "SELECT * FROM customers c JOIN orders o ON ";
        let s = suggest(text, text.len(), &known(), true).unwrap();
        assert_eq!(s.items[0].label, "o.customer_id = c.id");
        assert_eq!(s.items[0].kind, ItemKind::Join);
        // Joined the other way round, the condition is the same.
        let text = "SELECT * FROM orders o JOIN customers c ON ";
        let s = suggest(text, text.len(), &known(), true).unwrap();
        assert_eq!(s.items[0].label, "o.customer_id = c.id");
        // After `o.`, it is the columns of `o` that are proposed.
        let text = "SELECT * FROM customers c JOIN orders o ON o.cu";
        let s = suggest(text, text.len(), &known(), false).unwrap();
        assert_eq!(s.items[0].label, "customer_id");
    }

    #[test]
    fn the_functions_of_the_schemas_are_proposed() {
        let s = at_end("SELECT total_");
        let first = &s.unwrap().items[0];
        assert_eq!(first.label, "total_of");
        assert_eq!(first.insert, "total_of(");
        assert!(first.detail.contains("order_id integer"));
    }
}
