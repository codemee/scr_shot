use std::sync::mpsc::Sender;
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE,
    NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::event::AppEvent;

pub const WM_TRAY: u32 = WM_APP + 1;

const IDM_REGION: u32 = 100;
const IDM_ACTIVE: u32 = 101;
const IDM_PICK: u32   = 102;
const IDM_QUIT: u32   = 200;

pub struct Tray {
    hwnd: HWND,
    icon: HICON,
}

impl Tray {
    pub fn add(&self) {
        unsafe {
            let mut nid = nid_base(self.hwnd);
            nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
            nid.uCallbackMessage = WM_TRAY;
            nid.hIcon = self.icon;
            let tip = "srcshot";
            let bytes: Vec<u16> = tip.encode_utf16().chain(std::iter::once(0)).collect();
            let len = bytes.len().min(128);
            nid.szTip[..len].copy_from_slice(&bytes[..len]);
            Shell_NotifyIconW(NIM_ADD, &nid);
        }
    }

    pub fn remove(&self) {
        unsafe {
            let nid = nid_base(self.hwnd);
            Shell_NotifyIconW(NIM_DELETE, &nid);
        }
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        self.remove();
        unsafe { DestroyIcon(self.icon).ok(); }
    }
}

pub fn create_message_window(tx: Sender<AppEvent>) -> HWND {
    unsafe {
        let class = w!("srcshot_msg");
        let hinstance = get_instance();

        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(msg_wnd_proc),
            hInstance: hinstance,
            lpszClassName: class,
            ..Default::default()
        };
        RegisterClassExW(&wc);

        let tx_box = Box::new(tx);
        let hwnd = CreateWindowExW(
            Default::default(),
            class,
            w!("srcshot"),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT, CW_USEDEFAULT, CW_USEDEFAULT, CW_USEDEFAULT,
            HWND_MESSAGE,
            HMENU(std::ptr::null_mut()),
            hinstance,
            Some(Box::into_raw(tx_box) as _),
        )
        .expect("CreateWindowExW failed");

        hwnd
    }
}

pub fn make_tray(hwnd: HWND) -> Tray {
    let icon = unsafe { crate::icon::create_app_icon() };
    let t = Tray { hwnd, icon };
    t.add();
    t
}

unsafe extern "system" fn msg_wnd_proc(
    hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM,
) -> LRESULT {
    match msg {
        WM_NCCREATE => {
            let cs = &*(lp.0 as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as _);
            LRESULT(1)
        }
        WM_TRAY => {
            let event = lp.0 as u32 & 0xFFFF;
            if event == WM_RBUTTONUP || event == WM_CONTEXTMENU {
                show_context_menu(hwnd);
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            let tx = &*(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Sender<AppEvent>);
            match wp.0 as u32 {
                IDM_REGION => { let _ = tx.send(AppEvent::CaptureRegion); }
                IDM_ACTIVE => { let _ = tx.send(AppEvent::CaptureActiveWindow); }
                IDM_PICK   => { let _ = tx.send(AppEvent::CapturePickWindow); }
                IDM_QUIT   => {
                    let _ = tx.send(AppEvent::TrayQuit);
                    PostQuitMessage(0);
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_HOTKEY => {
            let tx = &*(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Sender<AppEvent>);
            crate::hotkey::handle_wm_hotkey(wp.0 as i32, tx);
            LRESULT(0)
        }
        WM_DESTROY => {
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Sender<AppEvent>;
            if !ptr.is_null() {
                drop(Box::from_raw(ptr));
            }
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

unsafe fn show_context_menu(hwnd: HWND) {
    let hmenu = CreatePopupMenu().unwrap();
    let _ = AppendMenuW(hmenu, MF_STRING, IDM_REGION as usize, w!("框選區域 (Alt+Shift+R)"));
    let _ = AppendMenuW(hmenu, MF_STRING, IDM_ACTIVE as usize, w!("作用中視窗 (Alt+Shift+A)"));
    let _ = AppendMenuW(hmenu, MF_STRING, IDM_PICK as usize,   w!("點選視窗 (Alt+Shift+W)"));
    let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, None);
    let _ = AppendMenuW(hmenu, MF_STRING, IDM_QUIT as usize, w!("結束"));

    let mut pt = windows::Win32::Foundation::POINT::default();
    let _ = GetCursorPos(&mut pt);
    SetForegroundWindow(hwnd);
    TrackPopupMenu(hmenu, TPM_RIGHTBUTTON, pt.x, pt.y, 0, hwnd, None);
    let _ = DestroyMenu(hmenu);
}

fn nid_base(hwnd: HWND) -> NOTIFYICONDATAW {
    let mut nid = NOTIFYICONDATAW::default();
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = 1;
    nid
}

fn get_instance() -> windows::Win32::Foundation::HINSTANCE {
    unsafe {
        windows::Win32::System::LibraryLoader::GetModuleHandleW(None)
            .unwrap()
            .into()
    }
}
