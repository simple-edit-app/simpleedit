//! Spelling in the editor: misspelled words underlined, a window of
//! suggestions for the one under the cursor, a personal dictionary.

use super::*;
use crate::spell::{self, Language, Speller};
use iced::widget::column;
use std::sync::Arc;

/// Files with more lines than this are not checked as they are typed.
const MAX_LINES: usize = 20_000;
/// Files with more lines than this are checked in the background, once the
/// typing pauses: checking every word at each keystroke would slow it.
const SYNC_LINES: usize = 300;
/// The pause.
const PAUSE: std::time::Duration = std::time::Duration::from_millis(250);

/// What marks are computed for: the file, its text (revision), the settings.
pub type SpellKey = (Option<PathBuf>, u64, u64);
/// Marks computed in the background, and what for (none: given up, the
/// text having changed again).
pub type SpellChecked = (SpellKey, Vec<(usize, usize, usize)>);

/// What the underlines are numbered from (the problems of the lint are
/// numbered from 0).
pub const MARK_BASE: usize = 1 << 30;

/// The misspelled words of `text`, as (line, byte column, end column).
fn spell_marks(speller: &Speller, text: &str, ext: &str) -> Vec<(usize, usize, usize)> {
    let mut starts = vec![0usize];
    starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
    spell::misspelled(speller, text, ext)
        .into_iter()
        .map(|(from, to)| {
            let line = starts.partition_point(|s| *s <= from).saturating_sub(1);
            (line, from - starts[line], to - starts[line])
        })
        .collect()
}

/// A misspelled word and what could replace it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    pub word: String,
    pub line: usize,
    pub col: usize,
    pub list: Vec<String>,
}

#[derive(Default)]
pub struct SpellState {
    speller: Option<Speller>,
    /// The language whose dictionary is being read in the background.
    loading: Option<String>,
    /// The code of the language of `speller`.
    language: String,
    available: Option<Vec<Language>>,
    /// The misspelled words: (line, byte column, end column).
    pub marks: Vec<(usize, usize, usize)>,
    /// What `marks` were computed for: the file, its text, the settings.
    key: Option<SpellKey>,
    /// The text asked for last, in the background: an answer for another
    /// one is of no use (and is not even computed when it is already old).
    wanted: Arc<std::sync::Mutex<Option<SpellKey>>>,
    /// Changes with the dictionary, the language, the words added.
    generation: u64,
    /// The languages of the picker.
    pub choices: Vec<String>,
}

/// The dictionary of a language with the words the user added.
fn load_speller(language: &Language) -> Option<Speller> {
    let mut speller = Speller::load(language).ok()?;
    if let Some(words) = personal_file().and_then(|f| std::fs::read_to_string(f).ok()) {
        for word in words.lines().filter(|w| !w.trim().is_empty()) {
            speller.add_personal(word.trim());
        }
    }
    Some(speller)
}

fn personal_file() -> Option<PathBuf> {
    crate::fsutil::config_dir().map(|d| d.join("simpleedit").join("dictionary.txt"))
}

impl SimpleEditApp {
    fn spell_languages(&mut self) -> &[Language] {
        self.spell.available.get_or_insert_with(spell::languages)
    }

    /// The dictionary of the language: whether it is there. When it is not,
    /// it is read in the background (the first file opened, a change of
    /// language) and the check is made again when it comes.
    fn ensure_speller(&mut self) -> (bool, Task<Message>) {
        let interface = self.config.locale.clone();
        let wanted = self.config.spell_language.clone();
        let picked = {
            let available = self.spell_languages();
            spell::pick_language(&wanted, &interface, available).cloned()
        };
        let Some(language) = picked else {
            self.spell.speller = None;
            return (false, Task::none());
        };
        if self.spell.speller.is_some() && self.spell.language == language.code {
            return (true, Task::none());
        }
        // The tests have no runtime to run a task: it is read on the spot.
        if cfg!(test) {
            return match load_speller(&language) {
                Some(speller) => {
                    self.spell.language = language.code;
                    self.spell.speller = Some(speller);
                    (true, Task::none())
                }
                None => {
                    self.spell.speller = None;
                    (false, Task::none())
                }
            };
        }
        if self.spell.loading.as_deref() == Some(language.code.as_str()) {
            return (false, Task::none());
        }
        self.spell.loading = Some(language.code.clone());
        let code = language.code.clone();
        (
            false,
            Task::perform(
                async move {
                    tokio::task::spawn_blocking(move || load_speller(&language))
                        .await
                        .ok()
                        .flatten()
                },
                move |speller| Message::SpellerLoaded(code.clone(), speller),
            ),
        )
    }

    /// The dictionary was read: the words are checked with it.
    pub(super) fn speller_loaded(
        &mut self,
        code: String,
        speller: Option<Speller>,
    ) -> Task<Message> {
        if self.spell.loading.as_deref() == Some(code.as_str()) {
            self.spell.loading = None;
        }
        // The language may have been changed while it was read.
        let interface = self.config.locale.clone();
        let wanted = self.config.spell_language.clone();
        let current = {
            let available = self.spell_languages();
            spell::pick_language(&wanted, &interface, available).map(|l| l.code.clone())
        };
        if current.as_deref() != Some(code.as_str()) {
            self.spell.key = None;
            return self.refresh_spelling();
        }
        self.spell.speller = speller;
        if self.spell.speller.is_some() {
            self.spell.language = code;
        }
        self.spell.generation += 1;
        self.spell.key = None;
        self.refresh_spelling()
    }

    /// Looks again at the words when the text, the file or the settings
    /// changed: at once, or for a big file being typed in, in the
    /// background once the typing pauses.
    pub(super) fn refresh_spelling(&mut self) -> Task<Message> {
        let eligible = self.config.spell_check
            && self.current_file.is_some()
            && !self.current_file.as_deref().is_some_and(is_virtual)
            && self.large_view().is_none()
            && self.editor.content.line_count() <= MAX_LINES
            && !(self.view_mode == ViewMode::Preview && self.preview_kind().is_some());
        if !eligible {
            if !self.spell.marks.is_empty() {
                self.spell.marks.clear();
            }
            self.spell.key = None;
            return Task::none();
        }
        let key = (
            self.current_file.clone(),
            self.editor.content.revision(),
            self.spell.generation,
        );
        if self.spell.key.as_ref() == Some(&key) {
            return Task::none();
        }
        // The same file and settings, a bigger text that changed: in the
        // background, the marks of before staying until then.
        let typed = self
            .spell
            .key
            .as_ref()
            .is_some_and(|k| k.0 == key.0 && k.2 == key.2);
        let later = typed && self.editor.content.line_count() > SYNC_LINES;
        self.spell.key = Some(key.clone());
        if !later {
            self.spell.marks.clear();
        }
        let (ready, loading) = self.ensure_speller();
        if !ready {
            self.spell.marks.clear();
            // Asked again when the dictionary is there.
            self.spell.key = None;
            return loading;
        }
        let text = crate::editor::buffer_text(&self.editor.content);
        let ext = self.editor.language.clone().unwrap_or_default();
        let Some(speller) = &self.spell.speller else {
            return Task::none();
        };
        if !later {
            self.spell.marks = spell_marks(speller, &text, &ext);
            return Task::none();
        }
        let speller = speller.clone();
        let wanted = self.spell.wanted.clone();
        if let Ok(mut w) = wanted.lock() {
            *w = Some(key.clone());
        }
        Task::perform(
            async move {
                tokio::time::sleep(PAUSE).await;
                let still = |w: &Arc<std::sync::Mutex<Option<SpellKey>>>| {
                    w.lock().is_ok_and(|w| w.as_ref() == Some(&key))
                };
                if !still(&wanted) {
                    return None;
                }
                tokio::task::spawn_blocking(move || {
                    let marks = spell_marks(&speller, &text, &ext);
                    (key, marks)
                })
                .await
                .ok()
            },
            Message::SpellChecked,
        )
    }

    /// The marks computed in the background, if still for the text of now.
    pub(super) fn spell_checked(&mut self, done: Option<SpellChecked>) {
        if let Some((_, marks)) = done.filter(|(key, _)| self.spell.key.as_ref() == Some(key)) {
            self.spell.marks = marks;
        }
    }

    /// The misspelled word at the cursor (or just before it), with
    /// suggestions.
    fn spell_at_cursor(&self) -> Option<Suggestion> {
        let (line, col) = self.editor.content.cursor_position();
        let &(_, start, end) = self
            .spell
            .marks
            .iter()
            .find(|(l, s, e)| *l == line && *s <= col && col <= *e)?;
        let word = self.editor.content.line(line)?.get(start..end)?.to_string();
        let list = self
            .spell
            .speller
            .as_ref()
            .map(|s| s.suggest(&word))
            .unwrap_or_default();
        Some(Suggestion {
            word,
            line,
            col: start,
            list,
        })
    }

    pub(super) fn open_spell_fix(&mut self) -> Task<Message> {
        match self.spell_at_cursor() {
            Some(found) => self.overlay = ActiveOverlay::Spelling(Box::new(found)),
            None => {
                let key = if self.spell.available.as_ref().is_some_and(Vec::is_empty) {
                    t!("spell.no_dictionary")
                } else {
                    t!("spell.none")
                };
                self.set_status(key, false);
            }
        }
        Task::none()
    }

    pub(super) fn spell_replace(&mut self, new: String) -> Task<Message> {
        let ActiveOverlay::Spelling(found) =
            std::mem::replace(&mut self.overlay, ActiveOverlay::None)
        else {
            return Task::none();
        };
        if self.editing_blocked() {
            return Task::none();
        }
        self.push_undo();
        self.undo_new_group = true;
        self.edited(|app| {
            app.editor
                .content
                .select_bytes(found.line, found.col, found.word.len());
            app.editor
                .content
                .perform(text_editor::Action::Edit(text_editor::Edit::Paste(
                    std::sync::Arc::new(new),
                )));
        });
        self.is_dirty = true;
        Task::none()
    }

    /// The word is known from now on: for ever (`add`) or until the app
    /// closes.
    pub(super) fn spell_learn(&mut self, add: bool) -> Task<Message> {
        let ActiveOverlay::Spelling(found) =
            std::mem::replace(&mut self.overlay, ActiveOverlay::None)
        else {
            return Task::none();
        };
        if let Some(speller) = &mut self.spell.speller {
            if add {
                speller.add_personal(&found.word);
                if let Some(file) = personal_file() {
                    if let Some(parent) = file.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    let mut words = std::fs::read_to_string(&file).unwrap_or_default();
                    words.push_str(&found.word);
                    words.push('\n');
                    if let Err(e) = crate::fsutil::write_atomic(&file, words.as_bytes(), false) {
                        self.set_status(format!("{}: {e}", t!("status.error")), true);
                    }
                }
            } else {
                speller.ignore(&found.word);
            }
        }
        self.spell.generation += 1;
        self.refresh_spelling()
    }

    pub(super) fn open_spell_languages(&mut self) -> Task<Message> {
        let codes: Vec<String> = self
            .spell_languages()
            .iter()
            .map(|l| l.code.clone())
            .collect();
        if codes.is_empty() {
            self.set_status(t!("spell.no_dictionary"), true);
            return Task::none();
        }
        self.spell.choices = codes;
        self.overlay = ActiveOverlay::SpellLanguages;
        Task::none()
    }

    pub(super) fn spell_set_language(&mut self, code: String) -> Task<Message> {
        self.overlay = ActiveOverlay::None;
        self.config.spell_language = code;
        self.config.save();
        self.spell.speller = None;
        self.spell.generation += 1;
        let check = self.refresh_spelling();
        let language = if self.spell.language.is_empty() {
            self.config.spell_language.clone()
        } else {
            self.spell.language.clone()
        };
        self.set_status(t!("spell.language_set", code = language.as_str()), false);
        check
    }

    /// The underlines of the misspelled words, for the editor to draw.
    pub(super) fn spell_marks(&self, dark: bool) -> Vec<crate::code_editor::DiagMark> {
        let color = if dark {
            iced::Color::from_rgb8(0x5f, 0xc7, 0xa5)
        } else {
            iced::Color::from_rgb8(0x1f, 0x9d, 0x76)
        };
        self.spell
            .marks
            .iter()
            .enumerate()
            .map(|(i, (line, col, end))| crate::code_editor::DiagMark {
                id: MARK_BASE + i,
                line: *line,
                col: *col,
                end_line: *line,
                end_col: *end,
                color,
                wavy: true,
                straight: false,
                wash: None,
            })
            .collect()
    }

    pub(super) fn view_spelling(&self, dark: bool, found: &Suggestion) -> Element<'_, Message> {
        let p = se_theme::palette(dark);
        let ghost = move |active: bool| (se_theme::GhostButton { dark, active }).style();
        let mut items: Vec<Element<Message>> = Vec::new();
        if found.list.is_empty() {
            items.push(
                text(t!("spell.no_suggestions").to_string())
                    .size(12.5)
                    .color(p.muted)
                    .into(),
            );
        }
        for (i, suggestion) in found.list.iter().enumerate() {
            items.push(
                button(text(suggestion.clone()).size(13))
                    .padding([6, 12])
                    .width(Length::Fill)
                    .on_press(Message::SpellReplace(suggestion.clone()))
                    .style(ghost(self.overlay_pick == i))
                    .into(),
            );
        }
        let (add_at, ignore_at) = (found.list.len(), found.list.len() + 1);
        container(
            column![
                row![
                    text(t!("spell.title").to_string()).size(14).color(p.text),
                    text(found.word.clone())
                        .size(14)
                        .font(se_theme::MONO)
                        .color(se_theme::error_color(dark)),
                ]
                .spacing(10),
                column(items).spacing(2),
                row![
                    button(text(t!("spell.add").to_string()).size(12.5))
                        .padding([5, 10])
                        .on_press(Message::SpellAdd)
                        .style(ghost(self.overlay_pick == add_at)),
                    button(text(t!("spell.ignore").to_string()).size(12.5))
                        .padding([5, 10])
                        .on_press(Message::SpellIgnore)
                        .style(ghost(self.overlay_pick == ignore_at)),
                ]
                .spacing(8),
            ]
            .spacing(12)
            .padding(16),
        )
        .width(300)
        .style(se_theme::card(dark))
        .into()
    }

    pub(super) fn view_spell_languages(&self, dark: bool) -> Element<'_, Message> {
        let p = se_theme::palette(dark);
        let current = &self.spell.language;
        let mut items: Vec<Element<Message>> =
            vec![button(text(t!("spell.language_auto").to_string()).size(13))
                .padding([6, 12])
                .width(Length::Fill)
                .on_press(Message::SpellLanguage(String::new()))
                .style(
                    (se_theme::GhostButton {
                        dark,
                        active: self.config.spell_language.is_empty() || self.overlay_pick == 0,
                    })
                    .style(),
                )
                .into()];
        for (row_index, code) in self.spell.choices.iter().enumerate() {
            items.push(
                button(text(code.clone()).size(13))
                    .padding([6, 12])
                    .width(Length::Fill)
                    .on_press(Message::SpellLanguage(code.clone()))
                    .style(
                        (se_theme::GhostButton {
                            dark,
                            active: (!self.config.spell_language.is_empty() && code == current)
                                || self.overlay_pick == row_index + 1,
                        })
                        .style(),
                    )
                    .into(),
            );
        }
        container(
            column![
                text(t!("spell.languages").to_string())
                    .size(14)
                    .color(p.text),
                scrollable(column(items).spacing(2).padding(4)).height(Length::Fixed(320.0)),
            ]
            .spacing(10)
            .padding(16),
        )
        .width(260)
        .style(se_theme::card(dark))
        .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app_with(text: &str, ext: &str) -> SimpleEditApp {
        let dir = tempfile::tempdir().unwrap();
        let dictionaries = dir.path().join("dictionaries");
        std::fs::create_dir(&dictionaries).unwrap();
        std::fs::write(
            dictionaries.join("xx.aff"),
            "SET UTF-8\nTRY esianrtolcdug\n",
        )
        .unwrap();
        std::fs::write(
            dictionaries.join("xx.dic"),
            "6\nhello\nworld\nthe\nnice\nis\nand\n",
        )
        .unwrap();
        let file = dir.path().join(format!("a.{ext}"));
        std::fs::write(&file, text).unwrap();
        let config = Config {
            spell_language: "xx".into(),
            ..Config::default()
        };
        let mut app = SimpleEditApp::blank(config);
        let _ = app.restore_session(crate::session::Session::default());
        app.spell.available = Some(spell::languages_in(&[dictionaries]));
        let _ = app.update(Message::TabRead(Ok(Loaded {
            path: file.clone(),
            text: text.into(),
            format: TextFormat::default(),
            stamp: DiskStamp::of(&file),
        })));
        std::mem::forget(dir);
        let _ = app.refresh_spelling();
        app
    }

    #[test]
    fn misspelled_words_are_found_with_their_places_and_follow_the_typing() {
        let mut app = app_with("hello wrold\nthe nice wrld is\n", "txt");
        assert_eq!(app.spell.marks, vec![(0, 6, 11), (1, 9, 13)]);
        assert_eq!(app.spell_marks(true).len(), 2);
        assert!(app.spell_marks(true)[0].id >= MARK_BASE);
        // Fixing one word, by typing, updates the marks.
        app.editor.content.select_bytes(0, 6, 5);
        let _ = app.update(Message::EditorAction(text_editor::Action::Edit(
            text_editor::Edit::Paste(std::sync::Arc::new("world".into())),
        )));
        let _ = app.refresh_spelling();
        assert_eq!(app.spell.marks, vec![(1, 9, 13)]);
        // Off: nothing is marked.
        app.config.spell_check = false;
        let _ = app.refresh_spelling();
        assert!(app.spell.marks.is_empty());
    }

    #[test]
    fn a_big_file_is_checked_once_the_typing_pauses_and_only_for_its_text() {
        let text = "hello wrold\n".to_string() + &"the nice world\n".repeat(SYNC_LINES);
        let mut app = app_with(&text, "txt");
        // Opened: checked at once.
        assert_eq!(app.spell.marks, vec![(0, 6, 11)]);
        // Typed in: the marks of before stay until the check, made later.
        app.editor.content.select_bytes(0, 6, 5);
        let _ = app.update(Message::EditorAction(text_editor::Action::Edit(
            text_editor::Edit::Paste(std::sync::Arc::new("world".into())),
        )));
        let _ = app.refresh_spelling();
        assert_eq!(app.spell.marks, vec![(0, 6, 11)]);
        let key = app.spell.key.clone().unwrap();
        // An answer for a text that is no more is dropped.
        let old = (key.0.clone(), key.1 - 1, key.2);
        let _ = app.update(Message::SpellChecked(Some((old, vec![]))));
        assert_eq!(app.spell.marks, vec![(0, 6, 11)]);
        let text = crate::editor::buffer_text(&app.editor.content);
        let marks = spell_marks(app.spell.speller.as_ref().unwrap(), &text, "txt");
        let _ = app.update(Message::SpellChecked(Some((key, marks))));
        assert!(app.spell.marks.is_empty());
    }

    #[test]
    fn a_word_is_corrected_added_or_ignored_from_the_window() {
        let mut app = app_with("hello wrold\n", "txt");
        app.editor.content.select_bytes(0, 8, 0);
        let _ = app.update(Message::SpellFix);
        let ActiveOverlay::Spelling(found) = app.overlay.clone() else {
            panic!("the window of suggestions");
        };
        assert_eq!(
            (found.word.as_str(), found.line, found.col),
            ("wrold", 0, 6)
        );
        let _ = app.update(Message::SpellReplace("world".into()));
        assert_eq!(
            crate::editor::buffer_text(&app.editor.content),
            "hello world\n"
        );
        assert_eq!(app.overlay, ActiveOverlay::None);
        // Ignoring a word clears its mark.
        let mut app = app_with("hello wrold\n", "txt");
        app.editor.content.select_bytes(0, 8, 0);
        let _ = app.update(Message::SpellFix);
        let _ = app.update(Message::SpellIgnore);
        assert!(app.spell.marks.is_empty());
        // Where nothing is wrong, there is no window.
        app.editor.content.select_bytes(0, 1, 0);
        let _ = app.update(Message::SpellFix);
        assert_eq!(app.overlay, ActiveOverlay::None);
    }

    #[test]
    fn a_program_is_checked_only_in_its_comments() {
        let app = app_with("let wrld = 1; // the wrld\n", "rs");
        assert_eq!(app.spell.marks, vec![(0, 21, 25)]);
    }

    #[test]
    fn a_language_is_chosen_and_nothing_is_found_without_a_dictionary() {
        let mut app = app_with("hello wrold\n", "txt");
        let _ = app.update(Message::SpellLanguage("xx".into()));
        assert_eq!(app.config.spell_language, "xx");
        assert_eq!(app.spell.marks.len(), 1);
        let mut app = app_with("hello wrold\n", "txt");
        app.spell.available = Some(Vec::new());
        app.spell.speller = None;
        app.spell.generation += 1;
        let _ = app.refresh_spelling();
        assert!(app.spell.marks.is_empty());
        let _ = app.update(Message::OpenSpellLanguages);
        assert!(app.status_is_error);
    }
}
