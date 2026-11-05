//! "Go to file" and "find in files": two cards over the editor that look
//! into every file of the projects.

use super::*;
use crate::finder::TextHit;
use iced::widget::{column, text_input};

/// Most files shown for a name typed.
const SHOWN: usize = 60;
/// Height of a line of the lists, to scroll them to the one picked.
const ROW: f32 = 34.0;

#[derive(Default)]
pub struct FinderState {
    /// The files of the projects, with how they are shown (from the project).
    pub files: Vec<(PathBuf, String)>,
    pub loading: bool,
    pub selected: usize,
    /// What is typed in "go to file".
    pub name: String,
    /// "Find in files": what is looked for, how, and what was found.
    pub text: String,
    pub case_sensitive: bool,
    pub regex: bool,
    pub hits: Vec<TextHit>,
    pub searching: bool,
    pub error: Option<String>,
    generation: u64,
    /// The folder "find in files" looks in (none: every project).
    pub scope: Option<PathBuf>,
}

pub(super) fn goto_field_id() -> iced::widget::Id {
    iced::widget::Id::new("goto-file")
}

pub(super) fn find_field_id() -> iced::widget::Id {
    iced::widget::Id::new("find-in-files")
}

fn list_id() -> iced::widget::Id {
    iced::widget::Id::new("finder-list")
}

impl SimpleEditApp {
    /// Where to look: the projects, or the project of the file in the editor.
    fn finder_roots(&self) -> Vec<PathBuf> {
        if !self.projects.roots.is_empty() {
            return self.projects.roots.clone();
        }
        self.current_file
            .as_deref()
            .filter(|p| !is_untitled(p) && !is_virtual(p))
            .and_then(crate::definition::project_root)
            .into_iter()
            .collect()
    }

    /// Lists the files again, in the background: those of `scope` (a folder
    /// of a project), or of every project.
    fn load_finder_files(&mut self, scope: Option<PathBuf>) -> Task<Message> {
        let all = self.finder_roots();
        let several = all.len() > 1;
        // Each folder looked in, with the top of its project (the paths are
        // shown from there).
        let roots: Vec<(PathBuf, PathBuf)> = match scope {
            Some(dir) => {
                let top = all
                    .iter()
                    .find(|r| dir.starts_with(r))
                    .cloned()
                    .unwrap_or_else(|| dir.clone());
                vec![(dir, top)]
            }
            None => all.into_iter().map(|r| (r.clone(), r)).collect(),
        };
        let exclude = self.config.projects_exclude.clone();
        self.finder.loading = true;
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    roots
                        .iter()
                        .flat_map(|(dir, root)| {
                            let top = root
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_default();
                            crate::finder::list_files(dir, &exclude)
                                .into_iter()
                                .map(move |file| {
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
            Message::FinderFiles,
        )
    }

    pub(super) fn open_goto_file(&mut self) -> Task<Message> {
        self.finder.name.clear();
        self.finder.selected = 0;
        self.overlay = ActiveOverlay::GotoFile;
        Task::batch([
            self.load_finder_files(None),
            iced::widget::operation::focus(goto_field_id()),
        ])
    }

    pub(super) fn open_find_in_files(&mut self) -> Task<Message> {
        self.finder.selected = 0;
        self.finder.error = None;
        // What is selected in the editor is what is looked for.
        if let Some(selected) = self
            .editor
            .content
            .selection()
            .filter(|s| !s.is_empty() && !s.contains('\n'))
        {
            self.finder.text = selected;
        }
        // Where to look: the folder picked in the tree (its project, for a
        // file), or every project.
        self.finder.scope = self.projects.selected.as_ref().and_then(|picked| {
            if picked.is_dir() {
                Some(picked.clone())
            } else {
                self.projects
                    .roots
                    .iter()
                    .find(|r| picked.starts_with(r))
                    .cloned()
            }
        });
        // One project only: it is the whole of it, nothing to say.
        if self.projects.roots.len() == 1
            && self.finder.scope.as_ref() == self.projects.roots.first()
        {
            self.finder.scope = None;
        }
        self.overlay = ActiveOverlay::FindInFiles;
        Task::batch([
            self.load_finder_files(self.finder.scope.clone()),
            iced::widget::operation::focus(find_field_id()),
        ])
    }

    /// Looks in a folder of a project (from its menu in the tree).
    pub(super) fn find_in_folder(&mut self, dir: PathBuf) -> Task<Message> {
        self.projects.selected = Some(dir);
        self.open_find_in_files()
    }

    /// Looks in every project again, not only in the folder.
    pub(super) fn find_everywhere(&mut self) -> Task<Message> {
        self.finder.scope = None;
        self.finder.hits.clear();
        self.load_finder_files(None)
    }

    pub(super) fn finder_files(&mut self, files: Vec<(PathBuf, String)>) -> Task<Message> {
        self.finder.files = files;
        self.finder.loading = false;
        if self.overlay == ActiveOverlay::FindInFiles {
            return self.search_files(false);
        }
        Task::none()
    }

    /// The files whose name matches what is typed, best first.
    pub(super) fn goto_matches(&self) -> Vec<usize> {
        let shown: Vec<String> = self.finder.files.iter().map(|(_, s)| s.clone()).collect();
        crate::finder::rank(&self.finder.name, &shown, SHOWN)
    }

    pub(super) fn goto_typed(&mut self, name: String) {
        self.finder.name = name;
        self.finder.selected = 0;
    }

    /// Opens the file picked (or the one highlighted).
    pub(super) fn goto_open(&mut self, pick: Option<usize>) -> Task<Message> {
        let matches = self.goto_matches();
        let index = pick.unwrap_or(self.finder.selected);
        let Some(&file) = matches.get(index) else {
            return Task::none();
        };
        let path = self.finder.files[file].0.clone();
        self.overlay = ActiveOverlay::None;
        self.remember_place();
        self.activate(path)
    }

    /// Looks for the text again, a moment after the last key (`now`: at once).
    pub(super) fn search_files(&mut self, wait: bool) -> Task<Message> {
        self.finder.generation += 1;
        let generation = self.finder.generation;
        self.finder.error = None;
        if self.finder.text.is_empty() {
            self.finder.hits.clear();
            self.finder.searching = false;
            return Task::none();
        }
        self.finder.searching = true;
        let files: Vec<PathBuf> = self.finder.files.iter().map(|(p, _)| p.clone()).collect();
        let (query, case, regex) = (
            self.finder.text.clone(),
            self.finder.case_sensitive,
            self.finder.regex,
        );
        Task::perform(
            async move {
                if wait {
                    tokio::time::sleep(std::time::Duration::from_millis(220)).await;
                }
                tokio::task::spawn_blocking(move || {
                    crate::finder::search(&files, &query, case, regex)
                })
                .await
                .unwrap_or_else(|e| Err(e.to_string()))
            },
            move |result| Message::FindResults(generation, result),
        )
    }

    pub(super) fn find_results(&mut self, generation: u64, result: Result<Vec<TextHit>, String>) {
        // An older search, for what was typed before.
        if generation != self.finder.generation {
            return;
        }
        self.finder.searching = false;
        match result {
            Ok(hits) => {
                self.finder.hits = hits;
                self.finder.selected = 0;
            }
            Err(e) => {
                self.finder.hits.clear();
                self.finder.error = Some(e);
            }
        }
    }

    /// Goes to the place picked (or the one highlighted).
    pub(super) fn find_open(&mut self, pick: Option<usize>) -> Task<Message> {
        let index = pick.unwrap_or(self.finder.selected);
        let Some(hit) = self.finder.hits.get(index).cloned() else {
            return Task::none();
        };
        self.overlay = ActiveOverlay::None;
        self.jump_to(crate::definition::Hit {
            path: hit.path,
            line: hit.line,
            col: hit.col,
            len: hit.len,
        })
    }

    /// The arrows move in the list of a finder card.
    pub(super) fn finder_step(&mut self, delta: i32) -> Task<Message> {
        let len = match self.overlay {
            ActiveOverlay::GotoFile => self.goto_matches().len(),
            ActiveOverlay::FindInFiles => self.finder.hits.len(),
            _ => return Task::none(),
        };
        self.finder.selected = crate::palette::step(self.finder.selected, delta, len);
        iced::widget::operation::scroll_to(
            list_id(),
            iced::widget::scrollable::AbsoluteOffset {
                x: 0.0,
                y: ((self.finder.selected as f32 - 4.0) * (ROW + 1.0)).max(0.0),
            },
        )
    }

    fn finder_row<'a>(
        &self,
        dark: bool,
        highlighted: bool,
        content: Element<'a, Message>,
        on_press: Message,
    ) -> Element<'a, Message> {
        button(container(content).center_y(Length::Fixed(ROW)))
            .padding([0, 10])
            .width(Length::Fill)
            .on_press(on_press)
            .style(
                (se_theme::MenuItem {
                    dark,
                    radius: se_theme::R_ROW,
                    highlighted,
                })
                .style(),
            )
            .into()
    }

    fn finder_card<'a>(
        &self,
        dark: bool,
        field: Element<'a, Message>,
        note: String,
        rows: Vec<Element<'a, Message>>,
    ) -> Element<'a, Message> {
        let p = se_theme::palette(dark);
        let mut list = column(rows).spacing(1).padding(6);
        if !note.is_empty() {
            list = list.push(container(text(note).size(12.5).color(p.muted)).padding([9, 10]));
        }
        container(column![
            container(field).padding([0, 16]).height(52).center_y(52),
            container(Space::new().height(1))
                .width(Length::Fill)
                .style(se_theme::rule(dark)),
            scrollable(list)
                .id(list_id())
                .style(se_theme::Scrollbar { dark }.style())
                .height(Length::Fixed(420.0)),
        ])
        .width(680)
        .style(se_theme::palette_card(dark))
        .into()
    }

    pub(super) fn view_goto_file(&self, dark: bool) -> Element<'_, Message> {
        let p = se_theme::palette(dark);
        let field = text_input(&t!("finder.goto_placeholder"), &self.finder.name)
            .id(goto_field_id())
            .on_input(Message::GotoFileQuery)
            .on_submit(Message::GotoFileOpen(None))
            .size(15)
            .padding(0)
            .style((se_theme::BareField { dark }).style());
        let matches = self.goto_matches();
        let rows = matches
            .iter()
            .enumerate()
            .map(|(i, &file)| {
                let shown = &self.finder.files[file].1;
                let (dir, name) = match shown.rsplit_once('/') {
                    Some((dir, name)) => (dir.to_string(), name.to_string()),
                    None => (String::new(), shown.clone()),
                };
                let highlighted = i == self.finder.selected;
                self.finder_row(
                    dark,
                    highlighted,
                    row![
                        crate::file_icons::file(std::path::Path::new(&name), dark),
                        text(name)
                            .size(13)
                            .font(se_theme::UI_SEMIBOLD)
                            .shaping(iced::widget::text::Shaping::Advanced)
                            .color(if highlighted { p.accent } else { p.text }),
                        text(dir)
                            .size(12)
                            .color(p.muted)
                            .shaping(iced::widget::text::Shaping::Advanced),
                    ]
                    .spacing(10)
                    .align_y(Alignment::Center)
                    .into(),
                    Message::GotoFileOpen(Some(i)),
                )
            })
            .collect();
        let note = if self.finder.loading && self.finder.files.is_empty() {
            t!("finder.loading").to_string()
        } else if self.finder_roots().is_empty() {
            t!("finder.no_project").to_string()
        } else if matches.is_empty() {
            t!("palette.no_results").to_string()
        } else {
            String::new()
        };
        self.finder_card(dark, field.into(), note, rows)
    }

    pub(super) fn view_find_in_files(&self, dark: bool) -> Element<'_, Message> {
        let p = se_theme::palette(dark);
        let toggle = |label: &'static str, on: bool, tip: String, msg: Message| {
            iced::widget::tooltip(
                button(text(label).size(12.5).font(se_theme::MONO))
                    .padding([3, 8])
                    .on_press(msg)
                    .style((se_theme::GhostButton { dark, active: on }).style()),
                container(text(tip).size(12))
                    .padding([4, 8])
                    .style(se_theme::tooltip(dark)),
                iced::widget::tooltip::Position::Bottom,
            )
        };
        // Where it looks, when it is one folder: a click on the cross widens
        // it to every project.
        let scope: Element<Message> = match &self.finder.scope {
            Some(dir) => {
                let name = dir
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                container(
                    row![
                        crate::sidebar::icons::folder(p.accent),
                        text(name)
                            .size(12)
                            .color(p.text)
                            .shaping(iced::widget::text::Shaping::Advanced),
                        button(text("×").size(13).color(p.muted))
                            .padding([0, 4])
                            .on_press(Message::FindEverywhere)
                            .style(
                                (se_theme::GhostButton {
                                    dark,
                                    active: false
                                })
                                .style()
                            ),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center),
                )
                .padding([2, 8])
                .style(se_theme::badge(dark))
                .into()
            }
            None => Space::new().width(0).into(),
        };
        let field = row![
            scope,
            text_input(&t!("finder.find_placeholder"), &self.finder.text)
                .id(find_field_id())
                .on_input(Message::FindQuery)
                .on_submit(Message::FindOpen(None))
                .size(15)
                .padding(0)
                .style((se_theme::BareField { dark }).style()),
            toggle(
                "Aa",
                self.finder.case_sensitive,
                t!("search.case_sensitive").to_string(),
                Message::FindToggleCase
            ),
            toggle(
                ".*",
                self.finder.regex,
                t!("search.regex").to_string(),
                Message::FindToggleRegex
            ),
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        let rows = self
            .finder
            .hits
            .iter()
            .enumerate()
            .map(|(i, hit)| {
                let highlighted = i == self.finder.selected;
                let name = hit
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let (from, to) = hit.preview_at;
                let preview = &hit.preview;
                let part = |a: usize, b: usize| preview.get(a..b).unwrap_or("").to_string();
                self.finder_row(
                    dark,
                    highlighted,
                    row![
                        crate::file_icons::file(&hit.path, dark),
                        text(format!("{name}:{}", hit.line + 1))
                            .size(12)
                            .color(if highlighted { p.accent } else { p.muted })
                            .width(Length::Fixed(170.0))
                            .wrapping(iced::widget::text::Wrapping::None),
                        row![
                            text(part(0, from)).size(12.5).font(se_theme::MONO),
                            text(part(from, to))
                                .size(12.5)
                                .font(se_theme::MONO)
                                .color(p.accent),
                            text(part(to, preview.len()))
                                .size(12.5)
                                .font(se_theme::MONO),
                        ]
                        .clip(true),
                    ]
                    .spacing(10)
                    .align_y(Alignment::Center)
                    .into(),
                    Message::FindOpen(Some(i)),
                )
            })
            .collect();
        let note = if let Some(error) = &self.finder.error {
            error.clone()
        } else if self.finder_roots().is_empty() {
            t!("finder.no_project").to_string()
        } else if self.finder.searching || (self.finder.loading && self.finder.files.is_empty()) {
            t!("finder.searching").to_string()
        } else if self.finder.text.is_empty() {
            String::new()
        } else if self.finder.hits.is_empty() {
            t!("palette.no_results").to_string()
        } else if self.finder.hits.len() >= crate::finder::MAX_HITS {
            t!("finder.too_many", n = crate::finder::MAX_HITS).to_string()
        } else {
            t!("finder.found", n = self.finder.hits.len()).to_string()
        };
        self.finder_card(dark, field.into(), note, rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app_on(dir: &Path) -> SimpleEditApp {
        let mut app = SimpleEditApp::blank(Config::default());
        let _ = app.restore_session(crate::session::Session::default());
        let _ = app.update(Message::ProjectPicked(Some(dir.to_path_buf())));
        app
    }

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

    fn listed(app: &mut SimpleEditApp, dir: &Path) {
        let files = crate::finder::list_files(dir, &[])
            .into_iter()
            .map(|f| {
                let rel = f.strip_prefix(dir).unwrap().to_string_lossy().into_owned();
                (f, rel)
            })
            .collect();
        let _ = app.update(Message::FinderFiles(files));
    }

    #[test]
    fn a_file_is_found_by_its_name_and_opened() {
        let dir = project();
        let mut app = app_on(dir.path());
        let _ = app.update(Message::OpenGotoFile);
        assert_eq!(app.overlay, ActiveOverlay::GotoFile);
        listed(&mut app, dir.path());
        let _ = app.update(Message::GotoFileQuery("sbmod".into()));
        let first = app.goto_matches()[0];
        assert_eq!(app.finder.files[first].1, "src/sidebar/mod.rs");
        // The arrows move, Enter opens the one highlighted.
        let _ = app.update(Message::GotoFileQuery("rs".into()));
        assert_eq!(app.goto_matches().len(), 2);
        let _ = app.finder_step(1);
        let second = app.finder.files[app.goto_matches()[1]].0.clone();
        let _ = app.update(Message::GotoFileOpen(None));
        assert_eq!(app.overlay, ActiveOverlay::None);
        assert_eq!(app.pending_open.as_ref(), Some(&second));
    }

    #[test]
    fn a_text_is_found_in_the_files_and_its_place_opened() {
        let dir = project();
        let mut app = app_on(dir.path());
        let _ = app.update(Message::OpenFindInFiles);
        assert_eq!(app.overlay, ActiveOverlay::FindInFiles);
        listed(&mut app, dir.path());
        let _ = app.update(Message::FindQuery("run".into()));
        assert!(app.finder.searching);
        // The search runs in the background; its answer is taken in by its
        // generation only.
        let files: Vec<PathBuf> = app.finder.files.iter().map(|(p, _)| p.clone()).collect();
        let found = crate::finder::search(&files, "run", false, false);
        let generation = app.finder.generation;
        let _ = app.update(Message::FindResults(generation - 1, Ok(Vec::new())));
        assert!(app.finder.searching, "an older answer is dropped");
        let _ = app.update(Message::FindResults(generation, found));
        assert_eq!(app.finder.hits.len(), 2);
        let target = app.finder.hits[1].clone();
        let _ = app.update(Message::FindOpen(Some(1)));
        assert_eq!(app.overlay, ActiveOverlay::None);
        assert_eq!(app.pending_open.as_ref(), Some(&target.path));
        // A bad regular expression is said, not a crash.
        let _ = app.update(Message::OpenFindInFiles);
        let _ = app.update(Message::FindToggleRegex);
        let generation = app.finder.generation;
        let _ = app.update(Message::FindResults(
            generation,
            crate::finder::search(&files, "(", false, true),
        ));
        assert!(app.finder.error.is_some() && app.finder.hits.is_empty());
    }

    #[test]
    fn find_in_files_looks_in_the_folder_picked_in_the_tree() {
        let dir = project();
        std::fs::write(dir.path().join("README.md"), "run it\n").unwrap();
        let mut app = app_on(dir.path());
        let sidebar = dir.path().join("src/sidebar");
        // A folder picked in the tree: only there.
        let _ = app.update(Message::ToggleProjectFolder(dir.path().join("src")));
        let _ = app.update(Message::ToggleProjectFolder(sidebar.clone()));
        let _ = app.update(Message::OpenFindInFiles);
        assert_eq!(app.finder.scope.as_ref(), Some(&sidebar));
        // The listing of that folder, as the background task makes it.
        let files: Vec<(PathBuf, String)> = crate::finder::list_files(&sidebar, &[])
            .into_iter()
            .map(|f| {
                let rel = f
                    .strip_prefix(dir.path())
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                (f, rel)
            })
            .collect();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].1, "src/sidebar/mod.rs", "shown from the project");
        // The cross widens it to everything.
        let _ = app.update(Message::FindEverywhere);
        assert_eq!(app.finder.scope, None);
        // The top of the only project: no folder to say.
        let _ = app.update(Message::ToggleProjectFolder(dir.path().to_path_buf()));
        let _ = app.update(Message::OpenFindInFiles);
        assert_eq!(app.finder.scope, None);
        // A file picked: its project.
        let _ = app.update(Message::TreeOpenFile(dir.path().join("README.md")));
        let _ = app.update(Message::OpenFindInFiles);
        assert_eq!(app.finder.scope, None);
        // From the menu of a folder.
        let _ = app.update(Message::FindInFolder(dir.path().join("src")));
        assert_eq!(app.finder.scope, Some(dir.path().join("src")));
        assert_eq!(app.overlay, ActiveOverlay::FindInFiles);
    }

    #[test]
    fn without_a_project_the_project_of_the_file_is_looked_into() {
        let dir = project();
        let mut app = SimpleEditApp::blank(Config::default());
        let _ = app.restore_session(crate::session::Session::default());
        assert!(app.finder_roots().is_empty());
        std::fs::write(dir.path().join("Cargo.toml"), "").unwrap();
        let _ = app.update(Message::TabRead(Ok(Loaded {
            path: dir.path().join("src/app.rs"),
            text: String::new(),
            format: TextFormat::default(),
            stamp: None,
        })));
        assert_eq!(app.finder_roots(), vec![dir.path().to_path_buf()]);
    }
}
