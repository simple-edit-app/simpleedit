//! A card that floats over the top-right corner of some content.
//!
//! `iced_aw`'s `FloatingElement` does the same but its overlay never passes
//! widget operations (focus, "which field has the keyboard") on to the
//! floating element, so a text input inside it can't be focused from code.
//! This one does.

use iced::advanced::layout::{Layout, Limits, Node};
use iced::advanced::overlay::{self, Group};
use iced::advanced::widget::{Operation, Tree};
use iced::advanced::{mouse, renderer, Clipboard, Shell, Widget};
use iced::{event, Element, Event, Length, Point, Rectangle, Size, Vector};

/// `underlay` with `card` floating `top` px below its top edge and `right`
/// px inside its right edge.
pub struct TopRight<'a, Message> {
    underlay: Element<'a, Message>,
    card: Element<'a, Message>,
    top: f32,
    right: f32,
}

impl<'a, Message> TopRight<'a, Message> {
    pub fn new(
        underlay: impl Into<Element<'a, Message>>,
        card: impl Into<Element<'a, Message>>,
        top: f32,
        right: f32,
    ) -> Self {
        Self {
            underlay: underlay.into(),
            card: card.into(),
            top,
            right,
        }
    }
}

impl<'a, Message: 'a> Widget<Message, iced::Theme, iced::Renderer> for TopRight<'a, Message> {
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.underlay), Tree::new(&self.card)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&[&self.underlay, &self.card]);
    }

    fn size(&self) -> Size<Length> {
        self.underlay.as_widget().size()
    }

    // Not the default (`size`): a `lazy` underlay has no element yet while
    // the view is being built, and its `size` would panic.
    fn size_hint(&self) -> Size<Length> {
        self.underlay.as_widget().size_hint()
    }

    fn layout(&self, tree: &mut Tree, renderer: &iced::Renderer, limits: &Limits) -> Node {
        self.underlay
            .as_widget()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn on_event(
        &mut self,
        tree: &mut Tree,
        event: Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) -> event::Status {
        self.underlay.as_widget_mut().on_event(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        )
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.underlay.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
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
        self.underlay.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn operate(
        &self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation<Message>,
    ) {
        self.underlay
            .as_widget()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, iced::Theme, iced::Renderer>> {
        let mut children = tree.children.iter_mut();
        let mut group = Group::new();

        if let Some(underlay) = self.underlay.as_widget_mut().overlay(
            children.next().expect("underlay state"),
            layout,
            renderer,
            translation,
        ) {
            group = group.push(underlay);
        }

        let card_state = children.next().expect("card state");
        group = group.push(overlay::Element::new(Box::new(CardOverlay {
            origin: layout.position() + translation,
            bounds: layout.bounds(),
            top: self.top,
            right: self.right,
            card: &mut self.card,
            state: card_state,
        })));

        Some(group.overlay())
    }
}

impl<'a, Message: 'a> From<TopRight<'a, Message>> for Element<'a, Message> {
    fn from(floating: TopRight<'a, Message>) -> Self {
        Element::new(floating)
    }
}

struct CardOverlay<'a, 'b, Message> {
    origin: Point,
    bounds: Rectangle,
    top: f32,
    right: f32,
    card: &'b mut Element<'a, Message>,
    state: &'b mut Tree,
}

impl<'a, 'b, Message> overlay::Overlay<Message, iced::Theme, iced::Renderer>
    for CardOverlay<'a, 'b, Message>
{
    fn layout(&mut self, renderer: &iced::Renderer, _bounds: Size) -> Node {
        let limits = Limits::new(Size::ZERO, self.bounds.size())
            .width(Length::Shrink)
            .height(Length::Shrink);
        let node = self.card.as_widget().layout(self.state, renderer, &limits);
        let x = self.origin.x + self.bounds.width - node.bounds().width - self.right;
        node.move_to(Point::new(x, self.origin.y + self.top))
    }

    fn draw(
        &self,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        let bounds = layout.bounds();
        self.card
            .as_widget()
            .draw(self.state, renderer, theme, style, layout, cursor, &bounds);
    }

    fn operate(
        &mut self,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation<Message>,
    ) {
        self.card
            .as_widget()
            .operate(self.state, layout, renderer, operation);
    }

    fn on_event(
        &mut self,
        event: Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    ) -> event::Status {
        self.card.as_widget_mut().on_event(
            self.state,
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            &layout.bounds(),
        )
    }

    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.card
            .as_widget()
            .mouse_interaction(self.state, layout, cursor, viewport, renderer)
    }

    fn overlay<'c>(
        &'c mut self,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
    ) -> Option<overlay::Element<'c, Message, iced::Theme, iced::Renderer>> {
        self.card
            .as_widget_mut()
            .overlay(self.state, layout, renderer, Vector::ZERO)
    }
}
