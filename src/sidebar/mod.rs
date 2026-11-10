use iced::{
    widget::{button, column, container, row, scrollable, text, Space},
    Alignment, Element, Length,
};
use std::collections::HashSet;
use std::path::PathBuf;

use crate::app::Message;
use crate::theme;

pub mod icons;

#[derive(Debug, Clone)]
pub enum SidebarMessage {
    OpenFile(PathBuf),
    RemoveFile(PathBuf),
    /// The cross beside a file: closes it at once, unsaved changes included.
    CloseNow(PathBuf),
    RequestSave,
    RequestSaveAs,
}

/// What the app should do in response to a sidebar interaction.
pub enum SidebarAction {
    None,
    OpenFile(PathBuf),
    CloseFile(PathBuf),
    CloseNow(PathBuf),
    SaveFile,
    SaveFileAs,
}

/// Height of a line of the tree of projects.
pub const TREE_ROW: f32 = 26.0;

/// The id of the tree's scrollable, to bring a file into view.
pub fn tree_id() -> iced::widget::Id {
    iced::widget::Id::new("project-tree")
}

/// Where to scroll the tree to show the line `index` with a few above it.
pub fn tree_offset(index: usize) -> f32 {
    ((index as f32 - 3.0) * (TREE_ROW + 1.0)).max(0.0)
}

/// The color of a file's name for how it stands in Git (none: as usual).
pub fn state_color(state: Option<crate::git::FileState>, dark: bool) -> Option<iced::Color> {
    use crate::git::FileState;
    let p = theme::palette(dark);
    Some(match state? {
        FileState::Modified => iced::Color::from_rgb8(0x5c, 0x9c, 0xe6),
        FileState::Added | FileState::Untracked => iced::Color::from_rgb8(0x6a, 0xb8, 0x4f),
        FileState::Conflicted => theme::error_color(dark),
        FileState::Ignored => iced::Color { a: 0.55, ..p.muted },
    })
}

/// A small red dot: the lint found errors there.
pub fn error_dot<'a>(dark: bool) -> Element<'a, Message> {
    container(Space::new().width(6).height(6))
        .style(theme::dot(theme::error_color(dark)))
        .into()
}

/// A panel of the sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    Files,
    Projects,
    Database,
    Api,
}

/// Which panels of the sidebar are open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Panels {
    pub files: bool,
    pub projects: bool,
    pub database: bool,
    pub api: bool,
}

pub struct SidebarState {
    pub files: Vec<PathBuf>,
}

impl SidebarState {
    pub fn new() -> Self {
        Self { files: Vec::new() }
    }

    pub fn files(&self) -> &[PathBuf] {
        &self.files
    }

    pub fn add_file(&mut self, path: PathBuf) {
        if !self.files.contains(&path) {
            self.files.push(path);
        }
    }

    /// Replace an untitled slot with its real path after Save As.
    pub fn rename_file(&mut self, old: &PathBuf, new: PathBuf) {
        if old == &new {
            return;
        }
        if let Some(pos) = self.files.iter().position(|p| p == old) {
            // Saving over a file that is already open must not list it twice.
            if self.files.contains(&new) {
                self.files.remove(pos);
            } else {
                self.files[pos] = new;
            }
        }
    }

    /// Moves the tab at `from` to the place `to` (the ones between shift).
    pub fn move_tab(&mut self, from: usize, to: usize) -> bool {
        if from == to || from >= self.files.len() || to >= self.files.len() {
            return false;
        }
        let file = self.files.remove(from);
        self.files.insert(to, file);
        true
    }

    /// The open files in a new order.
    pub fn set_order(&mut self, files: Vec<PathBuf>) {
        self.files = files;
    }

    pub fn remove_path(&mut self, path: &std::path::Path) {
        self.files.retain(|p| p != path);
    }

    pub fn update(&mut self, msg: SidebarMessage) -> SidebarAction {
        match msg {
            SidebarMessage::OpenFile(path) => SidebarAction::OpenFile(path),
            // The app removes the tab itself (`remove_path`) once any
            // unsaved-changes confirmation has been answered. The tab is
            // named by path: the list may have changed since it was drawn.
            SidebarMessage::RemoveFile(path) => {
                if self.files.contains(&path) {
                    SidebarAction::CloseFile(path)
                } else {
                    SidebarAction::None
                }
            }
            SidebarMessage::CloseNow(path) => {
                if self.files.contains(&path) {
                    SidebarAction::CloseNow(path)
                } else {
                    SidebarAction::None
                }
            }
            SidebarMessage::RequestSave => SidebarAction::SaveFile,
            SidebarMessage::RequestSaveAs => SidebarAction::SaveFileAs,
        }
    }

    /// The file list. `dirty` holds the open files with unsaved changes.
    #[allow(clippy::too_many_arguments)]
    pub fn view<'a>(
        &'a self,
        dark: bool,
        current_file: &Option<PathBuf>,
        dirty: &HashSet<PathBuf>,
        marked: &HashSet<PathBuf>,
        projects: &'a crate::projects::Projects,
        colors: &std::collections::BTreeMap<String, usize>,
        marks: &crate::projects::Marks,
        panels: Panels,
        width: f32,
    ) -> Element<'a, Message> {
        let lbl_close = t!("sidebar.close").to_string();
        let lbl_close_others = t!("menu.file_close_others").to_string();
        let lbl_close_all = t!("menu.file_close_all").to_string();
        let lbl_save = t!("sidebar.save").to_string();
        let lbl_save_as = t!("sidebar.save_as").to_string();
        let lbl_git = t!("sidebar.git").to_string();
        let lbl_delete = t!("sidebar.delete").to_string();
        let lbl_lint_fix = t!("menu.lint_fix").to_string();
        let lbl_show = t!("sidebar.show_in_tree").to_string();
        let lbl_history = t!("history.show").to_string();
        let lbl_split_right = t!("sidebar.split_right").to_string();
        let lbl_split_down = t!("sidebar.split_down").to_string();
        let lbl_bookmark_add = t!("sidebar.bookmark_add").to_string();
        let lbl_bookmark_remove = t!("sidebar.bookmark_remove").to_string();
        let p = theme::palette(dark);

        let labels = crate::app::unique_labels(&self.files);
        let items: Vec<Element<Message>> = self
            .files
            .iter()
            .zip(labels)
            .map(|(path, name)| {
                let is_active = current_file.as_ref() == Some(path);
                let is_selected = is_active;
                let is_dirty = dirty.contains(path);
                let path_clone = path.clone();
                let close_path = path.clone();
                let close_now = path.clone();

                let ext_color = if is_selected { p.accent } else { p.muted };
                let marker: Element<Message> = if is_dirty {
                    container(Space::new().width(6).height(6))
                        .style(theme::dot(p.accent))
                        .into()
                } else {
                    Space::new().width(6).height(6).into()
                };

                let row_button = button(
                    row![
                        crate::file_icons::file(path, dark),
                        text(name)
                            .size(13)
                            .shaping(iced::widget::text::Shaping::Advanced)
                            .width(Length::Fill),
                        marker,
                        button(text("×").size(14).color(ext_color))
                            .padding([0, 5])
                            .on_press(Message::Sidebar(SidebarMessage::CloseNow(
                                close_now.clone()
                            )))
                            .style(
                                (theme::GhostButton {
                                    dark,
                                    active: false
                                })
                                .style()
                            ),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                )
                .on_press(Message::Sidebar(SidebarMessage::OpenFile(path_clone)))
                .width(Length::Fill)
                .padding([7, 8])
                .style(
                    (theme::SidebarRow {
                        dark,
                        selected: is_selected,
                    })
                    .style(),
                );

                // Build context menu labels (owned Strings for 'static closure)
                let lbl_close = lbl_close.clone();
                let lbl_close_others = lbl_close_others.clone();
                let lbl_close_all = lbl_close_all.clone();
                let lbl_save = lbl_save.clone();
                let lbl_save_as = lbl_save_as.clone();
                let lbl_bookmark = if marked.contains(path) {
                    lbl_bookmark_remove.clone()
                } else {
                    lbl_bookmark_add.clone()
                };
                let bookmark_path = path.clone();
                let lbl_git = lbl_git.clone();
                let lbl_delete = lbl_delete.clone();
                let lbl_lint_fix = lbl_lint_fix.clone();
                let lbl_show = lbl_show.clone();
                let lbl_history = lbl_history.clone();
                let in_project = projects.roots.iter().any(|r| path.starts_with(r));
                let lbl_split_right = lbl_split_right.clone();
                let lbl_split_down = lbl_split_down.clone();
                let full_path = path.clone();

                let row_with_menu = crate::pointer_area::context_menu(row_button, move || {
                    let ctx_item =
                        |label: String, msg: Option<Message>| -> Element<'static, Message> {
                            let mut btn = button(text(label).size(13))
                                .padding([7, 10])
                                .width(Length::Fixed(176.0))
                                .style(
                                    (theme::MenuItem {
                                        dark,
                                        radius: theme::R_SM,
                                        highlighted: false,
                                    })
                                    .style(),
                                );
                            if let Some(m) = msg {
                                btn = btn.on_press(m);
                            }
                            btn.into()
                        };

                    container(
                        column![
                            ctx_item(
                                lbl_close.clone(),
                                Some(Message::Sidebar(SidebarMessage::RemoveFile(
                                    close_path.clone(),
                                ))),
                            ),
                            ctx_item(
                                lbl_close_others.clone(),
                                Some(Message::CloseTabs(Some(close_path.clone()))),
                            ),
                            ctx_item(lbl_close_all.clone(), Some(Message::CloseTabs(None))),
                            ctx_item(
                                lbl_save.clone(),
                                is_active.then_some(Message::Sidebar(SidebarMessage::RequestSave)),
                            ),
                            ctx_item(
                                lbl_save_as.clone(),
                                is_active
                                    .then_some(Message::Sidebar(SidebarMessage::RequestSaveAs)),
                            ),
                            ctx_item(
                                lbl_show.clone(),
                                in_project.then(|| Message::ShowInProjects(bookmark_path.clone())),
                            ),
                            ctx_item(
                                lbl_history.clone(),
                                Some(Message::History(crate::app::HistoryMsg::Open(
                                    bookmark_path.clone()
                                ))),
                            ),
                            ctx_item(
                                lbl_lint_fix.clone(),
                                crate::app::splittable(&bookmark_path)
                                    .then(|| Message::LintFixFile(bookmark_path.clone())),
                            ),
                            ctx_item(
                                lbl_split_right.clone(),
                                crate::app::splittable(&bookmark_path)
                                    .then(|| Message::SplitWith(bookmark_path.clone(), true)),
                            ),
                            ctx_item(
                                lbl_split_down.clone(),
                                crate::app::splittable(&bookmark_path)
                                    .then(|| Message::SplitWith(bookmark_path.clone(), false)),
                            ),
                            ctx_item(
                                lbl_git.clone(),
                                crate::app::git_menu_applies(&bookmark_path)
                                    .then(|| Message::GitFileMenu(bookmark_path.clone())),
                            ),
                            ctx_item(
                                lbl_bookmark.clone(),
                                (!crate::app::is_untitled(&bookmark_path))
                                    .then(|| Message::ToggleFileBookmark(bookmark_path.clone())),
                            ),
                            ctx_item(
                                lbl_delete.clone(),
                                (!crate::app::is_untitled(&bookmark_path))
                                    .then(|| Message::AskDeleteFile(bookmark_path.clone())),
                            ),
                        ]
                        .spacing(1)
                        .padding(6),
                    )
                    .style(theme::card(dark))
                    .into()
                });
                // The whole path, as the IDEs show it under the pointer.
                if crate::app::is_untitled(&full_path) {
                    row_with_menu
                } else {
                    iced::widget::tooltip(
                        row_with_menu,
                        container(
                            text(crate::app::display_path(&full_path))
                                .size(12)
                                .shaping(iced::widget::text::Shaping::Advanced),
                        )
                        .padding([4, 8])
                        .style(theme::tooltip(dark)),
                        iced::widget::tooltip::Position::Bottom,
                    )
                    .gap(4)
                    .delay(std::time::Duration::from_millis(500))
                    .into()
                }
            })
            .collect();

        let file_list: Element<Message> = if items.is_empty() {
            text(t!("sidebar.no_files"))
                .size(12)
                .color(theme::muted_text(dark))
                .into()
        } else {
            column(items).spacing(2).into()
        };

        let count = self.files.len();
        let footer = if count == 1 {
            t!("sidebar.open_one").to_string()
        } else {
            t!("sidebar.open_many", n = count).to_string()
        };

        let has_projects = !projects.roots.is_empty();
        let both = panels.files && panels.projects;
        let muted = theme::muted_text(dark);
        // Its height is fixed to the sidebar's: in a column that shrinks, the
        // lists would take what they need one after the other and the last
        // would be left with nothing.
        let mut content = column![].spacing(10).height(Length::Fill);
        if panels.projects {
            content = content.push(
                row![
                    text(t!("sidebar.projects").to_uppercase())
                        .size(10.5)
                        .color(muted)
                        .font(theme::UI_MEDIUM),
                    Space::new().width(Length::Fill),
                    iced::widget::tooltip(
                        button(text("−").size(15).color(muted))
                            .padding([0, 7])
                            .on_press(Message::CollapseProjects)
                            .style(
                                (theme::GhostButton {
                                    dark,
                                    active: false
                                })
                                .style()
                            ),
                        container(text(t!("sidebar.collapse_all").to_string()).size(12))
                            .padding([4, 8])
                            .style(theme::tooltip(dark)),
                        iced::widget::tooltip::Position::Bottom,
                    ),
                    button(text("+").size(15).color(muted))
                        .padding([0, 7])
                        .on_press(Message::AddProject)
                        .style(
                            (theme::GhostButton {
                                dark,
                                active: false
                            })
                            .style()
                        ),
                ]
                .padding([0, 8])
                .align_y(Alignment::Center),
            );
            content = content.push(if has_projects {
                scrollable(project_tree(
                    projects,
                    colors,
                    marks,
                    current_file,
                    marked,
                    dark,
                ))
                .id(tree_id())
                .on_scroll(|v| Message::TreeScrolled(v.absolute_offset().y, v.bounds().height))
                .style(theme::Scrollbar { dark }.style())
                .height(if both {
                    Length::FillPortion(5)
                } else {
                    Length::Fill
                })
                .into()
            } else {
                Element::from(
                    button(text(t!("sidebar.add_project").to_string()).size(12.5))
                        .padding([6, 12])
                        .width(Length::Fill)
                        .on_press(Message::AddProject)
                        .style((theme::Outline { dark }).style()),
                )
            });
        }
        if panels.files {
            content = content
                .push(
                    container(
                        text(t!("sidebar.files").to_uppercase())
                            .size(10.5)
                            .color(muted)
                            .font(theme::UI_MEDIUM),
                    )
                    .padding([0, 8]),
                )
                .push(
                    scrollable(file_list)
                        .style(theme::Scrollbar { dark }.style())
                        .height(if both && has_projects {
                            Length::FillPortion(2)
                        } else {
                            Length::Fill
                        }),
                )
                .push(container(text(footer).size(11.5).color(muted)).padding(8));
        }

        // The projects on top, the open files under them.
        container(content)
            .padding([14, 10])
            .width(width)
            .height(Length::Fill)
            .style(theme::panel(dark))
            .into()
    }
}

/// The tree of the projects: a line per folder or file that is shown.
/// What the menu of a line of the tree says.
struct ProjectMenuLabels {
    new_file: String,
    new_folder: String,
    refresh: String,
    remove: String,
    split_right: String,
    split_down: String,
    git: String,
    bookmark: String,
    delete: String,
    rename: String,
    copy: String,
    hide_ignored: String,
    duplicate: String,
    terminal: String,
    reveal: String,
    find: String,
    replace: String,
    history: String,
}

impl ProjectMenuLabels {
    fn new(bookmarked: bool, hide_ignored: bool) -> Self {
        Self {
            new_file: t!("sidebar.new_file").to_string(),
            new_folder: t!("sidebar.new_folder").to_string(),
            refresh: t!("sidebar.refresh").to_string(),
            remove: t!("sidebar.remove_project").to_string(),
            split_right: t!("sidebar.split_right").to_string(),
            split_down: t!("sidebar.split_down").to_string(),
            git: t!("sidebar.git").to_string(),
            bookmark: if bookmarked {
                t!("sidebar.bookmark_remove").to_string()
            } else {
                t!("sidebar.bookmark_add").to_string()
            },
            delete: t!("sidebar.delete").to_string(),
            rename: t!("sidebar.rename").to_string(),
            copy: t!("sidebar.copy_path").to_string(),
            duplicate: t!("sidebar.duplicate").to_string(),
            terminal: t!("sidebar.open_terminal").to_string(),
            reveal: t!("sidebar.reveal").to_string(),
            find: t!("sidebar.find_here").to_string(),
            replace: t!("sidebar.replace_here").to_string(),
            history: t!("history.show").to_string(),
            hide_ignored: if hide_ignored {
                t!("sidebar.show_ignored").to_string()
            } else {
                t!("sidebar.hide_ignored").to_string()
            },
        }
    }
}

fn project_tree<'a>(
    projects: &'a crate::projects::Projects,
    colors: &std::collections::BTreeMap<String, usize>,
    marks: &crate::projects::Marks,
    current: &Option<PathBuf>,
    marked: &HashSet<PathBuf>,
    dark: bool,
) -> Element<'a, Message> {
    let p = theme::palette(dark);
    let items: Vec<Element<Message>> = projects
        .rows()
        .into_iter()
        .map(|line| {
            let path = line.entry.path.clone();
            let current_file = !line.entry.is_dir && current.as_ref() == Some(&path);
            // The line picked, or the folder a dragged line would go to.
            let selected = projects.selected.as_ref() == Some(&path)
                || projects.drop_target.as_ref() == Some(&path);
            let chevron = if line.entry.is_dir {
                if line.open {
                    "▾"
                } else {
                    "▸"
                }
            } else {
                ""
            };
            let mut label = row![
                Space::new().width(Length::Fixed(line.depth as f32 * 14.0)),
                text(chevron).size(11).color(p.muted).width(14),
                if line.entry.is_dir {
                    crate::file_icons::folder(line.open, dark)
                } else {
                    crate::file_icons::file(&path, dark)
                },
                Space::new().width(2),
                text(line.entry.name.clone())
                    .size(13)
                    .shaping(iced::widget::text::Shaping::Advanced)
                    // Its color says how it stands in Git.
                    .color(if current_file {
                        p.accent
                    } else {
                        state_color(marks.git(&path, line.entry.is_dir), dark).unwrap_or(p.text)
                    })
                    .font(if line.root {
                        theme::UI_SEMIBOLD
                    } else {
                        iced::Font::DEFAULT
                    })
                    .width(Length::Fill),
            ]
            .spacing(4)
            .align_y(Alignment::Center);
            if marks.has_errors(&path, line.entry.is_dir) {
                label = label.push(error_dot(dark));
            }
            if line.root {
                let color = crate::projects::color_of(colors, &projects.roots, &path);
                label =
                    label.push(container(Space::new().width(8).height(8)).style(theme::dot(color)));
                label = label.push(
                    button(text("×").size(14).color(p.muted))
                        .padding([0, 5])
                        .on_press(Message::RemoveProject(path.clone()))
                        .style(
                            (theme::GhostButton {
                                dark,
                                active: false,
                            })
                            .style(),
                        ),
                );
            }
            let press = if line.entry.is_dir {
                Message::ToggleProjectFolder(path.clone())
            } else {
                Message::TreeOpenFile(path.clone())
            };
            let entry = button(container(label).center_y(Length::Fixed(TREE_ROW)))
                .on_press(press)
                .width(Length::Fill)
                .padding([0, 8])
                .style((theme::SidebarRow { dark, selected }).style());
            // Pressed, the line may be dragged onto a folder; the pointer
            // going over a line says where it would go.
            let press_path = path.clone();
            let entry = iced::widget::mouse_area(
                crate::pointer_area::pointer_area(entry)
                    .watching()
                    .on_press(move |_| Message::TreePress(press_path.clone())),
            )
            .on_enter(Message::TreeHover(path.clone()));

            let is_dir = line.entry.is_dir;
            let root = line.root;
            let bookmarked = marked.contains(&path);
            let hide_ignored = projects.hide_ignored;
            let menu_path = path.clone();
            let with_menu = crate::pointer_area::context_menu(entry, move || {
                // Built when the menu opens, not for every line of every view.
                let lbl = ProjectMenuLabels::new(bookmarked, hide_ignored);
                let item = |label: String, msg: Message| -> Element<'static, Message> {
                    button(text(label).size(13))
                        .padding([7, 10])
                        .width(Length::Fixed(196.0))
                        .on_press(msg)
                        .style(
                            (theme::MenuItem {
                                dark,
                                radius: theme::R_SM,
                                highlighted: false,
                            })
                            .style(),
                        )
                        .into()
                };
                let mut entries: Vec<Element<'static, Message>> = Vec::new();
                entries.push(item(
                    lbl.history.clone(),
                    Message::History(crate::app::HistoryMsg::Open(menu_path.clone())),
                ));
                if is_dir {
                    entries.push(item(
                        lbl.new_file.clone(),
                        Message::NewProjectEntry(menu_path.clone(), false),
                    ));
                    entries.push(item(
                        lbl.new_folder.clone(),
                        Message::NewProjectEntry(menu_path.clone(), true),
                    ));
                    entries.push(item(
                        lbl.find.clone(),
                        Message::FindInFolder(menu_path.clone()),
                    ));
                    entries.push(item(
                        lbl.replace.clone(),
                        Message::ReplaceInFolder(menu_path.clone()),
                    ));
                    entries.push(item(lbl.refresh.clone(), Message::RefreshProjects));
                    entries.push(item(
                        lbl.terminal.clone(),
                        Message::OpenTerminalAt(menu_path.clone()),
                    ));
                } else {
                    entries.push(item(
                        lbl.split_right.clone(),
                        Message::SplitWith(menu_path.clone(), true),
                    ));
                    entries.push(item(
                        lbl.split_down.clone(),
                        Message::SplitWith(menu_path.clone(), false),
                    ));
                    if crate::app::git_menu_applies(&menu_path) {
                        entries.push(item(
                            lbl.git.clone(),
                            Message::GitFileMenu(menu_path.clone()),
                        ));
                    }
                    entries.push(item(
                        lbl.bookmark.clone(),
                        Message::ToggleFileBookmark(menu_path.clone()),
                    ));
                }
                entries.push(item(lbl.copy.clone(), Message::CopyPath(menu_path.clone())));
                entries.push(item(
                    lbl.reveal.clone(),
                    Message::RevealInFileManager(menu_path.clone()),
                ));
                if !root {
                    entries.push(item(
                        lbl.rename.clone(),
                        Message::RenameProjectEntry(menu_path.clone()),
                    ));
                    entries.push(item(
                        lbl.duplicate.clone(),
                        Message::DuplicateEntry(menu_path.clone()),
                    ));
                    entries.push(item(
                        lbl.delete.clone(),
                        if is_dir {
                            Message::AskDeleteFolder(menu_path.clone())
                        } else {
                            Message::AskDeleteFile(menu_path.clone())
                        },
                    ));
                }
                if root {
                    entries.push(item(lbl.hide_ignored.clone(), Message::ToggleHideIgnored));
                    entries.push(item(
                        lbl.remove.clone(),
                        Message::RemoveProject(menu_path.clone()),
                    ));
                }
                container(column(entries).spacing(1).padding(6))
                    .style(theme::card(dark))
                    .into()
            });
            iced::widget::tooltip(
                with_menu,
                container(
                    text(crate::app::display_path(&path))
                        .size(12)
                        .shaping(iced::widget::text::Shaping::Advanced),
                )
                .padding([4, 8])
                .style(theme::tooltip(dark)),
                iced::widget::tooltip::Position::Bottom,
            )
            .gap(4)
            .delay(std::time::Duration::from_millis(600))
            .into()
        })
        .collect();
    column(items).spacing(1).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_file_appends_and_deduplicates() {
        let mut sidebar = SidebarState::new();
        let path = PathBuf::from("/tmp/test.rs");
        sidebar.add_file(path.clone());
        sidebar.add_file(path.clone());
        assert_eq!(sidebar.files.len(), 1);
    }

    #[test]
    fn a_tab_is_moved_to_another_place() {
        let mut sidebar = SidebarState::new();
        for name in ["a", "b", "c", "d"] {
            sidebar.add_file(PathBuf::from(name));
        }
        let order = |s: &SidebarState| -> Vec<String> {
            s.files.iter().map(|p| p.display().to_string()).collect()
        };
        assert!(sidebar.move_tab(0, 2));
        assert_eq!(order(&sidebar), ["b", "c", "a", "d"]);
        assert!(sidebar.move_tab(3, 0));
        assert_eq!(order(&sidebar), ["d", "b", "c", "a"]);
        // In place, or out of the list: nothing moves.
        assert!(!sidebar.move_tab(1, 1));
        assert!(!sidebar.move_tab(1, 9));
        assert_eq!(order(&sidebar), ["d", "b", "c", "a"]);
    }

    #[test]
    fn the_tree_scrolls_to_a_line_with_a_few_above_it() {
        assert_eq!(tree_offset(0), 0.0);
        assert_eq!(tree_offset(3), 0.0);
        assert_eq!(tree_offset(13), 10.0 * (TREE_ROW + 1.0));
    }

    #[test]
    fn add_multiple_files() {
        let mut sidebar = SidebarState::new();
        sidebar.add_file(PathBuf::from("/tmp/a.rs"));
        sidebar.add_file(PathBuf::from("/tmp/b.py"));
        assert_eq!(sidebar.files.len(), 2);
    }

    #[test]
    fn remove_requests_close_then_remove_path_drops_it() {
        let mut sidebar = SidebarState::new();
        sidebar.add_file(PathBuf::from("/tmp/a.rs"));
        assert!(matches!(
            sidebar.update(SidebarMessage::RemoveFile(PathBuf::from("/tmp/a.rs"))),
            SidebarAction::CloseFile(_)
        ));
        // Nothing is removed until the app confirms.
        assert_eq!(sidebar.files.len(), 1);
        sidebar.remove_path(std::path::Path::new("/tmp/a.rs"));
        assert!(sidebar.files.is_empty());
    }

    #[test]
    fn the_cross_closes_a_known_file_at_once() {
        let mut sidebar = SidebarState::new();
        sidebar.add_file(PathBuf::from("/tmp/a.rs"));
        assert!(matches!(
            sidebar.update(SidebarMessage::CloseNow(PathBuf::from("/tmp/a.rs"))),
            SidebarAction::CloseNow(_)
        ));
        // A tab that is already gone is ignored.
        assert!(matches!(
            sidebar.update(SidebarMessage::CloseNow(PathBuf::from("/tmp/other.rs"))),
            SidebarAction::None
        ));
    }

    #[test]
    fn renaming_onto_an_open_file_does_not_duplicate_it() {
        let mut sidebar = SidebarState::new();
        sidebar.add_file(PathBuf::from("untitled://2"));
        sidebar.add_file(PathBuf::from("/tmp/a.rs"));
        sidebar.rename_file(&PathBuf::from("untitled://2"), PathBuf::from("/tmp/a.rs"));
        assert_eq!(sidebar.files, vec![PathBuf::from("/tmp/a.rs")]);
    }
}
