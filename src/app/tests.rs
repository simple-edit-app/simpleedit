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
    use crate::database::Done;
    use text_editor::{Action, Edit};

    fn app() -> SimpleEditApp {
        let mut app = SimpleEditApp::blank(Config::default());
        let _ = app.restore_session(crate::session::Session::default());
        app
    }

    #[test]
    fn what_comes_by_itself_or_from_passing_over_leaves_a_menu_open() {
        let mut app = app();
        let _ = app.handle(Message::ToggleMenu(TopMenu::Edit));
        assert_eq!(app.open_menu, Some(TopMenu::Edit));
        for message in [
            Message::FileStates(Vec::new()),
            Message::TreeHover(PathBuf::from("/x")),
            Message::TreeScrolled(0.0, 100.0),
            Message::LintHover(None),
            Message::LintCardHover(false),
            Message::LintCloseCheck(0),
            Message::FinderFiles(Vec::new()),
            Message::FindResults(0, Ok(Vec::new())),
        ] {
            let _ = app.update(message);
            assert_eq!(app.open_menu, Some(TopMenu::Edit));
        }
        // A real action closes it.
        let _ = app.update(Message::ToggleSidebar);
        assert_eq!(app.open_menu, None);
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
        // The list starts after the bar of panels and its hairline.
        let gap = super::ACTIVITY_WIDTH + 1.0;
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

    fn open_text(app: &mut SimpleEditApp, path: &Path, text: &str) {
        let _ = app.update(Message::TabRead(Ok(Loaded {
            path: path.to_path_buf(),
            text: text.into(),
            format: TextFormat::default(),
            stamp: None,
        })));
    }

    fn pane_files(app: &SimpleEditApp) -> Vec<PathBuf> {
        let split = app.split.as_ref().expect("split");
        split
            .layout_ids()
            .iter()
            .map(|id| split.panes[id].file.clone())
            .collect()
    }

    fn paste(text: &str) -> text_editor::Action {
        text_editor::Action::Edit(text_editor::Edit::Paste(std::sync::Arc::new(text.into())))
    }

    #[test]
    fn the_editor_splits_in_many_panes_that_swap_on_focus() {
        let mut app = app();
        let (a, b) = (PathBuf::from("/s/a.txt"), PathBuf::from("/s/b.txt"));
        open_text(&mut app, &a, "alpha\n");
        // The same file in both panes: the new one is a copy that follows.
        let _ = app.update(Message::Split(true));
        assert_eq!(pane_files(&app), vec![a.clone(), a.clone()]);
        let _ = app.update(Message::EditorAction(paste("x")));
        assert_eq!(text(&app), "xalpha\n");
        let other = app.split.as_ref().unwrap().layout_ids()[1];
        assert_eq!(
            app.pane_content_mut(other).map(|c| c.text()),
            Some("xalpha\n".to_string())
        );
        // Another split, below: three panes in a tree.
        let _ = app.update(Message::Split(false));
        assert_eq!(app.split.as_ref().unwrap().panes.len(), 3);
        // Another file in the active pane: the others keep `a`, parked.
        open_text(&mut app, &b, "beta\n");
        assert_eq!(app.current_file.as_ref(), Some(&b));
        assert_eq!(pane_files(&app), vec![b.clone(), a.clone(), a.clone()]);
        // Clicking in another pane brings its file to the editor.
        let id = app.split.as_ref().unwrap().layout_ids()[2];
        let _ = app.update(Message::SplitFocus(id));
        assert_eq!(app.current_file.as_ref(), Some(&a));
        assert_eq!(app.split.as_ref().unwrap().active, id);
        assert_eq!(text(&app), "xalpha\n");
        // Closing the file of a pane takes the pane away, the split too once
        // one pane is left.
        let _ = app.update(Message::CloseOtherTabs);
        assert_eq!(pane_files(&app), vec![a.clone(), a.clone()]);
        // Closing the active pane gives the focus to another one.
        let _ = app.update(Message::CloseSplit);
        assert!(app.split.is_none());
        assert_eq!(app.current_file.as_ref(), Some(&a));
    }

    #[test]
    fn a_pane_that_is_not_the_first_can_be_split_too() {
        let mut app = app();
        open_text(&mut app, Path::new("/s/a.txt"), "alpha\n");
        let _ = app.update(Message::Split(true));
        let second = app.split.as_ref().unwrap().layout_ids()[1];
        let _ = app.update(Message::SplitFocus(second));
        let _ = app.update(Message::Split(true));
        assert_eq!(app.split.as_ref().unwrap().panes.len(), 3);
        let _ = app.update(Message::Split(false));
        assert_eq!(app.split.as_ref().unwrap().panes.len(), 4);
    }

    #[test]
    fn a_file_of_the_list_opens_in_a_new_pane() {
        let mut app = app();
        let (a, b) = (PathBuf::from("/s/a.txt"), PathBuf::from("/s/b.txt"));
        open_text(&mut app, &a, "alpha\n");
        open_text(&mut app, &b, "beta\n");
        let _ = app.update(Message::Sidebar(crate::sidebar::SidebarMessage::OpenFile(
            a.clone(),
        )));
        let _ = app.update(Message::SplitWith(b.clone(), false));
        assert_eq!(app.current_file.as_ref(), Some(&b));
        let split = app.split.as_ref().expect("split");
        assert_eq!(pane_files(&app), vec![a, b]);
        assert_ne!(split.layout_ids()[0], split.active);
    }

    #[test]
    fn a_scroll_stays_in_its_pane_and_a_click_takes_the_focus() {
        let mut app = app();
        let a = PathBuf::from("/s/a.txt");
        open_text(&mut app, &a, &"line\n".repeat(200));
        let _ = app.update(Message::Split(true));
        let (first, second) = {
            let ids = app.split.as_ref().unwrap().layout_ids();
            (ids[0], ids[1])
        };
        let _ = app.update(Message::SplitAction(
            second,
            text_editor::Action::Scroll { lines: 5 },
        ));
        assert_eq!(app.split.as_ref().unwrap().active, first, "no focus change");
        let _ = app.update(Message::SplitAction(
            second,
            text_editor::Action::Move(text_editor::Motion::Down),
        ));
        assert_eq!(app.split.as_ref().unwrap().active, second);
    }

    #[test]
    fn a_folder_added_becomes_a_project_that_is_kept_and_followed() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/main.rs"), "fn main() {}\n").unwrap();
        let mut app = app();
        let _ = app.update(Message::ProjectPicked(Some(dir.path().to_path_buf())));
        assert_eq!(app.projects.roots, vec![dir.path().to_path_buf()]);
        assert_eq!(
            app.config.projects, app.projects.roots,
            "kept in the settings"
        );
        // A folder of the tree opens, and the settings remember it.
        let src = dir.path().join("src");
        let _ = app.update(Message::ToggleProjectFolder(src.clone()));
        assert!(app.config.projects_open.contains(&src));
        assert!(app
            .projects
            .rows()
            .iter()
            .any(|r| r.entry.name == "main.rs"));
        // The same folder again, or none chosen: nothing changes.
        let _ = app.update(Message::ProjectPicked(Some(dir.path().to_path_buf())));
        let _ = app.update(Message::ProjectPicked(None));
        assert_eq!(app.projects.roots.len(), 1);
        // A file of the tree opens like any file of the list.
        let file = src.join("main.rs");
        let _ = app.update(Message::Sidebar(crate::sidebar::SidebarMessage::OpenFile(
            file.clone(),
        )));
        assert_eq!(app.pending_open.as_ref(), Some(&file));
        // A file of the list can be shown in the tree: its folders open.
        let _ = app.update(Message::ToggleProjectFolder(src.clone()));
        assert!(!app
            .projects
            .rows()
            .iter()
            .any(|r| r.entry.name == "main.rs"));
        let _ = app.update(Message::ShowInProjects(file.clone()));
        assert!(app
            .projects
            .rows()
            .iter()
            .any(|r| r.entry.name == "main.rs"));
        // What appears on the disk shows up by itself.
        std::fs::write(src.join("lib.rs"), "").unwrap();
        let _ = app.update(Message::RefreshProjects);
        assert!(app.projects.rows().iter().any(|r| r.entry.name == "lib.rs"));
        // The next run starts with the same projects, folders open as they were.
        let again = SimpleEditApp::blank(app.config.clone());
        assert_eq!(again.projects.roots, app.projects.roots);
        assert!(again
            .projects
            .rows()
            .iter()
            .any(|r| r.entry.name == "main.rs"));
        // Taking it out of the sidebar deletes nothing.
        let _ = app.update(Message::RemoveProject(dir.path().to_path_buf()));
        assert!(app.projects.roots.is_empty() && app.config.projects.is_empty());
        assert!(file.is_file());
    }

    #[test]
    fn the_bar_shows_one_panel_at_a_time_and_remembers_it() {
        use crate::sidebar::Panel;
        let mut app = app();
        assert!(app.panels.files && !app.panels.projects);
        // Asking for the other one takes its place.
        let _ = app.update(Message::TogglePanel(Panel::Projects));
        assert!(!app.panels.files && app.panels.projects);
        assert!(app.config.show_projects_panel && !app.config.show_files_panel);
        let _ = app.update(Message::TogglePanel(Panel::Files));
        assert!(app.panels.files && !app.panels.projects);
        // Asking for the one that is open closes it.
        let _ = app.update(Message::TogglePanel(Panel::Files));
        assert!(!app.panels.files && !app.panels.projects);
        // The sidebar was hidden: asking for a panel shows it again.
        app.show_sidebar = false;
        let _ = app.update(Message::TogglePanel(Panel::Projects));
        assert!(app.show_sidebar && app.panels.projects && app.config.show_sidebar);
        // The next run starts as it was left.
        let again = SimpleEditApp::blank(app.config.clone());
        assert!(!again.panels.files && again.panels.projects);
        // An old setting with both open keeps the projects, once there are some.
        let mut old = Config {
            show_files_panel: true,
            show_projects_panel: true,
            ..Config::default()
        };
        let none = SimpleEditApp::blank(old.clone());
        assert!(none.panels.files && !none.panels.projects);
        old.projects = vec![PathBuf::from("/tmp")];
        let some = SimpleEditApp::blank(old);
        assert!(!some.panels.files && some.panels.projects);
    }

    #[test]
    fn a_tab_dragged_elsewhere_keeps_its_new_place() {
        let mut app = app();
        for name in ["a", "b", "c", "d"] {
            let _ = app.update(Message::TabRead(Ok(Loaded {
                path: PathBuf::from(format!("/s/{name}.txt")),
                text: String::new(),
                format: TextFormat::default(),
                stamp: None,
            })));
        }
        let order = |app: &SimpleEditApp| -> Vec<String> {
            app.sidebar
                .files()
                .iter()
                .filter(|p| p.starts_with("/s"))
                .map(|p| p.file_stem().unwrap().to_string_lossy().into_owned())
                .collect()
        };
        let first = app
            .sidebar
            .files()
            .iter()
            .position(|p| p.starts_with("/s"))
            .unwrap();
        let _ = app.update(Message::TabMoved(first, first + 3));
        assert_eq!(order(&app), ["b", "c", "d", "a"]);
        // It is the order that is saved.
        let saved: Vec<String> = app
            .build_session()
            .files
            .iter()
            .filter(|f| f.path.starts_with("/s"))
            .map(|f| f.path.clone())
            .collect();
        assert_eq!(saved, ["/s/b.txt", "/s/c.txt", "/s/d.txt", "/s/a.txt"]);
    }

    fn open_named(app: &mut SimpleEditApp, path: &Path) {
        let _ = app.update(Message::TabRead(Ok(Loaded {
            path: path.to_path_buf(),
            text: String::new(),
            format: TextFormat::default(),
            stamp: None,
        })));
    }

    #[test]
    fn a_file_of_the_tree_opens_in_the_preview_tab_until_it_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["a.rs", "b.rs", "c.rs"] {
            std::fs::write(dir.path().join(name), "x\n").unwrap();
        }
        let (a, b, c) = (
            dir.path().join("a.rs"),
            dir.path().join("b.rs"),
            dir.path().join("c.rs"),
        );
        let mut app = app();
        let _ = app.update(Message::TreeOpenFile(a.clone()));
        assert_eq!(app.preview_tab.as_ref(), Some(&a));
        open_named(&mut app, &a);
        // Another file of the tree takes its tab, at its place.
        let place = app.sidebar.files().iter().position(|p| *p == a).unwrap();
        app.tree_click = None;
        let _ = app.update(Message::TreeOpenFile(b.clone()));
        open_named(&mut app, &b);
        assert!(
            !app.sidebar.files().contains(&a),
            "the preview was replaced"
        );
        assert_eq!(
            app.sidebar.files().iter().position(|p| *p == b),
            Some(place)
        );
        assert_eq!(app.preview_tab.as_ref(), Some(&b));
        // Edited, it stays: the next file gets a tab of its own.
        let _ = app.update(Message::EditorAction(text_editor::Action::Edit(
            text_editor::Edit::Insert('y'),
        )));
        assert_eq!(app.preview_tab, None);
        app.tree_click = None;
        let _ = app.update(Message::TreeOpenFile(c.clone()));
        open_named(&mut app, &c);
        assert!(app.sidebar.files().contains(&b) && app.sidebar.files().contains(&c));
        // Clicked twice at once, it is kept (the first click is dated now:
        // opening the file may take longer than a double click).
        app.tree_click = Some((c.clone(), std::time::Instant::now()));
        let _ = app.update(Message::TreeOpenFile(c.clone()));
        assert_eq!(app.preview_tab, None);
        // Or from the menu of its tab.
        app.preview_tab = Some(c.clone());
        let _ = app.update(Message::KeepTab);
        assert_eq!(app.preview_tab, None);
        // Or with two clicks on its tab (on another tab, nothing changes).
        app.preview_tab = Some(c.clone());
        let at = app.sidebar.files().iter().position(|p| *p == c).unwrap();
        let other = app.sidebar.files().iter().position(|p| *p == b).unwrap();
        let _ = app.update(Message::TabDoubleClick(other));
        assert_eq!(app.preview_tab.as_ref(), Some(&c));
        let _ = app.update(Message::TabDoubleClick(at));
        assert_eq!(app.preview_tab, None);
    }

    #[test]
    fn tabs_close_with_the_middle_button_and_come_back_with_ctrl_shift_t() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a.rs"), dir.path().join("b.rs"));
        std::fs::write(&a, "").unwrap();
        std::fs::write(&b, "").unwrap();
        let mut app = app();
        open_named(&mut app, &a);
        open_named(&mut app, &b);
        let at = app.sidebar.files().iter().position(|p| *p == a).unwrap();
        let _ = app.update(Message::CloseTabAt(at));
        assert!(!app.sidebar.files().contains(&a));
        let _ = app.update(Message::ReopenTab);
        assert_eq!(app.pending_open.as_ref(), Some(&a));
        // Nothing left to open again: nothing happens.
        open_named(&mut app, &a);
        app.pending_open = None;
        let _ = app.update(Message::ReopenTab);
        assert_eq!(app.pending_open, None);
    }

    #[test]
    fn ctrl_tab_goes_to_the_tabs_seen_before_while_ctrl_is_held() {
        let mut app = app();
        let tab = |n: &str| PathBuf::from(format!("/s/{n}.txt"));
        for name in ["a", "b", "c"] {
            open_named(&mut app, &tab(name));
        }
        // Seen in the order a, b, c; then b again: b, c, a from the latest.
        let _ = app.update(Message::Sidebar(crate::sidebar::SidebarMessage::OpenFile(
            tab("b"),
        )));
        let ctrl_tab = |app: &mut SimpleEditApp, shift: bool| {
            let _ = app.update(Message::KeyPressed {
                key: "Tab".into(),
                ctrl: true,
                shift,
                alt: false,
            });
        };
        ctrl_tab(&mut app, false);
        assert_eq!(app.current_file, Some(tab("c")), "the one seen before");
        ctrl_tab(&mut app, false);
        assert_eq!(app.current_file, Some(tab("a")), "Ctrl still held: older");
        ctrl_tab(&mut app, true);
        assert_eq!(app.current_file, Some(tab("c")), "and back");
        // Ctrl let go: the tab reached is now the latest.
        let _ = app.update(Message::ModifiersChanged {
            ctrl: false,
            alt: false,
            shift: false,
        });
        ctrl_tab(&mut app, false);
        assert_eq!(app.current_file, Some(tab("b")));
    }

    #[test]
    fn the_list_of_tabs_opens_under_its_button_and_a_pick_closes_it() {
        let mut app = app();
        let file = PathBuf::from("/s/a.txt");
        open_named(&mut app, &file);
        let _ = app.update(Message::OpenTabList(iced::Point::new(900.0, 40.0)));
        assert!(app.tab_list.is_some());
        let _ = app.update(Message::CloseTabList);
        assert!(app.tab_list.is_none());
        let _ = app.update(Message::OpenTabList(iced::Point::new(900.0, 40.0)));
        let _ = app.update(Message::PickTab(file.clone()));
        assert!(app.tab_list.is_none());
        assert_eq!(app.current_file, Some(file));
    }

    #[test]
    fn showing_a_file_in_the_tree_brings_the_projects_up() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/main.rs"), "").unwrap();
        let mut app = app();
        let _ = app.update(Message::ProjectPicked(Some(dir.path().to_path_buf())));
        // The files were on show, the sidebar hidden.
        app.panels = crate::sidebar::Panels {
            files: true,
            projects: false,
            database: false,
        };
        app.show_sidebar = false;
        let file = dir.path().join("src/main.rs");
        let _ = app.update(Message::ShowInProjects(file.clone()));
        assert!(app.show_sidebar && app.panels.projects && !app.panels.files);
        assert!(app.config.show_projects_panel && !app.config.show_files_panel);
        assert!(app.projects.rows().iter().any(|r| r.entry.path == file));
        // A file that is in no project: nothing happens.
        let _ = app.update(Message::TogglePanel(crate::sidebar::Panel::Files));
        let _ = app.update(Message::ShowInProjects(PathBuf::from("/elsewhere/x.rs")));
        assert!(app.panels.files && !app.panels.projects);
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
        assert_eq!(
            block(super::delete_file(loose.clone(), false, 5)),
            Ok(false)
        );
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
        assert_eq!(block(super::delete_file(a.clone(), false, 5)), Ok(false));
        assert!(!a.exists());
        assert!(
            repo.git(&["status", "--porcelain"]).starts_with(" D"),
            "not staged"
        );
        // Checked: git removes it too.
        assert_eq!(block(super::delete_file(b.clone(), true, 5)), Ok(true));
        assert!(repo.git(&["status", "--porcelain"]).contains("D  b.txt"));
        // A file of a repository that git doesn't know: only the disk.
        repo.write("new.txt", "n\n");
        let new = repo.path().join("new.txt");
        assert!(!block(super::is_tracked(&new)));
        assert_eq!(block(super::delete_file(new.clone(), true, 5)), Ok(false));
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
            get: false,
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
            false,
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

    // ---- the Database panel's tables

    /// An app with a table open in a tab and a page of it that can be edited.
    fn db_app(editable: bool) -> (SimpleEditApp, PathBuf) {
        use crate::database::protocol::Column;
        use crate::database::{DbMsg, Dialog, Field, Page};
        let mut app = app();
        // The connections are read from a file the other tests write too.
        app.db.connections.clear();
        let mut form = Dialog::new();
        form.set(Field::User, "dev".into());
        form.set(Field::Password, "secret".into());
        let id = app.db.apply(&form).unwrap();
        let _ = app.update(Message::Db(DbMsg::OpenTable(
            id,
            "shop".into(),
            "public".into(),
            "items".into(),
        )));
        let path = app.db_tabs.keys().next().unwrap().clone();
        let column = |name: &str, kind: &str, nullable: bool| Column {
            name: name.into(),
            kind: kind.into(),
            nullable,
        };
        let page = Page {
            columns: vec![
                column("id", "integer", false),
                column("label", "text", true),
                column("due", "date", true),
            ],
            rows: (1..=3)
                .map(|i| vec![Some(i.to_string()), Some(format!("row {i}")), None])
                .collect(),
            has_more: false,
            primary_key: vec!["id".into()],
            ctids: Vec::new(),
            editable,
            source: None,
            offset: 0,
        };
        let _ = app.update(Message::Db(DbMsg::Rows(path.clone(), Done::ok(page))));
        (app, path)
    }

    #[test]
    fn a_cell_is_typed_in_then_waits_for_the_commit() {
        use crate::database::DbMsg;
        let (mut app, path) = db_app(true);
        let _ = app.update(Message::Db(DbMsg::Edit(path.clone(), 1, 1)));
        let tab = &app.db_tabs[&path];
        assert_eq!(tab.editing.as_ref().map(|e| e.text.as_str()), Some("row 2"));
        let _ = app.update(Message::Db(DbMsg::EditInput(
            path.clone(),
            "renamed".into(),
        )));
        let _ = app.update(Message::Db(DbMsg::EditCommit(path.clone())));
        let tab = &app.db_tabs[&path];
        assert!(tab.editing.is_none());
        assert!(tab.pending.is_edited(1, 1));
        assert_eq!(tab.pending.count(), 1);
        // The page itself is not touched until the database says so.
        assert_eq!(
            tab.page.as_ref().unwrap().rows[1][1].as_deref(),
            Some("row 2")
        );
        let _ = app.update(Message::Db(DbMsg::Commit(path.clone())));
        assert!(app.db_tabs[&path].committing);
        // It went well: the page is read again, clean.
        let _ = app.update(Message::Db(DbMsg::Committed(path.clone(), Done::ok(1))));
        let tab = &app.db_tabs[&path];
        assert!(tab.pending.is_empty() && !tab.committing);
    }

    #[test]
    fn a_failed_commit_keeps_the_changes_and_says_why() {
        use crate::database::DbMsg;
        let (mut app, path) = db_app(true);
        let _ = app.update(Message::Db(DbMsg::DeleteRow(path.clone(), 0)));
        let _ = app.update(Message::Db(DbMsg::Commit(path.clone())));
        let _ = app.update(Message::Db(DbMsg::Committed(
            path.clone(),
            Done::err("change 1: violates a foreign key"),
        )));
        let tab = &app.db_tabs[&path];
        assert!(!tab.committing);
        assert!(tab.pending.is_deleted(0));
        assert_eq!(
            tab.notice.as_deref(),
            Some("change 1: violates a foreign key")
        );
        // Reverting takes everything back.
        let _ = app.update(Message::Db(DbMsg::RevertAll(path.clone())));
        let tab = &app.db_tabs[&path];
        assert!(tab.pending.is_empty() && tab.notice.is_none());
    }

    #[test]
    fn a_page_cannot_be_left_while_changes_wait() {
        use crate::database::DbMsg;
        let (mut app, path) = db_app(true);
        let _ = app.update(Message::Db(DbMsg::DeleteRow(path.clone(), 2)));
        for message in [
            DbMsg::Sort(path.clone(), 1),
            DbMsg::Turn(path.clone(), 1),
            DbMsg::Refresh(path.clone()),
            DbMsg::PageSize(path.clone(), 100),
            DbMsg::ClearFilter(path.clone()),
        ] {
            let _ = app.update(Message::Db(message));
            let tab = &app.db_tabs[&path];
            assert!(tab.pending.is_deleted(2));
            assert!(tab.notice.is_some());
            assert!(tab.order.is_empty() && tab.page_size == 500);
        }
        // With nothing pending, a sort goes through.
        let _ = app.update(Message::Db(DbMsg::RevertAll(path.clone())));
        let _ = app.update(Message::Db(DbMsg::Sort(path.clone(), 1)));
        assert_eq!(app.db_tabs[&path].order, vec![("label".to_string(), false)]);
    }

    #[test]
    fn the_page_size_and_the_filter_are_applied_and_remembered() {
        use crate::database::DbMsg;
        let (mut app, path) = db_app(true);
        let _ = app.update(Message::Db(DbMsg::PageSize(path.clone(), 100)));
        assert_eq!(app.db_tabs[&path].page_size, 100);
        assert_eq!(app.db.page_size, 100);
        // The tables opened next start with it.
        let _ = app.update(Message::Db(DbMsg::OpenTable(
            1,
            "shop".into(),
            "public".into(),
            "other".into(),
        )));
        assert!(app
            .db_tabs
            .values()
            .any(|t| t.table == "other" && t.page_size == 100));
        let _ = app.update(Message::Db(DbMsg::Filter(path.clone(), "  id > 1 ".into())));
        // Typed, not yet applied.
        assert_eq!(app.db_tabs[&path].filter, "");
        let _ = app.update(Message::Db(DbMsg::ApplyFilter(path.clone())));
        assert_eq!(app.db_tabs[&path].filter, "id > 1");
        let _ = app.update(Message::Db(DbMsg::ClearFilter(path.clone())));
        assert!(app.db_tabs[&path].filter.is_empty() && app.db_tabs[&path].filter_text.is_empty());
    }

    #[test]
    fn escape_gives_up_the_cell_and_the_date_being_picked() {
        use crate::database::DbMsg;
        let (mut app, path) = db_app(true);
        let _ = app.update(Message::Db(DbMsg::Edit(path.clone(), 0, 1)));
        assert!(app.db_tabs[&path].editing.is_some());
        let _ = app.update(Message::EscapePressed);
        assert!(app.db_tabs[&path].editing.is_none());
        let _ = app.update(Message::Db(DbMsg::OpenPicker(path.clone(), 0, 2)));
        assert!(app.db_tabs[&path].picker.is_some());
        let _ = app.update(Message::EscapePressed);
        assert!(app.db_tabs[&path].picker.is_none());
        assert!(app.db_tabs[&path].pending.is_empty());
    }

    #[test]
    fn a_date_is_picked_then_becomes_an_edit_and_null_too() {
        use crate::database::grid::TimePart;
        use crate::database::DbMsg;
        let (mut app, path) = db_app(true);
        // Not a date column: no picker.
        let _ = app.update(Message::Db(DbMsg::OpenPicker(path.clone(), 0, 1)));
        assert!(app.db_tabs[&path].picker.is_none());
        let _ = app.update(Message::Db(DbMsg::OpenPicker(path.clone(), 0, 2)));
        let _ = app.update(Message::Db(DbMsg::PickerShift(path.clone(), 1)));
        let _ = app.update(Message::Db(DbMsg::PickerDay(path.clone(), 15)));
        // A date has no time to type: this is ignored in the value.
        let _ = app.update(Message::Db(DbMsg::PickerTime(
            path.clone(),
            TimePart::Hour,
            "07".into(),
        )));
        let _ = app.update(Message::Db(DbMsg::PickerApply(path.clone())));
        let tab = &app.db_tabs[&path];
        assert!(tab.picker.is_none());
        let value = tab.pending.value(tab.page.as_ref().unwrap(), 0, 2);
        assert!(value.is_some_and(|v| v.len() == 10 && v.ends_with("-15")));
        // NULL, on a column that takes it and one that does not.
        let _ = app.update(Message::Db(DbMsg::SetNull(path.clone(), 1, 1)));
        assert!(app.db_tabs[&path].pending.is_edited(1, 1));
        let _ = app.update(Message::Db(DbMsg::SetNull(path.clone(), 1, 0)));
        assert!(!app.db_tabs[&path].pending.is_edited(1, 0));
        assert!(app.db_tabs[&path].notice.is_some());
    }

    #[test]
    fn a_click_gives_the_focus_to_a_cell_and_moves_it() {
        use crate::database::DbMsg;
        let (mut app, path) = db_app(true);
        assert_eq!(app.db_tabs[&path].selected, None);
        let _ = app.update(Message::Db(DbMsg::Select(path.clone(), 1, 2)));
        assert_eq!(app.db_tabs[&path].selected, Some((1, 2)));
        let _ = app.update(Message::Db(DbMsg::Select(path.clone(), 0, 0)));
        assert_eq!(app.db_tabs[&path].selected, Some((0, 0)));
        // Double click types in it, and the focus follows.
        let _ = app.update(Message::Db(DbMsg::Edit(path.clone(), 2, 1)));
        assert_eq!(app.db_tabs[&path].selected, Some((2, 1)));
        // A click on another cell keeps what was typed and leaves the cell.
        let _ = app.update(Message::Db(DbMsg::EditInput(path.clone(), "typed".into())));
        let _ = app.update(Message::Db(DbMsg::Select(path.clone(), 0, 1)));
        let tab = &app.db_tabs[&path];
        assert!(tab.editing.is_none() && tab.pending.is_edited(2, 1));
        // The focus is not kept when the page changes.
        let _ = app.update(Message::Db(DbMsg::RevertAll(path.clone())));
        assert_eq!(app.db_tabs[&path].selected, Some((0, 1)));
    }

    #[test]
    fn the_console_opens_and_takes_the_place_of_the_other_bottom_panels() {
        use crate::database::DbMsg;
        let (mut app, _) = db_app(true);
        assert!(!app.db.console.open);
        let _ = app.update(Message::Db(DbMsg::ToggleConsole));
        assert!(app.db.console.open);
        // The connection to run on is the first one.
        assert_eq!(
            app.db.console.connection,
            app.db.connections.first().map(|c| c.id)
        );
        let _ = app.update(Message::ToggleScratch);
        assert!(app.scratch.open && !app.db.console.open);
        let _ = app.update(Message::Db(DbMsg::ToggleConsole));
        assert!(app.db.console.open && !app.scratch.open);
        let _ = app.update(Message::Db(DbMsg::ToggleConsole));
        assert!(!app.db.console.open);
    }

    #[test]
    fn what_the_driver_ran_goes_to_the_log_and_the_end_is_shown() {
        use crate::database::{DbMsg, Executed};
        let (mut app, path) = db_app(true);
        let executed = vec![
            Executed {
                sql: "SELECT * FROM \"public\".\"items\" LIMIT 500 OFFSET 0".into(),
                ms: 3,
                rows: Some(3),
                error: None,
                internal: false,
            },
            Executed {
                sql: "DELETE FROM t WHERE id = '1'".into(),
                ms: 1,
                rows: None,
                error: Some("violates a foreign key".into()),
                internal: false,
            },
        ];
        let page = app.db_tabs[&path].page.clone().unwrap();
        let done = Done {
            result: Ok(page),
            executed: executed.clone(),
        };
        let _ = app.update(Message::Db(DbMsg::Rows(path.clone(), done)));
        let log = &app.db.console.log;
        assert_eq!(log.len(), 2);
        assert_eq!(log[0].sql, executed[0].sql);
        assert_eq!(log[1].error.as_deref(), Some("violates a foreign key"));
        assert_eq!(log[0].connection, "localhost");
        // Clearing empties it.
        let _ = app.update(Message::Db(DbMsg::ClearLog));
        assert!(app.db.console.log.is_empty());
    }

    #[test]
    fn a_select_opens_a_tab_and_the_rest_says_what_it_did() {
        use crate::database::{DbMsg, Executed, QueryResult, Ran};
        let (mut app, _) = db_app(true);
        let conn = app.db.connections[0].id;
        let select = QueryResult {
            sql: "SELECT a, b FROM t".into(),
            ms: 2,
            has_rows: true,
            columns: vec!["a".into(), "b".into()],
            rows: vec![vec![Some("1".into()), None]],
            affected: 1,
            truncated: false,
            deferred: false,
        };
        let update = QueryResult {
            sql: "UPDATE t SET b = 'x'".into(),
            ms: 1,
            has_rows: false,
            columns: vec![],
            rows: vec![],
            affected: 3,
            truncated: false,
            deferred: false,
        };
        let executed = vec![
            Executed {
                sql: select.sql.clone(),
                ms: 2,
                rows: Some(1),
                error: None,
                internal: false,
            },
            Executed {
                sql: update.sql.clone(),
                ms: 1,
                rows: Some(3),
                error: None,
                internal: false,
            },
        ];
        let tabs_before = app.db_tabs.len();
        let _ = app.update(Message::Db(DbMsg::Ran(
            conn,
            Done {
                result: Ok(Ran {
                    in_transaction: false,
                    results: vec![select, update],
                    failed: None,
                    position: None,
                }),
                executed,
            },
        )));
        // One tab, for the select, that cannot be edited and knows its query.
        assert_eq!(app.db_tabs.len(), tabs_before + 1);
        let tab = app.db_tabs.values().find(|t| t.query.is_some()).unwrap();
        assert_eq!(tab.query.as_deref(), Some("SELECT a, b FROM t"));
        assert!(!tab.editable());
        assert_eq!(tab.page.as_ref().unwrap().rows.len(), 1);
        // Both are in the log; the last run went well.
        assert_eq!(app.db.console.log.len(), 2);
        assert_eq!(app.db.console.last.as_ref().map(|l| l.0), Some(true));
        assert!(!app.db.console.running);
    }

    #[test]
    fn a_result_that_cannot_be_paged_is_never_run_again() {
        use crate::database::{DbMsg, Executed, QueryResult, Ran};
        let (mut app, _) = db_app(true);
        let conn = app.db.connections[0].id;
        let insert = QueryResult {
            sql: "INSERT INTO t VALUES (1) RETURNING id".into(),
            ms: 1,
            has_rows: true,
            columns: vec!["id".into()],
            rows: vec![vec![Some("1".into())]],
            affected: 0,
            truncated: false,
            deferred: false,
        };
        let _ = app.update(Message::Db(DbMsg::Ran(
            conn,
            Done {
                result: Ok(Ran {
                    in_transaction: false,
                    results: vec![insert],
                    failed: None,
                    position: None,
                }),
                executed: Vec::<Executed>::new(),
            },
        )));
        let path = app
            .db_tabs
            .iter()
            .find(|(_, t)| t.query.is_some())
            .map(|(p, _)| p.clone())
            .unwrap();
        // Reload does nothing: the write would be made twice.
        let _ = app.update(Message::Db(DbMsg::Refresh(path.clone())));
        let tab = &app.db_tabs[&path];
        assert!(!tab.loading && tab.page.as_ref().unwrap().rows.len() == 1);
    }

    #[test]
    fn an_open_transaction_is_shown_and_cancelling_ends_the_session() {
        use crate::database::{DbMsg, Ran};
        let (mut app, _) = db_app(true);
        let conn = app.db.connections[0].id;
        app.db.console.connection = Some(conn);
        let _ = app.db.live_of(conn);
        let _ = app.update(Message::Db(DbMsg::Ran(
            conn,
            Done {
                result: Ok(Ran {
                    in_transaction: true,
                    results: vec![],
                    failed: None,
                    position: None,
                }),
                executed: vec![],
            },
        )));
        assert!(app.db.console.in_transaction);
        // Another database, another session: the transaction is gone.
        let _ = app.update(Message::Db(DbMsg::ConsoleDatabase("other".into())));
        assert!(!app.db.console.in_transaction);
        // A run that is stopped takes its session with it.
        app.db.console.running = true;
        app.db.console.in_transaction = true;
        let _ = app.update(Message::Db(DbMsg::Cancel));
        assert!(!app.db.console.in_transaction && !app.db.live.contains_key(&conn));
        // Removing a data source ends its session too.
        let _ = app.db.live_of(conn);
        app.db.remove(conn);
        assert!(app.db.live.is_empty());
    }

    #[test]
    fn a_failed_query_shows_the_log_and_says_which_statement() {
        use crate::database::console::ConsoleTab;
        use crate::database::{DbMsg, Executed, Ran};
        let (mut app, _) = db_app(true);
        let conn = app.db.connections[0].id;
        app.db.console.tab = ConsoleTab::Queries;
        let _ = app.update(Message::Db(DbMsg::Ran(
            conn,
            Done {
                result: Ok(Ran {
                    in_transaction: false,
                    results: vec![],
                    failed: Some((1, "syntax error".into())),
                    position: None,
                }),
                executed: vec![Executed {
                    sql: "SELEKT 1".into(),
                    ms: 0,
                    rows: None,
                    error: Some("syntax error".into()),
                    internal: false,
                }],
            },
        )));
        let (ok, message) = app.db.console.last.clone().unwrap();
        assert!(!ok && message.contains('2') && message.contains("syntax error"));
        assert_eq!(app.db.console.tab, ConsoleTab::Log);
        assert_eq!(app.db.console.log.len(), 1);
        // No tab came of it.
        assert!(app.db_tabs.values().all(|t| t.query.is_none()));
    }

    #[test]
    fn running_without_a_connection_or_a_query_says_so() {
        use crate::database::DbMsg;
        let mut app = app();
        app.db.connections.clear();
        let _ = app.update(Message::Db(DbMsg::Run(true)));
        assert_eq!(app.db.console.last.as_ref().map(|l| l.0), Some(false));
        assert!(!app.db.console.running);
        let (mut app, _) = db_app(true);
        app.db.console.editor = iced::widget::text_editor::Content::with_text("  \n ");
        let _ = app.update(Message::Db(DbMsg::Run(false)));
        assert_eq!(app.db.console.last.as_ref().map(|l| l.0), Some(false));
        assert!(!app.db.console.running);
        // With something written, it runs.
        app.db.console.editor = iced::widget::text_editor::Content::with_text("SELECT 1");
        let _ = app.update(Message::Db(DbMsg::Run(false)));
        assert!(app.db.console.running);
    }

    #[test]
    fn a_risky_statement_waits_for_a_yes_before_it_runs() {
        use crate::database::DbMsg;
        let (mut app, _) = db_app(true);
        app.db.console.editor = iced::widget::text_editor::Content::with_text("DELETE FROM t");
        let _ = app.update(Message::Db(DbMsg::Run(false)));
        assert!(!app.db.console.running);
        assert!(app.db.console.guard.is_some());
        // Giving up leaves nothing waiting.
        let _ = app.update(Message::Db(DbMsg::GuardCancel));
        assert!(app.db.console.guard.is_none() && !app.db.console.running);
        // A yes runs it.
        let _ = app.update(Message::Db(DbMsg::Run(false)));
        let _ = app.update(Message::Db(DbMsg::GuardRun));
        assert!(app.db.console.running && app.db.console.guard.is_none());
    }

    #[test]
    fn on_a_production_source_a_change_with_a_where_waits_too() {
        use crate::database::DbMsg;
        let (mut app, _) = db_app(true);
        app.db.console.editor =
            iced::widget::text_editor::Content::with_text("UPDATE t SET a = 1 WHERE id = 2");
        let _ = app.update(Message::Db(DbMsg::Run(false)));
        assert!(app.db.console.running, "not production: it runs");
        app.db.console.running = false;
        let id = app.db.connections[0].id;
        app.db.connections[0].production = true;
        app.db.console.connection = Some(id);
        let _ = app.update(Message::Db(DbMsg::Run(false)));
        assert!(app.db.console.guard.is_some() && !app.db.console.running);
    }

    #[test]
    fn an_sql_file_runs_on_the_data_source_of_the_console() {
        use crate::database::DbMsg;
        let (mut app, _) = db_app(true);
        app.editor.content = crate::code_editor::Content::with_text("SELECT 1;\nSELECT 2");
        // Not an SQL file: nothing runs.
        app.editor.language = Some("md".into());
        let _ = app.update(Message::Db(DbMsg::RunFile));
        assert!(!app.db.console.running);
        app.editor.language = Some("sql".into());
        let _ = app.update(Message::Db(DbMsg::RunFile));
        assert!(app.db.console.running && app.db.console.open);
        assert_eq!(app.db.console.running_sql, vec!["SELECT 1", "SELECT 2"]);
    }

    #[test]
    fn shift_extends_a_block_of_cells_that_is_copied_as_a_whole() {
        use crate::database::DbMsg;
        let (mut app, path) = db_app(true);
        let _ = app.update(Message::Db(DbMsg::Select(path.clone(), 0, 0)));
        assert!(app.db_tabs[&path].block().is_none());
        // Shift and an arrow grow the block from where it started.
        assert!(app.db_key("ArrowDown", false, true, false).is_some());
        assert!(app.db_key("ArrowRight", false, true, false).is_some());
        assert_eq!(app.db_tabs[&path].block(), Some(((0, 0), (1, 1))));
        // A move without Shift leaves it.
        assert!(app.db_key("ArrowDown", false, false, false).is_some());
        assert!(app.db_tabs[&path].block().is_none());
        // Shift and a click do the same.
        let _ = app.update(Message::Db(DbMsg::Select(path.clone(), 0, 0)));
        app.mod_shift = true;
        let _ = app.update(Message::Db(DbMsg::Select(path.clone(), 1, 1)));
        assert_eq!(app.db_tabs[&path].block(), Some(((0, 0), (1, 1))));
        app.mod_shift = false;
        let _ = app.update(Message::Db(DbMsg::Select(path.clone(), 1, 1)));
        assert!(app.db_tabs[&path].block().is_none());
    }

    #[test]
    fn a_row_is_edited_in_a_form_that_moves_from_row_to_row() {
        use crate::database::DbMsg;
        let (mut app, path) = db_app(true);
        let _ = app.update(Message::Db(DbMsg::OpenForm(path.clone(), 1)));
        assert_eq!(app.db_tabs[&path].form, Some(1));
        // A letter typed, then the field emptied: the cell was not NULL.
        let _ = app.update(Message::Db(DbMsg::FormInput(path.clone(), 1, "new".into())));
        assert!(app.db_tabs[&path].pending.is_edited(1, 1));
        let _ = app.update(Message::Db(DbMsg::FormNull(path.clone(), 1)));
        assert_eq!(
            app.db_tabs[&path]
                .pending
                .cell(app.db_tabs[&path].page.as_ref().unwrap(), 1, 1),
            crate::database::grid::Cell::Null
        );
        // The row before and after, and no further than the rows there are.
        let _ = app.update(Message::Db(DbMsg::FormMove(path.clone(), -1)));
        assert_eq!(app.db_tabs[&path].form, Some(0));
        let _ = app.update(Message::Db(DbMsg::FormMove(path.clone(), -1)));
        assert_eq!(app.db_tabs[&path].form, Some(0));
        let _ = app.update(Message::Db(DbMsg::FormMove(path.clone(), 9)));
        assert_eq!(app.db_tabs[&path].form, Some(2));
        // The lists of values arrive; a failure is no list, not an error.
        let _ = app.update(Message::Db(DbMsg::ChoicesLoaded(
            path.clone(),
            Done::err("nope"),
        )));
        assert_eq!(app.db_tabs[&path].choices, Some(Vec::new()));
        // Escape closes it.
        assert!(app.db_escape());
        assert_eq!(app.db_tabs[&path].form, None);
    }

    #[test]
    fn a_form_changes_nothing_in_a_table_that_is_read_only() {
        use crate::database::DbMsg;
        let (mut app, path) = db_app(false);
        let _ = app.update(Message::Db(DbMsg::OpenForm(path.clone(), 0)));
        let _ = app.update(Message::Db(DbMsg::FormInput(path.clone(), 1, "x".into())));
        let _ = app.update(Message::Db(DbMsg::FormNull(path.clone(), 1)));
        assert!(app.db_tabs[&path].pending.is_empty());
    }

    #[test]
    fn the_health_of_a_database_is_read_and_cleaned_from_its_window() {
        use crate::database::protocol::{Health, IndexHealth};
        use crate::database::{DbMsg, HealthTab};
        let (mut app, _) = db_app(true);
        let id = app.db.connections[0].id;
        let _ = app.update(Message::Db(DbMsg::OpenHealth(id, "shop".into())));
        let view = app.db.health.as_ref().expect("the window opens");
        assert!(view.loading && view.data.is_none());
        let data = Health {
            database_size: 1 << 20,
            tables: Vec::new(),
            indexes: vec![IndexHealth {
                schema: "public".into(),
                table: "items".into(),
                name: "items_label_idx".into(),
                size: 8192,
                scans: 0,
                unique: false,
                primary: false,
            }],
        };
        let _ = app.update(Message::Db(DbMsg::HealthLoaded(Done::ok(data))));
        let view = app.db.health.as_ref().unwrap();
        assert!(!view.loading && view.data.is_some() && view.error.is_none());
        let _ = app.update(Message::Db(DbMsg::HealthTab(HealthTab::Indexes)));
        assert_eq!(app.db.health.as_ref().unwrap().tab, HealthTab::Indexes);
        // A clean-up runs one at a time.
        let _ = app.update(Message::Db(DbMsg::HealthRun(
            "VACUUM \"public\".\"items\"".into(),
        )));
        assert!(app.db.health.as_ref().unwrap().running.is_some());
        let _ = app.update(Message::Db(DbMsg::HealthRun(
            "ANALYZE \"public\".\"items\"".into(),
        )));
        assert_eq!(
            app.db.health.as_ref().unwrap().running.as_deref(),
            Some("VACUUM \"public\".\"items\"")
        );
        // A failure is said in the window.
        let _ = app.update(Message::Db(DbMsg::HealthRan(Done::err(
            "permission denied",
        ))));
        let view = app.db.health.as_ref().unwrap();
        assert!(view.running.is_none());
        assert_eq!(view.error.as_deref(), Some("permission denied"));
        // A statement for the console goes there, and the window closes.
        app.db.console.editor = iced::widget::text_editor::Content::new();
        let _ = app.update(Message::Db(DbMsg::HealthToConsole(
            "DROP INDEX \"public\".\"items_label_idx\";".into(),
        )));
        assert!(app.db.health.is_none() && app.db.console.open);
        assert!(app.db.console.editor.text().contains("DROP INDEX"));
        // Escape closes the window.
        let _ = app.update(Message::Db(DbMsg::OpenHealth(id, "shop".into())));
        assert!(app.db_escape() && app.db.health.is_none());
    }

    #[test]
    fn a_file_is_read_mapped_and_imported_into_a_table() {
        use crate::database::protocol::{ColumnDef, Structure};
        use crate::database::DbMsg;
        let (mut app, _) = db_app(true);
        let id = app.db.connections[0].id;
        let _ = app.update(Message::Db(DbMsg::ImportOpen(
            id,
            "shop".into(),
            "public".into(),
            "items".into(),
        )));
        let column = |name: &str, kind: &str, generated: bool| ColumnDef {
            name: name.into(),
            kind: kind.into(),
            nullable: true,
            default: None,
            identity: String::new(),
            generated,
            comment: String::new(),
        };
        let structure = Structure {
            columns: vec![
                column("id", "integer", false),
                column("label", "text", false),
                column("total", "integer", true),
            ],
            ..Structure::default()
        };
        let _ = app.update(Message::Db(DbMsg::ImportStructure(Done::ok(structure))));
        // A generated column cannot be filled.
        assert_eq!(app.db.import.as_ref().unwrap().targets.len(), 2);
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("rows.csv");
        std::fs::write(&file, "ID;Label;other\n1;a;x\n2;b;y\n").unwrap();
        let _ = app.update(Message::Db(DbMsg::ImportPicked(Some(file))));
        let im = app.db.import.as_ref().unwrap();
        assert_eq!(im.delimiter, ';');
        assert_eq!(im.sample.names, vec!["ID", "Label", "other"]);
        assert_eq!(
            im.mapping,
            vec![Some("id".to_string()), Some("label".to_string()), None]
        );
        // A column is filled by one field only.
        let _ = app.update(Message::Db(DbMsg::ImportMap(2, Some("id".into()))));
        let im = app.db.import.as_ref().unwrap();
        assert_eq!(im.mapping[0], None);
        assert_eq!(im.mapping[2].as_deref(), Some("id"));
        // Without a header, the fields are numbered and go by place.
        let _ = app.update(Message::Db(DbMsg::ImportHeader(false)));
        let im = app.db.import.as_ref().unwrap();
        assert_eq!(im.sample.names, vec!["1", "2", "3"]);
        assert_eq!(im.sample.rows.len(), 3);
        let _ = app.update(Message::Db(DbMsg::ImportHeader(true)));
        // Going: one import at a time, and what comes back is said.
        let _ = app.update(Message::Db(DbMsg::ImportRun));
        assert!(app.db.import.as_ref().unwrap().running);
        assert!(!app.db_escape(), "the window stays while it runs");
        let _ = app.update(Message::Db(DbMsg::ImportDone(Done::err("bad value"))));
        let im = app.db.import.as_ref().unwrap();
        assert!(!im.running && im.error.as_deref() == Some("bad value"));
        let _ = app.update(Message::Db(DbMsg::ImportRun));
        let _ = app.update(Message::Db(DbMsg::ImportDone(Done::ok(2))));
        assert_eq!(app.db.import.as_ref().unwrap().imported, Some(2));
        assert!(app.db_escape() && app.db.import.is_none());
    }

    #[test]
    fn a_table_is_copied_to_another_source_from_its_window() {
        use crate::database::{Copied, CopyField, DbMsg};
        let (mut app, _) = db_app(true);
        let id = app.db.connections[0].id;
        let _ = app.update(Message::Db(DbMsg::CopyOpen(
            id,
            "shop".into(),
            "public".into(),
            "items".into(),
        )));
        let copy = app.db.copy_table.as_ref().expect("the window opens");
        assert_eq!(
            (copy.name.as_str(), copy.schema.as_str()),
            ("items", "public")
        );
        assert!(copy.with_data && copy.dest == Some(id));
        // Nothing goes without a name.
        let _ = app.update(Message::Db(DbMsg::CopyField(CopyField::Name, "  ".into())));
        let _ = app.update(Message::Db(DbMsg::CopyRun));
        assert!(!app.db.copy_table.as_ref().unwrap().running);
        let _ = app.update(Message::Db(DbMsg::CopyField(
            CopyField::Name,
            "items2".into(),
        )));
        let _ = app.update(Message::Db(DbMsg::CopyData(false)));
        let _ = app.update(Message::Db(DbMsg::CopyRun));
        assert!(app.db.copy_table.as_ref().unwrap().running);
        // It cannot be closed, or changed, while it runs.
        assert!(!app.db_escape());
        let _ = app.update(Message::Db(DbMsg::CopyField(
            CopyField::Name,
            "other".into(),
        )));
        assert_eq!(app.db.copy_table.as_ref().unwrap().name, "items2");
        let failed = Copied {
            result: Err("relation already exists".into()),
            source_log: Vec::new(),
            dest_log: Vec::new(),
        };
        let _ = app.update(Message::Db(DbMsg::CopyDone(Box::new(failed))));
        let copy = app.db.copy_table.as_ref().unwrap();
        assert!(!copy.running && copy.error.as_deref() == Some("relation already exists"));
        let _ = app.update(Message::Db(DbMsg::CopyRun));
        let done = Copied {
            result: Ok(0),
            source_log: Vec::new(),
            dest_log: Vec::new(),
        };
        let _ = app.update(Message::Db(DbMsg::CopyDone(Box::new(done))));
        assert_eq!(app.db.copy_table.as_ref().unwrap().copied, Some(0));
        assert!(app.db_escape() && app.db.copy_table.is_none());
    }

    #[test]
    fn a_backup_and_a_restore_are_asked_for_in_one_window() {
        use crate::database::{BackupFlag, DbMsg};
        let (mut app, _) = db_app(true);
        let id = app.db.connections[0].id;
        let _ = app.update(Message::Db(DbMsg::BackupOpen(
            id,
            "shop".into(),
            "public".into(),
            "items".into(),
            false,
        )));
        let b = app.db.backup.as_ref().expect("the window opens");
        assert!(!b.restore && b.spec.format == "custom" && b.spec.no_owner);
        // Nothing goes before a file is chosen.
        let _ = app.update(Message::Db(DbMsg::BackupRun));
        assert!(!app.db.backup.as_ref().unwrap().running);
        // Structure only and data only exclude each other.
        let _ = app.update(Message::Db(DbMsg::BackupFlag(BackupFlag::SchemaOnly, true)));
        let _ = app.update(Message::Db(DbMsg::BackupFlag(BackupFlag::DataOnly, true)));
        let spec = &app.db.backup.as_ref().unwrap().spec;
        assert!(spec.data_only && !spec.schema_only);
        let _ = app.update(Message::Db(DbMsg::BackupFlag(BackupFlag::Plain, true)));
        assert_eq!(app.db.backup.as_ref().unwrap().spec.format, "plain");
        let _ = app.update(Message::Db(DbMsg::BackupPicked(Some("/tmp/x.sql".into()))));
        let _ = app.update(Message::Db(DbMsg::BackupRun));
        assert!(app.db.backup.as_ref().unwrap().running);
        assert!(!app.db_escape(), "the window stays while it works");
        let _ = app.update(Message::Db(DbMsg::BackupDone(Done::err(
            "pg_dump was not found",
        ))));
        let b = app.db.backup.as_ref().unwrap();
        assert!(!b.running && b.error.as_deref() == Some("pg_dump was not found"));
        let _ = app.update(Message::Db(DbMsg::BackupRun));
        let _ = app.update(Message::Db(DbMsg::BackupDone(Done::ok("4.2 KB".into()))));
        assert_eq!(
            app.db.backup.as_ref().unwrap().done.as_deref(),
            Some("4.2 KB")
        );
        assert!(app.db_escape() && app.db.backup.is_none());
        // A restore reads its backup in one transaction by default.
        let _ = app.update(Message::Db(DbMsg::BackupOpen(
            id,
            "shop".into(),
            String::new(),
            String::new(),
            true,
        )));
        let b = app.db.backup.as_ref().unwrap();
        assert!(b.restore && b.spec.single_transaction && !b.spec.clean);
    }

    #[test]
    fn the_arrows_and_tab_move_the_focus_from_cell_to_cell() {
        let (mut app, path) = db_app(true);
        let key = |app: &mut SimpleEditApp, k: &str, shift: bool| {
            app.db_key(k, false, shift, false).is_some()
        };
        // From nowhere, the first move starts at the first cell.
        assert!(key(&mut app, "ArrowDown", false));
        assert_eq!(app.db_tabs[&path].selected, Some((1, 0)));
        assert!(key(&mut app, "ArrowRight", false));
        assert!(key(&mut app, "ArrowRight", false));
        assert_eq!(app.db_tabs[&path].selected, Some((1, 2)));
        // The edge holds.
        assert!(key(&mut app, "ArrowRight", false));
        assert_eq!(app.db_tabs[&path].selected, Some((1, 2)));
        // Tab goes on to the next row, Shift+Tab back.
        assert!(key(&mut app, "Tab", false));
        assert_eq!(app.db_tabs[&path].selected, Some((2, 0)));
        assert!(key(&mut app, "Tab", true));
        assert_eq!(app.db_tabs[&path].selected, Some((1, 2)));
        assert!(key(&mut app, "Home", false));
        assert_eq!(app.db_tabs[&path].selected, Some((1, 0)));
        assert!(key(&mut app, "ArrowUp", false));
        assert!(key(&mut app, "ArrowUp", false));
        assert_eq!(app.db_tabs[&path].selected, Some((0, 0)));
        // F2 types in the cell, Alt+Delete marks the row.
        assert!(key(&mut app, "F2", false));
        assert!(app.db_tabs[&path].editing.is_some());
        // While typing, the keys are the field's.
        assert!(!key(&mut app, "ArrowRight", false));
        let _ = app.update(Message::EscapePressed);
        assert!(app.db_key("Delete", false, false, true).is_some());
        assert!(app.db_tabs[&path].pending.is_deleted(0));
        // A key that means nothing here goes on to the rest of the app.
        assert!(!key(&mut app, "z", false));
    }

    #[test]
    fn the_tabs_of_a_database_are_grouped_under_it() {
        use crate::database::DbMsg;
        let (mut app, path) = db_app(true);
        let conn = app.db.connections[0].id;
        let file = PathBuf::from("/tmp/some_file.txt");
        app.sidebar.add_file(file.clone());
        let groups = app.tab_groups();
        assert_eq!(groups.len(), 2);
        let db_root = crate::database::source_root(conn);
        assert_eq!(app.tab_key(&path), Some(db_root.clone()));
        assert!(groups.iter().any(|g| g.project.as_ref() == Some(&db_root)));
        // A second table of the same database joins the group.
        let _ = app.update(Message::Db(DbMsg::OpenTable(
            conn,
            "shop".into(),
            "public".into(),
            "other".into(),
        )));
        let groups = app.tab_groups();
        let mine = groups
            .iter()
            .find(|g| g.project.as_ref() == Some(&db_root))
            .unwrap();
        assert_eq!(mine.tabs.len(), 2);
        // It has a color of its own.
        assert!(app
            .config
            .project_colors
            .contains_key(&db_root.to_string_lossy().into_owned()));
    }

    #[test]
    fn a_reading_query_opens_a_paged_tab_and_one_from_a_table_can_be_edited() {
        use crate::database::protocol::Source;
        use crate::database::{DbMsg, Executed, QueryResult, Ran};
        let (mut app, _) = db_app(true);
        let conn = app.db.connections[0].id;
        let select = QueryResult {
            sql: "SELECT id, label FROM items".into(),
            ms: 0,
            has_rows: true,
            columns: vec!["id".into(), "label".into()],
            rows: vec![],
            affected: 0,
            truncated: false,
            deferred: true,
        };
        let _ = app.update(Message::Db(DbMsg::Ran(
            conn,
            Done {
                result: Ok(Ran {
                    in_transaction: false,
                    results: vec![select],
                    failed: None,
                    position: None,
                }),
                executed: Vec::<Executed>::new(),
            },
        )));
        let path = app
            .db_tabs
            .iter()
            .find(|(_, t)| t.query.is_some())
            .map(|(p, _)| p.clone())
            .unwrap();
        let tab = &app.db_tabs[&path];
        assert!(tab.paged && tab.loading);
        // Its page asks for the query, sorted and filtered like a table.
        let q = tab.query(0);
        assert_eq!(q.sql.as_deref(), Some("SELECT id, label FROM items"));
        assert!(!tab.is_static_result());
        // The page that comes tells where the rows are from: they can be edited there.
        let mut page = app
            .db_tabs
            .values()
            .find(|t| t.table == "items")
            .unwrap()
            .page
            .clone()
            .unwrap();
        page.source = Some(Source {
            schema: "public".into(),
            table: "items".into(),
        });
        let _ = app.update(Message::Db(DbMsg::Rows(path.clone(), Done::ok(page))));
        let tab = &app.db_tabs[&path];
        assert!(tab.editable());
        assert_eq!(tab.table_ref().table, "items");
        assert!(tab.title.starts_with(&t!("db.result").to_string()));
        // A sort by two columns, stacked with Alt.
        app.mod_alt = true;
        let _ = app.update(Message::Db(DbMsg::Sort(path.clone(), 1)));
        let _ = app.update(Message::Db(DbMsg::Sort(path.clone(), 2)));
        assert_eq!(app.db_tabs[&path].order.len(), 2);
    }

    #[test]
    fn typing_in_the_queries_proposes_tables_then_columns_and_accepts() {
        use crate::database::protocol::{Database, Kind, Schema, Table, TableColumn};
        use crate::database::{DbMsg, Tree};
        use iced::widget::text_editor::{Action, Edit, Motion};
        let (mut app, _) = db_app(true);
        let conn = app.db.connections[0].id;
        app.db.console.connection = Some(conn);
        app.db.trees.insert(
            conn,
            Tree::Loaded(vec![Database {
                name: "shop".into(),
                current: true,
                schemas: vec![Schema {
                    name: "public".into(),
                    tables: vec![Table {
                        name: "items".into(),
                        kind: Kind::Table,
                    }],
                    ..Default::default()
                }],
            }]),
        );
        let column = |name: &str, kind: &str| TableColumn {
            schema: "public".into(),
            table: "items".into(),
            name: name.into(),
            kind: kind.into(),
        };
        app.db
            .columns
            .insert(conn, vec![column("id", "integer"), column("label", "text")]);
        app.db.console.editor = iced::widget::text_editor::Content::with_text("SELECT * FROM it");
        app.db
            .console
            .editor
            .perform(Action::Move(Motion::DocumentEnd));
        // A character typed after FROM: the table comes first.
        let _ = app.update(Message::Db(DbMsg::Sql(Action::Edit(Edit::Insert('e')))));
        let completion = app.db.console.completion.as_ref().expect("proposals");
        assert_eq!(completion.suggestions.items[0].label, "items");
        assert_eq!(completion.typed, 3);
        // With one proposal, moving wraps onto itself.
        let _ = app.update(Message::Db(DbMsg::CompleteMove(-1)));
        assert_eq!(app.db.console.completion.as_ref().unwrap().selected, 0);
        // Accepted: what was typed gives way to the table.
        let _ = app.update(Message::Db(DbMsg::CompleteAccept));
        assert_eq!(
            app.db.console.editor.text().trim_end(),
            "SELECT * FROM items"
        );
        assert!(app.db.console.completion.is_none());
        // Then the columns of the table, after a dot and an alias.
        app.db.console.editor =
            iced::widget::text_editor::Content::with_text("SELECT i.la FROM items i");
        for _ in 0..4 {
            app.db.console.editor.perform(Action::Move(Motion::Right));
        }
        // The caret is after "SELE"; go to the end of "i.la".
        app.db.console.editor.perform(Action::Move(Motion::Home));
        for _ in 0..11 {
            app.db.console.editor.perform(Action::Move(Motion::Right));
        }
        let _ = app.update(Message::Db(DbMsg::Sql(Action::Edit(Edit::Backspace))));
        let _ = app.update(Message::Db(DbMsg::Sql(Action::Edit(Edit::Insert('a')))));
        let completion = app.db.console.completion.as_ref().expect("columns");
        assert_eq!(completion.suggestions.items[0].label, "label");
        // Moving the caret closes the list.
        let _ = app.update(Message::Db(DbMsg::Sql(Action::Move(Motion::Left))));
        assert!(app.db.console.completion.is_none());
        // Ctrl+Space asks for it with nothing typed.
        app.db.console.editor = iced::widget::text_editor::Content::with_text("SELECT ");
        app.db
            .console
            .editor
            .perform(Action::Move(Motion::DocumentEnd));
        let _ = app.update(Message::Db(DbMsg::Complete));
        assert!(app.db.console.completion.is_some());
    }

    #[test]
    fn a_row_is_added_filled_in_and_waits_for_the_commit() {
        use crate::database::grid::Cell;
        use crate::database::DbMsg;
        let (mut app, path) = db_app(true);
        let _ = app.update(Message::Db(DbMsg::AddRow(path.clone())));
        let tab = &app.db_tabs[&path];
        // It comes after the three rows, and the first column that is not the
        // key is typed in.
        assert_eq!(tab.pending.inserts.len(), 1);
        assert_eq!(tab.selected, Some((3, 1)));
        assert_eq!(tab.editing.as_ref().map(|e| (e.row, e.col)), Some((3, 1)));
        let _ = app.update(Message::Db(DbMsg::EditInput(path.clone(), "fresh".into())));
        let _ = app.update(Message::Db(DbMsg::EditCommit(path.clone())));
        let tab = &app.db_tabs[&path];
        let page = tab.page.as_ref().unwrap();
        assert_eq!(tab.pending.cell(page, 3, 1), Cell::Value("fresh"));
        assert_eq!(tab.pending.cell(page, 3, 0), Cell::Default);
        // A second one, then NULL in a cell of it and a date by the picker.
        let _ = app.update(Message::Db(DbMsg::AddRow(path.clone())));
        let _ = app.update(Message::Db(DbMsg::SetNull(path.clone(), 4, 1)));
        let _ = app.update(Message::Db(DbMsg::OpenPicker(path.clone(), 4, 2)));
        let _ = app.update(Message::Db(DbMsg::PickerDay(path.clone(), 9)));
        let _ = app.update(Message::Db(DbMsg::PickerApply(path.clone())));
        let tab = &app.db_tabs[&path];
        let page = tab.page.as_ref().unwrap();
        assert_eq!(tab.pending.cell(page, 4, 1), Cell::Null);
        assert!(tab
            .pending
            .value(page, 4, 2)
            .is_some_and(|d| d.ends_with("-09")));
        assert_eq!(tab.pending.count(), 2);
        // The commit sends both, as inserts.
        let changes = tab.pending.changes(page);
        assert_eq!(changes.len(), 2);
        assert!(changes
            .iter()
            .all(|c| c.kind == crate::database::protocol::ChangeKind::Insert));
        let _ = app.update(Message::Db(DbMsg::Commit(path.clone())));
        assert!(app.db_tabs[&path].committing);
        // A page cannot be left meanwhile.
        let _ = app.update(Message::Db(DbMsg::Committed(
            path.clone(),
            Done::err("change 2: null value in column violates not-null"),
        )));
        let tab = &app.db_tabs[&path];
        assert_eq!(tab.pending.inserts.len(), 2);
        assert!(tab.notice.is_some());
        let _ = app.update(Message::Db(DbMsg::Turn(path.clone(), 1)));
        assert_eq!(app.db_tabs[&path].offset(), 0);
    }

    #[test]
    fn a_new_row_can_be_removed_and_reverted_and_walked_to_with_the_keys() {
        use crate::database::DbMsg;
        let (mut app, path) = db_app(true);
        let _ = app.update(Message::Db(DbMsg::AddRow(path.clone())));
        let _ = app.update(Message::EscapePressed);
        // The keys go down onto the new row.
        for _ in 0..3 {
            assert!(app.db_key("ArrowDown", false, false, false).is_some());
        }
        assert_eq!(app.db_tabs[&path].selected.map(|s| s.0), Some(3));
        assert!(app.db_key("ArrowDown", false, false, false).is_some());
        assert_eq!(app.db_tabs[&path].selected.map(|s| s.0), Some(3));
        // Alt+Delete takes the new row out: nothing waits.
        assert!(app.db_key("Delete", false, false, true).is_some());
        assert!(app.db_tabs[&path].pending.is_empty());
        // With a table that cannot be edited, no row is added.
        let (mut app, path) = db_app(false);
        let _ = app.update(Message::Db(DbMsg::AddRow(path.clone())));
        assert!(app.db_tabs[&path].pending.is_empty());
    }

    #[test]
    fn an_entry_of_the_log_is_selected_and_copied_with_the_keys_or_the_menu() {
        use crate::database::console::ConsoleTab;
        use crate::database::{DbMsg, Executed, LogCopy};
        let mut app = app();
        app.db.console.log.clear();
        app.db.console.push(
            "shop",
            &[
                Executed {
                    sql: "SELECT 1".into(),
                    ms: 1,
                    rows: Some(1),
                    error: None,
                    internal: false,
                },
                Executed {
                    sql: "DELETE FROM t".into(),
                    ms: 2,
                    rows: None,
                    error: Some("boom".into()),
                    internal: false,
                },
            ],
        );
        app.db.console.open = true;
        app.db.console.tab = ConsoleTab::Log;
        // Nothing selected: the copy key is not the log's.
        assert!(app.db_key("c", true, false, false).is_none());
        let _ = app.update(Message::Db(DbMsg::SelectLog(1)));
        assert_eq!(app.db.console.selected_log, Some(1));
        // Cmd/Ctrl+C now copies it; so does the menu, for each part.
        assert!(app.db_key("c", true, false, false).is_some());
        for what in [LogCopy::Sql, LogCopy::Error, LogCopy::Entry] {
            let _ = app.update(Message::Db(DbMsg::CopyLog(1, what)));
        }
        // On the queries, the key is the editor's.
        app.db.console.tab = ConsoleTab::Queries;
        assert!(app.db_key("c", true, false, false).is_none());
        // Clearing forgets the selection.
        let _ = app.update(Message::Db(DbMsg::ClearLog));
        assert_eq!(app.db.console.selected_log, None);
    }

    /// What a successful operation on a table gave.
    fn altered(
        what: crate::database::AlterKind,
        conn: u64,
        result: Result<(), String>,
    ) -> crate::database::Altered {
        use crate::database::Ran;
        let done = match result {
            Ok(()) => Done::ok(Ran {
                in_transaction: false,
                results: vec![],
                failed: None,
                position: None,
            }),
            Err(e) => Done::ok(Ran {
                in_transaction: false,
                results: vec![],
                failed: Some((0, e)),
                position: None,
            }),
        };
        crate::database::Altered {
            what,
            conn,
            database: "shop".into(),
            schema: "public".into(),
            table: "items".into(),
            done,
        }
    }

    #[test]
    fn a_drop_asks_first_then_closes_the_tabs_of_the_table_and_says_when_it_fails() {
        use crate::database::{AlterKind, ConfirmAction, DbMsg, Kind};
        let (mut app, path) = db_app(true);
        let conn = app.db.connections[0].id;
        let _ = app.update(Message::Db(DbMsg::AskDrop(
            conn,
            "shop".into(),
            "public".into(),
            "items".into(),
            Kind::Table,
        )));
        let confirm = app.db.confirm.as_ref().unwrap();
        assert_eq!(confirm.action, ConfirmAction::Drop);
        assert_eq!(confirm.statement(), "DROP TABLE \"public\".\"items\"");
        // Nothing is run before the yes; a view is dropped as a view.
        assert!(!confirm.running);
        let _ = app.update(Message::Db(DbMsg::ConfirmCascade(true)));
        assert!(app
            .db
            .confirm
            .as_ref()
            .unwrap()
            .statement()
            .ends_with("CASCADE"));
        let _ = app.update(Message::Db(DbMsg::ConfirmRun));
        assert!(app.db.confirm.as_ref().unwrap().running);
        // A refusal of the database stays on the question.
        let _ = app.update(Message::Db(DbMsg::Altered(Box::new(altered(
            AlterKind::Drop,
            conn,
            Err("cannot drop: other objects depend on it".into()),
        )))));
        let confirm = app.db.confirm.as_ref().unwrap();
        assert!(!confirm.running);
        assert_eq!(
            confirm.error.as_deref(),
            Some("cannot drop: other objects depend on it")
        );
        assert!(app.db_tabs.contains_key(&path));
        // It went: the question goes, and so do the tabs of the table.
        let _ = app.update(Message::Db(DbMsg::ConfirmRun));
        let _ = app.update(Message::Db(DbMsg::Altered(Box::new(altered(
            AlterKind::Drop,
            conn,
            Ok(()),
        )))));
        assert!(app.db.confirm.is_none());
        assert!(!app.db_tabs.contains_key(&path));
        let view = Kind::View;
        let _ = app.update(Message::Db(DbMsg::AskDrop(
            conn,
            "shop".into(),
            "public".into(),
            "v".into(),
            view,
        )));
        assert!(app
            .db
            .confirm
            .as_ref()
            .unwrap()
            .statement()
            .starts_with("DROP VIEW"));
        // Escape gives the question up.
        let _ = app.update(Message::EscapePressed);
        assert!(app.db.confirm.is_none());
    }

    #[test]
    fn deleting_all_the_rows_asks_first_and_keeps_the_tabs_which_read_again() {
        use crate::database::{AlterKind, DbMsg};
        let (mut app, path) = db_app(true);
        let conn = app.db.connections[0].id;
        let _ = app.update(Message::Db(DbMsg::AskEmpty(
            conn,
            "shop".into(),
            "public".into(),
            "items".into(),
        )));
        assert_eq!(
            app.db.confirm.as_ref().unwrap().statement(),
            "DELETE FROM \"public\".\"items\""
        );
        let _ = app.update(Message::Db(DbMsg::ConfirmRun));
        let _ = app.update(Message::Db(DbMsg::Altered(Box::new(altered(
            AlterKind::Empty,
            conn,
            Ok(()),
        )))));
        assert!(app.db.confirm.is_none());
        // The table is still there, being read again.
        assert!(app.db_tabs[&path].loading);
    }

    #[test]
    fn a_data_source_is_deleted_on_the_second_click() {
        use crate::database::DbMsg;
        let (mut app, _path) = db_app(true);
        let id = app.db.connections[0].id;
        let _ = app.update(Message::Db(DbMsg::OpenDialog(Some(id))));
        let _ = app.update(Message::Db(DbMsg::Delete));
        assert!(app.db.connection(id).is_some());
        assert!(app.db.dialog.as_ref().unwrap().confirm_delete);
        let _ = app.update(Message::Db(DbMsg::Delete));
        assert!(app.db.connection(id).is_none());
        assert!(app.db.dialog.is_none());
    }

    fn orders_structure() -> crate::database::protocol::Structure {
        use crate::database::protocol::{
            ColumnDef, ConstraintDef, ConstraintKind, Reference, Structure,
        };
        let column = |name: &str, kind: &str| ColumnDef {
            name: name.into(),
            kind: kind.into(),
            nullable: true,
            default: None,
            identity: String::new(),
            generated: false,
            comment: String::new(),
        };
        Structure {
            columns: vec![column("id", "integer"), column("label", "text")],
            constraints: vec![ConstraintDef {
                name: "items_pkey".into(),
                kind: ConstraintKind::Primary,
                definition: "PRIMARY KEY (id)".into(),
            }],
            referenced_by: vec![Reference {
                name: "lines_item_fkey".into(),
                schema: "public".into(),
                table: "lines".into(),
                definition: "FOREIGN KEY (item_id) REFERENCES items(id)".into(),
            }],
            ..Default::default()
        }
    }

    #[test]
    fn a_table_of_the_tree_opens_to_its_structure_and_shows_its_ddl() {
        use crate::database::{DbMsg, Done};
        let (mut app, path) = db_app(true);
        let conn = app.db.connections[0].id;
        let key = (
            conn,
            "shop".to_string(),
            "public".to_string(),
            "items".to_string(),
        );
        // Opening the node reads the structure; it is kept.
        let _ = app.update(Message::Db(DbMsg::ToggleTable(
            conn,
            "shop".into(),
            "public".into(),
            "items".into(),
        )));
        assert!(app.db.open.contains(&format!("{conn}/shop/public/items")));
        let _ = app.update(Message::Db(DbMsg::StructureRead(
            key.clone(),
            false,
            Done::ok(orders_structure()),
        )));
        assert!(app.db.structures.contains_key(&key));
        // The tab of the table learns it too, for the links of its rows.
        assert!(app.db_tabs[&path].structure.is_some());
        // The DDL comes from what is kept.
        let _ = app.update(Message::Db(DbMsg::ShowDdl(
            conn,
            "shop".into(),
            "public".into(),
            "items".into(),
        )));
        let text = app.db.text.as_ref().unwrap();
        assert_eq!(text.title, "items");
        assert!(text.text.starts_with("CREATE TABLE \"public\".\"items\" ("));
        assert!(text
            .text
            .contains("CONSTRAINT \"items_pkey\" PRIMARY KEY (id)"));
        let _ = app.update(Message::Db(DbMsg::TextClose));
        assert!(app.db.text.is_none());
        // Folding it again.
        let _ = app.update(Message::Db(DbMsg::ToggleTable(
            conn,
            "shop".into(),
            "public".into(),
            "items".into(),
        )));
        assert!(!app.db.open.contains(&format!("{conn}/shop/public/items")));
        // A reload forgets what the tables were made of.
        let _ = app.update(Message::Db(DbMsg::Reload(conn)));
        assert!(app.db.structures.is_empty());
        // The filter of the tree is kept as it is typed.
        let _ = app.update(Message::Db(DbMsg::TreeFilter("it".into())));
        assert_eq!(app.db.tree_filter, "it");
    }

    #[test]
    fn a_value_is_shown_whole_and_json_is_indented() {
        use crate::database::DbMsg;
        let (mut app, path) = db_app(true);
        let _ = app.update(Message::Db(DbMsg::ViewValue(path.clone(), 0, 1)));
        let view = app.db.text.as_ref().unwrap();
        assert_eq!(view.title, "label");
        assert_eq!(
            view.text,
            app.db_tabs[&path].page.as_ref().unwrap().rows[0][1]
                .clone()
                .unwrap()
        );
        let _ = app.update(Message::Db(DbMsg::TextClose));
        let tab = app.db_tabs.get_mut(&path).unwrap();
        let page = tab.page.clone().unwrap();
        tab.pending
            .edits
            .insert((0, 1), Some("{\"a\":1}".to_string()));
        let _ = page;
        let _ = app.update(Message::Db(DbMsg::ViewValue(path, 0, 1)));
        assert_eq!(app.db.text.as_ref().unwrap().text, "{\n  \"a\": 1\n}");
    }

    #[test]
    fn a_column_is_dragged_to_a_width_that_a_new_page_keeps() {
        use crate::database::DbMsg;
        let (mut app, path) = db_app(true);
        let before = app.db_tabs[&path].widths[1];
        let _ = app.update(Message::Db(DbMsg::ResizeStart(path.clone(), 1)));
        // The first move is where the pointer was; the next ones are the drag.
        let _ = app.update(Message::Db(DbMsg::ResizeMove(300.0)));
        let _ = app.update(Message::Db(DbMsg::ResizeMove(350.0)));
        assert!((app.db_tabs[&path].widths[1] - (before + 50.0)).abs() < 0.01);
        let _ = app.update(Message::Db(DbMsg::ResizeMove(-5000.0)));
        assert_eq!(app.db_tabs[&path].widths[1], 40.0);
        let _ = app.update(Message::Db(DbMsg::ResizeEnd));
        assert!(app.db.resizing.is_none());
        // Another page of the table keeps it.
        let tab = app.db_tabs.get_mut(&path).unwrap();
        let page = tab.page.clone().unwrap();
        tab.set_page(page);
        assert_eq!(tab.widths[1], 40.0);
    }

    #[test]
    fn the_rows_a_row_is_linked_to_open_with_a_condition() {
        use crate::database::DbMsg;
        let (mut app, path) = db_app(true);
        let conn = app.db.connections[0].id;
        // Another table, a condition on it.
        let _ = app.update(Message::Db(DbMsg::OpenRelated(
            conn,
            "shop".into(),
            "public".into(),
            "lines".into(),
            "\"item_id\" = '7'".into(),
        )));
        let opened = app
            .db_tabs
            .iter()
            .find(|(_, t)| t.table == "lines")
            .map(|(_, t)| t)
            .unwrap();
        assert_eq!(opened.filter, "\"item_id\" = '7'");
        assert_eq!(opened.filter_text, "\"item_id\" = '7'");
        // The table that is already open gets the condition.
        let _ = app.update(Message::Db(DbMsg::OpenRelated(
            conn,
            "shop".into(),
            "public".into(),
            "items".into(),
            "\"id\" = '3'".into(),
        )));
        assert_eq!(app.db_tabs[&path].filter, "\"id\" = '3'");
        assert_eq!(app.db_tabs.len(), 2);
    }

    #[test]
    fn a_schema_is_named_then_created_or_refused() {
        use crate::database::{AlterKind, Altered, DbMsg, Done, Ran};
        let (mut app, _path) = db_app(true);
        let conn = app.db.connections[0].id;
        let _ = app.update(Message::Db(DbMsg::NewSchema(conn, "shop".into())));
        // Nothing is run without a name.
        let _ = app.update(Message::Db(DbMsg::SchemaCreate));
        assert!(!app.db.new_schema.as_ref().unwrap().running);
        let _ = app.update(Message::Db(DbMsg::SchemaName("sales".into())));
        let _ = app.update(Message::Db(DbMsg::SchemaCreate));
        assert!(app.db.new_schema.as_ref().unwrap().running);
        let altered = |done| {
            Message::Db(DbMsg::Altered(Box::new(Altered {
                what: AlterKind::Schema,
                conn,
                database: "shop".into(),
                schema: "sales".into(),
                table: String::new(),
                done,
            })))
        };
        let _ = app.update(altered(Done::failed("already exists".to_string())));
        let n = app.db.new_schema.as_ref().unwrap();
        assert!(!n.running);
        assert_eq!(n.error.as_deref(), Some("already exists"));
        let _ = app.update(Message::Db(DbMsg::SchemaCreate));
        let _ = app.update(altered(Done::ok(Ran {
            in_transaction: false,
            results: vec![],
            failed: None,
            position: None,
        })));
        assert!(app.db.new_schema.is_none());
    }

    #[test]
    fn a_new_table_starts_with_an_id_and_is_made_by_the_designer() {
        use crate::database::{DbMsg, DesignerMsg};
        let (mut app, _path) = db_app(true);
        let conn = app.db.connections[0].id;
        let _ = app.update(Message::Db(DbMsg::NewTable(
            conn,
            "shop".into(),
            "sales".into(),
        )));
        let d = app.db.designer.as_ref().unwrap();
        assert!(d.creating);
        assert_eq!(d.schema_to, "sales");
        // It needs a name first.
        assert!(d.problem().is_some());
        let _ = app.update(Message::Db(DbMsg::Designer(DesignerMsg::TableName(
            "orders".into(),
        ))));
        let d = app.db.designer.as_ref().unwrap();
        assert_eq!(d.problem(), None);
        assert!(d.statements()[0].starts_with("CREATE TABLE \"sales\".\"orders\""));
        let _ = app.update(Message::Db(DbMsg::Designer(DesignerMsg::Apply)));
        assert_eq!(
            app.db.designer.as_ref().unwrap().load,
            crate::database::designer::Load::Running
        );
    }

    #[test]
    fn a_running_query_is_stopped() {
        use crate::database::DbMsg;
        let (mut app, _path) = db_app(true);
        app.db.console.running = true;
        let _ = app.update(Message::Db(DbMsg::Cancel));
        assert!(!app.db.console.running);
        assert_eq!(app.db.console.last.as_ref().map(|l| l.0), Some(false));
    }

    #[test]
    fn the_designer_opens_loads_is_edited_and_applied_in_one_transaction() {
        use crate::database::designer::{DesignerTab, Load};
        use crate::database::protocol::{ColumnDef, ConstraintDef, ConstraintKind, Structure};
        use crate::database::{AlterKind, ColField, DbMsg, DesignerMsg};
        let (mut app, path) = db_app(true);
        let conn = app.db.connections[0].id;
        let _ = app.update(Message::Db(DbMsg::OpenDesigner(
            conn,
            "shop".into(),
            "public".into(),
            "items".into(),
        )));
        assert_eq!(app.db.designer.as_ref().unwrap().load, Load::Loading);
        let column = |name: &str, kind: &str| ColumnDef {
            name: name.into(),
            kind: kind.into(),
            nullable: true,
            default: None,
            identity: String::new(),
            generated: false,
            comment: String::new(),
        };
        let structure = Structure {
            columns: vec![column("id", "integer"), column("label", "text")],
            constraints: vec![ConstraintDef {
                name: "items_pkey".into(),
                kind: ConstraintKind::Primary,
                definition: "PRIMARY KEY (id)".into(),
            }],
            indexes: vec![],
            comment: String::new(),
            ..Default::default()
        };
        let _ = app.update(Message::Db(DbMsg::Designer(DesignerMsg::Loaded(Done::ok(
            structure,
        )))));
        assert_eq!(app.db.designer.as_ref().unwrap().load, Load::Ready);
        // Nothing changed: nothing to apply.
        let _ = app.update(Message::Db(DbMsg::Designer(DesignerMsg::Apply)));
        assert_eq!(app.db.designer.as_ref().unwrap().load, Load::Ready);
        // Edits: a rename, a new column, a dropped one, a new table name.
        let say = |app: &mut SimpleEditApp, m: DesignerMsg| {
            let _ = app.update(Message::Db(DbMsg::Designer(m)));
        };
        say(
            &mut app,
            DesignerMsg::Col(1, ColField::Name, "title".into()),
        );
        say(&mut app, DesignerMsg::ColAdd);
        say(&mut app, DesignerMsg::Col(2, ColField::Name, "note".into()));
        say(&mut app, DesignerMsg::ColDrop(0, true));
        say(&mut app, DesignerMsg::ColDrop(0, false));
        say(&mut app, DesignerMsg::TableName("things".into()));
        say(&mut app, DesignerMsg::Tab(DesignerTab::Sql));
        let d = app.db.designer.as_ref().unwrap();
        assert_eq!(d.tab, DesignerTab::Sql);
        assert_eq!(d.statements().len(), 3);
        assert_eq!(d.problem(), None);
        // A column that is new is removed, not marked.
        say(&mut app, DesignerMsg::ColAdd);
        assert_eq!(app.db.designer.as_ref().unwrap().columns.len(), 4);
        say(&mut app, DesignerMsg::ColDrop(3, true));
        assert_eq!(app.db.designer.as_ref().unwrap().columns.len(), 3);
        // A problem stops the apply.
        say(&mut app, DesignerMsg::Col(1, ColField::Name, String::new()));
        say(&mut app, DesignerMsg::Apply);
        assert_eq!(app.db.designer.as_ref().unwrap().load, Load::Ready);
        say(
            &mut app,
            DesignerMsg::Col(1, ColField::Name, "title".into()),
        );
        // Applied: it runs, and a refusal stays on the designer with its reason.
        say(&mut app, DesignerMsg::Apply);
        assert_eq!(app.db.designer.as_ref().unwrap().load, Load::Running);
        let _ = app.update(Message::Db(DbMsg::Altered(Box::new(altered(
            AlterKind::Modify(Some("things".into())),
            conn,
            Err("column \"note\" already exists".into()),
        )))));
        let d = app.db.designer.as_ref().unwrap();
        assert_eq!(d.load, Load::Ready);
        assert_eq!(d.error.as_deref(), Some("column \"note\" already exists"));
        assert!(app.db_tabs.contains_key(&path));
        // Then it goes: the designer closes, the tabs of the renamed table too.
        say(&mut app, DesignerMsg::Apply);
        let _ = app.update(Message::Db(DbMsg::Altered(Box::new(altered(
            AlterKind::Modify(Some("things".into())),
            conn,
            Ok(()),
        )))));
        assert!(app.db.designer.is_none());
        assert!(!app.db_tabs.contains_key(&path));
        // Escape closes a designer that is not running.
        let _ = app.update(Message::Db(DbMsg::OpenDesigner(
            conn,
            "shop".into(),
            "public".into(),
            "items".into(),
        )));
        let _ = app.update(Message::EscapePressed);
        assert!(app.db.designer.is_none());
    }

    #[test]
    fn a_table_that_cannot_be_edited_refuses_everything() {
        use crate::database::DbMsg;
        let (mut app, path) = db_app(false);
        let _ = app.update(Message::Db(DbMsg::Edit(path.clone(), 0, 1)));
        let _ = app.update(Message::Db(DbMsg::DeleteRow(path.clone(), 0)));
        let _ = app.update(Message::Db(DbMsg::SetNull(path.clone(), 0, 1)));
        let _ = app.update(Message::Db(DbMsg::OpenPicker(path.clone(), 0, 2)));
        let tab = &app.db_tabs[&path];
        assert!(tab.editing.is_none() && tab.picker.is_none() && tab.pending.is_empty());
    }

    #[test]
    fn a_deleted_row_cannot_be_typed_in() {
        use crate::database::DbMsg;
        let (mut app, path) = db_app(true);
        let _ = app.update(Message::Db(DbMsg::DeleteRow(path.clone(), 1)));
        let _ = app.update(Message::Db(DbMsg::Edit(path.clone(), 1, 1)));
        assert!(app.db_tabs[&path].editing.is_none());
        let _ = app.update(Message::Db(DbMsg::RevertRow(path.clone(), 1)));
        let _ = app.update(Message::Db(DbMsg::Edit(path.clone(), 1, 1)));
        assert!(app.db_tabs[&path].editing.is_some());
    }
}

/// Draws the window to a PNG, to look at the design without a screen:
/// `SNAPSHOT=/tmp/look.png cargo test look_tests -- --ignored`.
#[cfg(test)]
mod look_tests {
    use super::*;
    use crate::database::Done;

    #[test]
    #[ignore]
    fn the_window_is_drawn_to_a_png() {
        let Ok(out) = std::env::var("SNAPSHOT") else {
            return;
        };
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let config = Config {
            dark_mode: std::env::var("LIGHT").is_err(),
            ..Config::default()
        };
        let mut app = SimpleEditApp::blank(config);
        let _ = app.restore_session(crate::session::Session::default());
        let _ = app.update(Message::ProjectPicked(Some(root.clone())));
        let _ = app.update(Message::ToggleProjectFolder(root.join("src")));
        for name in ["src/theme.rs", "Cargo.toml", "README.md", "src/main.rs"] {
            let path = root.join(name);
            let _ = app.update(Message::TabRead(Ok(Loaded {
                text: std::fs::read_to_string(&path).unwrap(),
                path,
                format: TextFormat::default(),
                stamp: None,
            })));
        }
        let theme = if app.config.dark_mode {
            se_theme::ink_dark()
        } else {
            se_theme::ink_light()
        };
        let settings = iced::Settings {
            default_font: se_theme::UI,
            fonts: se_theme::font_files(),
            ..Default::default()
        };
        let _ = std::fs::remove_file(&out);
        let mut ui = iced_test::Simulator::with_size(settings, (1280.0, 800.0), app.view());
        let shot = ui.snapshot(&theme).unwrap();
        assert!(shot.matches_image(&out).unwrap());
    }

    /// The Database panel with a table open, and every tab of its settings:
    /// `SNAPSHOT=/tmp/db cargo test look_tests -- --ignored` writes
    /// `/tmp/db-table-wgpu.png` and `/tmp/db-<tab>-wgpu.png`.
    #[test]
    #[ignore]
    fn the_database_panel_is_drawn_to_pngs() {
        use crate::database::protocol::{Column, Database, Schema, Table};
        use crate::database::{
            Choice, DbMsg, Dialog, DialogTab, Field, Flag, Kind, Page, SshAuth, SslMode, Tree,
        };
        let Ok(out) = std::env::var("SNAPSHOT") else {
            return;
        };
        let config = Config {
            dark_mode: std::env::var("LIGHT").is_err(),
            ..Config::default()
        };
        let mut app = SimpleEditApp::blank(config);
        let _ = app.restore_session(crate::session::Session::default());
        let mut form = Dialog::new();
        form.set(Field::Database, "shop".into());
        form.set(Field::User, "dev".into());
        let id = app.db.apply(&form).unwrap();
        app.db.open.insert(id.to_string());
        app.db.open.insert(format!("{id}/shop/public"));
        app.db.trees.insert(
            id,
            Tree::Loaded(vec![Database {
                name: "shop".into(),
                current: true,
                schemas: vec![
                    Schema {
                        name: "public".into(),
                        tables: vec![
                            Table {
                                name: "customer_names".into(),
                                kind: Kind::View,
                            },
                            Table {
                                name: "customers".into(),
                                kind: Kind::Table,
                            },
                        ],
                        ..Default::default()
                    },
                    Schema {
                        name: "sales".into(),
                        tables: vec![],
                        ..Default::default()
                    },
                ],
            }]),
        );
        app.db.driver = crate::database::Driver::Missing;
        let _ = app.update(Message::TogglePanel(crate::sidebar::Panel::Database));
        let _ = app.update(Message::Db(DbMsg::OpenTable(
            id,
            "shop".into(),
            "public".into(),
            "customers".into(),
        )));
        let path = app.db_tabs.keys().next().unwrap().clone();
        let columns = [
            ("id", "integer"),
            ("name", "text"),
            ("email", "text"),
            ("created", "timestamp with time zone"),
            ("score", "numeric(8,2)"),
            ("tags", "text[]"),
        ];
        let rows = (1..=60)
            .map(|i| {
                vec![
                    Some(i.to_string()),
                    Some(format!("Client {i}")),
                    (i % 5 != 0).then(|| format!("c{i}@ex.fr")),
                    Some("2026-11-07 20:30:14.562141+00".into()),
                    Some(format!("{}.50", i * 3)),
                    Some("{a,b}".into()),
                ]
            })
            .collect();
        let _ = app.update(Message::Db(DbMsg::Rows(
            path.clone(),
            Done::ok(Page {
                columns: columns
                    .iter()
                    .enumerate()
                    .map(|(i, (n, k))| Column {
                        name: n.to_string(),
                        kind: k.to_string(),
                        nullable: i != 0,
                    })
                    .collect(),
                rows,
                has_more: true,
                primary_key: vec!["id".into()],
                ctids: Vec::new(),
                editable: true,
                source: None,
                offset: 0,
            }),
        )));
        // Something waits for Commit: an edit, a NULL, a deleted row, a sort.
        if let Some(tab) = app.db_tabs.get_mut(&path) {
            let page = tab.page.clone().unwrap();
            tab.pending
                .set(&page, 1, 1, Some("Client 2 (renamed)".into()));
            tab.pending.set(&page, 2, 2, None);
            tab.pending.delete(&page, 4);
            tab.order = vec![("score".into(), true)];
            tab.filter = "score > 3".into();
            tab.filter_text = "score > 3".into();
            tab.notice = Some("change 1: invalid input syntax for type date".into());
            // Two rows added, one of them filled in.
            let first = tab.pending.add_row(&page);
            tab.pending.set(&page, first, 1, Some("Client 61".into()));
            tab.pending.set(&page, first, 2, None);
            tab.pending.add_row(&page);
        }
        let theme = if app.config.dark_mode {
            se_theme::ink_dark()
        } else {
            se_theme::ink_light()
        };
        let settings = iced::Settings {
            default_font: se_theme::UI,
            fonts: se_theme::font_files(),
            ..Default::default()
        };
        let shoot = |app: &SimpleEditApp, name: &str| {
            let file = format!("{out}-{name}-wgpu.png");
            // The renderer adds its own suffix to the name.
            let _ = std::fs::remove_file(format!("{out}-{name}-wgpu-wgpu.png"));
            let mut ui =
                iced_test::Simulator::with_size(settings.clone(), (1280.0, 800.0), app.view());
            assert!(ui.snapshot(&theme).unwrap().matches_image(&file).unwrap());
        };
        if let Some(tab) = app.db_tabs.get_mut(&path) {
            tab.selected = Some((3, 1));
        }
        shoot(&app, "table");
        if let Some(tab) = app.db_tabs.get_mut(&path) {
            tab.picker = Some(crate::database::grid::Picker::new(
                1,
                3,
                crate::database::grid::PickKind::DateTime,
                Some("2026-11-07 20:30:14"),
            ));
        }
        shoot(&app, "picker");
        if let Some(tab) = app.db_tabs.get_mut(&path) {
            tab.picker = None;
        }

        // The console: the log of what was run, then the queries.
        let _ = app.update(Message::Db(DbMsg::ToggleConsole));
        app.db.console.push(
            "shop@localhost",
            &[
                crate::database::Executed {
                    sql: "SELECT * FROM \"public\".\"customers\" ORDER BY \"score\" DESC LIMIT 500 OFFSET 0".into(),
                    ms: 4,
                    rows: Some(500),
                    error: None,
                    internal: false,
                },
                crate::database::Executed {
                    sql: "UPDATE \"public\".\"customers\" SET \"name\" = 'Client 2 (renamed)' WHERE \"id\" = '2'".into(),
                    ms: 1,
                    rows: Some(1),
                    error: None,
                    internal: false,
                },
                crate::database::Executed {
                    sql: "DELETE FROM \"public\".\"customers\" WHERE \"id\" = '5'".into(),
                    ms: 2,
                    rows: None,
                    error: Some("update or delete on table \"customers\" violates foreign key constraint".into()),
                    internal: false,
                },
            ],
        );
        app.db.console.tab = crate::database::console::ConsoleTab::Log;
        app.db.console.selected_log = Some(1);
        shoot(&app, "log");
        app.db.console.tab = crate::database::console::ConsoleTab::Queries;
        app.db.console.editor = iced::widget::text_editor::Content::with_text(
            "SELECT id, name, email\nFROM customers\nWHERE score > 10\nORDER BY score DESC;\n\nUPDATE customers SET email = NULL WHERE id = 5;",
        );
        // The caret in the second statement: that one is framed.
        for _ in 0..5 {
            app.db
                .console
                .editor
                .perform(iced::widget::text_editor::Action::Move(
                    iced::widget::text_editor::Motion::Down,
                ));
        }
        app.db.console.last = Some((true, "2 statements run in 5 ms.".into()));
        app.db.driver = crate::database::Driver::Ready;
        shoot(&app, "queries");
        // The proposals under the word being typed.
        {
            use crate::database::completion::{Item, ItemKind, Suggestions};
            let item = |label: &str, detail: &str, kind: ItemKind| Item {
                label: label.into(),
                insert: label.into(),
                detail: detail.into(),
                kind,
            };
            app.db.console.editor = iced::widget::text_editor::Content::with_text(
                "SELECT id, name, email\nFROM customers c\nWHERE c.na",
            );
            app.db.console.completion = Some(crate::database::console::Completion {
                suggestions: Suggestions {
                    start: 0,
                    items: vec![
                        item("name", "customers · text", ItemKind::Column),
                        item("created", "customers · timestamp", ItemKind::Column),
                        item("NATURAL", "", ItemKind::Keyword),
                        item("customer_names", "public", ItemKind::View),
                        item("count", "", ItemKind::Function),
                    ],
                },
                selected: 0,
                line: 2,
                column: 8,
                typed: 2,
            });
        }
        shoot(&app, "completion");
        app.db.console.completion = None;
        // The table designer, and the question before a drop.
        {
            use crate::database::designer::Designer;
            use crate::database::protocol::{
                ColumnDef, ConstraintDef, ConstraintKind, IndexDef, Structure,
            };
            let column =
                |name: &str, kind: &str, nullable: bool, default: Option<&str>| ColumnDef {
                    name: name.into(),
                    kind: kind.into(),
                    nullable,
                    default: default.map(String::from),
                    identity: String::new(),
                    generated: false,
                    comment: String::new(),
                };
            let mut d = Designer::new(1, "shop".into(), "public".into(), "customers".into());
            d.load(Structure {
                columns: vec![
                    column(
                        "id",
                        "integer",
                        false,
                        Some("nextval('customers_id_seq'::regclass)"),
                    ),
                    column("name", "text", false, None),
                    column("email", "text", true, None),
                    column("score", "numeric(8,2)", true, Some("0")),
                ],
                constraints: vec![
                    ConstraintDef {
                        name: "customers_pkey".into(),
                        kind: ConstraintKind::Primary,
                        definition: "PRIMARY KEY (id)".into(),
                    },
                    ConstraintDef {
                        name: "customers_score_check".into(),
                        kind: ConstraintKind::Check,
                        definition: "CHECK ((score >= (0)::numeric))".into(),
                    },
                ],
                indexes: vec![IndexDef {
                    name: "customers_email_idx".into(),
                    definition:
                        "CREATE INDEX customers_email_idx ON public.customers USING btree (email)"
                            .into(),
                    unique: false,
                }],
                comment: String::new(),
                ..Default::default()
            });
            d.columns[2].name = "mail".into();
            d.columns[3].dropped = true;
            d.add_column();
            let n = d.columns.last_mut().unwrap();
            n.name = "created".into();
            n.kind = "timestamptz".into();
            n.default = "now()".into();
            d.add_constraint(ConstraintKind::Foreign);
            let f = d.constraints.last_mut().unwrap();
            f.columns = "id".into();
            f.reference = "public.accounts".into();
            f.on_delete = "CASCADE".into();
            d.constraints[1].dropped = true;
            app.db.designer = Some(d);
        }
        shoot(&app, "designer-columns");
        if let Some(d) = app.db.designer.as_mut() {
            d.tab = crate::database::designer::DesignerTab::Constraints;
        }
        shoot(&app, "designer-constraints");
        if let Some(d) = app.db.designer.as_mut() {
            d.tab = crate::database::designer::DesignerTab::Sql;
        }
        shoot(&app, "designer-sql");
        app.db.designer = None;
        app.db.confirm = Some(crate::database::Confirm {
            conn: 1,
            database: "shop".into(),
            schema: "public".into(),
            table: "customers".into(),
            kind: crate::database::Kind::Table,
            action: crate::database::ConfirmAction::Drop,
            cascade: false,
            running: false,
            error: Some("cannot drop table customers because other objects depend on it".into()),
        });
        shoot(&app, "confirm");
        app.db.confirm = None;

        // The tree with a table opened to its structure, the folders of the
        // schema, the filter; the DDL; a new table; a new schema.
        {
            use crate::database::protocol::{ColumnDef, ConstraintDef, ConstraintKind, IndexDef};
            let col = |name: &str, kind: &str, nullable: bool| ColumnDef {
                name: name.into(),
                kind: kind.into(),
                nullable,
                default: None,
                identity: String::new(),
                generated: false,
                comment: String::new(),
            };
            let structure = crate::database::protocol::Structure {
                columns: vec![
                    col("id", "integer", false),
                    col("name", "text", false),
                    col("account_id", "integer", true),
                ],
                constraints: vec![
                    ConstraintDef {
                        name: "customers_pkey".into(),
                        kind: ConstraintKind::Primary,
                        definition: "PRIMARY KEY (id)".into(),
                    },
                    ConstraintDef {
                        name: "customers_account_fkey".into(),
                        kind: ConstraintKind::Foreign,
                        definition: "FOREIGN KEY (account_id) REFERENCES accounts(id)".into(),
                    },
                ],
                indexes: vec![IndexDef {
                    name: "customers_name_idx".into(),
                    definition:
                        "CREATE UNIQUE INDEX customers_name_idx ON public.customers USING btree (name)"
                            .into(),
                    unique: true,
                }],
                ..Default::default()
            };
            let key = (
                id,
                "shop".to_string(),
                "public".to_string(),
                "customers".to_string(),
            );
            app.db.structures.insert(key.clone(), structure.clone());
            app.db.open.insert(format!("{id}/shop/public/customers"));
            app.db.open.insert(format!("{id}/shop/public/#routines"));
            if let Some(crate::database::Tree::Loaded(databases)) = app.db.trees.get_mut(&id) {
                let public = &mut databases[0].schemas[0];
                public.routines = vec![crate::database::protocol::Routine {
                    name: "total_due".into(),
                    args: "customer integer".into(),
                    procedure: false,
                }];
                public.sequences = vec!["invoice_numbers".into()];
                public.types = vec!["mood".into()];
            }
            shoot(&app, "tree-structure");
            let _ = app.update(Message::Db(DbMsg::TreeFilter("custo".into())));
            shoot(&app, "tree-filter");
            let _ = app.update(Message::Db(DbMsg::TreeFilter(String::new())));
            app.db.text = Some(crate::database::TextView {
                title: "customers".into(),
                text: crate::database::ddl::table_ddl("public", "customers", &structure),
            });
            shoot(&app, "ddl");
            app.db.text = None;
            app.db.designer = Some(crate::database::designer::Designer::for_new_table(
                id,
                "shop".into(),
                "public".into(),
            ));
            if let Some(d) = app.db.designer.as_mut() {
                d.name = "orders".into();
                d.add_column();
                let c = d.columns.last_mut().unwrap();
                c.name = "total".into();
                c.kind = "numeric(10,2)".into();
                c.nullable = false;
            }
            shoot(&app, "designer-new");
            app.db.designer = None;
            app.db.new_schema = Some(crate::database::NewSchema {
                conn: id,
                database: "shop".into(),
                name: "sales".into(),
                running: false,
                error: None,
            });
            shoot(&app, "new-schema");
            app.db.new_schema = None;
        }

        // The settings, a tab after the other, with something in each.
        let _ = app.update(Message::Db(DbMsg::OpenDialog(None)));
        let _ = app.update(Message::Db(DbMsg::Text(Field::Name, "Shop (local)".into())));
        let _ = app.update(Message::Db(DbMsg::Text(Field::User, "dev".into())));
        let _ = app.update(Message::Db(DbMsg::Check(Flag::SshEnabled, true)));
        let _ = app.update(Message::Db(DbMsg::Text(
            Field::SshHost,
            "bastion.example.com".into(),
        )));
        let _ = app.update(Message::Db(DbMsg::Choose(Choice::SshAuth(
            SshAuth::KeyPair,
        ))));
        let _ = app.update(Message::Db(DbMsg::Choose(Choice::SslMode(
            SslMode::VerifyFull,
        ))));
        let _ = app.update(Message::Db(DbMsg::Text(
            Field::SslCa,
            "/etc/ssl/ca.pem".into(),
        )));
        let _ = app.update(Message::Db(DbMsg::PropAdd));
        let _ = app.update(Message::Db(DbMsg::PropKey(0, "application_name".into())));
        let _ = app.update(Message::Db(DbMsg::PropValue(0, "simpleedit".into())));
        let _ = app.update(Message::Db(DbMsg::Check(Flag::ReadOnly, true)));
        for (tab, name) in [
            (DialogTab::General, "general"),
            (DialogTab::Options, "options"),
            (DialogTab::SshSsl, "ssh"),
            (DialogTab::Schemas, "schemas"),
            (DialogTab::Advanced, "advanced"),
            (DialogTab::Driver, "driver"),
        ] {
            let _ = app.update(Message::Db(DbMsg::Tab(tab)));
            shoot(&app, name);
        }
    }
}
