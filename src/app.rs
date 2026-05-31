use std::sync::mpsc;
use std::path::PathBuf;

use eframe::egui;
use image::RgbaImage;

use crate::app_state::{CaptureMode, CapturedImage, MainCmd, Win32Event};
use crate::clipboard;
use crate::config::Config;
use crate::editor::panel::EditorPanel;

pub struct ScreenshotApp {
    tx: mpsc::Sender<MainCmd>,
    rx: mpsc::Receiver<Win32Event>,
    config: Config,
    editor: Option<EditorPanel>,
    visible: bool,
    last_capture: Option<CapturedImage>,
}

impl ScreenshotApp {
    pub fn new(config: Config, tx: mpsc::Sender<MainCmd>, rx: mpsc::Receiver<Win32Event>) -> Self {
        Self {
            tx, rx, config,
            editor: None,
            visible: false,
            last_capture: None,
        }
    }
}

impl eframe::App for ScreenshotApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                Win32Event::CaptureResult(cap) => {
                    let mode = self.detect_mode();
                    self.last_capture = Some(cap.clone());
                    self.editor = Some(EditorPanel::new(&cap, mode));
                    self.visible = true;
                    frame.set_visible(true);
                }
                Win32Event::CaptureCancelled => {
                    self.visible = false;
                    frame.set_visible(false);
                }
                Win32Event::ShowWindow => {
                    self.visible = true;
                    frame.set_visible(true);
                }
                Win32Event::HideWindow => {
                    self.visible = false;
                    frame.set_visible(false);
                }
            }
        }

        if !self.visible {
            ctx.request_repaint();
            return;
        }

        if let Some(editor) = &mut self.editor {
            let save_happened = std::cell::Cell::new(false);
            let cancel_happened = std::cell::Cell::new(false);

            {
                let tx = self.tx.clone();
                let config_dir = self.config.output.directory.clone();
                let config_fmt = self.config.output.format.clone();
                let clip = self.config.copy_to_clipboard;

                let mut save_cb = |img: &RgbaImage| {
                    let name = generate_filename(&config_fmt);
                    let mut path = PathBuf::from(&config_dir);
                    path.push(&name);
                    let fmt = if config_fmt == "png" { image::ImageFormat::Png }
                             else { image::ImageFormat::Jpeg };

                    if let Err(e) = img.save_with_format(&path, fmt) {
                        eprintln!("save failed: {e}");
                    } else {
                        println!("saved: {}", path.display());
                    }

                    if clip {
                        if let Err(e) = clipboard::copy_image(img) {
                            eprintln!("clipboard failed: {e}");
                        }
                    }

                    save_happened.set(true);
                };

                let mut cancel_cb = || {
                    cancel_happened.set(true);
                };

                editor.show(ctx, &mut save_cb, &mut cancel_cb);

                if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                    cancel_happened.set(true);
                }
            }

            if save_happened.get() || cancel_happened.get() {
                self.editor = None;
                self.visible = false;
                frame.set_visible(false);
            }
        } else {
            egui::CentralPanel::default().show(ctx, |_ui| {});
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        let _ = self.tx.send(MainCmd::Quit);
    }
}

impl ScreenshotApp {
    fn detect_mode(&self) -> CaptureMode {
        CaptureMode::Region
    }
}

fn generate_filename(format: &str) -> String {
    let ext = if format == "png" { "png" } else { "jpg" };
    let now = chrono_now();
    format!("screenshot_{}_{}_{}_{}_{}_{}.{}",
        now.year, now.month, now.day,
        now.hour, now.minute, now.second,
        ext,
    )
}

struct Tm {
    year: u32, month: u32, day: u32,
    hour: u32, minute: u32, second: u32,
}

fn chrono_now() -> Tm {
    use std::time::{SystemTime, UNIX_EPOCH};
    let d = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let secs_per_day = 86400;
    let days = d / secs_per_day as u64;

    let mut y = 1970i64;
    let mut remaining = days as i64;

    loop {
        let days_in_year = if is_leap(y) { 366 } else { 365 };
        if remaining < days_in_year { break; }
        remaining -= days_in_year;
        y += 1;
    }

    let leap = is_leap(y);
    let mdays = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut m = 0;
    while m < 12 && remaining >= mdays[m] {
        remaining -= mdays[m];
        m += 1;
    }

    let day = remaining + 1;
    let sec_rem = d % secs_per_day;
    let hour = (sec_rem / 3600) as u32;
    let minute = ((sec_rem % 3600) / 60) as u32;
    let second = (sec_rem % 60) as u32;

    Tm {
        year: y as u32,
        month: (m + 1) as u32,
        day: day as u32,
        hour, minute, second,
    }
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}
