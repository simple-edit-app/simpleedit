//! The tools of a database: its health, and what its messages do.

use iced::Task;

use super::{Message, SimpleEditApp};
use crate::database::{self, DbMsg, HealthTab, HealthView};

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
