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

    let msg_hwnd = tray::create_message_window(tx.clone());
    hotkey::register_all(msg_hwnd)?;
    let _tray = tray::make_tray(msg_hwnd);

    // Pump the Win32 message loop on this thread; events arrive via WM_HOTKEY
    // and WM_COMMAND and are forwarded to our channel in tray::msg_wnd_proc.
    // We run a secondary thread to consume the channel and drive the state machine.
    let tx2 = tx.clone();
    let save_dir = config.save_dir.clone();

    std::thread::spawn(move || {
        state_machine(rx, tx2, save_dir);
    });

    // Main thread: Win32 message loop
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

fn state_machine(rx: Receiver<AppEvent>, tx: Sender<AppEvent>, save_dir: std::path::PathBuf) {
    let mut state = AppState::Idle;

    for event in rx {
        match (&state, event) {
            (AppState::Idle, AppEvent::CaptureRegion) => {
                state = AppState::OverlayRegion;
                let tx2 = tx.clone();
                std::thread::spawn(move || overlay::show_region(tx2));
            }
            (AppState::Idle, AppEvent::CaptureActiveWindow) => {
                state = AppState::Editing;
                let tx2 = tx.clone();
                let dir = save_dir.clone();
                std::thread::spawn(move || {
                    match screen::active_window_rect().and_then(|r| screen::capture_rect(r)) {
                        Ok(bmp) => crate::editor::open(bmp, tx2, dir),
                        Err(e) => {
                            eprintln!("capture error: {e}");
                            let _ = tx2.send(AppEvent::EditorCancelled);
                        }
                    }
                });
            }
            (AppState::Idle, AppEvent::CapturePickWindow) => {
                state = AppState::OverlayPick;
                let tx2 = tx.clone();
                std::thread::spawn(move || overlay::show_pick(tx2));
            }
            (AppState::OverlayRegion, AppEvent::RegionSelected(rect)) => {
                state = AppState::Editing;
                let tx2 = tx.clone();
                let dir = save_dir.clone();
                std::thread::spawn(move || {
                    // 等螢幕刷新（overlay 已隱藏，但 GDI 還需一點時間合成）
                    std::thread::sleep(std::time::Duration::from_millis(80));
                    match screen::capture_rect(rect) {
                        Ok(bmp) => crate::editor::open(bmp, tx2, dir),
                        Err(e) => {
                            eprintln!("capture error: {e}");
                            let _ = tx2.send(AppEvent::EditorCancelled);
                        }
                    }
                });
            }
            (AppState::OverlayPick, AppEvent::WindowPicked(hwnd_raw)) => {
                state = AppState::Editing;
                let tx2 = tx.clone();
                let dir = save_dir.clone();
                std::thread::spawn(move || {
                    // 等 GDI 完成合成（overlay 已隱藏，但需時間刷新）
                    std::thread::sleep(std::time::Duration::from_millis(80));
                    let hwnd = windows::Win32::Foundation::HWND(hwnd_raw as *mut _);
                    match screen::window_rect(hwnd).and_then(|r| screen::capture_rect(r)) {
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
            (_, AppEvent::TrayQuit) => {
                break;
            }
            _ => {} // ignore invalid transitions
        }
    }
}
