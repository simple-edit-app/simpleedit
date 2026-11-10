//! What the API panel and its tabs look like.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use iced::widget::{
    button, checkbox, column, container, pick_list, row, scrollable, text, text_editor, text_input,
    tooltip, Space,
};
use iced::{Alignment, Color, Element, Length};

use super::http::{class_of, Class, Kind};
use super::model::{AuthKind, BodyKind, CheckOp, CheckSource, ExtractSource, Item, Method, Pair};
use super::{
    Act, ApiMsg, ApiTab, AuthField, Dialog, NameFor, ReqSection, RespSection, SideTab, State, Table,
};
use crate::app::Message;
use crate::menu_bar;
use crate::theme as se_theme;

fn ghost(dark: bool, active: bool) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    (se_theme::GhostButton { dark, active }).style()
}

fn say(path: &Path, act: Act) -> Message {
    Message::Api(ApiMsg::Tab(path.to_path_buf(), act))
}

fn api(msg: ApiMsg) -> Message {
    Message::Api(msg)
}

/// The color of a method, whatever the theme.
pub fn method_color(method: Method, dark: bool) -> Color {
    let (r, g, b) = match method {
        Method::Get => (0x2e, 0xa0, 0x43),
        Method::Post => (0xd9, 0x8e, 0x04),
        Method::Put => (0x3b, 0x82, 0xf6),
        Method::Patch => (0x8b, 0x5c, 0xf6),
        Method::Delete => (0xe5, 0x48, 0x4d),
        Method::Head | Method::Options => {
            return se_theme::muted_text(dark);
        }
    };
    Color::from_rgb8(r, g, b)
}

fn status_color(status: u16, dark: bool) -> Color {
    match class_of(status) {
        Class::Success => Color::from_rgb8(0x2e, 0xa0, 0x43),
        Class::Info | Class::Redirect => Color::from_rgb8(0xd9, 0x8e, 0x04),
        Class::ClientError | Class::ServerError => se_theme::error_color(dark),
    }
}

fn short_method(method: Method) -> &'static str {
    match method {
        Method::Delete => "DEL",
        Method::Options => "OPT",
        Method::Patch => "PATCH",
        other => other.as_str(),
    }
}

fn badge<'a>(method: Method, dark: bool) -> Element<'a, Message> {
    text(short_method(method))
        .size(10.5)
        .font(se_theme::UI_SEMIBOLD)
        .color(method_color(method, dark))
        .width(Length::Fixed(40.0))
        .into()
}

fn small_button<'a>(
    label: &'a str,
    tip: String,
    message: Message,
    dark: bool,
) -> Element<'a, Message> {
    tooltip(
        button(text(label).size(13))
            .padding([2, 8])
            .on_press(message)
            .style(ghost(dark, false)),
        container(text(tip).size(12))
            .padding([4, 8])
            .style(se_theme::tooltip(dark)),
        tooltip::Position::Bottom,
    )
    .into()
}

/// A size in bytes, as a person reads it.
/// An entry of a list that is told apart by its id, not its name.
#[derive(Clone)]
struct Choice {
    id: Option<u64>,
    name: String,
}

impl PartialEq for Choice {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl std::fmt::Display for Choice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name)
    }
}

pub fn size(bytes: usize) -> String {
    crate::database::protocol::human_size(bytes as i64)
}

// ------------------------------------------------------------------ the panel

/// The side panel: the collections and the history, the environment that is on.
pub fn sidebar<'a>(
    state: &'a State,
    tabs: &'a std::collections::HashMap<PathBuf, ApiTab>,
    dark: bool,
) -> Element<'a, Message> {
    let muted = se_theme::muted_text(dark);
    let header = row![
        text(t!("sidebar.api").to_uppercase())
            .size(10.5)
            .color(muted)
            .font(se_theme::UI_MEDIUM),
        Space::new().width(Length::Fill),
        small_button(
            "↓",
            t!("api.import").to_string(),
            api(ApiMsg::ImportFile),
            dark
        ),
        small_button(
            "⌘",
            t!("api.import_curl").to_string(),
            api(ApiMsg::OpenCurl),
            dark
        ),
        small_button(
            "▤",
            t!("api.new_collection").to_string(),
            api(ApiMsg::AskNewCollection),
            dark
        ),
        small_button(
            "+",
            t!("api.new_request").to_string(),
            api(ApiMsg::NewRequest),
            dark
        ),
    ]
    .spacing(2)
    .padding([0, 8])
    .align_y(Alignment::Center);

    // The environment that is on, and the way to edit them.
    let none = t!("api.no_env").to_string();
    let mut names = vec![Choice {
        id: None,
        name: none.clone(),
    }];
    names.extend(state.environments.iter().map(|e| Choice {
        id: Some(e.id),
        name: e.name.clone(),
    }));
    let chosen = state
        .active_env
        .and_then(|id| state.environment(id))
        .map(|e| Choice {
            id: Some(e.id),
            name: e.name.clone(),
        })
        .unwrap_or(Choice {
            id: None,
            name: none.clone(),
        });
    let env_row = row![
        pick_list(names, Some(chosen), move |choice: Choice| {
            api(ApiMsg::SelectEnv(choice.id))
        })
        .text_size(12.5)
        .padding([3, 8])
        .width(Length::Fill)
        .style(se_theme::Select { dark }.style())
        .menu_style(se_theme::Select { dark }.menu_style()),
        small_button(
            "⚙",
            t!("api.environments").to_string(),
            api(ApiMsg::OpenEnvironments),
            dark
        ),
    ]
    .spacing(4)
    .padding([0, 8])
    .align_y(Alignment::Center);

    let side = row![
        button(text(t!("api.collections").to_string()).size(12.5))
            .padding([3, 10])
            .on_press(api(ApiMsg::Side(SideTab::Collections)))
            .style(ghost(dark, state.side == SideTab::Collections)),
        button(text(t!("api.history").to_string()).size(12.5))
            .padding([3, 10])
            .on_press(api(ApiMsg::Side(SideTab::History)))
            .style(ghost(dark, state.side == SideTab::History)),
    ]
    .spacing(4)
    .padding([0, 8]);

    let filter = text_input(&t!("api.filter"), &state.filter)
        .on_input(|t| api(ApiMsg::Filter(t)))
        .size(12.5)
        .padding([4, 8])
        .style(se_theme::Field::new(dark).style());

    let open: HashSet<(u64, u64)> = tabs.values().filter_map(|t| t.origin).collect();
    let mut rows: Vec<Element<Message>> = Vec::new();
    match state.side {
        SideTab::Collections => {
            let needle = state.filter.trim().to_lowercase();
            for collection in &state.collections {
                collection_rows(&mut rows, state, collection, &needle, &open, dark);
            }
            if state.collections.is_empty() {
                rows.push(
                    text(t!("api.empty").to_string())
                        .size(12.5)
                        .color(muted)
                        .into(),
                );
            }
        }
        SideTab::History => history_rows(&mut rows, state, dark),
    }
    let list = scrollable(column(rows).spacing(1))
        .style(se_theme::Scrollbar { dark }.style())
        .height(Length::Fill);
    container(
        column![
            header,
            env_row,
            side,
            container(filter).padding([0, 8]),
            container(list).padding([0, 4]).height(Length::Fill)
        ]
        .spacing(8)
        .height(Length::Fill),
    )
    .padding([14, 2])
    .height(Length::Fill)
    .style(se_theme::panel(dark))
    .into()
}

/// A line of the list: indented, one thing in it.
fn line<'a>(
    depth: u16,
    content: Element<'a, Message>,
    message: Message,
    active: bool,
    dark: bool,
) -> Element<'a, Message> {
    button(
        row![
            Space::new().width(Length::Fixed(depth as f32 * 14.0)),
            content
        ]
        .align_y(Alignment::Center),
    )
    .padding([3, 6])
    .width(Length::Fill)
    .on_press(message)
    .style(ghost(dark, active))
    .into()
}

fn matches(item: &Item, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    match item {
        Item::Request(r) => {
            r.title().to_lowercase().contains(needle) || r.url.to_lowercase().contains(needle)
        }
        Item::Folder(f) => {
            f.name.to_lowercase().contains(needle) || f.items.iter().any(|i| matches(i, needle))
        }
    }
}

fn collection_rows<'a>(
    out: &mut Vec<Element<'a, Message>>,
    state: &'a State,
    collection: &'a super::model::Collection,
    needle: &str,
    open: &HashSet<(u64, u64)>,
    dark: bool,
) {
    if !needle.is_empty()
        && !collection.name.to_lowercase().contains(needle)
        && !collection.items.iter().any(|i| matches(i, needle))
    {
        return;
    }
    let is_open = state.is_open(collection.id);
    let head = line(
        0,
        row![
            text(if is_open { "▾" } else { "▸" })
                .size(11)
                .width(Length::Fixed(14.0)),
            text(collection.name.as_str())
                .size(12.5)
                .font(se_theme::UI_SEMIBOLD),
            text(format!(" {}", collection.count()))
                .size(11)
                .color(se_theme::muted_text(dark)),
        ]
        .spacing(4)
        .align_y(Alignment::Center)
        .into(),
        api(ApiMsg::Toggle(collection.id)),
        false,
        dark,
    );
    let id = collection.id;
    out.push(crate::pointer_area::context_menu(head, move || {
        collection_menu(id, dark)
    }));
    if is_open {
        item_rows(
            out,
            state,
            collection.id,
            &collection.items,
            1,
            needle,
            open,
            dark,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn item_rows<'a>(
    out: &mut Vec<Element<'a, Message>>,
    state: &'a State,
    collection: u64,
    items: &'a [Item],
    depth: u16,
    needle: &str,
    open: &HashSet<(u64, u64)>,
    dark: bool,
) {
    for item in items.iter().filter(|i| matches(i, needle)) {
        match item {
            Item::Folder(folder) => {
                let is_open = state.is_open(folder.id);
                let head = line(
                    depth,
                    row![
                        text(if is_open { "▾" } else { "▸" })
                            .size(11)
                            .width(Length::Fixed(14.0)),
                        text(folder.name.as_str()).size(12.5),
                    ]
                    .spacing(4)
                    .align_y(Alignment::Center)
                    .into(),
                    api(ApiMsg::Toggle(folder.id)),
                    false,
                    dark,
                );
                let id = folder.id;
                out.push(crate::pointer_area::context_menu(head, move || {
                    folder_menu(collection, id, dark)
                }));
                if is_open {
                    item_rows(
                        out,
                        state,
                        collection,
                        &folder.items,
                        depth + 1,
                        needle,
                        open,
                        dark,
                    );
                }
            }
            Item::Request(request) => {
                let row_ = line(
                    depth,
                    row![
                        badge(request.method, dark),
                        text(request.title())
                            .size(12.5)
                            .wrapping(iced::widget::text::Wrapping::None),
                    ]
                    .spacing(4)
                    .align_y(Alignment::Center)
                    .into(),
                    api(ApiMsg::OpenRequest(collection, request.id)),
                    open.contains(&(collection, request.id)),
                    dark,
                );
                let id = request.id;
                out.push(crate::pointer_area::context_menu(row_, move || {
                    request_menu(collection, id, dark)
                }));
            }
        }
    }
}

fn history_rows<'a>(out: &mut Vec<Element<'a, Message>>, state: &'a State, dark: bool) {
    let muted = se_theme::muted_text(dark);
    let needle = state.filter.trim().to_lowercase();
    if state.history.is_empty() {
        out.push(
            text(t!("api.history_empty").to_string())
                .size(12.5)
                .color(muted)
                .into(),
        );
        return;
    }
    out.push(
        button(text(t!("api.history_clear").to_string()).size(12))
            .padding([2, 8])
            .on_press(api(ApiMsg::ClearHistory))
            .style(ghost(dark, false))
            .into(),
    );
    for (at, entry) in state.history.iter().enumerate() {
        if !needle.is_empty() && !entry.request.url.to_lowercase().contains(&needle) {
            continue;
        }
        let status: Element<Message> = match (entry.status, &entry.error) {
            (Some(code), _) => text(code.to_string())
                .size(11)
                .color(status_color(code, dark))
                .into(),
            (None, Some(_)) => text("✗").size(11).color(se_theme::error_color(dark)).into(),
            _ => text("").into(),
        };
        out.push(line(
            0,
            column![
                row![
                    badge(entry.request.method, dark),
                    text(entry.request.url.as_str())
                        .size(12.5)
                        .wrapping(iced::widget::text::Wrapping::None),
                ]
                .spacing(4)
                .align_y(Alignment::Center),
                row![
                    Space::new().width(Length::Fixed(44.0)),
                    status,
                    text(format!("{} ms", entry.ms)).size(11).color(muted),
                    text(entry.time.as_str()).size(11).color(muted),
                ]
                .spacing(8),
            ]
            .into(),
            api(ApiMsg::OpenHistory(at)),
            false,
            dark,
        ));
    }
}

fn collection_menu(id: u64, dark: bool) -> Element<'static, Message> {
    container(
        column![
            menu_bar::item(
                t!("api.new_request").to_string(),
                None,
                dark,
                api(ApiMsg::NewIn(id, 0))
            ),
            menu_bar::item(
                t!("api.new_folder").to_string(),
                None,
                dark,
                api(ApiMsg::AskNewFolder(id, 0))
            ),
            menu_bar::separator(dark),
            menu_bar::item(
                t!("api.rename").to_string(),
                None,
                dark,
                api(ApiMsg::AskRename(id, 0))
            ),
            menu_bar::item(
                t!("api.run_collection").to_string(),
                None,
                dark,
                api(ApiMsg::RunCollection(id))
            ),
            menu_bar::item(
                t!("api.export").to_string(),
                None,
                dark,
                api(ApiMsg::ExportCollection(id))
            ),
            menu_bar::separator(dark),
            menu_bar::item(
                t!("api.delete").to_string(),
                None,
                dark,
                api(ApiMsg::AskDelete(id, 0))
            ),
        ]
        .spacing(1)
        .padding(6)
        .width(230),
    )
    .style(se_theme::card(dark))
    .into()
}

fn folder_menu(collection: u64, id: u64, dark: bool) -> Element<'static, Message> {
    container(
        column![
            menu_bar::item(
                t!("api.new_request").to_string(),
                None,
                dark,
                api(ApiMsg::NewIn(collection, id))
            ),
            menu_bar::item(
                t!("api.new_folder").to_string(),
                None,
                dark,
                api(ApiMsg::AskNewFolder(collection, id))
            ),
            menu_bar::separator(dark),
            menu_bar::item(
                t!("api.rename").to_string(),
                None,
                dark,
                api(ApiMsg::AskRename(collection, id))
            ),
            menu_bar::item(
                t!("api.delete").to_string(),
                None,
                dark,
                api(ApiMsg::AskDelete(collection, id))
            ),
        ]
        .spacing(1)
        .padding(6)
        .width(230),
    )
    .style(se_theme::card(dark))
    .into()
}

fn request_menu(collection: u64, id: u64, dark: bool) -> Element<'static, Message> {
    container(
        column![
            menu_bar::item(
                t!("api.open").to_string(),
                None,
                dark,
                api(ApiMsg::OpenRequest(collection, id))
            ),
            menu_bar::item(
                t!("api.duplicate").to_string(),
                None,
                dark,
                api(ApiMsg::Duplicate(collection, id))
            ),
            menu_bar::item(
                t!("api.rename").to_string(),
                None,
                dark,
                api(ApiMsg::AskRename(collection, id))
            ),
            menu_bar::item(
                t!("api.copy_curl").to_string(),
                None,
                dark,
                api(ApiMsg::CopyCurlOf(collection, id))
            ),
            menu_bar::separator(dark),
            menu_bar::item(
                t!("api.delete").to_string(),
                None,
                dark,
                api(ApiMsg::AskDelete(collection, id))
            ),
        ]
        .spacing(1)
        .padding(6)
        .width(230),
    )
    .style(se_theme::card(dark))
    .into()
}

// ------------------------------------------------------------------ the tab

/// A request, and what came back.
pub fn tab<'a>(
    path: &'a Path,
    tab: &'a ApiTab,
    state: &'a State,
    dark: bool,
) -> Element<'a, Message> {
    let p = se_theme::palette(dark);
    let resolved = tab.request.resolve(&state.variables());

    // The name, and what can be done with the request as a whole.
    let name_path = path.to_path_buf();
    let mut head = row![text_input(&t!("api.name_hint"), &tab.request.name)
        .on_input(move |t| Message::Api(ApiMsg::Tab(name_path.clone(), Act::Name(t))))
        .size(13.5)
        .padding([4, 8])
        .style(se_theme::Field::new(dark).style())
        .width(Length::Fill),]
    .spacing(8)
    .padding([8, 12])
    .align_y(Alignment::Center);
    if tab.changed() {
        head = head.push(text("●").size(11).color(p.accent));
    }
    if tab.origin.is_none() && !state.collections.is_empty() {
        let choice = |c: &super::model::Collection| Choice {
            id: Some(c.id),
            name: c.name.clone(),
        };
        let names: Vec<Choice> = state.collections.iter().map(choice).collect();
        let chosen = tab
            .save_target
            .and_then(|id| state.collection(id))
            .or_else(|| state.collections.first())
            .map(choice);
        let target_path = path.to_path_buf();
        head = head.push(
            pick_list(names, chosen, move |picked: Choice| {
                let id = picked.id.unwrap_or(0);
                Message::Api(ApiMsg::Tab(target_path.clone(), Act::SaveTarget(id)))
            })
            .text_size(12.5)
            .padding([3, 8])
            .style(se_theme::Select { dark }.style())
            .menu_style(se_theme::Select { dark }.menu_style()),
        );
    }
    head = head
        .push(
            button(text(t!("api.save").to_string()).size(12.5))
                .padding([3, 12])
                .on_press_maybe(
                    (tab.changed() || tab.origin.is_none()).then(|| say(path, Act::Save)),
                )
                .style((se_theme::Outline { dark }).style()),
        )
        .push(small_button(
            "⌘",
            t!("api.copy_curl").to_string(),
            say(path, Act::CopyCurl),
            dark,
        ));

    // The method, the address, and Send.
    let url_path = path.to_path_buf();
    let method_path = path.to_path_buf();
    let send: Element<Message> = if tab.sending {
        button(text(t!("api.cancel").to_string()).size(13))
            .padding([5, 18])
            .on_press(say(path, Act::Cancel))
            .style((se_theme::Outline { dark }).style())
            .into()
    } else {
        button(
            text(t!("api.send").to_string())
                .size(13)
                .font(se_theme::UI_SEMIBOLD),
        )
        .padding([5, 18])
        .on_press(say(path, Act::Send))
        .style((se_theme::Primary { dark }).style())
        .into()
    };
    let address = row![
        pick_list(Method::ALL.to_vec(), Some(tab.request.method), move |m| {
            Message::Api(ApiMsg::Tab(method_path.clone(), Act::Method(m)))
        })
        .text_size(13)
        .padding([5, 8])
        .style(se_theme::Select { dark }.style())
        .menu_style(se_theme::Select { dark }.menu_style()),
        text_input("https://api.example.com/items", &tab.request.url)
            .on_input(move |t| Message::Api(ApiMsg::Tab(url_path.clone(), Act::Url(t))))
            .on_submit(say(path, Act::Send))
            .size(13)
            .font(se_theme::MONO)
            .padding([6, 10])
            .width(Length::Fill)
            .style(se_theme::Field::new(dark).style()),
        send,
    ]
    .spacing(8)
    .padding([0, 12])
    .align_y(Alignment::Center);

    let mut upper = column![head, address].spacing(8);
    if let Some(notice) = &tab.notice {
        upper =
            upper.push(container(text(notice.as_str()).size(12).color(p.muted)).padding([0, 12]));
    }
    if !resolved.missing.is_empty() {
        upper = upper.push(
            container(
                text(t!("api.missing_vars", names = resolved.missing.join(", ")).to_string())
                    .size(12)
                    .color(se_theme::error_color(dark)),
            )
            .padding([0, 12]),
        );
    }

    // The parts of the request.
    let count = |pairs: &[Pair]| {
        pairs
            .iter()
            .filter(|p| p.enabled && !p.key.is_empty())
            .count()
    };
    let section = |label: String, which: ReqSection| {
        button(text(label).size(12.5))
            .padding([3, 10])
            .on_press(say(path, Act::Section(which)))
            .style(ghost(dark, tab.section == which))
    };
    let body_mark = if tab.request.body.kind == BodyKind::None {
        ""
    } else {
        " •"
    };
    let auth_mark = if tab.request.auth.kind == AuthKind::None {
        ""
    } else {
        " •"
    };
    let sections = row![
        section(
            format!("{} ({})", t!("api.params"), count(&tab.request.params)),
            ReqSection::Params
        ),
        section(
            format!("{} ({})", t!("api.headers"), count(&tab.request.headers)),
            ReqSection::Headers
        ),
        section(format!("{}{body_mark}", t!("api.body")), ReqSection::Body),
        section(format!("{}{auth_mark}", t!("api.auth")), ReqSection::Auth),
        section(t!("api.settings").to_string(), ReqSection::Settings),
        section(
            format!(
                "{} ({})",
                t!("api.tests"),
                tab.request.checks.len() + tab.request.extracts.len()
            ),
            ReqSection::Tests
        ),
    ]
    .spacing(2)
    .padding([0, 12]);
    let content: Element<Message> = match tab.section {
        ReqSection::Params => pairs_table(path, Table::Params, &tab.request.params, false, dark),
        ReqSection::Headers => pairs_table(path, Table::Headers, &tab.request.headers, true, dark),
        ReqSection::Body => body_section(path, tab, dark),
        ReqSection::Auth => auth_section(path, tab, dark),
        ReqSection::Settings => settings_section(path, tab, dark),
        ReqSection::Tests => tests_section(path, tab, dark),
    };
    let request_part = column![
        upper,
        sections,
        scrollable(container(content).padding([0, 12]))
            .style(se_theme::Scrollbar { dark }.style())
            .height(Length::Fill),
    ]
    .spacing(8)
    .height(Length::FillPortion(2));

    let rule = container(Space::new().height(1))
        .width(Length::Fill)
        .style(se_theme::rule(dark));
    let response_part = container(response_view(path, tab, dark)).height(Length::FillPortion(3));
    container(column![request_part, rule, response_part])
        .width(Length::Fill)
        .height(Length::Fill)
        .style(se_theme::window(dark))
        .into()
}

/// The tests of the answer and the values kept from it.
fn tests_section<'a>(path: &Path, tab: &'a ApiTab, dark: bool) -> Element<'a, Message> {
    let muted = se_theme::muted_text(dark);
    let heading = |label: String| text(label).size(12.5).font(se_theme::UI_SEMIBOLD);
    let field = |placeholder: String, value: &str, width: u16| {
        text_input(&placeholder, value)
            .size(12.5)
            .padding([3, 6])
            .font(se_theme::MONO)
            .width(Length::FillPortion(width))
            .style(se_theme::Field::new(dark).style())
    };
    let select = || se_theme::Select { dark };
    let mut checks = column![heading(t!("api.checks_title").to_string())].spacing(6);
    for (i, check) in tab.request.checks.iter().enumerate() {
        let (p_on, p_src, p_name, p_op, p_exp, p_del) = (
            path.to_path_buf(),
            path.to_path_buf(),
            path.to_path_buf(),
            path.to_path_buf(),
            path.to_path_buf(),
            path.to_path_buf(),
        );
        checks = checks.push(
            row![
                checkbox(check.enabled)
                    .size(14)
                    .on_toggle(move |on| say(&p_on, Act::CheckOn(i, on))),
                pick_list(CheckSource::ALL.to_vec(), Some(check.source), move |s| {
                    say(&p_src, Act::CheckSource(i, s))
                })
                .text_size(12.5)
                .padding([3, 8])
                .width(Length::FillPortion(2))
                .style(select().style())
                .menu_style(select().menu_style()),
                field(t!("api.check_name").to_string(), &check.name, 3)
                    .on_input(move |t| say(&p_name, Act::CheckName(i, t))),
                pick_list(CheckOp::ALL.to_vec(), Some(check.op), move |o| {
                    say(&p_op, Act::CheckOp(i, o))
                })
                .text_size(12.5)
                .padding([3, 8])
                .width(Length::FillPortion(2))
                .style(select().style())
                .menu_style(select().menu_style()),
                field(t!("api.expected").to_string(), &check.expected, 3)
                    .on_input(move |t| say(&p_exp, Act::CheckExpected(i, t))),
                button(text("×").size(14).color(muted))
                    .padding([0, 7])
                    .on_press(say(&p_del, Act::CheckDel(i)))
                    .style(ghost(dark, false)),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        );
    }
    checks = checks.push(
        button(text(format!("+ {}", t!("api.add_check"))).size(12.5))
            .padding([3, 10])
            .on_press(say(path, Act::CheckAdd))
            .style((se_theme::Outline { dark }).style()),
    );
    let mut keeps = column![heading(t!("api.extracts_title").to_string())].spacing(6);
    for (i, extract) in tab.request.extracts.iter().enumerate() {
        let (p_on, p_var, p_src, p_path, p_del) = (
            path.to_path_buf(),
            path.to_path_buf(),
            path.to_path_buf(),
            path.to_path_buf(),
            path.to_path_buf(),
        );
        keeps = keeps.push(
            row![
                checkbox(extract.enabled)
                    .size(14)
                    .on_toggle(move |on| say(&p_on, Act::ExtractOn(i, on))),
                field(t!("api.var_name").to_string(), &extract.variable, 2)
                    .on_input(move |t| say(&p_var, Act::ExtractVar(i, t))),
                text("←").size(13).color(muted),
                pick_list(
                    ExtractSource::ALL.to_vec(),
                    Some(extract.source),
                    move |s| { say(&p_src, Act::ExtractFrom(i, s)) }
                )
                .text_size(12.5)
                .padding([3, 8])
                .width(Length::FillPortion(2))
                .style(select().style())
                .menu_style(select().menu_style()),
                field(t!("api.extract_path").to_string(), &extract.path, 4)
                    .on_input(move |t| say(&p_path, Act::ExtractPath(i, t))),
                button(text("×").size(14).color(muted))
                    .padding([0, 7])
                    .on_press(say(&p_del, Act::ExtractDel(i)))
                    .style(ghost(dark, false)),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        );
    }
    keeps = keeps.push(
        button(text(format!("+ {}", t!("api.add_extract"))).size(12.5))
            .padding([3, 10])
            .on_press(say(path, Act::ExtractAdd))
            .style((se_theme::Outline { dark }).style()),
    );
    column![checks, keeps].spacing(18).padding([4, 0]).into()
}

/// How the tests went, one line each.
fn outcomes_view<'a>(view: &'a super::ResponseView, dark: bool) -> Element<'a, Message> {
    let p = se_theme::palette(dark);
    if view.outcomes.is_empty() {
        return container(
            text(t!("api.tests_none").to_string())
                .size(12.5)
                .color(p.muted),
        )
        .padding([0, 12])
        .into();
    }
    let mut list = column![].spacing(4).padding([0, 12]);
    for outcome in &view.outcomes {
        let mark = if outcome.passed {
            text("✓").color(p.accent)
        } else {
            text("✗").color(se_theme::error_color(dark))
        };
        let mut line = row![mark.size(13), text(outcome.label.clone()).size(12.5)].spacing(8);
        if !outcome.passed {
            line = line.push(
                text(format!("({})", outcome.actual))
                    .size(12)
                    .font(se_theme::MONO)
                    .color(p.muted),
            );
        }
        list = list.push(line);
    }
    scrollable(list)
        .style(se_theme::Scrollbar { dark }.style())
        .height(Length::Fill)
        .into()
}

/// A table of keys and values: a line each, with its own switch.
fn pairs_table<'a>(
    path: &Path,
    table: Table,
    pairs: &'a [Pair],
    presets: bool,
    dark: bool,
) -> Element<'a, Message> {
    let muted = se_theme::muted_text(dark);
    let mut rows = column![row![
        Space::new().width(Length::Fixed(26.0)),
        text(t!("api.key").to_string())
            .size(11.5)
            .color(muted)
            .width(Length::FillPortion(1)),
        text(t!("api.value").to_string())
            .size(11.5)
            .color(muted)
            .width(Length::FillPortion(2)),
        Space::new().width(Length::Fixed(30.0)),
    ]
    .spacing(6)]
    .spacing(4);
    for (i, pair) in pairs.iter().enumerate() {
        let (p_on, p_key, p_value, p_del) = (
            path.to_path_buf(),
            path.to_path_buf(),
            path.to_path_buf(),
            path.to_path_buf(),
        );
        rows = rows.push(
            row![
                checkbox(pair.enabled)
                    .size(14)
                    .on_toggle(move |on| Message::Api(ApiMsg::Tab(
                        p_on.clone(),
                        Act::PairOn(table, i, on)
                    ))),
                text_input(&t!("api.key"), &pair.key)
                    .on_input(move |t| Message::Api(ApiMsg::Tab(
                        p_key.clone(),
                        Act::PairKey(table, i, t)
                    )))
                    .size(12.5)
                    .padding([3, 6])
                    .width(Length::FillPortion(1))
                    .style(se_theme::Field::new(dark).style()),
                text_input(&t!("api.value"), &pair.value)
                    .on_input(move |t| Message::Api(ApiMsg::Tab(
                        p_value.clone(),
                        Act::PairValue(table, i, t)
                    )))
                    .size(12.5)
                    .font(se_theme::MONO)
                    .padding([3, 6])
                    .width(Length::FillPortion(2))
                    .style(se_theme::Field::new(dark).style()),
                button(text("×").size(14).color(muted))
                    .padding([0, 7])
                    .on_press(Message::Api(ApiMsg::Tab(p_del, Act::PairDel(table, i))))
                    .style(ghost(dark, false)),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        );
    }
    let mut footer = row![button(text(format!("+ {}", t!("api.add"))).size(12.5))
        .padding([3, 10])
        .on_press(say(path, Act::PairAdd(table)))
        .style((se_theme::Outline { dark }).style())]
    .spacing(8)
    .align_y(Alignment::Center);
    if presets {
        let preset_path = path.to_path_buf();
        footer = footer.push(
            pick_list(
                COMMON_HEADERS
                    .iter()
                    .map(|h| h.to_string())
                    .collect::<Vec<_>>(),
                None::<String>,
                move |name: String| {
                    Message::Api(ApiMsg::Tab(preset_path.clone(), Act::Preset(name)))
                },
            )
            .placeholder(t!("api.common_header").to_string())
            .text_size(12.5)
            .padding([3, 8])
            .style(se_theme::Select { dark }.style())
            .menu_style(se_theme::Select { dark }.menu_style()),
        );
    }
    column![rows, footer].spacing(8).padding([0, 0]).into()
}

/// The headers people add most.
pub const COMMON_HEADERS: [&str; 12] = [
    "Accept",
    "Accept-Language",
    "Authorization",
    "Cache-Control",
    "Content-Type",
    "Cookie",
    "If-None-Match",
    "Origin",
    "Referer",
    "User-Agent",
    "X-Api-Key",
    "X-Requested-With",
];

fn body_section<'a>(path: &Path, tab: &'a ApiTab, dark: bool) -> Element<'a, Message> {
    let p = se_theme::palette(dark);
    let kinds = BodyKind::ALL.iter().fold(row![].spacing(4), |r, kind| {
        let label = match kind {
            BodyKind::None => t!("api.body_none"),
            BodyKind::Json => t!("api.body_json"),
            BodyKind::Text => t!("api.body_text"),
            BodyKind::Form => t!("api.body_form"),
            BodyKind::File => t!("api.body_file"),
        };
        r.push(
            button(text(label.to_string()).size(12.5))
                .padding([3, 10])
                .on_press(say(path, Act::BodyKind(*kind)))
                .style(ghost(dark, tab.request.body.kind == *kind)),
        )
    });
    let content: Element<Message> = match tab.request.body.kind {
        BodyKind::None => text(t!("api.body_none_hint").to_string())
            .size(12.5)
            .color(p.muted)
            .into(),
        BodyKind::Json | BodyKind::Text => {
            let body_path = path.to_path_buf();
            let json = tab.request.body.kind == BodyKind::Json;
            let editor = text_editor(&tab.body_editor)
                .on_action(move |a| Message::Api(ApiMsg::Tab(body_path.clone(), Act::Body(a))))
                .highlight_with::<crate::editor::highlighter::Highlighter>(
                    crate::editor::highlighter::Settings {
                        dark,
                        extension: if json { "json" } else { "txt" }.to_string(),
                    },
                    crate::editor::highlighter::to_format,
                )
                .font(se_theme::MONO)
                .size(13)
                .padding([8, 10])
                .height(Length::Fixed(260.0));
            let mut col = column![editor].spacing(6);
            if json {
                let valid =
                    serde_json::from_str::<serde_json::Value>(&tab.request.body.text).is_ok();
                let empty = tab.request.body.text.trim().is_empty();
                col = col.push(
                    row![
                        button(text(t!("api.format").to_string()).size(12))
                            .padding([2, 10])
                            .on_press_maybe(valid.then(|| say(path, Act::BodyFormat)))
                            .style((se_theme::Outline { dark }).style()),
                        text(if empty {
                            String::new()
                        } else if valid {
                            "✓ JSON".to_string()
                        } else {
                            "✗ JSON".to_string()
                        })
                        .size(12)
                        .color(if valid {
                            p.accent
                        } else {
                            se_theme::error_color(dark)
                        }),
                    ]
                    .spacing(10)
                    .align_y(Alignment::Center),
                );
            }
            col.into()
        }
        BodyKind::Form => pairs_table(path, Table::Form, &tab.request.body.form, false, dark),
        BodyKind::File => row![
            button(text(t!("api.choose_file").to_string()).size(12.5))
                .padding([4, 12])
                .on_press(say(path, Act::BodyFile))
                .style((se_theme::Outline { dark }).style()),
            text(tab.request.body.file.as_str()).size(12).color(p.muted),
        ]
        .spacing(10)
        .align_y(Alignment::Center)
        .into(),
    };
    // A body that the method does not usually carry.
    let hint = if tab.request.body.kind != BodyKind::None && !tab.request.method.has_body() {
        t!("api.body_ignored", method = tab.request.method.as_str()).to_string()
    } else {
        String::new()
    };
    column![kinds, content, text(hint).size(11.5).color(p.muted)]
        .spacing(10)
        .into()
}

fn auth_section<'a>(path: &Path, tab: &'a ApiTab, dark: bool) -> Element<'a, Message> {
    let p = se_theme::palette(dark);
    let kinds = AuthKind::ALL.iter().fold(row![].spacing(4), |r, kind| {
        let label = match kind {
            AuthKind::None => t!("api.auth_none"),
            AuthKind::Bearer => t!("api.auth_bearer"),
            AuthKind::Basic => t!("api.auth_basic"),
            AuthKind::ApiKey => t!("api.auth_key"),
        };
        r.push(
            button(text(label.to_string()).size(12.5))
                .padding([3, 10])
                .on_press(say(path, Act::AuthKind(*kind)))
                .style(ghost(dark, tab.request.auth.kind == *kind)),
        )
    });
    let field = |label: String, value: &str, which: AuthField, secret: bool| {
        let field_path = path.to_path_buf();
        row![
            text(label)
                .size(12.5)
                .color(p.muted)
                .width(Length::Fixed(110.0)),
            text_input("", value)
                .on_input(move |t| Message::Api(ApiMsg::Tab(
                    field_path.clone(),
                    Act::Auth(which, t)
                )))
                .secure(secret)
                .size(12.5)
                .padding([3, 8])
                .width(Length::Fill)
                .style(se_theme::Field::new(dark).style()),
        ]
        .spacing(10)
        .align_y(Alignment::Center)
    };
    let auth = &tab.request.auth;
    let content: Element<Message> = match auth.kind {
        AuthKind::None => text(t!("api.auth_none_hint").to_string())
            .size(12.5)
            .color(p.muted)
            .into(),
        AuthKind::Bearer => field(
            t!("api.token").to_string(),
            &auth.token,
            AuthField::Token,
            true,
        )
        .into(),
        AuthKind::Basic => column![
            field(
                t!("api.username").to_string(),
                &auth.username,
                AuthField::Username,
                false
            ),
            field(
                t!("api.password").to_string(),
                &auth.password,
                AuthField::Password,
                true
            ),
        ]
        .spacing(8)
        .into(),
        AuthKind::ApiKey => column![
            field(
                t!("api.key_name").to_string(),
                &auth.key_name,
                AuthField::KeyName,
                false
            ),
            field(
                t!("api.key_value").to_string(),
                &auth.key_value,
                AuthField::KeyValue,
                true
            ),
            row![
                button(text(t!("api.key_in_header").to_string()).size(12.5))
                    .padding([3, 10])
                    .on_press(say(path, Act::KeyInHeader(true)))
                    .style(ghost(dark, auth.key_in_header)),
                button(text(t!("api.key_in_query").to_string()).size(12.5))
                    .padding([3, 10])
                    .on_press(say(path, Act::KeyInHeader(false)))
                    .style(ghost(dark, !auth.key_in_header)),
            ]
            .spacing(4),
        ]
        .spacing(8)
        .into(),
    };
    let hint = if auth.kind == AuthKind::None {
        String::new()
    } else {
        t!("api.auth_variables").to_string()
    };
    column![kinds, content, text(hint).size(11.5).color(p.muted)]
        .spacing(10)
        .into()
}

fn settings_section<'a>(path: &Path, tab: &'a ApiTab, dark: bool) -> Element<'a, Message> {
    let p = se_theme::palette(dark);
    let settings = tab.request.settings;
    let (redirect_path, tls_path, timeout_path) =
        (path.to_path_buf(), path.to_path_buf(), path.to_path_buf());
    column![
        row![
            text(t!("api.timeout").to_string())
                .size(12.5)
                .color(p.muted)
                .width(Length::Fixed(150.0)),
            text_input("30", &settings.timeout_secs.to_string())
                .on_input(move |t| Message::Api(ApiMsg::Tab(timeout_path.clone(), Act::Timeout(t))))
                .size(12.5)
                .padding([3, 8])
                .width(Length::Fixed(80.0))
                .style(se_theme::Field::new(dark).style()),
            text(t!("api.timeout_hint").to_string())
                .size(11.5)
                .color(p.muted),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
        checkbox(settings.follow_redirects)
            .label(t!("api.redirects").to_string())
            .size(14)
            .text_size(12.5)
            .on_toggle(move |on| Message::Api(ApiMsg::Tab(
                redirect_path.clone(),
                Act::Redirects(on)
            ))),
        checkbox(settings.verify_tls)
            .label(t!("api.verify_tls").to_string())
            .size(14)
            .text_size(12.5)
            .on_toggle(move |on| Message::Api(ApiMsg::Tab(tls_path.clone(), Act::VerifyTls(on)))),
    ]
    .spacing(10)
    .into()
}

// ----------------------------------------------------------------- the answer

fn response_view<'a>(path: &Path, tab: &'a ApiTab, dark: bool) -> Element<'a, Message> {
    let p = se_theme::palette(dark);
    if tab.sending {
        return container(text(t!("api.sending").to_string()).size(13).color(p.muted))
            .padding(16)
            .into();
    }
    if let Some(error) = &tab.error {
        return container(
            text(error.as_str())
                .size(13)
                .color(se_theme::error_color(dark)),
        )
        .padding(16)
        .into();
    }
    let Some(view) = &tab.response else {
        return container(
            text(t!("api.no_response").to_string())
                .size(13)
                .color(p.muted),
        )
        .padding(16)
        .into();
    };
    let response = &view.response;
    let summary = row![
        text(format!("{} {}", response.status, response.reason))
            .size(13)
            .font(se_theme::UI_SEMIBOLD)
            .color(status_color(response.status, dark)),
        text(format!("{} ms", response.ms)).size(12).color(p.muted),
        text(size(response.body.len())).size(12).color(p.muted),
    ]
    .spacing(14)
    .align_y(Alignment::Center);
    let section = |label: String, which: RespSection| {
        button(text(label).size(12.5))
            .padding([3, 10])
            .on_press(say(path, Act::Resp(which)))
            .style(ghost(dark, tab.resp_section == which))
    };
    let bar = row![
        summary,
        Space::new().width(Length::Fill),
        section(t!("api.body").to_string(), RespSection::Body),
        section(
            format!("{} ({})", t!("api.headers"), response.headers.len()),
            RespSection::Headers
        ),
        section(
            format!("{} ({})", t!("api.cookies"), response.cookies.len()),
            RespSection::Cookies
        ),
        section(
            format!(
                "{} ({}/{})",
                t!("api.tests"),
                view.outcomes.iter().filter(|o| o.passed).count(),
                view.outcomes.len()
            ),
            RespSection::Tests
        ),
    ]
    .spacing(6)
    .padding([8, 12])
    .align_y(Alignment::Center);
    let content: Element<Message> = match tab.resp_section {
        RespSection::Body => body_view(path, view, dark),
        RespSection::Tests => outcomes_view(view, dark),
        RespSection::Headers => {
            let mut list = column![].spacing(4).padding([0, 12]);
            for (k, v) in &response.headers {
                list = list.push(
                    row![
                        text(k.as_str())
                            .size(12.5)
                            .font(se_theme::UI_SEMIBOLD)
                            .width(Length::Fixed(200.0)),
                        text(v.as_str()).size(12.5).font(se_theme::MONO),
                    ]
                    .spacing(12),
                );
            }
            scrollable(list)
                .style(se_theme::Scrollbar { dark }.style())
                .height(Length::Fill)
                .into()
        }
        RespSection::Cookies => {
            if response.cookies.is_empty() {
                container(
                    text(t!("api.no_cookies").to_string())
                        .size(12.5)
                        .color(p.muted),
                )
                .padding([0, 12])
                .into()
            } else {
                let mut list = column![].spacing(4).padding([0, 12]);
                for c in &response.cookies {
                    list = list.push(
                        row![
                            text(c.name.as_str())
                                .size(12.5)
                                .font(se_theme::UI_SEMIBOLD)
                                .width(Length::Fixed(160.0)),
                            text(c.value.as_str())
                                .size(12.5)
                                .font(se_theme::MONO)
                                .width(Length::Fixed(220.0)),
                            text(c.attributes.as_str()).size(12).color(p.muted),
                        ]
                        .spacing(12),
                    );
                }
                scrollable(list)
                    .style(se_theme::Scrollbar { dark }.style())
                    .height(Length::Fill)
                    .into()
            }
        }
    };
    column![bar, content].height(Length::Fill).into()
}

fn body_view<'a>(path: &Path, view: &'a super::ResponseView, dark: bool) -> Element<'a, Message> {
    let p = se_theme::palette(dark);
    let save = button(text(t!("api.save_body").to_string()).size(12))
        .padding([2, 10])
        .on_press(say(path, Act::SaveBody))
        .style((se_theme::Outline { dark }).style());
    if view.kind == Kind::Binary {
        return container(
            column![
                text(t!("api.binary", size = size(view.response.body.len())).to_string())
                    .size(12.5)
                    .color(p.muted),
                save,
            ]
            .spacing(10),
        )
        .padding([0, 12])
        .into();
    }
    let mut tools = row![]
        .spacing(8)
        .padding([0, 12])
        .align_y(Alignment::Center);
    if view.pretty.is_some() {
        tools = tools
            .push(
                button(text(t!("api.pretty").to_string()).size(12))
                    .padding([2, 10])
                    .on_press(say(path, Act::Pretty(true)))
                    .style(ghost(dark, view.show_pretty)),
            )
            .push(
                button(text(t!("api.raw").to_string()).size(12))
                    .padding([2, 10])
                    .on_press(say(path, Act::Pretty(false)))
                    .style(ghost(dark, !view.show_pretty)),
            );
    }
    let search_path = path.to_path_buf();
    let found = view.matches();
    tools = tools
        .push(
            text_input(&t!("api.search"), &view.search)
                .on_input(move |t| Message::Api(ApiMsg::Tab(search_path.clone(), Act::Search(t))))
                .on_submit(say(path, Act::SearchNext))
                .size(12)
                .padding([2, 8])
                .width(Length::Fixed(200.0))
                .style(se_theme::Field::new(dark).style()),
        )
        .push(
            text(if view.search.is_empty() {
                String::new()
            } else {
                t!("api.matches", n = found).to_string()
            })
            .size(12)
            .color(p.muted),
        )
        .push(Space::new().width(Length::Fill))
        .push(
            button(text(t!("api.copy").to_string()).size(12))
                .padding([2, 10])
                .on_press(say(path, Act::CopyBody))
                .style((se_theme::Outline { dark }).style()),
        )
        .push(save);
    let extension = match view.kind {
        Kind::Json => "json",
        Kind::Xml => "xml",
        Kind::Html => "html",
        _ => "txt",
    };
    let edit_path = path.to_path_buf();
    let editor = text_editor(&view.editor)
        .on_action(move |a| Message::Api(ApiMsg::Tab(edit_path.clone(), Act::RespEdit(a))))
        .highlight_with::<crate::editor::highlighter::Highlighter>(
            crate::editor::highlighter::Settings {
                dark,
                extension: extension.to_string(),
            },
            crate::editor::highlighter::to_format,
        )
        .font(se_theme::MONO)
        .size(13)
        .padding([8, 12])
        .height(Length::Fill);
    let mut out = column![tools, editor].spacing(6).height(Length::Fill);
    if view.cut {
        out = out.push(
            container(
                text(t!("api.cut", size = size(super::MAX_SHOWN)).to_string())
                    .size(11.5)
                    .color(p.muted),
            )
            .padding([0, 12]),
        );
    }
    if view.response.truncated {
        out = out.push(
            container(
                text(t!("api.truncated", size = size(super::http::MAX_BODY)).to_string())
                    .size(11.5)
                    .color(se_theme::error_color(dark)),
            )
            .padding([0, 12]),
        );
    }
    out.into()
}

// -------------------------------------------------------------------- windows

/// The window that is open over the panel, if one is.
pub fn dialog<'a>(state: &'a State, dark: bool) -> Option<Element<'a, Message>> {
    let p = se_theme::palette(dark);
    let dialog = state.dialog.as_ref()?;
    let head = |title: String| {
        row![
            text(title).size(13.5).font(se_theme::UI_SEMIBOLD),
            Space::new().width(Length::Fill),
            button(text("×").size(16).color(p.muted))
                .padding([0, 7])
                .on_press(api(ApiMsg::DialogClose))
                .style(ghost(dark, false)),
        ]
        .align_y(Alignment::Center)
    };
    let ok_button = |label: String, message: Message| {
        button(text(label).size(12.5))
            .padding([5, 16])
            .on_press(message)
            .style((se_theme::Primary { dark }).style())
    };
    let card: Element<Message> = match dialog {
        Dialog::Name { what, text: value } => {
            let title = match what {
                NameFor::NewCollection => t!("api.new_collection"),
                NameFor::NewFolder(..) => t!("api.new_folder"),
                NameFor::Rename(..) => t!("api.rename"),
                NameFor::NewEnvironment => t!("api.new_environment"),
            };
            container(
                column![
                    head(title.to_string()),
                    text_input(&t!("api.name_hint"), value)
                        .on_input(|t| api(ApiMsg::DialogText(t)))
                        .on_submit(api(ApiMsg::DialogOk))
                        .size(13)
                        .padding([5, 8])
                        .style(se_theme::Field::new(dark).style()),
                    ok_button(t!("api.ok").to_string(), api(ApiMsg::DialogOk)),
                ]
                .spacing(10)
                .width(Length::Fixed(360.0)),
            )
            .padding(16)
            .style(se_theme::card(dark))
            .into()
        }
        Dialog::Delete { what } => {
            let message = match what {
                super::Removal::Item(coll, 0) => state
                    .collection(*coll)
                    .map(|c| t!("api.delete_collection", name = c.name.as_str()).to_string())
                    .unwrap_or_default(),
                super::Removal::Item(..) => t!("api.delete_item").to_string(),
                super::Removal::Environment(id) => state
                    .environment(*id)
                    .map(|e| t!("api.delete_environment", name = e.name.as_str()).to_string())
                    .unwrap_or_default(),
            };
            container(
                column![
                    head(t!("api.delete").to_string()),
                    text(message).size(12.5),
                    ok_button(t!("api.delete").to_string(), api(ApiMsg::DialogOk)),
                ]
                .spacing(10)
                .width(Length::Fixed(380.0)),
            )
            .padding(16)
            .style(se_theme::card(dark))
            .into()
        }
        Dialog::Curl => {
            let mut body = column![
                head(t!("api.import_curl").to_string()),
                text(t!("api.curl_hint").to_string())
                    .size(12)
                    .color(p.muted),
                text_editor(&state.curl)
                    .on_action(|a| api(ApiMsg::CurlAction(a)))
                    .font(se_theme::MONO)
                    .size(12.5)
                    .padding([8, 10])
                    .height(Length::Fixed(200.0)),
            ]
            .spacing(10)
            .width(Length::Fixed(620.0));
            if let Some(error) = &state.curl_error {
                body = body.push(
                    text(error.as_str())
                        .size(12.5)
                        .color(se_theme::error_color(dark)),
                );
            }
            body = body.push(ok_button(
                t!("api.import").to_string(),
                api(ApiMsg::CurlRead),
            ));
            container(body)
                .padding(16)
                .style(se_theme::card(dark))
                .into()
        }
        Dialog::Report(lines) => {
            let mut list = column![].spacing(4);
            for l in lines {
                list = list.push(text(l.as_str()).size(12.5));
            }
            container(
                column![
                    head(t!("api.import_report").to_string()),
                    scrollable(list)
                        .style(se_theme::Scrollbar { dark }.style())
                        .height(Length::Shrink),
                    ok_button(t!("api.ok").to_string(), api(ApiMsg::DialogClose)),
                ]
                .spacing(10)
                .width(Length::Fixed(560.0)),
            )
            .max_height(420)
            .padding(16)
            .style(se_theme::card(dark))
            .into()
        }
        Dialog::Environments => environments(state, dark),
    };
    Some(card)
}

/// The environments: the list, and the variables of the one that is open.
fn environments<'a>(state: &'a State, dark: bool) -> Element<'a, Message> {
    let p = se_theme::palette(dark);
    let mut list = column![].spacing(2).width(Length::Fixed(170.0));
    for env in &state.environments {
        let mark = if state.active_env == Some(env.id) {
            "● "
        } else {
            ""
        };
        list = list.push(
            button(text(format!("{mark}{}", env.name)).size(12.5))
                .padding([3, 8])
                .width(Length::Fill)
                .on_press(api(ApiMsg::EditEnv(env.id)))
                .style(ghost(dark, state.editing_env == Some(env.id))),
        );
    }
    list = list.push(
        button(text(format!("+ {}", t!("api.new_environment"))).size(12.5))
            .padding([3, 8])
            .on_press(api(ApiMsg::AskNewEnv))
            .style((se_theme::Outline { dark }).style()),
    );
    let editor: Element<Message> = match state.editing_env.and_then(|id| state.environment(id)) {
        None => text(t!("api.env_none").to_string())
            .size(12.5)
            .color(p.muted)
            .into(),
        Some(env) => {
            let mut vars = column![row![
                Space::new().width(Length::Fixed(26.0)),
                text(t!("api.key").to_string())
                    .size(11.5)
                    .color(p.muted)
                    .width(Length::FillPortion(1)),
                text(t!("api.value").to_string())
                    .size(11.5)
                    .color(p.muted)
                    .width(Length::FillPortion(2)),
                text(t!("api.secret").to_string())
                    .size(11.5)
                    .color(p.muted)
                    .width(Length::Fixed(60.0)),
                Space::new().width(Length::Fixed(30.0)),
            ]
            .spacing(6)]
            .spacing(4);
            for (i, var) in env.vars.iter().enumerate() {
                vars = vars.push(
                    row![
                        checkbox(var.enabled)
                            .size(14)
                            .on_toggle(move |on| api(ApiMsg::VarEnabled(i, on))),
                        text_input(&t!("api.key"), &var.key)
                            .on_input(move |t| api(ApiMsg::VarKey(i, t)))
                            .size(12.5)
                            .padding([3, 6])
                            .width(Length::FillPortion(1))
                            .style(se_theme::Field::new(dark).style()),
                        text_input(&t!("api.value"), &var.value)
                            .on_input(move |t| api(ApiMsg::VarValue(i, t)))
                            .secure(var.secret)
                            .size(12.5)
                            .font(se_theme::MONO)
                            .padding([3, 6])
                            .width(Length::FillPortion(2))
                            .style(se_theme::Field::new(dark).style()),
                        container(
                            checkbox(var.secret)
                                .size(14)
                                .on_toggle(move |on| api(ApiMsg::VarSecret(i, on)))
                        )
                        .width(Length::Fixed(60.0)),
                        button(text("×").size(14).color(p.muted))
                            .padding([0, 7])
                            .on_press(api(ApiMsg::VarDel(i)))
                            .style(ghost(dark, false)),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center),
                );
            }
            let id = env.id;
            column![
                text_input(&t!("api.name_hint"), &env.name)
                    .on_input(|t| api(ApiMsg::EnvName(t)))
                    .size(13)
                    .padding([4, 8])
                    .style(se_theme::Field::new(dark).style()),
                scrollable(vars)
                    .style(se_theme::Scrollbar { dark }.style())
                    .height(Length::Fixed(280.0)),
                row![
                    button(text(format!("+ {}", t!("api.add_variable"))).size(12.5))
                        .padding([3, 10])
                        .on_press(api(ApiMsg::VarAdd))
                        .style((se_theme::Outline { dark }).style()),
                    Space::new().width(Length::Fill),
                    button(text(t!("api.use_env").to_string()).size(12.5))
                        .padding([3, 10])
                        .on_press(api(ApiMsg::SelectEnv(Some(id))))
                        .style((se_theme::Primary { dark }).style()),
                    button(text(t!("api.duplicate").to_string()).size(12.5))
                        .padding([3, 10])
                        .on_press(api(ApiMsg::DuplicateEnv(id)))
                        .style((se_theme::Outline { dark }).style()),
                    button(text(t!("api.export").to_string()).size(12.5))
                        .padding([3, 10])
                        .on_press(api(ApiMsg::ExportEnv(id)))
                        .style((se_theme::Outline { dark }).style()),
                    button(text(t!("api.delete").to_string()).size(12.5))
                        .padding([3, 10])
                        .on_press(api(ApiMsg::AskDeleteEnv(id)))
                        .style((se_theme::Outline { dark }).style()),
                ]
                .spacing(6),
                text(t!("api.env_hint").to_string())
                    .size(11.5)
                    .color(p.muted),
            ]
            .spacing(10)
            .width(Length::Fill)
            .into()
        }
    };
    container(
        column![
            row![
                text(t!("api.environments").to_string())
                    .size(13.5)
                    .font(se_theme::UI_SEMIBOLD),
                Space::new().width(Length::Fill),
                button(text("×").size(16).color(p.muted))
                    .padding([0, 7])
                    .on_press(api(ApiMsg::DialogClose))
                    .style(ghost(dark, false)),
            ]
            .align_y(Alignment::Center),
            row![list, editor].spacing(16),
        ]
        .spacing(12)
        .width(Length::Fixed(780.0)),
    )
    .padding(16)
    .style(se_theme::card(dark))
    .into()
}
