use windows::Win32::Foundation::{BOOL, COLORREF, HWND, RECT};
use windows::Win32::Graphics::Gdi::{
    BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    CreateBitmap, CreateCompatibleDC, CreateDIBSection, CreatePen,
    CreateSolidBrush, DeleteDC, DeleteObject, Ellipse, FillRect, GetDC,
    PatBlt, ReleaseDC, RoundRect, SelectObject,
    BLACKNESS, DIB_RGB_COLORS, PS_SOLID,
};
use windows::Win32::UI::WindowsAndMessaging::{CreateIconIndirect, HICON, ICONINFO};

/// GDI 繪製相機圖示（透明底，保留原有藍白設計），呼叫方負責 DestroyIcon
pub unsafe fn create_app_icon() -> HICON {
    let sz = 32i32;
    let null_hwnd = HWND(std::ptr::null_mut());
    let screen_dc = GetDC(null_hwnd);
    let mem_dc = CreateCompatibleDC(screen_dc);

    // 32bpp DIB（BGRA，alpha 通道由下面手動設定）
    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: sz, biHeight: -sz,
            biPlanes: 1, biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        bmiColors: [Default::default()],
    };
    let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
    let color_bmp = CreateDIBSection(screen_dc, &bmi, DIB_RGB_COLORS, &mut bits, None, 0)
        .unwrap();
    let old_bmp = SelectObject(mem_dc, color_bmp);

    // 背景填純紅色作為透明鍵（相機本身不含純紅像素）
    // COLORREF = 0x00BBGGRR，純紅 = R=255,G=0,B=0 → 0x00_00_00_FF
    let key_red = COLORREF(0x00_00_00_FF);
    let bg = CreateSolidBrush(key_red);
    FillRect(mem_dc, &RECT { left: 0, top: 0, right: sz, bottom: sz }, bg);
    DeleteObject(bg);

    // ── 藍色相機剪影（透明底，整體為藍色）──────────────────────
    let blue  = COLORREF(0x00_CC_66_33); // #3366CC

    let pen   = CreatePen(PS_SOLID, 0, blue);
    let brush = CreateSolidBrush(blue);
    let old_pen   = SelectObject(mem_dc, pen);
    let old_brush = SelectObject(mem_dc, brush);

    RoundRect(mem_dc, 11, 4, 21, 10, 3, 3); // 快門鍵突起
    RoundRect(mem_dc, 2, 8, 30, 26, 5, 5);  // 相機主體
    Ellipse(mem_dc, 10, 12, 22, 22);         // 鏡頭環（實心藍色）

    SelectObject(mem_dc, old_pen);
    SelectObject(mem_dc, old_brush);
    DeleteObject(pen);
    DeleteObject(brush);
    SelectObject(mem_dc, old_bmp);
    DeleteDC(mem_dc);
    ReleaseDC(null_hwnd, screen_dc);

    // ── Alpha 通道後處理 ────────────────────────────────────────
    // BGRA 格式：[0]=B, [1]=G, [2]=R, [3]=A
    // 純紅像素（R=255,G=0,B=0）= 透明鍵 → alpha=0
    // 其餘像素（相機本體）→ alpha=255
    {
        let n = (sz * sz) as usize;
        let pixels = std::slice::from_raw_parts_mut(bits as *mut u8, n * 4);
        for i in 0..n {
            let off = i * 4;
            let is_key = pixels[off + 2] == 255   // R=255
                      && pixels[off + 1] == 0      // G=0
                      && pixels[off]     == 0;     // B=0
            pixels[off + 3] = if is_key { 0 } else { 255 };
        }
    }

    // 遮罩 bitmap（全黑，靠 alpha 通道決定透明）
    let screen_dc2 = GetDC(null_hwnd);
    let mask_dc  = CreateCompatibleDC(screen_dc2);
    let mask_bmp = CreateBitmap(sz, sz, 1, 1, None);
    let old_mask = SelectObject(mask_dc, mask_bmp);
    PatBlt(mask_dc, 0, 0, sz, sz, BLACKNESS);
    SelectObject(mask_dc, old_mask);
    DeleteDC(mask_dc);
    ReleaseDC(null_hwnd, screen_dc2);

    let info = ICONINFO {
        fIcon: BOOL(1), xHotspot: 0, yHotspot: 0,
        hbmMask: mask_bmp, hbmColor: color_bmp,
    };
    let icon = CreateIconIndirect(&info).unwrap();
    DeleteObject(color_bmp);
    DeleteObject(mask_bmp);
    icon
}
