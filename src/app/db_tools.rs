//! The tools of a database: its health, and what its messages do.

use iced::Task;

use super::{Message, SimpleEditApp};
use crate::database::import::{self, Format};
use crate::database::{self, DbMsg, HealthTab, HealthView, Import, TableRef};

impl SimpleEditApp {
    pub(super) fn db_tools_message(&mut self, msg: DbMsg) -> Task<Message> {
        match msg {
            DbMsg::OpenHealth(conn, database) => {
                self.db.health = Some(HealthView {
                    conn,
                    database,
                    data: None,
                    loading: true,
                    error: None,
                    tab: HealthTab::Tables,
                    running: None,
                });
                self.db_health_load()
            }
            DbMsg::HealthLoaded(done) => {
                let conn = self.db.health.as_ref().map(|h| h.conn);
                let log = conn.map_or_else(Task::none, |c| self.db_log(c, &done.executed));
                if let Some(view) = self.db.health.as_mut() {
                    view.loading = false;
                    match done.result {
                        Ok(data) => {
                            view.data = Some(data);
                            view.error = None;
                        }
                        Err(error) => view.error = Some(error),
                    }
                }
                log
            }
            DbMsg::HealthTab(tab) => {
                if let Some(view) = self.db.health.as_mut() {
                    view.tab = tab;
                }
                Task::none()
            }
            DbMsg::HealthRun(statement) => {
                let Some((conn, database)) = self
                    .db
                    .health
                    .as_ref()
                    .filter(|h| h.running.is_none())
                    .map(|h| (h.conn, h.database.clone()))
                else {
                    return Task::none();
                };
                let Some(params) = self.db_params(conn, false) else {
                    return self.db_ask_password(conn);
                };
                if let Some(view) = self.db.health.as_mut() {
                    view.running = Some(statement.clone());
                    view.error = None;
                }
                Task::perform(
                    database::run_statements_in(params, database, vec![statement]),
                    |done| Message::Db(DbMsg::HealthRan(done)),
                )
            }
            DbMsg::HealthRan(done) => {
                let conn = self.db.health.as_ref().map(|h| h.conn);
                let log = conn.map_or_else(Task::none, |c| self.db_log(c, &done.executed));
                let failure = match &done.result {
                    Ok(ran) => ran.failed.as_ref().map(|(_, e)| e.clone()),
                    Err(e) => Some(e.clone()),
                };
                let Some(view) = self.db.health.as_mut() else {
                    return log;
                };
                view.running = None;
                match failure {
                    // The numbers are read again: what was cleaned shows.
                    None => Task::batch([log, self.db_health_load()]),
                    Some(error) => {
                        view.error = Some(error);
                        log
                    }
                }
            }
            DbMsg::HealthToConsole(sql) => {
                self.db.console.append_query(&sql);
                self.db.console.save();
                self.db.console.tab = database::console::ConsoleTab::Queries;
                self.db.console.open = true;
                self.db.health = None;
                Task::none()
            }
            DbMsg::HealthClose => {
                self.db.health = None;
                Task::none()
            }
            DbMsg::ImportOpen(conn, database, schema, table) => {
                let Some(params) = self.db_params(conn, false) else {
                    return self.db_ask_password(conn);
                };
                self.db.import = Some(Import {
                    conn,
                    database: database.clone(),
                    schema: schema.clone(),
                    table: table.clone(),
                    targets: Vec::new(),
                    path: None,
                    format: Format::Csv,
                    delimiter: ',',
                    header: true,
                    empty_null: true,
                    text: String::new(),
                    sample: import::Sample::default(),
                    mapping: Vec::new(),
                    running: false,
                    error: None,
                    imported: None,
                });
                Task::perform(
                    database::structure(
                        params,
                        TableRef {
                            database,
                            schema,
                            table,
                        },
                    ),
                    |done| Message::Db(DbMsg::ImportStructure(done)),
                )
            }
            DbMsg::ImportStructure(done) => {
                let conn = self.db.import.as_ref().map(|i| i.conn);
                let log = conn.map_or_else(Task::none, |c| self.db_log(c, &done.executed));
                if let Some(im) = self.db.import.as_mut() {
                    match done.result {
                        Ok(structure) => {
                            // A generated column, and an identity that is
                            // always generated, cannot be filled.
                            im.targets = structure
                                .columns
                                .iter()
                                .filter(|c| !c.generated && c.identity != "a")
                                .map(|c| import::Target {
                                    name: c.name.clone(),
                                    kind: c.kind.clone(),
                                })
                                .collect();
                            im.reread();
                        }
                        Err(error) => im.error = Some(error),
                    }
                }
                log
            }
            DbMsg::ImportPick => Task::perform(
                async {
                    rfd::AsyncFileDialog::new()
                        .add_filter("CSV, TSV, JSON", &["csv", "tsv", "txt", "json"])
                        .pick_file()
                        .await
                        .map(|f| f.path().to_path_buf())
                },
                |path| Message::Db(DbMsg::ImportPicked(path)),
            ),
            DbMsg::ImportPicked(path) => {
                let (Some(path), Some(im)) = (path, self.db.import.as_mut()) else {
                    return Task::none();
                };
                im.format = Format::of(&path.to_string_lossy());
                match read_sample(&path, im.format) {
                    Ok(text) => {
                        if im.format == Format::Csv {
                            im.delimiter = import::detect_delimiter(&text);
                        }
                        im.text = text;
                        im.error = None;
                        im.imported = None;
                        im.path = Some(path);
                        im.reread();
                    }
                    Err(error) => im.error = Some(error),
                }
                Task::none()
            }
            DbMsg::ImportFormat(format) => {
                if let Some(im) = self.db.import.as_mut() {
                    im.format = format;
                    im.reread();
                }
                Task::none()
            }
            DbMsg::ImportDelimiter(delimiter) => {
                if let Some(im) = self.db.import.as_mut() {
                    im.delimiter = delimiter;
                    im.reread();
                }
                Task::none()
            }
            DbMsg::ImportHeader(on) => {
                if let Some(im) = self.db.import.as_mut() {
                    im.header = on;
                    im.reread();
                }
                Task::none()
            }
            DbMsg::ImportEmptyNull(on) => {
                if let Some(im) = self.db.import.as_mut() {
                    im.empty_null = on;
                }
                Task::none()
            }
            DbMsg::ImportMap(at, to) => {
                if let Some(im) = self.db.import.as_mut() {
                    // A column is filled by one field only.
                    if let Some(name) = &to {
                        for other in im.mapping.iter_mut() {
                            if other.as_ref() == Some(name) {
                                *other = None;
                            }
                        }
                    }
                    if let Some(slot) = im.mapping.get_mut(at) {
                        *slot = to;
                    }
                }
                Task::none()
            }
            DbMsg::ImportRun => {
                let Some(im) = self.db.import.as_ref().filter(|i| !i.running) else {
                    return Task::none();
                };
                let (Some(path), true) = (im.path.clone(), im.mapping.iter().any(Option::is_some))
                else {
                    return Task::none();
                };
                let spec = import::spec(
                    im.format,
                    im.delimiter,
                    im.header,
                    im.empty_null,
                    &im.sample.names,
                    &im.mapping,
                    &im.targets,
                );
                let (conn, table) = (
                    im.conn,
                    TableRef {
                        database: im.database.clone(),
                        schema: im.schema.clone(),
                        table: im.table.clone(),
                    },
                );
                let Some(params) = self.db_params(conn, false) else {
                    return self.db_ask_password(conn);
                };
                if let Some(im) = self.db.import.as_mut() {
                    im.running = true;
                    im.error = None;
                    im.imported = None;
                }
                Task::perform(
                    database::import_file(params, table, path.to_string_lossy().to_string(), spec),
                    |done| Message::Db(DbMsg::ImportDone(done)),
                )
            }
            DbMsg::ImportDone(done) => {
                let Some(im) = self.db.import.as_mut() else {
                    return Task::none();
                };
                im.running = false;
                let (conn, schema, table) = (im.conn, im.schema.clone(), im.table.clone());
                let log = self.db_log(conn, &done.executed);
                let Some(im) = self.db.import.as_mut() else {
                    return log;
                };
                match done.result {
                    Ok(n) => {
                        im.imported = Some(n);
                        // The tabs of the table show the new rows.
                        let tabs: Vec<_> = self
                            .db_tabs
                            .iter()
                            .filter(|(_, t)| {
                                t.conn == conn
                                    && t.schema == schema
                                    && t.table == table
                                    && t.query.is_none()
                                    && t.pending.is_empty()
                            })
                            .map(|(p, _)| p.clone())
                            .collect();
                        let reloads: Vec<_> = tabs
                            .into_iter()
                            .map(|p| self.db_message(DbMsg::Refresh(p)))
                            .collect();
                        Task::batch(std::iter::once(log).chain(reloads))
                    }
                    Err(error) => {
                        im.error = Some(error);
                        log
                    }
                }
            }
            DbMsg::ImportClose => {
                if self.db.import.as_ref().is_some_and(|i| !i.running) {
                    self.db.import = None;
                }
                Task::none()
            }
            _ => Task::none(),
        }
    }

    /// Reads the numbers of the database the health window is about.
    fn db_health_load(&mut self) -> Task<Message> {
        let Some((conn, database)) = self
            .db
            .health
            .as_ref()
            .map(|h| (h.conn, h.database.clone()))
        else {
            return Task::none();
        };
        let Some(params) = self.db_params(conn, false) else {
            self.db.health = None;
            return self.db_ask_password(conn);
        };
        if let Some(view) = self.db.health.as_mut() {
            view.loading = true;
        }
        Task::perform(database::health(params, database), |done| {
            Message::Db(DbMsg::HealthLoaded(done))
        })
    }
}

/// What is read of a file to show it: the start of a CSV, all of a JSON (its
/// keys are in all of it).
fn read_sample(path: &std::path::Path, format: Format) -> Result<String, String> {
    use std::io::Read;
    let limit = if format == Format::Json {
        64 * 1024 * 1024
    } else {
        import::SAMPLE_BYTES
    };
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut bytes = Vec::new();
    file.take(limit as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}
