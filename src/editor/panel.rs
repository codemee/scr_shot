use egui::{self, Color32, Frame, Pos2, Rect, Sense, Stroke, TextureHandle, TextureOptions, Vec2};
use image::RgbaImage;

use crate::editor::tools::{Annotation, Annotations, Tool};
use crate::app_state::{CaptureMode, CapturedImage};

pub struct EditorPanel {
    pub image: RgbaImage,
    pub mode: CaptureMode,
    pub annotations: Annotations,
    current_tool: Tool,
    stroke_color: Color32,
    stroke_width: f32,
    font_size: f32,
    text_input: String,
    adding_text: bool,
    drag_start: Option<Pos2>,
    drag_current: Option<Pos2>,
    pending_arrow_from: Option<Pos2>,
    texture: Option<TextureHandle>,
}

impl EditorPanel {
    pub fn new(capture: &CapturedImage, mode: CaptureMode) -> Self {
        let img = capture.to_rgba_image();
        Self {
            image: img,
            mode,
            annotations: Annotations::new(),
            current_tool: Tool::Rect,
            stroke_color: Color32::RED,
            stroke_width: 3.0,
            font_size: 20.0,
            text_input: String::new(),
            adding_text: false,
            drag_start: None,
            drag_current: None,
            pending_arrow_from: None,
            texture: None,
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, save_cb: &mut dyn FnMut(&RgbaImage), cancel_cb: &mut dyn FnMut()) {
        if self.texture.is_none() || self.texture.as_ref().map(|t| t.size()[0] as u32 != self.image.width() || t.size()[1] as u32 != self.image.height()).unwrap_or(true) {
            let color_img = egui::ColorImage::from_rgba_unmultiplied(
                [self.image.width() as _, self.image.height() as _],
                self.image.as_raw(),
            );
            self.texture = Some(ctx.load_texture("screenshot", color_img, TextureOptions::LINEAR));
        }

        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                let tools = &[
                    (Tool::Select, "⫿"),
                    (Tool::Rect, "▭"),
                    (Tool::Ellipse, "○"),
                    (Tool::Arrow, "→"),
                    (Tool::Line, "∕"),
                    (Tool::Text, "T"),
                    (Tool::Mosaic, "▣"),
                ];
                for (tool, label) in tools {
                    let selected = self.current_tool == *tool;
                    if ui.selectable_label(selected, *label).clicked() {
                        self.current_tool = *tool;
                        self.drag_start = None;
                        self.pending_arrow_from = None;
                        self.adding_text = false;
                    }
                }
                ui.separator();
                if ui.button("↩").clicked() { self.annotations.undo(); }
                if ui.button("↪").clicked() { self.annotations.redo(); }
                ui.separator();

                egui::widgets::color_picker::color_edit_button_srgba(ui, &mut self.stroke_color, egui::widgets::color_picker::Alpha::Opaque);
                ui.add(egui::DragValue::new(&mut self.stroke_width).speed(0.5).range(1.0..=20.0).prefix("w:"));
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            let available = ui.available_size();
            let img_size = Vec2::new(self.image.width() as f32, self.image.height() as f32);
            let scale = (available.x / img_size.x).min(available.y / img_size.y).min(1.0);
            let display_size = img_size * scale;

            let (response, painter) = ui.allocate_painter(display_size, Sense::click_and_drag());
            let rect = response.rect;

            if let Some(tex) = &self.texture {
                ui.put(rect, egui::Image::new(tex, display_size));
            }

            let to_img = |p: Pos2| -> Pos2 {
                Pos2::new(
                    (p.x - rect.left()) / scale,
                    (p.y - rect.top()) / scale,
                )
            };

            let mut new_annotation: Option<Annotation> = None;

            if self.current_tool == Tool::Text && self.adding_text {
                let galley = painter.layout_no_wrap(
                    self.text_input.clone(),
                    egui::FontId::proportional(self.font_size),
                    self.stroke_color,
                );
                if let Some(start) = self.drag_start {
                    painter.galley(start, galley, self.stroke_color);
                }
            }

            if response.dragged() {
                if let Some(pos) = response.interact_pointer_pos() {
                    let img_pos = to_img(pos);
                    match self.current_tool {
                        Tool::Select => {}
                        Tool::Rect | Tool::Ellipse | Tool::Mosaic => {
                            self.drag_current = Some(img_pos);
                        }
                        Tool::Line => {
                            let mut ann = Annotation::Line {
                                points: vec![],
                                stroke: Stroke::new(self.stroke_width, self.stroke_color),
                            };
                            if let Some(start) = self.drag_start {
                                if let Annotation::Line { points, .. } = &mut ann {
                                    points.push(start);
                                    points.push(img_pos);
                                }
                            }
                            new_annotation = Some(ann);
                            self.drag_start = Some(img_pos);
                        }
                        Tool::Arrow => {
                            if self.drag_start.is_none() {
                                self.drag_start = Some(img_pos);
                            }
                            self.drag_current = Some(img_pos);
                        }
                        _ => {}
                    }
                }
            }

            if response.drag_started() {
                if let Some(pos) = response.interact_pointer_pos() {
                    let img_pos = to_img(pos);
                    match self.current_tool {
                        Tool::Rect | Tool::Ellipse | Tool::Mosaic | Tool::Line => {
                            self.drag_start = Some(img_pos);
                            self.drag_current = Some(img_pos);
                        }
                        Tool::Arrow => {
                            self.drag_start = Some(img_pos);
                            self.drag_current = Some(img_pos);
                        }
                        Tool::Text => {
                            self.drag_start = Some(img_pos);
                            self.adding_text = false;
                        }
                        _ => {}
                    }
                }
            }

            if response.drag_released() {
                match self.current_tool {
                    Tool::Rect => {
                        if let (Some(start), Some(current)) = (self.drag_start, self.drag_current) {
                            let r = Rect::from_two_pos(start, current);
                            new_annotation = Some(Annotation::Rect {
                                rect: r,
                                stroke: Stroke::new(self.stroke_width, self.stroke_color),
                                fill: None,
                            });
                        }
                    }
                    Tool::Ellipse => {
                        if let (Some(start), Some(current)) = (self.drag_start, self.drag_current) {
                            let r = Rect::from_two_pos(start, current);
                            new_annotation = Some(Annotation::Ellipse {
                                rect: r,
                                stroke: Stroke::new(self.stroke_width, self.stroke_color),
                                fill: None,
                            });
                        }
                    }
                    Tool::Mosaic => {
                        if let (Some(start), Some(current)) = (self.drag_start, self.drag_current) {
                            let r = Rect::from_two_pos(start, current);
                            if r.size().x > 5.0 && r.size().y > 5.0 {
                                new_annotation = Some(Annotation::Mosaic { rect: r });
                            }
                        }
                    }
                    Tool::Text => {
                        self.adding_text = true;
                    }
                    Tool::Arrow => {
                        if let (Some(from), Some(current)) = (self.drag_start, self.drag_current) {
                            new_annotation = Some(Annotation::Arrow {
                                from,
                                to: current,
                                stroke: Stroke::new(self.stroke_width, self.stroke_color),
                            });
                        }
                    }
                    _ => {}
                }
                self.drag_start = None;
                self.drag_current = None;
            }

            if let Some(ann) = new_annotation {
                self.annotations.push(ann);
            }

            let annotations = self.annotations.clone();
            for ann in &annotations.items {
                ann.draw(&painter);
            }

            if let (Some(start), Some(current)) = (self.drag_start, self.drag_current) {
                let r = Rect::from_two_pos(start, current);
                let preview = match self.current_tool {
                    Tool::Rect => Some(Annotation::Rect {
                        rect: r,
                        stroke: Stroke::new(self.stroke_width, self.stroke_color),
                        fill: None,
                    }),
                    Tool::Ellipse => Some(Annotation::Ellipse {
                        rect: r,
                        stroke: Stroke::new(self.stroke_width, self.stroke_color),
                        fill: None,
                    }),
                    Tool::Arrow => Some(Annotation::Arrow {
                        from: start,
                        to: current,
                        stroke: Stroke::new(self.stroke_width, self.stroke_color),
                    }),
                    Tool::Mosaic => Some(Annotation::Mosaic { rect: r }),
                    _ => None,
                };
                if let Some(p) = preview {
                    p.draw(&painter);
                }
            }
        });

        egui::TopBottomPanel::bottom("actions").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("📁 存檔").clicked() {
                    self.finalize_and_save(save_cb);
                }
                if ui.button("📋 複製到剪貼簿").clicked() {
                    self.finalize_and_save(save_cb);
                }
                if ui.button("✕ 取消").clicked() {
                    cancel_cb();
                }
            });
        });
    }

    pub fn finalize_and_save(&mut self, save_cb: &mut dyn FnMut(&RgbaImage)) {
        let mut final_img = self.image.clone();
        self.annotations.flatten_onto(&mut final_img);
        save_cb(&final_img);
    }
}
