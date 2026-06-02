use std::path::{Path, PathBuf};

pub struct Config {
    pub save_dir: PathBuf,
}

impl Default for Config {
    fn default() -> Self {
        Self { save_dir: load_save_dir() }
    }
}

/// 讀取上次儲存的目錄（%APPDATA%\srcshot\last_dir.txt），找不到則回傳桌面
pub fn load_save_dir() -> PathBuf {
    config_file()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|s| PathBuf::from(s.trim()))
        .filter(|p| p.is_dir())
        .unwrap_or_else(default_dir)
}

/// 將目錄路徑寫入設定檔
pub fn persist_save_dir(dir: &Path) {
    if let Some(p) = config_file() {
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&p, dir.to_string_lossy().as_bytes());
    }
}

fn config_file() -> Option<PathBuf> {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .map(|p| p.join("srcshot").join("last_dir.txt"))
}

fn default_dir() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .map(|p| p.join("Desktop"))
        .unwrap_or_else(|| PathBuf::from("."))
}
