//! The bar of tabs above the editor: a row of tabs that scrolls sideways
//! with the wheel and whose tabs are dragged to reorder them.
//!
//! Everything happens inside the widget, frame by frame: the wheel moves the
//! row at once, a dragged tab follows the pointer while the others make room
//! for it, and only the final order is told to the app. The tab of the file
//! in the editor is brought into view when it changes.

use iced::advanced::layout;
use iced::advanced::mouse;
use iced::advanced::overlay;
use iced::advanced::renderer::{self, Renderer as _};
use iced::advanced::widget::{tree, Operation, Tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget};
use iced::{Color, Element, Event, Length, Point, Rectangle, Size, Vector};

/// How far the pointer moves, button held, before a press becomes a drag.
const DRAG_START: f32 = 4.0;
/// Near an edge of the bar, a dragged tab scrolls it.
const EDGE: f32 = 28.0;
/// Two clicks closer than this are a double click.
const DOUBLE_CLICK: std::time::Duration = std::time::Duration::from_millis(400);
/// Pixels per line of the wheel.
const LINE: f32 = 50.0;

type OnMove<'a, Message> = Box<dyn Fn(usize, usize) -> Message + 'a>;
type OnClose<'a, Message> = Box<dyn Fn(usize) -> Message + 'a>;

pub struct TabStrip<'a, Message> {
    tabs: Vec<Element<'a, Message>>,
    active: Option<usize>,
    on_move: OnMove<'a, Message>,
    /// A click of the middle button on a tab.
    on_close: Option<OnClose<'a, Message>>,
    /// Two clicks in a row on a tab.
    on_double_click: Option<OnClose<'a, Message>>,
    /// Under a dragged tab, and of the thin bar that shows the scroll.
    background: Color,
    scrollbar: Color,
}

pub fn tab_strip<'a, Message>(
    tabs: Vec<Element<'a, Message>>,
    active: Option<usize>,
    on_move: impl Fn(usize, usize) -> Message + 'a,
    background: Color,
    scrollbar: Color,
) -> TabStrip<'a, Message> {
    TabStrip {
        tabs,
        active,
        on_move: Box::new(on_move),
        on_close: None,
        on_double_click: None,
        background,
        scrollbar,
    }
}

impl<'a, Message> TabStrip<'a, Message> {
    /// What a click of the middle button on a tab says (it closes it).
    pub fn on_close(mut self, on_close: impl Fn(usize) -> Message + 'a) -> Self {
        self.on_close = Some(Box::new(on_close));
        self
    }

    /// What two clicks in a row on a tab say.
    pub fn on_double_click(mut self, on_double: impl Fn(usize) -> Message + 'a) -> Self {
        self.on_double_click = Some(Box::new(on_double));
        self
    }
}

#[derive(Debug, Clone, Copy)]
struct Drag {
    index: usize,
    /// Where in the tab it was taken.
    grab: f32,
    /// Where the pointer is, in the row (scroll included).
    x: f32,
}

#[derive(Debug, Default)]
struct State {
    /// How far the row is scrolled.
    offset: f32,
    /// The active tab that was last brought into view.
    shown: Option<usize>,
    /// A press on a tab (its index, where in the row), not a drag yet.
    press: Option<(usize, f32)>,
    drag: Option<Drag>,
    /// The last click: on which tab, and when.
    last_click: Option<(usize, std::time::Instant)>,
    /// Width of the row and of the bar, at the last layout.
    content: f32,
    width: f32,
}

impl State {
    fn clamp(&mut self) {
        self.offset = self.offset.clamp(0.0, (self.content - self.width).max(0.0));
    }
}

/// The left edge and width of each tab, in the row.
fn spans(layout: Layout<'_>) -> Vec<(f32, f32)> {
    let origin = layout.bounds().x;
    layout
        .children()
        .map(|tab| {
            let b = tab.bounds();
            (b.x - origin, b.width)
        })
        .collect()
}

/// Where a dragged tab goes: among the others, after those whose middle is
/// left of its own.
fn target(spans: &[(f32, f32)], drag: &Drag) -> usize {
    let width = spans.get(drag.index).map_or(0.0, |s| s.1);
    let middle = drag.x - drag.grab + width / 2.0;
    spans
        .iter()
        .enumerate()
        .filter(|(i, (x, w))| *i != drag.index && x + w / 2.0 < middle)
        .count()
}

/// How far tab `i` is moved aside to make room for the dragged one.
fn shift(i: usize, drag: &Drag, to: usize, dragged_width: f32) -> f32 {
    let from = drag.index;
    if from < to && i > from && i <= to {
        -dragged_width
    } else if from > to && i >= to && i < from {
        dragged_width
    } else {
        0.0
    }
}

impl<'a, Message> Widget<Message, iced::Theme, iced::Renderer> for TabStrip<'a, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn children(&self) -> Vec<Tree> {
        self.tabs.iter().map(Tree::new).collect()
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&self.tabs);
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Shrink)
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let max = limits.max();
        // Each tab takes the width it needs: the limits shrink what would
        // fill (its underline), or a tab would be as wide as the room given,
        // which is endless here.
        let child_limits = layout::Limits::with_compression(
            Size::ZERO,
            Size::new(f32::INFINITY, max.height),
            Size::new(true, false),
        );
        let mut x = 0.0;
        let mut height: f32 = 0.0;
        let mut nodes = Vec::with_capacity(self.tabs.len());
        for (tab, state) in self.tabs.iter_mut().zip(&mut tree.children) {
            let node = tab.as_widget_mut().layout(state, renderer, &child_limits);
            let size = node.size();
            nodes.push(node.move_to(Point::new(x, 0.0)));
            x += size.width;
            height = height.max(size.height);
        }
        let width = if max.width.is_finite() { max.width } else { x };

        let state = tree.state.downcast_mut::<State>();
        state.content = x;
        state.width = width;
        // The tab of the file in the editor comes into view when it changes.
        if self.active != state.shown {
            state.shown = self.active;
            if let Some(node) = self.active.and_then(|i| nodes.get(i)) {
                let b = node.bounds();
                if b.x < state.offset {
                    state.offset = b.x;
                } else if b.x + b.width > state.offset + width {
                    state.offset = b.x + b.width - width;
                }
            }
        }
        state.clamp();
        layout::Node::with_children(Size::new(width, height), nodes)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        operation.traverse(&mut |operation| {
            for ((tab, state), layout) in self
                .tabs
                .iter_mut()
                .zip(&mut tree.children)
                .zip(layout.children())
            {
                tab.as_widget_mut()
                    .operate(state, layout, renderer, operation);
            }
        });
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
        let bounds = layout.bounds();
        let spans = spans(layout);
        let state = tree.state.downcast_mut::<State>();
        let over = cursor.position_over(bounds);
        // Where the pointer is in the row.
        let in_row = |state: &State, p: Point| p.x - bounds.x + state.offset;

        match event {
            Event::Mouse(mouse::Event::WheelScrolled { delta }) if over.is_some() => {
                let moved = match *delta {
                    mouse::ScrollDelta::Lines { x, y } => -(x + y) * LINE,
                    mouse::ScrollDelta::Pixels { x, y } => -(x + y),
                };
                let before = state.offset;
                state.offset += moved;
                state.clamp();
                if state.offset != before {
                    shell.capture_event();
                    shell.request_redraw();
                }
                return;
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Middle)) => {
                if let (Some(p), Some(on_close)) = (over, &self.on_close) {
                    let x = in_row(state, p);
                    if let Some(i) = spans
                        .iter()
                        .position(|(left, w)| x >= *left && x < left + w)
                    {
                        shell.publish(on_close(i));
                        shell.capture_event();
                        return;
                    }
                }
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(p) = over {
                    let x = in_row(state, p);
                    let tab = spans
                        .iter()
                        .position(|(left, w)| x >= *left && x < left + w);
                    state.press = tab.map(|i| (i, x));
                    // A second click on the same tab, at once: a double click.
                    let now = std::time::Instant::now();
                    let double = tab.is_some()
                        && state.last_click.is_some_and(|(i, at)| {
                            Some(i) == tab && now.duration_since(at) < DOUBLE_CLICK
                        });
                    if let (true, Some(i), Some(on_double)) = (double, tab, &self.on_double_click) {
                        shell.publish(on_double(i));
                        state.last_click = None;
                    } else {
                        state.last_click = tab.map(|i| (i, now));
                    }
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                if let (None, Some((index, start))) = (state.drag, state.press) {
                    let x = in_row(state, *position);
                    if (x - start).abs() > DRAG_START {
                        let left = spans.get(index).map_or(0.0, |s| s.0);
                        state.drag = Some(Drag {
                            index,
                            grab: start - left,
                            x,
                        });
                    }
                }
                if state.drag.is_some() {
                    // Near an edge, the row scrolls under the dragged tab.
                    if position.x < bounds.x + EDGE {
                        state.offset -= 8.0;
                    } else if position.x > bounds.x + bounds.width - EDGE {
                        state.offset += 8.0;
                    }
                    state.clamp();
                    let x = in_row(state, *position);
                    if let Some(drag) = &mut state.drag {
                        drag.x = x;
                    }
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                state.press = None;
                if let Some(drag) = state.drag.take() {
                    let to = target(&spans, &drag);
                    if to != drag.index {
                        shell.publish((self.on_move)(drag.index, to));
                    }
                    shell.request_redraw();
                }
            }
            _ => {}
        }

        let dragging = state.drag.is_some();
        let offset = state.offset;
        // The tabs see the pointer where it is on them; while one is dragged
        // they don't light up under it.
        let inner = if dragging {
            cursor.levitate()
        } else {
            match over {
                Some(p) => mouse::Cursor::Available(p + Vector::new(offset, 0.0)),
                None => cursor.levitate() + Vector::new(offset, 0.0),
            }
        };
        let inner_viewport = Rectangle {
            x: bounds.x + offset,
            ..bounds
        };
        for ((tab, state), layout) in self
            .tabs
            .iter_mut()
            .zip(&mut tree.children)
            .zip(layout.children())
        {
            tab.as_widget_mut().update(
                state,
                event,
                layout,
                inner,
                renderer,
                clipboard,
                shell,
                &inner_viewport,
            );
        }
        let _ = viewport;
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        let state = tree.state.downcast_ref::<State>();
        if state.drag.is_some() {
            return mouse::Interaction::Grabbing;
        }
        let bounds = layout.bounds();
        let Some(p) = cursor.position_over(bounds) else {
            return mouse::Interaction::None;
        };
        let inner = mouse::Cursor::Available(p + Vector::new(state.offset, 0.0));
        self.tabs
            .iter()
            .zip(&tree.children)
            .zip(layout.children())
            .map(|((tab, state), layout)| {
                tab.as_widget()
                    .mouse_interaction(state, layout, inner, viewport, renderer)
            })
            .max()
            .unwrap_or_default()
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
        let state = tree.state.downcast_ref::<State>();
        let bounds = layout.bounds();
        let Some(visible) = bounds.intersection(viewport) else {
            return;
        };
        let spans = spans(layout);
        let offset = state.offset;
        let drag = state.drag;
        let to = drag.map(|d| target(&spans, &d));
        let dragged_width = drag.and_then(|d| spans.get(d.index)).map_or(0.0, |s| s.1);
        let inner = match (drag, cursor.position_over(bounds)) {
            (None, Some(p)) => mouse::Cursor::Available(p + Vector::new(offset, 0.0)),
            _ => mouse::Cursor::Unavailable,
        };
        let inner_viewport = Rectangle {
            x: visible.x + offset,
            ..visible
        };

        renderer.with_layer(visible, |renderer| {
            renderer.with_translation(Vector::new(-offset, 0.0), |renderer| {
                for (i, ((tab, child), layout)) in self
                    .tabs
                    .iter()
                    .zip(&tree.children)
                    .zip(layout.children())
                    .enumerate()
                {
                    if drag.is_some_and(|d| d.index == i) {
                        continue;
                    }
                    let moved = match (drag, to) {
                        (Some(d), Some(to)) => shift(i, &d, to, dragged_width),
                        _ => 0.0,
                    };
                    renderer.with_translation(Vector::new(moved, 0.0), |renderer| {
                        tab.as_widget().draw(
                            child,
                            renderer,
                            theme,
                            style,
                            layout,
                            inner,
                            &inner_viewport,
                        );
                    });
                }
            });
        });

        // The dragged tab, on top of the others, under the pointer.
        if let Some(d) = drag {
            if let (Some(tab), Some(child), Some(tab_layout)) = (
                self.tabs.get(d.index),
                tree.children.get(d.index),
                layout.children().nth(d.index),
            ) {
                let b = tab_layout.bounds();
                let left = bounds.x + (d.x - d.grab) - offset;
                renderer.with_layer(visible, |renderer| {
                    renderer.fill_quad(
                        renderer::Quad {
                            bounds: Rectangle { x: left, ..b },
                            border: iced::Border {
                                radius: 0.0.into(),
                                ..iced::Border::default()
                            },
                            ..renderer::Quad::default()
                        },
                        self.background,
                    );
                    renderer.with_translation(Vector::new(left - b.x, 0.0), |renderer| {
                        tab.as_widget().draw(
                            child,
                            renderer,
                            theme,
                            style,
                            tab_layout,
                            mouse::Cursor::Unavailable,
                            &Rectangle::INFINITE,
                        );
                    });
                });
            }
        }

        // A thin bar at the bottom says how far it is scrolled.
        if state.content > bounds.width + 0.5 {
            let ratio = bounds.width / state.content;
            renderer.with_layer(visible, |renderer| {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle {
                            x: bounds.x + offset * ratio,
                            y: bounds.y + bounds.height - 3.0,
                            width: (bounds.width * ratio).max(24.0),
                            height: 3.0,
                        },
                        border: iced::Border {
                            radius: 0.0.into(),
                            ..iced::Border::default()
                        },
                        ..renderer::Quad::default()
                    },
                    self.scrollbar,
                );
            });
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, iced::Theme, iced::Renderer>> {
        let offset = tree.state.downcast_ref::<State>().offset;
        overlay::from_children(
            &mut self.tabs,
            tree,
            layout,
            renderer,
            viewport,
            translation - Vector::new(offset, 0.0),
        )
    }
}

impl<'a, Message: 'a> From<TabStrip<'a, Message>> for Element<'a, Message> {
    fn from(strip: TabStrip<'a, Message>) -> Self {
        Element::new(strip)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPANS: [(f32, f32); 4] = [(0.0, 100.0), (100.0, 60.0), (160.0, 80.0), (240.0, 50.0)];

    fn drag(index: usize, left: f32) -> Drag {
        // Taken by its left edge, the tab's left edge is where the pointer is.
        Drag {
            index,
            grab: 0.0,
            x: left,
        }
    }

    #[test]
    fn a_dragged_tab_goes_past_those_whose_middle_it_passed() {
        // Not moved: it stays.
        assert_eq!(target(&SPANS, &drag(0, 0.0)), 0);
        // The first tab (100 wide) moved right by 40: its middle (90) is
        // past the second's (130)? No: it stays.
        assert_eq!(target(&SPANS, &drag(0, 40.0)), 0);
        // By 90: its middle (140) is past the second's (130).
        assert_eq!(target(&SPANS, &drag(0, 90.0)), 1);
        // To the far right: last.
        assert_eq!(target(&SPANS, &drag(0, 400.0)), 3);
        // The last tab brought to the far left: first.
        assert_eq!(target(&SPANS, &drag(3, -50.0)), 0);
        assert_eq!(target(&SPANS, &drag(3, 150.0)), 2);
    }

    #[test]
    fn the_others_make_room_for_the_dragged_tab() {
        let d = drag(0, 200.0);
        // Moving the first to place 2: the second and third slide left by
        // its width, the fourth stays.
        assert_eq!(shift(1, &d, 2, 100.0), -100.0);
        assert_eq!(shift(2, &d, 2, 100.0), -100.0);
        assert_eq!(shift(3, &d, 2, 100.0), 0.0);
        // Moving the last to place 1: the second and third slide right.
        let d = drag(3, 120.0);
        assert_eq!(shift(0, &d, 1, 50.0), 0.0);
        assert_eq!(shift(1, &d, 1, 50.0), 50.0);
        assert_eq!(shift(2, &d, 1, 50.0), 50.0);
    }

    /// Lays a strip of tabs like those of the app (a button over an
    /// underline that fills its width) out in `width` pixels.
    fn laid_out(names: &[&str], active: Option<usize>, width: f32) -> (layout::Node, Tree) {
        use iced::widget::{button, column, container, text, Space};
        let renderer = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(<iced::Renderer as iced::advanced::renderer::Headless>::new(
                iced::Font::DEFAULT,
                iced::Pixels(14.0),
                Some("tiny-skia"),
            ))
            .expect("a renderer without a window");
        let tabs: Vec<Element<'_, ()>> = names
            .iter()
            .map(|name| {
                column![
                    button(text(name.to_string())).on_press(()),
                    container(Space::new().height(2)).width(Length::Fill),
                ]
                .into()
            })
            .collect();
        let mut strip: Element<'_, ()> =
            tab_strip(tabs, active, |_, _| (), Color::BLACK, Color::BLACK).into();
        let mut tree = Tree::new(&strip);
        let node = strip.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, Size::new(width, 200.0)),
        );
        (node, tree)
    }

    #[test]
    fn each_tab_takes_its_own_width_one_after_the_other() {
        let names = ["main.rs", "a-much-longer-name.ts", "x", "compose.yml"];
        let (node, _) = laid_out(&names, Some(0), 2000.0);
        let tabs = node.children();
        assert_eq!(tabs.len(), 4);
        let mut right = 0.0;
        for tab in tabs {
            let b = tab.bounds();
            assert!(
                b.width.is_finite() && b.width > 0.0 && b.width < 400.0,
                "{b:?}"
            );
            assert_eq!(b.x, right, "next to the one before");
            right = b.x + b.width;
        }
        // The longer the name, the wider the tab.
        assert!(tabs[1].bounds().width > tabs[2].bounds().width);
        assert_eq!(node.bounds().width, 2000.0, "the strip fills its room");
    }

    #[test]
    fn the_active_tab_is_scrolled_into_view() {
        let names: Vec<String> = (0..30).map(|i| format!("file-{i}.rs")).collect();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        let (node, tree) = laid_out(&names, Some(25), 300.0);
        let state = tree.state.downcast_ref::<State>();
        let tab = node.children()[25].bounds();
        assert!(state.offset > 0.0);
        // It is fully in the room shown.
        assert!(tab.x >= state.offset && tab.x + tab.width <= state.offset + 300.0 + 0.01);
        // The first tab needs no scroll.
        let (_, tree) = laid_out(&names, Some(0), 300.0);
        assert_eq!(tree.state.downcast_ref::<State>().offset, 0.0);
    }

    #[test]
    fn the_scroll_stays_within_the_row() {
        let mut state = State {
            content: 500.0,
            width: 200.0,
            offset: 400.0,
            ..State::default()
        };
        state.clamp();
        assert_eq!(state.offset, 300.0);
        state.offset = -20.0;
        state.clamp();
        assert_eq!(state.offset, 0.0);
        // A row that fits does not scroll.
        state.content = 150.0;
        state.offset = 30.0;
        state.clamp();
        assert_eq!(state.offset, 0.0);
    }
}
