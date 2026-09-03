//! Alternate renderings of the current buffer (Markdown preview, JSON tree).
//!
//! Only Markdown and JSON files get a preview; every other language stays in
//! [`ViewMode::Raw`] and shows no toolbar at all.

pub mod json;
pub mod markdown;

use iced::{
    widget::{button, container, row, text, tooltip, Space},
    Alignment, Element, Length,
};

use crate::app::Message;
use crate::theme;

/// How the editor pane renders the current buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    /// The plain text editor.
    Raw,
    /// The rendered preview only (Markdown document / JSON tree).
    Preview,
    /// Editor and preview side by side. Markdown only.
    Split,
}

/// Which preview a buffer supports, derived from its language token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewKind {
    Markdown,
    Json,
}

/// The preview available for a language token, if any.
pub fn kind_for(language: Option<&str>) -> Option<PreviewKind> {
    match language {
        Some("md") => Some(PreviewKind::Markdown),
        Some("json") => Some(PreviewKind::Json),
        _ => None,
    }
}

impl PreviewKind {
    /// The modes offered by this preview, in toolbar order.
    pub fn modes(self) -> &'static [ViewMode] {
        match self {
            PreviewKind::Markdown => &[ViewMode::Raw, ViewMode::Preview, ViewMode::Split],
            PreviewKind::Json => &[ViewMode::Raw, ViewMode::Preview],
        }
    }

    fn label(self, mode: ViewMode) -> String {
        match (self, mode) {
            (_, ViewMode::Raw) => t!("preview.raw").to_string(),
            (PreviewKind::Markdown, ViewMode::Preview) => t!("preview.rendered").to_string(),
            (PreviewKind::Json, ViewMode::Preview) => t!("preview.tree").to_string(),
            (_, ViewMode::Split) => t!("preview.split").to_string(),
        }
    }

    /// Stacked lines for the source, a pilcrow for the rendered document, a
    /// split square for the side-by-side view, literal braces for the JSON
    /// tree — chosen so every one of them is a plain, unambiguous glyph
    /// rather than a symbol that depends on a particular font's coverage.
    fn icon(self, mode: ViewMode) -> &'static str {
        match (self, mode) {
            (_, ViewMode::Raw) => "\u{2261}",
            (PreviewKind::Markdown, ViewMode::Preview) => "\u{00B6}",
            (PreviewKind::Json, ViewMode::Preview) => "{}",
            (_, ViewMode::Split) => "\u{25EB}",
        }
    }
}

/// Height of the mode toolbar, kept in sync with its padding.
pub const TOOLBAR_HEIGHT: f32 = 28.0;

/// The small right-aligned mode switcher shown at the top of the editor pane.
pub fn toolbar(kind: PreviewKind, mode: ViewMode, dark: bool) -> Element<'static, Message> {
    let mut bar = row![Space::with_width(Length::Fill)].spacing(2);

    for m in kind.modes() {
        // Advanced shaping: the same fix as the preview's own emoji glyphs,
        // in case one of these symbols isn't in whatever font the button
        // ends up resolving on a given system.
        let icon = button(
            text(kind.icon(*m))
                .size(15)
                .shaping(iced::widget::text::Shaping::Advanced),
        )
        .padding([1, 7])
        .on_press(Message::SetViewMode(*m))
        .style(iced::theme::Button::custom(theme::GhostButton {
            dark,
            active: mode == *m,
        }));

        bar = bar.push(
            tooltip(
                icon,
                text(kind.label(*m)).size(11),
                tooltip::Position::Bottom,
            )
            .gap(4)
            .padding(6)
            .style(theme::card(dark)),
        );
    }

    container(bar.align_items(Alignment::Center).padding([2, 8]))
        .width(Length::Fill)
        .height(Length::Fixed(TOOLBAR_HEIGHT))
        .style(theme::bar(dark))
        .into()
}
