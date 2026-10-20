//! Alternate renderings of the current buffer (Markdown preview, JSON tree).
//!
//! Only Markdown and JSON files get a preview; every other language stays in
//! [`ViewMode::Raw`] and shows no toolbar at all.

pub mod json;
pub mod markdown;

use iced::{
    widget::{button, container, row, text},
    Alignment, Element,
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
}

/// The segmented control that switches the editor pane between Raw, Preview
/// and Split (or Raw and Tree for JSON); it sits at the right of the
/// editor's toolbar.
pub fn segmented(kind: PreviewKind, mode: ViewMode, dark: bool) -> Element<'static, Message> {
    let mut bar = row![].spacing(2).align_y(Alignment::Center);

    for m in kind.modes() {
        bar = bar.push(
            button(text(kind.label(*m)).size(12))
                .padding([3, 10])
                .on_press(Message::SetViewMode(*m))
                .style(
                    (theme::Segment {
                        dark,
                        active: mode == *m,
                        radius: theme::R_SM,
                    })
                    .style(),
                ),
        );
    }

    container(bar)
        .padding(2)
        .style(theme::field(dark, false, theme::R_ROW))
        .into()
}
