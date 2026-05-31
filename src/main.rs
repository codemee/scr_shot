mod app;
mod app_state;
mod capture;
mod clipboard;
mod config;
mod editor;
mod hotkey;
mod tray;
mod win32_thread;

use std::sync::mpsc;

use app::ScreenshotApp;
use app_state::{MainCmd, Win32Event};
use config::Config;

fn main() {
    let config = Config::load();
    let hotkeys = config.hotkeys.clone();

    let (main_tx, main_rx): (mpsc::Sender<MainCmd>, mpsc::Receiver<MainCmd>) = mpsc::channel();
    let (win_tx, win_rx): (mpsc::Sender<Win32Event>, mpsc::Receiver<Win32Event>) = mpsc::channel();

    std::thread::spawn(move || {
        win32_thread::run(main_rx, win_tx, hotkeys);
    });

    let app = ScreenshotApp::new(config, main_tx, win_rx);

    let native_options = eframe::NativeOptions {
        initial_window_size: Some(egui::vec2(960.0, 720.0)),
        run_and_return: false,
        ..Default::default()
    };

    if let Err(e) = eframe::run_native(
        "srcshot",
        native_options,
        Box::new(|_cc| Box::new(app)),
    ) {
        eprintln!("eframe error: {e}");
    }
}
