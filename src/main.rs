#[macro_use]
extern crate rust_i18n;

use iced::{window, Size};

fn load_icon() -> Option<window::Icon> {
    let bytes = include_bytes!("assets/icon.png");
    let img = image::load_from_memory(bytes).ok()?.into_rgba8();
    let (w, h) = img.dimensions();
    window::icon::from_rgba(img.into_raw(), w, h).ok()
}

mod api;
mod app;
mod bookmarks;
mod changelog;
mod code_editor;
mod completion;
mod config;
mod database;
mod definition;
mod editor;
mod everywhere;
mod file_icons;
mod find_replace;
mod finder;
mod floating;
mod folding;
mod formatter;
mod fsutil;
mod git;
mod git_panel;
mod history;
mod i18n;
mod ipc;
mod lint;
mod local_history;
mod lsp;
mod lsp_install;
mod menu_bar;
#[cfg(all(target_os = "macos", not(test)))]
mod native_menu;
mod palette;
mod pointer_area;
mod preferences;
mod preview;
mod projects;
mod remote;
mod scratch;
mod search;
mod session;
mod sidebar;
mod spell;
mod ssh_trust;
mod tab_strip;
mod tabs;
mod tasks;
mod terminal;
mod text_ops;
mod theme;
mod trust;
mod updater;
mod watch;

i18n!("src/i18n");

/// The files named on the command line (`file`, `file:line`, `file:line:col`).
/// Names need not be valid UTF-8, and anything after `--` is a file even if
/// it starts with a dash; other options (`--something`) are ignored.
fn file_args(args: impl Iterator<Item = std::ffi::OsString>) -> Vec<ipc::Target> {
    let mut files = Vec::new();
    let mut only_files = false;
    for arg in args {
        if !only_files && arg == "--" {
            only_files = true;
        } else if only_files || !arg.to_string_lossy().starts_with('-') {
            files.push(ipc::Target::parse(&arg));
        }
    }
    files
}

/// `--new-window` (or `-n`): the files open in a window of their own.
fn wants_new_window(args: impl Iterator<Item = std::ffi::OsString>) -> bool {
    args.take_while(|a| a != "--")
        .any(|a| a == "--new-window" || a == "-n")
}

/// Started from the Finder, the app gets launchd's minimal `PATH`: the usual
/// tool folders are added to it, so that what it starts (language servers
/// and their `#!/usr/bin/env node`, formatters, `npm`) finds its programs.
const PATH_MARK: &str = "__SIMPLEEDIT_PATH__";

/// The `PATH` a login shell printed between two markers, so that what the
/// shell's startup files echo around it is ignored.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn marked_path(output: &str) -> Option<&str> {
    let start = output.find(PATH_MARK)? + PATH_MARK.len();
    let len = output[start..].find(PATH_MARK)?;
    let path = output[start..start + len].trim();
    (!path.is_empty()).then_some(path)
}

/// The `PATH` of the user's login shell (nvm and other version managers set
/// it up there), or `None` if the shell is slow or fails: it is given 3 s.
#[cfg(target_os = "macos")]
fn login_shell_path() -> Option<String> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    let shell = std::env::var_os("SHELL").unwrap_or_else(|| "/bin/zsh".into());
    let mut child = Command::new(shell)
        .arg("-lc")
        .arg(format!("printf '{PATH_MARK}%s{PATH_MARK}' \"$PATH\""))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut out = Vec::new();
        let _ = stdout.read_to_end(&mut out);
        let _ = tx.send(out);
    });
    match rx.recv_timeout(std::time::Duration::from_secs(3)) {
        Ok(out) => {
            let _ = child.wait();
            marked_path(&String::from_utf8_lossy(&out)).map(str::to_string)
        }
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            None
        }
    }
}

#[cfg(target_os = "macos")]
fn complete_path() {
    let mut dirs: Vec<std::path::PathBuf> =
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).collect();
    // The shell's own folders (nvm, ...) come first.
    if let Some(shell) = login_shell_path() {
        let mut first: Vec<std::path::PathBuf> = std::env::split_paths(&shell).collect();
        first.retain(|d| !d.as_os_str().is_empty());
        dirs.retain(|d| !first.contains(d));
        first.extend(dirs);
        dirs = first;
    }
    let mut extra: Vec<std::path::PathBuf> =
        ["/opt/homebrew/bin", "/opt/homebrew/sbin", "/usr/local/bin"]
            .map(std::path::PathBuf::from)
            .into();
    if let Some(home) = dirs::home_dir() {
        extra.extend(
            [".cargo/bin", ".local/bin", "go/bin", ".npm-global/bin"].map(|d| home.join(d)),
        );
    }
    for dir in extra {
        if !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    if let Ok(path) = std::env::join_paths(dirs) {
        // Single-threaded at this point.
        std::env::set_var("PATH", path);
    }
}

fn main() -> iced::Result {
    #[cfg(target_os = "macos")]
    complete_path();
    // The saved preference wins; Config::load() falls back to the system locale
    // only when no config file exists yet.
    let config = config::Config::load();
    rust_i18n::set_locale(&config.locale);

    let files = file_args(std::env::args_os().skip(1));
    // Files named while a window runs open there; the window comes forward.
    if !files.is_empty() && !wants_new_window(std::env::args_os().skip(1)) && ipc::send(&files) {
        return Ok(());
    }

    // winit clears the launcher's startup token when it creates its event
    // loop; keep a copy so the window can be activated with it (iced_winit).
    #[cfg(target_os = "linux")]
    if let Some(token) =
        std::env::var_os("XDG_ACTIVATION_TOKEN").or_else(|| std::env::var_os("DESKTOP_STARTUP_ID"))
    {
        // Single-threaded at this point.
        std::env::set_var("SIMPLEEDIT_ACTIVATION_TOKEN", token);
    }

    #[cfg(target_os = "linux")]
    let platform_specific = window::settings::PlatformSpecific {
        application_id: "simpleedit".to_string(),
        ..Default::default()
    };

    #[cfg(not(target_os = "linux"))]
    let platform_specific = window::settings::PlatformSpecific::default();

    let window_size = Size::new(config.window_width, config.window_height);
    // `boot` may run more than once in principle; the flags are handed over
    // the first time.
    let flags = std::sync::Mutex::new(Some(app::Flags { config, files }));
    let mut application = iced::application(
        move || {
            let flags = flags
                .lock()
                .ok()
                .and_then(|mut f| f.take())
                .expect("the application boots once");
            app::SimpleEditApp::new(flags)
        },
        app::SimpleEditApp::update,
        app::SimpleEditApp::view,
    )
    .title(app::SimpleEditApp::title)
    .subscription(app::SimpleEditApp::subscription)
    .theme(app::SimpleEditApp::theme)
    .default_font(theme::UI)
    .window(window::Settings {
        // Closing the window goes through the app (Message::Quit) so the
        // session is saved first.
        exit_on_close_request: false,
        size: window_size,
        min_size: Some(Size::new(600.0, 400.0)),
        resizable: true,
        icon: load_icon(),
        platform_specific,
        ..Default::default()
    });
    for font in theme::font_files() {
        application = application.font(font);
    }
    application.run()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    fn args(list: &[&str]) -> Vec<std::path::PathBuf> {
        file_args(list.iter().map(OsString::from))
            .into_iter()
            .map(|t| t.path)
            .collect()
    }

    #[test]
    fn the_shell_path_is_read_between_markers_and_noise_is_ignored() {
        let out = format!("welcome\nnvm ready\n{PATH_MARK}/a/bin:/b/bin{PATH_MARK}trailing");
        assert_eq!(marked_path(&out), Some("/a/bin:/b/bin"));
        assert_eq!(marked_path("no marker"), None);
        assert_eq!(marked_path(&format!("{PATH_MARK}{PATH_MARK}")), None);
        assert_eq!(marked_path(&format!("{PATH_MARK}/a")), None);
    }

    #[test]
    fn a_new_window_is_asked_for_by_an_option_not_a_file() {
        let asked = |list: &[&str]| wants_new_window(list.iter().map(OsString::from));
        assert!(asked(&["--new-window", "a"]));
        assert!(asked(&["a", "-n"]));
        assert!(!asked(&["a"]));
        assert!(!asked(&["--", "-n"]));
    }

    #[test]
    fn a_place_follows_a_file_name() {
        let found = file_args([OsString::from("zzz-none.rs:7:2")].into_iter());
        assert_eq!(found[0].path, std::path::PathBuf::from("zzz-none.rs"));
        assert_eq!((found[0].line, found[0].col), (Some(7), Some(2)));
    }

    #[test]
    fn every_file_argument_is_kept_and_options_are_not() {
        assert_eq!(args(&["a.txt", "--flag", "b.txt"]).len(), 2);
        assert_eq!(
            args(&["--", "-odd.txt"]),
            vec![std::path::PathBuf::from("-odd.txt")]
        );
        assert!(args(&[]).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn a_name_that_is_not_utf8_does_not_panic() {
        use std::os::unix::ffi::OsStringExt;
        let odd = OsString::from_vec(b"caf\xe9.txt".to_vec());
        assert_eq!(
            file_args(std::iter::once(odd.clone())),
            vec![ipc::Target::file(std::path::PathBuf::from(odd))]
        );
    }
}
