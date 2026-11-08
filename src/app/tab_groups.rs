//! The tabs by project: each project has a color, on its tabs and in the
//! tree, and with files of several places open its tabs are side by side
//! after a label that folds them and moves them together.

use super::*;
use iced::widget::{column, stack};

use crate::projects::COLORS;

const LINE: f32 = 2.0;

/// The project a file is in: the deepest one, none for a file elsewhere.
pub fn project_of<'a>(roots: &'a [PathBuf], path: &Path) -> Option<&'a PathBuf> {
    roots
        .iter()
        .filter(|r| path.starts_with(r))
        .max_by_key(|r| r.components().count())
}

/// The tabs of a project, or of the files of no project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub project: Option<PathBuf>,
    /// Places in the list of open files.
    pub tabs: Vec<usize>,
}

/// The files by project: the groups as they first appear, each with its
/// files in their order.
#[cfg(test)]
pub fn groups(files: &[PathBuf], roots: &[PathBuf]) -> Vec<Group> {
    groups_by(files, |path| project_of(roots, path).cloned())
}

/// The same, with what groups a file given by `key`.
pub fn groups_by(files: &[PathBuf], key: impl Fn(&Path) -> Option<PathBuf>) -> Vec<Group> {
    let mut groups: Vec<Group> = Vec::new();
    for (i, path) in files.iter().enumerate() {
        let project = key(path);
        match groups.iter_mut().find(|g| g.project == project) {
            Some(group) => group.tabs.push(i),
            None => groups.push(Group {
                project,
                tabs: vec![i],
            }),
        }
    }
    groups
}

/// A place in the bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    /// The label of a group, by its place in the groups.
    Label(usize),
    /// A tab, by its place in the list of open files.
    Tab(usize),
}

/// What the bar shows: labels only when there are several groups, and no
/// tabs for a folded group.
pub fn slots(groups: &[Group], folded: &BTreeSet<Option<PathBuf>>) -> Vec<Slot> {
    let several = groups.len() > 1;
    let mut slots = Vec::new();
    for (g, group) in groups.iter().enumerate() {
        if several {
            slots.push(Slot::Label(g));
        }
        if !several || !folded.contains(&group.project) {
            slots.extend(group.tabs.iter().map(|t| Slot::Tab(*t)));
        }
    }
    slots
}

/// The new order of the open files once the place `from` of the bar goes to
/// `to`. A label takes its whole group along; a tab left among those of
/// another group goes back to its own.
pub fn reorder(groups: &[Group], slots: &[Slot], from: usize, to: usize) -> Vec<usize> {
    let mut moved = slots.to_vec();
    let slot = moved.remove(from);
    moved.insert(to.min(moved.len()), slot);
    let dragged = match slot {
        Slot::Label(g) => Some(g),
        Slot::Tab(_) => None,
    };
    let shown: BTreeSet<usize> = slots
        .iter()
        .filter_map(|s| match s {
            Slot::Tab(t) => Some(*t),
            Slot::Label(_) => None,
        })
        .collect();
    let mut order: Vec<usize> = Vec::new();
    for s in moved {
        match s {
            Slot::Tab(t) => {
                let mine = dragged.is_some_and(|g| groups[g].tabs.contains(&t));
                if !mine {
                    order.push(t);
                }
            }
            // The group that moves comes whole; a folded one has its tabs
            // nowhere else.
            Slot::Label(g) => order.extend(
                groups[g]
                    .tabs
                    .iter()
                    .filter(|t| dragged == Some(g) || !shown.contains(t)),
            ),
        }
    }
    // Together again: each tab with the first one of its group.
    let mut together: Vec<usize> = Vec::with_capacity(order.len());
    let group_of = |t: usize| groups.iter().position(|g| g.tabs.contains(&t));
    let mut seen: Vec<Option<usize>> = Vec::new();
    for t in &order {
        let g = group_of(*t);
        if !seen.contains(&g) {
            seen.push(g);
            together.extend(order.iter().filter(|o| group_of(**o) == g));
        }
    }
    together
}

impl SimpleEditApp {
    pub(super) fn tab_groups(&self) -> Vec<Group> {
        groups_by(self.sidebar.files(), |path| self.tab_key(path))
    }

    /// What a tab is grouped under: its project, or for the table of a
    /// database, that database.
    pub(super) fn tab_key(&self, path: &Path) -> Option<PathBuf> {
        match self.db_tabs.get(path) {
            Some(tab) => Some(crate::database::source_root(tab.conn)),
            None => project_of(&self.projects.roots, path).cloned(),
        }
    }

    /// A data source takes a color no other group has, and keeps it.
    pub(super) fn assign_db_color(&mut self, conn: u64) {
        let key = crate::database::source_root(conn)
            .to_string_lossy()
            .into_owned();
        if self.config.project_colors.contains_key(&key) {
            return;
        }
        let taken: Vec<usize> = self.config.project_colors.values().copied().collect();
        let free = (0..COLORS.len())
            .find(|c| !taken.contains(c))
            .unwrap_or(conn as usize % COLORS.len());
        self.config.project_colors.insert(key, free);
        self.config_dirty = true;
    }

    pub(super) fn tab_slots(&self) -> Vec<Slot> {
        slots(&self.tab_groups(), &self.folded_tabs)
    }

    /// The open file at a place of the bar (none on a label).
    pub(super) fn tab_at(&self, slot: usize) -> Option<PathBuf> {
        match self.tab_slots().get(slot) {
            Some(Slot::Tab(t)) => self.sidebar.files().get(*t).cloned(),
            _ => None,
        }
    }

    /// The open files of a project (none: of no project).
    pub(super) fn tabs_of(&self, project: &Option<PathBuf>) -> Vec<PathBuf> {
        let files = self.sidebar.files();
        self.tab_groups()
            .into_iter()
            .find(|g| g.project == *project)
            .map_or_else(Vec::new, |g| {
                g.tabs.iter().map(|t| files[*t].clone()).collect()
            })
    }

    pub(super) fn tab_moved(&mut self, from: usize, to: usize) {
        let groups = self.tab_groups();
        let slots = slots(&groups, &self.folded_tabs);
        if from >= slots.len() || to >= slots.len() {
            return;
        }
        let files = self.sidebar.files();
        let order: Vec<PathBuf> = reorder(&groups, &slots, from, to)
            .into_iter()
            .map(|i| files[i].clone())
            .collect();
        self.sidebar.set_order(order);
    }

    /// The color of a project: the one it was given, its place otherwise.
    pub(super) fn project_color(&self, root: &Path) -> iced::Color {
        crate::projects::color_of(&self.config.project_colors, &self.projects.roots, root)
    }

    /// A new project takes a color no other one has, and keeps it.
    pub(super) fn assign_project_colors(&mut self) {
        for root in &self.projects.roots {
            let key = root.to_string_lossy().into_owned();
            if self.config.project_colors.contains_key(&key) {
                continue;
            }
            let taken: Vec<usize> = self
                .projects
                .roots
                .iter()
                .filter_map(|r| {
                    self.config
                        .project_colors
                        .get(&r.to_string_lossy().into_owned())
                        .copied()
                })
                .collect();
            let free = (0..COLORS.len()).find(|c| !taken.contains(c)).unwrap_or(0);
            self.config.project_colors.insert(key, free);
            self.config_dirty = true;
        }
    }

    /// The thin line of a project's color over a tab.
    pub(super) fn with_project_line<'a>(
        &self,
        tab: Element<'a, Message>,
        path: &Path,
    ) -> Element<'a, Message> {
        let Some(root) = self.tab_key(path) else {
            return tab;
        };
        let color = self.project_color(&root);
        stack![
            tab,
            container(Space::new().height(LINE))
                .width(Length::Fill)
                .style(se_theme::dot_square(color)),
        ]
        .into()
    }

    /// The label of a group: its color, its name, how many tabs it hides.
    pub(super) fn tab_group_label(&self, group: &Group, dark: bool) -> Element<'_, Message> {
        let p = se_theme::palette(dark);
        let folded = self.folded_tabs.contains(&group.project);
        let name = match &group.project {
            Some(root) => match crate::database::source_id(root) {
                Some(id) => self
                    .db
                    .connection(id)
                    .map(crate::database::Connection::label)
                    .unwrap_or_default(),
                None => root
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            },
            None => t!("tabs.other_files").to_string(),
        };
        let color = group
            .project
            .as_deref()
            .map_or(p.muted, |r| self.project_color(r));
        // Darker on a light bar, to be read on its tint.
        let ink = if dark {
            color
        } else {
            iced::Color::from_rgb(color.r * 0.6, color.g * 0.6, color.b * 0.6)
        };
        let mut label = row![
            text(if folded { "▸" } else { "▾" }).size(10).color(ink),
            text(name)
                .size(11.5)
                .font(se_theme::UI_SEMIBOLD)
                .color(ink)
                .shaping(iced::widget::text::Shaping::Advanced),
        ]
        .spacing(5)
        .align_y(Alignment::Center);
        if folded {
            label = label.push(
                text(group.tabs.len().to_string())
                    .size(11)
                    .font(se_theme::UI_SEMIBOLD)
                    .color(ink),
            );
        }
        let pill = button(container(label).center_y(Length::Fixed(20.0)))
            .padding([0, 10])
            .on_press(Message::TabGroupToggle(group.project.clone()))
            .style(se_theme::group_label(color));
        // Centered in the bar, with air around it: it is not a tab.
        let height = Length::Fixed(super::views::TAB_HEIGHT);
        let chip = container(pill)
            .padding(iced::Padding {
                top: 0.0,
                bottom: 0.0,
                left: 10.0,
                right: 6.0,
            })
            .center_y(height);
        let project = group.project.clone();
        let close_label = match &project {
            Some(root) if crate::database::source_id(root).is_some() => {
                t!("db.close_group").to_string()
            }
            Some(_) => t!("tabs.close_project").to_string(),
            None => t!("tabs.close_other_files").to_string(),
        };
        crate::pointer_area::context_menu(chip, move || {
            let close = button(text(close_label.clone()).size(13))
                .padding([7, 10])
                .width(Length::Fixed(250.0))
                .on_press(Message::TabGroupClose(project.clone()))
                .style(
                    (se_theme::MenuItem {
                        dark,
                        radius: se_theme::R_SM,
                        highlighted: false,
                    })
                    .style(),
                );
            let mut menu = column![close].spacing(6).padding(6);
            if let Some(root) = &project {
                let mut swatches = row![].spacing(4);
                for (i, (r, g, b)) in COLORS.iter().enumerate() {
                    swatches = swatches.push(
                        button(
                            container(Space::new().width(14).height(14))
                                .style(se_theme::dot(iced::Color::from_rgb8(*r, *g, *b))),
                        )
                        .padding(3)
                        .on_press(Message::ProjectColor(root.clone(), i))
                        .style(
                            (se_theme::GhostButton {
                                dark,
                                active: false,
                            })
                            .style(),
                        ),
                    );
                }
                menu = menu.push(
                    container(
                        column![
                            text(t!("tabs.project_color").to_string())
                                .size(12)
                                .color(p.muted),
                            swatches
                        ]
                        .spacing(6),
                    )
                    .padding([2, 10]),
                );
            }
            container(menu).style(se_theme::card(dark)).into()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roots() -> Vec<PathBuf> {
        vec![PathBuf::from("/p/a"), PathBuf::from("/p/b")]
    }

    fn files(names: &[&str]) -> Vec<PathBuf> {
        names.iter().map(PathBuf::from).collect()
    }

    /// The order of the names after moving the place `from` to `to`.
    fn moved(names: &[&str], fold: Option<&str>, from: usize, to: usize) -> Vec<String> {
        let list = files(names);
        let groups = groups(&list, &roots());
        let folded: BTreeSet<Option<PathBuf>> =
            fold.iter().map(|f| Some(PathBuf::from(f))).collect();
        let slots = slots(&groups, &folded);
        reorder(&groups, &slots, from, to)
            .into_iter()
            .map(|i| names[i].to_string())
            .collect()
    }

    #[test]
    fn a_file_is_in_the_deepest_project() {
        let roots = vec![PathBuf::from("/p"), PathBuf::from("/p/sub")];
        assert_eq!(
            project_of(&roots, Path::new("/p/sub/a.rs")),
            Some(&roots[1])
        );
        assert_eq!(project_of(&roots, Path::new("/p/a.rs")), Some(&roots[0]));
        assert_eq!(project_of(&roots, Path::new("/q/a.rs")), None);
    }

    #[test]
    fn tabs_are_grouped_by_project_with_labels_only_when_several() {
        let list = files(&["/p/a/1", "/p/b/1", "/p/a/2", "/x/1"]);
        let g = groups(&list, &roots());
        assert_eq!(g.len(), 3);
        assert_eq!(g[0].tabs, vec![0, 2]);
        assert_eq!(g[2].project, None);
        let none = BTreeSet::new();
        assert_eq!(
            slots(&g, &none),
            vec![
                Slot::Label(0),
                Slot::Tab(0),
                Slot::Tab(2),
                Slot::Label(1),
                Slot::Tab(1),
                Slot::Label(2),
                Slot::Tab(3),
            ]
        );
        // One group: the tabs alone, as before.
        let single = groups(&files(&["/p/a/1", "/p/a/2"]), &roots());
        assert_eq!(slots(&single, &none), vec![Slot::Tab(0), Slot::Tab(1)]);
        // Folded: its label only.
        let folded = BTreeSet::from([Some(PathBuf::from("/p/a"))]);
        assert_eq!(slots(&g, &folded)[..2], [Slot::Label(0), Slot::Label(1)]);
    }

    #[test]
    fn a_tab_moves_in_its_group_and_never_leaves_it() {
        let names = ["/p/a/1", "/p/a/2", "/p/b/1"];
        // [A] a1 a2 [B] b1: a1 after a2.
        assert_eq!(moved(&names, None, 1, 2), ["/p/a/2", "/p/a/1", "/p/b/1"]);
        // a1 dropped after b1: back in its group.
        assert_eq!(moved(&names, None, 1, 4), ["/p/a/2", "/p/a/1", "/p/b/1"]);
        // b1 dropped among A's: back in B.
        assert_eq!(moved(&names, None, 4, 2), ["/p/a/1", "/p/a/2", "/p/b/1"]);
    }

    #[test]
    fn a_label_moves_its_whole_group_folded_or_not() {
        let names = ["/p/a/1", "/p/a/2", "/p/b/1"];
        // [A] goes after b1.
        assert_eq!(moved(&names, None, 0, 4), ["/p/b/1", "/p/a/1", "/p/a/2"]);
        // [B] goes first.
        assert_eq!(moved(&names, None, 3, 0), ["/p/b/1", "/p/a/1", "/p/a/2"]);
        // A folded: [A] [B] b1; [B] goes first, A's tabs stay with it.
        assert_eq!(
            moved(&names, Some("/p/a"), 1, 0),
            ["/p/b/1", "/p/a/1", "/p/a/2"]
        );
    }

    #[test]
    fn colors_are_kept_and_chosen() {
        let mut app = SimpleEditApp::blank(Config::default());
        app.projects.roots = roots();
        app.assign_project_colors();
        assert_eq!(app.config.project_colors["/p/a"], 0);
        assert_eq!(app.config.project_colors["/p/b"], 1);
        // The first project goes: the second keeps its color, a new one
        // takes what is free.
        app.projects.roots = vec![PathBuf::from("/p/b"), PathBuf::from("/p/c")];
        app.assign_project_colors();
        assert_eq!(app.config.project_colors["/p/b"], 1);
        assert_eq!(app.config.project_colors["/p/c"], 0);
        let _ = app.update(Message::ProjectColor(PathBuf::from("/p/c"), 4));
        assert_eq!(app.config.project_colors["/p/c"], 4);
        let (r, g, b) = COLORS[4];
        assert_eq!(
            app.project_color(Path::new("/p/c")),
            iced::Color::from_rgb8(r, g, b)
        );
    }

    #[test]
    fn a_folded_group_opens_when_one_of_its_files_is_shown() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.rs");
        std::fs::write(&path, "").unwrap();
        let mut app = SimpleEditApp::blank(Config::default());
        let _ = app.restore_session(crate::session::Session::default());
        let root = Some(dir.path().to_path_buf());
        let _ = app.update(Message::TabGroupToggle(root.clone()));
        assert!(app.folded_tabs.contains(&root));
        let _ = app.update(Message::ProjectPicked(root.clone()));
        let _ = app.update(Message::TabRead(Ok(Loaded {
            path,
            text: String::new(),
            format: TextFormat::default(),
            stamp: None,
        })));
        assert!(!app.folded_tabs.contains(&root));
    }

    /// An app with `main.rs` open in two projects, `b`'s last and in front.
    fn two_projects() -> (tempfile::TempDir, tempfile::TempDir, SimpleEditApp) {
        let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let mut app = SimpleEditApp::blank(Config::default());
        let _ = app.restore_session(crate::session::Session::default());
        for dir in [a.path(), b.path()] {
            let _ = app.update(Message::ProjectPicked(Some(dir.to_path_buf())));
            let path = dir.join("main.rs");
            std::fs::write(&path, "").unwrap();
            let _ = app.update(Message::TabRead(Ok(Loaded {
                path,
                text: String::new(),
                format: TextFormat::default(),
                stamp: None,
            })));
        }
        (a, b, app)
    }

    #[test]
    fn the_tabs_of_a_project_are_closed_and_only_those() {
        let (a, b, mut app) = two_projects();
        let (in_a, in_b) = (a.path().join("main.rs"), b.path().join("main.rs"));
        let _ = app.update(Message::TabGroupClose(Some(b.path().to_path_buf())));
        assert_eq!(app.overlay, ActiveOverlay::None);
        assert!(!app.sidebar.files().contains(&in_b));
        assert!(app.sidebar.files().contains(&in_a), "the other one stays");
        // The files of no project are closed the same way.
        let _ = app.update(Message::TabGroupClose(None));
        assert!(app.sidebar.files().iter().all(|f| *f == in_a));
    }

    #[test]
    fn unsaved_tabs_of_a_project_are_asked_about_first() {
        let (_a, b, mut app) = two_projects();
        let in_b = b.path().join("main.rs");
        let _ = app.update(Message::EditorAction(text_editor::Action::Edit(
            text_editor::Edit::Paste(std::sync::Arc::new("x".into())),
        )));
        let _ = app.update(Message::TabGroupClose(Some(b.path().to_path_buf())));
        assert!(matches!(
            app.overlay,
            ActiveOverlay::ConfirmCloseMany(ref paths, 1) if paths == std::slice::from_ref(&in_b)
        ));
        assert!(app.sidebar.files().contains(&in_b), "nothing closed yet");
    }

    #[test]
    fn the_bar_tells_tabs_from_labels() {
        let mut app = SimpleEditApp::blank(Config::default());
        app.projects.roots = roots();
        app.sidebar = crate::sidebar::SidebarState::new();
        for f in ["/p/a/1", "/p/b/1"] {
            app.sidebar.add_file(PathBuf::from(f));
        }
        assert_eq!(app.tab_at(0), None, "a label");
        assert_eq!(app.tab_at(1), Some(PathBuf::from("/p/a/1")));
        let _ = app.update(Message::TabMoved(2, 0));
        assert_eq!(app.sidebar.files()[0], PathBuf::from("/p/b/1"));
    }

    /// Draws the bar with two projects, one group folded, to look at it
    /// without a screen: `SNAPSHOT=/tmp/x cargo test tabs_look -- --ignored`.
    #[test]
    #[ignore]
    fn tabs_look() {
        let Ok(out) = std::env::var("SNAPSHOT") else {
            return;
        };
        let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let mut app = SimpleEditApp::blank(Config::default());
        let _ = app.restore_session(crate::session::Session::default());
        for dir in [a.path(), b.path()] {
            let _ = app.update(Message::ProjectPicked(Some(dir.to_path_buf())));
            std::fs::write(dir.join("main.rs"), "fn main() {}\n").unwrap();
            std::fs::write(dir.join("lib.rs"), "\n").unwrap();
        }
        for path in [
            a.path().join("main.rs"),
            b.path().join("lib.rs"),
            a.path().join("lib.rs"),
            b.path().join("main.rs"),
        ] {
            let text = std::fs::read_to_string(&path).unwrap();
            let _ = app.update(Message::TabRead(Ok(Loaded {
                path,
                text,
                format: TextFormat::default(),
                stamp: None,
            })));
        }
        app.panels.projects = true;
        app.show_sidebar = true;
        let settings = iced::Settings {
            default_font: se_theme::UI,
            fonts: se_theme::font_files(),
            ..Default::default()
        };
        let mut ui = iced_test::Simulator::with_size(settings, (1280.0, 800.0), app.view());
        let shot = ui.snapshot(&se_theme::ink_dark()).unwrap();
        let _ = std::fs::remove_file(format!("{out}-wgpu.png"));
        assert!(shot.matches_image(&out).unwrap());
        drop(ui);
        // The first group folded, in the light theme.
        let _ = app.update(Message::TabGroupToggle(None));
        app.config.dark_mode = false;
        let settings = iced::Settings {
            default_font: se_theme::UI,
            fonts: se_theme::font_files(),
            ..Default::default()
        };
        let mut ui = iced_test::Simulator::with_size(settings, (1280.0, 800.0), app.view());
        let shot = ui.snapshot(&se_theme::ink_light()).unwrap();
        let light = format!("{out}.light.png");
        assert!(shot.matches_image(&light).unwrap());
    }
}
