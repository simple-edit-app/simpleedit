//! The diff pane: a toolbar to walk through the changes and the files, and
//! the diff itself, unified or side by side.

use iced::widget::{button, column, container, row, scrollable, text, Space};
use iced::{Alignment, Color, Element, Length};

use super::{GitMsg, Viewer};
use crate::app::Message;
use crate::git::diff::{Row, SideRow, Tag};
use crate::git::FileDiff;
use crate::theme as se_theme;

pub const ROW_HEIGHT: f32 = 18.0;
const FONT: f32 = 12.0;
/// JetBrains Mono's advance at 12 px.
const CHAR_W: f32 = FONT * 0.6;
const NUMBER_W: f32 = 44.0;
/// A diff longer than this many rows is cut (a widget per row).
const MAX_ROWS: usize = 4000;

/// The messages the toolbar's buttons send: the panel and an editor tab
/// each have their own.
pub struct Nav {
    pub prev_change: Message,
    pub next_change: Message,
    pub prev_file: Message,
    pub next_file: Message,
    pub toggle_side: Message,
}

impl Nav {
    pub fn panel() -> Nav {
        Nav {
            prev_change: Message::Git(GitMsg::PrevChange),
            next_change: Message::Git(GitMsg::NextChange),
            prev_file: Message::Git(GitMsg::PrevFile),
            next_file: Message::Git(GitMsg::NextFile),
            toggle_side: Message::Git(GitMsg::ToggleSideBySide),
        }
    }

    pub fn tab() -> Nav {
        use crate::app::DiffNav;
        Nav {
            prev_change: Message::DiffTab(DiffNav::PrevChange),
            next_change: Message::DiffTab(DiffNav::NextChange),
            prev_file: Message::DiffTab(DiffNav::PrevFile),
            next_file: Message::DiffTab(DiffNav::NextFile),
            toggle_side: Message::DiffTab(DiffNav::ToggleSide),
        }
    }
}

/// Added, removed and current-change backgrounds.
struct Colors {
    added: Color,
    removed: Color,
    added_now: Color,
    removed_now: Color,
    hunk: Color,
    number: Color,
}

fn colors(dark: bool) -> Colors {
    if dark {
        Colors {
            added: Color::from_rgba8(0x3f, 0xb9, 0x50, 0.16),
            removed: Color::from_rgba8(0xf8, 0x51, 0x49, 0.16),
            added_now: Color::from_rgba8(0x3f, 0xb9, 0x50, 0.32),
            removed_now: Color::from_rgba8(0xf8, 0x51, 0x49, 0.32),
            hunk: Color::from_rgba8(0x6e, 0xa8, 0xff, 0.12),
            number: Color::from_rgb8(0x5c, 0x61, 0x70),
        }
    } else {
        Colors {
            added: Color::from_rgba8(0x2e, 0xa0, 0x43, 0.16),
            removed: Color::from_rgba8(0xcf, 0x22, 0x2e, 0.14),
            added_now: Color::from_rgba8(0x2e, 0xa0, 0x43, 0.32),
            removed_now: Color::from_rgba8(0xcf, 0x22, 0x2e, 0.30),
            hunk: Color::from_rgba8(0x1b, 0x5f, 0xc6, 0.10),
            number: Color::from_rgb8(0x9a, 0x9f, 0xab),
        }
    }
}

/// Tabs become four spaces so the columns line up.
fn expand(text: &str) -> String {
    text.replace('\t', "    ")
}

fn cell<'a>(
    background: Option<Color>,
    content: Element<'a, Message>,
    width: f32,
) -> Element<'a, Message> {
    container(content)
        .width(Length::Fixed(width))
        .height(Length::Fixed(ROW_HEIGHT))
        .style(move |_: &iced::Theme| iced::widget::container::Style {
            background: background.map(iced::Background::Color),
            ..Default::default()
        })
        .into()
}

fn number<'a>(n: Option<u32>, color: Color) -> Element<'a, Message> {
    container(
        text(n.map(|n| n.to_string()).unwrap_or_default())
            .size(FONT - 1.0)
            .font(se_theme::MONO)
            .color(color),
    )
    .width(Length::Fixed(NUMBER_W))
    .padding([0, 6])
    .align_x(iced::alignment::Horizontal::Right)
    .height(Length::Fixed(ROW_HEIGHT))
    .align_y(iced::alignment::Vertical::Center)
    .into()
}

fn code<'a>(text_value: String, color: Color) -> Element<'a, Message> {
    container(
        text(text_value)
            .size(FONT)
            .font(se_theme::MONO)
            .color(color)
            .wrapping(iced::widget::text::Wrapping::None),
    )
    .height(Length::Fixed(ROW_HEIGHT))
    .align_y(iced::alignment::Vertical::Center)
    .padding([0, 6])
    .into()
}

/// The rows of the diff, as widgets.
fn body<'a>(
    diff: &'a FileDiff,
    current: usize,
    side_by_side: bool,
    scroll: &iced::widget::Id,
    dark: bool,
) -> Element<'a, Message> {
    let c = colors(dark);
    let p = se_theme::palette(dark);
    let mut rows: Vec<Element<'a, Message>> = Vec::new();
    let mut longest = 0usize;

    if side_by_side {
        let side = diff.side_rows();
        let blocks = diff.side_row_blocks();
        for row_ in &side {
            if let SideRow::Pair(l, r) = row_ {
                for line in [l, r].into_iter().flatten() {
                    longest = longest.max(expand(&line.text).chars().count());
                }
            }
        }
        let half = NUMBER_W + longest.max(30) as f32 * CHAR_W + 24.0;
        for (i, row_) in side.iter().enumerate().take(MAX_ROWS) {
            let now = blocks.get(i).copied().flatten() == Some(current);
            match row_ {
                SideRow::Hunk(header) => rows.push(cell(
                    Some(c.hunk),
                    code(header.to_string(), p.muted),
                    half * 2.0,
                )),
                SideRow::Pair(left, right) => {
                    let side_cell = |line: Option<&'a crate::git::diff::Line>, old: bool| {
                        let (background, text_value, n) = match line {
                            Some(l) => {
                                let bg = match (l.tag, now) {
                                    (Tag::Added, false) => Some(c.added),
                                    (Tag::Added, true) => Some(c.added_now),
                                    (Tag::Removed, false) => Some(c.removed),
                                    (Tag::Removed, true) => Some(c.removed_now),
                                    _ => None,
                                };
                                (bg, expand(&l.text), if old { l.old_no } else { l.new_no })
                            }
                            None => (Some(c.number.scale_alpha(0.12)), String::new(), None),
                        };
                        cell(
                            background,
                            row![number(n, c.number), code(text_value, p.text)].into(),
                            half,
                        )
                    };
                    rows.push(row![side_cell(*left, true), side_cell(*right, false)].into());
                }
            }
        }
    } else {
        let flat = diff.rows();
        let blocks = diff.row_blocks();
        for row_ in &flat {
            if let Row::Line(line) = row_ {
                longest = longest.max(expand(&line.text).chars().count());
            }
        }
        let width = NUMBER_W * 2.0 + 16.0 + longest.max(40) as f32 * CHAR_W + 24.0;
        for (i, row_) in flat.iter().enumerate().take(MAX_ROWS) {
            let now = blocks.get(i).copied().flatten() == Some(current);
            match row_ {
                Row::Hunk(header) => {
                    rows.push(cell(Some(c.hunk), code(header.to_string(), p.muted), width))
                }
                Row::Line(line) => {
                    let (background, marker) = match (line.tag, now) {
                        (Tag::Added, false) => (Some(c.added), "+"),
                        (Tag::Added, true) => (Some(c.added_now), "+"),
                        (Tag::Removed, false) => (Some(c.removed), "-"),
                        (Tag::Removed, true) => (Some(c.removed_now), "-"),
                        _ => (None, " "),
                    };
                    rows.push(cell(
                        background,
                        row![
                            number(line.old_no, c.number),
                            number(line.new_no, c.number),
                            code(format!("{marker} {}", expand(&line.text)), p.text),
                        ]
                        .into(),
                        width,
                    ));
                }
            }
        }
    }
    if rows.len() >= MAX_ROWS {
        rows.push(
            container(
                text(t!("git.diff_truncated").to_string())
                    .size(FONT)
                    .color(p.muted),
            )
            .padding(8)
            .into(),
        );
    }
    scrollable(column(rows))
        .id(scroll.clone())
        .direction(scrollable::Direction::Both {
            vertical: scrollable::Scrollbar::default(),
            horizontal: scrollable::Scrollbar::default(),
        })
        .style(se_theme::Scrollbar { dark }.style())
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn tool_button<'a>(
    label: &'a str,
    on_press: Option<Message>,
    active: bool,
    dark: bool,
) -> Element<'a, Message> {
    let mut b = button(text(label).size(13).font(se_theme::MONO)).padding([2, 8]);
    if let Some(m) = on_press {
        b = b.on_press(m);
    }
    b.style((se_theme::GhostButton { dark, active }).style())
        .into()
}

/// The toolbar and the diff of the open file.
pub fn view<'a>(viewer: &'a Viewer, nav: &Nav, dark: bool, empty: String) -> Element<'a, Message> {
    let p = se_theme::palette(dark);
    let count = viewer.entries.len();
    let at = viewer.index.map_or(0, |i| i + 1);
    let has_file = viewer.index.is_some();
    let changes = viewer.changes();
    let (added, removed) = viewer
        .file_diff()
        .map_or((0, 0), |d| (d.added(), d.removed()));

    let arrow = |label: &'static str, msg: &Message, enabled: bool| {
        tool_button(label, enabled.then(|| msg.clone()), false, dark)
    };
    let toolbar = row![
        arrow("↑", &nav.prev_change, has_file),
        arrow("↓", &nav.next_change, has_file),
        Space::new().width(10),
        arrow("←", &nav.prev_file, viewer.index.is_some_and(|i| i > 0)),
        text(t!("git.files_position", at = at, n = count).to_string())
            .size(12)
            .color(p.accent),
        arrow(
            "→",
            &nav.next_file,
            viewer.index.is_some_and(|i| i + 1 < count)
        ),
        Space::new().width(Length::Fill),
        text(format!("+{added}"))
            .size(12)
            .font(se_theme::MONO)
            .color(iced::Color::from_rgb8(0x3f, 0xb9, 0x50)),
        text(format!("−{removed}"))
            .size(12)
            .font(se_theme::MONO)
            .color(se_theme::error_color(dark)),
        Space::new().width(6),
        text(if changes == 1 {
            t!("git.difference_one").to_string()
        } else {
            t!("git.differences", n = changes).to_string()
        })
        .size(12)
        .color(p.muted),
        Space::new().width(8),
        tool_button(
            "▥",
            Some(nav.toggle_side.clone()),
            viewer.side_by_side,
            dark
        ),
    ]
    .spacing(4)
    .align_y(Alignment::Center);

    let header: Element<Message> = match viewer.current() {
        Some(entry) => {
            let mut label = entry.change.path.clone();
            if let Some(from) = &entry.change.from {
                label = format!("{from} → {label}");
            }
            text(label).size(12).color(p.muted).into()
        }
        None => Space::new().into(),
    };

    let content: Element<Message> = match (&viewer.diff, viewer.current()) {
        (Some(Ok(diff)), Some(_)) if diff.binary => centered(t!("git.binary").to_string(), dark),
        (Some(Ok(diff)), Some(_)) if diff.is_empty() => {
            centered(t!("git.no_diff").to_string(), dark)
        }
        (Some(Ok(diff)), Some(_)) => body(
            diff,
            viewer.change,
            viewer.side_by_side,
            &viewer.scroll,
            dark,
        ),
        (Some(Err(error)), _) => centered(error.clone(), dark),
        (None, Some(_)) => Space::new().into(),
        _ => centered(empty, dark),
    };

    column![
        container(toolbar).padding([6, 8]),
        container(header).padding([0, 10]),
        container(Space::new().height(1))
            .width(Length::Fill)
            .style(se_theme::rule(dark)),
        content,
    ]
    .spacing(4)
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn centered<'a>(label: String, dark: bool) -> Element<'a, Message> {
    container(text(label).size(13).color(se_theme::muted_text(dark)))
        .center(Length::Fill)
        .into()
}
