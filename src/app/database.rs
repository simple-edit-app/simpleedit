//! The Database panel inside the app: what its messages do.

use std::path::PathBuf;

use iced::Task;

use super::{Message, SimpleEditApp};
use crate::database::{self, DbMsg, Dialog, Driver, Field, Params, TableTab, TestState, Tree};

impl SimpleEditApp {
    /// Finds out whether the driver is installed.
    pub(super) fn db_probe_driver(&mut self) -> Task<Message> {
        Task::perform(
            database::probe_driver(self.db.driver_path.clone()),
            |driver| Message::Db(DbMsg::DriverProbed(driver)),
        )
    }

    /// The password of a connection that asks every time serves once.
    fn db_params(&mut self, id: u64, consume: bool) -> Option<Params> {
        let params = self.db.params(id)?;
        if consume {
            self.db.once.remove(&id);
        }
        Some(params)
    }

    /// Loads the databases, schemas and tables of a connection.
    fn db_load_tree(&mut self, id: u64) -> Task<Message> {
        let Some(params) = self.db_params(id, true) else {
            return self.db_ask_password(id);
        };
        self.db.trees.insert(id, Tree::Loading);
        Task::perform(database::tree(params), move |result| {
            Message::Db(DbMsg::TreeLoaded(id, result))
        })
    }

    /// Opens the form of a connection whose password is not known yet.
    fn db_ask_password(&mut self, id: u64) -> Task<Message> {
        if let Some(connection) = self.db.connection(id).cloned() {
            let mut form = Dialog::edit(&connection, String::new(), self.db.driver_path.clone());
            form.needs_password = true;
            self.db.dialog = Some(form);
        }
        if self.db.driver == Driver::Unknown {
            return self.db_probe_driver();
        }
        Task::none()
    }

    fn db_load_rows(&mut self, path: PathBuf, offset: u64) -> Task<Message> {
        let Some(tab) = self.db_tabs.get(&path) else {
            return Task::none();
        };
        let (conn, database, schema, table) = (
            tab.conn,
            tab.database.clone(),
            tab.schema.clone(),
            tab.table.clone(),
        );
        let Some(params) = self.db_params(conn, false) else {
            // The tab says why it is empty; the form asks for the password.
            if let Some(tab) = self.db_tabs.get_mut(&path) {
                tab.loading = false;
                tab.error = Some(t!("db.password_needed").to_string());
            }
            return self.db_ask_password(conn);
        };
        if let Some(tab) = self.db_tabs.get_mut(&path) {
            tab.loading = true;
            tab.error = None;
        }
        Task::perform(
            database::rows(params, database, schema, table, offset),
            move |result| Message::Db(DbMsg::Rows(path.clone(), result)),
        )
    }

    /// Opens a table in a tab, or goes to the one that shows it.
    fn db_open_table(
        &mut self,
        conn: u64,
        database: String,
        schema: String,
        table: String,
    ) -> Task<Message> {
        let title = if schema == "public" {
            table.clone()
        } else {
            format!("{schema}.{table}")
        };
        let existing = self
            .db_tabs
            .iter()
            .find(|(_, t)| {
                t.conn == conn && t.database == database && t.schema == schema && t.table == table
            })
            .map(|(p, _)| p.clone());
        if let Some(path) = existing {
            return self.activate(path);
        }
        self.db_counter += 1;
        let path = PathBuf::from(format!("db://{}/{}", self.db_counter, title));
        self.db_tabs
            .insert(path.clone(), TableTab::new(conn, database, schema, table));
        self.sidebar.add_file(path.clone());
        let show = self.activate(path.clone());
        Task::batch([show, self.db_load_rows(path, 0)])
    }

    pub(super) fn db_message(&mut self, msg: DbMsg) -> Task<Message> {
        match msg {
            DbMsg::OpenDialog(id) => {
                let driver_file = self.db.driver_path.clone();
                self.db.dialog = Some(match id.and_then(|id| self.db.connection(id).cloned()) {
                    Some(connection) => {
                        let password = self
                            .db
                            .secrets(&connection)
                            .map(|(password, _)| password)
                            .unwrap_or_default();
                        Dialog::edit(&connection, password, driver_file)
                    }
                    None => {
                        let mut form = Dialog::new();
                        form.driver_file = driver_file;
                        form
                    }
                });
                if self.db.driver == Driver::Unknown {
                    return self.db_probe_driver();
                }
                Task::none()
            }
            DbMsg::CloseDialog => {
                self.db.dialog = None;
                Task::none()
            }
            DbMsg::Tab(tab) => {
                if let Some(form) = &mut self.db.dialog {
                    form.tab = tab;
                }
                Task::none()
            }
            DbMsg::Text(field, value) => {
                if let Some(form) = &mut self.db.dialog {
                    form.set(field, value);
                }
                Task::none()
            }
            DbMsg::Check(flag, on) => {
                if let Some(form) = &mut self.db.dialog {
                    form.check(flag, on);
                }
                Task::none()
            }
            DbMsg::Choose(choice) => {
                if let Some(form) = &mut self.db.dialog {
                    form.choose(choice);
                }
                Task::none()
            }
            DbMsg::Script(action) => {
                if let Some(form) = &mut self.db.dialog {
                    form.script.perform(action);
                    form.test = TestState::Idle;
                }
                Task::none()
            }
            DbMsg::PropKey(i, value) => {
                if let Some(prop) = self
                    .db
                    .dialog
                    .as_mut()
                    .and_then(|f| f.draft.props.get_mut(i))
                {
                    prop.0 = value;
                }
                Task::none()
            }
            DbMsg::PropValue(i, value) => {
                if let Some(prop) = self
                    .db
                    .dialog
                    .as_mut()
                    .and_then(|f| f.draft.props.get_mut(i))
                {
                    prop.1 = value;
                }
                Task::none()
            }
            DbMsg::PropAdd => {
                if let Some(form) = &mut self.db.dialog {
                    form.draft.props.push((String::new(), String::new()));
                }
                Task::none()
            }
            DbMsg::PropRemove(i) => {
                if let Some(form) = &mut self.db.dialog {
                    if i < form.draft.props.len() {
                        form.draft.props.remove(i);
                    }
                }
                Task::none()
            }
            DbMsg::Browse(field) => Task::perform(
                async {
                    rfd::AsyncFileDialog::new()
                        .pick_file()
                        .await
                        .map(|f| f.path().to_path_buf())
                },
                move |path| Message::Db(DbMsg::Browsed(field, path)),
            ),
            DbMsg::Browsed(field, path) => {
                let Some(path) = path else {
                    return Task::none();
                };
                if let Some(form) = &mut self.db.dialog {
                    form.set(field, path.display().to_string());
                }
                if field == Field::DriverFile {
                    return self.db_use_driver_file(path.display().to_string());
                }
                Task::none()
            }
            DbMsg::Test => {
                let Some(form) = &mut self.db.dialog else {
                    return Task::none();
                };
                if !form.valid() {
                    return Task::none();
                }
                let params = Params {
                    target: form.target(),
                    scope: form.draft.scope.clone(),
                    driver: self.db.driver_path.clone(),
                };
                form.test = TestState::Running;
                Task::perform(database::test(params), |result| {
                    Message::Db(DbMsg::Tested(result))
                })
            }
            DbMsg::Tested(result) => {
                if let Some(form) = &mut self.db.dialog {
                    form.test = match result {
                        Ok(version) => TestState::Ok(version),
                        Err(error) => TestState::Failed(error),
                    };
                }
                Task::none()
            }
            DbMsg::Save => {
                let Some(form) = self.db.dialog.take().filter(Dialog::valid) else {
                    return Task::none();
                };
                // A driver file typed by hand.
                let typed = form.driver_file.trim().to_string();
                let mut tasks = Vec::new();
                if typed != self.db.driver_path {
                    tasks.push(self.db_use_driver_file(typed));
                }
                let Some(id) = self.db.apply(&form) else {
                    self.db.dialog = Some(form);
                    return Task::none();
                };
                // The tables open for it that failed read again, then the
                // tree (the one that reads last uses a password that asks
                // every time).
                let paths: Vec<PathBuf> = self
                    .db_tabs
                    .iter()
                    .filter(|(_, t)| t.conn == id && t.error.is_some())
                    .map(|(p, _)| p.clone())
                    .collect();
                for path in paths {
                    let offset = self.db_tabs.get(&path).map_or(0, TableTab::offset);
                    tasks.push(self.db_load_rows(path, offset));
                }
                self.db.open.insert(id.to_string());
                tasks.push(self.db_load_tree(id));
                Task::batch(tasks)
            }
            DbMsg::Duplicate(id) => {
                let Some(copy) = self.db.duplicate(id) else {
                    return Task::none();
                };
                self.db.dialog = None;
                self.db_message(DbMsg::OpenDialog(Some(copy)))
            }
            DbMsg::Delete => {
                if let Some(id) = self.db.dialog.as_ref().and_then(|d| d.editing) {
                    self.db.remove(id);
                }
                self.db.dialog = None;
                Task::none()
            }
            DbMsg::DriverProbed(driver) => {
                // A download in progress is not interrupted by a look.
                if self.db.driver != Driver::Downloading {
                    self.db.driver = driver;
                }
                Task::none()
            }
            DbMsg::DownloadDriver => {
                self.db.driver = Driver::Downloading;
                Task::perform(database::download_driver(), |result| {
                    Message::Db(DbMsg::DriverDownloaded(result))
                })
            }
            DbMsg::DriverDownloaded(result) => {
                match result {
                    Ok(()) => {
                        self.db.driver = Driver::Unknown;
                        return self.db_probe_driver();
                    }
                    Err(error) => self.db.driver = Driver::Failed(error),
                }
                Task::none()
            }
            DbMsg::UseDownloadedDriver => {
                if let Some(form) = &mut self.db.dialog {
                    form.driver_file.clear();
                }
                self.db_use_driver_file(String::new())
            }
            DbMsg::Toggle(key) => {
                if self.db.open.remove(&key) {
                    return Task::none();
                }
                self.db.open.insert(key.clone());
                // A connection that opens loads its tables the first time.
                if let Ok(id) = key.parse::<u64>() {
                    if !matches!(self.db.trees.get(&id), Some(Tree::Loaded(_))) {
                        return self.db_load_tree(id);
                    }
                }
                Task::none()
            }
            DbMsg::Reload(id) => {
                self.db.open.insert(id.to_string());
                self.db_load_tree(id)
            }
            DbMsg::TreeLoaded(id, result) => {
                if self.db.connection(id).is_some() {
                    self.db.trees.insert(
                        id,
                        match result {
                            Ok(databases) => {
                                // What holds the tables is shown open.
                                for database in &databases {
                                    if database.current {
                                        self.db.open.insert(format!("{id}/{}", database.name));
                                    }
                                    if let Some(schema) =
                                        database.schemas.iter().find(|s| !s.tables.is_empty())
                                    {
                                        self.db.open.insert(format!(
                                            "{id}/{}/{}",
                                            database.name, schema.name
                                        ));
                                    }
                                }
                                Tree::Loaded(databases)
                            }
                            Err(error) => Tree::Failed(error),
                        },
                    );
                }
                Task::none()
            }
            DbMsg::OpenTable(conn, database, schema, table) => {
                self.db_open_table(conn, database, schema, table)
            }
            DbMsg::Rows(path, result) => {
                if let Some(tab) = self.db_tabs.get_mut(&path) {
                    match result {
                        Ok(page) => tab.set_page(page),
                        Err(error) => {
                            tab.loading = false;
                            tab.error = Some(error);
                        }
                    }
                }
                Task::none()
            }
            DbMsg::Turn(path, pages) => {
                let Some(tab) = self.db_tabs.get(&path) else {
                    return Task::none();
                };
                let offset = (tab.offset() as i64 + pages * database::PAGE_SIZE as i64).max(0);
                self.db_load_rows(path, offset as u64)
            }
            DbMsg::Refresh(path) => {
                let offset = self.db_tabs.get(&path).map_or(0, TableTab::offset);
                self.db_load_rows(path, offset)
            }
            DbMsg::Scrolled(path, offset, height) => {
                if let Some(tab) = self.db_tabs.get_mut(&path) {
                    tab.scroll = (offset, height);
                }
                Task::none()
            }
        }
    }

    /// The driver is this file (or the downloaded one, for an empty path).
    fn db_use_driver_file(&mut self, path: String) -> Task<Message> {
        self.db.driver_path = path.trim().to_string();
        self.db.persist();
        self.db.driver = Driver::Unknown;
        self.db_probe_driver()
    }
}
