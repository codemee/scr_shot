use windows::Win32::Foundation::{BOOL, COLORREF, HWND, RECT};
use windows::Win32::Graphics::Gdi::{
    CreateBitmap, CreateCompatibleBitmap, CreateCompatibleDC, CreatePen,
    CreateSolidBrush, DeleteDC, DeleteObject, Ellipse, FillRect, GetDC,
    PatBlt, ReleaseDC, RoundRect, SelectObject, BLACKNESS, PS_SOLID,
};
use windows::Win32::UI::WindowsAndMessaging::{CreateIconIndirect, HICON, ICONINFO};

/// GDI 繪製相機圖示，呼叫方負責 DestroyIcon
pub unsafe fn create_app_icon() -> HICON {
    let sz = 32i32;
    let null_hwnd = HWND(std::ptr::null_mut());
    let screen_dc = GetDC(null_hwnd);

    // ── 色彩 bitmap ──────────────────────────────────────────
    let color_dc  = CreateCompatibleDC(screen_dc);
    let color_bmp = CreateCompatibleBitmap(screen_dc, sz, sz);
    let old_bmp   = SelectObject(color_dc, color_bmp);

    // COLORREF = 0x00BBGGRR  →  blue #3366CC
    let blue  = COLORREF(0x00_CC_66_33);
    let white = COLORREF(0x00_FF_FF_FF);

    // 藍色背景
    let bg = CreateSolidBrush(blue);
    FillRect(color_dc, &RECT { left: 0, top: 0, right: sz, bottom: sz }, bg);
    DeleteObject(bg);

    // 白色筆 + 白色刷
    let pen   = CreatePen(PS_SOLID, 0, white);
    let brush = CreateSolidBrush(white);
    let old_pen   = SelectObject(color_dc, pen);
    let old_brush = SelectObject(color_dc, brush);

    // 快門鍵突起
    RoundRect(color_dc, 11, 4, 21, 10, 3, 3);
    // 相機主體
    RoundRect(color_dc, 2, 8, 30, 26, 5, 5);

    // 鏡頭孔（藍色橢圓蓋在白色主體上）
    let bp = CreatePen(PS_SOLID, 0, blue);
    let bb = CreateSolidBrush(blue);
    SelectObject(color_dc, bp);
    SelectObject(color_dc, bb);
    Ellipse(color_dc, 8, 10, 24, 24);
    DeleteObject(bp);
    DeleteObject(bb);

    // 白色鏡頭環
    SelectObject(color_dc, pen);
    SelectObject(color_dc, brush);
    Ellipse(color_dc, 10, 12, 22, 22);

    // 藍色中心點
    let cp = CreatePen(PS_SOLID, 0, blue);
    let cb = CreateSolidBrush(blue);
    SelectObject(color_dc, cp);
    SelectObject(color_dc, cb);
    Ellipse(color_dc, 13, 15, 19, 19);
    DeleteObject(cp);
    DeleteObject(cb);

    SelectObject(color_dc, old_pen);
    SelectObject(color_dc, old_brush);
    DeleteObject(pen);
    DeleteObject(brush);
    SelectObject(color_dc, old_bmp);
    DeleteDC(color_dc);

    // ── 遮罩 bitmap（全黑 = 完全不透明）──────────────────────
    let mask_dc  = CreateCompatibleDC(screen_dc);
    let mask_bmp = CreateBitmap(sz, sz, 1, 1, None);
    let old_mask = SelectObject(mask_dc, mask_bmp);
    PatBlt(mask_dc, 0, 0, sz, sz, BLACKNESS);
    SelectObject(mask_dc, old_mask);
    DeleteDC(mask_dc);

    ReleaseDC(null_hwnd, screen_dc);

    let info = ICONINFO {
        fIcon:    BOOL(1),
        xHotspot: 0,
        yHotspot: 0,
        hbmMask:  mask_bmp,
        hbmColor: color_bmp,
    };
    let icon = CreateIconIndirect(&info).unwrap();

    DeleteObject(color_bmp);
    DeleteObject(mask_bmp);

    icon
}
