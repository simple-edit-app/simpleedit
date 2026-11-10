//! What the Database panel draws: the list in the sidebar, the settings of a
//! connection, and a table in its tab.

use std::fmt;
use std::path::Path;

use iced::widget::{
    button, checkbox, column, container, pick_list, row, scrollable, text, text_editor, text_input,
    tooltip, Space,
};
use iced::{Alignment, Element, Length};

use super::console::ConsoleTab;
use super::ddl;
use super::grid::{Cell, CopyFormat, PickKind, Picker, TimePart};
use super::protocol;
use super::{
    Auth, Choice, Connection, DbMsg, Dialog, DialogTab, Driver, Field, Flag, Kind, Remember,
    SshAuth, SslMode, State, TableTab, TestState, Tree, ROW_HEIGHT,
};
use crate::app::Message;
use crate::menu_bar;
use crate::theme as se_theme;

const TREE_ROW: f32 = 26.0;
/// Height of a day of the date picker.
const DAY: f32 = 28.0;
/// Width of the labels of the settings.
const LABEL: f32 = 170.0;

fn ghost(dark: bool, active: bool) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    (se_theme::GhostButton { dark, active }).style()
}

/// A small icon button with a tooltip.
fn icon_button<'a>(
    glyph: &'static str,
    tip: String,
    message: Message,
    dark: bool,
) -> Element<'a, Message> {
    let muted = se_theme::muted_text(dark);
    picture_button(text(glyph).size(13).color(muted).into(), tip, message, dark)
}

/// A button with a drawn picture and a tooltip.
fn picture_button<'a>(
    picture: Element<'a, Message>,
    tip: String,
    message: Message,
    dark: bool,
) -> Element<'a, Message> {
    tooltip(
        button(picture)
            .padding([0, 6])
            .on_press(message)
            .style(ghost(dark, false)),
        container(text(tip).size(12))
            .padding([4, 8])
            .style(se_theme::tooltip(dark)),
        tooltip::Position::Bottom,
    )
    .into()
}

// ----------------------------------------------------------------- sidebar

/// The panel: the connections as a tree of databases, schemas and tables.
pub fn sidebar(state: &State, dark: bool) -> Element<'_, Message> {
    let muted = se_theme::muted_text(dark);
    let header = row![
        text(t!("sidebar.database").to_uppercase())
            .size(10.5)
            .color(muted)
            .font(se_theme::UI_MEDIUM),
        Space::new().width(Length::Fill),
        picture_button(
            crate::sidebar::icons::console(muted),
            t!("db.console").to_string(),
            Message::Db(DbMsg::ToggleConsole),
            dark
        ),
        icon_button(
            "+",
            t!("db.add").to_string(),
            Message::Db(DbMsg::OpenDialog(None)),
            dark
        ),
    ]
    .padding([0, 8])
    .align_y(Alignment::Center);

    let body: Element<Message> = if state.connections.is_empty() {
        column![
            text(t!("db.empty").to_string()).size(12.5).color(muted),
            button(text(t!("db.add").to_string()).size(12.5))
                .padding([6, 12])
                .width(Length::Fill)
                .on_press(Message::Db(DbMsg::OpenDialog(None)))
                .style((se_theme::Outline { dark }).style()),
        ]
        .spacing(10)
        .padding([0, 8])
        .into()
    } else {
        let mut list = column![].spacing(1);
        for connection in &state.connections {
            list = list.push(connection_rows(state, connection, dark));
        }
        let filter = text_input(&t!("db.tree_filter"), &state.tree_filter)
            .on_input(|t| Message::Db(DbMsg::TreeFilter(t)))
            .size(12.5)
            .padding([4, 8])
            .style(se_theme::Field::new(dark).style());
        column![
            filter,
            scrollable(list)
                .style(se_theme::Scrollbar { dark }.style())
                .height(Length::Fill)
        ]
        .spacing(8)
        .height(Length::Fill)
        .into()
    };

    container(column![header, body].spacing(10).height(Length::Fill))
        .padding([14, 10])
        .height(Length::Fill)
        .style(se_theme::panel(dark))
        .into()
}

/// A line of the tree: indented, with an arrow if it opens.
fn node<'a>(
    depth: u16,
    arrow: &'static str,
    label: String,
    message: Message,
    dark: bool,
) -> button::Button<'a, Message> {
    let p = se_theme::palette(dark);
    button(
        container(
            row![
                text(arrow).size(10).color(p.muted).width(12),
                text(label)
                    .size(12.5)
                    .wrapping(iced::widget::text::Wrapping::None),
            ]
            .spacing(4)
            .align_y(Alignment::Center),
        )
        .center_y(Length::Fixed(TREE_ROW)),
    )
    .padding([0, 4 + depth * 14])
    .width(Length::Fill)
    .on_press(message)
    .style(ghost(dark, false))
}

fn connection_rows<'a>(
    state: &'a State,
    connection: &'a Connection,
    dark: bool,
) -> Element<'a, Message> {
    let p = se_theme::palette(dark);
    let id = connection.id;
    let key = id.to_string();
    let open = state.open.contains(&key)
        || (!state.tree_filter.trim().is_empty()
            && matches!(state.trees.get(&id), Some(Tree::Loaded(_))));
    let head = row![
        node(
            0,
            if open { "▾" } else { "▸" },
            connection.label(),
            Message::Db(DbMsg::Toggle(key.clone())),
            dark
        ),
        icon_button(
            "⟳",
            t!("db.refresh").to_string(),
            Message::Db(DbMsg::Reload(id)),
            dark
        ),
        icon_button(
            "✎",
            t!("db.edit").to_string(),
            Message::Db(DbMsg::OpenDialog(Some(id))),
            dark
        ),
    ]
    .align_y(Alignment::Center);
    let mut out = column![head].spacing(1);
    if !open {
        return out.into();
    }
    let note = |label: String, color: iced::Color| {
        container(text(label).size(12).color(color)).padding([2, 30])
    };
    match state.trees.get(&id) {
        None | Some(Tree::Loading) => {
            out = out.push(note(t!("db.loading").to_string(), p.muted));
        }
        Some(Tree::Failed(error)) => {
            if error.as_str() == t!("db.err_driver_missing") {
                // A click opens the settings of the source, where the driver
                // is downloaded.
                out = out.push(
                    button(note(error.clone(), se_theme::error_color(dark)))
                        .padding(0)
                        .on_press(Message::Db(DbMsg::OpenDialog(Some(id))))
                        .style(button::text),
                );
            } else {
                out = out.push(note(error.clone(), se_theme::error_color(dark)));
            }
        }
        Some(Tree::Loaded(databases)) if databases.iter().all(|d| d.schemas.is_empty()) => {
            out = out.push(note(t!("db.no_tables").to_string(), p.muted));
        }
        Some(Tree::Loaded(databases)) => {
            // The databases only show when there are several.
            let several = databases.len() > 1;
            let needle = state.tree_filter.trim().to_lowercase();
            let dialect = state.connection(id).map(|c| c.dialect).unwrap_or_default();
            let filtering = !needle.is_empty();
            let hit = |name: &str| !filtering || name.to_lowercase().contains(&needle);
            let is_open = |key: &str| filtering || state.open.contains(key);
            for database in databases {
                let database_key = format!("{id}/{}", database.name);
                if several {
                    let database_open = is_open(&database_key);
                    let line = node(
                        1,
                        if database_open { "▾" } else { "▸" },
                        database.name.clone(),
                        Message::Db(DbMsg::Toggle(database_key.clone())),
                        dark,
                    );
                    let (conn, name) = (id, database.name.clone());
                    out = out.push(crate::pointer_area::context_menu(line, move || {
                        database_menu(conn, name.clone(), dark)
                    }));
                    if !database_open {
                        continue;
                    }
                }
                let depth = if several { 2 } else { 1 };
                for schema in &database.schemas {
                    let tables: Vec<_> = schema.tables.iter().filter(|t| hit(&t.name)).collect();
                    let routines: Vec<_> =
                        schema.routines.iter().filter(|r| hit(&r.name)).collect();
                    let sequences: Vec<_> = schema.sequences.iter().filter(|n| hit(n)).collect();
                    let types: Vec<_> = schema.types.iter().filter(|n| hit(n)).collect();
                    if filtering
                        && tables.is_empty()
                        && routines.is_empty()
                        && sequences.is_empty()
                        && types.is_empty()
                    {
                        continue;
                    }
                    let schema_key = format!("{database_key}/{}", schema.name);
                    let schema_open = is_open(&schema_key);
                    let line = node(
                        depth,
                        if schema_open { "▾" } else { "▸" },
                        schema.name.clone(),
                        Message::Db(DbMsg::Toggle(schema_key.clone())),
                        dark,
                    );
                    let (conn, db_name, schema_name) =
                        (id, database.name.clone(), schema.name.clone());
                    out = out.push(crate::pointer_area::context_menu(line, move || {
                        schema_menu(conn, db_name.clone(), schema_name.clone(), dialect, dark)
                    }));
                    if !schema_open {
                        continue;
                    }
                    for table in tables {
                        let glyph = match table.kind {
                            Kind::Table => "▦",
                            Kind::View => "◫",
                            Kind::Matview => "◨",
                            Kind::Foreign => "▤",
                        };
                        let table_key = (
                            id,
                            database.name.clone(),
                            schema.name.clone(),
                            table.name.clone(),
                        );
                        let expand_key =
                            format!("{id}/{}/{}/{}", database.name, schema.name, table.name);
                        let expanded = state.open.contains(&expand_key);
                        let open = node(
                            0,
                            glyph,
                            table.name.clone(),
                            Message::Db(DbMsg::OpenTable(
                                id,
                                database.name.clone(),
                                schema.name.clone(),
                                table.name.clone(),
                            )),
                            dark,
                        );
                        // A right click: open it, change it, see its DDL, empty it
                        // or drop it.
                        let target = table_key.clone();
                        let kind = table.kind;
                        let menu = crate::pointer_area::context_menu(open, move || {
                            table_menu(target.clone(), kind, dialect, dark)
                        });
                        let arrow = button(
                            text(if expanded { "▾" } else { "▸" })
                                .size(10)
                                .color(p.muted),
                        )
                        .padding([0, 4])
                        .height(Length::Fixed(TREE_ROW))
                        .on_press(Message::Db(DbMsg::ToggleTable(
                            table_key.0,
                            table_key.1.clone(),
                            table_key.2.clone(),
                            table_key.3.clone(),
                        )))
                        .style(ghost(dark, false));
                        out = out.push(
                            row![
                                Space::new().width(Length::Fixed((depth + 1) as f32 * 14.0)),
                                arrow,
                                menu
                            ]
                            .align_y(Alignment::Center),
                        );
                        if expanded {
                            match state.structures.get(&table_key) {
                                Some(structure) => {
                                    for (glyph, label) in structure_lines(structure) {
                                        out = out.push(info_line(depth + 3, glyph, label, dark));
                                    }
                                }
                                None => {
                                    out = out.push(info_line(
                                        depth + 3,
                                        "",
                                        t!("db.loading").to_string(),
                                        dark,
                                    ));
                                }
                            }
                        }
                    }
                    // Functions, sequences and types, each in a folder.
                    let mut folder =
                        |name: &str,
                         title: String,
                         glyph: &'static str,
                         items: Vec<(String, Option<Message>)>| {
                            if items.is_empty() {
                                return;
                            }
                            let key = format!("{schema_key}/#{name}");
                            let open = is_open(&key);
                            out = std::mem::replace(&mut out, column![]).push(node(
                                depth + 1,
                                if open { "▾" } else { "▸" },
                                format!("{title} ({})", items.len()),
                                Message::Db(DbMsg::Toggle(key)),
                                dark,
                            ));
                            if open {
                                for (item, message) in items {
                                    let line = match message {
                                        // A click shows the SQL of the object.
                                        Some(message) => {
                                            node(depth + 2, glyph, item, message, dark).into()
                                        }
                                        None => info_line(depth + 2, glyph, item, dark),
                                    };
                                    out = std::mem::replace(&mut out, column![]).push(line);
                                }
                            }
                        };
                    folder(
                        "routines",
                        t!("db.tree_routines").to_string(),
                        "ƒ",
                        routines
                            .iter()
                            .map(|r| {
                                (
                                    format!("{}({})", r.name, r.args),
                                    Some(Message::Db(DbMsg::ShowFunction(
                                        id,
                                        database.name.clone(),
                                        schema.name.clone(),
                                        r.name.clone(),
                                        r.args.clone(),
                                    ))),
                                )
                            })
                            .collect(),
                    );
                    folder(
                        "sequences",
                        t!("db.tree_sequences").to_string(),
                        "#",
                        sequences.iter().map(|n| (n.to_string(), None)).collect(),
                    );
                    folder(
                        "types",
                        t!("db.tree_types").to_string(),
                        "τ",
                        types.iter().map(|n| (n.to_string(), None)).collect(),
                    );
                }
            }
        }
    }
    out.into()
}

/// A line of the tree that only informs: a column, an index, a function.
fn info_line<'a>(
    depth: u16,
    glyph: &'static str,
    label: String,
    dark: bool,
) -> Element<'a, Message> {
    let p = se_theme::palette(dark);
    container(
        row![
            text(glyph).size(10.5).color(p.accent).width(14),
            text(label)
                .size(12)
                .color(p.muted)
                .wrapping(iced::widget::text::Wrapping::None),
        ]
        .spacing(4)
        .align_y(Alignment::Center),
    )
    .padding([0, 4 + depth * 14])
    .height(Length::Fixed(TREE_ROW))
    .center_y(Length::Fixed(TREE_ROW))
    .into()
}

/// What a table is made of, as lines of the tree: its columns (a key is
/// marked), its indexes, its other constraints.
fn structure_lines(s: &protocol::Structure) -> Vec<(&'static str, String)> {
    use super::designer::{list, parse_constraint};
    use protocol::ConstraintKind;
    let columns_of = |kind: ConstraintKind| -> Vec<String> {
        s.constraints
            .iter()
            .filter(|c| c.kind == kind)
            .filter_map(|c| parse_constraint(kind, &c.definition))
            .flat_map(|p| list(&p.columns))
            .collect()
    };
    let (keys, foreign) = (
        columns_of(ConstraintKind::Primary),
        columns_of(ConstraintKind::Foreign),
    );
    let mut out = Vec::new();
    for c in &s.columns {
        let glyph = if keys.contains(&c.name) {
            "⚷"
        } else if foreign.contains(&c.name) {
            "→"
        } else {
            "·"
        };
        out.push((
            glyph,
            format!(
                "{}  {}{}",
                c.name,
                c.kind,
                if c.nullable { "" } else { "  NOT NULL" }
            ),
        ));
    }
    for i in &s.indexes {
        out.push((if i.unique { "◆" } else { "◇" }, i.name.clone()));
    }
    for c in s
        .constraints
        .iter()
        .filter(|c| c.kind != ConstraintKind::Primary)
    {
        let glyph = match c.kind {
            ConstraintKind::Unique => "◆",
            ConstraintKind::Foreign => "→",
            ConstraintKind::Check => "✓",
            _ => "·",
        };
        out.push((glyph, c.name.clone()));
    }
    out
}

/// What a right click on a schema offers.
fn schema_menu(
    conn: u64,
    database: String,
    schema: String,
    dialect: protocol::Dialect,
    dark: bool,
) -> Element<'static, Message> {
    // Making tables and schemas, the health, the backups: PostgreSQL's.
    if dialect != protocol::Dialect::Postgres {
        return container(
            column![menu_bar::item(
                t!("db.diagram").to_string(),
                None,
                dark,
                Message::Db(DbMsg::ShowDiagram(conn, database, schema)),
            )]
            .spacing(1)
            .padding(6)
            .width(230),
        )
        .style(se_theme::card(dark))
        .into();
    }
    let items: Vec<Element<'static, Message>> = vec![
        menu_bar::item(
            t!("db.new_table").to_string(),
            None,
            dark,
            Message::Db(DbMsg::NewTable(conn, database.clone(), schema.clone())),
        ),
        menu_bar::item(
            t!("db.new_schema").to_string(),
            None,
            dark,
            Message::Db(DbMsg::NewSchema(conn, database.clone())),
        ),
        menu_bar::item(
            t!("db.diagram").to_string(),
            None,
            dark,
            Message::Db(DbMsg::ShowDiagram(conn, database.clone(), schema.clone())),
        ),
        menu_bar::item(
            t!("db.health").to_string(),
            None,
            dark,
            Message::Db(DbMsg::OpenHealth(conn, database.clone())),
        ),
        menu_bar::item(
            t!("db.backup").to_string(),
            None,
            dark,
            Message::Db(DbMsg::BackupOpen(
                conn,
                database.clone(),
                schema.clone(),
                String::new(),
                false,
            )),
        ),
        menu_bar::item(
            t!("db.restore").to_string(),
            None,
            dark,
            Message::Db(DbMsg::BackupOpen(
                conn,
                database,
                String::new(),
                String::new(),
                true,
            )),
        ),
    ];
    container(column(items).spacing(1).padding(6).width(230))
        .style(se_theme::card(dark))
        .into()
}

/// What a right click on a database offers.
fn database_menu(conn: u64, database: String, dark: bool) -> Element<'static, Message> {
    container(
        column![
            menu_bar::item(
                t!("db.new_schema").to_string(),
                None,
                dark,
                Message::Db(DbMsg::NewSchema(conn, database.clone())),
            ),
            menu_bar::item(
                t!("db.health").to_string(),
                None,
                dark,
                Message::Db(DbMsg::OpenHealth(conn, database.clone())),
            ),
            menu_bar::item(
                t!("db.backup").to_string(),
                None,
                dark,
                Message::Db(DbMsg::BackupOpen(
                    conn,
                    database.clone(),
                    String::new(),
                    String::new(),
                    false,
                )),
            ),
            menu_bar::item(
                t!("db.restore").to_string(),
                None,
                dark,
                Message::Db(DbMsg::BackupOpen(
                    conn,
                    database,
                    String::new(),
                    String::new(),
                    true,
                )),
            )
        ]
        .padding(6)
        .width(230),
    )
    .style(se_theme::card(dark))
    .into()
}

// ------------------------------------------------------------------ dialog

/// A choice of a list, shown with its label.
#[derive(Clone, PartialEq)]
struct Opt<T: Copy + PartialEq>(T, String);

impl<T: Copy + PartialEq> fmt::Display for Opt<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.1)
    }
}

/// A list of choices, the current one selected.
fn pick<'a, T: Copy + PartialEq + 'a>(
    options: Vec<(T, String)>,
    current: T,
    to_message: fn(T) -> Choice,
    dark: bool,
) -> Element<'a, Message> {
    let options: Vec<Opt<T>> = options.into_iter().map(|(v, l)| Opt(v, l)).collect();
    let selected = options.iter().find(|o| o.0 == current).cloned();
    pick_list(options, selected, move |o: Opt<T>| {
        Message::Db(DbMsg::Choose(to_message(o.0)))
    })
    .text_size(12.5)
    .padding([5, 8])
    .width(Length::Fill)
    .style(se_theme::Select { dark }.style())
    .menu_style(se_theme::Select { dark }.menu_style())
    .into()
}

/// A line of settings: its name, then what holds it.
fn labeled<'a>(
    label: String,
    content: impl Into<Element<'a, Message>>,
    dark: bool,
) -> Element<'a, Message> {
    row![
        text(label)
            .size(12.5)
            .color(se_theme::muted_text(dark))
            .width(LABEL),
        content.into(),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

fn input<'a>(
    value: &str,
    field: Field,
    secure: bool,
    dark: bool,
) -> text_input::TextInput<'a, Message> {
    let input = text_input("", value)
        .on_input(move |t| Message::Db(DbMsg::Text(field, t)))
        .size(13)
        .padding([6, 10])
        .style(se_theme::Field::new(dark).style());
    if secure {
        input.secure(true)
    } else {
        input
    }
}

/// A path, with a button to pick it.
fn file_input<'a>(value: &str, field: Field, dark: bool) -> Element<'a, Message> {
    row![
        input(value, field, false, dark),
        button(text(t!("db.browse").to_string()).size(12.5))
            .padding([5, 12])
            .on_press(Message::Db(DbMsg::Browse(field)))
            .style((se_theme::Outline { dark }).style()),
    ]
    .spacing(6)
    .align_y(Alignment::Center)
    .into()
}

fn check<'a>(label: String, on: bool, flag: Flag) -> Element<'a, Message> {
    checkbox(on)
        .label(label)
        .size(14)
        .text_size(12.5)
        .on_toggle(move |on| Message::Db(DbMsg::Check(flag, on)))
        .into()
}

fn hint<'a>(label: String, dark: bool) -> Element<'a, Message> {
    text(label)
        .size(11.5)
        .color(se_theme::muted_text(dark))
        .into()
}

fn tab_label(tab: DialogTab) -> String {
    match tab {
        DialogTab::General => t!("db.tab_general").to_string(),
        DialogTab::Options => t!("db.tab_options").to_string(),
        DialogTab::SshSsl => t!("db.tab_ssh").to_string(),
        DialogTab::Schemas => t!("db.tab_schemas").to_string(),
        DialogTab::Advanced => t!("db.tab_advanced").to_string(),
        DialogTab::Driver => t!("db.tab_driver").to_string(),
    }
}

/// The settings of a connection, in tabs, as in the Data Sources window of a
/// JetBrains IDE.
pub fn dialog<'a>(state: &'a State, dialog: &'a Dialog, dark: bool) -> Element<'a, Message> {
    let p = se_theme::palette(dark);

    let title = row![
        text(
            if dialog.editing.is_some() {
                t!("db.dialog_edit")
            } else {
                t!("db.dialog_new")
            }
            .to_string()
        )
        .size(13.5)
        .font(se_theme::UI_SEMIBOLD)
        .color(p.text),
        Space::new().width(Length::Fill),
        button(text("×").size(16).color(p.muted))
            .padding([0, 7])
            .on_press(Message::Db(DbMsg::CloseDialog))
            .style(ghost(dark, false)),
    ]
    .align_y(Alignment::Center);

    let mut tabs = row![].spacing(2);
    for tab in DialogTab::ALL {
        tabs = tabs.push(
            button(text(tab_label(tab)).size(12.5))
                .padding([5, 12])
                .on_press(Message::Db(DbMsg::Tab(tab)))
                .style(ghost(dark, dialog.tab == tab)),
        );
    }

    let content: Element<Message> = match dialog.tab {
        DialogTab::General => general_tab(state, dialog, dark),
        DialogTab::Options => options_tab(dialog, dark),
        DialogTab::SshSsl => ssh_ssl_tab(dialog, dark),
        DialogTab::Schemas => schemas_tab(dialog, dark),
        DialogTab::Advanced => advanced_tab(dialog, dark),
        DialogTab::Driver => driver_tab(state, dialog, dark),
    };

    let driver_ready = *state.driver_of(dialog.draft.dialect) == Driver::Ready;
    let can_test = driver_ready && dialog.valid() && dialog.test != TestState::Running;
    let test_line: Element<Message> = match &dialog.test {
        TestState::Idle => Space::new().into(),
        TestState::Running => text(t!("db.testing").to_string())
            .size(12)
            .color(p.muted)
            .into(),
        TestState::Ok(version) => text(t!("db.test_ok", version = version.as_str()).to_string())
            .size(12)
            .color(p.accent)
            .into(),
        TestState::Failed(error) => text(error.as_str())
            .size(12)
            .color(se_theme::error_color(dark))
            .into(),
    };

    let mut buttons = row![
        button(text(t!("db.test").to_string()).size(12.5))
            .padding([5, 14])
            .on_press_maybe(can_test.then_some(Message::Db(DbMsg::Test)))
            .style((se_theme::Outline { dark }).style()),
        button(text(t!("db.save").to_string()).size(12.5))
            .padding([5, 14])
            .on_press_maybe(dialog.valid().then_some(Message::Db(DbMsg::Save)))
            .style((se_theme::Primary { dark }).style()),
        test_line,
        Space::new().width(Length::Fill),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    if let Some(id) = dialog.editing {
        buttons = buttons
            .push(
                button(text(t!("db.duplicate").to_string()).size(12.5))
                    .padding([5, 14])
                    .on_press(Message::Db(DbMsg::Duplicate(id)))
                    .style((se_theme::Outline { dark }).style()),
            )
            .push(
                button(
                    text(if dialog.confirm_delete {
                        t!("db.delete_confirm").to_string()
                    } else {
                        t!("db.delete").to_string()
                    })
                    .size(12.5)
                    .color(se_theme::error_color(dark)),
                )
                .padding([5, 14])
                .on_press(Message::Db(DbMsg::Delete))
                .style((se_theme::Outline { dark }).style()),
            );
    }

    let rule = || {
        container(Space::new().height(1))
            .width(Length::Fill)
            .style(se_theme::rule(dark))
    };
    container(
        column![
            title,
            tabs,
            rule(),
            scrollable(container(content).padding(iced::Padding {
                right: 12.0,
                ..iced::Padding::ZERO
            }))
            .style(se_theme::Scrollbar { dark }.style())
            .height(Length::Fixed(430.0)),
            rule(),
            buttons,
        ]
        .spacing(10)
        .width(Length::Fixed(780.0)),
    )
    .padding(16)
    .style(se_theme::card(dark))
    .into()
}

fn general_tab<'a>(state: &'a State, dialog: &'a Dialog, dark: bool) -> Element<'a, Message> {
    let p = se_theme::palette(dark);
    let d = &dialog.draft;
    let mut form = column![].spacing(10);
    if dialog.needs_password {
        form = form.push(
            text(t!("db.password_needed").to_string())
                .size(12)
                .color(p.accent),
        );
    }
    form = form
        .push(labeled(
            t!("db.field_name").to_string(),
            input(&d.name, Field::Name, false, dark),
            dark,
        ))
        .push(labeled(
            t!("db.field_comment").to_string(),
            input(&d.comment, Field::Comment, false, dark),
            dark,
        ))
        .push(labeled(
            t!("db.dialect").to_string(),
            pick(
                protocol::Dialect::ALL
                    .iter()
                    .map(|k| (*k, k.label().to_string()))
                    .collect(),
                d.dialect,
                Choice::Dialect,
                dark,
            ),
            dark,
        ));
    // A SQLite database is a file: nothing else is asked.
    if d.dialect == protocol::Dialect::Sqlite {
        return form
            .push(labeled(
                t!("db.field_file").to_string(),
                file_input(&d.database, Field::Database, dark),
                dark,
            ))
            .push(hint(t!("db.file_hint").to_string(), dark))
            .push(driver_line(state, d.dialect, dark))
            .into();
    }
    if d.dialect == protocol::Dialect::Postgres {
        form = form.push(labeled(
            t!("db.conn_type").to_string(),
            pick(
                vec![
                    (false, t!("db.conn_default").to_string()),
                    (true, t!("db.conn_url").to_string()),
                ],
                d.url_only,
                Choice::UrlOnly,
                dark,
            ),
            dark,
        ));
    }
    if d.url_only {
        form = form.push(labeled(
            t!("db.field_url").to_string(),
            input(&d.url, Field::Url, false, dark),
            dark,
        ));
    } else {
        form = form
            .push(labeled(
                t!("db.field_host").to_string(),
                input(&d.host, Field::Host, false, dark),
                dark,
            ))
            .push(labeled(
                t!("db.field_port").to_string(),
                input(&dialog.port, Field::Port, false, dark),
                dark,
            ))
            .push(labeled(
                t!("db.field_database").to_string(),
                input(&d.database, Field::Database, false, dark),
                dark,
            ));
    }
    form = form.push(labeled(
        t!("db.auth").to_string(),
        pick(
            Auth::ALL
                .iter()
                .filter(|a| d.dialect == protocol::Dialect::Postgres || **a != Auth::Pgpass)
                .map(|a| {
                    (
                        *a,
                        match a {
                            Auth::UserPassword => t!("db.auth_password"),
                            Auth::Pgpass => t!("db.auth_pgpass"),
                            Auth::NoAuth => t!("db.auth_none"),
                        }
                        .to_string(),
                    )
                })
                .collect(),
            d.auth,
            Choice::Auth,
            dark,
        ),
        dark,
    ));
    form = form.push(labeled(
        t!("db.field_user").to_string(),
        input(&d.user, Field::User, false, dark),
        dark,
    ));
    if d.auth == Auth::UserPassword {
        form = form
            .push(labeled(
                t!("db.field_password").to_string(),
                input(&dialog.password, Field::Password, true, dark),
                dark,
            ))
            .push(labeled(
                t!("db.remember").to_string(),
                pick(
                    Remember::ALL
                        .iter()
                        .map(|r| {
                            (
                                *r,
                                match r {
                                    Remember::Never => t!("db.remember_never"),
                                    Remember::UntilRestart => t!("db.remember_session"),
                                    Remember::Forever => t!("db.remember_forever"),
                                }
                                .to_string(),
                            )
                        })
                        .collect(),
                    d.remember,
                    Choice::Remember,
                    dark,
                ),
                dark,
            ));
        if d.remember == Remember::Forever {
            form = form.push(hint(t!("db.remember_forever_hint").to_string(), dark));
        }
    }
    // What the settings make, as a JetBrains IDE shows it under the fields.
    if !d.url_only {
        form = form.push(labeled(
            t!("db.field_url").to_string(),
            text(d.jdbc_url())
                .size(12.5)
                .font(se_theme::MONO)
                .color(p.muted),
            dark,
        ));
    }
    form.push(driver_line(state, d.dialect, dark)).into()
}

/// The state of the driver and what to do about it.
fn driver_line<'a>(
    state: &'a State,
    dialect: protocol::Dialect,
    dark: bool,
) -> Element<'a, Message> {
    let p = se_theme::palette(dark);
    let (label, color, action): (String, iced::Color, Option<String>) =
        match state.driver_of(dialect) {
            Driver::Ready => (t!("db.driver_ready").to_string(), p.muted, None),
            Driver::Downloading => (t!("db.driver_downloading").to_string(), p.muted, None),
            Driver::Failed(error) => (
                t!("db.driver_failed", error = error.as_str()).to_string(),
                se_theme::error_color(dark),
                Some(t!("db.driver_download").to_string()),
            ),
            Driver::Missing | Driver::Unknown => (
                t!("db.driver_missing").to_string(),
                se_theme::error_color(dark),
                Some(t!("db.driver_download").to_string()),
            ),
        };
    let mut line = row![text(label).size(12).color(color)]
        .spacing(10)
        .align_y(Alignment::Center);
    if let Some(action) = action {
        line = line.push(
            button(text(action).size(12.5))
                .padding([4, 12])
                .on_press(Message::Db(DbMsg::DownloadDriver(dialect)))
                .style((se_theme::Outline { dark }).style()),
        );
    }
    line.into()
}

fn options_tab<'a>(dialog: &'a Dialog, dark: bool) -> Element<'a, Message> {
    let d = &dialog.draft;
    column![
        check(
            t!("db.opt_read_only").to_string(),
            d.read_only,
            Flag::ReadOnly
        ),
        hint(t!("db.opt_read_only_hint").to_string(), dark),
        check(
            t!("db.opt_production").to_string(),
            d.production,
            Flag::Production
        ),
        hint(t!("db.opt_production_hint").to_string(), dark),
        check(
            t!("db.opt_single").to_string(),
            d.scope.single_database,
            Flag::SingleDatabase
        ),
        hint(t!("db.opt_single_hint").to_string(), dark),
        labeled(
            t!("db.opt_time_zone").to_string(),
            input(&d.time_zone, Field::TimeZone, false, dark),
            dark
        ),
        hint(t!("db.opt_time_zone_hint").to_string(), dark),
        labeled(
            t!("db.opt_timeout").to_string(),
            input(
                &if d.query_timeout == 0 {
                    String::new()
                } else {
                    d.query_timeout.to_string()
                },
                Field::QueryTimeout,
                false,
                dark
            ),
            dark
        ),
        hint(t!("db.opt_timeout_hint").to_string(), dark),
        text(t!("db.opt_script").to_string())
            .size(12.5)
            .color(se_theme::muted_text(dark)),
        text_editor(&dialog.script)
            .on_action(|a| Message::Db(DbMsg::Script(a)))
            .size(13)
            .font(se_theme::MONO)
            .height(Length::Fixed(140.0)),
        hint(t!("db.opt_script_hint").to_string(), dark),
    ]
    .spacing(10)
    .into()
}

fn ssh_ssl_tab<'a>(dialog: &'a Dialog, dark: bool) -> Element<'a, Message> {
    let p = se_theme::palette(dark);
    let d = &dialog.draft;
    let title = |label: String| {
        text(label)
            .size(12.5)
            .font(se_theme::UI_SEMIBOLD)
            .color(p.text)
    };
    let mut form = column![
        title(t!("db.ssh").to_string()),
        check(
            t!("db.ssh_use").to_string(),
            d.ssh.enabled,
            Flag::SshEnabled
        ),
    ]
    .spacing(10);
    if d.ssh.enabled {
        form = form
            .push(labeled(
                t!("db.ssh_host").to_string(),
                input(&d.ssh.host, Field::SshHost, false, dark),
                dark,
            ))
            .push(labeled(
                t!("db.ssh_port").to_string(),
                input(&dialog.ssh_port, Field::SshPort, false, dark),
                dark,
            ))
            .push(labeled(
                t!("db.ssh_user").to_string(),
                input(&d.ssh.user, Field::SshUser, false, dark),
                dark,
            ))
            .push(labeled(
                t!("db.ssh_auth").to_string(),
                pick(
                    SshAuth::ALL
                        .iter()
                        .map(|a| {
                            (
                                *a,
                                match a {
                                    SshAuth::Password => t!("db.ssh_auth_password"),
                                    SshAuth::KeyPair => t!("db.ssh_auth_key"),
                                    SshAuth::Agent => t!("db.ssh_auth_agent"),
                                }
                                .to_string(),
                            )
                        })
                        .collect(),
                    d.ssh.auth,
                    Choice::SshAuth,
                    dark,
                ),
                dark,
            ));
        if d.ssh.auth == SshAuth::KeyPair {
            form = form.push(labeled(
                t!("db.ssh_key").to_string(),
                file_input(&d.ssh.key_file, Field::SshKey, dark),
                dark,
            ));
        }
        if d.ssh.auth != SshAuth::Agent {
            form = form.push(labeled(
                if d.ssh.auth == SshAuth::Password {
                    t!("db.ssh_secret_password")
                } else {
                    t!("db.ssh_secret_passphrase")
                }
                .to_string(),
                input(&d.ssh_secret, Field::SshSecret, true, dark),
                dark,
            ));
        }
        form = form
            .push(labeled(
                t!("db.ssh_local_port").to_string(),
                input(&d.ssh.local_port, Field::SshLocalPort, false, dark),
                dark,
            ))
            .push(hint(t!("db.ssh_hint").to_string(), dark));
    }
    form = form
        .push(Space::new().height(6))
        .push(title(t!("db.ssl").to_string()))
        .push(labeled(
            t!("db.ssl_mode").to_string(),
            pick(
                SslMode::ALL
                    .iter()
                    .map(|m| {
                        (
                            *m,
                            match m {
                                SslMode::Disable => t!("db.ssl_disable"),
                                SslMode::Prefer => t!("db.ssl_prefer"),
                                SslMode::Require => t!("db.ssl_require"),
                                SslMode::VerifyCa => t!("db.ssl_verify_ca"),
                                SslMode::VerifyFull => t!("db.ssl_verify_full"),
                            }
                            .to_string(),
                        )
                    })
                    .collect(),
                d.ssl.mode,
                Choice::SslMode,
                dark,
            ),
            dark,
        ));
    if d.ssl.mode != SslMode::Disable {
        form = form
            .push(labeled(
                t!("db.ssl_ca").to_string(),
                file_input(&d.ssl.ca_file, Field::SslCa, dark),
                dark,
            ))
            .push(labeled(
                t!("db.ssl_cert").to_string(),
                file_input(&d.ssl.client_cert, Field::SslCert, dark),
                dark,
            ))
            .push(labeled(
                t!("db.ssl_key").to_string(),
                file_input(&d.ssl.client_key, Field::SslKey, dark),
                dark,
            ))
            .push(hint(t!("db.ssl_hint").to_string(), dark));
    }
    form.into()
}

fn schemas_tab<'a>(dialog: &'a Dialog, dark: bool) -> Element<'a, Message> {
    let d = &dialog.draft;
    column![
        labeled(
            t!("db.schema_pattern").to_string(),
            input(&d.scope.pattern, Field::SchemaPattern, false, dark),
            dark
        ),
        hint(t!("db.schema_pattern_hint").to_string(), dark),
        labeled(
            t!("db.object_filter").to_string(),
            input(&d.scope.object_filter, Field::ObjectFilter, false, dark),
            dark
        ),
        hint(t!("db.object_filter_hint").to_string(), dark),
        check(
            t!("db.show_system").to_string(),
            d.scope.system_schemas,
            Flag::SystemSchemas
        ),
        check(
            t!("db.show_templates").to_string(),
            d.scope.template_databases,
            Flag::TemplateDatabases
        ),
    ]
    .spacing(10)
    .into()
}

fn advanced_tab<'a>(dialog: &'a Dialog, dark: bool) -> Element<'a, Message> {
    let p = se_theme::palette(dark);
    let mut form = column![row![
        text(t!("db.prop_key").to_string())
            .size(12)
            .color(p.muted)
            .width(Length::FillPortion(2)),
        text(t!("db.prop_value").to_string())
            .size(12)
            .color(p.muted)
            .width(Length::FillPortion(3)),
        Space::new().width(30),
    ]
    .spacing(8),]
    .spacing(8);
    for (i, (key, value)) in dialog.draft.props.iter().enumerate() {
        form = form.push(
            row![
                text_input("", key)
                    .on_input(move |t| Message::Db(DbMsg::PropKey(i, t)))
                    .size(13)
                    .padding([6, 10])
                    .style(se_theme::Field::new(dark).style())
                    .width(Length::FillPortion(2)),
                text_input("", value)
                    .on_input(move |t| Message::Db(DbMsg::PropValue(i, t)))
                    .size(13)
                    .padding([6, 10])
                    .style(se_theme::Field::new(dark).style())
                    .width(Length::FillPortion(3)),
                button(text("×").size(15).color(p.muted))
                    .padding([0, 8])
                    .on_press(Message::Db(DbMsg::PropRemove(i)))
                    .style(ghost(dark, false)),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        );
    }
    form.push(
        button(text(t!("db.prop_add").to_string()).size(12.5))
            .padding([5, 12])
            .on_press(Message::Db(DbMsg::PropAdd))
            .style((se_theme::Outline { dark }).style()),
    )
    .push(hint(t!("db.props_hint").to_string(), dark))
    .into()
}

fn driver_tab<'a>(state: &'a State, dialog: &'a Dialog, dark: bool) -> Element<'a, Message> {
    let p = se_theme::palette(dark);
    let dialect = dialog.draft.dialect;
    let mut form = column![
        driver_line(state, dialect, dark),
        labeled(
            t!("db.driver_protocol").to_string(),
            text(super::DRIVER_PROTOCOL.to_string()).size(12.5),
            dark
        ),
        labeled(
            t!("db.driver_location").to_string(),
            text(
                super::downloaded_driver_of(dialect)
                    .map(|path| path.display().to_string())
                    .unwrap_or_default()
            )
            .size(12)
            .font(se_theme::MONO)
            .color(p.muted),
            dark
        ),
    ]
    .spacing(10);
    if matches!(state.driver_of(dialect), Driver::Ready) {
        form = form.push(
            button(text(t!("db.driver_redownload").to_string()).size(12.5))
                .padding([5, 12])
                .on_press(Message::Db(DbMsg::DownloadDriver(dialect)))
                .style((se_theme::Outline { dark }).style()),
        );
    }
    form.push(Space::new().height(6))
        .push(labeled(
            t!("db.driver_file").to_string(),
            file_input(&dialog.driver_file, Field::DriverFile, dark),
            dark,
        ))
        .push(hint(t!("db.driver_file_hint").to_string(), dark))
        .push(
            button(text(t!("db.driver_use_downloaded").to_string()).size(12.5))
                .padding([5, 12])
                .on_press(Message::Db(DbMsg::UseDownloadedDriver))
                .style((se_theme::Outline { dark }).style()),
        )
        .into()
}

// ------------------------------------------------------------------- table

/// A table in its tab: the bars, then the grid.
pub fn table<'a>(path: &'a Path, tab: &'a TableTab, dark: bool) -> Element<'a, Message> {
    let p = se_theme::palette(dark);
    let shown = tab.page.as_ref();
    let range = match shown {
        Some(page) if !page.rows.is_empty() => {
            let (from, to) = (page.offset + 1, page.offset + page.rows.len() as u64);
            t!("db.rows_range", from = from, to = to).to_string()
        }
        Some(_) => t!("db.no_rows").to_string(),
        None => String::new(),
    };
    let owned = || path.to_path_buf();
    let has_prev = tab.offset() > 0;
    let has_next = shown.is_some_and(|page| page.has_more);
    let nav = |glyph: &'static str, tip: String, message: Option<Message>| {
        tooltip(
            button(text(glyph).size(13))
                .padding([2, 9])
                .on_press_maybe(message)
                .style((se_theme::Outline { dark }).style()),
            container(text(tip).size(12))
                .padding([4, 8])
                .style(se_theme::tooltip(dark)),
            tooltip::Position::Bottom,
        )
    };

    // How many rows a page holds, among the usual sizes.
    let mut sizes = super::PAGE_SIZES.to_vec();
    if !sizes.contains(&tab.page_size) {
        sizes.push(tab.page_size);
        sizes.sort_unstable();
    }
    let size_path = owned();
    let page_size = tooltip(
        pick_list(sizes, Some(tab.page_size), move |n| {
            Message::Db(DbMsg::PageSize(size_path.clone(), n))
        })
        .text_size(12.5)
        .padding([3, 8])
        .style(se_theme::Select { dark }.style())
        .menu_style(se_theme::Select { dark }.menu_style()),
        container(text(t!("db.page_size").to_string()).size(12))
            .padding([4, 8])
            .style(se_theme::tooltip(dark)),
        tooltip::Position::Bottom,
    );

    // A tab of a query shows what it gave, not a table: no sort, filter or
    // page of its own.
    let is_query = tab.is_static_result();
    let title = if tab.query.is_some() {
        tab.title.clone()
    } else {
        format!("{}.{}", tab.schema, tab.table)
    };
    let mut bar = row![
        text(title).size(13).font(se_theme::UI_SEMIBOLD),
        text(range).size(12).color(p.muted),
    ]
    .spacing(10)
    .padding([8, 12])
    .align_y(Alignment::Center);
    // The number of rows under the filter: counted when asked for.
    if shown.is_some() && !is_query {
        let current = tab.where_clause();
        match &tab.total {
            Some((filter, total)) if *filter == current => {
                bar = bar.push(
                    text(t!("db.rows_total", n = total).to_string())
                        .size(12)
                        .color(p.muted),
                );
            }
            _ => {}
        }
        bar = bar.push(nav(
            "\u{03A3}",
            t!("db.count_rows").to_string(),
            Some(Message::Db(DbMsg::Count(owned()))),
        ));
    }
    // A table with no key is edited, its rows found by their place.
    if tab.editable() && shown.is_some_and(|page| page.primary_key.is_empty()) && !is_query {
        bar = bar.push(
            text(t!("db.keyless_table").to_string())
                .size(12)
                .color(p.muted),
        );
    }
    if shown.is_some() && !tab.editable() && !is_query {
        bar = bar.push(
            text(t!("db.read_only_table").to_string())
                .size(12)
                .color(p.muted),
        );
    }
    bar = bar.push(Space::new().width(Length::Fill));
    let pending = tab.pending.count();
    if pending > 0 || tab.editing.is_some() {
        bar = bar
            .push(
                text(t!("db.pending", n = pending).to_string())
                    .size(12)
                    .color(p.accent),
            )
            .push(
                button(text(t!("db.revert").to_string()).size(12.5))
                    .padding([3, 12])
                    .on_press(Message::Db(DbMsg::RevertAll(owned())))
                    .style((se_theme::Outline { dark }).style()),
            )
            .push(
                button(text(t!("db.preview").to_string()).size(12.5))
                    .padding([3, 12])
                    .on_press(Message::Db(DbMsg::Preview(owned(), true)))
                    .style((se_theme::Outline { dark }).style()),
            )
            .push(
                button(text(t!("db.commit").to_string()).size(12.5))
                    .padding([3, 14])
                    .on_press_maybe((!tab.committing).then(|| Message::Db(DbMsg::Commit(owned()))))
                    .style((se_theme::Primary { dark }).style()),
            );
    }
    if tab.editable() {
        bar = bar.push(nav(
            "+",
            t!("db.row_add").to_string(),
            Some(Message::Db(DbMsg::AddRow(owned()))),
        ));
    }
    if is_query {
        if let Some(sql) = &tab.query {
            // The query, on one line, to know what the tab is.
            let line = sql.split_whitespace().collect::<Vec<_>>().join(" ");
            bar = bar.push(
                text(line)
                    .size(12)
                    .font(se_theme::MONO)
                    .color(p.muted)
                    .wrapping(iced::widget::text::Wrapping::None),
            );
        }
        if shown.is_some_and(|page| page.has_more) {
            bar = bar.push(
                text(t!("db.result_truncated", n = protocol::MAX_RESULT_ROWS).to_string())
                    .size(12)
                    .color(p.accent),
            );
        }
    } else {
        bar = bar.push(page_size).push(nav(
            "‹",
            t!("db.prev").to_string(),
            has_prev.then(|| Message::Db(DbMsg::Turn(owned(), -1))),
        ));
        bar = bar.push(nav(
            "›",
            t!("db.next").to_string(),
            has_next.then(|| Message::Db(DbMsg::Turn(owned(), 1))),
        ));
    }
    bar = bar.push(nav(
        "⟳",
        t!("db.reload").to_string(),
        (!tab.loading).then(|| Message::Db(DbMsg::Refresh(owned()))),
    ));
    // All the rows with the filter and the sort, not only this page (what a
    // query that cannot be paged gave is on show already).
    if tab.query.is_none() || tab.paged {
        let ready = !tab.loading && tab.error.is_none();
        bar = bar
            .push(nav(
                "CSV",
                t!("db.export_csv").to_string(),
                ready.then(|| Message::Db(DbMsg::Export(owned(), "csv"))),
            ))
            .push(nav(
                "JSON",
                t!("db.export_json").to_string(),
                ready.then(|| Message::Db(DbMsg::Export(owned(), "json"))),
            ))
            .push(nav(
                "SQL",
                t!("db.export_sql").to_string(),
                ready.then(|| Message::Db(DbMsg::Export(owned(), "sql"))),
            ))
            .push(nav(
                "XLSX",
                t!("db.export_xlsx").to_string(),
                ready.then(|| Message::Db(DbMsg::Export(owned(), "xlsx"))),
            ))
            .push(nav(
                "MD",
                t!("db.export_md").to_string(),
                ready.then(|| Message::Db(DbMsg::Export(owned(), "md"))),
            ));
    }

    // The WHERE field: a condition on the rows, sent with the query.
    let filter_path = owned();
    let mut filter_bar = row![
        text("WHERE")
            .size(11.5)
            .font(se_theme::MONO)
            .color(p.muted)
            .width(52),
        text_input(&t!("db.where_hint"), &tab.filter_text)
            .on_input(move |t| Message::Db(DbMsg::Filter(filter_path.clone(), t)))
            .on_submit(Message::Db(DbMsg::ApplyFilter(owned())))
            .size(12.5)
            .font(se_theme::MONO)
            .padding([4, 8])
            .style(se_theme::Field::new(dark).style()),
        nav(
            "✓",
            t!("db.apply").to_string(),
            Some(Message::Db(DbMsg::ApplyFilter(owned()))),
        ),
        text_input(&t!("db.search_hint"), &tab.search_text)
            .on_input({
                let path = owned();
                move |t| Message::Db(DbMsg::SearchText(path.clone(), t))
            })
            .on_submit(Message::Db(DbMsg::ApplyFilter(owned())))
            .size(12.5)
            .padding([4, 8])
            .width(200)
            .style(se_theme::Field::new(dark).style()),
        nav(
            "▽",
            t!("db.column_filters").to_string(),
            Some(Message::Db(DbMsg::ToggleColumnFilters(owned()))),
        ),
    ]
    .spacing(8)
    .padding([0, 12])
    .align_y(Alignment::Center);
    if tab.is_filtered() {
        filter_bar = filter_bar.push(
            button(text("×").size(15).color(p.muted))
                .padding([0, 8])
                .on_press(Message::Db(DbMsg::ClearFilter(owned())))
                .style(ghost(dark, false)),
        );
    }

    let rule = || {
        container(Space::new().height(1))
            .width(Length::Fill)
            .style(se_theme::rule(dark))
    };
    let mut page = if is_query {
        column![bar, rule()]
    } else {
        column![bar, filter_bar, Space::new().height(8), rule()]
    };
    if let Some(notice) = &tab.notice {
        page = page.push(
            button(
                text(notice.as_str())
                    .size(12.5)
                    .color(se_theme::error_color(dark)),
            )
            .padding([6, 12])
            .on_press(Message::Db(DbMsg::DismissNotice(owned())))
            .style(|_: &iced::Theme, _| button::Style::default()),
        );
    }

    let body: Element<Message> = match (&tab.error, shown) {
        (Some(error), _) => container(
            text(error.as_str())
                .size(13)
                .color(se_theme::error_color(dark)),
        )
        .padding(16)
        .into(),
        (None, None) => container(text(t!("db.loading").to_string()).size(13).color(p.muted))
            .padding(16)
            .into(),
        (None, Some(_)) => grid(path, tab, dark),
    };
    let view: Element<Message> = container(page.push(body).height(Length::Fill))
        .width(Length::Fill)
        .height(Length::Fill)
        .style(se_theme::window(dark))
        .into();

    match (&tab.picker, tab.preview, tab.form) {
        (Some(picker), _, _) => {
            crate::floating::modal(view, date_picker(path, picker, dark), None, dark, false)
        }
        (None, true, _) => {
            crate::floating::modal(view, preview_card(path, tab, dark), None, dark, false)
        }
        (None, false, Some(row)) => crate::floating::modal(
            view,
            form_card(path, tab, row, dark),
            Some(Message::Db(DbMsg::CloseForm(path.to_path_buf()))),
            dark,
            false,
        ),
        (None, false, None) => view,
    }
}

/// A row as a form: a field for each column, typed by what the column holds.
fn form_card<'a>(
    path: &'a Path,
    tab: &'a TableTab,
    row: usize,
    dark: bool,
) -> Element<'a, Message> {
    let p = se_theme::palette(dark);
    let Some(page) = &tab.page else {
        return Space::new().into();
    };
    let count = tab.pending.rows(page);
    let deleted = tab.pending.is_deleted(row);
    let editable = page.editable && !deleted;
    let owned = || path.to_path_buf();
    let mut fields = column![].spacing(10).padding([0, 12]);
    for (c, column) in page.columns.iter().enumerate() {
        let cell = tab.pending.cell(page, row, c);
        let current = match cell {
            Cell::Value(v) => v.to_string(),
            _ => String::new(),
        };
        let kind = column.kind.to_lowercase();
        let listed = tab
            .choices
            .as_ref()
            .and_then(|all| all.iter().find(|ch| ch.column == column.name));
        let placeholder = match cell {
            Cell::Null => "NULL",
            Cell::Default => "DEFAULT",
            Cell::Value(_) => "",
        };
        let on_input = {
            let path = owned();
            move |t: String| Message::Db(DbMsg::FormInput(path.clone(), c, t))
        };
        let mut control = row![].spacing(8).align_y(Alignment::Center);
        let input: Element<Message> = if !editable {
            text(if current.is_empty() {
                placeholder.to_string()
            } else {
                current.clone()
            })
            .size(12.5)
            .color(if current.is_empty() { p.muted } else { p.text })
            .into()
        } else if kind == "boolean" || kind == "bool" {
            let chosen = match current.to_lowercase().as_str() {
                "t" | "true" => Some("true".to_string()),
                "f" | "false" => Some("false".to_string()),
                _ => None,
            };
            pick_list(
                vec!["true".to_string(), "false".to_string()],
                chosen,
                on_input,
            )
            .placeholder(placeholder)
            .text_size(12.5)
            .padding([3, 8])
            .style(se_theme::Select { dark }.style())
            .menu_style(se_theme::Select { dark }.menu_style())
            .into()
        } else if let Some(list) = listed {
            // The values the database allows: an enumeration, or the keys of
            // the table a foreign key points to.
            let mut values = list.values.clone();
            if !current.is_empty() && !values.contains(&current) {
                values.insert(0, current.clone());
            }
            let chosen = (!current.is_empty()).then_some(current.clone());
            let mut line = row![pick_list(values, chosen, on_input)
                .placeholder(placeholder)
                .text_size(12.5)
                .padding([3, 8])
                .style(se_theme::Select { dark }.style())
                .menu_style(se_theme::Select { dark }.menu_style())]
            .spacing(8)
            .align_y(Alignment::Center);
            if list.more {
                line = line.push(
                    text(t!("db.choices_more", n = protocol::MAX_CHOICES).to_string())
                        .size(11.5)
                        .color(p.muted),
                );
            }
            line.into()
        } else {
            text_input(placeholder, &current)
                .on_input(on_input)
                .size(12.5)
                .padding([3, 8])
                .width(Length::Fill)
                .style(se_theme::Field::new(dark).style())
                .into()
        };
        control = control.push(input);
        if editable && (kind == "json" || kind == "jsonb") && !current.is_empty() {
            let valid = serde_json::from_str::<serde_json::Value>(&current).is_ok();
            control = control
                .push(
                    text(if valid { "✓ JSON" } else { "✗ JSON" })
                        .size(11.5)
                        .color(if valid {
                            p.accent
                        } else {
                            se_theme::error_color(dark)
                        }),
                )
                .push(
                    button(text(t!("db.form_format").to_string()).size(12))
                        .padding([2, 8])
                        .on_press_maybe(valid.then(|| Message::Db(DbMsg::FormJson(owned(), c))))
                        .style((se_theme::Outline { dark }).style()),
                );
        }
        if editable && column.nullable && !matches!(cell, Cell::Null) {
            control = control.push(
                button(text("NULL").size(11.5))
                    .padding([2, 8])
                    .on_press(Message::Db(DbMsg::FormNull(owned(), c)))
                    .style((se_theme::Outline { dark }).style()),
            );
        }
        fields = fields.push(
            row![
                column![
                    text(column.name.as_str())
                        .size(12.5)
                        .font(se_theme::UI_SEMIBOLD),
                    text(column.kind.as_str()).size(11).color(p.muted),
                ]
                .width(Length::Fixed(150.0)),
                control,
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        );
    }
    let head = row![
        text(t!("db.form_title", n = row + 1, total = count).to_string())
            .size(13.5)
            .font(se_theme::UI_SEMIBOLD),
        Space::new().width(Length::Fill),
        button(text("‹").size(14))
            .padding([0, 8])
            .on_press_maybe((row > 0).then(|| Message::Db(DbMsg::FormMove(owned(), -1))))
            .style(ghost(dark, false)),
        button(text("›").size(14))
            .padding([0, 8])
            .on_press_maybe((row + 1 < count).then(|| Message::Db(DbMsg::FormMove(owned(), 1))))
            .style(ghost(dark, false)),
        button(text("×").size(16).color(p.muted))
            .padding([0, 7])
            .on_press(Message::Db(DbMsg::CloseForm(owned())))
            .style(ghost(dark, false)),
    ]
    .spacing(4)
    .align_y(Alignment::Center);
    container(
        column![
            head,
            scrollable(fields)
                .style(se_theme::Scrollbar { dark }.style())
                .height(Length::Shrink),
        ]
        .spacing(10)
        .width(Length::Fixed(640.0)),
    )
    .max_height(560)
    .padding(16)
    .style(se_theme::card(dark))
    .into()
}

/// The SQL that Commit would run, as the log will show it.
fn preview_card<'a>(path: &'a Path, tab: &'a TableTab, dark: bool) -> Element<'a, Message> {
    let p = se_theme::palette(dark);
    let table = tab.table_ref();
    let changes = tab
        .page
        .as_ref()
        .map(|page| tab.pending.changes(page))
        .unwrap_or_default();
    let mut list = column![].spacing(8);
    if changes.is_empty() {
        list = list.push(
            text(t!("db.no_changes").to_string())
                .size(12.5)
                .color(p.muted),
        );
    }
    for change in &changes {
        list = list.push(
            text(protocol::display_change(
                change,
                &table.schema,
                &table.table,
            ))
            .size(12.5)
            .font(se_theme::MONO),
        );
    }
    let head = row![
        text(t!("db.preview_title").to_string())
            .size(13.5)
            .font(se_theme::UI_SEMIBOLD),
        Space::new().width(Length::Fill),
        button(text("×").size(16).color(p.muted))
            .padding([0, 7])
            .on_press(Message::Db(DbMsg::Preview(path.to_path_buf(), false)))
            .style(ghost(dark, false)),
    ]
    .align_y(Alignment::Center);
    container(
        column![
            head,
            text(t!("db.preview_hint").to_string())
                .size(12)
                .color(p.muted),
            scrollable(list)
                .style(se_theme::Scrollbar { dark }.style())
                .height(Length::Shrink),
        ]
        .spacing(10)
        .width(Length::Fixed(640.0)),
    )
    .max_height(480)
    .padding(16)
    .style(se_theme::card(dark))
    .into()
}

/// What decides the entries of the menu of a cell.
#[derive(Clone, Copy)]
struct CellInfo {
    /// The row can be changed (a table with a key, on a session that writes).
    editable: bool,
    /// The cell has a value (not NULL, not empty).
    has_value: bool,
    nullable: bool,
    pick: Option<PickKind>,
    deleted: bool,
    /// Something of the row was edited.
    edited: bool,
    /// The row is one that is added.
    new_row: bool,
}

/// The menu of a cell: what can be done to it and to its row.
fn cell_menu(
    path: &Path,
    row: usize,
    col: usize,
    info: CellInfo,
    related: &Related,
    dark: bool,
) -> Element<'static, Message> {
    let CellInfo {
        editable,
        has_value,
        nullable,
        pick,
        deleted,
        edited,
        new_row,
    } = info;
    let path = path.to_path_buf();
    let mut items: Vec<Element<'static, Message>> = Vec::new();
    if has_value && !new_row {
        items.push(menu_bar::item(
            t!("db.cell_view").to_string(),
            None,
            dark,
            Message::Db(DbMsg::ViewValue(path.clone(), row, col)),
        ));
        items.push(menu_bar::item(
            t!("db.cell_copy").to_string(),
            None,
            dark,
            Message::Db(DbMsg::CopyCell(path.clone(), row, col)),
        ));
    }
    for (label, format) in [
        (t!("db.copy_csv"), CopyFormat::Csv),
        (t!("db.copy_insert"), CopyFormat::Insert),
        (t!("db.copy_markdown"), CopyFormat::Markdown),
        (t!("db.copy_tsv"), CopyFormat::Tsv),
    ] {
        items.push(menu_bar::item(
            label.to_string(),
            None,
            dark,
            Message::Db(DbMsg::CopySelection(path.clone(), format)),
        ));
    }
    items.push(menu_bar::item(
        t!("db.form_open").to_string(),
        None,
        dark,
        Message::Db(DbMsg::OpenForm(path.clone(), row)),
    ));
    if !new_row {
        items.push(menu_bar::item(
            t!("db.row_copy").to_string(),
            None,
            dark,
            Message::Db(DbMsg::CopyRow(path.clone(), row)),
        ));
    }
    if !related.links.is_empty() && !new_row {
        items.push(menu_bar::separator(dark));
        items.push(menu_bar::item(
            t!("db.links", n = related.links.len()).to_string(),
            None,
            dark,
            Message::Db(DbMsg::ShowLinks(Box::new(super::LinksView {
                conn: related.conn,
                database: related.database.clone(),
                schema: related.schema.clone(),
                links: related.links.clone(),
            }))),
        ));
    }
    if editable && (has_value || nullable || !new_row) {
        items.push(menu_bar::separator(dark));
    }
    if editable && !deleted {
        items.push(menu_bar::item(
            t!("db.cell_edit").to_string(),
            None,
            dark,
            Message::Db(DbMsg::Edit(path.clone(), row, col)),
        ));
        if nullable {
            items.push(menu_bar::item(
                t!("db.cell_null").to_string(),
                None,
                dark,
                Message::Db(DbMsg::SetNull(path.clone(), row, col)),
            ));
        }
        if new_row {
            items.push(menu_bar::item(
                t!("db.cell_default").to_string(),
                None,
                dark,
                Message::Db(DbMsg::SetDefault(path.clone(), row, col)),
            ));
        }
        if let Some(kind) = pick {
            let label = match kind {
                PickKind::Date => t!("db.pick_date"),
                PickKind::Time => t!("db.pick_time"),
                PickKind::DateTime => t!("db.pick_datetime"),
            };
            items.push(menu_bar::item(
                label.to_string(),
                None,
                dark,
                Message::Db(DbMsg::OpenPicker(path.clone(), row, col)),
            ));
        }
        items.push(menu_bar::separator(dark));
        items.push(menu_bar::item(
            t!("db.row_add").to_string(),
            None,
            dark,
            Message::Db(DbMsg::AddRow(path.clone())),
        ));
        items.push(menu_bar::item(
            if new_row {
                t!("db.row_remove")
            } else {
                t!("db.row_delete")
            }
            .to_string(),
            None,
            dark,
            Message::Db(DbMsg::DeleteRow(path.clone(), row)),
        ));
    }
    if editable && (deleted || edited) {
        items.push(menu_bar::item(
            if deleted {
                t!("db.row_undelete")
            } else {
                t!("db.row_revert")
            }
            .to_string(),
            None,
            dark,
            Message::Db(DbMsg::RevertRow(path, row)),
        ));
    }
    container(column(items).spacing(1).padding(6).width(280))
        .style(se_theme::card(dark))
        .into()
}

/// The rows a row is linked to, for its menu.
struct Related {
    conn: u64,
    database: String,
    /// The schema of the table: tables of it are shown without it.
    schema: String,
    links: Vec<ddl::Link>,
}

/// The header, then the rows in view: only those are made, the others are
/// space. The header stays in place when the rows scroll up and down.
/// The width of the edge of a column that is dragged.
const HANDLE: f32 = 4.0;

fn grid<'a>(path: &'a Path, tab: &'a TableTab, dark: bool) -> Element<'a, Message> {
    let p = se_theme::palette(dark);
    let Some(page) = &tab.page else {
        return Space::new().into();
    };
    let editable = page.editable;
    let total_width: f32 = tab.widths.iter().sum::<f32>() + 56.0;
    let cell = |content: Element<'a, Message>, width: f32| {
        container(content)
            .width(Length::Fixed(width))
            .height(Length::Fixed(ROW_HEIGHT))
            .padding([0, 10])
            .center_y(Length::Fixed(ROW_HEIGHT))
            .clip(true)
    };

    // The line number is the row's place in the table.
    let gutter_cell = |label: String, color: iced::Color| {
        container(text(label).size(11.5).color(color))
            .width(Length::Fixed(56.0))
            .height(Length::Fixed(ROW_HEIGHT))
            .padding([0, 10])
            .center_y(Length::Fixed(ROW_HEIGHT))
            .align_x(iced::alignment::Horizontal::Right)
    };
    let gutter = |label: String| gutter_cell(label, p.gutter);

    // The header: a click sorts by the column, as in a JetBrains IDE.
    let mut header = row![gutter(String::new())];
    for (i, (column, width)) in page.columns.iter().zip(&tab.widths).enumerate() {
        let marker = tab.order.iter().position(|(name, _)| *name == column.name);
        let arrow = marker.map(|at| {
            let desc = tab.order[at].1;
            // With several columns sorted, which place in the order.
            let place = if tab.order.len() > 1 {
                (at + 1).to_string()
            } else {
                String::new()
            };
            text(format!("{}{place}", if desc { "▼" } else { "▲" }))
                .size(10)
                .color(p.accent)
        });
        let mut label = row![
            text(column.name.as_str())
                .size(12.5)
                .font(se_theme::UI_SEMIBOLD)
                .wrapping(iced::widget::text::Wrapping::None),
            text(column.kind.as_str())
                .size(11)
                .color(p.muted)
                .wrapping(iced::widget::text::Wrapping::None),
        ]
        .spacing(6)
        .align_y(Alignment::Center);
        if let Some(arrow) = arrow {
            label = label.push(arrow);
        }
        header = header.push(
            button(label)
                .padding([0, 10])
                .width(Length::Fixed(*width - HANDLE))
                .height(Length::Fixed(ROW_HEIGHT))
                .on_press_maybe(
                    (!tab.is_static_result())
                        .then(|| Message::Db(DbMsg::Sort(path.to_path_buf(), i))),
                )
                .style(ghost(dark, marker.is_some())),
        );
        // The edge of the column: drag it to change the width.
        header = header.push(
            iced::widget::mouse_area(
                container(Space::new())
                    .width(Length::Fixed(HANDLE))
                    .height(Length::Fixed(ROW_HEIGHT))
                    .style(move |_: &iced::Theme| container::Style {
                        background: Some(iced::Background::Color(iced::Color {
                            a: 0.35,
                            ..p.muted
                        })),
                        ..container::Style::default()
                    }),
            )
            .on_press(Message::Db(DbMsg::ResizeStart(path.to_path_buf(), i)))
            .interaction(iced::mouse::Interaction::ResizingHorizontally),
        );
    }
    let header = container(header).style(move |_: &iced::Theme| container::Style {
        background: Some(iced::Background::Color(p.elevated)),
        ..container::Style::default()
    });

    // The fields under the names: a condition for each column.
    let filters: Option<Element<'a, Message>> = tab.show_filters.then(|| {
        let mut line = row![gutter(String::new())];
        for (column, width) in page.columns.iter().zip(&tab.widths) {
            let name = column.name.clone();
            let shown = tab
                .column_text
                .get(&column.name)
                .cloned()
                .unwrap_or_default();
            line = line.push(
                container(
                    text_input("", &shown)
                        .on_input({
                            let path = path.to_path_buf();
                            move |t| Message::Db(DbMsg::ColumnFilter(path.clone(), name.clone(), t))
                        })
                        .on_submit(Message::Db(DbMsg::ApplyFilter(path.to_path_buf())))
                        .size(12)
                        .padding([2, 6])
                        .style(se_theme::Field::new(dark).style()),
                )
                .width(Length::Fixed(*width))
                .padding([2, 4]),
            );
        }
        line.into()
    });

    // The rows of the page, then the ones that are added.
    let count = tab.pending.rows(page);
    let (offset, height) = tab.scroll;
    let first = ((offset / ROW_HEIGHT) as usize).saturating_sub(6);
    let last = (((offset + height) / ROW_HEIGHT) as usize + 6).min(count);
    let mut rows = column![Space::new().height(first as f32 * ROW_HEIGHT)];
    // The date kind of each column, once for all the rows.
    let picks: Vec<Option<PickKind>> = page.columns.iter().map(|c| PickKind::of(&c.kind)).collect();
    for i in first..last {
        let new_row = tab.pending.is_new(page, i);
        let deleted = tab.pending.is_deleted(i);
        let row_edited = tab.pending.edits.keys().any(|(r, _)| *r == i);
        let focused_row = tab.selected.is_some_and(|(r, _)| r == i);
        let mut line = row![gutter_cell(
            if new_row {
                "+".to_string()
            } else {
                (page.offset + i as u64 + 1).to_string()
            },
            if focused_row || new_row {
                p.accent
            } else {
                p.gutter
            }
        )];
        for (c, width) in tab.widths.iter().enumerate().take(page.columns.len()) {
            let edited = tab.pending.is_edited(i, c);
            let value = tab.pending.cell(page, i, c);
            let typing = tab.editing.as_ref().filter(|e| e.row == i && e.col == c);
            let content: Element<Message> = match (typing, value) {
                (Some(editing), _) => text_input("", &editing.text)
                    .id(super::edit_id())
                    .on_input({
                        let path = path.to_path_buf();
                        move |t| Message::Db(DbMsg::EditInput(path.clone(), t))
                    })
                    .on_submit(Message::Db(DbMsg::EditCommit(path.to_path_buf())))
                    .size(12.5)
                    .padding([2, 6])
                    .style(se_theme::Field::new(dark).style())
                    .into(),
                (None, Cell::Value(text_value)) => text(super::grid::shown(text_value))
                    .size(12.5)
                    .color(if deleted { p.muted } else { p.text })
                    .wrapping(iced::widget::text::Wrapping::None)
                    .into(),
                (None, Cell::Null) => text("NULL")
                    .size(11.5)
                    .color(p.muted)
                    .font(se_theme::UI_ITALIC)
                    .into(),
                // A new row's cell that is left to the column's default.
                (None, Cell::Default) => text("DEFAULT")
                    .size(11.5)
                    .color(p.muted)
                    .font(se_theme::UI_ITALIC)
                    .into(),
            };
            let mut base = cell(content, *width);
            if typing.is_some() {
                base = base.padding([0, 3]);
            }
            // The cell that has the focus is outlined, so that what is done
            // to it (the right click) is seen.
            let focused = tab.selected == Some((i, c));
            let in_block = tab.block().is_some_and(|((r0, c0), (r1, c1))| {
                (r0..=r1).contains(&i) && (c0..=c1).contains(&c)
            });
            let base = base.style(move |_: &iced::Theme| container::Style {
                background: if edited {
                    Some(iced::Background::Color(iced::Color {
                        a: 0.28,
                        ..p.accent
                    }))
                } else {
                    (focused || in_block).then_some(iced::Background::Color(iced::Color {
                        a: if in_block && !focused { 0.18 } else { 0.10 },
                        ..p.accent
                    }))
                },
                border: if focused {
                    iced::Border {
                        color: p.accent,
                        width: 1.5,
                        radius: 0.0.into(),
                    }
                } else {
                    iced::Border::default()
                },
                ..container::Style::default()
            });
            let nullable = page.columns[c].nullable;
            let pick = picks[c];
            let menu_path = path.to_path_buf();
            let mut hit = iced::widget::mouse_area(base);
            if editable {
                hit = hit.on_double_click(Message::Db(DbMsg::Edit(path.to_path_buf(), i, c)));
            }
            let info = CellInfo {
                editable,
                has_value: tab.pending.value(page, i, c).is_some_and(|v| !v.is_empty()),
                nullable,
                pick,
                deleted,
                edited: row_edited,
                new_row,
            };
            // The rows this row is linked to are worked out when the menu
            // opens, not for every cell at every frame.
            let element: Element<Message> = crate::pointer_area::context_menu(hit, move || {
                let links = match (&tab.structure, new_row, page.rows.get(i)) {
                    (Some(structure), false, Some(values)) if tab.query.is_none() => {
                        ddl::links(&tab.schema, structure, &page.columns, values, tab.dialect)
                    }
                    _ => Vec::new(),
                };
                let related = Related {
                    conn: tab.conn,
                    database: tab.database.clone(),
                    schema: tab.schema.clone(),
                    links,
                };
                cell_menu(&menu_path, i, c, info, &related, dark)
            });
            // A click, left or right, gives the focus (it is only heard here:
            // the menu and the double click below still get theirs).
            let select = Message::Db(DbMsg::Select(path.to_path_buf(), i, c));
            let element = crate::pointer_area::pointer_area(element)
                .watching()
                .on_press({
                    let select = select.clone();
                    move |_| select.clone()
                })
                .on_right_press(select);
            line = line.push(element);
        }
        rows = rows.push(
            container(line).style(move |_: &iced::Theme| container::Style {
                background: if deleted {
                    Some(iced::Background::Color(iced::Color {
                        a: 0.16,
                        ..se_theme::error_color(dark)
                    }))
                } else if new_row {
                    Some(iced::Background::Color(iced::Color {
                        a: 0.12,
                        ..p.accent
                    }))
                } else {
                    (i % 2 == 1).then_some(iced::Background::Color(iced::Color {
                        a: 0.35,
                        ..p.elevated
                    }))
                },
                ..container::Style::default()
            }),
        );
    }
    rows = rows.push(Space::new().height(count.saturating_sub(last) as f32 * ROW_HEIGHT));

    let path_owned = path.to_path_buf();
    let vertical = scrollable(rows)
        .id(super::grid_id())
        .on_scroll(move |v| {
            Message::Db(DbMsg::Scrolled(
                path_owned.clone(),
                v.absolute_offset().y,
                v.bounds().height,
            ))
        })
        .style(se_theme::Scrollbar { dark }.style())
        .width(Length::Fixed(total_width))
        .height(Length::Fill);

    let mut top = column![header].width(Length::Fixed(total_width));
    if let Some(filters) = filters {
        top = top.push(filters);
    }
    scrollable(top.push(vertical))
        .direction(scrollable::Direction::Horizontal(
            scrollable::Scrollbar::new(),
        ))
        .style(se_theme::Scrollbar { dark }.style())
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// The date picker of a cell: a month to look at, and a time to type, as the
/// column's type asks for.
fn date_picker<'a>(path: &'a Path, picker: &'a Picker, dark: bool) -> Element<'a, Message> {
    let p = se_theme::palette(dark);
    let owned = || path.to_path_buf();
    let title = match picker.kind {
        PickKind::Date => t!("db.picker_date"),
        PickKind::Time => t!("db.picker_time"),
        PickKind::DateTime => t!("db.picker_datetime"),
    };
    let head = row![
        text(title.to_string())
            .size(13.5)
            .font(se_theme::UI_SEMIBOLD),
        Space::new().width(Length::Fill),
        button(text("×").size(16).color(p.muted))
            .padding([0, 7])
            .on_press(Message::Db(DbMsg::PickerClose(owned())))
            .style(ghost(dark, false)),
    ]
    .align_y(Alignment::Center);
    let mut card = column![head].spacing(10);

    if picker.kind.has_date() {
        let months = t!("db.months").to_string();
        let month_name = months
            .split(',')
            .nth(picker.month as usize - 1)
            .unwrap_or("")
            .to_string();
        let step = |glyph: &'static str, by: i32| {
            button(text(glyph).size(13))
                .padding([2, 8])
                .on_press(Message::Db(DbMsg::PickerShift(owned(), by)))
                .style((se_theme::Outline { dark }).style())
        };
        card = card.push(
            row![
                step("«", -12),
                step("‹", -1),
                text(format!("{month_name} {}", picker.year))
                    .size(13)
                    .width(Length::Fill)
                    .align_x(iced::alignment::Horizontal::Center),
                step("›", 1),
                step("»", 12),
            ]
            .spacing(4)
            .align_y(Alignment::Center),
        );
        let weekdays = t!("db.weekdays").to_string();
        let mut names = row![].spacing(2);
        for name in weekdays.split(',') {
            names = names.push(
                text(name.to_string())
                    .size(11.5)
                    .color(p.muted)
                    .width(36)
                    .align_x(iced::alignment::Horizontal::Center),
            );
        }
        card = card.push(names);
        let blanks = picker.first_weekday();
        let days = picker.days_in_month();
        let mut week = row![].spacing(2);
        let mut weeks = column![].spacing(2);
        let mut filled = 0;
        for _ in 0..blanks {
            week = week.push(Space::new().width(36).height(DAY));
            filled += 1;
        }
        for day in 1..=days {
            let chosen = picker.day == Some(day);
            week = week.push(
                button(
                    text(day.to_string())
                        .size(12.5)
                        .width(Length::Fill)
                        .align_x(iced::alignment::Horizontal::Center),
                )
                .padding([0, 0])
                .width(36)
                .height(DAY)
                .on_press(Message::Db(DbMsg::PickerDay(owned(), day)))
                .style(ghost(dark, chosen)),
            );
            filled += 1;
            if filled % 7 == 0 {
                weeks = weeks.push(week);
                week = row![].spacing(2);
            }
        }
        if filled % 7 != 0 {
            weeks = weeks.push(week);
        }
        card = card.push(weeks);
    }

    if picker.kind.has_time() {
        let field = |value: &str, part: TimePart| {
            let path = owned();
            text_input("00", value)
                .on_input(move |t| Message::Db(DbMsg::PickerTime(path.clone(), part, t)))
                .size(13)
                .font(se_theme::MONO)
                .padding([4, 8])
                .width(52)
                .style(se_theme::Field::new(dark).style())
        };
        card = card.push(
            row![
                text(t!("db.picker_time").to_string())
                    .size(12.5)
                    .color(p.muted)
                    .width(60),
                field(&picker.hour, TimePart::Hour),
                text(":").size(13),
                field(&picker.minute, TimePart::Minute),
                text(":").size(13),
                field(&picker.second, TimePart::Second),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        );
    }

    card = card.push(
        row![
            button(text(t!("db.picker_today").to_string()).size(12.5))
                .padding([5, 14])
                .on_press(Message::Db(DbMsg::PickerToday(owned())))
                .style((se_theme::Outline { dark }).style()),
            Space::new().width(Length::Fill),
            button(text(t!("db.save").to_string()).size(12.5))
                .padding([5, 18])
                .on_press_maybe(
                    picker
                        .value()
                        .map(|_| Message::Db(DbMsg::PickerApply(owned()))),
                )
                .style((se_theme::Primary { dark }).style()),
        ]
        .align_y(Alignment::Center),
    );
    container(card.width(Length::Fixed(300.0)))
        .padding(16)
        .style(se_theme::card(dark))
        .into()
}

// ----------------------------------------------------------------- console

/// A connection in the list of the console.
#[derive(Clone, PartialEq)]
struct Conn(u64, String);

impl fmt::Display for Conn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.1)
    }
}

/// The panel at the bottom: the log of the commands, and the queries.
pub fn console<'a>(state: &'a State, dark: bool) -> Element<'a, Message> {
    let c = &state.console;
    let tab = |label: String, which: ConsoleTab| {
        button(text(label).size(12.5))
            .padding([4, 10])
            .on_press(Message::Db(DbMsg::ConsoleTab(which)))
            .style(ghost(dark, c.tab == which))
    };
    let mut header = row![
        tab(t!("db.console_queries").to_string(), ConsoleTab::Queries),
        tab(t!("db.console_log").to_string(), ConsoleTab::Log),
        Space::new().width(Length::Fill),
    ]
    .spacing(4)
    .align_y(Alignment::Center);

    match c.tab {
        ConsoleTab::Queries => {
            let options: Vec<Conn> = state
                .connections
                .iter()
                .map(|k| Conn(k.id, k.label()))
                .collect();
            let selected = c
                .connection
                .and_then(|id| options.iter().find(|o| o.0 == id).cloned());
            header = header.push(
                pick_list(options, selected, |o: Conn| {
                    Message::Db(DbMsg::ConsoleConnection(o.0))
                })
                .text_size(12.5)
                .padding([3, 8])
                .placeholder(t!("db.console_connection").to_string())
                .style(se_theme::Select { dark }.style())
                .menu_style(se_theme::Select { dark }.menu_style()),
            );
            // The database of the server the queries run in, when the tree
            // knows more than one.
            if let Some(Tree::Loaded(databases)) = c.connection.and_then(|id| state.trees.get(&id))
            {
                if databases.len() > 1 {
                    let names: Vec<String> = databases.iter().map(|d| d.name.clone()).collect();
                    let current = databases.iter().find(|d| d.current).map(|d| d.name.clone());
                    let chosen = if c.database.is_empty() {
                        current.clone()
                    } else {
                        Some(c.database.clone())
                    };
                    header = header.push(
                        pick_list(names, chosen, move |name: String| {
                            let own = current.as_deref() == Some(name.as_str());
                            Message::Db(DbMsg::ConsoleDatabase(if own {
                                String::new()
                            } else {
                                name
                            }))
                        })
                        .text_size(12.5)
                        .padding([3, 8])
                        .placeholder(t!("db.console_database").to_string())
                        .style(se_theme::Select { dark }.style())
                        .menu_style(se_theme::Select { dark }.menu_style()),
                    );
                }
            }
            let ready = !c.running && *state.driver_of(c.dialect) == Driver::Ready;
            if c.in_transaction {
                header = header
                    .push(
                        text(t!("db.tx_open").to_string())
                            .size(12)
                            .color(se_theme::error_color(dark)),
                    )
                    .push(
                        button(text(t!("db.tx_commit").to_string()).size(12.5))
                            .padding([3, 10])
                            .on_press_maybe(
                                ready.then_some(Message::Db(DbMsg::EndTransaction(true))),
                            )
                            .style((se_theme::Outline { dark }).style()),
                    )
                    .push(
                        button(text(t!("db.tx_rollback").to_string()).size(12.5))
                            .padding([3, 10])
                            .on_press_maybe(
                                ready.then_some(Message::Db(DbMsg::EndTransaction(false))),
                            )
                            .style((se_theme::Outline { dark }).style()),
                    );
            }
            if c.running {
                header = header.push(icon_button(
                    "■",
                    t!("db.cancel").to_string(),
                    Message::Db(DbMsg::Cancel),
                    dark,
                ));
            }
            header = header
                .push(
                    button(text(t!("db.run").to_string()).size(12.5))
                        .padding([3, 12])
                        .on_press_maybe(ready.then_some(Message::Db(DbMsg::Run(false))))
                        .style((se_theme::Primary { dark }).style()),
                )
                .push(
                    button(text(t!("db.format").to_string()).size(12.5))
                        .padding([3, 12])
                        .on_press(Message::Db(DbMsg::FormatSql))
                        .style((se_theme::Outline { dark }).style()),
                )
                .push(
                    button(text(t!("db.explain").to_string()).size(12.5))
                        .padding([3, 12])
                        .on_press_maybe(ready.then_some(Message::Db(DbMsg::Explain(false))))
                        .style((se_theme::Outline { dark }).style()),
                )
                .push(
                    button(text(t!("db.explain_analyze").to_string()).size(12.5))
                        .padding([3, 12])
                        .on_press_maybe(ready.then_some(Message::Db(DbMsg::Explain(true))))
                        .style((se_theme::Outline { dark }).style()),
                )
                .push(
                    button(text(t!("db.run_all").to_string()).size(12.5))
                        .padding([3, 12])
                        .on_press_maybe(ready.then_some(Message::Db(DbMsg::Run(true))))
                        .style((se_theme::Outline { dark }).style()),
                );
        }
        ConsoleTab::Log => {
            header = header.push(
                text_input(&t!("db.log_search"), &c.log_filter)
                    .on_input(|text| Message::Db(DbMsg::LogFilter(text)))
                    .size(12.5)
                    .padding([3, 8])
                    .width(220),
            );
            header = header.push(
                checkbox(c.show_internal)
                    .label(t!("db.log_internal").to_string())
                    .size(14)
                    .text_size(12.5)
                    .on_toggle(|on| Message::Db(DbMsg::ShowInternal(on))),
            );
            header = header.push(
                button(text(t!("db.log_clear").to_string()).size(12.5))
                    .padding([3, 12])
                    .on_press_maybe((!c.log.is_empty()).then_some(Message::Db(DbMsg::ClearLog)))
                    .style((se_theme::Outline { dark }).style()),
            );
        }
    }
    header = header.push(
        button(text("—").size(12))
            .padding([1, 8])
            .on_press(Message::Db(DbMsg::ToggleConsole))
            .style(ghost(dark, false)),
    );

    let content: Element<Message> = match c.tab {
        ConsoleTab::Log => log_view(state, dark),
        ConsoleTab::Queries => queries_view(state, dark),
    };
    column![
        crate::app::resize_edge(dark),
        container(header).padding([4, 10]),
        container(Space::new().height(1))
            .width(Length::Fill)
            .style(se_theme::rule(dark)),
        content,
    ]
    .height(Length::Fill)
    .into()
}

/// What was run, oldest first, the end in view.
fn log_view<'a>(state: &'a State, dark: bool) -> Element<'a, Message> {
    let p = se_theme::palette(dark);
    let log = &state.console.log;
    if log.is_empty() {
        return container(
            text(t!("db.log_empty").to_string())
                .size(12.5)
                .color(p.muted),
        )
        .padding(14)
        .into();
    }
    let mut list = column![].spacing(2).padding([6, 8]);
    for (at, entry) in log
        .iter()
        .enumerate()
        .filter(|(_, entry)| state.console.show_internal || !entry.internal)
        .filter(|(_, entry)| state.console.matches(entry))
    {
        let ok = entry.error.is_none();
        let mut head = row![
            text(entry.time.as_str())
                .size(11.5)
                .font(se_theme::MONO)
                .color(p.muted),
            text(if ok { "✓" } else { "✗" }).size(12).color(if ok {
                p.accent
            } else {
                se_theme::error_color(dark)
            }),
            text(entry.connection.as_str()).size(12).color(p.muted),
            text(format!("{} ms", entry.ms)).size(12).color(p.muted),
        ]
        .spacing(10)
        .align_y(Alignment::Center);
        if let Some(rows) = entry.rows {
            head = head.push(
                text(if rows == 1 {
                    t!("db.log_row_one").to_string()
                } else {
                    t!("db.log_rows", n = rows).to_string()
                })
                .size(12)
                .color(p.muted),
            );
        }
        let mut item = column![
            head,
            text(entry.sql.as_str())
                .size(12.5)
                .font(se_theme::MONO)
                .color(if entry.internal { p.muted } else { p.text }),
        ]
        .spacing(2);
        if let Some(error) = &entry.error {
            item = item.push(
                text(error.as_str())
                    .size(12.5)
                    .color(se_theme::error_color(dark)),
            );
        }
        // A click selects the entry (it is what Cmd/Ctrl+C copies), a right
        // click offers to copy it.
        let chosen = state.console.selected_log == Some(at);
        let framed =
            container(item)
                .padding([5, 8])
                .width(Length::Fill)
                .style(move |_: &iced::Theme| container::Style {
                    background: chosen.then_some(iced::Background::Color(iced::Color {
                        a: 0.14,
                        ..p.accent
                    })),
                    border: iced::Border {
                        color: if chosen {
                            p.accent
                        } else {
                            iced::Color::TRANSPARENT
                        },
                        width: if chosen { 1.0 } else { 0.0 },
                        radius: 0.0.into(),
                    },
                    ..container::Style::default()
                });
        let has_error = entry.error.is_some();
        let with_menu =
            crate::pointer_area::context_menu(framed, move || log_menu(at, has_error, dark));
        let select = Message::Db(DbMsg::SelectLog(at));
        list = list.push(
            crate::pointer_area::pointer_area(with_menu)
                .watching()
                .on_press({
                    let select = select.clone();
                    move |_| select.clone()
                })
                .on_right_press(select),
        );
    }
    scrollable(list)
        .id(super::log_id())
        .style(se_theme::Scrollbar { dark }.style())
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// The place to write queries, and what the last run said.
fn queries_view<'a>(state: &'a State, dark: bool) -> Element<'a, Message> {
    let p = se_theme::palette(dark);
    let c = &state.console;
    let visible = c.completion.is_some();
    let editor = text_editor(&c.editor)
        .placeholder(t!("db.queries_hint").to_string())
        .on_action(|a| Message::Db(DbMsg::Sql(a)))
        .key_binding(move |press| {
            use iced::keyboard::key::Named;
            use iced::keyboard::Key;
            let named = |n: Named| matches!(press.key.as_ref(), Key::Named(k) if k == n);
            let plain =
                !press.modifiers.command() && !press.modifiers.alt() && !press.modifiers.shift();
            // The list of proposals takes the arrows, Tab, Enter and Escape.
            if visible && plain {
                let message = if named(Named::ArrowDown) {
                    Some(DbMsg::CompleteMove(1))
                } else if named(Named::ArrowUp) {
                    Some(DbMsg::CompleteMove(-1))
                } else if named(Named::Tab) || named(Named::Enter) {
                    Some(DbMsg::CompleteAccept)
                } else if named(Named::Escape) {
                    Some(DbMsg::CompleteClose)
                } else {
                    None
                };
                if let Some(message) = message {
                    return Some(text_editor::Binding::Custom(Message::Db(message)));
                }
            }
            // Ctrl+Space asks for the proposals.
            if named(Named::Space) && (press.modifiers.control() || press.modifiers.command()) {
                return Some(text_editor::Binding::Custom(Message::Db(DbMsg::Complete)));
            }
            // Cmd/Ctrl+Enter runs; with Shift, everything.
            if named(Named::Enter) && press.modifiers.command() {
                return Some(text_editor::Binding::Custom(Message::Db(DbMsg::Run(
                    press.modifiers.shift(),
                ))));
            }
            text_editor::Binding::from_key_press(press)
        })
        // The SQL in color, with the palette of the code editor.
        .highlight_with::<crate::editor::highlighter::Highlighter>(
            crate::editor::highlighter::Settings {
                dark,
                extension: "sql".to_string(),
            },
            crate::editor::highlighter::to_format,
        )
        // No frame around the whole field: the statement that will run is the
        // one that is framed.
        .style(|theme, status| text_editor::Style {
            border: iced::Border {
                width: 0.0,
                ..iced::Border::default()
            },
            ..text_editor::default(theme, status)
        })
        .font(se_theme::MONO)
        .size(13)
        // A line is a row: the frame, the underline and the list below are
        // placed by logical line, which wrapping would shift.
        .wrapping(iced::widget::text::Wrapping::None)
        .padding([8, 12])
        .height(Length::Fill);
    // The statement "Run" would run (the caret is in it), framed; when
    // something is selected it is the selection that runs.
    let editor: Element<Message> = match c.statement_lines() {
        Some((first, last)) => {
            let p = se_theme::palette(dark);
            iced::widget::stack![
                editor,
                container(
                    container(
                        Space::new()
                            .width(Length::Fill)
                            .height(Length::Fixed((last - first + 1) as f32 * 16.9 + 2.0)),
                    )
                    .style(move |_: &iced::Theme| container::Style {
                        background: Some(iced::Background::Color(iced::Color {
                            a: 0.07,
                            ..p.accent
                        })),
                        border: iced::Border {
                            color: p.accent,
                            width: 1.0,
                            radius: 0.0.into(),
                        },
                        ..container::Style::default()
                    })
                )
                .padding(iced::Padding {
                    top: 8.0 + first as f32 * 16.9 - 1.0,
                    left: 4.0,
                    right: 4.0,
                    bottom: 0.0,
                })
                .width(Length::Fill)
                .height(Length::Fill)
            ]
            .into()
        }
        None => editor.into(),
    };
    // The word the server blamed, underlined in red (the same metrics).
    let editor: Element<Message> = match c.error_mark {
        Some(mark) => iced::widget::stack![
            editor,
            container(
                container(Space::new().height(2))
                    .width(Length::Fixed(mark.len as f32 * 7.8))
                    .style(move |_: &iced::Theme| container::Style {
                        background: Some(iced::Background::Color(se_theme::error_color(dark))),
                        ..container::Style::default()
                    })
            )
            .padding(iced::Padding {
                top: 8.0 + (mark.line as f32 + 1.0) * 16.9 - 1.0,
                left: 12.0 + mark.column as f32 * 7.8,
                ..iced::Padding::ZERO
            })
        ]
        .into(),
        None => editor,
    };
    // The list hangs under the word being typed (the editor's text metrics:
    // a character of the monospace font, and its line height).
    let editor: Element<Message> = match &c.completion {
        Some(completion) => iced::widget::stack![
            editor,
            container(completion_list(completion, dark)).padding(iced::Padding {
                top: 8.0 + (completion.line as f32 + 1.0) * 16.9 + 2.0,
                left: 12.0 + completion.column as f32 * 7.8,
                ..iced::Padding::ZERO
            })
        ]
        .into(),
        None => editor,
    };
    let status: Element<Message> = match &c.last {
        _ if c.guard.is_some() => {
            let risk = c.guard.as_ref().map(|g| g.risk);
            let message = match risk {
                Some(protocol::Risk::NoWhere) => t!("db.guard_no_where").to_string(),
                _ => t!("db.guard_writes").to_string(),
            };
            row![
                text(message).size(12).color(se_theme::error_color(dark)),
                button(text(t!("db.guard_run").to_string()).size(12.5))
                    .padding([3, 12])
                    .on_press(Message::Db(DbMsg::GuardRun))
                    .style((se_theme::Primary { dark }).style()),
                button(text(t!("db.guard_cancel").to_string()).size(12.5))
                    .padding([3, 12])
                    .on_press(Message::Db(DbMsg::GuardCancel))
                    .style((se_theme::Outline { dark }).style()),
            ]
            .spacing(10)
            .align_y(Alignment::Center)
            .into()
        }
        _ if c.running => text(t!("db.console_running").to_string())
            .size(12)
            .color(p.muted)
            .into(),
        Some((true, message)) => text(message.as_str()).size(12).color(p.accent).into(),
        Some((false, message)) => text(message.as_str())
            .size(12)
            .color(se_theme::error_color(dark))
            .into(),
        None if state.connections.is_empty() => text(t!("db.console_no_connection").to_string())
            .size(12)
            .color(p.muted)
            .into(),
        None => {
            // The completion knows all the columns, or says it does not.
            let partial = c.connection.is_some_and(|id| {
                state
                    .columns
                    .get(&id)
                    .is_some_and(|k| k.len() >= protocol::MAX_COMPLETION_COLUMNS)
            });
            text(if partial {
                t!("db.completion_partial").to_string()
            } else {
                t!("db.console_shortcuts").to_string()
            })
            .size(12)
            .color(if partial {
                se_theme::error_color(dark)
            } else {
                p.muted
            })
            .into()
        }
    };
    // One tab for each console.
    let mut tabs = row![].spacing(2).align_y(Alignment::Center);
    for (at, doc) in c.docs.iter().enumerate() {
        let mut tab = row![button(text(doc.title.as_str()).size(12))
            .padding([2, 8])
            .on_press(Message::Db(DbMsg::SwitchConsole(at)))
            .style(ghost(dark, at == c.active))]
        .align_y(Alignment::Center);
        if c.docs.len() > 1 {
            tab = tab.push(
                button(text("×").size(12))
                    .padding([0, 4])
                    .on_press(Message::Db(DbMsg::CloseConsole(at)))
                    .style(ghost(dark, false)),
            );
        }
        tabs = tabs.push(tab);
    }
    tabs = tabs.push(
        button(text("+").size(13))
            .padding([0, 8])
            .on_press(Message::Db(DbMsg::NewConsole))
            .style(ghost(dark, false)),
    );
    column![
        container(tabs).padding([2, 10]),
        editor,
        container(status).padding([4, 14])
    ]
    .height(Length::Fill)
    .into()
}

/// The proposals of the completion, the chosen one marked.
fn completion_list<'a>(
    completion: &'a super::console::Completion,
    dark: bool,
) -> Element<'a, Message> {
    use super::completion::ItemKind;
    let p = se_theme::palette(dark);
    let shown = 8;
    let from = completion.selected.saturating_sub(shown - 1);
    let mut list = column![].spacing(1);
    for (i, item) in completion
        .suggestions
        .items
        .iter()
        .enumerate()
        .skip(from)
        .take(shown)
    {
        let (glyph, color) = match item.kind {
            ItemKind::Keyword => ("K", p.muted),
            ItemKind::Function => ("ƒ", p.accent),
            ItemKind::Join => ("⋈", p.accent),
            ItemKind::Schema => ("S", p.muted),
            ItemKind::Table => ("▦", p.accent),
            ItemKind::View => ("◫", p.accent),
            ItemKind::Column => ("▏", p.accent),
        };
        list = list.push(
            button(
                row![
                    text(glyph).size(11).color(color).width(14),
                    text(item.label.as_str()).size(12.5).font(se_theme::MONO),
                    Space::new().width(Length::Fill),
                    text(item.detail.as_str()).size(11.5).color(p.muted),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            )
            .padding([3, 8])
            .width(Length::Fill)
            .on_press(Message::Db(DbMsg::CompletePick(i)))
            .style(ghost(dark, i == completion.selected)),
        );
    }
    container(list)
        .width(Length::Fixed(340.0))
        .padding(3)
        .style(se_theme::card(dark))
        .into()
}

/// What a right click on an entry of the log offers.
fn log_menu(at: usize, has_error: bool, dark: bool) -> Element<'static, Message> {
    use super::LogCopy;
    let mut items: Vec<Element<'static, Message>> = vec![
        menu_bar::item(
            t!("db.log_reuse").to_string(),
            None,
            dark,
            Message::Db(DbMsg::LogReuse(at)),
        ),
        menu_bar::item(
            t!("db.log_copy_sql").to_string(),
            None,
            dark,
            Message::Db(DbMsg::CopyLog(at, LogCopy::Sql)),
        ),
        menu_bar::item(
            t!("db.log_copy_entry").to_string(),
            None,
            dark,
            Message::Db(DbMsg::CopyLog(at, LogCopy::Entry)),
        ),
    ];
    if has_error {
        items.push(menu_bar::item(
            t!("db.log_copy_error").to_string(),
            None,
            dark,
            Message::Db(DbMsg::CopyLog(at, LogCopy::Error)),
        ));
    }
    container(column(items).spacing(1).padding(6).width(230))
        .style(se_theme::card(dark))
        .into()
}

/// What a right click on a table of the tree offers: open it, change it
/// (a table), delete all its rows (a table), drop it.
fn table_menu(
    target: (u64, String, String, String),
    kind: Kind,
    dialect: protocol::Dialect,
    dark: bool,
) -> Element<'static, Message> {
    let (id, database, schema, table) = target;
    // The designer, the import, the backups, dropping and emptying write
    // PostgreSQL; for the others the table opens and its definition shows.
    if dialect != protocol::Dialect::Postgres {
        return container(
            column![
                menu_bar::item(
                    t!("db.table_open").to_string(),
                    None,
                    dark,
                    Message::Db(DbMsg::OpenTable(
                        id,
                        database.clone(),
                        schema.clone(),
                        table.clone()
                    )),
                ),
                menu_bar::item(
                    t!("db.table_ddl").to_string(),
                    None,
                    dark,
                    Message::Db(DbMsg::ShowDefinition(id, database, schema, table, kind)),
                ),
            ]
            .spacing(1)
            .padding(6)
            .width(230),
        )
        .style(se_theme::card(dark))
        .into();
    }
    let mut items: Vec<Element<'static, Message>> = vec![menu_bar::item(
        t!("db.table_open").to_string(),
        None,
        dark,
        Message::Db(DbMsg::OpenTable(
            id,
            database.clone(),
            schema.clone(),
            table.clone(),
        )),
    )];
    if kind == Kind::Table {
        items.push(menu_bar::item(
            t!("db.table_modify").to_string(),
            None,
            dark,
            Message::Db(DbMsg::OpenDesigner(
                id,
                database.clone(),
                schema.clone(),
                table.clone(),
            )),
        ));
    }
    if kind == Kind::Table {
        items.push(menu_bar::item(
            t!("db.table_ddl").to_string(),
            None,
            dark,
            Message::Db(DbMsg::ShowDdl(
                id,
                database.clone(),
                schema.clone(),
                table.clone(),
            )),
        ));
    }
    if matches!(kind, Kind::View | Kind::Matview) {
        items.push(menu_bar::item(
            t!("db.table_ddl").to_string(),
            None,
            dark,
            Message::Db(DbMsg::ShowDefinition(
                id,
                database.clone(),
                schema.clone(),
                table.clone(),
                kind,
            )),
        ));
    }
    if kind == Kind::Table {
        items.push(menu_bar::item(
            t!("db.import").to_string(),
            None,
            dark,
            Message::Db(DbMsg::ImportOpen(
                id,
                database.clone(),
                schema.clone(),
                table.clone(),
            )),
        ));
    }
    if kind == Kind::Table {
        items.push(menu_bar::item(
            t!("db.backup").to_string(),
            None,
            dark,
            Message::Db(DbMsg::BackupOpen(
                id,
                database.clone(),
                schema.clone(),
                table.clone(),
                false,
            )),
        ));
    }
    if kind == Kind::Table {
        items.push(menu_bar::item(
            t!("db.copy_table").to_string(),
            None,
            dark,
            Message::Db(DbMsg::CopyOpen(
                id,
                database.clone(),
                schema.clone(),
                table.clone(),
            )),
        ));
    }
    items.push(menu_bar::separator(dark));
    if kind == Kind::Table {
        items.push(menu_bar::item(
            t!("db.table_empty").to_string(),
            None,
            dark,
            Message::Db(DbMsg::AskEmpty(
                id,
                database.clone(),
                schema.clone(),
                table.clone(),
            )),
        ));
    }
    let drop_label = match kind {
        Kind::Table => t!("db.table_drop"),
        Kind::View => t!("db.view_drop"),
        Kind::Matview => t!("db.matview_drop"),
        Kind::Foreign => t!("db.foreign_drop"),
    };
    items.push(menu_bar::item(
        drop_label.to_string(),
        None,
        dark,
        Message::Db(DbMsg::AskDrop(id, database, schema, table, kind)),
    ));
    container(column(items).spacing(1).padding(6).width(250))
        .style(se_theme::card(dark))
        .into()
}
