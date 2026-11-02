//! The small drawn icon of the Git button: a branch (two dots on a line and
//! one on a curve).

use iced::mouse;
use iced::widget::canvas::{self, Canvas, Frame, Geometry, Path, Stroke};
use iced::{Color, Element, Length, Point, Rectangle, Theme};

struct Branch {
    color: Color,
}

impl<Message> canvas::Program<Message> for Branch {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let stroke = Stroke::default().with_color(self.color).with_width(1.4);
        // The trunk, and the branch that leaves it and comes back.
        let (x, top, bottom, side) = (4.5, 2.5, 13.5, 11.5);
        frame.stroke(
            &Path::line(Point::new(x, top + 2.0), Point::new(x, bottom - 2.0)),
            stroke,
        );
        let curve = Path::new(|p| {
            p.move_to(Point::new(side, top + 2.2));
            p.bezier_curve_to(
                Point::new(side, 8.0),
                Point::new(x, 7.0),
                Point::new(x, 9.0),
            );
        });
        frame.stroke(&curve, stroke);
        for at in [
            Point::new(x, top),
            Point::new(x, bottom),
            Point::new(side, top),
        ] {
            frame.stroke(&Path::circle(at, 1.9), stroke);
        }
        vec![frame.into_geometry()]
    }
}

/// The icon, 16 px wide.
pub fn branch<'a, Message: 'a>(color: Color) -> Element<'a, Message> {
    Canvas::new(Branch { color })
        .width(Length::Fixed(16.0))
        .height(Length::Fixed(16.0))
        .into()
}
