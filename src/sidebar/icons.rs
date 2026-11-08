//! The small drawn icons of the bar beside the panels.

use iced::widget::canvas::{Canvas, Frame, Path, Program, Stroke};
use iced::{mouse, Color, Element, Length, Point, Rectangle, Renderer, Theme};

#[derive(Clone, Copy)]
enum Shape {
    /// A folder.
    Folder,
    /// Two sheets, one behind the other.
    Files,
    /// A database: a stack of discs.
    Database,
    /// A console: a window with a prompt.
    Console,
}

struct Icon {
    shape: Shape,
    color: Color,
}

impl<Message> Program<Message> for Icon {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<iced::widget::canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let stroke = Stroke::default().with_color(self.color).with_width(1.4);
        match self.shape {
            Shape::Folder => {
                let outline = Path::new(|p| {
                    p.move_to(Point::new(1.5, 4.0));
                    p.line_to(Point::new(1.5, 13.5));
                    p.line_to(Point::new(14.5, 13.5));
                    p.line_to(Point::new(14.5, 5.5));
                    p.line_to(Point::new(7.5, 5.5));
                    p.line_to(Point::new(6.0, 3.0));
                    p.line_to(Point::new(2.5, 3.0));
                    p.line_to(Point::new(1.5, 4.0));
                });
                frame.stroke(&outline, stroke);
            }
            Shape::Console => {
                frame.stroke(
                    &Path::rectangle(Point::new(1.5, 2.5), iced::Size::new(13.0, 11.0)),
                    stroke,
                );
                // The prompt: a chevron, then a cursor.
                frame.stroke(
                    &Path::new(|p| {
                        p.move_to(Point::new(4.5, 6.0));
                        p.line_to(Point::new(7.0, 8.2));
                        p.line_to(Point::new(4.5, 10.4));
                    }),
                    stroke,
                );
                frame.stroke(
                    &Path::line(Point::new(8.5, 10.6), Point::new(11.5, 10.6)),
                    stroke,
                );
            }
            Shape::Database => {
                // The top disc, then the sides and the two bands below it.
                let top = Path::new(|p| {
                    p.move_to(Point::new(2.0, 4.0));
                    p.line_to(Point::new(2.0, 12.0));
                });
                let right = Path::line(Point::new(14.0, 4.0), Point::new(14.0, 12.0));
                frame.stroke(&top, stroke);
                frame.stroke(&right, stroke);
                for y in [4.0_f32, 8.0, 12.0] {
                    frame.stroke(
                        &Path::new(|p| {
                            p.move_to(Point::new(2.0, y));
                            p.quadratic_curve_to(Point::new(8.0, y + 3.0), Point::new(14.0, y));
                        }),
                        stroke,
                    );
                }
                frame.stroke(
                    &Path::new(|p| {
                        p.move_to(Point::new(2.0, 4.0));
                        p.quadratic_curve_to(Point::new(8.0, 1.0), Point::new(14.0, 4.0));
                    }),
                    stroke,
                );
            }
            Shape::Files => {
                // The sheet behind, then the one in front.
                frame.stroke(
                    &Path::new(|p| {
                        p.move_to(Point::new(5.0, 4.0));
                        p.line_to(Point::new(5.0, 1.5));
                        p.line_to(Point::new(13.5, 1.5));
                        p.line_to(Point::new(13.5, 11.0));
                        p.line_to(Point::new(11.0, 11.0));
                    }),
                    stroke,
                );
                frame.stroke(
                    &Path::rectangle(Point::new(2.0, 4.0), iced::Size::new(9.0, 10.5)),
                    stroke,
                );
                for y in [7.5, 10.5] {
                    frame.stroke(&Path::line(Point::new(4.0, y), Point::new(9.0, y)), stroke);
                }
            }
        }
        vec![frame.into_geometry()]
    }
}

fn icon<'a, Message: 'a>(shape: Shape, color: Color) -> Element<'a, Message> {
    Canvas::new(Icon { shape, color })
        .width(Length::Fixed(16.0))
        .height(Length::Fixed(16.0))
        .into()
}

pub fn folder<'a, Message: 'a>(color: Color) -> Element<'a, Message> {
    icon(Shape::Folder, color)
}

pub fn console<'a, Message: 'a>(color: Color) -> Element<'a, Message> {
    icon(Shape::Console, color)
}

pub fn database<'a, Message: 'a>(color: Color) -> Element<'a, Message> {
    icon(Shape::Database, color)
}

pub fn files<'a, Message: 'a>(color: Color) -> Element<'a, Message> {
    icon(Shape::Files, color)
}
