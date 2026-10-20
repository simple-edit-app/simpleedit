//! Layers over some content: a card in its top-right corner, a dropdown at a
//! given point, a modal with a dimmed backdrop; plus a wrapper that reports
//! where a right click landed.

use iced::advanced::layout::{Layout, Limits, Node};
use iced::advanced::widget::{Operation, Tree};
use iced::advanced::{mouse, overlay, renderer, Clipboard, Shell, Widget};
use iced::widget::{container, mouse_area, opaque, stack};
use iced::{Element, Event, Length, Padding, Point, Rectangle, Size, Vector};

/// `underlay` with `card` floating `top` px below its top edge and `right`
/// px inside its right edge.
pub fn top_right<'a, Message: 'a>(
    underlay: impl Into<Element<'a, Message>>,
    card: impl Into<Element<'a, Message>>,
    top: f32,
    right: f32,
) -> Element<'a, Message> {
    stack![
        underlay.into(),
        container(card.into())
            .width(Length::Fill)
            .align_x(iced::alignment::Horizontal::Right)
            .padding(Padding {
                top,
                right,
                ..Padding::ZERO
            })
    ]
    .into()
}

/// `content` floating over `underlay`, its top-left corner at (`x`, `y`).
pub fn at_point<'a, Message: 'a>(
    underlay: impl Into<Element<'a, Message>>,
    content: impl Into<Element<'a, Message>>,
    x: f32,
    y: f32,
) -> Element<'a, Message> {
    stack![
        underlay.into(),
        container(content.into()).padding(Padding {
            top: y.max(0.0),
            left: x.max(0.0),
            ..Padding::ZERO
        })
    ]
    .into()
}

/// `content` over `underlay` on a dimmed backdrop; a click on the backdrop
/// sends `on_blur`, if any. `top` puts the content at the top (the palette)
/// rather than in the middle.
pub fn modal<'a, Message: Clone + 'a>(
    underlay: impl Into<Element<'a, Message>>,
    content: impl Into<Element<'a, Message>>,
    on_blur: Option<Message>,
    dark: bool,
    top: bool,
) -> Element<'a, Message> {
    let content = opaque(content.into());
    let backdrop = container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(iced::alignment::Horizontal::Center)
        .align_y(if top {
            iced::alignment::Vertical::Top
        } else {
            iced::alignment::Vertical::Center
        })
        .style(crate::theme::scrim(dark));
    let backdrop: Element<'a, Message> = match on_blur {
        Some(message) => mouse_area(backdrop).on_press(message).into(),
        None => backdrop.into(),
    };
    stack![underlay.into(), opaque(backdrop)].into()
}

/// Reports where a right click landed (relative to the content, ignoring a
/// strip of `exclude_right` px on its right edge), then lets the content —
/// a context menu — handle the click as usual.
///
/// `iced_aw`'s context menu takes the click before what it wraps can see it,
/// so the text editor under it can't move its caret to the clicked line.
pub struct RightClick<'a, Message> {
    content: Element<'a, Message>,
    exclude_right: f32,
    on_click: Box<dyn Fn(Point) -> Message + 'a>,
}

impl<'a, Message> RightClick<'a, Message> {
    pub fn new(
        content: impl Into<Element<'a, Message>>,
        exclude_right: f32,
        on_click: impl Fn(Point) -> Message + 'a,
    ) -> Self {
        Self {
            content: content.into(),
            exclude_right,
            on_click: Box::new(on_click),
        }
    }
}

impl<'a, Message: 'a> Widget<Message, iced::Theme, iced::Renderer> for RightClick<'a, Message> {
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &iced::Renderer, limits: &Limits) -> Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
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
        if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) = event {
            if let Some(position) = cursor.position_in(layout.bounds()) {
                if position.x < layout.bounds().width - self.exclude_right {
                    shell.publish((self.on_click)(position));
                }
            }
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
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
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

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
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

impl<'a, Message: 'a> From<RightClick<'a, Message>> for Element<'a, Message> {
    fn from(widget: RightClick<'a, Message>) -> Self {
        Element::new(widget)
    }
}
