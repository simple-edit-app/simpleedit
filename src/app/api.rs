//! The API panel: what its messages do.

use std::path::PathBuf;

use iced::widget::text_editor;
use iced::Task;

use super::{Message, SimpleEditApp};
use crate::api::model::{Environment, Folder, Item, Pair, Request, Variable};
use crate::api::store::HistoryEntry;
use crate::api::{
    self, checks, curl, http, openapi, postman, Act, ApiMsg, ApiTab, AuthField, Dialog, NameFor,
    Removal, ResponseView, Table,
};

/// The time as the history shows it.
fn now() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

impl SimpleEditApp {
    /// A tab for a request, and the tab in front.
    fn api_open(&mut self, request: Request, origin: Option<(u64, u64)>) -> Task<Message> {
        self.api.counter += 1;
        let title = {
            let t = request.title();
            if t.is_empty() {
                t!("api.untitled").to_string()
            } else {
                t
            }
        };
        let path = PathBuf::from(format!("api://{}/{}", self.api.counter, title));
        self.api_tabs
            .insert(path.clone(), ApiTab::new(request, origin));
        self.sidebar.add_file(path.clone());
        self.activate(path)
    }

    /// The tab that holds a request of a collection, if one is open.
    fn api_tab_of(&self, collection: u64, request: u64) -> Option<PathBuf> {
        self.api_tabs
            .iter()
            .find(|(_, t)| t.origin == Some((collection, request)))
            .map(|(p, _)| p.clone())
    }

    /// The tabs whose request no longer is in its collection come loose.
    fn api_release_orphans(&mut self) {
        let alive: Vec<(u64, u64)> = self
            .api_tabs
            .values()
            .filter_map(|t| t.origin)
            .filter(|(c, r)| {
                self.api
                    .collection(*c)
                    .is_some_and(|coll| coll.request(*r).is_some())
            })
            .collect();
        for tab in self.api_tabs.values_mut() {
            if let Some(origin) = tab.origin {
                if !alive.contains(&origin) {
                    tab.origin = None;
                }
            }
        }
    }

    pub(super) fn api_message(&mut self, msg: ApiMsg) -> Task<Message> {
        match msg {
            ApiMsg::Tab(path, act) => self.api_act(path, act),
            ApiMsg::NewRequest => self.api_open(Request::new(""), None),
            ApiMsg::OpenRequest(collection, id) => {
                if let Some(path) = self.api_tab_of(collection, id) {
                    return self.activate(path);
                }
                let request = self
                    .api
                    .collection(collection)
                    .and_then(|c| c.request(id))
                    .cloned();
                match request {
                    Some(request) => self.api_open(request, Some((collection, id))),
                    None => Task::none(),
                }
            }
            ApiMsg::OpenHistory(at) => {
                let request = self.api.history.get(at).map(|e| e.request.clone());
                match request {
                    Some(mut request) => {
                        request.id = 0;
                        self.api_open(request, None)
                    }
                    None => Task::none(),
                }
            }
            ApiMsg::Toggle(id) => {
                if !self.api.open.remove(&id) {
                    self.api.open.insert(id);
                }
                Task::none()
            }
            ApiMsg::Filter(text) => {
                self.api.filter = text;
                Task::none()
            }
            ApiMsg::Side(side) => {
                self.api.side = side;
                Task::none()
            }
            ApiMsg::ClearHistory => {
                self.api.history.clear();
                api::store::save_history(&self.api.history);
                Task::none()
            }
            ApiMsg::NewIn(collection, folder) => {
                let id = self.api.new_id();
                let mut request = Request::new(&t!("api.untitled"));
                request.id = id;
                let Some(list) = self
                    .api
                    .collection_mut(collection)
                    .and_then(|c| c.items_of(folder))
                else {
                    return Task::none();
                };
                list.push(Item::Request(request.clone()));
                self.api.open.insert(collection);
                if folder != 0 {
                    self.api.open.insert(folder);
                }
                self.api.save_collection(collection);
                self.api_open(request, Some((collection, id)))
            }
            ApiMsg::AskNewCollection => {
                self.api.dialog = Some(Dialog::Name {
                    what: NameFor::NewCollection,
                    text: String::new(),
                });
                Task::none()
            }
            ApiMsg::AskNewFolder(collection, folder) => {
                self.api.dialog = Some(Dialog::Name {
                    what: NameFor::NewFolder(collection, folder),
                    text: String::new(),
                });
                Task::none()
            }
            ApiMsg::AskRename(collection, id) => {
                let name = self.api.collection(collection).and_then(|c| {
                    if id == 0 {
                        Some(c.name.clone())
                    } else {
                        api::folder_of(c, id)
                            .map(|f| f.name.clone())
                            .or_else(|| c.request(id).map(|r| r.title()))
                    }
                });
                if let Some(text) = name {
                    self.api.dialog = Some(Dialog::Name {
                        what: NameFor::Rename(collection, id),
                        text,
                    });
                }
                Task::none()
            }
            ApiMsg::AskDelete(collection, id) => {
                self.api.dialog = Some(Dialog::Delete {
                    what: Removal::Item(collection, id),
                });
                Task::none()
            }
            ApiMsg::Duplicate(collection, id) => {
                let new_id = self.api.new_id();
                let suffix = t!("api.copy_suffix").to_string();
                let copy = self
                    .api
                    .collection_mut(collection)
                    .and_then(|c| c.duplicate(id, new_id, &suffix));
                if copy.is_some() {
                    self.api.save_collection(collection);
                }
                Task::none()
            }
            ApiMsg::CopyCurlOf(collection, id) => {
                let request = self
                    .api
                    .collection(collection)
                    .and_then(|c| c.request(id))
                    .cloned();
                match request {
                    Some(request) => {
                        let command = curl::export(&request.resolve(&self.api.variables()));
                        self.set_status(t!("api.curl_copied"), false);
                        iced::clipboard::write(command)
                    }
                    None => Task::none(),
                }
            }
            ApiMsg::DialogText(text) => {
                if let Some(Dialog::Name { text: value, .. }) = self.api.dialog.as_mut() {
                    *value = text;
                }
                Task::none()
            }
            ApiMsg::DialogOk => self.api_dialog_ok(),
            ApiMsg::DialogClose => {
                self.api_close_dialog();
                Task::none()
            }
            ApiMsg::OpenCurl => {
                self.api.curl = text_editor::Content::new();
                self.api.curl_error = None;
                self.api.dialog = Some(Dialog::Curl);
                Task::none()
            }
            ApiMsg::CurlAction(action) => {
                self.api.curl.perform(action);
                self.api.curl_error = None;
                Task::none()
            }
            ApiMsg::CurlRead => match curl::parse(&self.api.curl.text()) {
                Ok(request) => {
                    self.api.dialog = None;
                    self.api_open(request, None)
                }
                Err(error) => {
                    self.api.curl_error = Some(error);
                    Task::none()
                }
            },
            ApiMsg::OpenEnvironments => {
                self.api.editing_env = self
                    .api
                    .active_env
                    .or_else(|| self.api.environments.first().map(|e| e.id));
                self.api.dialog = Some(Dialog::Environments);
                Task::none()
            }
            ApiMsg::SelectEnv(id) => {
                self.api.active_env = id;
                self.api.save_environments();
                Task::none()
            }
            ApiMsg::EditEnv(id) => {
                self.api.editing_env = Some(id);
                self.api.save_environments();
                Task::none()
            }
            ApiMsg::AskNewEnv => {
                self.api.dialog = Some(Dialog::Name {
                    what: NameFor::NewEnvironment,
                    text: String::new(),
                });
                Task::none()
            }
            ApiMsg::DuplicateEnv(id) => {
                let Some(mut copy) = self.api.environment(id).cloned() else {
                    return Task::none();
                };
                copy.id = self.api.new_id();
                copy.name = format!("{} {}", copy.name, t!("api.copy_suffix"));
                self.api.editing_env = Some(copy.id);
                self.api.environments.push(copy);
                self.api.save_environments();
                Task::none()
            }
            ApiMsg::AskDeleteEnv(id) => {
                self.api.dialog = Some(Dialog::Delete {
                    what: Removal::Environment(id),
                });
                Task::none()
            }
            ApiMsg::EnvName(name) => {
                self.api_env_mut(|env| env.name = name);
                Task::none()
            }
            ApiMsg::VarKey(i, key) => {
                let id = self.api.editing_env;
                self.api_env_mut(|env| {
                    if let Some(v) = env.vars.get_mut(i) {
                        if let (true, Some(id)) = (v.secret, id) {
                            crate::api::store::forget_secret(id, &v.key);
                        }
                        v.key = key;
                    }
                });
                Task::none()
            }
            ApiMsg::VarValue(i, value) => {
                self.api_env_mut(|env| {
                    if let Some(v) = env.vars.get_mut(i) {
                        v.value = value;
                    }
                });
                Task::none()
            }
            ApiMsg::VarEnabled(i, on) => {
                self.api_env_mut(|env| {
                    if let Some(v) = env.vars.get_mut(i) {
                        v.enabled = on;
                    }
                });
                Task::none()
            }
            ApiMsg::VarSecret(i, on) => {
                let id = self.api.editing_env;
                self.api_env_mut(|env| {
                    if let Some(v) = env.vars.get_mut(i) {
                        if let (true, false, Some(id)) = (v.secret, on, id) {
                            crate::api::store::forget_secret(id, &v.key);
                        }
                        v.secret = on;
                    }
                });
                Task::none()
            }
            ApiMsg::VarAdd => {
                self.api_env_mut(|env| env.vars.push(Variable::default()));
                Task::none()
            }
            ApiMsg::VarDel(i) => {
                let id = self.api.editing_env;
                self.api_env_mut(|env| {
                    if i < env.vars.len() {
                        let v = env.vars.remove(i);
                        if let (true, Some(id)) = (v.secret, id) {
                            crate::api::store::forget_secret(id, &v.key);
                        }
                    }
                });
                Task::none()
            }
            ApiMsg::ImportFile => Task::perform(
                async {
                    let file = rfd::AsyncFileDialog::new()
                        .add_filter("Postman, JSON", &["json"])
                        .pick_file()
                        .await?;
                    let path = file.path().to_path_buf();
                    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string());
                    Some((path, text))
                },
                |picked| Message::Api(ApiMsg::Imported(picked)),
            ),
            ApiMsg::ImportRead(read) => self.api_imported(read),
            ApiMsg::Imported(picked) => {
                let Some((path, text)) = picked else {
                    return Task::none();
                };
                self.api_import(&path, text)
            }
            ApiMsg::RunCollection(id) => {
                let Some(collection) = self.api.collection(id) else {
                    return Task::none();
                };
                fn flat(items: &[api::model::Item], out: &mut Vec<Request>) {
                    for item in items {
                        match item {
                            Item::Request(r) => out.push(r.clone()),
                            Item::Folder(f) => flat(&f.items, out),
                        }
                    }
                }
                let mut requests = Vec::new();
                flat(&collection.items, &mut requests);
                if requests.is_empty() {
                    return Task::none();
                }
                // A run is on its way: its status stays, nothing starts twice.
                if self.api.run.is_some() {
                    return Task::none();
                }
                let name = collection.name.clone();
                self.set_status(
                    t!("api.running", name = name.as_str(), n = requests.len()),
                    false,
                );
                let (task, handle) = Task::perform(
                    checks::run_all(requests, self.api.variables()),
                    move |(results, kept)| {
                        Message::Api(ApiMsg::RunDone(name.clone(), results, kept))
                    },
                )
                .abortable();
                self.api.run = Some(handle);
                task
            }
            ApiMsg::CancelRun => {
                if let Some(handle) = self.api.run.take() {
                    handle.abort();
                    self.set_status(t!("api.run_stopped"), false);
                }
                Task::none()
            }
            ApiMsg::RunDone(name, results, kept) => {
                self.api.run = None;
                let mut lines = Vec::new();
                let (mut ok, mut bad) = (0, 0);
                for r in &results {
                    let good = r.error.is_none() && r.failed.is_empty();
                    if good {
                        ok += 1;
                    } else {
                        bad += 1;
                    }
                    let mark = if good { "✓" } else { "✗" };
                    let how = match (&r.error, r.status) {
                        (Some(e), _) => e.clone(),
                        (None, Some(s)) => {
                            format!("{s} · {} ms · {}", r.ms, t!("api.run_tests", n = r.passed))
                        }
                        _ => String::new(),
                    };
                    lines.push(format!("{mark} {} — {how}", r.name));
                    for f in &r.failed {
                        lines.push(format!("      ✗ {f}"));
                    }
                }
                lines.insert(
                    0,
                    t!("api.run_summary", name = name.as_str(), ok = ok, bad = bad).to_string(),
                );
                let mut changed = false;
                for (variable, value) in kept {
                    changed |= self.api.set_variable(&variable, &value);
                }
                if changed {
                    self.api.save_environments();
                }
                self.api.dialog = Some(Dialog::Report(lines));
                Task::none()
            }
            ApiMsg::ExportCollection(id) => {
                let Some(collection) = self.api.collection(id) else {
                    return Task::none();
                };
                let name = format!("{}.postman_collection.json", collection.name);
                let text = postman::export_collection(collection);
                Task::perform(save_text(name, text), |done| {
                    Message::Api(ApiMsg::Exported(done))
                })
            }
            ApiMsg::ExportEnv(id) => {
                let Some(env) = self.api.environment(id) else {
                    return Task::none();
                };
                let name = format!("{}.postman_environment.json", env.name);
                let text = postman::export_environment(env, false);
                Task::perform(save_text(name, text), |done| {
                    Message::Api(ApiMsg::Exported(done))
                })
            }
            ApiMsg::Exported(done) => {
                match done {
                    Some(Ok(path)) => {
                        self.set_status(t!("api.exported", path = path.as_str()), false)
                    }
                    Some(Err(error)) => self.set_status(error, true),
                    None => {}
                }
                Task::none()
            }
        }
    }

    /// Closes the open dialog, whichever way; the environments are kept.
    pub(super) fn api_close_dialog(&mut self) {
        if matches!(self.api.dialog, Some(Dialog::Environments)) {
            self.api.save_environments();
        }
        self.api.dialog = None;
    }

    /// Changes the environment that is open in its window.
    fn api_env_mut(&mut self, f: impl FnOnce(&mut Environment)) {
        let Some(id) = self.api.editing_env else {
            return;
        };
        if let Some(env) = self.api.environments.iter_mut().find(|e| e.id == id) {
            f(env);
        }
    }

    /// What a file the user picked holds: a collection or an environment.
    fn api_import(
        &mut self,
        path: &std::path::Path,
        text: Result<String, String>,
    ) -> Task<Message> {
        let report = |lines: Vec<String>| Some(Dialog::Report(lines));
        let text = match text {
            Ok(t) => t,
            Err(e) => {
                self.api.dialog = report(vec![format!("{}: {e}", path.display())]);
                return Task::none();
            }
        };
        if postman::is_environment(&text) {
            match postman::import_environment(&text) {
                Ok(mut env) => {
                    env.id = self.api.new_id();
                    let name = env.name.clone();
                    self.api.editing_env = Some(env.id);
                    self.api.environments.push(env);
                    self.api.save_environments();
                    self.set_status(t!("api.imported_env", name = name.as_str()), false);
                }
                Err(e) => self.api.dialog = report(vec![e]),
            }
            return Task::none();
        }
        // A big description takes time to read: not on the interface's thread.
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    openapi::import_if_openapi(&text)
                        .unwrap_or_else(|| postman::import_collection(&text))
                })
                .await
                .unwrap_or_else(|e| Err(e.to_string()))
            },
            |read| Message::Api(ApiMsg::ImportRead(read)),
        )
    }

    /// What reading a collection or a description gave.
    fn api_imported(&mut self, read: Result<postman::Imported, String>) -> Task<Message> {
        let report = |lines: Vec<String>| Some(Dialog::Report(lines));
        match read {
            Ok(imported) => {
                let mut collection = imported.collection;
                collection.id = self.api.new_id();
                let mut next = self.api.next_id;
                collection.number(&mut next);
                self.api.next_id = next;
                let (id, name, count) =
                    (collection.id, collection.name.clone(), collection.count());
                self.api.collections.push(collection);
                self.api.save_collection(id);
                self.api.open.insert(id);
                let mut lines =
                    vec![t!("api.imported", name = name.as_str(), n = count).to_string()];
                // The variables of the collection become an environment.
                if !imported.variables.is_empty() {
                    let env = Environment {
                        id: self.api.new_id(),
                        name: name.clone(),
                        vars: imported.variables,
                    };
                    lines.push(t!("api.imported_vars", name = name.as_str()).to_string());
                    self.api.environments.push(env);
                    self.api.save_environments();
                }
                lines.extend(imported.warnings);
                if lines.len() > 1 {
                    self.api.dialog = report(lines);
                } else {
                    self.set_status(lines.remove(0), false);
                }
            }
            Err(e) => self.api.dialog = report(vec![e]),
        }
        Task::none()
    }

    fn api_dialog_ok(&mut self) -> Task<Message> {
        let Some(dialog) = self.api.dialog.clone() else {
            return Task::none();
        };
        match dialog {
            Dialog::Name { what, text } => {
                let name = text.trim().to_string();
                if name.is_empty() {
                    return Task::none();
                }
                self.api.dialog = None;
                match what {
                    NameFor::NewCollection => {
                        let id = self.api.new_id();
                        self.api.collections.push(api::model::Collection {
                            id,
                            name,
                            items: Vec::new(),
                        });
                        self.api.open.insert(id);
                        self.api.save_collection(id);
                    }
                    NameFor::NewFolder(collection, parent) => {
                        let id = self.api.new_id();
                        if let Some(list) = self
                            .api
                            .collection_mut(collection)
                            .and_then(|c| c.items_of(parent))
                        {
                            list.push(Item::Folder(Folder {
                                id,
                                name,
                                items: Vec::new(),
                            }));
                            self.api.open.insert(collection);
                            self.api.open.insert(id);
                            self.api.save_collection(collection);
                        }
                    }
                    NameFor::Rename(collection, id) => {
                        if id == 0 {
                            if let Some(c) = self.api.collection_mut(collection) {
                                c.name = name;
                            }
                        } else if let Some(c) = self.api.collection_mut(collection) {
                            c.rename(id, &name);
                        }
                        self.api.save_collection(collection);
                        // A tab of that request takes the new name.
                        for tab in self.api_tabs.values_mut() {
                            if tab.origin == Some((collection, id)) {
                                let new = self
                                    .api
                                    .collection(collection)
                                    .and_then(|c| c.request(id))
                                    .map(|r| r.name.clone())
                                    .unwrap_or_default();
                                tab.request.name = new.clone();
                                tab.kept.name = new;
                            }
                        }
                    }
                    NameFor::NewEnvironment => {
                        let id = self.api.new_id();
                        self.api.environments.push(Environment {
                            id,
                            name,
                            vars: Vec::new(),
                        });
                        self.api.editing_env = Some(id);
                        self.api.save_environments();
                        // The window of the environments comes back.
                        self.api.dialog = Some(Dialog::Environments);
                    }
                }
            }
            Dialog::Delete { what } => {
                self.api.dialog = None;
                match what {
                    Removal::Item(collection, 0) => {
                        self.api.collections.retain(|c| c.id != collection);
                        api::store::delete_collection(collection);
                    }
                    Removal::Item(collection, id) => {
                        if let Some(c) = self.api.collection_mut(collection) {
                            c.remove(id);
                        }
                        self.api.save_collection(collection);
                    }
                    Removal::Environment(id) => {
                        if let Some(env) = self.api.environment(id).cloned() {
                            api::store::forget_secrets(&env);
                        }
                        self.api.environments.retain(|e| e.id != id);
                        if self.api.active_env == Some(id) {
                            self.api.active_env = None;
                        }
                        if self.api.editing_env == Some(id) {
                            self.api.editing_env = self.api.environments.first().map(|e| e.id);
                        }
                        self.api.save_environments();
                        self.api.dialog = Some(Dialog::Environments);
                    }
                }
                self.api_release_orphans();
            }
            _ => self.api.dialog = None,
        }
        Task::none()
    }

    /// What a request tab is asked.
    fn api_act(&mut self, path: PathBuf, act: Act) -> Task<Message> {
        let variables = self.api.variables();
        let Some(tab) = self.api_tabs.get_mut(&path) else {
            return Task::none();
        };
        match act {
            Act::Method(m) => tab.request.method = m,
            Act::Url(u) => tab.request.url = u,
            Act::Name(n) => tab.request.name = n,
            Act::Section(s) => tab.section = s,
            Act::PairAdd(table) => pairs_mut(tab, table).push(Pair::default()),
            Act::PairDel(table, i) => {
                let list = pairs_mut(tab, table);
                if i < list.len() {
                    list.remove(i);
                }
            }
            Act::PairKey(table, i, key) => {
                if let Some(p) = pairs_mut(tab, table).get_mut(i) {
                    p.key = key;
                }
            }
            Act::PairValue(table, i, value) => {
                if let Some(p) = pairs_mut(tab, table).get_mut(i) {
                    p.value = value;
                }
            }
            Act::PairOn(table, i, on) => {
                if let Some(p) = pairs_mut(tab, table).get_mut(i) {
                    p.enabled = on;
                }
            }
            Act::Preset(name) => tab.request.headers.push(Pair::new(&name, "")),
            Act::BodyKind(kind) => tab.request.body.kind = kind,
            Act::Body(action) => {
                let edits = matches!(action, text_editor::Action::Edit(_));
                tab.body_editor.perform(action);
                if edits {
                    let text = tab.body_editor.text();
                    tab.request.body.text = text.strip_suffix('\n').unwrap_or(&text).to_string();
                    tab.body_changed();
                }
            }
            Act::BodyFormat => {
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&tab.request.body.text)
                {
                    if let Ok(laid) = serde_json::to_string_pretty(&value) {
                        tab.body_editor = text_editor::Content::with_text(&laid);
                        tab.request.body.text = laid;
                        tab.body_changed();
                    }
                }
            }
            Act::BodyFile => {
                return Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .pick_file()
                            .await
                            .map(|f| f.path().to_path_buf())
                    },
                    move |picked| {
                        Message::Api(ApiMsg::Tab(path.clone(), Act::BodyFilePicked(picked)))
                    },
                );
            }
            Act::BodyFilePicked(picked) => {
                if let Some(file) = picked {
                    tab.request.body.file = file.to_string_lossy().to_string();
                }
            }
            Act::AuthKind(kind) => tab.request.auth.kind = kind,
            Act::Auth(field, value) => {
                let auth = &mut tab.request.auth;
                match field {
                    AuthField::Token => auth.token = value,
                    AuthField::Username => auth.username = value,
                    AuthField::Password => auth.password = value,
                    AuthField::KeyName => auth.key_name = value,
                    AuthField::KeyValue => auth.key_value = value,
                }
            }
            Act::KeyInHeader(on) => tab.request.auth.key_in_header = on,
            Act::Timeout(text) => {
                let digits: String = text.chars().filter(char::is_ascii_digit).take(5).collect();
                tab.request.settings.timeout_secs = digits.parse().unwrap_or(0);
            }
            Act::Redirects(on) => tab.request.settings.follow_redirects = on,
            Act::VerifyTls(on) => tab.request.settings.verify_tls = on,
            Act::Send => {
                if tab.sending {
                    return Task::none();
                }
                let resolved = tab.request.resolve(&variables);
                tab.token += 1;
                tab.sending = true;
                tab.error = None;
                tab.notice = None;
                let token = tab.token;
                let target = path.clone();
                // The body is read, laid out and tested off the interface
                // thread: a big answer must not freeze it.
                let checks = tab.request.checks.clone();
                let extracts = tab.request.extracts.clone();
                let vars = variables.clone();
                let work = async move {
                    let response = http::send(resolved).await?;
                    tokio::task::spawn_blocking(move || {
                        api::process(response, &checks, &extracts, &vars)
                    })
                    .await
                    .map_err(|e| e.to_string())
                };
                let (task, handle) = Task::perform(work, move |result| {
                    Message::Api(ApiMsg::Tab(
                        target.clone(),
                        Act::Sent(token, Box::new(result)),
                    ))
                })
                .abortable();
                tab.abort = Some(handle);
                return task;
            }
            Act::Cancel => {
                if let Some(handle) = tab.abort.take() {
                    handle.abort();
                }
                tab.sending = false;
                tab.token += 1;
                tab.notice = Some(t!("api.cancelled").to_string());
            }
            Act::Sent(token, result) => {
                if token != tab.token || !tab.sending {
                    return Task::none();
                }
                tab.sending = false;
                tab.abort = None;
                let request = tab.request.clone();
                let mut kept: Vec<(String, Result<String, String>)> = Vec::new();
                let entry = match *result {
                    Ok(mut done) => {
                        let entry = HistoryEntry {
                            time: now(),
                            request,
                            status: Some(done.response.status),
                            ms: done.response.ms,
                            error: None,
                        };
                        kept = std::mem::take(&mut done.kept);
                        tab.response = Some(ResponseView::new(done));
                        tab.error = None;
                        tab.resp_section = api::RespSection::Body;
                        entry
                    }
                    Err(error) => {
                        tab.response = None;
                        tab.error = Some(error.clone());
                        HistoryEntry {
                            time: now(),
                            request,
                            status: None,
                            ms: 0,
                            error: Some(error),
                        }
                    }
                };
                // The values asked for go to the environment that is on.
                let mut notes: Vec<String> = Vec::new();
                let mut names: Vec<String> = Vec::new();
                for (name, found) in kept {
                    match found {
                        Ok(value) => {
                            if self.api.set_variable(&name, &value) {
                                names.push(name);
                            } else {
                                notes.push(
                                    t!("api.value_not_kept", name = name.as_str()).to_string(),
                                );
                            }
                        }
                        Err(e) => notes.push(format!("{name}: {e}")),
                    }
                }
                if !names.is_empty() {
                    self.api.save_environments();
                    notes.insert(
                        0,
                        t!("api.kept", names = names.join(", ").as_str()).to_string(),
                    );
                }
                if !notes.is_empty() {
                    if let Some(tab) = self.api_tabs.get_mut(&path) {
                        tab.notice = Some(notes.join(" · "));
                    }
                }
                self.api.remember(entry);
            }
            Act::CheckAdd => tab.request.checks.push(api::model::Check::default()),
            Act::CheckDel(i) => {
                if i < tab.request.checks.len() {
                    tab.request.checks.remove(i);
                }
            }
            Act::CheckOn(i, on) => {
                if let Some(c) = tab.request.checks.get_mut(i) {
                    c.enabled = on;
                }
            }
            Act::CheckSource(i, source) => {
                if let Some(c) = tab.request.checks.get_mut(i) {
                    c.source = source;
                }
            }
            Act::CheckName(i, name) => {
                if let Some(c) = tab.request.checks.get_mut(i) {
                    c.name = name;
                }
            }
            Act::CheckOp(i, op) => {
                if let Some(c) = tab.request.checks.get_mut(i) {
                    c.op = op;
                }
            }
            Act::CheckExpected(i, expected) => {
                if let Some(c) = tab.request.checks.get_mut(i) {
                    c.expected = expected;
                }
            }
            Act::ExtractAdd => tab.request.extracts.push(api::model::Extract::default()),
            Act::ExtractDel(i) => {
                if i < tab.request.extracts.len() {
                    tab.request.extracts.remove(i);
                }
            }
            Act::ExtractOn(i, on) => {
                if let Some(e) = tab.request.extracts.get_mut(i) {
                    e.enabled = on;
                }
            }
            Act::ExtractVar(i, variable) => {
                if let Some(e) = tab.request.extracts.get_mut(i) {
                    e.variable = variable;
                }
            }
            Act::ExtractFrom(i, source) => {
                if let Some(e) = tab.request.extracts.get_mut(i) {
                    e.source = source;
                }
            }
            Act::ExtractPath(i, path) => {
                if let Some(e) = tab.request.extracts.get_mut(i) {
                    e.path = path;
                }
            }
            Act::Resp(section) => tab.resp_section = section,
            Act::Pretty(on) => {
                if let Some(view) = tab.response.as_mut() {
                    if view.pretty.is_some() {
                        view.show_pretty = on;
                        view.show();
                    }
                }
            }
            Act::RespEdit(action) => {
                // It can be selected and moved in, not typed in.
                if !matches!(action, text_editor::Action::Edit(_)) {
                    if let Some(view) = tab.response.as_mut() {
                        view.editor.perform(action);
                    }
                }
            }
            Act::Search(text) => {
                if let Some(view) = tab.response.as_mut() {
                    view.search = text;
                }
            }
            Act::SearchNext => {
                if let Some(view) = tab.response.as_mut() {
                    let text = view.editor.text();
                    let cursor = view.editor.cursor();
                    let from = crate::database::console::caret_offset(
                        &text,
                        cursor.position.line,
                        cursor.position.column,
                    );
                    // After the match that is selected already.
                    let from = if cursor.selection.is_some() {
                        from
                    } else {
                        from.saturating_sub(0)
                    };
                    if let Some((start, end)) = api::find_next(&text, &view.search, from) {
                        let (l0, c0) = api::position_of(&text, start);
                        let (l1, c1) = api::position_of(&text, end);
                        view.editor.move_to(text_editor::Cursor {
                            position: text_editor::Position {
                                line: l1,
                                column: c1,
                            },
                            selection: Some(text_editor::Position {
                                line: l0,
                                column: c0,
                            }),
                        });
                    }
                }
            }
            Act::SaveBody => {
                let Some(view) = tab.response.as_ref() else {
                    return Task::none();
                };
                let extension = match view.kind {
                    http::Kind::Json => "json",
                    http::Kind::Xml => "xml",
                    http::Kind::Html => "html",
                    http::Kind::Text => "txt",
                    http::Kind::Binary => "bin",
                };
                let name = format!("response.{extension}");
                let bytes = view.response.body.clone();
                return Task::perform(save_bytes(name, bytes), move |done| {
                    Message::Api(ApiMsg::Tab(path.clone(), Act::BodySaved(done)))
                });
            }
            Act::BodySaved(done) => match done {
                Some(Ok(file)) => {
                    let cut = tab.response.as_ref().is_some_and(|v| v.response.truncated);
                    let size = api::view::size(http::MAX_BODY);
                    tab.notice = Some(if cut {
                        t!("api.exported_truncated", path = file.as_str(), size = size).to_string()
                    } else {
                        t!("api.exported", path = file.as_str()).to_string()
                    })
                }
                Some(Err(error)) => tab.error = Some(error),
                None => {}
            },
            Act::CopyBody => {
                if let Some(view) = tab.response.as_ref() {
                    let text = match (&view.pretty, view.show_pretty) {
                        (Some(pretty), true) => pretty.clone(),
                        _ => view.response.text(),
                    };
                    return iced::clipboard::write(text);
                }
            }
            Act::Save => return self.api_save(path),
            Act::SaveTarget(id) => tab.save_target = Some(id),
            Act::CopyCurl => {
                let command = curl::export(&tab.request.resolve(&variables));
                self.set_status(t!("api.curl_copied"), false);
                return iced::clipboard::write(command);
            }
        }
        Task::none()
    }

    /// Keeps the request of a tab in its collection (a new one, in the one
    /// that was chosen).
    fn api_save(&mut self, path: PathBuf) -> Task<Message> {
        let Some(tab) = self.api_tabs.get(&path) else {
            return Task::none();
        };
        let request = tab.request.clone();
        match tab.origin {
            Some((collection, id)) => {
                if let Some(slot) = self
                    .api
                    .collection_mut(collection)
                    .and_then(|c| c.request_mut(id))
                {
                    let mut kept = request;
                    kept.id = id;
                    *slot = kept;
                }
                self.api.save_collection(collection);
                if let Some(tab) = self.api_tabs.get_mut(&path) {
                    tab.kept = tab.request.clone();
                }
            }
            None => {
                // The collection that was chosen, or the first, or a new one.
                let target = tab
                    .save_target
                    .filter(|id| self.api.collection(*id).is_some())
                    .or_else(|| self.api.collections.first().map(|c| c.id));
                let collection = match target {
                    Some(id) => id,
                    None => {
                        let id = self.api.new_id();
                        self.api.collections.push(api::model::Collection {
                            id,
                            name: t!("api.default_collection").to_string(),
                            items: Vec::new(),
                        });
                        id
                    }
                };
                let id = self.api.new_id();
                let mut kept = request;
                kept.id = id;
                if kept.name.trim().is_empty() {
                    kept.name = kept.url.trim().to_string();
                }
                if let Some(c) = self.api.collection_mut(collection) {
                    c.items.push(Item::Request(kept.clone()));
                }
                self.api.open.insert(collection);
                self.api.save_collection(collection);
                if let Some(tab) = self.api_tabs.get_mut(&path) {
                    tab.request = kept.clone();
                    tab.body_changed();
                    tab.kept = kept;
                    tab.origin = Some((collection, id));
                }
            }
        }
        Task::none()
    }
}

fn pairs_mut(tab: &mut ApiTab, table: Table) -> &mut Vec<Pair> {
    match table {
        Table::Params => &mut tab.request.params,
        Table::Headers => &mut tab.request.headers,
        Table::Form => &mut tab.request.body.form,
    }
}

/// Asks where to write a text, and writes it: where it went.
async fn save_text(name: String, text: String) -> Option<Result<String, String>> {
    save_bytes(name, text.into_bytes()).await
}

async fn save_bytes(name: String, bytes: Vec<u8>) -> Option<Result<String, String>> {
    let file = rfd::AsyncFileDialog::new()
        .set_file_name(name)
        .save_file()
        .await?;
    let path = file.path().to_path_buf();
    Some(
        std::fs::write(&path, bytes)
            .map(|_| path.display().to_string())
            .map_err(|e| format!("{}: {e}", path.display())),
    )
}
