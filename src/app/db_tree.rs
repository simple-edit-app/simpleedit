//! What the tree of the data sources and the grid add to the base: the
//! structure of a table in the tree, its DDL, a new table or schema, the rows
//! a row is linked to, a value shown whole, the width of a column.

use iced::Task;

use super::{Message, SimpleEditApp};
use crate::database::designer::Designer;
use crate::database::{
    self, ddl, AlterKind, DbMsg, NewSchema, Resizing, TableKey, TableRef, TextView,
};

/// A column cannot be made narrower, or wider, than this.
const MIN_WIDTH: f32 = 40.0;
const MAX_WIDTH: f32 = 1600.0;

fn key_name(key: &TableKey) -> String {
    if key.2 == "public" {
        key.3.clone()
    } else {
        format!("{}.{}", key.2, key.3)
    }
}

/// A value for the viewer: JSON is indented, the rest is as it is.
pub(super) fn readable(value: &str) -> String {
    let trimmed = value.trim_start();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(value) {
            if let Ok(pretty) = serde_json::to_string_pretty(&json) {
                return pretty;
            }
        }
    }
    value.to_string()
}

impl SimpleEditApp {
    /// Reads the structure of a table (for the tree, the DDL, the links).
    pub(super) fn db_read_structure(&mut self, key: TableKey, show: bool) -> Task<Message> {
        let Some(params) = self.db_params(key.0, false) else {
            return Task::none();
        };
        let table = TableRef {
            database: key.1.clone(),
            schema: key.2.clone(),
            table: key.3.clone(),
        };
        Task::perform(database::structure(params, table), move |done| {
            Message::Db(DbMsg::StructureRead(key.clone(), show, done))
        })
    }

    pub(super) fn db_tree_message(&mut self, msg: DbMsg) -> Task<Message> {
        match msg {
            DbMsg::NewTable(conn, database, schema) => {
                self.db.designer = Some(Designer::for_new_table(conn, database, schema));
                Task::none()
            }
            DbMsg::ShowDiagram(conn, database, schema) => {
                let Some(params) = self.db_params(conn, false) else {
                    return self.db_ask_password(conn);
                };
                Task::perform(database::table_columns(params, database), move |done| {
                    Message::Db(DbMsg::DiagramLoaded(schema.clone(), done))
                })
            }
            DbMsg::DiagramLoaded(schema, done) => {
                match done.result {
                    Ok(catalog) => {
                        let diagram = database::diagram::build(&catalog, &schema);
                        if diagram.tables.is_empty() {
                            self.set_status(t!("db.diagram_empty"), false);
                        } else {
                            self.overlay = crate::app::ActiveOverlay::Diagram(Box::new(
                                crate::app::DiagramView { schema, diagram },
                            ));
                        }
                    }
                    Err(error) => self.set_status(error, true),
                }
                Task::none()
            }
            DbMsg::CopyDiagram => {
                let text = match &self.overlay {
                    crate::app::ActiveOverlay::Diagram(view) => {
                        database::diagram::to_mermaid(&view.diagram)
                    }
                    _ => return Task::none(),
                };
                self.set_status(t!("db.diagram_copied"), false);
                iced::clipboard::write(text)
            }
            DbMsg::NewSchema(conn, database) => {
                self.db.new_schema = Some(NewSchema {
                    conn,
                    database,
                    name: String::new(),
                    running: false,
                    error: None,
                });
                Task::none()
            }
            DbMsg::SchemaName(name) => {
                if let Some(n) = self.db.new_schema.as_mut().filter(|n| !n.running) {
                    n.name = name;
                    n.error = None;
                }
                Task::none()
            }
            DbMsg::SchemaClose => {
                if self.db.new_schema.as_ref().is_some_and(|n| !n.running) {
                    self.db.new_schema = None;
                }
                Task::none()
            }
            DbMsg::SchemaCreate => {
                let Some(n) = self.db.new_schema.as_mut() else {
                    return Task::none();
                };
                if n.running || n.name.trim().is_empty() {
                    return Task::none();
                }
                n.running = true;
                n.error = None;
                let statement = format!(
                    "CREATE SCHEMA {}",
                    database::protocol::quote_ident(n.name.trim())
                );
                let target = (n.conn, n.database.clone(), n.name.trim().to_string());
                self.admin_run_schema(target, statement)
            }
            DbMsg::ToggleTable(conn, database, schema, table) => {
                let key = format!("{conn}/{database}/{schema}/{table}");
                if self.db.open.remove(&key) {
                    return Task::none();
                }
                self.db.open.insert(key);
                let table_key = (conn, database, schema, table);
                if self.db.structures.contains_key(&table_key) {
                    return Task::none();
                }
                self.db_read_structure(table_key, false)
            }
            DbMsg::StructureRead(key, show, done) => {
                let log = self.db_log(key.0, &done.executed);
                match done.result {
                    Ok(structure) => {
                        if show {
                            self.db.text = Some(TextView {
                                title: key_name(&key),
                                text: ddl::table_ddl(&key.2, &key.3, &structure),
                            });
                        }
                        for tab in self.db_tabs.values_mut() {
                            if (tab.conn, &tab.database, &tab.schema, &tab.table)
                                == (key.0, &key.1, &key.2, &key.3)
                            {
                                tab.structure = Some(structure.clone());
                            }
                        }
                        self.db.structures.insert(key, structure);
                    }
                    Err(error) => {
                        if show {
                            self.db.text = Some(TextView {
                                title: key_name(&key),
                                text: error,
                            });
                        }
                    }
                }
                log
            }
            DbMsg::ShowDdl(conn, database, schema, table) => {
                let key = (conn, database, schema, table);
                if let Some(structure) = self.db.structures.get(&key) {
                    self.db.text = Some(TextView {
                        title: key_name(&key),
                        text: ddl::table_ddl(&key.2, &key.3, structure),
                    });
                    return Task::none();
                }
                self.db_read_structure(key, true)
            }
            DbMsg::ShowDefinition(conn, database, schema, name, kind) => {
                let Some(params) = self.db_params(conn, false) else {
                    return self.db_ask_password(conn);
                };
                let table = database::TableRef {
                    database: database.clone(),
                    schema: schema.clone(),
                    table: name.clone(),
                };
                Task::perform(database::definition(params, table, kind), move |done| {
                    Message::Db(DbMsg::Definition(
                        conn,
                        database.clone(),
                        schema.clone(),
                        name.clone(),
                        done,
                    ))
                })
            }
            DbMsg::ShowFunction(conn, database, schema, name, args) => {
                let Some(params) = self.db_params(conn, false) else {
                    return self.db_ask_password(conn);
                };
                let shown = (database.clone(), schema.clone(), name.clone());
                Task::perform(
                    database::function_definition(params, database, schema, name, args),
                    move |done| {
                        Message::Db(DbMsg::Definition(
                            conn,
                            shown.0.clone(),
                            shown.1.clone(),
                            shown.2.clone(),
                            done,
                        ))
                    },
                )
            }
            DbMsg::Definition(conn, _database, schema, name, done) => {
                let log = self.db_log(conn, &done.executed);
                self.db.text = Some(TextView {
                    title: format!("{schema}.{name}"),
                    text: match done.result {
                        Ok(text) => text,
                        Err(error) => error,
                    },
                });
                log
            }
            DbMsg::TreeFilter(text) => {
                self.db.tree_filter = text;
                Task::none()
            }
            DbMsg::ViewValue(path, row, col) => {
                if let Some(tab) = self.db_tabs.get(&path) {
                    if let Some(page) = &tab.page {
                        let title = page
                            .columns
                            .get(col)
                            .map(|c| c.name.clone())
                            .unwrap_or_default();
                        let text = tab
                            .pending
                            .value(page, row, col)
                            .map(readable)
                            .unwrap_or_default();
                        self.db.text = Some(TextView { title, text });
                    }
                }
                Task::none()
            }
            DbMsg::CopyRow(path, row) => {
                let Some(tab) = self.db_tabs.get(&path) else {
                    return Task::none();
                };
                let Some(page) = &tab.page else {
                    return Task::none();
                };
                let line: Vec<String> = (0..page.columns.len())
                    .map(|c| tab.pending.value(page, row, c).unwrap_or("").to_string())
                    .collect();
                iced::clipboard::write(line.join("\t"))
            }
            DbMsg::CopyCell(path, row, col) => {
                let value = self.db_tabs.get(&path).and_then(|tab| {
                    let page = tab.page.as_ref()?;
                    tab.pending.value(page, row, col).map(str::to_string)
                });
                match value {
                    Some(value) => iced::clipboard::write(value),
                    None => Task::none(),
                }
            }
            DbMsg::CopySelection(path, format) => {
                let Some(tab) = self.db_tabs.get(&path) else {
                    return Task::none();
                };
                let Some(page) = &tab.page else {
                    return Task::none();
                };
                let Some(cell) = tab.selected else {
                    return Task::none();
                };
                let (from, to) = tab.block().unwrap_or((cell, cell));
                let table = tab.table_ref();
                let name = format!(
                    "{}.{}",
                    database::protocol::quote_ident_in(tab.dialect, &table.schema),
                    database::protocol::quote_ident_in(tab.dialect, &table.table)
                );
                iced::clipboard::write(database::grid::block_text(
                    page,
                    &tab.pending,
                    from,
                    to,
                    format,
                    &name,
                    tab.dialect,
                ))
            }
            DbMsg::TextCopy => match &self.db.text {
                Some(view) => iced::clipboard::write(view.text.clone()),
                None => Task::none(),
            },
            DbMsg::TextClose => {
                self.db.text = None;
                Task::none()
            }
            DbMsg::ShowLinks(view) => {
                self.db.links = Some(*view);
                Task::none()
            }
            DbMsg::CloseLinks => {
                self.db.links = None;
                Task::none()
            }
            DbMsg::OpenRelated(conn, database, schema, table, filter) => {
                self.db.links = None;
                self.db_open_table_with(conn, database, schema, table, Some(filter))
            }
            DbMsg::ResizeStart(path, col) => {
                self.db.resizing = Some(Resizing {
                    path,
                    col,
                    anchor: None,
                });
                Task::none()
            }
            DbMsg::ResizeMove(x) => {
                let Some(r) = self.db.resizing.as_mut() else {
                    return Task::none();
                };
                let Some(tab) = self.db_tabs.get_mut(&r.path) else {
                    return Task::none();
                };
                let Some(&width) = tab.widths.get(r.col) else {
                    return Task::none();
                };
                let (x0, w0) = *r.anchor.get_or_insert((x, width));
                let wanted = (w0 + x - x0).clamp(MIN_WIDTH, MAX_WIDTH);
                tab.widths[r.col] = wanted;
                if let Some(name) = tab
                    .page
                    .as_ref()
                    .and_then(|p| p.columns.get(r.col))
                    .map(|c| c.name.clone())
                {
                    tab.custom_widths.insert(name, wanted);
                }
                Task::none()
            }
            DbMsg::ResizeEnd => {
                self.db.resizing = None;
                Task::none()
            }
            _ => Task::none(),
        }
    }

    /// Runs `CREATE SCHEMA`.
    fn admin_run_schema(
        &mut self,
        target: (u64, String, String),
        statement: String,
    ) -> Task<Message> {
        let (conn, database, name) = target;
        let Some(params) = self.db_params(conn, false) else {
            if let Some(n) = self.db.new_schema.as_mut() {
                n.running = false;
            }
            return self.db_ask_password(conn);
        };
        let target_db = database.clone();
        Task::perform(
            database::run_statements_in(params, target_db, vec![statement]),
            move |done| {
                Message::Db(DbMsg::Altered(Box::new(database::Altered {
                    what: AlterKind::Schema,
                    conn,
                    database: database.clone(),
                    schema: name.clone(),
                    table: String::new(),
                    done,
                })))
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::readable;

    #[test]
    fn json_is_indented_and_the_rest_is_left_alone() {
        assert_eq!(
            readable("{\"a\":[1,2]}"),
            "{\n  \"a\": [\n    1,\n    2\n  ]\n}"
        );
        assert_eq!(readable("[1]"), "[\n  1\n]");
        assert_eq!(readable("{not json"), "{not json");
        assert_eq!(readable("plain text"), "plain text");
    }
}
