//! Files on another machine, over SSH: a window to browse a remote folder,
//! a local copy that is opened like any file, and the copy sent back at each
//! save.

use super::*;
use crate::remote::{self, Entry, RemoteFile};
use iced::widget::{column, text_input};

/// A remote folder being browsed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RemoteView {
    pub host: String,
    pub dir: String,
    pub entries: Vec<Entry>,
    pub loading: bool,
    pub error: Option<String>,
}

fn host_field_id() -> iced::widget::Id {
    iced::widget::Id::new("remote-host")
}

impl SimpleEditApp {
    pub(super) fn open_remote(&mut self) -> Task<Message> {
        self.overlay = ActiveOverlay::Remote(Box::new(RemoteView {
            host: self
                .config
                .remote_hosts
                .first()
                .cloned()
                .unwrap_or_default(),
            dir: ".".into(),
            ..RemoteView::default()
        }));
        iced::widget::operation::focus(host_field_id())
    }

    fn remote_view_mut(&mut self) -> Option<&mut RemoteView> {
        match &mut self.overlay {
            ActiveOverlay::Remote(view) => Some(view),
            _ => None,
        }
    }

    /// Lists the folder typed, on the machine typed.
    pub(super) fn remote_connect(&mut self) -> Task<Message> {
        let Some(view) = self.remote_view_mut() else {
            return Task::none();
        };
        let (host, dir) = (view.host.trim().to_string(), view.dir.trim().to_string());
        // A whole name (`user@host:/path/file`) opens that file at once.
        if let Some(file) = remote::parse(&host) {
            view.loading = true;
            view.error = None;
            return Task::perform(
                {
                    let file = file.clone();
                    async move { remote::download(&file, MAX_OPEN_BYTES as usize).await }
                },
                move |result| Message::RemoteDownloaded(file.clone(), result),
            );
        }
        if !remote::valid_host(&host) {
            view.loading = false;
            view.error = Some(t!("remote.bad_host").to_string());
            return Task::none();
        }
        view.loading = true;
        view.error = None;
        let dir = if dir.is_empty() { ".".to_string() } else { dir };
        Task::perform(
            {
                let (host, dir) = (host.clone(), dir.clone());
                async move { remote::list(&host, &dir).await }
            },
            move |result| Message::RemoteListed(host.clone(), result),
        )
    }

    pub(super) fn remote_listed(
        &mut self,
        host: String,
        result: Result<(String, Vec<Entry>), String>,
    ) -> Task<Message> {
        let remembered = result.is_ok();
        if let Some(view) = self.remote_view_mut() {
            if view.host.trim() == host {
                view.loading = false;
                match result {
                    Ok((dir, entries)) => {
                        view.dir = dir;
                        view.entries = entries;
                    }
                    Err(e) => {
                        view.entries.clear();
                        view.error = Some(e);
                    }
                }
            }
        }
        if remembered {
            self.config.remote_hosts.retain(|h| *h != host);
            self.config.remote_hosts.insert(0, host);
            self.config.remote_hosts.truncate(10);
            self.config_dirty = true;
        }
        Task::none()
    }

    /// A click in the list: a folder is entered, a file is copied here.
    pub(super) fn remote_pick(&mut self, name: String) -> Task<Message> {
        let Some(view) = self.remote_view_mut() else {
            return Task::none();
        };
        let is_dir = name == ".." || view.entries.iter().any(|e| e.name == name && e.is_dir);
        if is_dir {
            view.dir = if name == ".." {
                remote::parent(&view.dir)
            } else {
                remote::join(&view.dir, &name)
            };
            return self.remote_connect();
        }
        let file = RemoteFile {
            host: view.host.trim().to_string(),
            path: remote::join(&view.dir, &name),
        };
        view.loading = true;
        Task::perform(
            {
                let file = file.clone();
                async move { remote::download(&file, MAX_OPEN_BYTES as usize).await }
            },
            move |result| Message::RemoteDownloaded(file.clone(), result),
        )
    }

    /// The file arrived: it is saved in the local copy and opened there.
    pub(super) fn remote_downloaded(
        &mut self,
        file: RemoteFile,
        result: Result<(Vec<u8>, Option<u64>), String>,
    ) -> Task<Message> {
        let (bytes, date) = match result {
            Ok(found) => found,
            Err(e) => {
                if let Some(view) = self.remote_view_mut() {
                    view.loading = false;
                    view.error = Some(e);
                }
                return Task::none();
            }
        };
        let Some(local) = remote::local_copy(&file) else {
            self.set_status(t!("remote.no_folder"), true);
            return Task::none();
        };
        if let Some(parent) = local.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(e) = crate::fsutil::write_atomic(&local, &bytes, true) {
            self.set_status(format!("{}: {e}", t!("status.error")), true);
            return Task::none();
        }
        let local = absolute(local);
        self.remote_files.insert(local.clone(), (file, date));
        self.overlay = ActiveOverlay::None;
        Task::perform(read_file(local, false), Message::FileOpened)
    }

    /// After a save: the copy goes back to the machine it came from.
    pub(super) fn remote_send(&mut self, path: &Path) -> Task<Message> {
        let Some((file, known)) = self.remote_files.get(path).cloned() else {
            return Task::none();
        };
        self.set_status(t!("remote.sending", host = file.host.as_str()), false);
        let local = path.to_path_buf();
        let sent = local.clone();
        Task::perform(
            async move {
                let bytes = std::fs::read(&local).map_err(|e| e.to_string())?;
                remote::upload(&file, bytes, known).await
            },
            move |result| Message::RemoteSent(sent.clone(), result),
        )
    }

    pub(super) fn remote_sent(
        &mut self,
        path: PathBuf,
        result: Result<Option<u64>, String>,
    ) -> Task<Message> {
        match result {
            Ok(date) => {
                if let Some(entry) = self.remote_files.get_mut(&path) {
                    entry.1 = date;
                    let host = entry.0.host.clone();
                    self.set_status(t!("remote.sent", host = host.as_str()), false);
                }
            }
            Err(e) => self.set_status(format!("{}: {e}", t!("remote.send_failed")), true),
        }
        Task::none()
    }

    /// `host:/path` of a file that is a copy of a remote one.
    pub(super) fn remote_label(&self, path: &Path) -> Option<String> {
        self.remote_files
            .get(path)
            .map(|(file, _)| format!("{}:{}", file.host, file.path))
    }

    pub(super) fn view_remote(&self, dark: bool, view: &RemoteView) -> Element<'_, Message> {
        let p = se_theme::palette(dark);
        let ghost = move |active: bool| (se_theme::GhostButton { dark, active }).style();
        let mut items: Vec<Element<Message>> = Vec::new();
        if !view.entries.is_empty() || !view.dir.is_empty() {
            items.push(self.remote_row("..".to_string(), true, dark));
        }
        for entry in &view.entries {
            items.push(self.remote_row(entry.name.clone(), entry.is_dir, dark));
        }
        let status: Element<Message> = if view.loading {
            text(t!("remote.loading").to_string())
                .size(12)
                .color(p.muted)
                .into()
        } else if let Some(error) = &view.error {
            text(error.clone())
                .size(12)
                .color(se_theme::error_color(dark))
                .into()
        } else {
            Space::new().height(0).into()
        };
        container(
            column![
                text(t!("remote.title").to_string()).size(14).color(p.text),
                row![
                    text_input(&t!("remote.host"), &view.host)
                        .id(host_field_id())
                        .on_input(Message::RemoteHost)
                        .on_submit(Message::RemoteConnect)
                        .padding([6, 10])
                        .size(13)
                        .width(Length::FillPortion(2))
                        .style(se_theme::Field::new(dark).style()),
                    text_input(&t!("remote.folder"), &view.dir)
                        .on_input(Message::RemoteDir)
                        .on_submit(Message::RemoteConnect)
                        .padding([6, 10])
                        .size(13)
                        .width(Length::FillPortion(3))
                        .style(se_theme::Field::new(dark).style()),
                    button(text(t!("remote.go").to_string()).size(13))
                        .padding([6, 14])
                        .on_press(Message::RemoteConnect)
                        .style(ghost(true)),
                ]
                .spacing(8),
                status,
                scrollable(column(items).spacing(1).padding(4)).height(Length::Fixed(300.0)),
            ]
            .spacing(10)
            .padding(16),
        )
        .width(620)
        .style(se_theme::card(dark))
        .into()
    }

    fn remote_row(&self, name: String, is_dir: bool, dark: bool) -> Element<'_, Message> {
        let p = se_theme::palette(dark);
        button(
            row![
                text(if is_dir { "▸" } else { " " })
                    .size(13)
                    .color(p.muted)
                    .width(14),
                text(if is_dir {
                    format!("{name}/")
                } else {
                    name.clone()
                })
                .size(13)
                .font(se_theme::MONO),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .padding([4, 10])
        .width(Length::Fill)
        .on_press(Message::RemotePick(name))
        .style(
            (se_theme::GhostButton {
                dark,
                active: false,
            })
            .style(),
        )
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

    fn entry(name: &str, is_dir: bool) -> Entry {
        Entry {
            name: name.into(),
            is_dir,
        }
    }

    #[test]
    fn the_window_lists_a_folder_and_remembers_the_machine() {
        let mut app = app();
        let _ = app.update(Message::OpenRemote);
        let _ = app.update(Message::RemoteHost("me@box".into()));
        let _ = app.update(Message::RemoteConnect);
        let ActiveOverlay::Remote(view) = &app.overlay else {
            panic!("the window");
        };
        assert!(view.loading);
        let _ = app.update(Message::RemoteListed(
            "me@box".into(),
            Ok((
                "/srv".into(),
                vec![entry("app", true), entry("a.txt", false)],
            )),
        ));
        let ActiveOverlay::Remote(view) = &app.overlay else {
            panic!("the window");
        };
        assert_eq!(
            (view.dir.as_str(), view.entries.len(), view.loading),
            ("/srv", 2, false)
        );
        assert_eq!(app.config.remote_hosts, vec!["me@box".to_string()]);
        // A folder is entered, the parent climbed; a bad host is refused.
        let _ = app.update(Message::RemotePick("app".into()));
        let ActiveOverlay::Remote(view) = &app.overlay else {
            panic!("the window");
        };
        assert_eq!(view.dir, "/srv/app");
        let _ = app.update(Message::RemotePick("..".into()));
        let ActiveOverlay::Remote(view) = &app.overlay else {
            panic!("the window");
        };
        assert_eq!(view.dir, "/srv");
        // A whole name starts the opening of the file.
        let _ = app.update(Message::RemoteHost("me@box:/srv/a.txt".into()));
        let _ = app.update(Message::RemoteConnect);
        let ActiveOverlay::Remote(view) = &app.overlay else {
            panic!("the window");
        };
        assert!(view.loading && view.error.is_none());
        let _ = app.update(Message::RemoteHost("-oProxyCommand=x".into()));
        let _ = app.update(Message::RemoteConnect);
        let ActiveOverlay::Remote(view) = &app.overlay else {
            panic!("the window");
        };
        assert!(view.error.is_some() && !view.loading);
    }

    #[test]
    fn a_failure_is_shown_in_the_window() {
        let mut app = app();
        let _ = app.update(Message::OpenRemote);
        let _ = app.update(Message::RemoteHost("down".into()));
        let _ = app.update(Message::RemoteListed("down".into(), Err("no route".into())));
        let ActiveOverlay::Remote(view) = &app.overlay else {
            panic!("the window");
        };
        assert_eq!(view.error.as_deref(), Some("no route"));
        assert!(app.config.remote_hosts.is_empty());
    }

    #[test]
    fn a_download_is_kept_locally_opened_and_sent_back_at_each_save() {
        let mut app = app();
        let file = RemoteFile {
            host: "me@box".into(),
            path: "/srv/test-remote-copy.txt".into(),
        };
        let _ = app.update(Message::RemoteDownloaded(
            file.clone(),
            Ok((b"hello\n".to_vec(), Some(100))),
        ));
        let local = absolute(remote::local_copy(&file).unwrap());
        assert_eq!(std::fs::read(&local).unwrap(), b"hello\n");
        assert_eq!(app.remote_files.get(&local).map(|e| e.1), Some(Some(100)));
        assert_eq!(
            app.remote_label(&local).as_deref(),
            Some("me@box:/srv/test-remote-copy.txt")
        );
        // A file that is not a copy is not sent anywhere.
        let _ = app.remote_send(Path::new("/tmp/not-remote.txt"));
        // A confirmation brings the new date; a refusal is said.
        let _ = app.update(Message::RemoteSent(local.clone(), Ok(Some(200))));
        assert_eq!(app.remote_files.get(&local).map(|e| e.1), Some(Some(200)));
        assert!(!app.status_is_error);
        let _ = app.update(Message::RemoteSent(
            local.clone(),
            Err("the file changed on the server since it was opened".into()),
        ));
        assert!(app.status_is_error);
        let _ = std::fs::remove_file(local);
    }

    #[test]
    fn the_link_to_the_remote_file_survives_a_restart() {
        let mut app = app();
        let local = PathBuf::from("/tmp/simpleedit-test-link/f.txt");
        app.sidebar.add_file(local.clone());
        app.remote_files.insert(
            local.clone(),
            (
                RemoteFile {
                    host: "me@box".into(),
                    path: "/srv/f.txt".into(),
                },
                Some(7),
            ),
        );
        let session = app.build_session();
        let json = session.to_json().unwrap();
        let back: crate::session::Session = serde_json::from_str(&json).unwrap();
        let mut fresh = SimpleEditApp::blank(Config::default());
        let _ = fresh.restore_session(back);
        let (file, date) = fresh.remote_files.get(&local).expect("the link");
        assert_eq!(file.host, "me@box");
        assert_eq!(file.path, "/srv/f.txt");
        assert_eq!(*date, Some(7));
    }

    #[test]
    fn a_failed_download_stays_in_the_window() {
        let mut app = app();
        let _ = app.update(Message::OpenRemote);
        let file = RemoteFile {
            host: "box".into(),
            path: "/a".into(),
        };
        let _ = app.update(Message::RemoteDownloaded(
            file,
            Err("the file is too large".into()),
        ));
        let ActiveOverlay::Remote(view) = &app.overlay else {
            panic!("the window stays");
        };
        assert_eq!(view.error.as_deref(), Some("the file is too large"));
    }
}
