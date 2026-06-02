use std::sync::mpsc::Sender;
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC,
    CreatePen, CreateSolidBrush, DeleteDC, DeleteObject, DrawTextW,
    EndPaint, FillRect, GetDC, GetStockObject, InvalidateRect,
    ReleaseDC, RoundRect, SelectObject, SetBkMode, SetTextColor,
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
const BTN_W: i32 = 80;
const BTN_H: i32 = 36;
const BTN_MARGIN: i32 = 8;

const BTN_PEN: usize   = 10;
const BTN_ARROW: usize = 11;
const BTN_RECT: usize  = 12;
const BTN_TEXT: usize  = 13;
const BTN_COPY: usize  = 20;
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
        // 7 個按鈕的最小寬度：BTN_MARGIN + 7×(BTN_W+BTN_MARGIN)
        let min_w = BTN_MARGIN + 7 * (BTN_W + BTN_MARGIN) + 20;
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
        SetForegroundWindow(hwnd).ok();

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, HWND(std::ptr::null_mut()), 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        DestroyIcon(app_icon).ok();
        let _ = UnregisterClassW(class, hinstance);
        CoUninitialize();
    }
}

unsafe fn create_toolbar(parent: HWND) {
    let hinstance = get_instance();
    let buttons: &[(&windows::core::PCWSTR, usize)] = &[
        (&w!("筆"),   BTN_PEN),
        (&w!("箭頭"), BTN_ARROW),
        (&w!("矩形"), BTN_RECT),
        (&w!("文字"), BTN_TEXT),
        (&w!("複製"), BTN_COPY),
        (&w!("儲存"), BTN_SAVE),
        (&w!("復原"), BTN_UNDO),
    ];
    for (i, (label, id)) in buttons.iter().enumerate() {
        let x = BTN_MARGIN + i as i32 * (BTN_W + BTN_MARGIN);
        CreateWindowExW(
            Default::default(),
            w!("BUTTON"),
            **label,
            WS_CHILD | WS_VISIBLE | WINDOW_STYLE(0x0000000Bu32), // BS_OWNERDRAW
            x, 6, BTN_W, BTN_H,
            parent,
            HMENU(*id as *mut _),
            hinstance,
            None,
        )
        .unwrap();
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
                BTN_PEN | BTN_ARROW | BTN_RECT | BTN_TEXT => {
                    state.active_tool = match id {
                        BTN_PEN   => Tool::Pen,
                        BTN_ARROW => Tool::Arrow,
                        BTN_RECT  => Tool::Rect,
                        _         => Tool::Text,
                    };
                    // 強制所有工具按鈕重繪，使 active 高亮即時更新
                    for bid in [BTN_PEN, BTN_ARROW, BTN_RECT, BTN_TEXT] {
                        if let Ok(btn) = GetDlgItem(hwnd, bid as i32) {
                            InvalidateRect(btn, None, false);
                        }
                    }
                    // 將焦點還給編輯器視窗，確保 ESC 等鍵盤事件正常觸發
                    SetFocus(hwnd);
                }
                BTN_UNDO  => {
                    state.canvas.strokes.pop();
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
            let (_, cy_check) = client_xy(lp);
            state.hovering_canvas = cy_check >= TOOLBAR_H;
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
                Tool::Rect => {
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
                if let Some(stroke) = state.canvas.current.take() {
                    state.canvas.strokes.push((
                        stroke,
                        super::tool::Color(state.canvas.tool_color),
                        state.canvas.tool_thickness,
                    ));
                }
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
                (id == BTN_TEXT  && state.active_tool == Tool::Text);

            // COLORREF = 0x00BBGGRR
            let bg = if is_pressed {
                windows::Win32::Foundation::COLORREF(0x00_22_22_22)
            } else if is_active_tool {
                windows::Win32::Foundation::COLORREF(0x00_D4_78_00) // #0078D4 藍
            } else {
                match id {
                    BTN_COPY => windows::Win32::Foundation::COLORREF(0x00_80_80_00), // teal
                    BTN_SAVE => windows::Win32::Foundation::COLORREF(0x00_10_7C_10),
                    _        => windows::Win32::Foundation::COLORREF(0x00_E8_E8_E8),
                }
            };
            let text_color = match id {
                BTN_COPY | BTN_SAVE => windows::Win32::Foundation::COLORREF(0x00_FF_FF_FF),
                _ if is_active_tool || is_pressed => windows::Win32::Foundation::COLORREF(0x00_FF_FF_FF),
                _ => windows::Win32::Foundation::COLORREF(0x00_20_20_20),
            };

            let hdc = dis.hDC;
            let mut rc = dis.rcItem;

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

            // 文字（置中）
            SetBkMode(hdc, BACKGROUND_MODE(1)); // TRANSPARENT
            SetTextColor(hdc, text_color);
            let font = GetStockObject(DEFAULT_GUI_FONT);
            let old_f = SelectObject(hdc, font);
            let mut buf = [0u16; 32];
            let len = GetWindowTextW(dis.hwndItem, &mut buf);
            DrawTextW(
                hdc,
                &mut buf[..len as usize],
                &mut rc,
                DRAW_TEXT_FORMAT(0x0025), // DT_CENTER|DT_VCENTER|DT_SINGLELINE
            );
            SelectObject(hdc, old_f);

            LRESULT(1)
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

fn get_instance() -> windows::Win32::Foundation::HINSTANCE {
    unsafe {
        windows::Win32::System::LibraryLoader::GetModuleHandleW(None)
            .unwrap()
            .into()
    }
}
