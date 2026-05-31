use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotkeyConfig {
    pub fullscreen: String,
    pub active_window: String,
    pub region: String,
    pub select_window: String,
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            fullscreen: "PrintScreen".into(),
            active_window: "Ctrl+Shift+A".into(),
            region: "Ctrl+Shift+R".into(),
            select_window: "Ctrl+Shift+W".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputConfig {
    pub format: String,
    pub directory: String,
    pub filename_template: String,
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            format: "png".into(),
            directory: dirs().into(),
            filename_template: "screenshot_{yyyy}-{mm}-{dd}_{hh}-{ii}-{ss}".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub hotkeys: HotkeyConfig,
    pub output: OutputConfig,
    pub copy_to_clipboard: bool,
    pub show_cursor: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            hotkeys: HotkeyConfig::default(),
            output: OutputConfig::default(),
            copy_to_clipboard: true,
            show_cursor: false,
        }
    }
}

fn dirs() -> String {
    if let Ok(p) = std::env::var("USERPROFILE") {
        let mut path = PathBuf::from(p);
        path.push("Pictures");
        path.push("Screenshots");
        path.to_string_lossy().into()
    } else {
        ".".into()
    }
}

impl Config {
    pub fn load() -> Self {
        let path = config_path();
        if let Ok(s) = std::fs::read_to_string(&path) {
            if let Ok(c) = serde_json::from_str(&s) {
                return c;
            }
        }
        let cfg = Config::default();
        let _ = cfg.save();
        cfg
    }

    pub fn save(&self) -> Result<(), String> {
        let path = config_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let s = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(&path, s).map_err(|e| e.to_string())
    }
}

fn config_path() -> PathBuf {
    if let Ok(p) = std::env::var("APPDATA") {
        let mut path = PathBuf::from(p);
        path.push("srcshot");
        path.push("config.json");
        path
    } else {
        PathBuf::from("config.json")
    }
}
