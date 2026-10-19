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
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::sync::Mutex;

pub use iced::widget::text_editor::{Action, Edit, Motion, StyleSheet};

type Editor = <iced::Renderer as TextRenderer>::Editor;

/// The line-number column of a [`CodeEditor`].
#[derive(Debug, Clone, Copy)]
pub struct Gutter {
    pub background: Color,
    pub text: Color,
    pub border: Color,
}

/// Space between the gutter and the text.
const GUTTER_GAP: f32 = 10.0;

/// Width of the gutter for a buffer of `lines` lines at text size `size`.
fn gutter_width(size: f32, lines: usize) -> f32 {
    let digits = lines.max(1).to_string().len().max(3);
    digits as f32 * size * 0.62 + 20.0
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

        renderer.fill_editor(&internal.editor, origin, style.text_color, *viewport);

        let translation = Vector::new(bounds.x + padding.left, bounds.y + padding.top);

        if state.is_focused {
            match internal.editor.cursor() {
                Cursor::Caret(position) => {
                    let position = position + translation;

                    if bounds.contains(position) {
                        renderer.fill_quad(
                            renderer::Quad {
                                bounds: Rectangle {
                                    x: position.x,
                                    y: position.y,
                                    width: 1.0,
                                    height: self
                                        .line_height
                                        .to_absolute(
                                            self.text_size
                                                .unwrap_or_else(|| renderer.default_size()),
                                        )
                                        .into(),
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
            let size = self.size_px(renderer);
            let width = gutter_width(size, internal.editor.line_count());
            let column = Rectangle {
                x: bounds.x + self.padding.left,
                y: bounds.y,
                width,
                height: bounds.height,
            };
            let row_height = self.line_height.to_absolute(Pixels(size)).0;

            renderer.fill_quad(
                renderer::Quad {
                    bounds: column,
                    ..renderer::Quad::default()
                },
                gutter.background,
            );
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle {
                        x: column.x + column.width - 1.0,
                        width: 1.0,
                        ..column
                    },
                    ..renderer::Quad::default()
                },
                gutter.border,
            );

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
                        bounds: Size::new(width - 10.0, row_height),
                        size: Pixels(size),
                        line_height: self.line_height,
                        font: Font::MONOSPACE,
                        horizontal_alignment: alignment::Horizontal::Right,
                        vertical_alignment: alignment::Vertical::Center,
                        shaping: text::Shaping::Basic,
                    },
                    // With right/centre alignment the position is the anchor of
                    // the text, not the corner of its bounds.
                    Point::new(
                        column.x + width - 10.0,
                        bounds.y + padding.top + run.line_top + row_height / 2.0,
                    ),
                    gutter.text,
                    clip,
                );
            }
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

        if cursor.is_over(layout.bounds()) {
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
    fn gutter_grows_with_the_line_count() {
        assert_eq!(gutter_width(16.0, 5), gutter_width(16.0, 999));
        assert!(gutter_width(16.0, 1000) > gutter_width(16.0, 999));
        assert!(gutter_width(20.0, 10) > gutter_width(12.0, 10));
    }
}
