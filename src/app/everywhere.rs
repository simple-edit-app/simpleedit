//! Search everywhere in the app (Shift twice): the window, what each tab
//! lists, the searches it starts, its keys, and what opening a line does.

use super::*;
use crate::everywhere::{self as ew, Tab};
use crate::finder::TextHit;
use crate::lsp::SymbolHit;
use crate::palette::{Entry, PaletteCommand};
use crate::preferences::{PreferencesMessage, PrefsPage};
use iced::widget::{column, rich_text, span, text_input};
use std::collections::BTreeSet;

/// Lines of a group in the "All" tab before its "more".
const PER_GROUP: usize = 6;
/// Lines listed in one tab at most.
const MOST: usize = 120;
/// Height of a line.
const ROW: f32 = 30.0;
const WIDTH: f32 = 780.0;
const LIST_HEIGHT: f32 = 430.0;
/// How long the typing must rest before the servers and the files are
/// searched.
const SYMBOLS_AFTER: std::time::Duration = std::time::Duration::from_millis(150);
const TEXT_AFTER: std::time::Duration = std::time::Duration::from_millis(250);
/// "All" shows text results when the other groups have fewer lines.
const FEW: usize = 10;

/// A commit of the Git tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    pub hash: String,
    pub subject: String,
    pub author: String,
    pub when: String,
}

/// A setting that is on or off, listed among the actions.
struct Setting {
    label: fn() -> String,
    get: fn(&Config) -> bool,
    set: fn(bool) -> PreferencesMessage,
}

const SETTINGS: &[Setting] = &[
    Setting {
        label: || t!("prefs.word_wrap").to_string(),
        get: |c| c.word_wrap,
        set: PreferencesMessage::WordWrapToggled,
    },
    Setting {
        label: || t!("prefs.show_minimap").to_string(),
        get: |c| c.show_minimap,
        set: PreferencesMessage::ShowMinimapToggled,
    },
    Setting {
        label: || t!("prefs.show_line_numbers").to_string(),
        get: |c| c.show_line_numbers,
        set: PreferencesMessage::ShowLineNumbersToggled,
    },
    Setting {
        label: || t!("prefs.highlight_current_line").to_string(),
        get: |c| c.highlight_current_line,
        set: PreferencesMessage::HighlightCurrentLineToggled,
    },
    Setting {
        label: || t!("prefs.show_page_guide").to_string(),
        get: |c| c.show_page_guide,
        set: PreferencesMessage::ShowPageGuideToggled,
    },
    Setting {
        label: || t!("prefs.use_spaces").to_string(),
        get: |c| c.use_spaces,
        set: PreferencesMessage::UseSpacesToggled,
    },
    Setting {
        label: || t!("prefs.auto_indent").to_string(),
        get: |c| c.auto_indent,
        set: PreferencesMessage::AutoIndentToggled,
    },
    Setting {
        label: || t!("prefs.autocomplete_brackets").to_string(),
        get: |c| c.autocomplete_brackets,
        set: PreferencesMessage::AutocompleteBracketsToggled,
    },
    Setting {
        label: || t!("prefs.autocomplete_quotes").to_string(),
        get: |c| c.autocomplete_quotes,
        set: PreferencesMessage::AutocompleteQuotesToggled,
    },
    Setting {
        label: || t!("prefs.auto_save").to_string(),
        get: |c| c.auto_save,
        set: PreferencesMessage::AutoSaveToggled,
    },
    Setting {
        label: || t!("prefs.completion").to_string(),
        get: |c| c.completion_enabled,
        set: PreferencesMessage::CompletionToggled,
    },
    Setting {
        label: || t!("prefs.completion_auto").to_string(),
        get: |c| c.completion_auto,
        set: PreferencesMessage::CompletionAutoToggled,
    },
    Setting {
        label: || t!("prefs.everywhere_text").to_string(),
        get: |c| c.everywhere_text,
        set: PreferencesMessage::EverywhereTextToggled,
    },
    Setting {
        label: || t!("prefs.everywhere_git").to_string(),
        get: |c| c.everywhere_git,
        set: PreferencesMessage::EverywhereGitToggled,
    },
];

/// A line of the results.
#[derive(Debug, Clone)]
pub enum Row {
    /// The title of a group ("All" tab).
    Group(Tab),
    /// The rest of a group: its tab.
    More(Tab),
    /// A calculation and its result.
    Calc(String, String),
    File(PathBuf, String, Option<(usize, Option<usize>)>),
    Dir(PathBuf, String),
    Symbol(SymbolHit),
    Action(Entry),
    Setting(usize),
    Page(PrefsPage),
    Text(TextHit),
    Branch(String),
    Commit(Commit),
    /// A line that says something (loading, nothing found).
    Note(String),
}

impl Row {
    fn selectable(&self) -> bool {
        !matches!(self, Row::Group(_) | Row::Note(_))
    }
}

/// What is being searched, and what was found.
#[derive(Default)]
pub struct Everywhere {
    pub tab: Option<Tab>,
    pub query: String,
    pub rows: Vec<Row>,
    /// Index in `rows`.
    pub selected: usize,
    /// Libraries and what Git ignores too (Shift twice again).
    pub non_project: bool,
    pub filter_open: bool,
    /// File types left out by the filter.
    pub excluded: BTreeSet<String>,
    pub case_sensitive: bool,
    pub words: bool,
    pub regex: bool,
    files: Vec<(PathBuf, String)>,
    dirs: Vec<(PathBuf, String)>,
    files_loading: bool,
    files_generation: u64,
    symbols: Vec<SymbolHit>,
    symbols_busy: bool,
    symbols_waiting: bool,
    symbols_generation: u64,
    text: Vec<TextHit>,
    /// The query `text` was found for.
    text_for: String,
    text_error: Option<String>,
    text_waiting: bool,
    text_generation: u64,
    branches: Vec<String>,
    commits: Vec<Commit>,
    /// An abbreviation being written for an action.
    pub abbreviating: Option<(PaletteCommand, String)>,
}

pub(super) fn field_id() -> iced::widget::Id {
    iced::widget::Id::new("search-everywhere")
}

fn list_id() -> iced::widget::Id {
    iced::widget::Id::new("search-everywhere-list")
}

fn abbreviation_id() -> iced::widget::Id {
    iced::widget::Id::new("search-everywhere-abbreviation")
}

/// The extension of a path, for the filter of file types.
fn file_type(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

/// The folders of the listed files, shown like them with a final `/`.
fn folders_of(files: &[(PathBuf, String)]) -> Vec<(PathBuf, String)> {
    let mut seen = BTreeSet::new();
    let mut dirs = Vec::new();
    for (path, shown) in files {
        let (mut dir, mut rel) = (path.parent(), shown.rsplit_once('/').map(|(d, _)| d));
        while let (Some(d), Some(r)) = (dir, rel) {
            if r.is_empty() || !seen.insert(r.to_string()) {
                break;
            }
            dirs.push((d.to_path_buf(), format!("{r}/")));
            dir = d.parent();
            rel = r.rsplit_once('/').map(|(d, _)| d);
        }
    }
    dirs.sort_by(|a, b| a.1.cmp(&b.1));
    dirs
}

/// The icon of a symbol, by its LSP `SymbolKind`.
fn symbol_icon(kind: u64) -> (char, (u8, u8, u8)) {
    use crate::completion::Kind as K;
    let as_item = match kind {
        2..=4 => K::Module,
        5 => K::Class,
        6 => K::Method,
        7 => K::Property,
        8 => K::Field,
        9 => K::Constructor,
        10 => K::Enum,
        11 => K::Interface,
        12 => K::Function,
        13 => K::Variable,
        14 => K::Constant,
        22 => K::EnumMember,
        23 => K::Struct,
        24 => K::Event,
        25 => K::Operator,
        26 => K::TypeParameter,
        _ => K::Value,
    };
    as_item.icon()
}

impl SimpleEditApp {
    // ── Opening ──

    /// Opens the search on `tab`; asked again for the same tab (Shift twice
    /// again, the tab's shortcut again), it looks beyond the project too.
    pub(super) fn open_everywhere(&mut self, tab: Tab) -> Task<Message> {
        if self.overlay == ActiveOverlay::Everywhere && self.everywhere.tab == Some(tab) {
            return self.everywhere_non_project();
        }
        let tab = if tab == Tab::Git && !self.config.everywhere_git {
            Tab::All
        } else {
            tab
        };
        self.everywhere.tab = Some(tab);
        self.everywhere.non_project = false;
        self.everywhere.filter_open = false;
        self.everywhere.abbreviating = None;
        self.overlay = ActiveOverlay::Everywhere;
        self.everywhere_refresh();
        // The last search is there again, selected: typing replaces it.
        Task::batch([
            self.everywhere_load_files(),
            self.everywhere_load_git(),
            self.everywhere_searches(),
            iced::widget::operation::focus(field_id()),
            iced::widget::operation::select_all(field_id()),
        ])
    }

    fn everywhere_tab(&self) -> Tab {
        self.everywhere.tab.unwrap_or(Tab::All)
    }

    pub(super) fn everywhere_switch(&mut self, tab: Tab) -> Task<Message> {
        self.everywhere.tab = Some(tab);
        self.everywhere.filter_open = false;
        self.everywhere_refresh();
        Task::batch([
            self.everywhere_load_git(),
            self.everywhere_searches(),
            iced::widget::operation::focus(field_id()),
        ])
    }

    /// Shift twice while it is open: beyond the project, or back.
    pub(super) fn everywhere_non_project(&mut self) -> Task<Message> {
        self.everywhere.non_project = !self.everywhere.non_project;
        self.everywhere.symbols.clear();
        Task::batch([self.everywhere_load_files(), self.everywhere_searches()])
    }

    pub(super) fn everywhere_typed(&mut self, query: String) -> Task<Message> {
        self.everywhere.query = query;
        // What was found for another text must not be taken for this one's.
        if self.everywhere.text_for != self.everywhere.query.trim() {
            self.everywhere.text.clear();
        }
        self.everywhere_refresh();
        self.everywhere_searches()
    }

    // ── Loading ──

    /// Lists the files of the projects (and, beyond the project, all of
    /// them), in the background.
    fn everywhere_load_files(&mut self) -> Task<Message> {
        let roots = self.finder_roots();
        let several = roots.len() > 1;
        let exclude = self.config.projects_exclude.clone();
        let all = self.everywhere.non_project;
        self.everywhere.files_loading = true;
        self.everywhere.files_generation += 1;
        let generation = self.everywhere.files_generation;
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    roots
                        .iter()
                        .flat_map(|root| {
                            let top = root
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_default();
                            let files = if all {
                                crate::finder::list_all_files(root)
                            } else {
                                crate::finder::list_files(root, &exclude, true)
                            };
                            files
                                .into_iter()
                                .map(|file| {
                                    let rel = file
                                        .strip_prefix(root)
                                        .map(|r| r.to_string_lossy().replace('\\', "/"))
                                        .unwrap_or_default();
                                    let shown = if several { format!("{top}/{rel}") } else { rel };
                                    (file, shown)
                                })
                                .collect::<Vec<_>>()
                        })
                        .collect::<Vec<_>>()
                })
                .await
                .unwrap_or_default()
            },
            move |files| Message::EverywhereFiles(generation, files),
        )
    }

    pub(super) fn everywhere_files(
        &mut self,
        generation: u64,
        files: Vec<(PathBuf, String)>,
    ) -> Task<Message> {
        if generation != self.everywhere.files_generation {
            return Task::none();
        }
        self.everywhere.dirs = folders_of(&files);
        self.everywhere.files = files;
        self.everywhere.files_loading = false;
        self.everywhere_refresh();
        // The servers need a file of each language to be asked.
        self.everywhere_searches()
    }

    /// The branches and the last commits, for the Git tab.
    fn everywhere_load_git(&mut self) -> Task<Message> {
        if self.everywhere_tab() != Tab::Git {
            return Task::none();
        }
        let Some(repo) = self.git.repo.clone() else {
            return Task::none();
        };
        Task::perform(
            async move {
                let branches =
                    crate::git::run(&repo, &["branch", "-a", "--format=%(refname:short)"])
                        .await
                        .unwrap_or_default()
                        .lines()
                        .map(str::to_string)
                        .filter(|b| !b.is_empty() && !b.ends_with("/HEAD"))
                        .collect::<Vec<_>>();
                let commits = crate::git::run(
                    &repo,
                    &[
                        "log",
                        "--all",
                        "-n",
                        "2000",
                        "--format=%H%x1f%s%x1f%an%x1f%ar",
                    ],
                )
                .await
                .unwrap_or_default()
                .lines()
                .filter_map(|l| {
                    let mut f = l.split('\u{1f}');
                    Some(Commit {
                        hash: f.next()?.to_string(),
                        subject: f.next()?.to_string(),
                        author: f.next()?.to_string(),
                        when: f.next()?.to_string(),
                    })
                })
                .collect::<Vec<_>>();
                (branches, commits)
            },
            |(branches, commits)| Message::EverywhereGit(branches, commits),
        )
    }

    pub(super) fn everywhere_git(&mut self, branches: Vec<String>, commits: Vec<Commit>) {
        self.everywhere.branches = branches;
        self.everywhere.commits = commits;
        self.everywhere_refresh();
    }

    /// The servers to ask for names: for each project, one file of each
    /// language that has a server installed.
    fn everywhere_servers(&mut self) -> Vec<(String, PathBuf)> {
        let mut chosen: Vec<(String, PathBuf)> = Vec::new();
        let mut taken: HashSet<(&'static str, PathBuf)> = HashSet::new();
        let roots = self.finder_roots();
        let files: Vec<PathBuf> = self
            .everywhere
            .files
            .iter()
            .map(|(p, _)| p.clone())
            .collect();
        for path in files {
            let ext = file_type(&path);
            let Some(server) = crate::lsp::server_of(&ext) else {
                continue;
            };
            let Some(root) = roots.iter().find(|r| path.starts_with(r)) else {
                continue;
            };
            if taken.contains(&(server, root.clone())) || !self.server_installed(&ext) {
                continue;
            }
            taken.insert((server, root.clone()));
            chosen.push((ext, path));
        }
        chosen
    }

    /// Starts what the query needs, a moment after the last key: the names
    /// from the servers, the text in the files.
    fn everywhere_searches(&mut self) -> Task<Message> {
        let tab = self.everywhere_tab();
        let query = self.everywhere.query.trim().to_string();
        let mut tasks = Vec::new();
        let names = matches!(tab, Tab::All | Tab::Classes | Tab::Symbols)
            && !query.is_empty()
            && !query.starts_with('/')
            && ew::calculation(&query).is_none();
        if names {
            self.everywhere.symbols_generation += 1;
            self.everywhere.symbols_waiting = true;
            let generation = self.everywhere.symbols_generation;
            tasks.push(Task::perform(
                async move {
                    tokio::time::sleep(SYMBOLS_AFTER).await;
                    generation
                },
                Message::EverywhereSymbolsDue,
            ));
        } else {
            self.everywhere.symbols_waiting = false;
        }
        let text = (tab == Tab::Text
            || (tab == Tab::All && self.config.everywhere_text && query.chars().count() >= 3))
            && !query.is_empty();
        if text {
            self.everywhere.text_generation += 1;
            self.everywhere.text_waiting = true;
            let generation = self.everywhere.text_generation;
            tasks.push(Task::perform(
                async move {
                    tokio::time::sleep(TEXT_AFTER).await;
                    generation
                },
                Message::EverywhereTextDue,
            ));
        } else {
            self.everywhere.text_waiting = false;
            if query.is_empty() {
                self.everywhere.text.clear();
            }
        }
        Task::batch(tasks)
    }

    pub(super) fn everywhere_symbols_due(&mut self, generation: u64) -> Task<Message> {
        if generation != self.everywhere.symbols_generation
            || self.overlay != ActiveOverlay::Everywhere
        {
            return Task::none();
        }
        let servers = self.everywhere_servers();
        if servers.is_empty() {
            self.everywhere.symbols_waiting = false;
            self.everywhere_refresh();
            return Task::none();
        }
        let query = self.everywhere.query.trim().to_string();
        let (_, name) = ew::qualified(&query);
        let name = name.to_string();
        // Every kind of name is asked for: the Classes tab keeps the types.
        let dependencies = self.everywhere.non_project;
        Task::perform(
            crate::lsp::workspace_symbols(servers, name, true, dependencies),
            move |found| Message::EverywhereSymbols(generation, found),
        )
    }

    pub(super) fn everywhere_symbols(
        &mut self,
        generation: u64,
        found: crate::lsp::Symbols,
    ) -> Task<Message> {
        if generation != self.everywhere.symbols_generation {
            return Task::none();
        }
        self.everywhere.symbols = found.hits;
        self.everywhere.symbols_busy = found.busy;
        self.everywhere.symbols_waiting = false;
        self.everywhere_refresh();
        // A server still loading its project: asked again in a moment.
        if found.busy && self.overlay == ActiveOverlay::Everywhere {
            return Task::perform(
                async move {
                    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
                    generation
                },
                Message::EverywhereSymbolsDue,
            );
        }
        Task::none()
    }

    pub(super) fn everywhere_text_due(&mut self, generation: u64) -> Task<Message> {
        if generation != self.everywhere.text_generation
            || self.overlay != ActiveOverlay::Everywhere
        {
            return Task::none();
        }
        let query = self.everywhere.query.trim().to_string();
        let files: Vec<PathBuf> = self
            .everywhere
            .files
            .iter()
            .map(|(p, _)| p.clone())
            .filter(|p| !self.everywhere.excluded.contains(&file_type(p)))
            .collect();
        let (case, words, regex) = (
            self.everywhere.case_sensitive,
            self.everywhere.words,
            self.everywhere.regex,
        );
        // Whole words: the text (or the expression) between word boundaries.
        let (pattern, as_regex) = match (words, regex) {
            (true, true) => (format!(r"\b(?:{query})\b"), true),
            (true, false) => (format!(r"\b{}\b", regex::escape(&query)), true),
            (false, r) => (query, r),
        };
        let open = self.open_texts();
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    crate::finder::search(&files, &open, &pattern, case, as_regex)
                })
                .await
                .unwrap_or_else(|e| Err(e.to_string()))
            },
            move |found| Message::EverywhereText(generation, found),
        )
    }

    pub(super) fn everywhere_text(&mut self, generation: u64, found: Result<Vec<TextHit>, String>) {
        if generation != self.everywhere.text_generation {
            return;
        }
        self.everywhere.text_waiting = false;
        match found {
            Ok(hits) => {
                self.everywhere.text = hits;
                self.everywhere.text_for = self.everywhere.query.trim().to_string();
                self.everywhere.text_error = None;
            }
            Err(e) => {
                self.everywhere.text.clear();
                self.everywhere.text_error = Some(e);
            }
        }
        self.everywhere_refresh();
    }

    // ── What each tab lists ──

    /// The files and folders matching the query, best first; a `:line`
    /// at its end goes with the files.
    fn everywhere_file_rows(&self, query: &str, limit: usize) -> Vec<Row> {
        let (name, place) = ew::location(query);
        let name = name.trim();
        let excluded = &self.everywhere.excluded;
        let mut scored: Vec<(i64, Row)> = self
            .everywhere
            .files
            .iter()
            .filter(|(p, _)| !excluded.contains(&file_type(p)))
            .filter_map(|(p, shown)| {
                crate::finder::score(name, shown)
                    .map(|s| (s, Row::File(p.clone(), shown.clone(), place)))
            })
            .collect();
        if place.is_none() {
            scored.extend(self.everywhere.dirs.iter().filter_map(|(p, shown)| {
                crate::finder::score(name, shown.trim_end_matches('/'))
                    .map(|s| (s, Row::Dir(p.clone(), shown.clone())))
            }));
        }
        scored.sort_by_key(|a| std::cmp::Reverse(a.0));
        scored.into_iter().take(limit).map(|(_, r)| r).collect()
    }

    /// The recently opened files (an empty query).
    fn everywhere_recent_rows(&self) -> Vec<Row> {
        let mut seen = HashSet::new();
        self.mru
            .iter()
            .rev()
            .cloned()
            .chain(self.config.recent_files.iter().map(PathBuf::from))
            .filter(|p| !is_untitled(p) && !is_virtual(p) && p.is_file())
            .filter(|p| seen.insert(p.clone()))
            .take(30)
            .map(|p| {
                let shown = display_path(&p);
                Row::File(p, shown, None)
            })
            .collect()
    }

    /// The names found by the servers that match the query (the Classes
    /// tab: only the types), best first.
    fn everywhere_symbol_rows(&self, query: &str, classes: bool, limit: usize) -> Vec<Row> {
        let (container, name) = ew::qualified(query.trim());
        let excluded = &self.everywhere.excluded;
        let mut kept: Vec<(crate::completion::Score, &SymbolHit)> = self
            .everywhere
            .symbols
            .iter()
            .filter(|h| !classes || crate::lsp::is_class_kind(h.kind))
            .filter(|h| !excluded.contains(&file_type(&h.path)))
            .filter(|h| {
                container.is_none_or(|c| {
                    h.container.as_deref().is_some_and(|hc| {
                        crate::completion::fuzzy(c, hc).is_some() || hc.contains(c)
                    })
                })
            })
            .filter_map(|h| crate::completion::fuzzy(name, &h.name).map(|s| (s, h)))
            .collect();
        kept.sort_by(|(a, x), (b, y)| {
            a.tier
                .cmp(&b.tier)
                .then(b.same_case.cmp(&a.same_case))
                .then(x.name.len().cmp(&y.name.len()))
                .then(x.name.cmp(&y.name))
        });
        kept.into_iter()
            .take(limit)
            .map(|(_, h)| Row::Symbol(h.clone()))
            .collect()
    }

    /// The actions and the settings matching the query, best first; `/`
    /// first lists the pages and the settings.
    fn everywhere_action_rows(&self, query: &str, limit: usize) -> Vec<Row> {
        let q = query.trim();
        if let Some(rest) = q.strip_prefix('/') {
            let rest = rest.trim();
            let mut rows: Vec<Row> = PrefsPage::ALL
                .iter()
                .filter(|p| ew::action_score(rest, &p.label(), &[], &[]).is_some())
                .map(|p| Row::Page(*p))
                .collect();
            rows.extend(
                SETTINGS
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| ew::action_score(rest, &(s.label)(), &[], &[]).is_some())
                    .map(|(i, _)| Row::Setting(i)),
            );
            rows.truncate(limit);
            return rows;
        }
        let has_formatter = self
            .editor
            .language
            .as_deref()
            .is_some_and(crate::formatter::can_format);
        let entries =
            crate::palette::entries(&self.config, has_formatter, self.language_picker_enabled());
        let mut scored: Vec<(i64, Row)> = entries
            .into_iter()
            .filter_map(|e| {
                let abbreviations = self
                    .config
                    .action_abbreviations
                    .get(&e.command.id())
                    .cloned()
                    .unwrap_or_default();
                ew::action_score(q, &e.label, e.command.synonyms(), &abbreviations)
                    .map(|s| (s, Row::Action(e)))
            })
            .collect();
        if !q.is_empty() {
            scored.extend(SETTINGS.iter().enumerate().filter_map(|(i, s)| {
                ew::action_score(q, &(s.label)(), &[], &[])
                    .map(|score| (score - 50, Row::Setting(i)))
            }));
        }
        // Stable: equal scores keep the palette's order.
        scored.sort_by_key(|a| std::cmp::Reverse(a.0));
        scored.into_iter().take(limit).map(|(_, r)| r).collect()
    }

    fn everywhere_text_rows(&self, limit: usize) -> Vec<Row> {
        let excluded = &self.everywhere.excluded;
        self.everywhere
            .text
            .iter()
            .filter(|h| !excluded.contains(&file_type(&h.path)))
            .take(limit)
            .map(|h| Row::Text(h.clone()))
            .collect()
    }

    fn everywhere_git_rows(&self, query: &str, limit: usize) -> Vec<Row> {
        let q = query.trim().to_lowercase();
        let mut rows: Vec<Row> = self
            .everywhere
            .branches
            .iter()
            .filter(|b| q.is_empty() || crate::finder::score(&q, b).is_some())
            .map(|b| Row::Branch(b.clone()))
            .collect();
        rows.extend(
            self.everywhere
                .commits
                .iter()
                .filter(|c| {
                    q.is_empty()
                        || c.hash.starts_with(&q)
                        || c.subject.to_lowercase().contains(&q)
                        || c.author.to_lowercase().contains(&q)
                })
                .map(|c| Row::Commit(c.clone())),
        );
        rows.truncate(limit);
        rows
    }

    /// Builds the lines of the tab for the query, and selects the first one
    /// that can be opened.
    pub(super) fn everywhere_refresh(&mut self) {
        let tab = self.everywhere_tab();
        let query = self.everywhere.query.trim().to_string();
        let mut rows: Vec<Row> = Vec::new();
        let busy = self.everywhere.symbols_busy;
        match tab {
            Tab::All if query.is_empty() => rows = self.everywhere_recent_rows(),
            Tab::All => {
                if let Some(result) = ew::calculation(&query) {
                    rows.push(Row::Calc(query.clone(), result));
                }
                let mut others = 0;
                let groups = [
                    (
                        Tab::Classes,
                        self.everywhere_symbol_rows(&query, true, PER_GROUP + 1),
                    ),
                    (Tab::Files, self.everywhere_file_rows(&query, PER_GROUP + 1)),
                    (Tab::Symbols, {
                        // Not the types again: they are in their own group.
                        let mut s = self.everywhere_symbol_rows(&query, false, MOST);
                        s.retain(
                            |r| !matches!(r, Row::Symbol(h) if crate::lsp::is_class_kind(h.kind)),
                        );
                        s.truncate(PER_GROUP + 1);
                        s
                    }),
                    (
                        Tab::Actions,
                        self.everywhere_action_rows(&query, PER_GROUP + 1),
                    ),
                ];
                for (group, mut found) in groups {
                    if found.is_empty() {
                        continue;
                    }
                    rows.push(Row::Group(group));
                    let more = found.len() > PER_GROUP;
                    found.truncate(PER_GROUP);
                    others += found.len();
                    rows.extend(found);
                    if more {
                        rows.push(Row::More(group));
                    }
                }
                if self.config.everywhere_text && others < FEW {
                    let mut text = self.everywhere_text_rows(PER_GROUP + 1);
                    if !text.is_empty() {
                        rows.push(Row::Group(Tab::Text));
                        let more = text.len() > PER_GROUP;
                        text.truncate(PER_GROUP);
                        rows.extend(text);
                        if more {
                            rows.push(Row::More(Tab::Text));
                        }
                    }
                }
            }
            Tab::Files if query.is_empty() => rows = self.everywhere_recent_rows(),
            Tab::Files => rows = self.everywhere_file_rows(&query, MOST),
            Tab::Classes => rows = self.everywhere_symbol_rows(&query, true, MOST),
            Tab::Symbols => rows = self.everywhere_symbol_rows(&query, false, MOST),
            Tab::Actions => rows = self.everywhere_action_rows(&query, MOST),
            Tab::Text => {
                rows = self.everywhere_text_rows(MOST);
                if let Some(e) = &self.everywhere.text_error {
                    rows.push(Row::Note(e.clone()));
                }
            }
            Tab::Git => rows = self.everywhere_git_rows(&query, MOST),
        }
        let looks_up_names =
            matches!(tab, Tab::All | Tab::Classes | Tab::Symbols) && !query.is_empty();
        if looks_up_names && busy {
            rows.push(Row::Note(t!("everywhere.indexing").to_string()));
        }
        if rows.iter().all(|r| !r.selectable()) {
            let waiting = self.everywhere.files_loading
                || self.everywhere.symbols_waiting
                || self.everywhere.text_waiting;
            if waiting {
                rows.push(Row::Note(t!("everywhere.searching").to_string()));
            } else if !query.is_empty() {
                rows.push(Row::Note(t!("everywhere.nothing").to_string()));
            }
        }
        self.everywhere.selected = rows.iter().position(Row::selectable).unwrap_or(0);
        self.everywhere.rows = rows;
    }

    // ── Keys ──

    /// Moves the selection by `delta` lines that can be opened (going
    /// round), or to the first or last one.
    pub(super) fn everywhere_step(&mut self, delta: isize) -> Task<Message> {
        let selectable: Vec<usize> = self
            .everywhere
            .rows
            .iter()
            .enumerate()
            .filter(|(_, r)| r.selectable())
            .map(|(i, _)| i)
            .collect();
        if selectable.is_empty() {
            return Task::none();
        }
        let at = selectable
            .iter()
            .position(|i| *i == self.everywhere.selected)
            .unwrap_or(0) as isize;
        let next = match delta {
            isize::MIN => 0,
            isize::MAX => selectable.len() as isize - 1,
            d => (at + d).rem_euclid(selectable.len() as isize),
        };
        self.everywhere.selected = selectable[next as usize];
        iced::widget::operation::scroll_to(
            list_id(),
            iced::widget::scrollable::AbsoluteOffset {
                x: 0.0,
                y: ((self.everywhere.selected as f32 - 5.0) * ROW).max(0.0),
            },
        )
    }

    /// A key while the search is open; `None` when it isn't the search's.
    pub(super) fn everywhere_key(
        &mut self,
        key: &str,
        ctrl: bool,
        shift: bool,
        alt: bool,
    ) -> Option<Task<Message>> {
        Some(match (key, ctrl, alt) {
            ("ArrowDown", true, false) => self.everywhere_step(isize::MAX),
            ("ArrowUp", true, false) => self.everywhere_step(isize::MIN),
            ("ArrowDown", false, false) => self.everywhere_step(1),
            ("ArrowUp", false, false) => self.everywhere_step(-1),
            ("PageDown", false, false) => self.everywhere_step(10),
            ("PageUp", false, false) => self.everywhere_step(-10),
            ("Tab", false, false) => {
                let git = self.config.everywhere_git;
                let tab = self.everywhere_tab().step(if shift { -1 } else { 1 }, git);
                self.everywhere_switch(tab)
            }
            ("n", false, true) => self.everywhere_non_project(),
            _ => return None,
        })
    }

    // ── Opening a line ──

    /// Opens the line picked (or the selected one): Alt+Enter on an action
    /// sets its shortcut instead.
    pub(super) fn everywhere_open(&mut self, pick: Option<usize>) -> Task<Message> {
        let index = pick.unwrap_or(self.everywhere.selected);
        let Some(row) = self.everywhere.rows.get(index).cloned() else {
            return Task::none();
        };
        if self.mod_alt && pick.is_none() {
            if let Row::Action(entry) = &row {
                return self.everywhere_assign(entry.command);
            }
        }
        match row {
            Row::Group(_) | Row::Note(_) => Task::none(),
            Row::More(tab) => self.everywhere_switch(tab),
            Row::Calc(_, result) => {
                self.overlay = ActiveOverlay::None;
                self.set_status(t!("everywhere.copied", value = result.as_str()), false);
                iced::clipboard::write(result)
            }
            Row::File(path, _, place) => {
                self.overlay = ActiveOverlay::None;
                self.remember_place();
                match place {
                    Some((line, col)) => self.jump_to(crate::definition::Hit {
                        path,
                        line,
                        col: col.unwrap_or(0),
                        len: 0,
                    }),
                    None => self.activate(path),
                }
            }
            Row::Dir(path, _) => {
                self.overlay = ActiveOverlay::None;
                self.handle(Message::ShowInProjects(path))
            }
            Row::Symbol(hit) => {
                self.overlay = ActiveOverlay::None;
                self.remember_place();
                self.jump_to(crate::definition::Hit {
                    path: hit.path,
                    line: hit.line,
                    col: hit.col,
                    len: hit.name.len(),
                })
            }
            Row::Text(hit) => {
                self.overlay = ActiveOverlay::None;
                self.remember_place();
                self.jump_to(crate::definition::Hit {
                    path: hit.path,
                    line: hit.line,
                    col: hit.col,
                    len: hit.len,
                })
            }
            Row::Action(entry) => {
                if !entry.enabled {
                    return Task::none();
                }
                self.overlay = ActiveOverlay::None;
                self.handle(entry.command.message())
            }
            // A setting is turned on or off where it is: the list stays.
            Row::Setting(i) => {
                let setting = &SETTINGS[i];
                let on = (setting.get)(&self.config);
                let task = self.handle(Message::Preferences((setting.set)(!on)));
                self.config.save();
                self.everywhere_refresh();
                self.everywhere.selected = index;
                task
            }
            Row::Page(page) => {
                self.overlay = ActiveOverlay::None;
                self.show_preferences = true;
                self.handle(Message::Preferences(PreferencesMessage::PageSelected(page)))
            }
            Row::Branch(name) => {
                self.overlay = ActiveOverlay::None;
                // A remote branch is followed by a local one of its name.
                let local = name
                    .split_once('/')
                    .filter(|_| {
                        !self
                            .everywhere
                            .branches
                            .iter()
                            .any(|b| *b == name && !b.contains('/'))
                    })
                    .map_or(name.clone(), |(remote, rest)| {
                        if self.everywhere.branches.iter().any(|b| b == rest) || remote.is_empty() {
                            rest.to_string()
                        } else {
                            name.clone()
                        }
                    });
                self.git.switch_branch(local, false)
            }
            Row::Commit(commit) => {
                self.overlay = ActiveOverlay::None;
                use crate::git_panel::GitMsg;
                let mut tasks = Vec::new();
                if !self.git.open {
                    tasks.push(self.handle(Message::ToggleGit));
                }
                tasks
                    .push(self.handle(Message::Git(GitMsg::SelectTab(crate::git_panel::Tab::Log))));
                tasks.push(self.handle(Message::Git(GitMsg::LogSearch(commit.hash.clone()))));
                tasks.push(self.handle(Message::Git(GitMsg::LogSearchRun)));
                Task::batch(tasks)
            }
        }
    }

    /// Alt+Enter on an action: its shortcut is the next keys pressed.
    pub(super) fn everywhere_assign(&mut self, command: PaletteCommand) -> Task<Message> {
        match command.shortcut_target() {
            Some(target) => {
                self.capturing = Some(target);
                Task::none()
            }
            None => {
                self.set_status(t!("everywhere.no_shortcut"), true);
                Task::none()
            }
        }
    }

    // ── Abbreviations ──

    pub(super) fn everywhere_abbreviate(&mut self, command: PaletteCommand) -> Task<Message> {
        self.everywhere.abbreviating = Some((command, String::new()));
        iced::widget::operation::focus(abbreviation_id())
    }

    pub(super) fn everywhere_abbreviation_submit(&mut self) -> Task<Message> {
        if let Some((command, abbreviation)) = self.everywhere.abbreviating.take() {
            let abbreviation = abbreviation.trim().to_string();
            if !abbreviation.is_empty() {
                let list = self
                    .config
                    .action_abbreviations
                    .entry(command.id())
                    .or_default();
                if !list.contains(&abbreviation) {
                    list.push(abbreviation);
                }
                self.config.save();
            }
        }
        self.everywhere_refresh();
        iced::widget::operation::focus(field_id())
    }

    pub(super) fn everywhere_abbreviation_remove(
        &mut self,
        command: PaletteCommand,
        abbreviation: String,
    ) {
        if let Some(list) = self.config.action_abbreviations.get_mut(&command.id()) {
            list.retain(|a| *a != abbreviation);
            if list.is_empty() {
                self.config.action_abbreviations.remove(&command.id());
            }
            self.config.save();
        }
        self.everywhere_refresh();
    }

    // ── Filter, export ──

    pub(super) fn everywhere_types(&self) -> Vec<String> {
        let mut types: BTreeSet<String> = self
            .everywhere
            .files
            .iter()
            .map(|(p, _)| file_type(p))
            .filter(|t| !t.is_empty())
            .collect();
        types.extend(self.everywhere.excluded.iter().cloned());
        types.into_iter().collect()
    }

    pub(super) fn everywhere_filter_type(&mut self, kind: String, on: bool) -> Task<Message> {
        if on {
            self.everywhere.excluded.remove(&kind);
        } else {
            self.everywhere.excluded.insert(kind);
        }
        self.everywhere_refresh();
        if self.everywhere_tab() == Tab::Text {
            return self.everywhere_searches();
        }
        Task::none()
    }

    pub(super) fn everywhere_text_option(&mut self, option: u8) -> Task<Message> {
        match option {
            0 => self.everywhere.case_sensitive = !self.everywhere.case_sensitive,
            1 => self.everywhere.words = !self.everywhere.words,
            _ => self.everywhere.regex = !self.everywhere.regex,
        }
        self.everywhere_searches()
    }

    /// Whether the results can go to the list of "find in files" (not the
    /// actions, nor Git).
    fn everywhere_exportable(&self) -> bool {
        !matches!(self.everywhere_tab(), Tab::Actions | Tab::Git)
            && self
                .everywhere
                .rows
                .iter()
                .any(|r| matches!(r, Row::File(..) | Row::Symbol(_) | Row::Text(_)))
    }

    /// The results, in the list of "find in files", to go through them.
    pub(super) fn everywhere_export(&mut self) -> Task<Message> {
        if !self.everywhere_exportable() {
            return Task::none();
        }
        let tab = self.everywhere_tab();
        let query = self.everywhere.query.trim().to_string();
        let mut rows: Vec<Row> = match tab {
            Tab::Files => self.everywhere_file_rows(&query, usize::MAX),
            Tab::Classes => self.everywhere_symbol_rows(&query, true, usize::MAX),
            Tab::Symbols => self.everywhere_symbol_rows(&query, false, usize::MAX),
            Tab::Text => self.everywhere_text_rows(usize::MAX),
            _ => self.everywhere.rows.clone(),
        };
        rows.retain(|r| matches!(r, Row::File(..) | Row::Symbol(_) | Row::Text(_)));
        // A short query matches every file of a large project: the list,
        // drawn whole, would freeze the window.
        let cut = rows.len() > crate::finder::MAX_HITS;
        rows.truncate(crate::finder::MAX_HITS);
        let hits: Vec<TextHit> = rows
            .into_iter()
            .map(|r| match r {
                Row::File(path, shown, place) => TextHit {
                    path,
                    line: place.map_or(0, |p| p.0),
                    col: 0,
                    len: 0,
                    preview: shown,
                    preview_at: (0, 0),
                    stamp: 0,
                },
                Row::Symbol(h) => {
                    let preview = match &h.container {
                        Some(c) => format!("{} — {c}", h.name),
                        None => h.name.clone(),
                    };
                    TextHit {
                        len: h.name.len(),
                        preview_at: (0, h.name.len()),
                        path: h.path,
                        line: h.line,
                        col: h.col,
                        preview,
                        stamp: 0,
                    }
                }
                Row::Text(hit) => hit,
                _ => unreachable!("kept above"),
            })
            .collect();
        let occurrences: Vec<super::finder::Occurrence> = hits
            .into_iter()
            .map(|hit| super::finder::Occurrence {
                hit,
                replacement: None,
                excluded: false,
            })
            .collect();
        let results = super::finder::matches_text(occurrences.len());
        let q = query.as_str();
        let title = t!("results.everywhere", query = q, results = results).to_string();
        self.results_add(title, None, occurrences);
        self.overlay = ActiveOverlay::None;
        if cut {
            let n = crate::finder::MAX_HITS;
            self.set_status(t!("everywhere.export_cut", n = n), false);
        }
        Task::none()
    }

    // ── On screen ──

    fn everywhere_row_view<'a>(
        &'a self,
        index: usize,
        row: &'a Row,
        dark: bool,
    ) -> Element<'a, Message> {
        let p = se_theme::palette(dark);
        let selected = index == self.everywhere.selected;
        let muted = |s: String| -> Element<'a, Message> {
            text(s)
                .size(12)
                .color(p.muted)
                .wrapping(iced::widget::text::Wrapping::None)
                .shaping(iced::widget::text::Shaping::Advanced)
                .into()
        };
        let name = |s: String| -> Element<'a, Message> {
            text(s)
                .size(13)
                .color(p.text)
                .wrapping(iced::widget::text::Wrapping::None)
                .shaping(iced::widget::text::Shaping::Advanced)
                .into()
        };
        let glyph = |c: char, (r, g, b): (u8, u8, u8)| -> Element<'a, Message> {
            container(
                text(c)
                    .font(iced_fonts::NERD_FONT)
                    .size(14)
                    .color(iced::Color::from_rgb8(r, g, b)),
            )
            .width(18)
            .center_x(18)
            .into()
        };
        let line: Element<Message> = match row {
            Row::Group(tab) => {
                return container(
                    text(tab_label(*tab).to_uppercase())
                        .size(10.5)
                        .font(se_theme::UI_MEDIUM)
                        .color(p.muted),
                )
                .padding([0, 10])
                .center_y(ROW)
                .into();
            }
            Row::Note(note) => {
                return container(muted(note.clone()))
                    .padding([0, 10])
                    .center_y(ROW)
                    .into();
            }
            Row::More(_) => row![
                Space::new().width(18),
                text(t!("everywhere.more").to_string())
                    .size(12)
                    .color(p.accent)
            ]
            .spacing(8)
            .into(),
            Row::Calc(expression, result) => row![
                container(text("=").size(15).color(p.accent))
                    .width(18)
                    .center_x(18),
                name(format!("{expression} = {result}")),
                Space::new().width(Length::Fill),
                muted(t!("everywhere.copy_hint").to_string()),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
            .into(),
            Row::File(path, shown, place) => {
                let file = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let at = place.map_or(String::new(), |(l, c)| match c {
                    Some(c) => format!(":{}:{}", l + 1, c + 1),
                    None => format!(":{}", l + 1),
                });
                let dir = shown
                    .rsplit_once('/')
                    .map(|(d, _)| d.to_string())
                    .unwrap_or_default();
                row![
                    crate::file_icons::file(path, dark),
                    name(format!("{file}{at}")),
                    muted(dir),
                ]
                .spacing(8)
                .align_y(Alignment::Center)
                .into()
            }
            Row::Dir(_, shown) => {
                let trimmed = shown.trim_end_matches('/');
                let (parent, last) = trimmed.rsplit_once('/').unwrap_or(("", trimmed));
                row![
                    crate::file_icons::folder(false, dark),
                    name(format!("{last}/")),
                    muted(parent.to_string())
                ]
                .spacing(8)
                .align_y(Alignment::Center)
                .into()
            }
            Row::Symbol(hit) => {
                let (c, color) = symbol_icon(hit.kind);
                let file = hit
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let mut r = row![glyph(c, color), name(hit.name.clone())]
                    .spacing(8)
                    .align_y(Alignment::Center);
                if let Some(container_name) = &hit.container {
                    r = r.push(muted(container_name.clone()));
                }
                r.push(Space::new().width(Length::Fill))
                    .push(muted(format!("{file}:{}", hit.line + 1)))
                    .into()
            }
            Row::Action(entry) => {
                let abbreviations = self
                    .config
                    .action_abbreviations
                    .get(&entry.command.id())
                    .cloned()
                    .unwrap_or_default();
                let label = text(entry.label.clone()).size(13).color(if entry.enabled {
                    p.text
                } else {
                    p.muted
                });
                let mut r = row![glyph('\u{eb2c}', (0x9a, 0xa0, 0xad)), label]
                    .spacing(8)
                    .align_y(Alignment::Center);
                if !abbreviations.is_empty() {
                    r = r.push(muted(abbreviations.join(", ")));
                }
                r = r.push(Space::new().width(Length::Fill));
                if let Some(shortcut) = &entry.shortcut {
                    r = r.push(
                        container(text(shortcut.clone()).size(11.5).color(p.muted))
                            .padding([2, 6])
                            .style(se_theme::badge(dark)),
                    );
                }
                r.into()
            }
            Row::Setting(i) => {
                let setting = &SETTINGS[*i];
                let on = (setting.get)(&self.config);
                row![
                    glyph('\u{eaf8}', (0x9a, 0xa0, 0xad)),
                    name((setting.label)()),
                    Space::new().width(Length::Fill),
                    container(
                        text(if on {
                            t!("everywhere.on").to_string()
                        } else {
                            t!("everywhere.off").to_string()
                        })
                        .size(11)
                        .font(se_theme::UI_MEDIUM)
                        .color(if on { p.accent } else { p.muted }),
                    )
                    .padding([2, 8])
                    .style(se_theme::badge(dark)),
                ]
                .spacing(8)
                .align_y(Alignment::Center)
                .into()
            }
            Row::Page(page) => row![
                glyph('\u{eaf8}', (0x9a, 0xa0, 0xad)),
                name(format!(
                    "{} › {}",
                    t!("menu.file_preferences"),
                    page.label()
                )),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
            .into(),
            Row::Text(hit) => {
                let (from, to) = hit.preview_at;
                let piece = |a: usize, b: usize| hit.preview.get(a..b).unwrap_or("").to_string();
                let spans: Vec<iced::widget::text::Span<'_, ()>> = vec![
                    span(piece(0, from)).color(p.text).font(se_theme::MONO),
                    span(piece(from, to)).color(p.accent).font(se_theme::MONO),
                    span(piece(to, hit.preview.len()))
                        .color(p.text)
                        .font(se_theme::MONO),
                ];
                let file = hit
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                row![
                    crate::file_icons::file(&hit.path, dark),
                    rich_text(spans)
                        .size(12.5)
                        .wrapping(iced::widget::text::Wrapping::None),
                    Space::new().width(Length::Fill),
                    muted(format!("{file}:{}", hit.line + 1)),
                ]
                .spacing(8)
                .align_y(Alignment::Center)
                .into()
            }
            Row::Branch(branch) => {
                row![glyph('\u{e725}', (0xf5, 0x4d, 0x27)), name(branch.clone())]
                    .spacing(8)
                    .align_y(Alignment::Center)
                    .into()
            }
            Row::Commit(commit) => row![
                glyph('\u{eafc}', (0x9a, 0xa0, 0xad)),
                text(commit.hash.chars().take(8).collect::<String>())
                    .size(12)
                    .font(se_theme::MONO)
                    .color(p.accent),
                name(commit.subject.clone()),
                Space::new().width(Length::Fill),
                muted(format!("{} · {}", commit.author, commit.when)),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
            .into(),
        };
        let row_button = button(container(line).center_y(ROW).clip(true))
            .padding([0, 10])
            .width(Length::Fill)
            .on_press(Message::EverywhereOpen(Some(index)))
            .style(
                (se_theme::MenuItem {
                    dark,
                    radius: se_theme::R_ROW,
                    highlighted: selected,
                })
                .style(),
            );
        // An action's menu: its abbreviations and its shortcut.
        if let Row::Action(entry) = row {
            let command = entry.command;
            let abbreviations = self
                .config
                .action_abbreviations
                .get(&command.id())
                .cloned()
                .unwrap_or_default();
            let can_assign = command.shortcut_target().is_some();
            let (add, assign) = (
                t!("everywhere.add_abbreviation").to_string(),
                t!("everywhere.assign_shortcut").to_string(),
            );
            return crate::pointer_area::context_menu(row_button, move || {
                let item = |label: String, msg: Option<Message>| -> Element<'static, Message> {
                    let mut b = button(text(label).size(13))
                        .padding([7, 10])
                        .width(Length::Fixed(240.0))
                        .style(
                            (se_theme::MenuItem {
                                dark,
                                radius: se_theme::R_SM,
                                highlighted: false,
                            })
                            .style(),
                        );
                    if let Some(m) = msg {
                        b = b.on_press(m);
                    }
                    b.into()
                };
                let mut items = vec![
                    item(add.clone(), Some(Message::EverywhereAbbreviate(command))),
                    item(
                        assign.clone(),
                        can_assign.then_some(Message::EverywhereAssign(command)),
                    ),
                ];
                for a in &abbreviations {
                    let label =
                        t!("everywhere.remove_abbreviation", abbreviation = a.as_str()).to_string();
                    items.push(item(
                        label,
                        Some(Message::EverywhereAbbreviationRemove(command, a.clone())),
                    ));
                }
                container(column(items).spacing(1).padding(6))
                    .style(se_theme::card(dark))
                    .into()
            });
        }
        row_button.into()
    }

    pub(super) fn view_everywhere(&self, dark: bool) -> Element<'_, Message> {
        let p = se_theme::palette(dark);
        let current = self.everywhere_tab();
        let ghost = move |active: bool| (se_theme::GhostButton { dark, active }).style();
        let mut tabs = row![].spacing(2).align_y(Alignment::Center);
        for tab in Tab::shown(self.config.everywhere_git) {
            tabs = tabs.push(
                button(text(tab_label(tab)).size(12.5))
                    .padding([4, 10])
                    .on_press(Message::EverywhereTab(tab))
                    .style(ghost(tab == current)),
            );
        }
        let filterable = matches!(
            current,
            Tab::All | Tab::Files | Tab::Classes | Tab::Symbols | Tab::Text
        );
        let mut header = tabs.push(Space::new().width(Length::Fill));
        if current != Tab::Actions && current != Tab::Git {
            header = header.push(
                iced::widget::checkbox(self.everywhere.non_project)
                    .label(t!("everywhere.non_project").to_string())
                    .text_size(12)
                    .size(14)
                    .on_toggle(|_| Message::EverywhereNonProject),
            );
        }
        if filterable {
            let filtered = !self.everywhere.excluded.is_empty();
            header = header.push(
                button(
                    text(t!("everywhere.filter").to_string())
                        .size(12)
                        .color(if filtered { p.accent } else { p.text }),
                )
                .padding([4, 10])
                .on_press(Message::EverywhereFilter)
                .style(ghost(self.everywhere.filter_open)),
            );
        }
        let mut export = button(text(t!("everywhere.export").to_string()).size(12))
            .padding([4, 10])
            .style(ghost(false));
        if self.everywhere_exportable() {
            export = export.on_press(Message::EverywhereExport);
        }
        header = header.push(export);
        header = header.push(iced::widget::tooltip(
            button(text("×").size(16).color(p.muted))
                .padding([1, 8])
                .on_press(Message::CloseOverlay)
                .style(ghost(false)),
            container(text(t!("history.close").to_string()).size(12))
                .padding([4, 8])
                .style(se_theme::tooltip(dark)),
            iced::widget::tooltip::Position::Bottom,
        ));

        let placeholder = placeholder_of(current);
        let field = text_input(&placeholder, &self.everywhere.query)
            .id(field_id())
            .on_input(Message::EverywhereQuery)
            .on_submit(Message::EverywhereOpen(None))
            .size(15)
            .padding(0)
            .style((se_theme::BareField { dark }).style());
        let mut field_row = row![field].spacing(6).align_y(Alignment::Center);
        if current == Tab::Text {
            let option = |label: &'static str, on: bool, which: u8, tip: String| {
                iced::widget::tooltip(
                    button(text(label).size(12).font(se_theme::MONO))
                        .padding([3, 7])
                        .on_press(Message::EverywhereTextOption(which))
                        .style(ghost(on)),
                    container(text(tip).size(12))
                        .padding([4, 8])
                        .style(se_theme::tooltip(dark)),
                    iced::widget::tooltip::Position::Bottom,
                )
            };
            field_row = field_row
                .push(option(
                    "Aa",
                    self.everywhere.case_sensitive,
                    0,
                    t!("everywhere.case").to_string(),
                ))
                .push(option(
                    "W",
                    self.everywhere.words,
                    1,
                    t!("everywhere.words").to_string(),
                ))
                .push(option(
                    ".*",
                    self.everywhere.regex,
                    2,
                    t!("everywhere.regex").to_string(),
                ));
        }

        let mut body = column![
            container(header).padding([8, 10]),
            container(Space::new().height(1))
                .width(Length::Fill)
                .style(se_theme::rule(dark)),
            container(field_row)
                .padding([0, 16])
                .height(48)
                .center_y(48),
        ];
        if let Some((command, abbreviation)) = &self.everywhere.abbreviating {
            let label = crate::palette::entries(&self.config, true, true)
                .into_iter()
                .find(|e| e.command == *command)
                .map(|e| e.label)
                .unwrap_or_default();
            body = body.push(
                container(
                    row![
                        text(
                            t!("everywhere.abbreviation_for", action = label.as_str()).to_string()
                        )
                        .size(12.5)
                        .color(p.muted),
                        text_input("", abbreviation)
                            .id(abbreviation_id())
                            .on_input(Message::EverywhereAbbreviationTyped)
                            .on_submit(Message::EverywhereAbbreviationSubmit)
                            .size(13)
                            .padding([4, 8])
                            .width(160)
                            .style(se_theme::Field::new(dark).style()),
                    ]
                    .spacing(10)
                    .align_y(Alignment::Center),
                )
                .padding([4, 16]),
            );
        }
        if self.capturing.is_some() {
            body = body.push(
                container(
                    text(t!("everywhere.press_keys").to_string())
                        .size(12.5)
                        .color(p.accent),
                )
                .padding([4, 16]),
            );
        }
        if self.everywhere.filter_open && filterable {
            let types = self.everywhere_types();
            let mut boxes = iced_aw::Wrap::new().spacing(10.0).line_spacing(6.0);
            for kind in types {
                let on = !self.everywhere.excluded.contains(&kind);
                let k = kind.clone();
                boxes = boxes.push(
                    iced::widget::checkbox(on)
                        .label(format!(".{kind}"))
                        .text_size(12)
                        .size(13)
                        .on_toggle(move |v| Message::EverywhereFilterType(k.clone(), v)),
                );
            }
            body = body.push(container(boxes).padding([6, 16]));
        }
        body = body.push(
            container(Space::new().height(1))
                .width(Length::Fill)
                .style(se_theme::rule(dark)),
        );
        let rows: Vec<Element<Message>> = self
            .everywhere
            .rows
            .iter()
            .enumerate()
            .map(|(i, r)| self.everywhere_row_view(i, r, dark))
            .collect();
        body = body.push(
            scrollable(column(rows).spacing(0).padding(6))
                .id(list_id())
                .style(se_theme::Scrollbar { dark }.style())
                .height(Length::Fixed(LIST_HEIGHT)),
        );
        body = body
            .push(
                container(Space::new().height(1))
                    .width(Length::Fill)
                    .style(se_theme::rule(dark)),
            )
            .push(
                container(
                    text(t!("everywhere.keys").to_string())
                        .size(11)
                        .color(p.muted),
                )
                .padding([6, 12])
                .width(Length::Fill),
            );
        container(body)
            .width(WIDTH)
            .style(se_theme::palette_card(dark))
            .into()
    }
}

/// What a tab is called.
fn tab_label(tab: Tab) -> String {
    match tab {
        Tab::All => t!("everywhere.tab_all"),
        Tab::Classes => t!("everywhere.tab_classes"),
        Tab::Files => t!("everywhere.tab_files"),
        Tab::Symbols => t!("everywhere.tab_symbols"),
        Tab::Actions => t!("everywhere.tab_actions"),
        Tab::Text => t!("everywhere.tab_text"),
        Tab::Git => t!("everywhere.tab_git"),
    }
    .to_string()
}

/// What the field says before anything is typed, on a tab.
fn placeholder_of(tab: Tab) -> String {
    match tab {
        Tab::All => t!("everywhere.placeholder_all"),
        Tab::Classes => t!("everywhere.placeholder_classes"),
        Tab::Files => t!("everywhere.placeholder_files"),
        Tab::Symbols => t!("everywhere.placeholder_symbols"),
        Tab::Actions => t!("everywhere.placeholder_actions"),
        Tab::Text => t!("everywhere.placeholder_text"),
        Tab::Git => t!("everywhere.placeholder_git"),
    }
    .to_string()
}

// ─── Shift twice ───────────────────────────────────────────────────────────

/// Shift pressed and released twice in a row, with no other key between:
/// the subscription's state (it has none of its own).
pub(super) mod double_shift {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    struct Taps {
        /// When Shift went down, no other key since.
        down: Option<Instant>,
        /// When the last clean tap ended.
        last: Option<Instant>,
    }

    static TAPS: Mutex<Taps> = Mutex::new(Taps {
        down: None,
        last: None,
    });

    /// A tap is short; two taps are close.
    const TAP: Duration = Duration::from_millis(400);
    const BETWEEN: Duration = Duration::from_millis(450);

    /// What a keyboard event means for it: true on the second tap.
    pub fn hear(event: &iced::keyboard::Event, now: Instant) -> bool {
        use iced::keyboard::{key::Named, Event, Key};
        let Ok(mut taps) = TAPS.lock() else {
            return false;
        };
        match event {
            Event::KeyPressed {
                key: Key::Named(Named::Shift),
                repeat: false,
                ..
            } => {
                taps.down = Some(now);
                false
            }
            Event::KeyPressed {
                key: Key::Named(Named::Shift),
                ..
            } => false,
            Event::KeyReleased {
                key: Key::Named(Named::Shift),
                ..
            } => {
                let Some(down) = taps.down.take() else {
                    return false;
                };
                if now.duration_since(down) > TAP {
                    taps.last = None;
                    return false;
                }
                if taps
                    .last
                    .is_some_and(|last| now.duration_since(last) <= BETWEEN)
                {
                    taps.last = None;
                    true
                } else {
                    taps.last = Some(now);
                    false
                }
            }
            // Any other key: Shift was used to type, not tapped.
            Event::KeyPressed { .. } => {
                taps.down = None;
                taps.last = None;
                false
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src/sidebar")).unwrap();
        std::fs::write(
            dir.path().join("src/app.rs"),
            "fn main() {\n    run();\n}\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("src/sidebar/mod.rs"), "pub fn run() {}\n").unwrap();
        dir
    }

    fn app_on(dir: &Path) -> SimpleEditApp {
        let mut app = SimpleEditApp::blank(Config::default());
        let _ = app.restore_session(crate::session::Session::default());
        let _ = app.update(Message::ProjectPicked(Some(dir.to_path_buf())));
        app
    }

    /// Opens the search on `tab` with the files of `dir` listed.
    fn open(app: &mut SimpleEditApp, dir: &Path, tab: Tab) {
        let _ = app.update(Message::OpenEverywhere(tab));
        let files = crate::finder::list_files(dir, &[], true)
            .into_iter()
            .map(|f| {
                let rel = f.strip_prefix(dir).unwrap().to_string_lossy().into_owned();
                (f, rel)
            })
            .collect();
        let generation = app.everywhere.files_generation;
        let _ = app.update(Message::EverywhereFiles(generation, files));
    }

    fn typed(app: &mut SimpleEditApp, query: &str) {
        let _ = app.update(Message::EverywhereQuery(query.into()));
    }

    fn key(app: &mut SimpleEditApp, key: &str, ctrl: bool, shift: bool, alt: bool) {
        let _ = app.update(Message::KeyPressed {
            key: key.into(),
            ctrl,
            shift,
            alt,
            captured: false,
        });
    }

    fn selected(app: &SimpleEditApp) -> &Row {
        &app.everywhere.rows[app.everywhere.selected]
    }

    #[test]
    fn a_file_is_found_by_its_name_and_opened_at_a_line() {
        let dir = project();
        let mut app = app_on(dir.path());
        let _ = app.update(Message::OpenGotoFile);
        assert_eq!(app.overlay, ActiveOverlay::Everywhere);
        assert_eq!(app.everywhere.tab, Some(Tab::Files));
        open(&mut app, dir.path(), Tab::Files);
        typed(&mut app, "sbmod");
        assert!(
            matches!(selected(&app), Row::File(_, shown, None) if shown == "src/sidebar/mod.rs")
        );
        // A folder is found too.
        typed(&mut app, "sidebar");
        assert!(app
            .everywhere
            .rows
            .iter()
            .any(|r| matches!(r, Row::Dir(_, s) if s == "src/sidebar/")));
        // `:line` opens the file there.
        typed(&mut app, "app.rs:2");
        let Row::File(path, _, place) = selected(&app).clone() else {
            panic!("a file");
        };
        assert_eq!(place, Some((1, None)));
        let _ = app.update(Message::EverywhereOpen(None));
        assert_eq!(app.overlay, ActiveOverlay::None);
        assert_eq!(app.pending_open.as_ref(), Some(&path));
    }

    #[test]
    fn tab_goes_round_and_the_tab_shortcut_again_looks_beyond_the_project() {
        let dir = project();
        let mut app = app_on(dir.path());
        open(&mut app, dir.path(), Tab::All);
        key(&mut app, "Tab", false, false, false);
        assert_eq!(app.everywhere.tab, Some(Tab::Classes));
        key(&mut app, "Tab", false, true, false);
        key(&mut app, "Tab", false, true, false);
        assert_eq!(
            app.everywhere.tab,
            Some(Tab::Text),
            "Shift+Tab goes back, round"
        );
        // Shift twice again, or the same tab's shortcut: beyond the project.
        assert!(!app.everywhere.non_project);
        let _ = app.update(Message::DoubleShift);
        assert!(app.everywhere.non_project);
        let _ = app.update(Message::OpenEverywhere(Tab::Text));
        assert!(!app.everywhere.non_project);
        // The Git tab only when turned on.
        app.config.everywhere_git = true;
        key(&mut app, "Tab", false, false, false);
        assert_eq!(app.everywhere.tab, Some(Tab::Git));
        let _ = app.update(Message::EscapePressed);
        assert_eq!(app.overlay, ActiveOverlay::None);
    }

    #[test]
    fn all_groups_its_results_with_more_and_shows_recent_files_first() {
        let dir = project();
        let mut app = app_on(dir.path());
        let recent = dir.path().join("src/app.rs");
        app.config.add_recent(&recent);
        open(&mut app, dir.path(), Tab::All);
        assert!(
            matches!(selected(&app), Row::File(p, ..) if *p == recent),
            "recent first"
        );
        // Many files: the group stops with "more", which goes to its tab.
        let many: Vec<(PathBuf, String)> = (0..20)
            .map(|i| (dir.path().join(format!("a{i}.rs")), format!("a{i}.rs")))
            .collect();
        let generation = app.everywhere.files_generation;
        let _ = app.update(Message::EverywhereFiles(generation, many));
        typed(&mut app, "a1");
        let rows = &app.everywhere.rows;
        assert!(matches!(rows[0], Row::Group(Tab::Files)));
        let files = rows.iter().filter(|r| matches!(r, Row::File(..))).count();
        assert_eq!(files, PER_GROUP);
        let more = rows
            .iter()
            .position(|r| matches!(r, Row::More(Tab::Files)))
            .unwrap();
        let _ = app.update(Message::EverywhereOpen(Some(more)));
        assert_eq!(app.everywhere.tab, Some(Tab::Files));
        // Ctrl+↓ to the last line, Ctrl+↑ back to the first.
        key(&mut app, "ArrowDown", true, false, false);
        let last = app
            .everywhere
            .rows
            .iter()
            .rposition(Row::selectable)
            .unwrap();
        assert_eq!(app.everywhere.selected, last);
        key(&mut app, "ArrowUp", true, false, false);
        assert_eq!(app.everywhere.selected, 0);
    }

    #[test]
    fn a_calculation_is_shown_and_copied() {
        let dir = project();
        let mut app = app_on(dir.path());
        open(&mut app, dir.path(), Tab::All);
        typed(&mut app, "2+3*4");
        assert!(matches!(selected(&app), Row::Calc(_, r) if r == "14"));
        let _ = app.update(Message::EverywhereOpen(None));
        assert_eq!(app.overlay, ActiveOverlay::None);
        assert!(app.status_message.contains("14"), "{}", app.status_message);
    }

    #[test]
    fn actions_by_words_abbreviations_and_settings() {
        let dir = project();
        let mut app = app_on(dir.path());
        open(&mut app, dir.path(), Tab::Actions);
        // Nothing typed: every action.
        assert!(app.everywhere.rows.len() > 20);
        // An abbreviation of one's own comes first.
        let _ = app.update(Message::EverywhereAbbreviate(
            PaletteCommand::ToggleTerminal,
        ));
        let _ = app.update(Message::EverywhereAbbreviationTyped("tt".into()));
        let _ = app.update(Message::EverywhereAbbreviationSubmit);
        assert_eq!(
            app.config.action_abbreviations.get("ToggleTerminal"),
            Some(&vec!["tt".to_string()])
        );
        typed(&mut app, "tt");
        assert!(
            matches!(selected(&app), Row::Action(e) if e.command == PaletteCommand::ToggleTerminal)
        );
        let _ = app.update(Message::EverywhereAbbreviationRemove(
            PaletteCommand::ToggleTerminal,
            "tt".into(),
        ));
        assert!(app.config.action_abbreviations.is_empty());
        // A setting is turned on or off in place, the list staying open.
        let wrap = app.config.word_wrap;
        typed(&mut app, "/");
        assert!(app
            .everywhere
            .rows
            .iter()
            .any(|r| matches!(r, Row::Page(PrefsPage::Lint))));
        let i = app
            .everywhere
            .rows
            .iter()
            .position(|r| matches!(r, Row::Setting(i) if SETTINGS[*i].get as usize == SETTINGS[0].get as usize))
            .unwrap();
        let _ = app.update(Message::EverywhereOpen(Some(i)));
        assert_eq!(app.config.word_wrap, !wrap);
        assert_eq!(app.overlay, ActiveOverlay::Everywhere);
        // Alt+Enter on an action: its shortcut is the next keys pressed.
        let _ = app.update(Message::EverywhereAssign(PaletteCommand::ToggleTerminal));
        assert_eq!(
            app.capturing,
            Some(crate::config::ShortcutTarget::ToggleTerminal)
        );
        key(&mut app, "F7", false, false, true);
        assert_eq!(app.config.shortcuts.toggle_terminal.key, "F7");
    }

    fn symbol(name: &str, kind: u64, container: Option<&str>) -> SymbolHit {
        SymbolHit {
            name: name.into(),
            kind,
            container: container.map(str::to_string),
            path: PathBuf::from("/p/src/a.rs"),
            line: 3,
            col: 4,
        }
    }

    #[test]
    fn names_from_the_servers_by_tab_and_qualified() {
        let dir = project();
        let mut app = app_on(dir.path());
        open(&mut app, dir.path(), Tab::Symbols);
        typed(&mut app, "user");
        let generation = app.everywhere.symbols_generation;
        let found = crate::lsp::Symbols {
            hits: vec![
                symbol("UserAccount", 23, None),
                symbol("user_name", 12, Some("account")),
                symbol("parse_user", 12, None),
            ],
            busy: false,
        };
        // An older answer is dropped.
        let _ = app.update(Message::EverywhereSymbols(generation - 1, found.clone()));
        assert!(app.everywhere.symbols.is_empty());
        let _ = app.update(Message::EverywhereSymbols(generation, found));
        let names = |app: &SimpleEditApp| -> Vec<String> {
            app.everywhere
                .rows
                .iter()
                .filter_map(|r| match r {
                    Row::Symbol(h) => Some(h.name.clone()),
                    _ => None,
                })
                .collect()
        };
        assert_eq!(names(&app), vec!["user_name", "UserAccount", "parse_user"]);
        let _ = app.update(Message::EverywhereTab(Tab::Classes));
        assert_eq!(names(&app), vec!["UserAccount"]);
        let _ = app.update(Message::EverywhereTab(Tab::Symbols));
        typed(&mut app, "account.user");
        assert_eq!(names(&app), vec!["user_name"]);
        // Opening one goes to its place.
        let _ = app.update(Message::EverywhereOpen(None));
        assert_eq!(app.overlay, ActiveOverlay::None);
    }

    #[test]
    fn text_results_and_their_export_to_find_in_files() {
        let dir = project();
        let mut app = app_on(dir.path());
        open(&mut app, dir.path(), Tab::Text);
        typed(&mut app, "run");
        let files: Vec<PathBuf> = app
            .everywhere
            .files
            .iter()
            .map(|(p, _)| p.clone())
            .collect();
        let found = crate::finder::search(&files, &Default::default(), "run", false, false);
        let generation = app.everywhere.text_generation;
        let _ = app.update(Message::EverywhereText(generation, found));
        assert_eq!(
            app.everywhere
                .rows
                .iter()
                .filter(|r| matches!(r, Row::Text(_)))
                .count(),
            2
        );
        let _ = app.update(Message::EverywhereExport);
        assert_eq!(app.overlay, ActiveOverlay::None);
        assert!(app.results.open);
        assert_eq!(app.results.tabs[0].hits.len(), 2);
    }

    #[test]
    fn an_export_of_every_file_of_a_large_project_is_cut_and_says_so() {
        let dir = project();
        let mut app = app_on(dir.path());
        let _ = app.update(Message::OpenEverywhere(Tab::Files));
        let many = crate::finder::MAX_HITS + 50;
        let files = (0..many)
            .map(|i| {
                let rel = format!("src/f{i}.rs");
                (dir.path().join(&rel), rel)
            })
            .collect();
        let generation = app.everywhere.files_generation;
        let _ = app.update(Message::EverywhereFiles(generation, files));
        typed(&mut app, "f");
        let _ = app.update(Message::EverywhereExport);
        assert_eq!(app.results.tabs[0].hits.len(), crate::finder::MAX_HITS);
        assert_eq!(
            app.status_message,
            t!("everywhere.export_cut", n = crate::finder::MAX_HITS).to_string()
        );
    }

    #[test]
    fn the_filter_leaves_file_types_out() {
        let dir = project();
        std::fs::write(dir.path().join("src/notes.md"), "").unwrap();
        let mut app = app_on(dir.path());
        open(&mut app, dir.path(), Tab::Files);
        typed(&mut app, "s");
        assert!(app.everywhere_types().contains(&"md".to_string()));
        let _ = app.update(Message::EverywhereFilterType("rs".into(), false));
        assert!(app
            .everywhere
            .rows
            .iter()
            .all(|r| !matches!(r, Row::File(p, ..) if file_type(p) == "rs")));
    }

    #[test]
    fn git_branches_and_commits_are_found() {
        let mut app = app_on(project().path());
        app.config.everywhere_git = true;
        let _ = app.update(Message::OpenEverywhere(Tab::Git));
        let _ = app.update(Message::EverywhereGit(
            vec!["main".into(), "feature/search".into()],
            vec![Commit {
                hash: "abc123".into(),
                subject: "Search everywhere".into(),
                author: "me".into(),
                when: "now".into(),
            }],
        ));
        typed(&mut app, "search");
        let rows = &app.everywhere.rows;
        assert!(matches!(&rows[0], Row::Branch(b) if b == "feature/search"));
        assert!(matches!(&rows[1], Row::Commit(c) if c.hash == "abc123"));
        typed(&mut app, "abc");
        assert_eq!(app.everywhere.rows.len(), 1);
    }

    #[test]
    fn shift_tapped_twice_without_another_key() {
        use iced::keyboard::{key::Named, Event, Key, Location, Modifiers};
        let shift = |pressed: bool| {
            let key = Key::Named(Named::Shift);
            if pressed {
                Event::KeyPressed {
                    key: key.clone(),
                    modified_key: key,
                    physical_key: iced::keyboard::key::Physical::Unidentified(
                        iced::keyboard::key::NativeCode::Unidentified,
                    ),
                    location: Location::Left,
                    modifiers: Modifiers::SHIFT,
                    text: None,
                    repeat: false,
                }
            } else {
                Event::KeyReleased {
                    key: key.clone(),
                    modified_key: key,
                    physical_key: iced::keyboard::key::Physical::Unidentified(
                        iced::keyboard::key::NativeCode::Unidentified,
                    ),
                    location: Location::Left,
                    modifiers: Modifiers::empty(),
                }
            }
        };
        let letter = Event::KeyPressed {
            key: Key::Character("a".into()),
            modified_key: Key::Character("A".into()),
            physical_key: iced::keyboard::key::Physical::Unidentified(
                iced::keyboard::key::NativeCode::Unidentified,
            ),
            location: Location::Standard,
            modifiers: Modifiers::SHIFT,
            text: Some("A".into()),
            repeat: false,
        };
        let t0 = std::time::Instant::now();
        let at = |ms: u64| t0 + std::time::Duration::from_millis(ms);
        let hear = double_shift::hear;
        // Two quick taps.
        assert!(!hear(&shift(true), at(0)));
        assert!(!hear(&shift(false), at(80)));
        assert!(!hear(&shift(true), at(200)));
        assert!(hear(&shift(false), at(260)));
        // Typing capitals between: no.
        assert!(!hear(&shift(true), at(1000)));
        assert!(!hear(&letter, at(1050)));
        assert!(!hear(&shift(false), at(1100)));
        assert!(!hear(&shift(true), at(1150)));
        assert!(
            !hear(&shift(false), at(1200)),
            "the first one was used to type"
        );
        // Too slow between the taps: no.
        assert!(!hear(&shift(true), at(3000)));
        assert!(!hear(&shift(false), at(3050)));
        assert!(!hear(&shift(true), at(3700)));
        assert!(!hear(&shift(false), at(3750)));
    }

    /// Draws the window with the search open, to look at it without a
    /// screen: `SNAPSHOT=/tmp/se.png cargo test everywhere_look -- --ignored`.
    #[test]
    #[ignore]
    fn everywhere_look() {
        let Ok(out) = std::env::var("SNAPSHOT") else {
            return;
        };
        let dir = project();
        let mut app = app_on(dir.path());
        open(&mut app, dir.path(), Tab::All);
        typed(&mut app, "run");
        let generation = app.everywhere.symbols_generation;
        let _ = app.update(Message::EverywhereSymbols(
            generation,
            crate::lsp::Symbols {
                hits: vec![
                    SymbolHit {
                        path: dir.path().join("src/sidebar/mod.rs"),
                        ..symbol("run", 12, Some("sidebar"))
                    },
                    SymbolHit {
                        path: dir.path().join("src/app.rs"),
                        ..symbol("Runner", 23, None)
                    },
                ],
                busy: false,
            },
        ));
        let files: Vec<PathBuf> = app
            .everywhere
            .files
            .iter()
            .map(|(p, _)| p.clone())
            .collect();
        let found = crate::finder::search(&files, &Default::default(), "run", false, false);
        let generation = app.everywhere.text_generation;
        let _ = app.update(Message::EverywhereText(generation, found));
        let settings = iced::Settings {
            default_font: se_theme::UI,
            fonts: se_theme::font_files(),
            ..Default::default()
        };
        let mut ui = iced_test::Simulator::with_size(settings, (1280.0, 800.0), app.view());
        let shot = ui.snapshot(&se_theme::ink_dark()).unwrap();
        let _ = std::fs::remove_file(format!("{out}-wgpu.png"));
        assert!(shot.matches_image(&out).unwrap());
    }
}
