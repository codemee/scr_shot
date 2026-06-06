/// 主題支援（light / dark）
/// 自動偵測 Windows 暗色模式；可手動強制指定
use std::sync::atomic::{AtomicU8, Ordering};

const LIGHT_U: u8 = 0;
const DARK_U:  u8 = 1;

static THEME: AtomicU8 = AtomicU8::new(LIGHT_U);

#[derive(Clone, Copy, PartialEq)]
pub enum Theme { Light, Dark }

/// 主題相關色彩（COLORREF 格式：0x00BBGGRR）
pub struct ThemeColors {
    pub toolbar_bg: u32,         // 工具列 + 標籤列背景
    pub tab_bar_bg: u32,         // 標籤列外側底色
    pub tab_active: u32,         // 作用中標籤填色
    pub tab_inactive: u32,       // 非作用標籤填色
    pub tab_text_active: u32,    // 作用中標籤文字
    pub tab_text_inactive: u32,  // 非作用標籤文字
    pub tab_close_btn: u32,      // × 按鈕
    pub btn_icon: u32,           // 圖示按鈕圖示（一般）
    pub canvas_bg: u32,          // 畫布外側灰色區域
    pub tooltip_bg: u32,         // Tooltip 背景
    pub red_dot: u32,            // 未存檔紅點
}

/// 偵測 Windows 系統是否啟用暗色模式
fn detect() -> Theme {
    unsafe {
        use windows::Win32::System::Registry::*;
        use windows::core::PCWSTR;
        let key_path: Vec<u16> =
            "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize\0"
            .encode_utf16().collect();
        let val_name: Vec<u16> = "AppsUseLightTheme\0".encode_utf16().collect();
        let mut value: u32 = 1u32;
        let mut cb = std::mem::size_of::<u32>() as u32;
        let _ = RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(key_path.as_ptr()),
            PCWSTR(val_name.as_ptr()),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as *mut _),
            Some(&mut cb),
        );
        if value == 0 { Theme::Dark } else { Theme::Light }
    }
}

pub fn init(setting: &str) {
    let t = match setting {
        "dark"  => Theme::Dark,
        "light" => Theme::Light,
        _       => detect(),
    };
    THEME.store(if t == Theme::Dark { DARK_U } else { LIGHT_U }, Ordering::Relaxed);
    apply_menu_theme();
}

pub fn current() -> Theme {
    if THEME.load(Ordering::Relaxed) == DARK_U { Theme::Dark } else { Theme::Light }
}

pub fn set(t: Theme) {
    THEME.store(if t == Theme::Dark { DARK_U } else { LIGHT_U }, Ordering::Relaxed);
    apply_menu_theme();
}

/// 透過 uxtheme.dll 未公開 API 讓系統功能表跟隨主題
/// ordinal 135 = SetPreferredAppMode(2=ForceDark / 3=ForceLight)
/// ordinal 136 = FlushMenuThemes()
/// 系統層級的深色模式（與系統匣功能表背景同步，與 app 強制主題無關）
pub fn system_is_dark() -> bool {
    detect() == Theme::Dark
}

/// 顯示選單前呼叫：臨時強制 ForceDark/ForceLight，確保選單顏色與來源視窗一致
/// tray 傳 system_is_dark()，editor 傳 current()==Dark
pub fn set_window_dark_menu(hwnd: windows::Win32::Foundation::HWND, dark: bool) {
    unsafe {
        use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
        use windows::core::{PCSTR, PCWSTR};
        use windows::Win32::Foundation::BOOL;
        let name: Vec<u16> = "uxtheme.dll\0".encode_utf16().collect();
        let Ok(hmod) = LoadLibraryW(PCWSTR(name.as_ptr())) else { return };
        // ordinal 135 = SetPreferredAppMode: 2=ForceDark, 3=ForceLight
        if let Some(f) = GetProcAddress(hmod, PCSTR(135usize as *const u8)) {
            let set_mode: unsafe extern "system" fn(i32) -> i32 = std::mem::transmute(f);
            set_mode(if dark { 2 } else { 3 });
        }
        // ordinal 133 = AllowDarkModeForWindow
        if let Some(f) = GetProcAddress(hmod, PCSTR(133usize as *const u8)) {
            let allow: unsafe extern "system" fn(
                windows::Win32::Foundation::HWND, BOOL,
            ) -> BOOL = std::mem::transmute(f);
            allow(hwnd, BOOL(if dark { 1 } else { 0 }));
        }
        // ordinal 136 = FlushMenuThemes
        if let Some(f) = GetProcAddress(hmod, PCSTR(136usize as *const u8)) {
            let flush: unsafe extern "system" fn() = std::mem::transmute(f);
            flush();
        }
    }
}

/// TrackPopupMenu 回傳後呼叫：還原成 AllowDark(1) 基準模式
pub fn restore_after_menu() {
    apply_menu_theme();
}

pub fn apply_menu_theme() {
    unsafe {
        use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
        use windows::core::{PCSTR, PCWSTR};
        let name: Vec<u16> = "uxtheme.dll\0".encode_utf16().collect();
        let Ok(hmod) = LoadLibraryW(PCWSTR(name.as_ptr())) else { return };
        // 1 = AllowDark：讓 Windows 根據系統暗色模式決定，不受 app 主題強制影響
        if let Some(f) = GetProcAddress(hmod, PCSTR(135usize as *const u8)) {
            let set_mode: unsafe extern "system" fn(i32) -> i32 = std::mem::transmute(f);
            set_mode(1);
        }
        if let Some(f) = GetProcAddress(hmod, PCSTR(136usize as *const u8)) {
            let flush: unsafe extern "system" fn() = std::mem::transmute(f);
            flush();
        }
    }
}

pub fn colors() -> ThemeColors {
    match current() {
        Theme::Light => ThemeColors {
            toolbar_bg:         0x00_F0_F0_F0,
            tab_bar_bg:         0x00_D8_D8_D8,
            tab_active:         0x00_F0_F0_F0,
            tab_inactive:       0x00_C8_C8_C8,
            tab_text_active:    0x00_10_10_10,
            tab_text_inactive:  0x00_50_50_50,
            tab_close_btn:      0x00_70_70_70,
            btn_icon:           0x00_40_40_40,
            canvas_bg:          0x00_B0_B0_B0,
            tooltip_bg:         0x00_E1_FF_FF,
            red_dot:            0x00_00_00_CC,
        },
        Theme::Dark => ThemeColors {
            toolbar_bg:         0x00_2D_2D_2D,
            tab_bar_bg:         0x00_1E_1E_1E,
            tab_active:         0x00_3D_3D_3D,
            tab_inactive:       0x00_28_28_28,
            tab_text_active:    0x00_E4_E4_E4,
            tab_text_inactive:  0x00_90_90_90,
            tab_close_btn:      0x00_A0_A0_A0,
            btn_icon:           0x00_C4_C4_C4,
            canvas_bg:          0x00_3C_3C_3C,
            tooltip_bg:         0x00_3A_3A_3A,
            red_dot:            0x00_40_40_FF,
        },
    }
}
