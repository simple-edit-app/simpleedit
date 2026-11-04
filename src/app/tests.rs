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

    #[test]
    fn the_recents_submenu_opens_and_closes_with_the_menu() {
        let mut app = app();
        let _ = app.handle(Message::ToggleMenu(TopMenu::File));
        let _ = app.handle(Message::SetRecents(true));
        assert!(app.recents_open);
        let _ = app.handle(Message::SetRecents(false));
        assert!(!app.recents_open);
        let _ = app.handle(Message::SetRecents(true));
        let _ = app.handle(Message::ToggleMenu(TopMenu::File));
        assert!(!app.recents_open);
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
    fn typing_in_a_field_of_the_git_panel_leaves_the_editor_alone() {
        use crate::git_panel::GitMsg;
        let mut app = app();
        type_text(&mut app, "hello");
        let before = text(&app);
        app.git.open = true;
        app.git.tab = crate::git_panel::Tab::Branches;
        let _ = app.handle(Message::Git(GitMsg::RefName("fea".into())));
        let key = |app: &mut SimpleEditApp, key: &str| {
            let _ = app.handle(Message::KeyPressed {
                key: key.into(),
                ctrl: true,
                shift: false,
                alt: false,
            });
        };
        // Ctrl+Z would take back the editor's last typing, Ctrl+A select it.
        key(&mut app, "z");
        key(&mut app, "a");
        assert_eq!(text(&app), before);
        assert!(app.editor.content.selection().is_none());
        // Back in the editor, the shortcuts are its own again.
        let _ = app.handle(Message::EditorAction(Action::Move(
            iced::widget::text_editor::Motion::Left,
        )));
        key(&mut app, "a");
        assert!(app.editor.content.selection().is_some());
    }

    #[test]
    fn the_file_list_is_resized_by_dragging_its_edge() {
        let mut app = app();
        assert!(!app.sidebar_resizing);
        let _ = app.handle(Message::SidebarResizeStart);
        assert!(app.sidebar_resizing);
        let gap = se_theme::PANEL_GAP;
        let _ = app.handle(Message::SidebarResize(300.0 + gap));
        assert_eq!(app.sidebar_width, 300.0);
        // Never thinner than the names need, nor wider than the window allows.
        let _ = app.handle(Message::SidebarResize(5.0));
        assert_eq!(app.sidebar_width, super::SIDEBAR_MIN);
        let _ = app.handle(Message::SidebarResize(1.0e6));
        assert!(app.sidebar_width <= super::SIDEBAR_MAX);
        assert!(app.sidebar_width <= app.config.window_width * 0.6 + 0.01);
        let _ = app.handle(Message::SidebarResize(300.0 + gap));
        let _ = app.handle(Message::SidebarResizeEnd);
        assert!(!app.sidebar_resizing);
        assert_eq!(app.config.sidebar_width, 300.0, "remembered");
    }

    #[test]
    fn the_bottom_panels_are_resized_by_dragging_its_top_edge() {
        let mut app = app();
        let start = app.panel_height;
        let _ = app.handle(Message::PanelResizeStart);
        assert!(app.panel_resizing);
        // The first move only anchors the drag; moving up makes it taller.
        let _ = app.handle(Message::PanelResize(500.0));
        assert_eq!(app.panel_height, start);
        let _ = app.handle(Message::PanelResize(450.0));
        assert_eq!(app.panel_height, start + 50.0);
        // Within the least and the most the window allows.
        let _ = app.handle(Message::PanelResize(1.0e6));
        assert_eq!(app.panel_height, super::PANEL_MIN_HEIGHT);
        let _ = app.handle(Message::PanelResize(-1.0e6));
        assert!(app.panel_height <= app.config.window_height * 0.85 + 0.01);
        let _ = app.handle(Message::PanelResizeEnd);
        assert!(!app.panel_resizing && app.panel_drag.is_none());
    }

    #[test]
    fn a_path_is_shown_with_the_home_folder_as_a_tilde() {
        let home = dirs::home_dir().expect("a home folder");
        assert_eq!(super::display_path(&home.join("work/a.rs")), "~/work/a.rs");
        assert_eq!(super::display_path(Path::new("/etc/hosts")), "/etc/hosts");
    }

    #[test]
    fn deleting_a_file_asks_first_and_git_is_a_choice() {
        use crate::git::testing::{block, Repo};
        let mut app = app();
        // Not in a repository: no checkbox.
        let dir = tempfile::tempdir().unwrap();
        let loose = dir.path().join("loose.txt");
        std::fs::write(&loose, "x").unwrap();
        let _ = app.handle(Message::DeleteAsked(
            loose.clone(),
            block(super::is_tracked(&loose)),
        ));
        assert_eq!(
            app.overlay,
            ActiveOverlay::ConfirmFileDelete(loose.clone(), None)
        );
        let _ = app.handle(Message::DeleteFile(loose.clone(), false));
        assert!(matches!(app.overlay, ActiveOverlay::None));
        assert_eq!(block(super::delete_file(loose.clone(), false)), Ok(false));
        assert!(!loose.exists());
        // Tracked by git: the box is there, checked.
        let repo = Repo::new();
        repo.write("a.txt", "one\n");
        repo.write("b.txt", "two\n");
        repo.commit_all("first");
        let (a, b) = (repo.path().join("a.txt"), repo.path().join("b.txt"));
        assert!(block(super::is_tracked(&a)));
        let _ = app.handle(Message::DeleteAsked(a.clone(), true));
        assert_eq!(
            app.overlay,
            ActiveOverlay::ConfirmFileDelete(a.clone(), Some(true))
        );
        // Unchecked: only the disk.
        let _ = app.handle(Message::ToggleDeleteFromGit(false));
        assert_eq!(
            app.overlay,
            ActiveOverlay::ConfirmFileDelete(a.clone(), Some(false))
        );
        assert_eq!(block(super::delete_file(a.clone(), false)), Ok(false));
        assert!(!a.exists());
        assert!(
            repo.git(&["status", "--porcelain"]).starts_with(" D"),
            "not staged"
        );
        // Checked: git removes it too.
        assert_eq!(block(super::delete_file(b.clone(), true)), Ok(true));
        assert!(repo.git(&["status", "--porcelain"]).contains("D  b.txt"));
        // A file of a repository that git doesn't know: only the disk.
        repo.write("new.txt", "n\n");
        let new = repo.path().join("new.txt");
        assert!(!block(super::is_tracked(&new)));
        assert_eq!(block(super::delete_file(new.clone(), true)), Ok(false));
        assert!(!new.exists());
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

    /// An app whose scratch folder is a temporary one, with a scratch file
    /// open in the editor.
    fn app_on_scratch() -> (SimpleEditApp, tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app();
        app.scratch.dir = dir.path().join("scratches");
        let _ = app.handle(Message::ScratchName("notes".into()));
        let _ = app.handle(Message::ScratchExt(crate::scratch::Ext(Some("md"))));
        let _ = app.handle(Message::ScratchCreate);
        let path = app.scratch.dir.join("notes.md");
        assert!(path.is_file());
        // The read the app asked for.
        let _ = app.handle(Message::TabRead(Ok(Loaded {
            path: path.clone(),
            text: String::new(),
            format: TextFormat::default(),
            stamp: DiskStamp::of(&path),
        })));
        (app, dir, path)
    }

    #[test]
    fn a_scratch_file_made_without_a_name_is_numbered() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app();
        app.scratch.dir = dir.path().join("scratches");
        app.scratch.ext = crate::scratch::Ext(Some("md"));
        for _ in 0..2 {
            let _ = app.handle(Message::ScratchCreate);
        }
        assert!(app.scratch.dir.join("scratch1.md").is_file());
        assert!(app.scratch.dir.join("scratch2.md").is_file());
        assert_eq!(
            crate::scratch::next_name(&app.scratch.dir, "md"),
            "scratch3"
        );
        // Another extension has its own numbering.
        assert_eq!(
            crate::scratch::next_name(&app.scratch.dir, "py"),
            "scratch1"
        );
        assert!(app.scratch.error.is_none());
    }

    #[test]
    fn a_scratch_file_is_made_open_and_kept_out_of_the_file_list() {
        let (app, _dir, path) = app_on_scratch();
        assert_eq!(app.current_file.as_ref(), Some(&path));
        assert!(!app.sidebar.files().contains(&path));
        assert_eq!(app.scratch.files, vec![path]);
        assert!(app.scratch.name.is_empty());
    }

    #[test]
    fn a_scratch_file_is_saved_by_itself_and_never_closes() {
        let (mut app, _dir, path) = app_on_scratch();
        type_text(&mut app, "hello");
        assert!(app.is_dirty);
        let _ = app.handle(Message::AutoSave);
        assert!(!app.is_dirty);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello\n");
        let _ = app.handle(Message::CloseFile);
        assert_eq!(app.current_file.as_ref(), Some(&path));
    }

    #[test]
    fn typing_in_a_scratch_file_is_written_before_leaving_it() {
        let (mut app, _dir, path) = app_on_scratch();
        type_text(&mut app, "kept");
        let other = PathBuf::from("/x/other.txt");
        app.stash_current();
        app.load_buffer(other, CachedBuffer::blank());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "kept\n");
    }

    #[test]
    fn deleting_a_scratch_file_removes_it_and_its_buffer() {
        let (mut app, _dir, path) = app_on_scratch();
        let _ = app.handle(Message::ScratchAskDelete(path.clone()));
        // Nothing goes before the user confirms.
        assert!(matches!(
            app.overlay,
            ActiveOverlay::ConfirmScratchDelete(_)
        ));
        assert!(path.exists());
        let _ = app.handle(Message::ScratchDelete(path.clone()));
        assert!(!path.exists());
        assert!(app.scratch.files.is_empty());
        assert_ne!(app.current_file.as_ref(), Some(&path));
    }

    #[test]
    fn changing_the_folder_moves_the_scratch_files_along() {
        let (mut app, dir, path) = app_on_scratch();
        type_text(&mut app, "moved");
        let target = dir.path().join("elsewhere");
        let _ = app.handle(Message::SetScratchDir(
            target.to_string_lossy().into_owned(),
        ));
        assert!(!path.exists());
        assert_eq!(
            std::fs::read_to_string(target.join("notes.md")).unwrap(),
            "moved\n"
        );
        assert_eq!(app.scratch.dir, target);
        assert_eq!(app.scratch.files, vec![target.join("notes.md")]);
        assert_eq!(app.config.scratch_dir, target.to_string_lossy());
    }

    #[test]
    fn a_double_click_on_a_bookmarked_line_goes_there() {
        use super::bookmark_panel::Target;
        let mut app = app();
        let path = app.current_file.clone().unwrap();
        type_text(&mut app, "one\ntwo\nthree");
        app.bookmarks.toggle(&path, 0);
        let target = Target::Line(path, 0);
        // One click only selects.
        let _ = app.handle(Message::BookmarkClick(target.clone()));
        assert_eq!(app.bookmark_panel.selected.as_ref(), Some(&target));
        assert_eq!(app.editor.content.cursor_position().0, 2);
        let _ = app.handle(Message::BookmarkClick(target));
        assert_eq!(app.editor.content.cursor_position().0, 0);
    }

    #[test]
    fn bookmarks_are_deleted_from_the_panel_and_files_added_from_the_list() {
        use super::bookmark_panel::Target;
        let mut app = app();
        let file = PathBuf::from("/x/a.rs");
        app.bookmarks.toggle(&file, 4);
        let _ = app.handle(Message::ToggleFileBookmark(file.clone()));
        assert!(app.bookmarks.marked_paths().contains(&file));
        let _ = app.handle(Message::BookmarkDelete(Target::Line(file.clone(), 4)));
        assert!(app.bookmarks.lines(&file).is_empty());
        // The file itself stays until its own row is deleted.
        assert!(app.bookmarks.marked_paths().contains(&file));
        let _ = app.handle(Message::BookmarkDelete(Target::File(file.clone())));
        assert!(app.bookmarks.entries().is_empty());
    }

    #[test]
    fn a_background_tab_changed_on_disk_keeps_its_old_text_undoable() {
        let mut app = app();
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        std::fs::write(&file, "base\n").unwrap();
        let loaded = |text: &str| Loaded {
            path: file.clone(),
            text: text.into(),
            format: TextFormat::default(),
            stamp: DiskStamp::of(&file),
        };
        let _ = app.handle(Message::TabRead(Ok(loaded("base\n"))));
        type_text(&mut app, "edit");
        let edited = text(&app);
        // Another tab takes the editor; the file is rolled back meanwhile.
        app.is_dirty = false;
        app.stash_current();
        app.load_buffer(dir.path().join("other.txt"), CachedBuffer::blank());
        let _ = app.handle(Message::TabRead(Ok(loaded("base\n"))));
        assert_eq!(text(&app), "base\n");
        let _ = app.handle(Message::Undo);
        assert_eq!(text(&app), edited);
    }

    #[test]
    fn ctrl_z_in_a_diff_tab_only_goes_back_over_the_files_of_that_tab() {
        use crate::git::{Change, Kind, Source};
        use crate::git_panel::{DiffTab, Entry};
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
        std::fs::write(&a, "a mine\n").unwrap();
        std::fs::write(&b, "b mine\n").unwrap();
        let mut app = app();
        let _ = app.handle(Message::OpenDiffTab(DiffTab {
            repo: dir.path().to_path_buf(),
            title: "a.txt".into(),
            entries: vec![Entry {
                source: Source::Unstaged,
                change: Change {
                    path: "a.txt".into(),
                    from: None,
                    kind: Kind::Modified,
                },
                staged: false,
            }],
            start: 0,
        }));
        // b.txt was rolled back last (from elsewhere), a.txt before it.
        app.remember_file(a.clone());
        std::fs::write(&a, "a head\n").unwrap();
        app.remember_file(b.clone());
        std::fs::write(&b, "b head\n").unwrap();
        let _ = app.handle(Message::Undo);
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "a mine\n");
        assert_eq!(
            std::fs::read_to_string(&b).unwrap(),
            "b head\n",
            "not this tab's file"
        );
    }

    #[test]
    fn ctrl_z_in_a_diff_brings_back_what_a_rollback_removed() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        std::fs::write(&file, "mine\n").unwrap();
        let mut app = app();
        app.current_file = Some(PathBuf::from("diff://1/Commit"));
        app.git.repo = Some(dir.path().to_path_buf());
        // A rollback rewrites the file: what it held is kept first.
        app.remember_file(file.clone());
        std::fs::write(&file, "head\n").unwrap();
        let _ = app.handle(Message::Undo);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "mine\n");
        let _ = app.handle(Message::Redo);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "head\n");
        // Nothing more to go back to.
        let _ = app.handle(Message::Redo);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "head\n");
    }

    #[test]
    fn the_git_menu_of_a_file_commits_it_alone_or_shows_its_history() {
        use super::git_file::FileAction;
        let mut app = app();
        let file = PathBuf::from("/repo/src/a.rs");
        let root = Some(PathBuf::from("/repo"));
        let _ = app.handle(Message::GitFileMenu(file.clone()));
        assert!(matches!(app.overlay, ActiveOverlay::GitFile(_)));
        // Commit: the panel opens on the Commit tab, for that file only.
        let _ = app.handle(Message::GitFileRoot(
            file.clone(),
            FileAction::Commit,
            root.clone(),
        ));
        assert!(app.git.open);
        assert_eq!(app.git.only.as_deref(), Some("src/a.rs"));
        // History: the Log tab, restricted to the file.
        let _ = app.handle(Message::GitFileRoot(file, FileAction::History, root));
        assert_eq!(app.git.tab, crate::git_panel::Tab::Log);
        assert_eq!(app.git.log.file.as_deref(), Some("src/a.rs"));
    }

    #[test]
    fn choosing_a_revision_opens_a_diff_of_the_file_against_it() {
        let mut app = app();
        app.overlay = ActiveOverlay::GitPick(Box::new(super::git_file::Pick {
            repo: PathBuf::from("/repo"),
            file: "src/a.rs".into(),
            revisions: true,
            items: Vec::new(),
            filter: String::new(),
        }));
        let _ = app.handle(Message::GitPickChoose("0123456789abcdef".into()));
        // The revision is checked first, off the interface thread.
        assert!(matches!(app.overlay, ActiveOverlay::None));
        let _ = app.handle(Message::GitPickVerified(Ok((
            PathBuf::from("/repo"),
            "src/a.rs".into(),
            "0123456789abcdef".into(),
        ))));
        let tab = app.current_file.clone().unwrap();
        let state = app.diff_tabs.get(&tab).expect("a diff tab");
        let entry = &state.viewer.entries[0];
        assert_eq!(
            entry.source,
            crate::git::Source::Tree("0123456789abcdef".into())
        );
        assert_eq!(entry.change.path, "src/a.rs");
        assert!(tab.to_string_lossy().contains("a.rs ↔ 01234567"));
    }

    #[test]
    fn a_commit_diff_can_be_compared_with_the_current_file_and_back() {
        use crate::git::{Change, Kind, Source};
        use crate::git_panel::{DiffTab, Entry};
        let mut app = app();
        let sha = "0123456789abcdef".to_string();
        let _ = app.handle(Message::OpenDiffTab(DiffTab {
            repo: PathBuf::from("/repo"),
            title: "a commit".into(),
            entries: vec![Entry {
                source: Source::Commit(sha.clone()),
                change: Change {
                    path: "a.rs".into(),
                    from: None,
                    kind: Kind::Modified,
                },
                staged: false,
            }],
            start: 0,
        }));
        let tab = app.current_file.clone().unwrap();
        let source = |app: &SimpleEditApp| app.diff_tabs[&tab].viewer.entries[0].source.clone();
        let _ = app.handle(Message::DiffTab(DiffNav::ToggleCompare));
        assert_eq!(source(&app), Source::Tree(sha.clone()));
        let _ = app.handle(Message::DiffTab(DiffNav::ToggleCompare));
        assert_eq!(source(&app), Source::Commit(sha));
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
    fn a_definition_in_the_same_file_selects_the_name() {
        let mut app = app();
        type_text(&mut app, "x");
        let here = app.current_file.clone().unwrap();
        let hit = crate::definition::Hit {
            path: here,
            line: 0,
            col: 0,
            len: 1,
        };
        app.overlay = ActiveOverlay::References(RefList::searching("x".into()));
        let _ = app.handle(Message::DefinitionFound(
            "x".into(),
            crate::lsp::Outcome::Found(hit),
        ));
        assert_eq!(app.editor.content.selection().as_deref(), Some("x"));
    }

    #[test]
    fn a_definition_in_an_open_tab_switches_to_it() {
        let mut app = app();
        let first = app.current_file.clone().unwrap();
        type_text(&mut app, "call()");
        let _ = app.handle(Message::NewFile);
        let second = app.current_file.clone().unwrap();
        type_text(&mut app, "fn target() {}");
        let _ = app.activate(first);
        let hit = crate::definition::Hit {
            path: second.clone(),
            line: 0,
            col: 3,
            len: 6,
        };
        app.overlay = ActiveOverlay::References(RefList::searching("target".into()));
        let _ = app.handle(Message::DefinitionFound(
            "target".into(),
            crate::lsp::Outcome::Found(hit),
        ));
        assert_eq!(app.current_file.as_ref(), Some(&second));
        assert_eq!(app.editor.content.selection().as_deref(), Some("target"));
        assert!(app.pending_jump.is_none());
    }

    #[test]
    fn back_and_forward_walk_through_the_jumps() {
        let mut app = app();
        type_text(&mut app, "a\nb\nc");
        let _ = app.handle(Message::OpenGotoLine);
        app.goto_line_input = "1".into();
        let _ = app.handle(Message::GotoLineSubmit);
        assert_eq!(app.editor.content.cursor_position().0, 0);
        let _ = app.handle(Message::NavBack);
        assert_eq!(app.editor.content.cursor_position(), (2, 1));
        let _ = app.handle(Message::NavForward);
        assert_eq!(app.editor.content.cursor_position().0, 0);
        // Nothing further forward.
        let _ = app.handle(Message::NavForward);
        assert_eq!(app.editor.content.cursor_position().0, 0);
    }

    #[test]
    fn back_returns_to_the_tab_a_definition_was_followed_from() {
        let mut app = app();
        let first = app.current_file.clone().unwrap();
        type_text(&mut app, "call()");
        let _ = app.handle(Message::NewFile);
        let second = app.current_file.clone().unwrap();
        type_text(&mut app, "fn target() {}");
        let _ = app.activate(first.clone());
        let hit = crate::definition::Hit {
            path: second.clone(),
            line: 0,
            col: 3,
            len: 6,
        };
        app.overlay = ActiveOverlay::References(RefList::searching("target".into()));
        let _ = app.handle(Message::DefinitionFound(
            "target".into(),
            crate::lsp::Outcome::Found(hit),
        ));
        assert_eq!(app.current_file.as_ref(), Some(&second));
        let _ = app.handle(Message::NavBack);
        assert_eq!(app.current_file.as_ref(), Some(&first));
        let _ = app.handle(Message::NavForward);
        assert_eq!(app.current_file.as_ref(), Some(&second));
    }

    #[test]
    fn references_are_listed_and_a_click_goes_to_one() {
        let mut app = app();
        type_text(&mut app, "ab\ncd");
        let here = app.current_file.clone().unwrap();
        let reference = |line, col| crate::lsp::Reference {
            hit: crate::definition::Hit {
                path: here.clone(),
                line,
                col,
                len: 1,
            },
            preview: "x".into(),
        };
        let list = vec![reference(0, 1), reference(1, 0)];
        app.overlay = ActiveOverlay::References(RefList::searching("ab".into()));
        let _ = app.handle(Message::DefinitionFound(
            "ab".into(),
            crate::lsp::Outcome::References(list),
        ));
        assert!(matches!(app.overlay, ActiveOverlay::References(_)));
        let key = |name: &str| Message::KeyPressed {
            key: name.into(),
            ctrl: false,
            shift: false,
            alt: false,
        };
        let _ = app.handle(key("ArrowDown"));
        let ActiveOverlay::References(shown) = &app.overlay else {
            panic!("the list closed")
        };
        assert_eq!((shown.word.as_str(), shown.selected), ("ab", 1));
        let _ = app.handle(key("ArrowDown"));
        let _ = app.handle(key("ArrowUp"));
        let _ = app.handle(key("Return"));
        assert!(matches!(app.overlay, ActiveOverlay::None));
        assert_eq!(app.editor.content.selection().as_deref(), Some("c"));
        let _ = app.handle(Message::NavBack);
        assert_eq!(app.editor.content.cursor_position(), (1, 2));
    }

    #[test]
    fn the_install_offer_gives_way_to_the_next_message_and_to_the_install() {
        let mut app = app();
        let offer = || crate::lsp_install::SERVERS.first();
        app.server_offer = offer();
        app.set_status("other", false);
        assert!(app.server_offer.is_none());
        // The button starts the install and says so in the bar.
        app.server_offer = offer();
        let binary = offer().unwrap().binary;
        let _ = app.handle(Message::InstallServer(binary));
        assert!(app.server_offer.is_none());
        assert!(app.servers.installing.contains(binary));
        assert!(app.status_message.contains(binary));
    }

    #[test]
    fn the_cross_beside_a_file_closes_it_without_asking_even_when_modified() {
        let mut app = app();
        type_text(&mut app, "unsaved work");
        let path = app.current_file.clone().unwrap();
        assert!(app.is_path_dirty(&path));
        let _ = app.handle(Message::Sidebar(crate::sidebar::SidebarMessage::CloseNow(
            path.clone(),
        )));
        assert!(matches!(app.overlay, ActiveOverlay::None));
        // The work is gone; the last tab is replaced by a blank one.
        assert!(!app.is_dirty);
        assert_eq!(crate::editor::buffer_text(&app.editor.content), "");
    }

    #[test]
    fn closing_the_other_tabs_keeps_the_active_one_and_asks_once_for_modified_ones() {
        let mut app = app();
        type_text(&mut app, "first");
        let first = app.current_file.clone().unwrap();
        let _ = app.handle(Message::NewFile);
        let second = app.current_file.clone().unwrap();
        let _ = app.handle(Message::NewFile);
        let third = app.current_file.clone().unwrap();
        assert_eq!(app.sidebar.files().len(), 3);
        // The active (third) tab stays; the modified first one needs an answer.
        let _ = app.handle(Message::CloseOtherTabs);
        assert!(matches!(
            app.overlay,
            ActiveOverlay::ConfirmCloseMany(ref paths, 1) if paths == &[first.clone(), second.clone()]
        ));
        assert_eq!(app.sidebar.files().len(), 3);
        let _ = app.handle(Message::ConfirmDiscardMany(vec![first, second]));
        assert!(matches!(app.overlay, ActiveOverlay::None));
        assert_eq!(app.sidebar.files(), std::slice::from_ref(&third));
        assert_eq!(app.current_file, Some(third));
    }

    #[test]
    fn closing_clean_tabs_needs_no_answer_and_all_leaves_a_blank_tab() {
        let mut app = app();
        let _ = app.handle(Message::NewFile);
        let _ = app.handle(Message::NewFile);
        let keep = app.sidebar.files()[0].clone();
        let _ = app.handle(Message::CloseTabs(Some(keep.clone())));
        assert!(matches!(app.overlay, ActiveOverlay::None));
        assert_eq!(app.sidebar.files(), &[keep]);
        let _ = app.handle(Message::CloseTabs(None));
        assert!(matches!(app.overlay, ActiveOverlay::None));
        // The editor is never left without a tab.
        assert_eq!(app.sidebar.files().len(), 1);
        assert!(app.current_file.is_some());
    }

    #[test]
    fn a_click_outside_closes_the_open_menu() {
        let mut app = app();
        let _ = app.handle(Message::ToggleMenu(TopMenu::File));
        assert!(app.open_menu.is_some());
        let _ = app.handle(Message::CloseMenu);
        assert!(app.open_menu.is_none());
    }

    fn diff_tab(title: &str) -> crate::git_panel::DiffTab {
        crate::git_panel::DiffTab {
            repo: PathBuf::from("/tmp/no-such-repo"),
            title: title.into(),
            entries: Vec::new(),
            start: 0,
        }
    }

    #[test]
    fn a_diff_opens_in_an_editor_tab_that_is_not_a_file_and_is_not_saved() {
        let mut app = app();
        let _ = app.handle(Message::OpenDiffTab(diff_tab("abc1234 fix")));
        let path = app.current_file.clone().unwrap();
        assert!(is_diff(&path));
        assert!(is_untitled(&path), "no disk access for it");
        assert!(app.diff_tabs.contains_key(&path));
        assert_eq!(display_name(&path), "Δ abc1234 fix");
        // The session forgets it.
        let session = app.build_session();
        assert!(session.files.iter().all(|f| !f.path.starts_with("diff://")));
        assert!(session
            .active_file
            .as_deref()
            .is_none_or(|p| !p.starts_with("diff://")));
        // The same title reuses the tab.
        let before = app.sidebar.files().len();
        let _ = app.handle(Message::OpenDiffTab(diff_tab("abc1234 fix")));
        assert_eq!(app.sidebar.files().len(), before);
        // Closing it forgets its state.
        let _ = app.handle(Message::Sidebar(crate::sidebar::SidebarMessage::CloseNow(
            path.clone(),
        )));
        assert!(!app.diff_tabs.contains_key(&path));
    }

    #[test]
    fn the_git_panel_and_the_terminal_take_turns_at_the_bottom() {
        let mut app = app();
        let _ = app.handle(Message::ToggleGit);
        assert!(app.git.open);
        let _ = app.handle(Message::ToggleTerminal);
        assert!(app.terminal.open);
        assert!(!app.git.open, "one tool panel at a time");
        let _ = app.handle(Message::ToggleGit);
        assert!(app.git.open);
        assert!(!app.terminal.open);
    }

    #[test]
    fn editing_the_text_drops_the_blame_column() {
        let mut app = app();
        type_text(&mut app, "one\ntwo");
        let path = app.current_file.clone().unwrap();
        app.blame = Some(BlameState {
            path,
            repo: PathBuf::from("/tmp"),
            relative: "a".into(),
            annotations: crate::code_editor::Annotations {
                lines: std::sync::Arc::new(Vec::new()),
                text: iced::Color::BLACK,
                details: std::sync::Arc::new(Vec::new()),
                link: iced::Color::BLACK,
                card_text: iced::Color::BLACK,
                card_background: iced::Color::BLACK,
                card_border: iced::Color::BLACK,
            },
            shas: vec![],
        });
        assert!(app.blame_shown());
        type_text(&mut app, "x");
        assert!(app.blame.is_none());
    }

    #[test]
    fn a_right_click_in_the_gutter_is_told_from_one_in_the_text() {
        let mut app = app();
        type_text(&mut app, "a\nb");
        let origin = crate::editor::text_origin(&app.config, 2, false);
        let _ = app.handle(Message::EditorRightClick(iced::Point::new(
            origin.x - 20.0,
            origin.y + 5.0,
        )));
        assert!(app.gutter_click);
        let _ = app.handle(Message::EditorRightClick(iced::Point::new(
            origin.x + 30.0,
            origin.y + 5.0,
        )));
        assert!(!app.gutter_click);
    }

    #[test]
    fn the_git_panel_refreshing_itself_does_not_close_an_open_menu() {
        use crate::git_panel::GitMsg;
        let mut app = app();
        let _ = app.handle(Message::ToggleMenu(TopMenu::File));
        assert!(app.open_menu.is_some());
        // What the open panel does by itself every two seconds.
        let _ = app.update(Message::Git(GitMsg::Tick));
        let _ = app.update(Message::Git(GitMsg::StatusLoaded(Ok(Default::default()))));
        let _ = app.update(Message::Git(GitMsg::Done(Ok(String::new()))));
        assert!(app.open_menu.is_some());
        // A click on the panel is the user's: it closes the menu.
        let _ = app.update(Message::Git(GitMsg::SelectTab(crate::git_panel::Tab::Log)));
        assert!(app.open_menu.is_none());
    }

    /// A modified file with a place on disk, edited a while ago.
    fn dirty_real_file(app: &mut SimpleEditApp, dir: &Path) -> PathBuf {
        let path = dir.join("a.txt");
        std::fs::write(&path, "one\n").unwrap();
        app.current_file = Some(path.clone());
        app.disk_stamp = DiskStamp::of(&path);
        app.is_dirty = true;
        app.last_edit = std::time::Instant::now() - std::time::Duration::from_secs(5);
        path
    }

    fn app_with_autosave_off() -> SimpleEditApp {
        let config = Config {
            auto_save: false,
            ..Config::default()
        };
        let mut app = SimpleEditApp::blank(config);
        let _ = app.restore_session(crate::session::Session::default());
        app
    }

    #[test]
    fn a_modified_file_is_saved_by_itself_after_a_pause() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app();
        dirty_real_file(&mut app, dir.path());
        let _ = app.handle(Message::AutoSave);
        assert!(app.autosaving, "a save was started");
    }

    #[test]
    fn autosave_waits_for_the_pause_and_respects_the_setting_and_untitled_files() {
        let dir = tempfile::tempdir().unwrap();
        // Still typing.
        let mut typing = app();
        dirty_real_file(&mut typing, dir.path());
        typing.last_edit = std::time::Instant::now();
        let _ = typing.handle(Message::AutoSave);
        assert!(!typing.autosaving);
        // Turned off.
        let mut off = app_with_autosave_off();
        dirty_real_file(&mut off, dir.path());
        let _ = off.handle(Message::AutoSave);
        assert!(!off.autosaving);
        // Nothing to save: an untitled tab is not a file.
        let mut untitled = app();
        untitled.is_dirty = true;
        untitled.last_edit = std::time::Instant::now() - std::time::Duration::from_secs(5);
        let _ = untitled.handle(Message::AutoSave);
        assert!(!untitled.autosaving);
    }

    #[test]
    fn autosave_never_writes_over_a_file_that_changed_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app();
        let path = dirty_real_file(&mut app, dir.path());
        std::fs::write(&path, "changed elsewhere, and longer\n").unwrap();
        let _ = app.handle(Message::AutoSave);
        assert!(!app.autosaving);
    }

    #[test]
    fn changed_lines_are_marked_against_what_head_has() {
        let mut app = app();
        type_text(&mut app, "one\ntwo\nthree");
        let path = app.current_file.clone().unwrap();
        app.marks = Some(MarksState {
            path,
            base: Some("one\ntwo\n".into()),
            marks: Default::default(),
            paint: Default::default(),
            rev: (u64::MAX, u64::MAX),
        });
        app.refresh_marks_now();
        let paint = app.change_paint().expect("something changed");
        assert_eq!(paint.added.as_slice(), &[2]);
        // Back to what HEAD has: nothing to paint.
        app.marks.as_mut().unwrap().base = Some("one\ntwo\nthree".into());
        app.marks.as_mut().unwrap().rev = (u64::MAX, u64::MAX);
        app.refresh_marks_now();
        assert!(app.change_paint().is_none());
    }

    fn conflicted(operation: Option<crate::git::Operation>, files: usize) -> crate::git::Status {
        crate::git::Status {
            operation,
            branch: "main".into(),
            upstream: None,
            ahead: 0,
            behind: 0,
            staged: vec![],
            unstaged: (0..files)
                .map(|i| crate::git::Change {
                    path: format!("f{i}.txt"),
                    from: None,
                    kind: crate::git::Kind::Conflict,
                })
                .collect(),
        }
    }

    #[test]
    fn conflicts_are_announced_until_they_are_gone_or_the_user_closes_the_popup() {
        let mut app = app();
        assert!(app.conflict_toast(true).is_none(), "nothing to announce");
        app.git.status = Some(conflicted(Some(crate::git::Operation::Rebase), 2));
        assert!(app.conflict_toast(true).is_some());
        assert!(app.git.conflict_summary().unwrap().contains('2'));
        // Closed by the user: it stays closed while these conflicts last.
        let _ = app.handle(Message::CloseConflictToast);
        assert!(app.conflict_toast(true).is_none());
        app.git.status = Some(conflicted(Some(crate::git::Operation::Rebase), 1));
        let _ = app.update(Message::CloseMenu);
        assert!(app.conflict_toast(true).is_none());
        // The operation is given up: nothing left, so the next conflicts
        // are announced again.
        app.git.status = Some(conflicted(None, 0));
        let _ = app.update(Message::CloseMenu);
        assert!(app.conflict_toast(true).is_none());
        app.git.status = Some(conflicted(Some(crate::git::Operation::CherryPick), 1));
        let _ = app.update(Message::CloseMenu);
        assert!(app.conflict_toast(true).is_some());
        // The button opens the dialog that settles them, and the pop-up
        // gives way to it.
        let _ = app.handle(Message::OpenConflicts);
        assert!(matches!(app.overlay, ActiveOverlay::Conflicts));
        assert!(app.conflict_toast(true).is_none());
    }

    fn app_with_a_changed_line() -> SimpleEditApp {
        let mut app = app();
        type_text(&mut app, "one\nTWO\nthree");
        let path = app.current_file.clone().unwrap();
        app.marks = Some(MarksState {
            path,
            base: Some("one\ntwo\nthree\n".into()),
            marks: Default::default(),
            paint: Default::default(),
            rev: (u64::MAX, u64::MAX),
        });
        app.refresh_marks_now();
        app
    }

    #[test]
    fn clicking_a_change_bar_opens_the_button_and_the_button_puts_the_line_back() {
        let mut app = app_with_a_changed_line();
        let _ = app.handle(Message::ChangeBar(1));
        assert_eq!(app.change_paint().unwrap().revert, Some(1));
        let _ = app.handle(Message::RevertChange);
        assert_eq!(
            crate::editor::buffer_text(&app.editor.content).trim_end(),
            "one\ntwo\nthree"
        );
        assert!(app.revert.is_none());
        // And Ctrl+Z brings the change back.
        let _ = app.handle(Message::Undo);
        assert_eq!(
            crate::editor::buffer_text(&app.editor.content).trim_end(),
            "one\nTWO\nthree"
        );
    }

    #[test]
    fn the_button_closes_on_a_second_click_on_the_bar_or_elsewhere_or_after_typing() {
        let mut app = app_with_a_changed_line();
        let _ = app.handle(Message::ChangeBar(1));
        let _ = app.handle(Message::ChangeBar(1));
        assert_eq!(app.change_paint().unwrap().revert, None);
        let _ = app.handle(Message::ChangeBar(1));
        let _ = app.handle(Message::CloseRevert);
        assert_eq!(app.change_paint().unwrap().revert, None);
        // Typing moves the lines under it: it gives up.
        let _ = app.handle(Message::ChangeBar(1));
        type_text(&mut app, "x");
        app.refresh_marks_now();
        assert_eq!(app.change_paint().and_then(|p| p.revert), None);
    }

    fn merge_spec() -> crate::git_panel::MergeSpec {
        let text = "a\n<<<<<<< HEAD\nmine\n=======\ntheirs\n>>>>>>> t\nb\n";
        crate::git_panel::MergeSpec {
            repo: PathBuf::from("/tmp/no-such-repo"),
            path: "dir/f.txt".into(),
            merge: crate::git::conflicts::parse(text, false).unwrap(),
        }
    }

    #[test]
    fn a_conflicted_file_opens_in_a_tab_that_is_not_a_file_and_is_not_remembered() {
        let mut app = app();
        let _ = app.handle(Message::OpenMergeTab(std::sync::Arc::new(merge_spec())));
        let path = app.current_file.clone().unwrap();
        assert!(is_merge(&path) && is_virtual(&path) && is_untitled(&path));
        assert_eq!(display_name(&path), "⇄ f.txt");
        assert!(app.merge_tabs.contains_key(&path));
        let session = app.build_session();
        assert!(session
            .files
            .iter()
            .all(|f| !f.path.starts_with("merge://")));
        // The same file reuses its tab.
        let before = app.sidebar.files().len();
        let _ = app.handle(Message::OpenMergeTab(std::sync::Arc::new(merge_spec())));
        assert_eq!(app.sidebar.files().len(), before);
        // Settling a conflict in it, then closing it.
        let _ = app.handle(Message::Merge(crate::git_panel::merge::MergeMsg::All(
            crate::git::conflicts::Side::Yours,
        )));
        assert_eq!(app.merge_tabs[&path].merge.unresolved(), 0);
        let _ = app.handle(Message::Sidebar(crate::sidebar::SidebarMessage::CloseNow(
            path.clone(),
        )));
        assert!(!app.merge_tabs.contains_key(&path));
    }

    #[test]
    fn the_conflicts_dialog_only_opens_when_there_are_conflicts() {
        let mut app = app();
        let _ = app.handle(Message::OpenConflicts);
        assert!(matches!(app.overlay, ActiveOverlay::None));
        let mut status = crate::git::Status::default();
        status.unstaged.push(crate::git::Change {
            path: "a.txt".into(),
            from: None,
            kind: crate::git::Kind::Conflict,
        });
        app.git.status = Some(status);
        let _ = app.handle(Message::OpenConflicts);
        assert!(matches!(app.overlay, ActiveOverlay::Conflicts));
        // The status bar leads there too.
        let offer = app.status_offer().expect("an offer");
        assert!(matches!(offer.2, Message::OpenConflicts));
    }

    #[test]
    fn a_definition_never_used_says_so() {
        let mut app = app();
        app.overlay = ActiveOverlay::References(RefList::searching("lonely".into()));
        let _ = app.handle(Message::DefinitionFound(
            "lonely".into(),
            crate::lsp::Outcome::AtDefinition,
        ));
        assert!(empty_reason(&app).contains("lonely"));
    }

    #[test]
    fn a_missing_definition_is_reported() {
        let mut app = app();
        app.overlay = ActiveOverlay::References(RefList::searching("nothing".into()));
        let _ = app.handle(Message::DefinitionFound(
            "nothing".into(),
            crate::lsp::Outcome::NotFound,
        ));
        assert!(empty_reason(&app).contains("nothing"));
        assert!(!app.status_message.contains("nothing"));
    }

    /// Why the usages popup has nothing to list.
    fn empty_reason(app: &SimpleEditApp) -> String {
        match &app.overlay {
            ActiveOverlay::References(RefList {
                state: RefState::Empty(why),
                ..
            }) => why.clone(),
            other => panic!("not an empty popup: {other:?}"),
        }
    }

    #[test]
    fn the_popup_opens_while_searching_and_a_closed_one_drops_the_answer() {
        let mut app = app();
        type_text(&mut app, "name");
        let _ = app.handle(Message::GotoDefinition);
        // No file yet: nothing starts; with one, the popup shows the wait.
        app.overlay = ActiveOverlay::References(RefList::searching("name".into()));
        let _ = app.handle(Message::CloseOverlay);
        let _ = app.handle(Message::DefinitionFound(
            "name".into(),
            crate::lsp::Outcome::NotFound,
        ));
        assert!(matches!(app.overlay, ActiveOverlay::None));
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

    #[test]
    fn a_blame_line_shows_its_commit_like_an_ide_does() {
        let line = crate::git::history::BlameLine {
            sha: "cc695242ccbf0dde1002b50be37f9bb58b0385ed".into(),
            author: "Olivier Sandré".into(),
            email: "olivier@example.fr".into(),
            time: 1_784_000_000,
            summary: "Oc 540".into(),
            message: std::sync::Arc::from("Oc 540\n\nA second paragraph that is long enough to need wrapping because it goes well past the width of a readable line of text."),
        };
        let card = super::commit_card(&line);
        assert!(card[0].starts_with("commit cc695242"));
        assert!(card[1].contains("Olivier Sandré") && card[1].contains("<olivier@example.fr>"));
        assert!(card[2].starts_with("Date"));
        assert_eq!(card[3], "");
        assert_eq!(card[4], "Oc 540");
        // The long paragraph is wrapped, nothing is lost.
        let wrapped: Vec<&String> = card[6..].iter().collect();
        assert!(wrapped.len() >= 2);
        assert!(wrapped.iter().all(|l| l.chars().count() <= 72));
        assert!(wrapped
            .iter()
            .map(|l| l.as_str())
            .collect::<Vec<_>>()
            .join(" ")
            .ends_with("readable line of text."));
        // Without the whole message, the summary stands in.
        let short = crate::git::history::BlameLine {
            message: std::sync::Arc::from(""),
            ..line
        };
        assert_eq!(super::commit_card(&short)[4], "Oc 540");
    }

    #[test]
    fn the_way_to_pull_and_merge_is_chosen_in_the_panel_or_the_preferences_and_kept() {
        let mut app = app();
        assert_eq!(app.git.pull_mode, crate::git::PullMode::Merge);
        // Chosen behind the chevron: the modes close, nothing is run.
        app.git.branches.menu =
            Some((crate::git_panel::branches::Split::Pull, iced::Point::ORIGIN));
        let _ = app.handle(Message::SetPullMode(crate::git::PullMode::Rebase));
        assert!(app.git.branches.menu.is_none());
        assert!(!app.git.busy);
        let _ = app.handle(Message::SetMergeMode(crate::git::MergeMode::NoFastForward));
        assert_eq!(app.git.pull_mode, crate::git::PullMode::Rebase);
        assert_eq!(app.config.git_pull_mode, crate::git::PullMode::Rebase);
        assert_eq!(app.git.merge_mode, crate::git::MergeMode::NoFastForward);
        assert_eq!(
            app.config.git_merge_mode,
            crate::git::MergeMode::NoFastForward
        );
        assert!(app.config_dirty, "written with the next tick");
        // A config file keeps them, in words.
        let text = serde_json::to_string(&app.config).unwrap();
        assert!(text.contains("rebase") && text.contains("no-fast-forward"));
    }

    fn press(app: &mut SimpleEditApp, key: &str, ctrl: bool, shift: bool, alt: bool) {
        let _ = app.handle(Message::KeyPressed {
            key: key.into(),
            ctrl,
            shift,
            alt,
        });
    }

    #[test]
    fn the_ide_shortcuts_edit_lines() {
        let mut app = app();
        type_text(&mut app, "one\ntwo\nthree");
        // Ctrl+D duplicates the line, Ctrl+Y deletes it.
        press(&mut app, "d", true, false, false);
        assert_eq!(text(&app).trim_end(), "one\ntwo\nthree\nthree");
        press(&mut app, "y", true, false, false);
        assert_eq!(text(&app).trim_end(), "one\ntwo\nthree");
        // Alt+Shift+Up moves it above the one before.
        press(&mut app, "ArrowUp", false, true, true);
        assert_eq!(text(&app).trim_end(), "one\nthree\ntwo");
        // Ctrl+Z goes back, Ctrl+Shift+Z forward.
        press(&mut app, "z", true, false, false);
        assert_eq!(text(&app).trim_end(), "one\ntwo\nthree");
        press(&mut app, "z", true, true, false);
        assert_eq!(text(&app).trim_end(), "one\nthree\ntwo");
    }

    #[test]
    fn the_git_shortcuts_open_the_panel() {
        let mut app = app();
        press(&mut app, "k", true, false, false);
        assert!(app.git.open);
        assert_eq!(app.git.tab, crate::git_panel::Tab::Commit);
        // Bookmarks panel: Shift+F11.
        press(&mut app, "F11", false, true, false);
        assert!(app.bookmark_panel.open);
    }

    #[test]
    fn the_remote_is_fetched_by_itself_at_the_chosen_pace_and_new_commits_are_told() {
        let mut app = app();
        app.git.repo = Some(PathBuf::from("/tmp"));
        app.config.git_auto_fetch_minutes = 1;
        // One minute is sixty ticks of a second.
        app.ticks = 59;
        let _ = app.handle(Message::AutoSave);
        assert_eq!(app.ticks, 60);
        // Never: the pace is only a setting.
        let _ = app.handle(Message::SetAutoFetch(0));
        assert_eq!(app.config.git_auto_fetch_minutes, 0);
        assert!(app.config_dirty);
        // New commits on the remote show in the status bar.
        let mut before = conflicted(None, 0);
        before.upstream = Some("origin/main".into());
        let mut after = before.clone();
        after.behind = 3;
        app.git.status = Some(after);
        let _ = app.after_message(false, Some(Some(before)), false);
        assert!(app.status_message.contains('3'));
    }

    #[test]
    fn the_branch_in_the_status_bar_opens_a_list_to_switch_or_make_a_branch() {
        let mut app = app();
        // Not in a repository: nothing to switch.
        let _ = app.handle(Message::OpenBranchSwitcher);
        assert!(matches!(app.overlay, ActiveOverlay::None));
        app.git.repo = Some(PathBuf::from("/tmp"));
        app.git.status = Some(conflicted(None, 0));
        app.git.log.refs = Some(crate::git::history::Refs {
            branches: vec![crate::git::history::Branch {
                name: "main".into(),
                sha: String::new(),
                upstream: None,
                current: true,
                remote: false,
                ahead: 0,
                behind: 0,
            }],
            tags: vec![],
        });
        press(&mut app, "b", true, true, false);
        assert_eq!(app.overlay, ActiveOverlay::BranchSwitch(String::new()));
        let _ = app.handle(Message::BranchSwitchFilter("fea".into()));
        assert_eq!(app.overlay, ActiveOverlay::BranchSwitch("fea".into()));
        // A name no branch has: made, then switched to.
        let _ = app.handle(Message::BranchSwitchChoose("feature-x".into(), true));
        assert!(matches!(app.overlay, ActiveOverlay::None));
        assert!(app.git.busy);
        // An empty name does nothing.
        app.git.busy = false;
        let _ = app.handle(Message::OpenBranchSwitcher);
        let _ = app.handle(Message::BranchSwitchChoose(String::new(), false));
        assert!(!app.git.busy);
    }

    #[test]
    fn what_goes_wrong_with_the_panel_closed_is_told_in_the_status_bar() {
        let mut app = app();
        assert!(!app.git.open);
        let _ = app.handle(Message::Git(crate::git_panel::GitMsg::Done(Err(
            "push refused".into(),
        ))));
        assert!(app.status_message.contains("push refused"));
        assert!(app.status_is_error);
        // A conflict is not a failure: the pop-up says it.
        app.set_status("calm", false);
        let _ = app.handle(Message::Git(crate::git_panel::GitMsg::Done(Err(
            crate::git::CONFLICT.into(),
        ))));
        assert_eq!(app.status_message, "calm");
    }

    #[test]
    fn the_previous_version_of_a_line_opens_in_a_tab_of_its_own_with_its_blame() {
        let mut app = app();
        let line = |sha: &str, summary: &str| crate::git::history::BlameLine {
            sha: sha.into(),
            author: "A".into(),
            email: "a@example.com".into(),
            time: 0,
            summary: summary.into(),
            message: std::sync::Arc::from(""),
        };
        let lines = vec![
            line(&"a".repeat(40), "first"),
            line(&"b".repeat(40), "second"),
        ];
        let _ = app.handle(Message::BlamePreviousLoaded(Ok((
            PathBuf::from("/repo"),
            "src/f.rs".into(),
            "12345678".into(),
            "one\ntwo\n".into(),
            lines,
        ))));
        let tab = app.current_file.clone().unwrap();
        assert_eq!(display_name(&tab), "⟲ f.rs @ 12345678");
        assert!(is_untitled(&tab), "never written to disk by mistake");
        assert_eq!(text(&app), "one\ntwo\n");
        // Its blame is there: one note per line, the hashes kept for the next step back.
        assert!(app.blame_shown());
        assert_eq!(app.blame.as_ref().unwrap().shas.len(), 2);
        // A line a commit made can go back again; one not committed yet can't.
        app.gutter_line = Some(1);
        let task = app.handle(Message::BlamePrevious);
        let _ = task;
        // An error says why and opens nothing.
        let before = app.current_file.clone();
        let _ = app.handle(Message::BlamePreviousLoaded(Err("no such path".into())));
        assert_eq!(app.current_file, before);
        assert!(app.status_message.contains("no such path"));
    }
}
