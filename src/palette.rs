//! The command palette: a searchable list of the app's main actions, opened
//! with a shortcut (Ctrl/Cmd+Shift+P by default).

use iced::{
    widget::{button, column, container, row, scrollable, text, text_input, Space},
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
    SaveFile,
    SaveFileAs,
    CloseFile,
    Find,
    FormatCode,
    GotoLine,
    SetLanguage,
    ToggleSidebar,
    ToggleTheme,
    Preferences,
    ToggleBookmark,
    NextBookmark,
    PrevBookmark,
    ToggleFold,
    FoldAll,
    UnfoldAll,
    CheckForUpdate,
    Shortcuts,
    Changelog,
    Quit,
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
            PaletteCommand::SaveFile => Message::SaveFile,
            PaletteCommand::SaveFileAs => Message::SaveFileAs,
            PaletteCommand::CloseFile => Message::CloseFile,
            // The theme flips: the palette knows only the command, so the
            // app resolves the target (see `Message::ToggleTheme`).
            PaletteCommand::ToggleTheme => Message::ToggleTheme,
            PaletteCommand::Preferences => Message::TogglePreferences,
            PaletteCommand::ToggleBookmark => Message::ToggleBookmark,
            PaletteCommand::NextBookmark => Message::NextBookmark,
            PaletteCommand::PrevBookmark => Message::PrevBookmark,
            PaletteCommand::ToggleFold => Message::ToggleFold,
            PaletteCommand::FoldAll => Message::FoldAll,
            PaletteCommand::UnfoldAll => Message::UnfoldAll,
            PaletteCommand::CheckForUpdate => Message::CheckForUpdate,
            PaletteCommand::Shortcuts => Message::OpenShortcuts,
            PaletteCommand::Changelog => Message::OpenChangelog,
            PaletteCommand::Quit => Message::Quit,
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

pub fn input_id() -> iced::widget::Id {
    iced::widget::Id::new("command-palette")
}

pub fn list_id() -> iced::widget::Id {
    iced::widget::Id::new("command-palette-list")
}

/// Approximate height of one row, with its spacing.
const ROW_HEIGHT: f32 = 35.0;

/// Scrolls the list so the selected row stays in view (about four rows of
/// context above it).
pub fn scroll_to_selected<T: 'static>(selected: usize) -> iced::Task<T> {
    iced::widget::operation::scroll_to(
        list_id(),
        iced::widget::scrollable::AbsoluteOffset {
            x: 0.0,
            y: ((selected as f32 - 4.0) * ROW_HEIGHT).max(0.0),
        },
    )
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
        entry(
            PaletteCommand::SaveFile,
            t!("menu.file_save").to_string(),
            Some(sh.save_file.display()),
            true,
        ),
        entry(
            PaletteCommand::SaveFileAs,
            t!("menu.file_save_as").to_string(),
            Some(sh.save_as.display()),
            true,
        ),
        entry(
            PaletteCommand::CloseFile,
            t!("menu.file_close").to_string(),
            Some(sh.close_file.display()),
            true,
        ),
        entry(
            PaletteCommand::ToggleTheme,
            t!("palette.toggle_theme").to_string(),
            None,
            true,
        ),
        entry(
            PaletteCommand::Preferences,
            t!("menu.edit_preferences").to_string(),
            None,
            true,
        ),
        entry(
            PaletteCommand::ToggleBookmark,
            t!("shortcuts.toggle_bookmark").to_string(),
            Some(sh.toggle_bookmark.display()),
            true,
        ),
        entry(
            PaletteCommand::NextBookmark,
            t!("menu.bookmark_next").to_string(),
            Some(sh.next_bookmark.display()),
            true,
        ),
        entry(
            PaletteCommand::PrevBookmark,
            t!("menu.bookmark_prev").to_string(),
            Some(sh.prev_bookmark.display()),
            true,
        ),
        entry(
            PaletteCommand::ToggleFold,
            t!("menu.fold_toggle").to_string(),
            Some(sh.toggle_fold.display()),
            true,
        ),
        entry(
            PaletteCommand::FoldAll,
            t!("menu.fold_all").to_string(),
            None,
            true,
        ),
        entry(
            PaletteCommand::UnfoldAll,
            t!("menu.unfold_all").to_string(),
            None,
            true,
        ),
        entry(
            PaletteCommand::Shortcuts,
            t!("menu.help_shortcuts").to_string(),
            None,
            true,
        ),
        entry(
            PaletteCommand::CheckForUpdate,
            t!("menu.help_check_updates").to_string(),
            None,
            true,
        ),
        entry(
            PaletteCommand::Changelog,
            t!("menu.help_changelog").to_string(),
            None,
            true,
        ),
        entry(
            PaletteCommand::Quit,
            t!("menu.file_quit").to_string(),
            Some(sh.quit.display()),
            true,
        ),
    ]
}

/// The lowercase base letter of `c`: "É" and "é" both give "e", so a query
/// typed without accents finds a label that has them, and the other way round.
fn fold(c: char) -> char {
    match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => 'a',
        'ç' | 'ć' | 'č' | 'ĉ' | 'ċ' => 'c',
        'ď' | 'đ' => 'd',
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => 'e',
        'ĝ' | 'ğ' | 'ġ' | 'ģ' => 'g',
        'ì' | 'í' | 'î' | 'ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ı' => 'i',
        'ĺ' | 'ļ' | 'ľ' | 'ł' => 'l',
        'ñ' | 'ń' | 'ņ' | 'ň' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ŏ' | 'ő' | 'œ' => 'o',
        'ŕ' | 'ŗ' | 'ř' => 'r',
        'ś' | 'ŝ' | 'ş' | 'š' | 'ß' => 's',
        'ţ' | 'ť' => 't',
        'ù' | 'ú' | 'û' | 'ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => 'u',
        'ý' | 'ÿ' | 'ŷ' => 'y',
        'ź' | 'ż' | 'ž' => 'z',
        other => other,
    }
}

/// Lowercase letters with their accents removed.
fn folded(text: &str) -> impl Iterator<Item = char> + '_ {
    text.chars().flat_map(char::to_lowercase).map(fold)
}

/// Whether `query` matches `label`: every character of the query appears in
/// the label, in order, ignoring case and accents ("frmt" finds "Formater le
/// code", "pref" finds "Préférences").
fn matches(label: &str, query: &str) -> bool {
    let mut label = folded(label);
    folded(query)
        .filter(|c| !c.is_whitespace())
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
        text(">").size(15).font(theme::MONO).color(p.accent),
        text_input(&t!("palette.placeholder"), query)
            .id(input_id())
            .on_input(Message::PaletteQueryChanged)
            .on_submit(Message::PaletteSubmit)
            .size(15)
            .padding(0)
            .style((theme::BareField { dark }).style()),
        container(
            text(t!("key.escape").to_string())
                .size(11)
                .font(theme::MONO)
                .color(p.muted)
        )
        .padding([2, 6])
        .style(theme::badge(dark)),
    ]
    .spacing(10)
    .padding([0, 16])
    .height(52)
    .align_y(Alignment::Center);

    let mut list = column![container(
        text(t!("palette.title").to_string().to_uppercase())
            .size(10.5)
            .font(theme::UI_MEDIUM)
            .color(p.muted)
    )
    .padding(iced::Padding {
        top: 8.0,
        right: 10.0,
        bottom: 4.0,
        left: 10.0
    })]
    .spacing(1)
    .padding(6);

    if shown.is_empty() {
        list = list.push(
            container(
                text(t!("palette.no_results").to_string())
                    .size(13)
                    .color(p.muted),
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
            text(e.label.clone()).size(13).color(label_color),
            Space::new().width(Length::Fill),
            text(e.shortcut.clone().unwrap_or_default())
                .size(11.5)
                .font(theme::MONO)
                .color(shortcut_color),
        ]
        .spacing(10)
        .align_y(Alignment::Center);
        let mut b = button(content).padding([9, 10]).width(Length::Fill).style(
            (theme::MenuItem {
                dark,
                radius: theme::R_ROW,
                highlighted,
            })
            .style(),
        );
        if e.enabled {
            b = b.on_press(Message::PaletteRun(e.command));
        }
        list = list.push(b);
    }

    container(column![
        field,
        container(Space::new().height(1))
            .width(Length::Fill)
            .style(theme::rule(dark)),
        scrollable(list)
            .id(list_id())
            .style(theme::Scrollbar { dark }.style())
            .height(Length::Fixed(380.0)),
    ])
    .width(540)
    .style(theme::palette_card(dark))
    .into()
}

#[cfg(test)]
mod tests {
    #[test]
    fn accents_do_not_matter_in_either_direction() {
        assert!(matches("Préférences", "pref"));
        assert!(matches("Préférences", "préf"));
        assert!(matches("Préférences", "PREF"));
        assert!(matches("Preferences", "préf"));
        assert!(matches("Ouvrir… un fichier récent", "recent"));
        assert!(matches("Écrire", "ecr"));
        assert!(!matches("Préférences", "prx"));
    }

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
        for entry in entries(&Config::default(), true, true) {
            let _ = entry.command.message();
        }
    }

    #[test]
    fn the_real_list_has_every_command_once() {
        let list = entries(&Config::default(), true, true);
        for (i, a) in list.iter().enumerate() {
            assert!(
                list[i + 1..].iter().all(|b| b.command != a.command),
                "{:?} is listed twice",
                a.command
            );
        }
        // Everything the menus offer is reachable from here.
        assert!(list.len() >= 18);
    }
}
