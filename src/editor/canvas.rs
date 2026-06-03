use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC,
    DeleteDC, DeleteObject, SelectObject, HBITMAP, HDC, SRCCOPY,
};

use crate::capture::screen::ScreenBitmap;
use super::tool::{Color, Stroke};

enum UndoOp {
    Stroke,
    Crop {
        base:    ScreenBitmap,
        width:   i32,
        height:  i32,
        strokes: Vec<(Stroke, Color, i32)>,
    },
}

pub struct Canvas {
    pub width: i32,
    pub height: i32,
    // Base screenshot stored as raw BGRA
    base: ScreenBitmap,
    pub strokes: Vec<(Stroke, Color, i32)>, // (stroke, color, thickness)
    // In-progress stroke
    pub current: Option<Stroke>,
    pub tool_color: u32,   // COLORREF
    pub tool_thickness: i32,
    undo_ops: Vec<UndoOp>,
}

impl Canvas {
    pub fn new(base: ScreenBitmap) -> Self {
        let w = base.width;
        let h = base.height;
        Self {
            width: w,
            height: h,
            base,
            strokes: Vec::new(),
            current: None,
            tool_color: 0x00_00_00_FF, // red
            tool_thickness: 3,
            undo_ops: Vec::new(),
        }
    }

    /// Render everything onto `hdc` (should be a compat DC of the same size).
    /// `crop_mode`：裁切工具拖曳中，current stroke 改用固定白色 1px（與繪圖顏色無關）
    pub unsafe fn render(&self, hdc: HDC, screen_dc: HDC, crop_mode: bool) {
        // Paint base bitmap into hdc via a temp DC
        let mem_dc = CreateCompatibleDC(screen_dc);
        let bmp = bgra_to_hbitmap(screen_dc, &self.base);
        let old = SelectObject(mem_dc, bmp);
        BitBlt(hdc, 0, 0, self.width, self.height, mem_dc, 0, 0, SRCCOPY).unwrap();
        SelectObject(mem_dc, old);
        DeleteObject(bmp);
        DeleteDC(mem_dc);

        // Draw committed strokes
        for (stroke, color, thickness) in &self.strokes {
            stroke.draw(hdc, Color(color.0), *thickness);
        }

        // Draw in-progress stroke
        if let Some(ref s) = self.current {
            if crop_mode {
                // 雙層框線：黑色外框 + 白色內框，任何背景下都可見
                s.draw(hdc, Color(0x00_00_00_00), 3);
                s.draw(hdc, Color(0x00_FF_FF_FF), 1);
            } else {
                s.draw(hdc, Color(self.tool_color), self.tool_thickness);
            }
        }
    }

    /// 加入一筆筆畫，並記錄到 undo stack
    pub fn push_stroke(&mut self, stroke: Stroke, color: Color, thickness: i32) {
        self.strokes.push((stroke, color, thickness));
        self.undo_ops.push(UndoOp::Stroke);
    }

    /// 復原：移除最後一筆畫，或還原最後一次裁切
    pub fn undo(&mut self) {
        match self.undo_ops.pop() {
            Some(UndoOp::Stroke) => { self.strokes.pop(); }
            Some(UndoOp::Crop { base, width, height, strokes }) => {
                self.base    = base;
                self.width   = width;
                self.height  = height;
                self.strokes = strokes;
            }
            None => {}
        }
    }

    /// 裁切畫布：縮小 base bitmap，並平移所有筆畫座標
    pub fn crop(&mut self, r: windows::Win32::Foundation::RECT) {
        let x = r.left.clamp(0, self.width);
        let y = r.top.clamp(0, self.height);
        let w = (r.right.clamp(0, self.width) - x).max(0);
        let h = (r.bottom.clamp(0, self.height) - y).max(0);
        if w <= 0 || h <= 0 { return; }

        // 裁切前先儲存快照供 undo 使用
        self.undo_ops.push(UndoOp::Crop {
            base:    self.base.clone(),
            width:   self.width,
            height:  self.height,
            strokes: self.strokes.clone(),
        });

        let mut new_data = vec![0u8; (w * h * 4) as usize];
        for row in 0..h {
            let src = ((y + row) * self.width + x) as usize * 4;
            let dst = (row * w) as usize * 4;
            new_data[dst..dst + (w as usize * 4)]
                .copy_from_slice(&self.base.data[src..src + (w as usize * 4)]);
        }
        self.base.data   = new_data;
        self.base.width  = w;
        self.base.height = h;
        self.width  = w;
        self.height = h;

        for (stroke, _, _) in &mut self.strokes {
            stroke.translate(-x, -y);
        }
        self.current = None;
    }

    pub fn flatten_to_bitmap(&self) -> ScreenBitmap {
        // Return base + strokes composited via GDI
        // For simplicity, we render into a memory DC and read back pixels
        unsafe {
            let screen_dc = windows::Win32::Graphics::Gdi::GetDC(
                windows::Win32::Foundation::HWND(std::ptr::null_mut()),
            );
            let mem_dc = CreateCompatibleDC(screen_dc);
            let bmp = CreateCompatibleBitmap(screen_dc, self.width, self.height);
            let old = SelectObject(mem_dc, bmp);

            self.render(mem_dc, screen_dc, false);

            // Read back pixels
            let mut info = windows::Win32::Graphics::Gdi::BITMAPINFO {
                bmiHeader: windows::Win32::Graphics::Gdi::BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<windows::Win32::Graphics::Gdi::BITMAPINFOHEADER>() as u32,
                    biWidth: self.width,
                    biHeight: -self.height,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: windows::Win32::Graphics::Gdi::BI_RGB.0,
                    ..Default::default()
                },
                bmiColors: [Default::default()],
            };
            let mut pixels = vec![0u8; (self.width * self.height * 4) as usize];
            windows::Win32::Graphics::Gdi::GetDIBits(
                mem_dc, bmp, 0, self.height as u32,
                Some(pixels.as_mut_ptr() as _),
                &mut info,
                windows::Win32::Graphics::Gdi::DIB_RGB_COLORS,
            );

            SelectObject(mem_dc, old);
            DeleteObject(bmp);
            DeleteDC(mem_dc);
            windows::Win32::Graphics::Gdi::ReleaseDC(
                windows::Win32::Foundation::HWND(std::ptr::null_mut()), screen_dc,
            );

            ScreenBitmap { width: self.width, height: self.height, data: pixels }
        }
    }
}

unsafe fn bgra_to_hbitmap(screen_dc: HDC, bmp: &ScreenBitmap) -> HBITMAP {
    let info = windows::Win32::Graphics::Gdi::BITMAPINFO {
        bmiHeader: windows::Win32::Graphics::Gdi::BITMAPINFOHEADER {
            biSize: std::mem::size_of::<windows::Win32::Graphics::Gdi::BITMAPINFOHEADER>() as u32,
            biWidth: bmp.width,
            biHeight: -bmp.height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: windows::Win32::Graphics::Gdi::BI_RGB.0,
            biSizeImage: (bmp.width * bmp.height * 4) as u32,
            ..Default::default()
        },
        bmiColors: [Default::default()],
    };
    let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
    let hbmp = windows::Win32::Graphics::Gdi::CreateDIBSection(
        screen_dc,
        &info,
        windows::Win32::Graphics::Gdi::DIB_RGB_COLORS,
        &mut bits,
        None,
        0,
    )
    .unwrap();
    std::ptr::copy_nonoverlapping(bmp.data.as_ptr(), bits as *mut u8, bmp.data.len());
    hbmp
}
