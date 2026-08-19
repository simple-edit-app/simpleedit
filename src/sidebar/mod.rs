use iced::{
    widget::{button, column, container, scrollable, text, Space},
    Element, Length,
};
use std::path::PathBuf;

use crate::app::Message;
use crate::theme;

#[derive(Debug, Clone)]
pub enum SidebarMessage {
    OpenFile(PathBuf),
    RemoveFile(usize),
    RequestSave(PathBuf),
    RequestSaveAs,
}

/// What the app should do in response to a sidebar interaction.
pub enum SidebarAction {
    None,
    OpenFile(PathBuf),
    CloseFile(PathBuf),
    SaveFile(PathBuf),
    SaveFileAs,
}

pub struct SidebarState {
    pub files: Vec<PathBuf>,
    pub selected: Option<usize>,
}

impl SidebarState {
    pub fn new() -> Self {
        Self {
            files: Vec::new(),
            selected: None,
        }
    }

    pub fn files(&self) -> &[PathBuf] {
        &self.files
    }

    pub fn add_file(&mut self, path: PathBuf) {
        if !self.files.contains(&path) {
            self.files.push(path.clone());
        }
        self.selected = self.files.iter().position(|p| p == &path);
    }

    /// Replace an untitled slot with its real path after Save As.
    pub fn rename_file(&mut self, old: &PathBuf, new: PathBuf) {
        if let Some(pos) = self.files.iter().position(|p| p == old) {
            self.files[pos] = new;
            self.selected = Some(pos);
        }
    }

    pub fn update(&mut self, msg: SidebarMessage) -> SidebarAction {
        match msg {
            SidebarMessage::OpenFile(path) => {
                self.selected = self.files.iter().position(|p| p == &path);
                SidebarAction::OpenFile(path)
            }
            SidebarMessage::RemoveFile(idx) => {
                if idx < self.files.len() {
                    let path = self.files.remove(idx);
                    // Adjust or clear selected index
                    self.selected = match self.selected {
                        Some(sel) if sel == idx => None,
                        Some(sel) if sel > idx => Some(sel - 1),
                        other => other,
                    };
                    // Always propagate so app can update file_cache and save session
                    return SidebarAction::CloseFile(path);
                }
                SidebarAction::None
            }
            SidebarMessage::RequestSave(path) => SidebarAction::SaveFile(path),
            SidebarMessage::RequestSaveAs => SidebarAction::SaveFileAs,
        }
    }

    pub fn view(&self, dark: bool, current_file: &Option<PathBuf>) -> Element<'_, Message> {
        let lbl_close = t!("sidebar.close").to_string();
        let lbl_save = t!("sidebar.save").to_string();
        let lbl_save_as = t!("sidebar.save_as").to_string();

        let items: Vec<Element<Message>> = self
            .files
            .iter()
            .enumerate()
            .map(|(i, path)| {
                let name = if crate::app::is_untitled(path) {
                    let n = path
                        .to_str()
                        .unwrap_or("")
                        .trim_start_matches("untitled://");
                    format!("{} {}", t!("sidebar.untitled"), n)
                } else {
                    path.file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("?")
                        .to_string()
                };
                let is_selected = self.selected == Some(i);
                let is_active = current_file.as_ref() == Some(path);
                let path_clone = path.clone();

                let row_button = button(text(name).size(13))
                    .on_press(Message::Sidebar(SidebarMessage::OpenFile(path_clone)))
                    .width(Length::Fill)
                    .padding([6, 10])
                    .style(iced::theme::Button::custom(theme::SidebarRow {
                        dark,
                        selected: is_selected,
                    }));

                // Build context menu labels (owned Strings for 'static closure)
                let lbl_close = lbl_close.clone();
                let lbl_save = lbl_save.clone();
                let lbl_save_as = lbl_save_as.clone();
                let path_for_save = path.clone();
                let _path_for_close = path.clone();

                iced_aw::ContextMenu::new(row_button, move || {
                    let ctx_item =
                        |label: String, msg: Option<Message>| -> Element<'static, Message> {
                            let mut btn = button(text(label).size(13))
                                .padding([6, 10])
                                .width(Length::Fixed(160.0))
                                .style(iced::theme::Button::custom(theme::GhostButton {
                                    dark,
                                    active: false,
                                }));
                            if let Some(m) = msg {
                                btn = btn.on_press(m);
                            }
                            btn.into()
                        };

                    container(
                        column![
                            ctx_item(
                                lbl_close.clone(),
                                Some(Message::Sidebar(SidebarMessage::RemoveFile(i))),
                            ),
                            ctx_item(
                                lbl_save.clone(),
                                is_active.then(|| {
                                    Message::Sidebar(SidebarMessage::RequestSave(
                                        path_for_save.clone(),
                                    ))
                                }),
                            ),
                            ctx_item(
                                lbl_save_as.clone(),
                                is_active
                                    .then_some(Message::Sidebar(SidebarMessage::RequestSaveAs)),
                            ),
                        ]
                        .spacing(2)
                        .padding(6),
                    )
                    .style(theme::card(dark))
                    .into()
                })
                .into()
            })
            .collect();

        let file_list = if items.is_empty() {
            column![text(t!("sidebar.no_files"))
                .size(12)
                .style(theme::muted_text(dark))]
        } else {
            column(items).spacing(2)
        };

        let header = Space::with_height(0); // placeholder to keep layout identical

        let _ = header; // suppress unused warning

        container(
            scrollable(
                column![
                    text(t!("sidebar.files"))
                        .size(11)
                        .style(theme::muted_text(dark)),
                    file_list,
                ]
                .spacing(10)
                .padding(10),
            )
            .height(Length::Fill),
        )
        .width(220)
        .height(Length::Fill)
        .style(theme::panel(dark))
        .into()
    }
}

fn _action_button_unused() {} // dead code suppressor noop

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
        assert_eq!(sidebar.selected, Some(1));
    }

    #[test]
    fn remove_file() {
        let mut sidebar = SidebarState::new();
        sidebar.add_file(PathBuf::from("/tmp/a.rs"));
        sidebar.update(SidebarMessage::RemoveFile(0));
        assert!(sidebar.files.is_empty());
    }
}
