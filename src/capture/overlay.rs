use std::ffi::c_void;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, TRUE, WPARAM};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::app_state::{CaptureMode, CapturedImage};
use crate::capture::capturer;

const CLASS_NAME: &str = "SrcshotOverlay";
const WM_SELECTION_DONE: u32 = 0x9000;

struct Selection {
    dragging: bool,
    start_x: i32,
    start_y: i32,
    end_x: i32,
    end_y: i32,
}

impl Default for Selection {
    fn default() -> Self {
        Self { dragging: false, start_x: 0, start_y: 0, end_x: 0, end_y: 0 }
    }
}

struct HoverWindow {
    valid: bool,
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

impl Default for HoverWindow {
    fn default() -> Self {
        Self { valid: false, left: 0, top: 0, right: 0, bottom: 0 }
    }
}

struct OverlayData {
    mode: CaptureMode,
    screenshot: CapturedImage,
    screen_ox: i32,
    screen_oy: i32,
    screen_w: i32,
    screen_h: i32,
    selection: Selection,
    hover: HoverWindow,
}

fn register_class(hinst: HINSTANCE) {
    let mut wc: WNDCLASSA = unsafe { std::mem::zeroed() };
    wc.style = CS_HREDRAW | CS_VREDRAW;
    wc.lpfnWndProc = Some(wndproc);
    wc.hInstance = hinst;
    wc.hCursor = unsafe { LoadCursorW(None, IDC_CROSS) };
    wc.hbrBackground = HBRUSH(5 as _);
    wc.lpszClassName = windows::core::s!(CLASS_NAME);
    unsafe { let _ = RegisterClassA(&wc); }
}

unsafe extern "system" fn wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_CREATE {
        let data = Box::into_raw(Box::new(OverlayData {
            mode: CaptureMode::Region,
            screenshot: CapturedImage { data: vec![], width: 0, height: 0 },
            screen_ox: 0, screen_oy: 0,
            screen_w: 0, screen_h: 0,
            selection: Selection::default(),
            hover: HoverWindow::default(),
        }));
        SetWindowLongPtrA(hwnd, GWLP_USERDATA, data as isize);
        return LRESULT(0);
    }

    let data_ptr = GetWindowLongPtrA(hwnd, GWLP_USERDATA);
    if data_ptr == 0 {
        return DefWindowProcA(hwnd, msg, wparam, lparam);
    }
    let data = &mut *(data_ptr as *mut OverlayData);

    match msg {
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &ps);
            let sw = data.screen_w;
            let sh = data.screen_h;

            let mem_dc = CreateCompatibleDC(Some(hdc));
            let bmp = CreateCompatibleBitmap(hdc, sw, sh);
            SelectObject(mem_dc, bmp);

            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: data.screenshot.width as i32,
                    biHeight: -(data.screenshot.height as i32),
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB,
                    ..Default::default()
                },
                bmiColors: std::mem::zeroed(),
            };

            SetDIBitsToDevice(
                mem_dc,
                0, 0,
                data.screenshot.width as u32,
                data.screenshot.height as u32,
                0, 0,
                0u32,
                data.screenshot.height as u32,
                data.screenshot.data.as_ptr() as *const c_void,
                &info,
                DIB_RGB_COLORS,
            );

            if data.mode == CaptureMode::Region || data.mode == CaptureMode::SelectWindow {
                let overlay_dc = CreateCompatibleDC(Some(hdc));
                let overlay_info = BITMAPINFO {
                    bmiHeader: BITMAPINFOHEADER {
                        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                        biWidth: sw,
                        biHeight: -sh,
                        biPlanes: 1,
                        biBitCount: 32,
                        biCompression: BI_RGB,
                        ..Default::default()
                    },
                    bmiColors: std::mem::zeroed(),
                };
                let mut overlay_bits: *mut c_void = std::ptr::null_mut();
                let overlay_bmp = CreateDIBSection(
                    hdc,
                    &overlay_info,
                    DIB_RGB_COLORS,
                    Some(&mut overlay_bits),
                    None,
                    0,
                );
                SelectObject(overlay_dc, overlay_bmp);

                let dark = RGB(30, 30, 30);
                let brush = CreateSolidBrush(dark);
                let r = RECT { left: 0, top: 0, right: sw, bottom: sh };
                FillRect(overlay_dc, &r, brush);
                DeleteObject(brush);

                let bf = BLENDFUNCTION {
                    BlendOp: AC_SRC_OVER,
                    BlendFlags: 0,
                    SourceConstantAlpha: 150,
                    AlphaFormat: 0,
                };
                AlphaBlend(mem_dc, 0, 0, sw, sh, overlay_dc, 0, 0, sw, sh, bf);

                DeleteObject(overlay_bmp);
                DeleteDC(overlay_dc);
            }

            if data.selection.dragging {
                let x1 = data.selection.start_x.min(data.selection.end_x);
                let y1 = data.selection.start_y.min(data.selection.end_y);
                let x2 = data.selection.start_x.max(data.selection.end_x);
                let y2 = data.selection.start_y.max(data.selection.end_y);
                let cw = x2 - x1;
                let ch = y2 - y1;

                if cw > 0 && ch > 0 {
                    SetDIBitsToDevice(
                        mem_dc,
                        x1, y1,
                        cw as u32, ch as u32,
                        x1, y1,
                        y1 as u32, ch as u32,
                        data.screenshot.data.as_ptr() as *const c_void,
                        &info,
                        DIB_RGB_COLORS,
                    );

                    let pen = CreatePen(PS_DOT, 2, RGB(255, 255, 255));
                    SelectObject(mem_dc, pen);
                    SelectObject(mem_dc, GetStockObject(NULL_BRUSH));
                    Rectangle(mem_dc, x1, y1, x2, y2);
                    DeleteObject(pen);

                    let sz = format!(" {} × {} ", cw, ch);
                    let sz_w: Vec<u16> = sz.encode_utf16().collect();
                    SetBkColor(mem_dc, RGB(0, 0, 0));
                    SetTextColor(mem_dc, RGB(255, 255, 255));
                    TextOutW(mem_dc, x1 + 4, y1 - 22, &sz_w);
                }
            }

            if data.mode == CaptureMode::SelectWindow && data.hover.valid {
                let ox = data.screen_ox;
                let oy = data.screen_oy;
                let pen = CreatePen(PS_SOLID, 3, RGB(255, 100, 0));
                SelectObject(mem_dc, pen);
                SelectObject(mem_dc, GetStockObject(NULL_BRUSH));
                Rectangle(mem_dc,
                    data.hover.left - ox,
                    data.hover.top - oy,
                    data.hover.right - ox,
                    data.hover.bottom - oy,
                );
                DeleteObject(pen);
            }

            BitBlt(hdc, 0, 0, sw, sh, mem_dc, 0, 0, SRCCOPY);
            DeleteObject(bmp);
            DeleteDC(mem_dc);
            EndPaint(hwnd, &ps);
            LRESULT(0)
        }

        WM_MOUSEMOVE => {
            let x = (lparam.0 & 0xFFFF) as i16 as i32;
            let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;

            match data.mode {
                CaptureMode::Region => {
                    if data.selection.dragging {
                        data.selection.end_x = x;
                        data.selection.end_y = y;
                        InvalidateRect(hwnd, None, TRUE);
                    }
                }
                CaptureMode::SelectWindow => {
                    let pt = POINT { x, y };
                    let hwnd_under = WindowFromPoint(pt);
                    if hwnd_under != hwnd {
                        let mut rect = RECT::default();
                        if GetWindowRect(hwnd_under, &mut rect).is_ok() {
                            data.hover.valid = true;
                            data.hover.left = rect.left;
                            data.hover.top = rect.top;
                            data.hover.right = rect.right;
                            data.hover.bottom = rect.bottom;
                            InvalidateRect(hwnd, None, TRUE);
                        }
                    }
                }
                _ => {}
            }
            LRESULT(0)
        }

        WM_LBUTTONDOWN => {
            match data.mode {
                CaptureMode::Region => {
                    let x = (lparam.0 & 0xFFFF) as i16 as i32;
                    let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
                    data.selection.dragging = true;
                    data.selection.start_x = x;
                    data.selection.start_y = y;
                    data.selection.end_x = x;
                    data.selection.end_y = y;
                    SetCapture(hwnd);
                    InvalidateRect(hwnd, None, TRUE);
                }
                CaptureMode::SelectWindow => {
                    if data.hover.valid {
                        PostMessageA(hwnd, WM_SELECTION_DONE, WPARAM(0), LPARAM(0));
                    }
                }
                _ => {}
            }
            LRESULT(0)
        }

        WM_LBUTTONUP => {
            if data.selection.dragging {
                data.selection.dragging = false;
                ReleaseCapture();
                let x = (lparam.0 & 0xFFFF) as i16 as i32;
                let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
                data.selection.end_x = x;
                data.selection.end_y = y;
                PostMessageA(hwnd, WM_SELECTION_DONE, WPARAM(0), LPARAM(0));
            }
            LRESULT(0)
        }

        WM_KEYDOWN => {
            if wparam.0 == VK_ESCAPE.0 as usize {
                PostMessageA(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
            }
            LRESULT(0)
        }

        WM_DESTROY => {
            let _ = Box::from_raw(data_ptr as *mut OverlayData);
            SetWindowLongPtrA(hwnd, GWLP_USERDATA, 0);
            LRESULT(0)
        }

        _ => DefWindowProcA(hwnd, msg, wparam, lparam),
    }
}

pub unsafe fn run_overlay(mode: CaptureMode) -> Option<CapturedImage> {
    let hinst = GetModuleHandleA(None).unwrap();
    let screen_ox = GetSystemMetrics(SM_XVIRTUALSCREEN);
    let screen_oy = GetSystemMetrics(SM_YVIRTUALSCREEN);
    let screen_w = GetSystemMetrics(SM_CXVIRTUALSCREEN);
    let screen_h = GetSystemMetrics(SM_CYVIRTUALSCREEN);

    if screen_w <= 0 || screen_h <= 0 {
        return None;
    }

    register_class(hinst);

    let screenshot = capturer::capture_fullscreen().ok()?;

    let hwnd = CreateWindowExA(
        WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
        windows::core::s!(CLASS_NAME),
        windows::core::s!(""),
        WS_POPUP,
        screen_ox, screen_oy, screen_w, screen_h,
        None, None, hinst, None,
    );

    if hwnd.is_invalid() {
        return None;
    }

    let data_ptr = GetWindowLongPtrA(hwnd, GWLP_USERDATA);
    if data_ptr == 0 { return None; }
    let data = &mut *(data_ptr as *mut OverlayData);
    data.mode = mode;
    data.screenshot = screenshot;
    data.screen_ox = screen_ox;
    data.screen_oy = screen_oy;
    data.screen_w = screen_w;
    data.screen_h = screen_h;

    ShowWindow(hwnd, SW_SHOW);
    SetForegroundWindow(hwnd);
    UpdateWindow(hwnd);

    let mut msg = MSG::default();
    let mut result: Option<(i32, i32, i32, i32)> = None;

    loop {
        let ret = GetMessageA(&mut msg, None, 0, 0);
        if ret.0 == 0 || ret.0 == -1 { break; }

        if msg.message == WM_SELECTION_DONE {
            let dp = GetWindowLongPtrA(hwnd, GWLP_USERDATA);
            if dp != 0 {
                let d = &*(dp as *const OverlayData);
                if d.mode == CaptureMode::SelectWindow && d.hover.valid {
                    result = Some((d.hover.left, d.hover.top, d.hover.right, d.hover.bottom));
                } else if d.selection.dragging || true {
                    let x1 = d.selection.start_x.min(d.selection.end_x);
                    let y1 = d.selection.start_y.min(d.selection.end_y);
                    let x2 = d.selection.start_x.max(d.selection.end_x);
                    let y2 = d.selection.start_y.max(d.selection.end_y);
                    if (x2 - x1).abs() >= 2 && (y2 - y1).abs() >= 2 {
                        result = Some((x1, y1, x2, y2));
                    }
                }
            }
            break;
        }

        if msg.message == WM_CLOSE || msg.message == WM_DESTROY {
            break;
        }

        TranslateMessage(&msg);
        DispatchMessageA(&msg);
    }

    DestroyWindow(hwnd);

    if let Some((x1, y1, x2, y2)) = result {
        match mode {
            CaptureMode::SelectWindow => {
                let cx = (x1 + x2) / 2;
                let cy = (y1 + y2) / 2;
                capturer::capture_window_from_point(cx, cy).ok()
            }
            CaptureMode::Region => {
                let rx = x1 + screen_ox;
                let ry = y1 + screen_oy;
                let rw = x2 + screen_ox;
                let rh = y2 + screen_oy;
                capturer::capture_region((rx, ry, rw, rh)).ok()
            }
            _ => None,
        }
    } else {
        None
    }
}
