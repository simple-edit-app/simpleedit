//! The macOS menu bar: File, Edit, View and Help in the system bar at the top
//! of the screen, next to the application menu, instead of inside the window.
//!
//! Items carry no accelerator: the shortcuts are configurable and already
//! handled by the app's own key handling, a native accelerator would fire
//! them twice. Choosing an item sends the same `Message` as its in-window
//! counterpart on the other platforms.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use iced::futures::{SinkExt, Stream};
use muda::{CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu};
use tokio::sync::mpsc;

use crate::app::{HistoryMsg, Message};
use crate::config::Config;
use crate::menu_bar::BookmarkState;
use crate::preferences::PreferencesMessage;

/// What the menu shows; it is rebuilt when any of it changes.
pub struct State<'a> {
    pub config: &'a Config,
    pub has_formatter: bool,
    pub lang_picker_enabled: bool,
    pub bookmarks: BookmarkState,
}

impl State<'_> {
    /// Everything the menu's labels and enabled states depend on.
    pub fn signature(&self) -> String {
        let c = self.config;
        format!(
            "{}|{}|{}|{:?}|{}|{}|{}|{}",
            &*rust_i18n::locale(),
            c.dark_mode,
            c.show_line_numbers,
            c.recent_files,
            self.has_formatter,
            self.lang_picker_enabled,
            self.bookmarks.on_line,
            self.bookmarks.any,
        )
    }
}

type Actions = HashMap<MenuId, Message>;

static ACTIONS: OnceLock<Mutex<Actions>> = OnceLock::new();
static EVENTS: OnceLock<Mutex<Option<mpsc::UnboundedReceiver<MenuId>>>> = OnceLock::new();

fn actions() -> &'static Mutex<Actions> {
    ACTIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The messages of the items the user chooses. Runs for the life of the app.
pub fn subscription() -> iced::Subscription<Message> {
    iced::Subscription::run(events)
}

fn events() -> impl Stream<Item = Message> {
    iced::stream::channel(32, async |mut output| {
        let receiver = channel().lock().ok().and_then(|mut r| r.take());
        let Some(mut receiver) = receiver else {
            return std::future::pending().await;
        };
        while let Some(id) = receiver.recv().await {
            let message = actions().lock().ok().and_then(|a| a.get(&id).cloned());
            if let Some(message) = message {
                let _ = output.send(message).await;
            }
        }
    })
}

/// The channel the menu events go through, made by whoever comes first: the
/// subscription starts before the menu is installed.
fn channel() -> &'static Mutex<Option<mpsc::UnboundedReceiver<MenuId>>> {
    EVENTS.get_or_init(|| {
        let (tx, rx) = mpsc::unbounded_channel();
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            let _ = tx.send(event.id);
        }));
        Mutex::new(Some(rx))
    })
}

struct Builder {
    actions: Actions,
}

impl Builder {
    fn add(&mut self, menu: &Submenu, label: impl AsRef<str>, enabled: bool, message: Message) {
        let item = MenuItem::new(label, enabled, None);
        self.actions.insert(item.id().clone(), message);
        let _ = menu.append(&item);
    }

    fn on(&mut self, menu: &Submenu, label: impl AsRef<str>, message: Message) {
        self.add(menu, label, true, message);
    }
}

fn separator(menu: &Submenu) {
    let _ = menu.append(&PredefinedMenuItem::separator());
}

/// Installs the menu bar, replacing the previous one.
pub fn install(state: &State) {
    channel();
    let mut b = Builder {
        actions: HashMap::new(),
    };
    let menu = Menu::new();

    let _ = menu.append(&app_menu(&mut b));
    let _ = menu.append(&file_menu(&mut b, state));
    let _ = menu.append(&edit_menu(&mut b, state));
    let _ = menu.append(&view_menu(&mut b, state));
    let help = help_menu(&mut b);
    let _ = menu.append(&help);

    if let Ok(mut a) = actions().lock() {
        *a = b.actions;
    }
    menu.init_for_nsapp();
    help.set_as_help_menu_for_nsapp();
    // The menu must outlive this call: the system holds it by reference.
    thread_local!(static KEEP: std::cell::RefCell<Option<Menu>> = const { std::cell::RefCell::new(None) });
    KEEP.with(|keep| *keep.borrow_mut() = Some(menu));
}

/// The first menu, named after the app by the system.
fn app_menu(b: &mut Builder) -> Submenu {
    let menu = Submenu::new("SimpleEdit", true);
    b.on(&menu, t!("menu.help_about"), Message::About);
    separator(&menu);
    b.on(
        &menu,
        t!("menu.file_preferences"),
        Message::TogglePreferences,
    );
    separator(&menu);
    let _ = menu.append(&PredefinedMenuItem::services(None));
    separator(&menu);
    let _ = menu.append(&PredefinedMenuItem::hide(None));
    let _ = menu.append(&PredefinedMenuItem::hide_others(None));
    let _ = menu.append(&PredefinedMenuItem::show_all(None));
    separator(&menu);
    // Not the system's Quit: it must go through the app to save the session.
    b.on(&menu, t!("menu.file_quit"), Message::Quit);
    menu
}

fn file_menu(b: &mut Builder, state: &State) -> Submenu {
    let menu = Submenu::new(t!("menu.file"), true);
    b.on(&menu, t!("menu.file_new"), Message::NewFile);
    b.on(&menu, t!("menu.file_open"), Message::OpenFile);
    b.on(&menu, t!("menu.file_add_project"), Message::AddProject);
    b.on(&menu, t!("shortcuts.goto_file"), Message::OpenGotoFile);
    b.on(
        &menu,
        t!("shortcuts.find_in_files"),
        Message::OpenFindInFiles,
    );
    b.on(
        &menu,
        t!("shortcuts.replace_in_files"),
        Message::OpenReplaceInFiles,
    );
    b.on(
        &menu,
        t!("history.show"),
        Message::History(HistoryMsg::OpenCurrent),
    );
    b.on(
        &menu,
        t!("history.put_label"),
        Message::History(HistoryMsg::AskLabel),
    );
    b.on(&menu, t!("shortcuts.reopen_tab"), Message::ReopenTab);
    separator(&menu);

    let recents = state.config.recent_files.as_slice();
    let sub = Submenu::new(t!("menu.file_recents"), !recents.is_empty());
    let paths: Vec<PathBuf> = recents.iter().map(PathBuf::from).collect();
    for (path, name) in paths.iter().zip(crate::app::unique_labels(&paths)) {
        b.on(&sub, name, Message::OpenRecentFile(path.clone()));
    }
    separator(&sub);
    b.on(&sub, t!("menu.file_recents_clear"), Message::ClearRecents);
    let _ = menu.append(&sub);
    separator(&menu);

    b.on(&menu, t!("menu.file_save"), Message::SaveFile);
    b.on(&menu, t!("menu.file_save_as"), Message::SaveFileAs);
    b.on(&menu, t!("menu.file_close"), Message::CloseFile);
    b.on(&menu, t!("menu.file_close_others"), Message::CloseOtherTabs);
    b.on(&menu, t!("menu.file_close_all"), Message::CloseTabs(None));
    menu
}

fn edit_menu(b: &mut Builder, state: &State) -> Submenu {
    let menu = Submenu::new(t!("menu.edit"), true);
    b.on(&menu, t!("menu.edit_undo"), Message::Undo);
    b.on(&menu, t!("menu.edit_redo"), Message::Redo);
    separator(&menu);
    b.on(
        &menu,
        t!("menu.edit_duplicate_line"),
        Message::DuplicateLine,
    );
    b.on(&menu, t!("menu.edit_move_line_up"), Message::MoveLineUp);
    b.on(&menu, t!("menu.edit_move_line_down"), Message::MoveLineDown);
    b.on(
        &menu,
        t!("menu.edit_toggle_comment"),
        Message::ToggleComment,
    );
    b.on(&menu, t!("menu.edit_delete_line"), Message::DeleteLine);
    separator(&menu);
    b.on(&menu, t!("menu.edit_select_all"), Message::SelectAll);
    b.on(&menu, t!("menu.edit_find"), Message::ToggleSearch);
    b.on(&menu, t!("menu.edit_goto_line"), Message::OpenGotoLine);
    separator(&menu);
    b.on(&menu, t!("menu.lint_check"), Message::LintNow);
    b.on(&menu, t!("menu.lint_fix"), Message::LintFixAll);
    b.on(&menu, t!("menu.lint_next"), Message::LintNext(true));
    b.on(&menu, t!("menu.lint_prev"), Message::LintNext(false));
    separator(&menu);

    let marks = state.bookmarks;
    let toggle = if marks.on_line {
        t!("menu.bookmark_remove")
    } else {
        t!("menu.bookmark_add")
    };
    b.on(&menu, toggle, Message::ToggleBookmark);
    b.add(
        &menu,
        t!("menu.bookmark_clear"),
        marks.any,
        Message::ClearBookmarks,
    );
    b.add(
        &menu,
        t!("menu.bookmark_next"),
        marks.any,
        Message::NextBookmark,
    );
    b.add(
        &menu,
        t!("menu.bookmark_prev"),
        marks.any,
        Message::PrevBookmark,
    );
    separator(&menu);
    b.on(&menu, t!("menu.nav_back"), Message::NavBack);
    b.on(&menu, t!("menu.nav_forward"), Message::NavForward);
    separator(&menu);
    b.on(&menu, t!("menu.fold_toggle"), Message::ToggleFold);
    b.on(&menu, t!("menu.fold_all"), Message::FoldAll);
    b.on(&menu, t!("menu.unfold_all"), Message::UnfoldAll);
    separator(&menu);
    b.on(&menu, t!("shortcuts.complete"), Message::CompletionInvoke);
    b.on(&menu, t!("shortcuts.parameter_info"), Message::HintInvoke);
    b.add(
        &menu,
        t!("menu.edit_format"),
        state.has_formatter,
        Message::FormatFile,
    );
    menu
}

fn view_menu(b: &mut Builder, state: &State) -> Submenu {
    let config = state.config;
    let menu = Submenu::new(t!("menu.view"), true);
    b.on(&menu, t!("menu.view_palette"), Message::OpenPalette);
    b.on(&menu, t!("menu.view_sidebar"), Message::ToggleSidebar);
    b.on(&menu, t!("menu.view_split_right"), Message::Split(true));
    b.on(&menu, t!("menu.view_split_down"), Message::Split(false));
    b.on(&menu, t!("menu.view_split_close"), Message::CloseSplit);
    b.on(&menu, t!("git.title"), Message::ToggleGit);
    b.on(&menu, t!("terminal.title"), Message::ToggleTerminal);
    for (label, dark) in [(t!("toolbar.dark"), true), (t!("toolbar.light"), false)] {
        let item = CheckMenuItem::new(label, true, config.dark_mode == dark, None);
        b.actions
            .insert(item.id().clone(), Message::ThemeChanged(dark));
        let _ = menu.append(&item);
    }
    let numbers = if config.show_line_numbers {
        t!("menu.view_line_numbers_hide")
    } else {
        t!("menu.view_line_numbers_show")
    };
    b.on(
        &menu,
        numbers,
        Message::Preferences(PreferencesMessage::ShowLineNumbersToggled(
            !config.show_line_numbers,
        )),
    );
    b.add(
        &menu,
        t!("menu.view_language"),
        state.lang_picker_enabled,
        Message::OpenLanguagePicker,
    );
    menu
}

fn help_menu(b: &mut Builder) -> Submenu {
    let menu = Submenu::new(t!("menu.help"), true);
    b.on(&menu, t!("menu.help_shortcuts"), Message::OpenShortcuts);
    b.on(&menu, t!("menu.help_changelog"), Message::OpenChangelog);
    b.on(
        &menu,
        t!("menu.help_check_updates"),
        Message::CheckForUpdate,
    );
    menu
}
