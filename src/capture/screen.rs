use anyhow::{Context, Result};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Dwm::{
    DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS,
};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject,
    GetDC, ReleaseDC, SelectObject, SRCCOPY, HDC, HBRUSH,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowRect,
    GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
    CURSORINFO, CURSOR_SHOWING, GetCursorInfo, GetIconInfo, ICONINFO,
    DrawIconEx, DI_NORMAL, HICON,
};

#[derive(Clone)]
pub struct ScreenBitmap {
    pub width: i32,
    pub height: i32,
    pub data: Vec<u8>, // BGRA, row-major
}

pub fn capture_rect(rect: RECT, capture_cursor: bool) -> Result<ScreenBitmap> {
    let w = rect.right - rect.left;
    let h = rect.bottom - rect.top;
    anyhow::ensure!(w > 0 && h > 0, "empty rect");

    unsafe {
        let screen_dc = GetDC(HWND(std::ptr::null_mut()));
        let mem_dc = CreateCompatibleDC(screen_dc);
        let bmp = CreateCompatibleBitmap(screen_dc, w, h);
        let old = SelectObject(mem_dc, bmp);

        BitBlt(mem_dc, 0, 0, w, h, screen_dc, rect.left, rect.top, SRCCOPY)
            .context("BitBlt failed")?;

        if capture_cursor {
            draw_cursor(mem_dc, &rect);
        }

        let mut info = windows::Win32::Graphics::Gdi::BITMAPINFO {
            bmiHeader: windows::Win32::Graphics::Gdi::BITMAPINFOHEADER {
                biSize: std::mem::size_of::<windows::Win32::Graphics::Gdi::BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h, // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: windows::Win32::Graphics::Gdi::BI_RGB.0,
                ..Default::default()
            },
            bmiColors: [windows::Win32::Graphics::Gdi::RGBQUAD::default()],
        };

        let mut pixels = vec![0u8; (w * h * 4) as usize];
        windows::Win32::Graphics::Gdi::GetDIBits(
            mem_dc, bmp, 0, h as u32,
            Some(pixels.as_mut_ptr() as *mut _),
            &mut info,
            windows::Win32::Graphics::Gdi::DIB_RGB_COLORS,
        );

        SelectObject(mem_dc, old);
        DeleteObject(bmp);
        DeleteDC(mem_dc);
        ReleaseDC(HWND(std::ptr::null_mut()), screen_dc);

        Ok(ScreenBitmap { width: w, height: h, data: pixels })
    }
}

/// 將滑鼠游標繪製到已擷取的 DC 上（考慮游標熱點偏移）
unsafe fn draw_cursor(dc: HDC, rect: &RECT) {
    let mut ci = CURSORINFO {
        cbSize: std::mem::size_of::<CURSORINFO>() as u32,
        ..Default::default()
    };
    if GetCursorInfo(&mut ci).is_err() { return; }
    if (ci.flags.0 & CURSOR_SHOWING.0) == 0 { return; }

    let hicon = HICON(ci.hCursor.0);
    let mut ii = ICONINFO::default();
    if GetIconInfo(hicon, &mut ii).is_err() { return; }

    // 清理 GetIconInfo 配置的 bitmap（避免洩漏）
    if !ii.hbmColor.is_invalid() { let _ = DeleteObject(ii.hbmColor); }
    if !ii.hbmMask.is_invalid()  { let _ = DeleteObject(ii.hbmMask);  }

    let draw_x = ci.ptScreenPos.x - rect.left - ii.xHotspot as i32;
    let draw_y = ci.ptScreenPos.y - rect.top  - ii.yHotspot as i32;

    let _ = DrawIconEx(
        dc, draw_x, draw_y, hicon,
        0, 0, 0,
        HBRUSH(std::ptr::null_mut()),
        DI_NORMAL,
    );
}

pub fn active_window_rect() -> Result<RECT> {
    unsafe {
        let hwnd = GetForegroundWindow();
        anyhow::ensure!(!hwnd.is_invalid(), "no foreground window");
        visible_window_rect(hwnd)
    }
}

pub fn fullscreen_rect() -> Result<RECT> {
    unsafe {
        let left = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let top = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let width = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let height = GetSystemMetrics(SM_CYVIRTUALSCREEN);
        anyhow::ensure!(width > 0 && height > 0, "empty virtual screen");
        Ok(RECT {
            left,
            top,
            right: left + width,
            bottom: top + height,
        })
    }
}

pub fn window_rect(hwnd: HWND) -> Result<RECT> {
    unsafe { visible_window_rect(hwnd) }
}

unsafe fn visible_window_rect(hwnd: HWND) -> Result<RECT> {
    let mut rect = RECT::default();
    if DwmGetWindowAttribute(
        hwnd,
        DWMWA_EXTENDED_FRAME_BOUNDS,
        &mut rect as *mut _ as *mut _,
        std::mem::size_of::<RECT>() as u32,
    ).is_ok() && rect.right > rect.left && rect.bottom > rect.top {
        return Ok(rect);
    }

    unsafe {
        GetWindowRect(hwnd, &mut rect).context("GetWindowRect failed")?;
        Ok(rect)
    }
}
