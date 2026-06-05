/// 多語系支援（zh / en）
/// 使用 AtomicU8 儲存，允許執行中切換語言
use std::sync::atomic::{AtomicU8, Ordering};

const ZH: u8 = 0;
const EN: u8 = 1;

static LANG: AtomicU8 = AtomicU8::new(ZH);

#[derive(Clone, Copy, PartialEq)]
pub enum Lang { Zh, En }

impl Lang {
    fn as_u8(self) -> u8 { if self == Lang::Zh { ZH } else { EN } }
}

/// 偵測系統使用語言（zh-* → Zh，其餘 → En）
fn detect() -> Lang {
    unsafe {
        let mut buf = [0u16; 85];
        let len = windows::Win32::Globalization::GetUserDefaultLocaleName(&mut buf);
        if len > 0 {
            let name = String::from_utf16_lossy(&buf[..len as usize]);
            if name.starts_with("zh") { return Lang::Zh; }
        }
        Lang::En
    }
}

/// 初始化語言設定（"zh"、"en" 或 "auto"）
pub fn init(setting: &str) {
    let lang = match setting {
        "zh"  => Lang::Zh,
        "en"  => Lang::En,
        _     => detect(), // "auto" 或空值
    };
    LANG.store(lang.as_u8(), Ordering::Relaxed);
}

/// 取得目前語言
pub fn current() -> Lang {
    if LANG.load(Ordering::Relaxed) == ZH { Lang::Zh } else { Lang::En }
}

/// 切換至指定語言（設定後立即生效）
pub fn set(lang: Lang) {
    LANG.store(lang.as_u8(), Ordering::Relaxed);
}

/// 依目前語言回傳靜態字串
pub fn t(zh: &'static str, en: &'static str) -> &'static str {
    if current() == Lang::Zh { zh } else { en }
}

/// 依目前語言回傳 null-terminated Vec<u16>，用於 Win32 API（PCWSTR）
/// 呼叫方須將回傳值存入變數再取 .as_ptr()，避免懸空指標。
pub fn tw(zh: &'static str, en: &'static str) -> Vec<u16> {
    t(zh, en).encode_utf16().chain(Some(0u16)).collect()
}
