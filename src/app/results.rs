//! The Results panel, as IntelliJ IDEA's Find tool window: the matches of a
//! search in tabs (pinned or not), by file (and folder), to go through,
//! leave out, replace, run again, export.

use super::finder::{counts, files_text, matches_text, Occurrence, Search};
use super::*;
use iced::widget::{column, mouse_area};

/// What the panel's controls send.
#[derive(Debug, Clone)]
pub enum ResultsMsg {
    Select(usize),
    Open(usize),
    Exclude,
    Include,
    Remove,
    ReplaceSelected,
    ReplaceAll,
    Rerun,
    Rerun_(u64, Result<Vec<Occurrence>, String>),
    Export,
    Exported(Result<Option<PathBuf>, String>),
    Copy,
    Tab(usize),
    Close(usize),
    Pin(usize),
    Group,
    Scrolled(f32, f32),
    NewTab,
    Hide,
    /// The next (or previous) match, opened.
    Step(bool),
}

/// The matches of one search.
pub struct ResultTab {
    pub title: String,
    /// The search they come from (none: sent from search everywhere).
    pub search: Option<Search>,
    pub hits: Vec<Occurrence>,
    pub selected: usize,
    pub pinned: bool,
}

#[derive(Default)]
pub struct Results {
    pub open: bool,
    pub tabs: Vec<ResultTab>,
    pub active: usize,
    /// Under their folders too, not only their files.
    pub group_dirs: bool,
    /// Each search in a tab of its own.
    pub new_tab: bool,
    /// Clicked last: its keys are its own.
    pub focus: bool,
    generation: u64,
    /// How far the list is scrolled, and how high it is (only the lines in
    /// view are widgets).
    pub scrolled: f32,
    pub view_height: f32,
}

const ROW: f32 = 26.0;
/// The height of a line that names a folder, and of one that names a file.
const DIR_ROW: f32 = 24.0;
const FILE_ROW: f32 = 26.0;

/// How many matches each file has, in one pass: counted file by file, a
/// list of every file of a project took seconds to draw.
fn per_file(hits: &[Occurrence]) -> std::collections::HashMap<&Path, usize> {
    let mut counts = std::collections::HashMap::new();
    for o in hits {
        *counts.entry(o.hit.path.as_path()).or_insert(0) += 1;
    }
    counts
}

fn glyph<'a>(code: u32, color: iced::Color) -> Element<'a, Message> {
    text(char::from_u32(code).unwrap_or(' '))
        .font(iced_fonts::NERD_FONT)
        .size(14)
        .color(color)
        .into()
}

impl SimpleEditApp {
    /// Shows matches in the panel: in the current tab, or a new one when
    /// asked so or when the current one is pinned.
    pub(super) fn results_add(
        &mut self,
        title: String,
        search: Option<Search>,
        hits: Vec<Occurrence>,
    ) {
        let tab = ResultTab {
            title,
            search,
            hits,
            selected: 0,
            pinned: false,
        };
        let r = &mut self.results;
        match r.tabs.get_mut(r.active) {
            Some(current) if !r.new_tab && !current.pinned => *current = tab,
            _ => {
                r.tabs.push(tab);
                r.active = r.tabs.len() - 1;
                r.scrolled = 0.0;
            }
        }
        r.open = true;
        r.focus = true;
    }

    /// The window's matches, to the panel (Ctrl+Enter).
    pub(super) fn finder_to_window(&mut self) -> Task<Message> {
        if self.finder.hits.is_empty() {
            return Task::none();
        }
        let (n, files) = counts(&self.finder.hits);
        let query = self.finder.text.replace('\n', "⏎");
        let (matches, files) = (matches_text(n), files_text(files));
        let (q, m, f) = (query, matches, files);
        let title = t!("results.title", query = q, matches = m, files = f);
        let title = title.to_string();
        let search = self.finder.searched.clone();
        let hits = self.finder.hits.clone();
        self.results_add(title, search, hits);
        self.overlay = ActiveOverlay::None;
        Task::none()
    }

    fn results_tab(&mut self) -> Option<&mut ResultTab> {
        let active = self.results.active;
        self.results.tabs.get_mut(active)
    }

    /// Goes to a match of the current tab (the panel stays).
    fn results_open(&mut self, index: usize) -> Task<Message> {
        let Some(tab) = self.results_tab() else {
            return Task::none();
        };
        let Some(hit) = tab.hits.get(index).map(|o| o.hit.clone()) else {
            return Task::none();
        };
        tab.selected = index;
        self.remember_place();
        self.jump_to(crate::definition::Hit {
            path: hit.path,
            line: hit.line,
            col: hit.col,
            len: hit.len,
        })
    }

    /// Replaces in the current tab: the selected match, or all those not
    /// left out; they leave the list.
    fn results_replace(&mut self, all: bool) -> Task<Message> {
        let Some(tab) = self.results.tabs.get(self.results.active) else {
            return Task::none();
        };
        let Some(search) = tab.search.clone().filter(|s| s.replace.is_some()) else {
            return Task::none();
        };
        let chosen: Vec<usize> = if all {
            (0..tab.hits.len())
                .filter(|i| !tab.hits[*i].excluded)
                .collect()
        } else {
            tab.hits
                .get(tab.selected)
                .filter(|o| !o.excluded)
                .map(|_| vec![tab.selected])
                .unwrap_or_default()
        };
        let hits: Vec<crate::finder::TextHit> =
            chosen.iter().map(|i| tab.hits[*i].hit.clone()).collect();
        let (count, files, done) = self.replace_found(&search, &hits);
        // Only what was replaced leaves the list.
        let left = chosen.len() - done.len();
        if let Some(tab) = self.results_tab() {
            let mut gone: Vec<usize> = done.into_iter().map(|d| chosen[d]).collect();
            gone.sort_unstable();
            for i in gone.into_iter().rev() {
                tab.hits.remove(i);
            }
            tab.selected = tab.selected.min(tab.hits.len().saturating_sub(1));
        }
        if left > 0 {
            self.set_status(t!("results.not_replaced", n = left), true);
            return Task::none();
        }
        let (matches, files) = (matches_text(count), files_text(files));
        let done = t!("findrep.replaced", matches = matches, files = files);
        self.set_status(done, false);
        Task::none()
    }

    /// What the matches of the current tab read as text, one per line.
    fn results_text(&self) -> String {
        let Some(tab) = self.results.tabs.get(self.results.active) else {
            return String::new();
        };
        let mut out = format!("{}\n", tab.title);
        for o in tab.hits.iter().filter(|o| !o.excluded) {
            out.push_str(&format!(
                "{}:{}:{}: {}\n",
                display_path(&o.hit.path),
                o.hit.line + 1,
                o.hit.col + 1,
                o.hit.preview
            ));
        }
        out
    }

    pub(super) fn results_update(&mut self, msg: ResultsMsg) -> Task<Message> {
        if !matches!(
            msg,
            ResultsMsg::Hide | ResultsMsg::Rerun_(..) | ResultsMsg::Exported(_)
        ) {
            self.results.focus = true;
        }
        match msg {
            ResultsMsg::Select(i) => {
                if let Some(tab) = self.results_tab() {
                    tab.selected = i;
                }
                Task::none()
            }
            ResultsMsg::Open(i) => self.results_open(i),
            ResultsMsg::Exclude | ResultsMsg::Include => {
                let exclude = matches!(msg, ResultsMsg::Exclude);
                if let Some(tab) = self.results_tab() {
                    let i = tab.selected;
                    if let Some(o) = tab.hits.get_mut(i) {
                        o.excluded = exclude;
                    }
                    tab.selected = (i + 1).min(tab.hits.len().saturating_sub(1));
                }
                Task::none()
            }
            ResultsMsg::Remove => {
                if let Some(tab) = self.results_tab() {
                    if tab.selected < tab.hits.len() {
                        tab.hits.remove(tab.selected);
                        tab.selected = tab.selected.min(tab.hits.len().saturating_sub(1));
                    }
                }
                Task::none()
            }
            ResultsMsg::ReplaceSelected => self.results_replace(false),
            ResultsMsg::ReplaceAll => self.results_replace(true),
            ResultsMsg::Rerun => {
                let Some(search) = self
                    .results
                    .tabs
                    .get(self.results.active)
                    .and_then(|t| t.search.clone())
                else {
                    return Task::none();
                };
                self.results.generation += 1;
                let generation = self.results.generation;
                self.run_search(
                    search,
                    false,
                    Default::default(),
                    Default::default(),
                    move |found| Message::Results(ResultsMsg::Rerun_(generation, found)),
                )
            }
            ResultsMsg::Rerun_(generation, found) => {
                if generation == self.results.generation {
                    if let (Some(tab), Ok(hits)) = (self.results_tab(), found) {
                        tab.hits = hits;
                        tab.selected = 0;
                    }
                }
                Task::none()
            }
            ResultsMsg::Export => {
                let content = self.results_text();
                Task::perform(
                    async move {
                        let Some(handle) = rfd::AsyncFileDialog::new()
                            .set_file_name("results.txt")
                            .save_file()
                            .await
                        else {
                            return Ok(None);
                        };
                        let path = handle.path().to_path_buf();
                        tokio::fs::write(&path, content)
                            .await
                            .map(|_| Some(path))
                            .map_err(|e| e.to_string())
                    },
                    |r| Message::Results(ResultsMsg::Exported(r)),
                )
            }
            ResultsMsg::Exported(result) => {
                match result {
                    Ok(Some(path)) => self.set_status(
                        t!("results.exported", path = display_path(&path).as_str()),
                        false,
                    ),
                    Ok(None) => {}
                    Err(e) => self.set_status(format!("{}: {e}", t!("status.error")), true),
                }
                Task::none()
            }
            ResultsMsg::Copy => {
                self.set_status(t!("results.copied"), false);
                iced::clipboard::write(self.results_text())
            }
            ResultsMsg::Tab(i) => {
                if i < self.results.tabs.len() {
                    self.results.active = i;
                    self.results.scrolled = 0.0;
                }
                Task::none()
            }
            ResultsMsg::Close(i) => {
                if i < self.results.tabs.len() {
                    self.results.tabs.remove(i);
                    self.results.active = self
                        .results
                        .active
                        .min(self.results.tabs.len().saturating_sub(1));
                    self.results.scrolled = 0.0;
                }
                if self.results.tabs.is_empty() {
                    self.results.open = false;
                    self.results.focus = false;
                }
                Task::none()
            }
            ResultsMsg::Pin(i) => {
                if let Some(tab) = self.results.tabs.get_mut(i) {
                    tab.pinned = !tab.pinned;
                }
                Task::none()
            }
            ResultsMsg::Scrolled(offset, height) => {
                self.results.scrolled = offset;
                self.results.view_height = height;
                Task::none()
            }
            ResultsMsg::Group => {
                self.results.group_dirs = !self.results.group_dirs;
                Task::none()
            }
            ResultsMsg::NewTab => {
                self.results.new_tab = !self.results.new_tab;
                Task::none()
            }
            ResultsMsg::Hide => {
                self.results.open = !self.results.open && !self.results.tabs.is_empty();
                self.results.focus = self.results.open;
                Task::none()
            }
            ResultsMsg::Step(next) => {
                let Some(tab) = self.results_tab() else {
                    return Task::none();
                };
                if tab.hits.is_empty() {
                    return Task::none();
                }
                let len = tab.hits.len() as i64;
                let i = (tab.selected as i64 + if next { 1 } else { -1 }).rem_euclid(len) as usize;
                self.results.open = true;
                self.results_open(i)
            }
        }
    }

    /// A key while the panel has the focus; `None` when it isn't its.
    pub(super) fn results_key(
        &mut self,
        key: &str,
        ctrl: bool,
        shift: bool,
        alt: bool,
    ) -> Option<Task<Message>> {
        if !self.results.focus || !self.results.open || self.results.tabs.is_empty() {
            return None;
        }
        let msg = match (key, ctrl, shift, alt) {
            ("ArrowDown", false, false, false) | ("ArrowUp", false, false, false) => {
                let tab = self.results_tab()?;
                let len = tab.hits.len().max(1) as i64;
                let delta = if key == "ArrowDown" { 1 } else { -1 };
                tab.selected = (tab.selected as i64 + delta).rem_euclid(len) as usize;
                return Some(Task::none());
            }
            ("Return", false, false, false) | ("F4", false, false, false) => {
                let selected = self.results.tabs.get(self.results.active)?.selected;
                ResultsMsg::Open(selected)
            }
            ("Delete", false, false, false) => ResultsMsg::Exclude,
            ("Insert", false, false, false) => ResultsMsg::Include,
            ("x", true, false, false) => ResultsMsg::Remove,
            ("l", false, false, true) => ResultsMsg::ReplaceSelected,
            ("d", false, false, true) => ResultsMsg::ReplaceAll,
            ("F5", true, false, false) => ResultsMsg::Rerun,
            ("o", false, false, true) => ResultsMsg::Export,
            _ => return None,
        };
        Some(self.results_update(msg))
    }

    pub(super) fn results_view(&self, dark: bool) -> Element<'_, Message> {
        let p = se_theme::palette(dark);
        let ghost = move |active: bool| (se_theme::GhostButton { dark, active }).style();
        let icon_button = |code: u32, tip: String, msg: Option<Message>, on: bool| {
            let mut b = button(glyph(code, if on { p.accent } else { p.muted }))
                .padding([2, 6])
                .style(ghost(on));
            if let Some(m) = msg {
                b = b.on_press(m);
            }
            iced::widget::tooltip(
                b,
                container(text(tip).size(12))
                    .padding([4, 8])
                    .style(se_theme::tooltip(dark)),
                iced::widget::tooltip::Position::Top,
            )
        };
        let r = &self.results;
        let tab = r.tabs.get(r.active);
        let replacing = tab.is_some_and(|t| t.search.as_ref().is_some_and(|s| s.replace.is_some()));

        let mut header = row![text(t!("results.panel").to_string())
            .size(12)
            .font(se_theme::UI_SEMIBOLD)
            .color(p.text)]
        .spacing(4)
        .align_y(Alignment::Center);
        for (i, t) in r.tabs.iter().enumerate() {
            let mut label = row![text(t.title.clone()).size(12)]
                .spacing(4)
                .align_y(Alignment::Center);
            if t.pinned {
                label = label.push(glyph(0xeba0, p.accent));
            }
            header = header.push(
                button(label)
                    .padding([2, 8])
                    .on_press(Message::Results(ResultsMsg::Tab(i)))
                    .style(ghost(i == r.active)),
            );
        }
        header = header.push(Space::new().width(Length::Fill));
        if let Some(t) = tab {
            let active = r.active;
            header = header
                .push(icon_button(
                    0xeb2b,
                    t!("results.pin").to_string(),
                    Some(Message::Results(ResultsMsg::Pin(active))),
                    t.pinned,
                ))
                .push(icon_button(
                    0xeb37,
                    t!("results.rerun").to_string(),
                    t.search
                        .as_ref()
                        .map(|_| Message::Results(ResultsMsg::Rerun)),
                    false,
                ))
                .push(icon_button(
                    0xeb86,
                    t!("results.group_dirs").to_string(),
                    Some(Message::Results(ResultsMsg::Group)),
                    r.group_dirs,
                ))
                .push(icon_button(
                    0xeaa1,
                    t!("results.previous").to_string(),
                    Some(Message::Results(ResultsMsg::Step(false))),
                    false,
                ))
                .push(icon_button(
                    0xea9a,
                    t!("results.next").to_string(),
                    Some(Message::Results(ResultsMsg::Step(true))),
                    false,
                ))
                .push(icon_button(
                    0xebac,
                    t!("results.export").to_string(),
                    Some(Message::Results(ResultsMsg::Export)),
                    false,
                ))
                .push(icon_button(
                    0xebcc,
                    t!("results.copy").to_string(),
                    Some(Message::Results(ResultsMsg::Copy)),
                    false,
                ))
                .push(icon_button(
                    0xea76,
                    t!("results.close_tab").to_string(),
                    Some(Message::Results(ResultsMsg::Close(active))),
                    false,
                ));
        }
        header = header.push(
            iced::widget::checkbox(r.new_tab)
                .label(t!("results.new_tab").to_string())
                .text_size(11.5)
                .size(13)
                .on_toggle(|_| Message::Results(ResultsMsg::NewTab)),
        );
        header = header.push(
            button(text("—").size(12))
                .padding([1, 8])
                .on_press(Message::Results(ResultsMsg::Hide))
                .style(ghost(false)),
        );

        let mut list = column![].spacing(0);
        if let Some(t) = tab {
            let per_file = per_file(&t.hits);
            // First the layout, in numbers: each line has a height that does
            // not depend on what it holds, so that only the lines in view
            // (and a few around) are made into widgets.
            enum Line<'x> {
                Dir(Option<&'x Path>),
                File(&'x Path, usize),
                Hit(usize),
            }
            let mut layout: Vec<(Line, f32)> = Vec::with_capacity(t.hits.len() * 2);
            {
                let mut last_dir: Option<&Path> = None;
                let mut last_file: Option<&Path> = None;
                for (i, o) in t.hits.iter().enumerate() {
                    let path = o.hit.path.as_path();
                    if r.group_dirs {
                        let dir = path.parent();
                        if dir != last_dir {
                            last_dir = dir;
                            layout.push((Line::Dir(dir), DIR_ROW));
                        }
                    }
                    if Some(path) != last_file {
                        last_file = Some(path);
                        let in_file = per_file.get(path).copied().unwrap_or(0);
                        layout.push((Line::File(path, in_file), FILE_ROW));
                    }
                    layout.push((Line::Hit(i), ROW));
                }
            }
            let (offset, height) = (r.scrolled, r.view_height.max(120.0));
            let margin = 6.0 * ROW;
            let mut top = 0.0f32;
            let mut above = 0.0f32;
            let mut below = 0.0f32;
            for (line, h) in &layout {
                let bottom = top + h;
                if bottom < offset - margin {
                    above += h;
                } else if top > offset + height + margin {
                    below += h;
                } else {
                    match line {
                        Line::Dir(dir) => {
                            list = list.push(
                                container(
                                    row![
                                        crate::file_icons::folder(true, dark),
                                        text(dir.map(display_path).unwrap_or_default())
                                            .size(12)
                                            .color(p.muted)
                                    ]
                                    .spacing(6)
                                    .align_y(Alignment::Center),
                                )
                                .height(Length::Fixed(DIR_ROW))
                                .center_y(Length::Fixed(DIR_ROW))
                                .padding([0, 8]),
                            );
                        }
                        Line::File(path, in_file) => {
                            let name = path
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_default();
                            let shown = if r.group_dirs {
                                name
                            } else {
                                display_path(path)
                            };
                            list = list.push(
                                container(
                                    row![
                                        crate::file_icons::file(path, dark),
                                        text(shown)
                                            .size(12.5)
                                            .font(se_theme::UI_MEDIUM)
                                            .color(p.text),
                                        text(matches_text(*in_file)).size(11.5).color(p.muted),
                                    ]
                                    .spacing(6)
                                    .align_y(Alignment::Center),
                                )
                                .height(Length::Fixed(FILE_ROW))
                                .center_y(Length::Fixed(FILE_ROW))
                                .padding(iced::Padding {
                                    top: 0.0,
                                    bottom: 0.0,
                                    left: if r.group_dirs { 22.0 } else { 8.0 },
                                    right: 8.0,
                                }),
                            );
                        }
                        Line::Hit(i) => {
                            let o = &t.hits[*i];
                            let selected = *i == t.selected;
                            let row_view = button(
                                container(self.occurrence_line(o, dark, selected))
                                    .center_y(Length::Fixed(ROW)),
                            )
                            .padding(iced::Padding {
                                top: 0.0,
                                bottom: 0.0,
                                left: if r.group_dirs { 36.0 } else { 22.0 },
                                right: 8.0,
                            })
                            .width(Length::Fill)
                            .on_press(Message::Results(ResultsMsg::Select(*i)))
                            .style((se_theme::SidebarRow { dark, selected }).style());
                            list = list.push(
                                mouse_area(row_view)
                                    .on_double_click(Message::Results(ResultsMsg::Open(*i))),
                            );
                        }
                    }
                }
                top = bottom;
            }
            list = column![
                Space::new().height(Length::Fixed(above)),
                list,
                Space::new().height(Length::Fixed(below)),
            ];
            if t.hits.is_empty() {
                list = list.push(
                    container(
                        text(t!("everywhere.nothing").to_string())
                            .size(12)
                            .color(p.muted),
                    )
                    .padding([6, 10]),
                );
            }
        }

        let mut footer = row![text(t!("results.keys").to_string()).size(11).color(p.muted)]
            .spacing(8)
            .align_y(Alignment::Center);
        if replacing {
            footer = footer
                .push(Space::new().width(Length::Fill))
                .push(
                    button(text(t!("results.replace_selected").to_string()).size(12))
                        .padding([4, 10])
                        .on_press(Message::Results(ResultsMsg::ReplaceSelected))
                        .style((se_theme::Outline { dark }).style()),
                )
                .push(
                    button(text(t!("results.replace_all").to_string()).size(12))
                        .padding([4, 10])
                        .on_press(Message::Results(ResultsMsg::ReplaceAll))
                        .style((se_theme::Primary { dark }).style()),
                );
        }
        if !self.finder.undo.is_empty() {
            footer = footer.push(
                button(text(t!("findrep.undo").to_string()).size(12))
                    .padding([4, 10])
                    .on_press(Message::FindUndoReplace)
                    .style(ghost(false)),
            );
        }

        column![
            resize_edge(dark),
            container(
                column![
                    header,
                    scrollable(list)
                        .on_scroll(|v| Message::Results(ResultsMsg::Scrolled(
                            v.absolute_offset().y,
                            v.bounds().height
                        )))
                        .style(se_theme::Scrollbar { dark }.style())
                        .height(Length::Fill),
                    footer,
                ]
                .spacing(6)
            )
            .padding([8, 12])
            .height(Length::Fill),
        ]
        .height(Length::Fixed(self.panel_height))
        .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::finder::TextHit;

    fn occurrence(path: &str, line: usize) -> Occurrence {
        Occurrence {
            hit: TextHit {
                path: PathBuf::from(path),
                line,
                col: 0,
                len: 3,
                preview: "run".into(),
                preview_at: (0, 3),
                stamp: 0,
            },
            replacement: None,
            excluded: false,
        }
    }

    fn app() -> SimpleEditApp {
        let mut app = SimpleEditApp::blank(Config::default());
        let _ = app.restore_session(crate::session::Session::default());
        app
    }

    #[test]
    fn the_matches_of_each_file_are_counted_in_one_pass() {
        let hits = vec![
            occurrence("/x/a.rs", 0),
            occurrence("/x/a.rs", 4),
            occurrence("/x/b.rs", 1),
            occurrence("/x/a.rs", 9),
        ];
        let counts = per_file(&hits);
        assert_eq!(counts.get(Path::new("/x/a.rs")), Some(&3));
        assert_eq!(counts.get(Path::new("/x/b.rs")), Some(&1));
        assert_eq!(counts.len(), 2);
    }

    #[test]
    fn tabs_replace_each_other_unless_pinned_or_asked() {
        let mut app = app();
        app.results_add("a".into(), None, vec![occurrence("/x/a.rs", 0)]);
        app.results_add("b".into(), None, vec![occurrence("/x/b.rs", 0)]);
        assert_eq!(app.results.tabs.len(), 1);
        assert_eq!(app.results.tabs[0].title, "b");
        let _ = app.update(Message::Results(ResultsMsg::Pin(0)));
        app.results_add("c".into(), None, vec![]);
        assert_eq!(app.results.tabs.len(), 2);
        let _ = app.update(Message::Results(ResultsMsg::NewTab));
        app.results_add("d".into(), None, vec![]);
        assert_eq!(app.results.tabs.len(), 3);
        assert_eq!(app.results.active, 2);
        let _ = app.update(Message::Results(ResultsMsg::Close(2)));
        let _ = app.update(Message::Results(ResultsMsg::Close(1)));
        let _ = app.update(Message::Results(ResultsMsg::Close(0)));
        assert!(!app.results.open, "no tab left: the panel goes");
    }

    #[test]
    fn keys_exclude_include_remove_and_go_through() {
        let mut app = app();
        app.results_add(
            "r".into(),
            None,
            vec![
                occurrence("/x/a.rs", 0),
                occurrence("/x/a.rs", 4),
                occurrence("/x/b.rs", 1),
            ],
        );
        let key = |app: &mut SimpleEditApp, k: &str, ctrl: bool, alt: bool| {
            let _ = app.update(Message::KeyPressed {
                key: k.into(),
                ctrl,
                shift: false,
                alt,
                captured: false,
            });
        };
        key(&mut app, "Delete", false, false);
        assert!(app.results.tabs[0].hits[0].excluded);
        assert_eq!(app.results.tabs[0].selected, 1, "on to the next");
        key(&mut app, "ArrowUp", false, false);
        key(&mut app, "Insert", false, false);
        assert!(!app.results.tabs[0].hits[0].excluded);
        key(&mut app, "x", true, false);
        assert_eq!(app.results.tabs[0].hits.len(), 2);
        // The one after the removed one is selected; Ctrl+Alt+↓ goes round
        // to the first, opened.
        assert_eq!(app.results.tabs[0].selected, 1);
        key(&mut app, "ArrowDown", true, true);
        assert_eq!(app.results.tabs[0].selected, 0);
        assert_eq!(app.pending_open.as_deref(), Some(Path::new("/x/a.rs")));
        assert!(app.results_text().contains("b.rs:2:1: run"));
    }

    #[test]
    fn a_file_changed_since_the_search_is_not_replaced_and_its_matches_stay() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        std::fs::write(&path, "top\nfoo 1\nfoo 2\n").unwrap();
        let mut app = app();
        let _ = app.update(Message::OpenReplaceInFiles);
        let _ = app.update(Message::FindQuery("foo".into()));
        let _ = app.update(Message::FindReplaceText("bar".into()));
        let search = app.finder_search();
        let found = crate::finder::find_occurrences(
            std::slice::from_ref(&path),
            &HashMap::new(),
            &search.options,
            search.replace.as_ref(),
        )
        .unwrap();
        let hits: Vec<Occurrence> = found
            .into_iter()
            .map(|(hit, replacement)| Occurrence {
                hit,
                replacement,
                excluded: false,
            })
            .collect();
        app.results_add("r".into(), Some(search), hits);
        // The second match is left out, then a line goes above: the one
        // left out is now where the chosen one was.
        app.results.tabs[0].hits[1].excluded = true;
        std::fs::write(&path, "foo 1\nfoo 2\n").unwrap();
        let _ = app.update(Message::Results(ResultsMsg::ReplaceAll));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "foo 1\nfoo 2\n");
        assert_eq!(app.results.tabs[0].hits.len(), 2, "nothing left the list");
        assert!(app.status_is_error);
        // Found again: the chosen one goes, and only it.
        std::fs::write(&path, "top\nfoo 1\nfoo 2\n").unwrap();
        let _ = app.update(Message::Results(ResultsMsg::ReplaceAll));
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "top\nbar 1\nfoo 2\n"
        );
        assert_eq!(app.results.tabs[0].hits.len(), 1);
    }

    /// Draws the window then the panel, to look at them without a screen:
    /// `SNAPSHOT=/tmp/x cargo test finder_look -- --ignored`.
    #[test]
    #[ignore]
    fn finder_look() {
        let Ok(out) = std::env::var("SNAPSHOT") else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("app.rs"),
            "fn main() {\n    // run the app\n    run();\n    let s = \"run\";\n}\n",
        )
        .unwrap();
        let mut app = app();
        let _ = app.update(Message::ProjectPicked(Some(dir.path().to_path_buf())));
        let _ = app.update(Message::OpenReplaceInFiles);
        let files = crate::finder::list_files(dir.path(), &[], true)
            .into_iter()
            .map(|f| (f.clone(), f.display().to_string()))
            .collect();
        let _ = app.update(Message::FinderFiles(files));
        let _ = app.update(Message::FindQuery("run".into()));
        let _ = app.update(Message::FindReplaceText("start".into()));
        let search = app.finder_search();
        app.finder.searched = Some(search.clone());
        let files: Vec<PathBuf> = app.finder.files.iter().map(|(p, _)| p.clone()).collect();
        let found = crate::finder::find_occurrences(
            &files,
            &HashMap::new(),
            &search.options,
            search.replace.as_ref(),
        )
        .map(|f| {
            f.into_iter()
                .map(|(hit, replacement)| Occurrence {
                    hit,
                    replacement,
                    excluded: false,
                })
                .collect()
        });
        let generation = app.finder_generation();
        let _ = app.update(Message::FindResults(generation, found));
        let shoot = |app: &SimpleEditApp, name: &str| {
            let settings = iced::Settings {
                default_font: se_theme::UI,
                fonts: se_theme::font_files(),
                ..Default::default()
            };
            let mut ui = iced_test::Simulator::with_size(settings, (1280.0, 800.0), app.view());
            let shot = ui.snapshot(&se_theme::ink_dark()).unwrap();
            let path = format!("{out}-{name}.png");
            let _ = std::fs::remove_file(format!("{out}-{name}-wgpu.png"));
            assert!(shot.matches_image(&path).unwrap());
        };
        shoot(&app, "window");
        let _ = app.update(Message::FindToWindow);
        let _ = app.update(Message::Results(ResultsMsg::Exclude));
        shoot(&app, "panel");
    }
}
