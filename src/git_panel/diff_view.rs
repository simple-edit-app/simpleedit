//! The diff pane: a toolbar to walk through the changes and the files, and
//! the diff itself, unified or side by side.

use iced::widget::{button, column, container, row, scrollable, text, Space};
use iced::{Alignment, Color, Element, Length, Point};

use super::diff_syntax;
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
/// One button of the column in front of the rows (stage, roll back).
const ARROW_W: f32 = 24.0;

/// How wide that column is for the buttons it holds.
fn arrows_w(rollback: bool, stage: bool) -> f32 {
    ARROW_W * (usize::from(rollback) + usize::from(stage)) as f32
}
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
    /// Stage (or unstage) a block, or the lines picked.
    pub stage_block: fn(usize) -> Message,
    pub stage_selection: Message,
    /// Changes of blanks left out; more or less context.
    pub toggle_blanks: Message,
    pub cycle_context: Message,
    /// Those two are offered (Git reads the diff again for them).
    pub reading_options: bool,
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
            stage_block: |block| Message::Git(GitMsg::StageBlock(block)),
            stage_selection: Message::Git(GitMsg::StageSelection),
            toggle_blanks: Message::Git(GitMsg::ToggleBlanks),
            cycle_context: Message::Git(GitMsg::CycleContext),
            reading_options: true,
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
            stage_block: |block| Message::DiffTab(DiffNav::StageBlock(block)),
            stage_selection: Message::DiffTab(DiffNav::StageSelection),
            toggle_blanks: Message::DiffTab(DiffNav::ToggleBlanks),
            cycle_context: Message::DiffTab(DiffNav::CycleContext),
            reading_options: true,
        }
    }

    /// The diff of the local history: its arrows put a change back.
    pub fn history() -> Nav {
        use crate::app::DiffNav;
        use crate::app::HistoryMsg;
        let on = |nav: DiffNav| Message::History(HistoryMsg::Diff(nav));
        Nav {
            prev_change: on(DiffNav::PrevChange),
            next_change: on(DiffNav::NextChange),
            prev_file: on(DiffNav::PrevFile),
            next_file: on(DiffNav::NextFile),
            toggle_side: on(DiffNav::ToggleSide),
            rollback: Some(|block| Message::History(HistoryMsg::Diff(DiffNav::Apply(block)))),
            toggle_compare: None,
            open_file: None,
            pick: |p| Message::History(HistoryMsg::Diff(DiffNav::Pick(p))),
            copy: on(DiffNav::CopyPicked),
            select_all: on(DiffNav::PickAll),
            scrolled: |top, height| {
                Message::History(HistoryMsg::Diff(DiffNav::Scrolled(top, height)))
            },
            stage_block: |block| Message::History(HistoryMsg::Diff(DiffNav::Apply(block))),
            stage_selection: on(DiffNav::CopyPicked),
            toggle_blanks: on(DiffNav::ToggleBlanks),
            cycle_context: on(DiffNav::CycleContext),
            reading_options: false,
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
    /// The words that changed within a changed line.
    added_word: Color,
    removed_word: Color,
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
            added_word: Color::from_rgba8(0x3f, 0xb9, 0x50, 0.42),
            removed_word: Color::from_rgba8(0xf8, 0x51, 0x49, 0.42),
        }
    } else {
        Colors {
            added: Color::from_rgba8(0x2e, 0xa0, 0x43, 0.16),
            removed: Color::from_rgba8(0xcf, 0x22, 0x2e, 0.14),
            added_now: Color::from_rgba8(0x2e, 0xa0, 0x43, 0.32),
            removed_now: Color::from_rgba8(0xcf, 0x22, 0x2e, 0.30),
            hunk: Color::from_rgba8(0x1b, 0x5f, 0xc6, 0.10),
            number: Color::from_rgb8(0x9a, 0x9f, 0xab),
            added_word: Color::from_rgba8(0x2e, 0xa0, 0x43, 0.38),
            removed_word: Color::from_rgba8(0xcf, 0x22, 0x2e, 0.34),
        }
    }
}

/// Tabs become four spaces so the columns line up.
fn expand(text: &str) -> String {
    text.replace('\t', "    ")
}

/// How many characters fit on one line of a side of the side-by-side view
/// in `width` pixels (a little under, so a wrapped line is never cut short).
fn columns_for(width: f32, middle: f32) -> usize {
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

/// What a diff's drawing needs that costs a walk through all its text.
pub struct DiffLayout {
    /// The height of each row of the side-by-side view (empty in the other).
    pub heights: Vec<usize>,
    /// The longest line of the one-column view, in cells.
    pub longest: usize,
}

/// The cache of a [`Layout`]: for one diff (its revision) and one width.
pub type LayoutCache = std::cell::RefCell<Option<(u64, usize, bool, std::sync::Arc<DiffLayout>)>>;

/// The layout of a diff, made again only when the diff, the width or the
/// view changed (not at each frame).
pub fn layout(
    cache: &LayoutCache,
    rev: u64,
    diff: &FileDiff,
    columns: usize,
    side_by_side: bool,
) -> std::sync::Arc<DiffLayout> {
    if let Some((r, c, s, layout)) = &*cache.borrow() {
        if (*r, *c, *s) == (rev, columns, side_by_side) {
            return layout.clone();
        }
    }
    let layout = std::sync::Arc::new(if side_by_side {
        DiffLayout {
            heights: side_heights(diff, columns),
            longest: 0,
        }
    } else {
        let longest = diff
            .rows()
            .iter()
            .filter_map(|r| match r {
                Row::Line(line) => Some(expand(&line.text).chars().count()),
                _ => None,
            })
            .max()
            .unwrap_or(0);
        DiffLayout {
            heights: Vec::new(),
            longest,
        }
    });
    *cache.borrow_mut() = Some((rev, columns, side_by_side, layout.clone()));
    layout
}

/// Where a row of the side-by-side view starts, from the top.
pub fn side_top(layout: &DiffLayout, row: usize) -> f32 {
    layout.heights.iter().take(row).sum::<usize>() as f32 * ROW_HEIGHT
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

/// Ranges of character indexes.
type Marks = Vec<(usize, usize)>;

/// The characters of `old` and `new` that differ, as ranges of character
/// indexes (none when the two lines are so different that marking all of it
/// would say nothing).
fn word_marks(old: &str, new: &str) -> (Marks, Marks) {
    use similar::{ChangeTag, TextDiff};
    let diff = TextDiff::from_chars(old, new);
    if diff.ratio() < 0.4 {
        return Default::default();
    }
    let (mut old_marks, mut new_marks) = (Vec::new(), Vec::new());
    let (mut i, mut j) = (0usize, 0usize);
    let extend = |marks: &mut Vec<(usize, usize)>, at: usize| match marks.last_mut() {
        Some(last) if last.1 == at => last.1 = at + 1,
        _ => marks.push((at, at + 1)),
    };
    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Equal => {
                i += 1;
                j += 1;
            }
            ChangeTag::Delete => {
                extend(&mut old_marks, i);
                i += 1;
            }
            ChangeTag::Insert => {
                extend(&mut new_marks, j);
                j += 1;
            }
        }
    }
    (old_marks, new_marks)
}

/// For each row of the unified layout, the row of the line it replaces (or
/// that replaces it): the k-th removed line of a block goes with the k-th
/// added one.
fn partners(rows: &[Row<'_>]) -> Vec<Option<usize>> {
    let tag = |i: usize| match &rows[i] {
        Row::Line(l) => Some(l.tag),
        Row::Hunk(_) => None,
    };
    let mut partners = vec![None; rows.len()];
    let mut i = 0;
    while i < rows.len() {
        if tag(i) != Some(Tag::Removed) {
            i += 1;
            continue;
        }
        let removed_end = (i..rows.len())
            .find(|&k| tag(k) != Some(Tag::Removed))
            .unwrap_or(rows.len());
        let added_end = (removed_end..rows.len())
            .find(|&k| tag(k) != Some(Tag::Added))
            .unwrap_or(rows.len());
        for k in 0..(removed_end - i).min(added_end - removed_end) {
            partners[i + k] = Some(removed_end + k);
            partners[removed_end + k] = Some(i + k);
        }
        i = added_end.max(i + 1);
    }
    partners
}

/// A line of the diff, with the picked part highlighted and the words that
/// changed marked.
#[allow(clippy::too_many_arguments)]
fn shown_text<'a>(
    prefix: &str,
    text_value: &str,
    base: &[Option<Color>],
    picked: Option<(usize, usize)>,
    words: &[(usize, usize)],
    columns: usize,
    color: Color,
    highlight: Color,
    word_color: Color,
) -> Element<'a, Message> {
    let chars: Vec<char> = text_value.chars().collect();
    let wrapping = if columns > 0 {
        iced::widget::text::Wrapping::Glyph
    } else {
        iced::widget::text::Wrapping::None
    };
    let line_height = iced::widget::text::LineHeight::Absolute(iced::Pixels(ROW_HEIGHT));
    if picked.is_none() && words.is_empty() {
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
    }
    // Each character is plain, in a changed word, or picked (which wins), and
    // has the color of its syntax: runs of the same look are one span.
    let tint = |i: usize| base.get(i).copied().flatten().unwrap_or(color);
    let look = |i: usize| -> (u8, Color) {
        let kind = if picked.is_some_and(|(a, b)| (a..b).contains(&i)) {
            2
        } else if words.iter().any(|&(a, b)| (a..b).contains(&i)) {
            1
        } else {
            0
        };
        (kind, tint(i))
    };
    let plain = |t: String, c: Color| iced::widget::span(t).font(se_theme::MONO).color(c);
    let mut spans = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        let (kind, tone) = look(start);
        let end = (start..chars.len())
            .find(|&i| look(i) != (kind, tone))
            .unwrap_or(chars.len());
        let head = if start == 0 { prefix } else { "" };
        let piece = plain(
            format!("{head}{}", broken(&chars, start..end, columns)),
            tone,
        );
        spans.push(match kind {
            2 => piece.background(highlight),
            1 => piece.background(word_color),
            _ => piece,
        });
        start = end;
    }
    if spans.is_empty() {
        spans.push(plain(prefix.to_string(), color));
    }
    let rich: iced::widget::text::Rich<'a, (), Message> = iced::widget::rich_text(spans);
    rich.size(FONT)
        .line_height(line_height)
        .wrapping(wrapping)
        .into()
}

/// The button that stages a block (or, `true`, unstages it).
type StageButton = (fn(usize) -> Message, bool);

/// What [`body`] shows and how.
#[derive(Clone)]
struct Body<'a> {
    diff: &'a FileDiff,
    current: usize,
    side_by_side: bool,
    scroll: &'a iced::widget::Id,
    rollback: Option<fn(usize) -> Message>,
    /// Stage a block (or, `true`, unstage it).
    stage: Option<StageButton>,
    /// The syntax colors of the lines.
    syntax: Option<std::sync::Arc<diff_syntax::Spans>>,
    layout: std::sync::Arc<DiffLayout>,
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
        stage,
        syntax,
        layout,
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
    // In front of each row: the buttons of the block it opens (stage, roll back).
    let column_w = arrows_w(rollback.is_some(), stage.is_some());
    let small = |label: &'static str, tip: String, msg: Message| -> Element<'a, Message> {
        iced::widget::tooltip(
            button(text(label).size(13).color(se_theme::palette(dark).muted))
                .padding([0, 4])
                .height(Length::Fixed(ROW_HEIGHT))
                .on_press(msg)
                .style(
                    (se_theme::GhostButton {
                        dark,
                        active: false,
                    })
                    .style(),
                ),
            container(text(tip).size(12))
                .padding([3, 8])
                .style(se_theme::tooltip(dark)),
            iced::widget::tooltip::Position::Top,
        )
        .into()
    };
    let block_buttons = |b: usize| -> Vec<Element<'a, Message>> {
        let mut buttons = Vec::new();
        if let Some((make, undo)) = stage {
            let tip = if undo {
                t!("git.unstage_block")
            } else {
                t!("git.stage_block")
            };
            buttons.push(small(
                if undo { "−" } else { "+" },
                tip.to_string(),
                make(b),
            ));
        }
        if let Some(make) = rollback {
            buttons.push(small("↶", t!("git.rollback_block").to_string(), make(b)));
        }
        buttons
    };
    let with_arrow =
        |element: Element<'a, Message>, block: Option<usize>| -> Element<'a, Message> {
            if column_w == 0.0 {
                return element;
            }
            let buttons: Element<'a, Message> = match block {
                Some(b) => row(block_buttons(b)).into(),
                None => Space::new().width(Length::Fixed(column_w)).into(),
            };
            row![container(buttons).width(Length::Fixed(column_w)), element].into()
        };
    // What the grammar colors in a line, character by character.
    let syntax_of = |line: &crate::git::diff::Line, shown: &str| -> Vec<Option<Color>> {
        syntax
            .as_deref()
            .and_then(|spans| spans.get(line.pos.0)?.get(line.pos.1))
            .map(|spans| diff_syntax::colors_of(&line.text, spans))
            .filter(|colors| colors.len() == shown.chars().count())
            .unwrap_or_default()
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
        let heights = &layout.heights;
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
                    let (left_words, right_words) = match (left, right) {
                        (Some(l), Some(r)) if l.tag == Tag::Removed && r.tag == Tag::Added => {
                            word_marks(&expand(&l.text), &expand(&r.text))
                        }
                        _ => Default::default(),
                    };
                    let side_cell =
                        |line: Option<&'a crate::git::diff::Line>,
                         old: bool,
                         words: Vec<(usize, usize)>| {
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
                            let cell = container(row![
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
                                    &line.map(|l| syntax_of(l, &text_value)).unwrap_or_default(),
                                    picked,
                                    &words,
                                    columns,
                                    p.text,
                                    p.accent.scale_alpha(0.45),
                                    if old { c.removed_word } else { c.added_word },
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
                    // The buttons of a change sit between the sides: stage it, or
                    // carry it across.
                    let middle: Element<'a, Message> = match starts(&blocks, i) {
                        Some(b) if column_w > 0.0 => {
                            let mut buttons = Vec::new();
                            if let Some((make, undo)) = stage {
                                let tip = if undo {
                                    t!("git.unstage_block")
                                } else {
                                    t!("git.stage_block")
                                };
                                buttons.push(small(
                                    if undo { "−" } else { "+" },
                                    tip.to_string(),
                                    make(b),
                                ));
                            }
                            if let Some(make) = rollback {
                                buttons.push(small("»", t!("git.apply_here").to_string(), make(b)));
                            }
                            row(buttons).into()
                        }
                        _ => Space::new().into(),
                    };
                    let middle = container(middle).width(Length::Fixed(column_w));
                    rows.push(
                        row![
                            side_cell(*left, true, left_words),
                            middle,
                            side_cell(*right, false, right_words)
                        ]
                        .into(),
                    );
                }
            }
        }
        rows.push(Space::new().height(after).into());
        side.len() > MAX_ROWS
    } else {
        let flat = diff.rows();
        let blocks = diff.row_blocks();
        let width = NUMBER_W * 2.0 + 16.0 + layout.longest.max(40) as f32 * CHAR_W + 24.0;
        let partners = partners(&flat);
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
                    // The words that changed against the line it replaces.
                    let words = match partners[i].map(|j| flat[j].clone()) {
                        Some(Row::Line(other)) => {
                            let other = expand(&other.text);
                            let (old, new) = if line.tag == Tag::Removed {
                                (shown.as_str(), other.as_str())
                            } else {
                                (other.as_str(), shown.as_str())
                            };
                            let (old_words, new_words) = word_marks(old, new);
                            if line.tag == Tag::Removed {
                                old_words
                            } else {
                                new_words
                            }
                        }
                        _ => Vec::new(),
                    };
                    let code_cell = container(shown_text(
                        &format!("{marker} "),
                        &shown,
                        &syntax_of(line, &shown),
                        select.span(i, None, len),
                        &words,
                        0,
                        p.text,
                        p.accent.scale_alpha(0.45),
                        if line.tag == Tag::Removed {
                            c.removed_word
                        } else {
                            c.added_word
                        },
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
    // Blanks and context: only where Git reads the diff again for them.
    let reading: Element<Message> = if nav.reading_options {
        row![
            iced::widget::tooltip(
                tool_button(
                    "␣",
                    Some(nav.toggle_blanks.clone()),
                    viewer.ignore_blanks,
                    dark
                ),
                container(
                    text(if viewer.ignore_blanks {
                        t!("git.blanks_counted").to_string()
                    } else {
                        t!("git.blanks_ignored").to_string()
                    })
                    .size(12)
                )
                .padding([3, 8])
                .style(se_theme::tooltip(dark)),
                iced::widget::tooltip::Position::Bottom,
            ),
            iced::widget::tooltip(
                tool_button(
                    "⇕",
                    Some(nav.cycle_context.clone()),
                    viewer.context != 3,
                    dark
                ),
                container(
                    text(if viewer.context == u32::MAX {
                        t!("git.context_all").to_string()
                    } else {
                        t!("git.context_n", n = viewer.context).to_string()
                    })
                    .size(12)
                )
                .padding([3, 8])
                .style(se_theme::tooltip(dark)),
                iced::widget::tooltip::Position::Bottom,
            ),
        ]
        .spacing(4)
        .align_y(Alignment::Center)
        .into()
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
        reading,
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
            // Stage by block: the working tree's diff (to the index), or the
            // index's (out of it), as read now (a patch needs the diff as is).
            let stage = viewer
                .current()
                .and_then(|e| match e.source {
                    crate::git::Source::Unstaged => Some(false),
                    crate::git::Source::Staged => Some(true),
                    _ => None,
                })
                .filter(|_| !viewer.ignore_blanks)
                .map(|undo| (nav.stage_block, undo));
            let syntax = viewer.syntax_spans(dark);
            let layout_cache = &viewer.layout;
            let rev = viewer.diff_rev();
            let args = Body {
                layout: layout(layout_cache, rev, diff, 0, false),
                diff,
                current: viewer.change,
                side_by_side: viewer.side_by_side,
                scroll: &viewer.scroll,
                rollback,
                stage,
                syntax,
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
                    let cols =
                        columns_for(size.width, arrows_w(rollback.is_some(), stage.is_some()));
                    columns.set(cols);
                    body(Body {
                        layout: layout(layout_cache, rev, diff, cols, true),
                        columns: cols,
                        ..args.clone()
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
        // Stage (or unstage) the changed lines picked.
        let stage_lines = Some(viewer)
            .filter(|v| v.has_selection())
            .and_then(|v| v.index_patch(None))
            .map(|(_, undo)| (undo, nav.stage_selection.clone()));
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
            let stage_item = match &stage_lines {
                Some((undo, msg)) => item(
                    if *undo {
                        t!("git.unstage_lines").to_string()
                    } else {
                        t!("git.stage_lines").to_string()
                    },
                    None,
                    dark,
                    msg.clone(),
                ),
                None => disabled_item(t!("git.stage_lines").to_string(), dark),
            };
            let card = container(
                column![
                    stage_item,
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
        let columns = columns_for(1200.0, ARROW_W);
        assert!((60..=75).contains(&columns), "{columns}");
        // Narrower frame, fewer characters; never fewer than ten.
        assert!(columns_for(800.0, ARROW_W) < columns);
        assert_eq!(columns_for(50.0, 0.0), 10);
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
        let layout = |columns| DiffLayout {
            heights: side_heights(&diff, columns),
            longest: 0,
        };
        assert_eq!(side_top(&layout(50), 2), 2.0 * ROW_HEIGHT);
        assert_eq!(side_top(&layout(50), 3), 6.0 * ROW_HEIGHT);
        // Without a known width, one line per row.
        assert_eq!(side_top(&layout(0), 3), 3.0 * ROW_HEIGHT);
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

    #[test]
    fn the_words_that_changed_in_a_line_are_marked_in_both() {
        let (old, new) = word_marks("let total = price * 2;", "let total = price * 3;");
        assert_eq!(old, vec![(20, 21)]);
        assert_eq!(new, vec![(20, 21)]);
        // An insertion in the middle.
        let (old, new) = word_marks("hello world", "hello brave world");
        assert!(old.is_empty());
        assert_eq!(new, vec![(6, 12)]);
        // Lines with little in common: nothing marked (all of it would be).
        let (old, new) = word_marks("completely different", "xyzzy 12345 !!");
        assert!(old.is_empty() && new.is_empty());
    }

    #[test]
    fn a_removed_line_goes_with_the_added_line_in_its_place() {
        let text = "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1,4 +1,5 @@\n keep\n-old one\n-old two\n+new one\n+new two\n+new three\n tail\n";
        let file = parse_diff(text).remove(0);
        let rows = file.rows();
        let partners = partners(&rows);
        // rows: hunk, keep, -one, -two, +one, +two, +three, tail
        assert_eq!(partners[2], Some(4));
        assert_eq!(partners[4], Some(2));
        assert_eq!(partners[3], Some(5));
        assert_eq!(partners[6], None, "the extra added line has none");
        assert_eq!(partners[1], None);
    }

    #[test]
    fn staging_is_offered_by_the_source_and_not_when_blanks_are_left_out() {
        use crate::git::Source;
        let text = "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1,2 +1,2 @@\n keep\n-old\n+new\n";
        let entry = |source| crate::git_panel::Entry {
            source,
            change: crate::git::Change {
                path: "f".into(),
                from: None,
                kind: crate::git::Kind::Modified,
            },
            staged: false,
        };
        let mut viewer = Viewer {
            diff: Some(Ok(parse_diff(text).remove(0))),
            entries: vec![entry(Source::Unstaged)],
            index: Some(0),
            ..Default::default()
        };
        let (patch, undo) = viewer.index_patch(Some(0)).unwrap();
        assert!(!undo && patch.contains("-old") && patch.contains("+new"));
        // Lines: nothing picked, nothing to stage.
        assert!(viewer.index_patch(None).is_none());
        viewer.pick(DiffPointer::Press(1, true, 0));
        viewer.pick(DiffPointer::Drag(2, 3));
        assert!(viewer.index_patch(None).is_some());
        // The staged diff takes it out, the others don't do it at all.
        viewer.entries[0].source = Source::Staged;
        assert!(viewer.index_patch(Some(0)).unwrap().1);
        viewer.entries[0].source = Source::Commit("abc".into());
        assert!(viewer.index_patch(Some(0)).is_none());
        // Blanks left out: the patch would not fit the index.
        viewer.entries[0].source = Source::Unstaged;
        viewer.ignore_blanks = true;
        assert!(viewer.index_patch(Some(0)).is_none());
    }

    #[test]
    fn the_context_goes_through_its_steps() {
        let mut viewer = Viewer::default();
        let steps: Vec<u32> = (0..5)
            .map(|_| {
                let at = viewer.context;
                viewer.cycle_context();
                at
            })
            .collect();
        assert_eq!(steps, vec![3, 10, 30, u32::MAX, 3]);
    }
}
