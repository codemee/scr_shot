use image::RgbaImage;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureMode {
    FullScreen,
    ActiveWindow,
    Region,
    SelectWindow,
}

#[derive(Debug, Clone)]
pub struct CapturedImage {
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

impl CapturedImage {
    pub fn to_rgba_image(&self) -> RgbaImage {
        RgbaImage::from_raw(self.width, self.height, self.data.clone())
            .expect("invalid captured image data")
    }
}

pub enum Win32Event {
    CaptureResult(CapturedImage),
    CaptureCancelled,
}

pub enum MainCmd {
    StartCapture(CaptureMode),
    ShowWindow,
    HideWindow,
    Quit,
}
