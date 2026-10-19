use iced::{
    executor,
    widget::{button, column, container, row, scrollable, text, text_editor, text_input, Space},
    Alignment, Application, Command, Element, Length, Theme,
};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::{
    config::{Config, ShortcutConfig, ShortcutTarget},
    editor::EditorState,
    preferences::{PreferencesMessage, PreferencesState},
    preview::{self, PreviewKind, ViewMode},
    search::{SearchMessage, SearchState},
    sidebar::{SidebarAction, SidebarMessage, SidebarState},
    theme as se_theme,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TopMenu {
    File,
    Edit,
    View,
    Help,
}

/// Which modal overlay (if any) is currently shown.
#[derive(Debug, Clone, PartialEq)]
pub enum ActiveOverlay {
    None,
    About,
    GotoLine,
    LanguagePicker,
    Shortcuts {
        capturing: Option<ShortcutTarget>,
    },
    UpdateChecking,
    UpdateUpToDate,
    UpdateAvailable(String),
    UpdateDownloading,
    UpdateInstalling,
    UpdateError(String),
    /// Closing this tab would discard unsaved changes.
    ConfirmClose(PathBuf),
    /// Saving would overwrite changes made to the file outside the editor.
    ConfirmOverwrite(PathBuf),
}

#[derive(Debug, Clone)]
pub enum Message {
    EditorAction(text_editor::Action),
    Sidebar(SidebarMessage),
    Search(SearchMessage),
    Preferences(PreferencesMessage),
    OpenFile,
    FileOpened(Result<(PathBuf, String), String>),
    SaveFile,
    SaveFileAs,
    /// The tab the save was started from, and the path + exact text written.
    FileSaved(Option<PathBuf>, Result<(PathBuf, String), String>),
    NewFile,
    CloseFile,
    ToggleSearch,
    ToggleSidebar,
    TogglePreferences,
    ThemeChanged(bool),
    InsertTab,
    ToggleMenu(TopMenu),
    /// The pointer entered a menu title: switches menus if one is already open.
    HoverMenu(TopMenu),
    /// Resolves the on-screen position of an open top menu's button, so its
    /// dropdown can be anchored under it regardless of label/font/DPI.
    MenuBounds(TopMenu, Option<iced::Rectangle>),
    SelectAll,
    Quit,
    About,
    ContextCopy,
    ContextCut,
    ContextPaste,
    ContextDelete,
    ClipboardContent(Option<String>),
    FormatFile,
    FormatSelection,
    FileFormatted(Result<String, String>),
    SelectionFormatted(Result<String, String>),
    CloseErrorPanel,
    Undo,
    Redo,
    AutoSave,
    // Raw key press forwarded from the subscription (non-capturing fn pointer)
    KeyPressed {
        key: String,
        ctrl: bool,
        shift: bool,
        alt: bool,
    },
    EscapePressed,
    // Overlays
    CloseOverlay,
    OpenGotoLine,
    GotoLineInputChanged(String),
    GotoLineSubmit,
    OpenLanguagePicker,
    SetLanguage(String),
    OpenShortcuts,
    StartCaptureShortcut(ShortcutTarget),
    // Deferred cursor move after autocomplete pair insertion (layout must run first to shape buffer)
    MoveCursorLeft,
    // Line-level editing
    DuplicateLine,
    MoveLineUp,
    MoveLineDown,
    ToggleComment,
    DeleteLine,
    IndentSelection,
    DedentLine,
    // Recent files
    OpenRecentFile(PathBuf),
    // Deferred cursor restore (line, byte column) after with_text() (layout must run first)
    RestoreCursor(usize, usize),
    /// A message re-sent after a short delay because the editor had edits
    /// not yet laid out (see `layout_stale`); the count bounds the retries.
    Deferred(Box<Message>, u8),
    /// Whether a search field had focus when an editor shortcut was pressed.
    ShortcutChecked(Box<Message>, bool),
    /// The user confirmed closing a tab that has unsaved changes.
    ConfirmDiscard(PathBuf),
    /// The user chose to overwrite a file that changed on disk.
    ConfirmOverwrite,
    // Updater
    CheckForUpdate,
    UpdateChecked(Result<Option<String>, String>),
    ConfirmInstallUpdate(String),
    UpdateDownloaded(Result<std::path::PathBuf, String>),
    UpdateInstalled(Result<(), String>),
    // Markdown / JSON preview
    SetViewMode(ViewMode),
    JsonToggle(String),
    JsonExpandAll,
    JsonCollapseAll,
    /// Live modifier state, so a chevron click can tell Ctrl+click from Alt+click.
    ModifiersChanged {
        ctrl: bool,
        alt: bool,
    },
    /// An in-document anchor link (`#slug`) was clicked in the Markdown
    /// preview.
    MarkdownLinkClicked(String),
    /// Resolves the position of the heading `MarkdownLinkClicked` targeted.
    PreviewScrollTo(Option<f32>),
}

pub struct SimpleEditApp {
    config: Config,
    editor: EditorState,
    sidebar: SidebarState,
    search: SearchState,
    preferences: PreferencesState,
    show_search: bool,
    show_sidebar: bool,
    show_preferences: bool,
    open_menu: Option<TopMenu>,
    /// Last known screen position of each menu button. Reused while a fresh
    /// query is in flight so switching menus doesn't flicker.
    menu_bounds: HashMap<TopMenu, iced::Rectangle>,
    current_file: Option<PathBuf>,
    is_dirty: bool,
    is_formatting: bool,
    status_message: String,
    status_is_error: bool,
    format_error: Option<String>,
    show_error_panel: bool,
    untitled_counter: u32,
    file_cache: HashMap<PathBuf, (String, Option<String>, bool)>,
    overlay: ActiveOverlay,
    goto_line_input: String,
    /// Word-level undo history (capped by count and total size).
    undo_stack: Vec<Snapshot>,
    /// Redo history rebuilt when undoing.
    redo_stack: Vec<Snapshot>,
    /// Undo/redo history of the tabs that aren't the active one.
    undo_cache: HashMap<PathBuf, (Vec<Snapshot>, Vec<Snapshot>)>,
    /// Set whenever the buffer may have been edited, cleared when `view` runs
    /// (the layout that shapes the edited lines follows it). Motions across
    /// unshaped lines panic inside cosmic-text, so operations that move the
    /// cursor wait for it to be clear.
    layout_stale: std::cell::Cell<bool>,
    /// Preference changes are written out on the next autosave tick, not on
    /// every slider movement.
    config_dirty: bool,
    /// The last session JSON written, so unchanged sessions aren't rewritten.
    last_session: String,
    /// Modification time of the current file when it was last read or written,
    /// to notice edits made by other programs.
    disk_mtime: Option<std::time::SystemTime>,
    /// Whether the "changed on disk" warning was already shown.
    external_warned: bool,
    /// Size on disk of `size_for`, cached so `view` does no I/O.
    file_size: Option<u64>,
    size_for: Option<PathBuf>,
    /// Whether the next edit should open a new undo group.
    /// True at start, after whitespace, after a cursor move, after delete.
    undo_new_group: bool,
    /// How the editor pane renders the buffer (Markdown / JSON files only).
    view_mode: ViewMode,
    /// Parsed preview of the buffer, rebuilt only when the text changes.
    preview: PreviewDoc,
    /// The buffer `preview` was built from.
    preview_src: String,
    /// Folded node paths of the JSON tree.
    json_collapsed: std::sync::Arc<HashSet<String>>,
    /// Bumped whenever what the preview shows changes, so the cached widget
    /// tree (iced `lazy`) is rebuilt only then, not on every frame.
    preview_rev: u64,
    /// Ctrl/Cmd currently held, tracked for Ctrl+click on a chevron.
    mod_ctrl: bool,
    /// Alt currently held, tracked for Alt+click on a chevron.
    mod_alt: bool,
}

/// Cap on the memory the undo history of one buffer may hold.
const UNDO_MAX_BYTES: usize = 64 * 1024 * 1024;
const UNDO_MAX_STEPS: usize = 100;
/// Files larger than this are refused instead of freezing the editor.
const MAX_OPEN_BYTES: u64 = 64 * 1024 * 1024;
/// Marker error for a dialog the user dismissed: not worth a status message.
const CANCELLED: &str = "cancelled";

/// A buffer state with its cursor (line, byte column).
#[derive(Debug, Clone)]
struct Snapshot {
    text: String,
    cursor: (usize, usize),
}

fn mtime(path: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn trim_history(stack: &mut Vec<Snapshot>) {
    let mut bytes: usize = stack.iter().map(|s| s.text.len()).sum();
    while stack.len() > UNDO_MAX_STEPS || (bytes > UNDO_MAX_BYTES && stack.len() > 1) {
        bytes -= stack.remove(0).text.len();
    }
}

/// The cached parse backing the preview pane.
enum PreviewDoc {
    None,
    Markdown(std::sync::Arc<Vec<crate::preview::markdown::Block>>),
    Json(std::sync::Arc<serde_json::Value>),
    JsonError(String),
}

impl Application for SimpleEditApp {
    type Executor = executor::Default;
    type Message = Message;
    type Theme = Theme;
    type Flags = Option<PathBuf>;

    fn new(flags: Self::Flags) -> (Self, Command<Message>) {
        let config = Config::load();
        let session = crate::session::Session::load();

        let mut app = Self {
            editor: EditorState::new(),
            sidebar: SidebarState::new(),
            search: SearchState::new(),
            preferences: PreferencesState::from_config(&config),
            show_search: false,
            show_sidebar: true,
            show_preferences: false,
            open_menu: None,
            menu_bounds: HashMap::new(),
            current_file: None,
            is_dirty: false,
            is_formatting: false,
            status_message: t!("status.ready").to_string(),
            status_is_error: false,
            format_error: None,
            show_error_panel: false,
            untitled_counter: 1,
            file_cache: HashMap::new(),
            overlay: ActiveOverlay::None,
            goto_line_input: String::new(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            undo_cache: HashMap::new(),
            layout_stale: std::cell::Cell::new(false),
            config_dirty: false,
            last_session: String::new(),
            disk_mtime: None,
            external_warned: false,
            file_size: None,
            size_for: None,
            undo_new_group: true,
            view_mode: ViewMode::Raw,
            preview: PreviewDoc::None,
            preview_src: String::new(),
            json_collapsed: Default::default(),
            preview_rev: 0,
            mod_ctrl: false,
            mod_alt: false,
            config,
        };

        app.restore_session(session);

        // If a file path was given on the command line, open it on top of the session
        let cmd = if let Some(path) = flags {
            Command::perform(read_file(path), Message::FileOpened)
        } else {
            Command::none()
        };
        (app, cmd)
    }

    fn title(&self) -> String {
        let untitled = t!("app.untitled").to_string();
        let file_name = self
            .current_file
            .as_ref()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or(untitled.as_str());
        let dirty = if self.is_dirty { " •" } else { "" };
        format!("SimpleEdit — {}{}", file_name, dirty)
    }

    fn update(&mut self, message: Message) -> Command<Message> {
        let command = self.dispatch(message, 0);
        self.refresh_preview();
        self.refresh_file_size();
        command
    }

    fn subscription(&self) -> iced::Subscription<Message> {
        let keys = iced::keyboard::on_key_press(|key, modifiers| {
            use iced::keyboard::{key::Named, Key};

            if key == Key::Named(Named::Escape) {
                return Some(Message::EscapePressed);
            }
            if key == Key::Named(Named::Tab) {
                if modifiers.shift() {
                    return Some(Message::DedentLine);
                }
                return Some(Message::InsertTab);
            }

            let key_str = match key.as_ref() {
                Key::Character(c) => c.to_lowercase(),
                Key::Named(Named::Enter) => "Return".to_string(),
                Key::Named(Named::ArrowUp) => "ArrowUp".to_string(),
                Key::Named(Named::ArrowDown) => "ArrowDown".to_string(),
                Key::Named(Named::ArrowLeft) => "ArrowLeft".to_string(),
                Key::Named(Named::ArrowRight) => "ArrowRight".to_string(),
                Key::Named(Named::Home) => "Home".to_string(),
                Key::Named(Named::End) => "End".to_string(),
                Key::Named(Named::PageUp) => "PageUp".to_string(),
                Key::Named(Named::PageDown) => "PageDown".to_string(),
                Key::Named(Named::Delete) => "Delete".to_string(),
                Key::Named(Named::Backspace) => "Backspace".to_string(),
                Key::Named(Named::Space) => "Space".to_string(),
                Key::Named(Named::F1) => "F1".to_string(),
                Key::Named(Named::F2) => "F2".to_string(),
                Key::Named(Named::F3) => "F3".to_string(),
                Key::Named(Named::F4) => "F4".to_string(),
                Key::Named(Named::F5) => "F5".to_string(),
                Key::Named(Named::F6) => "F6".to_string(),
                Key::Named(Named::F7) => "F7".to_string(),
                Key::Named(Named::F8) => "F8".to_string(),
                Key::Named(Named::F9) => "F9".to_string(),
                Key::Named(Named::F10) => "F10".to_string(),
                Key::Named(Named::F11) => "F11".to_string(),
                Key::Named(Named::F12) => "F12".to_string(),
                _ => return None,
            };

            Some(Message::KeyPressed {
                key: key_str,
                ctrl: modifiers.command(),
                shift: modifiers.shift(),
                alt: modifiers.alt(),
            })
        });

        // Chevron clicks need the modifier state at click time, which a button
        // press message does not carry.
        let modifiers = iced::event::listen_with(|event, _status| match event {
            iced::Event::Keyboard(iced::keyboard::Event::ModifiersChanged(m)) => {
                Some(Message::ModifiersChanged {
                    ctrl: m.command(),
                    alt: m.alt(),
                })
            }
            // The window is closed through the app so the session is saved.
            iced::Event::Window(_, iced::window::Event::CloseRequested) => Some(Message::Quit),
            _ => None,
        });

        let autosave =
            iced::time::every(std::time::Duration::from_secs(5)).map(|_| Message::AutoSave);

        iced::Subscription::batch([keys, modifiers, autosave])
    }

    fn view(&self) -> Element<'_, Message> {
        // The layout pass right after this shapes whatever was edited.
        self.layout_stale.set(false);
        let dark = self.config.dark_mode;

        let menu_bar = crate::menu_bar::view(&self.config, self.open_menu);

        let preview_kind = self.preview_kind();
        let view_mode = if preview_kind.is_some() {
            self.view_mode
        } else {
            ViewMode::Raw
        };
        let editor_widget = self.editor_pane(preview_kind, view_mode, dark);
        // The context menu's actions (select/cut/copy/paste/format) all act on
        // the raw buffer, which isn't what's on screen in Preview mode — right-
        // clicking the rendered Markdown/JSON tree wouldn't do anything a user
        // could make sense of, so it's only offered over the raw editor.
        let editor_with_context: Element<'_, Message> = if view_mode == ViewMode::Preview {
            editor_widget
        } else {
            let lbl_select_all = t!("ctx.select_all").to_string();
            let lbl_cut = t!("ctx.cut").to_string();
            let lbl_copy = t!("ctx.copy").to_string();
            let lbl_paste = t!("ctx.paste").to_string();
            let lbl_delete = t!("ctx.delete").to_string();
            let lbl_format_sel = t!("ctx.format_selection").to_string();
            let lbl_format_all = t!("ctx.format_all").to_string();
            let has_fmt_ctx = self
                .editor
                .language
                .as_deref()
                .map(crate::formatter::has_formatter)
                .unwrap_or(false);
            let has_sel_ctx = self.editor.content.selection().is_some();
            iced_aw::ContextMenu::new(editor_widget, move || {
                let item = |label: String, msg: Message| -> Element<'static, Message> {
                    button(text(label).size(13))
                        .padding([6, 10])
                        .width(Length::Fixed(200.0))
                        .on_press(msg)
                        .style(iced::theme::Button::custom(crate::theme::GhostButton {
                            dark,
                            active: false,
                        }))
                        .into()
                };
                let disabled = |label: String| -> Element<'static, Message> {
                    container(text(label).size(13).style(crate::theme::muted_text(dark)))
                        .padding([6, 10])
                        .width(Length::Fixed(200.0))
                        .into()
                };
                let fmt_sel_el: Element<'static, Message> = if has_fmt_ctx && has_sel_ctx {
                    item(lbl_format_sel.clone(), Message::FormatSelection)
                } else {
                    disabled(lbl_format_sel.clone())
                };
                let fmt_all_el: Element<'static, Message> = if has_fmt_ctx {
                    item(lbl_format_all.clone(), Message::FormatFile)
                } else {
                    disabled(lbl_format_all.clone())
                };
                let ctx_items: Vec<Element<'static, Message>> = vec![
                    item(lbl_select_all.clone(), Message::SelectAll),
                    item(lbl_cut.clone(), Message::ContextCut),
                    item(lbl_copy.clone(), Message::ContextCopy),
                    item(lbl_paste.clone(), Message::ContextPaste),
                    item(lbl_delete.clone(), Message::ContextDelete),
                    fmt_sel_el,
                    fmt_all_el,
                ];
                container(column(ctx_items).spacing(2).padding(6))
                    .style(crate::theme::card(dark))
                    .into()
            })
            .into()
        };

        let search_panel = if self.show_search {
            Some(self.search.view(dark))
        } else {
            None
        };

        let editor_area: Element<Message> = if let Some(search) = search_panel {
            column![search, editor_with_context].into()
        } else {
            editor_with_context
        };

        let mut main_row = row![];
        if self.show_sidebar {
            main_row = main_row.push(self.sidebar.view(dark, &self.current_file));
        }
        main_row = main_row.push(editor_area);
        if self.show_preferences {
            main_row = main_row.push(self.preferences.view(dark));
        }

        let cursor = self.editor.content.cursor_position();
        let selection_info = self.editor.content.selection().map(|s| {
            let chars = s.chars().count();
            let lines = s.lines().count();
            if lines > 1 {
                format!("{}L {}C", lines, chars)
            } else {
                format!("{}C", chars)
            }
        });
        let file_size = self.file_size;
        let status_bar = crate::editor::statusbar::view(
            &self.status_message,
            &self.current_file,
            self.is_dirty,
            dark,
            cursor,
            self.status_is_error,
            self.editor.language.as_deref(),
            selection_info,
            file_size,
        );

        let error_panel: Option<Element<Message>> = if self.show_error_panel {
            if let Some(err) = &self.format_error {
                let header = row![
                    text(t!("panel.errors").to_string())
                        .size(11)
                        .style(se_theme::muted_text(dark)),
                    Space::with_width(Length::Fill),
                    button(text("✕").size(11).style(se_theme::muted_text(dark)))
                        .padding([2, 6])
                        .on_press(Message::CloseErrorPanel)
                        .style(iced::theme::Button::custom(se_theme::GhostButton {
                            dark,
                            active: false,
                        })),
                ]
                .padding([4, 10])
                .align_items(Alignment::Center);

                let error_color = iced::Color::from_rgb(0.88, 0.27, 0.18);
                let body = scrollable(
                    container(
                        text(err.clone())
                            .size(12)
                            .style(error_color)
                            .font(iced::Font::MONOSPACE),
                    )
                    .padding([4, 12, 8, 12])
                    .width(Length::Fill),
                )
                .height(Length::Fixed(100.0));

                Some(
                    container(column![header, body])
                        .width(Length::Fill)
                        .style(se_theme::error_panel(dark))
                        .into(),
                )
            } else {
                None
            }
        } else {
            None
        };

        let mut col = if self.is_formatting {
            let banner = container(
                text(t!("status.formatting").to_string())
                    .size(12)
                    .style(se_theme::accent_color()),
            )
            .width(Length::Fill)
            .padding([4, 14])
            .style(se_theme::accent_banner(dark));
            column![menu_bar, banner, main_row]
        } else {
            column![menu_bar, main_row]
        };
        if let Some(panel) = error_panel {
            col = col.push(panel);
        }
        let content = col
            .push(status_bar)
            .width(Length::Fill)
            .height(Length::Fill);

        // Base layout wrapped in a container
        let base: Element<Message> = container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .into();

        // Floating dropdown (menu bar)
        // Bounds resolve one update cycle after a menu first opens (see
        // Message::ToggleMenu); until then, render nothing.
        let with_dropdown: Element<Message> = if let (Some(menu), Some(bounds)) = (
            self.open_menu,
            self.open_menu
                .and_then(|m| self.menu_bounds.get(&m).copied()),
        ) {
            let has_fmt = self
                .editor
                .language
                .as_deref()
                .map(crate::formatter::has_formatter)
                .unwrap_or(false);
            let lang_override_enabled = self.language_picker_enabled();
            let dropdown =
                crate::menu_bar::dropdown_view(menu, &self.config, has_fmt, lang_override_enabled);
            iced_aw::floating_element::FloatingElement::new(base, dropdown)
                .anchor(iced_aw::floating_element::Anchor::NorthWest)
                .offset(iced_aw::floating_element::Offset {
                    x: bounds.x,
                    y: bounds.y + bounds.height,
                })
                .into()
        } else {
            base
        };

        // Modal overlays
        match &self.overlay {
            ActiveOverlay::None => with_dropdown,
            ActiveOverlay::About => {
                let overlay = self.view_about_overlay(dark);
                iced_aw::Modal::new(with_dropdown, Some(overlay))
                    .backdrop(Message::CloseOverlay)
                    .on_esc(Message::CloseOverlay)
                    .into()
            }
            ActiveOverlay::GotoLine => {
                let overlay = self.view_goto_line_overlay(dark);
                iced_aw::Modal::new(with_dropdown, Some(overlay))
                    .backdrop(Message::CloseOverlay)
                    .on_esc(Message::CloseOverlay)
                    .into()
            }
            ActiveOverlay::LanguagePicker => {
                let overlay = self.view_language_picker_overlay(dark);
                iced_aw::Modal::new(with_dropdown, Some(overlay))
                    .backdrop(Message::CloseOverlay)
                    .on_esc(Message::CloseOverlay)
                    .into()
            }
            ActiveOverlay::Shortcuts { capturing } => {
                let overlay = self.view_shortcuts_overlay(dark, *capturing);
                iced_aw::Modal::new(with_dropdown, Some(overlay))
                    .backdrop(Message::CloseOverlay)
                    .on_esc(Message::CloseOverlay)
                    .into()
            }
            ActiveOverlay::ConfirmOverwrite(path) => {
                let overlay = self.view_overwrite_overlay(dark, path);
                iced_aw::Modal::new(with_dropdown, Some(overlay))
                    .backdrop(Message::CloseOverlay)
                    .on_esc(Message::CloseOverlay)
                    .into()
            }
            ActiveOverlay::ConfirmClose(path) => {
                let overlay = self.view_confirm_overlay(dark, path);
                iced_aw::Modal::new(with_dropdown, Some(overlay))
                    .backdrop(Message::CloseOverlay)
                    .on_esc(Message::CloseOverlay)
                    .into()
            }
            ActiveOverlay::UpdateChecking
            | ActiveOverlay::UpdateDownloading
            | ActiveOverlay::UpdateInstalling
            | ActiveOverlay::UpdateUpToDate
            | ActiveOverlay::UpdateAvailable(_)
            | ActiveOverlay::UpdateError(_) => {
                let overlay = self.view_update_overlay(dark);
                iced_aw::Modal::new(with_dropdown, Some(overlay))
                    .backdrop(Message::CloseOverlay)
                    .on_esc(Message::CloseOverlay)
                    .into()
            }
        }
    }

    fn theme(&self) -> Theme {
        if self.config.dark_mode {
            se_theme::ink_dark()
        } else {
            se_theme::ink_light()
        }
    }
}

// ─── Message handling ───────────────────────────────────────────────────────

impl Message {
    /// Whether handling this moves the editor cursor, which is only safe once
    /// the editor has laid out its latest edits.
    fn needs_layout(&self) -> bool {
        matches!(
            self,
            Message::Undo
                | Message::Redo
                | Message::DuplicateLine
                | Message::MoveLineUp
                | Message::MoveLineDown
                | Message::ToggleComment
                | Message::DeleteLine
                | Message::IndentSelection
                | Message::DedentLine
                | Message::FileFormatted(_)
                | Message::GotoLineSubmit
                | Message::RestoreCursor(..)
                | Message::MoveCursorLeft
                | Message::SelectAll
                | Message::Search(
                    SearchMessage::Find | SearchMessage::FindPrevious | SearchMessage::Replace
                )
        )
    }

    /// Shortcuts that act on the buffer and so shouldn't fire while a
    /// text field is being typed in.
    fn is_editor_shortcut(&self) -> bool {
        matches!(
            self,
            Message::Undo
                | Message::Redo
                | Message::SelectAll
                | Message::DuplicateLine
                | Message::MoveLineUp
                | Message::MoveLineDown
                | Message::ToggleComment
                | Message::DeleteLine
        )
    }

    /// Messages that can't have touched the buffer.
    fn is_passive(&self) -> bool {
        matches!(
            self,
            Message::ToggleMenu(_)
                | Message::HoverMenu(_)
                | Message::MenuBounds(..)
                | Message::ModifiersChanged { .. }
                | Message::AutoSave
                | Message::Deferred(..)
                | Message::ShortcutChecked(..)
        )
    }
}

impl SimpleEditApp {
    /// Runs a message, first waiting out any not-yet-laid-out edit if the
    /// message needs the cursor to move.
    fn dispatch(&mut self, message: Message, tries: u8) -> Command<Message> {
        let message = match message {
            Message::Deferred(inner, n) => return self.dispatch(*inner, n),
            m => m,
        };
        if self.layout_stale.get() && message.needs_layout() && tries < 10 {
            return Command::perform(
                async { tokio::time::sleep(std::time::Duration::from_millis(20)).await },
                move |_| Message::Deferred(Box::new(message), tries + 1),
            );
        }
        let passive = message.is_passive();
        let command = self.handle(message);
        if !passive {
            self.layout_stale.set(true);
        }
        command
    }

    fn handle(&mut self, message: Message) -> Command<Message> {
        // Passive/background messages must not close an open dropdown: X11
        // fires a spurious ModifiersChanged right after the very click that
        // opens the menu, AutoSave ticks every 5s regardless of the UI, and
        // MenuBounds is this same open answering its own position query.
        let closes_menu = !matches!(
            &message,
            Message::ToggleMenu(_)
                | Message::HoverMenu(_)
                | Message::MenuBounds(..)
                | Message::ModifiersChanged { .. }
                | Message::AutoSave
        );
        if closes_menu {
            self.open_menu = None;
        }

        match message {
            Message::EditorAction(action) => {
                let is_edit = action.is_edit();

                if is_edit {
                    if let text_editor::Action::Edit(edit) = &action {
                        match edit {
                            text_editor::Edit::Insert(ch) => {
                                if ch.is_whitespace() {
                                    // Whitespace ends the current word group; next non-ws starts a new one
                                    self.undo_new_group = true;
                                } else if self.undo_new_group {
                                    // First non-whitespace of a new group: snapshot now
                                    self.push_undo();
                                    self.undo_new_group = false;
                                }
                                // else: mid-word, stay in current group
                            }
                            _ => {
                                // Backspace, Delete, Enter, Paste: always their own undo point
                                self.push_undo();
                                self.undo_new_group = true;
                            }
                        }
                    }
                } else {
                    // Motion action: next edit opens a new group
                    self.undo_new_group = true;
                }

                // Auto-indent: replicate leading whitespace of current line on Enter
                if self.config.auto_indent {
                    if let text_editor::Action::Edit(text_editor::Edit::Enter) = &action {
                        let (line_idx, col) = self.editor.content.cursor_position();
                        // Only the whitespace before the cursor: pressing Enter
                        // inside the indentation must not duplicate all of it.
                        let indent: String = self
                            .editor
                            .content
                            .line(line_idx)
                            .map(|l| {
                                l.chars()
                                    .take_while(|c| *c == ' ' || *c == '\t')
                                    .collect::<String>()
                            })
                            .unwrap_or_default();
                        let indent = indent[..indent.len().min(col)].to_string();
                        self.editor
                            .content
                            .perform(text_editor::Action::Edit(text_editor::Edit::Enter));
                        if !indent.is_empty() {
                            self.editor.content.perform(text_editor::Action::Edit(
                                text_editor::Edit::Paste(std::sync::Arc::new(indent)),
                            ));
                        }
                        self.is_dirty = true;
                        return Command::none();
                    }
                }

                // Auto-pairing of brackets and quotes
                if let text_editor::Action::Edit(text_editor::Edit::Insert(ch)) = &action {
                    if let Some(command) = self.autopair(*ch, &action) {
                        return command;
                    }
                }

                self.editor.content.perform(action);
                if is_edit {
                    self.is_dirty = true;
                }
                Command::none()
            }
            Message::InsertTab => {
                if !self.editing_blocked() {
                    if self.selection_lines().is_some() {
                        return self.dispatch(Message::IndentSelection, 0);
                    }
                    // With a single-line selection this replaces it, as
                    // in most editors.
                    let text = self.indent_unit();
                    self.push_undo();
                    self.undo_new_group = true;
                    self.editor.content.perform(text_editor::Action::Edit(
                        text_editor::Edit::Paste(std::sync::Arc::new(text)),
                    ));
                    self.is_dirty = true;
                }
                Command::none()
            }
            Message::SelectAll => {
                self.editor.content.perform(text_editor::Action::Move(
                    text_editor::Motion::DocumentStart,
                ));
                self.editor.content.perform(text_editor::Action::Select(
                    text_editor::Motion::DocumentEnd,
                ));
                Command::none()
            }
            Message::ToggleMenu(menu) => {
                self.open_menu = if self.open_menu == Some(menu) {
                    None
                } else {
                    Some(menu)
                };
                self.query_menu_bounds()
            }
            Message::HoverMenu(menu) => {
                if self.open_menu.is_some() && self.open_menu != Some(menu) {
                    self.open_menu = Some(menu);
                    self.query_menu_bounds()
                } else {
                    Command::none()
                }
            }
            Message::MenuBounds(menu, bounds) => {
                if let Some(bounds) = bounds {
                    self.menu_bounds.insert(menu, bounds);
                }
                Command::none()
            }
            Message::NewFile => {
                self.stash_current();
                self.untitled_counter += 1;
                let path = untitled_path(self.untitled_counter);
                self.load_buffer(path, String::new(), None, false);
                self.status_message = t!("status.new_file").to_string();
                Command::none()
            }
            Message::OpenFile => Command::perform(open_file(), Message::FileOpened),
            Message::FileOpened(result) => {
                match result {
                    Ok((path, content)) => {
                        // Re-opening the file being edited must not throw away
                        // its unsaved changes.
                        if self.current_file.as_ref() == Some(&path) && self.is_dirty {
                            return Command::none();
                        }
                        let cached = self.file_cache.remove(&path);
                        if self.current_file.as_ref() != Some(&path) {
                            self.stash_current();
                        }
                        // A background tab with unsaved edits wins over the disk.
                        let (content, language, dirty) = match cached {
                            Some((c, l, true)) => (c, l, true),
                            _ => (content, None, false),
                        };
                        self.config.add_recent(&path);
                        self.config.save();
                        self.load_buffer(path, content, language, dirty);
                        self.status_message = t!("status.file_opened").to_string();
                    }
                    Err(e) if e == CANCELLED => {}
                    Err(e) => {
                        self.status_message = format!("{}: {}", t!("status.error"), e);
                    }
                }
                Command::none()
            }
            Message::SaveFile => {
                if let Some(path) = self.current_file.clone() {
                    let changed = !is_untitled(&path)
                        && self.disk_mtime.is_some()
                        && mtime(&path) != self.disk_mtime;
                    if changed {
                        self.overlay = ActiveOverlay::ConfirmOverwrite(path);
                        return Command::none();
                    }
                }
                let content = crate::editor::buffer_text(&self.editor.content);
                let origin = self.current_file.clone();
                match origin.clone() {
                    Some(path) if !is_untitled(&path) => {
                        save_command(origin, save_file(path, content))
                    }
                    _ => save_command(origin, save_file_as(content)),
                }
            }
            Message::SaveFileAs => save_command(
                self.current_file.clone(),
                save_file_as(crate::editor::buffer_text(&self.editor.content)),
            ),
            Message::FileSaved(origin, result) => {
                match result {
                    Ok((path, saved)) => {
                        self.file_cache.remove(&path);
                        self.size_for = None;
                        // The tab the save started from may no longer be the
                        // active one (the dialog is asynchronous).
                        let from_current = origin == self.current_file;
                        if let Some(old) = origin.as_ref().filter(|o| is_untitled(o)) {
                            self.sidebar.rename_file(old, path.clone());
                            if !from_current {
                                if let Some(entry) = self.file_cache.remove(old) {
                                    self.file_cache.insert(path.clone(), entry);
                                }
                                if let Some(stacks) = self.undo_cache.remove(old) {
                                    self.undo_cache.insert(path.clone(), stacks);
                                }
                            }
                        }
                        self.sidebar.add_file(path.clone());
                        let ext = path
                            .extension()
                            .and_then(|e| e.to_str())
                            .unwrap_or("")
                            .to_string();
                        if from_current {
                            self.editor.adopt_extension_language(&ext);
                            // Edits made while the save was in flight stay dirty.
                            self.is_dirty =
                                crate::editor::buffer_text(&self.editor.content) != saved;
                            self.disk_mtime = mtime(&path);
                            self.external_warned = false;
                            self.current_file = Some(path.clone());
                        } else if let Some(entry) = self.file_cache.get_mut(&path) {
                            entry.2 = entry.0 != saved;
                        }
                        self.config.add_recent(&path);
                        self.config.save();
                        self.status_message = t!("status.file_saved").to_string();
                    }
                    Err(e) if e == CANCELLED => {}
                    Err(e) => {
                        self.status_message = format!("{}: {}", t!("status.error"), e);
                    }
                }
                Command::none()
            }
            Message::CloseFile => match self.current_file.clone() {
                Some(path) => self.request_close(path),
                None => Command::none(),
            },
            Message::ConfirmOverwrite => {
                self.overlay = ActiveOverlay::None;
                // Take the new on-disk state as the baseline, then save.
                self.disk_mtime = self.current_file.as_deref().and_then(mtime);
                self.handle(Message::SaveFile)
            }
            Message::ConfirmDiscard(path) => {
                self.overlay = ActiveOverlay::None;
                self.close_path(path)
            }
            Message::ToggleSearch => {
                self.show_search = !self.show_search;
                Command::none()
            }
            Message::ToggleSidebar => {
                self.show_sidebar = !self.show_sidebar;
                Command::none()
            }
            Message::TogglePreferences => {
                self.show_preferences = !self.show_preferences;
                Command::none()
            }
            Message::ThemeChanged(dark) => {
                self.config.dark_mode = dark;
                self.config.save();
                Command::none()
            }
            Message::Sidebar(msg) => match self.sidebar.update(msg) {
                SidebarAction::OpenFile(path) => self.activate(path),
                SidebarAction::CloseFile(path) => self.request_close(path),
                // Only offered for the active file.
                SidebarAction::SaveFile => self.dispatch(Message::SaveFile, 0),
                SidebarAction::SaveFileAs => save_command(
                    self.current_file.clone(),
                    save_file_as(crate::editor::buffer_text(&self.editor.content)),
                ),
                SidebarAction::None => Command::none(),
            },
            Message::Search(msg) => {
                let replacing = matches!(msg, SearchMessage::Replace | SearchMessage::ReplaceAll);
                let before = replacing.then(|| self.snapshot());
                let command = self.search.update(msg, &mut self.editor.content);
                // Replacements are edits like any other: undoable and dirty.
                if let Some(before) = before {
                    if crate::editor::buffer_text(&self.editor.content) != before.text {
                        self.undo_stack.push(before);
                        trim_history(&mut self.undo_stack);
                        self.redo_stack.clear();
                        self.undo_new_group = true;
                        self.is_dirty = true;
                    }
                }
                command
            }
            Message::Preferences(msg) => {
                self.preferences.update(msg, &mut self.config);
                self.config_dirty = true;
                Command::none()
            }
            Message::About => {
                self.overlay = ActiveOverlay::About;
                Command::none()
            }
            Message::Quit => {
                // Unsaved buffers live on in the session file, so quitting
                // loses nothing.
                self.save_session();
                self.config.save();
                std::process::exit(0)
            }
            Message::ContextCopy => {
                if let Some(text) = self.editor.content.selection() {
                    return iced::clipboard::write(text);
                }
                Command::none()
            }
            Message::ContextCut => {
                if let Some(text) = self.editor.content.selection() {
                    self.push_undo();
                    self.undo_new_group = true;
                    self.editor
                        .content
                        .perform(text_editor::Action::Edit(text_editor::Edit::Delete));
                    self.is_dirty = true;
                    return iced::clipboard::write(text);
                }
                Command::none()
            }
            Message::ContextPaste => iced::clipboard::read(Message::ClipboardContent),
            Message::ClipboardContent(Some(text)) => {
                self.push_undo();
                self.undo_new_group = true;
                self.editor
                    .content
                    .perform(text_editor::Action::Edit(text_editor::Edit::Paste(
                        std::sync::Arc::new(text),
                    )));
                self.is_dirty = true;
                Command::none()
            }
            Message::ClipboardContent(None) => Command::none(),
            Message::ContextDelete => {
                self.push_undo();
                self.undo_new_group = true;
                self.editor
                    .content
                    .perform(text_editor::Action::Edit(text_editor::Edit::Delete));
                self.is_dirty = true;
                Command::none()
            }
            Message::FormatFile => {
                if let Some(ext) = self.editor.language.clone() {
                    self.is_formatting = true;
                    self.status_message = t!("status.formatting").to_string();
                    let content = crate::editor::buffer_text(&self.editor.content);
                    Command::perform(
                        crate::formatter::format(content, ext),
                        Message::FileFormatted,
                    )
                } else {
                    self.status_message = t!("status.no_language").to_string();
                    Command::none()
                }
            }
            Message::FormatSelection => {
                if let (Some(ext), Some(selected)) = (
                    self.editor.language.clone(),
                    self.editor.content.selection(),
                ) {
                    self.is_formatting = true;
                    self.status_message = t!("status.formatting").to_string();
                    // Formatters end their output with a newline; a fragment
                    // that didn't end with one shouldn't gain one.
                    let had_newline = selected.ends_with('\n');
                    Command::perform(
                        async move {
                            crate::formatter::format(selected, ext).await.map(|f| {
                                if had_newline {
                                    f
                                } else {
                                    f.trim_end_matches('\n').to_string()
                                }
                            })
                        },
                        Message::SelectionFormatted,
                    )
                } else {
                    Command::none()
                }
            }
            Message::FileFormatted(result) => {
                self.is_formatting = false;
                match result {
                    Ok(formatted) => {
                        let (line, col) = self.editor.content.cursor_position();
                        self.status_is_error = false;
                        self.format_error = None;
                        self.status_message = t!("status.formatted").to_string();
                        return self.apply_text_edit(formatted, line, col);
                    }
                    Err(e) => {
                        self.status_is_error = true;
                        self.format_error = Some(e.clone());
                        self.show_error_panel = true;
                        self.status_message = t!("status.format_error").to_string();
                    }
                }
                Command::none()
            }
            Message::SelectionFormatted(result) => {
                self.is_formatting = false;
                match result {
                    Ok(formatted) => {
                        self.push_undo();
                        self.undo_new_group = true;
                        self.editor.content.perform(text_editor::Action::Edit(
                            text_editor::Edit::Paste(std::sync::Arc::new(formatted)),
                        ));
                        self.is_dirty = true;
                        self.status_is_error = false;
                        self.format_error = None;
                        self.status_message = t!("status.formatted").to_string();
                    }
                    Err(e) => {
                        self.status_is_error = true;
                        self.format_error = Some(e.clone());
                        self.show_error_panel = true;
                        self.status_message = t!("status.format_error").to_string();
                    }
                }
                Command::none()
            }
            Message::CloseErrorPanel => {
                self.show_error_panel = false;
                Command::none()
            }
            Message::Undo => {
                if let Some(prev) = self.undo_stack.pop() {
                    let current = self.snapshot();
                    self.redo_stack.push(current);
                    trim_history(&mut self.redo_stack);
                    return self.restore_snapshot(prev);
                }
                Command::none()
            }
            Message::Redo => {
                if let Some(next) = self.redo_stack.pop() {
                    let current = self.snapshot();
                    self.undo_stack.push(current);
                    trim_history(&mut self.undo_stack);
                    return self.restore_snapshot(next);
                }
                Command::none()
            }
            Message::AutoSave => {
                if self.config_dirty {
                    self.config_dirty = false;
                    self.config.save();
                }
                self.save_session();
                self.check_external_change()
            }
            Message::MoveCursorLeft => {
                self.editor
                    .content
                    .perform(text_editor::Action::Move(text_editor::Motion::Left));
                Command::none()
            }
            // --- Overlays ---
            Message::CloseOverlay => {
                self.overlay = ActiveOverlay::None;
                Command::none()
            }
            Message::OpenGotoLine => {
                self.goto_line_input = String::new();
                self.overlay = ActiveOverlay::GotoLine;
                text_input::focus(goto_line_id())
            }
            Message::GotoLineInputChanged(s) => {
                // Allow only digits
                self.goto_line_input = s.chars().filter(|c| c.is_ascii_digit()).collect();
                Command::none()
            }
            Message::GotoLineSubmit => {
                if let Ok(n) = self.goto_line_input.parse::<usize>() {
                    let max = self.editor.content.line_count().saturating_sub(1);
                    let target = n.saturating_sub(1).min(max);
                    crate::search::goto_line_start(&mut self.editor.content, target);
                }
                self.overlay = ActiveOverlay::None;
                Command::none()
            }
            Message::OpenLanguagePicker => {
                self.overlay = ActiveOverlay::LanguagePicker;
                Command::none()
            }
            Message::SetLanguage(ext) => {
                self.editor.language = if ext.is_empty() { None } else { Some(ext) };
                self.reset_view_mode();
                self.overlay = ActiveOverlay::None;
                Command::none()
            }
            Message::OpenShortcuts => {
                self.overlay = ActiveOverlay::Shortcuts { capturing: None };
                Command::none()
            }
            Message::StartCaptureShortcut(target) => {
                self.overlay = ActiveOverlay::Shortcuts {
                    capturing: Some(target),
                };
                Command::none()
            }
            // Raw key forwarded from the subscription fn pointer
            Message::KeyPressed {
                key,
                ctrl,
                shift,
                alt,
            } => {
                // In shortcut capture mode: save the new binding
                if let ActiveOverlay::Shortcuts {
                    capturing: Some(target),
                } = &self.overlay
                {
                    let target = *target;
                    let sc = ShortcutConfig {
                        ctrl,
                        shift,
                        alt,
                        key,
                    };
                    // A bare letter (or Shift+letter) would fire on every
                    // keystroke typed into the editor.
                    let is_function_key = sc.key.len() > 1
                        && sc.key.starts_with('F')
                        && sc.key[1..].bytes().all(|b| b.is_ascii_digit());
                    if !(sc.ctrl || sc.alt || is_function_key) {
                        self.status_message = t!("shortcuts.needs_modifier").to_string();
                        return Command::none();
                    }
                    if self.config.shortcuts.conflicts(target, &sc) {
                        self.status_message = t!("shortcuts.conflict").to_string();
                        return Command::none();
                    }
                    *self.config.shortcuts.slot_mut(target) = sc;
                    self.config.save();
                    self.overlay = ActiveOverlay::Shortcuts { capturing: None };
                    return Command::none();
                }
                // Don't fire shortcuts while any overlay is open
                if !matches!(self.overlay, ActiveOverlay::None) {
                    return Command::none();
                }
                // Shift+Enter: previous match. Plain Enter is the search
                // field's own on_submit, which iced correctly scopes to
                // when that field has focus; this subscription-level
                // shortcut can't tell which widget is focused, so it's
                // deliberately narrowed to the modified case only.
                if self.show_search && shift && key.eq_ignore_ascii_case("return") {
                    return self.dispatch(Message::Search(SearchMessage::FindPrevious), 0);
                }
                // Match against config shortcuts. A symbol key (`/`) may need
                // Shift on some layouts (AZERTY), so Shift doesn't count
                // against a binding that doesn't use it.
                let symbol_key = key.chars().count() == 1
                    && key.chars().next().is_some_and(|c| !c.is_alphanumeric());
                let sc_check = |s: &ShortcutConfig| -> bool {
                    s.key.eq_ignore_ascii_case(&key)
                        && s.ctrl == ctrl
                        && s.alt == alt
                        && (s.shift == shift || (symbol_key && !s.shift))
                };
                let sh = &self.config.shortcuts;
                // Order matters: Ctrl+Shift+S must be tried before Ctrl+S.
                let bindings: [(&ShortcutConfig, Message); 17] = [
                    (&sh.undo, Message::Undo),
                    (&sh.redo, Message::Redo),
                    (&sh.duplicate_line, Message::DuplicateLine),
                    (&sh.move_line_up, Message::MoveLineUp),
                    (&sh.move_line_down, Message::MoveLineDown),
                    (&sh.toggle_comment, Message::ToggleComment),
                    (&sh.delete_line, Message::DeleteLine),
                    (&sh.save_as, Message::SaveFileAs),
                    (&sh.new_file, Message::NewFile),
                    (&sh.save_file, Message::SaveFile),
                    (&sh.open_file, Message::OpenFile),
                    (&sh.close_file, Message::CloseFile),
                    (&sh.find, Message::ToggleSearch),
                    (&sh.select_all, Message::SelectAll),
                    (&sh.format_code, Message::FormatFile),
                    (&sh.goto_line, Message::OpenGotoLine),
                    (&sh.toggle_sidebar, Message::ToggleSidebar),
                ];
                let msg = bindings
                    .into_iter()
                    .find(|(s, _)| sc_check(s))
                    .map(|(_, m)| m)
                    .or_else(|| sc_check(&sh.quit).then_some(Message::Quit));
                match msg {
                    // With the search bar open, a text field may own the keys:
                    // Ctrl+Z / Ctrl+A belong to it, not to the buffer.
                    Some(m) if self.show_search && m.is_editor_shortcut() => {
                        Command::widget(crate::search::search_field_focused()).map(move |focused| {
                            Message::ShortcutChecked(Box::new(m.clone()), focused)
                        })
                    }
                    Some(m) => self.dispatch(m, 0),
                    None => Command::none(),
                }
            }
            Message::DuplicateLine => {
                if self.editing_blocked() {
                    return Command::none();
                }
                let text = crate::editor::buffer_text(&self.editor.content);
                let (line, col) = self.editor.content.cursor_position();
                let new_text = duplicate_line(&text, line);
                self.apply_text_edit(new_text, line + 1, col)
            }
            Message::MoveLineUp => {
                if self.editing_blocked() {
                    return Command::none();
                }
                let text = crate::editor::buffer_text(&self.editor.content);
                let (line, col) = self.editor.content.cursor_position();
                let (new_text, new_line) = move_line(&text, line, true);
                self.apply_text_edit(new_text, new_line, col)
            }
            Message::MoveLineDown => {
                if self.editing_blocked() {
                    return Command::none();
                }
                let text = crate::editor::buffer_text(&self.editor.content);
                let (line, col) = self.editor.content.cursor_position();
                let (new_text, new_line) = move_line(&text, line, false);
                self.apply_text_edit(new_text, new_line, col)
            }
            Message::ToggleComment => {
                if self.editing_blocked() {
                    return Command::none();
                }
                // Formats without comments (JSON) have nothing to toggle.
                let Some((prefix, suffix)) = comment_style(self.editor.language.as_deref()) else {
                    return Command::none();
                };
                let text = crate::editor::buffer_text(&self.editor.content);
                let (line, col) = self.editor.content.cursor_position();
                let (first, last) = self.selection_lines().unwrap_or((line, line));
                let new_text = toggle_comment_lines(&text, first, last, prefix, suffix);
                self.apply_text_edit(new_text, line, col)
            }
            Message::DeleteLine => {
                if self.editing_blocked() {
                    return Command::none();
                }
                let text = crate::editor::buffer_text(&self.editor.content);
                let (line, _) = self.editor.content.cursor_position();
                let (new_text, new_line) = delete_line_op(&text, line);
                self.apply_text_edit(new_text, new_line, 0)
            }
            Message::IndentSelection => {
                if self.editing_blocked() {
                    return Command::none();
                }
                let Some((first, last)) = self.selection_lines() else {
                    return Command::none();
                };
                let text = crate::editor::buffer_text(&self.editor.content);
                let (line, col) = self.editor.content.cursor_position();
                let new_text = indent_lines(&text, first, last, &self.indent_unit());
                self.apply_text_edit(new_text, line, col)
            }
            Message::DedentLine => {
                if self.editing_blocked() {
                    return Command::none();
                }
                let text = crate::editor::buffer_text(&self.editor.content);
                let (line, col) = self.editor.content.cursor_position();
                let (first, last) = self.selection_lines().unwrap_or((line, line));
                let new_text = dedent_lines(&text, first, last, &self.indent_unit());
                self.apply_text_edit(new_text, line, col)
            }
            Message::OpenRecentFile(path) => {
                if !path.exists() {
                    self.config
                        .recent_files
                        .retain(|f| *f != path.to_string_lossy().as_ref());
                    self.config.save();
                    self.status_message =
                        format!("{}: {}", t!("status.error"), t!("status.no_file"));
                    return Command::none();
                }
                self.sidebar.add_file(path.clone());
                self.activate(path)
            }
            Message::Deferred(inner, _) => self.handle(*inner),
            Message::ShortcutChecked(inner, focused) => {
                if focused {
                    Command::none()
                } else {
                    self.dispatch(*inner, 0)
                }
            }
            Message::RestoreCursor(line, col) => {
                let target = line.min(self.editor.content.line_count().saturating_sub(1));
                crate::search::goto_line_start(&mut self.editor.content, target);
                // `col` is a byte index; Motion::Right steps over characters.
                let steps = self
                    .editor
                    .content
                    .line(target)
                    .map(|l| {
                        let mut end = col.min(l.len());
                        while !l.is_char_boundary(end) {
                            end -= 1;
                        }
                        l[..end].chars().count()
                    })
                    .unwrap_or(0);
                for _ in 0..steps {
                    self.editor
                        .content
                        .perform(text_editor::Action::Move(text_editor::Motion::Right));
                }
                Command::none()
            }
            Message::EscapePressed => {
                if !matches!(self.overlay, ActiveOverlay::None) {
                    self.overlay = ActiveOverlay::None;
                }
                Command::none()
            }
            Message::CheckForUpdate => {
                self.overlay = ActiveOverlay::UpdateChecking;
                Command::perform(crate::updater::check_for_update(), Message::UpdateChecked)
            }
            Message::UpdateChecked(Ok(Some(version))) => {
                self.overlay = ActiveOverlay::UpdateAvailable(version);
                Command::none()
            }
            Message::UpdateChecked(Ok(None)) => {
                self.overlay = ActiveOverlay::UpdateUpToDate;
                Command::none()
            }
            Message::UpdateChecked(Err(e)) => {
                self.overlay = ActiveOverlay::UpdateError(e);
                Command::none()
            }
            Message::ConfirmInstallUpdate(version) => {
                self.overlay = ActiveOverlay::UpdateDownloading;
                Command::perform(
                    crate::updater::download_update(version),
                    Message::UpdateDownloaded,
                )
            }
            Message::UpdateDownloaded(Ok(path)) => {
                self.overlay = ActiveOverlay::UpdateInstalling;
                Command::perform(
                    crate::updater::install_update(path),
                    Message::UpdateInstalled,
                )
            }
            Message::UpdateDownloaded(Err(e)) => {
                self.overlay = ActiveOverlay::UpdateError(e);
                Command::none()
            }
            Message::UpdateInstalled(Ok(())) => {
                self.save_session();
                self.config.save();
                // On success this never returns — the process re-execs and exits.
                if let Err(e) = crate::updater::restart() {
                    self.overlay = ActiveOverlay::UpdateError(e);
                }
                Command::none()
            }
            Message::UpdateInstalled(Err(e)) => {
                self.overlay = ActiveOverlay::UpdateError(e);
                Command::none()
            }
            Message::SetViewMode(mode) => {
                self.view_mode = mode;
                Command::none()
            }
            Message::JsonToggle(path) => {
                // Ctrl+click opens every node, Alt+click closes every node.
                if self.mod_ctrl {
                    self.handle(Message::JsonExpandAll)
                } else if self.mod_alt {
                    self.handle(Message::JsonCollapseAll)
                } else {
                    let collapsed = std::sync::Arc::make_mut(&mut self.json_collapsed);
                    if !collapsed.remove(&path) {
                        collapsed.insert(path);
                    }
                    self.preview_rev += 1;
                    Command::none()
                }
            }
            Message::JsonExpandAll => {
                self.json_collapsed = Default::default();
                self.preview_rev += 1;
                Command::none()
            }
            Message::JsonCollapseAll => {
                if let PreviewDoc::Json(value) = &self.preview {
                    self.json_collapsed =
                        std::sync::Arc::new(crate::preview::json::container_paths(value));
                    self.preview_rev += 1;
                }
                Command::none()
            }
            Message::ModifiersChanged { ctrl, alt } => {
                self.mod_ctrl = ctrl;
                self.mod_alt = alt;
                Command::none()
            }
            Message::MarkdownLinkClicked(href) => {
                crate::preview::markdown::find_offset_command(&href).map(Message::PreviewScrollTo)
            }
            Message::PreviewScrollTo(Some(y)) => iced::widget::scrollable::scroll_to(
                crate::preview::markdown::scrollable_id(),
                iced::widget::scrollable::AbsoluteOffset {
                    x: 0.0,
                    // A little breathing room above the heading, clamped so
                    // targets near the top don't ask for a negative scroll.
                    y: (y - 12.0).max(0.0),
                },
            ),
            Message::PreviewScrollTo(None) => Command::none(),
        }
    }

    /// Asks the renderer where the open menu's button sits, so its dropdown
    /// can be anchored under it regardless of label/font/DPI.
    fn query_menu_bounds(&self) -> Command<Message> {
        match self.open_menu {
            Some(m) => iced::widget::container::visible_bounds(crate::menu_bar::menu_id(m))
                .map(move |bounds| Message::MenuBounds(m, bounds)),
            None => Command::none(),
        }
    }

    /// Whether a panel or dialog owns the keyboard, so editing commands
    /// (Tab, line operations) must leave the buffer alone.
    fn editing_blocked(&self) -> bool {
        self.show_search || self.show_preferences || !matches!(self.overlay, ActiveOverlay::None)
    }

    /// One level of indentation, per the tab settings.
    fn indent_unit(&self) -> String {
        if self.config.use_spaces {
            " ".repeat(self.config.tab_width)
        } else {
            "\t".to_string()
        }
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            text: crate::editor::buffer_text(&self.editor.content),
            cursor: self.editor.content.cursor_position(),
        }
    }

    fn push_undo(&mut self) {
        let snapshot = self.snapshot();
        self.undo_stack.push(snapshot);
        trim_history(&mut self.undo_stack);
        self.redo_stack.clear();
    }

    /// Replaces the buffer with a history snapshot and puts the cursor back.
    fn restore_snapshot(&mut self, snap: Snapshot) -> Command<Message> {
        let (line, col) = snap.cursor;
        self.is_dirty = true;
        self.undo_new_group = true;
        self.replace_buffer_text(&snap.text);
        Command::perform(async move { (line, col) }, |(l, c)| {
            Message::RestoreCursor(l, c)
        })
    }

    /// Applies a whole-buffer text transformation as one undo step and moves
    /// the cursor to (`line`, `col`). A no-op transformation changes nothing.
    fn apply_text_edit(&mut self, new_text: String, line: usize, col: usize) -> Command<Message> {
        if new_text == crate::editor::buffer_text(&self.editor.content) {
            return Command::none();
        }
        self.push_undo();
        self.is_dirty = true;
        self.undo_new_group = true;
        self.replace_buffer_text(&new_text);
        Command::perform(async move { (line, col) }, |(l, c)| {
            Message::RestoreCursor(l, c)
        })
    }

    fn replace_buffer_text(&mut self, new_text: &str) {
        crate::editor::set_buffer_text(&mut self.editor.content, new_text);
    }

    /// The (first, last) logical lines covered by a multi-line selection.
    /// `text_editor` exposes the selected text and the cursor, not the
    /// anchor, so the start line is inferred by checking which end of the
    /// selection the cursor is on against the buffer's actual lines.
    fn selection_lines(&self) -> Option<(usize, usize)> {
        let selected = self.editor.content.selection()?;
        let n = selected.split('\n').count();
        if n < 2 {
            return None;
        }
        let first = selected.split('\n').next()?;
        let last = selected.rsplit('\n').next()?;
        let (cursor_line, cursor_col) = self.editor.content.cursor_position();
        let fits = |start: usize| -> bool {
            let (Some(a), Some(b)) = (
                self.editor.content.line(start),
                self.editor.content.line(start + n - 1),
            ) else {
                return false;
            };
            a.ends_with(first) && b.starts_with(last)
        };
        // Cursor at the end of the selection (the usual drag direction)...
        if cursor_line + 1 >= n && cursor_col == last.len() && fits(cursor_line + 1 - n) {
            let start = cursor_line + 1 - n;
            return Some((start, cursor_line));
        }
        // ...or at its start.
        fits(cursor_line).then_some((cursor_line, cursor_line + n - 1))
    }

    /// Auto-closing of brackets and quotes for the typed character `ch`.
    /// `None` means "insert it normally".
    fn autopair(&mut self, ch: char, action: &text_editor::Action) -> Option<Command<Message>> {
        use text_editor::{Action, Edit, Motion};

        let closer = match ch {
            '(' => Some(')'),
            '[' => Some(']'),
            '{' => Some('}'),
            _ => None,
        };
        let is_quote = matches!(ch, '"' | '\'' | '`');
        let is_closer = matches!(ch, ')' | ']' | '}');
        let enabled = if is_quote {
            self.config.autocomplete_quotes
        } else {
            self.config.autocomplete_brackets
        };
        if !enabled || !(closer.is_some() || is_quote || is_closer) {
            return None;
        }

        let (line, col) = self.editor.content.cursor_position();
        let (before, after) = self
            .editor
            .content
            .line(line)
            .map(|l| {
                let col = col.min(l.len());
                (
                    l.get(..col).and_then(|s| s.chars().next_back()),
                    l.get(col..).and_then(|s| s.chars().next()),
                )
            })
            .unwrap_or((None, None));
        let selection = self.editor.content.selection();

        // Typing the closing character that is already there steps over it.
        if (is_closer || is_quote) && selection.is_none() && after == Some(ch) {
            self.editor.content.perform(Action::Move(Motion::Right));
            return Some(Command::none());
        }
        if is_closer {
            return None;
        }
        let close = closer.unwrap_or(ch);

        // With a selection, surround it instead of replacing it.
        if let Some(selected) = selection {
            self.push_undo();
            self.undo_new_group = true;
            self.editor
                .content
                .perform(Action::Edit(Edit::Paste(std::sync::Arc::new(format!(
                    "{ch}{selected}{close}"
                )))));
            self.is_dirty = true;
            return Some(Command::none());
        }

        // Only pair before whitespace / a closer / line end; quotes also
        // not after a word character (apostrophes: don't, it's).
        let room = after.map_or(true, |c| {
            c.is_whitespace() || matches!(c, ')' | ']' | '}' | ',' | ';' | ':' | '.')
        });
        let word_before = before.is_some_and(|c| c.is_alphanumeric() || c == '\\');
        if !room || (is_quote && word_before) {
            return None;
        }

        self.editor.content.perform(action.clone());
        self.editor
            .content
            .perform(Action::Edit(Edit::Insert(close)));
        self.is_dirty = true;
        // Motion::Left is a no-op on unshaped lines (cosmic_text bug).
        // Defer to the next update cycle so layout() shapes the buffer first.
        Some(Command::perform(async {}, |_| Message::MoveCursorLeft))
    }

    /// Whether the tab for `path` holds unsaved changes.
    fn is_path_dirty(&self, path: &Path) -> bool {
        if self.current_file.as_deref() == Some(path) {
            self.is_dirty
        } else {
            self.file_cache.get(path).is_some_and(|e| e.2)
        }
    }

    /// Closes a tab, asking first if that would discard unsaved changes.
    fn request_close(&mut self, path: PathBuf) -> Command<Message> {
        if self.is_path_dirty(&path) {
            self.overlay = ActiveOverlay::ConfirmClose(path);
            Command::none()
        } else {
            self.close_path(path)
        }
    }

    /// Removes a tab and, if it was the active one, switches to its neighbour.
    fn close_path(&mut self, path: PathBuf) -> Command<Message> {
        let index = self.sidebar.files().iter().position(|p| *p == path);
        self.file_cache.remove(&path);
        self.undo_cache.remove(&path);
        self.sidebar.remove_path(&path);

        let mut command = Command::none();
        if self.current_file.as_ref() == Some(&path) {
            self.editor = EditorState::new();
            self.reset_view_mode();
            self.current_file = None;
            self.is_dirty = false;
            self.undo_stack.clear();
            self.redo_stack.clear();
            self.undo_new_group = true;
            self.search.reset();
            self.status_message = t!("status.ready").to_string();
            let files = self.sidebar.files();
            let next = index
                .and_then(|i| files.get(i))
                .or_else(|| files.last())
                .cloned();
            if let Some(next) = next {
                command = self.activate(next);
            }
        }
        self.save_session();
        command
    }

    /// Parks the active buffer (text, language, dirty flag, undo history) so
    /// another file can take the editor.
    fn stash_current(&mut self) {
        if let Some(current) = self.current_file.clone() {
            self.file_cache.insert(
                current.clone(),
                (
                    crate::editor::buffer_text(&self.editor.content),
                    self.editor.language.clone(),
                    self.is_dirty,
                ),
            );
            self.undo_cache.insert(
                current,
                (
                    std::mem::take(&mut self.undo_stack),
                    std::mem::take(&mut self.redo_stack),
                ),
            );
        }
    }

    /// Puts a file's content in the editor and makes it the current tab.
    fn load_buffer(
        &mut self,
        path: PathBuf,
        content: String,
        language: Option<String>,
        dirty: bool,
    ) {
        self.editor = EditorState::from_content(&content);
        self.editor.restore_language(language, &path);
        self.reset_view_mode();
        self.sidebar.add_file(path.clone());
        let (undo, redo) = self.undo_cache.remove(&path).unwrap_or_default();
        self.undo_stack = undo;
        self.redo_stack = redo;
        self.undo_new_group = true;
        self.search.reset();
        self.disk_mtime = (!is_untitled(&path)).then(|| mtime(&path)).flatten();
        self.external_warned = false;
        self.current_file = Some(path);
        self.is_dirty = dirty;
    }

    /// Switches to a tab: instantly if its buffer is cached, from disk otherwise.
    fn activate(&mut self, path: PathBuf) -> Command<Message> {
        if self.current_file.as_ref() == Some(&path) {
            return Command::none();
        }
        if let Some((content, language, dirty)) = self.file_cache.remove(&path) {
            self.stash_current();
            self.load_buffer(path, content, language, dirty);
            self.status_message = t!("status.file_opened").to_string();
            Command::none()
        } else if is_untitled(&path) {
            self.stash_current();
            self.load_buffer(path, String::new(), None, false);
            Command::none()
        } else {
            // FileOpened parks the current buffer once the read succeeds.
            Command::perform(read_file(path), Message::FileOpened)
        }
    }

    /// Notices edits made to the current file by other programs: a clean
    /// buffer is reloaded, a modified one is warned about (saving then asks
    /// before overwriting).
    fn check_external_change(&mut self) -> Command<Message> {
        let Some(path) = self.current_file.clone().filter(|p| !is_untitled(p)) else {
            return Command::none();
        };
        let now = mtime(&path);
        if now.is_none() || now == self.disk_mtime {
            return Command::none();
        }
        if self.is_dirty {
            if !self.external_warned {
                self.external_warned = true;
                self.status_message = t!("status.changed_on_disk").to_string();
            }
            return Command::none();
        }
        self.disk_mtime = now;
        Command::perform(read_file(path), Message::FileOpened)
    }

    fn refresh_file_size(&mut self) {
        if self.size_for != self.current_file {
            self.size_for = self.current_file.clone();
            self.file_size = self
                .current_file
                .as_ref()
                .filter(|p| !is_untitled(p))
                .and_then(|p| std::fs::metadata(p).ok())
                .map(|m| m.len());
        }
    }

    /// The preview offered by the current buffer, if any.
    fn preview_kind(&self) -> Option<PreviewKind> {
        preview::kind_for(self.editor.language.as_deref())
    }

    /// Back to the plain editor: called whenever the buffer is replaced by a
    /// different file, since folded paths and the mode belong to one document.
    fn reset_view_mode(&mut self) {
        self.view_mode = ViewMode::Raw;
        self.json_collapsed = Default::default();
        self.preview_rev += 1;
        self.preview = PreviewDoc::None;
        self.preview_src.clear();
    }

    /// Reparses the buffer for the preview pane, but only when it is visible
    /// and its text actually changed since the last parse.
    fn refresh_preview(&mut self) {
        let Some(kind) = self.preview_kind() else {
            self.preview = PreviewDoc::None;
            self.preview_src.clear();
            return;
        };
        if self.view_mode == ViewMode::Raw {
            return;
        }

        let src = crate::editor::buffer_text(&self.editor.content);
        if matches!(self.preview, PreviewDoc::None) || src != self.preview_src {
            self.preview = match kind {
                PreviewKind::Markdown => {
                    PreviewDoc::Markdown(std::sync::Arc::new(crate::preview::markdown::parse(&src)))
                }
                PreviewKind::Json => match crate::preview::json::parse(&src) {
                    Ok(value) => PreviewDoc::Json(std::sync::Arc::new(value)),
                    Err(e) => PreviewDoc::JsonError(e),
                },
            };
            self.preview_src = src;
            self.preview_rev += 1;
        }
    }
}

// ─── Editor pane ────────────────────────────────────────────────────────────

impl SimpleEditApp {
    /// The editor pane: the text editor alone, or the mode toolbar above the
    /// editor, the preview, or both side by side.
    fn editor_pane(
        &self,
        kind: Option<PreviewKind>,
        mode: ViewMode,
        dark: bool,
    ) -> Element<'_, Message> {
        let search = self.show_search.then_some(&self.search);
        let Some(kind) = kind else {
            return self.editor.view(&self.config, search);
        };

        let body: Element<'_, Message> = match mode {
            ViewMode::Raw => self.editor.view(&self.config, search),
            ViewMode::Preview => self.preview_view(dark),
            ViewMode::Split => row![
                container(self.editor.view(&self.config, search)).width(Length::FillPortion(1)),
                container(Space::with_width(1))
                    .height(Length::Fill)
                    .style(se_theme::gutter(dark)),
                container(self.preview_view(dark)).width(Length::FillPortion(1)),
            ]
            .height(Length::Fill)
            .into(),
        };

        column![crate::preview::toolbar(kind, mode, dark), body]
            .height(Length::Fill)
            .into()
    }

    /// The rendered document, or the parser error when the JSON is invalid.
    fn preview_view(&self, dark: bool) -> Element<'_, Message> {
        match &self.preview {
            // `lazy` keeps the built widget tree until the revision changes:
            // rebuilding up to 20 000 JSON rows on every frame is wasteful.
            PreviewDoc::Markdown(blocks) => {
                let blocks = blocks.clone();
                iced::widget::lazy((self.preview_rev, dark), move |_| {
                    crate::preview::markdown::view(&blocks, dark)
                })
                .into()
            }
            PreviewDoc::Json(value) => {
                let (value, collapsed) = (value.clone(), self.json_collapsed.clone());
                iced::widget::lazy((self.preview_rev, dark), move |_| {
                    crate::preview::json::view(&value, &collapsed, dark)
                })
                .into()
            }
            PreviewDoc::JsonError(err) => container(
                column![
                    text(t!("preview.json_error").to_string())
                        .size(13)
                        .style(iced::Color::from_rgb(0.88, 0.27, 0.18)),
                    text(err.clone())
                        .size(12)
                        .font(iced::Font::MONOSPACE)
                        .style(se_theme::muted_text(dark)),
                ]
                .spacing(6),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .padding([16, 20])
            .into(),
            PreviewDoc::None => container(Space::with_height(0))
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
        }
    }
}

// ─── Overlay views ──────────────────────────────────────────────────────────

impl SimpleEditApp {
    fn language_picker_enabled(&self) -> bool {
        match &self.current_file {
            None => true,
            Some(path) => {
                if is_untitled(path) {
                    return true;
                }
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                // Enabled if the extension is NOT recognized (user must override)
                !crate::editor::SUPPORTED_LANGUAGES
                    .iter()
                    .any(|(tok, _)| *tok == ext)
            }
        }
    }

    fn view_about_overlay(&self, dark: bool) -> Element<'_, Message> {
        let p = se_theme::palette(dark);
        let version = env!("CARGO_PKG_VERSION");
        container(
            column![
                text("SimpleEdit").size(24).style(p.accent),
                text(format!("v{}", version)).size(15).style(p.text),
                text(t!("about.tagline").to_string())
                    .size(12)
                    .style(p.muted),
                button(text(t!("about.close").to_string()).size(13))
                    .padding([8, 24])
                    .on_press(Message::CloseOverlay)
                    .style(iced::theme::Button::custom(se_theme::GhostButton {
                        dark,
                        active: true,
                    })),
            ]
            .spacing(12)
            .align_items(Alignment::Center)
            .padding(36),
        )
        .width(320)
        .style(se_theme::card(dark))
        .into()
    }

    fn view_update_overlay(&self, dark: bool) -> Element<'_, Message> {
        let p = se_theme::palette(dark);

        let content: Element<'_, Message> = match &self.overlay {
            ActiveOverlay::UpdateChecking => column![text(t!("update.checking").to_string())
                .size(14)
                .style(p.text),]
            .spacing(12)
            .align_items(Alignment::Center)
            .padding(36)
            .into(),

            ActiveOverlay::UpdateUpToDate => column![
                text(t!("update.up_to_date").to_string())
                    .size(16)
                    .style(p.accent),
                button(text(t!("about.close").to_string()).size(13))
                    .padding([8, 24])
                    .on_press(Message::CloseOverlay)
                    .style(iced::theme::Button::custom(se_theme::GhostButton {
                        dark,
                        active: true,
                    })),
            ]
            .spacing(12)
            .align_items(Alignment::Center)
            .padding(36)
            .into(),

            ActiveOverlay::UpdateAvailable(version) => {
                let version = version.clone();
                column![
                    text(t!("update.available", version = version.as_str()).to_string())
                        .size(16)
                        .style(p.accent),
                    row![
                        button(text(t!("about.close").to_string()).size(13))
                            .padding([8, 20])
                            .on_press(Message::CloseOverlay)
                            .style(iced::theme::Button::custom(se_theme::GhostButton {
                                dark,
                                active: false,
                            })),
                        button(text(t!("update.download_install").to_string()).size(13))
                            .padding([8, 20])
                            .on_press(Message::ConfirmInstallUpdate(version))
                            .style(iced::theme::Button::custom(se_theme::GhostButton {
                                dark,
                                active: true,
                            })),
                    ]
                    .spacing(12),
                ]
                .spacing(16)
                .align_items(Alignment::Center)
                .padding(36)
                .into()
            }

            ActiveOverlay::UpdateDownloading => column![text(t!("update.downloading").to_string())
                .size(14)
                .style(p.text),]
            .spacing(12)
            .align_items(Alignment::Center)
            .padding(36)
            .into(),

            ActiveOverlay::UpdateInstalling => column![text(t!("update.installing").to_string())
                .size(14)
                .style(p.text),]
            .spacing(12)
            .align_items(Alignment::Center)
            .padding(36)
            .into(),

            ActiveOverlay::UpdateError(e) => column![
                text(t!("update.error").to_string())
                    .size(14)
                    .style(p.accent),
                text(e.clone()).size(12).style(p.muted),
                button(text(t!("about.close").to_string()).size(13))
                    .padding([8, 24])
                    .on_press(Message::CloseOverlay)
                    .style(iced::theme::Button::custom(se_theme::GhostButton {
                        dark,
                        active: false,
                    })),
            ]
            .spacing(12)
            .align_items(Alignment::Center)
            .padding(36)
            .into(),

            _ => unreachable!(),
        };

        container(content)
            .width(360)
            .style(se_theme::card(dark))
            .into()
    }

    fn view_confirm_overlay(&self, dark: bool, path: &Path) -> Element<'_, Message> {
        let name = file_label(path);
        confirm_card(
            dark,
            t!("confirm.title").to_string(),
            t!("confirm.message", name = name.as_str()).to_string(),
            t!("confirm.discard").to_string(),
            Message::ConfirmDiscard(path.to_path_buf()),
        )
    }

    fn view_overwrite_overlay(&self, dark: bool, path: &Path) -> Element<'_, Message> {
        let name = file_label(path);
        confirm_card(
            dark,
            t!("overwrite.title").to_string(),
            t!("overwrite.message", name = name.as_str()).to_string(),
            t!("overwrite.confirm").to_string(),
            Message::ConfirmOverwrite,
        )
    }

    fn view_goto_line_overlay(&self, dark: bool) -> Element<'_, Message> {
        let p = se_theme::palette(dark);
        container(
            column![
                text(t!("goto_line.title").to_string())
                    .size(14)
                    .style(p.text),
                text_input(t!("goto_line.placeholder").as_ref(), &self.goto_line_input)
                    .id(goto_line_id())
                    .on_input(Message::GotoLineInputChanged)
                    .on_submit(Message::GotoLineSubmit)
                    .padding(8)
                    .size(14),
                row![
                    button(text(t!("goto_line.cancel").to_string()).size(13))
                        .padding([7, 16])
                        .on_press(Message::CloseOverlay)
                        .style(iced::theme::Button::custom(se_theme::GhostButton {
                            dark,
                            active: false,
                        })),
                    Space::with_width(Length::Fill),
                    button(text(t!("goto_line.go").to_string()).size(13))
                        .padding([7, 16])
                        .on_press(Message::GotoLineSubmit)
                        .style(iced::theme::Button::custom(se_theme::GhostButton {
                            dark,
                            active: true,
                        })),
                ]
                .align_items(Alignment::Center),
            ]
            .spacing(14)
            .padding(24),
        )
        .width(320)
        .style(se_theme::card(dark))
        .into()
    }

    fn view_language_picker_overlay(&self, dark: bool) -> Element<'_, Message> {
        let p = se_theme::palette(dark);
        let current_lang = self.editor.language.as_deref().unwrap_or("");

        let mut items: Vec<Element<Message>> = vec![
            // "None / auto" row
            button(
                row![
                    text(t!("language.auto").to_string()).size(13),
                    Space::with_width(Length::Fill),
                    if current_lang.is_empty() {
                        text("✓").size(13).style(p.accent)
                    } else {
                        text("").size(13)
                    },
                ]
                .align_items(Alignment::Center),
            )
            .padding([6, 12])
            .width(Length::Fill)
            .on_press(Message::SetLanguage(String::new()))
            .style(iced::theme::Button::custom(se_theme::GhostButton {
                dark,
                active: current_lang.is_empty(),
            }))
            .into(),
        ];

        for (token, name) in crate::editor::SUPPORTED_LANGUAGES {
            let is_current = current_lang == *token;
            items.push(
                button(
                    row![
                        text(*name).size(13),
                        Space::with_width(Length::Fill),
                        if is_current {
                            text("✓").size(13).style(p.accent)
                        } else {
                            text("").size(13)
                        },
                    ]
                    .align_items(Alignment::Center),
                )
                .padding([6, 12])
                .width(Length::Fill)
                .on_press(Message::SetLanguage(token.to_string()))
                .style(iced::theme::Button::custom(se_theme::GhostButton {
                    dark,
                    active: is_current,
                }))
                .into(),
            );
        }

        container(
            column![
                text(t!("language.title").to_string())
                    .size(14)
                    .style(p.text),
                scrollable(column(items).spacing(2).padding(4)).height(Length::Fixed(320.0)),
            ]
            .spacing(10)
            .padding(16),
        )
        .width(260)
        .style(se_theme::card(dark))
        .into()
    }

    fn view_shortcuts_overlay(
        &self,
        dark: bool,
        capturing: Option<ShortcutTarget>,
    ) -> Element<'_, Message> {
        let p = se_theme::palette(dark);
        let sc = &self.config.shortcuts;

        let rows_data: Vec<(ShortcutTarget, String, &crate::config::ShortcutConfig)> = vec![
            (
                ShortcutTarget::Undo,
                t!("shortcuts.undo").to_string(),
                &sc.undo,
            ),
            (
                ShortcutTarget::Redo,
                t!("shortcuts.redo").to_string(),
                &sc.redo,
            ),
            (
                ShortcutTarget::DuplicateLine,
                t!("shortcuts.duplicate_line").to_string(),
                &sc.duplicate_line,
            ),
            (
                ShortcutTarget::MoveLineUp,
                t!("shortcuts.move_line_up").to_string(),
                &sc.move_line_up,
            ),
            (
                ShortcutTarget::MoveLineDown,
                t!("shortcuts.move_line_down").to_string(),
                &sc.move_line_down,
            ),
            (
                ShortcutTarget::ToggleComment,
                t!("shortcuts.toggle_comment").to_string(),
                &sc.toggle_comment,
            ),
            (
                ShortcutTarget::DeleteLine,
                t!("shortcuts.delete_line").to_string(),
                &sc.delete_line,
            ),
            (
                ShortcutTarget::NewFile,
                t!("shortcuts.new_file").to_string(),
                &sc.new_file,
            ),
            (
                ShortcutTarget::OpenFile,
                t!("shortcuts.open_file").to_string(),
                &sc.open_file,
            ),
            (
                ShortcutTarget::SaveFile,
                t!("shortcuts.save_file").to_string(),
                &sc.save_file,
            ),
            (
                ShortcutTarget::SaveAs,
                t!("shortcuts.save_as").to_string(),
                &sc.save_as,
            ),
            (
                ShortcutTarget::CloseFile,
                t!("shortcuts.close_file").to_string(),
                &sc.close_file,
            ),
            (
                ShortcutTarget::Find,
                t!("shortcuts.find").to_string(),
                &sc.find,
            ),
            (
                ShortcutTarget::SelectAll,
                t!("shortcuts.select_all").to_string(),
                &sc.select_all,
            ),
            (
                ShortcutTarget::FormatCode,
                t!("shortcuts.format_code").to_string(),
                &sc.format_code,
            ),
            (
                ShortcutTarget::GotoLine,
                t!("shortcuts.goto_line").to_string(),
                &sc.goto_line,
            ),
            (
                ShortcutTarget::ToggleSidebar,
                t!("shortcuts.toggle_sidebar").to_string(),
                &sc.toggle_sidebar,
            ),
            (
                ShortcutTarget::Quit,
                t!("shortcuts.quit").to_string(),
                &sc.quit,
            ),
        ];

        let mut rows: Vec<Element<Message>> = vec![row![
            text(t!("shortcuts.action").to_string())
                .size(11)
                .style(p.muted),
            Space::with_width(Length::Fill),
            text(t!("shortcuts.key").to_string())
                .size(11)
                .style(p.muted),
        ]
        .padding([4, 12])
        .into()];

        for (target, label_str, shortcut) in &rows_data {
            let target = *target;
            let is_capturing = capturing == Some(target);
            let key_label = if is_capturing {
                t!("shortcuts.capturing").to_string()
            } else {
                shortcut.display()
            };

            let sc_btn = button(
                text(key_label)
                    .size(12)
                    .style(if is_capturing { p.accent } else { p.text })
                    .font(iced::Font::MONOSPACE),
            )
            .padding([4, 10])
            .on_press(Message::StartCaptureShortcut(target))
            .style(iced::theme::Button::custom(se_theme::GhostButton {
                dark,
                active: is_capturing,
            }));

            rows.push(
                row![
                    text(label_str.clone()).size(13).style(p.text),
                    Space::with_width(Length::Fill),
                    sc_btn,
                ]
                .padding([3, 12])
                .align_items(Alignment::Center)
                .into(),
            );
        }

        let hint = if capturing.is_some() {
            t!("shortcuts.capturing").to_string()
        } else {
            t!("shortcuts.click_to_rebind").to_string()
        };

        container(
            column![
                row![
                    text(t!("shortcuts.title").to_string())
                        .size(14)
                        .style(p.text),
                    Space::with_width(Length::Fill),
                    button(text("✕").size(12))
                        .padding([4, 8])
                        .on_press(Message::CloseOverlay)
                        .style(iced::theme::Button::custom(se_theme::GhostButton {
                            dark,
                            active: false,
                        })),
                ]
                .align_items(Alignment::Center),
                scrollable(column(rows).spacing(2)).height(Length::Fixed(380.0)),
                text(hint).size(11).style(p.muted),
            ]
            .spacing(10)
            .padding(20),
        )
        .width(400)
        .style(se_theme::card(dark))
        .into()
    }

    // ─── Session persistence ─────────────────────────────────────────────────

    fn save_session(&mut self) {
        use crate::session::{FileSession, Session};
        let files: Vec<FileSession> = self
            .sidebar
            .files()
            .iter()
            .map(|path| {
                let (content, language, dirty) = if self.current_file.as_ref() == Some(path) {
                    (
                        crate::editor::buffer_text(&self.editor.content),
                        self.editor.language.clone(),
                        self.is_dirty,
                    )
                } else if let Some((c, l, d)) = self.file_cache.get(path) {
                    (c.clone(), l.clone(), *d)
                } else {
                    (String::new(), None, false)
                };
                FileSession {
                    path: path.to_str().unwrap_or("").to_string(),
                    content: if is_untitled(path) || dirty {
                        Some(content)
                    } else {
                        None
                    },
                    language,
                    dirty,
                }
            })
            .collect();

        let session = Session {
            files,
            active_file: self
                .current_file
                .as_ref()
                .and_then(|p| p.to_str())
                .map(|s| s.to_string()),
            untitled_counter: self.untitled_counter,
        };
        // Skip the write when nothing changed since the last one.
        if let Some(json) = session.to_json() {
            if json != self.last_session {
                Session::write(&json);
                self.last_session = json;
            }
        }
    }

    fn restore_session(&mut self, session: crate::session::Session) {
        self.untitled_counter = session.untitled_counter.max(1);

        for fs in &session.files {
            let path = PathBuf::from(&fs.path);
            let content = if let Some(c) = &fs.content {
                c.clone()
            } else if !is_untitled(&path) {
                // A file deleted since the last run is dropped, not shown empty.
                match std::fs::read_to_string(&path) {
                    Ok(c) => c,
                    Err(_) => continue,
                }
            } else {
                String::new()
            };
            self.sidebar.add_file(path.clone());
            self.file_cache
                .insert(path, (content, fs.language.clone(), fs.dirty));
        }

        if self.sidebar.files().is_empty() {
            // First launch (or nothing restorable): a blank untitled file
            let path = untitled_path(1);
            self.sidebar.add_file(path.clone());
            self.current_file = Some(path);
            self.untitled_counter = 1;
            return;
        }

        // Activate the previously active file, or the first one left
        let active_path = session
            .active_file
            .as_deref()
            .map(PathBuf::from)
            .filter(|p| self.file_cache.contains_key(p))
            .or_else(|| self.sidebar.files().first().cloned());

        if let Some(path) = active_path {
            if let Some((content, language, dirty)) = self.file_cache.remove(&path) {
                self.load_buffer(path, content, language, dirty);
            }
        }
    }
}

// ─── Text manipulation helpers ──────────────────────────────────────────────

/// The line-comment markers for a language token: (prefix, suffix). `None`
/// for formats that have no comments at all.
pub fn comment_style(language: Option<&str>) -> Option<(&'static str, &'static str)> {
    match language {
        Some("json") => None,
        Some("html") | Some("xml") | Some("md") => Some(("<!-- ", " -->")),
        Some("css") => Some(("/* ", " */")),
        Some("py") | Some("rb") | Some("sh") | Some("r") | Some("toml") | Some("yaml") => {
            Some(("# ", ""))
        }
        Some("sql") | Some("lua") => Some(("-- ", "")),
        _ => Some(("// ", "")),
    }
}

pub fn duplicate_line(text: &str, line_idx: usize) -> String {
    let mut lines: Vec<String> = text.split('\n').map(|l| l.to_string()).collect();
    if line_idx < lines.len() {
        let copy = lines[line_idx].clone();
        lines.insert(line_idx + 1, copy);
    }
    lines.join("\n")
}

/// Returns (new_text, new_cursor_line).
pub fn move_line(text: &str, line_idx: usize, up: bool) -> (String, usize) {
    let mut lines: Vec<String> = text.split('\n').map(|l| l.to_string()).collect();
    if up {
        if line_idx == 0 {
            return (text.to_string(), line_idx);
        }
        lines.swap(line_idx, line_idx - 1);
        (lines.join("\n"), line_idx - 1)
    } else {
        if line_idx + 1 >= lines.len() {
            return (text.to_string(), line_idx);
        }
        lines.swap(line_idx, line_idx + 1);
        (lines.join("\n"), line_idx + 1)
    }
}

/// Returns (new_text, new_cursor_line).
pub fn delete_line_op(text: &str, line_idx: usize) -> (String, usize) {
    let mut lines: Vec<String> = text.split('\n').map(|l| l.to_string()).collect();
    if line_idx >= lines.len() {
        return (text.to_string(), line_idx);
    }
    lines.remove(line_idx);
    let new_line = if lines.is_empty() {
        0
    } else {
        line_idx.min(lines.len() - 1)
    };
    (lines.join("\n"), new_line)
}

fn is_commented(trimmed: &str, prefix: &str, suffix: &str) -> bool {
    trimmed.starts_with(prefix.trim_end())
        && (suffix.is_empty() || trimmed.ends_with(suffix.trim_start()))
}

/// Comments lines `first..=last`, or uncomments them if every non-blank one
/// already is. Markers match with or without their padding space.
pub fn toggle_comment_lines(
    text: &str,
    first: usize,
    last: usize,
    prefix: &str,
    suffix: &str,
) -> String {
    let mut lines: Vec<String> = text.split('\n').map(|l| l.to_string()).collect();
    let last = last.min(lines.len().saturating_sub(1));
    if first > last {
        return text.to_string();
    }
    let range = first..=last;
    let all_commented = lines[range.clone()]
        .iter()
        .filter(|l| !l.trim().is_empty())
        .all(|l| is_commented(l.trim(), prefix, suffix))
        && lines[range.clone()].iter().any(|l| !l.trim().is_empty());

    for line in &mut lines[range] {
        if line.trim().is_empty() {
            continue;
        }
        let indent: String = line.chars().take_while(|c| c.is_whitespace()).collect();
        let content = line.trim();
        *line = if all_commented {
            let mut inner = content.strip_prefix(prefix.trim_end()).unwrap_or(content);
            inner = inner.strip_prefix(' ').unwrap_or(inner);
            if !suffix.is_empty() {
                inner = inner.strip_suffix(suffix.trim_start()).unwrap_or(inner);
                inner = inner.strip_suffix(' ').unwrap_or(inner);
            }
            format!("{indent}{inner}")
        } else {
            format!("{indent}{prefix}{content}{suffix}")
        };
    }
    lines.join("\n")
}

/// Adds one `indent` to every non-empty line in `first..=last`.
pub fn indent_lines(text: &str, first: usize, last: usize, indent: &str) -> String {
    text.split('\n')
        .enumerate()
        .map(|(i, line)| {
            if (first..=last).contains(&i) && !line.is_empty() {
                format!("{indent}{line}")
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Removes up to one `indent` (or a tab) from the start of a line.
fn dedent_one(line: &str, indent: &str) -> String {
    if let Some(stripped) = line.strip_prefix(indent) {
        stripped.to_string()
    } else if let Some(stripped) = line.strip_prefix('\t') {
        stripped.to_string()
    } else {
        let n = line
            .chars()
            .take(indent.len())
            .take_while(|c| *c == ' ')
            .count();
        line[n..].to_string()
    }
}

/// Removes one level of indentation from every line in `first..=last`.
pub fn dedent_lines(text: &str, first: usize, last: usize, indent: &str) -> String {
    text.split('\n')
        .enumerate()
        .map(|(i, line)| {
            if (first..=last).contains(&i) {
                dedent_one(line, indent)
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn file_label(path: &Path) -> String {
    if is_untitled(path) {
        t!("app.untitled").to_string()
    } else {
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("?")
            .to_string()
    }
}

/// A small modal asking to confirm a destructive action (or cancel).
fn confirm_card(
    dark: bool,
    title: String,
    message: String,
    confirm_label: String,
    confirm: Message,
) -> Element<'static, Message> {
    let p = se_theme::palette(dark);
    container(
        column![
            text(title).size(15).style(p.text),
            text(message).size(12).style(p.muted),
            row![
                button(text(t!("confirm.cancel").to_string()).size(13))
                    .padding([7, 16])
                    .on_press(Message::CloseOverlay)
                    .style(iced::theme::Button::custom(se_theme::GhostButton {
                        dark,
                        active: true,
                    })),
                Space::with_width(Length::Fill),
                button(text(confirm_label).size(13))
                    .padding([7, 16])
                    .on_press(confirm)
                    .style(iced::theme::Button::custom(se_theme::GhostButton {
                        dark,
                        active: false,
                    })),
            ]
            .align_items(Alignment::Center),
        ]
        .spacing(14)
        .padding(24),
    )
    .width(360)
    .style(se_theme::card(dark))
    .into()
}

fn goto_line_id() -> text_input::Id {
    text_input::Id::new("goto-line")
}

// ─── Helpers ────────────────────────────────────────────────────────────────

pub fn untitled_path(n: u32) -> PathBuf {
    PathBuf::from(format!("untitled://{}", n))
}

pub fn is_untitled(path: &Path) -> bool {
    path.to_str()
        .map(|s| s.starts_with("untitled://"))
        .unwrap_or(false)
}

// ─── File I/O ────────────────────────────────────────────────────────────────

/// Reads a text file, refusing ones too large to edit comfortably.
fn read_text(path: &Path) -> Result<String, String> {
    let len = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    if len > MAX_OPEN_BYTES {
        return Err(t!("status.err_too_large", mb = MAX_OPEN_BYTES / (1024 * 1024)).to_string());
    }
    std::fs::read_to_string(path).map_err(|e| e.to_string())
}

async fn open_file() -> Result<(PathBuf, String), String> {
    let handle = rfd::AsyncFileDialog::new()
        .set_title(t!("dialog.open_title").to_string())
        .add_filter(
            t!("dialog.filter_supported").to_string(),
            &[
                "txt",
                "md",
                "markdown",
                "rst",
                "rs",
                "toml",
                "lock",
                "py",
                "pyw",
                "js",
                "jsx",
                "mjs",
                "cjs",
                "ts",
                "tsx",
                "html",
                "htm",
                "xhtml",
                "css",
                "scss",
                "sass",
                "less",
                "json",
                "json5",
                "jsonc",
                "yaml",
                "yml",
                "xml",
                "svg",
                "plist",
                "sh",
                "bash",
                "zsh",
                "fish",
                "ps1",
                "c",
                "h",
                "cpp",
                "cc",
                "cxx",
                "hpp",
                "hh",
                "java",
                "go",
                "rb",
                "php",
                "swift",
                "kt",
                "kts",
                "sql",
                "lua",
                "r",
                "m",
                "vb",
                "cs",
                "makefile",
                "dockerfile",
                "gitignore",
                "env",
                "conf",
                "cfg",
                "ini",
                "properties",
                "log",
            ],
        )
        .add_filter(
            t!("dialog.filter_text").to_string(),
            &["txt", "md", "rst", "log"],
        )
        .add_filter(
            t!("dialog.filter_source").to_string(),
            &[
                "rs", "py", "js", "ts", "jsx", "tsx", "c", "h", "cpp", "java", "go", "rb", "php",
                "swift", "kt", "lua", "sql",
            ],
        )
        .add_filter(
            t!("dialog.filter_web").to_string(),
            &["html", "htm", "css", "scss", "json", "xml", "svg"],
        )
        .add_filter(
            t!("dialog.filter_config").to_string(),
            &["toml", "yaml", "yml", "ini", "cfg", "conf", "env"],
        )
        .add_filter(t!("dialog.filter_all").to_string(), &["*"])
        .pick_file()
        .await
        .ok_or_else(|| CANCELLED.to_string())?;

    read_file(handle.path().to_path_buf()).await
}

async fn read_file(path: PathBuf) -> Result<(PathBuf, String), String> {
    tokio::task::spawn_blocking(move || read_text(&path).map(|content| (path, content)))
        .await
        .map_err(|e| e.to_string())?
}

/// Writes `content` atomically; returns the path and exactly what was written.
async fn save_file(path: PathBuf, content: String) -> Result<(PathBuf, String), String> {
    tokio::task::spawn_blocking(move || {
        crate::fsutil::write_atomic(&path, content.as_bytes(), false)
            .map(|()| (path, content))
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

async fn save_file_as(content: String) -> Result<(PathBuf, String), String> {
    let handle = rfd::AsyncFileDialog::new()
        .set_title(t!("dialog.save_title").to_string())
        .save_file()
        .await
        .ok_or_else(|| CANCELLED.to_string())?;

    save_file(handle.path().to_path_buf(), content).await
}

/// Runs a save and reports it together with the tab it started from.
fn save_command(
    origin: Option<PathBuf>,
    save: impl std::future::Future<Output = Result<(PathBuf, String), String>> + Send + 'static,
) -> Command<Message> {
    Command::perform(save, move |result| {
        Message::FileSaved(origin.clone(), result)
    })
}

#[cfg(test)]
mod text_ops_tests {
    use super::*;

    #[test]
    fn duplicate_line_middle() {
        let text = "line1\nline2\nline3";
        let result = duplicate_line(text, 1);
        assert_eq!(result, "line1\nline2\nline2\nline3");
    }

    #[test]
    fn duplicate_line_first() {
        let text = "abc\ndef";
        let result = duplicate_line(text, 0);
        assert_eq!(result, "abc\nabc\ndef");
    }

    #[test]
    fn move_line_up_basic() {
        let text = "aaa\nbbb\nccc";
        let (result, new_line) = move_line(text, 1, true);
        assert_eq!(result, "bbb\naaa\nccc");
        assert_eq!(new_line, 0);
    }

    #[test]
    fn move_line_up_noop_at_top() {
        let text = "aaa\nbbb";
        let (result, new_line) = move_line(text, 0, true);
        assert_eq!(result, "aaa\nbbb");
        assert_eq!(new_line, 0);
    }

    #[test]
    fn move_line_down_basic() {
        let text = "aaa\nbbb\nccc";
        let (result, new_line) = move_line(text, 0, false);
        assert_eq!(result, "bbb\naaa\nccc");
        assert_eq!(new_line, 1);
    }

    #[test]
    fn delete_line_middle() {
        let text = "aaa\nbbb\nccc";
        let (result, new_line) = delete_line_op(text, 1);
        assert_eq!(result, "aaa\nccc");
        assert_eq!(new_line, 1);
    }

    #[test]
    fn delete_line_last() {
        let text = "aaa\nbbb";
        let (result, new_line) = delete_line_op(text, 1);
        assert_eq!(result, "aaa");
        assert_eq!(new_line, 0);
    }

    #[test]
    fn toggle_comment_add_and_remove_roundtrip() {
        let text = "    let x = 1;";
        let on = toggle_comment_lines(text, 0, 0, "// ", "");
        assert_eq!(on, "    // let x = 1;");
        assert_eq!(toggle_comment_lines(&on, 0, 0, "// ", ""), text);
    }

    #[test]
    fn uncomment_accepts_markers_without_the_padding_space() {
        assert_eq!(toggle_comment_lines("//foo", 0, 0, "// ", ""), "foo");
        assert_eq!(toggle_comment_lines("#foo", 0, 0, "# ", ""), "foo");
    }

    #[test]
    fn toggle_comment_covers_a_line_range_and_skips_blank_lines() {
        let text = "a\n\nb\nc";
        let on = toggle_comment_lines(text, 0, 2, "// ", "");
        assert_eq!(on, "// a\n\n// b\nc");
        assert_eq!(toggle_comment_lines(&on, 0, 2, "// ", ""), text);
        // A mixed selection comments everything first.
        assert_eq!(
            toggle_comment_lines("// a\nb", 0, 1, "// ", ""),
            "// // a\n// b"
        );
    }

    #[test]
    fn block_comment_styles_wrap_the_line() {
        let on = toggle_comment_lines("p { }", 0, 0, "/* ", " */");
        assert_eq!(on, "/* p { } */");
        assert_eq!(toggle_comment_lines(&on, 0, 0, "/* ", " */"), "p { }");
    }

    #[test]
    fn indent_lines_only_touches_the_range() {
        let text = "a\n\nb\nc";
        assert_eq!(indent_lines(text, 0, 2, "  "), "  a\n\n  b\nc");
    }

    #[test]
    fn dedent_lines_basic_and_partial() {
        assert_eq!(dedent_lines("    a\n    b", 0, 1, "    "), "a\nb");
        assert_eq!(dedent_lines("  a\nb\n\tc", 0, 2, "    "), "a\nb\nc");
        assert_eq!(dedent_lines("    a\n    b", 0, 0, "    "), "a\n    b");
    }

    #[test]
    fn comment_style_by_language() {
        assert_eq!(comment_style(Some("rs")), Some(("// ", "")));
        assert_eq!(comment_style(Some("py")), Some(("# ", "")));
        assert_eq!(comment_style(Some("sql")), Some(("-- ", "")));
        assert_eq!(comment_style(Some("css")), Some(("/* ", " */")));
        assert_eq!(comment_style(Some("html")), Some(("<!-- ", " -->")));
        assert_eq!(comment_style(Some("json")), None);
        assert_eq!(comment_style(None), Some(("// ", "")));
    }
}
