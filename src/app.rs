use std::sync::{Arc, Mutex};
use std::sync::mpsc::{self, Receiver, Sender};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
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
}

pub fn run(config: Config) -> anyhow::Result<()> {
    let (tx, rx) = mpsc::channel::<AppEvent>();
    let config      = Arc::new(Mutex::new(config));
    let editor_hwnd = Arc::new(Mutex::new(Option::<isize>::None));

    let msg_hwnd = tray::create_message_window(tx.clone(), config.clone(), editor_hwnd.clone());
    hotkey::register_all(msg_hwnd)?;
    let _tray = tray::make_tray(msg_hwnd);

    let tx2          = tx.clone();
    let save_dir     = { config.lock().unwrap().save_dir.clone() };
    let config_sm    = config.clone();
    let editor_sm    = editor_hwnd.clone();

    std::thread::spawn(move || {
        state_machine(rx, tx2, save_dir, config_sm, editor_sm);
    });

    unsafe {
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, HWND(std::ptr::null_mut()), 0, 0).as_bool() {
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
    editor_hwnd: Arc<Mutex<Option<isize>>>,
) {
    let mut state = AppState::Idle;

    for event in rx {
        match (&state, event) {
            (AppState::Idle, AppEvent::CaptureRegion) => {
                state = AppState::OverlayRegion;
                let tx2 = tx.clone();
                let hide = config.lock().unwrap().hide_editor_on_capture;
                let eh0 = if hide { Some(editor_hwnd.clone()) } else { None };
                std::thread::spawn(move || {
                    if let Some(eh) = eh0 { hide_editor_if_needed(&eh); }
                    overlay::show_region(tx2);
                });
            }
            (AppState::Idle, AppEvent::CapturePickWindow) => {
                state = AppState::OverlayPick;
                let tx2 = tx.clone();
                let hide = config.lock().unwrap().hide_editor_on_capture;
                let eh1 = if hide { Some(editor_hwnd.clone()) } else { None };
                std::thread::spawn(move || {
                    if let Some(eh) = eh1 { hide_editor_if_needed(&eh); }
                    overlay::show_pick(tx2);
                });
            }
            (AppState::Idle, AppEvent::CaptureActiveWindow) => {
                let tx2  = tx.clone();
                let dir  = save_dir.clone();
                let (delay, cursor, auto_copy, hide) = {
                    let c = config.lock().unwrap();
                    (c.capture_delay_secs, c.capture_cursor, c.auto_copy, c.hide_editor_on_capture)
                };
                let editor_clone = editor_hwnd.clone();
                let config_clone = config.clone();
                let eh2 = if hide { Some(editor_hwnd.clone()) } else { None };
                std::thread::spawn(move || {
                    if let Some(eh) = eh2 { hide_editor_if_needed(&eh); }
                    match screen::active_window_rect() {
                        Ok(rect) => {
                            if delay > 0 { overlay::show_countdown(delay, Some(rect)); }
                            match screen::capture_rect(rect, cursor) {
                                Ok(bmp) => dispatch_capture(bmp, auto_copy, tx2, dir, editor_clone, config_clone),
                                Err(e) => { eprintln!("capture error: {e}"); }
                            }
                        }
                        Err(e) => { eprintln!("capture error: {e}"); }
                    }
                });
            }
            (AppState::OverlayRegion, AppEvent::RegionSelected(rect)) => {
                state = AppState::Idle;
                let tx2  = tx.clone();
                let dir  = save_dir.clone();
                let (delay, cursor, auto_copy) = {
                    let c = config.lock().unwrap();
                    (c.capture_delay_secs, c.capture_cursor, c.auto_copy)
                };
                let editor_clone = editor_hwnd.clone();
                let config_clone = config.clone();
                std::thread::spawn(move || {
                    if delay > 0 {
                        overlay::show_countdown(delay, Some(rect));
                    } else {
                        std::thread::sleep(std::time::Duration::from_millis(80));
                    }
                    match screen::capture_rect(rect, cursor) {
                        Ok(bmp) => dispatch_capture(bmp, auto_copy, tx2, dir, editor_clone, config_clone),
                        Err(e) => { eprintln!("capture error: {e}"); }
                    }
                });
            }
            (AppState::OverlayPick, AppEvent::WindowPicked(hwnd_raw)) => {
                state = AppState::Idle;
                let tx2  = tx.clone();
                let dir  = save_dir.clone();
                let (delay, cursor, auto_copy) = {
                    let c = config.lock().unwrap();
                    (c.capture_delay_secs, c.capture_cursor, c.auto_copy)
                };
                let editor_clone = editor_hwnd.clone();
                let config_clone = config.clone();
                std::thread::spawn(move || {
                    let hwnd = windows::Win32::Foundation::HWND(hwnd_raw as *mut _);
                    if delay > 0 {
                        let hl = screen::window_rect(hwnd).ok();
                        overlay::show_countdown(delay, hl);
                    } else {
                        std::thread::sleep(std::time::Duration::from_millis(80));
                    }
                    match screen::window_rect(hwnd).and_then(|r| screen::capture_rect(r, cursor)) {
                        Ok(bmp) => dispatch_capture(bmp, auto_copy, tx2, dir, editor_clone, config_clone),
                        Err(e) => { eprintln!("capture error: {e}"); }
                    }
                });
            }
            (AppState::OverlayRegion | AppState::OverlayPick, AppEvent::OverlayCancelled) => {
                state = AppState::Idle;
            }
            // EditorSave/EditorCancelled: 舊架構保留，在持久視窗模式下可忽略
            (_, AppEvent::EditorSave { .. } | AppEvent::EditorCancelled) => {}
            (_, AppEvent::ShowEditor) => {
                // 雙按系統匣：顯示編輯視窗
                let hwnd_val = { *editor_hwnd.lock().unwrap() };
                if let Some(v) = hwnd_val {
                    unsafe {
                        let hw = HWND(v as *mut _);
                        if IsWindow(hw).as_bool() {
                            PostMessageW(hw, crate::editor::WM_SHOW_EDITOR, WPARAM(0), LPARAM(0)).ok();
                        }
                    }
                }
            }
            (_, AppEvent::TrayQuit) => {
                // 通知編輯器執行緒強制結束
                let hwnd_val = { *editor_hwnd.lock().unwrap() };
                if let Some(v) = hwnd_val {
                    unsafe {
                        let hw = HWND(v as *mut _);
                        if IsWindow(hw).as_bool() {
                            PostMessageW(hw, crate::editor::WM_FORCE_QUIT, WPARAM(0), LPARAM(0)).ok();
                        }
                    }
                }
                break;
            }
            _ => {}
        }
    }
}

/// 若選項開啟且編輯視窗可見，先隱藏再擷取
fn hide_editor_if_needed(editor_hwnd: &Arc<Mutex<Option<isize>>>) {
    let hwnd_val = { *editor_hwnd.lock().unwrap() };
    if let Some(v) = hwnd_val {
        unsafe {
            let hw = HWND(v as *mut _);
            use windows::Win32::UI::WindowsAndMessaging::{IsWindowVisible, ShowWindow, SW_HIDE};
            if IsWindow(hw).as_bool() && IsWindowVisible(hw).as_bool() {
                ShowWindow(hw, SW_HIDE);
                std::thread::sleep(std::time::Duration::from_millis(80));
            }
        }
    }
}

/// 捕獲後：auto_copy 先複製剪貼簿；送到既有編輯器或新建
fn dispatch_capture(
    bmp: crate::capture::screen::ScreenBitmap,
    auto_copy: bool,
    tx: Sender<AppEvent>,
    dir: std::path::PathBuf,
    editor_hwnd: Arc<Mutex<Option<isize>>>,
    config: Arc<Mutex<crate::config::Config>>,
) {
    if auto_copy {
        let _ = crate::output::clipboard::copy_to_clipboard(&bmp);
    }

    let hwnd_val = { *editor_hwnd.lock().unwrap() };
    if let Some(v) = hwnd_val {
        unsafe {
            let hw = HWND(v as *mut _);
            if IsWindow(hw).as_bool() {
                // 傳遞 heap 上的 bmp；editor 負責 Box::from_raw
                let ptr = Box::into_raw(Box::new(bmp));
                PostMessageW(hw, crate::editor::WM_NEW_TAB,
                    WPARAM(0), LPARAM(ptr as isize)).ok();
                return;
            }
        }
    }

    // 編輯器尚未存在，建立持久視窗
    let editor_hwnd_clone = editor_hwnd.clone();
    std::thread::spawn(move || {
        crate::editor::open(bmp, tx, dir, editor_hwnd_clone, config);
    });
}
