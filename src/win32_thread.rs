use std::sync::mpsc;
use std::time::Duration;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM, TRUE};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::UI::Shell::{NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_SETVERSION, NOTIFYICONDATAW, Shell_NotifyIconW};

use crate::app_state::{CaptureMode, CapturedImage, MainCmd, Win32Event};
use crate::capture::capturer;
use crate::capture::overlay;
use crate::config::HotkeyConfig;
use crate::hotkey;
use crate::tray;

const WM_TRAYICON: u32 = 0x8000;
const HIDDEN_CLASS: &str = "SrcshotHidden";
const TRAY_ID: u16 = 1001;

fn register_hidden_class(hinst: HINSTANCE) {
    let mut wc: WNDCLASSA = unsafe { std::mem::zeroed() };
    wc.lpfnWndProc = Some(hidden_wndproc);
    wc.hInstance = hinst;
    wc.lpszClassName = windows::core::s!(HIDDEN_CLASS);
    unsafe { let _ = RegisterClassA(&wc); }
}

unsafe extern "system" fn hidden_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_CREATE {
        let cs = &*(lparam.0 as *const CREATESTRUCTA);
        let data_ptr = cs.lpCreateParams;
        SetWindowLongPtrA(hwnd, GWLP_USERDATA, data_ptr as isize);
        return LRESULT(0);
    }

    let data_ptr = GetWindowLongPtrA(hwnd, GWLP_USERDATA);
    if data_ptr == 0 {
        return DefWindowProcA(hwnd, msg, wparam, lparam);
    }

    match msg {
        WM_HOTKEY => {
            let thread_data = &mut *(data_ptr as *mut ThreadData);
            let mode = match wparam.0 as u32 {
                1 => CaptureMode::FullScreen,
                2 => CaptureMode::ActiveWindow,
                3 => CaptureMode::Region,
                4 => CaptureMode::SelectWindow,
                _ => return LRESULT(0),
            };
            thread_data.last_capture_mode = mode;
            PostMessageA(hwnd, WM_START_CAPTURE, WPARAM(0), LPARAM(0));
            LRESULT(0)
        }

        WM_TRAYICON => {
            if (lparam.0 & 0xFFFF) == WM_LBUTTONUP as u64 || (lparam.0 & 0xFFFF) == 0x202 {
                // Set focus to main window (show it)
                PostMessageA(hwnd, WM_SHOW_MAIN, WPARAM(0), LPARAM(0));
            } else if (lparam.0 & 0xFFFF) == 0x205 {
                struct MenuInfo { hwnd: HWND, data_ptr: isize }
                let menu = CreatePopupMenu();
                AppendMenuA(menu, 0, 100, windows::core::s!("顯示視窗"));
                AppendMenuA(menu, 0, 101, windows::core::s!("全螢幕截圖"));
                AppendMenuA(menu, 0, 102, windows::core::s!("離開"));

                let mut pt = POINT::default();
                GetCursorPos(&mut pt);
                SetForegroundWindow(hwnd);
                let cmd = TrackPopupMenu(menu, 0x0100, pt.x, pt.y, 0, hwnd, None);
                if cmd == 100 {
                    PostMessageA(hwnd, WM_SHOW_MAIN, WPARAM(0), LPARAM(0));
                } else if cmd == 101 {
                    let td = &mut *(data_ptr as *mut ThreadData);
                    td.last_capture_mode = CaptureMode::FullScreen;
                    PostMessageA(hwnd, WM_START_CAPTURE, WPARAM(0), LPARAM(0));
                } else if cmd == 102 {
                    PostMessageA(hwnd, WM_QUIT, WPARAM(0), LPARAM(0));
                }
                DestroyMenu(menu);
            }
            LRESULT(0)
        }

        _ => DefWindowProcA(hwnd, msg, wparam, lparam),
    }
}

const WM_SHOW_MAIN: u32 = 0x8001;
const WM_START_CAPTURE: u32 = 0x8002;
const WM_HIDE_MAIN: u32 = 0x8003;

struct ThreadData {
    tx: mpsc::Sender<Win32Event>,
    rx: mpsc::Receiver<MainCmd>,
    last_capture_mode: CaptureMode,
    hotkey_entries: Vec<hotkey::HotkeyEntry>,
}

pub fn run(rx: mpsc::Receiver<MainCmd>, tx: mpsc::Sender<Win32Event>, hotkey_cfg: HotkeyConfig) {
    unsafe {
        let hinst = GetModuleHandleA(None).unwrap();
        register_hidden_class(hinst);

        let thread_data = Box::into_raw(Box::new(ThreadData {
            tx,
            rx,
            last_capture_mode: CaptureMode::Region,
            hotkey_entries: vec![],
        }));

        let hwnd = CreateWindowExA(
            WS_EX_APPWINDOW,
            windows::core::s!(HIDDEN_CLASS),
            windows::core::s!("srcshot"),
            WS_OVERLAPPEDWINDOW,
            0, 0, 0, 0,
            None, None, hinst,
            Some(thread_data as *const _ as *const _),
        );

        if hwnd.is_invalid() {
            let _ = Box::from_raw(thread_data);
            return;
        }

        let td = &mut *thread_data;
        td.hotkey_entries = hotkey::parse_and_register(hwnd.0 as isize, &hotkey_cfg);
        tray::create(hwnd, TRAY_ID);

        SetWindowPos(hwnd, None, 0, 0, 0, 0, SWP_HIDEWINDOW);

        let mut msg = MSG::default();

        loop {
            while PeekMessageA(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT {
                    tray::destroy(hwnd, TRAY_ID);
                    hotkey::unregister_all(hwnd.0 as isize, &td.hotkey_entries);
                    let _ = Box::from_raw(thread_data);
                    return;
                }

                if msg.message == WM_SHOW_MAIN {
                    let _ = td.tx.send(Win32Event::ShowWindow);
                    continue;
                }

                if msg.message == WM_START_CAPTURE {
                    let mode = td.last_capture_mode;
                    let result = match mode {
                        CaptureMode::FullScreen => capturer::capture_fullscreen(),
                        CaptureMode::ActiveWindow => capturer::capture_foreground_window(),
                        CaptureMode::Region => overlay::run_overlay(CaptureMode::Region),
                        CaptureMode::SelectWindow => overlay::run_overlay(CaptureMode::SelectWindow),
                    };
                    match result {
                        Some(img) => {
                            let _ = td.tx.send(Win32Event::CaptureResult(img));
                        }
                        None => {
                            let _ = td.tx.send(Win32Event::CaptureCancelled);
                        }
                    }
                    continue;
                }

                if msg.message == WM_HIDE_MAIN {
                    let _ = td.tx.send(Win32Event::HideWindow);
                    continue;
                }

                TranslateMessage(&msg);
                DispatchMessageA(&msg);
            }

            if let Ok(cmd) = td.rx.try_recv() {
                match cmd {
                    MainCmd::Quit => {
                        PostQuitMessage(0);
                    }
                    MainCmd::StartCapture(mode) => {
                        td.last_capture_mode = mode;
                        PostMessageA(hwnd, WM_START_CAPTURE, WPARAM(0), LPARAM(0));
                    }
                    MainCmd::ShowWindow => {
                        PostMessageA(hwnd, WM_SHOW_MAIN, WPARAM(0), LPARAM(0));
                    }
                    MainCmd::HideWindow => {
                        PostMessageA(hwnd, WM_HIDE_MAIN, WPARAM(0), LPARAM(0));
                    }
                }
            }

            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
