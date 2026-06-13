use std::sync::mpsc::Sender;
use std::sync::atomic::{AtomicIsize, Ordering};
use windows::core::w;
use windows::Win32::Foundation::{BOOL, COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS,
};
use windows::Win32::Graphics::Gdi::{
    BACKGROUND_MODE, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION,
    BeginPaint, CreateCompatibleDC, CreateDIBSection, CreateFontW, CreatePen,
    CreateSolidBrush, DeleteDC, DeleteObject, DIB_RGB_COLORS, DRAW_TEXT_FORMAT,
    DrawTextW, Ellipse, EndPaint, FillRect,
    GetDC, GetStockObject, InvalidateRect, NULL_BRUSH, PAINTSTRUCT, PS_SOLID,
    ReleaseDC, ScreenToClient, SelectObject, SetBkMode, SetTextColor, UpdateWindow, HBRUSH,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, ReleaseCapture, SetCapture, VK_ESCAPE,
};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::event::AppEvent;

const WM_OVERLAY_CANCEL: u32 = WM_APP + 50;
const WM_OVERLAY_PICK_CLICK: u32 = WM_APP + 51;
static OVERLAY_CANCEL_HWND: AtomicIsize = AtomicIsize::new(0);
static PICK_MOUSE_HWND: AtomicIsize = AtomicIsize::new(0);

// ─── Region selection ────────────────────────────────────────────────────────

struct RegionState {
    tx: Sender<AppEvent>,
    dragging: bool,
    start_x: i32,
    start_y: i32,
    cur_x: i32,
    cur_y: i32,
}

pub fn show_region(tx: Sender<AppEvent>) {
    unsafe {
        let class = w!("srcshot_overlay_region");
        let hinstance = get_instance();

        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(region_wnd_proc),
            hInstance: hinstance,
            hCursor: LoadCursorW(None, IDC_CROSS).unwrap(),
            hbrBackground: HBRUSH(GetStockObject(NULL_BRUSH).0),
            lpszClassName: class,
            ..Default::default()
        };
        let _ = RegisterClassExW(&wc);

        let state = Box::new(RegionState {
            tx,
            dragging: false,
            start_x: 0,
            start_y: 0,
            cur_x: 0,
            cur_y: 0,
        });

        let x = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let y = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let w = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let h = GetSystemMetrics(SM_CYVIRTUALSCREEN);

        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED,
            class,
            w!(""),
            WS_POPUP | WS_VISIBLE,
            x, y, w, h,
            HWND(std::ptr::null_mut()),
            HMENU(std::ptr::null_mut()),
            hinstance,
            Some(Box::into_raw(state) as _),
        )
        .unwrap();

        let _ = SetLayeredWindowAttributes(
            hwnd,
            windows::Win32::Foundation::COLORREF(0),
            80,
            LWA_ALPHA,
        );

        // 立即設定游標，避免視窗建立瞬間顯示忙碌圖示
        SetCursor(LoadCursorW(None, IDC_CROSS).unwrap());
        SetCapture(hwnd);
        SetTimer(hwnd, 1, 50, None);
        let esc_hook = install_escape_hook(hwnd);
        run_modal();
        uninstall_escape_hook(esc_hook);
        let _ = UnregisterClassW(class, hinstance);
    }
}

unsafe extern "system" fn region_wnd_proc(
    hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM,
) -> LRESULT {
    match msg {
        WM_NCCREATE => {
            let cs = &*(lp.0 as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as _);
            LRESULT(1)
        }
        WM_SETCURSOR => {
            SetCursor(LoadCursorW(None, IDC_CROSS).unwrap());
            LRESULT(1)
        }
        WM_KEYDOWN if wp.0 == VK_ESCAPE.0 as usize => {
            let state = &*(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut RegionState);
            cancel_overlay(hwnd, &state.tx);
            LRESULT(0)
        }
        WM_OVERLAY_CANCEL => {
            let state = &*(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut RegionState);
            cancel_overlay(hwnd, &state.tx);
            LRESULT(0)
        }
        WM_TIMER => {
            let state = &*(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut RegionState);
            if escape_pressed() {
                cancel_overlay(hwnd, &state.tx);
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let state = &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut RegionState);
            let (x, y) = cursor_screen_pos();
            state.dragging = true;
            state.start_x = x;
            state.start_y = y;
            state.cur_x = x;
            state.cur_y = y;
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let state = &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut RegionState);
            if state.dragging {
                let (x, y) = cursor_screen_pos();
                state.cur_x = x;
                state.cur_y = y;
                InvalidateRect(hwnd, None, true);
                UpdateWindow(hwnd);
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let state = &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut RegionState);
            if state.dragging {
                state.dragging = false;
                let rect = normalise(state.start_x, state.start_y, state.cur_x, state.cur_y);
                ReleaseCapture().unwrap();
                // 先隱藏 overlay，讓螢幕恢復原狀後再截圖
                ShowWindow(hwnd, SW_HIDE);
                if rect.right - rect.left > 4 && rect.bottom - rect.top > 4 {
                    let _ = state.tx.send(AppEvent::RegionSelected(rect));
                } else {
                    let _ = state.tx.send(AppEvent::OverlayCancelled);
                }
                PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0)).unwrap();
            }
            LRESULT(0)
        }
        WM_PAINT => {
            let state = &*(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut RegionState);
            let mut ps = windows::Win32::Graphics::Gdi::PAINTSTRUCT::default();
            let hdc = windows::Win32::Graphics::Gdi::BeginPaint(hwnd, &mut ps);

            // 每次都先整個填黑，避免舊框線殘留
            let mut full = RECT::default();
            GetClientRect(hwnd, &mut full);
            let bg = CreateSolidBrush(windows::Win32::Foundation::COLORREF(0x00_00_00_00));
            FillRect(hdc, &full, bg);
            DeleteObject(bg);

            if state.dragging {
                let r = normalise(state.start_x, state.start_y, state.cur_x, state.cur_y);
                let ovx = GetSystemMetrics(SM_XVIRTUALSCREEN);
                let ovy = GetSystemMetrics(SM_YVIRTUALSCREEN);
                let l  = r.left   - ovx;
                let t  = r.top    - ovy;
                let ri = r.right  - ovx;
                let b2 = r.bottom - ovy;

                // 3px 白色邊框（LWA_ALPHA 混合後仍有足夠對比）
                const BW: i32 = 3;
                let white = CreateSolidBrush(COLORREF(0x00_FF_FF_FF));
                FillRect(hdc, &RECT { left: l, top: t,               right: ri, bottom: (t+BW).min(b2) }, white);
                FillRect(hdc, &RECT { left: l, top: (b2-BW).max(t),  right: ri, bottom: b2             }, white);
                FillRect(hdc, &RECT { left: l, top: t,               right: (l+BW).min(ri), bottom: b2 }, white);
                FillRect(hdc, &RECT { left: (ri-BW).max(l), top: t,  right: ri, bottom: b2             }, white);
                DeleteObject(white);

                // 1px 黑色外框，提升在亮色背景的對比
                let black = CreateSolidBrush(COLORREF(0));
                FillRect(hdc, &RECT { left: (l-1).max(0), top: (t-1).max(0), right: ri+1, bottom: t.max(0) }, black);
                FillRect(hdc, &RECT { left: (l-1).max(0), top: b2,           right: ri+1, bottom: b2+1      }, black);
                FillRect(hdc, &RECT { left: (l-1).max(0), top: (t-1).max(0), right: l,    bottom: b2+1      }, black);
                FillRect(hdc, &RECT { left: ri,           top: (t-1).max(0), right: ri+1, bottom: b2+1      }, black);
                DeleteObject(black);
            }
            windows::Win32::Graphics::Gdi::EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_DESTROY => {
            KillTimer(hwnd, 1);
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut RegionState;
            if !ptr.is_null() {
                drop(Box::from_raw(ptr));
            }
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

// ─── Window-pick overlay ─────────────────────────────────────────────────────

struct PickState {
    tx: Sender<AppEvent>,
    hover: HWND,
}

pub fn show_pick(tx: Sender<AppEvent>) {
    unsafe {
        let class = w!("srcshot_overlay_pick");
        let hinstance = get_instance();

        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(pick_wnd_proc),
            hInstance: hinstance,
            hCursor: LoadCursorW(None, IDC_CROSS).unwrap(),
            hbrBackground: HBRUSH(GetStockObject(NULL_BRUSH).0),
            lpszClassName: class,
            ..Default::default()
        };
        let _ = RegisterClassExW(&wc);

        let state = Box::new(PickState {
            tx,
            hover: HWND(std::ptr::null_mut()),
        });

        let vx = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let vy = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let vw = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let vh = GetSystemMetrics(SM_CYVIRTUALSCREEN);

        // 不需要 CS_HREDRAW|CS_VREDRAW，因為用 UpdateLayeredWindow 不走 WM_PAINT
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED,
            class,
            w!(""),
            WS_POPUP,                              // 不帶 WS_VISIBLE，先畫完再顯示
            vx, vy, vw, vh,
            HWND(std::ptr::null_mut()),
            HMENU(std::ptr::null_mut()),
            hinstance,
            Some(Box::into_raw(state) as _),
        )
        .unwrap();

        // 初始聚光燈（無選取視窗，全暗）
        pick_update_overlay(hwnd, HWND(std::ptr::null_mut()));
        ShowWindow(hwnd, SW_SHOW);
        // 讓 overlay 成為前景視窗，確保 SetCapture 有效（前景視窗才能全域捕獲滑鼠）
        SetForegroundWindow(hwnd).ok();

        SetCursor(LoadCursorW(None, IDC_CROSS).unwrap());
        SetCapture(hwnd);
        SetTimer(hwnd, 1, 50, None); // 50ms 備援輪詢
        let esc_hook = install_escape_hook(hwnd);
        let mouse_hook = install_pick_mouse_hook(hwnd);
        run_modal();
        uninstall_pick_mouse_hook(mouse_hook);
        uninstall_escape_hook(esc_hook);
        let _ = UnregisterClassW(class, hinstance);
    }
}

unsafe extern "system" fn pick_wnd_proc(
    hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM,
) -> LRESULT {
    match msg {
        WM_NCCREATE => {
            let cs = &*(lp.0 as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as _);
            LRESULT(1)
        }
        WM_SETCURSOR => {
            SetCursor(LoadCursorW(None, IDC_CROSS).unwrap());
            LRESULT(1)
        }
        WM_KEYDOWN if wp.0 == VK_ESCAPE.0 as usize => {
            let state = &*(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut PickState);
            cancel_overlay(hwnd, &state.tx);
            LRESULT(0)
        }
        WM_OVERLAY_CANCEL => {
            let state = &*(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut PickState);
            cancel_overlay(hwnd, &state.tx);
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let state = &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut PickState);
            let (sx, sy) = cursor_screen_pos();
            let pt = POINT { x: sx, y: sy };
            let target = find_pick_target_at(hwnd, pt);

            if target != state.hover {
                state.hover = target;
                pick_update_overlay(hwnd, target);
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let state = &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut PickState);
            finish_pick(hwnd, state);
            LRESULT(0)
        }
        WM_OVERLAY_PICK_CLICK => {
            let state = &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut PickState);
            finish_pick(hwnd, state);
            LRESULT(0)
        }
        WM_TIMER => {
            let state = &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut PickState);
            if escape_pressed() {
                cancel_overlay(hwnd, &state.tx);
                return LRESULT(0);
            }
            // 備援輪詢：更新 hover（當 WM_MOUSEMOVE 因某原因未送達時的後備）
            let (sx, sy) = cursor_screen_pos();
            let pt = POINT { x: sx, y: sy };
            let target = find_pick_target_at(hwnd, pt);
            if target != state.hover {
                state.hover = target;
                pick_update_overlay(hwnd, target);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            KillTimer(hwnd, 1);
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut PickState;
            if !ptr.is_null() {
                drop(Box::from_raw(ptr));
            }
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

/// 枚舉所有頂層視窗，找出游標下方（排除 overlay 自身）的可見頂層視窗。
/// 不需要 hide/show overlay，完全消除閃爍。
unsafe fn find_window_at(overlay: HWND, pt: POINT) -> HWND {
    struct Ctx {
        pt: POINT,
        overlay: HWND,
        result: HWND,
    }

    unsafe extern "system" fn enum_cb(hwnd: HWND, lp: LPARAM) -> BOOL {
        let ctx = &mut *(lp.0 as *mut Ctx);
        if hwnd == ctx.overlay { return BOOL(1); }
        if !IsWindowVisible(hwnd).as_bool() { return BOOL(1); }
        if is_desktop_window(hwnd) { return BOOL(1); }
        if !is_pickable_app_window(hwnd) { return BOOL(1); }

        let Some(rc) = visible_window_rect(hwnd) else { return BOOL(1); };
        let pt = ctx.pt;
        if pt.x >= rc.left && pt.x < rc.right && pt.y >= rc.top && pt.y < rc.bottom {
            ctx.result = hwnd;
            return BOOL(0);
        }
        BOOL(1)
    }

    let mut ctx = Ctx {
        pt,
        overlay,
        result: HWND(std::ptr::null_mut()),
    };
    let _ = EnumWindows(Some(enum_cb), LPARAM(&mut ctx as *mut _ as isize));
    ctx.result
}

unsafe fn escape_pressed() -> bool {
    (GetAsyncKeyState(VK_ESCAPE.0 as i32) as u16 & 0x8000) != 0
}

unsafe fn cancel_overlay(hwnd: HWND, tx: &Sender<AppEvent>) {
    let _ = tx.send(AppEvent::OverlayCancelled);
    ReleaseCapture().ok();
    PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0)).ok();
}

unsafe fn finish_pick(hwnd: HWND, state: &mut PickState) {
    let (sx, sy) = cursor_screen_pos();
    let pt = POINT { x: sx, y: sy };
    state.hover = find_pick_target_at(hwnd, pt);

    // 先隱藏 overlay，讓 GDI 恢復畫面，再截圖（同 region overlay 的做法）
    ShowWindow(hwnd, SW_HIDE);
    if !state.hover.is_invalid() {
        let _ = state.tx.send(AppEvent::WindowPicked(state.hover.0 as isize));
    } else {
        let _ = state.tx.send(AppEvent::OverlayCancelled);
    }
    ReleaseCapture().ok();
    PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0)).ok();
}

unsafe fn install_escape_hook(hwnd: HWND) -> Option<HHOOK> {
    OVERLAY_CANCEL_HWND.store(hwnd.0 as isize, Ordering::SeqCst);
    SetWindowsHookExW(WH_KEYBOARD_LL, Some(overlay_keyboard_proc), None, 0).ok()
}

unsafe fn uninstall_escape_hook(hook: Option<HHOOK>) {
    OVERLAY_CANCEL_HWND.store(0, Ordering::SeqCst);
    if let Some(hook) = hook {
        UnhookWindowsHookEx(hook).ok();
    }
}

unsafe fn install_pick_mouse_hook(hwnd: HWND) -> Option<HHOOK> {
    PICK_MOUSE_HWND.store(hwnd.0 as isize, Ordering::SeqCst);
    SetWindowsHookExW(WH_MOUSE_LL, Some(pick_mouse_proc), None, 0).ok()
}

unsafe fn uninstall_pick_mouse_hook(hook: Option<HHOOK>) {
    PICK_MOUSE_HWND.store(0, Ordering::SeqCst);
    if let Some(hook) = hook {
        UnhookWindowsHookEx(hook).ok();
    }
}

unsafe extern "system" fn overlay_keyboard_proc(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && (wp.0 as u32 == WM_KEYDOWN || wp.0 as u32 == WM_SYSKEYDOWN) {
        let kb = &*(lp.0 as *const KBDLLHOOKSTRUCT);
        if kb.vkCode == VK_ESCAPE.0 as u32 {
            let hwnd_raw = OVERLAY_CANCEL_HWND.load(Ordering::SeqCst);
            if hwnd_raw != 0 {
                let hwnd = HWND(hwnd_raw as *mut _);
                PostMessageW(hwnd, WM_OVERLAY_CANCEL, WPARAM(0), LPARAM(0)).ok();
                return LRESULT(1);
            }
        }
    }
    CallNextHookEx(HHOOK(std::ptr::null_mut()), code, wp, lp)
}

unsafe extern "system" fn pick_mouse_proc(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let msg = wp.0 as u32;
        if msg == WM_LBUTTONDOWN || msg == WM_LBUTTONUP {
            let hwnd_raw = PICK_MOUSE_HWND.load(Ordering::SeqCst);
            if hwnd_raw != 0 {
                if msg == WM_LBUTTONUP {
                    let hwnd = HWND(hwnd_raw as *mut _);
                    PostMessageW(hwnd, WM_OVERLAY_PICK_CLICK, WPARAM(0), LPARAM(0)).ok();
                }
                return LRESULT(1);
            }
        }
    }
    CallNextHookEx(HHOOK(std::ptr::null_mut()), code, wp, lp)
}

/// 找出游標下方最適合擷取的 HWND：先鎖定 app 主視窗，再往下找最深層可見 child HWND。
unsafe fn find_pick_target_at(overlay: HWND, pt: POINT) -> HWND {
    let root = find_window_at(overlay, pt);
    if root.is_invalid() {
        return HWND(std::ptr::null_mut());
    }

    let mut current = root;
    loop {
        let mut child_pt = pt;
        if !ScreenToClient(current, &mut child_pt).as_bool() {
            return current;
        }
        let child = ChildWindowFromPointEx(
            current,
            child_pt,
            CWP_SKIPINVISIBLE | CWP_SKIPDISABLED,
        );
        if child.is_invalid() || child == current {
            return current;
        }
        current = child;
    }
}

unsafe fn is_desktop_window(hwnd: HWND) -> bool {
    let mut cls = [0u16; 64];
    let cn = GetClassNameW(hwnd, &mut cls) as usize;
    if cn == 0 { return false; }
    matches!(
        String::from_utf16_lossy(&cls[..cn]).as_str(),
        "Progman" | "WorkerW"
    )
}

unsafe fn is_pickable_app_window(hwnd: HWND) -> bool {
    let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
    if (ex_style & WS_EX_TOOLWINDOW.0) != 0 {
        return false;
    }
    if GetWindow(hwnd, GW_OWNER).is_ok_and(|owner| !owner.is_invalid()) {
        return false;
    }
    if is_cloaked_window(hwnd) {
        return false;
    }
    if !is_alt_tab_window(hwnd) {
        return false;
    }
    if (ex_style & WS_EX_APPWINDOW.0) != 0 {
        return true;
    }

    let style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
    GetWindowTextLengthW(hwnd) > 0 && (style & WS_SYSMENU.0) != 0
}

unsafe fn is_alt_tab_window(hwnd: HWND) -> bool {
    let mut walk = GetAncestor(hwnd, GA_ROOTOWNER);
    if walk.is_invalid() {
        walk = hwnd;
    }

    loop {
        let try_hwnd = GetLastActivePopup(walk);
        if try_hwnd == walk {
            break;
        }
        if IsWindowVisible(try_hwnd).as_bool() {
            break;
        }
        walk = try_hwnd;
    }

    walk == hwnd
}

unsafe fn is_cloaked_window(hwnd: HWND) -> bool {
    let mut cloaked = 0u32;
    DwmGetWindowAttribute(
        hwnd,
        DWMWA_CLOAKED,
        &mut cloaked as *mut _ as *mut _,
        std::mem::size_of::<u32>() as u32,
    ).is_ok() && cloaked != 0
}

unsafe fn visible_window_rect(hwnd: HWND) -> Option<RECT> {
    let mut rect = RECT::default();
    if DwmGetWindowAttribute(
        hwnd,
        DWMWA_EXTENDED_FRAME_BOUNDS,
        &mut rect as *mut _ as *mut _,
        std::mem::size_of::<RECT>() as u32,
    ).is_ok() && rect.right > rect.left && rect.bottom > rect.top {
        return Some(rect);
    }

    if GetWindowRect(hwnd, &mut rect).is_ok() && rect.right > rect.left && rect.bottom > rect.top {
        Some(rect)
    } else {
        None
    }
}

/// UpdateLayeredWindow 聚光燈效果：
/// - 全螢幕：半透明黑色遮罩
/// - 選取視窗區域：透明（直接看到底下內容）＋ 橘色粗邊框
unsafe fn pick_update_overlay(hwnd: HWND, hover: HWND) {
    let vx = GetSystemMetrics(SM_XVIRTUALSCREEN);
    let vy = GetSystemMetrics(SM_YVIRTUALSCREEN);
    let vw = GetSystemMetrics(SM_CXVIRTUALSCREEN);
    let vh = GetSystemMetrics(SM_CYVIRTUALSCREEN);
    if vw <= 0 || vh <= 0 { return; }

    let screen_dc = GetDC(HWND(std::ptr::null_mut()));
    let mem_dc = CreateCompatibleDC(screen_dc);

    // 32bpp 頂部優先 DIB（BGRA，每個 DWORD = 0xAARRGGBB）
    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize:     std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth:    vw,
            biHeight:  -vh,   // 負值 = top-down
            biPlanes:   1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        bmiColors: [Default::default()],
    };
    let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
    let dib = match CreateDIBSection(mem_dc, &bmi, DIB_RGB_COLORS, &mut bits, None, 0) {
        Ok(h) => h,
        Err(_) => { DeleteDC(mem_dc); ReleaseDC(HWND(std::ptr::null_mut()), screen_dc); return; }
    };
    let old_bmp = SelectObject(mem_dc, dib);

    let n = (vw * vh) as usize;
    let pixels = std::slice::from_raw_parts_mut(bits as *mut u32, n);

    // Step 1：全部填半透明黑色遮罩（A=0x88 ≈ 53%）
    pixels.fill(0x88_00_00_00);

    // Step 2：hover 視窗處挖空（透明）並畫橘色邊框
    if !hover.is_invalid() {
        if let Some(wr) = visible_window_rect(hover) {
            // 轉換到 DIB 座標（原點 = 虛擬螢幕左上角）
            let l = (wr.left   - vx).clamp(0, vw) as usize;
            let t = (wr.top    - vy).clamp(0, vh) as usize;
            let r = (wr.right  - vx).clamp(0, vw) as usize;
            let b = (wr.bottom - vy).clamp(0, vh) as usize;

            if r > l && b > t {
                const BW: usize = 4;          // 邊框寬度（像素）
                let vw = vw as usize;

                let il = l + BW;
                let it = t + BW;
                let ir = if r > BW { r - BW } else { l };
                let ib = if b > BW { b - BW } else { t };

                // 內部填 α=1（視覺透明但保留 hit-testing，避免點擊穿透到背後視窗）
                if il < ir && it < ib {
                    for row in it..ib {
                        pixels[row * vw + il..row * vw + ir].fill(0x01_00_00_00);
                    }
                }

                // 橘色邊框（A=0xFF，完全不透明）
                // ARGB: A=255, R=255, G=168, B=0 → 0xFF_FF_A8_00
                const BC: u32 = 0xFF_FF_A8_00;
                // 上框
                for row in t..it.min(b) {
                    pixels[row * vw + l..row * vw + r].fill(BC);
                }
                // 下框
                for row in ib.max(t)..b {
                    pixels[row * vw + l..row * vw + r].fill(BC);
                }
                // 左框（中間行）
                for row in it..ib {
                    pixels[row * vw + l..row * vw + il].fill(BC);
                }
                // 右框（中間行）
                for row in it..ib {
                    pixels[row * vw + ir..row * vw + r].fill(BC);
                }
            }
        }
    }

    // UpdateLayeredWindow（逐像素 alpha，不走 WM_PAINT）
    let blend = BLENDFUNCTION {
        BlendOp:             0,   // AC_SRC_OVER
        BlendFlags:          0,
        SourceConstantAlpha: 255,
        AlphaFormat:         1,   // AC_SRC_ALPHA（逐像素 alpha）
    };
    let pt_dst = POINT { x: vx, y: vy };
    let sz     = SIZE  { cx: vw as i32, cy: vh as i32 };
    let pt_src = POINT { x: 0, y: 0 };

    UpdateLayeredWindow(
        hwnd,
        screen_dc,
        Some(&pt_dst),
        Some(&sz),
        mem_dc,
        Some(&pt_src),
        COLORREF(0),
        Some(&blend),
        ULW_ALPHA,
    );

    SelectObject(mem_dc, old_bmp);
    DeleteObject(dib);
    DeleteDC(mem_dc);
    ReleaseDC(HWND(std::ptr::null_mut()), screen_dc);
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

unsafe fn run_modal() {
    let mut msg = MSG::default();
    while GetMessageW(&mut msg, HWND(std::ptr::null_mut()), 0, 0).as_bool() {
        let _ = TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
}

fn cursor_screen_pos() -> (i32, i32) {
    unsafe {
        let mut pt = windows::Win32::Foundation::POINT::default();
        let _ = windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut pt);
        (pt.x, pt.y)
    }
}

fn normalise(x1: i32, y1: i32, x2: i32, y2: i32) -> RECT {
    RECT {
        left: x1.min(x2),
        top: y1.min(y2),
        right: x1.max(x2),
        bottom: y1.max(y2),
    }
}

// ─── Countdown ───────────────────────────────────────────────────────────────

/// 顯示全螢幕置中的倒數計時視窗，每秒更新一次，計時結束後返回。
/// `highlight`：在倒數期間以橘色框標示擷取區域（螢幕座標）。
pub fn show_countdown(seconds: u32, highlight: Option<RECT>) {
    if seconds == 0 { return; }
    unsafe {
        let class     = w!("srcshot_countdown");
        let hinstance = get_instance();

        // ── 擷取區域橘色框 overlay ──────────────────────────────────────
        let hl_class = w!("srcshot_hl");
        let hl_hwnd = if let Some(rect) = highlight {
            let wc2 = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(hl_wnd_proc),
                hInstance: hinstance,
                lpszClassName: hl_class,
                ..Default::default()
            };
            let _ = RegisterClassExW(&wc2);
            let vx = GetSystemMetrics(SM_XVIRTUALSCREEN);
            let vy = GetSystemMetrics(SM_YVIRTUALSCREEN);
            let vw = GetSystemMetrics(SM_CXVIRTUALSCREEN);
            let vh = GetSystemMetrics(SM_CYVIRTUALSCREEN);
            let h = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TRANSPARENT,
                hl_class, w!(""), WS_POPUP,
                vx, vy, vw, vh,
                HWND(std::ptr::null_mut()), HMENU(std::ptr::null_mut()), hinstance, None,
            ).unwrap_or(HWND(std::ptr::null_mut()));
            if !h.0.is_null() {
                draw_capture_border(h, rect);
                ShowWindow(h, SW_SHOW);
            }
            h
        } else { HWND(std::ptr::null_mut()) };

        unsafe extern "system" fn cdown_proc(
            hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM,
        ) -> LRESULT {
            match msg {
                WM_NCCREATE => {
                    let cs = &*(lp.0 as *const CREATESTRUCTW);
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as _);
                    LRESULT(1)
                }
                WM_ERASEBKGND => LRESULT(1),
                WM_PAINT => {
                    let n = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as u32;
                    let mut ps = PAINTSTRUCT::default();
                    let hdc = BeginPaint(hwnd, &mut ps);
                    let mut rc = RECT::default();
                    GetClientRect(hwnd, &mut rc);

                    // 深色圓形背景
                    let bg  = CreateSolidBrush(COLORREF(0x00_28_28_28));
                    let pen = CreatePen(PS_SOLID, 0, COLORREF(0x00_28_28_28));
                    let op = SelectObject(hdc, pen);
                    let ob = SelectObject(hdc, bg);
                    Ellipse(hdc, rc.left, rc.top, rc.right, rc.bottom);
                    SelectObject(hdc, op); SelectObject(hdc, ob);
                    DeleteObject(bg); DeleteObject(pen);

                    // 大白數字
                    SetBkMode(hdc, BACKGROUND_MODE(1)); // TRANSPARENT
                    SetTextColor(hdc, COLORREF(0x00_FF_FF_FF));
                    let font = CreateFontW(
                        (rc.bottom - rc.top) * 3 / 4, 0, 0, 0,
                        700, 0, 0, 0, 0, 0, 0, 0, 0, w!("Segoe UI"),
                    );
                    let of = SelectObject(hdc, font);
                    let mut tw: Vec<u16> = format!("{}", n).encode_utf16().collect();
                    DrawTextW(hdc, &mut tw, &mut rc, DRAW_TEXT_FORMAT(0x25)); // center+vcenter+single
                    SelectObject(hdc, of);
                    DeleteObject(font);
                    EndPaint(hwnd, &ps);
                    LRESULT(0)
                }
                _ => DefWindowProcW(hwnd, msg, wp, lp),
            }
        }

        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(cdown_proc),
            hInstance: hinstance,
            hbrBackground: HBRUSH(GetStockObject(NULL_BRUSH).0),
            lpszClassName: class,
            ..Default::default()
        };
        let _ = RegisterClassExW(&wc);

        let sw = GetSystemMetrics(SM_CXSCREEN);
        let sh = GetSystemMetrics(SM_CYSCREEN);
        let sz = 140i32;

        let hwnd = match CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_LAYERED | WS_EX_TOOLWINDOW,
            class, w!(""),
            WS_POPUP,
            (sw - sz) / 2, (sh - sz) / 2, sz, sz,
            HWND(std::ptr::null_mut()),
            HMENU(std::ptr::null_mut()),
            hinstance,
            None,
        ) {
            Ok(h) => h,
            Err(_) => { let _ = UnregisterClassW(class, hinstance); return; }
        };

        // 80% 不透明
        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 204, LWA_ALPHA);

        for i in (1..=seconds).rev() {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, i as isize);
            ShowWindow(hwnd, SW_SHOW);
            // InvalidateRect 標記髒區域，UpdateWindow 才會觸發 WM_PAINT
            InvalidateRect(hwnd, None, false);
            UpdateWindow(hwnd);
            std::thread::sleep(std::time::Duration::from_secs(1));
        }

        // 先關閉 highlight overlay，再關閉倒數視窗，最後等畫面還原
        if !hl_hwnd.0.is_null() {
            DestroyWindow(hl_hwnd).ok();
            let _ = UnregisterClassW(hl_class, hinstance);
        }
        DestroyWindow(hwnd).ok();
        let _ = UnregisterClassW(class, hinstance);
        std::thread::sleep(std::time::Duration::from_millis(80));
    }
}

unsafe extern "system" fn hl_wnd_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    DefWindowProcW(hwnd, msg, wp, lp)
}

/// 在透明全螢幕 overlay 上以橘色框標示擷取區域
unsafe fn draw_capture_border(hwnd: HWND, capture_rect: RECT) {
    let vx = GetSystemMetrics(SM_XVIRTUALSCREEN);
    let vy = GetSystemMetrics(SM_YVIRTUALSCREEN);
    let vw = GetSystemMetrics(SM_CXVIRTUALSCREEN);
    let vh = GetSystemMetrics(SM_CYVIRTUALSCREEN);
    if vw <= 0 || vh <= 0 { return; }

    let screen_dc = GetDC(HWND(std::ptr::null_mut()));
    let mem_dc    = CreateCompatibleDC(screen_dc);
    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: vw, biHeight: -vh,
            biPlanes: 1, biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        bmiColors: [Default::default()],
    };
    let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
    let dib = match CreateDIBSection(mem_dc, &bmi, DIB_RGB_COLORS, &mut bits, None, 0) {
        Ok(h) => h,
        Err(_) => { DeleteDC(mem_dc); ReleaseDC(HWND(std::ptr::null_mut()), screen_dc); return; }
    };
    let old_bmp = SelectObject(mem_dc, dib);

    let n = (vw * vh) as usize;
    let pixels = std::slice::from_raw_parts_mut(bits as *mut u32, n);
    pixels.fill(0x00_00_00_00); // 全透明

    // 橘色框（4px，完全不透明）
    let l = (capture_rect.left   - vx).clamp(0, vw) as usize;
    let t = (capture_rect.top    - vy).clamp(0, vh) as usize;
    let r = (capture_rect.right  - vx).clamp(0, vw) as usize;
    let b = (capture_rect.bottom - vy).clamp(0, vh) as usize;

    if r > l && b > t {
        const BW: usize = 4;
        let vw_u = vw as usize;
        let il = (l + BW).min(r);
        let it = (t + BW).min(b);
        let ir = if r > BW { r - BW } else { l };
        let ib = if b > BW { b - BW } else { t };
        const BC: u32 = 0xFF_FF_A8_00;
        for row in t..it.min(b) { pixels[row*vw_u+l..row*vw_u+r].fill(BC); }
        for row in ib.max(t)..b  { pixels[row*vw_u+l..row*vw_u+r].fill(BC); }
        for row in it..ib { pixels[row*vw_u+l  ..row*vw_u+il].fill(BC); }
        for row in it..ib { pixels[row*vw_u+ir ..row*vw_u+r ].fill(BC); }
    }

    let blend  = BLENDFUNCTION { BlendOp: 0, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: 1 };
    let pt_dst = POINT { x: vx, y: vy };
    let sz     = SIZE  { cx: vw, cy: vh };
    let pt_src = POINT { x: 0,  y: 0  };
    UpdateLayeredWindow(hwnd, screen_dc, Some(&pt_dst), Some(&sz),
        mem_dc, Some(&pt_src), COLORREF(0), Some(&blend), ULW_ALPHA);

    SelectObject(mem_dc, old_bmp);
    DeleteObject(dib);
    DeleteDC(mem_dc);
    ReleaseDC(HWND(std::ptr::null_mut()), screen_dc);
}

fn get_instance() -> windows::Win32::Foundation::HINSTANCE {
    unsafe {
        windows::Win32::System::LibraryLoader::GetModuleHandleW(None)
            .unwrap()
            .into()
    }
}
