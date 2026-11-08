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

/// How a database is doing: its tables with their sizes and what waits to be
/// cleaned, and its indexes with how much they are read.
pub fn health<'a>(view: &'a super::HealthView, dark: bool) -> Element<'a, Message> {
    use super::protocol::human_size;
    use super::HealthTab;
    let p = se_theme::palette(dark);
    let cell = |label: String, width: f32| text(label).size(12).width(Length::Fixed(width));
    let muted = |label: String, width: f32| {
        text(label)
            .size(12)
            .color(p.muted)
            .width(Length::Fixed(width))
    };
    let mut head_line =
        row![
            text(t!("db.health_title", name = view.database.as_str()).to_string())
                .size(13.5)
                .font(se_theme::UI_SEMIBOLD),
        ]
        .spacing(12)
        .align_y(Alignment::Center);
    if let Some(data) = &view.data {
        head_line = head_line.push(
            text(human_size(data.database_size))
                .size(12.5)
                .color(p.muted),
        );
    }
    head_line = head_line
        .push(Space::new().width(Length::Fill))
        .push(
            button(text("⟳").size(13))
                .padding([2, 9])
                .on_press_maybe(
                    (!view.loading && view.running.is_none())
                        .then(|| Message::Db(DbMsg::OpenHealth(view.conn, view.database.clone()))),
                )
                .style((se_theme::Outline { dark }).style()),
        )
        .push(
            button(text("×").size(16).color(p.muted))
                .padding([0, 7])
                .on_press(Message::Db(DbMsg::HealthClose))
                .style(ghost(dark)),
        );
    let tabs = row![
        button(text(t!("db.health_tables").to_string()).size(12.5))
            .padding([3, 12])
            .on_press(Message::Db(DbMsg::HealthTab(HealthTab::Tables)))
            .style(
                (se_theme::GhostButton {
                    dark,
                    active: view.tab == HealthTab::Tables,
                })
                .style(),
            ),
        button(text(t!("db.health_indexes").to_string()).size(12.5))
            .padding([3, 12])
            .on_press(Message::Db(DbMsg::HealthTab(HealthTab::Indexes)))
            .style(
                (se_theme::GhostButton {
                    dark,
                    active: view.tab == HealthTab::Indexes,
                })
                .style(),
            ),
    ]
    .spacing(4);
    let mut list = column![].spacing(2);
    match (&view.data, view.tab) {
        (None, _) => {
            list = list.push(
                text(if view.loading {
                    t!("db.loading").to_string()
                } else {
                    String::new()
                })
                .size(12.5)
                .color(p.muted),
            );
        }
        (Some(data), HealthTab::Tables) => {
            list = list.push(
                row![
                    muted(t!("db.health_table").to_string(), 190.0),
                    muted(t!("db.health_rows").to_string(), 80.0),
                    muted(t!("db.health_size").to_string(), 80.0),
                    muted(t!("db.health_idx_size").to_string(), 80.0),
                    muted(t!("db.health_dead").to_string(), 70.0),
                    muted(t!("db.health_vacuum").to_string(), 120.0),
                    muted(t!("db.health_analyze").to_string(), 120.0),
                ]
                .spacing(8),
            );
            for t in &data.tables {
                let qualified = format!(
                    "{}.{}",
                    super::protocol::quote_ident(&t.schema),
                    super::protocol::quote_ident(&t.name)
                );
                // Many dead rows for the size of the table: worth a clean-up.
                let dirty = t.dead_rows > 1000 && t.dead_rows * 5 > t.rows.max(1);
                let run = |verb: &str| {
                    (view.running.is_none())
                        .then(|| Message::Db(DbMsg::HealthRun(format!("{verb} {qualified}"))))
                };
                list = list.push(
                    row![
                        cell(format!("{}.{}", t.schema, t.name), 190.0),
                        cell(t.rows.to_string(), 80.0),
                        cell(human_size(t.total_size), 80.0),
                        cell(human_size(t.indexes_size), 80.0),
                        text(t.dead_rows.to_string())
                            .size(12)
                            .width(Length::Fixed(70.0))
                            .color(if dirty {
                                se_theme::error_color(dark)
                            } else {
                                p.text
                            }),
                        muted(t.last_vacuum.clone(), 120.0),
                        muted(t.last_analyze.clone(), 120.0),
                        button(text("VACUUM").size(11.5))
                            .padding([1, 8])
                            .on_press_maybe(run("VACUUM"))
                            .style((se_theme::Outline { dark }).style()),
                        button(text("ANALYZE").size(11.5))
                            .padding([1, 8])
                            .on_press_maybe(run("ANALYZE"))
                            .style((se_theme::Outline { dark }).style()),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                );
            }
        }
        (Some(data), HealthTab::Indexes) => {
            list = list.push(
                row![
                    muted(t!("db.health_index").to_string(), 220.0),
                    muted(t!("db.health_table").to_string(), 170.0),
                    muted(t!("db.health_size").to_string(), 80.0),
                    muted(t!("db.health_scans").to_string(), 80.0),
                ]
                .spacing(8),
            );
            for i in &data.indexes {
                let unused = i.unused();
                let mut line = row![
                    text(i.name.as_str())
                        .size(12)
                        .width(Length::Fixed(220.0))
                        .color(if unused {
                            se_theme::error_color(dark)
                        } else {
                            p.text
                        }),
                    cell(format!("{}.{}", i.schema, i.table), 170.0),
                    cell(human_size(i.size), 80.0),
                    cell(i.scans.to_string(), 80.0),
                ]
                .spacing(8)
                .align_y(Alignment::Center);
                if unused {
                    line = line
                        .push(
                            text(t!("db.health_unused").to_string())
                                .size(11.5)
                                .color(se_theme::error_color(dark)),
                        )
                        .push(
                            button(text(t!("db.health_drop_in_console").to_string()).size(11.5))
                                .padding([1, 8])
                                .on_press(Message::Db(DbMsg::HealthToConsole(format!(
                                    "DROP INDEX {}.{};",
                                    super::protocol::quote_ident(&i.schema),
                                    super::protocol::quote_ident(&i.name)
                                ))))
                                .style((se_theme::Outline { dark }).style()),
                        );
                }
                list = list.push(line);
            }
        }
    }
    let mut body = column![
        head_line,
        tabs,
        scrollable(list)
            .style(se_theme::Scrollbar { dark }.style())
            .direction(scrollable::Direction::Both {
                vertical: scrollable::Scrollbar::default(),
                horizontal: scrollable::Scrollbar::default(),
            })
            .width(Length::Fill)
            .height(Length::Fixed(380.0)),
    ]
    .spacing(10)
    .width(Length::Fixed(900.0));
    if let Some(running) = &view.running {
        body = body.push(text(running.as_str()).size(12).color(p.muted));
    }
    if let Some(error) = &view.error {
        body = body.push(
            text(error.as_str())
                .size(12.5)
                .color(se_theme::error_color(dark)),
        );
    }
    container(body)
        .padding(16)
        .style(se_theme::card(dark))
        .into()
}

/// The window that puts the rows of a CSV or JSON file into a table: the
/// file, how it is read, and the column each of its fields goes to.
pub fn import<'a>(im: &'a super::Import, dark: bool) -> Element<'a, Message> {
    use super::import::{delimiter_name, Format, DELIMITERS};
    let p = se_theme::palette(dark);
    let toggle = |label: String, active: bool, message: Message| {
        button(text(label).size(12.5))
            .padding([3, 10])
            .on_press(message)
            .style((se_theme::GhostButton { dark, active }).style())
    };
    let mut file_line = row![button(text(t!("db.import_choose").to_string()).size(12.5))
        .padding([4, 14])
        .on_press_maybe((!im.running).then_some(Message::Db(DbMsg::ImportPick)))
        .style((se_theme::Outline { dark }).style())]
    .spacing(10)
    .align_y(Alignment::Center);
    file_line = file_line.push(
        text(
            im.path
                .as_ref()
                .map(|f| f.display().to_string())
                .unwrap_or_default(),
        )
        .size(12)
        .color(p.muted),
    );
    let mut body = column![
        head(
            t!("db.import_title", table = im.table.as_str()).to_string(),
            Message::Db(DbMsg::ImportClose),
            dark
        ),
        file_line,
    ]
    .spacing(10)
    .width(Length::Fixed(760.0));

    if im.path.is_some() {
        // How the file is read.
        let mut options = row![
            toggle(
                "CSV".to_string(),
                im.format == Format::Csv,
                Message::Db(DbMsg::ImportFormat(Format::Csv))
            ),
            toggle(
                "JSON".to_string(),
                im.format == Format::Json,
                Message::Db(DbMsg::ImportFormat(Format::Json))
            ),
        ]
        .spacing(4)
        .align_y(Alignment::Center);
        if im.format == Format::Csv {
            options = options.push(Space::new().width(12));
            for delimiter in DELIMITERS {
                options = options.push(toggle(
                    delimiter_name(delimiter),
                    im.delimiter == delimiter,
                    Message::Db(DbMsg::ImportDelimiter(delimiter)),
                ));
            }
            options = options.push(Space::new().width(12)).push(
                checkbox(im.header)
                    .label(t!("db.import_header").to_string())
                    .size(14)
                    .text_size(12.5)
                    .on_toggle(|on| Message::Db(DbMsg::ImportHeader(on))),
            );
        }
        options = options.push(Space::new().width(12)).push(
            checkbox(im.empty_null)
                .label(t!("db.import_empty_null").to_string())
                .size(14)
                .text_size(12.5)
                .on_toggle(|on| Message::Db(DbMsg::ImportEmptyNull(on))),
        );
        body = body.push(options);

        // Where each field goes.
        let skip = t!("db.import_skip").to_string();
        let mut choices: Vec<String> = vec![skip.clone()];
        choices.extend(im.targets.iter().map(|t| t.name.clone()));
        let mut list = column![row![
            text(t!("db.import_field").to_string())
                .size(12)
                .color(p.muted)
                .width(Length::Fixed(170.0)),
            text(t!("db.import_example").to_string())
                .size(12)
                .color(p.muted)
                .width(Length::Fixed(230.0)),
            text(t!("db.import_column").to_string())
                .size(12)
                .color(p.muted),
        ]
        .spacing(10)]
        .spacing(4);
        for (i, name) in im.sample.names.iter().enumerate() {
            let example = im
                .sample
                .rows
                .iter()
                .take(2)
                .filter_map(|r| r.get(i))
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join(" · ");
            let chosen = im
                .mapping
                .get(i)
                .cloned()
                .flatten()
                .unwrap_or_else(|| skip.clone());
            let skip_label = skip.clone();
            list = list.push(
                row![
                    text(name.as_str())
                        .size(12.5)
                        .font(se_theme::UI_SEMIBOLD)
                        .width(Length::Fixed(170.0))
                        .wrapping(iced::widget::text::Wrapping::None),
                    text(example)
                        .size(12)
                        .color(p.muted)
                        .width(Length::Fixed(230.0))
                        .wrapping(iced::widget::text::Wrapping::None),
                    pick_list(choices.clone(), Some(chosen), move |c: String| {
                        Message::Db(DbMsg::ImportMap(i, (c != skip_label).then_some(c)))
                    })
                    .text_size(12.5)
                    .padding([3, 8])
                    .style(se_theme::Select { dark }.style())
                    .menu_style(se_theme::Select { dark }.menu_style()),
                ]
                .spacing(10)
                .align_y(Alignment::Center),
            );
        }
        body = body.push(
            scrollable(list)
                .style(se_theme::Scrollbar { dark }.style())
                .height(Length::Shrink),
        );
    }
    if let Some(error) = &im.error {
        body = body.push(
            text(error.as_str())
                .size(12.5)
                .color(se_theme::error_color(dark)),
        );
    }
    if let Some(n) = im.imported {
        body = body.push(
            text(t!("db.import_done", n = n).to_string())
                .size(12.5)
                .color(p.accent),
        );
    }
    let ready = im.path.is_some() && im.mapping.iter().any(Option::is_some) && !im.running;
    body = body.push(
        row![
            button(text(t!("db.import_run").to_string()).size(12.5))
                .padding([5, 18])
                .on_press_maybe(ready.then_some(Message::Db(DbMsg::ImportRun)))
                .style((se_theme::Primary { dark }).style()),
            text(if im.running {
                t!("db.import_running").to_string()
            } else {
                String::new()
            })
            .size(12)
            .color(p.muted),
        ]
        .spacing(12)
        .align_y(Alignment::Center),
    );
    container(body)
        .max_height(620)
        .padding(16)
        .style(se_theme::card(dark))
        .into()
}

/// The window that copies a table to another data source: where it goes, and
/// whether its rows go too.
pub fn copy_table<'a>(
    copy: &'a super::CopyTable,
    connections: &'a [super::Connection],
    dark: bool,
) -> Element<'a, Message> {
    use super::CopyField;
    let p = se_theme::palette(dark);
    let field = |label: String, value: &str, hint: String, which: CopyField| {
        row![
            text(label)
                .size(12.5)
                .color(p.muted)
                .width(Length::Fixed(130.0)),
            text_input(&hint, value)
                .on_input(move |t| Message::Db(DbMsg::CopyField(which, t)))
                .size(12.5)
                .padding([4, 8])
                .width(Length::Fill)
                .style(se_theme::Field::new(dark).style()),
        ]
        .spacing(10)
        .align_y(Alignment::Center)
    };
    // The data sources it can go to, by name.
    let names: Vec<(u64, String)> = connections.iter().map(|c| (c.id, c.label())).collect();
    let chosen = copy
        .dest
        .and_then(|id| names.iter().find(|(i, _)| *i == id).map(|(_, n)| n.clone()));
    let by_name = names.clone();
    let source_name = connections
        .iter()
        .find(|c| c.id == copy.from.0)
        .map(|c| c.label())
        .unwrap_or_default();
    let ready = copy.dest.is_some()
        && !copy.name.trim().is_empty()
        && !copy.schema.trim().is_empty()
        && !copy.running;
    let (table, source) = (copy.from.3.as_str(), source_name.as_str());
    let title = t!("db.copy_title", table = table, source = source).to_string();
    let mut body = column![
        head(title, Message::Db(DbMsg::CopyClose), dark),
        row![
            text(t!("db.copy_to").to_string())
                .size(12.5)
                .color(p.muted)
                .width(Length::Fixed(130.0)),
            pick_list(
                names.iter().map(|(_, n)| n.clone()).collect::<Vec<_>>(),
                chosen,
                move |name: String| {
                    let id = by_name
                        .iter()
                        .find(|(_, n)| *n == name)
                        .map_or(0, |(id, _)| *id);
                    Message::Db(DbMsg::CopyDest(id))
                }
            )
            .text_size(12.5)
            .padding([3, 8])
            .style(se_theme::Select { dark }.style())
            .menu_style(se_theme::Select { dark }.menu_style()),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
        field(
            t!("db.copy_database").to_string(),
            &copy.database,
            t!("db.copy_database_hint").to_string(),
            CopyField::Database
        ),
        field(
            t!("db.copy_schema").to_string(),
            &copy.schema,
            String::new(),
            CopyField::Schema
        ),
        field(
            t!("db.copy_name").to_string(),
            &copy.name,
            String::new(),
            CopyField::Name
        ),
        checkbox(copy.with_data)
            .label(t!("db.copy_with_data").to_string())
            .size(14)
            .text_size(12.5)
            .on_toggle(|on| Message::Db(DbMsg::CopyData(on))),
        text(t!("db.copy_hint").to_string()).size(12).color(p.muted),
    ]
    .spacing(10)
    .width(Length::Fixed(560.0));
    if let Some(error) = &copy.error {
        body = body.push(
            text(error.as_str())
                .size(12.5)
                .color(se_theme::error_color(dark)),
        );
    }
    if let Some(rows) = copy.copied {
        body = body.push(
            text(t!("db.copy_done", n = rows).to_string())
                .size(12.5)
                .color(p.accent),
        );
    }
    body = body.push(
        row![
            button(text(t!("db.copy_run").to_string()).size(12.5))
                .padding([5, 18])
                .on_press_maybe(ready.then_some(Message::Db(DbMsg::CopyRun)))
                .style((se_theme::Primary { dark }).style()),
            text(if copy.running {
                t!("db.copy_running").to_string()
            } else {
                String::new()
            })
            .size(12)
            .color(p.muted),
        ]
        .spacing(12)
        .align_y(Alignment::Center),
    );
    container(body)
        .padding(16)
        .style(se_theme::card(dark))
        .into()
}

/// The window that backs a database up (or a schema, or a table), or puts a
/// backup back.
pub fn backup<'a>(b: &'a super::Backup, production: bool, dark: bool) -> Element<'a, Message> {
    use super::BackupFlag;
    let p = se_theme::palette(dark);
    let flag = |label: String, on: bool, which: BackupFlag| {
        checkbox(on)
            .label(label)
            .size(14)
            .text_size(12.5)
            .on_toggle(move |v| Message::Db(DbMsg::BackupFlag(which, v)))
    };
    let scope = if !b.table.is_empty() {
        format!("{}.{}", b.schema, b.table)
    } else if !b.schema.is_empty() {
        b.schema.clone()
    } else {
        b.database.clone()
    };
    let title = if b.restore {
        t!("db.restore_title", database = b.database.as_str()).to_string()
    } else {
        t!("db.backup_title", scope = scope.as_str()).to_string()
    };
    let mut options = column![].spacing(6);
    if !b.restore {
        options = options.push(flag(
            t!("db.backup_plain").to_string(),
            b.spec.format == "plain",
            BackupFlag::Plain,
        ));
    }
    options = options
        .push(flag(
            t!("db.backup_schema_only").to_string(),
            b.spec.schema_only,
            BackupFlag::SchemaOnly,
        ))
        .push(flag(
            t!("db.backup_data_only").to_string(),
            b.spec.data_only,
            BackupFlag::DataOnly,
        ))
        .push(flag(
            t!("db.backup_no_owner").to_string(),
            b.spec.no_owner,
            BackupFlag::NoOwner,
        ))
        .push(flag(
            t!("db.backup_no_privileges").to_string(),
            b.spec.no_privileges,
            BackupFlag::NoPrivileges,
        ));
    if b.restore {
        options = options
            .push(flag(
                t!("db.restore_clean").to_string(),
                b.spec.clean,
                BackupFlag::Clean,
            ))
            .push(flag(
                t!("db.restore_single").to_string(),
                b.spec.single_transaction,
                BackupFlag::SingleTransaction,
            ));
    }
    let mut body = column![
        head(title, Message::Db(DbMsg::BackupClose), dark),
        row![
            button(text(t!("db.import_choose").to_string()).size(12.5))
                .padding([4, 14])
                .on_press_maybe((!b.running).then_some(Message::Db(DbMsg::BackupPick)))
                .style((se_theme::Outline { dark }).style()),
            text(
                b.path
                    .as_ref()
                    .map(|f| f.display().to_string())
                    .unwrap_or_default()
            )
            .size(12)
            .color(p.muted),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
        options,
    ]
    .spacing(12)
    .width(Length::Fixed(560.0));
    // What a restore can do to what is there.
    if b.restore && (b.spec.clean || production) {
        body = body.push(
            text(if b.spec.clean {
                t!("db.restore_warning_clean").to_string()
            } else {
                t!("db.restore_warning_production").to_string()
            })
            .size(12.5)
            .color(se_theme::error_color(dark)),
        );
    }
    if let Some(error) = &b.error {
        body = body.push(
            text(error.as_str())
                .size(12.5)
                .color(se_theme::error_color(dark)),
        );
    }
    if let Some(said) = &b.done {
        body = body.push(
            text(if b.restore {
                t!("db.restore_done", kind = said.as_str()).to_string()
            } else {
                t!("db.backup_done", size = said.as_str()).to_string()
            })
            .size(12.5)
            .color(p.accent),
        );
    }
    body = body.push(
        row![
            button(
                text(if b.restore {
                    t!("db.restore_run").to_string()
                } else {
                    t!("db.backup_run").to_string()
                })
                .size(12.5)
            )
            .padding([5, 18])
            .on_press_maybe(
                (b.path.is_some() && !b.running).then_some(Message::Db(DbMsg::BackupRun))
            )
            .style((se_theme::Primary { dark }).style()),
            text(if b.running {
                t!("db.backup_running").to_string()
            } else {
                String::new()
            })
            .size(12)
            .color(p.muted),
        ]
        .spacing(12)
        .align_y(Alignment::Center),
    );
    container(body)
        .padding(16)
        .style(se_theme::card(dark))
        .into()
}
