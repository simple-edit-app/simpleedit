//! The table designer: what a table is made of, as it is edited, and the
//! statements that bring the table to what was edited (or make it, for a new
//! table).

use super::completion::quoted;
use super::protocol::{
    quote_ident, quote_literal, ColumnDef, ConstraintDef, ConstraintKind, Dialect, IndexDef,
    Structure,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesignerTab {
    Columns,
    Constraints,
    Indexes,
    General,
    Sql,
}

impl DesignerTab {
    pub const ALL: [DesignerTab; 5] = [
        DesignerTab::Columns,
        DesignerTab::Constraints,
        DesignerTab::Indexes,
        DesignerTab::General,
        DesignerTab::Sql,
    ];
}

#[derive(Debug, Clone, PartialEq)]
pub enum Load {
    Loading,
    Ready,
    Running,
}

/// A column being edited: what it was (none for a new one) and what it is.
#[derive(Debug, Clone, PartialEq)]
pub struct EditColumn {
    pub original: Option<ColumnDef>,
    pub name: String,
    pub kind: String,
    pub nullable: bool,
    pub default: String,
    pub comment: String,
    /// For a change of type: how the old values become the new ones
    /// (`col::type` when empty).
    pub using: String,
    pub dropped: bool,
}

impl EditColumn {
    /// The type was changed.
    pub fn retyped(&self) -> bool {
        self.original
            .as_ref()
            .is_some_and(|o| o.kind != self.kind.trim())
    }
}

/// What can be edited of a constraint.
#[derive(Debug, Clone, PartialEq)]
pub struct ConFields {
    pub name: String,
    /// The columns, separated by commas.
    pub columns: String,
    /// For a foreign key: the table it points to, its columns, what a
    /// deletion there does here.
    pub reference: String,
    pub ref_columns: String,
    pub on_delete: String,
    /// For a check: the condition.
    pub expr: String,
}

/// A constraint: one that exists is edited from what its definition says (it
/// is dropped and made again if it changes), a new one is described.
#[derive(Debug, Clone, PartialEq)]
pub struct EditConstraint {
    pub original: Option<ConstraintDef>,
    pub kind: ConstraintKind,
    pub name: String,
    pub columns: String,
    pub reference: String,
    pub ref_columns: String,
    pub on_delete: String,
    pub expr: String,
    /// What the definition says that is not edited (`DEFERRABLE`, `ON UPDATE …`).
    pub extra: String,
    /// What it was when read; none if the definition was not understood (then
    /// it can only be dropped).
    pub base: Option<ConFields>,
    pub dropped: bool,
}

impl EditConstraint {
    pub fn fields(&self) -> ConFields {
        ConFields {
            name: self.name.clone(),
            columns: self.columns.clone(),
            reference: self.reference.clone(),
            ref_columns: self.ref_columns.clone(),
            on_delete: self.on_delete.clone(),
            expr: self.expr.clone(),
        }
    }

    /// It can be edited: it is new, or its definition was understood.
    pub fn editable(&self) -> bool {
        self.original.is_none() || self.base.is_some()
    }

    pub fn modified(&self) -> bool {
        self.base.as_ref().is_some_and(|b| *b != self.fields())
    }

    /// Only its name changed.
    fn renamed_only(&self) -> bool {
        self.base.as_ref().is_some_and(|b| {
            b.name != self.name.trim()
                && ConFields {
                    name: String::new(),
                    ..b.clone()
                } == ConFields {
                    name: String::new(),
                    ..self.fields()
                }
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct IdxFields {
    pub name: String,
    pub columns: String,
    pub unique: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EditIndex {
    pub original: Option<IndexDef>,
    pub name: String,
    pub columns: String,
    pub unique: bool,
    /// The access method (`btree`), and what follows the columns (`WHERE …`).
    pub method: String,
    pub extra: String,
    pub base: Option<IdxFields>,
    pub dropped: bool,
}

impl EditIndex {
    pub fn fields(&self) -> IdxFields {
        IdxFields {
            name: self.name.clone(),
            columns: self.columns.clone(),
            unique: self.unique,
        }
    }

    pub fn editable(&self) -> bool {
        self.original.is_none() || self.base.is_some()
    }

    pub fn modified(&self) -> bool {
        self.base.as_ref().is_some_and(|b| *b != self.fields())
    }

    fn renamed_only(&self) -> bool {
        self.base.as_ref().is_some_and(|b| {
            b.name != self.name.trim() && b.columns == self.columns && b.unique == self.unique
        })
    }
}

/// What is wrong with what was edited.
#[derive(Debug, Clone, PartialEq)]
pub enum Problem {
    EmptyTableName,
    EmptySchema,
    EmptyColumnName,
    DuplicateColumn(String),
    EmptyType(String),
    NeedsColumns,
    NeedsReference,
    NeedsExpression,
    SeveralKeys,
    NoColumns,
}

pub struct Designer {
    pub conn: u64,
    pub database: String,
    pub schema: String,
    pub table: String,
    /// A new table, not one that is changed.
    pub creating: bool,
    pub tab: DesignerTab,
    pub load: Load,
    /// What the last try said, if it failed.
    pub error: Option<String>,
    pub original: Structure,
    pub name: String,
    /// The schema the table is in at the end.
    pub schema_to: String,
    pub comment: String,
    pub columns: Vec<EditColumn>,
    pub constraints: Vec<EditConstraint>,
    pub indexes: Vec<EditIndex>,
}

/// The actions of a foreign key on deletion.
pub const ON_DELETE: [&str; 5] = [
    "NO ACTION",
    "CASCADE",
    "SET NULL",
    "SET DEFAULT",
    "RESTRICT",
];

/// Types that are often wanted, for the list beside the type of a column.
pub const COMMON_TYPES: [&str; 22] = [
    "integer",
    "bigint",
    "smallint",
    "serial",
    "bigserial",
    "numeric(10,2)",
    "real",
    "double precision",
    "boolean",
    "text",
    "varchar(255)",
    "char(1)",
    "uuid",
    "date",
    "time",
    "timestamp",
    "timestamptz",
    "interval",
    "json",
    "jsonb",
    "bytea",
    "text[]",
];

// ---------------------------------------------------------------- parsing

/// The text between the parenthesis that opens at `from` and the one that
/// closes it, and where that is; strings and quoted names are skipped.
fn group(text: &str, from: usize) -> Option<(String, usize)> {
    let bytes = text.as_bytes();
    if bytes.get(from) != Some(&b'(') {
        return None;
    }
    let mut depth = 0usize;
    let mut i = from;
    while i < bytes.len() {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some((text[from + 1..i].to_string(), i + 1));
                }
            }
            q @ (b'\'' | b'"') => {
                i += 1;
                while i < bytes.len() && bytes[i] != q {
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// The first `(` that is not in a string or a quoted name.
fn open_paren(text: &str, from: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut i = from;
    while i < bytes.len() {
        match bytes[i] {
            b'(' => return Some(i),
            q @ (b'\'' | b'"') => {
                i += 1;
                while i < bytes.len() && bytes[i] != q {
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// What a definition read from the database says, in the fields that are
/// edited and the rest.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedConstraint {
    pub columns: String,
    pub reference: String,
    pub ref_columns: String,
    pub on_delete: String,
    pub expr: String,
    pub extra: String,
}

/// Reads the definition `pg_get_constraintdef` gives.
pub fn parse_constraint(kind: ConstraintKind, definition: &str) -> Option<ParsedConstraint> {
    let def = definition.trim();
    let empty = ParsedConstraint {
        columns: String::new(),
        reference: String::new(),
        ref_columns: String::new(),
        on_delete: ON_DELETE[0].to_string(),
        expr: String::new(),
        extra: String::new(),
    };
    match kind {
        ConstraintKind::Primary | ConstraintKind::Unique => {
            let open = open_paren(def, 0)?;
            let (columns, end) = group(def, open)?;
            Some(ParsedConstraint {
                columns: columns.trim().to_string(),
                extra: def[end..].trim().to_string(),
                ..empty
            })
        }
        ConstraintKind::Check => {
            let open = open_paren(def, 0)?;
            let (inner, end) = group(def, open)?;
            // `CHECK ((a > 0))`: the condition is in a pair of its own.
            let mut expr = inner.trim().to_string();
            if let Some((again, after)) = group(&expr, 0) {
                if after == expr.len() {
                    expr = again.trim().to_string();
                }
            }
            Some(ParsedConstraint {
                expr,
                extra: def[end..].trim().to_string(),
                ..empty
            })
        }
        ConstraintKind::Foreign => {
            let open = open_paren(def, 0)?;
            let (columns, end) = group(def, open)?;
            let rest = def[end..].trim_start();
            let rest = rest.strip_prefix("REFERENCES")?.trim_start();
            let (reference, ref_columns, tail) = match open_paren(rest, 0) {
                Some(p) => {
                    let (cols, after) = group(rest, p)?;
                    (
                        rest[..p].trim().to_string(),
                        cols.trim().to_string(),
                        rest[after..].to_string(),
                    )
                }
                None => {
                    let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
                    (
                        rest[..end].to_string(),
                        String::new(),
                        rest[end..].to_string(),
                    )
                }
            };
            // `ON DELETE action` is edited; the rest stays as it is.
            let mut on_delete = ON_DELETE[0].to_string();
            let mut extra = tail.trim().to_string();
            if let Some(at) = extra.find("ON DELETE ") {
                let after = &extra[at + "ON DELETE ".len()..];
                // The longest action that begins it ("SET NULL" before "SET").
                let mut actions = ON_DELETE.to_vec();
                actions.sort_by_key(|a| std::cmp::Reverse(a.len()));
                if let Some(action) = actions.iter().find(|a| after.starts_with(**a)) {
                    on_delete = action.to_string();
                    let cut = at + "ON DELETE ".len() + action.len();
                    extra = extra[..at]
                        .split_whitespace()
                        .chain(extra[cut..].split_whitespace())
                        .collect::<Vec<_>>()
                        .join(" ");
                }
            }
            Some(ParsedConstraint {
                columns: columns.trim().to_string(),
                reference,
                ref_columns,
                on_delete,
                extra,
                ..empty
            })
        }
        ConstraintKind::Other => None,
    }
}

/// What an index definition says.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedIndex {
    pub unique: bool,
    pub method: String,
    pub columns: String,
    pub extra: String,
}

/// Reads the `CREATE INDEX` statement `pg_get_indexdef` gives.
pub fn parse_index(definition: &str) -> Option<ParsedIndex> {
    let def = definition.trim();
    let unique = def.starts_with("CREATE UNIQUE INDEX");
    let at = def.find(" USING ")?;
    let after = &def[at + " USING ".len()..];
    let method_end = after.find(|c: char| c.is_whitespace() || c == '(')?;
    let method = after[..method_end].to_string();
    let rest = &after[method_end..];
    let open = rest.find('(')?;
    let (columns, end) = group(rest, open)?;
    Some(ParsedIndex {
        unique,
        method,
        columns: columns.trim().to_string(),
        extra: rest[end..].trim().to_string(),
    })
}

/// A list typed with commas, cut at the commas that are not inside
/// parentheses.
fn split_top(text: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut current = String::new();
    let mut quote: Option<char> = None;
    for c in text.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => {
                quote = None;
                current.push(c);
            }
            (Some(_), c) => current.push(c),
            (None, '\'' | '"') => {
                quote = Some(c);
                current.push(c);
            }
            (None, '(') => {
                depth += 1;
                current.push(c);
            }
            (None, ')') => {
                depth -= 1;
                current.push(c);
            }
            (None, ',') if depth == 0 => {
                parts.push(std::mem::take(&mut current));
            }
            (None, c) => current.push(c),
        }
    }
    parts.push(current);
    parts
        .into_iter()
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect()
}

/// The names of a list typed with commas.
pub fn list(text: &str) -> Vec<String> {
    split_top(text)
        .into_iter()
        .map(|s| s.trim_matches('"').to_string())
        .collect()
}

/// The columns of a key: names, quoted where they have to be.
fn columns_sql(text: &str) -> String {
    list(text)
        .iter()
        .map(|c| quoted(c, Dialect::Postgres))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The columns of an index: names are quoted where they have to be, what is
/// more than a name (`lower(a)`, `a DESC`) stays as it was typed.
fn index_columns_sql(text: &str) -> String {
    split_top(text)
        .iter()
        .map(|c| {
            let plain = c
                .chars()
                .all(|ch| ch.is_alphanumeric() || ch == '_' || ch == '"');
            if plain {
                quoted(c.trim_matches('"'), Dialect::Postgres)
            } else {
                c.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// A table named `schema.table` or `table`, quoted where it has to be.
fn table_sql(text: &str) -> String {
    text.split('.')
        .map(|part| quoted(part.trim().trim_matches('"'), Dialect::Postgres))
        .collect::<Vec<_>>()
        .join(".")
}

// ----------------------------------------------------------------- designer

impl Designer {
    /// The designer of a table, waiting for its structure.
    pub fn new(conn: u64, database: String, schema: String, table: String) -> Self {
        Designer {
            conn,
            database,
            name: table.clone(),
            schema_to: schema.clone(),
            schema,
            table,
            creating: false,
            tab: DesignerTab::Columns,
            load: Load::Loading,
            error: None,
            original: Structure::default(),
            comment: String::new(),
            columns: Vec::new(),
            constraints: Vec::new(),
            indexes: Vec::new(),
        }
    }

    /// The designer of a table that does not exist yet: it starts with an
    /// `id` column that is the primary key.
    pub fn for_new_table(conn: u64, database: String, schema: String) -> Self {
        let mut d = Designer::new(conn, database, schema, String::new());
        d.creating = true;
        d.load = Load::Ready;
        d.columns.push(EditColumn {
            original: None,
            name: "id".to_string(),
            kind: "serial".to_string(),
            nullable: false,
            default: String::new(),
            comment: String::new(),
            using: String::new(),
            dropped: false,
        });
        d.add_constraint(ConstraintKind::Primary);
        if let Some(key) = d.constraints.last_mut() {
            key.columns = "id".to_string();
        }
        d
    }

    /// Takes the structure that was read as what is edited.
    pub fn load(&mut self, structure: Structure) {
        self.comment = structure.comment.clone();
        self.columns = structure
            .columns
            .iter()
            .map(|c| EditColumn {
                original: Some(c.clone()),
                name: c.name.clone(),
                kind: c.kind.clone(),
                nullable: c.nullable,
                default: c.default.clone().unwrap_or_default(),
                comment: c.comment.clone(),
                using: String::new(),
                dropped: false,
            })
            .collect();
        self.constraints = structure
            .constraints
            .iter()
            .map(|c| {
                let parsed = parse_constraint(c.kind, &c.definition);
                let mut edit = EditConstraint {
                    original: Some(c.clone()),
                    kind: c.kind,
                    name: c.name.clone(),
                    columns: String::new(),
                    reference: String::new(),
                    ref_columns: String::new(),
                    on_delete: ON_DELETE[0].to_string(),
                    expr: String::new(),
                    extra: String::new(),
                    base: None,
                    dropped: false,
                };
                if let Some(p) = parsed {
                    edit.columns = p.columns;
                    edit.reference = p.reference;
                    edit.ref_columns = p.ref_columns;
                    edit.on_delete = p.on_delete;
                    edit.expr = p.expr;
                    edit.extra = p.extra;
                    edit.base = Some(edit.fields());
                }
                edit
            })
            .collect();
        self.indexes = structure
            .indexes
            .iter()
            .map(|i| {
                let parsed = parse_index(&i.definition);
                let mut edit = EditIndex {
                    original: Some(i.clone()),
                    name: i.name.clone(),
                    columns: String::new(),
                    unique: i.unique,
                    method: "btree".to_string(),
                    extra: String::new(),
                    base: None,
                    dropped: false,
                };
                if let Some(p) = parsed {
                    edit.columns = p.columns;
                    edit.unique = p.unique;
                    edit.method = p.method;
                    edit.extra = p.extra;
                    edit.base = Some(edit.fields());
                }
                edit
            })
            .collect();
        self.original = structure;
        self.load = Load::Ready;
    }

    pub fn add_column(&mut self) {
        self.columns.push(EditColumn {
            original: None,
            name: String::new(),
            kind: "text".to_string(),
            nullable: true,
            default: String::new(),
            comment: String::new(),
            using: String::new(),
            dropped: false,
        });
    }

    pub fn add_constraint(&mut self, kind: ConstraintKind) {
        self.constraints.push(EditConstraint {
            original: None,
            kind,
            name: String::new(),
            columns: String::new(),
            reference: String::new(),
            ref_columns: String::new(),
            on_delete: ON_DELETE[0].to_string(),
            expr: String::new(),
            extra: String::new(),
            base: None,
            dropped: false,
        });
    }

    pub fn add_index(&mut self) {
        self.indexes.push(EditIndex {
            original: None,
            name: String::new(),
            columns: String::new(),
            unique: false,
            method: "btree".to_string(),
            extra: String::new(),
            base: None,
            dropped: false,
        });
    }

    /// The table as it is named now, for what is run before it is moved.
    fn current(&self) -> String {
        if self.creating {
            return format!(
                "{}.{}",
                quote_ident(self.schema_to.trim()),
                quote_ident(self.name.trim())
            );
        }
        format!("{}.{}", quote_ident(&self.schema), quote_ident(&self.table))
    }

    /// Whether the table is moved to another schema.
    pub fn moved(&self) -> bool {
        !self.creating && self.schema_to.trim() != self.schema
    }

    /// What is wrong with what was edited, first thing first.
    pub fn problem(&self) -> Option<Problem> {
        if self.name.trim().is_empty() {
            return Some(Problem::EmptyTableName);
        }
        if self.schema_to.trim().is_empty() {
            return Some(Problem::EmptySchema);
        }
        let live: Vec<&EditColumn> = self.columns.iter().filter(|c| !c.dropped).collect();
        if live.is_empty() {
            return Some(Problem::NoColumns);
        }
        for (i, c) in live.iter().enumerate() {
            if c.name.trim().is_empty() {
                return Some(Problem::EmptyColumnName);
            }
            if c.kind.trim().is_empty() {
                return Some(Problem::EmptyType(c.name.clone()));
            }
            if live[..i].iter().any(|o| o.name.trim() == c.name.trim()) {
                return Some(Problem::DuplicateColumn(c.name.clone()));
            }
        }
        // The ones that are made: the new, and the existing that changed.
        let made: Vec<&EditConstraint> = self
            .constraints
            .iter()
            .filter(|c| !c.dropped && (c.original.is_none() || c.modified()))
            .collect();
        for c in &made {
            match c.kind {
                ConstraintKind::Primary | ConstraintKind::Unique if list(&c.columns).is_empty() => {
                    return Some(Problem::NeedsColumns)
                }
                ConstraintKind::Foreign => {
                    if list(&c.columns).is_empty() {
                        return Some(Problem::NeedsColumns);
                    }
                    if c.reference.trim().is_empty() {
                        return Some(Problem::NeedsReference);
                    }
                }
                ConstraintKind::Check if c.expr.trim().is_empty() => {
                    return Some(Problem::NeedsExpression)
                }
                _ => {}
            }
        }
        for i in self
            .indexes
            .iter()
            .filter(|i| !i.dropped && (i.original.is_none() || i.modified()))
        {
            if split_top(&i.columns).is_empty() {
                return Some(Problem::NeedsColumns);
            }
        }
        // A table has one primary key: the ones that stay or are made.
        let keys = self
            .constraints
            .iter()
            .filter(|c| c.kind == ConstraintKind::Primary && !c.dropped)
            .count();
        if keys > 1 {
            return Some(Problem::SeveralKeys);
        }
        None
    }

    /// The statements that bring the table to what was edited, in an order
    /// that works: what is dropped goes first, then the columns, then what is
    /// made again or added, and the table is moved and renamed last. For a
    /// new table, the statements that make it.
    pub fn statements(&self) -> Vec<String> {
        if self.creating {
            return self.create_statements();
        }
        let table = self.current();
        let mut out: Vec<String> = Vec::new();

        // What is dropped, or made again, goes before the columns it may use.
        for c in &self.constraints {
            let Some(original) = &c.original else {
                continue;
            };
            if c.dropped || (c.modified() && !c.renamed_only()) {
                out.push(format!(
                    "ALTER TABLE {table} DROP CONSTRAINT {}",
                    quote_ident(&original.name)
                ));
            } else if c.modified() && c.renamed_only() {
                out.push(format!(
                    "ALTER TABLE {table} RENAME CONSTRAINT {} TO {}",
                    quote_ident(&original.name),
                    quote_ident(c.name.trim())
                ));
            }
        }
        for i in &self.indexes {
            let Some(original) = &i.original else {
                continue;
            };
            let index = format!(
                "{}.{}",
                quote_ident(&self.schema),
                quote_ident(&original.name)
            );
            if i.dropped || (i.modified() && !i.renamed_only()) {
                out.push(format!("DROP INDEX {index}"));
            } else if i.modified() && i.renamed_only() {
                out.push(format!(
                    "ALTER INDEX {index} RENAME TO {}",
                    quote_ident(i.name.trim())
                ));
            }
        }

        // The columns.
        for c in &self.columns {
            match (&c.original, c.dropped) {
                (Some(original), true) => out.push(format!(
                    "ALTER TABLE {table} DROP COLUMN {}",
                    quote_ident(&original.name)
                )),
                (Some(original), false) => self.change_column(&table, original, c, &mut out),
                (None, false) => {
                    out.extend(add_column_sql(&table, c));
                }
                (None, true) => {}
            }
        }

        // What is made again or added. What is made again was written with
        // the names the columns had: they are the new ones by now.
        let renames: Vec<(&str, &str)> = self
            .columns
            .iter()
            .filter(|c| !c.dropped)
            .filter_map(|c| {
                let original = c.original.as_ref()?;
                (c.name.trim() != original.name).then(|| (original.name.as_str(), c.name.trim()))
            })
            .collect();
        for c in &self.constraints {
            if c.dropped {
                continue;
            }
            let again = c.original.is_some() && c.modified() && !c.renamed_only();
            if c.original.is_none() {
                out.push(format!("ALTER TABLE {table} ADD {}", constraint_sql(c)));
            } else if again {
                let mut moved = c.clone();
                moved.columns = rename_columns(&c.columns, &renames);
                moved.expr = rename_columns(&c.expr, &renames);
                out.push(format!(
                    "ALTER TABLE {table} ADD {}",
                    constraint_sql(&moved)
                ));
            }
        }
        for i in &self.indexes {
            if i.dropped {
                continue;
            }
            let again = i.original.is_some() && i.modified() && !i.renamed_only();
            if i.original.is_none() {
                out.push(index_sql(i, &table));
            } else if again {
                let mut moved = i.clone();
                moved.columns = rename_columns(&i.columns, &renames);
                moved.extra = rename_columns(&i.extra, &renames);
                out.push(index_sql(&moved, &table));
            }
        }

        // The table: its comment, then where it is and what it is called.
        if self.comment.trim() != self.original.comment.trim() {
            out.push(table_comment(&table, &self.comment));
        }
        let mut name = self.table.clone();
        let mut at = table;
        if self.moved() {
            out.push(format!(
                "ALTER TABLE {at} SET SCHEMA {}",
                quote_ident(self.schema_to.trim())
            ));
            at = format!(
                "{}.{}",
                quote_ident(self.schema_to.trim()),
                quote_ident(&name)
            );
        }
        if self.name.trim() != self.table {
            out.push(format!(
                "ALTER TABLE {at} RENAME TO {}",
                quote_ident(self.name.trim())
            ));
            name = self.name.trim().to_string();
        }
        let _ = name;
        out
    }

    /// The statements that make a new table.
    fn create_statements(&self) -> Vec<String> {
        let table = self.current();
        let mut parts: Vec<String> = Vec::new();
        for c in self.columns.iter().filter(|c| !c.dropped) {
            let mut def = format!("{} {}", quote_ident(c.name.trim()), c.kind.trim());
            if !c.nullable {
                def.push_str(" NOT NULL");
            }
            if !c.default.trim().is_empty() {
                def.push_str(&format!(" DEFAULT {}", c.default.trim()));
            }
            parts.push(def);
        }
        for c in self.constraints.iter().filter(|c| !c.dropped) {
            parts.push(constraint_sql(c));
        }
        let mut out = vec![format!(
            "CREATE TABLE {table} (\n  {}\n)",
            parts.join(",\n  ")
        )];
        for i in self.indexes.iter().filter(|i| !i.dropped) {
            out.push(index_sql(i, &table));
        }
        if !self.comment.trim().is_empty() {
            out.push(table_comment(&table, &self.comment));
        }
        for c in self.columns.iter().filter(|c| !c.dropped) {
            if !c.comment.trim().is_empty() {
                out.push(column_comment(&table, c.name.trim(), &c.comment));
            }
        }
        out
    }

    /// What changed in a column that stays.
    fn change_column(
        &self,
        table: &str,
        original: &ColumnDef,
        c: &EditColumn,
        out: &mut Vec<String>,
    ) {
        let old = quote_ident(&original.name);
        let name = quote_ident(c.name.trim());
        if c.name.trim() != original.name {
            out.push(format!("ALTER TABLE {table} RENAME COLUMN {old} TO {name}"));
        }
        let retyped = c.kind.trim() != original.kind;
        // A default is not carried over a change of type: it is put back.
        let old_default = original.default.clone().unwrap_or_default();
        let plain = !original.generated && original.identity.is_empty();
        let redo_default = retyped && plain && !old_default.is_empty();
        if redo_default {
            out.push(format!(
                "ALTER TABLE {table} ALTER COLUMN {name} DROP DEFAULT"
            ));
        }
        if retyped {
            let using = if c.using.trim().is_empty() {
                format!("{name}::{}", c.kind.trim())
            } else {
                c.using.trim().to_string()
            };
            out.push(format!(
                "ALTER TABLE {table} ALTER COLUMN {name} TYPE {} USING {using}",
                c.kind.trim()
            ));
        }
        if c.nullable != original.nullable {
            out.push(format!(
                "ALTER TABLE {table} ALTER COLUMN {name} {} NOT NULL",
                if c.nullable { "DROP" } else { "SET" }
            ));
        }
        let new_default = c.default.trim();
        if plain {
            if redo_default {
                if !new_default.is_empty() {
                    out.push(format!(
                        "ALTER TABLE {table} ALTER COLUMN {name} SET DEFAULT {new_default}"
                    ));
                }
            } else if new_default != old_default.trim() {
                out.push(if new_default.is_empty() {
                    format!("ALTER TABLE {table} ALTER COLUMN {name} DROP DEFAULT")
                } else {
                    format!("ALTER TABLE {table} ALTER COLUMN {name} SET DEFAULT {new_default}")
                });
            }
        }
        if c.comment.trim() != original.comment.trim() {
            out.push(column_comment(table, c.name.trim(), &c.comment));
        }
    }

    /// Whether anything was changed.
    pub fn has_changes(&self) -> bool {
        !self.statements().is_empty()
    }
}

fn add_column_sql(table: &str, c: &EditColumn) -> Vec<String> {
    let mut add = format!(
        "ALTER TABLE {table} ADD COLUMN {} {}",
        quote_ident(c.name.trim()),
        c.kind.trim()
    );
    if !c.nullable {
        add.push_str(" NOT NULL");
    }
    if !c.default.trim().is_empty() {
        add.push_str(&format!(" DEFAULT {}", c.default.trim()));
    }
    let mut out = vec![add];
    if !c.comment.trim().is_empty() {
        out.push(column_comment(table, c.name.trim(), &c.comment));
    }
    out
}

fn table_comment(table: &str, comment: &str) -> String {
    if comment.trim().is_empty() {
        format!("COMMENT ON TABLE {table} IS NULL")
    } else {
        format!(
            "COMMENT ON TABLE {table} IS {}",
            quote_literal(comment.trim())
        )
    }
}

fn column_comment(table: &str, column: &str, comment: &str) -> String {
    if comment.trim().is_empty() {
        format!("COMMENT ON COLUMN {table}.{} IS NULL", quote_ident(column))
    } else {
        format!(
            "COMMENT ON COLUMN {table}.{} IS {}",
            quote_ident(column),
            quote_literal(comment.trim())
        )
    }
}

/// `CREATE INDEX` for an index that is new or made again.
/// `text` (a list of columns, an expression) with the names of columns that
/// were renamed put in place of the old ones, bare or quoted; quoted text and
/// string literals are left alone. All at once, so that two columns can swap.
fn rename_columns(text: &str, renames: &[(&str, &str)]) -> String {
    if renames.is_empty() {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut at = 0;
    while at < chars.len() {
        let c = chars[at];
        if c == '\'' || c == '"' {
            let mut end = at + 1;
            while end < chars.len() {
                if chars[end] == c {
                    if chars.get(end + 1) == Some(&c) {
                        end += 2;
                        continue;
                    }
                    break;
                }
                end += 1;
            }
            let end = (end + 1).min(chars.len());
            let token: String = chars[at..end].iter().collect();
            let name = token.trim_matches('"');
            match renames.iter().find(|(old, _)| c == '"' && *old == name) {
                Some((_, new)) => out.push_str(&quoted(new, Dialect::Postgres)),
                None => out.push_str(&token),
            }
            at = end;
        } else if c.is_alphanumeric() || c == '_' {
            let mut end = at;
            while end < chars.len() && (chars[end].is_alphanumeric() || chars[end] == '_') {
                end += 1;
            }
            let word: String = chars[at..end].iter().collect();
            match renames.iter().find(|(old, _)| *old == word) {
                Some((_, new)) => out.push_str(&quoted(new, Dialect::Postgres)),
                None => out.push_str(&word),
            }
            at = end;
        } else {
            out.push(c);
            at += 1;
        }
    }
    out
}

fn index_sql(i: &EditIndex, table: &str) -> String {
    let name = if i.name.trim().is_empty() {
        String::new()
    } else {
        format!("{} ", quote_ident(i.name.trim()))
    };
    let method = if i.method == "btree" || i.method.is_empty() {
        String::new()
    } else {
        format!("USING {} ", i.method)
    };
    let extra = if i.extra.trim().is_empty() {
        String::new()
    } else {
        format!(" {}", i.extra.trim())
    };
    format!(
        "CREATE {}INDEX {name}ON {table} {method}({}){extra}",
        if i.unique { "UNIQUE " } else { "" },
        index_columns_sql(&i.columns)
    )
}

/// The definition of a constraint that is made, after `ADD` (or in the
/// middle of `CREATE TABLE`).
fn constraint_sql(c: &EditConstraint) -> String {
    let name = if c.name.trim().is_empty() {
        String::new()
    } else {
        format!("CONSTRAINT {} ", quote_ident(c.name.trim()))
    };
    let extra = if c.extra.trim().is_empty() {
        String::new()
    } else {
        format!(" {}", c.extra.trim())
    };
    match c.kind {
        ConstraintKind::Primary => {
            format!("{name}PRIMARY KEY ({}){extra}", columns_sql(&c.columns))
        }
        ConstraintKind::Unique => format!("{name}UNIQUE ({}){extra}", columns_sql(&c.columns)),
        ConstraintKind::Foreign => {
            let referenced = if list(&c.ref_columns).is_empty() {
                String::new()
            } else {
                format!(" ({})", columns_sql(&c.ref_columns))
            };
            let action = if c.on_delete == ON_DELETE[0] {
                String::new()
            } else {
                format!(" ON DELETE {}", c.on_delete)
            };
            format!(
                "{name}FOREIGN KEY ({}) REFERENCES {}{referenced}{action}{extra}",
                columns_sql(&c.columns),
                table_sql(&c.reference)
            )
        }
        ConstraintKind::Check => format!("{name}CHECK ({}){extra}", c.expr.trim()),
        ConstraintKind::Other => name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn column(name: &str, kind: &str, nullable: bool, default: Option<&str>) -> ColumnDef {
        ColumnDef {
            name: name.into(),
            kind: kind.into(),
            nullable,
            default: default.map(String::from),
            identity: String::new(),
            generated: false,
            comment: String::new(),
        }
    }

    fn designer() -> Designer {
        let mut d = Designer::new(1, "shop".into(), "public".into(), "items".into());
        d.load(Structure {
            columns: vec![
                column("id", "integer", false, Some("nextval('items_id_seq'::regclass)")),
                column("label", "text", false, None),
                column("score", "numeric(8,2)", true, None),
                column("active", "boolean", true, Some("true")),
            ],
            constraints: vec![
                ConstraintDef {
                    name: "items_pkey".into(),
                    kind: ConstraintKind::Primary,
                    definition: "PRIMARY KEY (id)".into(),
                },
                ConstraintDef {
                    name: "items_score_check".into(),
                    kind: ConstraintKind::Check,
                    definition: "CHECK ((score >= (0)::numeric))".into(),
                },
                ConstraintDef {
                    name: "items_owner_fkey".into(),
                    kind: ConstraintKind::Foreign,
                    definition:
                        "FOREIGN KEY (label) REFERENCES customers(name) ON UPDATE CASCADE ON DELETE SET NULL DEFERRABLE"
                            .into(),
                },
            ],
            indexes: vec![IndexDef {
                name: "items_label_idx".into(),
                definition: "CREATE INDEX items_label_idx ON public.items USING btree (label) WHERE (score > 0)".into(),
                unique: false,
            }],
            comment: "the items".into(),
            ..Default::default()
        });
        d
    }

    #[test]
    fn a_renamed_column_is_followed_in_what_is_made_again() {
        let mut d = designer();
        d.columns[1].name = "title".into();
        d.indexes[0].unique = true;
        d.constraints[2].on_delete = "CASCADE".into();
        let sql = d.statements().join("\n");
        assert!(
            sql.contains("RENAME COLUMN \"label\" TO \"title\""),
            "{sql}"
        );
        assert!(sql.contains("(title)"), "{sql}");
        assert!(!sql.contains("(label)"), "{sql}");
        assert_eq!(
            rename_columns("a, \"b c\" > 'a'", &[("a", "b"), ("b c", "d e")]),
            "b, \"d e\" > 'a'"
        );
    }

    #[test]
    fn nothing_edited_changes_nothing() {
        let d = designer();
        assert!(d.statements().is_empty());
        assert!(!d.has_changes());
        assert_eq!(d.problem(), None);
        assert_eq!(d.columns.len(), 4);
        assert_eq!(d.load, Load::Ready);
        // Every definition was understood: all can be edited.
        assert!(d.constraints.iter().all(EditConstraint::editable));
        assert!(d.indexes.iter().all(EditIndex::editable));
    }

    #[test]
    fn the_definitions_of_the_database_are_read_into_fields() {
        let p =
            parse_constraint(ConstraintKind::Primary, "PRIMARY KEY (id, \"Order Id\")").unwrap();
        assert_eq!(p.columns, "id, \"Order Id\"");
        let u = parse_constraint(ConstraintKind::Unique, "UNIQUE (a, b) DEFERRABLE").unwrap();
        assert_eq!(
            (u.columns.as_str(), u.extra.as_str()),
            ("a, b", "DEFERRABLE")
        );
        let c = parse_constraint(
            ConstraintKind::Check,
            "CHECK ((score >= (0)::numeric)) NOT VALID",
        )
        .unwrap();
        assert_eq!(c.expr, "score >= (0)::numeric");
        assert_eq!(c.extra, "NOT VALID");
        let f = parse_constraint(
            ConstraintKind::Foreign,
            "FOREIGN KEY (owner) REFERENCES sales.\"Order Items\"(id) ON UPDATE CASCADE ON DELETE SET NULL DEFERRABLE",
        )
        .unwrap();
        assert_eq!(f.columns, "owner");
        assert_eq!(f.reference, "sales.\"Order Items\"");
        assert_eq!(f.ref_columns, "id");
        assert_eq!(f.on_delete, "SET NULL");
        assert_eq!(f.extra, "ON UPDATE CASCADE DEFERRABLE");
        let plain =
            parse_constraint(ConstraintKind::Foreign, "FOREIGN KEY (a) REFERENCES t(b)").unwrap();
        assert_eq!(plain.on_delete, "NO ACTION");
        assert_eq!(plain.extra, "");
        assert!(
            parse_constraint(ConstraintKind::Other, "EXCLUDE USING gist (a WITH &&)").is_none()
        );
        let i =
            parse_index("CREATE UNIQUE INDEX i ON public.t USING gin (lower(a), b) WHERE (b > 0)")
                .unwrap();
        assert!(i.unique);
        assert_eq!(
            (i.method.as_str(), i.columns.as_str()),
            ("gin", "lower(a), b")
        );
        assert_eq!(i.extra, "WHERE (b > 0)");
    }

    #[test]
    fn a_constraint_that_changed_is_dropped_and_made_again_with_what_it_had() {
        let mut d = designer();
        // The foreign key now points to another column and deletes in cascade.
        d.constraints[2].ref_columns = "email".into();
        d.constraints[2].on_delete = "CASCADE".into();
        assert!(d.constraints[2].modified());
        assert_eq!(
            d.statements(),
            vec![
                "ALTER TABLE \"public\".\"items\" DROP CONSTRAINT \"items_owner_fkey\"",
                "ALTER TABLE \"public\".\"items\" ADD CONSTRAINT \"items_owner_fkey\" FOREIGN KEY (label) REFERENCES customers (email) ON DELETE CASCADE ON UPDATE CASCADE DEFERRABLE",
            ]
        );
        // The columns of a key.
        let mut d = designer();
        d.constraints[0].columns = "id, label".into();
        assert_eq!(
            d.statements(),
            vec![
                "ALTER TABLE \"public\".\"items\" DROP CONSTRAINT \"items_pkey\"",
                "ALTER TABLE \"public\".\"items\" ADD CONSTRAINT \"items_pkey\" PRIMARY KEY (id, label)",
            ]
        );
        // A check.
        let mut d = designer();
        d.constraints[1].expr = "score >= 1".into();
        assert!(d.statements()[1].ends_with("CHECK (score >= 1)"));
    }

    #[test]
    fn a_constraint_or_an_index_that_is_only_renamed_is_renamed() {
        let mut d = designer();
        d.constraints[1].name = "items_score_positive".into();
        d.indexes[0].name = "items_by_label".into();
        assert_eq!(
            d.statements(),
            vec![
                "ALTER TABLE \"public\".\"items\" RENAME CONSTRAINT \"items_score_check\" TO \"items_score_positive\"",
                "ALTER INDEX \"public\".\"items_label_idx\" RENAME TO \"items_by_label\"",
            ]
        );
    }

    #[test]
    fn an_index_that_changed_is_made_again_with_its_method_and_its_predicate() {
        let mut d = designer();
        d.indexes[0].columns = "label, score".into();
        d.indexes[0].unique = true;
        assert_eq!(
            d.statements(),
            vec![
                "DROP INDEX \"public\".\"items_label_idx\"",
                "CREATE UNIQUE INDEX \"items_label_idx\" ON \"public\".\"items\" (label, score) WHERE (score > 0)",
            ]
        );
        // An expression stays as it was typed.
        d.indexes[0].columns = "lower(label), score DESC".into();
        assert!(d.statements()[1].contains("(lower(label), score DESC)"));
    }

    #[test]
    fn a_dropped_constraint_that_was_also_changed_is_only_dropped() {
        let mut d = designer();
        d.constraints[2].ref_columns = "email".into();
        d.constraints[2].dropped = true;
        assert_eq!(
            d.statements(),
            vec!["ALTER TABLE \"public\".\"items\" DROP CONSTRAINT \"items_owner_fkey\""]
        );
    }

    #[test]
    fn a_change_of_type_can_say_how_the_values_are_converted() {
        let mut d = designer();
        d.columns[2].kind = "integer".into();
        assert!(d.columns[2].retyped());
        assert!(d.statements()[0].ends_with("USING \"score\"::integer"));
        d.columns[2].using = "round(score)::integer".into();
        assert!(d.statements()[0].ends_with("USING round(score)::integer"));
        // A name that is not changed: no USING of its own.
        let mut d = designer();
        d.columns[1].kind = "varchar(30)".into();
        assert!(d.statements()[0].contains("TYPE varchar(30) USING \"label\"::varchar(30)"));
    }

    #[test]
    fn a_table_moves_to_another_schema_then_is_renamed_there() {
        let mut d = designer();
        d.schema_to = "archive".into();
        assert!(d.moved());
        assert_eq!(
            d.statements(),
            vec!["ALTER TABLE \"public\".\"items\" SET SCHEMA \"archive\""]
        );
        d.name = "old_items".into();
        assert_eq!(
            d.statements(),
            vec![
                "ALTER TABLE \"public\".\"items\" SET SCHEMA \"archive\"",
                "ALTER TABLE \"archive\".\"items\" RENAME TO \"old_items\"",
            ]
        );
        d.schema_to = " ".into();
        assert_eq!(d.problem(), Some(Problem::EmptySchema));
    }

    #[test]
    fn a_new_table_is_made_with_its_columns_keys_indexes_and_comments() {
        let mut d = Designer::new(1, "shop".into(), "public".into(), String::new());
        d = Designer::for_new_table(d.conn, d.database.clone(), "public".into());
        assert!(d.creating && d.load == Load::Ready);
        assert_eq!(d.problem(), Some(Problem::EmptyTableName));
        d.name = "orders".into();
        d.add_column();
        let c = d.columns.last_mut().unwrap();
        c.name = "total".into();
        c.kind = "numeric(10,2)".into();
        c.nullable = false;
        c.default = "0".into();
        c.comment = "what is due".into();
        d.add_constraint(ConstraintKind::Check);
        d.constraints.last_mut().unwrap().expr = "total >= 0".into();
        d.add_index();
        d.indexes.last_mut().unwrap().columns = "total".into();
        d.comment = "the orders".into();
        assert_eq!(d.problem(), None);
        assert_eq!(
            d.statements(),
            vec![
                "CREATE TABLE \"public\".\"orders\" (\n  \"id\" serial NOT NULL,\n  \"total\" numeric(10,2) NOT NULL DEFAULT 0,\n  PRIMARY KEY (id),\n  CHECK (total >= 0)\n)",
                "CREATE INDEX ON \"public\".\"orders\" (total)",
                "COMMENT ON TABLE \"public\".\"orders\" IS 'the orders'",
                "COMMENT ON COLUMN \"public\".\"orders\".\"total\" IS 'what is due'",
            ]
        );
        // The schema is chosen in the form.
        d.schema_to = "sales".into();
        assert!(d.statements()[0].starts_with("CREATE TABLE \"sales\".\"orders\""));
        // No columns, no table.
        for c in &mut d.columns {
            c.dropped = true;
        }
        assert_eq!(d.problem(), Some(Problem::NoColumns));
    }

    #[test]
    fn a_column_is_added_with_its_type_default_and_comment() {
        let mut d = designer();
        d.add_column();
        let c = d.columns.last_mut().unwrap();
        c.name = "created".into();
        c.kind = "timestamptz".into();
        c.nullable = false;
        c.default = "now()".into();
        c.comment = "when it was made".into();
        assert_eq!(
            d.statements(),
            vec![
                "ALTER TABLE \"public\".\"items\" ADD COLUMN \"created\" timestamptz NOT NULL DEFAULT now()".to_string(),
                "COMMENT ON COLUMN \"public\".\"items\".\"created\" IS 'when it was made'".to_string(),
            ]
        );
    }

    #[test]
    fn a_column_is_renamed_retyped_and_made_not_null_in_the_right_order() {
        let mut d = designer();
        let c = &mut d.columns[2];
        c.name = "points".into();
        c.kind = "integer".into();
        c.nullable = false;
        c.default = "0".into();
        assert_eq!(
            d.statements(),
            vec![
                "ALTER TABLE \"public\".\"items\" RENAME COLUMN \"score\" TO \"points\"",
                "ALTER TABLE \"public\".\"items\" ALTER COLUMN \"points\" TYPE integer USING \"points\"::integer",
                "ALTER TABLE \"public\".\"items\" ALTER COLUMN \"points\" SET NOT NULL",
                "ALTER TABLE \"public\".\"items\" ALTER COLUMN \"points\" SET DEFAULT 0",
            ]
        );
    }

    #[test]
    fn a_default_is_taken_off_before_a_retype_and_put_back_after() {
        let mut d = designer();
        d.columns[3].kind = "text".into();
        d.columns[3].default = "'yes'".into();
        let statements = d.statements();
        assert_eq!(
            statements[0],
            "ALTER TABLE \"public\".\"items\" ALTER COLUMN \"active\" DROP DEFAULT"
        );
        assert!(statements[1].contains("TYPE text USING \"active\"::text"));
        assert_eq!(
            statements[2],
            "ALTER TABLE \"public\".\"items\" ALTER COLUMN \"active\" SET DEFAULT 'yes'"
        );
        let mut d = designer();
        d.columns[3].kind = "text".into();
        assert_eq!(d.statements().len(), 3);
        let mut d = designer();
        d.columns[3].kind = "text".into();
        d.columns[3].default.clear();
        assert_eq!(d.statements().len(), 2);
    }

    #[test]
    fn a_default_is_set_changed_and_dropped() {
        let mut d = designer();
        d.columns[3].default = "false".into();
        assert_eq!(
            d.statements(),
            vec!["ALTER TABLE \"public\".\"items\" ALTER COLUMN \"active\" SET DEFAULT false"]
        );
        d.columns[3].default.clear();
        assert_eq!(
            d.statements(),
            vec!["ALTER TABLE \"public\".\"items\" ALTER COLUMN \"active\" DROP DEFAULT"]
        );
    }

    #[test]
    fn a_column_is_dropped_and_a_dropped_new_one_is_nothing() {
        let mut d = designer();
        d.columns[2].dropped = true;
        d.add_column();
        d.columns.last_mut().unwrap().name = "x".into();
        d.columns.last_mut().unwrap().dropped = true;
        assert_eq!(
            d.statements(),
            vec!["ALTER TABLE \"public\".\"items\" DROP COLUMN \"score\""]
        );
        d.columns[2].name = String::new();
        assert_eq!(d.problem(), None);
    }

    #[test]
    fn constraints_and_indexes_are_dropped_first_and_added_after_the_columns() {
        let mut d = designer();
        d.constraints[1].dropped = true;
        d.constraints[2].dropped = true;
        d.indexes[0].dropped = true;
        d.add_column();
        let c = d.columns.last_mut().unwrap();
        c.name = "owner".into();
        c.kind = "integer".into();
        d.add_constraint(ConstraintKind::Foreign);
        let f = d.constraints.last_mut().unwrap();
        f.name = "items_owner_fk".into();
        f.columns = "owner".into();
        f.reference = "public.customers".into();
        f.ref_columns = "id".into();
        f.on_delete = "CASCADE".into();
        d.add_constraint(ConstraintKind::Unique);
        d.constraints.last_mut().unwrap().columns = "label, owner".into();
        d.add_constraint(ConstraintKind::Check);
        d.constraints.last_mut().unwrap().expr = "score < 1000".into();
        d.add_index();
        let i = d.indexes.last_mut().unwrap();
        i.columns = "owner".into();
        i.unique = true;
        i.name = "items_owner_idx".into();
        assert_eq!(
            d.statements(),
            vec![
                "ALTER TABLE \"public\".\"items\" DROP CONSTRAINT \"items_score_check\"",
                "ALTER TABLE \"public\".\"items\" DROP CONSTRAINT \"items_owner_fkey\"",
                "DROP INDEX \"public\".\"items_label_idx\"",
                "ALTER TABLE \"public\".\"items\" ADD COLUMN \"owner\" integer",
                "ALTER TABLE \"public\".\"items\" ADD CONSTRAINT \"items_owner_fk\" FOREIGN KEY (owner) REFERENCES public.customers (id) ON DELETE CASCADE",
                "ALTER TABLE \"public\".\"items\" ADD UNIQUE (label, owner)",
                "ALTER TABLE \"public\".\"items\" ADD CHECK (score < 1000)",
                "CREATE UNIQUE INDEX \"items_owner_idx\" ON \"public\".\"items\" (owner)",
            ]
        );
        assert_eq!(d.problem(), None);
    }

    #[test]
    fn the_primary_key_is_replaced_by_dropping_then_adding() {
        let mut d = designer();
        d.constraints[0].dropped = true;
        d.add_constraint(ConstraintKind::Primary);
        d.constraints.last_mut().unwrap().columns = "id, label".into();
        let statements = d.statements();
        assert!(statements[0].contains("DROP CONSTRAINT \"items_pkey\""));
        assert!(statements[1].ends_with("ADD PRIMARY KEY (id, label)"));
        assert_eq!(d.problem(), None);
        d.constraints[0].dropped = false;
        assert_eq!(d.problem(), Some(Problem::SeveralKeys));
    }

    #[test]
    fn the_table_comment_and_name_come_last() {
        let mut d = designer();
        d.comment = "things".into();
        d.name = "things".into();
        assert_eq!(
            d.statements(),
            vec![
                "COMMENT ON TABLE \"public\".\"items\" IS 'things'",
                "ALTER TABLE \"public\".\"items\" RENAME TO \"things\"",
            ]
        );
        d.comment.clear();
        assert!(d.statements()[0].ends_with("IS NULL"));
    }

    #[test]
    fn what_is_wrong_is_told_before_anything_runs() {
        let mut d = designer();
        d.name = "  ".into();
        assert_eq!(d.problem(), Some(Problem::EmptyTableName));
        d.name = "items".into();
        d.add_column();
        assert_eq!(d.problem(), Some(Problem::EmptyColumnName));
        d.columns.last_mut().unwrap().name = "label".into();
        assert_eq!(d.problem(), Some(Problem::DuplicateColumn("label".into())));
        d.columns.last_mut().unwrap().name = "x".into();
        d.columns.last_mut().unwrap().kind = " ".into();
        assert_eq!(d.problem(), Some(Problem::EmptyType("x".into())));
        d.columns.last_mut().unwrap().kind = "int".into();
        d.add_constraint(ConstraintKind::Unique);
        assert_eq!(d.problem(), Some(Problem::NeedsColumns));
        d.constraints.last_mut().unwrap().columns = "x".into();
        d.add_constraint(ConstraintKind::Foreign);
        d.constraints.last_mut().unwrap().columns = "x".into();
        assert_eq!(d.problem(), Some(Problem::NeedsReference));
        d.constraints.last_mut().unwrap().reference = "customers".into();
        d.add_constraint(ConstraintKind::Check);
        assert_eq!(d.problem(), Some(Problem::NeedsExpression));
        d.constraints.last_mut().unwrap().expr = "x > 0".into();
        d.add_index();
        assert_eq!(d.problem(), Some(Problem::NeedsColumns));
        d.indexes.last_mut().unwrap().columns = "x".into();
        assert_eq!(d.problem(), None);
        // An existing constraint emptied of its columns is refused too.
        d.constraints[0].columns.clear();
        assert_eq!(d.problem(), Some(Problem::NeedsColumns));
    }

    #[test]
    fn names_that_need_quotes_get_them_and_the_others_do_not() {
        let mut d = designer();
        d.add_column();
        let c = d.columns.last_mut().unwrap();
        c.name = "Order Id".into();
        c.kind = "int".into();
        d.add_constraint(ConstraintKind::Unique);
        d.constraints.last_mut().unwrap().columns = "Order Id, label".into();
        d.add_constraint(ConstraintKind::Foreign);
        let f = d.constraints.last_mut().unwrap();
        f.columns = "Order Id".into();
        f.reference = "sales.\"Order Items\"".into();
        let statements = d.statements();
        assert!(statements[1].ends_with("UNIQUE (\"Order Id\", label)"));
        assert!(statements[2].ends_with("REFERENCES sales.\"Order Items\""));
    }

    #[test]
    fn a_comment_of_a_column_is_changed_or_taken_off() {
        let mut d = designer();
        d.columns[1].comment = "it's a label".into();
        assert_eq!(
            d.statements(),
            vec!["COMMENT ON COLUMN \"public\".\"items\".\"label\" IS 'it''s a label'"]
        );
        d.columns[1].comment.clear();
        assert!(d.statements().is_empty());
    }
}
