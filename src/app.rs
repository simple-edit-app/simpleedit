use iced::{
    executor,
    widget::{button, column, container, row, scrollable, text, text_editor, text_input, Space},
    Alignment, Application, Command, Element, Length, Theme,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::{
    config::{Config, ShortcutConfig, ShortcutTarget},
    editor::EditorState,
    preferences::{PreferencesMessage, PreferencesState},
    search::{SearchMessage, SearchState},
    sidebar::{SidebarAction, SidebarMessage, SidebarState},
    theme as se_theme,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    Shortcuts { capturing: Option<ShortcutTarget> },
    UpdateChecking,
    UpdateUpToDate,
    UpdateAvailable(String),
    UpdateDownloading,
    UpdateInstalling,
    UpdateError(String),
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
    FileSaved(Result<PathBuf, String>),
    NewFile,
    CloseFile,
    ToggleSearch,
    ToggleSidebar,
    TogglePreferences,
    ThemeChanged(bool),
    InsertTab,
    ToggleMenu(TopMenu),
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
    // Deferred cursor line restore after with_text() (layout must run first)
    RestoreCursorLine(usize),
    // Updater
    CheckForUpdate,
    UpdateChecked(Result<Option<String>, String>),
    ConfirmInstallUpdate(String),
    UpdateDownloaded(Result<std::path::PathBuf, String>),
    UpdateInstalled(Result<(), String>),
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
    /// Word-level undo history (capped at 100 snapshots).
    undo_stack: Vec<String>,
    /// Redo history rebuilt when undoing.
    redo_stack: Vec<String>,
    /// Whether the next edit should open a new undo group.
    /// True at start, after whitespace, after a cursor move, after delete.
    undo_new_group: bool,
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
            undo_new_group: true,
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
        if !matches!(&message, Message::ToggleMenu(_)) {
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
                        let raw = self.editor.content.text();
                        let (line_idx, _) = self.editor.content.cursor_position();
                        let indent: String = raw
                            .lines()
                            .nth(line_idx)
                            .unwrap_or("")
                            .chars()
                            .take_while(|c| *c == ' ' || *c == '\t')
                            .collect();
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

                // Autocomplete brackets: ( → (), [ → [], { → {}
                if self.config.autocomplete_brackets {
                    if let text_editor::Action::Edit(text_editor::Edit::Insert(ch)) = &action {
                        let pair = match ch {
                            '(' => Some(')'),
                            '[' => Some(']'),
                            '{' => Some('}'),
                            _ => None,
                        };
                        if let Some(close) = pair {
                            self.editor.content.perform(action.clone());
                            self.editor.content.perform(text_editor::Action::Edit(
                                text_editor::Edit::Insert(close),
                            ));
                            self.is_dirty = true;
                            // Motion::Left is a no-op on unshaped lines (cosmic_text bug).
                            // Defer to the next update cycle so layout() shapes the buffer first.
                            return Command::perform(async {}, |_| Message::MoveCursorLeft);
                        }
                    }
                }

                // Autocomplete quotes: " → "", ' → '', ` → ``
                if self.config.autocomplete_quotes {
                    if let text_editor::Action::Edit(text_editor::Edit::Insert(ch)) = &action {
                        if matches!(ch, '"' | '\'' | '`') {
                            let c = *ch;
                            self.editor.content.perform(action.clone());
                            self.editor
                                .content
                                .perform(text_editor::Action::Edit(text_editor::Edit::Insert(c)));
                            self.is_dirty = true;
                            return Command::perform(async {}, |_| Message::MoveCursorLeft);
                        }
                    }
                }

                self.editor.content.perform(action);
                if is_edit {
                    self.is_dirty = true;
                }
                Command::none()
            }
            Message::InsertTab => {
                if !self.show_search && !self.show_preferences {
                    if self.editor.content.selection().is_some() {
                        return self.update(Message::IndentSelection);
                    }
                    let text = if self.config.use_spaces {
                        " ".repeat(self.config.tab_width)
                    } else {
                        "\t".to_string()
                    };
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
                Command::none()
            }
            Message::NewFile => {
                if let Some(current) = self.current_file.clone() {
                    self.file_cache.insert(
                        current,
                        (
                            self.editor.content.text().to_string(),
                            self.editor.language.clone(),
                            self.is_dirty,
                        ),
                    );
                }
                self.untitled_counter += 1;
                let path = untitled_path(self.untitled_counter);
                self.editor = EditorState::new();
                self.current_file = Some(path.clone());
                self.sidebar.add_file(path);
                self.is_dirty = false;
                self.undo_stack.clear();
                self.redo_stack.clear();
                self.undo_new_group = true;
                self.status_message = t!("status.new_file").to_string();
                Command::none()
            }
            Message::OpenFile => Command::perform(open_file(), Message::FileOpened),
            Message::FileOpened(result) => {
                match result {
                    Ok((path, content)) => {
                        self.editor = EditorState::from_content(&content);
                        let ext = path
                            .extension()
                            .and_then(|e| e.to_str())
                            .unwrap_or("")
                            .to_string();
                        self.editor.set_language_by_extension(&ext);
                        self.sidebar.add_file(path.clone());
                        self.config.add_recent(&path);
                        self.config.save();
                        self.current_file = Some(path);
                        self.is_dirty = false;
                        self.undo_stack.clear();
                        self.redo_stack.clear();
                        self.undo_new_group = true;
                        self.status_message = t!("status.file_opened").to_string();
                    }
                    Err(e) => {
                        self.status_message = format!("{}: {}", t!("status.error"), e);
                    }
                }
                Command::none()
            }
            Message::SaveFile => {
                let content = self.editor.content.text().to_string();
                match self.current_file.clone() {
                    Some(path) if !is_untitled(&path) => {
                        Command::perform(save_file(path, content), Message::FileSaved)
                    }
                    _ => Command::perform(save_file_as(content), Message::FileSaved),
                }
            }
            Message::SaveFileAs => Command::perform(
                save_file_as(self.editor.content.text().to_string()),
                Message::FileSaved,
            ),
            Message::FileSaved(result) => {
                match result {
                    Ok(path) => {
                        self.file_cache.remove(&path);
                        if let Some(old) = &self.current_file {
                            if is_untitled(old) {
                                self.file_cache.remove(old);
                                self.sidebar.rename_file(old, path.clone());
                            }
                        }
                        self.sidebar.add_file(path.clone());
                        let ext = path
                            .extension()
                            .and_then(|e| e.to_str())
                            .unwrap_or("")
                            .to_string();
                        self.editor.set_language_by_extension(&ext);
                        self.config.add_recent(&path);
                        self.config.save();
                        self.current_file = Some(path);
                        self.is_dirty = false;
                        self.status_message = t!("status.file_saved").to_string();
                    }
                    Err(e) => {
                        self.status_message = format!("{}: {}", t!("status.error"), e);
                    }
                }
                Command::none()
            }
            Message::CloseFile => {
                self.editor = EditorState::new();
                self.current_file = None;
                self.is_dirty = false;
                self.undo_stack.clear();
                self.redo_stack.clear();
                self.undo_new_group = true;
                self.status_message = t!("status.ready").to_string();
                self.save_session();
                Command::none()
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
                SidebarAction::OpenFile(path) => {
                    if self.current_file.as_ref() == Some(&path) {
                        return Command::none();
                    }
                    if let Some(current) = self.current_file.clone() {
                        self.file_cache.insert(
                            current,
                            (
                                self.editor.content.text().to_string(),
                                self.editor.language.clone(),
                                self.is_dirty,
                            ),
                        );
                    }
                    self.undo_stack.clear();
                    self.redo_stack.clear();
                    if let Some((content, language, dirty)) = self.file_cache.get(&path).cloned() {
                        self.editor = EditorState::from_content(&content);
                        self.editor.language = language;
                        self.current_file = Some(path);
                        self.is_dirty = dirty;
                        self.status_message = t!("status.file_opened").to_string();
                        Command::none()
                    } else {
                        Command::perform(read_file(path), Message::FileOpened)
                    }
                }
                SidebarAction::CloseFile(path) => {
                    if self.current_file.as_ref() == Some(&path) {
                        self.file_cache.remove(&path);
                        self.editor = EditorState::new();
                        self.current_file = None;
                        self.is_dirty = false;
                        self.undo_stack.clear();
                        self.redo_stack.clear();
                    } else {
                        self.file_cache.remove(&path);
                    }
                    self.save_session();
                    Command::none()
                }
                SidebarAction::SaveFile(path) => {
                    let content = self.editor.content.text().to_string();
                    Command::perform(save_file(path, content), Message::FileSaved)
                }
                SidebarAction::SaveFileAs => Command::perform(
                    save_file_as(self.editor.content.text().to_string()),
                    Message::FileSaved,
                ),
                SidebarAction::None => Command::none(),
            },
            Message::Search(msg) => {
                self.search.update(msg, &mut self.editor.content);
                Command::none()
            }
            Message::Preferences(msg) => {
                self.preferences.update(msg, &mut self.config);
                self.config.save();
                Command::none()
            }
            Message::About => {
                self.overlay = ActiveOverlay::About;
                Command::none()
            }
            Message::Quit => std::process::exit(0),
            Message::ContextCopy => {
                if let Some(text) = self.editor.content.selection() {
                    return iced::clipboard::write(text);
                }
                Command::none()
            }
            Message::ContextCut => {
                if let Some(text) = self.editor.content.selection() {
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
                    let content = self.editor.content.text().to_string();
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
                    Command::perform(
                        crate::formatter::format(selected, ext),
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
                        self.editor.content = text_editor::Content::with_text(&formatted);
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
            Message::SelectionFormatted(result) => {
                self.is_formatting = false;
                match result {
                    Ok(formatted) => {
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
                    let current = self.editor.content.text().to_string();
                    if self.redo_stack.len() >= 100 {
                        self.redo_stack.remove(0);
                    }
                    self.redo_stack.push(current);
                    let lang = self.editor.language.clone();
                    self.editor.content = text_editor::Content::with_text(&prev);
                    self.editor.language = lang;
                    self.editor
                        .content
                        .perform(text_editor::Action::Move(text_editor::Motion::DocumentEnd));
                    self.is_dirty = true;
                    self.undo_new_group = true;
                }
                Command::none()
            }
            Message::Redo => {
                if let Some(next) = self.redo_stack.pop() {
                    let current = self.editor.content.text().to_string();
                    if self.undo_stack.len() >= 100 {
                        self.undo_stack.remove(0);
                    }
                    self.undo_stack.push(current);
                    let lang = self.editor.language.clone();
                    self.editor.content = text_editor::Content::with_text(&next);
                    self.editor.language = lang;
                    self.editor
                        .content
                        .perform(text_editor::Action::Move(text_editor::Motion::DocumentEnd));
                    self.is_dirty = true;
                    self.undo_new_group = true;
                }
                Command::none()
            }
            Message::AutoSave => {
                self.save_session();
                Command::none()
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
                Command::none()
            }
            Message::GotoLineInputChanged(s) => {
                // Allow only digits
                self.goto_line_input = s.chars().filter(|c| c.is_ascii_digit()).collect();
                Command::none()
            }
            Message::GotoLineSubmit => {
                if let Ok(n) = self.goto_line_input.parse::<usize>() {
                    let target = n.saturating_sub(1); // 0-indexed
                    let max = self.editor.content.line_count().saturating_sub(1);
                    let target = target.min(max);
                    self.editor.content.perform(text_editor::Action::Move(
                        text_editor::Motion::DocumentStart,
                    ));
                    for _ in 0..target {
                        self.editor
                            .content
                            .perform(text_editor::Action::Move(text_editor::Motion::Down));
                    }
                    self.editor
                        .content
                        .perform(text_editor::Action::Move(text_editor::Motion::Home));
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
                    match target {
                        ShortcutTarget::NewFile => self.config.shortcuts.new_file = sc,
                        ShortcutTarget::OpenFile => self.config.shortcuts.open_file = sc,
                        ShortcutTarget::SaveFile => self.config.shortcuts.save_file = sc,
                        ShortcutTarget::SaveAs => self.config.shortcuts.save_as = sc,
                        ShortcutTarget::CloseFile => self.config.shortcuts.close_file = sc,
                        ShortcutTarget::Find => self.config.shortcuts.find = sc,
                        ShortcutTarget::SelectAll => self.config.shortcuts.select_all = sc,
                        ShortcutTarget::FormatCode => self.config.shortcuts.format_code = sc,
                        ShortcutTarget::GotoLine => self.config.shortcuts.goto_line = sc,
                        ShortcutTarget::ToggleSidebar => self.config.shortcuts.toggle_sidebar = sc,
                        ShortcutTarget::Quit => self.config.shortcuts.quit = sc,
                        ShortcutTarget::Undo => self.config.shortcuts.undo = sc,
                        ShortcutTarget::Redo => self.config.shortcuts.redo = sc,
                        ShortcutTarget::DuplicateLine => self.config.shortcuts.duplicate_line = sc,
                        ShortcutTarget::MoveLineUp => self.config.shortcuts.move_line_up = sc,
                        ShortcutTarget::MoveLineDown => self.config.shortcuts.move_line_down = sc,
                        ShortcutTarget::ToggleComment => self.config.shortcuts.toggle_comment = sc,
                        ShortcutTarget::DeleteLine => self.config.shortcuts.delete_line = sc,
                    }
                    self.config.save();
                    self.overlay = ActiveOverlay::Shortcuts { capturing: None };
                    return Command::none();
                }
                // Don't fire shortcuts while any overlay is open
                if !matches!(self.overlay, ActiveOverlay::None) {
                    return Command::none();
                }
                // Match against config shortcuts
                let sc_check = |s: &ShortcutConfig| -> bool {
                    s.key.eq_ignore_ascii_case(&key)
                        && s.ctrl == ctrl
                        && s.shift == shift
                        && s.alt == alt
                };
                let sh = &self.config.shortcuts;
                let msg = if sc_check(&sh.undo) {
                    Some(Message::Undo)
                } else if sc_check(&sh.redo) {
                    Some(Message::Redo)
                } else if sc_check(&sh.duplicate_line) {
                    Some(Message::DuplicateLine)
                } else if sc_check(&sh.move_line_up) {
                    Some(Message::MoveLineUp)
                } else if sc_check(&sh.move_line_down) {
                    Some(Message::MoveLineDown)
                } else if sc_check(&sh.toggle_comment) {
                    Some(Message::ToggleComment)
                } else if sc_check(&sh.delete_line) {
                    Some(Message::DeleteLine)
                } else if sc_check(&sh.save_as) {
                    Some(Message::SaveFileAs)
                } else if sc_check(&sh.new_file) {
                    Some(Message::NewFile)
                } else if sc_check(&sh.save_file) {
                    Some(Message::SaveFile)
                } else if sc_check(&sh.open_file) {
                    Some(Message::OpenFile)
                } else if sc_check(&sh.close_file) {
                    Some(Message::CloseFile)
                } else if sc_check(&sh.find) {
                    Some(Message::ToggleSearch)
                } else if sc_check(&sh.select_all) {
                    Some(Message::SelectAll)
                } else if sc_check(&sh.format_code) {
                    Some(Message::FormatFile)
                } else if sc_check(&sh.goto_line) {
                    Some(Message::OpenGotoLine)
                } else if sc_check(&sh.toggle_sidebar) {
                    Some(Message::ToggleSidebar)
                } else if sc_check(&sh.quit) {
                    Some(Message::Quit)
                } else {
                    None
                };
                if let Some(m) = msg {
                    self.update(m)
                } else {
                    Command::none()
                }
            }
            Message::DuplicateLine => {
                if !self.show_search && !self.show_preferences {
                    self.push_undo();
                    let text = self.editor.content.text();
                    let (line_idx, _) = self.editor.content.cursor_position();
                    let new_text = duplicate_line(&text, line_idx);
                    let lang = self.editor.language.clone();
                    self.editor.content = text_editor::Content::with_text(&new_text);
                    self.editor.language = lang;
                    self.is_dirty = true;
                    self.undo_new_group = true;
                    let target = line_idx + 1;
                    return Command::perform(async move { target }, Message::RestoreCursorLine);
                }
                Command::none()
            }
            Message::MoveLineUp => {
                if !self.show_search && !self.show_preferences {
                    self.push_undo();
                    let text = self.editor.content.text();
                    let (line_idx, _) = self.editor.content.cursor_position();
                    let (new_text, new_line) = move_line(&text, line_idx, true);
                    let lang = self.editor.language.clone();
                    self.editor.content = text_editor::Content::with_text(&new_text);
                    self.editor.language = lang;
                    self.is_dirty = true;
                    self.undo_new_group = true;
                    return Command::perform(async move { new_line }, Message::RestoreCursorLine);
                }
                Command::none()
            }
            Message::MoveLineDown => {
                if !self.show_search && !self.show_preferences {
                    self.push_undo();
                    let text = self.editor.content.text();
                    let (line_idx, _) = self.editor.content.cursor_position();
                    let (new_text, new_line) = move_line(&text, line_idx, false);
                    let lang = self.editor.language.clone();
                    self.editor.content = text_editor::Content::with_text(&new_text);
                    self.editor.language = lang;
                    self.is_dirty = true;
                    self.undo_new_group = true;
                    return Command::perform(async move { new_line }, Message::RestoreCursorLine);
                }
                Command::none()
            }
            Message::ToggleComment => {
                if !self.show_search && !self.show_preferences {
                    self.push_undo();
                    let text = self.editor.content.text();
                    let (line_idx, _) = self.editor.content.cursor_position();
                    let prefix = comment_prefix(self.editor.language.as_deref());
                    let new_text = toggle_comment(&text, line_idx, prefix);
                    let lang = self.editor.language.clone();
                    self.editor.content = text_editor::Content::with_text(&new_text);
                    self.editor.language = lang;
                    self.is_dirty = true;
                    self.undo_new_group = true;
                    return Command::perform(async move { line_idx }, Message::RestoreCursorLine);
                }
                Command::none()
            }
            Message::DeleteLine => {
                if !self.show_search && !self.show_preferences {
                    self.push_undo();
                    let text = self.editor.content.text();
                    let (line_idx, _) = self.editor.content.cursor_position();
                    let (new_text, new_line) = delete_line_op(&text, line_idx);
                    let lang = self.editor.language.clone();
                    self.editor.content = text_editor::Content::with_text(&new_text);
                    self.editor.language = lang;
                    self.is_dirty = true;
                    self.undo_new_group = true;
                    return Command::perform(async move { new_line }, Message::RestoreCursorLine);
                }
                Command::none()
            }
            Message::IndentSelection => {
                if !self.show_search && !self.show_preferences {
                    if let Some(selected) = self.editor.content.selection() {
                        let indent = if self.config.use_spaces {
                            " ".repeat(self.config.tab_width)
                        } else {
                            "\t".to_string()
                        };
                        self.push_undo();
                        let new_sel = indent_text(&selected, &indent);
                        self.editor.content.perform(text_editor::Action::Edit(
                            text_editor::Edit::Paste(std::sync::Arc::new(new_sel)),
                        ));
                        self.is_dirty = true;
                        self.undo_new_group = true;
                    }
                }
                Command::none()
            }
            Message::DedentLine => {
                if !self.show_search && !self.show_preferences {
                    let indent = if self.config.use_spaces {
                        " ".repeat(self.config.tab_width)
                    } else {
                        "\t".to_string()
                    };
                    if let Some(selected) = self.editor.content.selection() {
                        self.push_undo();
                        let new_sel = dedent_text(&selected, &indent);
                        self.editor.content.perform(text_editor::Action::Edit(
                            text_editor::Edit::Paste(std::sync::Arc::new(new_sel)),
                        ));
                        self.is_dirty = true;
                        self.undo_new_group = true;
                    } else {
                        self.push_undo();
                        let text = self.editor.content.text();
                        let (line_idx, _) = self.editor.content.cursor_position();
                        let new_text = dedent_line(&text, line_idx, &indent);
                        let lang = self.editor.language.clone();
                        self.editor.content = text_editor::Content::with_text(&new_text);
                        self.editor.language = lang;
                        self.is_dirty = true;
                        self.undo_new_group = true;
                        return Command::perform(
                            async move { line_idx },
                            Message::RestoreCursorLine,
                        );
                    }
                }
                Command::none()
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
                if let Some(current) = self.current_file.clone() {
                    self.file_cache.insert(
                        current,
                        (
                            self.editor.content.text().to_string(),
                            self.editor.language.clone(),
                            self.is_dirty,
                        ),
                    );
                }
                self.undo_stack.clear();
                self.redo_stack.clear();
                self.undo_new_group = true;
                self.sidebar.add_file(path.clone());
                if let Some((content, language, dirty)) = self.file_cache.get(&path).cloned() {
                    self.editor = EditorState::from_content(&content);
                    self.editor.language = language;
                    self.current_file = Some(path);
                    self.is_dirty = dirty;
                    self.status_message = t!("status.file_opened").to_string();
                    Command::none()
                } else {
                    Command::perform(read_file(path), Message::FileOpened)
                }
            }
            Message::RestoreCursorLine(target) => {
                let max = self.editor.content.line_count().saturating_sub(1);
                let target = target.min(max);
                self.editor.content.perform(text_editor::Action::Move(
                    text_editor::Motion::DocumentStart,
                ));
                for _ in 0..target {
                    self.editor
                        .content
                        .perform(text_editor::Action::Move(text_editor::Motion::Down));
                }
                self.editor
                    .content
                    .perform(text_editor::Action::Move(text_editor::Motion::Home));
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
                crate::updater::restart();
                Command::none()
            }
            Message::UpdateInstalled(Err(e)) => {
                self.overlay = ActiveOverlay::UpdateError(e);
                Command::none()
            }
        }
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

        let autosave =
            iced::time::every(std::time::Duration::from_secs(5)).map(|_| Message::AutoSave);

        iced::Subscription::batch([keys, autosave])
    }

    fn view(&self) -> Element<'_, Message> {
        let dark = self.config.dark_mode;

        let menu_bar = crate::menu_bar::view(&self.config, self.open_menu);

        let editor_widget = self.editor.view(&self.config);
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
        let editor_with_context = iced_aw::ContextMenu::new(editor_widget, move || {
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
        });

        let search_panel = if self.show_search {
            Some(self.search.view(dark))
        } else {
            None
        };

        let editor_area: Element<Message> = if let Some(search) = search_panel {
            column![search, editor_with_context].into()
        } else {
            editor_with_context.into()
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
        let file_size = self
            .current_file
            .as_ref()
            .filter(|p| !is_untitled(p))
            .and_then(|p| std::fs::metadata(p).ok())
            .map(|m| m.len());
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
        let with_dropdown: Element<Message> = if let Some(menu) = self.open_menu {
            let has_fmt = self
                .editor
                .language
                .as_deref()
                .map(crate::formatter::has_formatter)
                .unwrap_or(false);
            let lang_override_enabled = self.language_picker_enabled();
            let dropdown =
                crate::menu_bar::dropdown_view(menu, &self.config, has_fmt, lang_override_enabled);
            let x = crate::menu_bar::dropdown_x_offset(menu);
            iced_aw::floating_element::FloatingElement::new(base, dropdown)
                .anchor(iced_aw::floating_element::Anchor::NorthWest)
                .offset(iced_aw::floating_element::Offset {
                    x,
                    y: crate::menu_bar::BAR_HEIGHT,
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

    fn view_goto_line_overlay(&self, dark: bool) -> Element<'_, Message> {
        let p = se_theme::palette(dark);
        container(
            column![
                text(t!("goto_line.title").to_string())
                    .size(14)
                    .style(p.text),
                text_input(t!("goto_line.placeholder").as_ref(), &self.goto_line_input)
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

    // ─── Undo / redo ────────────────────────────────────────────────────────

    fn push_undo(&mut self) {
        let snapshot = self.editor.content.text().to_string();
        if self.undo_stack.len() >= 100 {
            self.undo_stack.remove(0);
        }
        self.undo_stack.push(snapshot);
        self.redo_stack.clear();
    }

    // ─── Session persistence ─────────────────────────────────────────────────

    fn save_session(&self) {
        use crate::session::{FileSession, Session};
        let files: Vec<FileSession> = self
            .sidebar
            .files()
            .iter()
            .map(|path| {
                let (content, language, dirty) = if self.current_file.as_ref() == Some(path) {
                    (
                        self.editor.content.text().to_string(),
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

        Session {
            files,
            active_file: self
                .current_file
                .as_ref()
                .and_then(|p| p.to_str())
                .map(|s| s.to_string()),
            untitled_counter: self.untitled_counter,
        }
        .save();
    }

    fn restore_session(&mut self, session: crate::session::Session) {
        if session.files.is_empty() {
            // First launch: start with a blank untitled file
            let path = untitled_path(1);
            self.sidebar.add_file(path.clone());
            self.current_file = Some(path);
            self.untitled_counter = 1;
            return;
        }

        self.untitled_counter = session.untitled_counter.max(1);

        for fs in &session.files {
            let path = PathBuf::from(&fs.path);
            self.sidebar.add_file(path.clone());

            let content = if let Some(c) = &fs.content {
                c.clone()
            } else if !is_untitled(&path) {
                std::fs::read_to_string(&path).unwrap_or_default()
            } else {
                String::new()
            };
            self.file_cache
                .insert(path, (content, fs.language.clone(), fs.dirty));
        }

        // Activate the previously active file
        let active_path = session
            .active_file
            .as_deref()
            .or_else(|| session.files.first().map(|f| f.path.as_str()))
            .map(PathBuf::from);

        if let Some(path) = active_path {
            if let Some((content, language, dirty)) = self.file_cache.remove(&path) {
                self.editor = EditorState::from_content(&content);
                self.editor.language = language;
                self.is_dirty = dirty;
                self.current_file = Some(path);
            }
        }
    }
}

// ─── Text manipulation helpers ──────────────────────────────────────────────

pub fn comment_prefix(language: Option<&str>) -> &'static str {
    match language {
        Some("rs") | Some("js") | Some("ts") | Some("jsx") | Some("tsx") | Some("c")
        | Some("cpp") | Some("java") | Some("go") | Some("swift") | Some("kt") => "// ",
        Some("py") | Some("rb") | Some("sh") | Some("r") | Some("toml") | Some("yaml") => "# ",
        Some("sql") | Some("lua") => "-- ",
        _ => "// ",
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

pub fn toggle_comment(text: &str, line_idx: usize, prefix: &str) -> String {
    let mut lines: Vec<String> = text.split('\n').map(|l| l.to_string()).collect();
    if line_idx >= lines.len() {
        return text.to_string();
    }
    let line = lines[line_idx].clone();
    let leading_ws: String = line.chars().take_while(|c| c.is_whitespace()).collect();
    let content = line.trim_start();
    if content.starts_with(prefix) {
        lines[line_idx] = format!(
            "{}{}",
            leading_ws,
            content.strip_prefix(prefix).unwrap_or(content)
        );
    } else {
        lines[line_idx] = format!("{}{}{}", leading_ws, prefix, content);
    }
    lines.join("\n")
}

pub fn indent_text(text: &str, indent: &str) -> String {
    text.split('\n')
        .map(|line| {
            if line.is_empty() {
                line.to_string()
            } else {
                format!("{}{}", indent, line)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn dedent_text(text: &str, indent: &str) -> String {
    text.split('\n')
        .map(|line| {
            if let Some(stripped) = line.strip_prefix(indent) {
                stripped.to_string()
            } else if line.starts_with('\t') {
                line.strip_prefix('\t').unwrap_or("").to_string()
            } else {
                let n = line
                    .chars()
                    .take(indent.len())
                    .take_while(|c| *c == ' ')
                    .count();
                line[n..].to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn dedent_line(text: &str, line_idx: usize, indent: &str) -> String {
    let mut lines: Vec<String> = text.split('\n').map(|l| l.to_string()).collect();
    if line_idx >= lines.len() {
        return text.to_string();
    }
    let line = lines[line_idx].clone();
    lines[line_idx] = if let Some(stripped) = line.strip_prefix(indent) {
        stripped.to_string()
    } else if line.starts_with('\t') {
        line.strip_prefix('\t').unwrap_or("").to_string()
    } else {
        let n = line
            .chars()
            .take(indent.len())
            .take_while(|c| *c == ' ')
            .count();
        line[n..].to_string()
    };
    lines.join("\n")
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

async fn open_file() -> Result<(PathBuf, String), String> {
    let handle = rfd::AsyncFileDialog::new()
        .set_title("Open File")
        .add_filter(
            "All supported files",
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
        .add_filter("Text files", &["txt", "md", "rst", "log"])
        .add_filter(
            "Source code",
            &[
                "rs", "py", "js", "ts", "jsx", "tsx", "c", "h", "cpp", "java", "go", "rb", "php",
                "swift", "kt", "lua", "sql",
            ],
        )
        .add_filter(
            "Web files",
            &["html", "htm", "css", "scss", "json", "xml", "svg"],
        )
        .add_filter(
            "Config files",
            &["toml", "yaml", "yml", "ini", "cfg", "conf", "env"],
        )
        .add_filter("All files", &["*"])
        .pick_file()
        .await
        .ok_or_else(|| "cancelled".to_string())?;

    let path = handle.path().to_path_buf();
    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    Ok((path, content))
}

async fn read_file(path: PathBuf) -> Result<(PathBuf, String), String> {
    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    Ok((path, content))
}

async fn save_file(path: PathBuf, content: String) -> Result<PathBuf, String> {
    std::fs::write(&path, &content).map_err(|e| e.to_string())?;
    Ok(path)
}

async fn save_file_as(content: String) -> Result<PathBuf, String> {
    let handle = rfd::AsyncFileDialog::new()
        .set_title("Save File As")
        .save_file()
        .await
        .ok_or_else(|| "cancelled".to_string())?;

    let path = handle.path().to_path_buf();
    std::fs::write(&path, &content).map_err(|e| e.to_string())?;
    Ok(path)
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
    fn toggle_comment_add() {
        let text = "fn main() {}";
        let result = toggle_comment(text, 0, "// ");
        assert_eq!(result, "// fn main() {}");
    }

    #[test]
    fn toggle_comment_remove() {
        let text = "// fn main() {}";
        let result = toggle_comment(text, 0, "// ");
        assert_eq!(result, "fn main() {}");
    }

    #[test]
    fn toggle_comment_preserves_indent() {
        let text = "    let x = 1;";
        let result = toggle_comment(text, 0, "// ");
        assert_eq!(result, "    // let x = 1;");
    }

    #[test]
    fn indent_text_basic() {
        let text = "line1\nline2";
        let result = indent_text(text, "    ");
        assert_eq!(result, "    line1\n    line2");
    }

    #[test]
    fn dedent_text_basic() {
        let text = "    line1\n    line2";
        let result = dedent_text(text, "    ");
        assert_eq!(result, "line1\nline2");
    }

    #[test]
    fn dedent_text_partial() {
        let text = "  line1\nline2";
        let result = dedent_text(text, "    ");
        assert_eq!(result, "line1\nline2");
    }

    #[test]
    fn comment_prefix_by_language() {
        assert_eq!(comment_prefix(Some("rs")), "// ");
        assert_eq!(comment_prefix(Some("py")), "# ");
        assert_eq!(comment_prefix(Some("sql")), "-- ");
        assert_eq!(comment_prefix(None), "// ");
    }
}
