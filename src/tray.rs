use windows::Win32::UI::Shell::{NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW, Shell_NotifyIconW};
use windows::Win32::UI::WindowsAndMessaging::{LoadIconW, IDI_APPLICATION};
use windows::Win32::Foundation::HWND;

pub const WM_TRAYICON: u32 = 0x8000;

pub fn create(hwnd: HWND, icon_id: u16) -> bool {
    let icon = unsafe { LoadIconW(None, IDI_APPLICATION) };

    let mut nid: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = icon_id;
    nid.uFlags = NIF_ICON.0 | NIF_MESSAGE.0 | NIF_TIP.0;
    nid.uCallbackMessage = WM_TRAYICON;
    nid.hIcon = icon;

    let tip: Vec<u16> = "srcshot\0".encode_utf16().collect();
    let mut i = 0;
    for &c in &tip {
        if i < 128 {
            nid.szTip[i] = c;
            i += 1;
        }
    }

    unsafe { Shell_NotifyIconW(NIM_ADD, &nid).as_bool() }
}

pub fn destroy(hwnd: HWND, icon_id: u16) -> bool {
    let mut nid: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = icon_id;
    unsafe { Shell_NotifyIconW(NIM_DELETE, &nid).as_bool() }
}
