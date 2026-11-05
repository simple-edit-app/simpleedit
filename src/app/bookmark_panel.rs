//! The bookmarks panel: every bookmarked file, with its bookmarked lines
//! under it; a double click opens the file (at the line).

use super::*;
use iced::widget::{canvas, column};
use iced::Color;

/// What a row of the panel stands for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    File(PathBuf),
    /// A file and one of its lines (0-based).
    Line(PathBuf, usize),
}

/// A second click this soon on the same row is a double click.
const DOUBLE_CLICK: std::time::Duration = std::time::Duration::from_millis(450);

#[derive(Debug, Default)]
pub struct Panel {
    pub open: bool,
    /// Files whose lines are folded away.
    pub collapsed: HashSet<PathBuf>,
    pub selected: Option<Target>,
    last_click: Option<(Target, std::time::Instant)>,
    /// The lines of the bookmarked files that are not in the editor.
    texts: HashMap<PathBuf, Vec<String>>,
    /// The `Bookmarks::rev` the texts were read for.
    built: u64,
}

/// A bookmark ribbon.
struct Ribbon {
    color: Color,
    filled: bool,
}

impl<M> canvas::Program<M> for Ribbon {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        _theme: &Theme,
        bounds: iced::Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        use iced::Point;
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let (left, right, top, bottom) = (3.5, 12.5, 1.5, 14.5);
        let middle = (left + right) / 2.0;
        let ribbon = canvas::Path::new(|p| {
            p.move_to(Point::new(left, top));
            p.line_to(Point::new(right, top));
            p.line_to(Point::new(right, bottom));
            p.line_to(Point::new(middle, bottom - 3.5));
            p.line_to(Point::new(left, bottom));
            p.close();
        });
        if self.filled {
            frame.fill(&ribbon, self.color);
        } else {
            frame.stroke(
                &ribbon,
                canvas::Stroke::default()
                    .with_color(self.color)
                    .with_width(1.4),
            );
        }
        vec![frame.into_geometry()]
    }
}

/// The icon, 16 px wide.
pub fn ribbon<'a, M: 'a>(color: Color, filled: bool) -> Element<'a, M> {
    canvas::Canvas::new(Ribbon { color, filled })
        .width(Length::Fixed(16.0))
        .height(Length::Fixed(16.0))
        .into()
}

impl SimpleEditApp {
    /// Reads, for the bookmarked files that need it, the lines to show.
    /// `all` forgets what was read before.
    pub(super) fn refresh_bookmark_texts(&mut self, all: bool) {
        let entries = self.bookmarks.entries();
        let panel = &mut self.bookmark_panel;
        if all {
            panel.texts.clear();
        }
        panel
            .texts
            .retain(|path, _| entries.iter().any(|e| &e.path == path));
        for entry in entries {
            if entry.lines.is_empty() || panel.texts.contains_key(&entry.path) {
                continue;
            }
            if let Ok(text) = file_io::read_text(&entry.path) {
                panel
                    .texts
                    .insert(entry.path, text.lines().map(str::to_string).collect());
            }
        }
        panel.built = self.bookmarks.rev;
    }

    /// Looks again at the files when the bookmarks changed.
    pub(super) fn refresh_bookmark_panel(&mut self) {
        if self.bookmark_panel.open && self.bookmark_panel.built != self.bookmarks.rev {
            self.refresh_bookmark_texts(false);
        }
    }

    /// The text of a bookmarked line: from the editor for the open file.
    fn bookmark_line_text(&self, path: &Path, line: usize) -> String {
        let text = if self.current_file.as_deref() == Some(path) {
            self.editor.content.line(line).map(|l| l.to_string())
        } else {
            self.bookmark_panel
                .texts
                .get(path)
                .and_then(|lines| lines.get(line).cloned())
        };
        text.unwrap_or_default().trim().to_string()
    }

    /// A click on a row: selects it; the second one in a row opens it.
    pub(super) fn bookmark_click(&mut self, target: Target) -> Task<Message> {
        let now = std::time::Instant::now();
        let double = self
            .bookmark_panel
            .last_click
            .as_ref()
            .is_some_and(|(last, at)| *last == target && now.duration_since(*at) < DOUBLE_CLICK);
        self.bookmark_panel.selected = Some(target.clone());
        if !double {
            self.bookmark_panel.last_click = Some((target, now));
            return Task::none();
        }
        self.bookmark_panel.last_click = None;
        let (path, line) = match target {
            Target::File(path) => (path, None),
            Target::Line(path, line) => (path, Some(line)),
        };
        if !is_untitled(&path) && !path.is_file() {
            self.set_status(t!("bookmarks.err_missing"), true);
            return Task::none();
        }
        match line {
            Some(line) => {
                self.remember_place();
                self.go_to(crate::definition::Hit {
                    path,
                    line,
                    col: 0,
                    len: 0,
                })
            }
            None => self.activate(path),
        }
    }

    /// Adds a file to the bookmarks, or takes it out.
    pub(super) fn toggle_file_bookmark(&mut self, path: PathBuf) {
        let added = self.bookmarks.toggle_file(&path);
        self.set_status(
            if added {
                t!("status.file_bookmark_added")
            } else {
                t!("status.file_bookmark_removed")
            },
            false,
        );
    }

    pub(super) fn delete_bookmark(&mut self, target: Target) {
        match target {
            Target::File(path) => self.bookmarks.remove_entry(&path),
            Target::Line(path, line) => {
                if self.bookmarks.has(&path, line) {
                    self.bookmarks.toggle(&path, line);
                }
            }
        }
    }

    pub(super) fn bookmark_panel_view(&self, dark: bool) -> Element<'_, Message> {
        let p = se_theme::palette(dark);
        let amber = Color::from_rgb8(0xf2, 0xc0, 0x4c);
        let ghost = move |active: bool| (se_theme::GhostButton { dark, active }).style();
        let header = row![
            text(t!("bookmarks.title").to_string())
                .size(12)
                .font(se_theme::UI_SEMIBOLD)
                .color(p.text),
            Space::new().width(Length::Fill),
            button(text("—").size(12))
                .padding([1, 8])
                .on_press(Message::ToggleBookmarkPanel)
                .style(ghost(false)),
        ]
        .align_y(Alignment::Center);

        let delete_label = t!("bookmarks.delete").to_string();
        // A row, with the menu that deletes it.
        let with_menu = move |row_button: Element<'static, Message>, target: Target| {
            let label = delete_label.clone();
            crate::pointer_area::context_menu(row_button, move || {
                container(
                    button(text(label.clone()).size(13))
                        .padding([7, 10])
                        .width(Length::Fixed(160.0))
                        .on_press(Message::BookmarkDelete(target.clone()))
                        .style(
                            (se_theme::MenuItem {
                                dark,
                                radius: se_theme::R_SM,
                                highlighted: false,
                            })
                            .style(),
                        ),
                )
                .padding(6)
                .style(se_theme::card(dark))
                .into()
            })
        };

        let home = dirs::home_dir();
        let mut list = column![].spacing(1);
        let entries = self.bookmarks.entries();
        if entries.is_empty() {
            list = list.push(
                text(t!("bookmarks.empty").to_string())
                    .size(12.5)
                    .color(p.muted),
            );
        }
        for entry in entries {
            let target = Target::File(entry.path.clone());
            let name = entry
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| entry.path.to_string_lossy().into_owned());
            let folder = entry
                .path
                .parent()
                .map(
                    |d| match home.as_deref().and_then(|h| d.strip_prefix(h).ok()) {
                        Some(rest) => format!("~/{}", rest.display()),
                        None => d.display().to_string(),
                    },
                )
                .unwrap_or_default();
            let folded = self.bookmark_panel.collapsed.contains(&entry.path);
            let arrow: Element<Message> = if entry.lines.is_empty() {
                Space::new().width(22).into()
            } else {
                button(text(if folded { "▸" } else { "▾" }).size(12))
                    .padding([0, 6])
                    .width(22)
                    .on_press(Message::BookmarkFold(entry.path.clone()))
                    .style(ghost(false))
                    .into()
            };
            let mut label = row![arrow].spacing(4).align_y(Alignment::Center);
            if entry.marked {
                label = label.push(ribbon(amber, true));
            }
            label = label
                .push(crate::file_icons::file(&entry.path, dark))
                .push(
                    text(name)
                        .size(13)
                        .shaping(iced::widget::text::Shaping::Advanced),
                )
                .push(
                    text(folder)
                        .size(11.5)
                        .color(p.muted)
                        .wrapping(iced::widget::text::Wrapping::None),
                );
            let selected = self.bookmark_panel.selected.as_ref() == Some(&target);
            let file_row = button(label)
                .padding([4, 6])
                .width(Length::Fill)
                .on_press(Message::BookmarkClick(target.clone()))
                .style((se_theme::SidebarRow { dark, selected }).style());
            list = list.push(with_menu(file_row.into(), target));
            if folded {
                continue;
            }
            for &line in &entry.lines {
                let target = Target::Line(entry.path.clone(), line);
                let selected = self.bookmark_panel.selected.as_ref() == Some(&target);
                let label = row![
                    Space::new().width(26),
                    ribbon(amber, true),
                    text(format!("{}:", line + 1))
                        .size(12.5)
                        .font(se_theme::MONO)
                        .color(p.muted),
                    text(self.bookmark_line_text(&entry.path, line))
                        .size(12.5)
                        .font(se_theme::MONO)
                        .wrapping(iced::widget::text::Wrapping::None),
                ]
                .spacing(6)
                .align_y(Alignment::Center);
                let line_row = button(label)
                    .padding([3, 6])
                    .width(Length::Fill)
                    .on_press(Message::BookmarkClick(target.clone()))
                    .style((se_theme::SidebarRow { dark, selected }).style());
                list = list.push(with_menu(line_row.into(), target));
            }
        }

        column![
            super::resize_edge(dark),
            container(
                column![
                    header,
                    scrollable(container(list).padding([0, 4]).width(Length::Fill))
                        .style(se_theme::Scrollbar { dark }.style())
                        .height(Length::Fill),
                ]
                .spacing(8)
            )
            .padding([8, 12])
            .height(Length::Fill),
        ]
        .height(Length::Fixed(self.panel_height))
        .into()
    }
}
