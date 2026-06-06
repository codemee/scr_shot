/// 選單項目小圖示（16×16 32bpp alpha bitmap）
/// 使用方式：
///   let icons = MenuIcons::build();
///   icons.apply(hmenu);
///   TrackPopupMenu(...);
///   // icons Drop 時自動刪除 bitmap
use std::sync::atomic::{AtomicBool, Ordering};
use windows::Win32::Foundation::{BOOL, COLORREF, HWND, RECT};

/// 建立圖示前先呼叫，告知目前選單的深色模式（tray 用系統、editor 用 app 主題）
static ICON_DARK: AtomicBool = AtomicBool::new(false);
pub fn set_icon_dark(dark: bool) {
    ICON_DARK.store(dark, Ordering::Relaxed);
}
use windows::Win32::Graphics::Gdi::{
    BeginPath, CreateCompatibleDC, CreateDIBSection, CreatePen, CreateSolidBrush,
    DeleteDC, DeleteObject, Ellipse, EndPath, FillPath, FillRect,
    GetDC, GetStockObject, LineTo, MoveToEx, NULL_BRUSH, Polygon, Rectangle,
    ReleaseDC, SelectObject, SetPixel,
    BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, PS_SOLID, HDC, HBRUSH,
};
use windows::Win32::UI::WindowsAndMessaging::{
    SetMenuItemInfoW, MENUITEMINFOW, MIIM_BITMAP, HMENU,
};

const SZ: i32 = 16;

/// 一個選單圖示（持有 HBITMAP，Drop 時釋放）
pub struct Icon(pub windows::Win32::Graphics::Gdi::HBITMAP);

impl Drop for Icon {
    fn drop(&mut self) {
        unsafe { DeleteObject(self.0); }
    }
}

/// 切換型圖示的顏色：開啟 → 藍色強調色，關閉 → 淡灰
/// dark 由呼叫端傳入：tray 用系統深色模式，editor 用 app 主題
fn toggle_fg(checked: bool, dark: bool) -> u32 {
    if checked {
        0x00_D4_78_00u32 // Windows 藍 #0078D4
    } else if dark {
        0x00_A0_A0_A0u32 // 深色背景：淡中灰
    } else {
        0x00_C8_C8_C8u32 // 淺色背景：更淡的灰
    }
}

/// 建立一個 16×16 的 icon bitmap，使用指定前景色
unsafe fn make_fg(fg: u32, draw_fn: impl FnOnce(HDC, u32)) -> Icon {
    let null_hwnd = HWND(std::ptr::null_mut());
    let screen_dc = GetDC(null_hwnd);
    let mem_dc = CreateCompatibleDC(screen_dc);

    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: SZ, biHeight: -SZ,
            biPlanes: 1, biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        bmiColors: [Default::default()],
    };
    let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
    let bmp = CreateDIBSection(screen_dc, &bmi, DIB_RGB_COLORS, &mut bits, None, 0).unwrap();
    let old = SelectObject(mem_dc, bmp);

    // 清空（透明）
    let clear = CreateSolidBrush(COLORREF(0));
    FillRect(mem_dc, &RECT{left:0,top:0,right:SZ,bottom:SZ}, clear);
    DeleteObject(clear);

    draw_fn(mem_dc, fg);

    // 修正 alpha 通道：非黑色 → alpha=255
    {
        let n = (SZ * SZ) as usize;
        let pixels = std::slice::from_raw_parts_mut(bits as *mut u8, n * 4);
        for i in 0..n {
            let off = i * 4;
            let b = pixels[off]; let g = pixels[off+1]; let r = pixels[off+2];
            pixels[off+3] = if r>0||g>0||b>0 {255} else {0};
        }
    }

    SelectObject(mem_dc, old);
    DeleteDC(mem_dc);
    ReleaseDC(null_hwnd, screen_dc);

    Icon(bmp)
}

/// 建立圖示，使用目前 ICON_DARK 設定的前景色（非切換型項目）
unsafe fn make(draw_fn: impl FnOnce(HDC, u32)) -> Icon {
    let fg = if ICON_DARK.load(Ordering::Relaxed) {
        0x00_C8_C8_C8u32
    } else {
        0x00_30_30_30u32
    };
    make_fg(fg, draw_fn)
}

/// 將 icon 套用到選單項目（by command ID）
/// 依 command ID 套用（用於一般 MF_STRING 項目）
pub unsafe fn apply(hmenu: HMENU, id: u32, icon: &Icon) {
    let mut mii = MENUITEMINFOW::default();
    mii.cbSize = std::mem::size_of::<MENUITEMINFOW>() as u32;
    mii.fMask  = MIIM_BITMAP;
    mii.hbmpItem = icon.0;
    SetMenuItemInfoW(hmenu, id, BOOL(0), &mii).ok();
}

/// 依位置索引套用（用於 MF_POPUP 子選單父項，無法以 ID 識別）
pub unsafe fn apply_at(hmenu: HMENU, pos: u32, icon: &Icon) {
    let mut mii = MENUITEMINFOW::default();
    mii.cbSize = std::mem::size_of::<MENUITEMINFOW>() as u32;
    mii.fMask  = MIIM_BITMAP;
    mii.hbmpItem = icon.0;
    SetMenuItemInfoW(hmenu, pos, BOOL(1), &mii).ok(); // BOOL(1) = fByPosition
}

// ── 具體圖示繪製 ──────────────────────────────────────────────────────────

/// 游標捕獲圖示（切換型）：開啟藍色、關閉灰色
pub unsafe fn icon_cursor_toggle(checked: bool, dark: bool) -> Icon {
    make_fg(toggle_fg(checked, dark), |dc, fg| {
        let pen = CreatePen(PS_SOLID, 1, COLORREF(fg));
        let nb  = SelectObject(dc, HBRUSH(GetStockObject(NULL_BRUSH).0));
        let op  = SelectObject(dc, pen);
        let _ = Polygon(dc, &[
            windows::Win32::Foundation::POINT{x:3,y:2},
            windows::Win32::Foundation::POINT{x:3,y:11},
            windows::Win32::Foundation::POINT{x:6,y:8},
            windows::Win32::Foundation::POINT{x:9,y:13},
            windows::Win32::Foundation::POINT{x:11,y:12},
            windows::Win32::Foundation::POINT{x:8,y:7},
            windows::Win32::Foundation::POINT{x:11,y:5},
        ]);
        SelectObject(dc, op); SelectObject(dc, nb);
        DeleteObject(pen);
    })
}

/// 剪貼簿圖示（切換型）
pub unsafe fn icon_clipboard_toggle(checked: bool, dark: bool) -> Icon {
    make_fg(toggle_fg(checked, dark), |dc, fg| {
        let pen = CreatePen(PS_SOLID, 1, COLORREF(fg));
        let nb  = SelectObject(dc, HBRUSH(GetStockObject(NULL_BRUSH).0));
        let op  = SelectObject(dc, pen);
        Rectangle(dc, 3, 4, 12, 13);
        let _ = MoveToEx(dc, 5, 4, None); let _ = LineTo(dc, 5, 2);
        let _ = LineTo(dc, 10, 2); let _ = LineTo(dc, 10, 4);
        SelectObject(dc, op); SelectObject(dc, nb);
        DeleteObject(pen);
    })
}

/// 隱藏圖示（切換型）
pub unsafe fn icon_hide_toggle(checked: bool, dark: bool) -> Icon {
    make_fg(toggle_fg(checked, dark), |dc, fg| {
        let pen = CreatePen(PS_SOLID, 1, COLORREF(fg));
        let nb  = SelectObject(dc, HBRUSH(GetStockObject(NULL_BRUSH).0));
        let op  = SelectObject(dc, pen);
        let _ = MoveToEx(dc, 1, 7, None);
        let _ = LineTo(dc, 7, 3); let _ = LineTo(dc, 13, 7);
        let _ = LineTo(dc, 7, 11); let _ = LineTo(dc, 1, 7);
        let _ = MoveToEx(dc, 2, 13, None); let _ = LineTo(dc, 12, 3);
        SelectObject(dc, op); SelectObject(dc, nb);
        DeleteObject(pen);
    })
}

pub unsafe fn icon_region() -> Icon {
    make(|dc, fg| {
        let pen = CreatePen(PS_SOLID, 1, COLORREF(fg));
        let nb  = SelectObject(dc, HBRUSH(GetStockObject(NULL_BRUSH).0));
        let op  = SelectObject(dc, pen);
        Rectangle(dc, 2, 3, 13, 11);
        // 四角缺口（選取框風格）
        for (x,y) in [(2,3),(11,3),(2,9),(11,9)] {
            SetPixel(dc, x, y, COLORREF(0));
        }
        SelectObject(dc, op); SelectObject(dc, nb);
        DeleteObject(pen);
    })
}

pub unsafe fn icon_active_window() -> Icon {
    make(|dc, fg| {
        let pen = CreatePen(PS_SOLID, 1, COLORREF(fg));
        let nb  = SelectObject(dc, HBRUSH(GetStockObject(NULL_BRUSH).0));
        let op  = SelectObject(dc, pen);
        Rectangle(dc, 2, 3, 13, 12); // 視窗外框
        let _ = MoveToEx(dc, 2, 6, None); let _ = LineTo(dc, 13, 6); // 標題列分隔線
        SelectObject(dc, op); SelectObject(dc, nb);
        DeleteObject(pen);
    })
}

pub unsafe fn icon_pick_window() -> Icon {
    make(|dc, fg| {
        // 游標箭頭
        let pen = CreatePen(PS_SOLID, 1, COLORREF(fg));
        let br  = CreateSolidBrush(COLORREF(fg));
        let op  = SelectObject(dc, pen);
        let ob  = SelectObject(dc, br);
        let _ = Polygon(dc, &[
            windows::Win32::Foundation::POINT{x:3,y:2},
            windows::Win32::Foundation::POINT{x:3,y:11},
            windows::Win32::Foundation::POINT{x:6,y:8},
            windows::Win32::Foundation::POINT{x:9,y:13},
            windows::Win32::Foundation::POINT{x:11,y:12},
            windows::Win32::Foundation::POINT{x:8,y:7},
            windows::Win32::Foundation::POINT{x:11,y:5},
        ]);
        SelectObject(dc, op); SelectObject(dc, ob);
        DeleteObject(pen); DeleteObject(br);
    })
}

pub unsafe fn icon_clock() -> Icon {
    make(|dc, fg| {
        let pen = CreatePen(PS_SOLID, 1, COLORREF(fg));
        let nb  = SelectObject(dc, HBRUSH(GetStockObject(NULL_BRUSH).0));
        let op  = SelectObject(dc, pen);
        Ellipse(dc, 2, 2, 13, 13); // 錶盤
        let _ = MoveToEx(dc, 7, 4, None); let _ = LineTo(dc, 7, 7);
        let _ = LineTo(dc, 10, 9); // 時針/分針
        SelectObject(dc, op); SelectObject(dc, nb);
        DeleteObject(pen);
    })
}

pub unsafe fn icon_language() -> Icon {
    make(|dc, fg| {
        // 地球 + 線條
        let pen = CreatePen(PS_SOLID, 1, COLORREF(fg));
        let nb  = SelectObject(dc, HBRUSH(GetStockObject(NULL_BRUSH).0));
        let op  = SelectObject(dc, pen);
        Ellipse(dc, 2, 2, 13, 13);
        let _ = MoveToEx(dc, 7, 2, None); let _ = LineTo(dc, 7, 13);
        let _ = MoveToEx(dc, 2, 7, None); let _ = LineTo(dc, 13, 7);
        // 橢圓表示緯線
        Ellipse(dc, 4, 2, 10, 13);
        SelectObject(dc, op); SelectObject(dc, nb);
        DeleteObject(pen);
    })
}

pub unsafe fn icon_theme_auto() -> Icon {
    make(|dc, fg| {
        let pen = CreatePen(PS_SOLID, 1, COLORREF(fg));
        let br  = CreateSolidBrush(COLORREF(fg));
        let op  = SelectObject(dc, pen);
        let ob  = SelectObject(dc, br);
        // 半圓（左實右空）
        let _ = BeginPath(dc);
        let _ = MoveToEx(dc, 7, 2, None);
        let _ = windows::Win32::Graphics::Gdi::ArcTo(dc, 2,2,12,12, 7,2, 7,12);
        let _ = LineTo(dc, 7, 2);
        let _ = EndPath(dc);
        let _ = FillPath(dc);
        let nb2 = SelectObject(dc, GetStockObject(NULL_BRUSH));
        Ellipse(dc, 2, 2, 12, 12);
        SelectObject(dc, nb2);
        SelectObject(dc, op); SelectObject(dc, ob);
        DeleteObject(pen); DeleteObject(br);
    })
}

pub unsafe fn icon_quit() -> Icon {
    make(|dc, fg| {
        let pen = CreatePen(PS_SOLID, 2, COLORREF(fg));
        let op  = SelectObject(dc, pen);
        let _ = MoveToEx(dc, 3, 3, None); let _ = LineTo(dc, 12, 12);
        let _ = MoveToEx(dc, 12, 3, None); let _ = LineTo(dc, 3, 12);
        SelectObject(dc, op);
        DeleteObject(pen);
    })
}
