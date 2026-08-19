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
    ToggleSidebar,
    Quit,
    Undo,
    Redo,
    DuplicateLine,
    MoveLineUp,
    MoveLineDown,
    ToggleComment,
    DeleteLine,
}

/// A single keyboard binding stored as display-friendly strings.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    fn alt(key: &str) -> Self {
        Self {
            ctrl: false,
            shift: false,
            alt: true,
            key: key.to_string(),
        }
    }

    pub fn display(&self) -> String {
        let mut s = String::new();
        if self.ctrl {
            s.push_str("Ctrl+");
        }
        if self.shift {
            s.push_str("Shift+");
        }
        if self.alt {
            s.push_str("Alt+");
        }
        if self.key.len() == 1 {
            s.push_str(&self.key.to_uppercase());
        } else {
            s.push_str(&self.key);
        }
        s
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    pub toggle_sidebar: ShortcutConfig,
    pub quit: ShortcutConfig,
    pub undo: ShortcutConfig,
    pub redo: ShortcutConfig,
    #[serde(default = "default_duplicate_line")]
    pub duplicate_line: ShortcutConfig,
    #[serde(default = "default_move_line_up")]
    pub move_line_up: ShortcutConfig,
    #[serde(default = "default_move_line_down")]
    pub move_line_down: ShortcutConfig,
    #[serde(default = "default_toggle_comment")]
    pub toggle_comment: ShortcutConfig,
    #[serde(default = "default_delete_line")]
    pub delete_line: ShortcutConfig,
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
            toggle_sidebar: ShortcutConfig::ctrl("b"),
            quit: ShortcutConfig::ctrl("q"),
            undo: ShortcutConfig::ctrl("z"),
            redo: ShortcutConfig::ctrl("y"),
            duplicate_line: default_duplicate_line(),
            move_line_up: default_move_line_up(),
            move_line_down: default_move_line_down(),
            toggle_comment: default_toggle_comment(),
            delete_line: default_delete_line(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub dark_mode: bool,
    pub font_size: f32,
    pub font_family: String,
    pub tab_width: usize,
    pub use_spaces: bool,
    pub show_line_numbers: bool,
    pub word_wrap: bool,
    pub auto_indent: bool,
    pub autocomplete_brackets: bool,
    pub autocomplete_quotes: bool,
    pub show_page_guide: bool,
    pub page_guide_column: usize,
    pub highlight_current_line: bool,
    pub locale: String,
    #[serde(default)]
    pub shortcuts: Shortcuts,
    #[serde(default)]
    pub recent_files: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            dark_mode: true,
            font_size: 14.0,
            font_family: "Monospace".to_string(),
            tab_width: 4,
            use_spaces: true,
            show_line_numbers: true,
            word_wrap: true,
            auto_indent: true,
            autocomplete_brackets: true,
            autocomplete_quotes: true,
            show_page_guide: false,
            page_guide_column: 80,
            highlight_current_line: true,
            locale: "en".to_string(),
            shortcuts: Shortcuts::default(),
            recent_files: Vec::new(),
        }
    }
}

fn detect_system_dark_mode() -> bool {
    #[cfg(target_os = "linux")]
    {
        if let Ok(out) = std::process::Command::new("gsettings")
            .args(["get", "org.gnome.desktop.interface", "color-scheme"])
            .output()
        {
            let s = String::from_utf8_lossy(&out.stdout);
            if s.contains("prefer-light") {
                return false;
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
        dirs::config_dir().map(|d| d.join("simpleedit").join("config.json"))
    }

    pub fn load() -> Self {
        if let Some(path) = Self::config_path() {
            if let Ok(s) = std::fs::read_to_string(&path) {
                if let Ok(cfg) = serde_json::from_str(&s) {
                    return cfg;
                }
            }
        }
        Config {
            dark_mode: detect_system_dark_mode(),
            locale: detect_system_locale(),
            ..Default::default()
        }
    }

    pub fn save(&self) {
        if let Some(path) = Self::config_path() {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(json) = serde_json::to_string_pretty(self) {
                let _ = std::fs::write(path, json);
            }
        }
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
        let sc = ShortcutConfig::ctrl("n");
        assert_eq!(sc.display(), "Ctrl+N");
        let sc2 = ShortcutConfig::ctrl_shift("s");
        assert_eq!(sc2.display(), "Ctrl+Shift+S");
        let sc3 = ShortcutConfig::alt("ArrowUp");
        assert_eq!(sc3.display(), "Alt+ArrowUp");
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
    fn add_recent_ignores_untitled() {
        let mut config = Config::default();
        config.add_recent(&PathBuf::from("untitled://1"));
        assert!(config.recent_files.is_empty());
    }
}
