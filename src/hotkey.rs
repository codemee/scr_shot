use windows::Win32::UI::WindowsAndMessaging::{RegisterHotKey, UnregisterHotKey, MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN, VK_SNAPSHOT};

use crate::app_state::CaptureMode;
use crate::config::Config;

pub struct HotkeyEntry {
    pub id: u32,
    pub mode: CaptureMode,
}

pub fn parse_and_register(hwnd: isize, config: &Config) -> Vec<HotkeyEntry> {
    let mut entries = Vec::new();
    let hwnd = std::mem::transmute(hwnd);

    if let Some((mods, vk)) = parse_hotkey_str(&config.hotkeys.fullscreen) {
        let id = 1;
        if unsafe { RegisterHotKey(hwnd, id, mods, vk).is_ok() } {
            entries.push(HotkeyEntry { id, mode: CaptureMode::FullScreen });
        }
    }

    if let Some((mods, vk)) = parse_hotkey_str(&config.hotkeys.active_window) {
        let id = 2;
        if unsafe { RegisterHotKey(hwnd, id, mods, vk).is_ok() } {
            entries.push(HotkeyEntry { id, mode: CaptureMode::ActiveWindow });
        }
    }

    if let Some((mods, vk)) = parse_hotkey_str(&config.hotkeys.region) {
        let id = 3;
        if unsafe { RegisterHotKey(hwnd, id, mods, vk).is_ok() } {
            entries.push(HotkeyEntry { id, mode: CaptureMode::Region });
        }
    }

    if let Some((mods, vk)) = parse_hotkey_str(&config.hotkeys.select_window) {
        let id = 4;
        if unsafe { RegisterHotKey(hwnd, id, mods, vk).is_ok() } {
            entries.push(HotkeyEntry { id, mode: CaptureMode::SelectWindow });
        }
    }

    entries
}

pub fn unregister_all(hwnd: isize, entries: &[HotkeyEntry]) {
    let hwnd = std::mem::transmute(hwnd);
    for entry in entries {
        unsafe { let _ = UnregisterHotKey(hwnd, entry.id); }
    }
}

fn parse_hotkey_str(s: &str) -> Option<(u32, u16)> {
    let parts: Vec<&str> = s.split('+').collect();
    let mut modifiers = 0u32;
    let mut key = None;

    for part in &parts {
        match *part {
            "Ctrl" => modifiers |= MOD_CONTROL.0,
            "Alt" => modifiers |= MOD_ALT.0,
            "Shift" => modifiers |= MOD_SHIFT.0,
            "Win" => modifiers |= MOD_WIN.0,
            "PrintScreen" => key = Some(VK_SNAPSHOT.0 as u16),
            other => {
                key = Some(char_to_vk(other.chars().next()?)?);
            }
        }
    }

    Some((modifiers, key?))
}

fn char_to_vk(c: char) -> Option<u16> {
    match c {
        'A'..='Z' => Some(c as u16),
        '0'..='9' => Some(c as u16),
        _ => None,
    }
}
