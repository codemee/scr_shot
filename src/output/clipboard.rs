use anyhow::{Context, Result};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{
    BITMAPINFOHEADER, BI_RGB,
};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};

use crate::capture::screen::ScreenBitmap;

pub fn copy_to_clipboard(bmp: &ScreenBitmap) -> Result<()> {
    unsafe {
        // Build a DIB in global memory (CF_DIB)
        let header = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: bmp.width,
            biHeight: -bmp.height, // top-down
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            biSizeImage: (bmp.width * bmp.height * 4) as u32,
            biXPelsPerMeter: 2835,
            biYPelsPerMeter: 2835,
            biClrUsed: 0,
            biClrImportant: 0,
        };

        let total = std::mem::size_of::<BITMAPINFOHEADER>() + bmp.data.len();
        let hmem = GlobalAlloc(GMEM_MOVEABLE, total).context("GlobalAlloc")?;
        let ptr = GlobalLock(hmem) as *mut u8;
        anyhow::ensure!(!ptr.is_null(), "GlobalLock failed");

        std::ptr::copy_nonoverlapping(
            &header as *const _ as *const u8,
            ptr,
            std::mem::size_of::<BITMAPINFOHEADER>(),
        );
        std::ptr::copy_nonoverlapping(
            bmp.data.as_ptr(),
            ptr.add(std::mem::size_of::<BITMAPINFOHEADER>()),
            bmp.data.len(),
        );
        GlobalUnlock(hmem);

        OpenClipboard(HWND(std::ptr::null_mut())).context("OpenClipboard")?;
        EmptyClipboard().context("EmptyClipboard")?;
        // CF_DIB = 8
        SetClipboardData(8, windows::Win32::Foundation::HANDLE(hmem.0))
            .context("SetClipboardData")?;
        CloseClipboard().context("CloseClipboard")?;

        Ok(())
    }
}
