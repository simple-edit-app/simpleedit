//! A collapsible JSON tree view.
//!
//! Object/array nodes carry a chevron; the set of collapsed node paths lives in
//! the app state so the tree can be rebuilt from the parsed document on every
//! frame without losing its open/closed state.

use std::collections::HashSet;

use iced::{
    widget::{button, column, container, row, scrollable, text, Space},
    Alignment, Color, Element, Font, Length,
};
use serde_json::Value;

use crate::app::Message;
use crate::theme;

/// Path of the document root; every other path is derived from it.
pub const ROOT: &str = "$";

/// Safety valve: a pathological document must not build millions of widgets.
const MAX_ROWS: usize = 20_000;
const INDENT: f32 = 16.0;
const CHEVRON_WIDTH: f32 = 18.0;
const SIZE: f32 = 13.0;

/// Parses the buffer, returning the parser message on failure.
pub fn parse(src: &str) -> Result<Value, String> {
    serde_json::from_str(src).map_err(|e| e.to_string())
}

/// Every collapsible path in the document (non-empty objects and arrays).
pub fn container_paths(value: &Value) -> HashSet<String> {
    let mut paths = HashSet::new();
    collect_paths(value, ROOT, &mut paths);
    paths
}

fn collect_paths(value: &Value, path: &str, out: &mut HashSet<String>) {
    match value {
        Value::Object(map) if !map.is_empty() => {
            out.insert(path.to_string());
            for (key, child) in map {
                collect_paths(child, &child_path(path, key, None), out);
            }
        }
        Value::Array(items) if !items.is_empty() => {
            out.insert(path.to_string());
            for (i, child) in items.iter().enumerate() {
                collect_paths(child, &child_path(path, "", Some(i)), out);
            }
        }
        _ => {}
    }
}

fn child_path(parent: &str, key: &str, index: Option<usize>) -> String {
    match index {
        Some(i) => format!("{}[{}]", parent, i),
        None => format!("{}.{}", parent, key),
    }
}

fn is_container(value: &Value) -> bool {
    match value {
        Value::Object(map) => !map.is_empty(),
        Value::Array(items) => !items.is_empty(),
        _ => false,
    }
}

/// The delimiters and child count of a container.
fn brackets(value: &Value) -> (&'static str, &'static str, usize) {
    match value {
        Value::Object(map) => ("{", "}", map.len()),
        Value::Array(items) => ("[", "]", items.len()),
        _ => ("", "", 0),
    }
}

// ─── Rendering ──────────────────────────────────────────────────────────────

fn string_color(dark: bool) -> Color {
    if dark {
        Color::from_rgb(0.30, 0.75, 0.56)
    } else {
        Color::from_rgb(0.13, 0.45, 0.32)
    }
}

fn number_color(dark: bool) -> Color {
    if dark {
        Color::from_rgb(0.90, 0.66, 0.38)
    } else {
        Color::from_rgb(0.66, 0.38, 0.13)
    }
}

fn literal_color(dark: bool) -> Color {
    if dark {
        Color::from_rgb(0.78, 0.60, 0.92)
    } else {
        Color::from_rgb(0.51, 0.29, 0.71)
    }
}

fn scalar_color(value: &Value, dark: bool) -> Color {
    match value {
        Value::String(_) => string_color(dark),
        Value::Number(_) => number_color(dark),
        _ => literal_color(dark),
    }
}

/// A scalar rendered as JSON (quoted and escaped for strings).
fn scalar_text(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".to_string())
}

fn mono(content: String, size: f32, color: Color) -> iced::widget::Text<'static> {
    // Advanced shaping so a glyph missing from the monospace font (emoji in a
    // string value, chiefly) falls back to another installed font.
    text(content)
        .size(size)
        .font(Font::MONOSPACE)
        .style(color)
        .shaping(iced::widget::text::Shaping::Advanced)
}

/// The scrollable tree. `collapsed` holds the paths currently folded shut.
pub fn view(value: &Value, collapsed: &HashSet<String>, dark: bool) -> Element<'static, Message> {
    let mut rows: Vec<Element<'static, Message>> = Vec::new();
    let mut budget = MAX_ROWS;
    push_node(
        &mut rows,
        &mut budget,
        value,
        ROOT,
        None,
        0,
        false,
        collapsed,
        dark,
    );

    if budget == 0 {
        rows.push(
            mono(
                t!("preview.truncated").to_string(),
                SIZE,
                theme::muted_text(dark),
            )
            .into(),
        );
    }

    scrollable(
        container(column(rows).spacing(1))
            .width(Length::Fill)
            .padding([12, 16]),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// Appends the rows for one node (and its children when expanded).
#[allow(clippy::too_many_arguments)]
fn push_node(
    rows: &mut Vec<Element<'static, Message>>,
    budget: &mut usize,
    value: &Value,
    path: &str,
    key: Option<&str>,
    depth: usize,
    comma: bool,
    collapsed: &HashSet<String>,
    dark: bool,
) {
    if *budget == 0 {
        return;
    }
    *budget -= 1;

    let tail = if comma { "," } else { "" };
    let muted = theme::muted_text(dark);

    if !is_container(value) {
        let value_text = match value {
            Value::Object(_) => "{}".to_string(),
            Value::Array(_) => "[]".to_string(),
            _ => scalar_text(value),
        };
        let color = match value {
            Value::Object(_) | Value::Array(_) => muted,
            _ => scalar_color(value, dark),
        };
        rows.push(
            line(depth)
                .push(Space::with_width(Length::Fixed(CHEVRON_WIDTH)))
                .push(key_label(key))
                .push(mono(format!("{}{}", value_text, tail), SIZE, color))
                .into(),
        );
        return;
    }

    let (open, close, count) = brackets(value);
    let folded = collapsed.contains(path);

    if folded {
        rows.push(
            line(depth)
                .push(chevron(path.to_string(), false, dark))
                .push(key_label(key))
                .push(mono(format!("{} … {}{}", open, close, tail), SIZE, muted))
                .push(mono(format!("  ({})", count), SIZE - 1.0, muted))
                .into(),
        );
        return;
    }

    rows.push(
        line(depth)
            .push(chevron(path.to_string(), true, dark))
            .push(key_label(key))
            .push(mono(open.to_string(), SIZE, muted))
            .into(),
    );

    match value {
        Value::Object(map) => {
            let last = map.len() - 1;
            for (i, (child_key, child)) in map.iter().enumerate() {
                push_node(
                    rows,
                    budget,
                    child,
                    &child_path(path, child_key, None),
                    Some(child_key),
                    depth + 1,
                    i != last,
                    collapsed,
                    dark,
                );
            }
        }
        Value::Array(items) => {
            let last = items.len() - 1;
            for (i, child) in items.iter().enumerate() {
                push_node(
                    rows,
                    budget,
                    child,
                    &child_path(path, "", Some(i)),
                    None,
                    depth + 1,
                    i != last,
                    collapsed,
                    dark,
                );
            }
        }
        _ => {}
    }

    if *budget == 0 {
        return;
    }
    rows.push(
        line(depth)
            .push(Space::with_width(Length::Fixed(CHEVRON_WIDTH)))
            .push(mono(format!("{}{}", close, tail), SIZE, muted))
            .into(),
    );
}

fn line(depth: usize) -> iced::widget::Row<'static, Message> {
    row![Space::with_width(Length::Fixed(depth as f32 * INDENT))].align_items(Alignment::Center)
}

fn key_label(key: Option<&str>) -> Element<'static, Message> {
    match key {
        Some(k) => mono(
            format!("{}: ", scalar_text(&Value::String(k.to_string()))),
            SIZE,
            theme::accent_color(),
        )
        .into(),
        None => Space::with_width(Length::Fixed(0.0)).into(),
    }
}

fn chevron(path: String, open: bool, dark: bool) -> Element<'static, Message> {
    button(
        text(if open { "▼" } else { "▶" })
            .size(9)
            .style(theme::muted_text(dark)),
    )
    .padding([0, 4])
    .width(Length::Fixed(CHEVRON_WIDTH))
    .on_press(Message::JsonToggle(path))
    .style(iced::theme::Button::custom(theme::GhostButton {
        dark,
        active: false,
    }))
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(src: &str) -> Value {
        parse(src).expect("valid json")
    }

    #[test]
    fn parse_reports_errors() {
        assert!(parse("{").is_err());
        assert!(parse(r#"{"a": 1}"#).is_ok());
    }

    #[test]
    fn object_key_order_is_preserved() {
        let v = value(r#"{"b": 1, "a": 2, "c": 3}"#);
        let keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(keys, ["b", "a", "c"]);
    }

    #[test]
    fn container_paths_covers_nested_nodes() {
        let paths = container_paths(&value(r#"{"a": {"b": [1, {"c": 2}]}}"#));
        let mut sorted: Vec<&str> = paths.iter().map(String::as_str).collect();
        sorted.sort_unstable();
        assert_eq!(sorted, ["$", "$.a", "$.a.b", "$.a.b[1]"]);
    }

    #[test]
    fn container_paths_skips_empty_and_scalar_nodes() {
        let paths = container_paths(&value(r#"{"a": {}, "b": [], "c": 1}"#));
        assert_eq!(paths.len(), 1);
        assert!(paths.contains(ROOT));
    }

    #[test]
    fn scalars_render_as_json() {
        assert_eq!(scalar_text(&value(r#""a\"b""#)), r#""a\"b""#);
        assert_eq!(scalar_text(&value("1.5")), "1.5");
        assert_eq!(scalar_text(&value("null")), "null");
        assert_eq!(scalar_text(&value("true")), "true");
    }

    #[test]
    fn brackets_report_child_counts() {
        assert_eq!(brackets(&value(r#"{"a": 1, "b": 2}"#)), ("{", "}", 2));
        assert_eq!(brackets(&value("[1, 2, 3]")), ("[", "]", 3));
    }
}
