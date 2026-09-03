#[macro_use]
extern crate rust_i18n;

use iced::{window, Application, Settings, Size};

fn load_icon() -> Option<window::Icon> {
    let bytes = include_bytes!("assets/icon.png");
    let img = image::load_from_memory(bytes).ok()?.into_rgba8();
    let (w, h) = img.dimensions();
    window::icon::from_rgba(img.into_raw(), w, h).ok()
}

mod app;
mod config;
mod editor;
mod formatter;
mod i18n;
mod menu_bar;
mod preferences;
mod preview;
mod search;
mod session;
mod sidebar;
mod theme;
mod updater;

i18n!("src/i18n");

fn main() -> iced::Result {
    // The saved preference wins; Config::load() falls back to the system locale
    // only when no config file exists yet.
    rust_i18n::set_locale(&config::Config::load().locale);

    let file_arg = std::env::args()
        .nth(1)
        .filter(|a| !a.starts_with('-'))
        .map(std::path::PathBuf::from);

    #[cfg(target_os = "linux")]
    let platform_specific = window::settings::PlatformSpecific {
        application_id: "simpleedit".to_string(),
    };

    #[cfg(not(target_os = "linux"))]
    let platform_specific = window::settings::PlatformSpecific::default();

    app::SimpleEditApp::run(Settings {
        flags: file_arg,
        window: window::Settings {
            size: Size::new(1200.0, 800.0),
            min_size: Some(Size::new(600.0, 400.0)),
            resizable: true,
            icon: load_icon(),
            platform_specific,
            ..Default::default()
        },
        ..Default::default()
    })
}
