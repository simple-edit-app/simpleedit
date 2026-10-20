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
}

fn default_duplicate_line() -> ShortcutConfig {
    ShortcutConfig::ctrl_shift("d")
}
fn default_move_line_up() -> ShortcutConfig {
    ShortcutConfig::alt("ArrowUp")
}
fn default_move_line_down() -> ShortcutConfig {
    ShortcutConfig::alt("ArrowDown")
}
fn default_toggle_comment() -> ShortcutConfig {
    ShortcutConfig::ctrl("/")
}
fn default_delete_line() -> ShortcutConfig {
    ShortcutConfig::ctrl_shift("k")
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
            close_file: ShortcutConfig::ctrl("w"),
            find: ShortcutConfig::ctrl("f"),
            select_all: ShortcutConfig::ctrl("a"),
            format_code: ShortcutConfig::ctrl_shift("f"),
            goto_line: ShortcutConfig::ctrl("g"),
            command_palette: ShortcutConfig::ctrl_shift("p"),
            toggle_sidebar: ShortcutConfig::ctrl("b"),
            quit: ShortcutConfig::ctrl("q"),
            undo: ShortcutConfig::ctrl("z"),
            redo: ShortcutConfig::ctrl("y"),
            duplicate_line: default_duplicate_line(),
            move_line_up: default_move_line_up(),
            move_line_down: default_move_line_down(),
            toggle_comment: default_toggle_comment(),
            delete_line: default_delete_line(),
            toggle_bookmark: ShortcutConfig::ctrl("F2"),
            next_bookmark: ShortcutConfig::plain("F2"),
            prev_bookmark: ShortcutConfig::shift("F2"),
        }
    }
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
    pub show_line_numbers: bool,
    pub auto_indent: bool,
    pub autocomplete_brackets: bool,
    pub autocomplete_quotes: bool,
    /// Wash over the line the cursor is on.
    pub highlight_current_line: bool,
    /// A vertical guide at `page_guide_column`.
    pub show_page_guide: bool,
    pub page_guide_column: usize,
    /// A miniature of the whole file along the editor's right edge.
    pub show_minimap: bool,
    /// Long lines wrap at the edge; off, they run past it (faster on huge lines).
    pub word_wrap: bool,
    pub locale: String,
    /// Whether the file list is shown.
    pub show_sidebar: bool,
    /// Size of the window when it was last closed or resized.
    pub window_width: f32,
    pub window_height: f32,
    /// The version that last ran, to show what's new after an update.
    pub last_version: String,
    pub shortcuts: Shortcuts,
    pub recent_files: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            dark_mode: true,
            font_size: 14.0,
            tab_width: 4,
            use_spaces: true,
            show_line_numbers: true,
            auto_indent: true,
            autocomplete_brackets: true,
            autocomplete_quotes: true,
            highlight_current_line: true,
            show_page_guide: false,
            page_guide_column: 80,
            show_minimap: true,
            word_wrap: true,
            locale: "en".to_string(),
            show_sidebar: true,
            window_width: 1200.0,
            window_height: 800.0,
            last_version: String::new(),
            shortcuts: Shortcuts::default(),
            recent_files: Vec::new(),
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
                match serde_json::from_str::<Config>(&s) {
                    Ok(mut cfg) => {
                        cfg.sanitize();
                        return cfg;
                    }
                    // Keep the unreadable file around instead of silently
                    // overwriting it with defaults on the next save.
                    Err(_) => {
                        let _ = crate::fsutil::set_aside(&path);
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
        assert_eq!(cfg.shortcuts.redo, ShortcutConfig::ctrl("y"));
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
        assert_eq!(sh.toggle_bookmark, ShortcutConfig::ctrl("F2"));
        assert_eq!(sh.next_bookmark, ShortcutConfig::plain("F2"));
        assert_eq!(sh.prev_bookmark, ShortcutConfig::shift("F2"));
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
}
