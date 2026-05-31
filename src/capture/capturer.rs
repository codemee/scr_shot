use windows::Win32::Foundation::{RECT, POINT, TRUE};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::{GetDesktopWindow, GetDC, ReleaseDC, GetWindowDC};

use crate::app_state::CapturedImage;

fn get_virtual_screen_rect() -> RECT {
    let x = unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) };
    let y = unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) };
    let w = unsafe { GetSystemMetrics(SM_CXVIRTUALSCREEN) };
    let h = unsafe { GetSystemMetrics(SM_CYVIRTUALSCREEN) };
    RECT { left: x, top: y, right: x + w, bottom: y + h }
}

fn capture_rect_gdi(rect: RECT) -> Result<CapturedImage, String> {
    let width = (rect.right - rect.left) as u32;
    let height = (rect.bottom - rect.top) as u32;
    if width == 0 || height == 0 {
        return Err("empty capture region".into());
    }

    let hwnd_desk = unsafe { GetDesktopWindow() };
    let hdc_screen = unsafe { GetDC(hwnd_desk) };
    if hdc_screen.is_invalid() {
        return Err("GetDC failed".into());
    }

    let hdc_mem = unsafe { CreateCompatibleDC(Some(hdc_screen)) };
    if hdc_mem.is_invalid() {
        unsafe { let _ = ReleaseDC(hwnd_desk, hdc_screen); }
        return Err("CreateCompatibleDC failed".into());
    }

    let hbmp = unsafe { CreateCompatibleBitmap(hdc_screen, width as i32, height as i32) };
    if hbmp.is_invalid() {
        unsafe { let _ = DeleteDC(hdc_mem); let _ = ReleaseDC(hwnd_desk, hdc_screen); }
        return Err("CreateCompatibleBitmap failed".into());
    }

    unsafe { SelectObject(hdc_mem, hbmp) };

    let ok = unsafe {
        BitBlt(
            hdc_mem, 0, 0, width as i32, height as i32,
            hdc_screen, rect.left, rect.top,
            SRCCOPY,
        )
    };
    if ok != TRUE {
        unsafe { DeleteObject(hbmp); DeleteDC(hdc_mem); ReleaseDC(hwnd_desk, hdc_screen); }
        return Err("BitBlt failed".into());
    }

    let bmp_info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width as i32,
            biHeight: -(height as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            biSizeImage: 0,
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
        },
        bmiColors: unsafe { std::mem::zeroed() },
    };

    let mut data = vec![0u8; (width * height * 4) as usize];

    let result = unsafe {
        GetDIBits(
            hdc_mem,
            hbmp,
            0,
            height as u32,
            Some(data.as_mut_ptr() as *mut _),
            &bmp_info,
            DIB_RGB_COLORS,
        )
    };

    unsafe {
        DeleteObject(hbmp);
        DeleteDC(hdc_mem);
        ReleaseDC(hwnd_desk, hdc_screen);
    }

    if result == 0 {
        return Err("GetDIBits failed".into());
    }

    Ok(CapturedImage { data, width, height })
}

pub fn capture_fullscreen() -> Result<CapturedImage, String> {
    let rect = get_virtual_screen_rect();
    capture_rect_gdi(rect)
}

pub fn capture_foreground_window() -> Result<CapturedImage, String> {
    unsafe {
        let hwnd = windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow();
        if hwnd.is_invalid() {
            return Err("no foreground window".into());
        }
        let mut rect = RECT::default();
        if windows::Win32::UI::WindowsAndMessaging::GetWindowRect(hwnd, &mut rect).is_ok() {
            capture_rect_gdi(rect)
        } else {
            Err("GetWindowRect failed".into())
        }
    }
}

pub fn capture_region(rect: (i32, i32, i32, i32)) -> Result<CapturedImage, String> {
    let r = RECT { left: rect.0, top: rect.1, right: rect.2, bottom: rect.3 };
    capture_rect_gdi(r)
}

pub fn capture_window_from_point(x: i32, y: i32) -> Result<CapturedImage, String> {
    unsafe {
        let pt = POINT { x, y };
        let hwnd = windows::Win32::UI::WindowsAndMessaging::WindowFromPoint(pt);
        if hwnd.is_invalid() {
            return Err("no window at point".into());
        }
        let mut rect = RECT::default();
        if windows::Win32::UI::WindowsAndMessaging::GetWindowRect(hwnd, &mut rect).is_ok() {
            capture_rect_gdi(rect)
        } else {
            Err("GetWindowRect failed".into())
        }
    }
}
