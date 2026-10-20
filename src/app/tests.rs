use super::*;

#[cfg(test)]
mod short_dir_tests {
    use super::*;

    #[test]
    fn a_long_folder_keeps_its_end_within_the_limit() {
        let dir = Path::new("/very/long/folder/name")
            .join("x".repeat(80))
            .join("project");
        let shown = short_dir(&dir);
        assert_eq!(shown.chars().count(), MAX_DIR_CHARS);
        assert!(shown.starts_with('…'));
        assert!(shown.ends_with("/project/"));
    }

    #[test]
    fn a_short_folder_is_left_alone() {
        assert_eq!(short_dir(Path::new("/tmp/a")), "/tmp/a/");
    }
}

#[cfg(test)]
mod behavior_tests {
    use super::*;
    use text_editor::{Action, Edit};

    fn app() -> SimpleEditApp {
        let mut app = SimpleEditApp::blank(Config::default());
        let _ = app.restore_session(crate::session::Session::default());
        app
    }

    fn type_text(app: &mut SimpleEditApp, text: &str) {
        for c in text.chars() {
            let _ = app.handle(Message::EditorAction(Action::Edit(Edit::Insert(c))));
        }
    }

    fn text(app: &SimpleEditApp) -> String {
        crate::editor::buffer_text(&app.editor.content)
    }

    #[test]
    fn closing_the_last_tab_leaves_a_tab_to_type_in() {
        let mut app = app();
        let first = app.current_file.clone().unwrap();
        let _ = app.close_path(first);
        assert!(app.current_file.is_some());
        type_text(&mut app, "hi");
        let session = app.build_session();
        assert!(session
            .files
            .iter()
            .any(|f| f.content.as_deref() == Some("hi\n")));
    }

    #[test]
    fn text_typed_with_no_tab_is_ignored() {
        let mut app = app();
        app.current_file = None;
        type_text(&mut app, "lost");
        assert_eq!(text(&app), "");
    }

    #[test]
    fn a_hidden_editor_is_not_edited() {
        let mut app = app();
        app.editor.language = Some("md".into());
        app.view_mode = ViewMode::Preview;
        assert!(app.editing_blocked());
        let _ = app.handle(Message::InsertTab);
        let _ = app.handle(Message::DeleteLine);
        assert!(!app.is_dirty);
        assert_eq!(text(&app), "");
        // The search card doesn't block the line commands any more.
        app.view_mode = ViewMode::Raw;
        app.show_search = true;
        assert!(!app.editing_blocked());
    }

    #[test]
    fn a_format_result_for_other_text_is_dropped() {
        let mut app = app();
        type_text(&mut app, "a");
        let stale = FormatJob {
            path: app.current_file.clone(),
            source: fingerprint("something else\n"),
        };
        let _ = app.handle(Message::FileFormatted(stale, Ok("zzz\n".into())));
        assert_eq!(text(&app), "a\n");
        assert!(app.status_is_error);

        let fresh = FormatJob {
            path: app.current_file.clone(),
            source: fingerprint(&text(&app)),
        };
        let _ = app.handle(Message::FileFormatted(fresh, Ok("A\n".into())));
        assert_eq!(text(&app), "A\n");
    }

    #[test]
    fn a_format_result_for_another_tab_is_dropped() {
        let mut app = app();
        type_text(&mut app, "a");
        let other = FormatJob {
            path: Some(PathBuf::from("/other.js")),
            source: fingerprint(&text(&app)),
        };
        let _ = app.handle(Message::FileFormatted(other, Ok("zzz\n".into())));
        assert_eq!(text(&app), "a\n");
    }

    #[test]
    fn tab_types_a_real_tab_or_fills_to_the_next_stop() {
        let mut app = app();
        app.config.use_spaces = false;
        let _ = app.handle(Message::InsertTab);
        assert_eq!(text(&app), "\t\n");

        let mut app = self::app();
        app.config.use_spaces = true;
        app.config.tab_width = 4;
        type_text(&mut app, "ab");
        let _ = app.handle(Message::InsertTab);
        assert_eq!(text(&app), "ab  \n");
    }

    #[test]
    fn an_undo_that_reaches_the_saved_text_is_clean() {
        let mut app = app();
        app.saved_fp = Some(fingerprint(""));
        type_text(&mut app, "x");
        assert!(app.is_dirty);
        let _ = app.handle(Message::Undo);
        assert_eq!(text(&app), "");
        assert!(!app.is_dirty);
    }

    #[test]
    fn backspace_at_the_start_leaves_no_undo_step() {
        let mut app = app();
        let _ = app.handle(Message::EditorAction(Action::Edit(Edit::Backspace)));
        let _ = app.handle(Message::EditorAction(Action::Edit(Edit::Delete)));
        assert!(app.undo_stack.is_empty());
        assert!(!app.is_dirty);
    }

    #[test]
    fn the_session_keeps_crlf_and_unsaved_text() {
        let mut app = app();
        app.format.ending = crate::tabs::LineEnding::Crlf;
        type_text(&mut app, "abc");
        let session = app.build_session();

        let mut again = SimpleEditApp::blank(Config::default());
        let _ = again.restore_session(session);
        assert_eq!(text(&again), "abc\n");
        assert!(again.is_dirty);
        assert_eq!(again.format.ending, crate::tabs::LineEnding::Crlf);
    }

    #[test]
    fn saved_tabs_are_not_read_until_they_are_opened() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        std::fs::write(&file, "disk").unwrap();
        let session = crate::session::Session {
            files: vec![],
            active_file: None,
            untitled_counter: 1,
        };
        let mut fs = crate::session::FileSession {
            path: String::new(),
            path_raw: None,
            content: None,
            language: Some("md".into()),
            dirty: false,
            crlf: false,
            final_newline: true,
            stamp: None,
        };
        fs.set_path(&file);
        let session = crate::session::Session {
            files: vec![fs],
            ..session
        };
        let mut app = SimpleEditApp::blank(Config::default());
        let _ = app.restore_session(session);
        // Waiting for the read, nothing editable meanwhile.
        assert!(app.current_file.is_none());
        assert_eq!(app.pending_open.as_ref(), Some(&file));
        let loaded = Loaded {
            path: file.clone(),
            text: "disk\n".into(),
            format: TextFormat::default(),
            stamp: DiskStamp::of(&file),
        };
        let _ = app.handle(Message::TabRead(Ok(loaded)));
        assert_eq!(app.current_file.as_ref(), Some(&file));
        assert_eq!(text(&app), "disk\n");
        // The language chosen last time survives.
        assert_eq!(app.editor.language.as_deref(), Some("md"));
    }

    #[test]
    fn a_slow_read_of_a_tab_left_behind_does_not_take_over() {
        let mut app = app();
        let here = app.current_file.clone().unwrap();
        app.pending_open = Some(here.clone());
        let late = Loaded {
            path: PathBuf::from("/late.txt"),
            text: "late\n".into(),
            format: TextFormat::default(),
            stamp: None,
        };
        let _ = app.handle(Message::TabRead(Ok(late)));
        assert_eq!(app.current_file.as_ref(), Some(&here));
    }

    #[test]
    fn external_changes_are_noticed_with_the_size_too() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        std::fs::write(&file, "one").unwrap();
        let mut app = app();
        app.current_file = Some(file.clone());
        app.disk_stamp = DiskStamp::of(&file);
        assert!(!app.changed_on_disk(&file));
        std::fs::write(&file, "longer content").unwrap();
        assert!(app.changed_on_disk(&file));
    }

    #[test]
    fn reloading_keeps_the_undo_history_and_cursor() {
        let mut app = app();
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        app.current_file = Some(file.clone());
        let loaded = |text: &str| Loaded {
            path: file.clone(),
            text: text.into(),
            format: TextFormat::default(),
            stamp: None,
        };
        app.apply_reload(loaded("one\n"));
        app.apply_reload(loaded("two\n"));
        assert_eq!(text(&app), "two\n");
        assert!(!app.is_dirty);
        // The old text is one undo away.
        let _ = app.handle(Message::Undo);
        assert_eq!(text(&app), "one\n");
    }

    #[test]
    fn a_reload_never_overwrites_unsaved_typing() {
        let mut app = app();
        let file = PathBuf::from("/x/a.txt");
        app.current_file = Some(file.clone());
        type_text(&mut app, "mine");
        app.apply_reload(Loaded {
            path: file,
            text: "theirs\n".into(),
            format: TextFormat::default(),
            stamp: None,
        });
        assert_eq!(text(&app), "mine\n");
        assert!(app.external_warned);
    }

    #[test]
    fn save_as_onto_a_tab_with_unsaved_work_is_refused() {
        let mut app = app();
        let other = PathBuf::from("/x/b.txt");
        app.sidebar.add_file(other.clone());
        app.file_cache.insert(
            other.clone(),
            CachedBuffer {
                dirty: true,
                ..CachedBuffer::blank()
            },
        );
        let origin = app.current_file.clone();
        let _ = app.handle(Message::SaveAsChosen(origin, Ok(other)));
        assert!(app.status_is_error);
    }

    #[test]
    fn save_as_renames_the_tab_and_moves_its_bookmarks() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
        let mut app = app();
        app.sidebar.add_file(a.clone());
        app.current_file = Some(a.clone());
        let _ = app.bookmarks.toggle(&a, 3);
        let _ = app.handle(Message::FileSaved(
            Some(a.clone()),
            Ok((b.clone(), "text\n".into())),
        ));
        assert!(app.sidebar.files().contains(&b));
        assert!(!app.sidebar.files().contains(&a));
        assert_eq!(app.bookmarks.lines(&b), &[3]);
        assert_eq!(app.current_file.as_ref(), Some(&b));
    }

    #[test]
    fn replace_all_keeps_a_trailing_blank_line_and_ignores_no_ops() {
        let mut app = app();
        app.editor.content = crate::code_editor::Content::with_text("a\n\n");
        app.search.query = "zzz".into();
        app.search.replacement = "y".into();
        let _ = app.handle(Message::Search(SearchMessage::ReplaceAll));
        assert_eq!(text(&app), "a\n\n");
        assert!(!app.is_dirty);
        assert!(app.undo_stack.is_empty());
    }

    #[test]
    fn same_named_tabs_get_their_folder() {
        let files = vec![
            PathBuf::from("/p/one/mod.rs"),
            PathBuf::from("/p/two/mod.rs"),
            PathBuf::from("/p/lib.rs"),
        ];
        let labels = unique_labels(&files);
        assert_eq!(labels, ["mod.rs — one", "mod.rs — two", "lib.rs"]);
    }

    #[test]
    fn untitled_tabs_are_named_with_their_number() {
        assert!(display_name(&untitled_path(3)).ends_with(" 3"));
    }

    #[test]
    fn line_commands_keep_the_selection_columns() {
        use crate::app::{remap_col, remap_span};
        assert_eq!(remap_col("a", "    a", 1), 5);
        assert_eq!(remap_col("a", "    a", 0), 0);
        assert_eq!(remap_col("    a", "a", 2), 0);
        let span = Span {
            start: (0, 0),
            end: (1, 1),
        };
        let moved = remap_span("a\nb\n", "    a\n    b\n", span, 0);
        assert_eq!((moved.start, moved.end), ((0, 0), (1, 5)));
    }

    #[test]
    fn a_relative_path_becomes_absolute() {
        let abs = absolute(PathBuf::from("does-not-exist-yet.txt"));
        assert!(abs.is_absolute());
        assert!(abs.ends_with("does-not-exist-yet.txt"));
    }

    #[test]
    fn pasting_lines_moves_the_bookmarks_below() {
        let mut app = app();
        app.editor.content = crate::code_editor::Content::with_text("a\nb\nc\n");
        let path = app.current_file.clone().unwrap();
        let _ = app.bookmarks.toggle(&path, 2);
        let _ = app.handle(Message::ClipboardContent(Some("x\ny\n".into())));
        assert_eq!(app.bookmarks.lines(&path), &[4]);
    }

    #[test]
    fn switching_tabs_keeps_each_cursor_and_text() {
        let mut app = app();
        let first = app.current_file.clone().unwrap();
        type_text(&mut app, "abc");
        let _ = app.handle(Message::NewFile);
        type_text(&mut app, "z");
        let _ = app.activate(first);
        assert_eq!(text(&app), "abc\n");
        assert_eq!(app.editor.content.cursor_position(), (0, 3));
        assert!(app.is_dirty);
    }

    #[test]
    fn a_failed_session_write_is_reported_and_retried() {
        let mut app = app();
        app.last_session = 7;
        app.session_rev = 3;
        let _ = app.handle(Message::SessionWritten(Err("disk full".into())));
        assert!(app.status_is_error);
        assert_eq!((app.last_session, app.session_rev), (0, 0));
    }

    #[test]
    fn a_second_window_does_not_write_the_session() {
        let mut app = app();
        app.session_owner = false;
        assert!(app.save_session_now().is_ok());
        assert!(app.session_write_job().is_none());
    }
}
