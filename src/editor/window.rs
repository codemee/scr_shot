use std::sync::mpsc::Sender;
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Foundation::COLORREF;
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC,
    CreateFontW, CreatePen, CreateRoundRectRgn, CreateSolidBrush, DeleteDC, DeleteObject,
    DrawTextW, EndPaint, FillRect, FillRgn, FrameRgn, GetDC, GetStockObject,
    IntersectClipRect, InvalidateRect,
    Arc as GdiArc, Ellipse, LineTo, MoveToEx, NULL_BRUSH, Polygon, Polyline,
    Rectangle as GdiRectangle, ReleaseDC, RestoreDC, RoundRect, SaveDC,
    SelectObject, SetBkMode,
    BACKGROUND_MODE, DEFAULT_GUI_FONT, DRAW_TEXT_FORMAT, HRGN, PAINTSTRUCT,
    PS_SOLID, SRCCOPY,
};
use windows::Win32::UI::Controls::{DRAWITEMSTRUCT, SetScrollInfo};
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, SetFocus, VK_ESCAPE, VK_RETURN};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::capture::screen::ScreenBitmap;
use crate::event::AppEvent;
use super::canvas::Canvas;
use super::tool::{Stroke, Tool};

// 活頁訊息（WM_APP = 0x8000）
pub const WM_NEW_TAB:     u32 = 0x8002; // app → editor：新增分頁（lParam = Box<ScreenBitmap>）
pub const WM_FORCE_QUIT:  u32 = 0x8003; // app → editor：強制結束執行緒
pub const WM_SHOW_EDITOR: u32 = 0x8004; // app → editor：顯示並帶到前景

const TAB_H:    i32 = 28;              // 分頁列高度
const CANVAS_Y: i32 = TAB_H + TOOLBAR_H; // 畫布起始 y（= 76）
const TAB_W:    i32 = 148;             // 每個分頁的寬度（日期時間戳記 Consolas 需要更多空間）
const TOOLBAR_H: i32 = 48;
const TOOLBAR_BG: u32 = 0x00_F0_F0_F0; // 工具列背景色（沉浸式風格）
const BTN_W: i32 = 40;
const BTN_H: i32 = 36;
const BTN_MARGIN: i32 = 4;

// 右鍵選單指令（3000 起，不與分頁切換 2000+ 衝突）
const CM_CAPTURE_REGION:  u32 = 3001;
const CM_CAPTURE_ACTIVE:  u32 = 3002;
const CM_CAPTURE_PICK:    u32 = 3003;
const CM_TOGGLE_CURSOR:   u32 = 3010;
const CM_TOGGLE_AUTOCOPY: u32 = 3011;
const CM_DELAY_0:  u32 = 3200; const CM_DELAY_1: u32 = 3201;
const CM_DELAY_2:  u32 = 3202; const CM_DELAY_3: u32 = 3203;
const CM_DELAY_5:  u32 = 3205; const CM_DELAY_CUSTOM: u32 = 3210;
const CM_TOGGLE_HIDE_ON_CAPTURE: u32 = 3012;
const CM_QUIT:     u32 = 3099;

const BTN_SETTINGS: usize = 25; // 設定按鈕（≡）

const BTN_PEN: usize   = 10;
const BTN_ARROW: usize = 11;
const BTN_RECT: usize  = 12;
const BTN_TEXT: usize  = 13;
const BTN_CROP:  usize = 14;
const BTN_COLOR: usize = 15;
const BTN_COPY:    usize = 20;
const BTN_SAVE:    usize = 21;
const BTN_SAVEAS:  usize = 23;
const BTN_UNDO:    usize = 22;

/// 每個分頁各自擁有的狀態
struct TabInfo {
    canvas: Canvas,
    save_dir: std::path::PathBuf,
    scroll_x: i32,
    scroll_y: i32,
    result_sent: bool,
    name: String,
    saved_path: Option<std::path::PathBuf>, // 已存檔的完整路徑；再次存檔時直接覆蓋
    modified: bool, // 有未存的修改（新截圖 = false，使用者編輯後 = true）
}

/// 整個編輯器視窗的狀態
struct EditorState {
    tx: Sender<AppEvent>,
    tabs: Vec<TabInfo>,
    active_tab: usize,
    tab_counter: u32,
    active_tool: Tool,
    dragging: bool,
    drag_start: POINT,
    default_save_dir: std::path::PathBuf,
    hovering_canvas: bool,
    tooltip: HWND,
    hover_btn: i32,
    hover_ticks: i32,
    tab_scroll: usize,
    editor_hwnd_arc: std::sync::Arc<std::sync::Mutex<Option<isize>>>,
    config: std::sync::Arc<std::sync::Mutex<crate::config::Config>>,
}

pub fn open(
    bmp: ScreenBitmap,
    tx: Sender<AppEvent>,
    save_dir: std::path::PathBuf,
    editor_hwnd_arc: std::sync::Arc<std::sync::Mutex<Option<isize>>>,
    config: std::sync::Arc<std::sync::Mutex<crate::config::Config>>,
) {
    unsafe {
        use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

        let class = w!("srcshot_editor");
        let hinstance = get_instance();

        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(editor_wnd_proc),
            hInstance: hinstance,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap(),
            hbrBackground: windows::Win32::Graphics::Gdi::HBRUSH(
                windows::Win32::Graphics::Gdi::GetStockObject(
                    windows::Win32::Graphics::Gdi::WHITE_BRUSH,
                ).0,
            ),
            lpszClassName: class,
            ..Default::default()
        };
        RegisterClassExW(&wc);

        let canvas = Canvas::new(bmp);
        let screen_w = GetSystemMetrics(SM_CXSCREEN);
        let screen_h = GetSystemMetrics(SM_CYSCREEN);
        let min_w = BTN_MARGIN + 11 * (BTN_W + BTN_MARGIN) + 20;
        let min_h = CANVAS_Y + 120;
        let win_w = (canvas.width + 20).max(min_w).min(screen_w * 9 / 10);
        let win_h = (canvas.height + CANVAS_Y + 45).max(min_h).min(screen_h * 9 / 10);

        let first_tab = TabInfo {
            canvas,
            save_dir: save_dir.clone(),
            scroll_x: 0,
            scroll_y: 0,
            result_sent: false,
            name: {
                let st = windows::Win32::System::SystemInformation::GetLocalTime();
                format!("{}{:02}{:02}{:02}{:02}{:02}", st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond)
            },
            saved_path: None,
            modified: false,
        };
        let state = Box::new(EditorState {
            tx,
            tabs: vec![first_tab],
            active_tab: 0,
            tab_counter: 1,
            active_tool: Tool::Pen,
            dragging: false,
            drag_start: POINT { x: 0, y: 0 },
            default_save_dir: save_dir,
            hovering_canvas: false,
            tooltip: HWND(std::ptr::null_mut()),
            hover_btn: -1,
            hover_ticks: 0,
            tab_scroll: 0,
            editor_hwnd_arc: editor_hwnd_arc.clone(),
            config: config.clone(),
        });

        let hwnd = CreateWindowExW(
            WS_EX_APPWINDOW,
            class,
            w!("srcshot 編輯器"),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            CW_USEDEFAULT, CW_USEDEFAULT, win_w, win_h,
            HWND(std::ptr::null_mut()),
            HMENU(std::ptr::null_mut()),
            hinstance,
            Some(Box::into_raw(state) as _),
        )
        .unwrap();

        // 設定與系統匣相同的視窗圖示
        let app_icon = crate::icon::create_app_icon();
        SendMessageW(hwnd, WM_SETICON, WPARAM(1), LPARAM(app_icon.0 as isize)); // ICON_BIG
        SendMessageW(hwnd, WM_SETICON, WPARAM(0), LPARAM(app_icon.0 as isize)); // ICON_SMALL

        // 向 app.rs 登記 HWND，讓後續截圖可直接送分頁訊息
        *editor_hwnd_arc.lock().unwrap() = Some(hwnd.0 as isize);

        create_toolbar(hwnd);
        // 設定初始視窗標題
        let sp0 = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const EditorState;
        if !sp0.is_null() { update_window_title(hwnd, &*sp0); }

        // 自製 tooltip 視窗（比 Win32 tooltip API 可靠）
        let tip_class = w!("srcshot_tipwnd");
        let tip_wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(tip_wnd_proc),
            hInstance: hinstance,
            lpszClassName: tip_class,
            ..Default::default()
        };
        let _ = RegisterClassExW(&tip_wc);
        let tooltip = CreateWindowExW(
            // WS_EX_LAYERED：DWM 合成不觸發下方 WM_PAINT
            // WS_EX_NOACTIVATE：不搶走焦點，編輯視窗陰影不消失
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED | WS_EX_NOACTIVATE,
            tip_class, w!(""),
            WS_POPUP | WS_BORDER,
            0, 0, 70, 22,
            hwnd, HMENU(std::ptr::null_mut()), hinstance, None,
        ).unwrap_or(HWND(std::ptr::null_mut()));
        if !tooltip.0.is_null() {
            // 設定完全不透明；WS_EX_LAYERED 需要此呼叫才會正常顯示
            SetLayeredWindowAttributes(tooltip, COLORREF(0), 255, LWA_ALPHA).ok();
        }
        let sp = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut EditorState;
        if !sp.is_null() { (*sp).tooltip = tooltip; }
        // 每 100ms 輪詢一次游標位置，用於 tooltip 偵測（子視窗擋住父視窗的 WM_MOUSEMOVE）
        SetTimer(hwnd, 3, 100, None);

        // SetForegroundWindow 在跨執行緒且距 WM_HOTKEY 較久時可能因前景鎖失敗。
        // 先暫設 TOPMOST 強制置頂，之後恢復 NOTOPMOST，讓視窗可靠地出現在前景。
        SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW).ok();
        SetForegroundWindow(hwnd).ok();
        SetWindowPos(hwnd, HWND_NOTOPMOST, 0, 0, 0, 0,
            SWP_NOMOVE | SWP_NOSIZE).ok();

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, HWND(std::ptr::null_mut()), 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        DestroyIcon(app_icon).ok();
        let _ = UnregisterClassW(class, hinstance);
        let _ = UnregisterClassW(w!("srcshot_tipwnd"), hinstance);
        CoUninitialize();
    }
}

unsafe fn create_toolbar(parent: HWND) {
    let hinstance = get_instance();
    for (i, id) in [BTN_PEN, BTN_ARROW, BTN_RECT, BTN_TEXT, BTN_CROP, BTN_COLOR, BTN_COPY, BTN_SAVE, BTN_SAVEAS, BTN_UNDO, BTN_SETTINGS]
        .iter().enumerate()
    {
        let x = BTN_MARGIN + i as i32 * (BTN_W + BTN_MARGIN);
        CreateWindowExW(
            Default::default(), w!("BUTTON"), w!(""),
            WS_CHILD | WS_VISIBLE | WINDOW_STYLE(0x0000000Bu32), // BS_OWNERDRAW
            x, 6, BTN_W, BTN_H,
            parent, HMENU(*id as *mut _), hinstance, None,
        ).unwrap();
    }
}

unsafe fn update_scrollbars(hwnd: HWND, state: &EditorState) {
    let mut rc = RECT::default();
    GetClientRect(hwnd, &mut rc).unwrap();

    // 根據 canvas vs client 決定是否需要捲軸
    let need_h = state.tabs[state.active_tab].canvas.width  > rc.right;
    let need_v = state.tabs[state.active_tab].canvas.height > (rc.bottom - CANVAS_Y);

    // 透過切換 WS_HSCROLL / WS_VSCROLL 樣式來顯示或隱藏捲軸
    let style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
    let mut new_style = style;
    if need_h { new_style |= WS_HSCROLL.0; } else { new_style &= !WS_HSCROLL.0; }
    if need_v { new_style |= WS_VSCROLL.0; } else { new_style &= !WS_VSCROLL.0; }

    if new_style != style {
        SetWindowLongW(hwnd, GWL_STYLE, new_style as i32);
        SetWindowPos(hwnd, None, 0, 0, 0, 0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED).ok();
        // 套用後 client rect 可能改變，重新讀取
        GetClientRect(hwnd, &mut rc).unwrap();
    }

    if need_h {
        let client_w = rc.right.max(1);
        let si = SCROLLINFO {
            cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
            fMask: SIF_ALL,
            nMin: 0,
            nMax: state.tabs[state.active_tab].canvas.width - 1,
            nPage: client_w as u32,
            nPos: state.tabs[state.active_tab].scroll_x,
            nTrackPos: 0,
        };
        SetScrollInfo(hwnd, SB_HORZ, &si, true);
    }

    if need_v {
        let client_h = (rc.bottom - CANVAS_Y).max(1);
        let si = SCROLLINFO {
            cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
            fMask: SIF_ALL,
            nMin: 0,
            nMax: state.tabs[state.active_tab].canvas.height - 1,
            nPage: client_h as u32,
            nPos: state.tabs[state.active_tab].scroll_y,
            nTrackPos: 0,
        };
        SetScrollInfo(hwnd, SB_VERT, &si, true);
    }
}

fn clamp_scroll(val: i32, canvas_size: i32, client_size: i32) -> i32 {
    val.clamp(0, (canvas_size - client_size).max(0))
}

unsafe extern "system" fn editor_wnd_proc(
    hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM,
) -> LRESULT {
    match msg {
        WM_NCCREATE => {
            let cs = &*(lp.0 as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as _);
            LRESULT(1)
        }
        WM_SIZE => {
            let state = &*(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut EditorState);
            update_scrollbars(hwnd, state);
            InvalidateRect(hwnd, None, false);
            LRESULT(0)
        }
        WM_HSCROLL => {
            let state = &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut EditorState);
            let mut rc = RECT::default();
            GetClientRect(hwnd, &mut rc).unwrap();
            let client_w = rc.right;
            let code = (wp.0 & 0xFFFF) as u32;
            let mut si = SCROLLINFO {
                cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
                fMask: SIF_ALL,
                ..Default::default()
            };
            GetScrollInfo(hwnd, SB_HORZ, &mut si);
            let new_x = match code {
                0 => state.tabs[state.active_tab].scroll_x - 20,        // SB_LINELEFT
                1 => state.tabs[state.active_tab].scroll_x + 20,        // SB_LINERIGHT
                2 => state.tabs[state.active_tab].scroll_x - client_w,  // SB_PAGELEFT
                3 => state.tabs[state.active_tab].scroll_x + client_w,  // SB_PAGERIGHT
                5 => si.nTrackPos,               // SB_THUMBTRACK
                _ => state.tabs[state.active_tab].scroll_x,
            };
            state.tabs[state.active_tab].scroll_x = clamp_scroll(new_x, state.tabs[state.active_tab].canvas.width, client_w);
            update_scrollbars(hwnd, state);
            InvalidateRect(hwnd, Some(&RECT{left:0,top:CANVAS_Y,right:32767,bottom:32767}), false);
            LRESULT(0)
        }
        WM_VSCROLL => {
            let state = &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut EditorState);
            let mut rc = RECT::default();
            GetClientRect(hwnd, &mut rc).unwrap();
            let client_h = rc.bottom - CANVAS_Y;
            let code = (wp.0 & 0xFFFF) as u32;
            let mut si = SCROLLINFO {
                cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
                fMask: SIF_ALL,
                ..Default::default()
            };
            GetScrollInfo(hwnd, SB_VERT, &mut si);
            let new_y = match code {
                0 => state.tabs[state.active_tab].scroll_y - 20,        // SB_LINEUP
                1 => state.tabs[state.active_tab].scroll_y + 20,        // SB_LINEDOWN
                2 => state.tabs[state.active_tab].scroll_y - client_h,  // SB_PAGEUP
                3 => state.tabs[state.active_tab].scroll_y + client_h,  // SB_PAGEDOWN
                5 => si.nTrackPos,               // SB_THUMBTRACK
                _ => state.tabs[state.active_tab].scroll_y,
            };
            state.tabs[state.active_tab].scroll_y = clamp_scroll(new_y, state.tabs[state.active_tab].canvas.height, client_h);
            update_scrollbars(hwnd, state);
            InvalidateRect(hwnd, Some(&RECT{left:0,top:CANVAS_Y,right:32767,bottom:32767}), false);
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            let state = &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut EditorState);
            let delta = ((wp.0 >> 16) as u16) as i16;
            let mut rc = RECT::default();
            GetClientRect(hwnd, &mut rc).unwrap();
            let client_h = rc.bottom - CANVAS_Y;
            let step = 60;
            let new_y = if delta > 0 {
                state.tabs[state.active_tab].scroll_y - step
            } else {
                state.tabs[state.active_tab].scroll_y + step
            };
            state.tabs[state.active_tab].scroll_y = clamp_scroll(new_y, state.tabs[state.active_tab].canvas.height, client_h);
            update_scrollbars(hwnd, state);
            InvalidateRect(hwnd, Some(&RECT{left:0,top:CANVAS_Y,right:32767,bottom:32767}), false);
            LRESULT(0)
        }
        WM_COMMAND => {
            let id = (wp.0 & 0xFFFF) as usize;
            let state = &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut EditorState);
            match id {
                BTN_PEN | BTN_ARROW | BTN_RECT | BTN_TEXT | BTN_CROP => {
                    state.active_tool = match id {
                        BTN_PEN   => Tool::Pen,
                        BTN_ARROW => Tool::Arrow,
                        BTN_RECT  => Tool::Rect,
                        BTN_TEXT  => Tool::Text,
                        _         => Tool::Crop,
                    };
                    for bid in [BTN_PEN, BTN_ARROW, BTN_RECT, BTN_TEXT, BTN_CROP] {
                        if let Ok(btn) = GetDlgItem(hwnd, bid as i32) {
                            InvalidateRect(btn, None, false);
                        }
                    }
                    SetFocus(hwnd);
                }
                BTN_COLOR => {
                    let chosen = simple_color_dialog(hwnd);
                    let final_color = if chosen == Some(0xFF_00_00_00) {
                        // 0xFF000000 = 哨兵值：使用者按了「自訂…」
                        custom_color_input_dialog(hwnd, state.tabs[state.active_tab].canvas.tool_color)
                    } else {
                        chosen
                    };
                    if let Some(color) = final_color {
                        state.tabs[state.active_tab].canvas.tool_color = color;
                        if let Ok(btn) = GetDlgItem(hwnd, BTN_COLOR as i32) {
                            InvalidateRect(btn, None, false);
                        }
                    }
                    SetFocus(hwnd);
                }
                BTN_UNDO  => {
                    state.tabs[state.active_tab].canvas.undo();
                    if !state.tabs[state.active_tab].modified {
                        state.tabs[state.active_tab].modified = true;
                    }
                    // 裁切復原後畫布可能變大，捲軸需重算
                    state.tabs[state.active_tab].scroll_x = state.tabs[state.active_tab].scroll_x.min((state.tabs[state.active_tab].canvas.width  - 1).max(0));
                    state.tabs[state.active_tab].scroll_y = state.tabs[state.active_tab].scroll_y.min((state.tabs[state.active_tab].canvas.height - 1).max(0));
                    update_scrollbars(hwnd, state);
                    InvalidateRect(hwnd, None, false);
                    SetFocus(hwnd);
                }
                BTN_COPY => {
                    // 複製後保留分頁（不關閉）
                    let flat = state.tabs[state.active_tab].canvas.flatten_to_bitmap();
                    let _ = crate::output::clipboard::copy_to_clipboard(&flat);
                    let _ = state.tx.send(AppEvent::EditorSave { to_clipboard: true, path: None });
                    SetFocus(hwnd);
                }
                BTN_SAVE => {
                    let flat = state.tabs[state.active_tab].canvas.flatten_to_bitmap();
                    // 已有儲存路徑 → 直接覆蓋；否則開對話框
                    let existing = state.tabs[state.active_tab].saved_path.clone();
                    let chosen = if let Some(ref p) = existing {
                        Some(p.clone())
                    } else {
                        // 首次存檔：以時間戳記（標籤名稱）為預設檔名
                        let tab_name = state.tabs[state.active_tab].name.clone();
                        show_save_dialog(hwnd, &state.tabs[state.active_tab].save_dir, &tab_name)
                    };
                    if let Some(path) = chosen {
                        let _ = crate::output::file::save_png(&flat, &path);
                        // 更新目錄記錄
                        if let Some(parent) = path.parent() {
                            let dir = parent.to_path_buf();
                            state.tabs[state.active_tab].save_dir = dir.clone();
                            state.default_save_dir = dir.clone();
                            crate::config::persist_save_dir(&dir);
                        }
                        // 標籤名稱改成檔名（不含副檔名）
                        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                            state.tabs[state.active_tab].name = stem.to_string();
                        }
                        // 記錄完整路徑供下次覆蓋
                        state.tabs[state.active_tab].saved_path = Some(path.clone());
                        state.tabs[state.active_tab].modified = false;
                        update_window_title(hwnd, state);
                        let _ = state.tx.send(AppEvent::EditorSave { to_clipboard: false, path: Some(path) });
                        state.tabs[state.active_tab].result_sent = true;
                        // 不關閉分頁，讓使用者繼續編輯
                        update_scrollbars(hwnd, state);
                        InvalidateRect(hwnd, None, false);
                        SetFocus(hwnd);
                    }
                    // 使用者取消對話框時不做任何事
                }
                BTN_SAVEAS => {
                    // 另存新檔：僅對已存過的標籤有效
                    if state.tabs[state.active_tab].saved_path.is_none() { return LRESULT(0); }
                    let flat = state.tabs[state.active_tab].canvas.flatten_to_bitmap();
                    // 另存新檔：以現有檔名為預設
                    let current_stem = state.tabs[state.active_tab].saved_path.as_ref()
                        .and_then(|p| p.file_stem())
                        .and_then(|s| s.to_str())
                        .unwrap_or(&state.tabs[state.active_tab].name)
                        .to_string();
                    if let Some(path) = show_save_dialog(hwnd, &state.tabs[state.active_tab].save_dir, &current_stem) {
                        let _ = crate::output::file::save_png(&flat, &path);
                        if let Some(parent) = path.parent() {
                            let dir = parent.to_path_buf();
                            state.tabs[state.active_tab].save_dir = dir.clone();
                            state.default_save_dir = dir.clone();
                            crate::config::persist_save_dir(&dir);
                        }
                        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                            state.tabs[state.active_tab].name = stem.to_string();
                        }
                        state.tabs[state.active_tab].saved_path = Some(path.clone());
                        state.tabs[state.active_tab].modified = false;
                        update_window_title(hwnd, state);
                        let _ = state.tx.send(AppEvent::EditorSave { to_clipboard: false, path: Some(path) });
                        state.tabs[state.active_tab].result_sent = true;
                        update_scrollbars(hwnd, state);
                        InvalidateRect(hwnd, None, false);
                        SetFocus(hwnd);
                    }
                }
                BTN_SETTINGS => {
                    // 在按鈕正下方顯示設定選單（不含擷取功能）
                    if let Ok(btn) = GetDlgItem(hwnd, BTN_SETTINGS as i32) {
                        let mut btn_rc = RECT::default();
                        GetWindowRect(btn, &mut btn_rc).ok();
                        show_settings_popup(hwnd, state, btn_rc.left, btn_rc.bottom);
                    }
                    SetFocus(hwnd);
                }
                // 右鍵選單指令（ID 3000 ~ 3999）
                id if id >= 3000 && id < 4000 => {
                    handle_context_menu_cmd(hwnd, state, id as u32);
                }
                // 下拉選單分頁切換（ID 2000 ~ 2999）
                id if id >= 2000 && id < 2000 + state.tabs.len() => {
                    state.active_tab = id - 2000;
                    state.dragging = false;
                    // 確保選取的標籤在可見範圍
                    let mut rc2 = RECT::default();
                    GetClientRect(hwnd, &mut rc2).ok();
                    let mv = ((rc2.right - 22) / TAB_W).max(1) as usize;
                    if state.active_tab < state.tab_scroll {
                        state.tab_scroll = state.active_tab;
                    } else if state.active_tab >= state.tab_scroll + mv {
                        state.tab_scroll = state.active_tab + 1 - mv;
                    }
                    update_scrollbars(hwnd, state);
                    update_window_title(hwnd, state);
                    InvalidateRect(hwnd, None, false);
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_KEYDOWN if wp.0 == VK_ESCAPE.0 as usize => {
            // ESC 只隱藏視窗，所有分頁原封不動保留
            ShowWindow(hwnd, SW_HIDE);
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let state = &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut EditorState);
            if !state.tooltip.0.is_null() { ShowWindow(state.tooltip, SW_HIDE); }
            state.hover_ticks = 0;
            let (cx, cy) = client_xy(lp);

            // ── 標籤列點擊（工具列下方）─────────────────────────────────
            if cy >= TOOLBAR_H && cy < CANVAS_Y {
                const DROP_W2: i32 = 22;
                let mut rc2 = RECT::default();
                GetClientRect(hwnd, &mut rc2).ok();
                let max_vis = ((rc2.right - DROP_W2) / TAB_W).max(1) as usize;
                let show_drop = state.tabs.len() > max_vis;

                // ▼ 下拉按鈕
                if show_drop && cx >= rc2.right - DROP_W2 {
                    let hmenu = CreatePopupMenu().unwrap();
                    for (i, tab) in state.tabs.iter().enumerate() {
                        let flag = if i == state.active_tab { MF_STRING | MF_CHECKED } else { MF_STRING };
                        let nw: Vec<u16> = tab.name.encode_utf16().chain(Some(0)).collect();
                        let _ = AppendMenuW(hmenu, flag, 2000 + i, windows::core::PCWSTR(nw.as_ptr()));
                    }
                    let mut pt = POINT::default();
                    GetCursorPos(&mut pt).ok();
                    SetForegroundWindow(hwnd).ok();
                    TrackPopupMenu(hmenu, TPM_RIGHTBUTTON, pt.x, pt.y, 0, hwnd, None);
                    let _ = DestroyMenu(hmenu);
                    return LRESULT(0);
                }

                let tab_scr = state.tab_scroll.min(state.tabs.len().saturating_sub(1));
                let vis_count = (tab_scr + max_vis).min(state.tabs.len()) - tab_scr;
                let slot = (cx / TAB_W) as usize;  // 點擊的視覺格子（0 起）
                let idx = slot + tab_scr;           // 實際標籤索引
                if slot < vis_count {
                    let tx0 = slot as i32 * TAB_W;
                    let tw  = TAB_W;
                    if cx >= tx0 + tw - 18 {
                        close_tab(hwnd, state, idx);
                    } else {
                        state.active_tab = idx;
                        state.dragging = false;
                        update_scrollbars(hwnd, state);
                        update_window_title(hwnd, state);
                        InvalidateRect(hwnd, None, false);
                    }
                }
                return LRESULT(0);
            }

            let cy_canvas = cy - CANVAS_Y;
            if cy_canvas < 0 { return LRESULT(0); }
            // 加上 scroll offset 轉成 canvas 座標
            let pt = POINT { x: cx + state.tabs[state.active_tab].scroll_x, y: cy_canvas + state.tabs[state.active_tab].scroll_y };

            match state.active_tool {
                Tool::Text => {
                    let text = simple_input_dialog(hwnd);
                    if !text.is_empty() {
                        let (c, t) = {
                            let tab = &state.tabs[state.active_tab];
                            (tab.canvas.tool_color, tab.canvas.tool_thickness)
                        };
                        state.tabs[state.active_tab].canvas.strokes.push((
                            Stroke::Text { pos: pt, text },
                            super::tool::Color(c), t,
                        ));
                        if !state.tabs[state.active_tab].modified {
                            state.tabs[state.active_tab].modified = true;
                        }
                        InvalidateRect(hwnd, None, false);
                    }
                }
                _ => {
                    state.dragging = true;
                    state.drag_start = pt;
                    if state.active_tool == Tool::Pen {
                        state.tabs[state.active_tab].canvas.current = Some(Stroke::Pen { points: vec![pt] });
                    }
                    SetCapture(hwnd);
                }
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let state = &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut EditorState);
            let (_, cy_mm) = client_xy(lp);
            state.hovering_canvas = cy_mm >= CANVAS_Y;
            if !state.dragging { return LRESULT(0); }
            let (cx, cy) = client_xy(lp);
            let pt = POINT {
                x: cx + state.tabs[state.active_tab].scroll_x,
                y: (cy - CANVAS_Y) + state.tabs[state.active_tab].scroll_y,
            };
            match state.active_tool {
                Tool::Pen => {
                    if let Some(Stroke::Pen { ref mut points }) = state.tabs[state.active_tab].canvas.current {
                        points.push(pt);
                    }
                }
                Tool::Arrow => {
                    state.tabs[state.active_tab].canvas.current = Some(Stroke::Arrow { from: state.drag_start, to: pt });
                }
                Tool::Rect | Tool::Crop => {
                    let s = state.drag_start;
                    state.tabs[state.active_tab].canvas.current = Some(Stroke::Rect {
                        r: RECT {
                            left: s.x.min(pt.x), top: s.y.min(pt.y),
                            right: s.x.max(pt.x), bottom: s.y.max(pt.y),
                        },
                    });
                }
                _ => {}
            }
            // 只刷畫布區域（CANVAS_Y 以下），避免工具列＋標籤列重繪閃爍
            InvalidateRect(hwnd, Some(&RECT { left: 0, top: CANVAS_Y,
                right: 32767, bottom: 32767 }), false);
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let state = &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut EditorState);
            if state.dragging {
                state.dragging = false;
                ReleaseCapture().unwrap();
                if state.active_tool == Tool::Crop {
                    // 取得裁切矩形後直接套用
                    if let Some(Stroke::Rect { r }) = state.tabs[state.active_tab].canvas.current.take() {
                        if r.right - r.left > 4 && r.bottom - r.top > 4 {
                            state.tabs[state.active_tab].canvas.crop(r);
                            state.tabs[state.active_tab].scroll_x = 0;
                            state.tabs[state.active_tab].scroll_y = 0;
                            if !state.tabs[state.active_tab].modified {
                                state.tabs[state.active_tab].modified = true;
                            }
                        }
                    }
                    state.tabs[state.active_tab].canvas.current = None;
                    // 裁切改變畫布尺寸，需要重算捲軸（可能觸發 SWP_FRAMECHANGED）
                    update_scrollbars(hwnd, state);
                    InvalidateRect(hwnd, None, false); // 裁切後全部重繪（標籤也可能有變）
                } else if let Some(stroke) = state.tabs[state.active_tab].canvas.current.take() {
                    let (c, t) = {
                        let tab = &state.tabs[state.active_tab];
                        (tab.canvas.tool_color, tab.canvas.tool_thickness)
                    };
                    state.tabs[state.active_tab].canvas.push_stroke(
                        stroke, super::tool::Color(c), t,
                    );
                    if !state.tabs[state.active_tab].modified {
                        state.tabs[state.active_tab].modified = true;
                        // 首次修改：刷新標籤列以立即顯示紅點
                        InvalidateRect(hwnd, Some(&RECT{left:0,top:TOOLBAR_H,right:32767,bottom:CANVAS_Y}), false);
                    }
                    // 一般筆畫不改變畫布尺寸，只刷畫布區，不呼叫 update_scrollbars
                    // 避免 SWP_FRAMECHANGED 污染髒區域造成標籤閃爍
                    InvalidateRect(hwnd, Some(&RECT{left:0,top:CANVAS_Y,right:32767,bottom:32767}), false);
                    return LRESULT(0);
                }
            }
            LRESULT(0)
        }
        WM_PAINT => {
            let state = &*(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut EditorState);
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);

            let mut rc = RECT::default();
            GetClientRect(hwnd, &mut rc).unwrap();
            let client_w = rc.right;
            let client_h = (rc.bottom - CANVAS_Y).max(0);

            // 只有更新區域涵蓋 header 時才重繪工具列＋標籤列（防止閃爍）
            if ps.rcPaint.top < CANVAS_Y {

            // ── 工具列背景（上方）───────────────────────────────────────
            let tb = CreateSolidBrush(COLORREF(TOOLBAR_BG));
            FillRect(hdc, &RECT{left:0,top:0,right:client_w.max(1),bottom:TOOLBAR_H}, tb);
            DeleteObject(tb);

            // ── 標籤列（工具列正下方）──────────────────────────────────
            let tab_bar_bg = COLORREF(0x00_D8_D8_D8);
            let tbb = CreateSolidBrush(tab_bar_bg);
            FillRect(hdc, &RECT{left:0,top:TOOLBAR_H,right:client_w.max(1),bottom:CANVAS_Y}, tbb);
            DeleteObject(tbb);

            // 下拉按鈕寬度（標籤太多時顯示）
            const DROP_W: i32 = 22;
            let max_tabs_visible = ((client_w - DROP_W) / TAB_W).max(1) as usize;
            let tab_scroll = state.tab_scroll.min(state.tabs.len().saturating_sub(1));
            let visible_end = (tab_scroll + max_tabs_visible).min(state.tabs.len());
            let show_drop = state.tabs.len() > max_tabs_visible || tab_scroll > 0;

            let r = 8i32;  // 圓角半徑
            let ty = TOOLBAR_H + 2;

            // 裁切到標籤列範圍，讓延伸超出的圓角底部自然被截斷
            let saved_dc = SaveDC(hdc);
            IntersectClipRect(hdc, 0, TOOLBAR_H, client_w.max(1), CANVAS_Y);

            for i in tab_scroll..visible_end {
                let tab = &state.tabs[i];
                let tx0 = (i - tab_scroll) as i32 * TAB_W;  // 相對於捲動偏移的位置
                let tw  = TAB_W;               // 全寬
                let is_active = i == state.active_tab;
                let fill_c = COLORREF(if is_active { TOOLBAR_BG } else { 0x00_C8_C8_C8 });

                // 圓角矩形區域：延伸到 CANVAS_Y + r，底部圓角被 IntersectClipRect 截掉
                let rgn = CreateRoundRectRgn(tx0, ty, tx0+tw, CANVAS_Y + r, r*2, r*2);

                // 填色（無外框線）
                let fb = CreateSolidBrush(fill_c);
                FillRgn(hdc, rgn, fb);
                DeleteObject(fb);

                DeleteObject(HRGN(rgn.0));

                // 文字（Consolas 等寬字型，適合時間戳記）
                SetBkMode(hdc, BACKGROUND_MODE(1));
                windows::Win32::Graphics::Gdi::SetTextColor(hdc,
                    COLORREF(if is_active { 0x00_10_10_10 } else { 0x00_50_50_50 }));
                let show_dot = tab.saved_path.is_none() || tab.modified;
                // 有紅點時縮短文字區域，避免疊字
                let text_right = if show_dot { tx0 + tw - 30 } else { tx0 + tw - 19 };
                let tab_font = CreateFontW(-13, 0, 0, 0, 400, 0, 0, 0, 0, 0, 0, 0, 0, w!("Consolas"));
                let of = SelectObject(hdc, tab_font);
                let mut nw: Vec<u16> = tab.name.encode_utf16().collect();
                let mut nrc = RECT{left:tx0+5, top:ty, right:text_right, bottom:CANVAS_Y};
                DrawTextW(hdc, &mut nw, &mut nrc, DRAW_TEXT_FORMAT(0x25));
                SelectObject(hdc, of);
                DeleteObject(tab_font);
                // 紅點：位於文字右側、× 左側
                if show_dot {
                    let rp = CreatePen(PS_SOLID, 0, COLORREF(0x00_00_00_CC));
                    let rb = CreateSolidBrush(COLORREF(0x00_00_00_CC));
                    let op = SelectObject(hdc, rp);
                    let ob = SelectObject(hdc, rb);
                    let dot_x = tx0 + tw - 25; // × 左側
                    let dot_y = (ty + CANVAS_Y) / 2;
                    Ellipse(hdc, dot_x - 4, dot_y - 4, dot_x + 4, dot_y + 4);
                    SelectObject(hdc, op); SelectObject(hdc, ob);
                    DeleteObject(rp); DeleteObject(rb);
                }
                // ×
                windows::Win32::Graphics::Gdi::SetTextColor(hdc, COLORREF(0x00_70_70_70));
                let of2 = SelectObject(hdc, GetStockObject(DEFAULT_GUI_FONT));
                let mut xw = [0xD7u16];
                let mut xrc = RECT{left:tx0+tw-18, top:ty, right:tx0+tw-4, bottom:CANVAS_Y};
                DrawTextW(hdc, &mut xw, &mut xrc, DRAW_TEXT_FORMAT(0x25));
                SelectObject(hdc, of2);
            }

            RestoreDC(hdc, saved_dc);

            // ▼ 下拉按鈕（分頁過多時）
            if show_drop {
                let dx = client_w - DROP_W;
                let dp = CreatePen(PS_SOLID, 1, COLORREF(0x00_B0_B0_B0));
                let db = CreateSolidBrush(COLORREF(0x00_C8_C8_C8));
                let op = SelectObject(hdc, dp);
                let ob = SelectObject(hdc, db);
                GdiRectangle(hdc, dx, TOOLBAR_H+2, dx+DROP_W-2, CANVAS_Y-2);
                SelectObject(hdc, op); SelectObject(hdc, ob);
                DeleteObject(dp); DeleteObject(db);
                SetBkMode(hdc, BACKGROUND_MODE(1));
                windows::Win32::Graphics::Gdi::SetTextColor(hdc, COLORREF(0x00_30_30_30));
                let of = SelectObject(hdc, GetStockObject(DEFAULT_GUI_FONT));
                let mut dw = [0x25BCu16]; // ▼
                let mut drc = RECT{left:dx,top:TOOLBAR_H+2,right:dx+DROP_W-2,bottom:CANVAS_Y-2};
                DrawTextW(hdc, &mut dw, &mut drc, DRAW_TEXT_FORMAT(0x25));
                SelectObject(hdc, of);
            }

            } // end if ps.rcPaint.top < CANVAS_Y

            // 只有更新區域涵蓋畫布時才做昂貴的 Canvas::render
            if ps.rcPaint.bottom > CANVAS_Y {
                let screen_dc = GetDC(HWND(std::ptr::null_mut()));

                let buf_dc  = CreateCompatibleDC(screen_dc);
                let buf_bmp = CreateCompatibleBitmap(screen_dc, client_w.max(1), client_h.max(1));
                let old_buf = SelectObject(buf_dc, buf_bmp);

                let gray = CreateSolidBrush(windows::Win32::Foundation::COLORREF(0x00_B0_B0_B0));
                FillRect(buf_dc, &RECT{left:0,top:0,right:client_w.max(1),bottom:client_h.max(1)}, gray);
                DeleteObject(gray);

                let canvas_dc  = CreateCompatibleDC(screen_dc);
                let canvas_bmp = CreateCompatibleBitmap(screen_dc,
                    state.tabs[state.active_tab].canvas.width,
                    state.tabs[state.active_tab].canvas.height);
                let old_canvas = SelectObject(canvas_dc, canvas_bmp);
                state.tabs[state.active_tab].canvas.render(canvas_dc, screen_dc,
                    state.active_tool == Tool::Crop);

                let vis_w = (state.tabs[state.active_tab].canvas.width
                    - state.tabs[state.active_tab].scroll_x).min(client_w).max(0);
                let vis_h = (state.tabs[state.active_tab].canvas.height
                    - state.tabs[state.active_tab].scroll_y).min(client_h).max(0);
                if vis_w > 0 && vis_h > 0 {
                    BitBlt(buf_dc, 0, 0, vis_w, vis_h, canvas_dc,
                        state.tabs[state.active_tab].scroll_x,
                        state.tabs[state.active_tab].scroll_y, SRCCOPY).unwrap();
                }

                SelectObject(canvas_dc, old_canvas);
                DeleteObject(canvas_bmp);
                DeleteDC(canvas_dc);

                BitBlt(hdc, 0, CANVAS_Y, client_w, client_h, buf_dc, 0, 0, SRCCOPY).unwrap();

                SelectObject(buf_dc, old_buf);
                DeleteObject(buf_bmp);
                DeleteDC(buf_dc);
                ReleaseDC(HWND(std::ptr::null_mut()), screen_dc);
            } // end if ps.rcPaint.bottom > CANVAS_Y

            EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_DRAWITEM => {
            let dis = &*(lp.0 as *const DRAWITEMSTRUCT);
            let id = dis.CtlID as usize;
            let state = &*(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const EditorState);

            let is_pressed   = (dis.itemState.0 & 0x0001) != 0; // ODS_SELECTED
            let is_active_tool =
                (id == BTN_PEN   && state.active_tool == Tool::Pen)   ||
                (id == BTN_ARROW && state.active_tool == Tool::Arrow)  ||
                (id == BTN_RECT  && state.active_tool == Tool::Rect)   ||
                (id == BTN_TEXT  && state.active_tool == Tool::Text)   ||
                (id == BTN_CROP  && state.active_tool == Tool::Crop);

            // COLORREF = 0x00BBGGRR
            let bg = if is_pressed {
                windows::Win32::Foundation::COLORREF(0x00_22_22_22)
            } else if is_active_tool {
                windows::Win32::Foundation::COLORREF(0x00_D4_78_00) // #0078D4 藍
            } else {
                match id {
                    _         => COLORREF(TOOLBAR_BG), // 與工具列同色，只顯示圖示
                }
            };
            let saveas_disabled = id == BTN_SAVEAS
                && state.tabs[state.active_tab].saved_path.is_none();
            let text_color = match id {
                _ if saveas_disabled => windows::Win32::Foundation::COLORREF(0x00_C0_C0_C0), // 禁用灰
                _ if is_active_tool || is_pressed => windows::Win32::Foundation::COLORREF(0x00_FF_FF_FF),
                _ => windows::Win32::Foundation::COLORREF(0x00_40_40_40),
            };

            let hdc = dis.hDC;
            let rc = dis.rcItem;

            // 背景一律 FillRect（蓋滿整個 rc 包含四角），
            // 避免 pressed→released 時 RoundRect 漏蓋角落產生殘影
            {
                let brush = CreateSolidBrush(bg);
                FillRect(hdc, &rc, brush);
                DeleteObject(brush);
            }

            // 圖示（縮小版，筆寬 1px，座標約縮 35%）
            let cx = (rc.left + rc.right) / 2;
            let cy = (rc.top + rc.bottom) / 2;
            let ip = CreatePen(PS_SOLID, 1, text_color);
            let ib = CreateSolidBrush(text_color);
            let op = SelectObject(hdc, ip);
            let ob = SelectObject(hdc, ib);
            let nb = GetStockObject(NULL_BRUSH);

            match id {
                BTN_PEN => {
                    let _ = Polyline(hdc, &[POINT{x:cx-5,y:cy-5}, POINT{x:cx+3,y:cy+3}]);
                    let _ = Polygon(hdc, &[
                        POINT{x:cx+3,y:cy+3}, POINT{x:cx+6,y:cy+1}, POINT{x:cx+1,y:cy+6},
                    ]);
                }
                BTN_ARROW => {
                    let _ = Polyline(hdc, &[POINT{x:cx-7,y:cy}, POINT{x:cx+2,y:cy}]);
                    let _ = Polygon(hdc, &[
                        POINT{x:cx+2,y:cy-4}, POINT{x:cx+7,y:cy}, POINT{x:cx+2,y:cy+4},
                    ]);
                }
                BTN_RECT => {
                    let o = SelectObject(hdc, nb);
                    let _ = GdiRectangle(hdc, cx-7, cy-5, cx+7, cy+5);
                    SelectObject(hdc, o);
                }
                BTN_TEXT => {
                    let _ = MoveToEx(hdc, cx-6, cy-5, None); let _ = LineTo(hdc, cx+6, cy-5);
                    let _ = MoveToEx(hdc, cx,   cy-5, None); let _ = LineTo(hdc, cx,   cy+5);
                }
                BTN_COLOR => {
                    // 底部色條（窄一點）+ 小 ▼
                    let bar_l = rc.left + 7;
                    let bar_r = rc.right - 7;
                    let bar_t = rc.bottom - 8;
                    let bar_b = rc.bottom - 4;
                    let border = CreateSolidBrush(COLORREF(0x00_A0_A0_A0));
                    FillRect(hdc, &RECT { left: bar_l-1, top: bar_t-1,
                        right: bar_r+1, bottom: bar_b+1 }, border);
                    DeleteObject(border);
                    let cb = CreateSolidBrush(COLORREF(state.tabs[state.active_tab].canvas.tool_color));
                    FillRect(hdc, &RECT { left: bar_l, top: bar_t, right: bar_r, bottom: bar_b }, cb);
                    DeleteObject(cb);
                    let ap = CreatePen(PS_SOLID, 1, COLORREF(0x00_40_40_40));
                    let ab = CreateSolidBrush(COLORREF(0x00_40_40_40));
                    let top = SelectObject(hdc, ap); let tob = SelectObject(hdc, ab);
                    let _ = Polygon(hdc, &[
                        POINT{x:cx-3,y:cy-2}, POINT{x:cx+3,y:cy-2}, POINT{x:cx,y:cy+1},
                    ]);
                    SelectObject(hdc, top); SelectObject(hdc, tob);
                    DeleteObject(ap); DeleteObject(ab);
                }
                BTN_CROP => {
                    let o = SelectObject(hdc, nb);
                    let _ = GdiRectangle(hdc, cx-5, cy-5, cx+5, cy+5);
                    SelectObject(hdc, o);
                    let _ = MoveToEx(hdc, cx-5, cy-8, None); let _ = LineTo(hdc, cx-5, cy-5);
                    let _ = MoveToEx(hdc, cx-8, cy-5, None); let _ = LineTo(hdc, cx-5, cy-5);
                    let _ = MoveToEx(hdc, cx+5, cy-8, None); let _ = LineTo(hdc, cx+5, cy-5);
                    let _ = MoveToEx(hdc, cx+8, cy-5, None); let _ = LineTo(hdc, cx+5, cy-5);
                    let _ = MoveToEx(hdc, cx-5, cy+8, None); let _ = LineTo(hdc, cx-5, cy+5);
                    let _ = MoveToEx(hdc, cx-8, cy+5, None); let _ = LineTo(hdc, cx-5, cy+5);
                    let _ = MoveToEx(hdc, cx+5, cy+8, None); let _ = LineTo(hdc, cx+5, cy+5);
                    let _ = MoveToEx(hdc, cx+8, cy+5, None); let _ = LineTo(hdc, cx+5, cy+5);
                }
                BTN_COPY => {
                    let o = SelectObject(hdc, nb);
                    let _ = GdiRectangle(hdc, cx-6, cy-2, cx+2, cy+6);
                    let _ = GdiRectangle(hdc, cx-2, cy-6, cx+6, cy+2);
                    SelectObject(hdc, o);
                }
                BTN_SAVE => {
                    let _ = MoveToEx(hdc, cx, cy-5, None); let _ = LineTo(hdc, cx, cy);
                    let _ = Polygon(hdc, &[
                        POINT{x:cx-4,y:cy}, POINT{x:cx,y:cy+5}, POINT{x:cx+4,y:cy},
                    ]);
                    let _ = MoveToEx(hdc, cx-5, cy+7, None); let _ = LineTo(hdc, cx+5, cy+7);
                }
                BTN_SAVEAS => {
                    // 儲存圖示 + 右上角 + 號（代表另存新檔）
                    let _ = MoveToEx(hdc, cx-2, cy-4, None); let _ = LineTo(hdc, cx-2, cy+1);
                    let _ = Polygon(hdc, &[
                        POINT{x:cx-6,y:cy+1}, POINT{x:cx-2,y:cy+6}, POINT{x:cx+2,y:cy+1},
                    ]);
                    let _ = MoveToEx(hdc, cx-7, cy+8, None); let _ = LineTo(hdc, cx+3, cy+8);
                    // 右上 + 號
                    let _ = MoveToEx(hdc, cx+5, cy-7, None); let _ = LineTo(hdc, cx+5, cy-3);
                    let _ = MoveToEx(hdc, cx+3, cy-5, None); let _ = LineTo(hdc, cx+7, cy-5);
                }
                BTN_UNDO => {
                    let _ = GdiArc(hdc, cx-6, cy-5, cx+6, cy+5, cx+6, cy, cx-6, cy);
                    let _ = Polygon(hdc, &[
                        POINT{x:cx-6,y:cy+3}, POINT{x:cx-8,y:cy}, POINT{x:cx-4,y:cy},
                    ]);
                }
                BTN_SETTINGS => {
                    // ≡ 三條橫線
                    for dy in [-5i32, 0, 5] {
                        let _ = MoveToEx(hdc, cx-7, cy+dy, None);
                        let _ = LineTo(hdc, cx+7, cy+dy);
                    }
                }
                _ => {}
            }

            SelectObject(hdc, op); SelectObject(hdc, ob);
            DeleteObject(ip); DeleteObject(ib);

            LRESULT(1)
        }
        // 右鍵選單：在視窗任意位置右鍵顯示捕獲/設定選單
        WM_CONTEXTMENU => {
            let state = &*(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const EditorState);
            let sx = (lp.0 & 0xFFFF) as i16 as i32;
            let sy = ((lp.0 >> 16) & 0xFFFF) as i16 as i32;
            show_editor_popup(hwnd, state, sx, sy);
            LRESULT(0)
        }
        WM_TIMER if wp.0 == 3 => {
            // 每 100ms 輪詢：用 WindowFromPoint 偵測游標在哪個按鈕上
            let state = &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut EditorState);
            let mut pt = POINT::default();
            GetCursorPos(&mut pt).ok();
            let under = WindowFromPoint(pt);
            let btn_hover: i32 = if under.is_invalid() { -1 } else {
                match GetDlgCtrlID(under) as usize {
                    BTN_PEN   => 0, BTN_ARROW => 1, BTN_RECT  => 2, BTN_TEXT  => 3,
                    BTN_CROP  => 4, BTN_COLOR => 5, BTN_COPY  => 6, BTN_SAVE   => 7,
                    BTN_SAVEAS => 8, BTN_UNDO => 9, BTN_SETTINGS => 10, _ => -1,
                }
            };
            if btn_hover != state.hover_btn {
                state.hover_btn = btn_hover;
                state.hover_ticks = 0;
                if !state.tooltip.0.is_null() { ShowWindow(state.tooltip, SW_HIDE); }
            } else if btn_hover >= 0 {
                state.hover_ticks += 1;
                if state.hover_ticks == 5 { // 5×100ms = 500ms 後顯示
                    let labels = ["筆","箭頭","矩形","文字","裁切","顏色","複製","儲存","另存","復原","設定"];
                    if let Some(label) = labels.get(btn_hover as usize) {
                        let text: Vec<u16> = label.encode_utf16().chain(Some(0)).collect();
                        SetWindowTextW(state.tooltip, windows::core::PCWSTR(text.as_ptr())).ok();
                        InvalidateRect(state.tooltip, None, false);
                        SetWindowPos(state.tooltip, HWND_TOPMOST,
                            pt.x + 4, pt.y + 20, 72, 22,
                            SWP_SHOWWINDOW | SWP_NOZORDER | SWP_NOACTIVATE).ok();
                    }
                }
            }
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_SETCURSOR => {
            // LOWORD(lParam) = hit-test code；1 = HTCLIENT
            if (lp.0 & 0xFFFF) as i32 == 1 {
                let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const EditorState;
                if !ptr.is_null() {
                    let state = &*ptr;
                    // hovering_canvas 由 WM_MOUSEMOVE 維護，避免使用不在 glob 中的 ScreenToClient
                    if state.hovering_canvas {
                        let cursor_id = match state.active_tool {
                            Tool::Text => IDC_IBEAM,
                            _          => IDC_CROSS,
                        };
                        SetCursor(LoadCursorW(None, cursor_id).unwrap());
                        return LRESULT(1);
                    }
                }
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        // WM_CLOSE：隱藏視窗（不銷毀），保留所有分頁
        WM_CLOSE => {
            ShowWindow(hwnd, SW_HIDE);
            LRESULT(0)
        }
        // 雙按系統匣 → 帶到前景
        WM_SHOW_EDITOR => {
            SetWindowPos(hwnd, HWND_TOPMOST, 0,0,0,0, SWP_NOMOVE|SWP_NOSIZE|SWP_SHOWWINDOW).ok();
            SetForegroundWindow(hwnd).ok();
            SetWindowPos(hwnd, HWND_NOTOPMOST, 0,0,0,0, SWP_NOMOVE|SWP_NOSIZE).ok();
            LRESULT(0)
        }
        // 新截圖分頁（lParam = *mut ScreenBitmap）
        WM_NEW_TAB => {
            if lp.0 != 0 {
                let bmp = *Box::from_raw(lp.0 as *mut ScreenBitmap);
                let state = &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut EditorState);
                state.tab_counter += 1;
                state.tabs.push(TabInfo {
                    canvas: Canvas::new(bmp),
                    save_dir: state.default_save_dir.clone(),
                    scroll_x: 0, scroll_y: 0,
                    result_sent: false,
                    name: {
                        let st = windows::Win32::System::SystemInformation::GetLocalTime();
                        format!("{}{:02}{:02}{:02}{:02}{:02}", st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond)
                    },
                    saved_path: None,
            modified: false,
                });
                state.active_tab = state.tabs.len() - 1;
                state.dragging = false;
                // 捲動到新標籤（讓既有標籤往左移）
                {
                    let mut rc_tmp = RECT::default();
                    GetClientRect(hwnd, &mut rc_tmp).ok();
                    let mv = ((rc_tmp.right - 22) / TAB_W).max(1) as usize;
                    if state.active_tab >= state.tab_scroll + mv {
                        state.tab_scroll = state.active_tab + 1 - mv;
                    }
                }
                SetWindowPos(hwnd, HWND_TOPMOST, 0,0,0,0, SWP_NOMOVE|SWP_NOSIZE|SWP_SHOWWINDOW).ok();
                SetForegroundWindow(hwnd).ok();
                SetWindowPos(hwnd, HWND_NOTOPMOST, 0,0,0,0, SWP_NOMOVE|SWP_NOSIZE).ok();
                update_scrollbars(hwnd, state);
                update_window_title(hwnd, state);
                InvalidateRect(hwnd, None, false);
            }
            LRESULT(0)
        }
        // 應用程式結束 → 真正銷毀
        WM_FORCE_QUIT => {
            DestroyWindow(hwnd).ok();
            LRESULT(0)
        }
        WM_DESTROY => {
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut EditorState;
            if !ptr.is_null() {
                let state = &mut *ptr;
                // 清除 HWND 登記
                *state.editor_hwnd_arc.lock().unwrap() = None;
                if !state.tooltip.0.is_null() { DestroyWindow(state.tooltip).ok(); }
                drop(Box::from_raw(ptr));
            }
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

/// 依目前作用中標籤更新視窗標題
unsafe fn update_window_title(hwnd: HWND, state: &EditorState) {
    if state.tabs.is_empty() { return; }
    let title: Vec<u16> = format!("ezshot-{}\0", state.tabs[state.active_tab].name)
        .encode_utf16().collect();
    SetWindowTextW(hwnd, windows::core::PCWSTR(title.as_ptr())).ok();
}

/// 關閉第 idx 個分頁；無分頁時隱藏視窗
unsafe fn close_tab(hwnd: HWND, state: &mut EditorState, idx: usize) {
    if idx >= state.tabs.len() { return; }
    state.tabs.remove(idx);
    if state.tabs.is_empty() {
        ShowWindow(hwnd, SW_HIDE);
        return;
    }
    if state.active_tab >= state.tabs.len() {
        state.active_tab = state.tabs.len() - 1;
    }
    state.dragging = false;
    update_scrollbars(hwnd, state);
    update_window_title(hwnd, state);
    InvalidateRect(hwnd, None, false);
}

fn client_xy(lp: LPARAM) -> (i32, i32) {
    let x = (lp.0 & 0xFFFF) as i16 as i32;
    let y = ((lp.0 >> 16) & 0xFFFF) as i16 as i32;
    (x, y)
}

unsafe fn simple_input_dialog(parent: HWND) -> String {
    let class = w!("srcshot_textinput");
    let hinstance = get_instance();

    struct InputState { text: String, done: bool }

    unsafe extern "system" fn input_wnd_proc(
        hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_NCCREATE => {
                let cs = &*(lp.0 as *const CREATESTRUCTW);
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as _);
                LRESULT(1)
            }
            WM_CREATE => {
                let hinstance = get_instance();
                let edit = CreateWindowExW(
                    Default::default(), w!("EDIT"), w!(""),
                    WS_CHILD | WS_VISIBLE | WS_BORDER | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
                    8, 8, 260, 24, hwnd, HMENU(1usize as *mut _), hinstance, None,
                ).unwrap();
                CreateWindowExW(
                    Default::default(), w!("BUTTON"), w!("確定"),
                    WS_CHILD | WS_VISIBLE | WINDOW_STYLE(BS_DEFPUSHBUTTON as u32),
                    8, 40, 80, 28, hwnd, HMENU(2usize as *mut _), hinstance, None,
                ).unwrap();
                // 送 WM_NEXTDLGCTL 讓 edit 自動成為焦點
                PostMessageW(hwnd, WM_NEXTDLGCTL, WPARAM(edit.0 as usize), LPARAM(1)).ok();
                LRESULT(0)
            }
            WM_COMMAND if (wp.0 & 0xFFFF) == 2 => {
                // 確定按鈕：讀取文字後同步銷毀視窗（不 PostMessage WM_CLOSE）
                let state = &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut InputState);
                if let Ok(edit) = GetDlgItem(hwnd, 1) {
                    let len = GetWindowTextLengthW(edit) + 1;
                    let mut buf = vec![0u16; len as usize];
                    GetWindowTextW(edit, &mut buf);
                    state.text = String::from_utf16_lossy(&buf)
                        .trim_end_matches('\0').to_string();
                }
                state.done = true;
                DestroyWindow(hwnd).ok(); // 同步銷毀，WM_DESTROY 不再 PostQuitMessage
                LRESULT(0)
            }
            WM_CLOSE => {
                // 使用者按 X 或 Escape（IsDialogMessageW 轉送）：直接銷毀
                let state = &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut InputState);
                state.done = true;
                DestroyWindow(hwnd).ok();
                LRESULT(0)
            }
            // WM_DESTROY：絕對不能呼叫 PostQuitMessage，否則會殺死編輯器迴圈
            WM_DESTROY => LRESULT(0),
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }

    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(input_wnd_proc),
        hInstance: hinstance,
        lpszClassName: class,
        ..Default::default()
    };
    let _ = RegisterClassExW(&wc);

    let mut state = InputState { text: String::new(), done: false };
    let dlg = CreateWindowExW(
        WS_EX_DLGMODALFRAME | WS_EX_TOPMOST,
        class,
        w!("輸入文字"),
        WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_VISIBLE,
        CW_USEDEFAULT, CW_USEDEFAULT, 290, 110,
        parent,
        HMENU(std::ptr::null_mut()),
        hinstance,
        Some(&mut state as *mut _ as _),
    )
    .unwrap();

    let mut msg = MSG::default();
    while GetMessageW(&mut msg, HWND(std::ptr::null_mut()), 0, 0).as_bool() {
        // Enter 直接確定（CreateWindowEx 建立的非 dialog 視窗，
        // IsDialogMessageW 可能無法正確觸發預設按鈕，所以手動攔截）
        if msg.message == WM_KEYDOWN && msg.wParam.0 == VK_RETURN.0 as usize {
            if let Ok(edit) = GetDlgItem(dlg, 1) {
                let len = GetWindowTextLengthW(edit) + 1;
                let mut buf = vec![0u16; len as usize];
                GetWindowTextW(edit, &mut buf);
                state.text = String::from_utf16_lossy(&buf)
                    .trim_end_matches('\0').to_string();
            }
            state.done = true;
            DestroyWindow(dlg).ok();
            break;
        }
        // IsDialogMessageW 處理 Escape（取消）、Tab（切焦點）
        if IsDialogMessageW(dlg, &msg).as_bool() {
            if state.done { break; }
            continue;
        }
        let _ = TranslateMessage(&msg);
        DispatchMessageW(&msg);
        if state.done { break; }
    }

    let _ = UnregisterClassW(class, hinstance);
    state.text
}

/// 顯示系統另存新檔對話框，回傳使用者選擇的路徑；取消則回傳 None
/// `default_name`：對話框預設檔名（不含副檔名）
unsafe fn show_save_dialog(owner: HWND, initial_dir: &std::path::Path, default_name: &str) -> Option<std::path::PathBuf> {
    use windows::Win32::System::Com::{CoCreateInstance, IBindCtx, CLSCTX_INPROC_SERVER};
    use windows::Win32::UI::Shell::{
        FileSaveDialog, IFileSaveDialog, IShellItem, SHCreateItemFromParsingName, SIGDN_FILESYSPATH,
    };

    let dialog: IFileSaveDialog =
        CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER).ok()?;

    // 預設副檔名（若使用者未輸入則自動補上）
    let _ = dialog.SetDefaultExtension(w!("png"));

    // 預設檔名（呼叫端傳入）
    let fname: Vec<u16> = format!("{}.png\0", default_name).encode_utf16().collect();
    let _ = dialog.SetFileName(windows::core::PCWSTR(fname.as_ptr()));

    // 起始資料夾（上次儲存位置）
    let dir_wide: Vec<u16> = initial_dir.to_string_lossy()
        .encode_utf16().chain(Some(0)).collect();
    let folder_res: windows::core::Result<IShellItem> =
        SHCreateItemFromParsingName(windows::core::PCWSTR(dir_wide.as_ptr()), None::<&IBindCtx>);
    if let Ok(ref folder) = folder_res {
        let _ = dialog.SetFolder(folder);
    }

    // 顯示對話框（內部執行 modal 訊息迴圈，阻塞直到關閉）
    dialog.Show(owner).ok()?;

    // 取得選擇的完整路徑
    let item = dialog.GetResult().ok()?;
    let path_raw = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
    let path_str = path_raw.to_string().ok()?;
    Some(std::path::PathBuf::from(path_str))
}

/// 以 hex 輸入自訂顏色（如 FF0000）。current 為預填值。
unsafe fn custom_color_input_dialog(owner: HWND, current: u32) -> Option<u32> {
    // current 是 COLORREF(0x00BBGGRR)，轉成 "RRGGBB" 顯示
    let r = current & 0xFF;
    let g = (current >> 8) & 0xFF;
    let b = (current >> 16) & 0xFF;
    let preset = format!("{:02X}{:02X}{:02X}\0", r, g, b);
    let preset_w: Vec<u16> = preset.encode_utf16().collect();

    let class     = w!("srcshot_hexdlg");
    let hinstance = get_instance();

    struct HState { text: String, done: bool }

    unsafe extern "system" fn hex_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        match msg {
            WM_NCCREATE => {
                let cs = &*(lp.0 as *const CREATESTRUCTW);
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as _);
                LRESULT(1)
            }
            WM_CREATE => {
                let hi = get_instance();
                CreateWindowExW(Default::default(), w!("STATIC"),
                    w!("16進位色碼（如 FF0000）："),
                    WS_CHILD | WS_VISIBLE, 8, 10, 200, 18,
                    hwnd, HMENU(std::ptr::null_mut()), hi, None).ok();
                let edit = CreateWindowExW(WS_EX_CLIENTEDGE, w!("EDIT"), w!(""),
                    WS_CHILD | WS_VISIBLE, 8, 32, 100, 24,
                    hwnd, HMENU(1usize as _), hi, None).unwrap();
                // 預填目前顏色
                let state = &*(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const HState);
                let pre: Vec<u16> = state.text.encode_utf16().chain(Some(0)).collect();
                SetWindowTextW(edit, windows::core::PCWSTR(pre.as_ptr())).ok();
                CreateWindowExW(Default::default(), w!("BUTTON"), w!("確定"),
                    WS_CHILD | WS_VISIBLE | WINDOW_STYLE(BS_DEFPUSHBUTTON as u32),
                    8, 64, 80, 28, hwnd, HMENU(2usize as _), hi, None).ok();
                CreateWindowExW(Default::default(), w!("BUTTON"), w!("取消"),
                    WS_CHILD | WS_VISIBLE, 96, 64, 80, 28,
                    hwnd, HMENU(3usize as _), hi, None).ok();
                PostMessageW(hwnd, WM_NEXTDLGCTL, WPARAM(edit.0 as usize), LPARAM(1)).ok();
                LRESULT(0)
            }
            WM_COMMAND if (wp.0 & 0xFFFF) == 2 => {
                let state = &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut HState);
                if let Ok(edit) = GetDlgItem(hwnd, 1) {
                    let n = GetWindowTextLengthW(edit) + 1;
                    let mut buf = vec![0u16; n as usize];
                    GetWindowTextW(edit, &mut buf);
                    state.text = String::from_utf16_lossy(&buf).trim_end_matches('\0').to_string();
                }
                state.done = true;
                DestroyWindow(hwnd).ok();
                LRESULT(0)
            }
            WM_COMMAND if (wp.0 & 0xFFFF) == 3 => {
                let state = &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut HState);
                state.text.clear();
                state.done = true;
                DestroyWindow(hwnd).ok();
                LRESULT(0)
            }
            WM_CLOSE => {
                let state = &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut HState);
                state.text.clear();
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
        lpfnWndProc: Some(hex_proc),
        hInstance: hinstance,
        lpszClassName: class,
        ..Default::default()
    };
    let _ = RegisterClassExW(&wc);

    let mut state = HState {
        text: format!("{:02X}{:02X}{:02X}", r, g, b),
        done: false,
    };
    let dlg = CreateWindowExW(
        WS_EX_DLGMODALFRAME | WS_EX_TOPMOST,
        class, w!("自訂顏色"),
        WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_VISIBLE,
        CW_USEDEFAULT, CW_USEDEFAULT, 200, 136,
        owner, HMENU(std::ptr::null_mut()), hinstance,
        Some(&mut state as *mut _ as _),
    ).unwrap_or(HWND(std::ptr::null_mut()));

    if dlg.0.is_null() { let _ = UnregisterClassW(class, hinstance); return None; }

    let mut msg = MSG::default();
    while GetMessageW(&mut msg, HWND(std::ptr::null_mut()), 0, 0).as_bool() {
        if msg.message == WM_KEYDOWN && msg.wParam.0 == VK_RETURN.0 as usize {
            if let Ok(edit) = GetDlgItem(dlg, 1) {
                let n = GetWindowTextLengthW(edit) + 1;
                let mut buf = vec![0u16; n as usize];
                GetWindowTextW(edit, &mut buf);
                state.text = String::from_utf16_lossy(&buf).trim_end_matches('\0').to_string();
            }
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
    let _ = preset_w; // suppress unused warning

    // 解析 RRGGBB → COLORREF(0x00BBGGRR)
    let s = state.text.trim().trim_start_matches('#');
    if s.len() == 6 {
        if let (Ok(r2), Ok(g2), Ok(b2)) = (
            u32::from_str_radix(&s[0..2], 16),
            u32::from_str_radix(&s[2..4], 16),
            u32::from_str_radix(&s[4..6], 16),
        ) {
            return Some(r2 | (g2 << 8) | (b2 << 16));
        }
    }
    None
}

/// 下拉式顏色選取面板（無標題列，定位在 BTN_COLOR 正下方）
/// 回傳：Some(COLORREF) 選色、Some(0xFF000000) 自訂、None 取消
unsafe fn simple_color_dialog(owner: HWND) -> Option<u32> {
    const PALETTE: [u32; 12] = [
        0x00_00_00_00, 0x00_40_40_40, 0x00_80_80_80, 0x00_FF_FF_FF,
        0x00_00_00_FF, 0x00_00_80_FF, 0x00_00_FF_FF, 0x00_00_C8_00,
        0x00_FF_FF_00, 0x00_FF_00_00, 0x00_80_00_80, 0x00_FF_00_FF,
    ];
    const SW: i32 = 28;   // swatch 大小
    const SG: i32 = 2;    // swatch 間距
    const CG: i32 = 4;    // 色盤到自訂行的間距
    const CH: i32 = 22;   // 自訂行高度
    const PAD: i32 = 4;
    const COLS: i32 = 6;
    const ROWS: i32 = 2;

    let win_w      = 2 * PAD + COLS * SW + (COLS - 1) * SG;
    let swatches_h = ROWS * SW + (ROWS - 1) * SG;
    let win_h      = 2 * PAD + swatches_h + CG + CH;
    let custom_y   = PAD + swatches_h + CG; // "自訂..." 文字起始 y

    let class     = w!("srcshot_colordrop");
    let hinstance = get_instance();

    struct CState { selected: i32, done: bool, win_w: i32, win_h: i32,
                    custom_y: i32 }

    unsafe extern "system" fn drop_proc(
        hwnd: HWND, msg: u32, _wp: WPARAM, lp: LPARAM,
    ) -> LRESULT {
        const SW3: i32 = 28; const SG3: i32 = 2;
        const PAD3: i32 = 4; const COLS3: i32 = 6; const ROWS3: i32 = 2;
        const PALETTE3: [u32; 12] = [
            0x00_00_00_00, 0x00_40_40_40, 0x00_80_80_80, 0x00_FF_FF_FF,
            0x00_00_00_FF, 0x00_00_80_FF, 0x00_00_FF_FF, 0x00_00_C8_00,
            0x00_FF_FF_00, 0x00_FF_00_00, 0x00_80_00_80, 0x00_FF_00_FF,
        ];
        match msg {
            WM_NCCREATE => {
                let cs = &*(lp.0 as *const CREATESTRUCTW);
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as _);
                LRESULT(1)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_PAINT => {
                let state = &*(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const CState);
                let mut ps = PAINTSTRUCT::default();
                let hdc = BeginPaint(hwnd, &mut ps);
                let mut rc = RECT::default();
                GetClientRect(hwnd, &mut rc);
                // 背景
                let bg = CreateSolidBrush(COLORREF(0x00_F5_F5_F5));
                FillRect(hdc, &rc, bg);
                DeleteObject(bg);
                // 12 個色塊
                for i in 0..12i32 {
                    let col = i % COLS3;
                    let row = i / COLS3;
                    let x = PAD3 + col * (SW3 + SG3);
                    let y = PAD3 + row * (SW3 + SG3);
                    let c = PALETTE3[i as usize];
                    let b = CreateSolidBrush(COLORREF(c));
                    let p = CreatePen(PS_SOLID, 1, COLORREF(0x00_A0_A0_A0));
                    let ob = SelectObject(hdc, b);
                    let op = SelectObject(hdc, p);
                    GdiRectangle(hdc, x, y, x + SW3, y + SW3);
                    SelectObject(hdc, ob); SelectObject(hdc, op);
                    DeleteObject(b); DeleteObject(p);
                }
                // 分隔線
                let sep = CreatePen(PS_SOLID, 1, COLORREF(0x00_C0_C0_C0));
                let op = SelectObject(hdc, sep);
                let _ = windows::Win32::Graphics::Gdi::MoveToEx(hdc, PAD3, state.custom_y - 2, None);
                let _ = windows::Win32::Graphics::Gdi::LineTo(hdc, rc.right - PAD3, state.custom_y - 2);
                SelectObject(hdc, op); DeleteObject(sep);
                // 「自訂…」文字
                let font = GetStockObject(DEFAULT_GUI_FONT);
                let of = SelectObject(hdc, font);
                SetBkMode(hdc, BACKGROUND_MODE(1));
                windows::Win32::Graphics::Gdi::SetTextColor(hdc, COLORREF(0x00_30_30_30));
                let mut custom_rc = RECT { left: PAD3, top: state.custom_y,
                    right: rc.right - PAD3, bottom: rc.bottom - PAD3 };
                DrawTextW(hdc, &mut "自訂…".encode_utf16().collect::<Vec<_>>(),
                    &mut custom_rc, DRAW_TEXT_FORMAT(0x25));
                SelectObject(hdc, of);
                EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_LBUTTONDOWN => {
                let state = &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut CState);
                let (cx2, cy2) = client_xy(lp);
                // 點在視窗外→取消
                if cx2 < 0 || cy2 < 0 || cx2 >= state.win_w || cy2 >= state.win_h {
                    state.done = true;
                    DestroyWindow(hwnd).ok();
                    return LRESULT(0);
                }
                // 點在色塊上
                for i in 0..12i32 {
                    let col = i % COLS3;
                    let row = i / COLS3;
                    let x = PAD3 + col * (SW3 + SG3);
                    let y = PAD3 + row * (SW3 + SG3);
                    if cx2 >= x && cx2 < x + SW3 && cy2 >= y && cy2 < y + SW3 {
                        state.selected = i;
                        state.done = true;
                        DestroyWindow(hwnd).ok();
                        return LRESULT(0);
                    }
                }
                // 點在「自訂…」區域
                if cy2 >= state.custom_y {
                    state.selected = -2;
                    state.done = true;
                    DestroyWindow(hwnd).ok();
                }
                LRESULT(0)
            }
            WM_KEYDOWN if _wp.0 == VK_ESCAPE.0 as usize => {
                let state = &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut CState);
                state.done = true;
                DestroyWindow(hwnd).ok();
                LRESULT(0)
            }
            WM_DESTROY => LRESULT(0),
            _ => DefWindowProcW(hwnd, msg, _wp, lp),
        }
    }

    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(drop_proc),
        hInstance: hinstance,
        lpszClassName: class,
        ..Default::default()
    };
    let _ = RegisterClassExW(&wc);

    // 取得 BTN_COLOR 的螢幕位置，下拉面板定位在按鈕正下方
    let mut btn_rc = RECT::default();
    if let Ok(btn) = GetDlgItem(owner, BTN_COLOR as i32) {
        GetWindowRect(btn, &mut btn_rc).ok();
    }
    let drop_x = btn_rc.left;
    let drop_y = btn_rc.bottom;

    let mut state = CState { selected: -1, done: false,
        win_w, win_h, custom_y };
    let drop = CreateWindowExW(
        WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
        class, w!(""),
        WS_POPUP | WS_BORDER | WS_VISIBLE,
        drop_x, drop_y, win_w, win_h,
        owner, HMENU(std::ptr::null_mut()), hinstance,
        Some(&mut state as *mut _ as _),
    ).unwrap_or(HWND(std::ptr::null_mut()));

    if drop.0.is_null() { let _ = UnregisterClassW(class, hinstance); return None; }

    // SetCapture 讓點在面板外也能收到 WM_LBUTTONDOWN
    windows::Win32::UI::Input::KeyboardAndMouse::SetCapture(drop);

    let mut msg = MSG::default();
    while GetMessageW(&mut msg, HWND(std::ptr::null_mut()), 0, 0).as_bool() {
        let _ = TranslateMessage(&msg);
        DispatchMessageW(&msg);
        if state.done { break; }
    }
    windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture().ok();
    let _ = UnregisterClassW(class, hinstance);

    match state.selected {
        -2 => Some(0xFF_00_00_00),
        i if i >= 0 => Some(PALETTE[i as usize]),
        _ => None,
    }
}

/// 自製 tooltip 視窗的 wnd proc：畫淺黃底 + 文字
unsafe extern "system" fn tip_wnd_proc(
    hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM,
) -> LRESULT {
    use windows::Win32::Foundation::COLORREF;
    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            let mut rc = RECT::default();
            GetClientRect(hwnd, &mut rc);
            let bg = CreateSolidBrush(COLORREF(0x00_E1_FF_FF)); // 淺黃
            FillRect(hdc, &rc, bg);
            DeleteObject(bg);
            let mut buf = [0u16; 32];
            let n = GetWindowTextW(hwnd, &mut buf) as usize;
            if n > 0 {
                SetBkMode(hdc, BACKGROUND_MODE(1)); // TRANSPARENT
                let font = GetStockObject(DEFAULT_GUI_FONT);
                let of = SelectObject(hdc, font);
                DrawTextW(hdc, &mut buf[..n], &mut rc, DRAW_TEXT_FORMAT(0x25));
                SelectObject(hdc, of);
            }
            EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

/// 工具列設定按鈕點擊後的設定選單（只有設定選項，不含擷取功能）
unsafe fn show_settings_popup(hwnd: HWND, state: &EditorState, sx: i32, sy: i32) {
    let (capture_cursor, delay_secs, auto_copy, hide_on_capture) = {
        let c = state.config.lock().unwrap();
        (c.capture_cursor, c.capture_delay_secs, c.auto_copy, c.hide_editor_on_capture)
    };
    let hmenu = CreatePopupMenu().unwrap();
    let cf = if capture_cursor { MF_STRING | MF_CHECKED } else { MF_STRING };
    let _ = AppendMenuW(hmenu, cf, CM_TOGGLE_CURSOR as usize, w!("擷取滑鼠游標"));
    let af = if auto_copy { MF_STRING | MF_CHECKED } else { MF_STRING };
    let _ = AppendMenuW(hmenu, af, CM_TOGGLE_AUTOCOPY as usize, w!("直接複製到剪貼簿"));
    let hf = if hide_on_capture { MF_STRING | MF_CHECKED } else { MF_STRING };
    let _ = AppendMenuW(hmenu, hf, CM_TOGGLE_HIDE_ON_CAPTURE as usize, w!("擷取前隱藏編輯視窗"));

    let delay_menu = CreatePopupMenu().unwrap();
    let _ = AppendMenuW(delay_menu, MF_STRING, CM_DELAY_0 as usize, w!("無延遲"));
    let _ = AppendMenuW(delay_menu, MF_STRING, CM_DELAY_1 as usize, w!("1 秒"));
    let _ = AppendMenuW(delay_menu, MF_STRING, CM_DELAY_2 as usize, w!("2 秒"));
    let _ = AppendMenuW(delay_menu, MF_STRING, CM_DELAY_3 as usize, w!("3 秒"));
    let _ = AppendMenuW(delay_menu, MF_STRING, CM_DELAY_5 as usize, w!("5 秒"));
    let is_preset = [0u32, 1, 2, 3, 5].contains(&delay_secs);
    let custom_label: Vec<u16> = if is_preset {
        "自訂...\0".encode_utf16().collect()
    } else {
        format!("自訂: {} 秒...\0", delay_secs).encode_utf16().collect()
    };
    let _ = AppendMenuW(delay_menu, MF_STRING, CM_DELAY_CUSTOM as usize,
        windows::core::PCWSTR(custom_label.as_ptr()));
    let cur_id = match delay_secs { 1=>CM_DELAY_1, 2=>CM_DELAY_2, 3=>CM_DELAY_3, 5=>CM_DELAY_5, _=>CM_DELAY_0 };
    let _ = CheckMenuRadioItem(delay_menu, CM_DELAY_0, CM_DELAY_5, cur_id, MF_BYCOMMAND.0);
    if !is_preset { let _ = CheckMenuItem(delay_menu, CM_DELAY_CUSTOM, MF_BYCOMMAND.0 | MF_CHECKED.0); }
    let _ = AppendMenuW(hmenu, MF_POPUP, delay_menu.0 as usize, w!("延遲擷取"));

    SetForegroundWindow(hwnd).ok();
    TrackPopupMenu(hmenu, TPM_RIGHTBUTTON, sx, sy, 0, hwnd, None);
    let _ = DestroyMenu(hmenu);
}

/// 在編輯視窗顯示右鍵選單（捕獲方式 + 設定），與系統匣選單功能相同
unsafe fn show_editor_popup(hwnd: HWND, state: &EditorState, sx: i32, sy: i32) {
    let (capture_cursor, delay_secs, auto_copy) = {
        let c = state.config.lock().unwrap();
        (c.capture_cursor, c.capture_delay_secs, c.auto_copy)
    };

    let hmenu = CreatePopupMenu().unwrap();
    // ── 捕獲方式 ──
    let _ = AppendMenuW(hmenu, MF_STRING, CM_CAPTURE_REGION as usize, w!("框選區域 (Alt+Shift+R)"));
    let _ = AppendMenuW(hmenu, MF_STRING, CM_CAPTURE_ACTIVE as usize, w!("作用中視窗 (Alt+Shift+A)"));
    let _ = AppendMenuW(hmenu, MF_STRING, CM_CAPTURE_PICK   as usize, w!("點選視窗 (Alt+Shift+W)"));
    let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, None);
    // ── 設定 ──
    let cf = if capture_cursor { MF_STRING | MF_CHECKED } else { MF_STRING };
    let _ = AppendMenuW(hmenu, cf, CM_TOGGLE_CURSOR as usize, w!("擷取滑鼠游標"));
    let af = if auto_copy { MF_STRING | MF_CHECKED } else { MF_STRING };
    let _ = AppendMenuW(hmenu, af, CM_TOGGLE_AUTOCOPY as usize, w!("直接複製到剪貼簿"));
    // ── 延遲子選單 ──
    let delay_menu = CreatePopupMenu().unwrap();
    let _ = AppendMenuW(delay_menu, MF_STRING, CM_DELAY_0 as usize, w!("無延遲"));
    let _ = AppendMenuW(delay_menu, MF_STRING, CM_DELAY_1 as usize, w!("1 秒"));
    let _ = AppendMenuW(delay_menu, MF_STRING, CM_DELAY_2 as usize, w!("2 秒"));
    let _ = AppendMenuW(delay_menu, MF_STRING, CM_DELAY_3 as usize, w!("3 秒"));
    let _ = AppendMenuW(delay_menu, MF_STRING, CM_DELAY_5 as usize, w!("5 秒"));
    let is_preset = [0u32, 1, 2, 3, 5].contains(&delay_secs);
    let custom_label: Vec<u16> = if is_preset {
        "自訂...\0".encode_utf16().collect()
    } else {
        format!("自訂: {} 秒...\0", delay_secs).encode_utf16().collect()
    };
    let _ = AppendMenuW(delay_menu, MF_STRING, CM_DELAY_CUSTOM as usize,
        windows::core::PCWSTR(custom_label.as_ptr()));
    let current_delay_id = match delay_secs { 1=>CM_DELAY_1, 2=>CM_DELAY_2, 3=>CM_DELAY_3, 5=>CM_DELAY_5, _=>CM_DELAY_0 };
    let _ = CheckMenuRadioItem(delay_menu, CM_DELAY_0, CM_DELAY_5, current_delay_id, MF_BYCOMMAND.0);
    if !is_preset { let _ = CheckMenuItem(delay_menu, CM_DELAY_CUSTOM, MF_BYCOMMAND.0 | MF_CHECKED.0); }
    let _ = AppendMenuW(hmenu, MF_POPUP, delay_menu.0 as usize, w!("延遲擷取"));
    let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, None);
    let _ = AppendMenuW(hmenu, MF_STRING, CM_QUIT as usize, w!("結束"));

    SetForegroundWindow(hwnd).ok();
    TrackPopupMenu(hmenu, TPM_RIGHTBUTTON, sx, sy, 0, hwnd, None);
    let _ = DestroyMenu(hmenu);
}

/// 處理來自右鍵選單的 WM_COMMAND
unsafe fn handle_context_menu_cmd(hwnd: HWND, state: &mut EditorState, id: u32) {
    match id {
        CM_CAPTURE_REGION => { let _ = state.tx.send(crate::event::AppEvent::CaptureRegion); }
        CM_CAPTURE_ACTIVE => { let _ = state.tx.send(crate::event::AppEvent::CaptureActiveWindow); }
        CM_CAPTURE_PICK   => { let _ = state.tx.send(crate::event::AppEvent::CapturePickWindow); }
        CM_TOGGLE_CURSOR  => {
            let mut c = state.config.lock().unwrap();
            c.capture_cursor = !c.capture_cursor;
            crate::config::persist_settings(&c);
        }
        CM_TOGGLE_AUTOCOPY => {
            let mut c = state.config.lock().unwrap();
            c.auto_copy = !c.auto_copy;
            crate::config::persist_settings(&c);
        }
        CM_TOGGLE_HIDE_ON_CAPTURE => {
            let mut c = state.config.lock().unwrap();
            c.hide_editor_on_capture = !c.hide_editor_on_capture;
            crate::config::persist_settings(&c);
        }
        CM_DELAY_0 | CM_DELAY_1 | CM_DELAY_2 | CM_DELAY_3 | CM_DELAY_5 => {
            let mut c = state.config.lock().unwrap();
            c.capture_delay_secs = match id {
                CM_DELAY_1 => 1, CM_DELAY_2 => 2, CM_DELAY_3 => 3, CM_DELAY_5 => 5, _ => 0,
            };
            crate::config::persist_settings(&c);
        }
        CM_DELAY_CUSTOM => {
            // 重用自訂延遲輸入框
            let current = state.config.lock().unwrap().capture_delay_secs;
            // 用 custom_color_input_dialog 的樣板建立數字輸入
            let config_arc = state.config.clone();
            let class = w!("srcshot_delaydlg2");
            let hinstance = get_instance();
            struct DS { value: u32, confirmed: bool, done: bool }
            unsafe extern "system" fn dp(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
                match msg {
                    WM_NCCREATE => { let cs = &*(lp.0 as *const CREATESTRUCTW); SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as _); LRESULT(1) }
                    WM_CREATE => {
                        let hi = {
                            use windows::Win32::System::LibraryLoader::GetModuleHandleW;
                            windows::Win32::Foundation::HINSTANCE::from(GetModuleHandleW(None).unwrap())
                        };
                        CreateWindowExW(Default::default(), w!("STATIC"), w!("延遲秒數（0–99）："), WS_CHILD|WS_VISIBLE, 8,10,200,18, hwnd, HMENU(std::ptr::null_mut()), hi, None).ok();
                        let edit = CreateWindowExW(WS_EX_CLIENTEDGE, w!("EDIT"), w!(""), WS_CHILD|WS_VISIBLE|WINDOW_STYLE(0x2000u32), 8,32,80,24, hwnd, HMENU(1usize as _), hi, None).unwrap();
                        CreateWindowExW(Default::default(), w!("BUTTON"), w!("確定"), WS_CHILD|WS_VISIBLE|WINDOW_STYLE(BS_DEFPUSHBUTTON as u32), 8,64,80,28, hwnd, HMENU(2usize as _), hi, None).ok();
                        CreateWindowExW(Default::default(), w!("BUTTON"), w!("取消"), WS_CHILD|WS_VISIBLE, 96,64,80,28, hwnd, HMENU(3usize as _), hi, None).ok();
                        let s = &*(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const DS);
                        let pre: Vec<u16> = format!("{}\0", s.value).encode_utf16().collect();
                        SetWindowTextW(edit, windows::core::PCWSTR(pre.as_ptr())).ok();
                        PostMessageW(hwnd, WM_NEXTDLGCTL, WPARAM(edit.0 as usize), LPARAM(1)).ok();
                        LRESULT(0)
                    }
                    WM_COMMAND if (wp.0 & 0xFFFF) == 2 => {
                        let s = &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut DS);
                        if let Ok(edit) = GetDlgItem(hwnd, 1) {
                            let n = GetWindowTextLengthW(edit) + 1;
                            let mut buf = vec![0u16; n as usize];
                            GetWindowTextW(edit, &mut buf);
                            s.value = String::from_utf16_lossy(&buf).trim_end_matches('\0').parse::<u32>().unwrap_or(0).min(99);
                        }
                        s.confirmed = true; s.done = true; DestroyWindow(hwnd).ok(); LRESULT(0)
                    }
                    WM_COMMAND if (wp.0 & 0xFFFF) == 3 => { let s = &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut DS); s.done = true; DestroyWindow(hwnd).ok(); LRESULT(0) }
                    WM_CLOSE => { let s = &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut DS); s.done = true; DestroyWindow(hwnd).ok(); LRESULT(0) }
                    WM_DESTROY => LRESULT(0),
                    _ => DefWindowProcW(hwnd, msg, wp, lp),
                }
            }
            let wc = WNDCLASSEXW { cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32, lpfnWndProc: Some(dp), hInstance: hinstance, lpszClassName: class, ..Default::default() };
            let _ = RegisterClassExW(&wc);
            let mut ds = DS { value: current, confirmed: false, done: false };
            let dlg = CreateWindowExW(WS_EX_DLGMODALFRAME|WS_EX_TOPMOST, class, w!("自訂延遲秒數"), WS_OVERLAPPED|WS_CAPTION|WS_SYSMENU|WS_VISIBLE, CW_USEDEFAULT, CW_USEDEFAULT, 200, 140, hwnd, HMENU(std::ptr::null_mut()), hinstance, Some(&mut ds as *mut _ as _)).unwrap_or(HWND(std::ptr::null_mut()));
            if !dlg.0.is_null() {
                let mut msg = MSG::default();
                while GetMessageW(&mut msg, HWND(std::ptr::null_mut()), 0, 0).as_bool() {
                    if msg.message == WM_KEYDOWN && msg.wParam.0 == VK_RETURN.0 as usize {
                        if let Ok(edit) = GetDlgItem(dlg, 1) {
                            let n = GetWindowTextLengthW(edit) + 1;
                            let mut buf = vec![0u16; n as usize];
                            GetWindowTextW(edit, &mut buf);
                            ds.value = String::from_utf16_lossy(&buf).trim_end_matches('\0').parse::<u32>().unwrap_or(0).min(99);
                        }
                        ds.confirmed = true; ds.done = true; DestroyWindow(dlg).ok(); break;
                    }
                    if IsDialogMessageW(dlg, &msg).as_bool() { if ds.done { break; } continue; }
                    let _ = TranslateMessage(&msg); DispatchMessageW(&msg);
                    if ds.done { break; }
                }
            }
            let _ = UnregisterClassW(class, hinstance);
            if ds.confirmed {
                let mut c = config_arc.lock().unwrap();
                c.capture_delay_secs = ds.value;
                crate::config::persist_settings(&c);
            }
        }
        CM_QUIT => { let _ = state.tx.send(crate::event::AppEvent::TrayQuit); }
        _ => {}
    }
}

fn get_instance() -> windows::Win32::Foundation::HINSTANCE {
    unsafe {
        windows::Win32::System::LibraryLoader::GetModuleHandleW(None)
            .unwrap()
            .into()
    }
}
