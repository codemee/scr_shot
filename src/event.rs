use std::path::PathBuf;
use windows::Win32::Foundation::RECT;

#[derive(Debug)]
pub enum AppEvent {
    CaptureRegion,
    CaptureActiveWindow,
    CapturePickWindow,
    OverlayCancelled,
    RegionSelected(RECT),
    WindowPicked(isize), // HWND.0 as isize — avoids Send bound on *mut c_void
    EditorSave { to_clipboard: bool, path: Option<PathBuf> },
    EditorCancelled,
    TrayQuit,
}
