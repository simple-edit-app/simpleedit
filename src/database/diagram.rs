//! The diagram of the tables of a schema and of the foreign keys between
//! them: where each table goes, what is drawn, and the same as Mermaid text.

use super::protocol::Catalog;

pub const BOX_WIDTH: f32 = 220.0;
pub const HEADER: f32 = 28.0;
pub const ROW: f32 = 18.0;
const GAP_X: f32 = 70.0;
const GAP_Y: f32 = 50.0;
const MARGIN: f32 = 24.0;
/// Most columns drawn in a box (the rest are counted).
const MAX_ROWS: usize = 14;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagramColumn {
    pub name: String,
    pub kind: String,
    /// Probably the key of the table: called `id`, or pointed to by another
    /// table.
    pub key: bool,
    /// Points to another table.
    pub foreign: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DiagramTable {
    pub schema: String,
    pub name: String,
    pub columns: Vec<DiagramColumn>,
    /// Columns that are not drawn.
    pub hidden: usize,
    pub x: f32,
    pub y: f32,
    pub height: f32,
}

/// A foreign key: from a table to another.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub from: usize,
    pub to: usize,
    pub columns: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Diagram {
    pub tables: Vec<DiagramTable>,
    pub links: Vec<Link>,
    pub width: f32,
    pub height: f32,
}

/// The diagram of the tables of `schema` (those of other schemas that they
/// point to come too, named with their schema).
pub fn build(catalog: &Catalog, schema: &str) -> Diagram {
    let mut tables: Vec<DiagramTable> = Vec::new();
    let index = |tables: &Vec<DiagramTable>, s: &str, n: &str| {
        tables.iter().position(|t| t.schema == s && t.name == n)
    };
    for column in catalog.columns.iter().filter(|c| c.schema == schema) {
        let at = match index(&tables, &column.schema, &column.table) {
            Some(at) => at,
            None => {
                tables.push(DiagramTable {
                    schema: column.schema.clone(),
                    name: column.table.clone(),
                    columns: Vec::new(),
                    hidden: 0,
                    x: 0.0,
                    y: 0.0,
                    height: 0.0,
                });
                tables.len() - 1
            }
        };
        tables[at].columns.push(DiagramColumn {
            name: column.name.clone(),
            kind: column.kind.clone(),
            key: column.name == "id",
            foreign: false,
        });
    }
    let mut links = Vec::new();
    for fk in catalog.foreign_keys.iter().filter(|f| f.schema == schema) {
        let Some(from) = index(&tables, &fk.schema, &fk.table) else {
            continue;
        };
        // A table of another schema is added, with its columns.
        let to = match index(&tables, &fk.ref_schema, &fk.ref_table) {
            Some(to) => to,
            None => {
                tables.push(DiagramTable {
                    schema: fk.ref_schema.clone(),
                    name: fk.ref_table.clone(),
                    columns: catalog
                        .columns
                        .iter()
                        .filter(|c| c.schema == fk.ref_schema && c.table == fk.ref_table)
                        .map(|c| DiagramColumn {
                            name: c.name.clone(),
                            kind: c.kind.clone(),
                            key: c.name == "id",
                            foreign: false,
                        })
                        .collect(),
                    hidden: 0,
                    x: 0.0,
                    y: 0.0,
                    height: 0.0,
                });
                tables.len() - 1
            }
        };
        for name in &fk.columns {
            if let Some(c) = tables[from].columns.iter_mut().find(|c| c.name == *name) {
                c.foreign = true;
            }
        }
        for name in &fk.ref_columns {
            if let Some(c) = tables[to].columns.iter_mut().find(|c| c.name == *name) {
                c.key = true;
            }
        }
        links.push(Link {
            from,
            to,
            columns: fk.columns.join(", "),
        });
    }
    layout(&mut tables, &links);
    let width = tables.iter().map(|t| t.x + BOX_WIDTH).fold(0.0, f32::max) + MARGIN;
    let height = tables.iter().map(|t| t.y + t.height).fold(0.0, f32::max) + MARGIN;
    Diagram {
        tables,
        links,
        width,
        height,
    }
}

/// The boxes in rows, the most connected first, so that the links that
/// matter are short.
fn layout(tables: &mut [DiagramTable], links: &[Link]) {
    let mut degree = vec![0usize; tables.len()];
    for l in links {
        degree[l.from] += 1;
        degree[l.to] += 1;
    }
    let mut order: Vec<usize> = (0..tables.len()).collect();
    order.sort_by(|a, b| {
        degree[*b]
            .cmp(&degree[*a])
            .then_with(|| tables[*a].name.cmp(&tables[*b].name))
    });
    for t in tables.iter_mut() {
        if t.columns.len() > MAX_ROWS {
            t.hidden = t.columns.len() - MAX_ROWS;
            t.columns.truncate(MAX_ROWS);
        }
        t.height = HEADER + ROW * (t.columns.len() + usize::from(t.hidden > 0)) as f32 + 8.0;
    }
    let per_row = (tables.len() as f32).sqrt().ceil().max(1.0) as usize;
    let mut y = MARGIN;
    for row in order.chunks(per_row) {
        let mut tallest: f32 = 0.0;
        for (n, &i) in row.iter().enumerate() {
            tables[i].x = MARGIN + n as f32 * (BOX_WIDTH + GAP_X);
            tables[i].y = y;
            tallest = tallest.max(tables[i].height);
        }
        y += tallest + GAP_Y;
    }
}

fn word(text: &str) -> String {
    let cleaned: String = text
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "x".to_string()
    } else {
        cleaned
    }
}

/// The diagram as Mermaid text (`erDiagram`).
pub fn to_mermaid(diagram: &Diagram) -> String {
    let name = |t: &DiagramTable| word(&t.name);
    let mut out = String::from("erDiagram\n");
    for t in &diagram.tables {
        out.push_str(&format!("  {} {{\n", name(t)));
        for c in &t.columns {
            let mark = match (c.key, c.foreign) {
                (true, true) => " PK, FK",
                (true, false) => " PK",
                (false, true) => " FK",
                (false, false) => "",
            };
            out.push_str(&format!("    {} {}{mark}\n", word(&c.kind), word(&c.name)));
        }
        out.push_str("  }\n");
    }
    for l in &diagram.links {
        out.push_str(&format!(
            "  {} ||--o{{ {} : \"{}\"\n",
            name(&diagram.tables[l.to]),
            name(&diagram.tables[l.from]),
            l.columns.replace('"', "'")
        ));
    }
    out
}

/// Draws the diagram on a canvas.
pub struct Drawing<'a> {
    pub diagram: &'a Diagram,
    pub dark: bool,
}

impl<M> iced::widget::canvas::Program<M> for Drawing<'_> {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: iced::Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<iced::widget::canvas::Geometry> {
        use iced::widget::canvas::{Frame, Path, Stroke, Text};
        use iced::{Point, Size};
        let p = crate::theme::palette(self.dark);
        let mut frame = Frame::new(renderer, bounds.size());
        let d = self.diagram;
        // The links first, under the boxes: from the side of a box that
        // faces the other one.
        for l in &d.links {
            let (a, b) = (&d.tables[l.from], &d.tables[l.to]);
            let (ax, bx) = if a.x < b.x {
                (a.x + BOX_WIDTH, b.x)
            } else if a.x > b.x {
                (a.x, b.x + BOX_WIDTH)
            } else {
                (a.x + BOX_WIDTH, b.x + BOX_WIDTH)
            };
            let (ay, by) = (a.y + HEADER / 2.0, b.y + HEADER / 2.0);
            let bend = if a.x == b.x { 40.0 } else { 0.0 };
            let line = Path::new(|path| {
                path.move_to(Point::new(ax, ay));
                if bend > 0.0 {
                    path.line_to(Point::new(ax + bend, ay));
                    path.line_to(Point::new(bx + bend, by));
                }
                path.line_to(Point::new(bx, by));
            });
            frame.stroke(&line, Stroke::default().with_color(p.muted).with_width(1.2));
            frame.fill(&Path::circle(Point::new(bx, by), 3.5), p.accent);
        }
        for t in &d.tables {
            let rect = Path::rectangle(Point::new(t.x, t.y), Size::new(BOX_WIDTH, t.height));
            frame.fill(&rect, p.elevated);
            frame.stroke(
                &rect,
                Stroke::default()
                    .with_color(p.border_strong)
                    .with_width(1.0),
            );
            frame.fill(
                &Path::rectangle(Point::new(t.x, t.y), Size::new(BOX_WIDTH, HEADER)),
                p.accent_soft,
            );
            let title = if t.schema.is_empty() {
                t.name.clone()
            } else {
                format!("{}.{}", t.schema, t.name)
            };
            frame.fill_text(Text {
                content: title,
                position: Point::new(t.x + 10.0, t.y + 6.0),
                color: p.text,
                size: 13.0.into(),
                font: crate::theme::UI_SEMIBOLD,
                ..Text::default()
            });
            for (i, c) in t.columns.iter().enumerate() {
                let y = t.y + HEADER + 4.0 + i as f32 * ROW;
                let color = if c.foreign { p.accent } else { p.text };
                let mark = match (c.key, c.foreign) {
                    (true, true) => "PK FK",
                    (true, false) => "PK",
                    (false, true) => "FK",
                    (false, false) => "",
                };
                frame.fill_text(Text {
                    content: mark.to_string(),
                    position: Point::new(t.x + 8.0, y),
                    color: p.muted,
                    size: 10.5.into(),
                    font: crate::theme::MONO,
                    ..Text::default()
                });
                frame.fill_text(Text {
                    content: c.name.clone(),
                    position: Point::new(t.x + 46.0, y),
                    color,
                    size: 12.0.into(),
                    font: crate::theme::MONO,
                    ..Text::default()
                });
                let kind: String = c.kind.chars().take(14).collect();
                frame.fill_text(Text {
                    content: kind,
                    position: Point::new(t.x + BOX_WIDTH - 96.0, y),
                    color: p.muted,
                    size: 11.0.into(),
                    font: crate::theme::MONO,
                    ..Text::default()
                });
            }
            if t.hidden > 0 {
                frame.fill_text(Text {
                    content: format!("… +{}", t.hidden),
                    position: Point::new(
                        t.x + 46.0,
                        t.y + HEADER + 4.0 + t.columns.len() as f32 * ROW,
                    ),
                    color: p.muted,
                    size: 11.0.into(),
                    ..Text::default()
                });
            }
        }
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::protocol::{ForeignKey, TableColumn};

    fn column(schema: &str, table: &str, name: &str, kind: &str) -> TableColumn {
        TableColumn {
            schema: schema.into(),
            table: table.into(),
            name: name.into(),
            kind: kind.into(),
        }
    }

    fn catalog() -> Catalog {
        Catalog {
            columns: vec![
                column("shop", "customers", "id", "integer"),
                column("shop", "customers", "name", "text"),
                column("shop", "orders", "id", "integer"),
                column("shop", "orders", "customer_id", "integer"),
                column("shop", "orders", "total", "numeric(8,2)"),
                column("other", "countries", "code", "text"),
                column("other", "ignored", "x", "text"),
            ],
            foreign_keys: vec![
                ForeignKey {
                    schema: "shop".into(),
                    table: "orders".into(),
                    columns: vec!["customer_id".into()],
                    ref_schema: "shop".into(),
                    ref_table: "customers".into(),
                    ref_columns: vec!["id".into()],
                },
                ForeignKey {
                    schema: "shop".into(),
                    table: "customers".into(),
                    columns: vec!["name".into()],
                    ref_schema: "other".into(),
                    ref_table: "countries".into(),
                    ref_columns: vec!["code".into()],
                },
                ForeignKey {
                    schema: "other".into(),
                    table: "ignored".into(),
                    columns: vec!["x".into()],
                    ref_schema: "other".into(),
                    ref_table: "countries".into(),
                    ref_columns: vec!["code".into()],
                },
            ],
        }
    }

    #[test]
    fn the_tables_of_the_schema_and_the_ones_they_point_to_are_drawn() {
        let d = build(&catalog(), "shop");
        let names: Vec<&str> = d.tables.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names.len(), 3, "{names:?}");
        assert!(names.contains(&"countries") && !names.contains(&"ignored"));
        assert_eq!(
            d.links.len(),
            2,
            "the links of other schemas' tables stay out"
        );
        let orders = d.tables.iter().find(|t| t.name == "orders").unwrap();
        let by = |name: &str| orders.columns.iter().find(|c| c.name == name).unwrap();
        assert!(by("id").key && !by("id").foreign);
        assert!(by("customer_id").foreign && !by("customer_id").key);
        let countries = d.tables.iter().find(|t| t.name == "countries").unwrap();
        assert!(countries.columns[0].key, "pointed to: a key");
    }

    #[test]
    fn the_boxes_do_not_overlap_and_the_diagram_holds_them() {
        let d = build(&catalog(), "shop");
        for (i, a) in d.tables.iter().enumerate() {
            assert!(a.x + BOX_WIDTH <= d.width && a.y + a.height <= d.height);
            for b in &d.tables[i + 1..] {
                let apart = a.x + BOX_WIDTH <= b.x
                    || b.x + BOX_WIDTH <= a.x
                    || a.y + a.height <= b.y
                    || b.y + b.height <= a.y;
                assert!(apart, "{} and {} overlap", a.name, b.name);
            }
        }
        assert_eq!(build(&catalog(), "nothing").tables.len(), 0);
    }

    #[test]
    fn long_tables_are_cut_and_counted() {
        let mut c = Catalog::default();
        for i in 0..20 {
            c.columns
                .push(column("s", "wide", &format!("c{i}"), "text"));
        }
        let d = build(&c, "s");
        assert_eq!(
            (d.tables[0].columns.len(), d.tables[0].hidden),
            (MAX_ROWS, 6)
        );
    }

    #[test]
    fn mermaid_names_the_tables_the_keys_and_the_links() {
        let text = to_mermaid(&build(&catalog(), "shop"));
        assert!(text.starts_with("erDiagram\n"));
        assert!(text.contains("  orders {\n    integer id PK\n    integer customer_id FK\n    numeric_8_2_ total\n  }"));
        assert!(text.contains("  customers ||--o{ orders : \"customer_id\""));
    }
}
