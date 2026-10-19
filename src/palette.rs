//! The command palette: a searchable list of the app's main actions, opened
//! with a shortcut (Ctrl/Cmd+Shift+P by default).

use iced::{
    widget::{button, column, container, row, text, text_input, Space},
    Alignment, Element, Length,
};

use crate::app::Message;
use crate::config::Config;
use crate::theme;

/// What a palette entry does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteCommand {
    NewFile,
    OpenFile,
    Find,
    FormatCode,
    GotoLine,
    SetLanguage,
    ToggleSidebar,
}

impl PaletteCommand {
    /// The message that performs the command.
    pub fn message(self) -> Message {
        match self {
            PaletteCommand::NewFile => Message::NewFile,
            PaletteCommand::OpenFile => Message::OpenFile,
            PaletteCommand::Find => Message::ToggleSearch,
            PaletteCommand::FormatCode => Message::FormatFile,
            PaletteCommand::GotoLine => Message::OpenGotoLine,
            PaletteCommand::SetLanguage => Message::OpenLanguagePicker,
            PaletteCommand::ToggleSidebar => Message::ToggleSidebar,
        }
    }
}

/// One row of the palette.
#[derive(Debug, Clone)]
pub struct Entry {
    pub command: PaletteCommand,
    pub label: String,
    pub shortcut: Option<String>,
    /// Disabled entries are listed but do nothing (no formatter for this
    /// file type, or a language that is already detected).
    pub enabled: bool,
}

pub fn input_id() -> text_input::Id {
    text_input::Id::new("command-palette")
}

/// Every command, with its current shortcut.
pub fn entries(config: &Config, has_formatter: bool, language_enabled: bool) -> Vec<Entry> {
    let sh = &config.shortcuts;
    let entry = |command, label: String, shortcut: Option<String>, enabled| Entry {
        command,
        label,
        shortcut,
        enabled,
    };
    vec![
        entry(
            PaletteCommand::NewFile,
            t!("palette.new_file").to_string(),
            Some(sh.new_file.display()),
            true,
        ),
        entry(
            PaletteCommand::OpenFile,
            t!("palette.open_file").to_string(),
            Some(sh.open_file.display()),
            true,
        ),
        entry(
            PaletteCommand::Find,
            t!("palette.find").to_string(),
            Some(sh.find.display()),
            true,
        ),
        entry(
            PaletteCommand::FormatCode,
            t!("palette.format_code").to_string(),
            Some(sh.format_code.display()),
            has_formatter,
        ),
        entry(
            PaletteCommand::GotoLine,
            t!("palette.goto_line").to_string(),
            Some(sh.goto_line.display()),
            true,
        ),
        entry(
            PaletteCommand::SetLanguage,
            t!("palette.set_language").to_string(),
            None,
            language_enabled,
        ),
        entry(
            PaletteCommand::ToggleSidebar,
            t!("palette.toggle_sidebar").to_string(),
            Some(sh.toggle_sidebar.display()),
            true,
        ),
    ]
}

/// Whether `query` matches `label`: every character of the query appears in
/// the label, in order, ignoring case ("frmt" finds "Formater le code").
fn matches(label: &str, query: &str) -> bool {
    let mut label = label.chars().flat_map(char::to_lowercase);
    query
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .all(|q| label.any(|l| l == q))
}

/// The entries that match the query, in their original order.
pub fn filter(entries: Vec<Entry>, query: &str) -> Vec<Entry> {
    entries
        .into_iter()
        .filter(|e| matches(&e.label, query))
        .collect()
}

/// Wraps an index into `0..len` (moving past either end wraps around).
pub fn step(selected: usize, delta: i32, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    (selected as i64 + delta as i64).rem_euclid(len as i64) as usize
}

/// The palette box: query field, then the list.
pub fn view<'a>(
    dark: bool,
    query: &'a str,
    shown: &[Entry],
    selected: usize,
) -> Element<'a, Message> {
    let p = theme::palette(dark);

    let field = row![
        text(">").size(15).font(theme::MONO).style(p.accent),
        text_input(&t!("palette.placeholder"), query)
            .id(input_id())
            .on_input(Message::PaletteQueryChanged)
            .on_submit(Message::PaletteSubmit)
            .size(15)
            .padding(0)
            .style(iced::theme::TextInput::Custom(Box::new(theme::BareField {
                dark
            }))),
        container(text("esc").size(11).font(theme::MONO).style(p.muted))
            .padding([2, 6])
            .style(theme::badge(dark)),
    ]
    .spacing(10)
    .padding([0, 16])
    .height(52)
    .align_items(Alignment::Center);

    let mut list = column![container(
        text(t!("palette.title").to_string().to_uppercase())
            .size(10.5)
            .font(theme::UI_MEDIUM)
            .style(p.muted)
    )
    .padding([8, 10, 4, 10])]
    .spacing(1)
    .padding(6);

    if shown.is_empty() {
        list = list.push(
            container(
                text(t!("palette.no_results").to_string())
                    .size(13)
                    .style(p.muted),
            )
            .padding([9, 10]),
        );
    }
    for (i, e) in shown.iter().enumerate() {
        let highlighted = i == selected;
        let shortcut_color = if highlighted { p.accent } else { p.muted };
        let label_color = if !e.enabled {
            p.muted
        } else if highlighted {
            p.accent
        } else {
            p.text
        };
        let content = row![
            text(e.label.clone()).size(13).style(label_color),
            Space::with_width(Length::Fill),
            text(e.shortcut.clone().unwrap_or_default())
                .size(11.5)
                .font(theme::MONO)
                .style(shortcut_color),
        ]
        .spacing(10)
        .align_items(Alignment::Center);
        let mut b = button(content).padding([9, 10]).width(Length::Fill).style(
            iced::theme::Button::custom(theme::MenuItem {
                dark,
                radius: theme::R_ROW,
                highlighted,
            }),
        );
        if e.enabled {
            b = b.on_press(Message::PaletteRun(e.command));
        }
        list = list.push(b);
    }

    container(column![
        field,
        container(Space::with_height(1))
            .width(Length::Fill)
            .style(theme::rule(dark)),
        list,
    ])
    .width(540)
    .style(theme::palette_card(dark))
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(command: PaletteCommand, label: &str) -> Entry {
        Entry {
            command,
            label: label.to_string(),
            shortcut: None,
            enabled: true,
        }
    }

    // Built by hand: `entries` uses translations, whose locale is global
    // state shared with other tests.
    fn sample() -> Vec<Entry> {
        vec![
            entry(PaletteCommand::NewFile, "New file"),
            entry(PaletteCommand::OpenFile, "Open…"),
            entry(PaletteCommand::Find, "Find / Replace…"),
            entry(PaletteCommand::FormatCode, "Format code"),
            entry(PaletteCommand::GotoLine, "Go to line…"),
        ]
    }

    #[test]
    fn empty_query_lists_everything() {
        assert_eq!(filter(sample(), "").len(), 5);
        assert_eq!(filter(sample(), "   ").len(), 5);
    }

    #[test]
    fn query_is_a_case_insensitive_subsequence() {
        let hits = filter(sample(), "FRMT");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].command, PaletteCommand::FormatCode);
        // Spaces in the query are ignored: "g l" finds "Go to line…" only.
        assert_eq!(filter(sample(), "g l").len(), 1);
        assert!(filter(sample(), "zzzz").is_empty());
    }

    #[test]
    fn selection_wraps_around() {
        assert_eq!(step(0, -1, 7), 6);
        assert_eq!(step(6, 1, 7), 0);
        assert_eq!(step(2, 1, 7), 3);
        assert_eq!(step(0, 1, 0), 0);
    }

    #[test]
    fn every_command_maps_to_a_message() {
        let all = [
            PaletteCommand::NewFile,
            PaletteCommand::OpenFile,
            PaletteCommand::Find,
            PaletteCommand::FormatCode,
            PaletteCommand::GotoLine,
            PaletteCommand::SetLanguage,
            PaletteCommand::ToggleSidebar,
        ];
        for c in all {
            let _ = c.message();
        }
    }

    #[test]
    fn the_real_list_has_every_command_once() {
        let list = entries(&Config::default(), true, true);
        assert_eq!(list.len(), 7);
    }
}
