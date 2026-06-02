use windows::Win32::Foundation::POINT;
use windows::Win32::Graphics::Gdi::{
    CreatePen, CreateSolidBrush, DeleteObject, LineTo, MoveToEx,
    Rectangle, SelectObject, TextOutW, HDC, PS_SOLID,
};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Tool {
    Pen,
    Arrow,
    Rect,
    Text,
    Crop,
}

#[derive(Clone, Debug)]
pub enum Stroke {
    Pen { points: Vec<POINT> },
    Arrow { from: POINT, to: POINT },
    Rect { r: windows::Win32::Foundation::RECT },
    Text { pos: POINT, text: String },
}

#[derive(Clone, Copy)]
pub struct Color(pub u32); // COLORREF (0x00BBGGRR)

impl Stroke {
    /// 平移所有座標（裁切後調整用）
    pub fn translate(&mut self, dx: i32, dy: i32) {
        match self {
            Stroke::Pen { points } => {
                for p in points.iter_mut() { p.x += dx; p.y += dy; }
            }
            Stroke::Arrow { from, to } => {
                from.x += dx; from.y += dy; to.x += dx; to.y += dy;
            }
            Stroke::Rect { r } => {
                r.left += dx; r.top += dy; r.right += dx; r.bottom += dy;
            }
            Stroke::Text { pos, .. } => { pos.x += dx; pos.y += dy; }
        }
    }
}

impl Stroke {
    pub fn draw(&self, hdc: HDC, color: Color, thickness: i32) {
        unsafe {
            let pen = CreatePen(PS_SOLID, thickness, windows::Win32::Foundation::COLORREF(color.0));
            let old_pen = SelectObject(hdc, pen);
            let brush = CreateSolidBrush(windows::Win32::Foundation::COLORREF(color.0));
            let old_brush = SelectObject(hdc, brush);

            match self {
                Stroke::Pen { points } => {
                    if let Some(first) = points.first() {
                        MoveToEx(hdc, first.x, first.y, None);
                        for p in points.iter().skip(1) {
                            LineTo(hdc, p.x, p.y);
                        }
                    }
                }
                Stroke::Arrow { from, to } => {
                    draw_arrow(hdc, *from, *to, color.0, thickness);
                }
                Stroke::Rect { r } => {
                    let null_brush = windows::Win32::Graphics::Gdi::GetStockObject(
                        windows::Win32::Graphics::Gdi::NULL_BRUSH,
                    );
                    SelectObject(hdc, null_brush);
                    Rectangle(hdc, r.left, r.top, r.right, r.bottom);
                }
                Stroke::Text { pos, text } => {
                    windows::Win32::Graphics::Gdi::SetTextColor(
                        hdc,
                        windows::Win32::Foundation::COLORREF(color.0),
                    );
                    windows::Win32::Graphics::Gdi::SetBkMode(
                        hdc,
                        windows::Win32::Graphics::Gdi::TRANSPARENT,
                    );
                    let wide: Vec<u16> = text.encode_utf16().collect();
                    TextOutW(hdc, pos.x, pos.y, &wide);
                }
            }

            SelectObject(hdc, old_pen);
            SelectObject(hdc, old_brush);
            DeleteObject(pen);
            DeleteObject(brush);
        }
    }
}

fn draw_arrow(hdc: HDC, from: POINT, to: POINT, color: u32, thickness: i32) {
    unsafe {
        let pen = CreatePen(PS_SOLID, thickness, windows::Win32::Foundation::COLORREF(color));
        let old_pen = SelectObject(hdc, pen);
        MoveToEx(hdc, from.x, from.y, None);
        LineTo(hdc, to.x, to.y);

        // Arrowhead
        let dx = (to.x - from.x) as f64;
        let dy = (to.y - from.y) as f64;
        let len = (dx * dx + dy * dy).sqrt().max(1.0);
        let ux = dx / len;
        let uy = dy / len;
        let arrow_len = (14.0 + thickness as f64 * 2.0).min(len * 0.4);
        let arrow_width = arrow_len * 0.5;

        let tip_x = to.x as f64;
        let tip_y = to.y as f64;
        let base_x = tip_x - ux * arrow_len;
        let base_y = tip_y - uy * arrow_len;

        let lx = (base_x + uy * arrow_width) as i32;
        let ly = (base_y - ux * arrow_width) as i32;
        let rx = (base_x - uy * arrow_width) as i32;
        let ry = (base_y + ux * arrow_width) as i32;

        let brush = CreateSolidBrush(windows::Win32::Foundation::COLORREF(color));
        let old_brush = SelectObject(hdc, brush);

        let pts = [
            POINT { x: to.x, y: to.y },
            POINT { x: lx, y: ly },
            POINT { x: rx, y: ry },
        ];
        windows::Win32::Graphics::Gdi::Polygon(hdc, &pts);

        SelectObject(hdc, old_pen);
        SelectObject(hdc, old_brush);
        DeleteObject(pen);
        DeleteObject(brush);
    }
}
