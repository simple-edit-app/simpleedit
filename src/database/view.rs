//! What the Database panel draws: the list in the sidebar, the settings of a
//! connection, and a table in its tab.

use std::fmt;
use std::path::Path;

use iced::widget::{
    button, checkbox, column, container, pick_list, row, scrollable, text, text_editor, text_input,
    tooltip, Space,
};
use iced::{Alignment, Element, Length};

use super::{
    Auth, Choice, Connection, DbMsg, Dialog, DialogTab, Driver, Field, Flag, Kind, Remember,
    SshAuth, SslMode, State, TableTab, TestState, Tree, ROW_HEIGHT,
};
use crate::app::Message;
use crate::theme as se_theme;

const TREE_ROW: f32 = 26.0;
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
    tooltip(
        button(text(glyph).size(13).color(muted))
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
        scrollable(list)
            .style(se_theme::Scrollbar { dark }.style())
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
    let open = state.open.contains(&key);
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
            out = out.push(note(error.clone(), se_theme::error_color(dark)));
        }
        Some(Tree::Loaded(databases)) if databases.iter().all(|d| d.schemas.is_empty()) => {
            out = out.push(note(t!("db.no_tables").to_string(), p.muted));
        }
        Some(Tree::Loaded(databases)) => {
            // The databases only show when there are several.
            let several = databases.len() > 1;
            for database in databases {
                let database_key = format!("{id}/{}", database.name);
                if several {
                    let database_open = state.open.contains(&database_key);
                    out = out.push(node(
                        1,
                        if database_open { "▾" } else { "▸" },
                        database.name.clone(),
                        Message::Db(DbMsg::Toggle(database_key.clone())),
                        dark,
                    ));
                    if !database_open {
                        continue;
                    }
                }
                let depth = if several { 2 } else { 1 };
                for schema in &database.schemas {
                    let schema_key = format!("{database_key}/{}", schema.name);
                    let schema_open = state.open.contains(&schema_key);
                    out = out.push(node(
                        depth,
                        if schema_open { "▾" } else { "▸" },
                        schema.name.clone(),
                        Message::Db(DbMsg::Toggle(schema_key)),
                        dark,
                    ));
                    if !schema_open {
                        continue;
                    }
                    for table in &schema.tables {
                        let glyph = match table.kind {
                            Kind::Table => "▦",
                            Kind::View => "◫",
                            Kind::Matview => "◨",
                            Kind::Foreign => "▤",
                        };
                        out = out.push(node(
                            depth + 1,
                            glyph,
                            table.name.clone(),
                            Message::Db(DbMsg::OpenTable(
                                id,
                                database.name.clone(),
                                schema.name.clone(),
                                table.name.clone(),
                            )),
                            dark,
                        ));
                    }
                }
            }
        }
    }
    out.into()
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

    let driver_ready = state.driver == Driver::Ready;
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
                    text(t!("db.delete").to_string())
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
    form.push(driver_line(state, dark)).into()
}

/// The state of the driver and what to do about it.
fn driver_line<'a>(state: &'a State, dark: bool) -> Element<'a, Message> {
    let p = se_theme::palette(dark);
    let (label, color, action): (String, iced::Color, Option<String>) = match &state.driver {
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
                .on_press(Message::Db(DbMsg::DownloadDriver))
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
    let mut form = column![
        driver_line(state, dark),
        labeled(
            t!("db.driver_protocol").to_string(),
            text(super::DRIVER_PROTOCOL.to_string()).size(12.5),
            dark
        ),
        labeled(
            t!("db.driver_location").to_string(),
            text(super::downloaded_driver().display().to_string())
                .size(12)
                .font(se_theme::MONO)
                .color(p.muted),
            dark
        ),
    ]
    .spacing(10);
    if matches!(state.driver, Driver::Ready) {
        form = form.push(
            button(text(t!("db.driver_redownload").to_string()).size(12.5))
                .padding([5, 12])
                .on_press(Message::Db(DbMsg::DownloadDriver))
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

/// A table in its tab: a bar, then the grid.
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
    let bar = row![
        text(format!("{}.{}", tab.schema, tab.table))
            .size(13)
            .font(se_theme::UI_SEMIBOLD),
        text(range).size(12).color(p.muted),
        Space::new().width(Length::Fill),
        nav(
            "‹",
            t!("db.prev").to_string(),
            has_prev.then(|| Message::Db(DbMsg::Turn(path.to_path_buf(), -1))),
        ),
        nav(
            "›",
            t!("db.next").to_string(),
            has_next.then(|| Message::Db(DbMsg::Turn(path.to_path_buf(), 1))),
        ),
        nav(
            "⟳",
            t!("db.reload").to_string(),
            (!tab.loading).then(|| Message::Db(DbMsg::Refresh(path.to_path_buf()))),
        ),
    ]
    .spacing(10)
    .padding([8, 12])
    .align_y(Alignment::Center);

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

    container(
        column![
            bar,
            container(Space::new().height(1))
                .width(Length::Fill)
                .style(se_theme::rule(dark)),
            body,
        ]
        .height(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .style(se_theme::window(dark))
    .into()
}

/// The header, then the rows in view: only those are made, the others are
/// space. The header stays in place when the rows scroll up and down.
fn grid<'a>(path: &'a Path, tab: &'a TableTab, dark: bool) -> Element<'a, Message> {
    let p = se_theme::palette(dark);
    let Some(page) = &tab.page else {
        return Space::new().into();
    };
    let total_width: f32 = tab.widths.iter().sum::<f32>() + 56.0;
    let cell = |content: Element<'a, Message>, width: f32| {
        container(content)
            .width(Length::Fixed(width))
            .height(Length::Fixed(ROW_HEIGHT))
            .padding([0, 10])
            .center_y(Length::Fixed(ROW_HEIGHT))
    };

    // The line number is the row's place in the table.
    let gutter = |label: String| {
        container(text(label).size(11.5).color(p.gutter))
            .width(Length::Fixed(56.0))
            .height(Length::Fixed(ROW_HEIGHT))
            .padding([0, 10])
            .center_y(Length::Fixed(ROW_HEIGHT))
            .align_x(iced::alignment::Horizontal::Right)
    };

    let mut header = row![gutter(String::new())];
    for (column, width) in page.columns.iter().zip(&tab.widths) {
        header = header.push(cell(
            row![
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
            .align_y(Alignment::Center)
            .into(),
            *width,
        ));
    }
    let header = container(header).style(move |_: &iced::Theme| container::Style {
        background: Some(iced::Background::Color(p.elevated)),
        ..container::Style::default()
    });

    let count = page.rows.len();
    let (offset, height) = tab.scroll;
    let first = ((offset / ROW_HEIGHT) as usize).saturating_sub(6);
    let last = (((offset + height) / ROW_HEIGHT) as usize + 6).min(count);
    let mut rows = column![Space::new().height(first as f32 * ROW_HEIGHT)];
    for (i, values) in page.rows.iter().enumerate().take(last).skip(first) {
        let mut line = row![gutter((page.offset + i as u64 + 1).to_string())];
        for (value, width) in values.iter().zip(&tab.widths) {
            let content: Element<Message> = match value {
                Some(text_value) => text(text_value.as_str())
                    .size(12.5)
                    .wrapping(iced::widget::text::Wrapping::None)
                    .into(),
                None => text("NULL")
                    .size(11.5)
                    .color(p.muted)
                    .font(se_theme::UI_ITALIC)
                    .into(),
            };
            line = line.push(cell(content, *width));
        }
        rows = rows.push(
            container(line).style(move |_: &iced::Theme| container::Style {
                background: (i % 2 == 1).then_some(iced::Background::Color(iced::Color {
                    a: 0.35,
                    ..p.elevated
                })),
                ..container::Style::default()
            }),
        );
    }
    rows = rows.push(Space::new().height(count.saturating_sub(last) as f32 * ROW_HEIGHT));

    let path_owned = path.to_path_buf();
    let vertical = scrollable(rows)
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

    scrollable(column![header, vertical].width(Length::Fixed(total_width)))
        .direction(scrollable::Direction::Horizontal(
            scrollable::Scrollbar::new(),
        ))
        .style(se_theme::Scrollbar { dark }.style())
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
