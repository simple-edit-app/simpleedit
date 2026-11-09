//! Spelling in the editor: misspelled words underlined, a window of
//! suggestions for the one under the cursor, a personal dictionary.

use super::*;
use crate::spell::{self, Language, Speller};
use iced::widget::column;

/// Files with more lines than this are not checked as they are typed.
const MAX_LINES: usize = 20_000;

/// What the underlines are numbered from (the problems of the lint are
/// numbered from 0).
pub const MARK_BASE: usize = 1 << 30;

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
    /// The code of the language of `speller`.
    language: String,
    available: Option<Vec<Language>>,
    /// The misspelled words: (line, byte column, end column).
    pub marks: Vec<(usize, usize, usize)>,
    /// What `marks` were computed for: the file, its text, the settings.
    key: Option<(Option<PathBuf>, u64, u64)>,
    /// Changes with the dictionary, the language, the words added.
    generation: u64,
    /// The languages of the picker.
    pub choices: Vec<String>,
}

fn personal_file() -> Option<PathBuf> {
    crate::fsutil::config_dir().map(|d| d.join("simpleedit").join("dictionary.txt"))
}

impl SimpleEditApp {
    fn spell_languages(&mut self) -> &[Language] {
        self.spell.available.get_or_insert_with(spell::languages)
    }

    /// The dictionary of the language, loaded when first needed (and again
    /// when the language changes).
    fn ensure_speller(&mut self) -> bool {
        let interface = self.config.locale.clone();
        let wanted = self.config.spell_language.clone();
        let picked = {
            let available = self.spell_languages();
            spell::pick_language(&wanted, &interface, available).cloned()
        };
        let Some(language) = picked else {
            self.spell.speller = None;
            return false;
        };
        if self.spell.speller.is_some() && self.spell.language == language.code {
            return true;
        }
        match Speller::load(&language) {
            Ok(mut speller) => {
                if let Some(words) = personal_file().and_then(|f| std::fs::read_to_string(f).ok()) {
                    for word in words.lines().filter(|w| !w.trim().is_empty()) {
                        speller.add_personal(word.trim());
                    }
                }
                self.spell.language = language.code;
                self.spell.speller = Some(speller);
                true
            }
            Err(_) => {
                self.spell.speller = None;
                false
            }
        }
    }

    /// Looks again at the words when the text, the file or the settings
    /// changed.
    pub(super) fn refresh_spelling(&mut self) {
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
            return;
        }
        let key = (
            self.current_file.clone(),
            self.editor.content.revision(),
            self.spell.generation,
        );
        if self.spell.key.as_ref() == Some(&key) {
            return;
        }
        self.spell.key = Some(key);
        self.spell.marks.clear();
        if !self.ensure_speller() {
            return;
        }
        let text = crate::editor::buffer_text(&self.editor.content);
        let ext = self.editor.language.clone().unwrap_or_default();
        let Some(speller) = &self.spell.speller else {
            return;
        };
        let mut starts = vec![0usize];
        starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
        self.spell.marks = spell::misspelled(speller, &text, &ext)
            .into_iter()
            .map(|(from, to)| {
                let line = starts.partition_point(|s| *s <= from).saturating_sub(1);
                (line, from - starts[line], to - starts[line])
            })
            .collect();
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
                    let _ = std::fs::write(&file, words);
                }
            } else {
                speller.ignore(&found.word);
            }
        }
        self.spell.generation += 1;
        self.refresh_spelling();
        Task::none()
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
        self.refresh_spelling();
        let language = if self.spell.language.is_empty() {
            self.config.spell_language.clone()
        } else {
            self.spell.language.clone()
        };
        self.set_status(t!("spell.language_set", code = language.as_str()), false);
        Task::none()
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
        app.refresh_spelling();
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
        app.refresh_spelling();
        assert_eq!(app.spell.marks, vec![(1, 9, 13)]);
        // Off: nothing is marked.
        app.config.spell_check = false;
        app.refresh_spelling();
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
        app.refresh_spelling();
        assert!(app.spell.marks.is_empty());
        let _ = app.update(Message::OpenSpellLanguages);
        assert!(app.status_is_error);
    }
}
