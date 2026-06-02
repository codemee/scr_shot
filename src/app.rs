use std::sync::{Arc, Mutex};
use std::sync::mpsc::{self, Receiver, Sender};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::capture::{overlay, screen};
use crate::config::Config;
use crate::event::AppEvent;
use crate::hotkey;
use crate::tray;

#[derive(PartialEq)]
enum AppState {
    Idle,
    OverlayRegion,
    OverlayPick,
    Editing,
}

pub fn run(config: Config) -> anyhow::Result<()> {
    let (tx, rx) = mpsc::channel::<AppEvent>();
    let config   = Arc::new(Mutex::new(config));

    let msg_hwnd = tray::create_message_window(tx.clone(), config.clone());
    hotkey::register_all(msg_hwnd)?;
    let _tray = tray::make_tray(msg_hwnd);

    let tx2       = tx.clone();
    let save_dir  = { config.lock().unwrap().save_dir.clone() };
    let config_sm = config.clone();

    std::thread::spawn(move || state_machine(rx, tx2, save_dir, config_sm));

    unsafe {
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, windows::Win32::Foundation::HWND(std::ptr::null_mut()), 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    hotkey::unregister_all(msg_hwnd);
    Ok(())
}

fn state_machine(
    rx: Receiver<AppEvent>,
    tx: Sender<AppEvent>,
    save_dir: std::path::PathBuf,
    config: Arc<Mutex<Config>>,
) {
    let mut state = AppState::Idle;

    for event in rx {
        match (&state, event) {
            // ── Overlay 立刻出現，不延遲 ──────────────────────────────
            (AppState::Idle, AppEvent::CaptureRegion) => {
                state = AppState::OverlayRegion;
                let tx2 = tx.clone();
                std::thread::spawn(move || overlay::show_region(tx2));
            }
            (AppState::Idle, AppEvent::CapturePickWindow) => {
                state = AppState::OverlayPick;
                let tx2 = tx.clone();
                std::thread::spawn(move || overlay::show_pick(tx2));
            }

            // ── 作用中視窗：先鎖定視窗，再延遲後截圖 ─────────────────
            (AppState::Idle, AppEvent::CaptureActiveWindow) => {
                state = AppState::Editing;
                let tx2  = tx.clone();
                let dir  = save_dir.clone();
                let (delay, cursor) = {
                    let c = config.lock().unwrap();
                    (c.capture_delay_secs, c.capture_cursor)
                };
                std::thread::spawn(move || {
                    // 先取得目前作用中視窗的 rect（選定標的）
                    match screen::active_window_rect() {
                        Ok(rect) => {
                            if delay > 0 { overlay::show_countdown(delay, Some(rect)); }
                            match screen::capture_rect(rect, cursor) {
                                Ok(bmp) => crate::editor::open(bmp, tx2, dir),
                                Err(e) => {
                                    eprintln!("capture error: {e}");
                                    let _ = tx2.send(AppEvent::EditorCancelled);
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("capture error: {e}");
                            let _ = tx2.send(AppEvent::EditorCancelled);
                        }
                    }
                });
            }

            // ── 框選完成：延遲後截圖 ──────────────────────────────────
            (AppState::OverlayRegion, AppEvent::RegionSelected(rect)) => {
                state = AppState::Editing;
                let tx2  = tx.clone();
                let dir  = save_dir.clone();
                let (delay, cursor) = {
                    let c = config.lock().unwrap();
                    (c.capture_delay_secs, c.capture_cursor)
                };
                std::thread::spawn(move || {
                    if delay > 0 {
                        overlay::show_countdown(delay, Some(rect));
                    } else {
                        std::thread::sleep(std::time::Duration::from_millis(80));
                    }
                    match screen::capture_rect(rect, cursor) {
                        Ok(bmp) => crate::editor::open(bmp, tx2, dir),
                        Err(e) => {
                            eprintln!("capture error: {e}");
                            let _ = tx2.send(AppEvent::EditorCancelled);
                        }
                    }
                });
            }

            // ── 點選視窗完成：延遲後截圖 ─────────────────────────────
            (AppState::OverlayPick, AppEvent::WindowPicked(hwnd_raw)) => {
                state = AppState::Editing;
                let tx2  = tx.clone();
                let dir  = save_dir.clone();
                let (delay, cursor) = {
                    let c = config.lock().unwrap();
                    (c.capture_delay_secs, c.capture_cursor)
                };
                std::thread::spawn(move || {
                    let hwnd = windows::Win32::Foundation::HWND(hwnd_raw as *mut _);
                    if delay > 0 {
                        let hl = screen::window_rect(hwnd).ok();
                        overlay::show_countdown(delay, hl);
                    } else {
                        std::thread::sleep(std::time::Duration::from_millis(80));
                    }
                    match screen::window_rect(hwnd).and_then(|r| screen::capture_rect(r, cursor)) {
                        Ok(bmp) => crate::editor::open(bmp, tx2, dir),
                        Err(e) => {
                            eprintln!("capture error: {e}");
                            let _ = tx2.send(AppEvent::EditorCancelled);
                        }
                    }
                });
            }

            (AppState::OverlayRegion | AppState::OverlayPick, AppEvent::OverlayCancelled) => {
                state = AppState::Idle;
            }
            (AppState::Editing, AppEvent::EditorSave { .. } | AppEvent::EditorCancelled) => {
                state = AppState::Idle;
            }
            (_, AppEvent::TrayQuit) => break,
            _ => {}
        }
    }
}
