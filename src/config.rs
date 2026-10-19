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

impl Shortcuts {
    /// The binding for `target`.
    pub fn slot_mut(&mut self, target: ShortcutTarget) -> &mut ShortcutConfig {
        match target {
            ShortcutTarget::NewFile => &mut self.new_file,
            ShortcutTarget::OpenFile => &mut self.open_file,
            ShortcutTarget::SaveFile => &mut self.save_file,
            ShortcutTarget::SaveAs => &mut self.save_as,
            ShortcutTarget::CloseFile => &mut self.close_file,
            ShortcutTarget::Find => &mut self.find,
            ShortcutTarget::SelectAll => &mut self.select_all,
            ShortcutTarget::FormatCode => &mut self.format_code,
            ShortcutTarget::GotoLine => &mut self.goto_line,
            ShortcutTarget::CommandPalette => &mut self.command_palette,
            ShortcutTarget::ToggleSidebar => &mut self.toggle_sidebar,
            ShortcutTarget::Quit => &mut self.quit,
            ShortcutTarget::Undo => &mut self.undo,
            ShortcutTarget::Redo => &mut self.redo,
            ShortcutTarget::DuplicateLine => &mut self.duplicate_line,
            ShortcutTarget::MoveLineUp => &mut self.move_line_up,
            ShortcutTarget::MoveLineDown => &mut self.move_line_down,
            ShortcutTarget::ToggleComment => &mut self.toggle_comment,
            ShortcutTarget::DeleteLine => &mut self.delete_line,
            ShortcutTarget::ToggleBookmark => &mut self.toggle_bookmark,
            ShortcutTarget::NextBookmark => &mut self.next_bookmark,
            ShortcutTarget::PrevBookmark => &mut self.prev_bookmark,
        }
    }

    /// Whether some action other than `target` is already bound to `sc`.
    pub fn conflicts(&mut self, target: ShortcutTarget, sc: &ShortcutConfig) -> bool {
        ALL_TARGETS
            .iter()
            .filter(|t| **t != target)
            .any(|t| self.slot_mut(*t) == sc)
    }
}

const ALL_TARGETS: [ShortcutTarget; 22] = [
    ShortcutTarget::NewFile,
    ShortcutTarget::OpenFile,
    ShortcutTarget::SaveFile,
    ShortcutTarget::SaveAs,
    ShortcutTarget::CloseFile,
    ShortcutTarget::Find,
    ShortcutTarget::SelectAll,
    ShortcutTarget::FormatCode,
    ShortcutTarget::GotoLine,
    ShortcutTarget::CommandPalette,
    ShortcutTarget::ToggleSidebar,
    ShortcutTarget::Quit,
    ShortcutTarget::Undo,
    ShortcutTarget::Redo,
    ShortcutTarget::DuplicateLine,
    ShortcutTarget::MoveLineUp,
    ShortcutTarget::MoveLineDown,
    ShortcutTarget::ToggleComment,
    ShortcutTarget::DeleteLine,
    ShortcutTarget::ToggleBookmark,
    ShortcutTarget::NextBookmark,
    ShortcutTarget::PrevBookmark,
];

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
    pub locale: String,
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
            locale: "en".to_string(),
            last_version: String::new(),
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
        dirs::config_dir().map(|d| d.join("simpleedit").join("config.json"))
    }

    pub fn load() -> Self {
        if let Some(path) = Self::config_path() {
            if let Ok(s) = std::fs::read_to_string(&path) {
                match serde_json::from_str(&s) {
                    Ok(cfg) => return cfg,
                    // Keep the unreadable file around instead of silently
                    // overwriting it with defaults on the next save.
                    Err(_) => {
                        let _ = std::fs::rename(&path, path.with_extension("json.bak"));
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
        let (n, s, up) = if cfg!(target_os = "macos") {
            ("⌘N", "⇧⌘S", "⌥↑")
        } else {
            ("Ctrl+N", "Ctrl+Shift+S", "Alt+ArrowUp")
        };
        assert_eq!(ShortcutConfig::ctrl("n").display(), n);
        assert_eq!(ShortcutConfig::ctrl_shift("s").display(), s);
        assert_eq!(ShortcutConfig::alt("ArrowUp").display(), up);
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
        let mut sh = Shortcuts::default();
        for target in ALL_TARGETS {
            let own = sh.slot_mut(target).clone();
            assert!(!sh.conflicts(target, &own), "{target:?} collides");
        }
        assert_eq!(sh.toggle_bookmark, ShortcutConfig::ctrl("F2"));
        assert_eq!(sh.next_bookmark, ShortcutConfig::plain("F2"));
        assert_eq!(sh.prev_bookmark, ShortcutConfig::shift("F2"));
    }

    #[test]
    fn shortcut_conflicts_are_detected() {
        let mut sh = Shortcuts::default();
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
    fn add_recent_ignores_untitled() {
        let mut config = Config::default();
        config.add_recent(&PathBuf::from("untitled://1"));
        assert!(config.recent_files.is_empty());
    }
}
