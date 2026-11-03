//! The diff pane: a toolbar to walk through the changes and the files, and
//! the diff itself, unified or side by side.

use iced::widget::{button, column, container, row, scrollable, text, Space};
use iced::{Alignment, Color, Element, Length, Point};

use super::{DiffPointer, GitMsg, Selection, Viewer};
use crate::app::Message;
use crate::git::diff::{Row, SideRow, Tag};
use crate::git::FileDiff;
use crate::pointer_area::pointer_area;
use crate::theme as se_theme;

pub const ROW_HEIGHT: f32 = 18.0;
const FONT: f32 = 12.0;
/// JetBrains Mono's advance at 12 px.
const CHAR_W: f32 = FONT * 0.6;
const NUMBER_W: f32 = 44.0;
/// The column of rollback arrows.
const ARROW_W: f32 = 24.0;
/// A diff longer than this many rows is cut (only the rows in view are
/// widgets, but the file has to be laid out).
const MAX_ROWS: usize = 20_000;

/// The messages the toolbar's buttons send: the panel and an editor tab
/// each have their own.
pub struct Nav {
    pub prev_change: Message,
    pub next_change: Message,
    pub prev_file: Message,
    pub next_file: Message,
    pub toggle_side: Message,
    /// The arrow that puts a block of changes back (the working tree's diff).
    pub rollback: Option<fn(usize) -> Message>,
    /// Switches a commit's own diff to a comparison with the current file
    /// (and back), and opens the file to edit it: the editor tab only.
    pub toggle_compare: Option<Message>,
    pub open_file: Option<Message>,
    /// The mouse on the rows (to select and copy them).
    pub pick: fn(DiffPointer) -> Message,
    pub copy: Message,
    pub select_all: Message,
    /// The scrollable of the diff moved: (offset, height shown).
    pub scrolled: fn(i32, i32) -> Message,
}

impl Nav {
    pub fn panel() -> Nav {
        Nav {
            prev_change: Message::Git(GitMsg::PrevChange),
            next_change: Message::Git(GitMsg::NextChange),
            prev_file: Message::Git(GitMsg::PrevFile),
            next_file: Message::Git(GitMsg::NextFile),
            toggle_side: Message::Git(GitMsg::ToggleSideBySide),
            rollback: Some(|block| Message::Git(GitMsg::RollbackBlock(block))),
            toggle_compare: None,
            open_file: Some(Message::Git(GitMsg::OpenFile)),
            pick: |p| Message::Git(GitMsg::Pick(p)),
            copy: Message::Git(GitMsg::CopyPicked),
            select_all: Message::Git(GitMsg::PickAll),
            scrolled: |top, height| Message::Git(GitMsg::DiffScrolled(top, height)),
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
            rollback: Some(|block| Message::DiffTab(DiffNav::Apply(block))),
            toggle_compare: Some(Message::DiffTab(DiffNav::ToggleCompare)),
            open_file: Some(Message::DiffTab(DiffNav::OpenFile)),
            pick: |p| Message::DiffTab(DiffNav::Pick(p)),
            copy: Message::DiffTab(DiffNav::CopyPicked),
            select_all: Message::DiffTab(DiffNav::PickAll),
            scrolled: |top, height| Message::DiffTab(DiffNav::Scrolled(top, height)),
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

/// How many characters fit on one line of a side of the side-by-side view
/// in `width` pixels (a little under, so a wrapped line is never cut short).
fn columns_for(width: f32, arrows: bool) -> usize {
    let middle = if arrows { ARROW_W } else { 0.0 };
    // The scrollbar, then both sides, each with its number column and padding.
    let side = (width - middle - 14.0) / 2.0;
    (((side - NUMBER_W - 12.0) / CHAR_W).floor() as usize)
        .saturating_sub(1)
        .max(10)
}

/// How many columns of the monospace grid a character takes (two for the
/// wide ones, CJK and the like).
fn cells(c: char) -> usize {
    unicode_width::UnicodeWidthChar::width(c)
        .unwrap_or(1)
        .max(1)
}

/// The index of the character each line of a text starts at when it is
/// broken into lines of `columns` cells (the first is always 0).
fn line_starts(chars: &[char], columns: usize) -> Vec<usize> {
    let mut starts = vec![0];
    if columns == 0 {
        return starts;
    }
    let (mut used, mut line_start) = (0, 0);
    for (i, &c) in chars.iter().enumerate() {
        let width = cells(c);
        if used + width > columns && i > line_start {
            starts.push(i);
            line_start = i;
            used = 0;
        }
        used += width;
    }
    starts
}

/// How many lines a text takes in a side `columns` cells wide.
fn lines_of(text: &str, columns: usize) -> usize {
    if columns == 0 {
        return 1;
    }
    let chars: Vec<char> = expand(text).chars().collect();
    line_starts(&chars, columns).len()
}

/// How many text lines each row of the side-by-side view is tall.
fn side_heights(diff: &FileDiff, columns: usize) -> Vec<usize> {
    diff.side_rows()
        .iter()
        .map(|row| match row {
            SideRow::Hunk(_) => 1,
            SideRow::Pair(l, r) => [l, r]
                .into_iter()
                .flatten()
                .map(|line| lines_of(&line.text, columns))
                .max()
                .unwrap_or(1),
        })
        .collect()
}

/// Where a row of the side-by-side view starts, from the top.
pub fn side_top(diff: &FileDiff, row: usize, columns: usize) -> f32 {
    side_heights(diff, columns).iter().take(row).sum::<usize>() as f32 * ROW_HEIGHT
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

/// What is picked, and how the mouse picks it.
#[derive(Clone, Copy)]
struct Sel {
    sel: Option<Selection>,
    dragging: bool,
    make: fn(DiffPointer) -> Message,
}

/// Where the text of a cell starts, and what its lines are made of.
#[derive(Clone)]
struct Layout {
    /// How far from the cell's left edge the text begins.
    text_x: f32,
    /// Cells in front of the text that aren't part of it (`+ `).
    prefix: usize,
    /// Cells per line when wrapped (0: never wrapped).
    columns: usize,
    /// The text.
    chars: std::sync::Arc<Vec<char>>,
}

/// The character of a text the pointer is on (the nearest gap between two),
/// given where it is in the cell.
fn char_at(point: Point, at: &Layout) -> usize {
    let col = ((point.x - at.text_x) / CHAR_W).round().max(0.0) as usize;
    let chars = &at.chars;
    // The line of the text the pointer is on, and the characters it holds.
    let (start, end, col) = if at.columns > 0 {
        let starts = line_starts(chars, at.columns);
        let line = ((point.y / ROW_HEIGHT).floor().max(0.0) as usize).min(starts.len() - 1);
        let end = starts.get(line + 1).copied().unwrap_or(chars.len());
        (starts[line], end, col)
    } else {
        (0, chars.len(), col.saturating_sub(at.prefix))
    };
    // Walk the line a cell at a time: a wide character spans two.
    let (mut idx, mut used) = (start, 0);
    while idx < end && used + cells(chars[idx]) <= col {
        used += cells(chars[idx]);
        idx += 1;
    }
    idx
}

impl Sel {
    /// The picked characters of a row (of one side, side by side).
    fn span(&self, row: usize, right: Option<bool>, len: usize) -> Option<(usize, usize)> {
        let sel = self.sel?;
        if right.is_some_and(|r| r != sel.right) {
            return None;
        }
        sel.span(row, len)
    }

    /// A cell that can be pressed and dragged over to pick its text.
    fn catch<'a>(
        &self,
        element: Element<'a, Message>,
        row: usize,
        right: bool,
        at: Layout,
    ) -> Element<'a, Message> {
        let make = self.make;
        let pressed = at.clone();
        let area = pointer_area(element)
            .on_press(move |p| make(DiffPointer::Press(row, right, char_at(p, &pressed))))
            .on_release(make(DiffPointer::Release));
        if self.dragging {
            area.on_move(move |p| make(DiffPointer::Drag(row, char_at(p, &at))))
        } else {
            area
        }
        .into()
    }
}

/// `chars[range]`, broken into lines of `columns` cells (counted from the
/// start of the whole text), so the wrapping is ours and not the widget's.
fn broken(chars: &[char], range: std::ops::Range<usize>, columns: usize) -> String {
    let starts = line_starts(chars, columns);
    let mut out = String::new();
    for i in range {
        if i > 0 && starts.binary_search(&i).is_ok() {
            out.push('\n');
        }
        out.push(chars[i]);
    }
    out
}

/// A line of the diff, with the picked part highlighted.
fn shown_text<'a>(
    prefix: &str,
    text_value: &str,
    picked: Option<(usize, usize)>,
    columns: usize,
    color: Color,
    highlight: Color,
) -> Element<'a, Message> {
    let chars: Vec<char> = text_value.chars().collect();
    let wrapping = if columns > 0 {
        iced::widget::text::Wrapping::Glyph
    } else {
        iced::widget::text::Wrapping::None
    };
    let line_height = iced::widget::text::LineHeight::Absolute(iced::Pixels(ROW_HEIGHT));
    let Some((start, end)) = picked else {
        return text(format!(
            "{prefix}{}",
            broken(&chars, 0..chars.len(), columns)
        ))
        .size(FONT)
        .font(se_theme::MONO)
        .color(color)
        .line_height(line_height)
        .wrapping(wrapping)
        .into();
    };
    let plain = |t: String| iced::widget::span(t).font(se_theme::MONO).color(color);
    let spans = vec![
        plain(format!("{prefix}{}", broken(&chars, 0..start, columns))),
        plain(broken(&chars, start..end, columns)).background(highlight),
        plain(broken(&chars, end..chars.len(), columns)),
    ];
    let rich: iced::widget::text::Rich<'a, (), Message> = iced::widget::rich_text(spans);
    rich.size(FONT)
        .line_height(line_height)
        .wrapping(wrapping)
        .into()
}

/// What [`body`] shows and how.
#[derive(Clone, Copy)]
struct Body<'a> {
    diff: &'a FileDiff,
    current: usize,
    side_by_side: bool,
    scroll: &'a iced::widget::Id,
    rollback: Option<fn(usize) -> Message>,
    /// Characters per line of a side (side by side).
    columns: usize,
    select: Sel,
    /// Where the scrollable is and how tall it shows: only the rows around
    /// that are built.
    top: f32,
    height: f32,
    /// Tells where the scrollable is now (offset, height of what it shows).
    scrolled: fn(i32, i32) -> Message,
    dark: bool,
}

/// Rows built beyond the ones in view, so a scroll shows no blank.
const OVERSCAN: f32 = 600.0;

/// Which rows of a list (`heights` in pixels) are around the part in view
/// (`top`, `height`): the first, the last + 1, and the space left out above
/// and below.
pub(super) fn visible(heights: &[f32], top: f32, height: f32) -> (usize, usize, f32, f32) {
    // The scrollable never goes past the end (a diff that got shorter keeps
    // an offset it can't have): the last screenful is what it shows then.
    let total: f32 = heights.iter().sum();
    let top = top.min((total - height).max(0.0));
    let (from, to) = ((top - OVERSCAN).max(0.0), top + height + OVERSCAN);
    let (mut first, mut y) = (0, 0.0);
    while first < heights.len() && y + heights[first] <= from {
        y += heights[first];
        first += 1;
    }
    let before = y;
    let mut last = first;
    while last < heights.len() && y < to {
        y += heights[last];
        last += 1;
    }
    let after: f32 = heights[last..].iter().sum();
    (first, last, before, after)
}

/// The rows of the diff, as widgets.
fn body<'a>(args: Body<'a>) -> Element<'a, Message> {
    let Body {
        diff,
        current,
        side_by_side,
        scroll,
        rollback,
        columns,
        select,
        top,
        height,
        scrolled,
        dark,
    } = args;
    let c = colors(dark);
    let p = se_theme::palette(dark);
    let mut rows: Vec<Element<'a, Message>> = Vec::new();
    let mut longest = 0usize;
    // In front of each row: the arrow that rolls back the block it opens.
    let with_arrow =
        |element: Element<'a, Message>, block: Option<usize>| -> Element<'a, Message> {
            let Some(make) = rollback else {
                return element;
            };
            let arrow: Element<'a, Message> = match block {
                Some(b) => button(text("↶").size(13).color(se_theme::palette(dark).muted))
                    .padding([0, 4])
                    .height(Length::Fixed(ROW_HEIGHT))
                    .on_press(make(b))
                    .style(
                        (se_theme::GhostButton {
                            dark,
                            active: false,
                        })
                        .style(),
                    )
                    .into(),
                None => Space::new().width(Length::Fixed(ARROW_W)).into(),
            };
            row![container(arrow).width(Length::Fixed(ARROW_W)), element].into()
        };
    let starts = |blocks: &[Option<usize>], i: usize| -> Option<usize> {
        match blocks.get(i).copied().flatten() {
            Some(b) if i == 0 || blocks.get(i - 1).copied().flatten() != Some(b) => Some(b),
            _ => None,
        }
    };

    let truncated = if side_by_side {
        let side = diff.side_rows();
        let blocks = diff.side_row_blocks();
        let heights = side_heights(diff, columns);
        let pixels: Vec<f32> = heights
            .iter()
            .take(MAX_ROWS)
            .map(|h| *h as f32 * ROW_HEIGHT)
            .collect();
        let (first, last, before, after) = visible(&pixels, top, height);
        rows.push(Space::new().height(before).into());
        for (i, row_) in side.iter().enumerate().skip(first).take(last - first) {
            let now = blocks.get(i).copied().flatten() == Some(current);
            let lines = heights.get(i).copied().unwrap_or(1);
            let tall = Length::Fixed(lines as f32 * ROW_HEIGHT);
            match row_ {
                SideRow::Hunk(header) => rows.push(
                    container(code(header.to_string(), p.muted))
                        .width(Length::Fill)
                        .height(Length::Fixed(ROW_HEIGHT))
                        .style(move |_: &iced::Theme| iced::widget::container::Style {
                            background: Some(iced::Background::Color(c.hunk)),
                            ..Default::default()
                        })
                        .into(),
                ),
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
                        let chars: Vec<char> = text_value.chars().collect();
                        let picked = select.span(i, Some(!old), chars.len());
                        let at = Layout {
                            text_x: NUMBER_W + 6.0,
                            prefix: 0,
                            columns,
                            chars: std::sync::Arc::new(chars),
                        };
                        let cell =
                            container(row![
                                container(
                                    text(n.map(|n| n.to_string()).unwrap_or_default())
                                        .size(FONT - 1.0)
                                        .font(se_theme::MONO)
                                        .color(c.number),
                                )
                                .width(Length::Fixed(NUMBER_W))
                                .padding([0, 6])
                                .align_x(iced::alignment::Horizontal::Right)
                                .height(Length::Fixed(ROW_HEIGHT))
                                .align_y(iced::alignment::Vertical::Center),
                                container(shown_text(
                                    "",
                                    &text_value,
                                    picked,
                                    columns,
                                    p.text,
                                    p.accent.scale_alpha(0.45),
                                ))
                                .width(Length::Fill)
                                .padding([0, 6])
                                .align_y(iced::alignment::Vertical::Top),
                            ])
                            .width(Length::FillPortion(1))
                            .height(tall)
                            .clip(true)
                            .style(move |_: &iced::Theme| iced::widget::container::Style {
                                background: background.map(iced::Background::Color),
                                ..Default::default()
                            });
                        select.catch(cell.into(), i, !old, at)
                    };
                    // The chevron that carries a change across sits between the sides.
                    let middle: Element<'a, Message> = match (rollback, starts(&blocks, i)) {
                        (Some(make), Some(b)) => iced::widget::tooltip(
                            button(text("»").size(13).color(p.accent))
                                .padding([0, 4])
                                .height(Length::Fixed(ROW_HEIGHT))
                                .on_press(make(b))
                                .style(
                                    (se_theme::GhostButton {
                                        dark,
                                        active: false,
                                    })
                                    .style(),
                                ),
                            container(text(t!("git.apply_here").to_string()).size(12))
                                .padding([3, 8])
                                .style(se_theme::tooltip(dark)),
                            iced::widget::tooltip::Position::Top,
                        )
                        .into(),
                        _ => Space::new().into(),
                    };
                    let middle = container(middle).width(Length::Fixed(if rollback.is_some() {
                        ARROW_W
                    } else {
                        0.0
                    }));
                    rows.push(
                        row![side_cell(*left, true), middle, side_cell(*right, false)].into(),
                    );
                }
            }
        }
        rows.push(Space::new().height(after).into());
        side.len() > MAX_ROWS
    } else {
        let flat = diff.rows();
        let blocks = diff.row_blocks();
        for row_ in &flat {
            if let Row::Line(line) = row_ {
                longest = longest.max(expand(&line.text).chars().count());
            }
        }
        let width = NUMBER_W * 2.0 + 16.0 + longest.max(40) as f32 * CHAR_W + 24.0;
        let pixels = vec![ROW_HEIGHT; flat.len().min(MAX_ROWS)];
        let (first, last, before, after) = visible(&pixels, top, height);
        rows.push(Space::new().height(before).into());
        for (i, row_) in flat.iter().enumerate().skip(first).take(last - first) {
            let now = blocks.get(i).copied().flatten() == Some(current);
            match row_ {
                Row::Hunk(header) => rows.push(with_arrow(
                    cell(Some(c.hunk), code(header.to_string(), p.muted), width),
                    None,
                )),
                Row::Line(line) => {
                    let (background, marker) = match (line.tag, now) {
                        (Tag::Added, false) => (Some(c.added), "+"),
                        (Tag::Added, true) => (Some(c.added_now), "+"),
                        (Tag::Removed, false) => (Some(c.removed), "-"),
                        (Tag::Removed, true) => (Some(c.removed_now), "-"),
                        _ => (None, " "),
                    };
                    let shown = expand(&line.text);
                    let chars: Vec<char> = shown.chars().collect();
                    let len = chars.len();
                    let at = Layout {
                        text_x: NUMBER_W * 2.0 + 6.0,
                        prefix: 2,
                        columns: 0,
                        chars: std::sync::Arc::new(chars),
                    };
                    let code_cell = container(shown_text(
                        &format!("{marker} "),
                        &shown,
                        select.span(i, None, len),
                        0,
                        p.text,
                        p.accent.scale_alpha(0.45),
                    ))
                    .height(Length::Fixed(ROW_HEIGHT))
                    .align_y(iced::alignment::Vertical::Center)
                    .padding([0, 6]);
                    let line_cell = select.catch(
                        cell(
                            background,
                            row![
                                number(line.old_no, c.number),
                                number(line.new_no, c.number),
                                code_cell,
                            ]
                            .into(),
                            width,
                        ),
                        i,
                        true,
                        at,
                    );
                    rows.push(with_arrow(line_cell, starts(&blocks, i)));
                }
            }
        }
        rows.push(Space::new().height(after).into());
        flat.len() > MAX_ROWS
    };
    if truncated {
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
    let list = scrollable(column(rows))
        .id(scroll.clone())
        .direction(if side_by_side {
            // Wrapped to the frame: nothing to scroll sideways.
            scrollable::Direction::Vertical(scrollable::Scrollbar::default())
        } else {
            scrollable::Direction::Both {
                vertical: scrollable::Scrollbar::default(),
                horizontal: scrollable::Scrollbar::default(),
            }
        })
        .on_scroll(move |viewport| {
            scrolled(
                viewport.absolute_offset().y as i32,
                viewport.bounds().height as i32,
            )
        })
        .style(se_theme::Scrollbar { dark }.style())
        .width(Length::Fill)
        .height(Length::Fill);
    // Letting go outside a cell still ends the drag.
    pointer_area(list)
        .on_release((select.make)(DiffPointer::Release))
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
    // A commit's diff can be put against the current file instead, where the
    // arrows bring its old lines into the file; and back.
    let toggled = viewer.current().and_then(|e| match &e.source {
        crate::git::Source::Commit(rev) if is_revision(rev) => {
            Some(t!("git.compare_local").to_string())
        }
        crate::git::Source::Tree(rev) if is_revision(rev) => {
            Some(t!("git.show_commit_diff").to_string())
        }
        _ => None,
    });
    let text_button = |label: String, msg: &Option<Message>| -> Element<'a, Message> {
        match msg {
            Some(msg) => button(text(label).size(12))
                .padding([3, 8])
                .on_press(msg.clone())
                .style((se_theme::Outline { dark }).style())
                .into(),
            None => Space::new().into(),
        }
    };
    let compare_button = match toggled {
        Some(label) => text_button(label, &nav.toggle_compare),
        None => Space::new().into(),
    };
    let open_button = if has_file {
        text_button(t!("git.open_file").to_string(), &nav.open_file)
    } else {
        Space::new().into()
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
        compare_button,
        open_button,
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
        iced::widget::tooltip(
            tool_button(
                "▥",
                Some(nav.toggle_side.clone()),
                viewer.side_by_side,
                dark
            ),
            container(
                text(if viewer.side_by_side {
                    t!("git.view_unified").to_string()
                } else {
                    t!("git.view_side").to_string()
                })
                .size(12)
            )
            .padding([3, 8])
            .style(se_theme::tooltip(dark)),
            iced::widget::tooltip::Position::Bottom,
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
        (Some(Ok(diff)), Some(_)) => {
            // Only the working tree's own diff can be rolled back by block.
            let rollback = viewer
                .current()
                .is_some_and(|e| {
                    matches!(
                        e.source,
                        crate::git::Source::Unstaged
                            | crate::git::Source::Staged
                            | crate::git::Source::Tree(_)
                    )
                })
                .then_some(nav.rollback)
                .flatten();
            let select = Sel {
                sel: viewer.selection,
                dragging: viewer.is_dragging(),
                make: nav.pick,
            };
            let args = Body {
                diff,
                current: viewer.change,
                side_by_side: viewer.side_by_side,
                scroll: &viewer.scroll,
                rollback,
                columns: 0,
                select,
                top: viewer.view_top.get(),
                height: viewer.view_height.get(),
                scrolled: nav.scrolled,
                dark,
            };
            if viewer.side_by_side {
                // Both sides share the width, long lines wrap in them.
                let columns = &viewer.columns;
                iced::widget::responsive(move |size| {
                    let cols = columns_for(size.width, rollback.is_some());
                    columns.set(cols);
                    body(Body {
                        columns: cols,
                        ..args
                    })
                })
                .into()
            } else {
                body(args)
            }
        }
        (Some(Err(error)), _) => centered(error.clone(), dark),
        (None, Some(_)) => Space::new().into(),
        _ => centered(empty, dark),
    };

    // Right click: copy what is picked, pick it all. A diff can't be cut
    // from or pasted in, but the entries are shown, greyed.
    let content: Element<Message> = if viewer.file_diff().is_some() {
        let can_copy = viewer.has_selection();
        let (copy, select_all) = (nav.copy.clone(), nav.select_all.clone());
        let key = |letter: &str| {
            if cfg!(target_os = "macos") {
                format!("⌘{letter}")
            } else {
                format!("Ctrl+{letter}")
            }
        };
        let (sh_copy, sh_all) = (key("C"), key("A"));
        crate::pointer_area::context_menu(content, move || {
            use crate::menu_bar::{disabled_item, item};
            let copy_item = if can_copy {
                item(
                    t!("ctx.copy").to_string(),
                    Some(sh_copy.clone()),
                    dark,
                    copy.clone(),
                )
            } else {
                disabled_item(t!("ctx.copy").to_string(), dark)
            };
            let card = container(
                column![
                    disabled_item(t!("ctx.cut").to_string(), dark),
                    copy_item,
                    disabled_item(t!("ctx.paste").to_string(), dark),
                    item(
                        t!("ctx.select_all").to_string(),
                        Some(sh_all.clone()),
                        dark,
                        select_all.clone(),
                    ),
                ]
                .spacing(1)
                .padding(6)
                .width(Length::Fixed(220.0)),
            )
            .style(se_theme::card(dark));
            card.into()
        })
    } else {
        content
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

/// Whether a diff source names a commit (and not a branch or a tag).
fn is_revision(rev: &str) -> bool {
    rev.len() >= 7 && rev.chars().all(|c| c.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::diff::parse_diff;

    fn diff_with_a_long_line() -> FileDiff {
        let long = "x".repeat(200);
        parse_diff(&format!(
            "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1,2 +1,2 @@\n a\n-b\n+{long}\n"
        ))
        .remove(0)
    }

    #[test]
    fn the_sides_share_the_width_and_leave_room_for_the_numbers() {
        // 1200 px: two sides of ~590 px, minus numbers and padding.
        let columns = columns_for(1200.0, true);
        assert!((60..=75).contains(&columns), "{columns}");
        // Narrower frame, fewer characters; never fewer than ten.
        assert!(columns_for(800.0, true) < columns);
        assert_eq!(columns_for(50.0, false), 10);
    }

    #[test]
    fn a_long_line_takes_as_many_lines_as_it_needs() {
        assert_eq!(lines_of("short", 50), 1);
        assert_eq!(lines_of(&"x".repeat(200), 50), 4);
        assert_eq!(lines_of(&"x".repeat(201), 50), 5);
        // Tabs count as four spaces; an empty line is one line.
        assert_eq!(lines_of("\t\t", 4), 2);
        assert_eq!(lines_of("", 50), 1);
    }

    #[test]
    fn a_row_is_as_tall_as_its_longest_side() {
        let diff = diff_with_a_long_line();
        let heights = side_heights(&diff, 50);
        // The hunk header, the context line, then the changed pair.
        assert_eq!(heights, vec![1, 1, 4]);
        // Rows below start after the wrapped ones.
        assert_eq!(side_top(&diff, 2, 50), 2.0 * ROW_HEIGHT);
        assert_eq!(side_top(&diff, 3, 50), 6.0 * ROW_HEIGHT);
        // Without a known width, one line per row.
        assert_eq!(side_top(&diff, 3, 0), 3.0 * ROW_HEIGHT);
    }

    fn viewer_on(text: &str, side_by_side: bool) -> Viewer {
        Viewer {
            diff: Some(Ok(parse_diff(text).remove(0))),
            side_by_side,
            ..Default::default()
        }
    }

    const SMALL: &str =
        "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1,2 +1,2 @@\n \tfoo bar\n-old line\n+new line\n";

    #[test]
    fn dragging_picks_characters_and_copies_them() {
        let mut viewer = viewer_on(SMALL, true);
        // Rows: header, context ("\tfoo bar"), changed pair. Press inside
        // "foo" of the context (after the tab's four columns), drag to the
        // second row's "new".
        viewer.pick(DiffPointer::Press(1, true, 5));
        viewer.pick(DiffPointer::Drag(2, 3));
        viewer.pick(DiffPointer::Release);
        // Moving without the button down changes nothing.
        viewer.pick(DiffPointer::Drag(0, 0));
        assert_eq!(viewer.selected_text().as_deref(), Some("oo bar\nnew"));
        // The left side holds the old lines.
        viewer.pick(DiffPointer::Press(2, false, 4));
        viewer.pick(DiffPointer::Drag(2, 8));
        assert_eq!(viewer.selected_text().as_deref(), Some("line"));
    }

    #[test]
    fn a_tab_is_copied_as_a_tab() {
        let mut viewer = viewer_on(SMALL, true);
        viewer.pick(DiffPointer::Press(1, true, 0));
        viewer.pick(DiffPointer::Drag(1, 7));
        assert_eq!(viewer.selected_text().as_deref(), Some("\tfoo"));
    }

    #[test]
    fn a_double_click_picks_the_word() {
        let mut viewer = viewer_on(SMALL, true);
        viewer.pick(DiffPointer::Press(1, true, 6));
        viewer.pick(DiffPointer::Release);
        viewer.pick(DiffPointer::Press(1, true, 6));
        assert_eq!(viewer.selected_text().as_deref(), Some("foo"));
    }

    #[test]
    fn select_all_picks_every_row_of_both_views() {
        let mut viewer = viewer_on(SMALL, true);
        viewer.select_all();
        assert_eq!(
            viewer.selected_text().as_deref(),
            Some("\tfoo bar\nnew line")
        );
        viewer.side_by_side = false;
        viewer.select_all();
        assert_eq!(
            viewer.selected_text().as_deref(),
            Some("\tfoo bar\nold line\nnew line")
        );
    }

    #[test]
    fn the_pointer_finds_its_character() {
        let at = Layout {
            text_x: 50.0,
            prefix: 0,
            columns: 10,
            chars: std::sync::Arc::new("x".repeat(25).chars().collect()),
        };
        let p = |x: f32, y: f32| Point::new(x, y);
        assert_eq!(char_at(p(50.0, 5.0), &at), 0);
        // Nearest gap: just under half a character still rounds down.
        assert_eq!(char_at(p(50.0 + CHAR_W * 3.4, 5.0), &at), 3);
        assert_eq!(char_at(p(50.0 + CHAR_W * 3.6, 5.0), &at), 4);
        // Left of the text, and past the end of a wrapped line.
        assert_eq!(char_at(p(10.0, 5.0), &at), 0);
        assert_eq!(char_at(p(900.0, 5.0), &at), 10);
        // The second line of a wrapped text starts at `columns`.
        assert_eq!(char_at(p(50.0 + CHAR_W * 2.0, ROW_HEIGHT + 3.0), &at), 12);
        // Never past the end.
        assert_eq!(char_at(p(900.0, ROW_HEIGHT * 5.0), &at), 25);
        // Unified: the "+ " in front doesn't count.
        let unified = Layout {
            columns: 0,
            prefix: 2,
            ..at.clone()
        };
        assert_eq!(char_at(p(50.0 + CHAR_W * 5.0, 5.0), &unified), 3);
        // A wide character spans two cells: "日本語" takes six.
        let wide = Layout {
            text_x: 0.0,
            prefix: 0,
            columns: 0,
            chars: std::sync::Arc::new("日本語x".chars().collect()),
        };
        assert_eq!(char_at(p(CHAR_W * 2.0, 5.0), &wide), 1);
        assert_eq!(char_at(p(CHAR_W * 6.0, 5.0), &wide), 3);
        assert_eq!(char_at(p(CHAR_W * 7.0, 5.0), &wide), 4);
    }

    #[test]
    fn broken_lines_are_as_long_as_the_columns() {
        let chars: Vec<char> = "abcdefgh".chars().collect();
        assert_eq!(broken(&chars, 0..8, 3), "abc\ndef\ngh");
        // A part keeps the breaks of the whole text.
        assert_eq!(broken(&chars, 2..5, 3), "c\nde");
        assert_eq!(broken(&chars, 0..8, 0), "abcdefgh");
        // Wide characters take two cells: three of them don't fit in 5.
        let wide: Vec<char> = "日本語".chars().collect();
        assert_eq!(broken(&wide, 0..3, 5), "日本\n語");
        assert_eq!(lines_of("日本語", 5), 2);
    }

    #[test]
    fn only_the_rows_around_the_view_are_built_and_the_rest_is_space() {
        let heights = vec![ROW_HEIGHT; 1000];
        let total: f32 = heights.iter().sum();
        // At the top: the first screen and the overscan, space below.
        let (first, last, before, after) = visible(&heights, 0.0, 400.0);
        assert_eq!((first, before), (0, 0.0));
        assert!(last > 400 / 18 && last < 100, "{last}");
        assert!((before + after + (last - first) as f32 * ROW_HEIGHT - total).abs() < 0.01);
        // In the middle: space on both sides, the view's rows in between.
        let (first, last, before, after) = visible(&heights, 9000.0, 400.0);
        assert!(first > 0 && last < 1000);
        assert!(before > 0.0 && after > 0.0);
        let shown_from = first as f32 * ROW_HEIGHT;
        let shown_to = last as f32 * ROW_HEIGHT;
        assert!(shown_from <= 9000.0 && shown_to >= 9400.0);
        assert!((before + after + (last - first) as f32 * ROW_HEIGHT - total).abs() < 0.01);
        // Taller rows (wrapped lines) are measured, not counted.
        let tall = vec![ROW_HEIGHT * 3.0; 100];
        let (first, ..) = visible(&tall, 2000.0, 300.0);
        assert!(first > 20 && first < 40, "{first}");
        // A stale offset past the end (the diff got shorter): the last screen.
        let (first, last, _, after) = visible(&heights, 1e9, 400.0);
        assert_eq!(last, 1000);
        assert!(first > 900 && first < last);
        assert_eq!(after, 0.0);
        // No rows, no work.
        assert_eq!(visible(&[], 500.0, 400.0), (0, 0, 0.0, 0.0));
    }

    #[test]
    fn copying_a_big_diff_takes_a_blink_not_minutes() {
        let mut text = String::from("diff --git a/a b/a\n--- a/a\n+++ b/a\n@@ -0,0 +1,5000 @@\n");
        for i in 0..5000 {
            text.push_str(&format!("+line number {i} of a big file\n"));
        }
        let mut viewer = Viewer {
            diff: Some(Ok(parse_diff(&text).remove(0))),
            side_by_side: true,
            ..Default::default()
        };
        let started = std::time::Instant::now();
        viewer.select_all();
        let copied = viewer.selected_text().unwrap();
        assert_eq!(copied.lines().count(), 5000);
        // Quadratic work took tens of seconds here.
        assert!(started.elapsed() < std::time::Duration::from_secs(3));
    }
}
