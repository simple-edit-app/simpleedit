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
                    .style(
                        (theme::MenuTitle {
                            dark,
                            open: open == Some(menu),
                        })
                        .style(),
                    ),
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
            .style(
                (theme::Segment {
                    dark,
                    active: dark == is_dark,
                    radius: theme::R_PILL,
                })
                .style(),
            )
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
    .style(move |_: &iced::Theme| container::Style {
        text_color: Some(p.text),
        background: Some(iced::Background::Color(p.elevated)),
        border: iced::Border {
            color: p.border,
            width: 1.0,
            radius: theme::R_PILL.into(),
        },
        shadow: iced::Shadow::default(),
        ..container::Style::default()
    });

    let bar = row![
        top_button(t!("menu.file").to_string(), TopMenu::File),
        top_button(t!("menu.edit").to_string(), TopMenu::Edit),
        top_button(t!("menu.view").to_string(), TopMenu::View),
        top_button(t!("menu.help").to_string(), TopMenu::Help),
        Space::new().width(Length::Fill),
        theme_switch,
    ]
    .spacing(2)
    .padding([0, 14])
    .height(42)
    .align_y(Alignment::Center);

    container(bar)
        .width(Length::Fill)
        .style(theme::bar(dark))
        .into()
}

/// What the bookmark entries need to know about the active file.
#[derive(Debug, Clone, Copy, Default)]
pub struct BookmarkState {
    /// The cursor's line is bookmarked.
    pub on_line: bool,
    /// The file has at least one bookmark.
    pub any: bool,
}

/// The bookmark entries shared by the Edit menu and the context menu: add or
/// remove the cursor line's bookmark, and remove them all.
pub fn bookmark_items(
    state: BookmarkState,
    dark: bool,
    toggle_hint: Option<String>,
) -> Vec<Element<'static, Message>> {
    let toggle_label = if state.on_line {
        t!("menu.bookmark_remove")
    } else {
        t!("menu.bookmark_add")
    };
    let clear: Element<'static, Message> = if state.any {
        item(
            t!("menu.bookmark_clear").to_string(),
            None,
            dark,
            Message::ClearBookmarks,
        )
    } else {
        disabled_item(t!("menu.bookmark_clear").to_string(), dark)
    };
    vec![
        item(
            toggle_label.to_string(),
            toggle_hint,
            dark,
            Message::ToggleBookmark,
        ),
        clear,
    ]
}

/// The floating dropdown panel for the active menu.
pub fn dropdown_view(
    menu: TopMenu,
    config: &Config,
    has_formatter: bool,
    lang_picker_enabled: bool,
    bookmarks: BookmarkState,
    recents_open: bool,
) -> Element<'static, Message> {
    let dark = config.dark_mode;
    let items = menu_items(menu, config, has_formatter, lang_picker_enabled, bookmarks);
    let card = container(column(items).spacing(1).padding(6).width(260)).style(theme::card(dark));
    if menu == TopMenu::File && recents_open && !config.recent_files.is_empty() {
        row![card, recents_submenu(config)].spacing(4).into()
    } else {
        card.into()
    }
}

/// The recent files, then "Clear", in a card beside the File menu.
fn recents_submenu(config: &Config) -> Element<'static, Message> {
    let dark = config.dark_mode;
    let recents: Vec<PathBuf> = config.recent_files.iter().map(PathBuf::from).collect();
    let mut items: Vec<Element<'static, Message>> = recents
        .iter()
        .zip(crate::app::unique_labels(&recents))
        .map(|(path, name)| item(name, None, dark, Message::OpenRecentFile(path.clone())))
        .collect();
    items.push(separator(dark));
    items.push(item(
        t!("menu.file_recents_clear").to_string(),
        None,
        dark,
        Message::ClearRecents,
    ));
    container(column(items).spacing(1).padding(6).width(300))
        .style(theme::card(dark))
        .into()
}

/// The stable `Id` of a top menu button's wrapping container, so its
/// dropdown can be anchored to wherever that button actually renders —
/// independent of label length, font, or DPI.
pub fn menu_id(menu: TopMenu) -> iced::widget::Id {
    match menu {
        TopMenu::File => iced::widget::Id::new("menu-file"),
        TopMenu::Edit => iced::widget::Id::new("menu-edit"),
        TopMenu::View => iced::widget::Id::new("menu-view"),
        TopMenu::Help => iced::widget::Id::new("menu-help"),
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
            text(label)
                .size(13)
                .shaping(iced::widget::text::Shaping::Advanced),
            Space::new().width(Length::Fill),
            text(sc).size(11.5).color(theme::muted_text(dark)),
        ]
        .align_y(Alignment::Center)
        .into()
    } else {
        text(label)
            .size(13)
            .shaping(iced::widget::text::Shaping::Advanced)
            .into()
    };

    button(content)
        .padding([7, 10])
        .width(Length::Fill)
        .on_press(message)
        .style(
            (theme::MenuItem {
                dark,
                radius: theme::R_SM,
                highlighted: false,
            })
            .style(),
        )
        .into()
}

pub fn disabled_item(label: String, dark: bool) -> Element<'static, Message> {
    container(text(label).size(13).color(theme::muted_text(dark)))
        .padding([7, 10])
        .width(Length::Fill)
        .into()
}

pub fn separator(dark: bool) -> Element<'static, Message> {
    container(
        container(Space::new().height(1))
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
    bookmarks: BookmarkState,
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

            items.push(separator(dark));
            let recents_at = items.len();
            items.push(if config.recent_files.is_empty() {
                disabled_item(t!("menu.file_recents").to_string(), dark)
            } else {
                mouse_area(item(
                    t!("menu.file_recents").to_string(),
                    Some("›".to_string()),
                    dark,
                    Message::SetRecents(true),
                ))
                .on_enter(Message::SetRecents(true))
                .into()
            });
            items.push(separator(dark));

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
            // Pointing at any other entry folds the submenu away.
            items
                .into_iter()
                .enumerate()
                .map(|(i, entry)| {
                    if i == recents_at {
                        entry
                    } else {
                        mouse_area(entry)
                            .on_enter(Message::SetRecents(false))
                            .into()
                    }
                })
                .collect()
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
            let mut items = vec![
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
                separator(dark),
            ];
            items.extend(bookmark_items(bookmarks, dark, sc(&sh.toggle_bookmark)));
            for (label, shortcut, message) in [
                (
                    t!("menu.bookmark_next"),
                    &sh.next_bookmark,
                    Message::NextBookmark,
                ),
                (
                    t!("menu.bookmark_prev"),
                    &sh.prev_bookmark,
                    Message::PrevBookmark,
                ),
            ] {
                items.push(if bookmarks.any {
                    item(label.to_string(), sc(shortcut), dark, message)
                } else {
                    disabled_item(label.to_string(), dark)
                });
            }
            items.push(separator(dark));
            items.push(item(
                t!("menu.fold_toggle").to_string(),
                sc(&sh.toggle_fold),
                dark,
                Message::ToggleFold,
            ));
            items.push(item(
                t!("menu.fold_all").to_string(),
                None,
                dark,
                Message::FoldAll,
            ));
            items.push(item(
                t!("menu.unfold_all").to_string(),
                None,
                dark,
                Message::UnfoldAll,
            ));
            items.extend([
                separator(dark),
                format_item,
                item(
                    t!("menu.edit_preferences").to_string(),
                    None,
                    dark,
                    Message::TogglePreferences,
                ),
            ]);
            items
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
                t!("menu.help_changelog").to_string(),
                None,
                dark,
                Message::OpenChangelog,
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
