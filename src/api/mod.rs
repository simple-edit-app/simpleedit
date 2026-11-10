//! A light version of Postman: requests to an API, the collections that keep
//! them, the environments with their variables.

pub mod checks;
pub mod curl;
pub mod http;
pub mod model;
pub mod openapi;
pub mod postman;
pub mod store;
#[cfg(test)]
pub mod testing;
pub mod view;

use std::collections::HashSet;
use std::path::PathBuf;

use iced::widget::text_editor;

use http::Response;
use model::{AuthKind, BodyKind, Collection, Environment, Folder, Item, Method, Request};
use store::HistoryEntry;

/// The tabs of a request hold these tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Table {
    Params,
    Headers,
    Form,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReqSection {
    Params,
    Headers,
    Body,
    Auth,
    Settings,
    /// The tests of the answer, and the values kept from it.
    Tests,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RespSection {
    Body,
    Headers,
    Cookies,
    /// How the tests of the request went.
    Tests,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthField {
    Token,
    Username,
    Password,
    KeyName,
    KeyValue,
}

/// The two lists of the side panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SideTab {
    Collections,
    History,
}

/// What a request tab is asked.
#[derive(Debug, Clone)]
pub enum Act {
    Method(Method),
    Url(String),
    Name(String),
    Section(ReqSection),
    PairAdd(Table),
    PairDel(Table, usize),
    PairKey(Table, usize, String),
    PairValue(Table, usize, String),
    PairOn(Table, usize, bool),
    /// A header that is usual, added with its name.
    Preset(String),
    BodyKind(BodyKind),
    Body(text_editor::Action),
    /// The JSON of the body, laid out.
    BodyFormat,
    BodyFile,
    BodyFilePicked(Option<PathBuf>),
    AuthKind(AuthKind),
    Auth(AuthField, String),
    KeyInHeader(bool),
    Timeout(String),
    Redirects(bool),
    VerifyTls(bool),
    Send,
    Cancel,
    /// The answer to a request: the number of the sending, and what came.
    Sent(u64, Box<Result<Response, String>>),
    Resp(RespSection),
    Pretty(bool),
    /// Moving or selecting in the answer (it cannot be typed in).
    RespEdit(text_editor::Action),
    Search(String),
    SearchNext,
    SaveBody,
    BodySaved(Option<Result<String, String>>),
    CopyBody,
    Save,
    SaveTarget(u64),
    CopyCurl,
    CheckAdd,
    CheckDel(usize),
    CheckOn(usize, bool),
    CheckSource(usize, model::CheckSource),
    CheckName(usize, String),
    CheckOp(usize, model::CheckOp),
    CheckExpected(usize, String),
    ExtractAdd,
    ExtractDel(usize),
    ExtractOn(usize, bool),
    ExtractVar(usize, String),
    ExtractFrom(usize, model::ExtractSource),
    ExtractPath(usize, String),
}

/// What the user is asked in a window.
#[derive(Debug, Clone, PartialEq)]
pub enum Dialog {
    /// A name for a new collection or folder, or to rename one.
    Name { what: NameFor, text: String },
    /// A question before something is deleted.
    Delete { what: Removal },
    /// A `curl` command to read.
    Curl,
    /// The environments, to edit.
    Environments,
    /// What an import said.
    Report(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NameFor {
    NewCollection,
    /// A folder in a collection (0: at the top) .
    NewFolder(u64, u64),
    /// A collection (id of the item 0), a folder or a request.
    Rename(u64, u64),
    NewEnvironment,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Removal {
    /// A collection (item 0), a folder or a request.
    Item(u64, u64),
    Environment(u64),
}

/// A request open in a tab.
pub struct ApiTab {
    pub request: Request,
    /// The collection and the request it came from, once it is kept.
    pub origin: Option<(u64, u64)>,
    /// The request as it was last kept, to know what changed.
    pub kept: Request,
    pub section: ReqSection,
    pub body_editor: text_editor::Content,
    pub response: Option<ResponseView>,
    pub sending: bool,
    /// The number of the sending that counts: an older answer is dropped.
    pub token: u64,
    pub abort: Option<iced::task::Handle>,
    pub error: Option<String>,
    pub notice: Option<String>,
    pub resp_section: RespSection,
    /// The collection a new request is kept in.
    pub save_target: Option<u64>,
}

/// An answer, ready to be read.
pub struct ResponseView {
    pub response: Response,
    pub kind: http::Kind,
    /// The body laid out, when it can be.
    pub pretty: Option<String>,
    pub show_pretty: bool,
    pub editor: text_editor::Content,
    pub search: String,
    /// The body is longer than what is shown.
    pub cut: bool,
    /// How the tests of the request went on this answer.
    pub outcomes: Vec<checks::Outcome>,
}

/// Most of a body that is put in the viewer.
pub const MAX_SHOWN: usize = 2 * 1024 * 1024;
/// The size up to which a body is parsed to be laid out.
const MAX_PRETTY: usize = 8 * MAX_SHOWN;

impl ResponseView {
    pub fn new(response: Response) -> ResponseView {
        let kind = http::kind_of(response.header("content-type"), &response.body);
        let text = if kind == http::Kind::Binary {
            String::new()
        } else {
            response.text()
        };
        // A minified body is laid out even when it is long: a single line of
        // megabytes freezes the viewer, and only the start of it is shown.
        let pretty = if text.len() <= MAX_PRETTY {
            http::pretty(kind, &text)
        } else {
            None
        };
        let mut view = ResponseView {
            response,
            kind,
            show_pretty: pretty.is_some(),
            pretty,
            editor: text_editor::Content::new(),
            search: String::new(),
            cut: false,
            outcomes: Vec::new(),
        };
        view.show();
        view
    }

    /// Puts the body, laid out or as it came, in the viewer.
    pub fn show(&mut self) {
        let raw = if self.kind == http::Kind::Binary {
            String::new()
        } else {
            self.response.text()
        };
        let shown = match (&self.pretty, self.show_pretty) {
            (Some(pretty), true) => pretty.clone(),
            _ => raw,
        };
        self.cut = shown.len() > MAX_SHOWN;
        let shown = if self.cut {
            let mut end = MAX_SHOWN;
            while !shown.is_char_boundary(end) {
                end -= 1;
            }
            shown[..end].to_string()
        } else {
            shown
        };
        self.editor = text_editor::Content::with_text(&shown);
    }

    /// How many times what is searched is in the body.
    pub fn matches(&self) -> usize {
        let needle = self.search.to_ascii_lowercase();
        if needle.is_empty() {
            return 0;
        }
        self.editor
            .text()
            .to_ascii_lowercase()
            .matches(&needle)
            .count()
    }
}

impl ApiTab {
    pub fn new(request: Request, origin: Option<(u64, u64)>) -> ApiTab {
        let body = request.body.text.clone();
        ApiTab {
            kept: request.clone(),
            request,
            origin,
            section: ReqSection::Params,
            body_editor: text_editor::Content::with_text(&body),
            response: None,
            sending: false,
            token: 0,
            abort: None,
            error: None,
            notice: None,
            resp_section: RespSection::Body,
            save_target: None,
        }
    }

    /// Something changed since the request was kept.
    pub fn changed(&self) -> bool {
        self.request != self.kept
    }
}

/// What the Api panel holds, and the windows over it.
pub struct State {
    pub collections: Vec<Collection>,
    pub environments: Vec<Environment>,
    pub active_env: Option<u64>,
    pub history: Vec<HistoryEntry>,
    /// The next number for a collection, a folder, a request, an environment.
    pub next_id: u64,
    /// The collections and folders that are open in the panel.
    pub open: HashSet<u64>,
    pub filter: String,
    pub side: SideTab,
    pub dialog: Option<Dialog>,
    /// The environment being edited in its window.
    pub editing_env: Option<u64>,
    /// The `curl` command typed in its window.
    pub curl: text_editor::Content,
    pub curl_error: Option<String>,
    /// How many tabs were opened (they are named after it).
    pub counter: u64,
}

impl State {
    pub fn load() -> State {
        let stored = store::load();
        State::from(stored)
    }

    pub fn from(stored: store::Stored) -> State {
        let mut state = State {
            collections: stored.collections,
            environments: stored.environments,
            active_env: stored.active_env,
            history: stored.history,
            next_id: stored.next_id,
            open: HashSet::new(),
            filter: String::new(),
            side: SideTab::Collections,
            dialog: None,
            editing_env: None,
            curl: text_editor::Content::new(),
            curl_error: None,
            counter: 0,
        };
        // Whatever has no number gets one (a file written by hand).
        let mut next = state.next_id;
        for collection in &mut state.collections {
            if collection.id == 0 {
                next += 1;
                collection.id = next;
            }
            collection.number(&mut next);
        }
        state.next_id = next;
        if state
            .active_env
            .is_some_and(|id| !state.environments.iter().any(|e| e.id == id))
        {
            state.active_env = None;
        }
        state
    }

    /// A number that nothing has.
    pub fn new_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    /// Puts a value in a variable of the environment that is on; `false`
    /// when none is.
    pub fn set_variable(&mut self, name: &str, value: &str) -> bool {
        let Some(id) = self.active_env else {
            return false;
        };
        let Some(env) = self.environments.iter_mut().find(|e| e.id == id) else {
            return false;
        };
        match env.vars.iter_mut().find(|v| v.key.trim() == name) {
            Some(var) => {
                var.value = value.to_string();
                var.enabled = true;
            }
            None => env.vars.push(model::Variable {
                enabled: true,
                key: name.to_string(),
                value: value.to_string(),
                secret: false,
            }),
        }
        true
    }

    pub fn collection(&self, id: u64) -> Option<&Collection> {
        self.collections.iter().find(|c| c.id == id)
    }

    pub fn collection_mut(&mut self, id: u64) -> Option<&mut Collection> {
        self.collections.iter_mut().find(|c| c.id == id)
    }

    pub fn environment(&self, id: u64) -> Option<&Environment> {
        self.environments.iter().find(|e| e.id == id)
    }

    /// The variables of the environment that is on.
    pub fn variables(&self) -> std::collections::HashMap<String, String> {
        self.active_env
            .and_then(|id| self.environment(id))
            .map(Environment::values)
            .unwrap_or_default()
    }

    /// Keeps the collection in its file.
    pub fn save_collection(&self, id: u64) {
        if let Some(c) = self.collection(id) {
            store::save_collection(c);
        }
    }

    pub fn save_environments(&self) {
        store::save_environments(&self.environments, self.active_env, self.next_id);
    }

    /// Adds a request that was sent to the history, newest first.
    pub fn remember(&mut self, entry: HistoryEntry) {
        self.history.insert(0, entry);
        self.history.truncate(store::MAX_HISTORY);
        store::save_history(&self.history);
    }

    /// A folder is open in the panel (the filter opens them all).
    pub fn is_open(&self, id: u64) -> bool {
        !self.filter.trim().is_empty() || self.open.contains(&id)
    }
}

/// The messages of the Api panel and its tabs.
#[derive(Debug, Clone)]
pub enum ApiMsg {
    /// A tab of a request is asked something.
    Tab(PathBuf, Act),
    NewRequest,
    OpenRequest(u64, u64),
    OpenHistory(usize),
    Toggle(u64),
    Filter(String),
    Side(SideTab),
    ClearHistory,
    /// A new request in a collection, in a folder of it (0: at the top).
    NewIn(u64, u64),
    AskNewCollection,
    AskNewFolder(u64, u64),
    AskRename(u64, u64),
    AskDelete(u64, u64),
    Duplicate(u64, u64),
    CopyCurlOf(u64, u64),
    DialogText(String),
    DialogOk,
    DialogClose,
    /// The windows over the panel.
    OpenCurl,
    CurlAction(text_editor::Action),
    CurlRead,
    OpenEnvironments,
    /// The environment that is on (none: no variables).
    SelectEnv(Option<u64>),
    EditEnv(u64),
    AskNewEnv,
    DuplicateEnv(u64),
    AskDeleteEnv(u64),
    EnvName(String),
    VarKey(usize, String),
    VarValue(usize, String),
    VarEnabled(usize, bool),
    VarSecret(usize, bool),
    VarAdd,
    VarDel(usize),
    /// Import: pick a file, and what it was; export a collection or an
    /// environment: pick where, and how it went.
    ImportFile,
    Imported(Option<(PathBuf, Result<String, String>)>),
    ExportCollection(u64),
    /// Sends all the requests of a collection, in order, and reports.
    RunCollection(u64),
    /// The results of a run, and the values it kept.
    RunDone(String, Vec<checks::RunResult>, Vec<(String, String)>),
    ExportEnv(u64),
    Exported(Option<Result<String, String>>),
}

/// The folder of a request in a list: its folder items, for the panel.
pub fn folder_of(collection: &Collection, id: u64) -> Option<&Folder> {
    fn find(items: &[Item], id: u64) -> Option<&Folder> {
        for item in items {
            if let Item::Folder(f) = item {
                if f.id == id {
                    return Some(f);
                }
                if let Some(found) = find(&f.items, id) {
                    return Some(found);
                }
            }
        }
        None
    }
    find(&collection.items, id)
}

/// The place of the next `needle` in `text`, from `from` on and then from the
/// start (whatever the case, for the letters of ASCII): its start and end.
pub fn find_next(text: &str, needle: &str, from: usize) -> Option<(usize, usize)> {
    if needle.is_empty() {
        return None;
    }
    let (haystack, needle) = (text.to_ascii_lowercase(), needle.to_ascii_lowercase());
    let from = from.min(haystack.len());
    let start = haystack[from..]
        .find(&needle)
        .map(|i| i + from)
        .or_else(|| haystack.find(&needle))?;
    Some((start, start + needle.len()))
}

/// The line and the column of a place of the text.
pub fn position_of(text: &str, offset: usize) -> (usize, usize) {
    let before = &text[..offset.min(text.len())];
    let line = before.matches('\n').count();
    let column = before.len() - before.rfind('\n').map_or(0, |i| i + 1);
    (line, column)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_next_match_is_found_from_a_place_then_from_the_start() {
        let text = "Alpha beta ALPHA gamma alpha";
        assert_eq!(find_next(text, "alpha", 0), Some((0, 5)));
        assert_eq!(find_next(text, "alpha", 1), Some((11, 16)));
        assert_eq!(find_next(text, "alpha", 17), Some((23, 28)));
        // Past the last one, it goes round.
        assert_eq!(find_next(text, "alpha", 24), Some((0, 5)));
        assert_eq!(find_next(text, "zeta", 0), None);
        assert_eq!(find_next(text, "", 0), None);
        assert_eq!(find_next("", "a", 0), None);
    }

    #[test]
    fn a_place_is_a_line_and_a_column() {
        let text = "ab\ncde\nf";
        assert_eq!(position_of(text, 0), (0, 0));
        assert_eq!(position_of(text, 4), (1, 1));
        assert_eq!(position_of(text, 7), (2, 0));
        assert_eq!(position_of(text, 8), (2, 1));
        assert_eq!(position_of(text, 99), (2, 1));
    }
}
