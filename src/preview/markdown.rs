//! A small block-level Markdown parser and its iced renderer.
//!
//! Deliberately partial: it covers what a text editor preview needs
//! (headings, paragraphs, lists, quotes, fenced code, rules and the common
//! inline markers) and renders everything else as plain text.

use std::collections::HashMap;

use iced::{
    advanced::widget::{operation, Id as WidgetId, Operation},
    alignment::Horizontal,
    mouse,
    widget::{column, container, mouse_area, row, scrollable, text, Space},
    Color, Command, Element, Font, Length, Rectangle, Vector,
};
use iced_aw::{Grid, GridRow};

use crate::app::Message;
use crate::theme;

/// Inline emphasis of a run of text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Plain,
    Bold,
    Italic,
    BoldItalic,
    Code,
    Link,
}

/// A run of text sharing one inline style.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub style: Style,
    /// The `(url)` half of a `[label](url)` link; `None` for every other
    /// style.
    pub href: Option<String>,
}

impl Span {
    fn new(text: impl Into<String>, style: Style) -> Self {
        Self {
            text: text.into(),
            style,
            href: None,
        }
    }

    fn link(text: impl Into<String>, href: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            style: Style::Link,
            href: Some(href.into()),
        }
    }
}

/// A GFM table column's declared alignment (from its separator cell, e.g.
/// `:---:`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

impl From<Align> for Horizontal {
    fn from(align: Align) -> Self {
        match align {
            Align::Left => Horizontal::Left,
            Align::Center => Horizontal::Center,
            Align::Right => Horizontal::Right,
        }
    }
}

/// A top-level Markdown block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    Heading {
        level: u8,
        spans: Vec<Span>,
    },
    Paragraph(Vec<Span>),
    Quote(Vec<Span>),
    ListItem {
        depth: usize,
        marker: String,
        spans: Vec<Span>,
    },
    Code(String),
    Rule,
    Table {
        alignments: Vec<Align>,
        header: Vec<Vec<Span>>,
        rows: Vec<Vec<Vec<Span>>>,
    },
}

// ─── Parsing ────────────────────────────────────────────────────────────────

/// Splits a Markdown document into blocks.
pub fn parse(src: &str) -> Vec<Block> {
    let mut blocks: Vec<Block> = Vec::new();
    let mut para: Vec<String> = Vec::new();
    let mut quote: Vec<String> = Vec::new();
    let mut code: Option<Vec<String>> = None;

    // A table's second line (the separator) is what confirms the first line
    // was a header, not a paragraph — indexed access lets us peek ahead for
    // it and then consume as many further rows as belong to the table.
    let lines: Vec<&str> = src.lines().collect();
    let mut idx = 0;

    while idx < lines.len() {
        let raw = lines[idx];

        // Inside a fenced block everything is literal until the closing fence.
        if let Some(buf) = code.as_mut() {
            if is_fence(raw.trim_start()) {
                blocks.push(Block::Code(buf.join("\n")));
                code = None;
            } else {
                buf.push(raw.to_string());
            }
            idx += 1;
            continue;
        }

        let line = raw.trim_end();
        let trimmed = line.trim_start();

        if is_fence(trimmed) {
            flush_para(&mut para, &mut blocks);
            flush_quote(&mut quote, &mut blocks);
            code = Some(Vec::new());
            idx += 1;
        } else if trimmed.is_empty() {
            flush_para(&mut para, &mut blocks);
            flush_quote(&mut quote, &mut blocks);
            idx += 1;
        } else if is_rule(trimmed) {
            flush_para(&mut para, &mut blocks);
            flush_quote(&mut quote, &mut blocks);
            blocks.push(Block::Rule);
            idx += 1;
        } else if let Some((level, rest)) = heading(trimmed) {
            flush_para(&mut para, &mut blocks);
            flush_quote(&mut quote, &mut blocks);
            blocks.push(Block::Heading {
                level,
                spans: parse_inline(rest),
            });
            idx += 1;
        } else if let Some(rest) = trimmed.strip_prefix('>') {
            flush_para(&mut para, &mut blocks);
            quote.push(rest.trim_start().to_string());
            idx += 1;
        } else if is_table_row(trimmed)
            && lines
                .get(idx + 1)
                .is_some_and(|next| table_separator(next).is_some())
        {
            flush_para(&mut para, &mut blocks);
            flush_quote(&mut quote, &mut blocks);
            let alignments = table_separator(lines[idx + 1]).unwrap_or_default();
            let header = table_cells(trimmed)
                .iter()
                .map(|cell| parse_inline(cell))
                .collect();
            idx += 2;

            let mut rows = Vec::new();
            while idx < lines.len() {
                let row = lines[idx].trim();
                if row.is_empty() || !is_table_row(row) {
                    break;
                }
                rows.push(
                    table_cells(row)
                        .iter()
                        .map(|cell| parse_inline(cell))
                        .collect(),
                );
                idx += 1;
            }
            blocks.push(Block::Table {
                alignments,
                header,
                rows,
            });
        } else if let Some((depth, marker, rest)) = list_item(line) {
            flush_para(&mut para, &mut blocks);
            flush_quote(&mut quote, &mut blocks);
            blocks.push(Block::ListItem {
                depth,
                marker,
                spans: parse_inline(rest),
            });
            idx += 1;
        } else {
            flush_quote(&mut quote, &mut blocks);
            para.push(trimmed.to_string());
            idx += 1;
        }
    }

    if let Some(buf) = code {
        blocks.push(Block::Code(buf.join("\n")));
    }
    flush_para(&mut para, &mut blocks);
    flush_quote(&mut quote, &mut blocks);
    blocks
}

fn flush_para(para: &mut Vec<String>, blocks: &mut Vec<Block>) {
    if !para.is_empty() {
        blocks.push(Block::Paragraph(parse_inline(&para.join(" "))));
        para.clear();
    }
}

fn flush_quote(quote: &mut Vec<String>, blocks: &mut Vec<Block>) {
    if !quote.is_empty() {
        blocks.push(Block::Quote(parse_inline(&quote.join(" "))));
        quote.clear();
    }
}

fn is_fence(line: &str) -> bool {
    line.starts_with("```") || line.starts_with("~~~")
}

fn is_rule(line: &str) -> bool {
    let stripped: String = line.chars().filter(|c| !c.is_whitespace()).collect();
    stripped.len() >= 3
        && ['-', '*', '_']
            .iter()
            .any(|c| stripped.chars().all(|ch| ch == *c))
}

/// `## Title` → `(2, "Title")`.
fn heading(line: &str) -> Option<(u8, &str)> {
    let level = line.chars().take_while(|c| *c == '#').count();
    if level == 0 || level > 6 {
        return None;
    }
    let rest = &line[level..];
    if rest.is_empty() {
        return Some((level as u8, ""));
    }
    rest.strip_prefix(' ')
        .map(|r| (level as u8, r.trim_start()))
}

/// `  - item` → `(depth, "•", "item")`; `2. item` → `(depth, "2.", "item")`.
fn list_item(line: &str) -> Option<(usize, String, &str)> {
    let indent: usize = line
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .map(|c| if c == '\t' { 4 } else { 1 })
        .sum();
    let rest = line.trim_start();

    for bullet in ['-', '*', '+'] {
        if let Some(body) = rest.strip_prefix(bullet).and_then(|r| r.strip_prefix(' ')) {
            return Some((indent / 2, "•".to_string(), body.trim_start()));
        }
    }

    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() || digits.len() > 9 {
        return None;
    }
    let after = &rest[digits.len()..];
    for sep in ['.', ')'] {
        if let Some(body) = after.strip_prefix(sep).and_then(|r| r.strip_prefix(' ')) {
            return Some((indent / 2, format!("{}{}", digits, sep), body.trim_start()));
        }
    }
    None
}

/// A line that could plausibly be a table row — the real test is whether the
/// *next* line is a valid separator (see [`table_separator`]).
fn is_table_row(line: &str) -> bool {
    line.contains('|')
}

/// `| :--- | ---: | :---: |` → one [`Align`] per column; `None` if the line
/// isn't a valid separator row (each cell must be dashes, optionally with a
/// leading and/or trailing colon).
fn table_separator(line: &str) -> Option<Vec<Align>> {
    if !is_table_row(line) {
        return None;
    }
    let cells = table_cells(line);
    if cells.is_empty() {
        return None;
    }
    cells
        .iter()
        .map(|cell| {
            let left = cell.starts_with(':');
            let right = cell.ends_with(':');
            let dashes = cell.trim_matches(':');
            (!dashes.is_empty() && dashes.chars().all(|c| c == '-')).then_some(
                match (left, right) {
                    (true, true) => Align::Center,
                    (false, true) => Align::Right,
                    _ => Align::Left,
                },
            )
        })
        .collect()
}

/// Splits a table row into its cell texts: drops one leading/trailing `|`
/// (the conventional outer pipes), and keeps a `|` from splitting the row
/// when it's escaped (`\|`) or inside a `` `code span` ``.
fn table_cells(line: &str) -> Vec<String> {
    let trimmed = line.trim();
    let chars: Vec<char> = trimmed.chars().collect();
    let mut cells = Vec::new();
    let mut cell = String::new();
    let mut in_code = false;
    let mut i = 0;

    while i < chars.len() {
        match chars[i] {
            '\\' if i + 1 < chars.len() && chars[i + 1] == '|' => {
                cell.push('|');
                i += 2;
            }
            '`' => {
                in_code = !in_code;
                cell.push('`');
                i += 1;
            }
            '|' if !in_code => {
                cells.push(cell.trim().to_string());
                cell.clear();
                i += 1;
            }
            c => {
                cell.push(c);
                i += 1;
            }
        }
    }
    cells.push(cell.trim().to_string());

    if trimmed.starts_with('|') && cells.first().is_some_and(String::is_empty) {
        cells.remove(0);
    }
    if trimmed.ends_with('|') && cells.last().is_some_and(String::is_empty) {
        cells.pop();
    }
    cells
}

/// Splits a line into styled inline spans.
pub fn parse_inline(src: &str) -> Vec<Span> {
    let chars: Vec<char> = src.chars().collect();
    let mut spans: Vec<Span> = Vec::new();
    let mut buf = String::new();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];
        match c {
            '\\' if i + 1 < chars.len() => {
                buf.push(chars[i + 1]);
                i += 2;
            }
            '`' => match find(&chars, i + 1, "`") {
                Some(end) => {
                    push_plain(&mut spans, &mut buf);
                    spans.push(Span::new(collect(&chars, i + 1, end), Style::Code));
                    i = end + 1;
                }
                None => {
                    buf.push(c);
                    i += 1;
                }
            },
            '*' | '_' => {
                let run = chars[i..].iter().take_while(|ch| **ch == c).count().min(3);
                let start = i + run;
                match emphasis_end(&chars, i, c, run) {
                    Some(end) => {
                        push_plain(&mut spans, &mut buf);
                        let style = match run {
                            1 => Style::Italic,
                            2 => Style::Bold,
                            _ => Style::BoldItalic,
                        };
                        spans.push(Span::new(collect(&chars, start, end), style));
                        i = end + run;
                    }
                    None => {
                        buf.push(c);
                        i += 1;
                    }
                }
            }
            '[' | '!' => {
                let open = if c == '!' { i + 1 } else { i };
                let is_link = chars.get(open) == Some(&'[');
                match is_link.then(|| link(&chars, open)).flatten() {
                    Some((label, href, end)) => {
                        push_plain(&mut spans, &mut buf);
                        spans.push(Span::link(label, href));
                        i = end;
                    }
                    None => {
                        buf.push(c);
                        i += 1;
                    }
                }
            }
            _ => {
                buf.push(c);
                i += 1;
            }
        }
    }

    push_plain(&mut spans, &mut buf);
    spans
}

/// Where the emphasis opened at `i` closes, if it does.
///
/// Follows the two rules that matter in practice: the delimiters must hug the
/// emphasized text (so `2 * 3 * 4` stays arithmetic), and `_` only delimits at
/// a word boundary (so `snake_case_name` stays one word).
fn emphasis_end(chars: &[char], i: usize, marker: char, run: usize) -> Option<usize> {
    let start = i + run;
    if chars.get(start).map_or(true, |c| c.is_whitespace()) {
        return None;
    }
    if marker == '_' && i > 0 && chars[i - 1].is_alphanumeric() {
        return None;
    }

    let delim: String = std::iter::repeat(marker).take(run).collect();
    let mut from = start + 1;
    while let Some(end) = find(chars, from, &delim) {
        let closes = !chars[end - 1].is_whitespace()
            && (marker != '_' || chars.get(end + run).map_or(true, |c| !c.is_alphanumeric()));
        if closes {
            return Some(end);
        }
        from = end + 1;
    }
    None
}

/// Parses `[label](url)` starting at the `[`; returns the label, the url,
/// and the index just past the closing `)`.
fn link(chars: &[char], start: usize) -> Option<(String, String, usize)> {
    let close = find(chars, start + 1, "]")?;
    if chars.get(close + 1) != Some(&'(') {
        return None;
    }
    let end = find(chars, close + 2, ")")?;
    Some((
        collect(chars, start + 1, close),
        collect(chars, close + 2, end),
        end + 1,
    ))
}

/// Index of the next occurrence of `needle` at or after `from`.
fn find(chars: &[char], from: usize, needle: &str) -> Option<usize> {
    let needle: Vec<char> = needle.chars().collect();
    if from >= chars.len() || needle.is_empty() {
        return None;
    }
    (from..=chars.len().saturating_sub(needle.len()))
        .find(|i| chars[*i..*i + needle.len()] == needle[..])
}

fn collect(chars: &[char], from: usize, to: usize) -> String {
    chars[from..to].iter().collect()
}

fn push_plain(spans: &mut Vec<Span>, buf: &mut String) {
    if !buf.is_empty() {
        spans.push(Span::new(std::mem::take(buf), Style::Plain));
    }
}

// ─── Anchors ────────────────────────────────────────────────────────────────
//
// A TOC link like `[Bugs](#4-bugs-fonctionnels)` targets a heading by a slug
// GitHub derives from its text. We regenerate the same slugs from our parsed
// headings and, on click, scroll the preview's own `scrollable` to whichever
// one matches.

/// GitHub's heading-to-anchor algorithm: lowercase, drop anything but
/// letters/digits/spaces/hyphens, turn spaces into hyphens.
fn slugify(text: &str) -> String {
    let mut slug = String::new();
    for c in text.chars() {
        if c.is_alphanumeric() {
            slug.extend(c.to_lowercase());
        } else if c == ' ' || c == '-' {
            slug.push('-');
        }
    }
    slug
}

/// The plain text of a heading's spans, markers already stripped by the
/// inline parser — exactly what GitHub slugifies.
fn heading_text(spans: &[Span]) -> String {
    spans.iter().map(|s| s.text.as_str()).collect()
}

/// One slug per block — `Some` only for headings — with GitHub's collision
/// rule: a slug repeated later in the document gets `-1`, `-2`, ... appended.
fn assign_slugs(blocks: &[Block]) -> Vec<Option<String>> {
    let mut seen: HashMap<String, u32> = HashMap::new();
    blocks
        .iter()
        .map(|block| {
            let Block::Heading { spans, .. } = block else {
                return None;
            };
            let base = slugify(&heading_text(spans));
            let count = seen.entry(base.clone()).or_insert(0);
            let slug = if *count == 0 {
                base
            } else {
                format!("{base}-{count}")
            };
            *count += 1;
            Some(slug)
        })
        .collect()
}

/// Stable id of the preview's own scrollable, so a link click can scroll it.
pub fn scrollable_id() -> scrollable::Id {
    scrollable::Id::new("markdown-preview")
}

fn anchor_id(slug: &str) -> container::Id {
    container::Id::new(format!("md-anchor-{slug}"))
}

/// Walks the widget tree to find how far a heading's anchor sits below the
/// top of the preview's scrollable content — the `y` a `scrollable::scroll_to`
/// needs. Container/scrollable bounds from `Operation::container` /
/// `::scrollable` are given in unscrolled content space, so this is a plain
/// subtraction; no need to know the current scroll position at all.
struct FindHeadingOffset {
    scrollable_id: WidgetId,
    target_id: WidgetId,
    scrollable_top: Option<f32>,
    offset: Option<f32>,
}

impl Operation<Option<f32>> for FindHeadingOffset {
    fn container(
        &mut self,
        id: Option<&WidgetId>,
        bounds: Rectangle,
        operate_on_children: &mut dyn FnMut(&mut dyn Operation<Option<f32>>),
    ) {
        if self.offset.is_some() {
            return;
        }
        if id == Some(&self.target_id) {
            if let Some(top) = self.scrollable_top {
                self.offset = Some(bounds.y - top);
            }
            return;
        }
        operate_on_children(self);
    }

    fn scrollable(
        &mut self,
        _state: &mut dyn operation::Scrollable,
        id: Option<&WidgetId>,
        bounds: Rectangle,
        _translation: Vector,
    ) {
        if id == Some(&self.scrollable_id) {
            self.scrollable_top = Some(bounds.y);
        }
    }

    fn finish(&self) -> operation::Outcome<Option<f32>> {
        operation::Outcome::Some(self.offset)
    }
}

/// A `Command` resolving to the target heading's offset for `href`
/// (`#some-slug`), or `None` if it names no heading in the current document.
pub fn find_offset_command(href: &str) -> Command<Option<f32>> {
    let slug = href.trim_start_matches('#').to_lowercase();
    Command::widget(FindHeadingOffset {
        scrollable_id: scrollable_id().into(),
        target_id: anchor_id(&slug).into(),
        scrollable_top: None,
        offset: None,
    })
}

// ─── Rendering ──────────────────────────────────────────────────────────────

const BODY_SIZE: f32 = 15.0;

fn heading_size(level: u8) -> f32 {
    match level {
        1 => 27.0,
        2 => 23.0,
        3 => 20.0,
        4 => 17.0,
        5 => 15.0,
        _ => 14.0,
    }
}

fn bold_font() -> Font {
    Font {
        weight: iced::font::Weight::Bold,
        ..Font::DEFAULT
    }
}

fn italic_font() -> Font {
    Font {
        style: iced::font::Style::Italic,
        ..Font::DEFAULT
    }
}

fn bold_italic_font() -> Font {
    Font {
        weight: iced::font::Weight::Bold,
        style: iced::font::Style::Italic,
        ..Font::DEFAULT
    }
}

/// The rendered document, scrollable.
pub fn view(blocks: &[Block], dark: bool) -> Element<'static, Message> {
    let slugs = assign_slugs(blocks);
    let body: Vec<Element<'static, Message>> = blocks
        .iter()
        .zip(slugs.iter())
        .map(|(b, slug)| block_view(b, dark, slug.as_deref()))
        .collect();

    scrollable(
        container(column(body).spacing(12))
            .width(Length::Fill)
            .padding([16, 24]),
    )
    .id(scrollable_id())
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn block_view(block: &Block, dark: bool, slug: Option<&str>) -> Element<'static, Message> {
    let p = theme::palette(dark);
    match block {
        Block::Heading { level, spans } => {
            let heading = spans_view(spans, heading_size(*level), p.text, dark, bold_font());
            match slug {
                Some(slug) => container(heading).id(anchor_id(slug)).into(),
                None => heading,
            }
        }
        Block::Paragraph(spans) => spans_view(spans, BODY_SIZE, p.text, dark, Font::DEFAULT),
        Block::Quote(spans) => {
            container(spans_view(spans, BODY_SIZE, p.muted, dark, italic_font()))
                .width(Length::Fill)
                .padding([8, 14])
                .style(theme::quote_block(dark))
                .into()
        }
        Block::ListItem {
            depth,
            marker,
            spans,
        } => row![
            Space::with_width(Length::Fixed(8.0 + *depth as f32 * 18.0)),
            text(marker.clone())
                .size(BODY_SIZE)
                .style(theme::muted_text(dark)),
            Space::with_width(Length::Fixed(8.0)),
            spans_view(spans, BODY_SIZE, p.text, dark, Font::DEFAULT),
        ]
        .into(),
        Block::Code(code) => container(
            text(code.clone())
                .size(13.0)
                .font(Font::MONOSPACE)
                .style(p.text)
                .shaping(iced::widget::text::Shaping::Advanced),
        )
        .width(Length::Fill)
        .padding([10, 14])
        .style(theme::code_block(dark))
        .into(),
        Block::Rule => container(Space::with_height(1))
            .width(Length::Fill)
            .style(theme::gutter(dark))
            .into(),
        Block::Table {
            alignments,
            header,
            rows,
        } => table_view(alignments, header, rows, dark),
    }
}

/// A GFM table. Cells size to their own content (not stretched to fill the
/// pane, matching how these render elsewhere), so a wide table scrolls
/// horizontally rather than forcing every column down to an unreadable
/// width.
fn table_view(
    alignments: &[Align],
    header: &[Vec<Span>],
    rows: &[Vec<Vec<Span>>],
    dark: bool,
) -> Element<'static, Message> {
    let p = theme::palette(dark);
    let columns = header
        .len()
        .max(rows.iter().map(Vec::len).max().unwrap_or(0));

    let align_of = |col: usize| alignments.get(col).copied().unwrap_or(Align::Left).into();

    let cell = |spans: &[Span], col: usize, is_header: bool| -> Element<'static, Message> {
        let font = if is_header {
            bold_font()
        } else {
            Font::DEFAULT
        };
        container(spans_view(spans, BODY_SIZE, p.text, dark, font))
            .width(Length::Fill)
            .align_x(align_of(col))
            .padding([6, 10])
            .style(theme::table_cell(dark, is_header))
            .into()
    };

    let mut grid = Grid::new().column_width(Length::Shrink);

    let mut header_row = GridRow::new();
    for col in 0..columns {
        let spans = header.get(col).map(Vec::as_slice).unwrap_or(&[]);
        header_row = header_row.push(cell(spans, col, true));
    }
    grid = grid.push(header_row);

    for row in rows {
        let mut grid_row = GridRow::new();
        for col in 0..columns {
            let spans = row.get(col).map(Vec::as_slice).unwrap_or(&[]);
            grid_row = grid_row.push(cell(spans, col, false));
        }
        grid = grid.push(grid_row);
    }

    scrollable(container(grid).style(theme::table_border(dark)))
        .direction(scrollable::Direction::Horizontal(
            scrollable::Properties::default(),
        ))
        .width(Length::Fill)
        .into()
}

/// Lays out spans as a wrapping run of words. iced 0.12 has no rich text
/// widget, so each word is its own text widget; fragments that are not
/// separated by whitespace in the source (`**bold**,`) are grouped into one
/// unspaced row so the punctuation stays glued to the word.
fn spans_view(
    spans: &[Span],
    size: f32,
    color: Color,
    dark: bool,
    base_font: Font,
) -> Element<'static, Message> {
    let accent = theme::accent_color();
    let muted = theme::muted_text(dark);

    let mut clusters: Vec<Element<'static, Message>> = Vec::new();
    let mut cluster: Vec<Element<'static, Message>> = Vec::new();

    for span in spans {
        if span.text.starts_with(char::is_whitespace) && !cluster.is_empty() {
            clusters.push(row(std::mem::take(&mut cluster)).into());
        }
        for (i, word) in span.text.split_whitespace().enumerate() {
            if i > 0 && !cluster.is_empty() {
                clusters.push(row(std::mem::take(&mut cluster)).into());
            }
            // Advanced shaping is what makes cosmic-text fall back to another
            // installed font for a glyph missing from `base_font` — emoji chief
            // among them, which live in a dedicated color-emoji font.
            let widget = text(word.to_string())
                .size(size)
                .shaping(iced::widget::text::Shaping::Advanced);
            let widget = match span.style {
                Style::Plain => widget.font(base_font).style(color),
                Style::Bold => widget.font(bold_font()).style(color),
                Style::Italic => widget.font(italic_font()).style(color),
                Style::BoldItalic => widget.font(bold_italic_font()).style(color),
                Style::Code => widget.font(Font::MONOSPACE).size(size - 1.0).style(muted),
                Style::Link => widget.font(base_font).style(accent),
            };
            // Only in-document anchors (`#slug`) are wired up to jump
            // anywhere; an external link stays styled but inert.
            let element: Element<'static, Message> = match &span.href {
                Some(href) if href.starts_with('#') => mouse_area(widget)
                    .interaction(mouse::Interaction::Pointer)
                    .on_press(Message::MarkdownLinkClicked(href.clone()))
                    .into(),
                _ => widget.into(),
            };
            cluster.push(element);
        }
        if span.text.ends_with(char::is_whitespace) && !cluster.is_empty() {
            clusters.push(row(std::mem::take(&mut cluster)).into());
        }
    }
    if !cluster.is_empty() {
        clusters.push(row(cluster).into());
    }

    iced_aw::Wrap::with_elements(clusters)
        .spacing(size * 0.28)
        .line_spacing(5.0)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(text: &str) -> Vec<Span> {
        vec![Span::new(text, Style::Plain)]
    }

    #[test]
    fn parses_headings() {
        assert_eq!(
            parse("# Title"),
            vec![Block::Heading {
                level: 1,
                spans: plain("Title")
            }]
        );
        assert_eq!(
            parse("### Deep"),
            vec![Block::Heading {
                level: 3,
                spans: plain("Deep")
            }]
        );
    }

    #[test]
    fn hash_without_space_is_not_a_heading() {
        assert_eq!(parse("#hashtag"), vec![Block::Paragraph(plain("#hashtag"))]);
    }

    #[test]
    fn joins_paragraph_lines_and_splits_on_blank() {
        assert_eq!(
            parse("one\ntwo\n\nthree"),
            vec![
                Block::Paragraph(plain("one two")),
                Block::Paragraph(plain("three")),
            ]
        );
    }

    #[test]
    fn parses_fenced_code_verbatim() {
        let blocks = parse("```rust\nlet x = *y;\n```");
        assert_eq!(blocks, vec![Block::Code("let x = *y;".to_string())]);
    }

    #[test]
    fn unclosed_fence_still_yields_a_code_block() {
        assert_eq!(parse("```\nabc"), vec![Block::Code("abc".to_string())]);
    }

    #[test]
    fn parses_lists() {
        assert_eq!(
            parse("- a\n  - b\n3. c"),
            vec![
                Block::ListItem {
                    depth: 0,
                    marker: "•".to_string(),
                    spans: plain("a")
                },
                Block::ListItem {
                    depth: 1,
                    marker: "•".to_string(),
                    spans: plain("b")
                },
                Block::ListItem {
                    depth: 0,
                    marker: "3.".to_string(),
                    spans: plain("c")
                },
            ]
        );
    }

    #[test]
    fn parses_quotes_and_rules() {
        assert_eq!(
            parse("> quoted\n> more\n\n---"),
            vec![Block::Quote(plain("quoted more")), Block::Rule]
        );
    }

    #[test]
    fn dashes_inside_a_list_are_not_a_rule() {
        assert!(matches!(parse("- a")[0], Block::ListItem { .. }));
    }

    #[test]
    fn parses_inline_emphasis() {
        assert_eq!(
            parse_inline("a **b** c *d* `e` ***f***"),
            vec![
                Span::new("a ", Style::Plain),
                Span::new("b", Style::Bold),
                Span::new(" c ", Style::Plain),
                Span::new("d", Style::Italic),
                Span::new(" ", Style::Plain),
                Span::new("e", Style::Code),
                Span::new(" ", Style::Plain),
                Span::new("f", Style::BoldItalic),
            ]
        );
    }

    #[test]
    fn unmatched_markers_stay_literal() {
        assert_eq!(parse_inline("2 * 3 * 4"), plain("2 * 3 * 4"));
        assert_eq!(parse_inline("a_b"), plain("a_b"));
        assert_eq!(parse_inline("*dangling"), plain("*dangling"));
    }

    #[test]
    fn underscores_inside_a_word_are_literal() {
        assert_eq!(parse_inline("snake_case_name"), plain("snake_case_name"));
        assert_eq!(
            parse_inline("an _emphasized_ word"),
            vec![
                Span::new("an ", Style::Plain),
                Span::new("emphasized", Style::Italic),
                Span::new(" word", Style::Plain),
            ]
        );
    }

    #[test]
    fn parses_links_and_images() {
        assert_eq!(
            parse_inline("see [docs](http://x) and ![pic](y.png)"),
            vec![
                Span::new("see ", Style::Plain),
                Span::link("docs", "http://x"),
                Span::new(" and ", Style::Plain),
                Span::link("pic", "y.png"),
            ]
        );
    }

    #[test]
    fn bracket_without_url_stays_literal() {
        assert_eq!(parse_inline("[todo]"), plain("[todo]"));
    }

    #[test]
    fn backslash_escapes_markers() {
        assert_eq!(parse_inline(r"\*not bold\*"), plain("*not bold*"));
    }

    #[test]
    fn empty_document_has_no_blocks() {
        assert!(parse("").is_empty());
        assert!(parse("\n\n").is_empty());
    }

    #[test]
    fn parses_a_table() {
        let blocks = parse("| A | B |\n| --- | --- |\n| 1 | 2 |\n| 3 | 4 |");
        assert_eq!(
            blocks,
            vec![Block::Table {
                alignments: vec![Align::Left, Align::Left],
                header: vec![plain("A"), plain("B")],
                rows: vec![vec![plain("1"), plain("2")], vec![plain("3"), plain("4")],],
            }]
        );
    }

    #[test]
    fn table_survives_without_outer_pipes() {
        let blocks = parse("A | B\n--- | ---\n1 | 2");
        assert_eq!(
            blocks,
            vec![Block::Table {
                alignments: vec![Align::Left, Align::Left],
                header: vec![plain("A"), plain("B")],
                rows: vec![vec![plain("1"), plain("2")]],
            }]
        );
    }

    #[test]
    fn table_reads_column_alignment() {
        let blocks = parse("| L | C | R |\n| :--- | :---: | ---: |\n| a | b | c |");
        let Block::Table { alignments, .. } = &blocks[0] else {
            panic!("expected a table");
        };
        assert_eq!(alignments, &[Align::Left, Align::Center, Align::Right]);
    }

    #[test]
    fn table_stops_at_a_blank_line() {
        let blocks = parse("| A |\n| --- |\n| 1 |\n\nafter");
        assert_eq!(blocks.len(), 2);
        assert!(matches!(blocks[0], Block::Table { .. }));
        assert_eq!(blocks[1], Block::Paragraph(plain("after")));
    }

    #[test]
    fn a_lone_pipe_row_without_a_separator_stays_a_paragraph() {
        let blocks = parse("A | B\nnot a separator");
        assert_eq!(
            blocks,
            vec![Block::Paragraph(parse_inline("A | B not a separator"))]
        );
    }

    #[test]
    fn table_cells_split_on_unescaped_pipes_only() {
        assert_eq!(
            table_cells(r"| `a|b` | c\|d |"),
            vec!["`a|b`".to_string(), r"c|d".to_string()]
        );
    }

    #[test]
    fn ragged_rows_are_kept_as_parsed_not_padded() {
        // table_view pads/truncates these to the header's column count at
        // render time; the parser just records what's actually there.
        let blocks = parse("| A | B |\n| --- | --- |\n| 1 |\n| 2 | 3 | 4 |");
        let Block::Table { rows, .. } = &blocks[0] else {
            panic!("expected a table");
        };
        assert_eq!(rows[0], vec![plain("1")]);
        assert_eq!(rows[1], vec![plain("2"), plain("3"), plain("4")]);
    }

    #[test]
    fn slugify_matches_github() {
        assert_eq!(slugify("Bugs fonctionnels"), "bugs-fonctionnels");
        assert_eq!(
            slugify("1. Inventaire et santé mesurée"),
            "1-inventaire-et-santé-mesurée"
        );
        assert_eq!(slugify("Qualité, tests, CI"), "qualité-tests-ci");
    }

    #[test]
    fn assign_slugs_only_tags_headings() {
        let blocks = parse("# Title\n\ntext\n\n## Title");
        let slugs = assign_slugs(&blocks);
        assert_eq!(
            slugs,
            vec![
                Some("title".to_string()),
                None,
                // GitHub's collision rule: repeated slugs get -1, -2, ...
                Some("title-1".to_string()),
            ]
        );
    }
}
