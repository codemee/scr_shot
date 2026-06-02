use std::sync::{Arc, Mutex};
use std::sync::mpsc::Sender;
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE,
    NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::config::Config;
use crate::event::AppEvent;

pub const WM_TRAY: u32 = WM_APP + 1;

const IDM_REGION:  u32 = 100;
const IDM_ACTIVE:  u32 = 101;
const IDM_PICK:    u32 = 102;
const IDM_CURSOR:  u32 = 110;
const IDM_DELAY_0: u32 = 200;
const IDM_DELAY_1: u32 = 201;
const IDM_DELAY_2: u32 = 202;
const IDM_DELAY_3: u32 = 203;
const IDM_DELAY_5:      u32 = 205;
const IDM_DELAY_CUSTOM: u32 = 210;
const IDM_QUIT:         u32 = 999;

/// 儲存在 GWLP_USERDATA 中的視窗資料
struct WndData {
    tx:     Sender<AppEvent>,
    config: Arc<Mutex<Config>>,
}

pub struct Tray {
    hwnd: HWND,
    icon: HICON,
}

impl Tray {
    pub fn add(&self) {
        unsafe {
            let mut nid = nid_base(self.hwnd);
            nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
            nid.uCallbackMessage = WM_TRAY;
            nid.hIcon = self.icon;
            let tip = "srcshot";
            let bytes: Vec<u16> = tip.encode_utf16().chain(std::iter::once(0)).collect();
            let len = bytes.len().min(128);
            nid.szTip[..len].copy_from_slice(&bytes[..len]);
            Shell_NotifyIconW(NIM_ADD, &nid);
        }
    }

    pub fn remove(&self) {
        unsafe {
            let nid = nid_base(self.hwnd);
            Shell_NotifyIconW(NIM_DELETE, &nid);
        }
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        self.remove();
        unsafe { DestroyIcon(self.icon).ok(); }
    }
}

pub fn create_message_window(tx: Sender<AppEvent>, config: Arc<Mutex<Config>>) -> HWND {
    unsafe {
        let class = w!("srcshot_msg");
        let hinstance = get_instance();

        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(msg_wnd_proc),
            hInstance: hinstance,
            lpszClassName: class,
            ..Default::default()
        };
        RegisterClassExW(&wc);

        let data = Box::new(WndData { tx, config });
        let hwnd = CreateWindowExW(
            Default::default(),
            class,
            w!("srcshot"),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT, CW_USEDEFAULT, CW_USEDEFAULT, CW_USEDEFAULT,
            HWND_MESSAGE,
            HMENU(std::ptr::null_mut()),
            hinstance,
            Some(Box::into_raw(data) as _),
        )
        .expect("CreateWindowExW failed");

        hwnd
    }
}

pub fn make_tray(hwnd: HWND) -> Tray {
    let icon = unsafe { crate::icon::create_app_icon() };
    let t = Tray { hwnd, icon };
    t.add();
    t
}

unsafe extern "system" fn msg_wnd_proc(
    hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM,
) -> LRESULT {
    match msg {
        WM_NCCREATE => {
            let cs = &*(lp.0 as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as _);
            LRESULT(1)
        }
        WM_TRAY => {
            let event = lp.0 as u32 & 0xFFFF;
            if event == WM_RBUTTONUP || event == WM_CONTEXTMENU {
                show_context_menu(hwnd);
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            let data = &*(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const WndData);
            match wp.0 as u32 {
                IDM_REGION => { let _ = data.tx.send(AppEvent::CaptureRegion); }
                IDM_ACTIVE => { let _ = data.tx.send(AppEvent::CaptureActiveWindow); }
                IDM_PICK   => { let _ = data.tx.send(AppEvent::CapturePickWindow); }
                IDM_CURSOR => {
                    let mut c = data.config.lock().unwrap();
                    c.capture_cursor = !c.capture_cursor;
                    crate::config::persist_settings(&c);
                }
                IDM_DELAY_0 | IDM_DELAY_1 | IDM_DELAY_2 | IDM_DELAY_3 | IDM_DELAY_5 => {
                    let mut c = data.config.lock().unwrap();
                    c.capture_delay_secs = match wp.0 as u32 {
                        IDM_DELAY_1 => 1,
                        IDM_DELAY_2 => 2,
                        IDM_DELAY_3 => 3,
                        IDM_DELAY_5 => 5,
                        _           => 0,
                    };
                    crate::config::persist_settings(&c);
                }
                IDM_DELAY_CUSTOM => {
                    // 先釋放鎖，再顯示對話框
                    let config_ref = data.config.clone();
                    show_custom_delay_dialog(hwnd, &config_ref);
                }
                IDM_QUIT => {
                    let _ = data.tx.send(AppEvent::TrayQuit);
                    PostQuitMessage(0);
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_HOTKEY => {
            let data = &*(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const WndData);
            crate::hotkey::handle_wm_hotkey(wp.0 as i32, &data.tx);
            LRESULT(0)
        }
        WM_DESTROY => {
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WndData;
            if !ptr.is_null() {
                drop(Box::from_raw(ptr));
            }
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

unsafe fn show_context_menu(hwnd: HWND) {
    let (capture_cursor, delay_secs) = {
        let data = &*(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const WndData);
        let c = data.config.lock().unwrap();
        (c.capture_cursor, c.capture_delay_secs)
    };

    let hmenu = CreatePopupMenu().unwrap();

    // ── 擷取方式 ──
    let _ = AppendMenuW(hmenu, MF_STRING, IDM_REGION as usize, w!("框選區域 (Alt+Shift+R)"));
    let _ = AppendMenuW(hmenu, MF_STRING, IDM_ACTIVE as usize, w!("作用中視窗 (Alt+Shift+A)"));
    let _ = AppendMenuW(hmenu, MF_STRING, IDM_PICK   as usize, w!("點選視窗 (Alt+Shift+W)"));
    let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, None);

    // ── 擷取游標選項 ──
    let cursor_flag = if capture_cursor { MF_STRING | MF_CHECKED } else { MF_STRING };
    let _ = AppendMenuW(hmenu, cursor_flag, IDM_CURSOR as usize, w!("擷取滑鼠游標"));

    // ── 延遲子選單 ──
    let delay_menu = CreatePopupMenu().unwrap();
    let _ = AppendMenuW(delay_menu, MF_STRING, IDM_DELAY_0 as usize, w!("無延遲"));
    let _ = AppendMenuW(delay_menu, MF_STRING, IDM_DELAY_1 as usize, w!("1 秒"));
    let _ = AppendMenuW(delay_menu, MF_STRING, IDM_DELAY_2 as usize, w!("2 秒"));
    let _ = AppendMenuW(delay_menu, MF_STRING, IDM_DELAY_3 as usize, w!("3 秒"));
    let _ = AppendMenuW(delay_menu, MF_STRING, IDM_DELAY_5 as usize, w!("5 秒"));

    // 自訂項目：若目前是自訂值，顯示「自訂: N 秒...」
    let is_preset = [0u32, 1, 2, 3, 5].contains(&delay_secs);
    let custom_label: Vec<u16> = if is_preset {
        "自訂...\0".encode_utf16().collect()
    } else {
        format!("自訂: {} 秒...\0", delay_secs).encode_utf16().collect()
    };
    let _ = AppendMenuW(delay_menu, MF_STRING, IDM_DELAY_CUSTOM as usize,
        windows::core::PCWSTR(custom_label.as_ptr()));

    // 標記目前選取項（checkmark）
    let preset_ids = [(IDM_DELAY_0, 0u32), (IDM_DELAY_1, 1), (IDM_DELAY_2, 2),
                      (IDM_DELAY_3, 3), (IDM_DELAY_5, 5)];
    for (id, secs) in preset_ids {
        let flag = if is_preset && secs == delay_secs { MF_CHECKED.0 } else { MF_UNCHECKED.0 };
        let _ = CheckMenuItem(delay_menu, id, MF_BYCOMMAND.0 | flag);
    }
    let custom_flag = if is_preset { MF_UNCHECKED.0 } else { MF_CHECKED.0 };
    let _ = CheckMenuItem(delay_menu, IDM_DELAY_CUSTOM, MF_BYCOMMAND.0 | custom_flag);

    let _ = AppendMenuW(hmenu, MF_POPUP, delay_menu.0 as usize, w!("延遲擷取"));

    let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, None);
    let _ = AppendMenuW(hmenu, MF_STRING, IDM_QUIT as usize, w!("結束"));

    let mut pt = windows::Win32::Foundation::POINT::default();
    let _ = GetCursorPos(&mut pt);
    SetForegroundWindow(hwnd).ok();
    TrackPopupMenu(hmenu, TPM_RIGHTBUTTON, pt.x, pt.y, 0, hwnd, None);
    let _ = DestroyMenu(hmenu); // 連同子選單一起釋放
}

/// 顯示自訂延遲秒數對話框，確定後更新設定
unsafe fn show_custom_delay_dialog(owner: HWND, config: &Arc<Mutex<Config>>) {
    let class     = w!("srcshot_delay_dlg");
    let hinstance = get_instance();

    struct DlgState { value: u32, confirmed: bool, done: bool }

    unsafe extern "system" fn dlg_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        match msg {
            WM_NCCREATE => {
                let cs = &*(lp.0 as *const CREATESTRUCTW);
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as _);
                LRESULT(1)
            }
            WM_CREATE => {
                let hi = get_instance();
                // 說明文字
                CreateWindowExW(Default::default(), w!("STATIC"),
                    w!("延遲秒數（0 – 99）："),
                    WS_CHILD | WS_VISIBLE,
                    8, 10, 180, 18, hwnd, HMENU(std::ptr::null_mut()), hi, None).ok();
                // 數字輸入框（ES_NUMBER = 0x2000）
                let edit = CreateWindowExW(WS_EX_CLIENTEDGE, w!("EDIT"), w!(""),
                    WS_CHILD | WS_VISIBLE | WINDOW_STYLE(0x2000u32),
                    8, 32, 80, 24, hwnd, HMENU(1usize as _), hi, None).unwrap();
                // 確定
                CreateWindowExW(Default::default(), w!("BUTTON"), w!("確定"),
                    WS_CHILD | WS_VISIBLE | WINDOW_STYLE(BS_DEFPUSHBUTTON as u32),
                    8, 64, 80, 28, hwnd, HMENU(2usize as _), hi, None).ok();
                // 取消
                CreateWindowExW(Default::default(), w!("BUTTON"), w!("取消"),
                    WS_CHILD | WS_VISIBLE,
                    96, 64, 80, 28, hwnd, HMENU(3usize as _), hi, None).ok();
                // 預填目前值
                let state = &*(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const DlgState);
                let pre: Vec<u16> = format!("{}\0", state.value).encode_utf16().collect();
                SetWindowTextW(edit, windows::core::PCWSTR(pre.as_ptr())).ok();
                PostMessageW(hwnd, WM_NEXTDLGCTL, WPARAM(edit.0 as usize), LPARAM(1)).ok();
                LRESULT(0)
            }
            WM_COMMAND if (wp.0 & 0xFFFF) == 2 => { // 確定
                let state = &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut DlgState);
                if let Ok(edit) = GetDlgItem(hwnd, 1) {
                    let len = GetWindowTextLengthW(edit) + 1;
                    let mut buf = vec![0u16; len as usize];
                    GetWindowTextW(edit, &mut buf);
                    let s = String::from_utf16_lossy(&buf).trim_end_matches('\0').to_string();
                    state.value = s.parse::<u32>().unwrap_or(0).min(99);
                }
                state.confirmed = true;
                state.done = true;
                DestroyWindow(hwnd).ok();
                LRESULT(0)
            }
            WM_COMMAND if (wp.0 & 0xFFFF) == 3 => { // 取消
                let state = &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut DlgState);
                state.done = true;
                DestroyWindow(hwnd).ok();
                LRESULT(0)
            }
            WM_CLOSE => {
                let state = &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut DlgState);
                state.done = true;
                DestroyWindow(hwnd).ok();
                LRESULT(0)
            }
            WM_DESTROY => LRESULT(0),
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }

    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(dlg_proc),
        hInstance: hinstance,
        lpszClassName: class,
        ..Default::default()
    };
    let _ = RegisterClassExW(&wc);

    let current = config.lock().unwrap().capture_delay_secs;
    let mut state = DlgState { value: current, confirmed: false, done: false };

    let dlg = CreateWindowExW(
        WS_EX_DLGMODALFRAME | WS_EX_TOPMOST,
        class,
        w!("自訂延遲秒數"),
        WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_VISIBLE,
        CW_USEDEFAULT, CW_USEDEFAULT, 200, 140,
        owner,
        HMENU(std::ptr::null_mut()),
        hinstance,
        Some(&mut state as *mut _ as _),
    )
    .unwrap();

    let mut msg = MSG::default();
    while GetMessageW(&mut msg, HWND(std::ptr::null_mut()), 0, 0).as_bool() {
        // Enter 直接確定
        if msg.message == WM_KEYDOWN && msg.wParam.0 == 0x0D {
            if let Ok(edit) = GetDlgItem(dlg, 1) {
                let len = GetWindowTextLengthW(edit) + 1;
                let mut buf = vec![0u16; len as usize];
                GetWindowTextW(edit, &mut buf);
                let s = String::from_utf16_lossy(&buf).trim_end_matches('\0').to_string();
                state.value = s.parse::<u32>().unwrap_or(0).min(99);
            }
            state.confirmed = true;
            state.done = true;
            DestroyWindow(dlg).ok();
            break;
        }
        if IsDialogMessageW(dlg, &msg).as_bool() {
            if state.done { break; }
            continue;
        }
        let _ = TranslateMessage(&msg);
        DispatchMessageW(&msg);
        if state.done { break; }
    }

    let _ = UnregisterClassW(class, hinstance);

    if state.confirmed {
        let mut c = config.lock().unwrap();
        c.capture_delay_secs = state.value;
        crate::config::persist_settings(&c);
    }
}

fn nid_base(hwnd: HWND) -> NOTIFYICONDATAW {
    let mut nid = NOTIFYICONDATAW::default();
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = 1;
    nid
}

fn get_instance() -> windows::Win32::Foundation::HINSTANCE {
    unsafe {
        windows::Win32::System::LibraryLoader::GetModuleHandleW(None)
            .unwrap()
            .into()
    }
}
