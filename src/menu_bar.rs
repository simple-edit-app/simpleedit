use std::path::PathBuf;

use iced::{
    widget::{button, column, container, mouse_area, row, text, Space},
    Alignment, Element, Length,
};

use crate::app::{Message, TopMenu};
use crate::config::{Config, ShortcutConfig};
use crate::preferences::PreferencesMessage;
use crate::theme;

/// The title bar: the menus on the left and the Dark/Light switch on the
/// right. No layout side effects; the dropdown is drawn separately.
pub fn view(config: &Config, open: Option<TopMenu>) -> Element<'static, Message> {
    let dark = config.dark_mode;

    // Each button sits in its own `container` carrying a stable `Id`, so its
    // dropdown can be anchored to the button's actual rendered position
    // (queried via `container::visible_bounds`) instead of a hardcoded pixel
    // offset that only happened to fit one language's label widths.
    let top_button = |label: String, menu: TopMenu| {
        // Hovering a title while another menu is open switches to it, as in
        // any standard menu bar.
        mouse_area(
            container(
                button(text(label).size(13))
                    .padding([5, 9])
                    .on_press(Message::ToggleMenu(menu))
                    .style(iced::theme::Button::custom(theme::MenuTitle {
                        dark,
                        open: open == Some(menu),
                    })),
            )
            .id(menu_id(menu)),
        )
        .on_enter(Message::HoverMenu(menu))
    };

    // Sombre | Clair, a pill: the active side wears the accent wash.
    let side = |label: String, is_dark: bool| {
        button(text(label).size(12))
            .padding([3, 10])
            .on_press(Message::ThemeChanged(is_dark))
            .style(iced::theme::Button::custom(theme::Segment {
                dark,
                active: dark == is_dark,
                radius: theme::R_PILL,
            }))
    };
    let p = theme::palette(dark);
    let theme_switch = container(
        row![
            side(t!("toolbar.dark").to_string(), true),
            side(t!("toolbar.light").to_string(), false),
        ]
        .spacing(2),
    )
    .padding(2)
    .style(move |_: &iced::Theme| container::Appearance {
        text_color: Some(p.text),
        background: Some(iced::Background::Color(p.elevated)),
        border: iced::Border {
            color: p.border,
            width: 1.0,
            radius: theme::R_PILL.into(),
        },
        shadow: iced::Shadow::default(),
    });

    let bar = row![
        top_button(t!("menu.file").to_string(), TopMenu::File),
        top_button(t!("menu.edit").to_string(), TopMenu::Edit),
        top_button(t!("menu.view").to_string(), TopMenu::View),
        top_button(t!("menu.help").to_string(), TopMenu::Help),
        Space::with_width(Length::Fill),
        theme_switch,
    ]
    .spacing(2)
    .padding([0, 14])
    .height(42)
    .align_items(Alignment::Center);

    container(bar)
        .width(Length::Fill)
        .style(theme::bar(dark))
        .into()
}

/// The floating dropdown panel for the active menu.
pub fn dropdown_view(
    menu: TopMenu,
    config: &Config,
    has_formatter: bool,
    lang_picker_enabled: bool,
) -> Element<'static, Message> {
    let items = menu_items(menu, config, has_formatter, lang_picker_enabled);
    container(column(items).spacing(1).padding(6).width(260))
        .style(theme::card(config.dark_mode))
        .into()
}

/// The stable `Id` of a top menu button's wrapping container, so its
/// dropdown can be anchored to wherever that button actually renders —
/// independent of label length, font, or DPI.
pub fn menu_id(menu: TopMenu) -> container::Id {
    match menu {
        TopMenu::File => container::Id::new("menu-file"),
        TopMenu::Edit => container::Id::new("menu-edit"),
        TopMenu::View => container::Id::new("menu-view"),
        TopMenu::Help => container::Id::new("menu-help"),
    }
}

/// A clickable menu item with an optional shortcut hint on the right.
pub fn item(
    label: String,
    shortcut: Option<String>,
    dark: bool,
    message: Message,
) -> Element<'static, Message> {
    let content: Element<'static, Message> = if let Some(sc) = shortcut {
        row![
            text(label).size(13),
            Space::with_width(Length::Fill),
            text(sc).size(11.5).style(theme::muted_text(dark)),
        ]
        .align_items(Alignment::Center)
        .into()
    } else {
        text(label).size(13).into()
    };

    button(content)
        .padding([7, 10])
        .width(Length::Fill)
        .on_press(message)
        .style(iced::theme::Button::custom(theme::MenuItem {
            dark,
            radius: theme::R_SM,
            highlighted: false,
        }))
        .into()
}

pub fn disabled_item(label: String, dark: bool) -> Element<'static, Message> {
    container(text(label).size(13).style(theme::muted_text(dark)))
        .padding([7, 10])
        .width(Length::Fill)
        .into()
}

pub fn separator(dark: bool) -> Element<'static, Message> {
    container(
        container(Space::with_height(1))
            .width(Length::Fill)
            .style(theme::rule(dark)),
    )
    .padding([3, 4])
    .width(Length::Fill)
    .into()
}

fn sc(sc: &ShortcutConfig) -> Option<String> {
    Some(sc.display())
}

fn menu_items(
    menu: TopMenu,
    config: &Config,
    has_formatter: bool,
    lang_picker_enabled: bool,
) -> Vec<Element<'static, Message>> {
    let dark = config.dark_mode;
    let sh = &config.shortcuts;
    match menu {
        TopMenu::File => {
            let mut items = vec![
                item(
                    t!("menu.file_new").to_string(),
                    sc(&sh.new_file),
                    dark,
                    Message::NewFile,
                ),
                item(
                    t!("menu.file_open").to_string(),
                    sc(&sh.open_file),
                    dark,
                    Message::OpenFile,
                ),
            ];

            if !config.recent_files.is_empty() {
                items.push(separator(dark));
                items.push(disabled_item(t!("menu.file_recents").to_string(), dark));
                for path_str in &config.recent_files {
                    let path = PathBuf::from(path_str);
                    let name = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or(path_str)
                        .to_string();
                    items.push(item(name, None, dark, Message::OpenRecentFile(path)));
                }
                items.push(separator(dark));
            }

            items.push(item(
                t!("menu.file_save").to_string(),
                sc(&sh.save_file),
                dark,
                Message::SaveFile,
            ));
            items.push(item(
                t!("menu.file_save_as").to_string(),
                sc(&sh.save_as),
                dark,
                Message::SaveFileAs,
            ));
            items.push(item(
                t!("menu.file_close").to_string(),
                sc(&sh.close_file),
                dark,
                Message::CloseFile,
            ));
            items.push(item(
                t!("menu.file_quit").to_string(),
                sc(&sh.quit),
                dark,
                Message::Quit,
            ));
            items
        }
        TopMenu::Edit => {
            let format_item: Element<'static, Message> = if has_formatter {
                item(
                    t!("menu.edit_format").to_string(),
                    sc(&sh.format_code),
                    dark,
                    Message::FormatFile,
                )
            } else {
                disabled_item(t!("menu.edit_format").to_string(), dark)
            };
            vec![
                item(
                    t!("menu.edit_undo").to_string(),
                    sc(&sh.undo),
                    dark,
                    Message::Undo,
                ),
                item(
                    t!("menu.edit_redo").to_string(),
                    sc(&sh.redo),
                    dark,
                    Message::Redo,
                ),
                separator(dark),
                item(
                    t!("menu.edit_duplicate_line").to_string(),
                    sc(&sh.duplicate_line),
                    dark,
                    Message::DuplicateLine,
                ),
                item(
                    t!("menu.edit_move_line_up").to_string(),
                    sc(&sh.move_line_up),
                    dark,
                    Message::MoveLineUp,
                ),
                item(
                    t!("menu.edit_move_line_down").to_string(),
                    sc(&sh.move_line_down),
                    dark,
                    Message::MoveLineDown,
                ),
                item(
                    t!("menu.edit_toggle_comment").to_string(),
                    sc(&sh.toggle_comment),
                    dark,
                    Message::ToggleComment,
                ),
                item(
                    t!("menu.edit_delete_line").to_string(),
                    sc(&sh.delete_line),
                    dark,
                    Message::DeleteLine,
                ),
                separator(dark),
                item(
                    t!("menu.edit_select_all").to_string(),
                    sc(&sh.select_all),
                    dark,
                    Message::SelectAll,
                ),
                item(
                    t!("menu.edit_find").to_string(),
                    sc(&sh.find),
                    dark,
                    Message::ToggleSearch,
                ),
                item(
                    t!("menu.edit_goto_line").to_string(),
                    sc(&sh.goto_line),
                    dark,
                    Message::OpenGotoLine,
                ),
                format_item,
                item(
                    t!("menu.edit_preferences").to_string(),
                    None,
                    dark,
                    Message::TogglePreferences,
                ),
            ]
        }
        TopMenu::View => {
            let lang_item: Element<'static, Message> = if lang_picker_enabled {
                item(
                    t!("menu.view_language").to_string(),
                    None,
                    dark,
                    Message::OpenLanguagePicker,
                )
            } else {
                disabled_item(t!("menu.view_language").to_string(), dark)
            };
            vec![
                item(
                    t!("menu.view_palette").to_string(),
                    sc(&sh.command_palette),
                    dark,
                    Message::OpenPalette,
                ),
                item(
                    t!("menu.view_sidebar").to_string(),
                    sc(&sh.toggle_sidebar),
                    dark,
                    Message::ToggleSidebar,
                ),
                item(
                    if config.dark_mode {
                        t!("toolbar.light").to_string()
                    } else {
                        t!("toolbar.dark").to_string()
                    },
                    None,
                    dark,
                    Message::ThemeChanged(!config.dark_mode),
                ),
                item(
                    if config.show_line_numbers {
                        t!("menu.view_line_numbers_hide").to_string()
                    } else {
                        t!("menu.view_line_numbers_show").to_string()
                    },
                    None,
                    dark,
                    Message::Preferences(PreferencesMessage::ShowLineNumbersToggled(
                        !config.show_line_numbers,
                    )),
                ),
                lang_item,
            ]
        }
        TopMenu::Help => vec![
            item(
                t!("menu.help_about").to_string(),
                None,
                dark,
                Message::About,
            ),
            item(
                t!("menu.help_shortcuts").to_string(),
                None,
                dark,
                Message::OpenShortcuts,
            ),
            item(
                t!("menu.help_check_updates").to_string(),
                None,
                dark,
                Message::CheckForUpdate,
            ),
        ],
    }
}
