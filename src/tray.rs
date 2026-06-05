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
const IDM_CURSOR:         u32 = 110;
const IDM_AUTO_COPY:      u32 = 111;
const IDM_HIDE_ON_CAPTURE: u32 = 112;
const IDM_DELAY_0: u32 = 200;
const IDM_DELAY_1: u32 = 201;
const IDM_DELAY_2: u32 = 202;
const IDM_DELAY_3: u32 = 203;
const IDM_DELAY_5:      u32 = 205;
const IDM_DELAY_CUSTOM: u32 = 210;
const IDM_LANG_AUTO: u32 = 300;
const IDM_LANG_ZH:   u32 = 301;
const IDM_LANG_EN:   u32 = 302;
const IDM_THEME_AUTO:  u32 = 310;
const IDM_THEME_LIGHT: u32 = 311;
const IDM_THEME_DARK:  u32 = 312;
const IDM_QUIT:         u32 = 999;

/// 儲存在 GWLP_USERDATA 中的視窗資料
struct WndData {
    tx:          Sender<AppEvent>,
    config:      Arc<Mutex<Config>>,
    editor_hwnd: Arc<Mutex<Option<isize>>>,
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
            let tip = "ezshot";
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

pub fn create_message_window(
    tx: Sender<AppEvent>,
    config: Arc<Mutex<Config>>,
    editor_hwnd: Arc<Mutex<Option<isize>>>,
) -> HWND {
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

        let data = Box::new(WndData { tx, config, editor_hwnd });
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
            } else if event == WM_LBUTTONDBLCLK {
                let data = &*(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const WndData);
                let _ = data.tx.send(AppEvent::ShowEditor);
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
                IDM_AUTO_COPY => {
                    let mut c = data.config.lock().unwrap();
                    c.auto_copy = !c.auto_copy;
                    crate::config::persist_settings(&c);
                }
                IDM_HIDE_ON_CAPTURE => {
                    let mut c = data.config.lock().unwrap();
                    c.hide_editor_on_capture = !c.hide_editor_on_capture;
                    crate::config::persist_settings(&c);
                }
                IDM_LANG_AUTO => {
                    let mut c = data.config.lock().unwrap();
                    c.language = "auto".to_string();
                    crate::config::persist_settings(&c);
                    crate::i18n::init("auto");
                }
                IDM_LANG_ZH => {
                    let mut c = data.config.lock().unwrap();
                    c.language = "zh".to_string();
                    crate::config::persist_settings(&c);
                    crate::i18n::set(crate::i18n::Lang::Zh);
                }
                IDM_LANG_EN => {
                    let mut c = data.config.lock().unwrap();
                    c.language = "en".to_string();
                    crate::config::persist_settings(&c);
                    crate::i18n::set(crate::i18n::Lang::En);
                }
                IDM_THEME_AUTO => {
                    let mut c = data.config.lock().unwrap();
                    c.theme = "auto".to_string();
                    crate::config::persist_settings(&c);
                    crate::theme::init("auto");
                    notify_editor_theme_changed(&data.editor_hwnd);
                }
                IDM_THEME_LIGHT => {
                    let mut c = data.config.lock().unwrap();
                    c.theme = "light".to_string();
                    crate::config::persist_settings(&c);
                    crate::theme::set(crate::theme::Theme::Light);
                    notify_editor_theme_changed(&data.editor_hwnd);
                }
                IDM_THEME_DARK => {
                    let mut c = data.config.lock().unwrap();
                    c.theme = "dark".to_string();
                    crate::config::persist_settings(&c);
                    crate::theme::set(crate::theme::Theme::Dark);
                    notify_editor_theme_changed(&data.editor_hwnd);
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
    use crate::i18n::tw;
    let (capture_cursor, delay_secs, auto_copy, hide_on_capture, lang_setting, theme_setting) = {
        let data = &*(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const WndData);
        let c = data.config.lock().unwrap();
        (c.capture_cursor, c.capture_delay_secs, c.auto_copy, c.hide_editor_on_capture, c.language.clone(), c.theme.clone())
    };

    let hmenu = CreatePopupMenu().unwrap();

    // ── 擷取方式 ──
    let s_region = tw("框選區域 (Alt+Shift+R)", "Capture Region (Alt+Shift+R)");
    let s_active = tw("作用中視窗 (Alt+Shift+A)", "Active Window (Alt+Shift+A)");
    let s_pick   = tw("點選視窗 (Alt+Shift+W)", "Pick Window (Alt+Shift+W)");
    let _ = AppendMenuW(hmenu, MF_STRING, IDM_REGION as usize, windows::core::PCWSTR(s_region.as_ptr()));
    let _ = AppendMenuW(hmenu, MF_STRING, IDM_ACTIVE as usize, windows::core::PCWSTR(s_active.as_ptr()));
    let _ = AppendMenuW(hmenu, MF_STRING, IDM_PICK   as usize, windows::core::PCWSTR(s_pick.as_ptr()));
    let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, None);

    // ── 設定選項 ──
    let s_cursor = tw("擷取滑鼠游標", "Capture Mouse Cursor");
    let s_copy   = tw("直接複製到剪貼簿", "Auto Copy to Clipboard");
    let s_hide   = tw("擷取前隱藏編輯視窗", "Hide Editor Before Capture");
    let cursor_flag = if capture_cursor { MF_STRING | MF_CHECKED } else { MF_STRING };
    let _ = AppendMenuW(hmenu, cursor_flag, IDM_CURSOR as usize, windows::core::PCWSTR(s_cursor.as_ptr()));
    let auto_copy_flag = if auto_copy { MF_STRING | MF_CHECKED } else { MF_STRING };
    let _ = AppendMenuW(hmenu, auto_copy_flag, IDM_AUTO_COPY as usize, windows::core::PCWSTR(s_copy.as_ptr()));
    let hide_flag = if hide_on_capture { MF_STRING | MF_CHECKED } else { MF_STRING };
    let _ = AppendMenuW(hmenu, hide_flag, IDM_HIDE_ON_CAPTURE as usize, windows::core::PCWSTR(s_hide.as_ptr()));

    // ── 延遲子選單 ──
    let s_delay_title = tw("延遲擷取", "Capture Delay");
    let delay_menu = CreatePopupMenu().unwrap();
    let sd = [
        tw("無延遲", "No Delay"), tw("1 秒", "1 sec"), tw("2 秒", "2 sec"),
        tw("3 秒", "3 sec"),      tw("5 秒", "5 sec"),
    ];
    for (s, id) in sd.iter().zip([IDM_DELAY_0,IDM_DELAY_1,IDM_DELAY_2,IDM_DELAY_3,IDM_DELAY_5]) {
        let _ = AppendMenuW(delay_menu, MF_STRING, id as usize, windows::core::PCWSTR(s.as_ptr()));
    }
    let is_preset = [0u32, 1, 2, 3, 5].contains(&delay_secs);
    let custom_label: Vec<u16> = if is_preset {
        crate::i18n::t("自訂...", "Custom...").encode_utf16().chain(Some(0)).collect()
    } else {
        crate::i18n::t("自訂: {} 秒...", "Custom: {} sec...").replace("{}", &delay_secs.to_string()).encode_utf16().chain(Some(0)).collect()
    };
    let _ = AppendMenuW(delay_menu, MF_STRING, IDM_DELAY_CUSTOM as usize,
        windows::core::PCWSTR(custom_label.as_ptr()));
    let preset_ids = [(IDM_DELAY_0,0u32),(IDM_DELAY_1,1),(IDM_DELAY_2,2),(IDM_DELAY_3,3),(IDM_DELAY_5,5)];
    for (id, secs) in preset_ids {
        let _ = CheckMenuItem(delay_menu, id, MF_BYCOMMAND.0 | if is_preset && secs==delay_secs {MF_CHECKED.0} else {MF_UNCHECKED.0});
    }
    let _ = CheckMenuItem(delay_menu, IDM_DELAY_CUSTOM, MF_BYCOMMAND.0 | if is_preset {MF_UNCHECKED.0} else {MF_CHECKED.0});
    let _ = AppendMenuW(hmenu, MF_POPUP, delay_menu.0 as usize, windows::core::PCWSTR(s_delay_title.as_ptr()));
    // 記錄延遲子選單位置供後續套圖示用
    let delay_pos = unsafe { windows::Win32::UI::WindowsAndMessaging::GetMenuItemCount(hmenu) - 1 };

    // ── 語言子選單 ──
    let s_lang_title = tw("語言", "Language");
    let lang_menu = CreatePopupMenu().unwrap();
    let sl = [tw("自動", "Auto"), tw("中文", "中文"), tw("English", "English")];
    for (s, id) in sl.iter().zip([IDM_LANG_AUTO, IDM_LANG_ZH, IDM_LANG_EN]) {
        let _ = AppendMenuW(lang_menu, MF_STRING, id as usize, windows::core::PCWSTR(s.as_ptr()));
    }
    let cur_lang_id = match lang_setting.as_str() { "zh" => IDM_LANG_ZH, "en" => IDM_LANG_EN, _ => IDM_LANG_AUTO };
    let _ = CheckMenuRadioItem(lang_menu, IDM_LANG_AUTO, IDM_LANG_EN, cur_lang_id, MF_BYCOMMAND.0);
    let _ = AppendMenuW(hmenu, MF_POPUP, lang_menu.0 as usize, windows::core::PCWSTR(s_lang_title.as_ptr()));
    let lang_pos = unsafe { windows::Win32::UI::WindowsAndMessaging::GetMenuItemCount(hmenu) - 1 };

    // ── 主題子選單 ──
    let s_theme_title = tw("主題", "Theme");
    let theme_menu = CreatePopupMenu().unwrap();
    let st = [tw("自動","Auto"), tw("淺色","Light"), tw("深色","Dark")];
    for (s, id) in st.iter().zip([IDM_THEME_AUTO, IDM_THEME_LIGHT, IDM_THEME_DARK]) {
        let _ = AppendMenuW(theme_menu, MF_STRING, id as usize, windows::core::PCWSTR(s.as_ptr()));
    }
    let cur_theme_id = match theme_setting.as_str() { "light"=>IDM_THEME_LIGHT, "dark"=>IDM_THEME_DARK, _=>IDM_THEME_AUTO };
    let _ = CheckMenuRadioItem(theme_menu, IDM_THEME_AUTO, IDM_THEME_DARK, cur_theme_id, MF_BYCOMMAND.0);
    let _ = AppendMenuW(hmenu, MF_POPUP, theme_menu.0 as usize, windows::core::PCWSTR(s_theme_title.as_ptr()));
    let theme_pos = unsafe { windows::Win32::UI::WindowsAndMessaging::GetMenuItemCount(hmenu) - 1 };

    let s_quit = tw("結束", "Quit");
    let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, None);
    let _ = AppendMenuW(hmenu, MF_STRING, IDM_QUIT as usize, windows::core::PCWSTR(s_quit.as_ptr()));

    // ── 套用選單圖示 ──
    use crate::menu_icon as mi;
    let sys_dark_icons = crate::theme::system_is_dark();
    mi::set_icon_dark(sys_dark_icons);
    let ic_region  = mi::icon_region();
    let ic_active  = mi::icon_active_window();
    let ic_pick    = mi::icon_pick_window();
    // 切換型項目：tray 選單跟隨系統深色模式
    let ic_cursor  = mi::icon_cursor_toggle(capture_cursor, sys_dark_icons);
    let ic_copy    = mi::icon_clipboard_toggle(auto_copy, sys_dark_icons);
    let ic_hide    = mi::icon_hide_toggle(hide_on_capture, sys_dark_icons);
    let ic_delay   = mi::icon_clock();
    let ic_lang    = mi::icon_language();
    let ic_theme   = mi::icon_theme_auto();
    let ic_quit    = mi::icon_quit();
    mi::apply(hmenu, IDM_REGION,          &ic_region);
    mi::apply(hmenu, IDM_ACTIVE,          &ic_active);
    mi::apply(hmenu, IDM_PICK,            &ic_pick);
    mi::apply(hmenu, IDM_CURSOR,          &ic_cursor);
    mi::apply(hmenu, IDM_AUTO_COPY,       &ic_copy);
    mi::apply(hmenu, IDM_HIDE_ON_CAPTURE, &ic_hide);
    // POPUP 子選單父項：以位置索引套用（apply_at）
    mi::apply_at(hmenu, delay_pos as u32, &ic_delay);
    mi::apply_at(hmenu, lang_pos  as u32, &ic_lang);
    mi::apply_at(hmenu, theme_pos as u32, &ic_theme);
    mi::apply(hmenu, IDM_QUIT, &ic_quit);

    let mut pt = windows::Win32::Foundation::POINT::default();
    let _ = GetCursorPos(&mut pt);
    crate::theme::set_window_dark_menu(hwnd, crate::theme::system_is_dark());
    SetForegroundWindow(hwnd).ok();
    TrackPopupMenu(hmenu, TPM_RIGHTBUTTON, pt.x, pt.y, 0, hwnd, None);
    crate::theme::restore_after_menu();
    // icons 在 TrackPopupMenu 返回後、DestroyMenu 前先保留（避免 menu 讀取已釋放的 bitmap）
    let _ = DestroyMenu(hmenu);
    let _ = (ic_region, ic_active, ic_pick, ic_cursor, ic_copy, ic_hide, ic_delay, ic_lang, ic_theme, ic_quit);
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

fn notify_editor_theme_changed(editor_hwnd: &Arc<Mutex<Option<isize>>>) {
    if let Some(raw) = *editor_hwnd.lock().unwrap() {
        unsafe {
            PostMessageW(
                HWND(raw as *mut _),
                crate::editor::WM_THEME_CHANGED,
                WPARAM(0), LPARAM(0),
            ).ok();
        }
    }
}
