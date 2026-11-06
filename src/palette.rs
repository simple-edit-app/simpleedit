//! The app's main actions, as search everywhere lists them on its Actions
//! tab (Ctrl/Cmd+Shift+A by default).

use crate::app::Message;
use crate::config::Config;

/// What a palette entry does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteCommand {
    NewFile,
    OpenFile,
    AddProject,
    GotoFile,
    FindInFiles,
    ReplaceInFiles,
    LocalHistory,
    PutLabel,
    ReopenTab,
    Complete,
    ParameterInfo,
    SaveFile,
    SaveFileAs,
    CloseFile,
    CloseOtherTabs,
    CloseAllTabs,
    Find,
    FormatCode,
    GotoLine,
    SetLanguage,
    ToggleSidebar,
    LintNow,
    LintFix,
    LintNext,
    LintPrev,
    SplitRight,
    SplitDown,
    CloseSplit,
    ToggleTerminal,
    ToggleGit,
    ToggleTheme,
    Preferences,
    ToggleBookmark,
    NextBookmark,
    PrevBookmark,
    NavBack,
    NavForward,
    ToggleFold,
    FoldAll,
    UnfoldAll,
    CheckForUpdate,
    Shortcuts,
    Changelog,
    Quit,
}

impl PaletteCommand {
    /// What the command is called in the settings (its abbreviations).
    pub fn id(self) -> String {
        format!("{self:?}")
    }

    /// Other words it is known by, for search everywhere (English and
    /// French, the languages of the app).
    pub fn synonyms(self) -> &'static [&'static str] {
        use PaletteCommand::*;
        match self {
            NewFile => &["create", "créer", "untitled"],
            OpenFile => &["load", "charger"],
            GotoFile => &["navigate", "naviguer"],
            FindInFiles => &["grep", "search text", "rechercher texte"],
            ReplaceInFiles => &["substitute", "sed", "rename text", "substituer"],
            LocalHistory => &["revisions", "versions", "restore", "restaurer", "révisions"],
            PutLabel => &["mark", "marquer", "checkpoint"],
            Complete => &["autocomplete", "suggestion", "autocomplétion"],
            ParameterInfo => &["signature", "arguments", "hint"],
            FormatCode => &["reformat", "prettier", "indent", "reformater"],
            GotoLine => &["jump", "sauter"],
            ToggleSidebar => &["panel", "files", "panneau", "toggle", "basculer"],
            SplitRight | SplitDown => &["divide", "pane", "diviser", "vertical", "horizontal"],
            ToggleTerminal => &["console", "shell", "toggle", "basculer"],
            ToggleGit => &["vcs", "commit", "version control", "toggle", "basculer"],
            ToggleTheme => &[
                "dark", "light", "sombre", "clair", "mode", "toggle", "basculer",
            ],
            Preferences => &[
                "settings",
                "options",
                "réglages",
                "paramètres",
                "configuration",
            ],
            Shortcuts => &["keymap", "keys", "touches", "raccourcis"],
            Quit => &["exit", "close app", "sortir"],
            CloseFile => &["close tab", "fermer onglet"],
            LintNow | LintFix => &["inspect", "problems", "errors", "erreurs", "problèmes"],
            ToggleFold | FoldAll | UnfoldAll => &["collapse", "expand", "replier", "déplier"],
            ToggleBookmark | NextBookmark | PrevBookmark => &["mark", "favorite", "signet"],
            NavBack | NavForward => &["history", "historique", "previous", "next"],
            CheckForUpdate => &["upgrade", "version", "mise à jour"],
            _ => &[],
        }
    }

    /// The configurable shortcut that runs it, if it has one.
    pub fn shortcut_target(self) -> Option<crate::config::ShortcutTarget> {
        use crate::config::ShortcutTarget as T;
        use PaletteCommand::*;
        Some(match self {
            NewFile => T::NewFile,
            OpenFile => T::OpenFile,
            GotoFile => T::GotoFile,
            FindInFiles => T::FindInFiles,
            ReplaceInFiles => T::ReplaceInFiles,
            ReopenTab => T::ReopenTab,
            Complete => T::Complete,
            ParameterInfo => T::ParameterInfo,
            SaveFile => T::SaveFile,
            SaveFileAs => T::SaveAs,
            CloseFile => T::CloseFile,
            Find => T::Find,
            FormatCode => T::FormatCode,
            GotoLine => T::GotoLine,
            ToggleSidebar => T::ToggleSidebar,
            ToggleTerminal => T::ToggleTerminal,
            ToggleGit => T::ToggleGit,
            ToggleBookmark => T::ToggleBookmark,
            NextBookmark => T::NextBookmark,
            PrevBookmark => T::PrevBookmark,
            NavBack => T::NavBack,
            NavForward => T::NavForward,
            ToggleFold => T::ToggleFold,
            Quit => T::Quit,
            _ => return None,
        })
    }

    /// The message that performs the command.
    pub fn message(self) -> Message {
        match self {
            PaletteCommand::NewFile => Message::NewFile,
            PaletteCommand::OpenFile => Message::OpenFile,
            PaletteCommand::AddProject => Message::AddProject,
            PaletteCommand::GotoFile => Message::OpenGotoFile,
            PaletteCommand::FindInFiles => Message::OpenFindInFiles,
            PaletteCommand::ReplaceInFiles => Message::OpenReplaceInFiles,
            PaletteCommand::LocalHistory => Message::History(crate::app::HistoryMsg::OpenCurrent),
            PaletteCommand::PutLabel => Message::History(crate::app::HistoryMsg::AskLabel),
            PaletteCommand::ReopenTab => Message::ReopenTab,
            PaletteCommand::Complete => Message::CompletionInvoke,
            PaletteCommand::ParameterInfo => Message::HintInvoke,
            PaletteCommand::Find => Message::ToggleSearch,
            PaletteCommand::FormatCode => Message::FormatFile,
            PaletteCommand::GotoLine => Message::OpenGotoLine,
            PaletteCommand::SetLanguage => Message::OpenLanguagePicker,
            PaletteCommand::ToggleSidebar => Message::ToggleSidebar,
            PaletteCommand::LintNow => Message::LintNow,
            PaletteCommand::LintFix => Message::LintFixAll,
            PaletteCommand::LintNext => Message::LintNext(true),
            PaletteCommand::LintPrev => Message::LintNext(false),
            PaletteCommand::SplitRight => Message::Split(true),
            PaletteCommand::SplitDown => Message::Split(false),
            PaletteCommand::CloseSplit => Message::CloseSplit,
            PaletteCommand::ToggleTerminal => Message::ToggleTerminal,
            PaletteCommand::ToggleGit => Message::ToggleGit,
            PaletteCommand::SaveFile => Message::SaveFile,
            PaletteCommand::SaveFileAs => Message::SaveFileAs,
            PaletteCommand::CloseFile => Message::CloseFile,
            PaletteCommand::CloseOtherTabs => Message::CloseOtherTabs,
            PaletteCommand::CloseAllTabs => Message::CloseTabs(None),
            // The theme flips: the palette knows only the command, so the
            // app resolves the target (see `Message::ToggleTheme`).
            PaletteCommand::ToggleTheme => Message::ToggleTheme,
            PaletteCommand::Preferences => Message::TogglePreferences,
            PaletteCommand::ToggleBookmark => Message::ToggleBookmark,
            PaletteCommand::NextBookmark => Message::NextBookmark,
            PaletteCommand::PrevBookmark => Message::PrevBookmark,
            PaletteCommand::NavBack => Message::NavBack,
            PaletteCommand::NavForward => Message::NavForward,
            PaletteCommand::ToggleFold => Message::ToggleFold,
            PaletteCommand::FoldAll => Message::FoldAll,
            PaletteCommand::UnfoldAll => Message::UnfoldAll,
            PaletteCommand::CheckForUpdate => Message::CheckForUpdate,
            PaletteCommand::Shortcuts => Message::OpenShortcuts,
            PaletteCommand::Changelog => Message::OpenChangelog,
            PaletteCommand::Quit => Message::Quit,
        }
    }
}

/// One row of the palette.
#[derive(Debug, Clone)]
pub struct Entry {
    pub command: PaletteCommand,
    pub label: String,
    pub shortcut: Option<String>,
    /// Disabled entries are listed but do nothing (no formatter for this
    /// file type, or a language that is already detected).
    pub enabled: bool,
}

/// Every command, with its current shortcut.
pub fn entries(config: &Config, has_formatter: bool, language_enabled: bool) -> Vec<Entry> {
    let sh = &config.shortcuts;
    let entry = |command, label: String, shortcut: Option<String>, enabled| Entry {
        command,
        label,
        shortcut,
        enabled,
    };
    vec![
        entry(
            PaletteCommand::NewFile,
            t!("palette.new_file").to_string(),
            Some(sh.new_file.display()),
            true,
        ),
        entry(
            PaletteCommand::OpenFile,
            t!("palette.open_file").to_string(),
            Some(sh.open_file.display()),
            true,
        ),
        entry(
            PaletteCommand::AddProject,
            t!("menu.file_add_project").to_string(),
            None,
            true,
        ),
        entry(
            PaletteCommand::GotoFile,
            t!("shortcuts.goto_file").to_string(),
            Some(sh.goto_file.display()),
            true,
        ),
        entry(
            PaletteCommand::FindInFiles,
            t!("shortcuts.find_in_files").to_string(),
            Some(sh.find_in_files.display()),
            true,
        ),
        entry(
            PaletteCommand::ReplaceInFiles,
            t!("shortcuts.replace_in_files").to_string(),
            Some(sh.replace_in_files.display()),
            true,
        ),
        entry(
            PaletteCommand::LocalHistory,
            t!("history.show").to_string(),
            None,
            true,
        ),
        entry(
            PaletteCommand::PutLabel,
            t!("history.put_label").to_string(),
            None,
            true,
        ),
        entry(
            PaletteCommand::ReopenTab,
            t!("shortcuts.reopen_tab").to_string(),
            Some(sh.reopen_tab.display()),
            true,
        ),
        entry(
            PaletteCommand::Find,
            t!("palette.find").to_string(),
            Some(sh.find.display()),
            true,
        ),
        entry(
            PaletteCommand::Complete,
            t!("shortcuts.complete").to_string(),
            Some(sh.complete.display()),
            true,
        ),
        entry(
            PaletteCommand::ParameterInfo,
            t!("shortcuts.parameter_info").to_string(),
            Some(sh.parameter_info.display()),
            true,
        ),
        entry(
            PaletteCommand::FormatCode,
            t!("palette.format_code").to_string(),
            Some(sh.format_code.display()),
            has_formatter,
        ),
        entry(
            PaletteCommand::GotoLine,
            t!("palette.goto_line").to_string(),
            Some(sh.goto_line.display()),
            true,
        ),
        entry(
            PaletteCommand::SetLanguage,
            t!("palette.set_language").to_string(),
            None,
            language_enabled,
        ),
        entry(
            PaletteCommand::ToggleGit,
            t!("git.title").to_string(),
            Some(sh.toggle_git.display()),
            true,
        ),
        entry(
            PaletteCommand::ToggleTerminal,
            t!("terminal.title").to_string(),
            Some(sh.toggle_terminal.display()),
            true,
        ),
        entry(
            PaletteCommand::ToggleSidebar,
            t!("palette.toggle_sidebar").to_string(),
            Some(sh.toggle_sidebar.display()),
            true,
        ),
        entry(
            PaletteCommand::LintNow,
            t!("menu.lint_check").to_string(),
            None,
            true,
        ),
        entry(
            PaletteCommand::LintFix,
            t!("menu.lint_fix").to_string(),
            None,
            true,
        ),
        entry(
            PaletteCommand::LintNext,
            t!("menu.lint_next").to_string(),
            None,
            true,
        ),
        entry(
            PaletteCommand::LintPrev,
            t!("menu.lint_prev").to_string(),
            None,
            true,
        ),
        entry(
            PaletteCommand::SplitRight,
            t!("menu.view_split_right").to_string(),
            None,
            true,
        ),
        entry(
            PaletteCommand::SplitDown,
            t!("menu.view_split_down").to_string(),
            None,
            true,
        ),
        entry(
            PaletteCommand::CloseSplit,
            t!("menu.view_split_close").to_string(),
            None,
            true,
        ),
        entry(
            PaletteCommand::SaveFile,
            t!("menu.file_save").to_string(),
            Some(sh.save_file.display()),
            true,
        ),
        entry(
            PaletteCommand::SaveFileAs,
            t!("menu.file_save_as").to_string(),
            Some(sh.save_as.display()),
            true,
        ),
        entry(
            PaletteCommand::CloseFile,
            t!("menu.file_close").to_string(),
            Some(sh.close_file.display()),
            true,
        ),
        entry(
            PaletteCommand::CloseOtherTabs,
            t!("menu.file_close_others").to_string(),
            None,
            true,
        ),
        entry(
            PaletteCommand::CloseAllTabs,
            t!("menu.file_close_all").to_string(),
            None,
            true,
        ),
        entry(
            PaletteCommand::ToggleTheme,
            t!("palette.toggle_theme").to_string(),
            None,
            true,
        ),
        entry(
            PaletteCommand::Preferences,
            t!("menu.file_preferences").to_string(),
            None,
            true,
        ),
        entry(
            PaletteCommand::ToggleBookmark,
            t!("shortcuts.toggle_bookmark").to_string(),
            Some(sh.toggle_bookmark.display()),
            true,
        ),
        entry(
            PaletteCommand::NextBookmark,
            t!("menu.bookmark_next").to_string(),
            Some(sh.next_bookmark.display()),
            true,
        ),
        entry(
            PaletteCommand::PrevBookmark,
            t!("menu.bookmark_prev").to_string(),
            Some(sh.prev_bookmark.display()),
            true,
        ),
        entry(
            PaletteCommand::NavBack,
            t!("menu.nav_back").to_string(),
            Some(sh.nav_back.display()),
            true,
        ),
        entry(
            PaletteCommand::NavForward,
            t!("menu.nav_forward").to_string(),
            Some(sh.nav_forward.display()),
            true,
        ),
        entry(
            PaletteCommand::ToggleFold,
            t!("menu.fold_toggle").to_string(),
            Some(sh.toggle_fold.display()),
            true,
        ),
        entry(
            PaletteCommand::FoldAll,
            t!("menu.fold_all").to_string(),
            None,
            true,
        ),
        entry(
            PaletteCommand::UnfoldAll,
            t!("menu.unfold_all").to_string(),
            None,
            true,
        ),
        entry(
            PaletteCommand::Shortcuts,
            t!("menu.help_shortcuts").to_string(),
            None,
            true,
        ),
        entry(
            PaletteCommand::CheckForUpdate,
            t!("menu.help_check_updates").to_string(),
            None,
            true,
        ),
        entry(
            PaletteCommand::Changelog,
            t!("menu.help_changelog").to_string(),
            None,
            true,
        ),
        entry(
            PaletteCommand::Quit,
            t!("menu.file_quit").to_string(),
            Some(sh.quit.display()),
            true,
        ),
    ]
}

/// The lowercase base letter of `c`: "É" and "é" both give "e", so a query
/// typed without accents finds a label that has them, and the other way round.
fn fold(c: char) -> char {
    match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => 'a',
        'ç' | 'ć' | 'č' | 'ĉ' | 'ċ' => 'c',
        'ď' | 'đ' => 'd',
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => 'e',
        'ĝ' | 'ğ' | 'ġ' | 'ģ' => 'g',
        'ì' | 'í' | 'î' | 'ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ı' => 'i',
        'ĺ' | 'ļ' | 'ľ' | 'ł' => 'l',
        'ñ' | 'ń' | 'ņ' | 'ň' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ŏ' | 'ő' | 'œ' => 'o',
        'ŕ' | 'ŗ' | 'ř' => 'r',
        'ś' | 'ŝ' | 'ş' | 'š' | 'ß' => 's',
        'ţ' | 'ť' => 't',
        'ù' | 'ú' | 'û' | 'ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => 'u',
        'ý' | 'ÿ' | 'ŷ' => 'y',
        'ź' | 'ż' | 'ž' => 'z',
        other => other,
    }
}

/// Lowercase letters with their accents removed: "Préférences" and
/// "preferences" read the same.
pub fn folded(text: &str) -> String {
    text.chars()
        .flat_map(char::to_lowercase)
        .map(fold)
        .collect()
}

/// Wraps an index into `0..len` (moving past either end wraps around).
pub fn step(selected: usize, delta: i32, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    (selected as i64 + delta as i64).rem_euclid(len as i64) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accents_are_folded() {
        assert_eq!(folded("Préférences"), "preferences");
        assert_eq!(folded("Écrire"), "ecrire");
    }

    #[test]
    fn selection_wraps_around() {
        assert_eq!(step(0, -1, 7), 6);
        assert_eq!(step(6, 1, 7), 0);
        assert_eq!(step(2, 1, 7), 3);
        assert_eq!(step(0, 1, 0), 0);
    }

    #[test]
    fn every_command_maps_to_a_message() {
        for entry in entries(&Config::default(), true, true) {
            let _ = entry.command.message();
        }
    }

    #[test]
    fn the_real_list_has_every_command_once() {
        let list = entries(&Config::default(), true, true);
        for (i, a) in list.iter().enumerate() {
            assert!(
                list[i + 1..].iter().all(|b| b.command != a.command),
                "{:?} is listed twice",
                a.command
            );
        }
        // Everything the menus offer is reachable from here.
        assert!(list.len() >= 18);
    }
}
