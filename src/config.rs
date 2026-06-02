use std::path::{Path, PathBuf};

pub struct Config {
    pub save_dir: PathBuf,
    pub capture_cursor: bool,
    pub capture_delay_secs: u32,
    pub auto_copy: bool,       // 擷取後直接複製到剪貼簿，跳過編輯器
}

impl Default for Config {
    fn default() -> Self {
        let (cursor, delay, auto_copy) = load_settings();
        Self {
            save_dir: load_save_dir(),
            capture_cursor: cursor,
            capture_delay_secs: delay,
            auto_copy,
        }
    }
}

// ── 儲存目錄（由編輯器執行緒寫入）──────────────────────────────────────

pub fn load_save_dir() -> PathBuf {
    config_dir()
        .map(|p| p.join("last_dir.txt"))
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|s| PathBuf::from(s.trim()))
        .filter(|p| p.is_dir())
        .unwrap_or_else(default_dir)
}

pub fn persist_save_dir(dir: &Path) {
    if let Some(base) = config_dir() {
        let _ = std::fs::create_dir_all(&base);
        let _ = std::fs::write(base.join("last_dir.txt"), dir.to_string_lossy().as_bytes());
    }
}

// ── 擷取設定（由系統匣執行緒讀寫）──────────────────────────────────────

fn load_settings() -> (bool, u32, bool) {
    let mut cursor    = false;
    let mut delay     = 0u32;
    let mut auto_copy = false;
    let path = match config_dir().map(|p| p.join("settings.ini")) {
        Some(p) => p,
        None => return (cursor, delay, auto_copy),
    };
    if let Ok(content) = std::fs::read_to_string(&path) {
        for line in content.lines() {
            if let Some(v) = line.strip_prefix("capture_cursor=") {
                cursor = v == "1";
            } else if let Some(v) = line.strip_prefix("capture_delay_secs=") {
                delay = v.parse().unwrap_or(0);
            } else if let Some(v) = line.strip_prefix("auto_copy=") {
                auto_copy = v == "1";
            }
        }
    }
    (cursor, delay, auto_copy)
}

pub fn persist_settings(config: &Config) {
    if let Some(base) = config_dir() {
        let _ = std::fs::create_dir_all(&base);
        let content = format!(
            "capture_cursor={}\ncapture_delay_secs={}\nauto_copy={}\n",
            if config.capture_cursor { 1 } else { 0 },
            config.capture_delay_secs,
            if config.auto_copy { 1 } else { 0 },
        );
        let _ = std::fs::write(base.join("settings.ini"), content.as_bytes());
    }
}

// ── 共用輔助 ───────────────────────────────────────────────────────────

fn config_dir() -> Option<PathBuf> {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .map(|p| p.join("srcshot"))
}

fn default_dir() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .map(|p| p.join("Desktop"))
        .unwrap_or_else(|| PathBuf::from("."))
}
