//! The Database panel inside the app: what its messages do.

use std::path::PathBuf;

use iced::Task;

use super::{ActiveOverlay, Message, SimpleEditApp};
use crate::database::{
    self, grid, DbMsg, Dialog, Driver, Field, Params, TableTab, TestState, Tree,
};

impl SimpleEditApp {
    /// Finds out whether the driver is installed.
    pub(super) fn db_probe_driver(&mut self) -> Task<Message> {
        Task::perform(
            database::probe_driver(self.db.driver_path.clone()),
            |driver| Message::Db(DbMsg::DriverProbed(driver)),
        )
    }

    /// The password of a connection that asks every time serves once.
    pub(super) fn db_params(&mut self, id: u64, consume: bool) -> Option<Params> {
        let params = self.db.params(id)?;
        if consume {
            self.db.once.remove(&id);
        }
        Some(params)
    }

    /// Loads the databases, schemas and tables of a connection.
    pub(super) fn db_load_tree(&mut self, id: u64) -> Task<Message> {
        let Some(params) = self.db_params(id, true) else {
            return self.db_ask_password(id);
        };
        self.db.trees.insert(id, Tree::Loading);
        Task::perform(database::tree(params), move |result| {
            Message::Db(DbMsg::TreeLoaded(id, result))
        })
    }

    /// Opens the form of a connection whose password is not known yet.
    pub(super) fn db_ask_password(&mut self, id: u64) -> Task<Message> {
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

    pub(super) fn db_load_rows(&mut self, path: PathBuf, offset: u64) -> Task<Message> {
        let Some(tab) = self.db_tabs.get(&path) else {
            return Task::none();
        };
        let (conn, table, query) = (tab.conn, tab.table_ref(), tab.query(offset));
        // A query that cannot be paged is run again as it is.
        // A query that cannot be paged (it writes, or it is a plan) is not run
        // again: what it gave stays on show, and running it twice could do
        // its work twice.
        if tab.query.is_some() && !tab.paged {
            return Task::none();
        }
        let live = tab.live.then(|| self.db.live.get(&conn).cloned()).flatten();
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
            database::rows_with(live, params, table, query),
            move |done| Message::Db(DbMsg::Rows(path.clone(), done)),
        )
    }

    /// Puts what the driver ran in the log of the console, and shows the end.
    pub(super) fn db_log(&mut self, conn: u64, executed: &[database::Executed]) -> Task<Message> {
        let label = self
            .db
            .connection(conn)
            .map(database::Connection::label)
            .unwrap_or_default();
        self.db_log_as(&label, executed)
    }

    /// The same, for a connection that is named but may not be kept yet.
    fn db_log_as(&mut self, label: &str, executed: &[database::Executed]) -> Task<Message> {
        if executed.is_empty() {
            return Task::none();
        }
        self.db.console.push(label, executed);
        if self.db.console.open {
            iced::widget::operation::snap_to_end(database::log_id())
        } else {
            Task::none()
        }
    }

    /// A tab for the rows a query gave: one that only reads is paged, sorted
    /// and filtered like a table (its rows are asked for again); another one
    /// shows what it gave.
    fn db_open_result(&mut self, conn: u64, result: &database::QueryResult) -> Task<Message> {
        self.assign_db_color(conn);
        self.db_counter += 1;
        let title = format!("{} {}", t!("db.result"), self.db_counter);
        let path = PathBuf::from(format!("db://{}/{title}", self.db_counter));
        let mut tab = TableTab::new(
            conn,
            self.db.console.database.clone(),
            String::new(),
            String::new(),
            self.db.page_size,
        );
        tab.title = title;
        tab.query = Some(result.sql.clone());
        tab.paged = result.deferred;
        tab.live = true;
        if !result.deferred {
            tab.set_page(database::result_page(result));
        }
        self.db_tabs.insert(path.clone(), tab);
        self.sidebar.add_file(path.clone());
        let show = self.activate(path.clone());
        if result.deferred {
            Task::batch([show, self.db_load_rows(path, 0)])
        } else {
            show
        }
    }

    /// Works out what the queries could be completed with at the caret.
    fn db_complete(&mut self, force: bool) {
        let Some(conn) = self.db.console.connection else {
            self.db.console.completion = None;
            return;
        };
        let known = self.db.known(conn);
        let console = &mut self.db.console;
        let text = console.editor.text();
        let at = console.editor.cursor().position;
        let caret = database::console::caret_offset(&text, at.line, at.column);
        console.completion = database::completion::suggest(&text, caret, &known, force).map(|s| {
            let line_start = text[..s.start].rfind('\n').map_or(0, |n| n + 1);
            database::console::Completion {
                line: text[..s.start].matches('\n').count(),
                column: text[line_start..s.start].chars().count(),
                typed: text[s.start..caret].chars().count(),
                suggestions: s,
                selected: 0,
            }
        });
    }

    /// Puts a proposal in the place of what was typed.
    fn db_complete_with(&mut self, at: usize) -> Task<Message> {
        use iced::widget::text_editor::{Action, Edit};
        let console = &mut self.db.console;
        let Some(completion) = console.completion.take() else {
            return Task::none();
        };
        let Some(item) = completion.suggestions.items.get(at) else {
            return Task::none();
        };
        for _ in 0..completion.typed {
            console.editor.perform(Action::Edit(Edit::Backspace));
        }
        console
            .editor
            .perform(Action::Edit(Edit::Paste(std::sync::Arc::new(
                item.insert.clone(),
            ))));
        Task::none()
    }

    /// Runs the statements of the console.
    fn db_run(&mut self, all: bool) -> Task<Message> {
        if self.db.console.running {
            return Task::none();
        }
        let statements = self.db.console.statements(all);
        let Some(conn) = self
            .db
            .console
            .connection
            .filter(|id| self.db.connection(*id).is_some())
            .or_else(|| self.db.connections.first().map(|c| c.id))
        else {
            self.db.console.last = Some((false, t!("db.console_no_connection").to_string()));
            return Task::none();
        };
        self.db.console.connection = Some(conn);
        self.db.console.save();
        if statements.is_empty() {
            self.db.console.last = Some((false, t!("db.console_nothing").to_string()));
            return Task::none();
        }
        self.db_exec(conn, statements)
    }

    /// Runs statements in the session of the console.
    fn db_exec(&mut self, conn: u64, statements: Vec<String>) -> Task<Message> {
        let Some(params) = self.db_params(conn, false) else {
            return self.db_ask_password(conn);
        };
        self.db.console.running = true;
        self.db.console.last = None;
        let live = self.db.live_of(conn);
        let database = self.db.console.database.clone();
        let (task, handle) = Task::perform(
            database::run_with(Some(live), params, database, statements, false),
            move |done| Message::Db(DbMsg::Ran(conn, done)),
        )
        .abortable();
        self.db.console.abort = Some(handle);
        task
    }

    /// Whether the tab has changes that wait: it then says so, and what was
    /// asked (another page, a sort…) is not done.
    fn db_blocked(&mut self, path: &PathBuf) -> bool {
        match self.db_tabs.get_mut(path) {
            Some(tab) if !tab.pending.is_empty() => {
                tab.notice = Some(t!("db.pending_block").to_string());
                true
            }
            _ => false,
        }
    }

    /// What was typed in the cell becomes a pending edit.
    fn db_commit_edit(&mut self, path: &PathBuf) {
        let Some(tab) = self.db_tabs.get_mut(path) else {
            return;
        };
        let (Some(editing), Some(page)) = (tab.editing.take(), tab.page.as_ref()) else {
            return;
        };
        // Nothing typed in a cell that is NULL leaves it NULL.
        let was_null = matches!(
            tab.pending.cell(page, editing.row, editing.col),
            grid::Cell::Null
        );
        let value = if editing.text.is_empty() && was_null {
            None
        } else {
            Some(editing.text)
        };
        let page = page.clone();
        tab.pending.set(&page, editing.row, editing.col, value);
    }

    /// The keys of the table in front when no field is being typed in: the
    /// arrows and Tab move the focus from cell to cell, F2 types in the cell,
    /// Alt+Delete marks its row, Cmd/Ctrl+C copies its value.
    pub(super) fn db_key(
        &mut self,
        key: &str,
        ctrl: bool,
        shift: bool,
        alt: bool,
    ) -> Option<Task<Message>> {
        // Nothing here while something else asks for the keys.
        if self.show_preferences || self.overlay != ActiveOverlay::None || self.tree_focus {
            return None;
        }
        // The log in view: Cmd/Ctrl+C copies the entry that was clicked.
        if ctrl
            && !shift
            && !alt
            && key == "c"
            && self.db.console.open
            && self.db.console.tab == database::console::ConsoleTab::Log
        {
            if let Some(at) = self.db.console.selected_log {
                return Some(self.db_message(DbMsg::CopyLog(at, database::LogCopy::Sql)));
            }
        }
        let console_typing =
            self.db.console.open && self.db.console.tab == database::console::ConsoleTab::Queries;
        let path = self.current_file.clone()?;
        let tab = self.db_tabs.get_mut(&path)?;
        if tab.editing.is_some() || tab.picker.is_some() {
            return None;
        }
        let page = tab.page.as_ref()?;
        let (rows, cols) = (tab.pending.rows(page), page.columns.len());
        if rows == 0 || cols == 0 {
            return None;
        }
        let (row, col) = tab.selected.unwrap_or((0, 0));
        let page_rows = ((tab.scroll.1 / database::ROW_HEIGHT) as usize)
            .saturating_sub(1)
            .max(1);
        let target = match (key, ctrl, alt) {
            ("ArrowUp", false, false) => (row.saturating_sub(1), col),
            ("ArrowDown", false, false) => ((row + 1).min(rows - 1), col),
            ("ArrowLeft", false, false) => (row, col.saturating_sub(1)),
            ("ArrowRight", false, false) => (row, (col + 1).min(cols - 1)),
            ("Home", false, false) => (row, 0),
            ("End", false, false) => (row, cols - 1),
            ("PageUp", false, false) => (row.saturating_sub(page_rows), col),
            ("PageDown", false, false) => ((row + page_rows).min(rows - 1), col),
            // Tab goes to the next cell, and to the next row at the end of one.
            ("Tab", false, false) if !shift => {
                if col + 1 < cols {
                    (row, col + 1)
                } else {
                    ((row + 1).min(rows - 1), 0)
                }
            }
            ("Tab", false, false) => {
                if col > 0 {
                    (row, col - 1)
                } else {
                    (row.saturating_sub(1), cols - 1)
                }
            }
            ("F2", false, false) => {
                return Some(self.db_message(DbMsg::Edit(path, row, col)));
            }
            ("Delete", false, true) => {
                return Some(self.db_message(DbMsg::DeleteRow(path, row)));
            }
            // The console's own editor has Cmd/Ctrl+C when it is the one in use.
            ("c", true, false) if !shift && !console_typing => {
                let value = tab.pending.value(page, row, col)?.to_string();
                return Some(iced::clipboard::write(value));
            }
            _ => return None,
        };
        tab.selected = Some(target);
        Some(Self::db_reveal(tab, target.0))
    }

    /// Scrolls the table so that a row is in view.
    fn db_reveal(tab: &mut TableTab, row: usize) -> Task<Message> {
        let (offset, height) = tab.scroll;
        let top = row as f32 * database::ROW_HEIGHT;
        let bottom = top + database::ROW_HEIGHT;
        let wanted = if top < offset {
            Some(top)
        } else if bottom > offset + height {
            Some(bottom - height)
        } else {
            None
        };
        match wanted {
            Some(y) => {
                tab.scroll.0 = y;
                iced::widget::operation::scroll_to(
                    database::grid_id(),
                    iced::widget::scrollable::AbsoluteOffset { x: 0.0, y },
                )
            }
            None => Task::none(),
        }
    }

    /// Escape gives up what is being typed or picked in the table in front.
    pub(super) fn db_escape(&mut self) -> bool {
        if self.db.text.take().is_some() {
            return true;
        }
        if self.db.new_schema.as_ref().is_some_and(|n| !n.running) {
            self.db.new_schema = None;
            return true;
        }
        // A question waiting for a yes, or the table designer.
        if self.db.confirm.as_ref().is_some_and(|c| !c.running) {
            self.db.confirm = None;
            return true;
        }
        if self
            .db
            .designer
            .as_ref()
            .is_some_and(|d| d.load != database::designer::Load::Running)
        {
            self.db.designer = None;
            return true;
        }
        let Some(tab) = self
            .current_file
            .as_ref()
            .and_then(|p| self.db_tabs.get_mut(p))
        else {
            return false;
        };
        tab.picker.take().is_some() || tab.editing.take().is_some()
    }

    /// Opens a table in a tab, or goes to the one that shows it.
    pub(super) fn db_open_table(
        &mut self,
        conn: u64,
        database: String,
        schema: String,
        table: String,
    ) -> Task<Message> {
        self.db_open_table_with(conn, database, schema, table, None)
    }

    /// Opens a table, with a condition on its rows if there is one.
    pub(super) fn db_open_table_with(
        &mut self,
        conn: u64,
        database: String,
        schema: String,
        table: String,
        filter: Option<String>,
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
            let show = self.activate(path.clone());
            let Some(filter) = filter else {
                return show;
            };
            // Rows with edits waiting are not read again under them.
            if self.db_blocked(&path) {
                return show;
            }
            if let Some(tab) = self.db_tabs.get_mut(&path) {
                tab.filter_text = filter.clone();
                tab.filter = filter;
                tab.notice = None;
            }
            return Task::batch([show, self.db_load_rows(path, 0)]);
        }
        self.assign_db_color(conn);
        self.db_counter += 1;
        let path = PathBuf::from(format!("db://{}/{}", self.db_counter, title));
        let key = (conn, database.clone(), schema.clone(), table.clone());
        let mut tab = TableTab::new(conn, database, schema, table, self.db.page_size);
        tab.structure = self.db.structures.get(&key).cloned();
        if let Some(filter) = filter {
            tab.filter_text = filter.clone();
            tab.filter = filter;
        }
        self.db_tabs.insert(path.clone(), tab);
        self.sidebar.add_file(path.clone());
        let show = self.activate(path.clone());
        let mut tasks = vec![show, self.db_load_rows(path, 0)];
        // What the table is made of: for the links of its rows.
        if !self.db.structures.contains_key(&key) {
            tasks.push(self.db_read_structure(key, false));
        }
        Task::batch(tasks)
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
                    browse: None,
                };
                form.test = TestState::Running;
                Task::perform(database::test(params), |result| {
                    Message::Db(DbMsg::Tested(result))
                })
            }
            DbMsg::Tested(done) => {
                let mut label = String::new();
                if let Some(form) = &mut self.db.dialog {
                    label = form.draft.label();
                    form.test = match done.result {
                        Ok(version) => TestState::Ok(version),
                        Err(error) => TestState::Failed(error),
                    };
                }
                self.db_log_as(&label, &done.executed)
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
                // The first click asks; the second deletes.
                if let Some(dialog) = self.db.dialog.as_mut().filter(|d| !d.confirm_delete) {
                    dialog.confirm_delete = true;
                    return Task::none();
                }
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
                self.db.structures.retain(|k, _| k.0 != id);
                self.db_load_tree(id)
            }
            DbMsg::TreeLoaded(id, done) => {
                let log = self.db_log(id, &done.executed);
                let read_columns = done.result.is_ok();
                if self.db.connection(id).is_some() {
                    self.db.trees.insert(
                        id,
                        match done.result {
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
                // The columns of the tables come for the completion of queries.
                let columns = match (read_columns, self.db.params(id)) {
                    (true, Some(params)) => {
                        Task::perform(database::table_columns(params), move |done| {
                            Message::Db(DbMsg::ColumnsLoaded(id, done))
                        })
                    }
                    _ => Task::none(),
                };
                Task::batch([log, columns])
            }
            DbMsg::OpenTable(conn, database, schema, table) => {
                self.db_open_table(conn, database, schema, table)
            }
            DbMsg::Rows(path, done) => {
                let conn = self.db_tabs.get(&path).map(|t| t.conn);
                let log = match conn {
                    Some(conn) => self.db_log(conn, &done.executed),
                    None => Task::none(),
                };
                if let Some(tab) = self.db_tabs.get_mut(&path) {
                    match done.result {
                        Ok(page) => tab.set_page(page),
                        Err(error) => {
                            tab.loading = false;
                            tab.error = Some(error);
                        }
                    }
                }
                log
            }
            DbMsg::Turn(path, pages) => {
                if self.db_blocked(&path) {
                    return Task::none();
                }
                let Some(tab) = self.db_tabs.get(&path) else {
                    return Task::none();
                };
                let offset = (tab.offset() as i64 + pages * tab.page_size as i64).max(0);
                self.db_load_rows(path, offset as u64)
            }
            DbMsg::Refresh(path) => {
                if self.db_blocked(&path) {
                    return Task::none();
                }
                let offset = self.db_tabs.get(&path).map_or(0, TableTab::offset);
                self.db_load_rows(path, offset)
            }
            DbMsg::Filter(path, text) => {
                if let Some(tab) = self.db_tabs.get_mut(&path) {
                    tab.filter_text = text;
                }
                Task::none()
            }
            DbMsg::ApplyFilter(path) => {
                if self.db_blocked(&path) {
                    return Task::none();
                }
                if let Some(tab) = self.db_tabs.get_mut(&path) {
                    tab.filter = tab.filter_text.trim().to_string();
                    tab.notice = None;
                }
                self.db_load_rows(path, 0)
            }
            DbMsg::ClearFilter(path) => {
                if self.db_blocked(&path) {
                    return Task::none();
                }
                if let Some(tab) = self.db_tabs.get_mut(&path) {
                    tab.filter_text.clear();
                    tab.filter.clear();
                    tab.notice = None;
                }
                self.db_load_rows(path, 0)
            }
            DbMsg::PageSize(path, size) => {
                if self.db_blocked(&path) {
                    return Task::none();
                }
                if let Some(tab) = self.db_tabs.get_mut(&path) {
                    tab.page_size = size;
                }
                // The next tables open with it.
                self.db.page_size = size;
                self.db.persist();
                self.db_load_rows(path, 0)
            }
            DbMsg::Sort(path, column) => {
                if self.db_blocked(&path) {
                    return Task::none();
                }
                // Alt+click stacks the sort on another column.
                let add = self.mod_alt;
                let Some(tab) = self.db_tabs.get_mut(&path) else {
                    return Task::none();
                };
                let Some(name) = tab
                    .page
                    .as_ref()
                    .and_then(|p| p.columns.get(column))
                    .map(|c| c.name.clone())
                else {
                    return Task::none();
                };
                tab.order = tab.sorted_by(&name, add);
                tab.notice = None;
                self.db_load_rows(path, 0)
            }
            DbMsg::Edit(path, row, col) => {
                // What was being typed elsewhere is kept first.
                self.db_commit_edit(&path);
                let Some(tab) = self.db_tabs.get_mut(&path) else {
                    return Task::none();
                };
                let Some(page) = tab.page.as_ref().filter(|p| p.editable) else {
                    return Task::none();
                };
                if tab.pending.is_deleted(row) || row >= tab.pending.rows(page) {
                    return Task::none();
                }
                let text = tab
                    .pending
                    .value(page, row, col)
                    .unwrap_or_default()
                    .to_string();
                tab.picker = None;
                tab.notice = None;
                tab.selected = Some((row, col));
                tab.editing = Some(grid::Editing { row, col, text });
                iced::widget::operation::focus(database::edit_id())
            }
            DbMsg::EditInput(path, text) => {
                if let Some(editing) = self.db_tabs.get_mut(&path).and_then(|t| t.editing.as_mut())
                {
                    editing.text = text;
                }
                Task::none()
            }
            DbMsg::EditCommit(path) => {
                self.db_commit_edit(&path);
                Task::none()
            }
            DbMsg::SetNull(path, row, col) => {
                if let Some(tab) = self.db_tabs.get_mut(&path) {
                    let Some(page) = tab.page.clone().filter(|p| p.editable) else {
                        return Task::none();
                    };
                    if page.columns.get(col).is_some_and(|c| !c.nullable) {
                        tab.notice = Some(t!("db.not_nullable").to_string());
                    } else {
                        tab.pending.set(&page, row, col, None);
                        tab.notice = None;
                    }
                }
                Task::none()
            }
            DbMsg::DeleteRow(path, row) => {
                if let Some(tab) = self.db_tabs.get_mut(&path) {
                    if let (true, Some(page)) = (tab.editable(), tab.page.clone()) {
                        tab.editing = None;
                        tab.pending.delete(&page, row);
                        tab.selected = None;
                        tab.notice = None;
                    }
                }
                Task::none()
            }
            DbMsg::RevertRow(path, row) => {
                if let Some(tab) = self.db_tabs.get_mut(&path) {
                    if let Some(page) = tab.page.clone() {
                        tab.pending.revert_row(&page, row);
                    }
                }
                Task::none()
            }
            DbMsg::AddRow(path) => {
                self.db_commit_edit(&path);
                let Some(tab) = self.db_tabs.get_mut(&path) else {
                    return Task::none();
                };
                let Some(page) = tab.page.clone().filter(|p| p.editable) else {
                    return Task::none();
                };
                let row = tab.pending.add_row(&page);
                // The first column that is not the key: the key is most often
                // given by the database.
                let col = page
                    .columns
                    .iter()
                    .position(|c| !page.primary_key.contains(&c.name))
                    .unwrap_or(0);
                tab.selected = Some((row, col));
                tab.notice = None;
                let reveal = Self::db_reveal(tab, row);
                Task::batch([reveal, self.db_message(DbMsg::Edit(path, row, col))])
            }
            DbMsg::SetDefault(path, row, col) => {
                if let Some(tab) = self.db_tabs.get_mut(&path) {
                    if let Some(page) = tab.page.clone() {
                        tab.pending.set_default(&page, row, col);
                    }
                }
                Task::none()
            }
            DbMsg::RevertAll(path) => {
                if let Some(tab) = self.db_tabs.get_mut(&path) {
                    tab.pending = grid::Pending::default();
                    tab.editing = None;
                    tab.notice = None;
                }
                Task::none()
            }
            DbMsg::Commit(path) => {
                self.db_commit_edit(&path);
                let Some(tab) = self.db_tabs.get_mut(&path) else {
                    return Task::none();
                };
                let Some(page) = tab.page.as_ref() else {
                    return Task::none();
                };
                if tab.committing || tab.pending.is_empty() {
                    return Task::none();
                }
                let changes = tab.pending.changes(page);
                if changes.is_empty() {
                    tab.notice = Some(t!("db.no_changes").to_string());
                    return Task::none();
                }
                let (conn, table) = (tab.conn, tab.table_ref());
                let Some(params) = self.db_params(conn, false) else {
                    return self.db_ask_password(conn);
                };
                if let Some(tab) = self.db_tabs.get_mut(&path) {
                    tab.committing = true;
                    tab.notice = None;
                }
                Task::perform(database::apply(params, table, changes), move |result| {
                    Message::Db(DbMsg::Committed(path.clone(), result))
                })
            }
            DbMsg::Preview(path, on) => {
                if let Some(tab) = self.db_tabs.get_mut(&path) {
                    tab.preview = on;
                }
                Task::none()
            }
            DbMsg::Committed(path, done) => {
                let Some(conn) = self.db_tabs.get(&path).map(|t| t.conn) else {
                    return Task::none();
                };
                let log = self.db_log(conn, &done.executed);
                let Some(tab) = self.db_tabs.get_mut(&path) else {
                    return log;
                };
                tab.committing = false;
                match done.result {
                    Ok(_) => {
                        tab.pending = grid::Pending::default();
                        tab.notice = None;
                        // What the table is now.
                        let offset = tab.offset();
                        Task::batch([log, self.db_load_rows(path, offset)])
                    }
                    Err(error) => {
                        tab.notice = Some(error);
                        log
                    }
                }
            }
            DbMsg::Select(path, row, col) => {
                if let Some(tab) = self.db_tabs.get_mut(&path) {
                    // A click elsewhere ends what was typed.
                    if tab
                        .editing
                        .as_ref()
                        .is_some_and(|e| (e.row, e.col) != (row, col))
                    {
                        self.db_commit_edit(&path);
                    }
                }
                if let Some(tab) = self.db_tabs.get_mut(&path) {
                    tab.selected = Some((row, col));
                }
                Task::none()
            }
            DbMsg::ToggleConsole => {
                let console = &mut self.db.console;
                console.open = !console.open;
                if console.open {
                    // One tool panel at the bottom.
                    self.terminal.open = false;
                    self.terminal.focused = false;
                    self.scratch.open = false;
                    self.bookmark_panel.open = false;
                    self.git.open = false;
                    if self.db.console.connection.is_none() {
                        self.db.console.connection = self.db.connections.first().map(|c| c.id);
                    }
                    if self.db.driver == Driver::Unknown {
                        return self.db_probe_driver();
                    }
                } else {
                    console.save();
                }
                Task::none()
            }
            DbMsg::ConsoleTab(tab) => {
                self.db.console.tab = tab;
                if tab == database::console::ConsoleTab::Log {
                    return iced::widget::operation::snap_to_end(database::log_id());
                }
                Task::none()
            }
            DbMsg::ConsoleConnection(id) => {
                if self.db.console.connection != Some(id) {
                    // Another connection: its session, not this one's.
                    self.db.console.database.clear();
                    self.db.console.in_transaction = false;
                }
                self.db.console.connection = Some(id);
                Task::none()
            }
            DbMsg::ConsoleDatabase(name) => {
                if self.db.console.database != name {
                    // The session moves to another database: a transaction
                    // of the old one is rolled back.
                    self.db.console.database = name;
                    self.db.console.in_transaction = false;
                }
                Task::none()
            }
            DbMsg::Explain(analyze) => {
                if self.db.console.running {
                    return Task::none();
                }
                let statements = self.db.console.statements(false);
                let Some(conn) = self.db.console.connection else {
                    self.db.console.last =
                        Some((false, t!("db.console_no_connection").to_string()));
                    return Task::none();
                };
                if statements.is_empty() {
                    self.db.console.last = Some((false, t!("db.console_nothing").to_string()));
                    return Task::none();
                }
                // Only a statement that reads can be run to be measured.
                if analyze && statements.iter().any(|s| !database::protocol::wrappable(s)) {
                    self.db.console.last = Some((false, t!("db.explain_not_read").to_string()));
                    return Task::none();
                }
                let prefix = if analyze {
                    "EXPLAIN (ANALYZE, BUFFERS) "
                } else {
                    "EXPLAIN "
                };
                let statements = statements
                    .into_iter()
                    .map(|s| format!("{prefix}{s}"))
                    .collect();
                self.db_exec(conn, statements)
            }
            DbMsg::EndTransaction(commit) => {
                let Some(conn) = self.db.console.connection else {
                    return Task::none();
                };
                if self.db.console.running {
                    return Task::none();
                }
                let end = if commit { "COMMIT" } else { "ROLLBACK" };
                self.db_exec(conn, vec![end.to_string()])
            }
            DbMsg::Export(path, format) => {
                let Some(tab) = self.db_tabs.get(&path) else {
                    return Task::none();
                };
                let (conn, table, query) = (tab.conn, tab.table_ref(), tab.query(0));
                let live = tab.live.then(|| self.db.live.get(&conn).cloned()).flatten();
                let name = if tab.query.is_some() || tab.table.is_empty() {
                    "result".to_string()
                } else {
                    tab.table.clone()
                };
                let Some(params) = self.db_params(conn, false) else {
                    return self.db_ask_password(conn);
                };
                let ext = format.to_string();
                Task::perform(
                    async move {
                        let file = rfd::AsyncFileDialog::new()
                            .set_file_name(format!("{name}.{ext}"))
                            .save_file()
                            .await?;
                        let target = file.path().to_string_lossy().to_string();
                        Some(database::export(live, params, table, query, target, format).await)
                    },
                    move |done| Message::Db(DbMsg::Exported(path.clone(), done)),
                )
            }
            DbMsg::Exported(path, done) => {
                let Some(done) = done else {
                    return Task::none();
                };
                let log = match self.db_tabs.get(&path).map(|t| t.conn) {
                    Some(conn) => self.db_log(conn, &done.executed),
                    None => Task::none(),
                };
                if let Some(tab) = self.db_tabs.get_mut(&path) {
                    tab.notice = Some(match done.result {
                        Ok(file) => t!("db.exported", path = file.as_str()).to_string(),
                        Err(error) => error,
                    });
                }
                log
            }
            DbMsg::ShowInternal(on) => {
                self.db.console.show_internal = on;
                Task::none()
            }
            DbMsg::SelectLog(at) => {
                self.db.console.selected_log = Some(at);
                Task::none()
            }
            DbMsg::CopyLog(at, what) => {
                let Some(entry) = self.db.console.log.get(at) else {
                    return Task::none();
                };
                let text = match what {
                    database::LogCopy::Sql => entry.sql.clone(),
                    database::LogCopy::Error => entry.error.clone().unwrap_or_default(),
                    database::LogCopy::Entry => entry.text(),
                };
                iced::clipboard::write(text)
            }
            DbMsg::AskDrop(conn, database, schema, table, kind) => self.admin_ask(
                (conn, database, schema, table),
                kind,
                database::ConfirmAction::Drop,
            ),
            DbMsg::AskEmpty(conn, database, schema, table) => self.admin_ask(
                (conn, database, schema, table),
                database::Kind::Table,
                database::ConfirmAction::Empty,
            ),
            DbMsg::ConfirmCascade(on) => {
                if let Some(c) = self.db.confirm.as_mut() {
                    c.cascade = on;
                }
                Task::none()
            }
            DbMsg::ConfirmRun => self.admin_confirm_run(),
            DbMsg::ConfirmClose => {
                if self.db.confirm.as_ref().is_some_and(|c| !c.running) {
                    self.db.confirm = None;
                }
                Task::none()
            }
            DbMsg::OpenDesigner(conn, database, schema, table) => {
                self.admin_open_designer((conn, database, schema, table))
            }
            DbMsg::Designer(msg) => self.admin_designer(msg),
            msg @ (DbMsg::NewTable(..)
            | DbMsg::NewSchema(..)
            | DbMsg::SchemaName(_)
            | DbMsg::SchemaCreate
            | DbMsg::SchemaClose
            | DbMsg::ToggleTable(..)
            | DbMsg::StructureRead(..)
            | DbMsg::ShowDefinition(..)
            | DbMsg::ShowFunction(..)
            | DbMsg::Stopped(..)
            | DbMsg::Definition(..)
            | DbMsg::ShowDdl(..)
            | DbMsg::TreeFilter(_)
            | DbMsg::ViewValue(..)
            | DbMsg::CopyRow(..)
            | DbMsg::TextCopy
            | DbMsg::CopyCell(..)
            | DbMsg::TextClose
            | DbMsg::OpenRelated(..)
            | DbMsg::ResizeStart(..)
            | DbMsg::ResizeMove(_)
            | DbMsg::ResizeEnd) => self.db_tree_message(msg),
            DbMsg::Altered(altered) => self.admin_altered(*altered),
            DbMsg::ClearLog => {
                self.db.console.log.clear();
                self.db.console.selected_log = None;
                Task::none()
            }
            DbMsg::Sql(action) => {
                use iced::widget::text_editor::{Action, Edit};
                // Typing proposes; moving the caret, a click or a new line closes.
                let typed = matches!(
                    &action,
                    Action::Edit(Edit::Insert(_) | Edit::Backspace | Edit::Delete | Edit::Paste(_))
                );
                self.db.console.editor.perform(action);
                if typed {
                    self.db_complete(false);
                } else {
                    self.db.console.completion = None;
                }
                Task::none()
            }
            DbMsg::Complete => {
                self.db_complete(true);
                Task::none()
            }
            DbMsg::CompleteMove(by) => {
                if let Some(c) = &mut self.db.console.completion {
                    let n = c.suggestions.items.len() as i32;
                    c.selected = (c.selected as i32 + by).rem_euclid(n.max(1)) as usize;
                }
                Task::none()
            }
            DbMsg::CompleteAccept => {
                let at = self.db.console.completion.as_ref().map(|c| c.selected);
                match at {
                    Some(at) => self.db_complete_with(at),
                    None => Task::none(),
                }
            }
            DbMsg::CompletePick(at) => self.db_complete_with(at),
            DbMsg::CompleteClose => {
                self.db.console.completion = None;
                Task::none()
            }
            DbMsg::Stopped(done) => {
                // What the stop ran is in the log; nothing else to say.
                match self.db.console.connection {
                    Some(conn) => self.db_log(conn, &done.executed),
                    None => Task::none(),
                }
            }
            DbMsg::ColumnsLoaded(id, done) => {
                let log = self.db_log(id, &done.executed);
                if let Ok(columns) = done.result {
                    self.db.columns.insert(id, columns);
                }
                log
            }
            DbMsg::Run(all) => self.db_run(all),
            DbMsg::Cancel => {
                // The task is dropped, so is the driver: the server sees the
                // connection go and stops the query.
                if let Some(handle) = self.db.console.abort.take() {
                    handle.abort();
                }
                if self.db.console.running {
                    self.db.console.running = false;
                    // The driver is stopped with the session it held, and a
                    // transaction of it is rolled back by the server.
                    let mut stop = Task::none();
                    if let Some(conn) = self.db.console.connection {
                        // The server is told to stop it: before PostgreSQL 14 it
                        // would not notice that the driver went away.
                        let pid = self.db.live.get(&conn).map_or(0, database::Live::pid);
                        if pid > 0 {
                            if let Some(params) = self.db_params(conn, false) {
                                stop = Task::perform(database::cancel(params, pid), |done| {
                                    Message::Db(DbMsg::Stopped(done))
                                });
                            }
                        }
                        self.db.end_live(conn);
                    }
                    self.db.console.in_transaction = false;
                    self.db.console.last = Some((false, t!("db.console_cancelled").to_string()));
                    return stop;
                }
                Task::none()
            }
            DbMsg::Ran(conn, done) => {
                self.db.console.running = false;
                self.db.console.abort = None;
                let log = self.db_log(conn, &done.executed);
                let mut tasks = vec![log];
                match done.result {
                    Err(error) => {
                        // The session is gone with its transaction.
                        self.db.console.in_transaction = false;
                        self.db.console.last = Some((false, error));
                    }
                    Ok(ran) => {
                        self.db.console.in_transaction = ran.in_transaction;
                        let ms: u64 = ran.results.iter().map(|r| r.ms).sum();
                        self.db.console.last = Some(match &ran.failed {
                            Some((index, error)) => (
                                false,
                                t!("db.console_failed", n = index + 1, error = error.as_str())
                                    .to_string(),
                            ),
                            None => (
                                true,
                                t!("db.console_done", n = ran.results.len(), ms = ms).to_string(),
                            ),
                        });
                        // A SELECT shows its rows in a tab, like a table does;
                        // the rest says what it did in the log.
                        for result in ran.results.iter().filter(|r| r.has_rows).take(10) {
                            tasks.push(self.db_open_result(conn, result));
                        }
                        if ran.failed.is_some() {
                            self.db.console.tab = database::console::ConsoleTab::Log;
                        }
                    }
                }
                Task::batch(tasks)
            }
            DbMsg::OpenPicker(path, row, col) => {
                self.db_commit_edit(&path);
                if let Some(tab) = self.db_tabs.get_mut(&path) {
                    let Some(page) = tab.page.as_ref().filter(|p| p.editable) else {
                        return Task::none();
                    };
                    let Some(kind) = page
                        .columns
                        .get(col)
                        .and_then(|c| grid::PickKind::of(&c.kind))
                    else {
                        return Task::none();
                    };
                    let current = tab.pending.value(page, row, col);
                    tab.picker = Some(grid::Picker::new(row, col, kind, current));
                    tab.notice = None;
                }
                Task::none()
            }
            DbMsg::PickerShift(path, months) => {
                if let Some(picker) = self.db_tabs.get_mut(&path).and_then(|t| t.picker.as_mut()) {
                    picker.shift(months);
                }
                Task::none()
            }
            DbMsg::PickerDay(path, day) => {
                if let Some(picker) = self.db_tabs.get_mut(&path).and_then(|t| t.picker.as_mut()) {
                    picker.day = Some(day);
                }
                Task::none()
            }
            DbMsg::PickerTime(path, part, text) => {
                if let Some(picker) = self.db_tabs.get_mut(&path).and_then(|t| t.picker.as_mut()) {
                    picker.set_time(part, text);
                }
                Task::none()
            }
            DbMsg::PickerToday(path) => {
                if let Some(picker) = self.db_tabs.get_mut(&path).and_then(|t| t.picker.as_mut()) {
                    picker.today();
                }
                Task::none()
            }
            DbMsg::PickerApply(path) => {
                if let Some(tab) = self.db_tabs.get_mut(&path) {
                    let value = tab.picker.as_ref().and_then(grid::Picker::value);
                    if let (Some(value), Some(picker), Some(page)) =
                        (value, tab.picker.take(), tab.page.clone())
                    {
                        tab.pending.set(&page, picker.row, picker.col, Some(value));
                    }
                }
                Task::none()
            }
            DbMsg::PickerClose(path) => {
                if let Some(tab) = self.db_tabs.get_mut(&path) {
                    tab.picker = None;
                }
                Task::none()
            }
            DbMsg::DismissNotice(path) => {
                if let Some(tab) = self.db_tabs.get_mut(&path) {
                    tab.notice = None;
                }
                Task::none()
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
