use windows::Win32::Foundation::RECT;

#[derive(Debug)]
pub enum AppEvent {
    CaptureRegion,
    CaptureActiveWindow,
    CapturePickWindow,
    OverlayCancelled,
    RegionSelected(RECT),
    WindowPicked(isize), // HWND.0 as isize — avoids Send bound on *mut c_void
    EditorSave,
    ShowEditor, // 雙按系統匣圖示 → 顯示編輯視窗
    TrayQuit,
}
