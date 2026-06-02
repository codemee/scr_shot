#![windows_subsystem = "windows"]
#![allow(unused_must_use)]

mod app;
mod capture;
mod config;
mod editor;
mod event;
mod hotkey;
mod icon;
mod output;
mod tray;

fn main() {
    let config = config::Config::default();
    if let Err(e) = app::run(config) {
        eprintln!("fatal: {e}");
    }
}
