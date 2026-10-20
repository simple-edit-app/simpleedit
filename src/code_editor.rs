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
use iced::advanced::graphics::text::cosmic_text::Cursor as Cursor_;
use iced::advanced::layout::{self, Layout};
use iced::advanced::mouse;
use iced::advanced::renderer::{self, Renderer as _};
use iced::advanced::text::editor::{Cursor, Editor as _, Position, Selection};
use iced::advanced::text::highlighter::{self, Highlighter};
use iced::advanced::text::{self, LineHeight, Renderer as TextRenderer};
use iced::advanced::widget::{self, tree};
use iced::advanced::Shell;
use iced::alignment;
use iced::keyboard::{self, key};
use iced::{
    Background, Border, Color, Element, Event, Font, Length, Padding, Pixels, Point, Rectangle,
    Size, Vector,
};

use crate::folding::{self, Folds, Kind, Regions, Scope};

use std::cell::RefCell;
use std::ops::DerefMut;
use std::ops::Range;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

pub use iced::widget::text_editor::{Action, Edit, Motion};

/// How the editor looks: the surface behind the text, the caret and the
/// selection.
#[derive(Debug, Clone, Copy)]
pub struct Style {
    pub background: Background,
    pub border: Border,
    /// The caret.
    pub value: Color,
    pub selection: Color,
}

type Editor = <iced::Renderer as TextRenderer>::Editor;

/// The line-number column of a [`CodeEditor`].
#[derive(Debug, Clone, Copy)]
pub struct Gutter {
    /// Color of the numbers.
    pub text: Color,
    /// Color of the current line's number.
    pub current: Color,
}

/// Wash over the cursor's line, with an accent bar on its left edge.
#[derive(Debug, Clone, Copy)]
pub struct LineHighlight {
    pub background: Color,
    pub bar: Color,
}

/// What to paint for an active search: a background behind every match, and
/// a stronger one behind the current match.
#[derive(Debug, Clone)]
pub struct SearchPaint {
    pub query: String,
    pub case_sensitive: bool,
    pub use_regex: bool,
    /// The query compiled once (regex mode), not on every frame.
    pub regex: Option<Arc<regex::Regex>>,
    /// (line, byte range within the line) of the current match.
    pub current: Option<(usize, Range<usize>)>,
    pub color: Color,
    pub current_color: Color,
    pub current_border: Color,
}

/// Colors of the minimap: a miniature of the whole file along the right edge.
#[derive(Debug, Clone, Copy)]
pub struct Minimap {
    /// The miniature lines.
    pub bar: Color,
    /// Wash over the part of the file on screen.
    pub viewport: Color,
    /// Outline of that part.
    pub viewport_border: Color,
    /// Marker for search matches.
    pub marker: Color,
}

/// Bookmarked lines: a flag in the gutter and a tick in the minimap.
#[derive(Debug, Clone)]
pub struct BookmarkPaint {
    /// 0-based, sorted.
    pub lines: Vec<usize>,
    pub color: Color,
}

/// Folding: a chevron in the gutter at the head of each foldable block, and a
/// marker after a folded one.
#[derive(Debug, Clone)]
pub struct FoldPaint {
    /// What the language folds on.
    pub kind: Kind,
    /// Chevron of a block that is open.
    pub chevron: Color,
    /// Chevron and marker of a folded block.
    pub folded: Color,
    /// Behind the marker.
    pub marker_background: Color,
    /// What the pointer on a chevron explains, one line each.
    pub tooltip: Vec<String>,
    pub tooltip_text: Color,
    pub tooltip_background: Color,
    pub tooltip_border: Color,
}

/// What the app can ask of the folds of a [`Content`].
#[derive(Debug, Clone, Copy)]
pub enum FoldCommand {
    /// Fold the block around the cursor, or unfold it.
    AtCursor,
    /// Fold every block (`true`) or unfold them all.
    All(bool),
    /// Fold or unfold the block headed by a line, with its siblings or
    /// everything inside it.
    Line(usize, Scope),
}

/// Width of the minimap.
pub const MINIMAP_WIDTH: f32 = 84.0;
/// Space above and below the miniature.
const MINIMAP_INSET: f32 = 8.0;
/// Width of a miniature character.
const MINIMAP_CHAR: f32 = 0.8;
/// Beyond this many lines the search markers aren't computed (they would
/// cost a scan of the whole file on every redraw).
const MINIMAP_MAX_MARKED_LINES: usize = 20_000;

/// Height of one line of the miniature: 2px, less when the file is too
/// tall to fit.
fn minimap_scale(lines: usize, height: f32) -> f32 {
    let room = (height - 2.0 * MINIMAP_INSET).max(1.0);
    (room / lines.max(1) as f32).min(2.0)
}

/// The line under `y` (relative to the minimap's top).
fn minimap_line_at(y: f32, scale: f32, lines: usize) -> usize {
    (((y - MINIMAP_INSET) / scale).max(0.0) as usize).min(lines.saturating_sub(1))
}

/// Where a line's miniature starts and how long it is, in characters:
/// indentation counts for its width, trailing space doesn't. `None` for a
/// blank line.
fn minimap_span(line: &str) -> Option<(usize, usize)> {
    let mut start = 0;
    for c in line.chars() {
        match c {
            ' ' => start += 1,
            '\t' => start += 4,
            _ => break,
        }
    }
    let body = line.trim();
    (!body.is_empty()).then(|| (start, body.chars().count()))
}

/// Space between the gutter and the text.
const GUTTER_GAP: f32 = 6.0;
/// Width of the strip, at the right edge of the gutter, holding the chevrons.
const CHEVRON_WIDTH: f32 = 22.0;

/// Width of the gutter for a buffer of `lines` lines at text size `size`.
fn gutter_width(size: f32, lines: usize) -> f32 {
    let digits = lines.max(1).to_string().len().max(2);
    (digits as f32 * size * 0.6 + 34.0).max(54.0)
}

/// Where the text starts, from the widget's top-left corner: past the
/// padding and the gutter.
pub fn text_origin(size: f32, lines: usize, gutter: bool, padding: Padding) -> Vector {
    let gutter = if gutter {
        gutter_width(size, lines) + GUTTER_GAP
    } else {
        0.0
    };
    Vector::new(padding.left + gutter, padding.top)
}

/// The minimap's strip along the right edge of the widget.
fn minimap_rect(bounds: Rectangle) -> Rectangle {
    Rectangle {
        x: bounds.x + bounds.width - MINIMAP_WIDTH,
        y: bounds.y,
        width: MINIMAP_WIDTH,
        height: bounds.height,
    }
}

/// Byte ranges of the matches of `paint` in one line of text.
fn line_matches(
    line: &str,
    paint: &SearchPaint,
    regex: Option<&regex::Regex>,
) -> Vec<Range<usize>> {
    if paint.query.is_empty() {
        return Vec::new();
    }
    if paint.use_regex {
        regex
            .map(|re| {
                re.find_iter(line)
                    .filter(|m| !m.is_empty())
                    .map(|m| m.range())
                    .collect()
            })
            .unwrap_or_default()
    } else if paint.case_sensitive {
        line.match_indices(paint.query.as_str())
            .map(|(i, m)| i..i + m.len())
            .collect()
    } else if let Some(re) = regex {
        re.find_iter(line).map(|m| m.range()).collect()
    } else {
        crate::search::find_literal_ci(line, &paint.query)
    }
}

/// A multi-line text input.
#[allow(missing_debug_implementations)]
pub struct CodeEditor<'a, Highlighter, Message, Theme = iced::Theme>
where
    Highlighter: text::Highlighter,
{
    content: &'a Content,
    font: Option<Font>,
    text_size: Option<Pixels>,
    line_height: LineHeight,
    width: Length,
    height: Length,
    padding: Padding,
    gutter: Option<Gutter>,
    line_highlight: Option<LineHighlight>,
    search: Option<SearchPaint>,
    /// Column and color of a vertical page guide.
    guide: Option<(usize, Color)>,
    minimap: Option<Minimap>,
    bookmarks: Option<BookmarkPaint>,
    folding: Option<FoldPaint>,
    on_fold: Option<Box<dyn Fn(usize, Scope) -> Message + 'a>>,
    style: Style,
    on_edit: Option<Box<dyn Fn(Action) -> Message + 'a>>,
    highlighter_settings: Highlighter::Settings,
    highlighter_format: fn(&Highlighter::Highlight, &Theme) -> highlighter::Format<Font>,
}

impl<'a, Message, Theme> CodeEditor<'a, highlighter::PlainText, Message, Theme> {
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
            line_highlight: None,
            search: None,
            guide: None,
            minimap: None,
            bookmarks: None,
            folding: None,
            on_fold: None,
            style: Style {
                background: Background::Color(Color::TRANSPARENT),
                border: Border::default(),
                value: Color::BLACK,
                selection: Color::BLACK,
            },
            on_edit: None,
            highlighter_settings: (),
            highlighter_format: |_highlight, _theme| highlighter::Format::default(),
        }
    }
}

impl<'a, Highlighter, Message, Theme> CodeEditor<'a, Highlighter, Message, Theme>
where
    Highlighter: text::Highlighter,
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

    pub fn font(mut self, font: Font) -> Self {
        self.font = Some(font);
        self
    }

    pub fn line_height(mut self, line_height: LineHeight) -> Self {
        self.line_height = line_height;
        self
    }

    /// Washes the cursor's line.
    pub fn line_highlight(mut self, highlight: LineHighlight) -> Self {
        self.line_highlight = Some(highlight);
        self
    }

    /// Paints search matches behind the text.
    pub fn search(mut self, paint: SearchPaint) -> Self {
        self.search = Some(paint);
        self
    }

    /// Draws a vertical guide after `column` characters (monospace fonts).
    pub fn page_guide(mut self, column: usize, color: Color) -> Self {
        self.guide = Some((column, color));
        self
    }

    /// Shows a miniature of the whole file on the right; dragging in it
    /// scrolls the text.
    pub fn minimap(mut self, minimap: Minimap) -> Self {
        self.minimap = Some(minimap);
        self
    }

    /// Marks lines (see [`BookmarkPaint`]).
    pub fn bookmarks(mut self, bookmarks: BookmarkPaint) -> Self {
        self.bookmarks = Some(bookmarks);
        self
    }

    /// Lets blocks fold (see [`FoldPaint`]); `on_fold` hears about a click on
    /// a chevron, with how far it reaches.
    pub fn folding(
        mut self,
        paint: FoldPaint,
        on_fold: impl Fn(usize, Scope) -> Message + 'a,
    ) -> Self {
        self.folding = Some(paint);
        self.on_fold = Some(Box::new(on_fold));
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
            line_highlight: self.line_highlight,
            search: self.search,
            guide: self.guide,
            minimap: self.minimap,
            bookmarks: self.bookmarks,
            folding: self.folding,
            on_fold: self.on_fold,
            style: self.style,
            on_edit: self.on_edit,
            highlighter_settings: settings,
            highlighter_format: to_format,
        }
    }

    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// How many rows to scroll so that the line at `y` (from the minimap's
    /// top) ends up in the middle of the screen.
    fn minimap_scroll(&self, y: f32, height: f32) -> Option<i32> {
        let internal = self.content.0.borrow();
        let buffer = internal.editor.buffer();
        let lines = buffer.lines.len();
        let (first, last) =
            buffer
                .layout_runs()
                .fold(None, |range: Option<(usize, usize)>, run| {
                    Some(match range {
                        Some((a, b)) => (a.min(run.line_i), b.max(run.line_i)),
                        None => (run.line_i, run.line_i),
                    })
                })?;
        let wanted = minimap_line_at(y, minimap_scale(lines, height), lines);
        let target = wanted.saturating_sub((last - first).div_ceil(2));
        // Rows, not lines: a wrapped line takes several (1 if not laid out yet).
        let rows = |from: usize, to: usize| -> i32 {
            buffer.lines[from..to]
                .iter()
                .map(|l| l.layout_opt().as_ref().map_or(1, |v| v.len().max(1)) as i32)
                .sum()
        };
        let delta = if target >= first {
            rows(first, target)
        } else {
            -rows(target, first)
        };
        (delta != 0).then_some(delta)
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_minimap(
        &self,
        renderer: &mut iced::Renderer,
        editor: &Editor,
        version: (u64, u64),
        cache: &RefCell<MinimapCache>,
        rect: Rectangle,
        minimap: Minimap,
        regex: Option<&regex::Regex>,
    ) {
        let buffer = editor.buffer();
        let lines = buffer.lines.len();
        let mut cache = cache.borrow_mut();
        if cache.key != Some(version) {
            cache.spans = buffer
                .lines
                .iter()
                .map(|l| minimap_span(l.text()))
                .collect();
            cache.key = Some(version);
        }
        let scale = minimap_scale(lines, rect.height);
        let top = rect.y + MINIMAP_INSET;
        let quad = |renderer: &mut iced::Renderer, x, y, width, height, color| {
            let bounds = Rectangle {
                x,
                y,
                width,
                height,
            };
            if let Some(bounds) = rect.intersection(&bounds) {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds,
                        ..renderer::Quad::default()
                    },
                    color,
                );
            }
        };

        // The part of the file on screen.
        let range = buffer
            .layout_runs()
            .fold(None, |range: Option<(usize, usize)>, run| {
                Some(match range {
                    Some((a, b)) => (a.min(run.line_i), b.max(run.line_i)),
                    None => (run.line_i, run.line_i),
                })
            });
        if let Some((first, last)) = range {
            let band = Rectangle {
                x: rect.x + 0.5,
                y: top + first as f32 * scale,
                width: rect.width - 1.0,
                height: ((last - first + 1) as f32 * scale).max(6.0),
            };
            if let Some(bounds) = rect.intersection(&band) {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds,
                        border: iced::Border {
                            color: minimap.viewport_border,
                            width: 1.0,
                            radius: 3.0.into(),
                        },
                        ..renderer::Quad::default()
                    },
                    minimap.viewport,
                );
            }
        }

        // The lines: one bar each, or one per pixel row when they don't fit
        // (the longest of the lines that share it).
        let group = (1.0 / scale).ceil().max(1.0) as usize;
        let step = group as f32 * scale;
        let max_width = rect.width - 12.0;
        for (row, first) in (0..lines).step_by(group).enumerate() {
            let span = cache.spans[first..(first + group).min(lines)]
                .iter()
                .flatten()
                .copied()
                .fold(None, |acc: Option<(usize, usize)>, (start, len)| {
                    Some(match acc {
                        Some((s, end)) => (s.min(start), end.max(start + len)),
                        None => (start, start + len),
                    })
                });
            let Some((start, end)) = span else { continue };
            let x = start as f32 * MINIMAP_CHAR;
            if x >= max_width {
                continue;
            }
            quad(
                renderer,
                rect.x + 6.0 + x,
                top + row as f32 * step,
                ((end - start) as f32 * MINIMAP_CHAR).clamp(1.5, max_width - x),
                (step * 0.7).max(1.0),
                minimap.bar,
            );
        }

        // Bookmarks: a line across the minimap, and a tick on the left edge.
        if let Some(marks) = &self.bookmarks {
            for &line in marks.lines.iter().filter(|l| **l < lines) {
                let y = top + line as f32 * scale;
                quad(
                    renderer,
                    rect.x,
                    y + (scale.max(3.0) - 2.0) / 2.0,
                    rect.width,
                    2.0,
                    Color {
                        a: marks.color.a * 0.8,
                        ..marks.color
                    },
                );
                quad(
                    renderer,
                    rect.x,
                    top + line as f32 * scale,
                    5.0,
                    scale.max(3.0),
                    marks.color,
                );
            }
        }

        // Search matches: a tick on the right edge.
        if let Some(paint) = self
            .search
            .as_ref()
            .filter(|_| lines <= MINIMAP_MAX_MARKED_LINES)
        {
            let search_key = (
                version.0,
                version.1,
                paint.query.clone(),
                paint.case_sensitive,
                paint.use_regex,
            );
            if cache.search_key.as_ref() != Some(&search_key) {
                cache.marked = buffer
                    .lines
                    .iter()
                    .enumerate()
                    .filter(|(_, line)| !line_matches(line.text(), paint, regex).is_empty())
                    .map(|(i, _)| i)
                    .collect();
                cache.search_key = Some(search_key);
            }
            for &i in &cache.marked {
                quad(
                    renderer,
                    rect.x + rect.width - 4.0,
                    top + i as f32 * scale,
                    4.0,
                    scale.max(2.0),
                    minimap.marker,
                );
            }
        }
    }

    fn size_px(&self, renderer: &iced::Renderer) -> f32 {
        self.text_size.unwrap_or_else(|| renderer.default_size()).0
    }

    /// The foldable line whose chevron is under `at` (relative to the
    /// widget).
    fn chevron_line_at(
        &self,
        at: Point,
        padding: Padding,
        renderer: &iced::Renderer,
    ) -> Option<usize> {
        self.folding.as_ref()?;
        let right = padding.left - GUTTER_GAP;
        if at.x < right - CHEVRON_WIDTH || at.x > right {
            return None;
        }
        let internal = self.content.0.borrow();
        let size = self.size_px(renderer);
        let row = self.line_height.to_absolute(Pixels(size)).0;
        let mut previous = None;
        for run in internal.editor.buffer().layout_runs() {
            let first = previous != Some(run.line_i);
            previous = Some(run.line_i);
            let top = padding.top + run.line_top;
            if first
                && run.line_height > 0.0
                && at.y >= top
                && at.y < top + row
                && internal.regions.at(run.line_i).is_some()
            {
                return Some(run.line_i);
            }
        }
        None
    }

    /// The padding around the text proper: the user's padding plus the gutter.
    fn text_padding(&self, renderer: &iced::Renderer, lines: usize) -> Padding {
        let origin = text_origin(
            self.size_px(renderer),
            lines,
            self.gutter.is_some(),
            self.padding,
        );
        Padding {
            left: origin.x,
            right: self.padding.right
                + if self.minimap.is_some() {
                    MINIMAP_WIDTH
                } else {
                    0.0
                },
            ..self.padding
        }
    }
}

/// The content of a [`CodeEditor`].
pub struct Content(RefCell<Internal>);

/// Identifies a freshly created [`Content`], so the widget can tell that the
/// text it shows was swapped for another one.
static NEXT_CONTENT_ID: AtomicU64 = AtomicU64::new(1);

struct Internal {
    id: u64,
    /// Topmost line edited since the widget last told its highlighter.
    changed_from: Option<usize>,
    editor: Editor,
    is_dirty: bool,
    /// Bumped by every edit, to know when cached per-line data is stale.
    revision: u64,
    wrapping: text::Wrapping,
    folds: Folds,
    regions: Arc<Regions>,
    /// (revision, kind) the regions were found for.
    regions_key: Option<(u64, Kind)>,
    /// The cursor must not stay in folded lines: set by an edit or a jump,
    /// acted on once the regions are up to date.
    reveal_cursor: bool,
}

impl Internal {
    /// Brings the blocks up to date with the text, drops the folds that lost
    /// their block, and tells the editor which lines are folded away.
    fn refresh_folds(&mut self, kind: Kind) {
        if self.regions_key != Some((self.revision, kind)) {
            let count = self.editor.line_count();
            self.regions = Arc::new(if count > folding::MAX_LINES {
                Regions::default()
            } else {
                let editor = &self.editor;
                Regions::find(
                    (0..count)
                        .filter_map(|i| editor.line(i))
                        .map(|line| line.text),
                    kind,
                )
            });
            self.regions_key = Some((self.revision, kind));
            self.folds.prune(&self.regions);
        }
        if std::mem::take(&mut self.reveal_cursor) {
            let line = self.editor.cursor().position.line;
            let _ = self.folds.reveal(&self.regions, line);
        }
        self.editor
            .set_hidden_lines(self.folds.hidden(&self.regions));
    }

    /// If the cursor is in folded lines, moves it out: below them when it
    /// was moving down from `from`, else to the header.
    fn step_out_of_folds(&mut self, from: usize) {
        if self.folds.is_empty() {
            return;
        }
        let cursor = self.editor.cursor();
        let line = cursor.position.line;
        let hidden = self.folds.hidden(&self.regions);
        let Some(range) = hidden.iter().find(|r| r.contains(&line)) else {
            return;
        };
        let target = if line >= from {
            range.end
        } else {
            range.start - 1
        };
        let Some(text) = self.editor.line(target) else {
            return;
        };
        let mut column = cursor.position.column.min(text.text.len());
        while !text.text.is_char_boundary(column) {
            column -= 1;
        }
        drop(text);
        // `move_to` only ever sets a selection, never clears one; here the
        // selection (if any) is meant to stay.
        self.editor.move_to(Cursor {
            position: Position {
                line: target,
                column,
            },
            selection: cursor.selection,
        });
    }
}

/// Ordered (line, byte) ends of a selection.
pub type SelectionRange = ((usize, usize), (usize, usize));

impl Content {
    pub fn new() -> Self {
        Self::with_text("")
    }

    pub fn with_text(text: &str) -> Self {
        // The buffer keeps one line per `\n`, so a final newline would show
        // as an extra empty line; the app writes one back when it saves.
        let text = text.strip_suffix('\n').unwrap_or(text);
        Self(RefCell::new(Internal {
            id: NEXT_CONTENT_ID.fetch_add(1, Ordering::Relaxed),
            changed_from: None,
            editor: Editor::with_text(text),
            is_dirty: true,
            revision: 0,
            wrapping: text::Wrapping::Word,
            folds: Folds::default(),
            regions: Arc::default(),
            regions_key: None,
            reveal_cursor: false,
        }))
    }

    pub fn perform(&mut self, action: Action) {
        // cosmic-text drops one trailing control character from every pasted
        // segment (meant for the newline), which eats a final tab, and keeps
        // the `\r` of Windows line endings.
        if let Action::Edit(Edit::Paste(text)) = &action {
            let text = normalize_pasted(text);
            if text
                .chars()
                .next_back()
                .is_some_and(|c| c != '\n' && c.is_control())
            {
                // A sentinel keeps the tab; it is deleted right after.
                self.perform_raw(Action::Edit(Edit::Paste(Arc::new(format!("{text}x")))));
                self.perform_raw(Action::Edit(Edit::Backspace));
            } else {
                self.perform_raw(Action::Edit(Edit::Paste(Arc::new(text))));
            }
            return;
        }
        self.perform_raw(action);
    }

    fn perform_raw(&mut self, action: Action) {
        let internal = self.0.get_mut();
        let from = internal.editor.cursor().position.line;
        let lines_before = internal.editor.line_count();
        let moves_cursor = !matches!(action, Action::Edit(_) | Action::Scroll { .. });
        // Remember the topmost line an edit may touch (the selection can
        // start above the cursor), for the highlighter.
        let touched = matches!(action, Action::Edit(_)).then(|| {
            let cursor = internal.editor.cursor();
            let line = cursor.position.line;
            let above = cursor
                .selection
                .map_or(0, |anchor| anchor.line.abs_diff(line));
            line.saturating_sub(above)
        });
        if touched.is_some() {
            internal.revision += 1;
        }
        internal.editor.perform(action);
        if let Some(before) = touched {
            let after = internal.editor.cursor().position.line;
            let top = before.min(after);
            internal.changed_from = Some(internal.changed_from.map_or(top, |c| c.min(top)));
            // Folds below the edit move with their lines.
            let delta = internal.editor.line_count() as isize - lines_before as isize;
            internal.folds.shift(top, delta);
            internal.reveal_cursor = true;
        } else if moves_cursor {
            internal.step_out_of_folds(from);
        }
        internal.is_dirty = true;
    }

    pub fn line_count(&self) -> usize {
        self.0.borrow().editor.line_count()
    }

    /// Puts the cursor at (`line`, byte `col`) and, with `len_bytes`, selects
    /// that many bytes of the text after it (a selection may span lines).
    ///
    /// The position is set directly on the editor: no motion, no layout, so
    /// the cost doesn't depend on where in the file it is.
    pub fn select_bytes(&mut self, line: usize, col: usize, len_bytes: usize) {
        let line = line.min(self.line_count().saturating_sub(1));
        let start = (line, col);
        let end = if len_bytes == 0 {
            None
        } else {
            // Walk the lines the selection covers to find where it ends.
            let (mut line_i, mut at, mut left) = (line, col, len_bytes);
            loop {
                let Some(text) = self.line(line_i) else {
                    break Some((line_i.saturating_sub(1), usize::MAX));
                };
                let rest = text.len().saturating_sub(at);
                if left <= rest {
                    break Some((line_i, at + left));
                }
                left -= rest + 1;
                line_i += 1;
                at = 0;
            }
        };
        let internal = self.0.get_mut();
        // `move_to` only ever sets a selection, never clears one.
        if internal.editor.cursor().selection.is_some() {
            internal.editor.perform(Action::Move(Motion::Left));
        }
        let at = |(line, column)| Position { line, column };
        internal.editor.move_to(match end {
            Some(end) => Cursor {
                position: at(end),
                selection: Some(at(start)),
            },
            None => Cursor {
                position: at(start),
                selection: None,
            },
        });
        // A jump (search, go to line, bookmark) opens what hides its target.
        internal.reveal_cursor = true;
        internal.is_dirty = true;
    }

    /// Folds or unfolds blocks, for a text of the given kind.
    pub fn fold(&self, kind: Kind, command: FoldCommand) {
        let mut internal = self.0.borrow_mut();
        internal.refresh_folds(kind);
        let regions = Arc::clone(&internal.regions);
        match command {
            FoldCommand::AtCursor => {
                let line = internal.editor.cursor().position.line;
                if let Some(block) = regions.innermost(line) {
                    internal.folds.toggle(&regions, block, Scope::One);
                }
            }
            FoldCommand::All(fold) => internal.folds.set_all(&regions, fold),
            FoldCommand::Line(line, scope) => {
                if let Some(block) = regions.at(line) {
                    internal.folds.toggle(&regions, block, scope);
                }
            }
        }
        internal.refresh_folds(kind);
        internal.step_out_of_folds(usize::MAX);
        internal.refresh_folds(kind);
    }

    /// The lines folded away now.
    #[cfg(test)]
    fn hidden_lines(&self) -> Vec<Range<usize>> {
        let internal = self.0.borrow();
        internal.folds.hidden(&internal.regions)
    }

    pub fn line(&self, index: usize) -> Option<impl std::ops::Deref<Target = str> + '_> {
        std::cell::Ref::filter_map(self.0.borrow(), |internal| {
            match internal.editor.line(index)?.text {
                std::borrow::Cow::Borrowed(text) => Some(text),
                std::borrow::Cow::Owned(_) => None,
            }
        })
        .ok()
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
                        match internal.editor.line(self.current)?.text {
                            std::borrow::Cow::Borrowed(text) => Some(text),
                            std::borrow::Cow::Owned(_) => None,
                        }
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

    /// The selection as ordered (line, byte) ends; no text is copied.
    pub fn selection_range(&self) -> Option<SelectionRange> {
        let cursor = self.0.borrow().editor.cursor();
        let anchor = cursor.selection?;
        let (a, b) = (
            (anchor.line, anchor.column),
            (cursor.position.line, cursor.position.column),
        );
        if a == b {
            return None;
        }
        Some(if a < b { (a, b) } else { (b, a) })
    }

    /// Characters and lines in the selection, counted without copying it.
    pub fn selection_counts(&self) -> Option<(usize, usize)> {
        let (start, end) = self.selection_range()?;
        let mut chars = 0;
        for line_i in start.0..=end.0 {
            let line = self.line(line_i)?;
            let from = if line_i == start.0 { start.1 } else { 0 };
            let to = if line_i == end.0 { end.1 } else { line.len() };
            chars += line.get(from..to)?.chars().count();
            if line_i != end.0 {
                chars += 1;
            }
        }
        Some((chars, end.0 - start.0 + 1))
    }

    /// Word wrapping on or off (long lines then run off the edge).
    pub fn set_wrapping(&self, wrapping: bool) {
        self.0.borrow_mut().wrapping = if wrapping {
            text::Wrapping::Word
        } else {
            text::Wrapping::None
        };
    }

    /// Bumped by every edit.
    pub fn revision(&self) -> u64 {
        self.0.borrow().revision
    }

    pub fn selection(&self) -> Option<String> {
        self.0.borrow().editor.copy()
    }

    /// The cursor as (line, byte index within the line).
    pub fn cursor_position(&self) -> (usize, usize) {
        let position = self.0.borrow().editor.cursor().position;
        (position.line, position.column)
    }
}

/// A pasted text as the buffer wants it: only `\n` between lines.
fn normalize_pasted(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// Moves the cursor to the start of logical line `target`.
pub fn goto_line_start(content: &mut Content, target: usize) {
    content.select_bytes(target, 0, 0);
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
    /// A drag that started in the minimap.
    minimap_drag: bool,
    modifiers: keyboard::Modifiers,
    /// The line whose chevron the pointer is on.
    hovered_fold: Option<usize>,
    highlighter: RefCell<Highlighter>,
    highlighter_settings: Highlighter::Settings,
    highlighter_format_address: usize,
    /// The `Content` the highlighter has been following.
    content_id: u64,
    /// Per-line data of the minimap, recomputed only after an edit.
    minimap_cache: RefCell<MinimapCache>,
}

/// What the minimap derives from the text: it costs a pass over the whole
/// file, so it is kept until the text or the search changes.
#[derive(Default)]
struct MinimapCache {
    /// (content id, revision) the spans were computed for.
    key: Option<(u64, u64)>,
    spans: Vec<Option<(usize, usize)>>,
    /// (content id, revision, query, case, regex) the marked lines are for.
    search_key: Option<(u64, u64, String, bool, bool)>,
    marked: Vec<usize>,
}

impl<'a, Highlighter, Message, Theme> iced::advanced::Widget<Message, Theme, iced::Renderer>
    for CodeEditor<'a, Highlighter, Message, Theme>
where
    Highlighter: text::Highlighter,
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
            minimap_drag: false,
            modifiers: keyboard::Modifiers::default(),
            hovered_fold: None,
            highlighter: RefCell::new(Highlighter::new(&self.highlighter_settings)),
            highlighter_settings: self.highlighter_settings.clone(),
            highlighter_format_address: self.highlighter_format as usize,
            content_id: self.content.0.borrow().id,
            minimap_cache: RefCell::default(),
        })
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: self.width,
            height: self.height,
        }
    }

    fn layout(
        &mut self,
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

        if let Some(paint) = &self.folding {
            internal.refresh_folds(paint.kind);
        }

        let limits = limits.height(self.height);
        let padding = self.text_padding(renderer, internal.editor.line_count());
        let wrapping = internal.wrapping;
        internal.editor.update(
            limits.shrink(padding).max(),
            self.font.unwrap_or_else(|| renderer.default_font()),
            self.text_size.unwrap_or_else(|| renderer.default_size()),
            self.line_height,
            wrapping,
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
                        .expand(Size::new(0.0, padding.y())),
                )
            }
        }
    }

    fn update(
        &mut self,
        tree: &mut widget::Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let Some(on_edit) = self.on_edit.as_ref() else {
            return;
        };

        let state = tree.state.downcast_mut::<State<Highlighter>>();
        let padding = self.text_padding(renderer, self.content.line_count());

        if let Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) = event {
            state.modifiers = *modifiers;
        }

        if self.folding.is_some() {
            let hovered = cursor
                .position_in(layout.bounds())
                .and_then(|at| self.chevron_line_at(at, padding, renderer));
            if hovered != state.hovered_fold {
                state.hovered_fold = hovered;
                shell.request_redraw();
            }
        }

        if let (Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)), Some(on_fold)) =
            (event, &self.on_fold)
        {
            let bounds = layout.bounds();
            if let Some(line) = cursor
                .position_in(bounds)
                .and_then(|at| self.chevron_line_at(at, padding, renderer))
            {
                let modifiers = state.modifiers;
                let scope = if modifiers.alt() {
                    Scope::Subtree
                } else if modifiers.command() {
                    Scope::Siblings
                } else {
                    Scope::One
                };
                shell.publish(on_fold(line, scope));
                shell.capture_event();
                return;
            }
        }

        if self.minimap.is_some() {
            let rect = minimap_rect(layout.bounds());
            let target = match event {
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                    if cursor.is_over(rect) =>
                {
                    state.minimap_drag = true;
                    cursor.position().map(|p| p.y)
                }
                Event::Mouse(mouse::Event::CursorMoved { position }) if state.minimap_drag => {
                    Some(position.y)
                }
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                    if state.minimap_drag =>
                {
                    state.minimap_drag = false;
                    shell.capture_event();
                    return;
                }
                _ => None,
            };
            if let Some(y) = target {
                if let Some(lines) = self.minimap_scroll(y - rect.y, rect.height) {
                    shell.publish(on_edit(Action::Scroll { lines }));
                }
                shell.capture_event();
                return;
            }
        }

        let Some(update) = Update::from_event(event, state, layout.bounds(), padding, cursor)
        else {
            return;
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

        shell.capture_event();
    }

    fn draw(
        &self,
        tree: &widget::Tree,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
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

        let appearance = self.style;

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

        let size = self.size_px(renderer);
        let row_height = self.line_height.to_absolute(Pixels(size)).0;
        let cursor_line = internal.editor.cursor().position.line;

        // Behind the text: the page guide, the cursor line's wash and the
        // search matches. (Text is always painted above quads.)
        if let Some((column, color)) = self.guide {
            // JetBrains Mono's advance is 0.6 em.
            let x = origin.x + column as f32 * size * 0.6;
            if x < bounds.x + bounds.width {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle {
                            x,
                            y: bounds.y,
                            width: 1.0,
                            height: bounds.height,
                        },
                        ..renderer::Quad::default()
                    },
                    color,
                );
            }
        }
        let regex = self.search.as_ref().and_then(|p| p.regex.clone());
        for run in internal.editor.buffer().layout_runs() {
            if run.line_height <= 0.0 {
                continue;
            }
            let top = bounds.y + padding.top + run.line_top;
            if let Some(highlight) = self.line_highlight.filter(|_| run.line_i == cursor_line) {
                let left = origin.x - GUTTER_GAP;
                let band = Rectangle {
                    x: left,
                    y: top,
                    width: bounds.x + bounds.width
                        - if self.minimap.is_some() {
                            MINIMAP_WIDTH
                        } else {
                            0.0
                        }
                        - left,
                    height: row_height,
                };
                if let Some(band) = bounds.intersection(&band) {
                    renderer.fill_quad(
                        renderer::Quad {
                            bounds: band,
                            ..renderer::Quad::default()
                        },
                        highlight.background,
                    );
                    renderer.fill_quad(
                        renderer::Quad {
                            bounds: Rectangle { width: 2.0, ..band },
                            ..renderer::Quad::default()
                        },
                        highlight.bar,
                    );
                }
            }
            if let Some(paint) = &self.search {
                for range in line_matches(run.text, paint, regex.as_deref()) {
                    let current = paint
                        .current
                        .as_ref()
                        .is_some_and(|(l, r)| *l == run.line_i && *r == range);
                    let span = run.highlight(
                        Cursor_::new(run.line_i, range.start),
                        Cursor_::new(run.line_i, range.end),
                    );
                    let Some((x, width)) = span else { continue };
                    let rect = Rectangle {
                        x: origin.x + x,
                        y: top,
                        width,
                        height: row_height,
                    };
                    let Some(rect) = bounds.intersection(&rect) else {
                        continue;
                    };
                    renderer.fill_quad(
                        renderer::Quad {
                            bounds: rect,
                            border: iced::Border {
                                color: if current {
                                    paint.current_border
                                } else {
                                    Color::TRANSPARENT
                                },
                                width: if current { 1.0 } else { 0.0 },
                                radius: 3.0.into(),
                            },
                            ..renderer::Quad::default()
                        },
                        if current {
                            paint.current_color
                        } else {
                            paint.color
                        },
                    );
                }
            }
        }

        renderer.fill_editor(&internal.editor, origin, style.text_color, *viewport);

        let translation = Vector::new(bounds.x + padding.left, bounds.y + padding.top);
        let caret_height = (size * 1.3).min(row_height);

        if state.is_focused {
            match internal.editor.selection() {
                Selection::Caret(position) => {
                    let position = position + translation;

                    if bounds.contains(position) {
                        renderer.fill_quad(
                            renderer::Quad {
                                bounds: Rectangle {
                                    x: position.x,
                                    // A caret a little shorter than the row,
                                    // centred in it.
                                    y: position.y + (row_height - caret_height) / 2.0,
                                    width: 2.0,
                                    height: caret_height,
                                },
                                ..renderer::Quad::default()
                            },
                            self.style.value,
                        );
                    }
                }
                Selection::Range(ranges) => {
                    for range in ranges
                        .into_iter()
                        .filter_map(|range| bounds.intersection(&(range + translation)))
                    {
                        renderer.fill_quad(
                            renderer::Quad {
                                bounds: range,
                                ..renderer::Quad::default()
                            },
                            self.style.selection,
                        );
                    }
                }
            }
        }

        if let Some(gutter) = self.gutter {
            let width = gutter_width(size, internal.editor.line_count());
            let column = Rectangle {
                x: bounds.x + self.padding.left,
                y: bounds.y,
                width,
                height: bounds.height,
            };
            let clip = column.intersection(viewport).unwrap_or(column);
            let mut previous = None;
            // One layout run per visual row: a wrapped line produces several
            // runs sharing a `line_i`, and only the first gets its number.
            for run in internal.editor.buffer().layout_runs() {
                if previous == Some(run.line_i) || run.line_height <= 0.0 {
                    continue;
                }
                previous = Some(run.line_i);

                if let Some(fold) = &self.folding {
                    if internal.regions.at(run.line_i).is_some() {
                        let folded = internal.folds.is_folded(run.line_i);
                        {
                            let side = row_height.min(CHEVRON_WIDTH - 2.0);
                            let button = Rectangle {
                                x: column.x + width - CHEVRON_WIDTH / 2.0 - side / 2.0,
                                y: bounds.y
                                    + padding.top
                                    + run.line_top
                                    + (row_height - side) / 2.0,
                                width: side,
                                height: side,
                            };
                            if let Some(visible) = clip.intersection(&button) {
                                renderer.fill_quad(
                                    renderer::Quad {
                                        bounds: visible,
                                        border: iced::Border {
                                            radius: 5.0.into(),
                                            ..iced::Border::default()
                                        },
                                        ..renderer::Quad::default()
                                    },
                                    if folded {
                                        fold.marker_background
                                    } else {
                                        Color {
                                            a: fold.marker_background.a * 0.6,
                                            ..fold.marker_background
                                        }
                                    },
                                );
                            }
                            renderer.fill_text(
                                text::Text {
                                    content: if folded { "▸" } else { "▾" }.to_string(),
                                    bounds: Size::new(CHEVRON_WIDTH, row_height),
                                    size: Pixels(size * 1.5),
                                    line_height: self.line_height,
                                    font: self.font.unwrap_or(Font::MONOSPACE),
                                    align_x: text::Alignment::Center,
                                    align_y: alignment::Vertical::Center,
                                    shaping: text::Shaping::Advanced,
                                    wrapping: text::Wrapping::None,
                                },
                                Point::new(
                                    column.x + width - CHEVRON_WIDTH / 2.0,
                                    bounds.y + padding.top + run.line_top + row_height / 2.0,
                                ),
                                if folded { fold.folded } else { fold.chevron },
                                clip,
                            );
                        }
                    }
                }

                let number = (run.line_i + 1).to_string();
                renderer.fill_text(
                    text::Text {
                        content: number.clone(),
                        bounds: Size::new(width - CHEVRON_WIDTH, row_height),
                        size: Pixels(size),
                        line_height: self.line_height,
                        font: self.font.unwrap_or(Font::MONOSPACE),
                        align_x: text::Alignment::Right,
                        align_y: alignment::Vertical::Center,
                        shaping: text::Shaping::Basic,
                        wrapping: text::Wrapping::None,
                    },
                    // With right/centre alignment the position is the anchor of
                    // the text, not the corner of its bounds.
                    Point::new(
                        column.x + width - CHEVRON_WIDTH,
                        bounds.y + padding.top + run.line_top + row_height / 2.0,
                    ),
                    if run.line_i == cursor_line {
                        gutter.current
                    } else {
                        gutter.text
                    },
                    clip,
                );
            }
        }

        if let Some(marks) = &self.bookmarks {
            // A small flag at the left edge of the gutter (or of the text,
            // without line numbers), on the first row of each marked line.
            let x = bounds.x + self.padding.left + 6.0;
            let mut previous = None;
            for run in internal.editor.buffer().layout_runs() {
                if previous == Some(run.line_i) || run.line_height <= 0.0 {
                    continue;
                }
                previous = Some(run.line_i);
                if marks.lines.binary_search(&run.line_i).is_err() {
                    continue;
                }
                let height = 11.0_f32.min(row_height);
                let flag = Rectangle {
                    x,
                    y: bounds.y + padding.top + run.line_top + (row_height - height) / 2.0,
                    width: 7.0,
                    height,
                };
                if let Some(flag) = bounds.intersection(&flag) {
                    renderer.fill_quad(
                        renderer::Quad {
                            bounds: flag,
                            border: iced::Border {
                                radius: iced::border::Radius::default()
                                    .top_left(2.0)
                                    .top_right(2.0)
                                    .bottom_right(1.0)
                                    .bottom_left(1.0),
                                ..iced::Border::default()
                            },
                            ..renderer::Quad::default()
                        },
                        marks.color,
                    );
                }
            }
        }

        // After a folded block's header, a marker for what is hidden.
        if let Some(fold) = &self.folding {
            let runs: Vec<_> = internal.editor.buffer().layout_runs().collect();
            for (i, run) in runs.iter().enumerate() {
                let last_row = runs.get(i + 1).is_none_or(|next| next.line_i != run.line_i);
                if !last_row || run.line_height <= 0.0 || !internal.folds.is_folded(run.line_i) {
                    continue;
                }
                let height = (row_height - 8.0).max(6.0);
                let pill = Rectangle {
                    x: origin.x + run.line_w + 8.0,
                    y: bounds.y + padding.top + run.line_top + (row_height - height) / 2.0,
                    width: 26.0,
                    height,
                };
                if let Some(visible) = bounds.intersection(&pill) {
                    renderer.fill_quad(
                        renderer::Quad {
                            bounds: visible,
                            border: iced::Border {
                                radius: 4.0.into(),
                                ..iced::Border::default()
                            },
                            ..renderer::Quad::default()
                        },
                        fold.marker_background,
                    );
                    renderer.fill_text(
                        text::Text {
                            content: "⋯".to_string(),
                            bounds: pill.size(),
                            size: Pixels(size * 0.9),
                            line_height: self.line_height,
                            font: self.font.unwrap_or(Font::MONOSPACE),
                            align_x: text::Alignment::Center,
                            align_y: alignment::Vertical::Center,
                            shaping: text::Shaping::Advanced,
                            wrapping: text::Wrapping::None,
                        },
                        pill.center(),
                        fold.folded,
                        visible,
                    );
                }
            }
        }

        // What the chevron under the pointer can do, in a layer of its own
        // (text and quads of the layer below would hide it).
        if let (Some(line), Some(fold)) = (state.hovered_fold, &self.folding) {
            let top = internal
                .editor
                .buffer()
                .layout_runs()
                .find(|run| run.line_i == line && run.line_height > 0.0)
                .map(|run| bounds.y + padding.top + run.line_top);
            if let Some(top) = top.filter(|_| !fold.tooltip.is_empty()) {
                let text_size = 12.0;
                let row = 18.0;
                let pad = 8.0;
                let chars = fold.tooltip.iter().map(|l| l.chars().count()).max();
                let width = chars.unwrap_or(0) as f32 * text_size * 0.66 + 2.0 * pad;
                let height = fold.tooltip.len() as f32 * row + 2.0 * pad;
                let x = bounds.x + padding.left + 4.0;
                let y = (top + row_height + 2.0).min(bounds.y + bounds.height - height - 4.0);
                let card = Rectangle {
                    x,
                    y,
                    width,
                    height,
                };
                renderer.with_layer(bounds, |renderer| {
                    renderer.fill_quad(
                        renderer::Quad {
                            bounds: card,
                            border: iced::Border {
                                color: fold.tooltip_border,
                                width: 1.0,
                                radius: 6.0.into(),
                            },
                            ..renderer::Quad::default()
                        },
                        fold.tooltip_background,
                    );
                    for (i, line) in fold.tooltip.iter().enumerate() {
                        renderer.fill_text(
                            text::Text {
                                content: line.clone(),
                                bounds: Size::new(width - 2.0 * pad, row),
                                size: Pixels(text_size),
                                line_height: LineHeight::Absolute(Pixels(row)),
                                font: self.font.unwrap_or(Font::MONOSPACE),
                                align_x: text::Alignment::Left,
                                align_y: alignment::Vertical::Top,
                                shaping: text::Shaping::Advanced,
                                wrapping: text::Wrapping::None,
                            },
                            Point::new(x + pad, y + pad + i as f32 * row),
                            fold.tooltip_text,
                            card,
                        );
                    }
                });
            }
        }

        if let Some(minimap) = self.minimap {
            self.draw_minimap(
                renderer,
                &internal.editor,
                (internal.id, internal.revision),
                &state.minimap_cache,
                minimap_rect(bounds),
                minimap,
                regex.as_deref(),
            );
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

        if self.minimap.is_some() && cursor.is_over(minimap_rect(layout.bounds())) {
            mouse::Interaction::Pointer
        } else if cursor.is_over(layout.bounds()) {
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
    Theme: 'a,
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
        event: &Event,
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
                        let click = mouse::Click::new(
                            cursor_position - inset,
                            mouse::Button::Left,
                            state.last_click,
                        );
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

                    // A shortcut (Ctrl/Cmd+key) types nothing; AltGr, which
                    // some systems report as Ctrl+Alt, still does.
                    let typing_shortcut = modifiers.command() && !modifiers.alt();
                    if let Some(text) = text {
                        if let Some(c) = text.chars().find(|c| !c.is_control()) {
                            if typing_shortcut {
                                return None;
                            }
                            return edit(Edit::Insert(c));
                        }
                    }

                    if let keyboard::Key::Named(named_key) = key.as_ref() {
                        // Alt+Up / Alt+Down move the line (an app shortcut).
                        if modifiers.alt()
                            && matches!(named_key, key::Named::ArrowUp | key::Named::ArrowDown)
                        {
                            return None;
                        }
                        if let Some(motion) = motion(named_key) {
                            let motion = if is_jump_modifier_pressed(*modifiers) {
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

/// Whether the editor itself handles this key combination (a shortcut bound
/// to it could never fire while the editor has the focus). `key` is the
/// name an app shortcut uses.
pub fn consumes_key(key: &str, ctrl: bool, alt: bool) -> bool {
    match key {
        "Return" | "Backspace" | "Delete" | "ArrowLeft" | "ArrowRight" | "Home" | "End"
        | "PageUp" | "PageDown" => true,
        // Alt+Up/Down are left to the app (see `Update::from_event`).
        "ArrowUp" | "ArrowDown" => !alt,
        "c" | "x" | "v" => ctrl && !alt,
        _ => false,
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

    fn key_event(key: key::Named, modifiers: keyboard::Modifiers) -> Event {
        Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(key),
            modified_key: keyboard::Key::Named(key),
            physical_key: key::Physical::Unidentified(key::NativeCode::Unidentified),
            location: keyboard::Location::Standard,
            modifiers,
            text: None,
            repeat: false,
        })
    }

    const JSON: &str = "{\n  \"a\": [\n    1,\n    2\n  ],\n  \"b\": 1\n}\n";

    fn json_kind() -> Kind {
        Kind::of(Some("json"))
    }

    /// A text with its inner array folded: lines 2 and 3 are hidden.
    fn folded_json() -> Content {
        let mut content = Content::with_text(JSON);
        content.select_bytes(1, 0, 0);
        content.fold(json_kind(), FoldCommand::AtCursor);
        content
    }

    #[test]
    fn folding_the_block_at_the_cursor_hides_its_lines() {
        let content = folded_json();
        assert_eq!(content.hidden_lines(), vec![2..4]);
        content.fold(json_kind(), FoldCommand::AtCursor);
        assert!(content.hidden_lines().is_empty());
    }

    #[test]
    fn fold_all_and_a_click_on_a_chevron() {
        let content = Content::with_text(JSON);
        content.fold(json_kind(), FoldCommand::All(true));
        assert_eq!(content.hidden_lines(), vec![1..6]);
        content.fold(json_kind(), FoldCommand::All(false));
        content.fold(json_kind(), FoldCommand::Line(1, Scope::One));
        assert_eq!(content.hidden_lines(), vec![2..4]);
    }

    #[test]
    fn folding_moves_a_cursor_that_was_inside_to_the_header() {
        let mut content = Content::with_text(JSON);
        content.select_bytes(2, 1, 0);
        content.fold(json_kind(), FoldCommand::All(true));
        assert_eq!(content.cursor_position(), (0, 1));
    }

    #[test]
    fn a_fold_follows_the_lines_added_above_it() {
        let mut content = folded_json();
        content.select_bytes(0, 0, 0);
        content.perform(Action::Edit(Edit::Enter));
        content.fold(json_kind(), FoldCommand::Line(99, Scope::One));
        assert_eq!(content.hidden_lines(), vec![3..5]);
    }

    #[test]
    fn jumping_into_a_fold_opens_it() {
        let mut content = folded_json();
        content.select_bytes(3, 0, 0);
        content.fold(json_kind(), FoldCommand::Line(99, Scope::One));
        assert!(content.hidden_lines().is_empty());
    }

    fn focused_state() -> State<highlighter::PlainText> {
        State {
            is_focused: true,
            last_click: None,
            drag_click: None,
            partial_scroll: 0.0,
            minimap_drag: false,
            modifiers: keyboard::Modifiers::default(),
            hovered_fold: None,
            highlighter: RefCell::new(highlighter::PlainText::new(&())),
            highlighter_settings: (),
            highlighter_format_address: 0,
            content_id: 0,
            minimap_cache: RefCell::default(),
        }
    }

    fn from_key(key: key::Named, modifiers: keyboard::Modifiers) -> Option<Update> {
        Update::from_event(
            &key_event(key, modifiers),
            &focused_state(),
            Rectangle::new(Point::ORIGIN, Size::new(100.0, 100.0)),
            Padding::ZERO,
            mouse::Cursor::Unavailable,
        )
    }

    #[test]
    fn alt_arrows_reach_the_app_but_plain_arrows_move_the_cursor() {
        assert!(from_key(key::Named::ArrowUp, keyboard::Modifiers::ALT).is_none());
        assert!(from_key(key::Named::ArrowDown, keyboard::Modifiers::ALT).is_none());
        assert!(matches!(
            from_key(key::Named::ArrowUp, keyboard::Modifiers::empty()),
            Some(Update::Action(Action::Move(Motion::Up)))
        ));
    }

    #[test]
    fn consumed_keys_are_known() {
        assert!(!consumes_key("ArrowUp", false, true));
        assert!(consumes_key("ArrowUp", false, false));
        assert!(consumes_key("c", true, false));
        assert!(!consumes_key("d", true, false));
    }

    #[test]
    fn minimap_lines_fit_the_height_but_never_exceed_two_pixels() {
        assert_eq!(minimap_scale(10, 800.0), 2.0);
        // 1000 lines in 800 - 16 px.
        assert!((minimap_scale(1000, 800.0) - 0.784).abs() < 1e-3);
        assert!(minimap_scale(0, 800.0) > 0.0);
    }

    #[test]
    fn minimap_position_maps_back_to_a_line() {
        let scale = minimap_scale(1000, 800.0);
        assert_eq!(minimap_line_at(0.0, scale, 1000), 0);
        assert_eq!(minimap_line_at(800.0, scale, 1000), 999);
        let y = MINIMAP_INSET + 500.0 * scale + 0.1;
        assert_eq!(minimap_line_at(y, scale, 1000), 500);
        assert_eq!(minimap_line_at(5.0, 2.0, 0), 0);
    }

    #[test]
    fn minimap_span_counts_indentation_but_not_blank_lines() {
        assert_eq!(minimap_span("fn main() {"), Some((0, 11)));
        assert_eq!(minimap_span("    let x = 1;  "), Some((4, 10)));
        assert_eq!(minimap_span("\tx"), Some((4, 1)));
        assert_eq!(minimap_span("   "), None);
        assert_eq!(minimap_span(""), None);
    }

    #[test]
    fn gutter_has_a_minimum_width_and_grows_with_the_line_count() {
        assert_eq!(gutter_width(16.0, 5), gutter_width(16.0, 99));
        assert_eq!(gutter_width(16.0, 5), 54.0);
        assert!(gutter_width(16.0, 999) > gutter_width(16.0, 99));
        assert!(gutter_width(16.0, 9999) > gutter_width(16.0, 999));
        assert!(gutter_width(30.0, 999) > gutter_width(12.0, 999));
    }
}
