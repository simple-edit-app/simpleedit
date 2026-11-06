//! Local history in the app: what is kept and when, the window that lists
//! the revisions of a file or a folder, and putting things back.

use super::*;
use crate::local_history::{self as lh, Chunk, LineKind, Row, Store};
use iced::widget::{column, text_input};

#[derive(Debug, Clone, PartialEq)]
pub struct HistoryView {
    /// The file or the folder looked at.
    pub scope: PathBuf,
    pub rows: Vec<Row>,
    /// What the search leaves, as places in `rows`.
    pub shown: Vec<usize>,
    pub search: String,
    pub selected: Option<usize>,
    /// The text of the revision picked and the file now (none: it is gone).
    pub old: String,
    pub current: Option<String>,
    pub chunks: Vec<Chunk>,
}

#[derive(Debug, Clone)]
pub enum HistoryMsg {
    /// The history of the file in front.
    OpenCurrent,
    Open(PathBuf),
    Search(String),
    Searched(String, Vec<usize>),
    Select(usize),
    /// Back to the revision or label picked.
    Revert,
    RevertChunk(usize),
    Patch,
    /// The two files side by side, or the compact list.
    ToggleView,
    Diff(DiffNav),
    Done(Result<String, String>),
    AskLabel,
    LabelText(String),
    LabelSubmit,
}

impl HistoryMsg {
    pub fn is_background(&self) -> bool {
        matches!(
            self,
            HistoryMsg::Searched(..)
                | HistoryMsg::Done(_)
                | HistoryMsg::Diff(DiffNav::Scrolled(..))
        )
    }
}

fn label_field_id() -> iced::widget::Id {
    iced::widget::Id::new("local-history-label")
}

/// Runs away from the interface (at once in tests).
fn in_background(work: impl FnOnce() + Send + 'static) {
    #[cfg(test)]
    work();
    #[cfg(not(test))]
    std::thread::spawn(work);
}

/// Keeps what a file holds on the disk, before it is deleted.
pub(super) fn record_before_delete(path: &Path, days: u32) {
    if days > 0 {
        if let Some(store) = Store::global() {
            let _ = store.record_file(path, lh::now());
        }
    }
}

impl SimpleEditApp {
    fn history_on(&self) -> bool {
        self.config.local_history_days > 0
    }

    /// Keeps a text of a file in the history.
    pub(super) fn history_record(&self, path: &Path, text: &str) {
        if !self.history_on() || is_untitled(path) || is_virtual(path) {
            return;
        }
        let Some(store) = Store::global() else {
            return;
        };
        let (path, text) = (path.to_path_buf(), text.to_string());
        let time = lh::now();
        in_background(move || {
            let _ = store.record(&path, &text, time);
        });
    }

    /// Keeps the text being typed, now and then, saved or not.
    pub(super) fn history_tick(&mut self) {
        if !self.ticks.is_multiple_of(60) || !self.is_dirty {
            return;
        }
        if let Some(path) = self.current_file.clone() {
            let text = crate::editor::buffer_text(&self.editor.content);
            self.history_record(&path, &text);
        }
    }

    /// Forgets what is too old (once, soon after the start).
    pub(super) fn history_prune(&self) {
        let days = self.config.local_history_days;
        if days == 0 {
            return;
        }
        if let Some(store) = Store::global() {
            in_background(move || store.prune(days, lh::MAX_BYTES));
        }
    }

    /// A label on this moment, what is open being kept first.
    pub(super) fn history_label(&self, name: String, auto: bool) {
        if !self.history_on() {
            return;
        }
        let Some(store) = Store::global() else {
            return;
        };
        let mut texts: Vec<(PathBuf, String)> = self
            .file_cache
            .iter()
            .filter(|(p, _)| !is_untitled(p) && !is_virtual(p))
            .filter_map(|(p, b)| b.text().map(|t| (p.clone(), t)))
            .collect();
        if let Some(path) = self
            .current_file
            .as_ref()
            .filter(|p| !is_untitled(p) && !is_virtual(p))
        {
            texts.push((
                path.clone(),
                crate::editor::buffer_text(&self.editor.content),
            ));
        }
        let time = lh::now();
        in_background(move || {
            for (path, text) in texts {
                let _ = store.record(&path, &text, time);
            }
            let _ = store.put_label(&name, auto, time);
        });
    }

    fn history_open(&mut self, scope: PathBuf) -> Task<Message> {
        if !self.history_on() {
            self.set_status(t!("history.disabled"), true);
            return Task::none();
        }
        let Some(store) = Store::global() else {
            return Task::none();
        };
        // What it holds now is in the list before it is read.
        let now = self.file_text(&scope).map(|(text, _)| text);
        if let Some(text) = &now {
            let _ = store.record(&scope, text, lh::now());
        }
        let rows = store.rows(&scope);
        if rows.is_empty() {
            self.set_status(t!("history.empty"), false);
            return Task::none();
        }
        let shown = (0..rows.len()).collect();
        let mut view = HistoryView {
            scope,
            rows,
            shown,
            search: String::new(),
            selected: None,
            old: String::new(),
            current: None,
            chunks: Vec::new(),
        };
        // The latest that differs from the file: there is something to see.
        let first = view
            .rows
            .iter()
            .position(|r| match (r, &now) {
                (Row::Revision { blob, .. }, Some(text)) => store.text(blob).as_ref() != Some(text),
                _ => false,
            })
            .or_else(|| {
                view.rows
                    .iter()
                    .position(|r| matches!(r, Row::Revision { .. }))
            });
        self.history_pick(&mut view, first);
        self.history_diff.side_by_side = true;
        self.history_set(view, 0)
    }

    /// Reads the revision picked, and what differs from the file now.
    fn history_pick(&self, view: &mut HistoryView, row: Option<usize>) {
        view.selected = row;
        view.chunks.clear();
        view.old.clear();
        view.current = None;
        let Some(Row::Revision { path, blob, .. }) = row.and_then(|i| view.rows.get(i)) else {
            return;
        };
        let Some(store) = Store::global() else {
            return;
        };
        view.old = store.text(blob).unwrap_or_default();
        view.current = self.file_text(path).map(|(text, _)| text);
        view.chunks = lh::chunks(&view.old, view.current.as_deref().unwrap_or(""));
    }

    /// The diff of the revision picked, for the two files side by side.
    fn history_show_diff(&mut self, view: &HistoryView) {
        let Some(Row::Revision { path, .. }) = view.selected.and_then(|i| view.rows.get(i)) else {
            self.history_diff = crate::git_panel::Viewer::default();
            self.history_diff.side_by_side = true;
            return;
        };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let current = view.current.as_deref().unwrap_or("");
        let entry = crate::git_panel::Entry {
            source: crate::git::Source::Tree(String::new()),
            change: crate::git::Change {
                path: name.clone(),
                from: None,
                kind: crate::git::Kind::Modified,
            },
            staged: false,
        };
        self.history_diff
            .show(entry, lh::whole_diff(&name, &view.old, current));
    }

    /// The window on `view`, with its diff on the change it was on (the
    /// first one for another revision).
    fn history_set(&mut self, view: HistoryView, change: usize) -> Task<Message> {
        self.history_show_diff(&view);
        self.overlay = ActiveOverlay::LocalHistory(Box::new(view));
        let last = self.history_diff.changes().saturating_sub(1);
        self.history_diff.change = change.min(last);
        self.history_diff.scroll_to_change()
    }

    /// A change of the diff put back: the arrow of its block.
    fn history_revert_block(&mut self, index: usize) -> Task<Message> {
        let Some(mut view) = self.history_view_mut() else {
            return Task::none();
        };
        let Some(Row::Revision { path, .. }) = view.selected.and_then(|i| view.rows.get(i)) else {
            return Task::none();
        };
        let blocks = match self.history_diff.file_diff() {
            Some(diff) => diff.blocks(),
            None => return Task::none(),
        };
        let Some(block) = blocks.get(index) else {
            return Task::none();
        };
        let current = view.current.as_deref().unwrap_or("");
        let new = lh::replace_lines(current, block.at, block.added.len(), &block.removed);
        let path = path.clone();
        if let Err(e) = self.history_put(&path, &new) {
            self.set_status(format!("{}: {e}", t!("status.error")), true);
        }
        let selected = view.selected;
        let change = self.history_diff.change;
        self.history_pick(&mut view, selected);
        self.history_set(view, change)
    }

    fn history_diff_nav(&mut self, nav: DiffNav) -> Task<Message> {
        let viewer = &mut self.history_diff;
        let nowhere = Path::new("");
        match nav {
            DiffNav::NextChange => viewer.next_change(nowhere),
            DiffNav::PrevChange => viewer.prev_change(nowhere),
            DiffNav::Pick(pick) => {
                viewer.pick(pick);
                Task::none()
            }
            DiffNav::Scrolled(top, height) => {
                viewer.view_top.set(top as f32);
                viewer.view_height.set(height as f32);
                Task::none()
            }
            DiffNav::CopyPicked => match viewer.selected_text() {
                Some(text) => iced::clipboard::write(text),
                None => Task::none(),
            },
            DiffNav::PickAll => {
                viewer.select_all();
                Task::none()
            }
            DiffNav::ToggleSide => {
                viewer.side_by_side = !viewer.side_by_side;
                viewer.selection = None;
                viewer.scroll_to_change()
            }
            DiffNav::Apply(block) => self.history_revert_block(block),
            _ => Task::none(),
        }
    }

    fn history_view_mut(&mut self) -> Option<HistoryView> {
        match &self.overlay {
            ActiveOverlay::LocalHistory(view) => Some((**view).clone()),
            _ => None,
        }
    }

    /// A file becomes `new`, wherever it is held; it can be gone.
    fn history_put(&mut self, path: &Path, new: &str) -> Result<(), String> {
        match self.file_text(path) {
            Some((text, holder)) => {
                self.history_record(path, &text);
                if self.put_text(path, &holder, &text, new) {
                    Ok(())
                } else {
                    Err(path.display().to_string())
                }
            }
            None => {
                if let Some(dir) = path.parent() {
                    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
                }
                std::fs::write(path, new).map_err(|e| e.to_string())
            }
        }
    }

    /// Back to the revision picked; on a label, every file of the folder
    /// goes back to what it held then.
    fn history_revert(&mut self) -> Task<Message> {
        let Some(view) = self.history_view_mut() else {
            return Task::none();
        };
        let Some(row) = view.selected.and_then(|i| view.rows.get(i)).cloned() else {
            return Task::none();
        };
        let Some(store) = Store::global() else {
            return Task::none();
        };
        let targets: Vec<(PathBuf, String)> = match row {
            Row::Revision { path, blob, .. } => {
                store.text(&blob).map(|t| (path, t)).into_iter().collect()
            }
            Row::Label(label) => store.state_at(&view.scope, label.time),
        };
        self.history_label(t!("history.auto_revert").to_string(), true);
        let mut done = 0;
        let mut failed = None;
        for (path, text) in targets {
            match self.history_put(&path, &text) {
                Ok(()) => done += 1,
                Err(e) => failed = Some(e),
            }
        }
        self.overlay = ActiveOverlay::None;
        match failed {
            Some(e) => self.set_status(format!("{}: {e}", t!("status.error")), true),
            None => {
                let n = done.to_string();
                self.set_status(t!("history.reverted", n = n.as_str()), false);
            }
        }
        Task::none()
    }

    fn history_revert_chunk(&mut self, index: usize) -> Task<Message> {
        let Some(view) = self.history_view_mut() else {
            return Task::none();
        };
        let Some(Row::Revision { path, .. }) = view.selected.and_then(|i| view.rows.get(i)) else {
            return Task::none();
        };
        let (Some(chunk), Some(current)) = (view.chunks.get(index), view.current.as_deref()) else {
            return Task::none();
        };
        let new = lh::revert_chunk(&view.old, current, chunk);
        let path = path.clone();
        if let Err(e) = self.history_put(&path, &new) {
            self.set_status(format!("{}: {e}", t!("status.error")), true);
        }
        let selected = view.selected;
        let change = self.history_diff.change;
        let mut view = view;
        self.history_pick(&mut view, selected);
        self.history_set(view, change)
    }

    fn history_patch(&mut self) -> Task<Message> {
        let Some(view) = self.history_view_mut() else {
            return Task::none();
        };
        let Some(Row::Revision { path, .. }) = view.selected.and_then(|i| view.rows.get(i)) else {
            return Task::none();
        };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let patch = lh::patch(&name, &view.old, view.current.as_deref().unwrap_or(""));
        Task::perform(
            async move {
                let Some(file) = rfd::AsyncFileDialog::new()
                    .set_file_name(format!("{name}.patch"))
                    .save_file()
                    .await
                else {
                    return Ok(String::new());
                };
                tokio::fs::write(file.path(), patch)
                    .await
                    .map_err(|e| e.to_string())?;
                Ok(t!("git.patch_saved").to_string())
            },
            |result| Message::History(HistoryMsg::Done(result)),
        )
    }

    /// The revisions whose text holds what is looked for.
    fn history_search(&mut self, query: String) -> Task<Message> {
        let ActiveOverlay::LocalHistory(view) = &mut self.overlay else {
            return Task::none();
        };
        view.search = query.clone();
        let rows = view.rows.clone();
        if query.is_empty() {
            view.shown = (0..rows.len()).collect();
            return Task::none();
        }
        Task::perform(
            async move {
                let asked = query.clone();
                let found = tokio::task::spawn_blocking(move || {
                    let Some(store) = Store::global() else {
                        return Vec::new();
                    };
                    let wanted = query.to_lowercase();
                    rows.iter()
                        .enumerate()
                        .filter(|(_, row)| match row {
                            Row::Revision { blob, .. } => store
                                .text(blob)
                                .is_some_and(|t| t.to_lowercase().contains(&wanted)),
                            Row::Label(_) => false,
                        })
                        .map(|(i, _)| i)
                        .collect()
                })
                .await
                .unwrap_or_default();
                (asked, found)
            },
            |(asked, found)| Message::History(HistoryMsg::Searched(asked, found)),
        )
    }

    pub(super) fn history_update(&mut self, msg: HistoryMsg) -> Task<Message> {
        match msg {
            HistoryMsg::OpenCurrent => match self.current_file.clone() {
                Some(path) if !is_untitled(&path) && !is_virtual(&path) => self.history_open(path),
                _ => Task::none(),
            },
            HistoryMsg::Open(path) => self.history_open(path),
            HistoryMsg::Search(query) => self.history_search(query),
            HistoryMsg::Searched(asked, found) => {
                if let ActiveOverlay::LocalHistory(view) = &mut self.overlay {
                    if view.search == asked {
                        view.shown = found;
                    }
                }
                Task::none()
            }
            HistoryMsg::Select(row) => {
                let Some(mut view) = self.history_view_mut() else {
                    return Task::none();
                };
                self.history_pick(&mut view, Some(row));
                self.history_set(view, 0)
            }
            HistoryMsg::Revert => self.history_revert(),
            HistoryMsg::RevertChunk(index) => self.history_revert_chunk(index),
            HistoryMsg::Patch => self.history_patch(),
            HistoryMsg::ToggleView => {
                self.history_compact = !self.history_compact;
                Task::none()
            }
            HistoryMsg::Diff(nav) => self.history_diff_nav(nav),
            HistoryMsg::Done(result) => {
                match result {
                    Ok(said) if said.is_empty() => {}
                    Ok(said) => self.set_status(said, false),
                    Err(e) => self.set_status(format!("{}: {e}", t!("status.error")), true),
                }
                Task::none()
            }
            HistoryMsg::AskLabel => {
                self.overlay = ActiveOverlay::HistoryLabel(String::new());
                iced::widget::operation::focus(label_field_id())
            }
            HistoryMsg::LabelText(name) => {
                self.overlay = ActiveOverlay::HistoryLabel(name);
                Task::none()
            }
            HistoryMsg::LabelSubmit => {
                if let ActiveOverlay::HistoryLabel(name) = &self.overlay {
                    let name = name.trim().to_string();
                    if name.is_empty() {
                        return Task::none();
                    }
                    self.history_label(name, false);
                    self.set_status(t!("history.label_put"), false);
                }
                self.overlay = ActiveOverlay::None;
                Task::none()
            }
        }
    }

    /// A path as the window shows it: the file first, as what follows may
    /// be cut, then its folder from the project (else from home).
    fn history_title(&self, path: &Path) -> String {
        let root = crate::app::tab_groups::project_of(&self.projects.roots, path);
        let folder = |dir: &Path| match root.and_then(|r| Some((r, dir.strip_prefix(r).ok()?))) {
            Some((r, rel)) => {
                let name = r.file_name().unwrap_or_default().to_string_lossy();
                if rel.as_os_str().is_empty() {
                    name.into_owned()
                } else {
                    format!("{name}/{}", rel.display())
                }
            }
            None => display_path(dir),
        };
        let folder_itself = path.is_dir() || self.projects.roots.iter().any(|r| r == path);
        match (folder_itself, path.file_name(), path.parent()) {
            (false, Some(file), Some(dir)) => {
                format!("{} — {}", file.to_string_lossy(), folder(dir))
            }
            _ => folder(path),
        }
    }

    pub(super) fn view_history_label<'a>(
        &'a self,
        name: &'a str,
        dark: bool,
    ) -> Element<'a, Message> {
        let p = se_theme::palette(dark);
        container(
            column![
                text(t!("history.put_label").to_string())
                    .size(14)
                    .font(se_theme::UI_SEMIBOLD)
                    .color(p.text),
                text_input(&t!("history.label_placeholder"), name)
                    .id(label_field_id())
                    .on_input(|t| Message::History(HistoryMsg::LabelText(t)))
                    .on_submit(Message::History(HistoryMsg::LabelSubmit))
                    .padding([7, 10])
                    .size(13)
                    .style(se_theme::Field::new(dark).style()),
            ]
            .spacing(10)
            .padding(16),
        )
        .width(380)
        .style(se_theme::card(dark))
        .into()
    }

    pub(super) fn view_local_history<'a>(
        &'a self,
        view: &'a HistoryView,
        dark: bool,
    ) -> Element<'a, Message> {
        let p = se_theme::palette(dark);
        let ghost = move |active: bool| (se_theme::GhostButton { dark, active }).style();
        let several = view.scope.is_dir() || !view.scope.exists();
        let mut list = column![].spacing(1);
        for index in &view.shown {
            let Some(row) = view.rows.get(*index) else {
                continue;
            };
            let time = crate::git_panel::stash::format_time(row.time());
            let selected = view.selected == Some(*index);
            let line: Element<Message> = match row {
                Row::Revision { path, .. } => {
                    let mut line = column![text(time).size(12.5).color(p.text)];
                    if several {
                        let shown = path
                            .strip_prefix(&view.scope)
                            .unwrap_or(path)
                            .display()
                            .to_string();
                        let gone = !path.exists();
                        line = line.push(
                            text(if gone {
                                t!("history.gone", file = shown.as_str()).to_string()
                            } else {
                                shown
                            })
                            .size(11.5)
                            .color(p.muted)
                            .wrapping(iced::widget::text::Wrapping::None),
                        );
                    }
                    line.into()
                }
                Row::Label(label) => column![
                    text(label.name.as_str())
                        .size(12.5)
                        .font(se_theme::UI_SEMIBOLD)
                        .color(p.accent),
                    text(time).size(11.5).color(p.muted),
                ]
                .into(),
            };
            list = list.push(
                button(line)
                    .padding([4, 8])
                    .width(Length::Fill)
                    .on_press(Message::History(HistoryMsg::Select(*index)))
                    .style(ghost(selected)),
            );
        }
        let left = column![
            text_input(&t!("history.search"), &view.search)
                .on_input(|t| Message::History(HistoryMsg::Search(t)))
                .size(12.5)
                .padding([5, 8])
                .style(se_theme::Field::new(dark).style()),
            scrollable(list)
                .style(se_theme::Scrollbar { dark }.style())
                .height(Length::Fill),
        ]
        .spacing(6)
        .padding(8)
        .width(Length::Fixed(280.0));

        let picked = view.selected.and_then(|i| view.rows.get(i));
        let is_revision = matches!(picked, Some(Row::Revision { .. }));
        let mut chunks = column![].spacing(10);
        for (i, chunk) in view.chunks.iter().enumerate() {
            let mut lines = column![].spacing(0);
            for (kind, line) in &chunk.lines {
                let (mark, color) = match kind {
                    LineKind::Same => (" ", p.muted),
                    LineKind::Old => ("-", iced::Color::from_rgb8(0xe5, 0x53, 0x4b)),
                    LineKind::New => ("+", iced::Color::from_rgb8(0x3f, 0xb5, 0x6b)),
                };
                lines = lines.push(
                    text(format!("{mark} {line}"))
                        .size(12)
                        .font(se_theme::MONO)
                        .color(color)
                        .wrapping(iced::widget::text::Wrapping::None),
                );
            }
            chunks = chunks.push(column![
                button(text(t!("history.revert_chunk").to_string()).size(11.5))
                    .padding([2, 8])
                    .on_press(Message::History(HistoryMsg::RevertChunk(i)))
                    .style(ghost(false)),
                lines,
            ]);
        }
        if view.chunks.is_empty() && is_revision {
            chunks = chunks.push(
                text(t!("history.same").to_string())
                    .size(12.5)
                    .color(p.muted),
            );
        }
        if let Some(Row::Label(_)) = picked {
            chunks = chunks.push(
                text(t!("history.label_hint").to_string())
                    .size(12.5)
                    .color(p.muted),
            );
        }
        let view_label = if self.history_compact {
            t!("history.view_files").to_string()
        } else {
            t!("history.view_compact").to_string()
        };
        let toolbar = row![
            // From the project, and cut before the buttons.
            container(
                text(self.history_title(&view.scope))
                    .size(12.5)
                    .color(p.muted)
                    .wrapping(iced::widget::text::Wrapping::None),
            )
            .width(Length::Fill)
            .clip(true),
            button(text(view_label).size(12.5))
                .padding([4, 10])
                .on_press(Message::History(HistoryMsg::ToggleView))
                .style(ghost(false)),
            button(text(t!("history.create_patch").to_string()).size(12.5))
                .padding([4, 10])
                .on_press_maybe(is_revision.then_some(Message::History(HistoryMsg::Patch)))
                .style(ghost(false)),
            button(text(t!("history.revert").to_string()).size(12.5))
                .padding([4, 12])
                .on_press_maybe(
                    picked
                        .is_some()
                        .then_some(Message::History(HistoryMsg::Revert))
                )
                .style((se_theme::Primary { dark }).style()),
            iced::widget::tooltip(
                button(text("×").size(16).color(p.muted))
                    .padding([1, 8])
                    .on_press(Message::CloseOverlay)
                    .style(ghost(false)),
                container(text(t!("history.close").to_string()).size(12))
                    .padding([4, 8])
                    .style(se_theme::tooltip(dark)),
                iced::widget::tooltip::Position::Bottom,
            ),
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        let body: Element<Message> = if self.history_compact || !is_revision {
            scrollable(container(chunks).padding([4, 4]))
                .direction(iced::widget::scrollable::Direction::Both {
                    vertical: Default::default(),
                    horizontal: Default::default(),
                })
                .style(se_theme::Scrollbar { dark }.style())
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else {
            crate::git_panel::diff_view::view(
                &self.history_diff,
                &crate::git_panel::diff_view::Nav::history(),
                dark,
                String::new(),
            )
        };
        let right = column![container(toolbar).padding(8), body].width(Length::Fill);
        container(row![
            left,
            container(Space::new().width(1).height(Length::Fill)).style(se_theme::rule(dark)),
            right,
        ])
        .width(1180)
        .height(640)
        .style(se_theme::palette_card(dark))
        .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An app with a project of one file, and its history reachable.
    fn app_on(dir: &Path) -> SimpleEditApp {
        let mut app = SimpleEditApp::blank(Config::default());
        let _ = app.restore_session(crate::session::Session::default());
        let _ = app.update(Message::ProjectPicked(Some(dir.to_path_buf())));
        app
    }

    fn view(app: &SimpleEditApp) -> &HistoryView {
        match &app.overlay {
            ActiveOverlay::LocalHistory(view) => view,
            other => panic!("the window, not {other:?}"),
        }
    }

    fn revision_rows(app: &SimpleEditApp) -> Vec<usize> {
        let rows = &view(app).rows;
        (0..rows.len())
            .filter(|i| matches!(rows[*i], Row::Revision { .. }))
            .collect()
    }

    #[test]
    fn an_older_revision_comes_back_on_a_file_that_is_not_open() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        std::fs::write(&path, "new\n").unwrap();
        let store = Store::global().unwrap();
        store.record(&path, "old\n", 100);
        let mut app = app_on(dir.path());
        let _ = app.update(Message::History(HistoryMsg::Open(path.clone())));
        let rows = revision_rows(&app);
        assert_eq!(rows.len(), 2, "what the file holds now is kept too");
        let _ = app.update(Message::History(HistoryMsg::Select(rows[1])));
        assert_eq!(view(&app).old, "old\n");
        assert_eq!(view(&app).chunks.len(), 1);
        let _ = app.update(Message::History(HistoryMsg::Revert));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "old\n");
        assert_eq!(app.overlay, ActiveOverlay::None);
    }

    #[test]
    fn a_deleted_file_comes_back_from_its_folder() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub/gone.txt");
        let store = Store::global().unwrap();
        store.record(&path, "was here\n", 100);
        let mut app = app_on(dir.path());
        let _ = app.update(Message::History(HistoryMsg::Open(dir.path().to_path_buf())));
        assert_eq!(view(&app).current, None);
        let _ = app.update(Message::History(HistoryMsg::Revert));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "was here\n");
    }

    #[test]
    fn a_change_is_put_back_alone_and_the_window_stays() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        let old: String = (0..30).map(|i| format!("line {i}\n")).collect();
        let now = old
            .replace("line 2\n", "two\n")
            .replace("line 25\n", "many\n");
        std::fs::write(&path, &now).unwrap();
        Store::global().unwrap().record(&path, &old, 100);
        let mut app = app_on(dir.path());
        let _ = app.update(Message::History(HistoryMsg::Open(path.clone())));
        let older = revision_rows(&app)[1];
        let _ = app.update(Message::History(HistoryMsg::Select(older)));
        assert_eq!(view(&app).chunks.len(), 2);
        let _ = app.update(Message::History(HistoryMsg::RevertChunk(0)));
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("line 2\n") && text.contains("many"));
        assert_eq!(view(&app).chunks.len(), 1, "the other one is left");
    }

    #[test]
    fn a_lone_change_at_the_end_of_a_file_is_reached_by_opening_and_by_the_arrows() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        let old: String = (0..200).map(|i| format!("line {i}\n")).collect();
        std::fs::write(&path, old.replace("line 199\n", "changed\n")).unwrap();
        Store::global().unwrap().record(&path, &old, 100);
        let mut app = app_on(dir.path());
        let open = app.update(Message::History(HistoryMsg::Open(path)));
        // The revision that differs is picked, and shown on its change.
        assert_eq!(view(&app).selected, Some(revision_rows(&app)[1]));
        assert_eq!(app.history_diff.changes(), 1);
        assert!(open.units() > 0, "opening goes to the change");
        assert!(app.history_diff.change_row().is_some_and(|row| row > 190));
        // One change: both arrows still bring it into view.
        let down = app.update(Message::History(HistoryMsg::Diff(DiffNav::NextChange)));
        assert!(down.units() > 0, "the arrow scrolls");
        let up = app.update(Message::History(HistoryMsg::Diff(DiffNav::PrevChange)));
        assert!(up.units() > 0);
        assert_eq!(app.history_diff.change, 0);
    }

    #[test]
    fn the_two_files_come_first_and_their_arrows_put_a_change_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        let old: String = (0..30).map(|i| format!("line {i}\n")).collect();
        let now = old
            .replace("line 2\n", "two\n")
            .replace("line 25\n", "many\n");
        std::fs::write(&path, &now).unwrap();
        Store::global().unwrap().record(&path, &old, 100);
        let mut app = app_on(dir.path());
        let _ = app.update(Message::History(HistoryMsg::Open(path.clone())));
        let older = revision_rows(&app)[1];
        let _ = app.update(Message::History(HistoryMsg::Select(older)));
        assert!(app.history_diff.side_by_side, "two files by default");
        assert!(!app.history_compact);
        assert_eq!(app.history_diff.changes(), 2);
        // The second block goes back; the first stays.
        let _ = app.update(Message::History(HistoryMsg::Diff(DiffNav::Apply(1))));
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("two\n") && text.contains("line 25\n"));
        assert_eq!(app.history_diff.changes(), 1, "the diff follows");
        // The view can be switched.
        let _ = app.update(Message::History(HistoryMsg::ToggleView));
        assert!(app.history_compact);
    }

    #[test]
    fn a_label_is_put_and_taking_it_back_restores_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        let store = Store::global().unwrap();
        store.record(&path, "before\n", 100);
        store.put_label("safe point", false, 150);
        store.record(&path, "after\n", 200);
        std::fs::write(&path, "after\n").unwrap();
        let mut app = app_on(dir.path());
        let _ = app.update(Message::History(HistoryMsg::Open(dir.path().to_path_buf())));
        let label = view(&app)
            .rows
            .iter()
            .position(|r| matches!(r, Row::Label(l) if l.name == "safe point"))
            .unwrap();
        let _ = app.update(Message::History(HistoryMsg::Select(label)));
        let _ = app.update(Message::History(HistoryMsg::Revert));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "before\n");
    }

    #[test]
    fn a_label_is_named_by_hand() {
        let mut app = app_on(tempfile::tempdir().unwrap().path());
        let _ = app.update(Message::History(HistoryMsg::AskLabel));
        let _ = app.update(Message::History(HistoryMsg::LabelSubmit));
        assert_eq!(app.overlay, ActiveOverlay::HistoryLabel(String::new()));
        let unique = format!("before the refactoring {}", lh::now());
        let _ = app.update(Message::History(HistoryMsg::LabelText(unique.clone())));
        let _ = app.update(Message::History(HistoryMsg::LabelSubmit));
        assert_eq!(app.overlay, ActiveOverlay::None);
        let labels = Store::global().unwrap().labels();
        assert!(labels.iter().any(|l| l.name == unique && !l.auto));
    }

    #[test]
    fn nothing_is_kept_when_turned_off() {
        let mut app = app_on(tempfile::tempdir().unwrap().path());
        let root = app.projects.roots[0].clone();
        let name = root.file_name().unwrap().to_string_lossy().into_owned();
        let title = app.history_title(&root.join("src/a.rs"));
        assert_eq!(title, format!("a.rs — {name}/src"), "the file first");
        assert_eq!(app.history_title(&root), name, "a project alone");
        app.config.local_history_days = 0;
        let path = PathBuf::from(format!("/nowhere/off-{}.txt", lh::now()));
        app.history_record(&path, "text");
        assert!(Store::global().unwrap().file(&path).revisions.is_empty());
        let _ = app.update(Message::History(HistoryMsg::Open(path)));
        assert_eq!(app.overlay, ActiveOverlay::None);
    }

    /// Draws the window, to look at it without a screen:
    /// `SNAPSHOT=/tmp/x cargo test history_look -- --ignored`.
    #[test]
    #[ignore]
    fn history_look() {
        let Ok(out) = std::env::var("SNAPSHOT") else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let deep = dir
            .path()
            .join("packages/a-rather-long-name/src/modules/deep/down");
        std::fs::create_dir_all(&deep).unwrap();
        let path = deep.join("a-file-with-a-long-name-to-see-how-it-is-cut.rs");
        let store = Store::global().unwrap();
        let t = lh::now() - 7200;
        store.record(&path, "fn main() {\n    old();\n}\n", t);
        store.put_label("Before the refactoring", false, t + 600);
        store.record(&path, "fn main() {\n    middle();\n}\n", t + 1200);
        std::fs::write(&path, "fn main() {\n    new();\n    more();\n}\n").unwrap();
        let mut app = app_on(dir.path());
        let _ = app.update(Message::History(HistoryMsg::Open(path)));
        let older = revision_rows(&app)[2];
        let _ = app.update(Message::History(HistoryMsg::Select(older)));
        let _ = app.update(Message::History(HistoryMsg::Diff(DiffNav::Scrolled(
            0, 500,
        ))));
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

    #[test]
    fn what_was_saved_is_in_the_history() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        let mut app = app_on(dir.path());
        let _ = app.update(Message::FileSaved(
            Some(path.clone()),
            Ok((path.clone(), "saved\n".to_string())),
        ));
        let store = Store::global().unwrap();
        let revisions = store.file(&path).revisions;
        assert_eq!(store.text(&revisions[0].blob).as_deref(), Some("saved\n"));
    }
}
