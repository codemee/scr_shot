#![windows_subsystem = "windows"]
#![allow(unused_must_use)]

mod app;
mod capture;
mod config;
mod editor;
mod event;
mod hotkey;
mod i18n;
mod icon;
mod menu_icon;
mod theme;
mod output;
mod tray;

fn main() {
    let config = config::Config::default();
    i18n::init(&config.language);
    theme::init(&config.theme);
    if let Err(e) = app::run(config) {
        eprintln!("fatal: {e}");
    }
}
