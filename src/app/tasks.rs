//! The tasks of a project: a list to pick from, and a panel with the output
//! of the one that runs, its error places a click away.

use super::*;
use crate::tasks::{self as engine, Event, Location, TaskSpec};
use iced::widget::column;

/// Lines of output kept; the oldest go first.
const MAX_LINES: usize = 5000;

/// One line of output, with the place it names.
pub struct OutLine {
    pub text: String,
    pub at: Option<Location>,
}

#[derive(Default)]
pub struct TaskPanel {
    pub open: bool,
    pub lines: Vec<OutLine>,
    /// The task that runs, or ran last.
    pub task: Option<TaskSpec>,
    /// How it ended: `None` while it runs.
    pub done: Option<Done>,
    /// Which run the lines are from: events of an older one are dropped.
    pub run: u64,
    /// Dropping it stops the command.
    handle: Option<iced::task::Handle>,
    /// What the picker lists.
    pub choices: Vec<TaskSpec>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Done {
    Code(i32),
    /// Ended without a code, or could not start.
    NoCode,
    Stopped,
}

pub(super) fn log_id() -> iced::widget::Id {
    iced::widget::Id::new("task-log")
}

impl SimpleEditApp {
    /// The folder whose tasks are offered: the project of the file in front,
    /// else the first project.
    fn task_root(&self) -> Option<PathBuf> {
        self.current_file
            .as_deref()
            .filter(|p| !is_untitled(p))
            .and_then(crate::definition::project_root)
            .or_else(|| self.projects.roots.first().cloned())
    }

    /// Lists the tasks of the project, to pick one.
    pub(super) fn open_task_picker(&mut self) -> Task<Message> {
        let Some(root) = self.task_root() else {
            self.set_status(t!("tasks.none"), true);
            return Task::none();
        };
        let choices = engine::detect(&root);
        if choices.is_empty() {
            self.set_status(t!("tasks.none"), true);
            return Task::none();
        }
        self.tasks.choices = choices;
        self.overlay = ActiveOverlay::TaskPicker;
        Task::none()
    }

    /// Runs a task, the output of the last one replaced.
    pub(super) fn start_task(&mut self, task: TaskSpec) -> Task<Message> {
        // A command written by the project runs once its folder is trusted.
        if let Some(root) = task
            .custom
            .as_ref()
            .filter(|r| !crate::trust::is_trusted(r))
        {
            self.overlay = ActiveOverlay::ConfirmTrust(root.clone());
            return Task::none();
        }
        self.tasks.run += 1;
        let run = self.tasks.run;
        self.tasks.lines.clear();
        self.tasks.done = None;
        self.tasks.task = Some(task.clone());
        self.tasks.open = true;
        let (start, handle) = Task::run(engine::run(task), move |event| {
            Message::TaskEvent(run, event)
        })
        .abortable();
        self.tasks.handle = Some(handle.abort_on_drop());
        start
    }

    pub(super) fn task_event(&mut self, run: u64, event: Event) -> Task<Message> {
        if run != self.tasks.run {
            return Task::none();
        }
        match event {
            Event::Line(raw) => {
                let text = engine::strip_ansi(&raw);
                let dir = self
                    .tasks
                    .task
                    .as_ref()
                    .map(|t| t.dir.clone())
                    .unwrap_or_default();
                let at = engine::location(&text, &dir);
                self.tasks.lines.push(OutLine { text, at });
                if self.tasks.lines.len() > MAX_LINES {
                    let extra = self.tasks.lines.len() - MAX_LINES;
                    self.tasks.lines.drain(..extra);
                }
                iced::widget::operation::snap_to_end(log_id())
            }
            Event::Done(code) => {
                self.tasks.handle = None;
                self.tasks.done = Some(code.map_or(Done::NoCode, Done::Code));
                let name = self
                    .tasks
                    .task
                    .as_ref()
                    .map(|t| t.name.clone())
                    .unwrap_or_default();
                let ok = code == Some(0);
                self.set_status(format!("{name}: {}", self.task_state_text()), !ok);
                Task::none()
            }
        }
    }

    pub(super) fn task_stop(&mut self) -> Task<Message> {
        if self.tasks.handle.take().is_some() {
            self.tasks.done = Some(Done::Stopped);
            self.tasks.run += 1;
        }
        Task::none()
    }

    /// How the task is doing, in words.
    fn task_state_text(&self) -> String {
        match self.tasks.done {
            None => t!("tasks.running").to_string(),
            Some(Done::Code(code)) => t!("tasks.exit", code = code).to_string(),
            Some(Done::NoCode) => t!("tasks.killed").to_string(),
            Some(Done::Stopped) => t!("tasks.stopped").to_string(),
        }
    }

    /// A click on a place of the output.
    pub(super) fn task_open_at(&mut self, at: Location) -> Task<Message> {
        self.jump_to(crate::definition::Hit {
            path: at.path,
            line: at.line.saturating_sub(1),
            col: at.col.saturating_sub(1),
            len: 0,
        })
    }

    pub(super) fn view_task_picker(&self, dark: bool) -> Element<'_, Message> {
        let p = se_theme::palette(dark);
        let items: Vec<Element<Message>> = self
            .tasks
            .choices
            .iter()
            .enumerate()
            .map(|(i, task)| {
                button(
                    row![
                        text(task.name.clone()).size(13),
                        Space::new().width(Length::Fill),
                        text(short_dir(&task.dir)).size(11.5).color(p.muted),
                    ]
                    .spacing(12)
                    .align_y(Alignment::Center),
                )
                .padding([6, 12])
                .width(Length::Fill)
                .on_press(Message::RunTask(i))
                .style(
                    (se_theme::GhostButton {
                        dark,
                        active: self.overlay_pick == i,
                    })
                    .style(),
                )
                .into()
            })
            .collect();
        container(
            column![
                text(t!("tasks.picker_title").to_string())
                    .size(14)
                    .color(p.text),
                scrollable(column(items).spacing(2).padding(4)).height(Length::Fixed(320.0)),
            ]
            .spacing(10)
            .padding(16),
        )
        .width(420)
        .style(se_theme::card(dark))
        .into()
    }

    /// The output of the task at the bottom of the editor.
    pub(super) fn tasks_panel(&self, dark: bool) -> Element<'_, Message> {
        let p = se_theme::palette(dark);
        let ghost = move |active: bool| (se_theme::GhostButton { dark, active }).style();
        let small = |label: String, message: Message| {
            button(text(label).size(12))
                .padding([2, 8])
                .on_press(message)
                .style(ghost(false))
        };
        let running = self.tasks.task.is_some() && self.tasks.done.is_none();
        let mut header = row![text(t!("tasks.title").to_string())
            .size(12)
            .font(se_theme::UI_SEMIBOLD)
            .color(p.text)]
        .spacing(8)
        .align_y(Alignment::Center);
        if let Some(task) = &self.tasks.task {
            header = header
                .push(text(task.name.clone()).size(12).font(se_theme::MONO))
                .push(
                    text(self.task_state_text())
                        .size(12)
                        .color(match self.tasks.done {
                            Some(Done::Code(0)) => p.accent,
                            Some(_) => se_theme::error_color(dark),
                            None => p.muted,
                        }),
                );
        }
        header = header.push(Space::new().width(Length::Fill));
        if running {
            header = header.push(small(t!("tasks.stop").to_string(), Message::TaskStop));
        } else if self.tasks.task.is_some() {
            header = header.push(small(t!("tasks.rerun").to_string(), Message::TaskRerun));
        }
        header = header
            .push(small(t!("tasks.run").to_string(), Message::OpenTaskPicker))
            .push(small(t!("tasks.clear").to_string(), Message::TaskClear))
            .push(small("—".to_string(), Message::TaskToggle));

        let mut log = column![].spacing(0).padding([4, 10]);
        for line in &self.tasks.lines {
            let shown = text(line.text.clone())
                .size(12)
                .font(se_theme::MONO)
                .shaping(iced::widget::text::Shaping::Advanced)
                .wrapping(iced::widget::text::Wrapping::None);
            log = log.push(match &line.at {
                Some(at) => button(shown.color(p.accent))
                    .padding(0)
                    .on_press(Message::TaskOpenAt(at.clone()))
                    .style(ghost(false))
                    .into(),
                None => Element::from(shown.color(p.text)),
            });
        }
        container(
            column![
                container(header).padding([6, 12]),
                container(Space::new().height(1))
                    .width(Length::Fill)
                    .style(se_theme::rule(dark)),
                scrollable(log)
                    .id(log_id())
                    .style(se_theme::Scrollbar { dark }.style())
                    .width(Length::Fill)
                    .height(Length::Fill),
            ]
            .spacing(0),
        )
        .height(Length::Fixed(self.panel_height))
        .width(Length::Fill)
        .style(se_theme::panel(dark))
        .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> SimpleEditApp {
        let mut app = SimpleEditApp::blank(Config::default());
        let _ = app.restore_session(crate::session::Session::default());
        app
    }

    fn task(dir: &Path) -> TaskSpec {
        TaskSpec {
            name: "build".into(),
            command: "true".into(),
            dir: dir.to_path_buf(),
            custom: None,
        }
    }

    #[test]
    fn output_lines_gather_with_the_places_they_name_and_the_end_is_said() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.rs"), "").unwrap();
        let mut app = app();
        let _ = app.start_task(task(dir.path()));
        let run = app.tasks.run;
        assert!(app.tasks.open && app.tasks.done.is_none());
        let _ = app.update(Message::TaskEvent(run, Event::Line("plain".into())));
        let _ = app.update(Message::TaskEvent(
            run,
            Event::Line("\u{1b}[31merror\u{1b}[0m at a.rs:3:4".into()),
        ));
        assert_eq!(app.tasks.lines[1].text, "error at a.rs:3:4");
        let at = app.tasks.lines[1].at.clone().expect("a place");
        assert_eq!((at.path, at.line, at.col), (dir.path().join("a.rs"), 3, 4));
        assert!(app.tasks.lines[0].at.is_none());
        let _ = app.update(Message::TaskEvent(run, Event::Done(Some(2))));
        assert_eq!(app.tasks.done, Some(Done::Code(2)));
        assert!(app.status_is_error);
    }

    #[test]
    fn what_an_older_run_says_is_dropped_and_a_stop_ends_the_run() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app();
        let _ = app.start_task(task(dir.path()));
        let old = app.tasks.run;
        let _ = app.start_task(task(dir.path()));
        let _ = app.update(Message::TaskEvent(old, Event::Line("late".into())));
        assert!(app.tasks.lines.is_empty());
        let _ = app.update(Message::TaskStop);
        assert_eq!(app.tasks.done, Some(Done::Stopped));
        let run = app.tasks.run;
        let _ = app.update(Message::TaskEvent(run - 1, Event::Line("after".into())));
        assert!(app.tasks.lines.is_empty());
    }

    #[test]
    fn the_oldest_lines_go_when_there_are_too_many() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app();
        let _ = app.start_task(task(dir.path()));
        let run = app.tasks.run;
        for i in 0..MAX_LINES + 10 {
            let _ = app.update(Message::TaskEvent(run, Event::Line(format!("l{i}"))));
        }
        assert_eq!(app.tasks.lines.len(), MAX_LINES);
        assert_eq!(app.tasks.lines[0].text, "l10");
    }

    #[test]
    fn the_picker_lists_the_tasks_of_the_project_and_runs_the_one_chosen() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "").unwrap();
        let file = dir.path().join("src.rs");
        std::fs::write(&file, "x\n").unwrap();
        let mut app = app();
        let _ = app.update(Message::TabRead(Ok(Loaded {
            path: file,
            text: "x\n".into(),
            format: TextFormat::default(),
            stamp: None,
        })));
        let _ = app.update(Message::OpenTaskPicker);
        assert_eq!(app.overlay, ActiveOverlay::TaskPicker);
        assert!(app.tasks.choices.iter().any(|t| t.name == "cargo test"));
        let _ = app.update(Message::RunTask(0));
        assert_eq!(app.overlay, ActiveOverlay::None);
        assert!(app.tasks.open && app.tasks.task.is_some());
        // Nothing to run in a folder without tasks.
        let empty = tempfile::tempdir().unwrap();
        let lone = empty.path().join("a.txt");
        std::fs::write(&lone, "x\n").unwrap();
        let mut app = self::app();
        let _ = app.update(Message::TabRead(Ok(Loaded {
            path: lone,
            text: "x\n".into(),
            format: TextFormat::default(),
            stamp: None,
        })));
        let _ = app.update(Message::OpenTaskPicker);
        assert_eq!(app.overlay, ActiveOverlay::None);
        assert!(app.status_is_error);
    }
}
