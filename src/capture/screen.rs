use anyhow::{Context, Result};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject,
    GetDC, ReleaseDC, SelectObject, SRCCOPY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowRect,
};

pub struct ScreenBitmap {
    pub width: i32,
    pub height: i32,
    pub data: Vec<u8>, // BGRA, row-major
}

pub fn capture_rect(rect: RECT) -> Result<ScreenBitmap> {
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

        let mut info = windows::Win32::Graphics::Gdi::BITMAPINFO {
            bmiHeader: windows::Win32::Graphics::Gdi::BITMAPINFOHEADER {
                biSize: std::mem::size_of::<windows::Win32::Graphics::Gdi::BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h, // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: windows::Win32::Graphics::Gdi::BI_RGB.0,
                biSizeImage: 0,
                biXPelsPerMeter: 0,
                biYPelsPerMeter: 0,
                biClrUsed: 0,
                biClrImportant: 0,
            },
            bmiColors: [windows::Win32::Graphics::Gdi::RGBQUAD::default()],
        };

        let mut pixels = vec![0u8; (w * h * 4) as usize];
        windows::Win32::Graphics::Gdi::GetDIBits(
            mem_dc,
            bmp,
            0,
            h as u32,
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

pub fn active_window_rect() -> Result<RECT> {
    unsafe {
        let hwnd = GetForegroundWindow();
        anyhow::ensure!(!hwnd.is_invalid(), "no foreground window");
        let mut rect = RECT::default();
        GetWindowRect(hwnd, &mut rect).context("GetWindowRect failed")?;
        Ok(rect)
    }
}

pub fn window_rect(hwnd: HWND) -> Result<RECT> {
    unsafe {
        let mut rect = RECT::default();
        GetWindowRect(hwnd, &mut rect).context("GetWindowRect failed")?;
        Ok(rect)
    }
}
