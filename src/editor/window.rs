use std::sync::mpsc::Sender;
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Foundation::COLORREF;
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC,
    CreatePen, CreateSolidBrush, DeleteDC, DeleteObject, DrawTextW,
    EndPaint, FillRect, GetDC, GetStockObject, InvalidateRect,
    Arc as GdiArc, LineTo, MoveToEx, NULL_BRUSH, Polygon, Polyline,
    Rectangle as GdiRectangle, ReleaseDC, RoundRect, SelectObject, SetBkMode,
    BACKGROUND_MODE, DEFAULT_GUI_FONT, DRAW_TEXT_FORMAT, PAINTSTRUCT,
    PS_SOLID, SRCCOPY,
};
use windows::Win32::UI::Controls::{DRAWITEMSTRUCT, SetScrollInfo};
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, SetFocus, VK_ESCAPE, VK_RETURN};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::capture::screen::ScreenBitmap;
use crate::event::AppEvent;
use super::canvas::Canvas;
use super::tool::{Stroke, Tool};

const TOOLBAR_H: i32 = 48;
const TOOLBAR_BG: u32 = 0x00_F0_F0_F0; // 工具列背景色（沉浸式風格）
const BTN_W: i32 = 40;
const BTN_H: i32 = 36;
const BTN_MARGIN: i32 = 4;

const BTN_PEN: usize   = 10;
const BTN_ARROW: usize = 11;
const BTN_RECT: usize  = 12;
const BTN_TEXT: usize  = 13;
const BTN_CROP:  usize = 14;
const BTN_COLOR: usize = 15;
const BTN_COPY:  usize = 20;
const BTN_SAVE: usize  = 21;
const BTN_UNDO: usize  = 22;

struct EditorState {
    tx: Sender<AppEvent>,
    canvas: Canvas,
    active_tool: Tool,
    dragging: bool,
    drag_start: POINT,
    save_dir: std::path::PathBuf,
    result_sent: bool,
    scroll_x: i32,
    scroll_y: i32,
    hovering_canvas: bool, // 游標是否在畫布區（工具列以下）
    tooltip: HWND,         // 自製 tooltip 視窗
    hover_btn: i32,        // 目前 hover 的按鈕索引（0-6），-1 = 無
    hover_ticks: i32,      // 連續 hover 相同按鈕的 poll 次數（用於延遲顯示）
}

pub fn open(bmp: ScreenBitmap, tx: Sender<AppEvent>, save_dir: std::path::PathBuf) {
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

        // 視窗初始大小：不小於工具列按鈕所需寬度，不超過畫面的 90%
        let screen_w = GetSystemMetrics(SM_CXSCREEN);
        let screen_h = GetSystemMetrics(SM_CYSCREEN);
        // 9 個按鈕的最小寬度：BTN_MARGIN + 9×(BTN_W+BTN_MARGIN)
        let min_w = BTN_MARGIN + 9 * (BTN_W + BTN_MARGIN) + 20;
        let min_h = TOOLBAR_H + 120; // 工具列 + 最小畫布顯示高度
        let win_w = (canvas.width + 20).max(min_w).min(screen_w * 9 / 10);
        let win_h = (canvas.height + TOOLBAR_H + 45).max(min_h).min(screen_h * 9 / 10);

        let state = Box::new(EditorState {
            tx,
            canvas,
            active_tool: Tool::Pen,
            dragging: false,
            drag_start: POINT { x: 0, y: 0 },
            save_dir,
            result_sent: false,
            scroll_x: 0,
            scroll_y: 0,
            hovering_canvas: false,
            tooltip: HWND(std::ptr::null_mut()),
            hover_btn: -1,
            hover_ticks: 0,
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

        create_toolbar(hwnd);

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
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            tip_class, w!(""),
            WS_POPUP | WS_BORDER,
            0, 0, 70, 22,
            hwnd, HMENU(std::ptr::null_mut()), hinstance, None,
        ).unwrap_or(HWND(std::ptr::null_mut()));
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
    for (i, id) in [BTN_PEN, BTN_ARROW, BTN_RECT, BTN_TEXT, BTN_CROP, BTN_COLOR, BTN_COPY, BTN_SAVE, BTN_UNDO]
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
    let need_h = state.canvas.width  > rc.right;
    let need_v = state.canvas.height > (rc.bottom - TOOLBAR_H);

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
            nMax: state.canvas.width - 1,
            nPage: client_w as u32,
            nPos: state.scroll_x,
            nTrackPos: 0,
        };
        SetScrollInfo(hwnd, SB_HORZ, &si, true);
    }

    if need_v {
        let client_h = (rc.bottom - TOOLBAR_H).max(1);
        let si = SCROLLINFO {
            cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
            fMask: SIF_ALL,
            nMin: 0,
            nMax: state.canvas.height - 1,
            nPage: client_h as u32,
            nPos: state.scroll_y,
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
                0 => state.scroll_x - 20,        // SB_LINELEFT
                1 => state.scroll_x + 20,        // SB_LINERIGHT
                2 => state.scroll_x - client_w,  // SB_PAGELEFT
                3 => state.scroll_x + client_w,  // SB_PAGERIGHT
                5 => si.nTrackPos,               // SB_THUMBTRACK
                _ => state.scroll_x,
            };
            state.scroll_x = clamp_scroll(new_x, state.canvas.width, client_w);
            update_scrollbars(hwnd, state);
            InvalidateRect(hwnd, None, false);
            LRESULT(0)
        }
        WM_VSCROLL => {
            let state = &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut EditorState);
            let mut rc = RECT::default();
            GetClientRect(hwnd, &mut rc).unwrap();
            let client_h = rc.bottom - TOOLBAR_H;
            let code = (wp.0 & 0xFFFF) as u32;
            let mut si = SCROLLINFO {
                cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
                fMask: SIF_ALL,
                ..Default::default()
            };
            GetScrollInfo(hwnd, SB_VERT, &mut si);
            let new_y = match code {
                0 => state.scroll_y - 20,        // SB_LINEUP
                1 => state.scroll_y + 20,        // SB_LINEDOWN
                2 => state.scroll_y - client_h,  // SB_PAGEUP
                3 => state.scroll_y + client_h,  // SB_PAGEDOWN
                5 => si.nTrackPos,               // SB_THUMBTRACK
                _ => state.scroll_y,
            };
            state.scroll_y = clamp_scroll(new_y, state.canvas.height, client_h);
            update_scrollbars(hwnd, state);
            InvalidateRect(hwnd, None, false);
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            let state = &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut EditorState);
            let delta = ((wp.0 >> 16) as u16) as i16;
            let mut rc = RECT::default();
            GetClientRect(hwnd, &mut rc).unwrap();
            let client_h = rc.bottom - TOOLBAR_H;
            let step = 60;
            let new_y = if delta > 0 {
                state.scroll_y - step
            } else {
                state.scroll_y + step
            };
            state.scroll_y = clamp_scroll(new_y, state.canvas.height, client_h);
            update_scrollbars(hwnd, state);
            InvalidateRect(hwnd, None, false);
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
                        custom_color_input_dialog(hwnd, state.canvas.tool_color)
                    } else {
                        chosen
                    };
                    if let Some(color) = final_color {
                        state.canvas.tool_color = color;
                        if let Ok(btn) = GetDlgItem(hwnd, BTN_COLOR as i32) {
                            InvalidateRect(btn, None, false);
                        }
                    }
                    SetFocus(hwnd);
                }
                BTN_UNDO  => {
                    state.canvas.undo();
                    // 裁切復原後畫布可能變大，捲軸需重算
                    state.scroll_x = state.scroll_x.min((state.canvas.width  - 1).max(0));
                    state.scroll_y = state.scroll_y.min((state.canvas.height - 1).max(0));
                    update_scrollbars(hwnd, state);
                    InvalidateRect(hwnd, None, false);
                    SetFocus(hwnd);
                }
                BTN_COPY => {
                    let flat = state.canvas.flatten_to_bitmap();
                    let _ = crate::output::clipboard::copy_to_clipboard(&flat);
                    let _ = state.tx.send(AppEvent::EditorSave { to_clipboard: true, path: None });
                    state.result_sent = true;
                    PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0)).unwrap();
                }
                BTN_SAVE => {
                    let flat = state.canvas.flatten_to_bitmap();
                    if let Some(path) = show_save_dialog(hwnd, &state.save_dir) {
                        let _ = crate::output::file::save_png(&flat, &path);
                        if let Some(parent) = path.parent() {
                            state.save_dir = parent.to_path_buf();
                            crate::config::persist_save_dir(&state.save_dir);
                        }
                        let _ = state.tx.send(AppEvent::EditorSave { to_clipboard: false, path: Some(path) });
                        state.result_sent = true;
                        PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0)).unwrap();
                    }
                    // 使用者取消對話框時不關閉編輯器
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_KEYDOWN if wp.0 == VK_ESCAPE.0 as usize => {
            let state = &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut EditorState);
            let _ = state.tx.send(AppEvent::EditorCancelled);
            state.result_sent = true;
            PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0)).unwrap();
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let state = &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut EditorState);
            // 點擊時隱藏 tooltip
            if !state.tooltip.0.is_null() { ShowWindow(state.tooltip, SW_HIDE); }
            state.hover_ticks = 0;
            let (cx, cy) = client_xy(lp);
            let cy_canvas = cy - TOOLBAR_H;
            if cy_canvas < 0 { return LRESULT(0); }
            // 加上 scroll offset 轉成 canvas 座標
            let pt = POINT { x: cx + state.scroll_x, y: cy_canvas + state.scroll_y };

            match state.active_tool {
                Tool::Text => {
                    let text = simple_input_dialog(hwnd);
                    if !text.is_empty() {
                        state.canvas.strokes.push((
                            Stroke::Text { pos: pt, text },
                            super::tool::Color(state.canvas.tool_color),
                            state.canvas.tool_thickness,
                        ));
                        InvalidateRect(hwnd, None, false);
                    }
                }
                _ => {
                    state.dragging = true;
                    state.drag_start = pt;
                    if state.active_tool == Tool::Pen {
                        state.canvas.current = Some(Stroke::Pen { points: vec![pt] });
                    }
                    SetCapture(hwnd);
                }
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let state = &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut EditorState);
            let (_, cy_mm) = client_xy(lp);
            state.hovering_canvas = cy_mm >= TOOLBAR_H;
            if !state.dragging { return LRESULT(0); }
            let (cx, cy) = client_xy(lp);
            let pt = POINT {
                x: cx + state.scroll_x,
                y: (cy - TOOLBAR_H) + state.scroll_y,
            };
            match state.active_tool {
                Tool::Pen => {
                    if let Some(Stroke::Pen { ref mut points }) = state.canvas.current {
                        points.push(pt);
                    }
                }
                Tool::Arrow => {
                    state.canvas.current = Some(Stroke::Arrow { from: state.drag_start, to: pt });
                }
                Tool::Rect | Tool::Crop => {
                    let s = state.drag_start;
                    state.canvas.current = Some(Stroke::Rect {
                        r: RECT {
                            left: s.x.min(pt.x), top: s.y.min(pt.y),
                            right: s.x.max(pt.x), bottom: s.y.max(pt.y),
                        },
                    });
                }
                _ => {}
            }
            InvalidateRect(hwnd, None, false);
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let state = &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut EditorState);
            if state.dragging {
                state.dragging = false;
                ReleaseCapture().unwrap();
                if state.active_tool == Tool::Crop {
                    // 取得裁切矩形後直接套用
                    if let Some(Stroke::Rect { r }) = state.canvas.current.take() {
                        if r.right - r.left > 4 && r.bottom - r.top > 4 {
                            state.canvas.crop(r);
                            state.scroll_x = 0;
                            state.scroll_y = 0;
                        }
                    }
                    state.canvas.current = None;
                } else if let Some(stroke) = state.canvas.current.take() {
                    state.canvas.push_stroke(
                        stroke,
                        super::tool::Color(state.canvas.tool_color),
                        state.canvas.tool_thickness,
                    );
                }
                update_scrollbars(hwnd, state);
                InvalidateRect(hwnd, None, false);
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
            let client_h = (rc.bottom - TOOLBAR_H).max(0);

            // 工具列背景（沉浸式風格：與按鈕非作用態同色）
            let tb = CreateSolidBrush(COLORREF(TOOLBAR_BG));
            FillRect(hdc, &RECT { left: 0, top: 0, right: client_w.max(1), bottom: TOOLBAR_H }, tb);
            DeleteObject(tb);

            let screen_dc = GetDC(HWND(std::ptr::null_mut()));

            // ── 雙緩衝：canvas 區域（工具列以下）──────────────────
            let buf_dc  = CreateCompatibleDC(screen_dc);
            let buf_bmp = CreateCompatibleBitmap(screen_dc, client_w.max(1), client_h.max(1));
            let old_buf = SelectObject(buf_dc, buf_bmp);

            // 用灰色填滿整個 buffer，超出 canvas 的區域自然呈灰色
            let gray = CreateSolidBrush(windows::Win32::Foundation::COLORREF(0x00_B0_B0_B0));
            FillRect(buf_dc, &RECT { left: 0, top: 0, right: client_w.max(1), bottom: client_h.max(1) }, gray);
            DeleteObject(gray);

            // 繪製 canvas 到獨立 DC
            let canvas_dc  = CreateCompatibleDC(screen_dc);
            let canvas_bmp = CreateCompatibleBitmap(screen_dc, state.canvas.width, state.canvas.height);
            let old_canvas = SelectObject(canvas_dc, canvas_bmp);
            state.canvas.render(canvas_dc, screen_dc);

            // 計算 canvas 在 client 中的可見像素數（scroll 後剩多少）
            let vis_w = (state.canvas.width  - state.scroll_x).min(client_w).max(0);
            let vis_h = (state.canvas.height - state.scroll_y).min(client_h).max(0);
            if vis_w > 0 && vis_h > 0 {
                BitBlt(buf_dc, 0, 0, vis_w, vis_h, canvas_dc, state.scroll_x, state.scroll_y, SRCCOPY).unwrap();
            }

            SelectObject(canvas_dc, old_canvas);
            DeleteObject(canvas_bmp);
            DeleteDC(canvas_dc);

            // 一次性 BitBlt 到畫面（工具列以下）
            BitBlt(hdc, 0, TOOLBAR_H, client_w, client_h, buf_dc, 0, 0, SRCCOPY).unwrap();

            SelectObject(buf_dc, old_buf);
            DeleteObject(buf_bmp);
            DeleteDC(buf_dc);
            ReleaseDC(HWND(std::ptr::null_mut()), screen_dc);

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
            let text_color = match id {
                _ if is_active_tool || is_pressed => windows::Win32::Foundation::COLORREF(0x00_FF_FF_FF),
                _ => windows::Win32::Foundation::COLORREF(0x00_40_40_40),
            };

            let hdc = dis.hDC;
            let rc = dis.rcItem;

            // 繪製圓角矩形背景
            let pen   = CreatePen(PS_SOLID, 0, bg);
            let brush = CreateSolidBrush(bg);
            let old_p = SelectObject(hdc, pen);
            let old_b = SelectObject(hdc, brush);
            RoundRect(hdc, rc.left, rc.top, rc.right, rc.bottom, 8, 8);
            SelectObject(hdc, old_p);
            SelectObject(hdc, old_b);
            DeleteObject(pen);
            DeleteObject(brush);

            // 圖示（以 GDI 繪製，置中於按鈕）
            let cx = (rc.left + rc.right) / 2;
            let cy = (rc.top + rc.bottom) / 2;
            let ip = CreatePen(PS_SOLID, 2, text_color);
            let ib = CreateSolidBrush(text_color);
            let op = SelectObject(hdc, ip);
            let ob = SelectObject(hdc, ib);
            let nb = GetStockObject(NULL_BRUSH);

            match id {
                BTN_PEN => {
                    // 斜線筆身 + 三角筆尖
                    let _ = Polyline(hdc, &[POINT{x:cx-8,y:cy-8}, POINT{x:cx+4,y:cy+4}]);
                    let _ = Polygon(hdc, &[
                        POINT{x:cx+4,y:cy+4}, POINT{x:cx+9,y:cy+1}, POINT{x:cx+1,y:cy+9},
                    ]);
                }
                BTN_ARROW => {
                    // 水平線 + 右箭頭
                    let _ = Polyline(hdc, &[POINT{x:cx-10,y:cy}, POINT{x:cx+3,y:cy}]);
                    let _ = Polygon(hdc, &[
                        POINT{x:cx+3,y:cy-6}, POINT{x:cx+11,y:cy}, POINT{x:cx+3,y:cy+6},
                    ]);
                }
                BTN_RECT => {
                    // 空心矩形
                    let o = SelectObject(hdc, nb);
                    let _ = GdiRectangle(hdc, cx-10, cy-7, cx+10, cy+7);
                    SelectObject(hdc, o);
                }
                BTN_TEXT => {
                    let _ = MoveToEx(hdc, cx-9, cy-8, None); let _ = LineTo(hdc, cx+9, cy-8);
                    let _ = MoveToEx(hdc, cx,   cy-8, None); let _ = LineTo(hdc, cx,   cy+8);
                }
                BTN_COLOR => {
                    // 底部粗色條（帶淺灰邊框確保白色也可見）+ 小 ▼
                    let bar_l = rc.left + 5;
                    let bar_r = rc.right - 5;
                    let bar_t = rc.bottom - 9;
                    let bar_b = rc.bottom - 4;
                    // 灰色邊框（比色條多 1px）
                    let border = CreateSolidBrush(COLORREF(0x00_A0_A0_A0));
                    FillRect(hdc, &RECT { left: bar_l - 1, top: bar_t - 1,
                        right: bar_r + 1, bottom: bar_b + 1 }, border);
                    DeleteObject(border);
                    // 彩色條
                    let cb = CreateSolidBrush(COLORREF(state.canvas.tool_color));
                    FillRect(hdc, &RECT { left: bar_l, top: bar_t, right: bar_r, bottom: bar_b }, cb);
                    DeleteObject(cb);
                    // 小 ▼（在色條正上方居中）
                    let ap = CreatePen(PS_SOLID, 1, COLORREF(0x00_40_40_40));
                    let ab = CreateSolidBrush(COLORREF(0x00_40_40_40));
                    let top = SelectObject(hdc, ap);
                    let tob = SelectObject(hdc, ab);
                    let _ = Polygon(hdc, &[
                        POINT { x: cx - 4, y: cy - 2 },
                        POINT { x: cx + 4, y: cy - 2 },
                        POINT { x: cx,     y: cy + 2 },
                    ]);
                    SelectObject(hdc, top); SelectObject(hdc, tob);
                    DeleteObject(ap); DeleteObject(ab);
                }
                BTN_CROP => {
                    // 經典裁切圖示：矩形 + 四角延伸線
                    let o = SelectObject(hdc, GetStockObject(NULL_BRUSH));
                    let _ = GdiRectangle(hdc, cx-7, cy-7, cx+7, cy+7);
                    SelectObject(hdc, o);
                    let _ = MoveToEx(hdc, cx-7, cy-11, None); let _ = LineTo(hdc, cx-7, cy-7);
                    let _ = MoveToEx(hdc, cx-11,cy-7,  None); let _ = LineTo(hdc, cx-7, cy-7);
                    let _ = MoveToEx(hdc, cx+7, cy-11, None); let _ = LineTo(hdc, cx+7, cy-7);
                    let _ = MoveToEx(hdc, cx+11,cy-7,  None); let _ = LineTo(hdc, cx+7, cy-7);
                    let _ = MoveToEx(hdc, cx-7, cy+11, None); let _ = LineTo(hdc, cx-7, cy+7);
                    let _ = MoveToEx(hdc, cx-11,cy+7,  None); let _ = LineTo(hdc, cx-7, cy+7);
                    let _ = MoveToEx(hdc, cx+7, cy+11, None); let _ = LineTo(hdc, cx+7, cy+7);
                    let _ = MoveToEx(hdc, cx+11,cy+7,  None); let _ = LineTo(hdc, cx+7, cy+7);
                }
                BTN_COPY => {
                    // 兩個偏移的空心矩形
                    let o = SelectObject(hdc, nb);
                    let _ = GdiRectangle(hdc, cx-9, cy-3, cx+4, cy+9);
                    let _ = GdiRectangle(hdc, cx-4, cy-9, cx+9, cy+3);
                    SelectObject(hdc, o);
                }
                BTN_SAVE => {
                    // 向下箭頭 + 底線
                    let _ = MoveToEx(hdc, cx, cy-9, None); let _ = LineTo(hdc, cx, cy);
                    let _ = Polygon(hdc, &[
                        POINT{x:cx-7,y:cy}, POINT{x:cx,y:cy+8}, POINT{x:cx+7,y:cy},
                    ]);
                    let _ = MoveToEx(hdc, cx-10, cy+10, None); let _ = LineTo(hdc, cx+10, cy+10);
                }
                BTN_UNDO => {
                    // 上半圓弧（從右到左，逆時針）= 標準 undo 外形
                    let _ = GdiArc(hdc, cx-9, cy-8, cx+9, cy+8,
                        cx+9, cy,   // 起點：右中
                        cx-9, cy);  // 終點：左中
                    // 箭頭尖端朝下（弧線在左中結束，方向向下）
                    let _ = Polygon(hdc, &[
                        POINT{x:cx-9,  y:cy+6}, // 尖端
                        POINT{x:cx-14, y:cy},   // 左底
                        POINT{x:cx-4,  y:cy},   // 右底
                    ]);
                }
                _ => {}
            }

            SelectObject(hdc, op); SelectObject(hdc, ob);
            DeleteObject(ip); DeleteObject(ib);

            LRESULT(1)
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
                    BTN_CROP  => 4, BTN_COLOR => 5, BTN_COPY  => 6, BTN_SAVE  => 7,
                    BTN_UNDO  => 8, _ => -1,
                }
            };
            if btn_hover != state.hover_btn {
                state.hover_btn = btn_hover;
                state.hover_ticks = 0;
                if !state.tooltip.0.is_null() { ShowWindow(state.tooltip, SW_HIDE); }
            } else if btn_hover >= 0 {
                state.hover_ticks += 1;
                if state.hover_ticks == 5 { // 5×100ms = 500ms 後顯示
                    let labels = ["筆","箭頭","矩形","文字","裁切","顏色","複製","儲存","復原"];
                    if let Some(label) = labels.get(btn_hover as usize) {
                        let text: Vec<u16> = label.encode_utf16().chain(Some(0)).collect();
                        SetWindowTextW(state.tooltip, windows::core::PCWSTR(text.as_ptr())).ok();
                        InvalidateRect(state.tooltip, None, false);
                        SetWindowPos(state.tooltip, HWND_TOPMOST,
                            pt.x + 4, pt.y + 20, 72, 22, SWP_SHOWWINDOW | SWP_NOZORDER).ok();
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
        WM_DESTROY => {
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut EditorState;
            if !ptr.is_null() {
                let state = &mut *ptr;
                if !state.result_sent {
                    let _ = state.tx.send(AppEvent::EditorCancelled);
                }
                if !state.tooltip.0.is_null() { DestroyWindow(state.tooltip).ok(); }
                drop(Box::from_raw(ptr));
            }
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
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
unsafe fn show_save_dialog(owner: HWND, initial_dir: &std::path::Path) -> Option<std::path::PathBuf> {
    use windows::Win32::System::Com::{CoCreateInstance, IBindCtx, CLSCTX_INPROC_SERVER};
    use windows::Win32::UI::Shell::{
        FileSaveDialog, IFileSaveDialog, IShellItem, SHCreateItemFromParsingName, SIGDN_FILESYSPATH,
    };

    let dialog: IFileSaveDialog =
        CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER).ok()?;

    // 預設副檔名（若使用者未輸入則自動補上）
    let _ = dialog.SetDefaultExtension(w!("png"));

    // 預設檔名（時間戳記）
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let fname: Vec<u16> = format!("srcshot_{}.png\0", ts).encode_utf16().collect();
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

fn get_instance() -> windows::Win32::Foundation::HINSTANCE {
    unsafe {
        windows::Win32::System::LibraryLoader::GetModuleHandleW(None)
            .unwrap()
            .into()
    }
}
