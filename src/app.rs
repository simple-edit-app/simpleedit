use iced::{
    widget::{button, column, container, row, scrollable, text, text_editor, text_input, Space},
    Alignment, Element, Length, Task, Theme,
};
use std::collections::{HashMap, HashSet};

use crate::history::{History, Snapshot};
use std::path::{Path, PathBuf};

use crate::{
    config::{Config, ShortcutConfig, ShortcutTarget},
    editor::EditorState,
    preferences::{PreferencesMessage, PreferencesState},
    preview::{self, PreviewKind, ViewMode},
    search::{SearchMessage, SearchState},
    sidebar::{SidebarAction, SidebarMessage, SidebarState},
    tabs::{fingerprint, CachedBuffer, DiskStamp, Loaded, TextFormat},
    text_ops::{self, comment_style},
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
    /// Every release.
    Changelog,
    /// The releases since the given version, shown after an update.
    WhatsNew(String),
    GotoLine,
    LanguagePicker,
    Shortcuts,
    Palette,
    UpdateChecking,
    UpdateUpToDate,
    /// The new version, and what it changes.
    UpdateAvailable(String, Vec<crate::changelog::Entry>),
    UpdateDownloading,
    UpdateInstalling,
    UpdateError(String),
    /// Closing this tab would discard unsaved changes.
    ConfirmClose(PathBuf),
    /// The session couldn't be written: quitting would lose unsaved buffers.
    ConfirmQuit(String),
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
    /// A file chosen by the user (dialog, command line, recent files).
    FileOpened(Result<Loaded, String>),
    /// A file read to switch to its tab; superseded by a later switch.
    TabRead(Result<Loaded, String>),
    /// The open file changed on disk and was read again.
    FileReloaded(Result<Loaded, String>),
    SaveFile,
    SaveFileAs,
    /// The path picked in "Save as", with the tab it was started from.
    SaveAsChosen(Option<PathBuf>, Result<PathBuf, String>),
    /// The tab the save was started from, and the path + exact text written.
    FileSaved(Option<PathBuf>, Result<(PathBuf, String), String>),
    NewFile,
    CloseFile,
    ToggleSearch,
    ToggleSidebar,
    TogglePreferences,
    ThemeChanged(bool),
    /// Switches between the dark and the light theme.
    ToggleTheme,
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
    FileFormatted(FormatJob, Result<String, String>),
    SelectionFormatted(FormatJob, Result<String, String>),
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
    /// Enter pressed while a search field might own it; `shift` goes back.
    SearchEnter {
        shift: bool,
    },
    SearchEnterChecked {
        shift: bool,
        in_find_field: bool,
    },
    /// The autosave wrote (or failed to write) the session.
    SessionWritten(Result<(), String>),
    /// Quit although the session couldn't be saved.
    QuitAnyway,
    /// Re-parses the preview if the text changed (typing is debounced).
    PreviewTick,
    /// The window was resized (kept for the next start).
    WindowResized(f32, f32),
    // Overlays
    CloseOverlay,
    OpenGotoLine,
    GotoLineInputChanged(String),
    GotoLineSubmit,
    OpenLanguagePicker,
    SetLanguage(String),
    OpenShortcuts,
    StartCaptureShortcut(ShortcutTarget),
    // Task palette
    OpenPalette,
    PaletteQueryChanged(String),
    PaletteSubmit,
    PaletteRun(crate::palette::PaletteCommand),
    // Line-level editing
    DuplicateLine,
    MoveLineUp,
    MoveLineDown,
    ToggleComment,
    DeleteLine,
    // Bookmarks
    ToggleBookmark,
    NextBookmark,
    PrevBookmark,
    ClearBookmarks,
    /// A right click over the text, relative to the editor.
    EditorRightClick(iced::Point),
    IndentSelection,
    DedentLine,
    // Recent files
    OpenRecentFile(PathBuf),
    /// Whether a search field had focus when an editor shortcut was pressed.
    ShortcutChecked(Box<Message>, bool),
    /// The user confirmed closing a tab that has unsaved changes.
    ConfirmDiscard(PathBuf),
    /// The user chose to overwrite a file that changed on disk.
    ConfirmOverwrite,
    // Updater
    CheckForUpdate,
    UpdateChecked(Result<Option<(String, Vec<crate::changelog::Entry>)>, String>),
    OpenChangelog,
    ConfirmInstallUpdate(String),
    UpdateDownloaded(Result<(std::path::PathBuf, String), String>),
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

/// What the window starts with.
#[derive(Default)]
pub struct Flags {
    pub config: Config,
    /// Files named on the command line.
    pub files: Vec<PathBuf>,
}

pub struct SimpleEditApp {
    config: Config,
    /// Marked lines of every file, saved between runs.
    bookmarks: crate::bookmarks::Bookmarks,
    editor: EditorState,
    sidebar: SidebarState,
    search: SearchState,
    preferences: PreferencesState,
    show_search: bool,
    show_sidebar: bool,
    show_preferences: bool,
    /// The shortcut being rebound, while waiting for its new key.
    capturing: Option<ShortcutTarget>,
    palette_query: String,
    palette_selected: usize,
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
    file_cache: HashMap<PathBuf, CachedBuffer>,
    overlay: ActiveOverlay,
    goto_line_input: String,
    /// Word-level undo history (capped by count and total size).
    undo_stack: History,
    /// Redo history rebuilt when undoing.
    redo_stack: History,
    /// Undo/redo history of the tabs that aren't the active one.
    undo_cache: HashMap<PathBuf, (History, History)>,
    /// Preference changes are written out on the next autosave tick, not on
    /// every slider movement.
    config_dirty: bool,
    /// Fingerprint of the last session JSON written, so unchanged sessions
    /// aren't rewritten.
    last_session: u64,
    /// Held while this is the window that saves the session.
    session_lock: Option<crate::session::InstanceLock>,
    /// Whether this window saves the session and the bookmarks.
    session_owner: bool,
    /// Bumped by every message that may change what the session holds, so the
    /// autosave tick can skip the work when nothing happened.
    rev: u64,
    /// `rev` the session was last built at.
    session_rev: u64,
    /// How the current file stores its text (line endings, final newline).
    format: TextFormat,
    /// State on disk of the current file when it was last read or written,
    /// to notice edits made by other programs.
    disk_stamp: Option<DiskStamp>,
    /// Fingerprint of the text last read or saved, to see that an undo got
    /// back to it.
    saved_fp: Option<u64>,
    /// The tab the user last asked to switch to, so a slow read of another
    /// one doesn't take over.
    pending_open: Option<PathBuf>,
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
    /// The status-bar label of the selection (`None` without one), kept up to
    /// date by `update` so `view` doesn't copy a possibly huge selection on
    /// every frame.
    selection_info: Option<String>,
    /// What `selection_info` was computed for (selection, text revision).
    selection_key: Option<(Option<crate::code_editor::SelectionRange>, u64)>,
    /// `text_rev` the preview was parsed at.
    preview_text_rev: u64,
    /// Bumped by every message that may change the buffer text.
    text_rev: u64,
    /// Folded node paths of the JSON tree.
    json_collapsed: std::sync::Arc<HashSet<String>>,
    /// Bumped whenever what the preview shows changes, so the cached widget
    /// tree (iced `lazy`) is rebuilt only then, not on every frame.
    preview_rev: u64,
    /// Closers typed by the auto-pairing and not stepped over yet:
    /// (line, byte column, character).
    pending_closers: Vec<(usize, usize, char)>,
    /// Ctrl/Cmd currently held, tracked for Ctrl+click on a chevron.
    mod_ctrl: bool,
    /// Alt currently held, tracked for Alt+click on a chevron.
    mod_alt: bool,
}

/// Cap on the memory the undo history of one buffer may hold.
const UNDO_MAX_BYTES: usize = 64 * 1024 * 1024;
/// Cap on the undo history of all tabs together.
const UNDO_TOTAL_BYTES: usize = 256 * 1024 * 1024;
const UNDO_MAX_STEPS: usize = 100;
/// Files larger than this are refused instead of freezing the editor.
const MAX_OPEN_BYTES: u64 = 64 * 1024 * 1024;
/// The same limit for pasted text.
/// A line longer than this makes layout and typing slow.
const LONG_LINE_BYTES: usize = 20_000;
const MAX_PASTE_BYTES: usize = 64 * 1024 * 1024;
/// Marker error for a dialog the user dismissed: not worth a status message.
const CANCELLED: &str = "cancelled";

/// What a background format run started from: the format result is only
/// applied if the buffer still reads the same.
#[derive(Debug, Clone)]
pub struct FormatJob {
    path: Option<PathBuf>,
    /// Fingerprint of the text that was sent (the whole buffer, or the
    /// selection).
    source: u64,
}

fn trim_history(stack: &mut History) {
    stack.trim(UNDO_MAX_STEPS, UNDO_MAX_BYTES);
}

fn history_bytes(stacks: &(History, History)) -> usize {
    stacks.0.bytes() + stacks.1.bytes()
}

/// The cached parse backing the preview pane.
enum PreviewDoc {
    None,
    Markdown(std::sync::Arc<Vec<crate::preview::markdown::Block>>),
    Json(std::sync::Arc<serde_json::Value>),
    JsonError(String),
}

impl SimpleEditApp {
    /// An app with one empty untitled tab, reading nothing from disk.
    fn blank(config: Config) -> Self {
        Self {
            bookmarks: crate::bookmarks::Bookmarks::default(),
            editor: EditorState::new(),
            sidebar: SidebarState::new(),
            search: SearchState::new(),
            preferences: PreferencesState::from_config(&config),
            show_search: false,
            show_sidebar: config.show_sidebar,
            show_preferences: false,
            capturing: None,
            palette_query: String::new(),
            palette_selected: 0,
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
            undo_stack: History::default(),
            redo_stack: History::default(),
            undo_cache: HashMap::new(),
            config_dirty: false,
            last_session: 0,
            session_lock: None,
            session_owner: true,
            rev: 1,
            session_rev: 0,
            format: TextFormat::default(),
            disk_stamp: None,
            saved_fp: None,
            pending_open: None,
            external_warned: false,
            file_size: None,
            size_for: None,
            undo_new_group: true,
            view_mode: ViewMode::Raw,
            preview: PreviewDoc::None,
            selection_info: None,
            selection_key: None,
            preview_text_rev: 0,
            text_rev: 1,
            json_collapsed: Default::default(),
            preview_rev: 0,
            pending_closers: Vec::new(),
            mod_ctrl: false,
            mod_alt: false,
            config,
        }
    }
}

impl SimpleEditApp {
    pub fn new(flags: Flags) -> (Self, Task<Message>) {
        // The first window owns the session; others start empty and leave it
        // alone, so closing them can't overwrite it.
        let lock = crate::session::InstanceLock::acquire();
        let owner = lock.is_some();
        let loaded = if owner {
            crate::session::Session::load()
        } else {
            crate::session::Loaded {
                session: crate::session::Session::default(),
                set_aside: None,
            }
        };
        let mut app = Self::blank(flags.config);
        app.session_lock = lock;
        app.session_owner = owner;
        if owner {
            app.bookmarks = crate::bookmarks::Bookmarks::load();
        } else {
            app.set_status(t!("status.second_window"), false);
        }

        app.bookmarks.prune_missing();
        let mut commands = vec![app.restore_session(loaded.session)];
        if let Some(aside) = loaded.set_aside {
            let name = aside
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            app.set_status(t!("status.session_set_aside", name = name.as_str()), true);
        }
        app.announce_new_version();

        // Files given on the command line open on top of the session.
        for path in flags.files {
            commands.push(Task::perform(read_file(path, true), Message::FileOpened));
        }
        (app, Task::batch(commands))
    }

    pub fn title(&self) -> String {
        let name = self
            .current_file
            .as_deref()
            .map(file_label)
            .unwrap_or_else(|| t!("app.untitled").to_string());
        let dirty = if self.is_dirty { " •" } else { "" };
        format!("SimpleEdit — {}{}", name, dirty)
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        // What the autosave and the preview have to look at again.
        let passive = matches!(
            &message,
            Message::AutoSave
                | Message::ModifiersChanged { .. }
                | Message::MenuBounds(..)
                | Message::ToggleMenu(_)
                | Message::HoverMenu(_)
                | Message::SessionWritten(_)
                | Message::PreviewTick
                | Message::WindowResized(..)
        );
        let edits_text = !passive && !matches!(&message, Message::EditorAction(a) if !a.is_edit());
        if edits_text {
            self.text_rev += 1;
        }
        if !passive {
            self.rev += 1;
        }
        // While typing in a preview, the parse waits for the next tick.
        let typing = matches!(&message, Message::EditorAction(a) if a.is_edit());
        let command = self.handle(message);
        if !typing {
            self.refresh_preview();
        }
        self.refresh_file_size();
        if !passive {
            self.refresh_selection_info();
        }
        command
    }

    pub fn subscription(&self) -> iced::Subscription<Message> {
        // `listen_with` also sees events that a widget captured: the search
        // field swallows every key, the editor all arrows and Enter, and the
        // shortcuts must still work then.
        let keys = iced::event::listen_with(|event, status, _window| {
            use iced::event::Status;
            use iced::keyboard::{key::Named, Event, Key};

            let iced::Event::Keyboard(Event::KeyPressed { key, modifiers, .. }) = event else {
                return None;
            };
            if key == Key::Named(Named::Escape) {
                return Some(Message::EscapePressed);
            }

            let key_str = match key.as_ref() {
                Key::Character(c) => c.to_lowercase(),
                Key::Named(Named::Enter) => "Return".to_string(),
                Key::Named(Named::Tab) => "Tab".to_string(),
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
            // Plain typing in a widget is none of the app's business (and
            // would wake it up on every keystroke); modified keys and keys a
            // widget left alone are.
            let ignored = status == Status::Ignored;
            let is_enter = key_str == "Return";
            if !(ignored || modifiers.command() || modifiers.alt() || is_enter) {
                return None;
            }

            Some(Message::KeyPressed {
                key: key_str,
                ctrl: modifiers.command(),
                shift: modifiers.shift(),
                alt: modifiers.alt(),
            })
        });

        // Chevron clicks need the modifier state at click time, which a button
        // press message does not carry.
        let modifiers = iced::event::listen_with(|event, _status, _window| match event {
            iced::Event::Keyboard(iced::keyboard::Event::ModifiersChanged(m)) => {
                Some(Message::ModifiersChanged {
                    ctrl: m.command(),
                    alt: m.alt(),
                })
            }
            // The window is closed through the app so the session is saved.
            iced::Event::Window(iced::window::Event::CloseRequested) => Some(Message::Quit),
            iced::Event::Window(iced::window::Event::Resized(size)) => {
                Some(Message::WindowResized(size.width, size.height))
            }
            _ => None,
        });

        let autosave =
            iced::time::every(std::time::Duration::from_secs(2)).map(|_| Message::AutoSave);

        let mut subscriptions = vec![keys, modifiers, autosave];
        if self.view_mode != ViewMode::Raw && self.preview_kind().is_some() {
            subscriptions.push(
                iced::time::every(std::time::Duration::from_millis(150))
                    .map(|_| Message::PreviewTick),
            );
        }
        iced::Subscription::batch(subscriptions)
    }

    pub fn view(&self) -> Element<'_, Message> {
        let dark = self.config.dark_mode;

        let title_bar = crate::menu_bar::view(&self.config, self.open_menu);

        let preview_kind = self.preview_kind();
        let view_mode = if preview_kind.is_some() {
            self.view_mode
        } else {
            ViewMode::Raw
        };
        let editor_with_context = self.editor_pane(preview_kind, view_mode, dark);

        // The search card floats over the top-right corner of the editor.
        let body: Element<'_, Message> = if self.show_search {
            crate::floating::top_right(
                editor_with_context,
                self.search.view(dark),
                12.0,
                // Clear of the minimap.
                16.0 + if self.config.show_minimap && view_mode != ViewMode::Preview {
                    crate::code_editor::MINIMAP_WIDTH
                } else {
                    0.0
                },
            )
        } else {
            editor_with_context
        };

        let (line, byte_col) = self.editor.content.cursor_position();
        // Columns count characters, not the bytes the editor indexes by.
        let cursor = (
            line,
            self.editor
                .content
                .line(line)
                .map_or(0, |l| l[..floor_boundary(&l, byte_col)].chars().count()),
        );
        let selection_info = self.selection_info.clone();
        let status_bar = crate::editor::statusbar::view(crate::editor::statusbar::Info {
            status: &self.status_message,
            is_error: self.status_is_error,
            dark,
            cursor,
            line_ending: self.format.ending.label(),
            language: self.editor.language.as_deref(),
            selection: selection_info,
            file_size: self.file_size,
            use_spaces: self.config.use_spaces,
            tab_width: self.config.tab_width,
        });

        let error_panel: Option<Element<Message>> = if self.show_error_panel {
            self.format_error.as_ref().map(|err| {
                let header = row![
                    text(t!("panel.errors").to_string())
                        .size(10.5)
                        .font(se_theme::UI_MEDIUM)
                        .color(se_theme::muted_text(dark)),
                    Space::new().width(Length::Fill),
                    button(text("×").size(16).color(se_theme::muted_text(dark)))
                        .padding([2, 6])
                        .on_press(Message::CloseErrorPanel)
                        .style(
                            (se_theme::GhostButton {
                                dark,
                                active: false,
                            })
                            .style()
                        ),
                ]
                .padding([4, 14])
                .align_y(Alignment::Center);

                let body = scrollable(
                    container(
                        text(err.clone())
                            .size(12)
                            .color(se_theme::error_color(dark))
                            .font(se_theme::MONO),
                    )
                    .padding(iced::Padding {
                        top: 4.0,
                        right: 14.0,
                        bottom: 8.0,
                        left: 14.0,
                    })
                    .width(Length::Fill),
                )
                .style(se_theme::Scrollbar { dark }.style())
                .height(Length::Fixed(100.0));

                container(column![rule(dark), header, body])
                    .width(Length::Fill)
                    .style(se_theme::error_panel(dark))
                    .into()
            })
        } else {
            None
        };

        // The editor panel: toolbar, (formatting banner), body, (errors), status.
        let mut panel = column![self.toolbar_view(preview_kind, view_mode, dark), rule(dark)];
        if self.is_formatting {
            panel = panel.push(
                container(
                    text(t!("status.formatting").to_string())
                        .size(12)
                        .color(se_theme::accent_color(dark)),
                )
                .width(Length::Fill)
                .padding([4, 14])
                .style(se_theme::accent_banner(dark)),
            );
        }
        panel = panel.push(body);
        if let Some(errors) = error_panel {
            panel = panel.push(errors);
        }
        let editor_panel = container(panel.push(status_bar))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(se_theme::editor_panel(dark));

        // The floating panels, separated by a gap, on the window background.
        let mut main_row = row![]
            .spacing(se_theme::PANEL_GAP)
            .padding(iced::Padding {
                top: 0.0,
                right: se_theme::PANEL_GAP,
                bottom: se_theme::PANEL_GAP,
                left: se_theme::PANEL_GAP,
            })
            .height(Length::Fill);
        if self.show_sidebar {
            main_row = main_row.push(self.sidebar.view(
                dark,
                &self.current_file,
                &self.dirty_files(),
            ));
        }
        main_row = main_row.push(editor_panel);

        let base: Element<Message> = container(column![title_bar, main_row])
            .width(Length::Fill)
            .height(Length::Fill)
            .style(se_theme::window(dark))
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
            let dropdown = crate::menu_bar::dropdown_view(
                menu,
                &self.config,
                has_fmt,
                lang_override_enabled,
                self.bookmark_state(),
            );
            crate::floating::at_point(base, dropdown, bounds.x, bounds.y + bounds.height)
        } else {
            base
        };

        // The Preferences window floats over everything but the dialogs.
        let with_prefs: Element<Message> = if self.show_preferences {
            let window = container(self.preferences.view(&self.config, self.capturing))
                .width(Length::Fill)
                .height(Length::Fill)
                .max_width(900)
                .max_height(640);
            crate::floating::modal(
                with_dropdown,
                container(window).padding(40),
                Some(Message::TogglePreferences),
                dark,
                false,
            )
        } else {
            with_dropdown
        };

        // Modal overlays
        match &self.overlay {
            ActiveOverlay::None => with_prefs,
            ActiveOverlay::Palette => {
                let shown = self.palette_entries();
                let card =
                    crate::palette::view(dark, &self.palette_query, &shown, self.palette_selected);
                crate::floating::modal(
                    with_prefs,
                    column![Space::new().height(84), card].align_x(Alignment::Center),
                    Some(Message::CloseOverlay),
                    dark,
                    true,
                )
            }
            other => {
                let overlay = match other {
                    ActiveOverlay::About => self.view_about_overlay(dark),
                    ActiveOverlay::Changelog => {
                        self.changelog_card(dark, t!("changelog.title").to_string(), None)
                    }
                    ActiveOverlay::WhatsNew(previous) => self.changelog_card(
                        dark,
                        t!("changelog.whats_new", version = env!("CARGO_PKG_VERSION")).to_string(),
                        Some(previous.as_str()),
                    ),
                    ActiveOverlay::GotoLine => self.view_goto_line_overlay(dark),
                    ActiveOverlay::LanguagePicker => self.view_language_picker_overlay(dark),
                    ActiveOverlay::Shortcuts => self.view_shortcuts_overlay(dark),
                    ActiveOverlay::ConfirmOverwrite(path) => {
                        self.view_overwrite_overlay(dark, path)
                    }
                    ActiveOverlay::ConfirmClose(path) => self.view_confirm_overlay(dark, path),
                    ActiveOverlay::ConfirmQuit(error) => confirm_card(
                        dark,
                        t!("quit.title").to_string(),
                        t!("quit.message", error = error.as_str()).to_string(),
                        t!("quit.confirm").to_string(),
                        Message::QuitAnyway,
                    ),
                    _ => self.view_update_overlay(dark),
                };
                // A running download or install can't be dismissed: its
                // outcome would arrive without context. Escape reaches the
                // app through its own subscription, for every overlay.
                let on_blur = self
                    .overlay_is_dismissable()
                    .then_some(Message::CloseOverlay);
                crate::floating::modal(with_prefs, overlay, on_blur, dark, false)
            }
        }
    }

    pub fn theme(&self) -> Theme {
        if self.config.dark_mode {
            se_theme::ink_dark()
        } else {
            se_theme::ink_light()
        }
    }
}

// ─── Message handling ───────────────────────────────────────────────────────

impl Message {
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
                | Message::InsertTab
                | Message::DedentLine
        )
    }
}

impl SimpleEditApp {
    fn handle(&mut self, message: Message) -> Task<Message> {
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
                | Message::SessionWritten(_)
                | Message::PreviewTick
                | Message::WindowResized(..)
        );
        if closes_menu {
            // Escape on an open menu closes only that menu.
            if matches!(message, Message::EscapePressed) && self.open_menu.take().is_some() {
                return Task::none();
            }
            self.open_menu = None;
        }

        match message {
            Message::EditorAction(action) => {
                // Without a tab there is nothing to edit (the next one is
                // still being read).
                if self.current_file.is_none() && action.is_edit() {
                    return Task::none();
                }
                // Lines added or removed by an edit move the bookmarks below.
                let probe = action.is_edit().then(|| self.edit_probe());
                let command = self.editor_action(action);
                if let Some((count, top)) = probe {
                    self.shift_bookmarks(count, top);
                }
                command
            }
            Message::InsertTab => {
                if !self.editing_blocked() {
                    if self.selection_span().is_some() {
                        return self.handle(Message::IndentSelection);
                    }
                    // With a single-line selection this replaces it, as
                    // in most editors.
                    let text = self.tab_text();
                    self.push_undo();
                    self.undo_new_group = true;
                    self.edited(|app| {
                        app.editor.content.perform(text_editor::Action::Edit(
                            text_editor::Edit::Paste(std::sync::Arc::new(text)),
                        ));
                    });
                    self.is_dirty = true;
                }
                Task::none()
            }
            Message::SelectAll => {
                self.editor.content.perform(text_editor::Action::Move(
                    text_editor::Motion::DocumentStart,
                ));
                self.editor.content.perform(text_editor::Action::Select(
                    text_editor::Motion::DocumentEnd,
                ));
                Task::none()
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
                    Task::none()
                }
            }
            Message::MenuBounds(menu, bounds) => {
                if let Some(bounds) = bounds {
                    self.menu_bounds.insert(menu, bounds);
                }
                Task::none()
            }
            Message::NewFile => {
                self.stash_current();
                self.untitled_counter += 1;
                let path = untitled_path(self.untitled_counter);
                self.load_buffer(path, CachedBuffer::blank());
                self.set_status(t!("status.new_file"), false);
                Task::none()
            }
            Message::OpenFile => Task::perform(open_file(), Message::FileOpened),
            Message::FileOpened(result) => {
                match result {
                    Ok(loaded) => self.open_loaded(loaded),
                    Err(e) if e == CANCELLED => {}
                    Err(e) => self.set_status(format!("{}: {}", t!("status.error"), e), true),
                }
                Task::none()
            }
            Message::TabRead(result) => {
                match result {
                    // A slower read of a tab the user has left behind.
                    // (Its cached state stays for the next visit.)
                    Ok(loaded)
                        if self
                            .pending_open
                            .as_ref()
                            .is_some_and(|p| *p != loaded.path) => {}
                    Ok(loaded) => self.open_loaded(loaded),
                    Err(e) => {
                        self.set_status(format!("{}: {}", t!("status.error"), e), true);
                        let failed = self.pending_open.take();
                        // Without a tab to show, drop the one that can't be read.
                        if self.current_file.is_none() {
                            if let Some(path) = failed {
                                self.sidebar.remove_path(&path);
                                self.file_cache.remove(&path);
                            }
                            return self.ensure_tab();
                        }
                    }
                }
                Task::none()
            }
            Message::FileReloaded(result) => {
                match result {
                    Ok(loaded) => self.apply_reload(loaded),
                    Err(e) => self.set_status(format!("{}: {}", t!("status.error"), e), true),
                }
                Task::none()
            }
            Message::SaveFile => {
                if let Some(path) = self.current_file.clone() {
                    if self.changed_on_disk(&path) {
                        self.overlay = ActiveOverlay::ConfirmOverwrite(path);
                        return Task::none();
                    }
                }
                let content = crate::editor::buffer_text(&self.editor.content);
                let origin = self.current_file.clone();
                match origin.clone() {
                    Some(path) if !is_untitled(&path) => {
                        save_command(origin, save_file(path, content, self.format))
                    }
                    Some(_) => self.start_save_as(origin),
                    None => Task::none(),
                }
            }
            Message::SaveFileAs => self.start_save_as(self.current_file.clone()),
            Message::SaveAsChosen(origin, result) => match result {
                Ok(path) => {
                    // Another tab holds unsaved work for that file: writing
                    // over the file would silently drop it.
                    let elsewhere = origin.as_ref() != Some(&path)
                        && self.sidebar.files().contains(&path)
                        && self.is_path_dirty(&path);
                    if elsewhere {
                        self.set_status(
                            t!("status.save_as_open_tab", name = file_label(&path).as_str()),
                            true,
                        );
                        return Task::none();
                    }
                    // Whatever the tab holds now is what gets written.
                    let content = if origin == self.current_file {
                        crate::editor::buffer_text(&self.editor.content)
                    } else {
                        origin
                            .as_ref()
                            .and_then(|o| self.file_cache.get(o))
                            .and_then(CachedBuffer::text)
                            .unwrap_or_default()
                    };
                    let format = if origin == self.current_file {
                        self.format
                    } else {
                        origin
                            .as_ref()
                            .and_then(|o| self.file_cache.get(o))
                            .map(|c| c.format)
                            .unwrap_or_default()
                    };
                    save_command(origin, save_file(path, content, format))
                }
                Err(e) if e == CANCELLED => Task::none(),
                Err(e) => {
                    self.set_status(format!("{}: {}", t!("status.error"), e), true);
                    Task::none()
                }
            },
            Message::FileSaved(origin, result) => {
                match result {
                    Ok((path, saved)) => {
                        self.size_for = None;
                        // The tab the save started from may no longer be the
                        // active one (the dialog is asynchronous).
                        let from_current = origin == self.current_file;
                        if let Some(old) = origin.as_ref().filter(|o| **o != path) {
                            // "Save as" turns the tab into the new file; the
                            // old file stays as it is on disk.
                            if self.sidebar.files().contains(&path) {
                                self.sidebar.remove_path(&path);
                                self.undo_cache.remove(&path);
                            }
                            self.file_cache.remove(&path);
                            self.sidebar.rename_file(old, path.clone());
                            self.bookmarks.rename(old, &path);
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
                        let saved_fp = fingerprint(&saved);
                        let stamp = DiskStamp::of(&path);
                        if from_current {
                            let first_line = first_line_of(&self.editor.content);
                            self.editor.adopt_path_language(&path, &first_line);
                            // Edits made while the save was in flight stay dirty.
                            let now = crate::editor::buffer_text(&self.editor.content);
                            self.is_dirty = now != saved;
                            self.saved_fp = Some(saved_fp);
                            self.disk_stamp = stamp;
                            self.external_warned = false;
                            self.current_file = Some(path.clone());
                        } else if let Some(entry) = self.file_cache.get_mut(&path) {
                            entry.dirty = entry.text().is_some_and(|t| t != saved);
                            entry.saved_fp = Some(saved_fp);
                            entry.stamp = stamp;
                        }
                        self.config.add_recent(&path);
                        self.config_dirty = true;
                        self.set_status(t!("status.file_saved"), false);
                    }
                    Err(e) if e == CANCELLED => {}
                    Err(e) => self.set_status(format!("{}: {}", t!("status.error"), e), true),
                }
                Task::none()
            }
            Message::CloseFile => match self.current_file.clone() {
                Some(path) => self.request_close(path),
                None => Task::none(),
            },
            Message::ConfirmOverwrite => {
                self.overlay = ActiveOverlay::None;
                // Take the new on-disk state as the baseline, then save.
                self.disk_stamp = self.current_file.as_deref().and_then(DiskStamp::of);
                self.handle(Message::SaveFile)
            }
            Message::ConfirmDiscard(path) => {
                self.overlay = ActiveOverlay::None;
                self.close_path(path)
            }
            Message::ToggleSearch => {
                self.show_search = !self.show_search;
                if self.show_search {
                    // Ready to type, as the card opens.
                    iced::widget::operation::focus(crate::search::find_field_id())
                } else {
                    Task::none()
                }
            }
            Message::ToggleSidebar => {
                self.show_sidebar = !self.show_sidebar;
                self.config.show_sidebar = self.show_sidebar;
                self.config_dirty = true;
                Task::none()
            }
            Message::WindowResized(width, height) => {
                // Remembered, and written with the next autosave tick.
                if (self.config.window_width, self.config.window_height) != (width, height) {
                    self.config.window_width = width;
                    self.config.window_height = height;
                    self.config_dirty = true;
                }
                Task::none()
            }
            Message::TogglePreferences => {
                self.show_preferences = !self.show_preferences;
                Task::none()
            }
            Message::ToggleTheme => {
                let dark = !self.config.dark_mode;
                self.handle(Message::ThemeChanged(dark))
            }
            Message::ThemeChanged(dark) => {
                self.config.dark_mode = dark;
                self.config.save();
                Task::none()
            }
            Message::Sidebar(msg) => match self.sidebar.update(msg) {
                SidebarAction::OpenFile(path) => self.activate(path),
                SidebarAction::CloseFile(path) => self.request_close(path),
                // Only offered for the active file.
                SidebarAction::SaveFile => self.handle(Message::SaveFile),
                SidebarAction::SaveFileAs => self.start_save_as(self.current_file.clone()),
                SidebarAction::None => Task::none(),
            },
            Message::Search(SearchMessage::ReplaceAll) => {
                let old = crate::editor::buffer_text(&self.editor.content);
                match self.search.replace_all(&old) {
                    Ok(new_text) if new_text != old => {
                        self.push_undo();
                        self.undo_new_group = true;
                        self.is_dirty = true;
                        self.search.reset_matches();
                        // Minimal replacement: scroll and cursor survive.
                        self.replace_buffer_text(&new_text);
                    }
                    Ok(_) => self.search.reset_matches(),
                    Err(e) => self.search.set_error(e),
                }
                Task::none()
            }
            Message::Search(msg) => {
                let replacing = matches!(msg, SearchMessage::Replace);
                let before = replacing.then(|| self.snapshot());
                let probe = replacing.then(|| self.edit_probe());
                let command = self.search.update(msg, &mut self.editor.content);
                // Replacements are edits like any other: undoable and dirty.
                if let (Some(before), Some((count, top))) = (before, probe) {
                    if crate::editor::buffer_text(&self.editor.content) != before.text {
                        self.undo_stack.push_back(before);
                        trim_history(&mut self.undo_stack);
                        self.redo_stack.clear();
                        self.undo_new_group = true;
                        self.is_dirty = true;
                        self.shift_bookmarks(count, top);
                    }
                }
                command
            }
            Message::Preferences(msg) => {
                if self.preferences.update(msg, &mut self.config) {
                    self.config_dirty = true;
                }
                Task::none()
            }
            Message::About => {
                self.overlay = ActiveOverlay::About;
                Task::none()
            }
            Message::Quit => match self.save_session_now() {
                Ok(()) => {
                    self.config.save();
                    std::process::exit(0)
                }
                // Unsaved buffers live on in the session file; if it can't be
                // written, quitting would lose them.
                Err(e) => {
                    self.overlay = ActiveOverlay::ConfirmQuit(e);
                    Task::none()
                }
            },
            Message::QuitAnyway => {
                self.config.save();
                std::process::exit(0)
            }
            Message::ContextCopy => {
                if let Some(text) = self.editor.content.selection() {
                    return iced::clipboard::write(text);
                }
                Task::none()
            }
            Message::ContextCut => {
                if self.editing_blocked() {
                    return Task::none();
                }
                if let Some(text) = self.editor.content.selection() {
                    self.push_undo();
                    self.undo_new_group = true;
                    self.edited(|app| {
                        app.editor
                            .content
                            .perform(text_editor::Action::Edit(text_editor::Edit::Delete));
                    });
                    self.is_dirty = true;
                    return iced::clipboard::write(text);
                }
                Task::none()
            }
            Message::ContextPaste => {
                if self.editing_blocked() {
                    return Task::none();
                }
                iced::clipboard::read().map(Message::ClipboardContent)
            }
            Message::ClipboardContent(Some(text)) => {
                if self.editing_blocked() {
                    return Task::none();
                }
                if text.len() > MAX_PASTE_BYTES {
                    self.set_status(
                        format!(
                            "{}: {}",
                            t!("status.error"),
                            t!("status.err_paste_too_large")
                        ),
                        true,
                    );
                    return Task::none();
                }
                self.push_undo();
                self.undo_new_group = true;
                self.edited(|app| {
                    app.editor.content.perform(text_editor::Action::Edit(
                        text_editor::Edit::Paste(std::sync::Arc::new(text)),
                    ));
                });
                self.is_dirty = true;
                Task::none()
            }
            Message::ClipboardContent(None) => Task::none(),
            Message::ContextDelete => {
                // Nothing selected, nothing to delete (and no empty undo step).
                if self.editing_blocked() || self.editor.content.selection().is_none() {
                    return Task::none();
                }
                self.push_undo();
                self.undo_new_group = true;
                self.edited(|app| {
                    app.editor
                        .content
                        .perform(text_editor::Action::Edit(text_editor::Edit::Delete));
                });
                self.is_dirty = true;
                Task::none()
            }
            Message::FormatFile => {
                if self.is_formatting {
                    return Task::none();
                }
                if let Some(ext) = self.editor.language.clone() {
                    self.is_formatting = true;
                    self.set_status(t!("status.formatting"), false);
                    let content = crate::editor::buffer_text(&self.editor.content);
                    let job = FormatJob {
                        path: self.current_file.clone(),
                        source: fingerprint(&content),
                    };
                    let target = self.format_target();
                    Task::perform(
                        crate::formatter::format(content, ext, target),
                        move |result| Message::FileFormatted(job.clone(), result),
                    )
                } else {
                    self.set_status(t!("status.no_language"), false);
                    Task::none()
                }
            }
            Message::FormatSelection => {
                if self.is_formatting {
                    return Task::none();
                }
                if let (Some(ext), Some(selected)) = (
                    self.editor.language.clone(),
                    self.editor.content.selection(),
                ) {
                    self.is_formatting = true;
                    self.set_status(t!("status.formatting"), false);
                    let job = FormatJob {
                        path: self.current_file.clone(),
                        source: fingerprint(&selected),
                    };
                    let target = self.format_target();
                    // Formatters end their output with a newline; a fragment
                    // that didn't end with one shouldn't gain one.
                    let had_newline = selected.ends_with('\n');
                    Task::perform(
                        async move {
                            crate::formatter::format(selected, ext, target)
                                .await
                                .map(|f| {
                                    if had_newline {
                                        f
                                    } else {
                                        f.trim_end_matches('\n').to_string()
                                    }
                                })
                        },
                        move |result| Message::SelectionFormatted(job.clone(), result),
                    )
                } else {
                    Task::none()
                }
            }
            Message::FileFormatted(job, result) => {
                self.is_formatting = false;
                // The user moved on, or kept typing: the result describes text
                // that is no longer there.
                let current = self.current_file == job.path
                    && fingerprint(&crate::editor::buffer_text(&self.editor.content)) == job.source;
                if !current {
                    self.set_status(t!("status.format_stale"), true);
                    return Task::none();
                }
                match result {
                    Ok(formatted) => {
                        let (line, col) = self.editor.content.cursor_position();
                        self.format_error = None;
                        self.show_error_panel = false;
                        self.set_status(t!("status.formatted"), false);
                        let formatted = TextFormat::decode(&formatted).0;
                        return self.apply_text_edit(formatted, line, col, None);
                    }
                    Err(e) => self.show_format_error(e),
                }
                Task::none()
            }
            Message::SelectionFormatted(job, result) => {
                self.is_formatting = false;
                let current = self.current_file == job.path
                    && self
                        .editor
                        .content
                        .selection()
                        .is_some_and(|sel| fingerprint(&sel) == job.source);
                if !current {
                    self.set_status(t!("status.format_stale"), true);
                    return Task::none();
                }
                match result {
                    Ok(formatted) => {
                        self.push_undo();
                        self.undo_new_group = true;
                        self.edited(|app| {
                            app.editor.content.perform(text_editor::Action::Edit(
                                text_editor::Edit::Paste(std::sync::Arc::new(formatted)),
                            ));
                        });
                        self.is_dirty = true;
                        self.format_error = None;
                        self.show_error_panel = false;
                        self.set_status(t!("status.formatted"), false);
                    }
                    Err(e) => self.show_format_error(e),
                }
                Task::none()
            }
            Message::CloseErrorPanel => {
                self.show_error_panel = false;
                Task::none()
            }
            Message::Undo => {
                if self.editing_blocked() {
                    return Task::none();
                }
                if let Some(prev) = self.undo_stack.pop_back() {
                    let current = self.snapshot();
                    self.redo_stack.push_back(current);
                    trim_history(&mut self.redo_stack);
                    return self.restore_snapshot(prev);
                }
                Task::none()
            }
            Message::Redo => {
                if self.editing_blocked() {
                    return Task::none();
                }
                if let Some(next) = self.redo_stack.pop_back() {
                    let current = self.snapshot();
                    self.undo_stack.push_back(current);
                    trim_history(&mut self.undo_stack);
                    return self.restore_snapshot(next);
                }
                Task::none()
            }
            Message::AutoSave => {
                if self.config_dirty {
                    self.config_dirty = false;
                    self.config.save();
                }
                let external = self.check_external_change();
                if self.rev == self.session_rev {
                    return external;
                }
                match self.session_write_job() {
                    Some(job) => {
                        Task::batch([external, Task::perform(job, Message::SessionWritten)])
                    }
                    None => external,
                }
            }
            Message::SessionWritten(result) => {
                match result {
                    Ok(()) => {}
                    Err(e) => {
                        // Try again at the next tick.
                        self.last_session = 0;
                        self.session_rev = 0;
                        self.set_status(format!("{}: {}", t!("status.session_not_saved"), e), true);
                    }
                }
                Task::none()
            }
            Message::PreviewTick => {
                self.refresh_preview();
                Task::none()
            }
            // --- Overlays ---
            Message::CloseOverlay => {
                if self.overlay_is_dismissable() {
                    self.capturing = None;
                    self.overlay = ActiveOverlay::None;
                }
                Task::none()
            }
            Message::OpenGotoLine => {
                self.goto_line_input = String::new();
                self.overlay = ActiveOverlay::GotoLine;
                iced::widget::operation::focus(goto_line_id())
            }
            Message::GotoLineInputChanged(s) => {
                // Allow only digits
                self.goto_line_input = s.chars().filter(|c| c.is_ascii_digit()).collect();
                Task::none()
            }
            Message::GotoLineSubmit => {
                if let Ok(n) = self.goto_line_input.parse::<usize>() {
                    let max = self.editor.content.line_count().saturating_sub(1);
                    let target = n.saturating_sub(1).min(max);
                    crate::search::goto_line_start(&mut self.editor.content, target);
                }
                self.overlay = ActiveOverlay::None;
                Task::none()
            }
            Message::OpenLanguagePicker => {
                self.overlay = ActiveOverlay::LanguagePicker;
                Task::none()
            }
            Message::SetLanguage(ext) => {
                self.editor.language = if ext.is_empty() { None } else { Some(ext) };
                self.reset_view_mode();
                self.overlay = ActiveOverlay::None;
                Task::none()
            }
            Message::OpenShortcuts => {
                self.capturing = None;
                self.overlay = ActiveOverlay::Shortcuts;
                Task::none()
            }
            Message::StartCaptureShortcut(target) => {
                self.capturing = Some(target);
                Task::none()
            }
            Message::OpenPalette => {
                self.palette_query.clear();
                self.palette_selected = 0;
                self.overlay = ActiveOverlay::Palette;
                iced::widget::operation::focus(crate::palette::input_id())
            }
            Message::PaletteQueryChanged(query) => {
                self.palette_query = query;
                self.palette_selected = 0;
                crate::palette::scroll_to_selected(0)
            }
            Message::PaletteSubmit => {
                let shown = self.palette_entries();
                match shown.get(self.palette_selected) {
                    Some(entry) if entry.enabled => {
                        let command = entry.command;
                        self.handle(Message::PaletteRun(command))
                    }
                    _ => Task::none(),
                }
            }
            Message::PaletteRun(command) => {
                self.overlay = ActiveOverlay::None;
                self.handle(command.message())
            }
            // Raw key forwarded from the subscription fn pointer
            Message::KeyPressed {
                key,
                ctrl,
                shift,
                alt,
            } => {
                // In shortcut capture mode: save the new binding
                if let Some(target) = self.capturing {
                    let sc = ShortcutConfig {
                        ctrl,
                        shift,
                        alt,
                        key,
                    };
                    // A bare letter (or Shift+letter) would fire on every
                    // keystroke typed into the editor.
                    if !(sc.ctrl || sc.alt || crate::config::is_function_key(&sc.key)) {
                        let ctrl = if cfg!(target_os = "macos") {
                            "⌘"
                        } else {
                            "Ctrl"
                        };
                        self.set_status(t!("shortcuts.needs_modifier", ctrl = ctrl), true);
                        return Task::none();
                    }
                    if crate::code_editor::consumes_key(&sc.key, sc.ctrl, sc.alt) {
                        self.set_status(t!("shortcuts.reserved"), true);
                        return Task::none();
                    }
                    if self.config.shortcuts.conflicts(target, &sc) {
                        self.set_status(t!("shortcuts.conflict"), true);
                        return Task::none();
                    }
                    *self.config.shortcuts.slot_mut(target) = sc;
                    self.config.save();
                    self.capturing = None;
                    return Task::none();
                }
                // The palette: arrows move the selection (Enter is the
                // field's own submit, Escape closes).
                if self.overlay == ActiveOverlay::Palette {
                    let delta = match key.as_str() {
                        "ArrowDown" => 1,
                        "ArrowUp" => -1,
                        _ => 0,
                    };
                    if delta != 0 {
                        let len = self.palette_entries().len();
                        self.palette_selected =
                            crate::palette::step(self.palette_selected, delta, len);
                        return crate::palette::scroll_to_selected(self.palette_selected);
                    }
                    return Task::none();
                }
                // Don't fire shortcuts while any overlay or the Preferences
                // window is open
                if !matches!(self.overlay, ActiveOverlay::None) || self.show_preferences {
                    return Task::none();
                }
                // Enter in the search card goes to the next match, Shift+Enter
                // to the previous one — when the find field has the focus
                // (it captures the key itself, which is why this listens to
                // captured events too).
                if key == "Return" && !ctrl && !alt {
                    if self.show_search {
                        return iced::advanced::widget::operate(crate::search::find_field_focused()).map(
                            move |in_find_field| Message::SearchEnterChecked {
                                shift,
                                in_find_field,
                            },
                        );
                    }
                    return Task::none();
                }
                // Tab indents, Shift+Tab dedents; a search field keeps it.
                if key == "Tab" {
                    if ctrl || alt {
                        return Task::none();
                    }
                    let m = if shift {
                        Message::DedentLine
                    } else {
                        Message::InsertTab
                    };
                    return self.check_search_focus_then(m);
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
                let bindings: [(&ShortcutConfig, Message); 21] = [
                    (&sh.undo, Message::Undo),
                    (&sh.redo, Message::Redo),
                    (&sh.duplicate_line, Message::DuplicateLine),
                    (&sh.move_line_up, Message::MoveLineUp),
                    (&sh.move_line_down, Message::MoveLineDown),
                    (&sh.toggle_comment, Message::ToggleComment),
                    (&sh.delete_line, Message::DeleteLine),
                    (&sh.toggle_bookmark, Message::ToggleBookmark),
                    (&sh.prev_bookmark, Message::PrevBookmark),
                    (&sh.next_bookmark, Message::NextBookmark),
                    (&sh.save_as, Message::SaveFileAs),
                    (&sh.new_file, Message::NewFile),
                    (&sh.save_file, Message::SaveFile),
                    (&sh.open_file, Message::OpenFile),
                    (&sh.close_file, Message::CloseFile),
                    (&sh.find, Message::ToggleSearch),
                    (&sh.select_all, Message::SelectAll),
                    (&sh.format_code, Message::FormatFile),
                    (&sh.goto_line, Message::OpenGotoLine),
                    (&sh.command_palette, Message::OpenPalette),
                    (&sh.toggle_sidebar, Message::ToggleSidebar),
                ];
                let msg = bindings
                    .into_iter()
                    .find(|(s, _)| sc_check(s))
                    .map(|(_, m)| m)
                    .or_else(|| sc_check(&sh.quit).then_some(Message::Quit));
                match msg {
                    Some(m) => self.check_search_focus_then(m),
                    None => Task::none(),
                }
            }
            Message::DuplicateLine => {
                if self.editing_blocked() {
                    return Task::none();
                }
                let text = crate::editor::buffer_text(&self.editor.content);
                let span = self.selection_span();
                let (first, last) = self.lines_to_edit(span);
                let new_text = text_ops::duplicate_lines(&text, first, last);
                // The copy goes below, and the cursor follows it.
                let shift = (last - first + 1) as isize;
                self.apply_line_edit(&text, new_text, shift, span)
            }
            Message::MoveLineUp => self.move_lines(true),
            Message::MoveLineDown => self.move_lines(false),
            Message::ToggleComment => {
                if self.editing_blocked() {
                    return Task::none();
                }
                // Plain text and formats without comments (JSON) have nothing
                // to toggle.
                let Some((prefix, suffix)) = comment_style(self.editor.language.as_deref()) else {
                    return Task::none();
                };
                let text = crate::editor::buffer_text(&self.editor.content);
                let span = self.selection_span();
                let (first, last) = self.lines_to_edit(span);
                let new_text = text_ops::toggle_comment_lines(&text, first, last, prefix, suffix);
                self.apply_line_edit(&text, new_text, 0, span)
            }
            Message::DeleteLine => {
                if self.editing_blocked() {
                    return Task::none();
                }
                let text = crate::editor::buffer_text(&self.editor.content);
                let (first, last) = self.lines_to_edit(self.selection_span());
                let (new_text, new_line) = text_ops::delete_lines(&text, first, last);
                self.apply_text_edit(new_text, new_line, 0, None)
            }
            Message::ToggleBookmark => {
                let Some(path) = self.current_file.clone() else {
                    return Task::none();
                };
                let (line, _) = self.editor.content.cursor_position();
                let added = self.bookmarks.toggle(&path, line);
                self.status_message = if added {
                    t!("status.bookmark_added")
                } else {
                    t!("status.bookmark_removed")
                }
                .to_string();
                self.status_is_error = false;
                Task::none()
            }
            Message::EditorRightClick(at) => {
                // The context menu acts on the clicked line: move the caret
                // there, unless there is a selection to act on.
                if self.editor.content.selection().is_none() {
                    let origin =
                        crate::editor::text_origin(&self.config, self.editor.content.line_count());
                    self.editor
                        .content
                        .perform(text_editor::Action::Click(at - origin));
                    self.undo_new_group = true;
                }
                Task::none()
            }
            Message::NextBookmark => self.jump_to_bookmark(true),
            Message::PrevBookmark => self.jump_to_bookmark(false),
            Message::ClearBookmarks => {
                if let Some(path) = self.current_file.clone() {
                    self.bookmarks.clear(&path);
                    self.status_message = t!("status.bookmarks_cleared").to_string();
                    self.status_is_error = false;
                }
                Task::none()
            }
            Message::IndentSelection => {
                if self.editing_blocked() {
                    return Task::none();
                }
                let Some(span) = self.selection_span() else {
                    return Task::none();
                };
                let (first, last) = span.lines();
                let text = crate::editor::buffer_text(&self.editor.content);
                let new_text = text_ops::indent_lines(&text, first, last, &self.indent_unit());
                self.apply_line_edit(&text, new_text, 0, Some(span))
            }
            Message::DedentLine => {
                if self.editing_blocked() {
                    return Task::none();
                }
                let text = crate::editor::buffer_text(&self.editor.content);
                let span = self.selection_span();
                let (first, last) = self.lines_to_edit(span);
                let new_text = text_ops::dedent_lines(
                    &text,
                    first,
                    last,
                    &self.indent_unit(),
                    self.config.tab_width,
                );
                self.apply_line_edit(&text, new_text, 0, span)
            }
            Message::OpenRecentFile(path) => {
                if !path.exists() {
                    self.config
                        .recent_files
                        .retain(|f| *f != path.to_string_lossy().as_ref());
                    self.config.save();
                    self.set_status(
                        format!("{}: {}", t!("status.error"), t!("status.no_file")),
                        true,
                    );
                    return Task::none();
                }
                // The tab appears once the file has been read.
                if self.sidebar.files().contains(&path) {
                    self.activate(path)
                } else {
                    Task::perform(read_file(path, false), Message::FileOpened)
                }
            }
            Message::ShortcutChecked(inner, focused) => {
                if focused {
                    Task::none()
                } else {
                    self.handle(*inner)
                }
            }
            Message::SearchEnter { shift } => self.handle(Message::Search(if shift {
                SearchMessage::FindPrevious
            } else {
                SearchMessage::Find
            })),
            Message::SearchEnterChecked {
                shift,
                in_find_field,
            } => {
                if in_find_field {
                    self.handle(Message::SearchEnter { shift })
                } else {
                    Task::none()
                }
            }
            Message::EscapePressed => {
                // Innermost first: a pending rebind, a dialog, the
                // Preferences window, the search card.
                if self.capturing.is_some() {
                    self.capturing = None;
                } else if !matches!(self.overlay, ActiveOverlay::None) {
                    if self.overlay_is_dismissable() {
                        self.overlay = ActiveOverlay::None;
                    }
                } else if self.show_preferences {
                    self.show_preferences = false;
                } else if self.show_search {
                    self.show_search = false;
                }
                Task::none()
            }
            Message::CheckForUpdate => {
                self.overlay = ActiveOverlay::UpdateChecking;
                Task::perform(crate::updater::check_for_update(), Message::UpdateChecked)
            }
            Message::OpenChangelog => {
                self.overlay = ActiveOverlay::Changelog;
                Task::none()
            }
            Message::UpdateChecked(Ok(Some((version, notes)))) => {
                self.overlay = ActiveOverlay::UpdateAvailable(version, notes);
                Task::none()
            }
            Message::UpdateChecked(Ok(None)) => {
                self.overlay = ActiveOverlay::UpdateUpToDate;
                Task::none()
            }
            Message::UpdateChecked(Err(e)) => {
                self.overlay = ActiveOverlay::UpdateError(e);
                Task::none()
            }
            Message::ConfirmInstallUpdate(version) => {
                self.overlay = ActiveOverlay::UpdateDownloading;
                Task::perform(
                    crate::updater::download_update(version),
                    Message::UpdateDownloaded,
                )
            }
            Message::UpdateDownloaded(Ok((path, hash))) => {
                self.overlay = ActiveOverlay::UpdateInstalling;
                Task::perform(
                    crate::updater::install_update(path, hash),
                    Message::UpdateInstalled,
                )
            }
            Message::UpdateDownloaded(Err(e)) => {
                self.overlay = ActiveOverlay::UpdateError(e);
                Task::none()
            }
            Message::UpdateInstalled(Ok(())) => {
                if let Err(e) = self.save_session_now() {
                    self.overlay = ActiveOverlay::UpdateError(e);
                    return Task::none();
                }
                self.config.save();
                // On success this never returns — the process re-execs and exits.
                if let Err(e) = crate::updater::restart() {
                    self.overlay = ActiveOverlay::UpdateError(e);
                }
                Task::none()
            }
            Message::UpdateInstalled(Err(e)) => {
                self.overlay = ActiveOverlay::UpdateError(e);
                Task::none()
            }
            Message::SetViewMode(mode) => {
                self.view_mode = mode;
                Task::none()
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
                    Task::none()
                }
            }
            Message::JsonExpandAll => {
                self.json_collapsed = Default::default();
                self.preview_rev += 1;
                Task::none()
            }
            Message::JsonCollapseAll => {
                if let PreviewDoc::Json(value) = &self.preview {
                    self.json_collapsed =
                        std::sync::Arc::new(crate::preview::json::container_paths(value));
                    self.preview_rev += 1;
                }
                Task::none()
            }
            Message::ModifiersChanged { ctrl, alt } => {
                self.mod_ctrl = ctrl;
                self.mod_alt = alt;
                Task::none()
            }
            Message::MarkdownLinkClicked(href) => {
                crate::preview::markdown::find_offset_command(&href).map(Message::PreviewScrollTo)
            }
            Message::PreviewScrollTo(Some(y)) => iced::widget::operation::scroll_to(
                crate::preview::markdown::scrollable_id(),
                iced::widget::scrollable::AbsoluteOffset {
                    x: 0.0,
                    // A little breathing room above the heading, clamped so
                    // targets near the top don't ask for a negative scroll.
                    y: (y - 12.0).max(0.0),
                },
            ),
            Message::PreviewScrollTo(None) => Task::none(),
        }
    }

    /// Asks the renderer where the open menu's button sits, so its dropdown
    /// can be anchored under it regardless of label/font/DPI.
    fn query_menu_bounds(&self) -> Task<Message> {
        match self.open_menu {
            Some(m) => iced::widget::selector::find(iced::widget::selector::id(
                crate::menu_bar::menu_id(m),
            ))
            .map(move |target| Message::MenuBounds(m, target.and_then(|t| t.visible_bounds()))),
            None => Task::none(),
        }
    }

    /// Whether the open overlay can be closed by hand.
    fn overlay_is_dismissable(&self) -> bool {
        !matches!(
            self.overlay,
            ActiveOverlay::UpdateDownloading | ActiveOverlay::UpdateInstalling
        )
    }

    /// Recomputes the selection label of the status bar.
    fn refresh_selection_info(&mut self) {
        let key = (
            self.editor.content.selection_range(),
            self.editor.content.revision(),
        );
        if self.selection_key == Some(key) {
            return;
        }
        self.selection_key = Some(key);
        self.selection_info = self
            .editor
            .content
            .selection_counts()
            .map(|(chars, lines)| {
                if lines > 1 {
                    t!("status.selection_lines", lines = lines, chars = chars).to_string()
                } else {
                    t!("status.selection_chars", chars = chars).to_string()
                }
            });
    }

    /// Shows a message in the status bar (in red for an error).
    fn set_status(&mut self, message: impl ToString, is_error: bool) {
        self.status_message = message.to_string();
        self.status_is_error = is_error;
    }

    /// Whether a panel or dialog owns the keyboard, or there is nothing to
    /// edit, so editing commands (Tab, line operations) must leave the buffer
    /// alone. A hidden editor (Preview mode) counts: the user can't see what
    /// they would change.
    fn editing_blocked(&self) -> bool {
        self.current_file.is_none()
            || self.show_preferences
            || !matches!(self.overlay, ActiveOverlay::None)
            || (self.view_mode == ViewMode::Preview && self.preview_kind().is_some())
    }

    /// Runs `m` unless a search field has the keyboard (it owns Tab, Ctrl+A…).
    fn check_search_focus_then(&mut self, m: Message) -> Task<Message> {
        if self.show_search && m.is_editor_shortcut() {
            iced::advanced::widget::operate(crate::search::search_field_focused())
                .map(move |focused| Message::ShortcutChecked(Box::new(m.clone()), focused))
        } else {
            self.handle(m)
        }
    }

    /// One level of indentation, per the tab settings.
    fn indent_unit(&self) -> String {
        if self.config.use_spaces {
            " ".repeat(self.config.tab_width)
        } else {
            "\t".to_string()
        }
    }

    /// What the Tab key types: spaces up to the next tab stop, or a tab.
    fn tab_text(&self) -> String {
        if !self.config.use_spaces {
            return "\t".to_string();
        }
        let (line, col) = self.editor.content.cursor_position();
        let before = self
            .editor
            .content
            .line(line)
            .map(|l| l[..floor_boundary(&l, col)].to_string())
            .unwrap_or_default();
        " ".repeat(text_ops::spaces_to_tab_stop(&before, self.config.tab_width))
    }

    /// Whether `edit` would change the buffer: Backspace at the very start
    /// or Delete at the very end do nothing, and shouldn't leave an undo step.
    fn edit_changes_text(&self, edit: &text_editor::Edit) -> bool {
        if self.editor.content.selection().is_some() {
            return true;
        }
        let (line, col) = self.editor.content.cursor_position();
        match edit {
            text_editor::Edit::Backspace => line > 0 || col > 0,
            text_editor::Edit::Delete => {
                let last = self.editor.content.line_count().saturating_sub(1);
                line < last
                    || self
                        .editor
                        .content
                        .line(line)
                        .is_some_and(|l| col < l.len())
            }
            _ => true,
        }
    }

    /// An action in the text area: undo grouping, auto-indent, auto-pairing,
    /// then the action itself.
    fn editor_action(&mut self, action: text_editor::Action) -> Task<Message> {
        let is_edit = action.is_edit();

        if is_edit {
            if let text_editor::Action::Edit(edit) = &action {
                if !self.edit_changes_text(edit) {
                    return Task::none();
                }
                match edit {
                    text_editor::Edit::Insert(ch) => {
                        if self.editor.content.selection().is_some() {
                            // Typing over a selection is a step of its own.
                            self.push_undo();
                            self.undo_new_group = ch.is_whitespace();
                        } else if ch.is_whitespace() {
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
                if let text_editor::Edit::Paste(text) = edit {
                    if text.len() > MAX_PASTE_BYTES {
                        self.undo_stack.pop_back();
                        self.set_status(
                            format!(
                                "{}: {}",
                                t!("status.error"),
                                t!("status.err_paste_too_large")
                            ),
                            true,
                        );
                        return Task::none();
                    }
                }
            }
        } else {
            // Motion action: next edit opens a new group
            self.undo_new_group = true;
            self.pending_closers.clear();
        }

        // Auto-indent: replicate leading whitespace of current line on Enter
        if self.config.auto_indent {
            if let text_editor::Action::Edit(text_editor::Edit::Enter) = &action {
                self.pending_closers.clear();
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
                return Task::none();
            }
        }

        // Auto-pairing of brackets and quotes
        if let text_editor::Action::Edit(text_editor::Edit::Insert(ch)) = &action {
            if let Some(command) = self.autopair(*ch, &action) {
                return command;
            }
            self.shift_pending_closers(ch.len_utf8());
        } else if is_edit {
            self.pending_closers.clear();
        }

        self.editor.content.perform(action);
        if is_edit {
            self.is_dirty = true;
        }
        Task::none()
    }

    /// Characters typed on the cursor's line push the auto-inserted closers
    /// after it to the right.
    fn shift_pending_closers(&mut self, bytes: usize) {
        let (line, col) = self.editor.content.cursor_position();
        for (l, c, _) in &mut self.pending_closers {
            if *l == line && *c >= col {
                *c += bytes;
            }
        }
    }

    /// (line count, topmost line an edit may touch), taken before an edit.
    fn edit_probe(&self) -> (usize, usize) {
        let (line, _) = self.editor.content.cursor_position();
        let above = self
            .editor
            .content
            .selection()
            .map_or(0, |s| s.matches('\n').count());
        (self.editor.content.line_count(), line.saturating_sub(above))
    }

    /// Moves the current file's bookmarks after an edit that changed the line
    /// count (see `edit_probe`).
    fn shift_bookmarks(&mut self, count_before: usize, top_before: usize) {
        let (Some(path), count) = (&self.current_file, self.editor.content.line_count()) else {
            return;
        };
        if count == count_before {
            return;
        }
        let top = top_before.min(self.editor.content.cursor_position().0);
        self.bookmarks
            .shift(path, top, count as isize - count_before as isize);
    }

    /// Runs an edit of the buffer and moves the bookmarks along with it.
    fn edited(&mut self, edit: impl FnOnce(&mut Self)) {
        let (count, top) = self.edit_probe();
        edit(self);
        self.shift_bookmarks(count, top);
    }

    fn bookmark_lines(&self) -> &[usize] {
        self.current_file
            .as_deref()
            .map_or(&[], |path| self.bookmarks.lines(path))
    }

    fn bookmark_state(&self) -> crate::menu_bar::BookmarkState {
        let line = self.editor.content.cursor_position().0;
        crate::menu_bar::BookmarkState {
            on_line: self
                .current_file
                .as_deref()
                .is_some_and(|path| self.bookmarks.has(path, line)),
            any: !self.bookmark_lines().is_empty(),
        }
    }

    /// Moves the cursor to the next (or previous) bookmark of the file.
    fn jump_to_bookmark(&mut self, forward: bool) -> Task<Message> {
        let Some(path) = self.current_file.clone() else {
            return Task::none();
        };
        let (line, _) = self.editor.content.cursor_position();
        let total = self.editor.content.line_count();
        let target = if forward {
            self.bookmarks.next(&path, line, total)
        } else {
            self.bookmarks.previous(&path, line, total)
        };
        match target {
            Some(target) => {
                crate::search::goto_line_start(&mut self.editor.content, target);
                self.undo_new_group = true;
            }
            None => self.set_status(t!("status.no_bookmarks"), false),
        }
        Task::none()
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            text: crate::editor::buffer_text(&self.editor.content),
            cursor: self.editor.content.cursor_position(),
        }
    }

    fn push_undo(&mut self) {
        let snapshot = self.snapshot();
        self.undo_stack.push_back(snapshot);
        trim_history(&mut self.undo_stack);
        self.redo_stack.clear();
    }

    /// Keeps the undo history of the background tabs within a global budget
    /// (the largest histories go first).
    fn enforce_undo_budget(&mut self) {
        let mut total: usize = self.undo_cache.values().map(history_bytes).sum();
        while total > UNDO_TOTAL_BYTES {
            let Some((largest, size)) = self
                .undo_cache
                .iter()
                .map(|(p, h)| (p.clone(), history_bytes(h)))
                .max_by_key(|(_, size)| *size)
            else {
                break;
            };
            self.undo_cache.remove(&largest);
            total -= size;
        }
    }

    /// Puts the cursor at (`line`, byte column `col`), clamped to the buffer.
    fn restore_cursor(&mut self, line: usize, col: usize) {
        self.editor.content.select_bytes(line, col, 0);
    }

    /// Replaces the buffer with a history snapshot and puts the cursor back.
    fn restore_snapshot(&mut self, snap: Snapshot) -> Task<Message> {
        let (line, col) = snap.cursor;
        // Back to the saved text means nothing left to save.
        self.is_dirty = self.saved_fp != Some(fingerprint(&snap.text));
        self.undo_new_group = true;
        self.replace_buffer_text(&snap.text);
        self.restore_cursor(line, col);
        Task::none()
    }

    /// Applies a whole-buffer text transformation as one undo step and moves
    /// the cursor to (`line`, `col`), or selects `selection`. A no-op
    /// transformation changes nothing.
    fn apply_text_edit(
        &mut self,
        new_text: String,
        line: usize,
        col: usize,
        selection: Option<Span>,
    ) -> Task<Message> {
        let old = crate::editor::buffer_text(&self.editor.content);
        if new_text == old {
            return Task::none();
        }
        // One copy of the buffer serves the undo step, the bookmarks and the
        // minimal replacement.
        self.undo_stack.push_back(Snapshot {
            text: old.clone(),
            cursor: self.editor.content.cursor_position(),
        });
        trim_history(&mut self.undo_stack);
        self.redo_stack.clear();
        self.is_dirty = true;
        self.undo_new_group = true;
        if let Some(path) = &self.current_file {
            self.bookmarks.remap(path, &old, &new_text);
        }
        crate::editor::replace_text(&mut self.editor.content, &old, &new_text);
        match selection {
            Some(span) => {
                let len = self.span_len(span);
                self.editor
                    .content
                    .select_bytes(span.start.0, span.start.1, len);
            }
            None => self.restore_cursor(line, col),
        }
        Task::none()
    }

    /// Like `apply_text_edit` for a line command: the cursor (and selection)
    /// keep their place on the text, `line_shift` lines further down.
    fn apply_line_edit(
        &mut self,
        old: &str,
        new_text: String,
        line_shift: isize,
        selection: Option<Span>,
    ) -> Task<Message> {
        let (line, col) = self.editor.content.cursor_position();
        let new_line = (line as isize + line_shift).max(0) as usize;
        let col = remap_col(nth_line(old, line), nth_line(&new_text, new_line), col);
        let selection = selection.map(|s| remap_span(old, &new_text, s, line_shift));
        self.apply_text_edit(new_text, new_line, col, selection)
    }

    fn replace_buffer_text(&mut self, new_text: &str) {
        if let Some(path) = &self.current_file {
            if !self.bookmarks.lines(path).is_empty() {
                let old = crate::editor::buffer_text(&self.editor.content);
                self.bookmarks.remap(path, &old, new_text);
            }
        }
        crate::editor::set_buffer_text(&mut self.editor.content, new_text);
    }

    /// The bytes covered by `span` in the buffer as it is now.
    fn span_len(&self, span: Span) -> usize {
        let content = &self.editor.content;
        let len_of = |i: usize| content.line(i).map_or(0, |l| l.len());
        if span.start.0 == span.end.0 {
            return span.end.1.saturating_sub(span.start.1);
        }
        let middle: usize = (span.start.0 + 1..span.end.0).map(|i| len_of(i) + 1).sum();
        len_of(span.start.0).saturating_sub(span.start.1) + 1 + middle + span.end.1
    }

    /// The span of a selection running over several lines. (`text_editor`
    /// exposes the selected text and the cursor, not the anchor, so the start
    /// is inferred by checking which end of the selection the cursor is on
    /// against the buffer's actual lines.)
    fn selection_span(&self) -> Option<Span> {
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
            let start_line = cursor_line + 1 - n;
            let start_col = self
                .editor
                .content
                .line(start_line)
                .map_or(0, |l| l.len().saturating_sub(first.len()));
            return Some(Span {
                start: (start_line, start_col),
                end: (cursor_line, cursor_col),
            });
        }
        // ...or at its start.
        fits(cursor_line).then_some(Span {
            start: (cursor_line, cursor_col),
            end: (cursor_line + n - 1, last.len()),
        })
    }

    /// The lines a line command applies to: the selected ones, else the
    /// cursor's.
    fn lines_to_edit(&self, span: Option<Span>) -> (usize, usize) {
        span.map_or_else(
            || {
                let line = self.editor.content.cursor_position().0;
                (line, line)
            },
            Span::lines,
        )
    }

    /// Alt+Up / Alt+Down: the selected lines (or the cursor's) one line up or down.
    fn move_lines(&mut self, up: bool) -> Task<Message> {
        if self.editing_blocked() {
            return Task::none();
        }
        let text = crate::editor::buffer_text(&self.editor.content);
        let span = self.selection_span();
        let (first, last) = self.lines_to_edit(span);
        let (new_text, new_first) = text_ops::move_lines(&text, first, last, up);
        let shift = new_first as isize - first as isize;
        self.apply_line_edit(&text, new_text, shift, span)
    }

    /// Auto-closing of brackets and quotes for the typed character `ch`.
    /// `None` means "insert it normally".
    fn autopair(&mut self, ch: char, action: &text_editor::Action) -> Option<Task<Message>> {
        use text_editor::{Action, Edit};

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

        // Typing the closing character that was inserted automatically steps
        // over it; one the user typed (or any other) is just inserted.
        let pending = self
            .pending_closers
            .iter()
            .position(|&(l, c, p)| l == line && c == col && p == ch);
        if (is_closer || is_quote) && selection.is_none() && after == Some(ch) {
            if let Some(i) = pending {
                self.pending_closers.remove(i);
                // Replace the character with itself rather than moving the
                // cursor: a motion over a line just typed on would panic.
                self.editor.content.perform(Action::Edit(Edit::Delete));
                self.editor.content.perform(Action::Edit(Edit::Insert(ch)));
                return Some(Task::none());
            }
        }
        if is_closer {
            return None;
        }
        let close = closer.unwrap_or(ch);

        // With a selection, surround it instead of replacing it. (The undo
        // step was opened by `editor_action`.)
        if let Some(selected) = selection {
            self.pending_closers.clear();
            self.editor
                .content
                .perform(Action::Edit(Edit::Paste(std::sync::Arc::new(format!(
                    "{ch}{selected}{close}"
                )))));
            self.is_dirty = true;
            return Some(Task::none());
        }

        // Only pair before whitespace / a closer / line end; quotes also
        // not after a word character (apostrophes: don't, it's), and a
        // single quote not at all in Rust, where it mostly starts a lifetime.
        let room = after.is_none_or(|c| {
            c.is_whitespace() || matches!(c, ')' | ']' | '}' | ',' | ';' | ':' | '.')
        });
        let word_before = before.is_some_and(|c| c.is_alphanumeric() || c == '\\');
        let rust_lifetime = ch == '\'' && self.editor.language.as_deref() == Some("rs");
        if !room || (is_quote && word_before) || rust_lifetime {
            return None;
        }

        self.editor.content.perform(action.clone());
        self.editor
            .content
            .perform(Action::Edit(Edit::Insert(close)));
        self.is_dirty = true;
        // Back over the closer, which then sits right after the cursor.
        let (line, end) = self.editor.content.cursor_position();
        let col = end - close.len_utf8();
        self.editor.content.select_bytes(line, col, 0);
        self.pending_closers.push((line, col, close));
        Some(Task::none())
    }

    /// Whether the tab for `path` holds unsaved changes.
    fn is_path_dirty(&self, path: &Path) -> bool {
        if self.current_file.as_deref() == Some(path) {
            self.is_dirty
        } else {
            self.file_cache.get(path).is_some_and(|e| e.dirty)
        }
    }

    /// Closes a tab, asking first if that would discard unsaved changes.
    fn request_close(&mut self, path: PathBuf) -> Task<Message> {
        if self.is_path_dirty(&path) {
            self.overlay = ActiveOverlay::ConfirmClose(path);
            Task::none()
        } else {
            self.close_path(path)
        }
    }

    /// Removes a tab and, if it was the active one, switches to its neighbour.
    fn close_path(&mut self, path: PathBuf) -> Task<Message> {
        let index = self.sidebar.files().iter().position(|p| *p == path);
        self.file_cache.remove(&path);
        self.undo_cache.remove(&path);
        self.sidebar.remove_path(&path);
        // An untitled file is gone for good once closed; a saved one keeps
        // its bookmarks for the next time it is opened.
        if is_untitled(&path) {
            self.bookmarks.clear(&path);
        }

        let mut command = Task::none();
        if self.current_file.as_ref() == Some(&path) {
            self.editor = EditorState::new();
            self.reset_view_mode();
            self.current_file = None;
            self.is_dirty = false;
            self.undo_stack.clear();
            self.redo_stack.clear();
            self.undo_new_group = true;
            self.search.reset();
            self.format = TextFormat::default();
            self.disk_stamp = None;
            self.saved_fp = None;
            self.set_status(t!("status.ready"), false);
            let files = self.sidebar.files();
            let next = index
                .and_then(|i| files.get(i))
                .or_else(|| files.last())
                .cloned();
            command = match next {
                Some(next) => self.activate(next),
                None => self.ensure_tab(),
            };
        }
        let _ = self.save_session_now();
        command
    }

    /// The editor is never left without a tab: when the last one is closed, a
    /// blank untitled one takes its place (text typed with nowhere to live
    /// would be lost).
    fn ensure_tab(&mut self) -> Task<Message> {
        if self.current_file.is_some() {
            return Task::none();
        }
        if let Some(first) = self.sidebar.files().first().cloned() {
            return self.activate(first);
        }
        self.untitled_counter = 1;
        self.load_buffer(untitled_path(1), CachedBuffer::blank());
        Task::none()
    }

    /// Parks the active buffer (text, cursor, scroll, language, dirty flag,
    /// undo history) so another file can take the editor.
    fn stash_current(&mut self) {
        if let Some(current) = self.current_file.clone() {
            let content = std::mem::take(&mut self.editor.content);
            self.file_cache.insert(
                current.clone(),
                CachedBuffer {
                    content: Some(content),
                    language: self.editor.language.clone(),
                    dirty: self.is_dirty,
                    stamp: self.disk_stamp,
                    format: self.format,
                    saved_fp: self.saved_fp,
                    view_mode: self.view_mode,
                },
            );
            self.undo_cache.insert(
                current,
                (
                    std::mem::take(&mut self.undo_stack),
                    std::mem::take(&mut self.redo_stack),
                ),
            );
            self.enforce_undo_budget();
        }
    }

    /// Puts a buffer in the editor and makes it the current tab.
    fn load_buffer(&mut self, path: PathBuf, buffer: CachedBuffer) {
        let content = buffer.content.unwrap_or_default();
        let first_line = first_line_of(&content);
        self.editor = EditorState {
            content,
            language: None,
        };
        self.editor
            .restore_language(buffer.language, &path, &first_line);
        self.reset_view_mode();
        self.view_mode = buffer.view_mode;
        self.sidebar.add_file(path.clone());
        let (undo, redo) = self.undo_cache.remove(&path).unwrap_or_default();
        self.undo_stack = undo;
        self.redo_stack = redo;
        self.undo_new_group = true;
        self.pending_closers.clear();
        self.search.reset();
        self.format_error = None;
        self.show_error_panel = false;
        self.format = buffer.format;
        self.disk_stamp = buffer.stamp;
        self.saved_fp = buffer.saved_fp;
        self.external_warned = false;
        self.current_file = Some(path);
        self.is_dirty = buffer.dirty;
        self.text_rev += 1;
    }

    /// Makes a file just read the active tab.
    fn open_loaded(&mut self, loaded: Loaded) {
        let path = loaded.path.clone();
        self.pending_open = None;
        if self.current_file.as_ref() == Some(&path) {
            // Opening the file being edited again: bring it up to date, but
            // never throw away unsaved changes.
            if self.is_dirty {
                self.set_status(t!("status.already_open_modified"), false);
            } else {
                self.apply_reload(loaded);
            }
            return;
        }
        let cached = self.file_cache.remove(&path);
        self.stash_current();
        let mut warn_changed = false;
        let buffer = match cached {
            // A background tab with unsaved edits wins over the disk.
            Some(c) if c.dirty && c.content.is_some() => {
                warn_changed = c.stamp.is_some() && loaded.stamp != c.stamp;
                c
            }
            other => {
                let view_mode = other.as_ref().map(|c| c.view_mode);
                let language = other.and_then(|c| c.language);
                let mut buffer = CachedBuffer::from_loaded(loaded, language);
                if let Some(mode) = view_mode {
                    buffer.view_mode = mode;
                }
                buffer
            }
        };
        self.config.add_recent(&path);
        self.config_dirty = true;
        self.load_buffer(path, buffer);
        if warn_changed {
            self.external_warned = true;
            self.set_status(t!("status.changed_on_disk"), true);
        } else if self.has_very_long_line() {
            self.set_status(t!("status.long_line_hint"), false);
        } else {
            self.set_status(t!("status.file_opened"), false);
        }
    }

    /// Whether a line is so long (minified JSON, say) that typing on it is
    /// slow: the status bar then suggests formatting the file.
    fn has_very_long_line(&self) -> bool {
        self.editor
            .language
            .as_deref()
            .is_some_and(crate::formatter::has_formatter)
            && self
                .editor
                .content
                .lines()
                .any(|line| line.len() > LONG_LINE_BYTES)
    }

    /// Replaces the text of the active tab by a newer version from disk,
    /// keeping the undo history (the old text is one Ctrl+Z away), the cursor,
    /// the scroll position, the language and the view mode.
    fn apply_reload(&mut self, loaded: Loaded) {
        if self.current_file.as_ref() != Some(&loaded.path) {
            return;
        }
        if self.is_dirty {
            // Typing started while the file was being read: the disk version
            // is not applied, and saving will ask first.
            self.external_warned = true;
            self.set_status(t!("status.changed_on_disk"), true);
            return;
        }
        let old = crate::editor::buffer_text(&self.editor.content);
        if old != loaded.text {
            let (line, col) = self.editor.content.cursor_position();
            self.push_undo();
            self.undo_new_group = true;
            self.replace_buffer_text(&loaded.text);
            self.restore_cursor(line, col);
            self.text_rev += 1;
        }
        self.saved_fp = Some(fingerprint(&loaded.text));
        self.format = loaded.format;
        self.disk_stamp = loaded.stamp;
        self.external_warned = false;
        self.size_for = None;
        self.set_status(t!("status.file_reloaded"), false);
    }

    /// Switches to a tab: instantly if its buffer is cached, from disk otherwise.
    fn activate(&mut self, path: PathBuf) -> Task<Message> {
        if self.current_file.as_ref() == Some(&path) {
            return Task::none();
        }
        self.pending_open = Some(path.clone());
        let cached = self.file_cache.get(&path);
        if let Some(entry) = cached.filter(|e| e.content.is_some()) {
            // A saved file may have changed since this copy was made.
            let changed =
                !is_untitled(&path) && entry.stamp.is_some() && DiskStamp::of(&path) != entry.stamp;
            if changed && !entry.dirty {
                return Task::perform(read_file(path, false), Message::TabRead);
            }
            let Some(entry) = self.file_cache.remove(&path) else {
                return Task::none();
            };
            self.pending_open = None;
            self.stash_current();
            self.load_buffer(path, entry);
            if changed {
                // Modified here and there: saving will ask before overwriting.
                self.external_warned = true;
                self.set_status(t!("status.changed_on_disk"), true);
            } else {
                self.set_status(t!("status.file_opened"), false);
            }
            Task::none()
        } else if is_untitled(&path) {
            self.pending_open = None;
            self.stash_current();
            self.load_buffer(path, CachedBuffer::blank());
            Task::none()
        } else {
            // TabRead parks the current buffer once the read succeeds.
            Task::perform(read_file(path, false), Message::TabRead)
        }
    }

    /// Whether the file changed on disk since this tab read or wrote it.
    fn changed_on_disk(&self, path: &Path) -> bool {
        !is_untitled(path)
            && self.disk_stamp.is_some()
            && DiskStamp::of(path).is_some_and(|now| Some(now) != self.disk_stamp)
    }

    /// Notices edits made to the current file by other programs: a clean
    /// buffer is reloaded, a modified one is warned about (saving then asks
    /// before overwriting).
    fn check_external_change(&mut self) -> Task<Message> {
        let Some(path) = self.current_file.clone().filter(|p| !is_untitled(p)) else {
            return Task::none();
        };
        let Some(now) = DiskStamp::of(&path) else {
            return Task::none();
        };
        if Some(now) == self.disk_stamp {
            return Task::none();
        }
        if self.is_dirty {
            if !self.external_warned {
                self.external_warned = true;
                self.set_status(t!("status.changed_on_disk"), true);
            }
            return Task::none();
        }
        // The new state becomes the baseline only once it has been applied.
        Task::perform(read_file(path, false), Message::FileReloaded)
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
    }

    /// Reparses the buffer for the preview pane, but only when it is visible
    /// and its text may have changed since the last parse.
    fn refresh_preview(&mut self) {
        let Some(kind) = self.preview_kind() else {
            self.preview = PreviewDoc::None;
            return;
        };
        if self.view_mode == ViewMode::Raw {
            return;
        }
        if !matches!(self.preview, PreviewDoc::None) && self.preview_text_rev == self.text_rev {
            return;
        }

        let src = crate::editor::buffer_text(&self.editor.content);
        self.preview = match kind {
            PreviewKind::Markdown => {
                PreviewDoc::Markdown(std::sync::Arc::new(crate::preview::markdown::parse(&src)))
            }
            PreviewKind::Json => match crate::preview::json::parse(&src) {
                Ok(value) => PreviewDoc::Json(std::sync::Arc::new(value)),
                Err(e) => PreviewDoc::JsonError(e),
            },
        };
        self.preview_text_rev = self.text_rev;
        self.preview_rev += 1;
    }

    /// Shows a formatter's failure: the whole output in the panel, a short
    /// note in the status bar.
    fn show_format_error(&mut self, error: String) {
        let short = crate::formatter::short_error(&error);
        self.format_error = Some(error);
        self.show_error_panel = true;
        self.set_status(format!("{}: {short}", t!("status.format_error")), true);
    }

    /// The file the formatter should believe it is formatting, so it finds
    /// the project's configuration next to it. `None` for an unsaved buffer.
    fn format_target(&self) -> Option<PathBuf> {
        self.current_file.clone().filter(|p| !is_untitled(p))
    }

    /// Opens the "Save as" dialog; the chosen path comes back as `SaveAsChosen`.
    fn start_save_as(&self, origin: Option<PathBuf>) -> Task<Message> {
        let suggestion = origin.clone().filter(|p| !is_untitled(p));
        let name = origin.as_deref().map(file_label);
        Task::perform(pick_save_path(suggestion, name), move |result| {
            Message::SaveAsChosen(origin.clone(), result)
        })
    }
}

/// The first line of the buffer (capped), to recognize a `#!` script.
fn first_line_of(content: &crate::code_editor::Content) -> String {
    content
        .line(0)
        .map(|l| l.chars().take(200).collect())
        .unwrap_or_default()
}

/// The largest char boundary of `s` at or before `index`.
fn floor_boundary(s: &str, index: usize) -> usize {
    let mut index = index.min(s.len());
    while !s.is_char_boundary(index) {
        index -= 1;
    }
    index
}

/// A selection over several lines: (line, byte column) of both ends.
#[derive(Debug, Clone, Copy)]
struct Span {
    start: (usize, usize),
    end: (usize, usize),
}

impl Span {
    /// The lines a line command touches: a selection ending at the very
    /// start of a line doesn't include it.
    fn lines(self) -> (usize, usize) {
        let last = if self.end.1 == 0 && self.end.0 > self.start.0 {
            self.end.0 - 1
        } else {
            self.end.0
        };
        (self.start.0, last)
    }
}

fn nth_line(text: &str, index: usize) -> &str {
    text.split('\n').nth(index).unwrap_or("")
}

/// A column of `old_line` as it reads in `new_line` after an edit that
/// changed the line's length at its start.
fn remap_col(old_line: &str, new_line: &str, col: usize) -> usize {
    if col == 0 {
        return 0;
    }
    let delta = new_line.len() as isize - old_line.len() as isize;
    let col = (col as isize + delta).clamp(0, new_line.len() as isize) as usize;
    floor_boundary(new_line, col)
}

fn remap_span(old: &str, new: &str, span: Span, line_shift: isize) -> Span {
    let map = |(line, col): (usize, usize)| {
        let new_line = (line as isize + line_shift).max(0) as usize;
        (
            new_line,
            remap_col(nth_line(old, line), nth_line(new, new_line), col),
        )
    };
    Span {
        start: map(span.start),
        end: map(span.end),
    }
}

// ─── Editor pane ────────────────────────────────────────────────────────────

impl SimpleEditApp {
    /// The editor pane: the text editor alone, the preview, or both side by
    /// side (the mode switch lives in the panel's toolbar).
    fn editor_pane(
        &self,
        kind: Option<PreviewKind>,
        mode: ViewMode,
        dark: bool,
    ) -> Element<'_, Message> {
        let search = self.show_search.then_some(&self.search);
        let marks = self.bookmark_lines();
        // The context menu's actions (select/cut/copy/paste/format) all act on
        // the raw buffer: it wraps the editor only — not the preview beside
        // it, and not the preview alone, where it would act on text the user
        // can't see.
        let editor = || self.with_context_menu(self.editor.view(&self.config, search, marks), dark);
        if kind.is_none() {
            return editor();
        }

        match mode {
            ViewMode::Raw => editor(),
            ViewMode::Preview => self.preview_view(dark),
            ViewMode::Split => row![
                container(editor()).width(Length::FillPortion(54)),
                container(Space::new().width(1))
                    .height(Length::Fill)
                    .style(se_theme::rule(dark)),
                container(self.preview_view(dark)).width(Length::FillPortion(46)),
            ]
            .height(Length::Fill)
            .into(),
        }
    }

    /// The 40px bar above the editor: file name and folder on the left, the
    /// Raw/Preview/Split switch (Markdown, JSON) or Format button on the right.
    fn toolbar_view(
        &self,
        kind: Option<PreviewKind>,
        mode: ViewMode,
        dark: bool,
    ) -> Element<'_, Message> {
        let name = self
            .current_file
            .as_deref()
            .map(file_label)
            .unwrap_or_else(|| t!("app.untitled").to_string());
        let dir = self
            .current_file
            .as_deref()
            .filter(|p| !is_untitled(p))
            .and_then(Path::parent)
            .map(short_dir)
            .unwrap_or_default();

        let right: Element<Message> = match kind {
            Some(kind) => crate::preview::segmented(kind, mode, dark),
            None => {
                let has_formatter = self
                    .editor
                    .language
                    .as_deref()
                    .is_some_and(crate::formatter::has_formatter);
                button(text(t!("menu.edit_format").to_string()).size(12))
                    .padding([5, 11])
                    .on_press_maybe(has_formatter.then_some(Message::FormatFile))
                    .style((se_theme::Outline { dark }).style())
                    .into()
            }
        };

        row![
            text(name)
                .size(13)
                .font(se_theme::UI_SEMIBOLD)
                .shaping(iced::widget::text::Shaping::Advanced),
            text(dir)
                .size(12)
                .color(se_theme::muted_text(dark))
                .shaping(iced::widget::text::Shaping::Advanced),
            Space::new().width(Length::Fill),
            right,
        ]
        .spacing(10)
        .padding([0, 14])
        .height(40)
        .align_y(Alignment::Center)
        .into()
    }

    /// Wraps the editor with the right-click menu.
    fn with_context_menu<'a>(
        &self,
        editor_widget: Element<'a, Message>,
        dark: bool,
    ) -> Element<'a, Message> {
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
        let has_sel_ctx = self.selection_info.is_some();
        let sh_select_all = self.config.shortcuts.select_all.display();
        let sh_format_all = self.config.shortcuts.format_code.display();
        // Cut/Copy/Paste are the platform's own keys, not configurable.
        let clip = |letter: &str| {
            if cfg!(target_os = "macos") {
                format!("⌘{letter}")
            } else {
                format!("Ctrl+{letter}")
            }
        };
        let (sh_cut, sh_copy, sh_paste) = (clip("X"), clip("C"), clip("V"));
        let bookmark_state = self.bookmark_state();
        let sh_bookmark = self.config.shortcuts.toggle_bookmark.display();

        let minimap_width = if self.config.show_minimap {
            crate::code_editor::MINIMAP_WIDTH
        } else {
            0.0
        };
        let menu = iced_aw::ContextMenu::new(editor_widget, move || {
            use crate::menu_bar::{disabled_item, item, separator};
            let fmt_sel_el: Element<'static, Message> = if has_fmt_ctx && has_sel_ctx {
                item(lbl_format_sel.clone(), None, dark, Message::FormatSelection)
            } else {
                disabled_item(lbl_format_sel.clone(), dark)
            };
            let fmt_all_el: Element<'static, Message> = if has_fmt_ctx {
                item(
                    lbl_format_all.clone(),
                    Some(sh_format_all.clone()),
                    dark,
                    Message::FormatFile,
                )
            } else {
                disabled_item(lbl_format_all.clone(), dark)
            };
            let ctx_items: Vec<Element<'static, Message>> = vec![
                item(
                    lbl_cut.clone(),
                    Some(sh_cut.clone()),
                    dark,
                    Message::ContextCut,
                ),
                item(
                    lbl_copy.clone(),
                    Some(sh_copy.clone()),
                    dark,
                    Message::ContextCopy,
                ),
                item(
                    lbl_paste.clone(),
                    Some(sh_paste.clone()),
                    dark,
                    Message::ContextPaste,
                ),
                item(lbl_delete.clone(), None, dark, Message::ContextDelete),
                item(
                    lbl_select_all.clone(),
                    Some(sh_select_all.clone()),
                    dark,
                    Message::SelectAll,
                ),
                separator(dark),
                fmt_sel_el,
                fmt_all_el,
                separator(dark),
            ];
            let mut ctx_items = ctx_items;
            ctx_items.extend(crate::menu_bar::bookmark_items(
                bookmark_state,
                dark,
                Some(sh_bookmark.clone()),
            ));
            container(
                column(ctx_items)
                    .spacing(1)
                    .padding(6)
                    .width(Length::Fixed(264.0)),
            )
            .style(crate::theme::card(dark))
            .into()
        });
        crate::floating::RightClick::new(menu, minimap_width, Message::EditorRightClick).into()
    }

    /// Open files with unsaved changes (the dot in the sidebar).
    fn dirty_files(&self) -> HashSet<PathBuf> {
        let mut dirty: HashSet<PathBuf> = self
            .file_cache
            .iter()
            .filter(|(_, entry)| entry.dirty)
            .map(|(path, _)| path.clone())
            .collect();
        if let (true, Some(current)) = (self.is_dirty, &self.current_file) {
            dirty.insert(current.clone());
        }
        dirty
    }

    /// The command palette's rows for the current query.
    fn palette_entries(&self) -> Vec<crate::palette::Entry> {
        let has_formatter = self
            .editor
            .language
            .as_deref()
            .is_some_and(crate::formatter::has_formatter);
        crate::palette::filter(
            crate::palette::entries(&self.config, has_formatter, self.language_picker_enabled()),
            &self.palette_query,
        )
    }

    /// The rendered document, or the parser error when the JSON is invalid.
    fn preview_view(&self, dark: bool) -> Element<'_, Message> {
        match &self.preview {
            // `lazy` keeps the built widget tree until the revision changes:
            // rebuilding up to 20 000 JSON rows on every frame is wasteful.
            PreviewDoc::Markdown(blocks) => {
                let blocks = blocks.clone();
                // The wrapper gives `lazy` (which reports a shrink size) the
                // full pane.
                container(iced::widget::lazy((self.preview_rev, dark), move |_| {
                    crate::preview::markdown::view(&blocks, dark)
                }))
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
            }
            PreviewDoc::Json(value) => {
                let (value, collapsed) = (value.clone(), self.json_collapsed.clone());
                container(iced::widget::lazy((self.preview_rev, dark), move |_| {
                    crate::preview::json::view(&value, &collapsed, dark)
                }))
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
            }
            PreviewDoc::JsonError(err) => container(
                column![
                    text(t!("preview.json_error").to_string())
                        .size(13)
                        .color(se_theme::error_color(dark)),
                    text(err.clone())
                        .size(12)
                        .font(se_theme::MONO)
                        .color(se_theme::muted_text(dark)),
                ]
                .spacing(6),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .padding([16, 20])
            .into(),
            PreviewDoc::None => container(Space::new().height(0))
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
        }
    }
}

#[path = "app/file_io.rs"]
mod file_io;
use file_io::*;
#[path = "app/views.rs"]
mod views;

#[cfg(test)]
#[path = "app/tests.rs"]
mod tests;

/// What a tab is called: the file name, or "Untitled N".
pub fn display_name(path: &Path) -> String {
    if is_untitled(path) {
        let n = path
            .to_str()
            .unwrap_or("")
            .trim_start_matches("untitled://");
        format!("{} {}", t!("app.untitled"), n)
    } else {
        path.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "?".to_string())
    }
}

fn file_label(path: &Path) -> String {
    display_name(path)
}

/// `display_name` of each path, with the parent folder added to the ones
/// that would otherwise read the same.
pub fn unique_labels(paths: &[PathBuf]) -> Vec<String> {
    let names: Vec<String> = paths.iter().map(|p| display_name(p)).collect();
    paths
        .iter()
        .zip(&names)
        .map(|(path, name)| {
            let twins = names.iter().filter(|n| *n == name).count();
            let parent = path
                .parent()
                .and_then(|d| d.file_name())
                .map(|d| d.to_string_lossy().into_owned());
            match parent {
                Some(parent) if twins > 1 && !is_untitled(path) => format!("{name} — {parent}"),
                _ => name.clone(),
            }
        })
        .collect()
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
            text(title).size(15).color(p.text),
            text(message)
                .size(12)
                .color(p.muted)
                .shaping(iced::widget::text::Shaping::Advanced),
            row![
                button(text(t!("confirm.cancel").to_string()).size(13))
                    .padding([7, 16])
                    .on_press(Message::CloseOverlay)
                    .style((se_theme::GhostButton { dark, active: true }).style()),
                Space::new().width(Length::Fill),
                button(text(confirm_label).size(13))
                    .padding([7, 16])
                    .on_press(confirm)
                    .style(
                        (se_theme::GhostButton {
                            dark,
                            active: false,
                        })
                        .style()
                    ),
            ]
            .align_y(Alignment::Center),
        ]
        .spacing(14)
        .padding(24),
    )
    .width(360)
    .style(se_theme::card(dark))
    .into()
}

/// A 1px horizontal hairline.
fn rule<'a>(dark: bool) -> Element<'a, Message> {
    container(Space::new().height(1))
        .width(Length::Fill)
        .style(se_theme::rule(dark))
        .into()
}

/// A folder for the toolbar: the home directory shown as `~`, with a
/// trailing slash.
fn short_dir(dir: &Path) -> String {
    let shown = match dirs::home_dir()
        .and_then(|home| dir.strip_prefix(&home).ok().map(Path::to_path_buf))
    {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Some(rest) => format!("~/{}", rest.display()),
        None => dir.display().to_string(),
    };
    let shown = if shown.ends_with('/') {
        shown
    } else {
        format!("{shown}/")
    };
    // The toolbar has one line: keep the end of a long path (the folder the
    // file is in matters most).
    let count = shown.chars().count();
    if count > MAX_DIR_CHARS {
        let tail: String = shown.chars().skip(count - (MAX_DIR_CHARS - 1)).collect();
        format!("…{tail}")
    } else {
        shown
    }
}

/// Longest folder path shown in the toolbar, in characters.
const MAX_DIR_CHARS: usize = 56;

fn goto_line_id() -> iced::widget::Id {
    iced::widget::Id::new("goto-line")
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

/// Releases as a column: version and date, then each section and its changes.
fn changelog_list<'a>(dark: bool, entries: &[crate::changelog::Entry]) -> Element<'a, Message> {
    let p = se_theme::palette(dark);
    let mut list = column![]
        .spacing(6)
        .width(Length::Fill)
        .padding(iced::Padding {
            top: 0.0,
            right: 12.0,
            bottom: 0.0,
            left: 0.0,
        });
    for entry in entries {
        list = list.push(Space::new().height(8));
        let head = if entry.date.is_empty() {
            format!("v{}", entry.version)
        } else {
            format!("v{} · {}", entry.version, entry.date)
        };
        list = list.push(text(head).size(15).color(p.text));
        for (title, changes) in &entry.sections {
            list = list.push(text(title.clone()).size(12).color(p.accent));
            for change in changes {
                list = list.push(text(format!("•  {change}")).size(13).color(p.muted));
            }
        }
    }
    list.into()
}
