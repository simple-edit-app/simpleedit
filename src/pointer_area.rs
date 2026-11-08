//! Like iced's `MouseArea`, but its press and move messages carry where the
//! pointer is inside the area: a diff needs it to tell which character was
//! clicked.

use iced::advanced::layout;
use iced::advanced::mouse;
use iced::advanced::overlay;
use iced::advanced::renderer;
use iced::advanced::widget::{tree, Operation, Tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget};
use iced::{Element, Event, Length, Point, Rectangle, Size, Vector};

type At<'a, Message> = Box<dyn Fn(Point) -> Message + 'a>;

pub struct PointerArea<'a, Message> {
    content: Element<'a, Message>,
    on_press: Option<At<'a, Message>>,
    on_move: Option<At<'a, Message>>,
    on_release: Option<Message>,
    quiet: bool,
    /// Sees the events before its content does and takes none from it (a
    /// button under it still gets its press).
    early: bool,
    on_press_abs: Option<At<'a, Message>>,
    on_right_press: Option<Message>,
}

pub fn pointer_area<'a, Message>(
    content: impl Into<Element<'a, Message>>,
) -> PointerArea<'a, Message> {
    PointerArea {
        content: content.into(),
        on_press: None,
        on_move: None,
        on_release: None,
        quiet: false,
        early: false,
        on_press_abs: None,
        on_right_press: None,
    }
}

impl<'a, Message: Clone> PointerArea<'a, Message> {
    /// What this area makes of an event: presses, releases, moves, exits.
    fn pointer_events(
        &self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        shell: &mut Shell<'_, Message>,
    ) {
        let state: &mut State = tree.state.downcast_mut();
        let here = cursor.position_in(layout.bounds());
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let (Some(at), Some(p)) = (self.on_press.as_ref(), here) {
                    shell.publish(at(p));
                    if !self.early {
                        shell.capture_event();
                    }
                }
                if let (Some(at), Some(p), true) = (
                    self.on_press_abs.as_ref(),
                    cursor.position(),
                    here.is_some(),
                ) {
                    shell.publish(at(p));
                }
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) => {
                // Not captured: a context menu under it still opens.
                if let (Some(message), Some(_)) = (self.on_right_press.as_ref(), here) {
                    shell.publish(message.clone());
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if let Some(message) = self.on_release.as_ref() {
                    shell.publish(message.clone());
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if let (Some(at), Some(p)) = (self.on_move.as_ref(), here) {
                    if state.last != Some(p) {
                        state.last = Some(p);
                        shell.publish(at(p));
                    }
                }
            }
            _ => {}
        }
    }
}

impl<'a, Message> PointerArea<'a, Message> {
    /// A left press, with the pointer's place in the area.
    pub fn on_press(mut self, f: impl Fn(Point) -> Message + 'a) -> Self {
        self.on_press = Some(Box::new(f));
        self
    }

    /// The pointer moved over the area.
    pub fn on_move(mut self, f: impl Fn(Point) -> Message + 'a) -> Self {
        self.on_move = Some(Box::new(f));
        self
    }

    /// Takes no part in widget operations (focus, scrolling…). A context
    /// menu's overlay needs it: `iced_aw` runs the operation on it with the
    /// layout of the widget underneath, which a deeper overlay doesn't fit.
    pub fn quiet(mut self) -> Self {
        self.quiet = true;
        self
    }

    /// Over a widget that takes the press itself (a button): this hears it
    /// first and captures nothing.
    pub fn watching(mut self) -> Self {
        self.early = true;
        self
    }

    /// A left press, with the pointer's place in the window.
    pub fn on_press_in_window(mut self, f: impl Fn(Point) -> Message + 'a) -> Self {
        self.on_press_abs = Some(Box::new(f));
        self
    }

    /// A right press, which this area only hears of.
    pub fn on_right_press(mut self, message: Message) -> Self {
        self.on_right_press = Some(message);
        self
    }

    pub fn on_release(mut self, message: Message) -> Self {
        self.on_release = Some(message);
        self
    }
}

#[derive(Default)]
struct State {
    last: Option<Point>,
}

impl<Message: Clone> Widget<Message, iced::Theme, iced::Renderer> for PointerArea<'_, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        if self.quiet {
            return;
        }
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        if self.early {
            self.pointer_events(tree, event, layout, cursor, shell);
        }
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
        if !self.early && !shell.is_event_captured() {
            self.pointer_events(tree, event, layout, cursor, shell);
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        let inner = self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        );
        if inner == mouse::Interaction::None && cursor.is_over(layout.bounds()) {
            mouse::Interaction::Text
        } else {
            inner
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, iced::Theme, iced::Renderer>> {
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a, Message: Clone + 'a> From<PointerArea<'a, Message>> for Element<'a, Message> {
    fn from(area: PointerArea<'a, Message>) -> Self {
        Element::new(area)
    }
}

/// A context menu whose overlay takes no part in widget operations (see
/// [`PointerArea::quiet`]): `iced_aw` would run them on it with the layout of
/// the widget underneath, which a deeper overlay doesn't fit, and iced panics
/// when, say, a list scrolls while a menu is open.
pub fn context_menu<'a>(
    base: impl Into<Element<'a, crate::app::Message>>,
    menu: impl Fn() -> Element<'a, crate::app::Message> + 'a,
) -> Element<'a, crate::app::Message> {
    iced_aw::ContextMenu::new(base, move || -> Element<'a, crate::app::Message> {
        pointer_area(menu()).quiet().into()
    })
    .into()
}
