//! The popins that work on a table: the question before a drop or a
//! deletion, and the table designer.

use iced::widget::{
    button, checkbox, column, container, pick_list, row, scrollable, text, text_input, tooltip,
    Space,
};
use iced::{Alignment, Element, Length};

use super::designer::{Designer, DesignerTab, Load, Problem, COMMON_TYPES, ON_DELETE};
use super::protocol::ConstraintKind;
use super::{ColField, ConField, Confirm, ConfirmAction, DbMsg, DesignerMsg, IdxField, Kind};
use crate::app::Message;
use crate::theme as se_theme;

fn ghost(dark: bool) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    (se_theme::GhostButton {
        dark,
        active: false,
    })
    .style()
}

fn say(msg: DesignerMsg) -> Message {
    Message::Db(DbMsg::Designer(msg))
}

/// A text field that sends what is typed.
fn field<'a>(
    value: &str,
    placeholder: &str,
    on_input: impl Fn(String) -> Message + 'a,
    dark: bool,
) -> text_input::TextInput<'a, Message> {
    text_input(placeholder, value)
        .on_input(on_input)
        .size(12.5)
        .padding([4, 8])
        .style(se_theme::Field::new(dark).style())
}

fn icon<'a>(
    glyph: &'static str,
    tip: String,
    message: Message,
    dark: bool,
) -> Element<'a, Message> {
    tooltip(
        button(text(glyph).size(14))
            .padding([2, 8])
            .on_press(message)
            .style(ghost(dark)),
        container(text(tip).size(12))
            .padding([4, 8])
            .style(se_theme::tooltip(dark)),
        tooltip::Position::Bottom,
    )
    .into()
}

fn head<'a>(title: String, close: Message, dark: bool) -> Element<'a, Message> {
    let p = se_theme::palette(dark);
    row![
        text(title)
            .size(13.5)
            .font(se_theme::UI_SEMIBOLD)
            .color(p.text),
        Space::new().width(Length::Fill),
        button(text("×").size(16).color(p.muted))
            .padding([0, 7])
            .on_press(close)
            .style(ghost(dark)),
    ]
    .align_y(Alignment::Center)
    .into()
}

// ------------------------------------------------------------------ confirm

fn kind_name(kind: Kind) -> String {
    match kind {
        Kind::Table => t!("db.kind_table"),
        Kind::View => t!("db.kind_view"),
        Kind::Matview => t!("db.kind_matview"),
        Kind::Foreign => t!("db.kind_foreign"),
    }
    .to_string()
}

/// The question before a drop, or before all the rows of a table go.
pub fn confirm<'a>(c: &'a Confirm, dark: bool) -> Element<'a, Message> {
    let p = se_theme::palette(dark);
    let name = format!("{}.{}", c.schema, c.table);
    let (title, message, action) = match c.action {
        ConfirmAction::Drop => (
            t!("db.confirm_drop_title", name = name.as_str()).to_string(),
            t!("db.confirm_drop_text", kind = kind_name(c.kind).as_str()).to_string(),
            t!("db.confirm_drop_button").to_string(),
        ),
        ConfirmAction::Empty => (
            t!("db.confirm_empty_title", name = name.as_str()).to_string(),
            t!("db.confirm_empty_text").to_string(),
            t!("db.confirm_empty_button").to_string(),
        ),
    };
    let mut body = column![
        head(title, Message::Db(DbMsg::ConfirmClose), dark),
        text(message).size(12.5).color(p.muted),
        // What will be run, to be sure of it.
        container(text(c.statement()).size(12.5).font(se_theme::MONO))
            .padding([6, 10])
            .width(Length::Fill)
            .style(se_theme::rule(dark)),
    ]
    .spacing(10);
    if c.action == ConfirmAction::Drop {
        body = body.push(
            checkbox(c.cascade)
                .label(t!("db.confirm_cascade").to_string())
                .size(14)
                .text_size(12.5)
                .on_toggle(|on| Message::Db(DbMsg::ConfirmCascade(on))),
        );
    }
    if let Some(error) = &c.error {
        body = body.push(
            text(error.as_str())
                .size(12.5)
                .color(se_theme::error_color(dark)),
        );
    }
    body = body.push(
        row![
            button(text(action).size(12.5))
                .padding([5, 16])
                .on_press_maybe((!c.running).then_some(Message::Db(DbMsg::ConfirmRun)))
                .style((se_theme::Primary { dark }).style()),
            text(if c.running {
                t!("db.confirm_running").to_string()
            } else {
                String::new()
            })
            .size(12)
            .color(p.muted),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    );
    container(body.width(Length::Fixed(480.0)))
        .padding(16)
        .style(se_theme::card(dark))
        .into()
}

// ----------------------------------------------------------------- designer

fn tab_label(tab: DesignerTab) -> String {
    match tab {
        DesignerTab::Columns => t!("db.dtab_columns"),
        DesignerTab::Constraints => t!("db.dtab_constraints"),
        DesignerTab::Indexes => t!("db.dtab_indexes"),
        DesignerTab::General => t!("db.dtab_general"),
        DesignerTab::Sql => t!("db.dtab_sql"),
    }
    .to_string()
}

fn problem_text(problem: &Problem) -> String {
    match problem {
        Problem::EmptyTableName => t!("db.dp_empty_table").to_string(),
        Problem::EmptySchema => t!("db.dp_empty_schema").to_string(),
        Problem::NoColumns => t!("db.dp_no_columns").to_string(),
        Problem::EmptyColumnName => t!("db.dp_empty_column").to_string(),
        Problem::DuplicateColumn(name) => t!("db.dp_duplicate", name = name.as_str()).to_string(),
        Problem::EmptyType(name) => t!("db.dp_empty_type", name = name.as_str()).to_string(),
        Problem::NeedsColumns => t!("db.dp_needs_columns").to_string(),
        Problem::NeedsReference => t!("db.dp_needs_reference").to_string(),
        Problem::NeedsExpression => t!("db.dp_needs_expression").to_string(),
        Problem::SeveralKeys => t!("db.dp_several_keys").to_string(),
    }
}

fn constraint_label(kind: ConstraintKind) -> String {
    match kind {
        ConstraintKind::Primary => t!("db.dcon_primary"),
        ConstraintKind::Unique => t!("db.dcon_unique"),
        ConstraintKind::Foreign => t!("db.dcon_foreign"),
        ConstraintKind::Check => t!("db.dcon_check"),
        ConstraintKind::Other => t!("db.dcon_other"),
    }
    .to_string()
}

/// The popin to change a table: its columns, keys and constraints, indexes,
/// name and comment, and the SQL that comes of it.
pub fn designer<'a>(d: &'a Designer, dark: bool) -> Element<'a, Message> {
    let p = se_theme::palette(dark);
    let mut tabs = row![].spacing(2);
    for tab in DesignerTab::ALL {
        tabs = tabs.push(
            button(text(tab_label(tab)).size(12.5))
                .padding([5, 12])
                .on_press(say(DesignerMsg::Tab(tab)))
                .style(
                    (se_theme::GhostButton {
                        dark,
                        active: d.tab == tab,
                    })
                    .style(),
                ),
        );
    }
    let content: Element<Message> = if d.load == Load::Loading {
        text(t!("db.designer_loading").to_string())
            .size(12.5)
            .color(p.muted)
            .into()
    } else {
        match d.tab {
            DesignerTab::Columns => columns_tab(d, dark),
            DesignerTab::Constraints => constraints_tab(d, dark),
            DesignerTab::Indexes => indexes_tab(d, dark),
            DesignerTab::General => general_tab(d, dark),
            DesignerTab::Sql => sql_tab(d, dark),
        }
    };

    let problem = d.problem();
    let changes = d.has_changes();
    let status: Element<Message> = match (&d.error, &problem) {
        (Some(error), _) => text(error.as_str())
            .size(12)
            .color(se_theme::error_color(dark))
            .into(),
        (None, Some(problem)) if changes => text(problem_text(problem))
            .size(12)
            .color(se_theme::error_color(dark))
            .into(),
        _ if d.load == Load::Running => text(t!("db.confirm_running").to_string())
            .size(12)
            .color(p.muted)
            .into(),
        _ => Space::new().into(),
    };
    let ready = d.load == Load::Ready && changes && problem.is_none();
    let footer = row![
        button(
            text(if d.creating {
                t!("db.dcreate").to_string()
            } else {
                t!("db.dapply").to_string()
            })
            .size(12.5)
        )
        .padding([5, 18])
        .on_press_maybe(ready.then_some(say(DesignerMsg::Apply)))
        .style((se_theme::Primary { dark }).style()),
        status,
    ]
    .spacing(12)
    .align_y(Alignment::Center);

    let full_name = format!("{}.{}", d.schema, d.table);
    let title = if d.creating {
        t!("db.designer_new_title", schema = d.schema.as_str()).to_string()
    } else {
        t!("db.designer_title", name = full_name.as_str()).to_string()
    };
    let rule = || {
        container(Space::new().height(1))
            .width(Length::Fill)
            .style(se_theme::rule(dark))
    };
    container(
        column![
            head(title, say(DesignerMsg::Close), dark),
            tabs,
            rule(),
            scrollable(container(content).padding(iced::Padding {
                right: 12.0,
                ..iced::Padding::ZERO
            }))
            .style(se_theme::Scrollbar { dark }.style())
            .height(Length::Fixed(440.0)),
            rule(),
            footer,
        ]
        .spacing(10)
        .width(Length::Fixed(960.0)),
    )
    .padding(16)
    .style(se_theme::card(dark))
    .into()
}

fn columns_tab<'a>(d: &'a Designer, dark: bool) -> Element<'a, Message> {
    let p = se_theme::palette(dark);
    let heading = |label: String, width: Length| text(label).size(11.5).color(p.muted).width(width);
    let mut list = column![row![
        heading(t!("db.dcol_name").to_string(), Length::Fixed(170.0)),
        heading(t!("db.dcol_type").to_string(), Length::Fixed(184.0)),
        heading(t!("db.dcol_notnull").to_string(), Length::Fixed(70.0)),
        heading(t!("db.dcol_default").to_string(), Length::Fixed(160.0)),
        heading(t!("db.dcol_comment").to_string(), Length::Fill),
    ]
    .spacing(8)]
    .spacing(6);
    for (i, c) in d.columns.iter().enumerate() {
        if c.dropped {
            list = list.push(
                row![
                    text(c.name.as_str()).size(12.5).color(p.muted).width(170),
                    text(c.kind.as_str()).size(12).color(p.muted),
                    text(t!("db.ddropped").to_string())
                        .size(11.5)
                        .color(se_theme::error_color(dark)),
                    Space::new().width(Length::Fill),
                    icon(
                        "↺",
                        t!("db.drestore").to_string(),
                        say(DesignerMsg::ColDrop(i, false)),
                        dark
                    ),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            );
            continue;
        }
        let special = c
            .original
            .as_ref()
            .is_some_and(|o| o.generated || !o.identity.is_empty());
        let default: Element<Message> = if special {
            text(t!("db.dcol_managed").to_string())
                .size(11.5)
                .color(p.muted)
                .width(160)
                .into()
        } else {
            field(
                &c.default,
                "",
                move |t| say(DesignerMsg::Col(i, ColField::Default, t)),
                dark,
            )
            .width(160)
            .into()
        };
        let using = c.retyped().then(|| {
            row![
                text(t!("db.dcol_using").to_string())
                    .size(11.5)
                    .color(p.muted),
                field(
                    &c.using,
                    &format!("\"{}\"::{}", c.name.trim(), c.kind.trim()),
                    move |t| say(DesignerMsg::Col(i, ColField::Using, t)),
                    dark,
                )
                .font(se_theme::MONO)
                .width(Length::Fill),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
        });
        list = list.push(
            row![
                field(
                    &c.name,
                    "",
                    move |t| say(DesignerMsg::Col(i, ColField::Name, t)),
                    dark
                )
                .width(170),
                field(
                    &c.kind,
                    "",
                    move |t| say(DesignerMsg::Col(i, ColField::Type, t)),
                    dark
                )
                .font(se_theme::MONO)
                .width(140),
                pick_list(COMMON_TYPES.to_vec(), None::<&str>, move |t| {
                    say(DesignerMsg::Col(i, ColField::Type, t.to_string()))
                })
                .placeholder("")
                .text_size(12)
                .padding([4, 6])
                .width(36)
                .style(se_theme::Select { dark }.style())
                .menu_style(se_theme::Select { dark }.menu_style()),
                container(
                    checkbox(!c.nullable)
                        .size(14)
                        .on_toggle(move |on| say(DesignerMsg::ColNull(i, !on)))
                )
                .width(70)
                .center_x(Length::Fixed(70.0)),
                default,
                field(
                    &c.comment,
                    "",
                    move |t| say(DesignerMsg::Col(i, ColField::Comment, t)),
                    dark
                )
                .width(Length::Fill),
                icon(
                    "×",
                    t!("db.ddrop").to_string(),
                    say(DesignerMsg::ColDrop(i, true)),
                    dark
                ),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        );
        if let Some(using) = using {
            list = list.push(using);
        }
    }
    list.push(
        button(text(t!("db.dcol_add").to_string()).size(12.5))
            .padding([5, 12])
            .on_press(say(DesignerMsg::ColAdd))
            .style((se_theme::Outline { dark }).style()),
    )
    .push(
        // PostgreSQL has no way to move a column: say it rather than hide it.
        text(t!("db.dcol_order_note").to_string())
            .size(11.5)
            .color(p.muted),
    )
    .into()
}

fn constraints_tab<'a>(d: &'a Designer, dark: bool) -> Element<'a, Message> {
    let p = se_theme::palette(dark);
    let mut list = column![].spacing(8);
    for (i, c) in d.constraints.iter().enumerate() {
        if let Some(original) = c.original.as_ref().filter(|_| c.dropped || !c.editable()) {
            let color = if c.dropped { p.muted } else { p.text };
            let mut line = row![
                text(constraint_label(c.kind))
                    .size(12)
                    .color(p.accent)
                    .width(110),
                text(original.name.as_str())
                    .size(12.5)
                    .color(color)
                    .width(220),
                text(original.definition.as_str())
                    .size(12)
                    .font(se_theme::MONO)
                    .color(p.muted)
                    .width(Length::Fill),
            ]
            .spacing(8)
            .align_y(Alignment::Center);
            line = line.push(if c.dropped {
                icon(
                    "↺",
                    t!("db.drestore").to_string(),
                    say(DesignerMsg::ConDrop(i, false)),
                    dark,
                )
            } else {
                icon(
                    "×",
                    t!("db.ddrop").to_string(),
                    say(DesignerMsg::ConDrop(i, true)),
                    dark,
                )
            });
            list = list.push(line);
            continue;
        }
        // A new one, described; one that exists, edited from its definition.
        let mut line = row![
            text(constraint_label(c.kind))
                .size(12)
                .color(p.accent)
                .width(110),
            field(
                &c.name,
                &t!("db.dcon_name"),
                move |t| say(DesignerMsg::Con(i, ConField::Name, t)),
                dark
            )
            .width(190),
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        let columns = || {
            field(
                &c.columns,
                &t!("db.dcon_columns"),
                move |t| say(DesignerMsg::Con(i, ConField::Columns, t)),
                dark,
            )
            .width(Length::FillPortion(2))
        };
        match c.kind {
            ConstraintKind::Primary | ConstraintKind::Unique | ConstraintKind::Other => {
                line = line.push(columns());
            }
            ConstraintKind::Foreign => {
                line = line
                    .push(columns())
                    .push(
                        field(
                            &c.reference,
                            &t!("db.dcon_reference"),
                            move |t| say(DesignerMsg::Con(i, ConField::Reference, t)),
                            dark,
                        )
                        .width(Length::FillPortion(2)),
                    )
                    .push(
                        field(
                            &c.ref_columns,
                            &t!("db.dcon_ref_columns"),
                            move |t| say(DesignerMsg::Con(i, ConField::RefColumns, t)),
                            dark,
                        )
                        .width(Length::FillPortion(1)),
                    )
                    .push(
                        pick_list(ON_DELETE.to_vec(), Some(c.on_delete.as_str()), move |t| {
                            say(DesignerMsg::Con(i, ConField::OnDelete, t.to_string()))
                        })
                        .text_size(12)
                        .padding([4, 6])
                        .style(se_theme::Select { dark }.style())
                        .menu_style(se_theme::Select { dark }.menu_style()),
                    );
            }
            ConstraintKind::Check => {
                line = line.push(
                    field(
                        &c.expr,
                        &t!("db.dcon_expr"),
                        move |t| say(DesignerMsg::Con(i, ConField::Expr, t)),
                        dark,
                    )
                    .font(se_theme::MONO)
                    .width(Length::Fill),
                );
            }
        }
        line = line.push(icon(
            "×",
            if c.original.is_some() {
                t!("db.ddrop").to_string()
            } else {
                t!("db.dremove").to_string()
            },
            say(DesignerMsg::ConDrop(i, true)),
            dark,
        ));
        list = list.push(line);
    }
    let add = |label: String, kind: ConstraintKind| {
        button(text(label).size(12.5))
            .padding([5, 12])
            .on_press(say(DesignerMsg::ConAdd(kind)))
            .style((se_theme::Outline { dark }).style())
    };
    list.push(
        row![
            add(
                t!("db.dcon_add_primary").to_string(),
                ConstraintKind::Primary
            ),
            add(t!("db.dcon_add_unique").to_string(), ConstraintKind::Unique),
            add(
                t!("db.dcon_add_foreign").to_string(),
                ConstraintKind::Foreign
            ),
            add(t!("db.dcon_add_check").to_string(), ConstraintKind::Check),
        ]
        .spacing(8),
    )
    .into()
}

fn indexes_tab<'a>(d: &'a Designer, dark: bool) -> Element<'a, Message> {
    let p = se_theme::palette(dark);
    let mut list = column![].spacing(8);
    for (i, x) in d.indexes.iter().enumerate() {
        if let Some(original) = x.original.as_ref().filter(|_| x.dropped || !x.editable()) {
            let color = if x.dropped { p.muted } else { p.text };
            list = list.push(
                row![
                    text(original.name.as_str())
                        .size(12.5)
                        .color(color)
                        .width(220),
                    text(original.definition.as_str())
                        .size(12)
                        .font(se_theme::MONO)
                        .color(p.muted)
                        .width(Length::Fill),
                    if x.dropped {
                        icon(
                            "↺",
                            t!("db.drestore").to_string(),
                            say(DesignerMsg::IdxDrop(i, false)),
                            dark,
                        )
                    } else {
                        icon(
                            "×",
                            t!("db.ddrop").to_string(),
                            say(DesignerMsg::IdxDrop(i, true)),
                            dark,
                        )
                    },
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            );
            continue;
        }
        list = list.push(
            row![
                field(
                    &x.name,
                    &t!("db.dcon_name"),
                    move |t| say(DesignerMsg::Idx(i, IdxField::Name, t)),
                    dark
                )
                .width(220),
                field(
                    &x.columns,
                    &t!("db.dcon_columns"),
                    move |t| say(DesignerMsg::Idx(i, IdxField::Columns, t)),
                    dark
                )
                .width(Length::Fill),
                checkbox(x.unique)
                    .label(t!("db.didx_unique").to_string())
                    .size(14)
                    .text_size(12.5)
                    .on_toggle(move |on| say(DesignerMsg::IdxUnique(i, on))),
                icon(
                    "×",
                    if x.original.is_some() {
                        t!("db.ddrop").to_string()
                    } else {
                        t!("db.dremove").to_string()
                    },
                    say(DesignerMsg::IdxDrop(i, true)),
                    dark
                ),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        );
    }
    list.push(
        button(text(t!("db.didx_add").to_string()).size(12.5))
            .padding([5, 12])
            .on_press(say(DesignerMsg::IdxAdd))
            .style((se_theme::Outline { dark }).style()),
    )
    .into()
}

fn general_tab<'a>(d: &'a Designer, dark: bool) -> Element<'a, Message> {
    let p = se_theme::palette(dark);
    let labeled = |label: String, content: Element<'a, Message>| -> Element<'a, Message> {
        row![text(label).size(12.5).color(p.muted).width(150), content]
            .spacing(8)
            .align_y(Alignment::Center)
            .into()
    };
    column![
        labeled(
            t!("db.dgen_schema").to_string(),
            field(&d.schema_to, "", |t| say(DesignerMsg::TableSchema(t)), dark).into()
        ),
        labeled(
            t!("db.dgen_name").to_string(),
            field(&d.name, "", |t| say(DesignerMsg::TableName(t)), dark).into()
        ),
        labeled(
            t!("db.dgen_comment").to_string(),
            field(&d.comment, "", |t| say(DesignerMsg::TableComment(t)), dark).into()
        ),
    ]
    .spacing(10)
    .into()
}

fn sql_tab<'a>(d: &'a Designer, dark: bool) -> Element<'a, Message> {
    let p = se_theme::palette(dark);
    let statements = d.statements();
    if statements.is_empty() {
        return text(t!("db.dsql_none").to_string())
            .size(12.5)
            .color(p.muted)
            .into();
    }
    let mut list = column![text(t!("db.dsql_hint").to_string()).size(12).color(p.muted)].spacing(8);
    for statement in statements {
        list = list.push(
            text(format!("{statement};"))
                .size(12.5)
                .font(se_theme::MONO),
        );
    }
    list.into()
}

// -------------------------------------------------------------- text, schema

/// A text shown whole, to read and copy: the DDL of a table, a long value.
pub fn text_view<'a>(view: &'a super::TextView, dark: bool) -> Element<'a, Message> {
    let p = se_theme::palette(dark);
    let body = scrollable(
        container(
            text(view.text.as_str())
                .size(12.5)
                .font(se_theme::MONO)
                .color(p.text),
        )
        .padding(iced::Padding {
            right: 12.0,
            ..iced::Padding::ZERO
        }),
    )
    .style(se_theme::Scrollbar { dark }.style())
    .direction(scrollable::Direction::Both {
        vertical: scrollable::Scrollbar::default(),
        horizontal: scrollable::Scrollbar::default(),
    })
    .width(Length::Fill)
    .height(Length::Fixed(380.0));
    container(
        column![
            head(view.title.clone(), Message::Db(DbMsg::TextClose), dark),
            container(body)
                .padding(10)
                .width(Length::Fill)
                .style(se_theme::rule(dark)),
            row![
                button(text(t!("db.text_copy").to_string()).size(12.5))
                    .padding([5, 16])
                    .on_press(Message::Db(DbMsg::TextCopy))
                    .style((se_theme::Primary { dark }).style()),
                text(t!("db.text_size", n = view.text.chars().count()).to_string())
                    .size(12)
                    .color(p.muted),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        ]
        .spacing(10)
        .width(Length::Fixed(720.0)),
    )
    .padding(16)
    .style(se_theme::card(dark))
    .into()
}

/// The question to make a schema: its name.
pub fn new_schema<'a>(n: &'a super::NewSchema, dark: bool) -> Element<'a, Message> {
    let p = se_theme::palette(dark);
    let mut body = column![
        head(
            t!("db.new_schema_title").to_string(),
            Message::Db(DbMsg::SchemaClose),
            dark
        ),
        text_input(&t!("db.new_schema_name"), &n.name)
            .on_input(|t| Message::Db(DbMsg::SchemaName(t)))
            .on_submit(Message::Db(DbMsg::SchemaCreate))
            .size(13)
            .padding([6, 10])
            .style(se_theme::Field::new(dark).style()),
    ]
    .spacing(10);
    if let Some(error) = &n.error {
        body = body.push(
            text(error.as_str())
                .size(12.5)
                .color(se_theme::error_color(dark)),
        );
    }
    body = body.push(
        row![
            button(text(t!("db.dcreate").to_string()).size(12.5))
                .padding([5, 16])
                .on_press_maybe(
                    (!n.running && !n.name.trim().is_empty())
                        .then_some(Message::Db(DbMsg::SchemaCreate))
                )
                .style((se_theme::Primary { dark }).style()),
            text(if n.running {
                t!("db.confirm_running").to_string()
            } else {
                String::new()
            })
            .size(12)
            .color(p.muted),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    );
    container(body.width(Length::Fixed(420.0)))
        .padding(16)
        .style(se_theme::card(dark))
        .into()
}
