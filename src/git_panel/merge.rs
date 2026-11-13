//! Settling a conflict by hand: three columns — what the user has, the
//! result, what comes from elsewhere — and, for each conflicting chunk, the
//! arrows that take a side into the result or leave it out.

use std::path::PathBuf;

use std::time::{Duration, Instant};

use iced::widget::{button, column, container, row, scrollable, text, text_editor, Space};
use iced::{Alignment, Color, Element, Length};

use crate::app::Message;
use crate::git::conflicts::{Chunk, Merge, Pick, Segment, Side};
use crate::theme as se_theme;

const ROW_H: f32 = 18.0;
const FONT: f32 = 12.0;
const CONTEXT: usize = 3;

/// A conflicted file open in an editor tab.
pub struct MergeTab {
    pub repo: PathBuf,
    /// Relative to the repository.
    pub path: String,
    pub merge: Merge,
    pub scroll: iced::widget::Id,
    /// The conflict the arrows are on.
    pub current: usize,
    /// The result of each conflict, as an editor to type in.
    contents: Vec<text_editor::Content>,
    /// What came before, to go back to.
    past: Vec<Merge>,
    future: Vec<Merge>,
    /// When the last typing was, so a burst of keys is one step.
    typed_at: Option<Instant>,
}

impl MergeTab {
    /// Whether conflicts were settled or edited and not yet applied.
    pub fn is_modified(&self) -> bool {
        !self.past.is_empty()
    }
}

#[derive(Debug, Clone)]
pub enum MergeMsg {
    /// Put a side of conflict `n` into the result.
    Take(usize, Side),
    /// Leave a side of conflict `n` out.
    Ignore(usize, Side),
    /// Every conflict: this side in, the other out.
    All(Side),
    /// Settle the conflicts that need no choice.
    Auto,
    /// Typing in the result of conflict `n`.
    Edit(usize, text_editor::Action),
    Undo,
    Redo,
    Next,
    Prev,
    Apply,
}

/// What the view shows, top to bottom.
enum Item<'a> {
    Line(&'a str),
    /// Identical lines folded away.
    Gap(usize),
    Conflict(usize, &'a Chunk),
}

fn items(merge: &Merge) -> Vec<Item<'_>> {
    let mut items = Vec::new();
    let mut n = 0;
    for segment in &merge.segments {
        match segment {
            Segment::Text(lines) if lines.len() > 2 * CONTEXT + 1 => {
                items.extend(lines[..CONTEXT].iter().map(|l| Item::Line(l)));
                items.push(Item::Gap(lines.len() - 2 * CONTEXT));
                items.extend(lines[lines.len() - CONTEXT..].iter().map(|l| Item::Line(l)));
            }
            Segment::Text(lines) => items.extend(lines.iter().map(|l| Item::Line(l))),
            Segment::Conflict(chunk) => {
                items.push(Item::Conflict(n, chunk));
                n += 1;
            }
        }
    }
    items
}

/// How many rows a conflict takes (at least the result's, which is typed in).
fn chunk_rows(chunk: &Chunk) -> usize {
    chunk
        .yours
        .len()
        .max(chunk.theirs.len())
        .max(chunk.result().len())
        .max(1)
}

/// The text of a result editor, as lines.
fn lines_of(content: &text_editor::Content) -> Vec<String> {
    let text = content.text();
    let text = text.strip_suffix('\n').unwrap_or(&text);
    if text.is_empty() {
        Vec::new()
    } else {
        text.split('\n').map(str::to_string).collect()
    }
}

impl MergeTab {
    pub fn new(repo: PathBuf, path: String, merge: Merge) -> Self {
        let mut tab = MergeTab {
            repo,
            path,
            merge,
            scroll: iced::widget::Id::unique(),
            current: 0,
            contents: Vec::new(),
            past: Vec::new(),
            future: Vec::new(),
            typed_at: None,
        };
        tab.sync_contents();
        tab
    }

    /// The editors show what the conflicts now give.
    fn sync_contents(&mut self) {
        self.contents = self
            .merge
            .conflicts()
            .map(|c| {
                if c.resolved() {
                    text_editor::Content::with_text(&c.result().join("\n"))
                } else {
                    text_editor::Content::new()
                }
            })
            .collect();
    }

    /// Remembers the state before a change.
    fn remember(&mut self) {
        self.past.push(self.merge.clone());
        if self.past.len() > 200 {
            self.past.remove(0);
        }
        self.future.clear();
        self.typed_at = None;
    }

    pub fn can_undo(&self) -> bool {
        !self.past.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }

    /// The row where conflict `n` starts.
    fn row_of(&self, n: usize) -> usize {
        let mut rows = 0;
        for item in items(&self.merge) {
            match item {
                Item::Conflict(i, chunk) => {
                    if i == n {
                        return rows;
                    }
                    // Its arrows take a row of their own.
                    rows += chunk_rows(chunk) + 1;
                }
                _ => rows += 1,
            }
        }
        rows
    }

    pub fn scroll_to_current(&self) -> iced::Task<Message> {
        let y = (self.row_of(self.current).saturating_sub(3)) as f32 * ROW_H;
        iced::widget::operation::scroll_to(
            self.scroll.clone(),
            iced::widget::scrollable::AbsoluteOffset { x: 0.0, y },
        )
    }

    pub fn update(&mut self, msg: MergeMsg) -> iced::Task<Message> {
        let count = self.merge.conflicts().count();
        match msg {
            MergeMsg::Take(n, side) => {
                self.remember();
                if let Some(chunk) = self.merge.conflicts_mut().nth(n) {
                    chunk.clear_text();
                    chunk.set(side, Pick::Taken);
                }
                self.current = n;
                self.sync_contents();
            }
            MergeMsg::Ignore(n, side) => {
                self.remember();
                if let Some(chunk) = self.merge.conflicts_mut().nth(n) {
                    chunk.clear_text();
                    chunk.set(side, Pick::Ignored);
                }
                self.current = n;
                self.sync_contents();
            }
            MergeMsg::All(side) => {
                self.remember();
                let other = if side == Side::Yours {
                    Side::Theirs
                } else {
                    Side::Yours
                };
                for chunk in self.merge.conflicts_mut() {
                    chunk.clear_text();
                    chunk.set(side, Pick::Taken);
                    chunk.set(other, Pick::Ignored);
                }
                self.sync_contents();
            }
            MergeMsg::Auto => {
                self.remember();
                self.merge.auto_resolve();
                self.sync_contents();
            }
            MergeMsg::Edit(n, action) => {
                if action.is_edit() {
                    // A burst of keys is one step to go back over.
                    let burst = self
                        .typed_at
                        .is_some_and(|t| t.elapsed() < Duration::from_millis(1200));
                    if !burst {
                        self.remember();
                    }
                    self.typed_at = Some(Instant::now());
                }
                self.current = n;
                if let (Some(content), true) = (self.contents.get_mut(n), true) {
                    content.perform(action.clone());
                    if action.is_edit() {
                        let lines = lines_of(content);
                        if let Some(chunk) = self.merge.conflicts_mut().nth(n) {
                            chunk.set_text(lines);
                        }
                    }
                }
            }
            MergeMsg::Undo => {
                if let Some(before) = self.past.pop() {
                    self.future.push(std::mem::replace(&mut self.merge, before));
                    self.typed_at = None;
                    self.sync_contents();
                }
            }
            MergeMsg::Redo => {
                if let Some(after) = self.future.pop() {
                    self.past.push(std::mem::replace(&mut self.merge, after));
                    self.typed_at = None;
                    self.sync_contents();
                }
            }
            MergeMsg::Next => {
                if count > 0 {
                    self.current = (self.current + 1).min(count - 1);
                }
                return self.scroll_to_current();
            }
            MergeMsg::Prev => {
                self.current = self.current.saturating_sub(1);
                return self.scroll_to_current();
            }
            MergeMsg::Apply => {}
        }
        iced::Task::none()
    }

    // ─── The view ───────────────────────────────────────────────────────────

    pub fn view(&self, dark: bool) -> Element<'_, Message> {
        let p = se_theme::palette(dark);
        let unresolved = self.merge.unresolved();
        let toolbar = row![
            small("↑", Some(MergeMsg::Prev), dark),
            small("↓", Some(MergeMsg::Next), dark),
            Space::new().width(6),
            small("↶", self.can_undo().then_some(MergeMsg::Undo), dark),
            small("↷", self.can_redo().then_some(MergeMsg::Redo), dark),
            Space::new().width(10),
            text(t!("git.conflicts_left", n = unresolved).to_string())
                .size(12)
                .color(if unresolved == 0 { p.accent } else { p.muted }),
            Space::new().width(Length::Fill),
            labelled(t!("git.auto_resolve").to_string(), MergeMsg::Auto, dark),
            labelled(
                t!("git.take_all_yours").to_string(),
                MergeMsg::All(Side::Yours),
                dark
            ),
            labelled(
                t!("git.take_all_theirs").to_string(),
                MergeMsg::All(Side::Theirs),
                dark
            ),
            {
                let mut apply = button(text(t!("git.apply").to_string()).size(12.5))
                    .padding([4, 14])
                    .style((se_theme::Primary { dark }).style());
                if unresolved == 0 {
                    apply = apply.on_press(Message::Merge(MergeMsg::Apply));
                }
                apply
            },
        ]
        .spacing(6)
        .align_y(Alignment::Center);

        let heads = row![
            heading(t!("git.yours").to_string(), dark),
            heading(t!("git.result").to_string(), dark),
            heading(t!("git.theirs").to_string(), dark),
        ];

        let mut body = column![];
        for item in items(&self.merge) {
            body = body.push(match item {
                Item::Line(line) => self.triple_line(line, dark),
                Item::Gap(n) => container(
                    text(t!("git.folded_lines", n = n).to_string())
                        .size(FONT - 1.0)
                        .color(p.muted),
                )
                .height(Length::Fixed(ROW_H))
                .center_x(Length::Fill)
                .into(),
                Item::Conflict(n, chunk) => self.conflict_group(n, chunk, dark),
            });
        }

        column![
            container(toolbar).padding([6, 10]),
            container(heads).padding([0, 4]),
            container(Space::new().height(1))
                .width(Length::Fill)
                .style(se_theme::rule(dark)),
            scrollable(body)
                .id(self.scroll.clone())
                .style(se_theme::Scrollbar { dark }.style())
                .width(Length::Fill)
                .height(Length::Fill),
        ]
        .spacing(2)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    /// A line the three columns share.
    fn triple_line<'a>(&self, line: &'a str, dark: bool) -> Element<'a, Message> {
        let cell = || column_of(vec![code_line(line, None, dark)]);
        row![cell(), cell(), cell()].into()
    }

    /// One conflict: its side, its result (an editor), its other side.
    fn conflict_group<'a>(
        &'a self,
        n: usize,
        chunk: &'a Chunk,
        dark: bool,
    ) -> Element<'a, Message> {
        let current = n == self.current;
        let typed = self.contents.get(n).map_or(0, |c| c.line_count());
        let rows = chunk_rows(chunk).max(typed);
        let alpha = if current { 0.30 } else { 0.18 };
        let dim = |c: Color, pick: Pick| Color {
            a: if pick == Pick::Pending {
                c.a
            } else {
                c.a * 0.5
            },
            ..c
        };
        let (mine, theirs, result) = (
            Color::from_rgba8(0x6e, 0xa8, 0xff, alpha),
            Color::from_rgba8(0xe6, 0xc0, 0x6a, alpha),
            Color::from_rgba8(0x3f, 0xb9, 0x50, alpha),
        );
        let side_col = |lines: &'a [String], side: Side, color: Color| -> Element<'a, Message> {
            let pick = chunk.pick(side);
            let arrow = if side == Side::Yours { "»" } else { "«" };
            let mut cells: Vec<Element<'a, Message>> = lines
                .iter()
                .map(|l| code_line(l, Some(dim(color, pick)), dark))
                .collect();
            while cells.len() < rows {
                cells.push(code_line("", Some(dim(color, pick)), dark));
            }
            let controls = container(row![
                small_text(
                    arrow,
                    (pick != Pick::Taken).then_some(MergeMsg::Take(n, side)),
                    dark
                ),
                small_text(
                    "✕",
                    (pick == Pick::Pending).then_some(MergeMsg::Ignore(n, side)),
                    dark
                ),
            ])
            .height(Length::Fixed(ROW_H));
            column![controls, column(cells)]
                .width(Length::FillPortion(1))
                .into()
        };
        // The result is typed in like any text.
        let p = se_theme::palette(dark);
        let editor = text_editor(&self.contents[n])
            .placeholder(t!("git.unresolved").to_string())
            .on_action(move |action| Message::Merge(MergeMsg::Edit(n, action)))
            .font(se_theme::MONO)
            .size(FONT)
            .line_height(iced::widget::text::LineHeight::Absolute(iced::Pixels(
                ROW_H,
            )))
            .padding([0, 6])
            .wrapping(iced::widget::text::Wrapping::None)
            .height(Length::Fixed(rows as f32 * ROW_H))
            .style(move |_: &iced::Theme, _| text_editor::Style {
                background: iced::Background::Color(result),
                border: iced::Border::default(),
                placeholder: p.muted,
                value: p.text,
                selection: Color {
                    a: 0.35,
                    ..p.accent
                },
            });
        row![
            side_col(&chunk.yours, Side::Yours, mine),
            column![Space::new().height(ROW_H), editor].width(Length::FillPortion(1)),
            side_col(&chunk.theirs, Side::Theirs, theirs),
        ]
        .into()
    }
}

fn column_of<'a>(items: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    container(column(items))
        .width(Length::FillPortion(1))
        .into()
}

fn code_line<'a>(line: &str, background: Option<Color>, dark: bool) -> Element<'a, Message> {
    let p = se_theme::palette(dark);
    container(
        text(line.replace('\t', "    "))
            .size(FONT)
            .font(se_theme::MONO)
            .color(p.text)
            .wrapping(iced::widget::text::Wrapping::None),
    )
    .width(Length::Fill)
    .height(Length::Fixed(ROW_H))
    .padding([0, 6])
    .clip(true)
    .style(move |_: &iced::Theme| iced::widget::container::Style {
        background: background.map(iced::Background::Color),
        ..Default::default()
    })
    .into()
}

fn heading<'a>(label: String, dark: bool) -> Element<'a, Message> {
    container(
        text(label)
            .size(12)
            .font(se_theme::UI_SEMIBOLD)
            .color(se_theme::muted_text(dark)),
    )
    .width(Length::FillPortion(1))
    .padding([2, 8])
    .into()
}

fn small<'a>(label: &'a str, msg: Option<MergeMsg>, dark: bool) -> Element<'a, Message> {
    small_text(label, msg, dark)
}

fn small_text<'a>(label: &'a str, msg: Option<MergeMsg>, dark: bool) -> Element<'a, Message> {
    let mut b = button(text(label).size(12).font(se_theme::MONO))
        .padding([0, 6])
        .height(Length::Fixed(ROW_H));
    if let Some(m) = msg {
        b = b.on_press(Message::Merge(m));
    }
    b.style(
        (se_theme::GhostButton {
            dark,
            active: false,
        })
        .style(),
    )
    .into()
}

fn labelled<'a>(label: String, msg: MergeMsg, dark: bool) -> Element<'a, Message> {
    button(text(label).size(12.5))
        .padding([4, 10])
        .on_press(Message::Merge(msg))
        .style(
            (se_theme::GhostButton {
                dark,
                active: false,
            })
            .style(),
        )
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::conflicts::parse;

    fn tab() -> MergeTab {
        let text = "a\n<<<<<<< HEAD\nmine\n=======\ntheirs\n>>>>>>> t\nb\n<<<<<<< HEAD\nx\n=======\ny\n>>>>>>> t\nc\n";
        MergeTab::new(
            PathBuf::from("/r"),
            "f.txt".into(),
            parse(text, false).unwrap(),
        )
    }

    #[test]
    fn taking_and_ignoring_settle_a_conflict_side_by_side() {
        let mut t = tab();
        let _ = t.update(MergeMsg::Take(0, Side::Yours));
        assert!(!t.merge.conflicts().next().unwrap().resolved());
        let _ = t.update(MergeMsg::Ignore(0, Side::Theirs));
        assert!(t.merge.conflicts().next().unwrap().resolved());
        assert_eq!(t.merge.unresolved(), 1);
        // Taking both keeps yours then theirs.
        let _ = t.update(MergeMsg::Take(1, Side::Theirs));
        let _ = t.update(MergeMsg::Take(1, Side::Yours));
        assert_eq!(t.merge.unresolved(), 0);
        assert_eq!(t.merge.render(), "a\nmine\nb\nx\ny\nc\n");
    }

    #[test]
    fn one_click_settles_every_conflict_for_a_side() {
        let mut t = tab();
        let _ = t.update(MergeMsg::All(Side::Theirs));
        assert_eq!(t.merge.render(), "a\ntheirs\nb\ny\nc\n");
    }

    #[test]
    fn the_result_can_be_typed_and_undone_and_redone() {
        let mut t = tab();
        let _ = t.update(MergeMsg::Take(0, Side::Yours));
        let _ = t.update(MergeMsg::Ignore(0, Side::Theirs));
        let _ = t.update(MergeMsg::Edit(
            0,
            text_editor::Action::Edit(text_editor::Edit::Insert('!')),
        ));
        assert_eq!(t.merge.render().lines().nth(1), Some("!mine"));
        assert!(t.can_undo());
        let _ = t.update(MergeMsg::Undo);
        assert_eq!(t.merge.render().lines().nth(1), Some("mine"));
        assert!(t.can_redo());
        let _ = t.update(MergeMsg::Redo);
        assert_eq!(t.merge.render().lines().nth(1), Some("!mine"));
    }

    #[test]
    fn undo_brings_back_a_choice() {
        let mut t = tab();
        let _ = t.update(MergeMsg::All(Side::Theirs));
        assert_eq!(t.merge.unresolved(), 0);
        let _ = t.update(MergeMsg::Undo);
        assert_eq!(t.merge.unresolved(), 2);
    }

    #[test]
    fn simple_conflicts_settle_by_themselves() {
        // Only one side moved from the base: it wins without asking.
        let text = "a\n<<<<<<< HEAD\nmine\n||||||| base\nold\n=======\nold\n>>>>>>> t\nb\n";
        let mut t = MergeTab::new(PathBuf::new(), "f".into(), parse(text, false).unwrap());
        let _ = t.update(MergeMsg::Auto);
        assert_eq!(t.merge.unresolved(), 0);
        assert_eq!(t.merge.render(), "a\nmine\nb\n");
    }

    #[test]
    fn the_arrows_stay_inside_the_conflicts() {
        let mut t = tab();
        let _ = t.update(MergeMsg::Next);
        assert_eq!(t.current, 1);
        let _ = t.update(MergeMsg::Next);
        assert_eq!(t.current, 1);
        let _ = t.update(MergeMsg::Prev);
        let _ = t.update(MergeMsg::Prev);
        assert_eq!(t.current, 0);
    }

    #[test]
    fn long_common_text_is_folded_around_the_conflicts() {
        let many: Vec<String> = (0..30).map(|i| format!("line {i}")).collect();
        let text = format!(
            "{}\n<<<<<<< HEAD\nm\n=======\nt\n>>>>>>> t\n",
            many.join("\n")
        );
        let t = MergeTab::new(PathBuf::new(), "f".into(), parse(&text, false).unwrap());
        let shown = items(&t.merge);
        // 3 lines, a fold, 3 lines, the conflict.
        assert_eq!(shown.len(), 8);
        assert!(matches!(shown[3], Item::Gap(24)));
        assert_eq!(t.row_of(0), 7);
    }
}
