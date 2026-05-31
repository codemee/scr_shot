use egui::{Color32, Pos2, Rect, Stroke, Vec2};

#[derive(Debug, Clone)]
pub enum Annotation {
    Rect {
        rect: Rect,
        stroke: Stroke,
        fill: Option<Color32>,
    },
    Ellipse {
        rect: Rect,
        stroke: Stroke,
        fill: Option<Color32>,
    },
    Arrow {
        from: Pos2,
        to: Pos2,
        stroke: Stroke,
    },
    Line {
        points: Vec<Pos2>,
        stroke: Stroke,
    },
    Text {
        pos: Pos2,
        text: String,
        color: Color32,
        font_size: f32,
    },
    Mosaic {
        rect: Rect,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Select,
    Rect,
    Ellipse,
    Arrow,
    Line,
    Text,
    Mosaic,
}

impl Default for Tool {
    fn default() -> Self { Self::Select }
}

impl Annotation {
    pub fn draw(&self, painter: &egui::Painter) {
        match self {
            Annotation::Rect { rect, stroke, fill } => {
                if let Some(fc) = fill {
                    painter.rect_filled(*rect, 0.0, fc);
                }
                painter.rect_stroke(*rect, 0.0, *stroke);
            }
            Annotation::Ellipse { rect, stroke, fill } => {
                if let Some(fc) = fill {
                    painter.circle_filled(rect.center(), rect.size().x.min(rect.size().y) / 2.0, fc);
                }
                painter.circle_stroke(rect.center(), rect.size().x.min(rect.size().y) / 2.0, *stroke);
            }
            Annotation::Arrow { from, to, stroke } => {
                painter.line_segment([*from, *to], *stroke);
                let dir = *to - *from;
                let len = dir.length();
                if len > 0.0 {
                    let dir = dir / len;
                    let perp = Vec2::new(-dir.y, dir.x);
                    let tip_size = stroke.width * 2.5;
                    let p1 = *to - dir * tip_size + perp * tip_size * 0.4;
                    let p2 = *to - dir * tip_size - perp * tip_size * 0.4;
                    painter.line_segment([p1, *to], *stroke);
                    painter.line_segment([p2, *to], *stroke);
                    painter.line_segment([p1, p2], *stroke);
                }
            }
            Annotation::Line { points, stroke } => {
                if points.len() >= 2 {
                    for i in 1..points.len() {
                        painter.line_segment([points[i - 1], points[i]], *stroke);
                    }
                }
            }
            Annotation::Text { pos, text, color, font_size } => {
                let galley = painter.layout_no_wrap(text.clone(), egui::FontId::proportional(*font_size), *color);
                painter.galley(*pos, galley, *color);
            }
            Annotation::Mosaic { rect } => {
                painter.rect_filled(*rect, 0.0, Color32::from_gray(128));
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct Annotations {
    pub items: Vec<Annotation>,
    undo_stack: Vec<Vec<Annotation>>,
    redo_stack: Vec<Vec<Annotation>>,
}

impl Annotations {
    pub fn new() -> Self {
        Self { items: vec![], undo_stack: vec![], redo_stack: vec![] }
    }

    pub fn push(&mut self, ann: Annotation) {
        self.undo_stack.push(self.items.clone());
        self.redo_stack.clear();
        self.items.push(ann);
    }

    pub fn undo(&mut self) {
        if let Some(prev) = self.undo_stack.pop() {
            self.redo_stack.push(self.items.clone());
            self.items = prev;
        }
    }

    pub fn redo(&mut self) {
        if let Some(next) = self.redo_stack.pop() {
            self.undo_stack.push(self.items.clone());
            self.items = next;
        }
    }

    pub fn can_undo(&self) -> bool { !self.undo_stack.is_empty() }
    pub fn can_redo(&self) -> bool { !self.redo_stack.is_empty() }

    pub fn flatten_onto(&self, img: &mut image::RgbaImage) {
        for ann in &self.items {
            self.flatten_one(img, ann);
        }
    }

    fn flatten_one(&self, img: &mut image::RgbaImage, ann: &Annotation) {
        let (w, h) = img.dimensions();
        match ann {
            Annotation::Rect { rect, stroke, fill } => {
                if let Some(fc) = fill {
                    let r = to_image_rect(*rect, w, h);
                    for y in r.1..r.3 {
                        for x in r.0..r.2 {
                            if x < w && y < h {
                                let p = img.get_pixel_mut(x, y);
                                blend_pixel(p, fc);
                            }
                        }
                    }
                }
                draw_stroke_rect(img, *rect, *stroke, w, h);
            }
            Annotation::Mosaic { rect } => {
                let r = to_image_rect(*rect, w, h);
                let block = 8.max(1);
                for by in (r.1..r.3).step_by(block as usize) {
                    for bx in (r.0..r.2).step_by(block as usize) {
                        if bx < w && by < h {
                            let c = img.get_pixel(bx.min(w - 1), by.min(h - 1));
                            let avg = [
                                c[0] as u32, c[1] as u32, c[2] as u32, c[3] as u32,
                            ];
                            let avg_pixel = image::Rgba([
                                (avg[0] / 1) as u8,
                                (avg[1] / 1) as u8,
                                (avg[2] / 1) as u8,
                                (avg[3] / 1) as u8,
                            ]);
                            for dy in 0..block {
                                for dx in 0..block {
                                    let px = bx + dx;
                                    let py = by + dy;
                                    if px < w && py < h {
                                        img.put_pixel(px, py, avg_pixel);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

fn to_image_rect(rect: Rect, img_w: u32, img_h: u32) -> (u32, u32, u32, u32) {
    let x1 = rect.min.x.max(0.0) as u32;
    let y1 = rect.min.y.max(0.0) as u32;
    let x2 = (rect.max.x as u32).min(img_w);
    let y2 = (rect.max.y as u32).min(img_h);
    (x1, y1, x2, y2)
}

fn blend_pixel(p: &mut image::Rgba<u8>, color: Color32) {
    let a = color.a();
    if a == 255 {
        p[0] = color.r();
        p[1] = color.g();
        p[2] = color.b();
    } else if a > 0 {
        let t = a as f32 / 255.0;
        p[0] = (p[0] as f32 * (1.0 - t) + color.r() as f32 * t) as u8;
        p[1] = (p[1] as f32 * (1.0 - t) + color.g() as f32 * t) as u8;
        p[2] = (p[2] as f32 * (1.0 - t) + color.b() as f32 * t) as u8;
    }
}

fn draw_stroke_rect(img: &mut image::RgbaImage, rect: Rect, stroke: Stroke, img_w: u32, img_h: u32) {
    let x1 = rect.min.x.max(0.0) as u32;
    let y1 = rect.min.y.max(0.0) as u32;
    let x2 = (rect.max.x as u32).min(img_w);
    let y2 = (rect.max.y as u32).min(img_h);
    let width = stroke.width.max(1.0) as u32;

    for y in y1..y2 {
        for x in x1..(x1 + width).min(img_w) {
            blend_pixel(img.get_pixel_mut(x, y), stroke.color);
        }
        for x in x2.saturating_sub(width)..x2 {
            blend_pixel(img.get_pixel_mut(x, y), stroke.color);
        }
    }
    for x in x1..x2 {
        for y in y1..(y1 + width).min(img_h) {
            blend_pixel(img.get_pixel_mut(x, y), stroke.color);
        }
        for y in y2.saturating_sub(width)..y2 {
            blend_pixel(img.get_pixel_mut(x, y), stroke.color);
        }
    }
}
