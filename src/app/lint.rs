//! Lint in the app: when the linters run, what they found, the card that
//! opens on a problem and the fixes.

use super::*;
use crate::lint::{self, run, tools, Diagnostic, Fix};
use iced::widget::{column, mouse_area};

/// What a run is for: the file, and the text it read (the content's id and
/// revision).
pub type Key = (PathBuf, u64, u64);

/// What a run found: the problems, what failed, and the tools that were
/// left out (they read the disk and the file has unsaved changes).
pub type Outcome = (Vec<Diagnostic>, Vec<String>, Vec<String>, Vec<String>);

/// How long the text must rest before it is looked at again.
const QUIET: std::time::Duration = std::time::Duration::from_millis(600);

/// The card stays this long when the pointer leaves, to let it reach it.
const GRACE: std::time::Duration = std::time::Duration::from_millis(350);

#[derive(Default)]
pub struct LintState {
    /// The file the problems are of.
    pub file: Option<PathBuf>,
    pub diagnostics: Vec<Diagnostic>,
    /// The text they were found in.
    key: Option<Key>,
    /// What the last run was started on.
    launched: Option<Key>,
    generation: u64,
    /// The tools that failed in the last run.
    pub failed: Vec<String>,
    /// Those that ran (language server included).
    pub ran: Vec<String>,
    /// Look again at once (after a save, or when asked).
    pub force: bool,
    /// The problem whose card is open, and where it goes.
    pub hover: Option<(usize, iced::Point)>,
    card_hovered: bool,
    /// Problems whose fixes were asked of the language server.
    fetched: HashSet<usize>,
    close_token: u64,
    /// Fix this file once it is the one in the editor.
    fix_after_open: Option<PathBuf>,
}

fn ext_of(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

impl SimpleEditApp {
    /// The text the problems are about: a real file, as it is now.
    fn lint_key(&self) -> Option<Key> {
        let path = self
            .current_file
            .clone()
            .filter(|p| !is_virtual(p) && !is_untitled(p))?;
        Some((
            path,
            self.editor.content.id(),
            self.editor.content.revision(),
        ))
    }

    /// Whether the user turned the lint on for the language of a file.
    pub(super) fn lint_on_for(&self, path: &Path) -> bool {
        tools::group_of(&ext_of(path)).is_some_and(|g| self.config.lint_languages.contains(g.id))
    }

    /// The group of the file in the editor, if the lint is off for it: what
    /// the menu offers to turn on.
    pub(super) fn lint_group_off(&self) -> Option<&'static tools::Group> {
        let path = self.lint_key()?.0;
        tools::group_of(&ext_of(&path)).filter(|g| !self.config.lint_languages.contains(g.id))
    }

    fn lint_plan(&self, path: &Path) -> (tools::Plan, String) {
        let ext = ext_of(path);
        let manual = tools::group_of(&ext)
            .and_then(|g| self.config.lint_tools.get(g.id))
            .map(Vec::as_slice);
        (
            tools::plan(path, &ext, manual, &self.config.lint_programs),
            ext,
        )
    }

    fn clear_lint(&mut self) {
        if self.lint.file.is_some() || !self.lint.diagnostics.is_empty() {
            let fix_after_open = self.lint.fix_after_open.take();
            self.lint = LintState {
                generation: self.lint.generation + 1,
                fix_after_open,
                ..LintState::default()
            };
        }
    }

    /// Starts a run when one is due: at once for a file just opened, a
    /// moment after the last key typed, after a save.
    pub(super) fn lint_tick(&mut self) -> Task<Message> {
        if let Some(path) = self.lint.fix_after_open.clone() {
            if self.current_file.as_ref() == Some(&path) {
                self.lint.fix_after_open = None;
                return self.start_fix_all();
            }
        }
        let Some(key) = self
            .lint_key()
            .filter(|k| self.config.lint_enabled && self.lint_on_for(&k.0))
        else {
            self.clear_lint();
            return Task::none();
        };
        let trigger = self.config.lint_trigger;
        let opened = self.lint.file.as_ref() != Some(&key.0);
        if !opened && self.lint.launched.as_ref() == Some(&key) && !self.lint.force {
            return Task::none();
        }
        let due = self.lint.force
            || if opened {
                self.config.lint_on_open
            } else {
                trigger == lint::Trigger::OnType && self.last_edit.elapsed() >= QUIET
            };
        if !due {
            return Task::none();
        }
        if opened {
            self.lint.diagnostics.clear();
            self.lint.key = None;
            self.lint.hover = None;
            self.lint.file = Some(key.0.clone());
        }
        self.launch_lint(key)
    }

    fn launch_lint(&mut self, key: Key) -> Task<Message> {
        self.lint.force = false;
        self.lint.launched = Some(key.clone());
        self.lint.generation += 1;
        let generation = self.lint.generation;
        let (plan, ext) = self.lint_plan(&key.0);
        let text = crate::editor::buffer_text(&self.editor.content);
        let dirty = self.is_dirty;
        // The tools that read the disk say nothing of text not saved yet.
        let (run_now, skipped): (Vec<_>, Vec<_>) = plan
            .tools
            .into_iter()
            .partition(|(tool, _)| tool.stdin || !dirty);
        let skipped: Vec<String> = skipped.iter().map(|(t, _)| t.name.to_string()).collect();
        let lsp = plan.lsp;
        let mut ran: Vec<String> = run_now.iter().map(|(t, _)| t.name.to_string()).collect();
        if lsp {
            ran.push(t!("lint.language_server").to_string());
        }
        ran.extend(skipped.iter().cloned());
        let path = key.0.clone();
        Task::perform(
            async move {
                let job = run::Job {
                    path: path.clone(),
                    text: text.clone(),
                };
                let (mut found, mut failed) = run::all(run_now, job).await;
                if lsp {
                    match crate::lsp::diagnostics(&ext, &path, &text).await {
                        Ok(mut list) => found.append(&mut list),
                        Err(e) => failed.push(format!("{ext}: {e}")),
                    }
                }
                found.sort_by_key(|d| (d.line, d.col));
                (found, failed, skipped, ran)
            },
            move |outcome| Message::LintDone(generation, key.clone(), outcome),
        )
    }

    pub(super) fn lint_done(&mut self, generation: u64, key: Key, outcome: Outcome) {
        if generation != self.lint.generation || self.lint_key().map(|k| k.0) != Some(key.0.clone())
        {
            return;
        }
        let (mut found, failed, skipped, ran) = outcome;
        // What the tools left out keeps showing until they run again.
        found.extend(
            self.lint
                .diagnostics
                .iter()
                .filter(|d| skipped.contains(&d.source))
                .cloned(),
        );
        found.sort_by_key(|d| (d.line, d.col));
        self.lint.diagnostics = found;
        self.lint.key = Some(key);
        // A tool that fails is told once, not at every run.
        if !failed.is_empty() && failed != self.lint.failed {
            self.set_status(format!("{}: {}", t!("lint.failed"), failed[0]), true);
        }
        self.lint.ran = ran;
        self.lint.failed = failed;
        self.lint.fetched.clear();
        self.lint.hover = None;
    }

    /// Moves the problems below an edit that added or removed lines, so that
    /// they keep to their text until the next look.
    pub(super) fn shift_problems(&mut self, top: usize, delta: isize) {
        if self.lint.diagnostics.is_empty() || self.lint.file != self.current_file {
            return;
        }
        let removed = delta.unsigned_abs();
        let moved = |line: usize| line.saturating_add_signed(delta);
        self.lint
            .diagnostics
            .retain(|d| delta >= 0 || d.line <= top || d.line > top + removed);
        for d in &mut self.lint.diagnostics {
            if d.line > top {
                d.line = moved(d.line);
            }
            if d.end_line > top {
                d.end_line = moved(d.end_line);
            }
        }
        self.lint.hover = None;
    }

    /// Whether the problems were found in the text as it is now.
    fn lint_fresh(&self) -> bool {
        self.lint.key.is_some() && self.lint.key == self.lint_key()
    }

    /// The problems to show, by their number.
    pub(super) fn visible_problems(&self) -> Vec<usize> {
        self.lint
            .diagnostics
            .iter()
            .enumerate()
            .filter(|(_, d)| self.config.lint_level.shows(d.severity))
            .map(|(i, _)| i)
            .collect()
    }

    /// The problems as the editor draws them.
    pub(super) fn problem_paint(&self) -> Option<crate::code_editor::DiagPaint> {
        if !self.config.lint_enabled
            || !self.config.lint_highlight
            || self.lint.diagnostics.is_empty()
            || self.lint.file != self.current_file
        {
            return None;
        }
        use lint::Marking;
        let marks = self
            .visible_problems()
            .into_iter()
            .map(|i| {
                let d = &self.lint.diagnostics[i];
                let marking = lint::marking(&self.config, d.severity);
                crate::code_editor::DiagMark {
                    id: i,
                    line: d.line,
                    col: d.col,
                    end_line: d.end_line,
                    end_col: d.end_col,
                    color: lint::color(&self.config, d.severity),
                    wavy: matches!(marking, Marking::Wavy | Marking::WavyHighlight),
                    straight: marking == Marking::Straight,
                    wash: match marking {
                        Marking::Highlight => Some(0.28),
                        Marking::WavyHighlight => Some(0.13),
                        _ => None,
                    },
                }
            })
            .collect();
        Some(crate::code_editor::DiagPaint { marks })
    }

    /// What the status bar says: how many problems the file has, once it was
    /// looked at.
    pub(super) fn problem_counts(&self) -> Option<lint::Counts> {
        if !self.config.lint_enabled || self.lint.key.is_none() || self.lint.ran.is_empty() {
            return None;
        }
        let shown: Vec<Diagnostic> = self
            .visible_problems()
            .into_iter()
            .map(|i| self.lint.diagnostics[i].clone())
            .collect();
        Some(lint::counts(&shown))
    }

    /// Said in the status bar when the language is on but no tool could
    /// look at the file.
    pub(super) fn lint_hint(&self) -> Option<String> {
        (self.config.lint_enabled
            && self.lint.key.is_some()
            && self.lint.file == self.current_file
            && self.lint.ran.is_empty()
            && self.lint.failed.is_empty())
        .then(|| t!("lint.no_tool").to_string())
    }

    pub(super) fn lint_hover(&mut self, over: Option<(usize, iced::Point)>) -> Task<Message> {
        self.lint.close_token += 1;
        let Some((id, at)) = over else {
            return self.close_card_later();
        };
        if id >= self.lint.diagnostics.len() {
            return Task::none();
        }
        self.lint.hover = Some((id, at));
        // The fixes of a language server come when its card opens.
        let d = &self.lint.diagnostics[id];
        let key = self.lint_key();
        if d.fixes.is_empty()
            && d.raw.is_some()
            && self.lint.fetched.insert(id)
            && self.lint_fresh()
        {
            if let Some(key) = key {
                let (d, text, ext) = (
                    d.clone(),
                    crate::editor::buffer_text(&self.editor.content),
                    ext_of(&key.0),
                );
                let path = key.0.clone();
                return Task::perform(
                    async move { crate::lsp::code_actions(&ext, &path, &text, &d).await },
                    move |r| Message::LintFixesLoaded(key.clone(), id, r),
                );
            }
        }
        Task::none()
    }

    fn close_card_later(&mut self) -> Task<Message> {
        let token = self.lint.close_token;
        Task::perform(
            async move {
                tokio::time::sleep(GRACE).await;
                token
            },
            Message::LintCloseCheck,
        )
    }

    pub(super) fn lint_card_hovered(&mut self, over: bool) -> Task<Message> {
        self.lint.card_hovered = over;
        self.lint.close_token += 1;
        if over {
            Task::none()
        } else {
            self.close_card_later()
        }
    }

    pub(super) fn lint_close_check(&mut self, token: u64) {
        if token == self.lint.close_token && !self.lint.card_hovered {
            self.lint.hover = None;
        }
    }

    pub(super) fn lint_fixes_loaded(
        &mut self,
        key: Key,
        id: usize,
        result: Result<Vec<Fix>, String>,
    ) {
        if self.lint.key.as_ref() != Some(&key) {
            return;
        }
        if let (Ok(fixes), Some(d)) = (result, self.lint.diagnostics.get_mut(id)) {
            d.fixes = fixes;
        }
    }

    /// Puts one fix of a problem in the text.
    pub(super) fn lint_apply(&mut self, id: usize, fix: usize) -> Task<Message> {
        self.lint.hover = None;
        if !self.lint_fresh() {
            self.lint.force = true;
            self.set_status(t!("lint.stale"), true);
            return Task::none();
        }
        let Some(fix) = self
            .lint
            .diagnostics
            .get(id)
            .and_then(|d| d.fixes.get(fix))
            .cloned()
        else {
            return Task::none();
        };
        let old = crate::editor::buffer_text(&self.editor.content);
        let (new, _) = lint::apply(&old, &[&fix]);
        let (line, col) = self.editor.content.cursor_position();
        self.lint.force = true;
        self.apply_text_edit(new, line, col, None)
    }

    /// Looks again at the file at once.
    pub(super) fn lint_now(&mut self) -> Task<Message> {
        self.lint.force = true;
        self.lint.launched = None;
        if self.lint_key().is_none() {
            return Task::none();
        }
        self.lint.file = None;
        self.lint_tick()
    }

    /// Fixes every problem the tools and the language server can fix.
    pub(super) fn start_fix_all(&mut self) -> Task<Message> {
        self.start_fix(None)
    }

    /// Fixes the problems of one source (a linter, the language server) and
    /// leaves the others alone.
    pub(super) fn start_fix_source(&mut self, source: String) -> Task<Message> {
        self.start_fix(Some(source))
    }

    /// Fixes the problems the settings say (the errors, to begin with).
    fn start_fix(&mut self, source: Option<String>) -> Task<Message> {
        self.lint.hover = None;
        let Some(key) = self
            .lint_key()
            .filter(|k| self.config.lint_enabled && self.lint_on_for(&k.0))
        else {
            return Task::none();
        };
        let (plan, ext) = self.lint_plan(&key.0);
        let text = crate::editor::buffer_text(&self.editor.content);
        let path = key.0.clone();
        let level = self.config.lint_fix_level;
        self.set_status(t!("lint.fixing"), false);
        Task::perform(
            async move {
                run::fix(
                    plan.tools,
                    plan.lsp,
                    &ext,
                    &path,
                    &text,
                    level,
                    source.as_deref(),
                )
                .await
            },
            move |fixed| Message::LintFixedAll(key.clone(), fixed),
        )
    }

    pub(super) fn lint_fixed_all(&mut self, key: Key, fixed: String) -> Task<Message> {
        if self.lint_key().as_ref() != Some(&key) {
            self.set_status(t!("lint.stale"), true);
            return Task::none();
        }
        let old = crate::editor::buffer_text(&self.editor.content);
        if fixed == old {
            self.set_status(t!("lint.nothing_to_fix"), false);
            return Task::none();
        }
        let (line, col) = self.editor.content.cursor_position();
        self.lint.force = true;
        let task = self.apply_text_edit(fixed, line, col, None);
        self.set_status(t!("lint.fixed"), false);
        task
    }

    /// Fixes a file of the list: it is opened first if it is not the one in
    /// the editor.
    pub(super) fn lint_fix_file(&mut self, path: PathBuf) -> Task<Message> {
        if self.current_file.as_ref() == Some(&path) {
            return self.start_fix_all();
        }
        self.lint.fix_after_open = Some(path.clone());
        self.activate(path)
    }

    /// The cursor goes to the next problem (or the previous one).
    pub(super) fn lint_next(&mut self, forward: bool) {
        let shown = self.visible_problems();
        if shown.is_empty() {
            return;
        }
        let at = self.editor.content.cursor_position();
        let places: Vec<(usize, usize)> = shown
            .iter()
            .map(|i| {
                (
                    self.lint.diagnostics[*i].line,
                    self.lint.diagnostics[*i].col,
                )
            })
            .collect();
        let target = if forward {
            places.iter().find(|p| **p > at).or(places.first())
        } else {
            places.iter().rev().find(|p| **p < at).or(places.last())
        };
        if let Some(&(line, col)) = target {
            self.editor.content.select_bytes(line, col, 0);
        }
    }

    /// Fixes known before a save, when the settings ask for it.
    pub(super) fn fix_before_save(&mut self) {
        if !self.config.lint_enabled || !self.config.lint_fix_on_save || !self.lint_fresh() {
            return;
        }
        let old = crate::editor::buffer_text(&self.editor.content);
        let fixes = lint::fixes_at(&self.lint.diagnostics, self.config.lint_fix_level);
        let (new, applied) = lint::apply(&old, &fixes);
        if applied > 0 {
            let (line, col) = self.editor.content.cursor_position();
            let _ = self.apply_text_edit(new, line, col, None);
        }
    }

    /// The card of the problem under the pointer, and where it goes.
    pub(super) fn lint_card(&self, dark: bool) -> Option<(iced::Point, Element<'_, Message>)> {
        let (id, at) = self.lint.hover?;
        let d = self.lint.diagnostics.get(id)?;
        let p = se_theme::palette(dark);
        let link = move || {
            (se_theme::GhostButton {
                dark,
                active: false,
            })
            .style()
        };
        let mut card = column![row![
            text("●")
                .size(11)
                .color(lint::color(&self.config, d.severity)),
            text(d.message.clone())
                .size(13)
                .shaping(iced::widget::text::Shaping::Advanced),
        ]
        .spacing(8)]
        .spacing(6);
        let origin = match &d.code {
            Some(code) => format!("{}: {code}", d.source),
            None => d.source.clone(),
        };
        card = card.push(text(origin).size(11.5).color(p.muted));
        for (i, fix) in d.fixes.iter().take(3).enumerate() {
            card = card.push(
                button(
                    text(fix.title.clone())
                        .size(12.5)
                        .color(p.accent)
                        .shaping(iced::widget::text::Shaping::Advanced),
                )
                .padding([3, 6])
                .on_press(Message::LintApply(id, i))
                .style(link()),
            );
        }
        // Fixing them all is for the problems of the same source as this one.
        if self
            .lint
            .diagnostics
            .iter()
            .any(|o| o.source == d.source && (!o.fixes.is_empty() || o.raw.is_some()))
        {
            card = card.push(
                button(
                    text(t!("lint.fix_source", name = d.source.as_str()).to_string())
                        .size(12.5)
                        .color(p.accent),
                )
                .padding([3, 6])
                .on_press(Message::LintFixSource(d.source.clone()))
                .style(link()),
            );
        }
        let x = at.x.min(self.config.window_width - 560.0).max(4.0);
        Some((
            iced::Point::new(x, at.y),
            mouse_area(
                container(card)
                    .padding(10)
                    .max_width(540)
                    .style(se_theme::card(dark)),
            )
            .on_enter(Message::LintCardHover(true))
            .on_exit(Message::LintCardHover(false))
            .into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lint::{Edit, Severity};

    fn fix(line: usize, col: usize, end_col: usize, text: &str) -> Fix {
        Fix {
            title: "Fix".into(),
            safe: true,
            edits: vec![Edit {
                line,
                col,
                end_line: line,
                end_col,
                text: text.into(),
            }],
        }
    }

    pub fn problem(line: usize, col: usize, end_col: usize, severity: Severity) -> Diagnostic {
        Diagnostic {
            line,
            col,
            end_line: line,
            end_col,
            severity,
            message: "m".into(),
            code: Some("rule".into()),
            source: "Test".into(),
            fixes: Vec::new(),
            raw: None,
        }
    }

    fn open_json(app: &mut SimpleEditApp, text: &str) -> PathBuf {
        let path = PathBuf::from("/s/data.json");
        let _ = app.update(Message::TabRead(Ok(Loaded {
            path: path.clone(),
            text: text.into(),
            format: TextFormat::default(),
            stamp: None,
        })));
        path
    }

    fn blank() -> SimpleEditApp {
        let mut config = Config::default();
        config.lint_languages.insert("json".to_string());
        let mut app = SimpleEditApp::blank(config);
        let _ = app.restore_session(crate::session::Session::default());
        app
    }

    #[test]
    fn a_file_opened_is_looked_at_and_the_answer_is_shown_if_still_current() {
        let mut app = blank();
        let path = open_json(&mut app, "{\"a\": }\n");
        // `update` started a run for the file.
        let key = app.lint.launched.clone().expect("a run");
        assert_eq!(key.0, path);
        let generation = app.lint.generation;
        let mut with_fix = problem(0, 6, 7, Severity::Error);
        with_fix.fixes.push(fix(0, 6, 6, "1"));
        app.lint_done(
            generation,
            key.clone(),
            (
                vec![with_fix, problem(0, 0, 1, Severity::Warning)],
                vec![],
                vec![],
                vec!["Test".to_string()],
            ),
        );
        assert_eq!(app.lint.diagnostics.len(), 2);
        let paint = app.problem_paint().expect("something to draw");
        assert_eq!(paint.marks.len(), 2);
        let counts = app.problem_counts().unwrap();
        assert_eq!((counts.errors, counts.warnings), (1, 1));
        // An answer of an older run is dropped.
        app.lint_done(
            generation - 1,
            key.clone(),
            (vec![], vec![], vec![], vec!["Test".to_string()]),
        );
        assert_eq!(app.lint.diagnostics.len(), 2);
        // The level hides what is below it.
        app.config.lint_level = lint::Level::Errors;
        assert_eq!(app.problem_paint().unwrap().marks.len(), 1);
        app.config.lint_highlight = false;
        assert!(app.problem_paint().is_none());
        // The fix goes into the text, as one step that can be undone.
        app.config.lint_highlight = true;
        // Sorted by position: the warning is first, the error with its fix second.
        let before = app.lint.generation;
        let _ = app.update(Message::LintApply(1, 0));
        assert_eq!(
            crate::editor::buffer_text(&app.editor.content),
            "{\"a\": 1}\n"
        );
        assert!(app.lint.generation > before, "it is looked at again");
        let _ = app.update(Message::Undo);
        assert_eq!(
            crate::editor::buffer_text(&app.editor.content),
            "{\"a\": }\n"
        );
    }

    #[test]
    fn nothing_is_looked_at_until_the_language_is_turned_on() {
        let mut app = SimpleEditApp::blank(Config::default());
        let _ = app.restore_session(crate::session::Session::default());
        open_json(&mut app, "{\"a\": }\n");
        assert!(app.lint.launched.is_none() && app.problem_counts().is_none());
        // The menu offers to turn it on for this language.
        assert_eq!(app.lint_group_off().map(|g| g.id), Some("json"));
        let _ = app.update(Message::Lint(lint::Setting::Language("json", true)));
        assert!(app.lint.launched.is_some(), "looked at at once");
        assert!(app.lint_group_off().is_none());
        // A file of a language no linter knows is never looked at.
        let text = PathBuf::from("/s/notes.txt");
        let _ = app.update(Message::TabRead(Ok(Loaded {
            path: text,
            text: "x\n".into(),
            format: TextFormat::default(),
            stamp: None,
        })));
        assert!(app.lint.file.is_none());
    }

    #[test]
    fn fixing_from_a_card_leaves_the_other_sources_alone() {
        let mut app = blank();
        open_json(&mut app, "ab ,cd ;ef\n");
        let key = app.lint.launched.clone().unwrap();
        let mut one = problem(0, 2, 3, Severity::Error);
        one.source = "ESLint".into();
        one.fixes.push(fix(0, 2, 3, ""));
        let mut two = problem(0, 6, 7, Severity::Error);
        two.source = "TypeScript".into();
        two.fixes.push(fix(0, 6, 7, ""));
        app.lint_done(
            app.lint.generation,
            key,
            (vec![one, two], vec![], vec![], vec!["Test".to_string()]),
        );
        // The card of an ESLint problem offers to fix ESLint's.
        let _ = app.update(Message::LintHover(Some((0, iced::Point::ORIGIN))));
        assert!(app.lint_card(true).is_some());
        // What the task would apply: the fixes of that source alone.
        let _ = app.update(Message::LintFixSource("ESLint".into()));
        let ours: Vec<&Fix> = app
            .lint
            .diagnostics
            .iter()
            .filter(|d| d.source == "ESLint")
            .filter_map(|d| d.fixes.first())
            .collect();
        assert_eq!(lint::apply("ab ,cd ;ef\n", &ours).0, "ab,cd ;ef\n");
    }

    #[test]
    fn each_severity_is_marked_the_way_it_was_chosen() {
        let mut app = blank();
        open_json(&mut app, "aaaa\nbbbb\n");
        let key = app.lint.launched.clone().unwrap();
        app.lint_done(
            app.lint.generation,
            key,
            (
                vec![
                    problem(0, 0, 2, Severity::Error),
                    problem(1, 0, 2, Severity::Warning),
                ],
                vec![],
                vec![],
                vec!["Test".to_string()],
            ),
        );
        let _ = app.update(Message::Lint(lint::Setting::Marking(
            Severity::Error,
            lint::Marking::Highlight,
        )));
        let _ = app.update(Message::Lint(lint::Setting::Marking(
            Severity::Warning,
            lint::Marking::Straight,
        )));
        let _ = app.update(Message::Lint(lint::Setting::Color(
            Severity::Warning,
            "#0000ff".into(),
        )));
        let marks = app.problem_paint().unwrap().marks;
        let (error, warning) = (&marks[0], &marks[1]);
        assert!(error.wash.is_some() && !error.wavy && !error.straight);
        assert!(warning.straight && warning.wash.is_none() && !warning.wavy);
        assert_eq!(warning.color, iced::Color::from_rgb8(0, 0, 255));
    }

    #[test]
    fn a_fix_is_refused_once_the_text_has_moved_on() {
        let mut app = blank();
        open_json(&mut app, "{\"a\": }\n");
        let key = app.lint.launched.clone().unwrap();
        let mut d = problem(0, 6, 7, Severity::Error);
        d.fixes.push(fix(0, 6, 6, "1"));
        app.lint_done(
            app.lint.generation,
            key,
            (vec![d], vec![], vec![], vec!["Test".to_string()]),
        );
        let _ = app.update(Message::EditorAction(text_editor::Action::Edit(
            text_editor::Edit::Insert('x'),
        )));
        let before = crate::editor::buffer_text(&app.editor.content);
        let _ = app.update(Message::LintApply(0, 0));
        assert_eq!(crate::editor::buffer_text(&app.editor.content), before);
        assert!(app.status_is_error);
    }

    #[test]
    fn the_settings_decide_when_it_runs() {
        let mut app = blank();
        app.config.lint_trigger = lint::Trigger::Manual;
        app.config.lint_on_open = false;
        let path = open_json(&mut app, "[1]\n");
        assert!(app.lint.launched.is_none(), "manual: nothing by itself");
        let _ = app.update(Message::LintNow);
        assert_eq!(app.lint.launched.as_ref().map(|k| &k.0), Some(&path));
        // A first look when a file is opened is its own choice.
        app.config.lint_on_open = true;
        let other = PathBuf::from("/s/other.json");
        let _ = app.update(Message::TabRead(Ok(Loaded {
            path: other.clone(),
            text: "[2]\n".into(),
            format: TextFormat::default(),
            stamp: None,
        })));
        assert_eq!(app.lint.launched.as_ref().map(|k| &k.0), Some(&other));
        // Off: nothing is kept.
        app.config.lint_enabled = false;
        let _ = app.update(Message::AutoSave);
        assert!(app.lint.file.is_none() && app.problem_counts().is_none());
        // On type: not before the text has rested.
        app.config.lint_enabled = true;
        app.config.lint_trigger = lint::Trigger::OnType;
        let _ = app.update(Message::AutoSave);
        let first = app.lint.generation;
        let _ = app.update(Message::EditorAction(text_editor::Action::Edit(
            text_editor::Edit::Insert(' '),
        )));
        let _ = app.update(Message::AutoSave);
        assert_eq!(app.lint.generation, first, "still typing");
        app.last_edit = std::time::Instant::now() - std::time::Duration::from_secs(2);
        let _ = app.update(Message::AutoSave);
        assert_eq!(app.lint.generation, first + 1);
    }

    #[test]
    fn problems_below_an_edit_move_with_their_lines() {
        let mut app = blank();
        open_json(&mut app, "a\nb\nc\nd\n");
        let key = app.lint.launched.clone().unwrap();
        app.lint_done(
            app.lint.generation,
            key,
            (
                vec![
                    problem(0, 0, 1, Severity::Error),
                    problem(2, 0, 1, Severity::Error),
                    problem(3, 0, 1, Severity::Error),
                ],
                vec![],
                vec![],
                vec!["Test".to_string()],
            ),
        );
        // A line added under the first one.
        app.shift_problems(0, 1);
        let lines: Vec<usize> = app.lint.diagnostics.iter().map(|d| d.line).collect();
        assert_eq!(lines, vec![0, 3, 4]);
        // Two lines removed under the first: the rest moves up, and what
        // was on the removed lines goes.
        app.shift_problems(0, -2);
        let lines: Vec<usize> = app.lint.diagnostics.iter().map(|d| d.line).collect();
        assert_eq!(lines, vec![0, 1, 2]);
        app.shift_problems(0, -1);
        let lines: Vec<usize> = app.lint.diagnostics.iter().map(|d| d.line).collect();
        assert_eq!(lines, vec![0, 1], "the one on the removed line is gone");
    }

    #[test]
    fn the_cursor_goes_from_problem_to_problem() {
        let mut app = blank();
        open_json(&mut app, "aaaa\nbbbb\ncccc\n");
        let key = app.lint.launched.clone().unwrap();
        app.lint_done(
            app.lint.generation,
            key,
            (
                vec![
                    problem(0, 1, 2, Severity::Error),
                    problem(2, 2, 3, Severity::Error),
                ],
                vec![],
                vec![],
                vec!["Test".to_string()],
            ),
        );
        let _ = app.update(Message::LintNext(true));
        assert_eq!(app.editor.content.cursor_position(), (0, 1));
        let _ = app.update(Message::LintNext(true));
        assert_eq!(app.editor.content.cursor_position(), (2, 2));
        let _ = app.update(Message::LintNext(true));
        assert_eq!(app.editor.content.cursor_position(), (0, 1), "round again");
        let _ = app.update(Message::LintNext(false));
        assert_eq!(app.editor.content.cursor_position(), (2, 2));
    }

    #[test]
    fn the_card_follows_the_pointer_and_waits_for_it_to_come() {
        let mut app = blank();
        open_json(&mut app, "aaaa\n");
        let key = app.lint.launched.clone().unwrap();
        app.lint_done(
            app.lint.generation,
            key,
            (
                vec![problem(0, 1, 2, Severity::Error)],
                vec![],
                vec![],
                vec!["Test".to_string()],
            ),
        );
        let _ = app.update(Message::LintHover(Some((0, iced::Point::new(10.0, 20.0)))));
        assert!(app.lint_card(true).is_some());
        // The pointer leaves: the card stays until the check says so...
        let _ = app.update(Message::LintHover(None));
        let token = app.lint.close_token;
        // ...unless the pointer is on it.
        let _ = app.update(Message::LintCardHover(true));
        app.lint_close_check(token);
        assert!(app.lint.hover.is_some());
        let _ = app.update(Message::LintCardHover(false));
        app.lint_close_check(app.lint.close_token);
        assert!(app.lint.hover.is_none());
    }

    #[test]
    fn fixes_before_saving_are_those_known_when_it_is_asked_for() {
        let mut app = blank();
        open_json(&mut app, "{\"a\": }\n");
        let key = app.lint.launched.clone().unwrap();
        let mut d = problem(0, 6, 7, Severity::Error);
        d.fixes.push(fix(0, 6, 6, "1"));
        app.lint_done(
            app.lint.generation,
            key,
            (vec![d], vec![], vec![], vec!["Test".to_string()]),
        );
        app.fix_before_save();
        assert_eq!(
            crate::editor::buffer_text(&app.editor.content),
            "{\"a\": }\n",
            "off"
        );
        app.config.lint_fix_on_save = true;
        app.fix_before_save();
        assert_eq!(
            crate::editor::buffer_text(&app.editor.content),
            "{\"a\": 1}\n"
        );
    }
}
