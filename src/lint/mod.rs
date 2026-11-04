//! Lint: the problems that linters and language servers find in a file,
//! shown in the editor with a fix for those that have one.
//!
//! A *tool* (eslint, ruff, shellcheck, …) is run on the file as typed (or as
//! saved, for those that read the disk) and its report is turned into
//! [`Diagnostic`]s. Language servers give theirs through `crate::lsp`.

pub mod parse;
pub mod run;
pub mod tools;

/// How bad a problem is; the order is the order of importance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    Hint,
    Info,
    Warning,
    Error,
}

/// When the linters run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Trigger {
    /// A moment after the last key typed (the tools that read the disk, on
    /// save).
    #[default]
    OnType,
    OnSave,
    /// Only when asked.
    Manual,
}

impl Trigger {
    pub const ALL: [Trigger; 3] = [Trigger::OnType, Trigger::OnSave, Trigger::Manual];
}

impl std::fmt::Display for Trigger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&match self {
            Trigger::OnType => t!("lint.trigger_type"),
            Trigger::OnSave => t!("lint.trigger_save"),
            Trigger::Manual => t!("lint.trigger_manual"),
        })
    }
}

/// What is shown of the problems.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Level {
    Errors,
    #[default]
    Warnings,
    /// Notes and hints too.
    All,
}

impl Level {
    pub const ALL: [Level; 3] = [Level::Errors, Level::Warnings, Level::All];

    pub fn shows(self, severity: Severity) -> bool {
        match self {
            Level::Errors => severity == Severity::Error,
            Level::Warnings => severity >= Severity::Warning,
            Level::All => true,
        }
    }
}

impl std::fmt::Display for Level {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&match self {
            Level::Errors => t!("lint.level_errors"),
            Level::Warnings => t!("lint.level_warnings"),
            Level::All => t!("lint.level_all"),
        })
    }
}

/// How a problem is marked in the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Marking {
    /// A wavy line under it.
    Wavy,
    /// A plain line under it.
    Straight,
    /// The text on a colored background.
    Highlight,
    /// A wavy line and a faint background.
    #[default]
    WavyHighlight,
}

impl Marking {
    pub const ALL: [Marking; 4] = [
        Marking::Wavy,
        Marking::Straight,
        Marking::Highlight,
        Marking::WavyHighlight,
    ];
}

impl std::fmt::Display for Marking {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&match self {
            Marking::Wavy => t!("lint.marking_wavy"),
            Marking::Straight => t!("lint.marking_straight"),
            Marking::Highlight => t!("lint.marking_highlight"),
            Marking::WavyHighlight => t!("lint.marking_both"),
        })
    }
}

/// The names the colors of the settings go by.
pub fn color_key(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => "info",
        Severity::Hint => "hint",
    }
}

/// How a severity is marked: the one chosen, or the default.
pub fn marking(config: &crate::config::Config, severity: Severity) -> Marking {
    config
        .lint_markings
        .get(color_key(severity))
        .copied()
        .unwrap_or_default()
}

/// `#rrggbb` (or `rrggbb`) as a color.
pub fn parse_hex(text: &str) -> Option<iced::Color> {
    let hex = text.trim().trim_start_matches('#');
    if hex.len() != 6 || !hex.is_ascii() {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some(iced::Color::from_rgb8(byte(0)?, byte(2)?, byte(4)?))
}

/// The color a severity is shown in: the one chosen, or the default.
pub fn color(config: &crate::config::Config, severity: Severity) -> iced::Color {
    let dark = config.dark_mode;
    config
        .lint_colors
        .get(color_key(severity))
        .and_then(|hex| parse_hex(hex))
        .unwrap_or_else(|| default_color(severity, dark))
}

pub fn default_color(severity: Severity, dark: bool) -> iced::Color {
    match severity {
        Severity::Error => crate::theme::error_color(dark),
        Severity::Warning => iced::Color::from_rgb8(0xe0, 0xa1, 0x1b),
        Severity::Info => iced::Color::from_rgb8(0x4f, 0x9d, 0xe8),
        Severity::Hint => crate::theme::muted_text(dark),
    }
}

/// A change made to the settings of the lint.
#[derive(Debug, Clone, PartialEq)]
pub enum Setting {
    Enabled(bool),
    Highlight(bool),
    OnOpen(bool),
    /// How a severity is marked.
    Marking(Severity, Marking),
    FixLevel(Level),
    /// The color of a severity, as typed (empty: the default).
    Color(Severity, String),
    /// The lint is turned on or off for a language group.
    Language(&'static str, bool),
    Trigger(Trigger),
    Level(Level),
    FixOnSave(bool),
    /// A language group goes back to automatic (true) or is chosen by hand.
    Auto(&'static str, bool),
    /// A tool of a language group is chosen or not.
    Tool(&'static str, &'static str, bool),
    /// The path of a tool, typed by the user (empty: found by itself).
    Program(&'static str, String),
}

/// The settings with `setting` applied.
pub fn apply_setting(config: &mut crate::config::Config, setting: Setting) {
    match setting {
        Setting::Enabled(on) => config.lint_enabled = on,
        Setting::Highlight(on) => config.lint_highlight = on,
        Setting::OnOpen(on) => config.lint_on_open = on,
        Setting::Marking(severity, m) => {
            config
                .lint_markings
                .insert(color_key(severity).to_string(), m);
        }
        Setting::FixLevel(l) => config.lint_fix_level = l,
        Setting::Color(severity, text) => {
            if text.trim().is_empty() {
                config.lint_colors.remove(color_key(severity));
            } else {
                config
                    .lint_colors
                    .insert(color_key(severity).to_string(), text);
            }
        }
        Setting::Language(group, on) => {
            if on {
                config.lint_languages.insert(group.to_string());
            } else {
                config.lint_languages.remove(group);
            }
        }
        Setting::Trigger(t) => config.lint_trigger = t,
        Setting::Level(l) => config.lint_level = l,
        Setting::FixOnSave(on) => config.lint_fix_on_save = on,
        Setting::Auto(group, true) => {
            config.lint_tools.remove(group);
        }
        Setting::Auto(group, false) => {
            // By hand starts from what was automatic.
            let chosen = tools::GROUPS
                .iter()
                .find(|g| g.id == group)
                .map(|g| {
                    g.tools
                        .iter()
                        .copied()
                        .filter(|id| tools::tool_by_id(id).is_some_and(|t| t.auto))
                        .chain(std::iter::once(tools::LSP))
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            config.lint_tools.insert(group.to_string(), chosen);
        }
        Setting::Program(tool, path) => {
            if path.trim().is_empty() {
                config.lint_programs.remove(tool);
            } else {
                config.lint_programs.insert(tool.to_string(), path);
            }
        }
        Setting::Tool(group, tool, on) => {
            let list = config.lint_tools.entry(group.to_string()).or_default();
            list.retain(|t| t != tool);
            if on {
                list.push(tool.to_string());
            }
        }
    }
}

/// A replacement in the text a diagnostic was found in: lines from 0, columns
/// in bytes, the end excluded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub line: usize,
    pub col: usize,
    pub end_line: usize,
    pub end_col: usize,
    pub text: String,
}

/// What puts a problem right.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fix {
    pub title: String,
    pub edits: Vec<Edit>,
    /// Safe to apply without looking (those that may change what the code
    /// does are left to be chosen one by one).
    pub safe: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub line: usize,
    pub col: usize,
    pub end_line: usize,
    pub end_col: usize,
    pub severity: Severity,
    pub message: String,
    /// The rule: `comma-spacing`, `TS2552`, `E501`…
    pub code: Option<String>,
    /// Who found it: `ESLint`, `ruff`, `TypeScript`…
    pub source: String,
    pub fixes: Vec<Fix>,
    /// The problem as a language server said it (to ask it for fixes).
    pub raw: Option<serde_json::Value>,
}

/// How many problems of each severity there are.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    pub errors: usize,
    pub warnings: usize,
    pub others: usize,
}

pub fn counts(diagnostics: &[Diagnostic]) -> Counts {
    diagnostics.iter().fold(Counts::default(), |mut c, d| {
        match d.severity {
            Severity::Error => c.errors += 1,
            Severity::Warning => c.warnings += 1,
            _ => c.others += 1,
        }
        c
    })
}

/// The byte offset of (`line`, `col`) in `text`.
pub fn offset_of(text: &str, line: usize, col: usize) -> usize {
    let mut start = 0;
    for _ in 0..line {
        match text[start..].find('\n') {
            Some(i) => start += i + 1,
            None => return text.len(),
        }
    }
    let end = text[start..].find('\n').map_or(text.len(), |i| start + i);
    (start + col).min(end)
}

/// The (line, byte column) of byte `offset` in `text`.
pub fn position_of(text: &str, offset: usize) -> (usize, usize) {
    let offset = offset.min(text.len());
    let before = &text.as_bytes()[..offset];
    let line = before.iter().filter(|b| **b == b'\n').count();
    let start = before
        .iter()
        .rposition(|b| *b == b'\n')
        .map_or(0, |i| i + 1);
    (line, offset - start)
}

/// `text` with the fixes applied. A fix that touches what another one
/// already changed is left out (it applies on the next run). Returns the new
/// text and how many fixes went in.
pub fn apply(text: &str, fixes: &[&Fix]) -> (String, usize) {
    let mut taken: Vec<(usize, usize, &str)> = Vec::new();
    let mut applied = 0;
    let mut ordered: Vec<&&Fix> = fixes.iter().collect();
    ordered.sort_by_key(|f| {
        f.edits
            .iter()
            .map(|e| offset_of(text, e.line, e.col))
            .min()
            .unwrap_or(usize::MAX)
    });
    for fix in ordered {
        let spans: Vec<(usize, usize, &str)> = fix
            .edits
            .iter()
            .map(|e| {
                let start = offset_of(text, e.line, e.col);
                let end = offset_of(text, e.end_line, e.end_col).max(start);
                (start, end, e.text.as_str())
            })
            .collect();
        let clash = spans.iter().any(|(s, e, _)| {
            taken
                .iter()
                .any(|(ts, te, _)| (s < te && ts < e) || (s == ts && e == te))
        });
        if clash || spans.is_empty() {
            continue;
        }
        taken.extend(spans);
        applied += 1;
    }
    taken.sort_by_key(|(s, e, _)| (*s, *e));
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for (start, end, replacement) in taken {
        if start < at {
            continue;
        }
        out.push_str(&text[at..start]);
        out.push_str(replacement);
        at = end;
    }
    out.push_str(&text[at..]);
    (out, applied)
}

/// The same, for the problems that `level` shows.
pub fn fixes_at(diagnostics: &[Diagnostic], level: Level) -> Vec<&Fix> {
    diagnostics
        .iter()
        .filter(|d| level.shows(d.severity))
        .filter_map(|d| d.fixes.first().filter(|f| f.safe))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_change_the_config_and_a_language_goes_by_hand_from_what_was_automatic() {
        let mut config = crate::config::Config::default();
        assert!(
            config.lint_languages.is_empty(),
            "nothing is on to begin with"
        );
        apply_setting(&mut config, Setting::Language("py", true));
        assert!(config.lint_languages.contains("py"));
        apply_setting(&mut config, Setting::Language("py", false));
        assert!(config.lint_languages.is_empty());
        apply_setting(&mut config, Setting::Auto("py", false));
        assert_eq!(
            config.lint_tools["py"],
            vec!["ruff".to_string(), "lsp".to_string()]
        );
        apply_setting(&mut config, Setting::Tool("py", "flake8", true));
        apply_setting(&mut config, Setting::Tool("py", "ruff", false));
        assert_eq!(
            config.lint_tools["py"],
            vec!["lsp".to_string(), "flake8".to_string()]
        );
        apply_setting(&mut config, Setting::Auto("py", true));
        assert!(config.lint_tools.is_empty());
        apply_setting(
            &mut config,
            Setting::Program("flake8", "/opt/flake8".into()),
        );
        assert_eq!(config.lint_programs["flake8"], "/opt/flake8");
        apply_setting(&mut config, Setting::Program("flake8", "  ".into()));
        assert!(config.lint_programs.is_empty());
        apply_setting(&mut config, Setting::Enabled(false));
        apply_setting(&mut config, Setting::Level(Level::Errors));
        assert!(!config.lint_enabled && config.lint_level == Level::Errors);
    }

    #[test]
    fn colors_are_the_ones_chosen_or_the_defaults() {
        assert_eq!(
            parse_hex("#ff8000"),
            Some(iced::Color::from_rgb8(255, 128, 0))
        );
        assert_eq!(parse_hex("00ff00"), Some(iced::Color::from_rgb8(0, 255, 0)));
        assert!(parse_hex("#ff80").is_none() && parse_hex("zzzzzz").is_none());
        let mut config = crate::config::Config::default();
        let default = color(&config, Severity::Warning);
        assert_eq!(default, default_color(Severity::Warning, true));
        apply_setting(
            &mut config,
            Setting::Color(Severity::Warning, "#112233".into()),
        );
        assert_eq!(
            color(&config, Severity::Warning),
            iced::Color::from_rgb8(0x11, 0x22, 0x33)
        );
        // Half typed: the default stays until it is a color.
        apply_setting(
            &mut config,
            Setting::Color(Severity::Warning, "#1122".into()),
        );
        assert_eq!(color(&config, Severity::Warning), default);
        apply_setting(&mut config, Setting::Color(Severity::Warning, " ".into()));
        assert!(config.lint_colors.is_empty());
        // Each severity has its own way of being marked.
        assert_eq!(marking(&config, Severity::Error), Marking::WavyHighlight);
        apply_setting(
            &mut config,
            Setting::Marking(Severity::Error, Marking::Highlight),
        );
        apply_setting(
            &mut config,
            Setting::Marking(Severity::Warning, Marking::Straight),
        );
        assert_eq!(marking(&config, Severity::Error), Marking::Highlight);
        assert_eq!(marking(&config, Severity::Warning), Marking::Straight);
        assert_eq!(marking(&config, Severity::Info), Marking::WavyHighlight);
    }

    #[test]
    fn fixing_them_all_goes_by_the_level() {
        let fix = |text: &str| Fix {
            title: String::new(),
            safe: true,
            edits: vec![Edit {
                line: 0,
                col: 0,
                end_line: 0,
                end_col: 0,
                text: text.into(),
            }],
        };
        let d = |severity, text: &str| Diagnostic {
            line: 0,
            col: 0,
            end_line: 0,
            end_col: 1,
            severity,
            message: String::new(),
            code: None,
            source: String::new(),
            fixes: vec![fix(text)],
            raw: None,
        };
        let all = [
            d(Severity::Error, "e"),
            d(Severity::Warning, "w"),
            d(Severity::Hint, "h"),
        ];
        assert_eq!(fixes_at(&all, Level::Errors).len(), 1);
        assert_eq!(fixes_at(&all, Level::Warnings).len(), 2);
        assert_eq!(fixes_at(&all, Level::All).len(), 3);
        assert_eq!(
            crate::config::Config::default().lint_fix_level,
            Level::Errors
        );
    }

    #[test]
    fn the_level_picks_what_is_shown() {
        assert!(Level::Errors.shows(Severity::Error) && !Level::Errors.shows(Severity::Warning));
        assert!(Level::Warnings.shows(Severity::Warning) && !Level::Warnings.shows(Severity::Info));
        assert!(Level::All.shows(Severity::Hint));
    }

    fn edit(line: usize, col: usize, end_col: usize, text: &str) -> Edit {
        Edit {
            line,
            col,
            end_line: line,
            end_col,
            text: text.into(),
        }
    }

    fn fix(edits: Vec<Edit>) -> Fix {
        Fix {
            title: "fix".into(),
            edits,
            safe: true,
        }
    }

    #[test]
    fn offsets_and_positions_go_both_ways() {
        let text = "ab\ncde\n\nf";
        assert_eq!(offset_of(text, 1, 2), 5);
        assert_eq!(position_of(text, 5), (1, 2));
        assert_eq!(offset_of(text, 3, 0), 8);
        assert_eq!(offset_of(text, 9, 0), text.len());
        // A column past the end of a line stays on the line.
        assert_eq!(offset_of(text, 0, 50), 2);
    }

    #[test]
    fn fixes_apply_together_and_a_clash_waits() {
        let text = "a ,b ,c\n";
        let one = fix(vec![edit(0, 1, 2, "")]);
        let two = fix(vec![edit(0, 4, 5, "")]);
        let (out, n) = apply(text, &[&two, &one]);
        assert_eq!((out.as_str(), n), ("a,b,c\n", 2));
        // The same text changed twice: the second is left for later.
        let clash = fix(vec![edit(0, 1, 3, "X")]);
        let (out, n) = apply(text, &[&one, &clash]);
        assert_eq!((out.as_str(), n), ("a,b ,c\n", 1));
        // An insertion, and a fix with several edits.
        let both = fix(vec![edit(0, 0, 0, "["), edit(0, 7, 7, "]")]);
        assert_eq!(apply(text, &[&both]).0, "[a ,b ,c]\n");
    }

    #[test]
    fn problems_are_counted_by_severity() {
        let d = |severity| Diagnostic {
            line: 0,
            col: 0,
            end_line: 0,
            end_col: 1,
            severity,
            message: String::new(),
            code: None,
            source: String::new(),
            fixes: vec![],
            raw: None,
        };
        let c = counts(&[
            d(Severity::Error),
            d(Severity::Warning),
            d(Severity::Warning),
            d(Severity::Hint),
        ]);
        assert_eq!((c.errors, c.warnings, c.others), (1, 2, 1));
    }
}
