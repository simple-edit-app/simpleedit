#[macro_use]
extern crate rust_i18n;

use iced::{window, Size};

fn load_icon() -> Option<window::Icon> {
    let bytes = include_bytes!("assets/icon.png");
    let img = image::load_from_memory(bytes).ok()?.into_rgba8();
    let (w, h) = img.dimensions();
    window::icon::from_rgba(img.into_raw(), w, h).ok()
}

mod app;
mod bookmarks;
mod changelog;
mod code_editor;
mod config;
mod editor;
mod floating;
mod formatter;
mod fsutil;
mod history;
mod i18n;
mod menu_bar;
mod palette;
mod preferences;
mod preview;
mod search;
mod session;
mod sidebar;
mod tabs;
mod text_ops;
mod theme;
mod updater;

i18n!("src/i18n");

/// The files named on the command line. Names need not be valid UTF-8, and
/// anything after `--` is a file even if it starts with a dash; other options
/// (`--something`) are ignored.
fn file_args(args: impl Iterator<Item = std::ffi::OsString>) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    let mut only_files = false;
    for arg in args {
        if !only_files && arg == "--" {
            only_files = true;
        } else if only_files || !arg.to_string_lossy().starts_with('-') {
            files.push(std::path::PathBuf::from(arg));
        }
    }
    files
}

fn main() -> iced::Result {
    // The saved preference wins; Config::load() falls back to the system locale
    // only when no config file exists yet.
    let config = config::Config::load();
    rust_i18n::set_locale(&config.locale);

    let files = file_args(std::env::args_os().skip(1));

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
            vec![std::path::PathBuf::from(odd)]
        );
    }
}
