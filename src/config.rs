use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Identifies which action is being rebound in the shortcuts config panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortcutTarget {
    NewFile,
    OpenFile,
    SaveFile,
    SaveAs,
    CloseFile,
    Find,
    SelectAll,
    FormatCode,
    GotoLine,
    CommandPalette,
    ToggleSidebar,
    Quit,
    Undo,
    Redo,
    DuplicateLine,
    MoveLineUp,
    MoveLineDown,
    ToggleComment,
    DeleteLine,
    ToggleBookmark,
    NextBookmark,
    PrevBookmark,
    ToggleFold,
    NavBack,
    NavForward,
    ToggleTerminal,
    ToggleGit,
    GitCommit,
    GitPush,
    GitPull,
    GitBranches,
    BookmarkPanel,
    GotoFile,
    FindInFiles,
    ReplaceInFiles,
    ReopenTab,
    Complete,
    ParameterInfo,
    GotoClass,
    GotoSymbol,
    RunTask,
    RenameSymbol,
    AddNextOccurrence,
    SpellFix,
    SelectAllOccurrences,
    AddCursorAbove,
    AddCursorBelow,
}

/// How a key is named to the user.
pub fn key_label(key: &str) -> String {
    match key {
        "Return" => t!("key.enter").to_string(),
        "ArrowUp" => t!("key.up").to_string(),
        "ArrowDown" => t!("key.down").to_string(),
        "ArrowLeft" => t!("key.left").to_string(),
        "ArrowRight" => t!("key.right").to_string(),
        "PageUp" => t!("key.page_up").to_string(),
        "PageDown" => t!("key.page_down").to_string(),
        "Delete" => t!("key.delete").to_string(),
        "Backspace" => t!("key.backspace").to_string(),
        "Space" => t!("key.space").to_string(),
        "Tab" => t!("key.tab").to_string(),
        k if k.chars().count() == 1 => k.to_uppercase(),
        k => k.to_string(),
    }
}

/// A single keyboard binding stored as display-friendly strings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShortcutConfig {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    /// Single character ("n", "s") or named key ("Return", "F1", "ArrowUp").
    pub key: String,
}

impl ShortcutConfig {
    fn ctrl(key: &str) -> Self {
        Self {
            ctrl: true,
            shift: false,
            alt: false,
            key: key.to_string(),
        }
    }
    fn ctrl_shift(key: &str) -> Self {
        Self {
            ctrl: true,
            shift: true,
            alt: false,
            key: key.to_string(),
        }
    }
    fn ctrl_alt(key: &str) -> Self {
        Self {
            ctrl: true,
            shift: false,
            alt: true,
            key: key.to_string(),
        }
    }
    fn ctrl_alt_shift(key: &str) -> Self {
        Self {
            ctrl: true,
            shift: true,
            alt: true,
            key: key.to_string(),
        }
    }
    fn alt_shift(key: &str) -> Self {
        Self {
            ctrl: false,
            shift: true,
            alt: true,
            key: key.to_string(),
        }
    }
    fn plain(key: &str) -> Self {
        Self {
            ctrl: false,
            shift: false,
            alt: false,
            key: key.to_string(),
        }
    }
    fn shift(key: &str) -> Self {
        Self {
            ctrl: false,
            shift: true,
            alt: false,
            key: key.to_string(),
        }
    }
    fn alt(key: &str) -> Self {
        Self {
            ctrl: false,
            shift: false,
            alt: true,
            key: key.to_string(),
        }
    }

    /// How the binding is shown: `Ctrl+Shift+S` (or `Cmd`-style glyphs on
    /// macOS, in the platform's order: ⌥⇧⌘S).
    pub fn display(&self) -> String {
        if cfg!(target_os = "macos") {
            return self.display_mac();
        }
        let mut s = String::new();
        if self.ctrl {
            s.push_str(&t!("key.ctrl"));
            s.push('+');
        }
        if self.shift {
            s.push_str(&t!("key.shift"));
            s.push('+');
        }
        if self.alt {
            s.push_str(&t!("key.alt"));
            s.push('+');
        }
        s.push_str(&key_label(&self.key));
        s
    }

    fn display_mac(&self) -> String {
        let mut s = String::new();
        if self.alt {
            s.push('⌥');
        }
        if self.shift {
            s.push('⇧');
        }
        if self.ctrl {
            s.push('⌘');
        }
        s.push_str(&match self.key.as_str() {
            "ArrowUp" => "↑".to_string(),
            "ArrowDown" => "↓".to_string(),
            "ArrowLeft" => "←".to_string(),
            "ArrowRight" => "→".to_string(),
            "Return" => "↩".to_string(),
            k if k.len() == 1 => k.to_uppercase(),
            k => k.to_string(),
        });
        s
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Shortcuts {
    pub new_file: ShortcutConfig,
    pub open_file: ShortcutConfig,
    pub save_file: ShortcutConfig,
    pub save_as: ShortcutConfig,
    pub close_file: ShortcutConfig,
    pub find: ShortcutConfig,
    pub select_all: ShortcutConfig,
    pub format_code: ShortcutConfig,
    pub goto_line: ShortcutConfig,
    pub command_palette: ShortcutConfig,
    pub toggle_sidebar: ShortcutConfig,
    pub quit: ShortcutConfig,
    pub undo: ShortcutConfig,
    pub redo: ShortcutConfig,
    pub duplicate_line: ShortcutConfig,
    pub move_line_up: ShortcutConfig,
    pub move_line_down: ShortcutConfig,
    pub toggle_comment: ShortcutConfig,
    pub delete_line: ShortcutConfig,
    pub toggle_bookmark: ShortcutConfig,
    pub next_bookmark: ShortcutConfig,
    pub prev_bookmark: ShortcutConfig,
    pub toggle_fold: ShortcutConfig,
    pub nav_back: ShortcutConfig,
    pub nav_forward: ShortcutConfig,
    pub toggle_terminal: ShortcutConfig,
    pub toggle_git: ShortcutConfig,
    pub git_commit: ShortcutConfig,
    pub git_push: ShortcutConfig,
    pub git_pull: ShortcutConfig,
    pub git_branches: ShortcutConfig,
    pub bookmark_panel: ShortcutConfig,
    pub goto_file: ShortcutConfig,
    pub find_in_files: ShortcutConfig,
    pub replace_in_files: ShortcutConfig,
    pub reopen_tab: ShortcutConfig,
    /// Opens the list of completions where the cursor is.
    pub complete: ShortcutConfig,
    /// Shows the parameters of the call at the cursor.
    pub parameter_info: ShortcutConfig,
    /// Search everywhere, on its Classes tab.
    pub goto_class: ShortcutConfig,
    /// Search everywhere, on its Symbols tab.
    pub goto_symbol: ShortcutConfig,
    pub run_task: ShortcutConfig,
    pub rename_symbol: ShortcutConfig,
    pub add_next_occurrence: ShortcutConfig,
    pub spell_fix: ShortcutConfig,
    pub select_all_occurrences: ShortcutConfig,
    pub add_cursor_above: ShortcutConfig,
    pub add_cursor_below: ShortcutConfig,
}

/// Back and forward, as the IDEs do: Ctrl+Alt+←/→ on Windows, ⌘[ / ⌘] on
/// macOS. On Linux desktops Ctrl+Alt+arrows switch workspace, so Alt+←/→.
fn default_nav_back() -> ShortcutConfig {
    if cfg!(target_os = "macos") {
        ShortcutConfig::ctrl("[")
    } else if cfg!(windows) {
        ShortcutConfig::ctrl_alt("ArrowLeft")
    } else {
        ShortcutConfig::alt("ArrowLeft")
    }
}
fn default_nav_forward() -> ShortcutConfig {
    if cfg!(target_os = "macos") {
        ShortcutConfig::ctrl("]")
    } else if cfg!(windows) {
        ShortcutConfig::ctrl_alt("ArrowRight")
    } else {
        ShortcutConfig::alt("ArrowRight")
    }
}
/// Ctrl+Space, as the IDEs; on macOS ⌘Space is Spotlight's.
fn default_complete() -> ShortcutConfig {
    if cfg!(target_os = "macos") {
        ShortcutConfig::ctrl_shift("Space")
    } else {
        ShortcutConfig::ctrl("Space")
    }
}
fn default_duplicate_line() -> ShortcutConfig {
    ShortcutConfig::ctrl("d")
}
fn default_move_line_up() -> ShortcutConfig {
    ShortcutConfig::alt_shift("ArrowUp")
}
fn default_move_line_down() -> ShortcutConfig {
    ShortcutConfig::alt_shift("ArrowDown")
}
fn default_toggle_comment() -> ShortcutConfig {
    ShortcutConfig::ctrl("/")
}
fn default_delete_line() -> ShortcutConfig {
    ShortcutConfig::ctrl("y")
}

/// The one table tying each action to its field: it generates both accessors
/// and the list of all targets, so a new shortcut is added in a single place.
macro_rules! shortcut_slots {
    ($($target:ident => $field:ident,)*) => {
        impl Shortcuts {
            /// The binding for `target`.
            pub fn slot_mut(&mut self, target: ShortcutTarget) -> &mut ShortcutConfig {
                match target { $(ShortcutTarget::$target => &mut self.$field,)* }
            }

            /// The binding for `target`, read only.
            pub fn slot(&self, target: ShortcutTarget) -> &ShortcutConfig {
                match target { $(ShortcutTarget::$target => &self.$field,)* }
            }
        }

        const ALL_TARGETS: &[ShortcutTarget] = &[$(ShortcutTarget::$target,)*];
    };
}

shortcut_slots! {
    NewFile => new_file,
    OpenFile => open_file,
    SaveFile => save_file,
    SaveAs => save_as,
    CloseFile => close_file,
    Find => find,
    SelectAll => select_all,
    FormatCode => format_code,
    GotoLine => goto_line,
    CommandPalette => command_palette,
    ToggleSidebar => toggle_sidebar,
    Quit => quit,
    Undo => undo,
    Redo => redo,
    DuplicateLine => duplicate_line,
    MoveLineUp => move_line_up,
    MoveLineDown => move_line_down,
    ToggleComment => toggle_comment,
    DeleteLine => delete_line,
    ToggleBookmark => toggle_bookmark,
    NextBookmark => next_bookmark,
    PrevBookmark => prev_bookmark,
    ToggleFold => toggle_fold,
    NavBack => nav_back,
    NavForward => nav_forward,
    ToggleTerminal => toggle_terminal,
    ToggleGit => toggle_git,
    GitCommit => git_commit,
    GitPush => git_push,
    GitPull => git_pull,
    GitBranches => git_branches,
    BookmarkPanel => bookmark_panel,
    GotoFile => goto_file,
    FindInFiles => find_in_files,
    ReplaceInFiles => replace_in_files,
    ReopenTab => reopen_tab,
    Complete => complete,
    ParameterInfo => parameter_info,
    GotoClass => goto_class,
    GotoSymbol => goto_symbol,
    RunTask => run_task,
    RenameSymbol => rename_symbol,
    AddNextOccurrence => add_next_occurrence,
    SpellFix => spell_fix,
    SelectAllOccurrences => select_all_occurrences,
    AddCursorAbove => add_cursor_above,
    AddCursorBelow => add_cursor_below,
}

impl Shortcuts {
    /// Whether an action listed before `target` has the same binding.
    fn conflicts_before(&self, target: ShortcutTarget, sc: &ShortcutConfig) -> bool {
        ALL_TARGETS
            .iter()
            .take_while(|t| **t != target)
            .any(|t| self.slot(*t) == sc)
    }

    /// Whether some action other than `target` is already bound to `sc`.
    pub fn conflicts(&self, target: ShortcutTarget, sc: &ShortcutConfig) -> bool {
        ALL_TARGETS
            .iter()
            .filter(|t| **t != target)
            .any(|t| self.slot(*t) == sc)
    }
}

/// `F1`..`F12`: keys that need no modifier to be a shortcut.
pub fn is_function_key(key: &str) -> bool {
    key.len() > 1 && key.starts_with('F') && key[1..].bytes().all(|b| b.is_ascii_digit())
}

impl Default for Shortcuts {
    fn default() -> Self {
        Self {
            new_file: ShortcutConfig::ctrl("n"),
            open_file: ShortcutConfig::ctrl("o"),
            save_file: ShortcutConfig::ctrl("s"),
            save_as: ShortcutConfig::ctrl_shift("s"),
            close_file: ShortcutConfig::ctrl("F4"),
            find: ShortcutConfig::ctrl("f"),
            select_all: ShortcutConfig::ctrl("a"),
            format_code: ShortcutConfig::ctrl_alt("l"),
            goto_line: ShortcutConfig::ctrl("g"),
            command_palette: ShortcutConfig::ctrl_shift("a"),
            toggle_sidebar: ShortcutConfig::ctrl("b"),
            quit: ShortcutConfig::ctrl("q"),
            undo: ShortcutConfig::ctrl("z"),
            redo: ShortcutConfig::ctrl_shift("z"),
            duplicate_line: default_duplicate_line(),
            move_line_up: default_move_line_up(),
            move_line_down: default_move_line_down(),
            toggle_comment: default_toggle_comment(),
            delete_line: default_delete_line(),
            toggle_bookmark: ShortcutConfig::plain("F11"),
            next_bookmark: ShortcutConfig::plain("F2"),
            prev_bookmark: ShortcutConfig::shift("F2"),
            toggle_fold: ShortcutConfig::ctrl("."),
            nav_back: default_nav_back(),
            nav_forward: default_nav_forward(),
            toggle_terminal: ShortcutConfig::alt("F12"),
            toggle_git: ShortcutConfig::alt("g"),
            git_commit: ShortcutConfig::ctrl("k"),
            git_push: ShortcutConfig::ctrl_shift("k"),
            git_pull: ShortcutConfig::ctrl("t"),
            git_branches: ShortcutConfig::ctrl_shift("b"),
            bookmark_panel: ShortcutConfig::shift("F11"),
            goto_file: ShortcutConfig::ctrl_shift("n"),
            find_in_files: ShortcutConfig::ctrl_shift("f"),
            replace_in_files: ShortcutConfig::ctrl_shift("r"),
            reopen_tab: ShortcutConfig::ctrl_shift("t"),
            complete: default_complete(),
            parameter_info: ShortcutConfig::ctrl("p"),
            // IntelliJ's Ctrl+N is New file here.
            goto_class: ShortcutConfig::ctrl_alt("n"),
            goto_symbol: ShortcutConfig::ctrl_alt_shift("n"),
            run_task: ShortcutConfig::ctrl_alt("r"),
            rename_symbol: ShortcutConfig::shift("F6"),
            add_next_occurrence: ShortcutConfig::alt("j"),
            spell_fix: ShortcutConfig::ctrl_alt("s"),
            select_all_occurrences: ShortcutConfig::ctrl_alt_shift("j"),
            add_cursor_above: ShortcutConfig::ctrl_alt("ArrowUp"),
            add_cursor_below: ShortcutConfig::ctrl_alt("ArrowDown"),
        }
    }
}

impl Shortcuts {
    /// What these were before they followed the IDEs' keymap (version 1).
    fn before_the_ide_keymap() -> Shortcuts {
        Shortcuts {
            close_file: ShortcutConfig::ctrl("w"),
            format_code: ShortcutConfig::ctrl_shift("f"),
            command_palette: ShortcutConfig::ctrl_shift("p"),
            redo: ShortcutConfig::ctrl("y"),
            duplicate_line: ShortcutConfig::ctrl_shift("d"),
            delete_line: ShortcutConfig::ctrl_shift("k"),
            move_line_up: ShortcutConfig::alt("ArrowUp"),
            move_line_down: ShortcutConfig::alt("ArrowDown"),
            toggle_bookmark: ShortcutConfig::ctrl("F2"),
            toggle_fold: ShortcutConfig::ctrl_shift("l"),
            nav_back: if cfg!(target_os = "macos") {
                ShortcutConfig::ctrl("-")
            } else {
                ShortcutConfig::alt("ArrowLeft")
            },
            nav_forward: if cfg!(target_os = "macos") {
                ShortcutConfig::ctrl_shift("-")
            } else {
                ShortcutConfig::alt("ArrowRight")
            },
            ..Shortcuts::default()
        }
    }

    /// A config saved before the new keymap: every shortcut still at its old
    /// default moves to the new one; the ones the user chose are left alone.
    pub fn adopt_the_ide_keymap(&mut self) {
        let old = Shortcuts::before_the_ide_keymap();
        let new = Shortcuts::default();
        for target in ALL_TARGETS {
            if self.slot(*target) == old.slot(*target) {
                *self.slot_mut(*target) = new.slot(*target).clone();
            }
        }
    }
}

/// The keymap the defaults are from, and what a config without the number is.
const KEYMAP_VERSION: u32 = 2;
fn first_keymap() -> u32 {
    1
}

/// A snippet the user wrote.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct UserSnippet {
    pub trigger: String,
    pub body: String,
    /// The file extensions it is for (`rs`, `py`…); empty: every file.
    #[serde(default)]
    pub languages: Vec<String>,
}

/// Every field has a default, so a config written by an older version (or
/// one with extra legacy keys, which are ignored) still loads.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub dark_mode: bool,
    /// Editor text size, in pixels.
    pub font_size: f32,
    pub tab_width: usize,
    pub use_spaces: bool,
    /// Each file is indented as it already is.
    pub detect_indent: bool,
    pub show_line_numbers: bool,
    pub auto_indent: bool,
    pub autocomplete_brackets: bool,
    pub autocomplete_quotes: bool,
    /// Wash over the line the cursor is on.
    pub highlight_current_line: bool,
    /// A vertical guide at `page_guide_column`.
    pub show_page_guide: bool,
    /// Black or white, bright accents, strong borders.
    pub high_contrast: bool,
    /// The machines opened over SSH, the last first.
    pub remote_hosts: Vec<String>,
    /// Misspelled words are underlined (prose, and the comments of programs).
    pub spell_check: bool,
    /// The dictionary: `fr`, `en_US`… (empty: the language of the interface).
    pub spell_language: String,
    /// Files are formatted when they are saved.
    pub format_on_save: bool,
    /// Vertical lines at each indent step.
    pub show_indent_guides: bool,
    /// Dots for spaces and a line for tabs.
    pub show_whitespace: bool,
    /// A box around the bracket at the cursor and its mate.
    pub match_brackets: bool,
    /// The other places of the word under the cursor are marked.
    pub highlight_word: bool,
    /// The user's snippets: a word that Ctrl+Space (or the list of a language
    /// server) offers, and the text it becomes (`${1:place}` and `$0` as in
    /// the language server protocol). They are written in the config file.
    pub snippets: Vec<UserSnippet>,
    pub page_guide_column: usize,
    /// A miniature of the whole file along the editor's right edge.
    pub show_minimap: bool,
    /// Modified files are written to disk by themselves, a moment after the last edit.
    pub auto_save: bool,
    /// Long lines wrap at the edge; off, they run past it (faster on huge lines).
    pub word_wrap: bool,
    pub locale: String,
    /// Whether the file list is shown.
    pub show_sidebar: bool,
    /// Which panels the bar beside the sidebar has open.
    pub show_files_panel: bool,
    pub show_projects_panel: bool,
    pub show_database_panel: bool,
    pub show_api_panel: bool,
    /// Width of the file list, in pixels (dragged by its right edge).
    pub sidebar_width: f32,
    /// How `pull` and `merge` are made in the Git panel.
    pub git_pull_mode: crate::git::PullMode,
    pub git_merge_mode: crate::git::MergeMode,
    /// Width of the actions of the Git panel's Branches tab, in pixels.
    pub git_actions_width: f32,
    /// Minutes between two fetches in the background (0: never).
    pub git_auto_fetch_minutes: u32,
    /// Whether problems are looked for in the files (linters and language servers).
    pub lint_enabled: bool,
    /// Whether they are underlined in the editor.
    pub lint_highlight: bool,
    /// The languages (groups of `lint::tools`) the user turned the lint on
    /// for: none to begin with.
    pub lint_languages: std::collections::BTreeSet<String>,
    /// Which problems "fix them all" fixes: errors only to begin with.
    pub lint_fix_level: crate::lint::Level,
    /// How each severity is marked in the text (`error`, `warning`, `info`,
    /// `hint`).
    pub lint_markings: std::collections::BTreeMap<String, crate::lint::Marking>,
    /// The color of each severity (`error`, `warning`, `info`, `hint`) as
    /// `#rrggbb`; the theme's own when missing.
    pub lint_colors: std::collections::BTreeMap<String, String>,
    /// A first look when a file is opened (and when the editor starts).
    pub lint_on_open: bool,
    /// Completion from the language servers.
    pub completion_enabled: bool,
    /// The list opens by itself while typing (else with the shortcut only).
    pub completion_auto: bool,
    /// What each language's server proposes (`rust.postfix`, …); a missing
    /// one is on.
    pub completion_options: std::collections::BTreeMap<String, bool>,
    /// Search everywhere: text results at the end of "All" when few others.
    pub everywhere_text: bool,
    /// Search everywhere: the Git tab (branches and commits).
    pub everywhere_git: bool,
    /// Abbreviations of actions, by action (search everywhere).
    pub action_abbreviations: std::collections::BTreeMap<String, Vec<String>>,
    /// Searches and replacements made in files, latest first (Alt+↓).
    /// Working days the local history goes back (0: it is off).
    pub local_history_days: u32,
    /// The color of each project, by its folder.
    pub project_colors: std::collections::BTreeMap<String, usize>,
    /// Find in files: the files Git ignores one by one too.
    pub find_ignored: bool,
    pub find_history: Vec<String>,
    /// Folders whose own programs (`node_modules/.bin`…) may run by themselves.
    pub trusted_folders: Vec<String>,
    pub replace_history: Vec<String>,
    pub lint_trigger: crate::lint::Trigger,
    pub lint_level: crate::lint::Level,
    /// Fixes are applied when a file is saved.
    pub lint_fix_on_save: bool,
    /// The tools chosen by hand for a language (a group of `lint::tools`):
    /// none listed means whatever is ready by itself.
    pub lint_tools: std::collections::BTreeMap<String, Vec<String>>,
    /// The path of a tool chosen by the user (by tool id): it wins over the
    /// search in the project and the system.
    pub lint_programs: std::collections::BTreeMap<String, String>,
    /// Size of the window when it was last closed or resized.
    pub window_width: f32,
    pub window_height: f32,
    /// The version that last ran, to show what's new after an update.
    pub last_version: String,
    pub shortcuts: Shortcuts,
    /// Which default keymap `shortcuts` was written under (1: before the IDEs').
    #[serde(default = "first_keymap")]
    pub shortcuts_version: u32,
    /// Name of the copy kept of a config file whose unreadable settings were
    /// reset (never saved).
    #[serde(skip)]
    pub set_aside: Option<String>,
    pub recent_files: Vec<String>,
    /// The folders added to the sidebar as projects, and those of them that
    /// are open in the tree.
    pub projects: Vec<std::path::PathBuf>,
    pub projects_open: Vec<std::path::PathBuf>,
    /// What Git ignores is left out of the trees.
    pub projects_hide_ignored: bool,
    /// Names left out of the trees and of the finders: `*` for anything, a
    /// trailing `/` for folders only.
    pub projects_exclude: Vec<String>,
    /// Where scratch files are kept; empty: the default folder.
    pub scratch_dir: String,
    /// The API client: a file of certificate authorities (PEM) to trust
    /// besides the system's, a client certificate with its key (PEM) for
    /// mutual TLS, and a proxy address (empty: the system's).
    pub api_ca_file: String,
    pub api_client_cert: String,
    pub api_proxy: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            dark_mode: true,
            font_size: 14.0,
            tab_width: 4,
            use_spaces: true,
            detect_indent: true,
            show_line_numbers: true,
            auto_indent: true,
            autocomplete_brackets: true,
            autocomplete_quotes: true,
            highlight_current_line: true,
            show_page_guide: false,
            high_contrast: false,
            remote_hosts: Vec::new(),
            spell_check: true,
            spell_language: String::new(),
            format_on_save: false,
            show_indent_guides: true,
            show_whitespace: false,
            match_brackets: true,
            highlight_word: true,
            snippets: Vec::new(),
            page_guide_column: 80,
            show_minimap: true,
            word_wrap: true,
            auto_save: true,
            locale: "en".to_string(),
            show_sidebar: true,
            show_files_panel: true,
            show_projects_panel: false,
            show_database_panel: false,
            show_api_panel: false,
            sidebar_width: 208.0,
            git_pull_mode: crate::git::PullMode::default(),
            git_merge_mode: crate::git::MergeMode::default(),
            git_actions_width: crate::git_panel::branches::ACTIONS_DEFAULT,
            git_auto_fetch_minutes: 5,
            lint_enabled: true,
            lint_highlight: true,
            lint_languages: std::collections::BTreeSet::new(),
            lint_fix_level: crate::lint::Level::Errors,
            lint_markings: std::collections::BTreeMap::new(),
            lint_colors: std::collections::BTreeMap::new(),
            lint_on_open: true,
            completion_enabled: true,
            completion_auto: true,
            completion_options: std::collections::BTreeMap::new(),
            everywhere_text: true,
            everywhere_git: false,
            action_abbreviations: std::collections::BTreeMap::new(),
            local_history_days: 5,
            project_colors: std::collections::BTreeMap::new(),
            find_ignored: true,
            find_history: Vec::new(),
            trusted_folders: Vec::new(),
            replace_history: Vec::new(),
            lint_trigger: crate::lint::Trigger::default(),
            lint_level: crate::lint::Level::default(),
            lint_fix_on_save: false,
            lint_tools: std::collections::BTreeMap::new(),
            lint_programs: std::collections::BTreeMap::new(),
            window_width: 1200.0,
            window_height: 800.0,
            last_version: String::new(),
            shortcuts: Shortcuts::default(),
            shortcuts_version: KEYMAP_VERSION,
            set_aside: None,
            recent_files: Vec::new(),
            projects: Vec::new(),
            projects_open: Vec::new(),
            projects_hide_ignored: false,
            projects_exclude: Vec::new(),
            scratch_dir: String::new(),
            api_ca_file: String::new(),
            api_client_cert: String::new(),
            api_proxy: String::new(),
        }
    }
}

/// The desktop portal's `color-scheme` (1 = dark, 2 = light, 0 = none),
/// read from `gdbus`'s output such as `(<<uint32 1>>,)`.
#[cfg(any(target_os = "linux", test))]
fn parse_portal_color_scheme(output: &str) -> Option<bool> {
    let value = output.split("uint32").nth(1)?.trim_start();
    match value.chars().next()? {
        '1' => Some(true),
        '0' | '2' => Some(false),
        _ => None,
    }
}

fn detect_system_dark_mode() -> bool {
    #[cfg(target_os = "linux")]
    {
        // The portal covers every desktop (KDE, GNOME, …).
        if let Ok(out) = std::process::Command::new("gdbus")
            .args([
                "call",
                "--session",
                "--dest",
                "org.freedesktop.portal.Desktop",
                "--object-path",
                "/org/freedesktop/portal/desktop",
                "--method",
                "org.freedesktop.portal.Settings.Read",
                "org.freedesktop.appearance",
                "color-scheme",
            ])
            .output()
        {
            if out.status.success() {
                if let Some(dark) = parse_portal_color_scheme(&String::from_utf8_lossy(&out.stdout))
                {
                    return dark;
                }
            }
        }
        if let Ok(out) = std::process::Command::new("gsettings")
            .args(["get", "org.gnome.desktop.interface", "color-scheme"])
            .output()
        {
            // GNOME reports 'default' for "no preference", which is the light
            // look; only an explicit 'prefer-dark' means dark.
            let s = String::from_utf8_lossy(&out.stdout);
            if out.status.success() && !s.trim().is_empty() {
                return s.contains("prefer-dark");
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Ok(out) = std::process::Command::new("defaults")
            .args(["read", "-g", "AppleInterfaceStyle"])
            .output()
        {
            let s = String::from_utf8_lossy(&out.stdout);
            if !s.trim().eq_ignore_ascii_case("dark") {
                return false;
            }
        }
    }
    true
}

/// Best-effort UI language from the environment; English unless the system is French.
fn detect_system_locale() -> String {
    let lang = std::env::var("LC_ALL")
        .or_else(|_| std::env::var("LC_MESSAGES"))
        .or_else(|_| std::env::var("LANG"))
        .unwrap_or_default();

    #[cfg(target_os = "macos")]
    let lang = if lang.is_empty() {
        // GUI apps launched from Finder inherit no LANG.
        std::process::Command::new("defaults")
            .args(["read", "-g", "AppleLocale"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default()
    } else {
        lang
    };

    if lang.starts_with("fr") {
        "fr".to_string()
    } else {
        "en".to_string()
    }
}

impl Config {
    pub fn config_path() -> Option<PathBuf> {
        crate::fsutil::config_dir().map(|d| d.join("simpleedit").join("config.json"))
    }

    pub fn load() -> Self {
        if let Some(path) = Self::config_path() {
            if let Ok(s) = std::fs::read_to_string(&path) {
                match Self::parse(&s) {
                    Ok((mut cfg, dropped)) => {
                        if dropped {
                            // A newer version wrote values this one does not
                            // know: the rest is kept, the original too.
                            let bak = path.with_file_name(format!(
                                "config.json.bak-{}",
                                std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .map(|d| d.as_secs())
                                    .unwrap_or(0)
                            ));
                            if std::fs::copy(&path, &bak).is_ok() {
                                cfg.set_aside =
                                    bak.file_name().map(|n| n.to_string_lossy().into_owned());
                            }
                        }
                        cfg.sanitize();
                        if cfg.shortcuts_version < KEYMAP_VERSION {
                            cfg.shortcuts.adopt_the_ide_keymap();
                            cfg.shortcuts_version = KEYMAP_VERSION;
                        }
                        return cfg;
                    }
                    // Keep the unreadable file around instead of silently
                    // overwriting it with defaults on the next save.
                    Err(_) => {
                        let aside = crate::fsutil::set_aside(&path);
                        return Config {
                            dark_mode: detect_system_dark_mode(),
                            locale: detect_system_locale(),
                            set_aside: aside.and_then(|a| {
                                a.file_name().map(|n| n.to_string_lossy().into_owned())
                            }),
                            ..Default::default()
                        };
                    }
                }
            }
        }
        Config {
            dark_mode: detect_system_dark_mode(),
            locale: detect_system_locale(),
            ..Default::default()
        }
    }

    /// Reads a config, leaving out the settings that fail to parse (a value
    /// from a newer version) rather than all of them. The flag tells if any
    /// were left out.
    fn parse(s: &str) -> Result<(Config, bool), serde_json::Error> {
        match serde_json::from_str::<Config>(s) {
            Ok(cfg) => Ok((cfg, false)),
            Err(e) => {
                let serde_json::Value::Object(mut map) = serde_json::from_str(s)? else {
                    return Err(e);
                };
                let before = map.len();
                map.retain(|key, value| {
                    let one = serde_json::Map::from_iter([(key.clone(), value.clone())]);
                    serde_json::from_value::<Config>(serde_json::Value::Object(one)).is_ok()
                });
                let dropped = map.len() < before;
                let cfg = serde_json::from_value(serde_json::Value::Object(map))?;
                Ok((cfg, dropped))
            }
        }
    }

    /// Brings hand-edited values back into a range the app works with.
    pub fn sanitize(&mut self) {
        self.tab_width = self.tab_width.clamp(1, 16);
        self.font_size = if self.font_size.is_finite() {
            self.font_size.clamp(8.0, 32.0)
        } else {
            Config::default().font_size
        };
        self.page_guide_column = self.page_guide_column.clamp(1, 500);
        for size in [&mut self.window_width, &mut self.window_height] {
            *size = if size.is_finite() {
                size.clamp(600.0, 10_000.0)
            } else {
                800.0
            };
        }
        if !matches!(self.locale.as_str(), "en" | "fr") {
            self.locale = detect_system_locale();
        }
        // An unusable binding falls back to its default, and so does the
        // later of two actions sharing one.
        let defaults = Shortcuts::default();
        for &target in ALL_TARGETS {
            let usable = {
                let sc = self.shortcuts.slot(target);
                !sc.key.is_empty()
                    && !self.shortcuts.conflicts_before(target, sc)
                    && (sc.ctrl || sc.alt || is_function_key(&sc.key))
            };
            if !usable {
                *self.shortcuts.slot_mut(target) = defaults.slot(target).clone();
            }
        }
    }

    pub fn save(&self) {
        if let Some(path) = Self::config_path() {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(json) = serde_json::to_string_pretty(self) {
                let _ = crate::fsutil::write_atomic(&path, json.as_bytes(), true);
            }
        }
    }

    pub fn clear_recent(&mut self) {
        self.recent_files.clear();
    }

    /// Add a file to the recent files list (max 10, no duplicates, no untitled).
    pub fn add_recent(&mut self, path: &Path) {
        if path
            .to_str()
            .map(|s| s.starts_with("untitled://"))
            .unwrap_or(false)
        {
            return;
        }
        let s = path.to_string_lossy().to_string();
        self.recent_files.retain(|f| f != &s);
        self.recent_files.insert(0, s);
        self.recent_files.truncate(10);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_snippets_of_the_user_are_read_from_the_config_file() {
        let config: Config = serde_json::from_str(
            r#"{"snippets": [{"trigger": "fnmain", "body": "fn main() {}", "languages": ["rs"]},
                             {"trigger": "todo", "body": "// TODO: $0"}]}"#,
        )
        .unwrap();
        assert_eq!(config.snippets.len(), 2);
        assert_eq!(config.snippets[0].languages, vec!["rs".to_string()]);
        assert!(config.snippets[1].languages.is_empty());
        // Without any, the file of an older version still reads.
        assert!(serde_json::from_str::<Config>("{}")
            .unwrap()
            .snippets
            .is_empty());
    }

    use super::*;

    #[test]
    fn default_config_is_valid() {
        let config = Config::default();
        assert_eq!(config.tab_width, 4);
        assert!(config.use_spaces);
        assert_eq!(config.font_size, 14.0);
        assert_eq!(config.locale, "en");
    }

    #[test]
    fn config_serialization_roundtrip() {
        let config = Config::default();
        let json = serde_json::to_string(&config).unwrap();
        let restored: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(config.tab_width, restored.tab_width);
        assert_eq!(config.dark_mode, restored.dark_mode);
        assert_eq!(config.font_size, restored.font_size);
    }

    #[test]
    fn shortcut_display() {
        // (The wording of the modifiers follows the interface language.)
        let n = ShortcutConfig::ctrl("n").display();
        let s = ShortcutConfig::ctrl_shift("s").display();
        assert!(n.ends_with('N') && s.ends_with('S'));
        assert!(s.len() > n.len());
        if cfg!(target_os = "macos") {
            assert_eq!(ShortcutConfig::alt("ArrowUp").display(), "⌥↑");
        } else {
            assert!(!ShortcutConfig::alt("ArrowUp").display().contains("Arrow"));
        }
    }

    #[test]
    fn hand_edited_values_are_brought_back_in_range() {
        let mut cfg: Config = serde_json::from_str(
            r#"{"tab_width": 0, "font_size": -3, "page_guide_column": 0, "locale": "de",
                "window_width": 5, "shortcuts": {"undo": {"ctrl": false, "shift": false,
                "alt": false, "key": ""}}}"#,
        )
        .unwrap();
        cfg.sanitize();
        assert_eq!(cfg.tab_width, 1);
        assert_eq!(cfg.font_size, 8.0);
        assert_eq!(cfg.page_guide_column, 1);
        assert!(matches!(cfg.locale.as_str(), "en" | "fr"));
        assert_eq!(cfg.window_width, 600.0);
        assert_eq!(cfg.shortcuts.undo, ShortcutConfig::ctrl("z"));

        let mut cfg = Config {
            tab_width: 1_000_000_000,
            font_size: f32::NAN,
            ..Config::default()
        };
        cfg.sanitize();
        assert_eq!(cfg.tab_width, 16);
        assert_eq!(cfg.font_size, 14.0);
    }

    #[test]
    fn two_actions_on_one_key_keep_only_the_first() {
        let mut cfg = Config::default();
        cfg.shortcuts.redo = ShortcutConfig::ctrl("z");
        cfg.sanitize();
        assert_eq!(cfg.shortcuts.undo, ShortcutConfig::ctrl("z"));
        assert_eq!(cfg.shortcuts.redo, ShortcutConfig::ctrl_shift("z"));
    }

    #[test]
    fn the_portal_color_scheme_is_read() {
        assert_eq!(parse_portal_color_scheme("(<<uint32 1>>,)"), Some(true));
        assert_eq!(parse_portal_color_scheme("(<<uint32 2>>,)"), Some(false));
        assert_eq!(parse_portal_color_scheme("(<<uint32 0>>,)"), Some(false));
        assert_eq!(parse_portal_color_scheme("error"), None);
    }

    #[test]
    fn partial_and_legacy_configs_still_load() {
        let cfg: Config =
            serde_json::from_str(r#"{"dark_mode": false, "word_wrap": true, "font_family": "X"}"#)
                .unwrap();
        assert!(!cfg.dark_mode);
        assert_eq!(cfg.tab_width, 4);
        assert_eq!(cfg.shortcuts.undo, ShortcutConfig::ctrl("z"));
    }

    #[test]
    fn default_shortcuts_never_collide() {
        let sh = Shortcuts::default();
        for &target in ALL_TARGETS {
            let own = sh.slot(target).clone();
            assert!(!sh.conflicts(target, &own), "{target:?} collides");
        }
        assert_eq!(sh.toggle_bookmark, ShortcutConfig::plain("F11"));
        assert_eq!(sh.next_bookmark, ShortcutConfig::plain("F2"));
        assert_eq!(sh.prev_bookmark, ShortcutConfig::shift("F2"));
        assert_eq!(sh.toggle_fold, ShortcutConfig::ctrl("."));
    }

    #[test]
    fn shortcut_conflicts_are_detected() {
        let sh = Shortcuts::default();
        assert!(sh.conflicts(ShortcutTarget::Undo, &ShortcutConfig::ctrl("s")));
        assert!(!sh.conflicts(ShortcutTarget::SaveFile, &ShortcutConfig::ctrl("s")));
        assert!(!sh.conflicts(ShortcutTarget::Undo, &ShortcutConfig::ctrl("1")));
    }

    #[test]
    fn add_recent_deduplication() {
        let mut config = Config::default();
        config.add_recent(&PathBuf::from("/home/user/file.rs"));
        config.add_recent(&PathBuf::from("/home/user/other.rs"));
        config.add_recent(&PathBuf::from("/home/user/file.rs"));
        assert_eq!(config.recent_files.len(), 2);
        assert_eq!(config.recent_files[0], "/home/user/file.rs");
    }

    #[test]
    fn add_recent_max_10() {
        let mut config = Config::default();
        for i in 0..15 {
            config.add_recent(&PathBuf::from(format!("/file{}.txt", i)));
        }
        assert_eq!(config.recent_files.len(), 10);
    }

    #[test]
    fn clear_recent_empties_the_list() {
        let mut config = Config::default();
        config.add_recent(&PathBuf::from("/home/user/file.rs"));
        config.clear_recent();
        assert!(config.recent_files.is_empty());
    }

    #[test]
    fn add_recent_ignores_untitled() {
        let mut config = Config::default();
        config.add_recent(&PathBuf::from("untitled://1"));
        assert!(config.recent_files.is_empty());
    }

    #[test]
    fn an_unknown_value_only_resets_its_own_setting() {
        let (cfg, dropped) =
            Config::parse(r#"{"tab_width": 7, "lint_trigger": "from_the_future"}"#).unwrap();
        assert!(dropped);
        assert_eq!(cfg.tab_width, 7);
        assert!(Config::parse("not json").is_err());
    }

    #[test]
    fn the_default_shortcuts_follow_the_ides_and_never_share_a_key() {
        let sh = Shortcuts::default();
        assert_eq!(sh.duplicate_line, ShortcutConfig::ctrl("d"));
        assert_eq!(sh.delete_line, ShortcutConfig::ctrl("y"));
        assert_eq!(sh.redo, ShortcutConfig::ctrl_shift("z"));
        assert_eq!(sh.format_code, ShortcutConfig::ctrl_alt("l"));
        assert_eq!(sh.command_palette, ShortcutConfig::ctrl_shift("a"));
        assert_eq!(sh.move_line_up, ShortcutConfig::alt_shift("ArrowUp"));
        assert_eq!(sh.toggle_bookmark, ShortcutConfig::plain("F11"));
        assert_eq!(sh.git_commit, ShortcutConfig::ctrl("k"));
        assert_eq!(sh.git_push, ShortcutConfig::ctrl_shift("k"));
        for (i, a) in ALL_TARGETS.iter().enumerate() {
            for b in &ALL_TARGETS[i + 1..] {
                assert!(
                    sh.slot(*a) != sh.slot(*b),
                    "{a:?} and {b:?} share {}",
                    sh.slot(*a).display()
                );
            }
        }
    }

    #[test]
    fn an_old_config_moves_to_the_new_keymap_but_keeps_what_the_user_chose() {
        // Written by a version before the new keymap: no version number.
        let mut old = Shortcuts::before_the_ide_keymap();
        old.find = ShortcutConfig::ctrl("e");
        old.delete_line = ShortcutConfig::ctrl_alt("x");
        let json = serde_json::json!({ "shortcuts": old });
        let config: Config = serde_json::from_value(json).unwrap();
        assert_eq!(config.shortcuts_version, 1);
        let mut shortcuts = config.shortcuts;
        shortcuts.adopt_the_ide_keymap();
        // Still the old default: follows the new keymap.
        assert_eq!(shortcuts.duplicate_line, ShortcutConfig::ctrl("d"));
        assert_eq!(shortcuts.redo, ShortcutConfig::ctrl_shift("z"));
        // Chosen by the user: kept.
        assert_eq!(shortcuts.find, ShortcutConfig::ctrl("e"));
        assert_eq!(shortcuts.delete_line, ShortcutConfig::ctrl_alt("x"));
        // A new install starts on the new keymap, and so is not moved again.
        assert_eq!(Config::default().shortcuts_version, KEYMAP_VERSION);
    }
}
