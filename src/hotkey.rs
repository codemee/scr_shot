use anyhow::{Context, Result};
use std::sync::mpsc::Sender;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, MOD_ALT, MOD_SHIFT,
    VK_A, VK_F, VK_R, VK_W,
};

use crate::event::AppEvent;

pub const ID_HOTKEY_REGION: i32 = 1;
pub const ID_HOTKEY_ACTIVE: i32 = 2;
pub const ID_HOTKEY_PICK: i32   = 3;
pub const ID_HOTKEY_FULLSCREEN: i32 = 4;

pub fn register_all(msg_hwnd: HWND) -> Result<()> {
    unsafe {
        RegisterHotKey(msg_hwnd, ID_HOTKEY_REGION, MOD_ALT | MOD_SHIFT, VK_R.0 as u32)
            .context("RegisterHotKey R")?;
        RegisterHotKey(msg_hwnd, ID_HOTKEY_ACTIVE, MOD_ALT | MOD_SHIFT, VK_A.0 as u32)
            .context("RegisterHotKey A")?;
        RegisterHotKey(msg_hwnd, ID_HOTKEY_PICK,   MOD_ALT | MOD_SHIFT, VK_W.0 as u32)
            .context("RegisterHotKey W")?;
        RegisterHotKey(msg_hwnd, ID_HOTKEY_FULLSCREEN, MOD_ALT | MOD_SHIFT, VK_F.0 as u32)
            .context("RegisterHotKey F")?;
    }
    Ok(())
}

pub fn unregister_all(msg_hwnd: HWND) {
    unsafe {
        let _ = UnregisterHotKey(msg_hwnd, ID_HOTKEY_REGION);
        let _ = UnregisterHotKey(msg_hwnd, ID_HOTKEY_ACTIVE);
        let _ = UnregisterHotKey(msg_hwnd, ID_HOTKEY_PICK);
        let _ = UnregisterHotKey(msg_hwnd, ID_HOTKEY_FULLSCREEN);
    }
}

pub fn handle_wm_hotkey(id: i32, tx: &Sender<AppEvent>) {
    match id {
        ID_HOTKEY_REGION => { let _ = tx.send(AppEvent::CaptureRegion); }
        ID_HOTKEY_ACTIVE => { let _ = tx.send(AppEvent::CaptureActiveWindow); }
        ID_HOTKEY_PICK   => { let _ = tx.send(AppEvent::CapturePickWindow); }
        ID_HOTKEY_FULLSCREEN => { let _ = tx.send(AppEvent::CaptureFullscreen); }
        _ => {}
    }
}
