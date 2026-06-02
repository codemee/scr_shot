use std::sync::mpsc::Sender;
use windows::core::w;
use windows::Win32::Foundation::{BOOL, COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION,
    CreateCompatibleDC, CreateDIBSection, CreateSolidBrush, DeleteDC,
    DeleteObject, DIB_RGB_COLORS, FillRect,
    GetDC, GetStockObject, InvalidateRect, NULL_BRUSH, ReleaseDC,
    SelectObject, UpdateWindow, HBRUSH,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    ReleaseCapture, SetCapture, VK_ESCAPE,
};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::event::AppEvent;

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
        run_modal();
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
            let _ = state.tx.send(AppEvent::OverlayCancelled);
            ReleaseCapture().unwrap();
            PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0)).unwrap();
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
        run_modal();
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
            let _ = state.tx.send(AppEvent::OverlayCancelled);
            ReleaseCapture().unwrap();
            PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0)).unwrap();
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let state = &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut PickState);
            let (sx, sy) = cursor_screen_pos();
            let pt = POINT { x: sx, y: sy };

            // 用 EnumWindows 枚舉頂層視窗，跳過 overlay 自身
            // 不需要 hide/show，完全消除閃爍
            let target = find_window_at(hwnd, pt);

            if target != state.hover {
                state.hover = target;
                pick_update_overlay(hwnd, target);
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let state = &*(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut PickState);
            // 先隱藏 overlay，讓 GDI 恢復畫面，再截圖（同 region overlay 的做法）
            ShowWindow(hwnd, SW_HIDE);
            if !state.hover.is_invalid() {
                let _ = state.tx.send(AppEvent::WindowPicked(state.hover.0 as isize));
            } else {
                let _ = state.tx.send(AppEvent::OverlayCancelled);
            }
            ReleaseCapture().unwrap();
            PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0)).unwrap();
            LRESULT(0)
        }
        WM_TIMER => {
            // 備援輪詢：更新 hover（當 WM_MOUSEMOVE 因某原因未送達時的後備）
            let state = &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut PickState);
            let (sx, sy) = cursor_screen_pos();
            let pt = POINT { x: sx, y: sy };
            let target = find_window_at(hwnd, pt);
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
    struct Ctx { pt: POINT, overlay: HWND, result: HWND }

    unsafe extern "system" fn enum_cb(hwnd: HWND, lp: LPARAM) -> BOOL {
        let ctx = &mut *(lp.0 as *mut Ctx);
        if hwnd == ctx.overlay { return BOOL(1); }          // 跳過 overlay 自身
        if !IsWindowVisible(hwnd).as_bool() { return BOOL(1); }
        let mut rc = RECT::default();
        if GetWindowRect(hwnd, &mut rc).is_err() { return BOOL(1); }
        if rc.right <= rc.left || rc.bottom <= rc.top { return BOOL(1); } // 跳過零尺寸視窗
        // 跳過桌面背景視窗（Progman = 傳統桌面，WorkerW = 含小工具的桌面）
        let mut cls = [0u16; 32];
        let cn = GetClassNameW(hwnd, &mut cls) as usize;
        if cn > 0 {
            let name = String::from_utf16_lossy(&cls[..cn]);
            if name == "Progman" || name == "WorkerW" {
                return BOOL(1);
            }
        }
        let pt = ctx.pt;
        if pt.x >= rc.left && pt.x < rc.right && pt.y >= rc.top && pt.y < rc.bottom {
            ctx.result = hwnd;
            return BOOL(0); // 找到，停止枚舉
        }
        BOOL(1)
    }

    let mut ctx = Ctx { pt, overlay, result: HWND(std::ptr::null_mut()) };
    let _ = EnumWindows(Some(enum_cb), LPARAM(&mut ctx as *mut _ as isize));
    ctx.result
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
        let mut wr = RECT::default();
        if GetWindowRect(hover, &mut wr).is_ok() {
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

fn get_instance() -> windows::Win32::Foundation::HINSTANCE {
    unsafe {
        windows::Win32::System::LibraryLoader::GetModuleHandleW(None)
            .unwrap()
            .into()
    }
}
