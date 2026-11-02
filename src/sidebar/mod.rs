use iced::{
    widget::{button, column, container, row, scrollable, text, Space},
    Alignment, Element, Length,
};
use std::collections::HashSet;
use std::path::PathBuf;

use crate::app::Message;
use crate::theme;

#[derive(Debug, Clone)]
pub enum SidebarMessage {
    OpenFile(PathBuf),
    RemoveFile(PathBuf),
    RequestSave,
    RequestSaveAs,
}

/// What the app should do in response to a sidebar interaction.
pub enum SidebarAction {
    None,
    OpenFile(PathBuf),
    CloseFile(PathBuf),
    SaveFile,
    SaveFileAs,
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
            SidebarMessage::RequestSave => SidebarAction::SaveFile,
            SidebarMessage::RequestSaveAs => SidebarAction::SaveFileAs,
        }
    }

    /// The file list. `dirty` holds the open files with unsaved changes.
    pub fn view(
        &self,
        dark: bool,
        current_file: &Option<PathBuf>,
        dirty: &HashSet<PathBuf>,
    ) -> Element<'_, Message> {
        let lbl_close = t!("sidebar.close").to_string();
        let lbl_save = t!("sidebar.save").to_string();
        let lbl_save_as = t!("sidebar.save_as").to_string();
        let p = theme::palette(dark);

        let labels = crate::app::unique_labels(&self.files);
        let items: Vec<Element<Message>> = self
            .files
            .iter()
            .zip(labels)
            .map(|(path, name)| {
                let untitled = crate::app::is_untitled(path);
                // Short extension in mono, e.g. "rs", "json".
                let ext = if untitled {
                    String::new()
                } else {
                    path.extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("")
                        .to_lowercase()
                };
                let is_active = current_file.as_ref() == Some(path);
                let is_selected = is_active;
                let is_dirty = dirty.contains(path);
                let path_clone = path.clone();
                let close_path = path.clone();

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
                        text(ext)
                            .size(9.5)
                            .font(theme::MONO)
                            .color(ext_color)
                            .width(20),
                        text(name)
                            .size(13)
                            .shaping(iced::widget::text::Shaping::Advanced)
                            .width(Length::Fill),
                        marker,
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
                let lbl_save = lbl_save.clone();
                let lbl_save_as = lbl_save_as.clone();

                iced_aw::ContextMenu::new(row_button, move || {
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
                                lbl_save.clone(),
                                is_active.then_some(Message::Sidebar(SidebarMessage::RequestSave)),
                            ),
                            ctx_item(
                                lbl_save_as.clone(),
                                is_active
                                    .then_some(Message::Sidebar(SidebarMessage::RequestSaveAs)),
                            ),
                        ]
                        .spacing(1)
                        .padding(6),
                    )
                    .style(theme::card(dark))
                    .into()
                })
                .into()
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

        container(
            column![
                container(
                    text(t!("sidebar.files").to_uppercase())
                        .size(10.5)
                        .color(theme::muted_text(dark))
                        .font(theme::UI_MEDIUM)
                )
                .padding([0, 8]),
                scrollable(file_list)
                    .style(theme::Scrollbar { dark }.style())
                    .height(Length::Fill),
                container(text(footer).size(11.5).color(theme::muted_text(dark))).padding(8),
            ]
            .spacing(10),
        )
        .padding([14, 10])
        .width(208)
        .height(Length::Fill)
        .style(theme::panel(dark))
        .into()
    }
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
    fn renaming_onto_an_open_file_does_not_duplicate_it() {
        let mut sidebar = SidebarState::new();
        sidebar.add_file(PathBuf::from("untitled://2"));
        sidebar.add_file(PathBuf::from("/tmp/a.rs"));
        sidebar.rename_file(&PathBuf::from("untitled://2"), PathBuf::from("/tmp/a.rs"));
        assert_eq!(sidebar.files, vec![PathBuf::from("/tmp/a.rs")]);
    }
}
