//! Dropping a table, deleting all its rows, and the table designer: what
//! their messages do.

use std::path::PathBuf;

use iced::Task;

use super::{Message, SimpleEditApp};
use crate::database::designer::{Designer, Load};
use crate::database::protocol::ConstraintKind;
use crate::database::{
    self, AlterKind, Altered, ColField, ConField, Confirm, ConfirmAction, DbMsg, DesignerMsg,
    IdxField, Kind, TableRef,
};

impl SimpleEditApp {
    /// Asks the user before dropping a table or deleting all its rows.
    pub(super) fn admin_ask(
        &mut self,
        target: (u64, String, String, String),
        kind: Kind,
        action: ConfirmAction,
    ) -> Task<Message> {
        let (conn, database, schema, table) = target;
        self.db.confirm = Some(Confirm {
            conn,
            database,
            schema,
            table,
            kind,
            action,
            cascade: false,
            running: false,
            error: None,
        });
        Task::none()
    }

    /// Runs statements on a table's database and says what they gave.
    fn admin_run(
        &mut self,
        what: AlterKind,
        target: (u64, String, String, String),
        statements: Vec<String>,
    ) -> Task<Message> {
        let (conn, database, schema, table) = target;
        let Some(params) = self.db_params(conn, false) else {
            // The password is asked first.
            if let Some(c) = self.db.confirm.as_mut() {
                c.running = false;
            }
            if let Some(d) = self.db.designer.as_mut() {
                d.load = Load::Ready;
            }
            return self.db_ask_password(conn);
        };
        let target_db = database.clone();
        Task::perform(
            // All or none: a change that stops half way would leave the table
            // as neither the old one nor the new one.
            database::run_transaction(params, target_db, statements),
            move |done| {
                Message::Db(DbMsg::Altered(Box::new(Altered {
                    what: what.clone(),
                    conn,
                    database: database.clone(),
                    schema: schema.clone(),
                    table: table.clone(),
                    done,
                })))
            },
        )
    }

    pub(super) fn admin_confirm_run(&mut self) -> Task<Message> {
        let Some(c) = self.db.confirm.as_mut() else {
            return Task::none();
        };
        if c.running {
            return Task::none();
        }
        c.running = true;
        c.error = None;
        let statement = c.statement();
        let what = match c.action {
            ConfirmAction::Drop => AlterKind::Drop,
            ConfirmAction::Empty => AlterKind::Empty,
        };
        let target = (
            c.conn,
            c.database.clone(),
            c.schema.clone(),
            c.table.clone(),
        );
        self.admin_run(what, target, vec![statement])
    }

    /// The tabs that show a table (or rows read from it).
    fn admin_tabs(&self, a: &Altered) -> Vec<PathBuf> {
        self.db_tabs
            .iter()
            .filter(|(_, t)| {
                let r = t.table_ref();
                t.conn == a.conn
                    && r.schema == a.schema
                    && r.table == a.table
                    && (t.database == a.database || t.database.is_empty() || a.database.is_empty())
            })
            .map(|(p, _)| p.clone())
            .collect()
    }

    /// What an operation on a table gave: on success the tree is read again
    /// and the tabs of the table are closed or read again.
    pub(super) fn admin_altered(&mut self, a: Altered) -> Task<Message> {
        let log = self.db_log(a.conn, &a.done.executed);
        let failure = match &a.done.result {
            Err(error) => Some(error.clone()),
            Ok(ran) => ran.failed.as_ref().map(|(_, error)| error.clone()),
        };
        if let Some(error) = failure {
            match a.what {
                AlterKind::Drop | AlterKind::Empty => {
                    if let Some(c) = self.db.confirm.as_mut() {
                        c.running = false;
                        c.error = Some(error);
                    }
                }
                AlterKind::Modify(_) => {
                    if let Some(d) = self.db.designer.as_mut() {
                        d.load = Load::Ready;
                        d.error = Some(error);
                    }
                }
                AlterKind::Schema => {
                    if let Some(n) = self.db.new_schema.as_mut() {
                        n.running = false;
                        n.error = Some(error);
                    }
                }
            }
            return log;
        }
        let tabs = self.admin_tabs(&a);
        let mut tasks = vec![log];
        let close = match &a.what {
            AlterKind::Drop => {
                self.db.confirm = None;
                true
            }
            AlterKind::Empty => {
                self.db.confirm = None;
                false
            }
            AlterKind::Modify(renamed) => {
                self.db.designer = None;
                renamed.is_some()
            }
            AlterKind::Schema => {
                self.db.new_schema = None;
                false
            }
        };
        // What the tables are made of may have changed.
        self.db.structures.retain(|k, _| k.0 != a.conn);
        for tab in self.db_tabs.values_mut().filter(|t| t.conn == a.conn) {
            tab.structure = None;
        }
        for path in tabs {
            // Changes waiting for Commit are not thrown away with the tab.
            if let Some(tab) = self
                .db_tabs
                .get_mut(&path)
                .filter(|t| !t.pending.is_empty() || t.editing.is_some())
            {
                tab.notice = Some(t!("db.page_kept_edits").to_string());
                continue;
            }
            if close {
                tasks.push(self.close_path(path));
            } else {
                let offset = self
                    .db_tabs
                    .get(&path)
                    .map_or(0, database::TableTab::offset);
                tasks.push(self.db_load_rows(path, offset));
            }
        }
        // The tables of the tree, and the columns for the completion.
        if a.what != AlterKind::Empty {
            tasks.push(self.db_load_tree(a.conn));
        }
        Task::batch(tasks)
    }

    pub(super) fn admin_open_designer(
        &mut self,
        target: (u64, String, String, String),
    ) -> Task<Message> {
        let (conn, database, schema, table) = target;
        self.db.designer = Some(Designer::new(
            conn,
            database.clone(),
            schema.clone(),
            table.clone(),
        ));
        let Some(params) = self.db_params(conn, false) else {
            self.db.designer = None;
            return self.db_ask_password(conn);
        };
        let window = database::AnswerFor::Designer(conn, schema.clone(), table.clone());
        Task::perform(
            database::structure(
                params,
                TableRef {
                    database,
                    schema,
                    table,
                },
            ),
            move |done| {
                Message::Db(DbMsg::Answer(
                    window.clone(),
                    Box::new(DbMsg::Designer(DesignerMsg::Loaded(done))),
                ))
            },
        )
    }

    pub(super) fn admin_designer(&mut self, msg: DesignerMsg) -> Task<Message> {
        // The structure that was read goes first: it is also logged.
        if let DesignerMsg::Loaded(done) = msg {
            let conn = self.db.designer.as_ref().map(|d| d.conn);
            let log = match conn {
                Some(conn) => self.db_log(conn, &done.executed),
                None => Task::none(),
            };
            if let Some(d) = self.db.designer.as_mut() {
                match done.result {
                    Ok(structure) => d.load(structure),
                    Err(error) => {
                        d.load = Load::Ready;
                        d.error = Some(error);
                    }
                }
            }
            return log;
        }
        if let DesignerMsg::Apply = msg {
            return self.admin_apply();
        }
        let Some(d) = self.db.designer.as_mut() else {
            return Task::none();
        };
        if d.load == Load::Running {
            return Task::none();
        }
        d.error = None;
        match msg {
            DesignerMsg::Loaded(_) | DesignerMsg::Apply => {}
            DesignerMsg::Close => self.db.designer = None,
            DesignerMsg::Tab(tab) => d.tab = tab,
            DesignerMsg::TableName(name) => d.name = name,
            DesignerMsg::TableSchema(text) => d.schema_to = text,
            DesignerMsg::TableComment(text) => d.comment = text,
            DesignerMsg::Col(i, field, text) => {
                if let Some(c) = d.columns.get_mut(i) {
                    match field {
                        ColField::Name => c.name = text,
                        ColField::Type => c.kind = text,
                        ColField::Default => c.default = text,
                        ColField::Comment => c.comment = text,
                        ColField::Using => c.using = text,
                    }
                }
            }
            DesignerMsg::ColNull(i, nullable) => {
                if let Some(c) = d.columns.get_mut(i) {
                    c.nullable = nullable;
                }
            }
            DesignerMsg::ColDrop(i, dropped) => {
                if let Some(c) = d.columns.get_mut(i) {
                    if c.original.is_some() {
                        c.dropped = dropped;
                    } else {
                        d.columns.remove(i);
                    }
                }
            }
            DesignerMsg::ColAdd => d.add_column(),
            DesignerMsg::Con(i, field, text) => {
                if let Some(c) = d.constraints.get_mut(i) {
                    match field {
                        ConField::Name => c.name = text,
                        ConField::Columns => c.columns = text,
                        ConField::Reference => c.reference = text,
                        ConField::RefColumns => c.ref_columns = text,
                        ConField::OnDelete => c.on_delete = text,
                        ConField::Expr => c.expr = text,
                    }
                }
            }
            DesignerMsg::ConDrop(i, dropped) => {
                if let Some(c) = d.constraints.get_mut(i) {
                    if c.original.is_some() {
                        c.dropped = dropped;
                    } else {
                        d.constraints.remove(i);
                    }
                }
            }
            DesignerMsg::ConAdd(kind) => d.add_constraint(kind),
            DesignerMsg::Idx(i, field, text) => {
                if let Some(x) = d.indexes.get_mut(i) {
                    match field {
                        IdxField::Name => x.name = text,
                        IdxField::Columns => x.columns = text,
                    }
                }
            }
            DesignerMsg::IdxUnique(i, unique) => {
                if let Some(x) = d.indexes.get_mut(i) {
                    x.unique = unique;
                }
            }
            DesignerMsg::IdxDrop(i, dropped) => {
                if let Some(x) = d.indexes.get_mut(i) {
                    if x.original.is_some() {
                        x.dropped = dropped;
                    } else {
                        d.indexes.remove(i);
                    }
                }
            }
            DesignerMsg::IdxAdd => d.add_index(),
        }
        Task::none()
    }

    /// Runs what the designer changed, in one transaction.
    fn admin_apply(&mut self) -> Task<Message> {
        let Some(d) = self.db.designer.as_mut() else {
            return Task::none();
        };
        if d.load != Load::Ready || d.problem().is_some() {
            return Task::none();
        }
        let statements = d.statements();
        if statements.is_empty() {
            return Task::none();
        }
        d.load = Load::Running;
        d.error = None;
        // A table that is renamed or moved is no longer where its tabs read it.
        let renamed = (!d.creating && (d.name.trim() != d.table || d.moved()))
            .then(|| d.name.trim().to_string());
        let target = (
            d.conn,
            d.database.clone(),
            d.schema.clone(),
            d.table.clone(),
        );
        // A failure leaves nothing: the session ends with its transaction open.
        let mut all = vec!["BEGIN".to_string()];
        all.extend(statements);
        all.push("COMMIT".to_string());
        self.admin_run(AlterKind::Modify(renamed), target, all)
    }
}

/// Whether a constraint kind has its own columns to give.
#[allow(dead_code)]
fn has_columns(kind: ConstraintKind) -> bool {
    matches!(
        kind,
        ConstraintKind::Primary | ConstraintKind::Unique | ConstraintKind::Foreign
    )
}
