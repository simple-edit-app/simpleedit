//! A multi-line text editor with a line-number gutter.
//!
//! Adapted from `iced_widget::text_editor` (iced 0.12.3, MIT), which keeps its
//! scroll position and wrapped-line layout private. A gutter that stays
//! aligned needs both — a long line that wraps takes several rows but has one
//! number, and scrolling moves everything — so this copy owns the `Content`
//! and reads the row layout straight from the renderer's text buffer.
//!
//! Differences from the original:
//! - fixed to `iced::Renderer`, whose editor exposes its cosmic-text buffer;
//! - an optional [`Gutter`] drawn in the left padding, from the buffer's layout;
//! - a settable text size;
//! - clicks and drags are offset by the left padding (the original swaps the
//!   two padding components).

use iced::advanced::clipboard::{self, Clipboard};
use iced::advanced::graphics::text::cosmic_text::Cursor as Cursor_;
use iced::advanced::layout::{self, Layout};
use iced::advanced::mouse;
use iced::advanced::renderer::{self, Renderer as _};
use iced::advanced::text::editor::{Cursor, Editor as _};
use iced::advanced::text::highlighter::{self, Highlighter};
use iced::advanced::text::{self, LineHeight, Renderer as TextRenderer};
use iced::advanced::widget::{self, tree};
use iced::advanced::Shell;
use iced::alignment;
use iced::event::{self, Event};
use iced::keyboard::{self, key};
use iced::{Color, Element, Font, Length, Padding, Pixels, Point, Rectangle, Size, Vector};

use std::cell::RefCell;
use std::ops::DerefMut;
use std::ops::Range;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::sync::Mutex;

pub use iced::widget::text_editor::{Action, Edit, Motion, StyleSheet};

type Editor = <iced::Renderer as TextRenderer>::Editor;

/// The line-number column of a [`CodeEditor`].
#[derive(Debug, Clone, Copy)]
pub struct Gutter {
    /// Color of the numbers.
    pub text: Color,
    /// Color of the current line's number.
    pub current: Color,
}

/// Wash over the cursor's line, with an accent bar on its left edge.
#[derive(Debug, Clone, Copy)]
pub struct LineHighlight {
    pub background: Color,
    pub bar: Color,
}

/// What to paint for an active search: a background behind every match, and
/// a stronger one behind the current match.
#[derive(Debug, Clone)]
pub struct SearchPaint {
    pub query: String,
    pub case_sensitive: bool,
    pub use_regex: bool,
    /// (line, byte range within the line) of the current match.
    pub current: Option<(usize, Range<usize>)>,
    pub color: Color,
    pub current_color: Color,
    pub current_border: Color,
}

/// Colors of the minimap: a miniature of the whole file along the right edge.
#[derive(Debug, Clone, Copy)]
pub struct Minimap {
    /// The miniature lines.
    pub bar: Color,
    /// Wash over the part of the file on screen.
    pub viewport: Color,
    /// Outline of that part.
    pub viewport_border: Color,
    /// Marker for search matches.
    pub marker: Color,
}

/// Width of the minimap.
pub const MINIMAP_WIDTH: f32 = 84.0;
/// Space above and below the miniature.
const MINIMAP_INSET: f32 = 8.0;
/// Width of a miniature character.
const MINIMAP_CHAR: f32 = 0.8;
/// Beyond this many lines the search markers aren't computed (they would
/// cost a scan of the whole file on every redraw).
const MINIMAP_MAX_MARKED_LINES: usize = 20_000;

/// Height of one line of the miniature: 2px, less when the file is too
/// tall to fit.
fn minimap_scale(lines: usize, height: f32) -> f32 {
    let room = (height - 2.0 * MINIMAP_INSET).max(1.0);
    (room / lines.max(1) as f32).min(2.0)
}

/// The line under `y` (relative to the minimap's top).
fn minimap_line_at(y: f32, scale: f32, lines: usize) -> usize {
    (((y - MINIMAP_INSET) / scale).max(0.0) as usize).min(lines.saturating_sub(1))
}

/// Where a line's miniature starts and how long it is, in characters:
/// indentation counts for its width, trailing space doesn't. `None` for a
/// blank line.
fn minimap_span(line: &str) -> Option<(usize, usize)> {
    let mut start = 0;
    for c in line.chars() {
        match c {
            ' ' => start += 1,
            '\t' => start += 4,
            _ => break,
        }
    }
    let body = line.trim();
    (!body.is_empty()).then(|| (start, body.chars().count()))
}

/// Space between the gutter and the text.
const GUTTER_GAP: f32 = 6.0;

/// Width of the gutter for a buffer of `lines` lines at text size `size`.
fn gutter_width(size: f32, lines: usize) -> f32 {
    let digits = lines.max(1).to_string().len().max(2);
    (digits as f32 * size * 0.6 + 26.0).max(46.0)
}

/// The minimap's strip along the right edge of the widget.
fn minimap_rect(bounds: Rectangle) -> Rectangle {
    Rectangle {
        x: bounds.x + bounds.width - MINIMAP_WIDTH,
        y: bounds.y,
        width: MINIMAP_WIDTH,
        height: bounds.height,
    }
}

/// Byte ranges of the matches of `paint` in one line of text.
fn line_matches(
    line: &str,
    paint: &SearchPaint,
    regex: Option<&regex::Regex>,
) -> Vec<Range<usize>> {
    if paint.query.is_empty() {
        return Vec::new();
    }
    if paint.use_regex {
        regex
            .map(|re| {
                re.find_iter(line)
                    .filter(|m| !m.is_empty())
                    .map(|m| m.range())
                    .collect()
            })
            .unwrap_or_default()
    } else if paint.case_sensitive {
        line.match_indices(paint.query.as_str())
            .map(|(i, m)| i..i + m.len())
            .collect()
    } else {
        crate::search::find_literal_ci(line, &paint.query)
    }
}

/// A multi-line text input.
#[allow(missing_debug_implementations)]
pub struct CodeEditor<'a, Highlighter, Message, Theme = iced::Theme>
where
    Highlighter: text::Highlighter,
    Theme: StyleSheet,
{
    content: &'a Content,
    font: Option<Font>,
    text_size: Option<Pixels>,
    line_height: LineHeight,
    width: Length,
    height: Length,
    padding: Padding,
    gutter: Option<Gutter>,
    line_highlight: Option<LineHighlight>,
    search: Option<SearchPaint>,
    /// Column and color of a vertical page guide.
    guide: Option<(usize, Color)>,
    minimap: Option<Minimap>,
    style: Theme::Style,
    on_edit: Option<Box<dyn Fn(Action) -> Message + 'a>>,
    highlighter_settings: Highlighter::Settings,
    highlighter_format: fn(&Highlighter::Highlight, &Theme) -> highlighter::Format<Font>,
}

impl<'a, Message, Theme> CodeEditor<'a, highlighter::PlainText, Message, Theme>
where
    Theme: StyleSheet,
{
    pub fn new(content: &'a Content) -> Self {
        Self {
            content,
            font: None,
            text_size: None,
            line_height: LineHeight::default(),
            width: Length::Fill,
            height: Length::Shrink,
            padding: Padding::new(5.0),
            gutter: None,
            line_highlight: None,
            search: None,
            guide: None,
            minimap: None,
            style: Default::default(),
            on_edit: None,
            highlighter_settings: (),
            highlighter_format: |_highlight, _theme| highlighter::Format::default(),
        }
    }
}

impl<'a, Highlighter, Message, Theme> CodeEditor<'a, Highlighter, Message, Theme>
where
    Highlighter: text::Highlighter,
    Theme: StyleSheet,
{
    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.height = height.into();
        self
    }

    /// Sets the message produced when an action is performed; without it the
    /// editor is disabled.
    pub fn on_action(mut self, on_edit: impl Fn(Action) -> Message + 'a) -> Self {
        self.on_edit = Some(Box::new(on_edit));
        self
    }

    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.padding = padding.into();
        self
    }

    pub fn size(mut self, size: impl Into<Pixels>) -> Self {
        self.text_size = Some(size.into());
        self
    }

    /// Shows line numbers, aligned with the text however it wraps or scrolls.
    pub fn gutter(mut self, gutter: Gutter) -> Self {
        self.gutter = Some(gutter);
        self
    }

    pub fn font(mut self, font: Font) -> Self {
        self.font = Some(font);
        self
    }

    pub fn line_height(mut self, line_height: LineHeight) -> Self {
        self.line_height = line_height;
        self
    }

    /// Washes the cursor's line.
    pub fn line_highlight(mut self, highlight: LineHighlight) -> Self {
        self.line_highlight = Some(highlight);
        self
    }

    /// Paints search matches behind the text.
    pub fn search(mut self, paint: SearchPaint) -> Self {
        self.search = Some(paint);
        self
    }

    /// Draws a vertical guide after `column` characters (monospace fonts).
    pub fn page_guide(mut self, column: usize, color: Color) -> Self {
        self.guide = Some((column, color));
        self
    }

    /// Shows a miniature of the whole file on the right; dragging in it
    /// scrolls the text.
    pub fn minimap(mut self, minimap: Minimap) -> Self {
        self.minimap = Some(minimap);
        self
    }

    pub fn highlight<H: text::Highlighter>(
        self,
        settings: H::Settings,
        to_format: fn(&H::Highlight, &Theme) -> highlighter::Format<Font>,
    ) -> CodeEditor<'a, H, Message, Theme> {
        CodeEditor {
            content: self.content,
            font: self.font,
            text_size: self.text_size,
            line_height: self.line_height,
            width: self.width,
            height: self.height,
            padding: self.padding,
            gutter: self.gutter,
            line_highlight: self.line_highlight,
            search: self.search,
            guide: self.guide,
            minimap: self.minimap,
            style: self.style,
            on_edit: self.on_edit,
            highlighter_settings: settings,
            highlighter_format: to_format,
        }
    }

    pub fn style(mut self, style: impl Into<Theme::Style>) -> Self {
        self.style = style.into();
        self
    }

    /// How many rows to scroll so that the line at `y` (from the minimap's
    /// top) ends up in the middle of the screen.
    fn minimap_scroll(&self, y: f32, height: f32) -> Option<i32> {
        let internal = self.content.0.borrow();
        let buffer = internal.editor.buffer();
        let lines = buffer.lines.len();
        let (first, last) =
            buffer
                .layout_runs()
                .fold(None, |range: Option<(usize, usize)>, run| {
                    Some(match range {
                        Some((a, b)) => (a.min(run.line_i), b.max(run.line_i)),
                        None => (run.line_i, run.line_i),
                    })
                })?;
        let wanted = minimap_line_at(y, minimap_scale(lines, height), lines);
        let target = wanted.saturating_sub((last - first).div_ceil(2));
        // Rows, not lines: a wrapped line takes several (1 if not laid out yet).
        let rows = |from: usize, to: usize| -> i32 {
            buffer.lines[from..to]
                .iter()
                .map(|l| l.layout_opt().as_ref().map_or(1, |v| v.len().max(1)) as i32)
                .sum()
        };
        let delta = if target >= first {
            rows(first, target)
        } else {
            -rows(target, first)
        };
        (delta != 0).then_some(delta)
    }

    fn draw_minimap(
        &self,
        renderer: &mut iced::Renderer,
        editor: &Editor,
        rect: Rectangle,
        minimap: Minimap,
        regex: Option<&regex::Regex>,
    ) {
        let buffer = editor.buffer();
        let lines = buffer.lines.len();
        let scale = minimap_scale(lines, rect.height);
        let top = rect.y + MINIMAP_INSET;
        let quad = |renderer: &mut iced::Renderer, x, y, width, height, color| {
            let bounds = Rectangle {
                x,
                y,
                width,
                height,
            };
            if let Some(bounds) = rect.intersection(&bounds) {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds,
                        ..renderer::Quad::default()
                    },
                    color,
                );
            }
        };

        // The part of the file on screen.
        let range = buffer
            .layout_runs()
            .fold(None, |range: Option<(usize, usize)>, run| {
                Some(match range {
                    Some((a, b)) => (a.min(run.line_i), b.max(run.line_i)),
                    None => (run.line_i, run.line_i),
                })
            });
        if let Some((first, last)) = range {
            let band = Rectangle {
                x: rect.x + 0.5,
                y: top + first as f32 * scale,
                width: rect.width - 1.0,
                height: ((last - first + 1) as f32 * scale).max(6.0),
            };
            if let Some(bounds) = rect.intersection(&band) {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds,
                        border: iced::Border {
                            color: minimap.viewport_border,
                            width: 1.0,
                            radius: 3.0.into(),
                        },
                        ..renderer::Quad::default()
                    },
                    minimap.viewport,
                );
            }
        }

        // The lines: one bar each, or one per pixel row when they don't fit
        // (the longest of the lines that share it).
        let group = (1.0 / scale).ceil().max(1.0) as usize;
        let step = group as f32 * scale;
        let max_width = rect.width - 12.0;
        for (row, first) in (0..lines).step_by(group).enumerate() {
            let span = buffer.lines[first..(first + group).min(lines)]
                .iter()
                .filter_map(|l| minimap_span(l.text()))
                .fold(None, |acc: Option<(usize, usize)>, (start, len)| {
                    Some(match acc {
                        Some((s, end)) => (s.min(start), end.max(start + len)),
                        None => (start, start + len),
                    })
                });
            let Some((start, end)) = span else { continue };
            let x = start as f32 * MINIMAP_CHAR;
            if x >= max_width {
                continue;
            }
            quad(
                renderer,
                rect.x + 6.0 + x,
                top + row as f32 * step,
                ((end - start) as f32 * MINIMAP_CHAR).clamp(1.5, max_width - x),
                (step * 0.7).max(1.0),
                minimap.bar,
            );
        }

        // Search matches: a tick on the right edge.
        if let Some(paint) = self
            .search
            .as_ref()
            .filter(|_| lines <= MINIMAP_MAX_MARKED_LINES)
        {
            for (i, line) in buffer.lines.iter().enumerate() {
                if !line_matches(line.text(), paint, regex).is_empty() {
                    quad(
                        renderer,
                        rect.x + rect.width - 4.0,
                        top + i as f32 * scale,
                        4.0,
                        scale.max(2.0),
                        minimap.marker,
                    );
                }
            }
        }
    }

    fn size_px(&self, renderer: &iced::Renderer) -> f32 {
        self.text_size.unwrap_or_else(|| renderer.default_size()).0
    }

    /// The padding around the text proper: the user's padding plus the gutter.
    fn text_padding(&self, renderer: &iced::Renderer, lines: usize) -> Padding {
        let gutter = if self.gutter.is_some() {
            gutter_width(self.size_px(renderer), lines)
        } else {
            0.0
        };
        Padding {
            left: self.padding.left + gutter + if gutter > 0.0 { GUTTER_GAP } else { 0.0 },
            right: self.padding.right
                + if self.minimap.is_some() {
                    MINIMAP_WIDTH
                } else {
                    0.0
                },
            ..self.padding
        }
    }
}

/// The content of a [`CodeEditor`].
pub struct Content(RefCell<Internal>);

/// Identifies a freshly created [`Content`], so the widget can tell that the
/// text it shows was swapped for another one.
static NEXT_CONTENT_ID: AtomicU64 = AtomicU64::new(1);

/// Bounds, font, size and line height of the last editor layout. A new
/// `Content` has never been laid out, so [`Content::settle`] borrows these.
static LAST_LAYOUT: Mutex<Option<(Size, Font, Pixels, LineHeight)>> = Mutex::new(None);

fn remember_layout(bounds: Size, font: Font, size: Pixels, line_height: LineHeight) {
    if let Ok(mut last) = LAST_LAYOUT.lock() {
        *last = Some((bounds, font, size, line_height));
    }
}

struct Internal {
    id: u64,
    /// Topmost line edited since the widget last told its highlighter.
    changed_from: Option<usize>,
    editor: Editor,
    is_dirty: bool,
}

impl Content {
    pub fn new() -> Self {
        Self::with_text("")
    }

    pub fn with_text(text: &str) -> Self {
        Self(RefCell::new(Internal {
            id: NEXT_CONTENT_ID.fetch_add(1, Ordering::Relaxed),
            changed_from: None,
            editor: Editor::with_text(text),
            is_dirty: true,
        }))
    }

    pub fn perform(&mut self, action: Action) {
        let internal = self.0.get_mut();
        // Remember the topmost line an edit may touch (the selection can
        // start above the cursor), for the highlighter.
        let touched = matches!(action, Action::Edit(_)).then(|| {
            let line = internal.editor.cursor_position().0;
            let above = internal
                .editor
                .selection()
                .map_or(0, |s| s.matches('\n').count());
            line.saturating_sub(above)
        });
        internal.editor.perform(action);
        if let Some(before) = touched {
            let after = internal.editor.cursor_position().0;
            let top = before.min(after);
            internal.changed_from = Some(internal.changed_from.map_or(top, |c| c.min(top)));
        }
        internal.is_dirty = true;
    }

    /// Lays out and shapes the text now.
    ///
    /// An edit leaves its lines unshaped until the widget's next layout, and
    /// cosmic-text panics ("layout not found") when a motion such as
    /// `Home` or `Down` starts on one, while `Left`/`Right` silently do
    /// nothing. Calling this first makes such motions reliable. A no-op until
    /// an editor has been laid out once.
    pub fn settle(&mut self) {
        let last = LAST_LAYOUT.lock().ok().and_then(|last| *last);
        let Some((bounds, font, size, line_height)) = last else {
            return;
        };
        let internal = self.0.get_mut();
        // The editor tells its highlighter about edited lines during this
        // update; the real highlighter isn't at hand, so `changed_from`
        // (kept by `perform`) passes that on at the next layout.
        internal.editor.update(
            bounds,
            font,
            size,
            line_height,
            &mut highlighter::PlainText::new(&()),
        );
    }

    pub fn line_count(&self) -> usize {
        self.0.borrow().editor.line_count()
    }

    pub fn line(&self, index: usize) -> Option<impl std::ops::Deref<Target = str> + '_> {
        std::cell::Ref::filter_map(self.0.borrow(), |internal| internal.editor.line(index)).ok()
    }

    pub fn lines(&self) -> impl Iterator<Item = impl std::ops::Deref<Target = str> + '_> {
        struct Lines<'a> {
            internal: std::cell::Ref<'a, Internal>,
            current: usize,
        }

        impl<'a> Iterator for Lines<'a> {
            type Item = std::cell::Ref<'a, str>;

            fn next(&mut self) -> Option<Self::Item> {
                let line =
                    std::cell::Ref::filter_map(std::cell::Ref::clone(&self.internal), |internal| {
                        internal.editor.line(self.current)
                    })
                    .ok()?;
                self.current += 1;
                Some(line)
            }
        }

        Lines {
            internal: self.0.borrow(),
            current: 0,
        }
    }

    /// The text, lines joined with `'\n'` and always ending in a newline.
    pub fn text(&self) -> String {
        let mut text = self
            .lines()
            .enumerate()
            .fold(String::new(), |mut contents, (i, line)| {
                if i > 0 {
                    contents.push('\n');
                }
                contents.push_str(&line);
                contents
            });
        if !text.ends_with('\n') {
            text.push('\n');
        }
        text
    }

    pub fn selection(&self) -> Option<String> {
        self.0.borrow().editor.selection()
    }

    /// The cursor as (line, byte index within the line).
    pub fn cursor_position(&self) -> (usize, usize) {
        self.0.borrow().editor.cursor_position()
    }
}

impl Default for Content {
    fn default() -> Self {
        Self::new()
    }
}

struct State<Highlighter: text::Highlighter> {
    is_focused: bool,
    last_click: Option<mouse::Click>,
    drag_click: Option<mouse::click::Kind>,
    partial_scroll: f32,
    /// A drag that started in the minimap.
    minimap_drag: bool,
    highlighter: RefCell<Highlighter>,
    highlighter_settings: Highlighter::Settings,
    highlighter_format_address: usize,
    /// The `Content` the highlighter has been following.
    content_id: u64,
}

impl<'a, Highlighter, Message, Theme> iced::advanced::Widget<Message, Theme, iced::Renderer>
    for CodeEditor<'a, Highlighter, Message, Theme>
where
    Highlighter: text::Highlighter,
    Theme: StyleSheet,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State<Highlighter>>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State {
            is_focused: false,
            last_click: None,
            drag_click: None,
            partial_scroll: 0.0,
            minimap_drag: false,
            highlighter: RefCell::new(Highlighter::new(&self.highlighter_settings)),
            highlighter_settings: self.highlighter_settings.clone(),
            highlighter_format_address: self.highlighter_format as usize,
            content_id: self.content.0.borrow().id,
        })
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: self.width,
            height: self.height,
        }
    }

    fn layout(
        &self,
        tree: &mut widget::Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let mut internal = self.content.0.borrow_mut();
        let state = tree.state.downcast_mut::<State<Highlighter>>();

        if state.highlighter_format_address != self.highlighter_format as usize {
            state.highlighter.borrow_mut().change_line(0);
            state.highlighter_format_address = self.highlighter_format as usize;
        }

        // The highlighter remembers how far it got; a different `Content` is
        // new text, so it must start over (iced's own widget forgets this and
        // leaves a swapped-in buffer uncolored).
        if state.content_id != internal.id {
            state.highlighter.borrow_mut().change_line(0);
            state.content_id = internal.id;
        }

        if state.highlighter_settings != self.highlighter_settings {
            state
                .highlighter
                .borrow_mut()
                .update(&self.highlighter_settings);
            state.highlighter_settings = self.highlighter_settings.clone();
        }

        if let Some(line) = internal.changed_from.take() {
            state.highlighter.borrow_mut().change_line(line);
        }

        let limits = limits.height(self.height);
        let padding = self.text_padding(renderer, internal.editor.line_count());
        remember_layout(
            limits.shrink(padding).max(),
            self.font.unwrap_or_else(|| renderer.default_font()),
            self.text_size.unwrap_or_else(|| renderer.default_size()),
            self.line_height,
        );

        internal.editor.update(
            limits.shrink(padding).max(),
            self.font.unwrap_or_else(|| renderer.default_font()),
            self.text_size.unwrap_or_else(|| renderer.default_size()),
            self.line_height,
            state.highlighter.borrow_mut().deref_mut(),
        );

        match self.height {
            Length::Fill | Length::FillPortion(_) | Length::Fixed(_) => {
                layout::Node::new(limits.max())
            }
            Length::Shrink => {
                let min_bounds = internal.editor.min_bounds();
                layout::Node::new(
                    limits
                        .height(min_bounds.height)
                        .max()
                        .expand(Size::new(0.0, padding.vertical())),
                )
            }
        }
    }

    fn on_event(
        &mut self,
        tree: &mut widget::Tree,
        event: Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) -> event::Status {
        let Some(on_edit) = self.on_edit.as_ref() else {
            return event::Status::Ignored;
        };

        let state = tree.state.downcast_mut::<State<Highlighter>>();
        let padding = self.text_padding(renderer, self.content.line_count());

        if self.minimap.is_some() {
            let rect = minimap_rect(layout.bounds());
            let target = match event {
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                    if cursor.is_over(rect) =>
                {
                    state.minimap_drag = true;
                    cursor.position().map(|p| p.y)
                }
                Event::Mouse(mouse::Event::CursorMoved { position }) if state.minimap_drag => {
                    Some(position.y)
                }
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                    if state.minimap_drag =>
                {
                    state.minimap_drag = false;
                    return event::Status::Captured;
                }
                _ => None,
            };
            if let Some(y) = target {
                if let Some(lines) = self.minimap_scroll(y - rect.y, rect.height) {
                    shell.publish(on_edit(Action::Scroll { lines }));
                }
                return event::Status::Captured;
            }
        }

        let Some(update) = Update::from_event(event, state, layout.bounds(), padding, cursor)
        else {
            return event::Status::Ignored;
        };

        match update {
            Update::Click(click) => {
                let action = match click.kind() {
                    mouse::click::Kind::Single => Action::Click(click.position()),
                    mouse::click::Kind::Double => Action::SelectWord,
                    mouse::click::Kind::Triple => Action::SelectLine,
                };

                state.is_focused = true;
                state.last_click = Some(click);
                state.drag_click = Some(click.kind());

                shell.publish(on_edit(action));
            }
            Update::Scroll(lines) => {
                let lines = lines + state.partial_scroll;
                state.partial_scroll = lines.fract();

                shell.publish(on_edit(Action::Scroll {
                    lines: lines as i32,
                }));
            }
            Update::Unfocus => {
                state.is_focused = false;
                state.drag_click = None;
            }
            Update::Release => {
                state.drag_click = None;
            }
            Update::Action(action) => {
                shell.publish(on_edit(action));
            }
            Update::Copy => {
                if let Some(selection) = self.content.selection() {
                    clipboard.write(clipboard::Kind::Standard, selection);
                }
            }
            Update::Cut => {
                if let Some(selection) = self.content.selection() {
                    clipboard.write(clipboard::Kind::Standard, selection);
                    shell.publish(on_edit(Action::Edit(Edit::Delete)));
                }
            }
            Update::Paste => {
                if let Some(contents) = clipboard.read(clipboard::Kind::Standard) {
                    shell.publish(on_edit(Action::Edit(Edit::Paste(Arc::new(contents)))));
                }
            }
        }

        event::Status::Captured
    }

    fn draw(
        &self,
        tree: &widget::Tree,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();

        let mut internal = self.content.0.borrow_mut();
        let state = tree.state.downcast_ref::<State<Highlighter>>();

        internal.editor.highlight(
            self.font.unwrap_or_else(|| renderer.default_font()),
            state.highlighter.borrow_mut().deref_mut(),
            |highlight| (self.highlighter_format)(highlight, theme),
        );

        let is_disabled = self.on_edit.is_none();
        let is_mouse_over = cursor.is_over(bounds);

        let appearance = if is_disabled {
            theme.disabled(&self.style)
        } else if state.is_focused {
            theme.focused(&self.style)
        } else if is_mouse_over {
            theme.hovered(&self.style)
        } else {
            theme.active(&self.style)
        };

        renderer.fill_quad(
            renderer::Quad {
                bounds,
                border: appearance.border,
                ..renderer::Quad::default()
            },
            appearance.background,
        );

        let padding = self.text_padding(renderer, internal.editor.line_count());
        let origin = bounds.position() + Vector::new(padding.left, padding.top);

        let size = self.size_px(renderer);
        let row_height = self.line_height.to_absolute(Pixels(size)).0;
        let cursor_line = internal.editor.cursor_position().0;

        // Behind the text: the page guide, the cursor line's wash and the
        // search matches. (Text is always painted above quads.)
        if let Some((column, color)) = self.guide {
            // JetBrains Mono's advance is 0.6 em.
            let x = origin.x + column as f32 * size * 0.6;
            if x < bounds.x + bounds.width {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle {
                            x,
                            y: bounds.y,
                            width: 1.0,
                            height: bounds.height,
                        },
                        ..renderer::Quad::default()
                    },
                    color,
                );
            }
        }
        let regex = self
            .search
            .as_ref()
            .filter(|p| p.use_regex)
            .and_then(|p| crate::search::build_regex(&p.query, p.case_sensitive).ok());
        for run in internal.editor.buffer().layout_runs() {
            let top = bounds.y + padding.top + run.line_top;
            if let Some(highlight) = self.line_highlight.filter(|_| run.line_i == cursor_line) {
                let left = origin.x - GUTTER_GAP;
                let band = Rectangle {
                    x: left,
                    y: top,
                    width: bounds.x + bounds.width
                        - if self.minimap.is_some() {
                            MINIMAP_WIDTH
                        } else {
                            0.0
                        }
                        - left,
                    height: row_height,
                };
                if let Some(band) = bounds.intersection(&band) {
                    renderer.fill_quad(
                        renderer::Quad {
                            bounds: band,
                            ..renderer::Quad::default()
                        },
                        highlight.background,
                    );
                    renderer.fill_quad(
                        renderer::Quad {
                            bounds: Rectangle { width: 2.0, ..band },
                            ..renderer::Quad::default()
                        },
                        highlight.bar,
                    );
                }
            }
            if let Some(paint) = &self.search {
                for range in line_matches(run.text, paint, regex.as_ref()) {
                    let current = paint
                        .current
                        .as_ref()
                        .is_some_and(|(l, r)| *l == run.line_i && *r == range);
                    let span = run.highlight(
                        Cursor_::new(run.line_i, range.start),
                        Cursor_::new(run.line_i, range.end),
                    );
                    let Some((x, width)) = span else { continue };
                    let rect = Rectangle {
                        x: origin.x + x,
                        y: top,
                        width,
                        height: row_height,
                    };
                    let Some(rect) = bounds.intersection(&rect) else {
                        continue;
                    };
                    renderer.fill_quad(
                        renderer::Quad {
                            bounds: rect,
                            border: iced::Border {
                                color: if current {
                                    paint.current_border
                                } else {
                                    Color::TRANSPARENT
                                },
                                width: if current { 1.0 } else { 0.0 },
                                radius: 3.0.into(),
                            },
                            ..renderer::Quad::default()
                        },
                        if current {
                            paint.current_color
                        } else {
                            paint.color
                        },
                    );
                }
            }
        }

        renderer.fill_editor(&internal.editor, origin, style.text_color, *viewport);

        let translation = Vector::new(bounds.x + padding.left, bounds.y + padding.top);
        let caret_height = (size * 1.3).min(row_height);

        if state.is_focused {
            match internal.editor.cursor() {
                Cursor::Caret(position) => {
                    let position = position + translation;

                    if bounds.contains(position) {
                        renderer.fill_quad(
                            renderer::Quad {
                                bounds: Rectangle {
                                    x: position.x,
                                    // A caret a little shorter than the row,
                                    // centred in it.
                                    y: position.y + (row_height - caret_height) / 2.0,
                                    width: 2.0,
                                    height: caret_height,
                                },
                                ..renderer::Quad::default()
                            },
                            theme.value_color(&self.style),
                        );
                    }
                }
                Cursor::Selection(ranges) => {
                    for range in ranges
                        .into_iter()
                        .filter_map(|range| bounds.intersection(&(range + translation)))
                    {
                        renderer.fill_quad(
                            renderer::Quad {
                                bounds: range,
                                ..renderer::Quad::default()
                            },
                            theme.selection_color(&self.style),
                        );
                    }
                }
            }
        }

        if let Some(gutter) = self.gutter {
            let width = gutter_width(size, internal.editor.line_count());
            let column = Rectangle {
                x: bounds.x + self.padding.left,
                y: bounds.y,
                width,
                height: bounds.height,
            };
            let clip = column.intersection(viewport).unwrap_or(column);
            let mut previous = None;
            // One layout run per visual row: a wrapped line produces several
            // runs sharing a `line_i`, and only the first gets its number.
            for run in internal.editor.buffer().layout_runs() {
                if previous == Some(run.line_i) {
                    continue;
                }
                previous = Some(run.line_i);

                let number = (run.line_i + 1).to_string();
                renderer.fill_text(
                    text::Text {
                        content: &number,
                        bounds: Size::new(width - 14.0, row_height),
                        size: Pixels(size),
                        line_height: self.line_height,
                        font: self.font.unwrap_or(Font::MONOSPACE),
                        horizontal_alignment: alignment::Horizontal::Right,
                        vertical_alignment: alignment::Vertical::Center,
                        shaping: text::Shaping::Basic,
                    },
                    // With right/centre alignment the position is the anchor of
                    // the text, not the corner of its bounds.
                    Point::new(
                        column.x + width - 14.0,
                        bounds.y + padding.top + run.line_top + row_height / 2.0,
                    ),
                    if run.line_i == cursor_line {
                        gutter.current
                    } else {
                        gutter.text
                    },
                    clip,
                );
            }
        }

        if let Some(minimap) = self.minimap {
            self.draw_minimap(
                renderer,
                &internal.editor,
                minimap_rect(bounds),
                minimap,
                regex.as_ref(),
            );
        }
    }

    fn mouse_interaction(
        &self,
        _state: &widget::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        let is_disabled = self.on_edit.is_none();

        if self.minimap.is_some() && cursor.is_over(minimap_rect(layout.bounds())) {
            mouse::Interaction::Pointer
        } else if cursor.is_over(layout.bounds()) {
            if is_disabled {
                mouse::Interaction::NotAllowed
            } else {
                mouse::Interaction::Text
            }
        } else {
            mouse::Interaction::default()
        }
    }
}

impl<'a, Highlighter, Message, Theme> From<CodeEditor<'a, Highlighter, Message, Theme>>
    for Element<'a, Message, Theme, iced::Renderer>
where
    Highlighter: text::Highlighter,
    Message: 'a,
    Theme: StyleSheet + 'a,
{
    fn from(editor: CodeEditor<'a, Highlighter, Message, Theme>) -> Self {
        Self::new(editor)
    }
}

enum Update {
    Click(mouse::Click),
    Scroll(f32),
    Unfocus,
    Release,
    Action(Action),
    Copy,
    Cut,
    Paste,
}

impl Update {
    fn from_event<H: Highlighter>(
        event: Event,
        state: &State<H>,
        bounds: Rectangle,
        padding: Padding,
        cursor: mouse::Cursor,
    ) -> Option<Self> {
        let action = |action| Some(Update::Action(action));
        let edit = |edit| action(Action::Edit(edit));
        // Positions are relative to the text area, past the padding.
        let inset = Vector::new(padding.left, padding.top);

        match event {
            Event::Mouse(event) => match event {
                mouse::Event::ButtonPressed(mouse::Button::Left) => {
                    if let Some(cursor_position) = cursor.position_in(bounds) {
                        let click = mouse::Click::new(cursor_position - inset, state.last_click);
                        Some(Update::Click(click))
                    } else if state.is_focused {
                        Some(Update::Unfocus)
                    } else {
                        None
                    }
                }
                mouse::Event::ButtonReleased(mouse::Button::Left) => Some(Update::Release),
                mouse::Event::CursorMoved { .. } => match state.drag_click {
                    Some(mouse::click::Kind::Single) => {
                        let cursor_position = cursor.position_in(bounds)? - inset;
                        action(Action::Drag(cursor_position))
                    }
                    _ => None,
                },
                mouse::Event::WheelScrolled { delta } if cursor.is_over(bounds) => {
                    Some(Update::Scroll(match delta {
                        mouse::ScrollDelta::Lines { y, .. } => {
                            if y.abs() > 0.0 {
                                y.signum() * -(y.abs() * 4.0).max(1.0)
                            } else {
                                0.0
                            }
                        }
                        mouse::ScrollDelta::Pixels { y, .. } => -y / 4.0,
                    }))
                }
                _ => None,
            },
            Event::Keyboard(event) => match event {
                keyboard::Event::KeyPressed {
                    key,
                    modifiers,
                    text,
                    ..
                } if state.is_focused => {
                    match key.as_ref() {
                        keyboard::Key::Named(key::Named::Enter) => {
                            return edit(Edit::Enter);
                        }
                        keyboard::Key::Named(key::Named::Backspace) => {
                            return edit(Edit::Backspace);
                        }
                        keyboard::Key::Named(key::Named::Delete) => {
                            return edit(Edit::Delete);
                        }
                        keyboard::Key::Named(key::Named::Escape) => {
                            return Some(Self::Unfocus);
                        }
                        keyboard::Key::Character("c") if modifiers.command() => {
                            return Some(Self::Copy);
                        }
                        keyboard::Key::Character("x") if modifiers.command() => {
                            return Some(Self::Cut);
                        }
                        keyboard::Key::Character("v")
                            if modifiers.command() && !modifiers.alt() =>
                        {
                            return Some(Self::Paste);
                        }
                        _ => {}
                    }

                    if let Some(text) = text {
                        if let Some(c) = text.chars().find(|c| !c.is_control()) {
                            return edit(Edit::Insert(c));
                        }
                    }

                    if let keyboard::Key::Named(named_key) = key.as_ref() {
                        if let Some(motion) = motion(named_key) {
                            let motion = if is_jump_modifier_pressed(modifiers) {
                                motion.widen()
                            } else {
                                motion
                            };

                            return action(if modifiers.shift() {
                                Action::Select(motion)
                            } else {
                                Action::Move(motion)
                            });
                        }
                    }

                    None
                }
                _ => None,
            },
            _ => None,
        }
    }
}

fn motion(key: key::Named) -> Option<Motion> {
    match key {
        key::Named::ArrowLeft => Some(Motion::Left),
        key::Named::ArrowRight => Some(Motion::Right),
        key::Named::ArrowUp => Some(Motion::Up),
        key::Named::ArrowDown => Some(Motion::Down),
        key::Named::Home => Some(Motion::Home),
        key::Named::End => Some(Motion::End),
        key::Named::PageUp => Some(Motion::PageUp),
        key::Named::PageDown => Some(Motion::PageDown),
        _ => None,
    }
}

fn is_jump_modifier_pressed(modifiers: keyboard::Modifiers) -> bool {
    if cfg!(target_os = "macos") {
        modifiers.alt()
    } else {
        modifiers.control()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settle_makes_motions_safe_right_after_an_edit() {
        // Without `settle`, `Home` right after an edit panics in cosmic-text.
        remember_layout(
            Size::new(800.0, 600.0),
            Font::DEFAULT,
            Pixels(14.0),
            LineHeight::default(),
        );
        let mut content = Content::with_text("a\nb\nc\nd\n");
        content.perform(Action::Edit(Edit::Insert('x')));
        content.settle();
        content.perform(Action::Move(Motion::DocumentStart));
        content.perform(Action::Move(Motion::Down));
        content.perform(Action::Move(Motion::Down));
        content.perform(Action::Move(Motion::Home));
        assert_eq!(content.cursor_position(), (2, 0));
    }

    #[test]
    fn minimap_lines_fit_the_height_but_never_exceed_two_pixels() {
        assert_eq!(minimap_scale(10, 800.0), 2.0);
        // 1000 lines in 800 - 16 px.
        assert!((minimap_scale(1000, 800.0) - 0.784).abs() < 1e-3);
        assert!(minimap_scale(0, 800.0) > 0.0);
    }

    #[test]
    fn minimap_position_maps_back_to_a_line() {
        let scale = minimap_scale(1000, 800.0);
        assert_eq!(minimap_line_at(0.0, scale, 1000), 0);
        assert_eq!(minimap_line_at(800.0, scale, 1000), 999);
        let y = MINIMAP_INSET + 500.0 * scale + 0.1;
        assert_eq!(minimap_line_at(y, scale, 1000), 500);
        assert_eq!(minimap_line_at(5.0, 2.0, 0), 0);
    }

    #[test]
    fn minimap_span_counts_indentation_but_not_blank_lines() {
        assert_eq!(minimap_span("fn main() {"), Some((0, 11)));
        assert_eq!(minimap_span("    let x = 1;  "), Some((4, 10)));
        assert_eq!(minimap_span("\tx"), Some((4, 1)));
        assert_eq!(minimap_span("   "), None);
        assert_eq!(minimap_span(""), None);
    }

    #[test]
    fn gutter_has_a_minimum_width_and_grows_with_the_line_count() {
        assert_eq!(gutter_width(16.0, 5), gutter_width(16.0, 99));
        assert_eq!(gutter_width(16.0, 5), 46.0);
        assert!(gutter_width(16.0, 999) > gutter_width(16.0, 99));
        assert!(gutter_width(16.0, 9999) > gutter_width(16.0, 999));
        assert!(gutter_width(30.0, 999) > gutter_width(12.0, 999));
    }
}
