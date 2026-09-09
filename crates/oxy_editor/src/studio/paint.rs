//! Basic PNG painting shared by the pixel canvas and the native viewport.
use super::PaintTool;
use crate::app::Editor;
use egui::{Color32, Pos2, Rect, Sense, Vec2};
use oxy_core::{document::*, painting::PaintImage};

impl Editor {
    fn texture_id(&self) -> Option<Id> {
        self.selected
            .as_deref()
            .and_then(|id| self.scene().entity(id))
            .and_then(|e| {
                e.material
                    .texture
                    .clone()
                    .or_else(|| e.ui.as_ref().and_then(|u| u.texture.clone()))
            })
    }
    fn texture_uses(&self, id: &str) -> usize {
        self.state
            .project
            .scenes
            .iter()
            .flat_map(|s| &s.entities)
            .chain(
                self.state
                    .project
                    .assets
                    .iter()
                    .filter_map(|a| a.model.as_ref())
                    .flatten(),
            )
            .filter(|e| {
                e.material.texture.as_deref() == Some(id)
                    || e.ui.as_ref().and_then(|u| u.texture.as_deref()) == Some(id)
            })
            .count()
    }
    fn create_texture(&mut self, size: u32) {
        let Some(entity_id) = self.selected.clone() else {
            return;
        };
        let id = new_id();
        let name = format!(
            "Pintura {size} · {}",
            self.scene()
                .entity(&entity_id)
                .map(|e| e.name.as_str())
                .unwrap_or("peça")
        );
        match PaintImage::new(size, size, [235, 231, 218, 255]) {
            Ok(image) => {
                self.state.images.insert(id.clone(), image);
                self.state.project.assets.push(Asset {
                    id: id.clone(),
                    name,
                    path: format!("assets/{id}.png"),
                    kind: AssetKind::Texture,
                    model: None,
                });
                if let Some(e) = self.scene_mut().entity_mut(&entity_id) {
                    if !e.has_geometry()
                        && let Some(element) = &mut e.ui
                    {
                        element.texture = Some(id.clone());
                    } else {
                        e.material.texture = Some(id.clone());
                    }
                    e.material.color = [1.; 4];
                }
                self.refresh_texture(&id);
                self.studio.shared_edit = Some(id);
            }
            Err(e) => self.log(e),
        }
    }
    fn copy_texture(&mut self, id: &str) {
        if !self.ensure_texture(id) {
            return;
        }
        let Some(image) = self.state.images.get(id).cloned() else {
            return;
        };
        let Some(entity_id) = self.selected.clone() else {
            return;
        };
        let copy = new_id();
        let name = self
            .state
            .project
            .asset(id)
            .map(|a| format!("{} (cópia)", a.name))
            .unwrap_or("Textura independente".into());
        self.state.project.assets.push(Asset {
            id: copy.clone(),
            name,
            path: format!("assets/{copy}.png"),
            kind: AssetKind::Texture,
            model: None,
        });
        self.state.images.insert(copy.clone(), image);
        if let Some(e) = self.scene_mut().entity_mut(&entity_id) {
            if !e.has_geometry()
                && let Some(element) = &mut e.ui
            {
                element.texture = Some(copy.clone());
            } else {
                e.material.texture = Some(copy.clone());
            }
        }
        self.refresh_texture(&copy);
        self.studio.shared_edit = Some(copy);
    }
    pub fn paint_at_uv(&mut self, uv: [f32; 2]) {
        if self.studio.tool == PaintTool::Select {
            return;
        }
        let Some(id) = self.texture_id() else { return };
        if !self.ensure_texture(&id) {
            return;
        }
        if self.texture_uses(&id) > 1 && self.studio.shared_edit.as_ref() != Some(&id) {
            return;
        }
        if self.studio.tool == PaintTool::Sample {
            if let Some(image) = self.state.images.get(&id) {
                self.studio.color = image.sample_uv(uv);
            }
            return;
        }
        let Some(image) = self.state.images.get_mut(&id) else {
            return;
        };
        let [x, y] = image.uv_pixel(uv);
        let point = [x as f32 + 0.5, y as f32 + 0.5];
        let changed = match self.studio.tool {
            PaintTool::Brush => {
                let previous = self.studio.last_pixel.unwrap_or(point);
                self.studio.last_pixel = Some(point);
                if (previous[0] - point[0]).abs() > image.width as f32 * 0.3
                    || (previous[1] - point[1]).abs() > image.height as f32 * 0.3
                {
                    image.brush(point, self.studio.radius, self.studio.color)
                } else {
                    image.stroke(previous, point, self.studio.radius, self.studio.color)
                }
            }
            PaintTool::Fill => image.flood_fill(x, y, self.studio.color),
            PaintTool::Sample => {
                self.studio.color = image.pixel(x, y);
                false
            }
            PaintTool::Select => false,
        };
        if changed {
            self.refresh_texture(&id);
        }
    }
    pub(super) fn paint_ui(&mut self, ui: &mut egui::Ui) {
        if !ui.input(|i| i.pointer.primary_down()) {
            self.studio.last_pixel = None;
        }
        let Some(_entity) = self.selected.clone() else {
            self.viewport(ui, false);
            return;
        };
        if let Some(mut entity) = self.scene().entity(&_entity).cloned() {
            let interface = !entity.has_geometry() && entity.ui.is_some();
            self.texture_controls(ui, &mut entity, interface);
            if let Some(original) = self.scene_mut().entity_mut(&_entity) {
                *original = entity;
            }
        }
        if let Some(id) = self.texture_id() {
            self.ensure_texture(&id);
        }
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.studio.tool, PaintTool::Brush, "Pincel");
            ui.selectable_value(&mut self.studio.tool, PaintTool::Fill, "Preencher região");
            ui.selectable_value(&mut self.studio.tool, PaintTool::Sample, "Conta-gotas");
            ui.selectable_value(&mut self.studio.tool, PaintTool::Select,"Selecionar faces").on_hover_text("Escolha uma face na peça ou em sua ilha de textura, sem pintar. Ctrl alterna a seleção.");
            ui.color_edit_button_srgba_unmultiplied(&mut self.studio.color);
            ui.add(egui::Slider::new(&mut self.studio.radius, 0.5..=64.).text("Raio px"));
            if ui.button("Enquadrar peça").clicked() {
                self.frame_selection();
            }
        });
        ui.horizontal_wrapped(|ui| {
            for color in [
                [235, 231, 218, 255],
                [38, 44, 55, 255],
                [184, 64, 53, 255],
                [213, 165, 77, 255],
                [91, 168, 152, 255],
                [86, 127, 181, 255],
                [122, 84, 152, 255],
                [0, 0, 0, 0],
            ] {
                let c = Color32::from_rgba_unmultiplied(color[0], color[1], color[2], color[3]);
                if ui.add(egui::Button::new("   ").fill(c)).clicked() {
                    self.studio.color = color;
                }
            }
            if self.texture_id().is_none() {
                if ui.button("Criar 256×256").clicked() {
                    self.create_texture(256)
                }
                if ui.button("Criar 512×512").clicked() {
                    self.create_texture(512)
                }
            }
            if ui.button("Importar PNG").clicked() {
                self.import(AssetKind::Texture);
            }
            if ui
                .add_enabled(
                    self.texture_id().is_some(),
                    egui::Button::new("Exportar PNG"),
                )
                .clicked()
                && let Some(id) = self.texture_id()
                && let Some(image) = self.state.images.get(&id)
                && let Some(path) = rfd::FileDialog::new()
                    .add_filter("PNG", &["png"])
                    .set_file_name("pintura.png")
                    .save_file()
            {
                let result = image.save(&path);
                if let Err(e) = result {
                    self.log(e)
                } else {
                    self.log("PNG exportado com pixels reais.");
                }
            }
        });
        let Some(id) = self.texture_id() else {
            self.viewport(ui, true);
            return;
        };
        let uses = self.texture_uses(&id);
        if uses > 1 && self.studio.shared_edit.as_ref() != Some(&id) {
            ui.group(|ui|{ui.colored_label(Color32::from_rgb(232,190,113),format!("Esta textura está vinculada a {uses} peças ou imagens. A pintura afetará todas essas referências."));ui.horizontal(|ui|{if ui.button("Criar cópia independente").clicked(){self.copy_texture(&id)}
if ui.button("Editar textura compartilhada").clicked(){self.studio.shared_edit=Some(id.clone());}});});
        }
        let width = (ui.available_width() * 0.48).max(100.);
        let mesh = self.model_source().ok();
        let key = self
            .state
            .images
            .buffer_identity(&id)
            .map(|identity| (id.clone(), identity));
        egui::SidePanel::left("paint_pixels")
            .resizable(true)
            .default_width(width)
            .width_range(100.0..=1200.0)
            .show_inside(ui, |ui| {
                if let Some(image) = self.state.images.get(&id) {
                    if self.studio.texture_key != key || self.studio.texture.is_none() {
                        let color_image = egui::ColorImage::from_rgba_unmultiplied(
                            [image.width as usize, image.height as usize],
                            &image.pixels,
                        );
                        if let Some(handle) = &mut self.studio.texture {
                            handle.set(color_image, egui::TextureOptions::NEAREST);
                        } else {
                            self.studio.texture = Some(ui.ctx().load_texture(
                                "oxy_paint",
                                color_image,
                                egui::TextureOptions::NEAREST,
                            ));
                        }
                        self.studio.texture_key = key.clone();
                        self.studio.canvas_uploads += 1;
                    }
                    ui.label(format!("{} × {} · RGBA", image.width, image.height));
                    let ratio = image.width as f32 / image.height as f32;
                    let h = (ui.available_width() / ratio).min(ui.available_height().max(10.));
                    let size = Vec2::new(h * ratio, h);
                    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
                    for y in 0..((rect.height() / 16.).ceil() as usize) {
                        for x in 0..((rect.width() / 16.).ceil() as usize) {
                            let r = Rect::from_min_size(
                                rect.min + Vec2::new(x as f32 * 16., y as f32 * 16.),
                                Vec2::splat(16.),
                            )
                            .intersect(rect);
                            ui.painter().rect_filled(
                                r,
                                0.,
                                if (x + y) % 2 == 0 {
                                    Color32::from_gray(62)
                                } else {
                                    Color32::from_gray(90)
                                },
                            );
                        }
                    }
                    ui.painter().image(
                        self.studio.texture.as_ref().unwrap().id(),
                        rect,
                        Rect::from_min_max(Pos2::ZERO, Pos2::new(1., 1.)),
                        Color32::WHITE,
                    );
                    if let Some(mesh) = &mesh {
                        self.draw_uv_islands(ui, rect, mesh);
                    }
                    if let Some(uv) = self.studio.hover_uv {
                        let p = rect.min + Vec2::new(uv[0] * rect.width(), uv[1] * rect.height());
                        ui.painter()
                            .circle_stroke(p, 5., egui::Stroke::new(1.5, Color32::WHITE));
                    }
                    let press = ui.input(|i| {
                        i.events.iter().find_map(|event| match event {
                            egui::Event::PointerButton {
                                pos,
                                button: egui::PointerButton::Primary,
                                pressed: true,
                                ..
                            } if rect.contains(*pos) => Some(*pos),
                            _ => None,
                        })
                    });
                    if let Some(p) = press {
                        self.studio.last_pixel = Some([
                            (p.x - rect.left()) / rect.width() * image.width as f32,
                            (p.y - rect.top()) / rect.height() * image.height as f32,
                        ]);
                    }
                    if (response.dragged_by(egui::PointerButton::Primary)
                        || response.clicked()
                        || press.is_some())
                        && let Some(p) = response.interact_pointer_pos()
                    {
                        let uv = [
                            (p.x - rect.min.x) / rect.width(),
                            (p.y - rect.min.y) / rect.height(),
                        ];
                        if self.paint_selecting() {
                            if response.clicked() {
                                self.select_paint_face(None, uv, ui.input(|i| i.modifiers.ctrl));
                            }
                        } else {
                            self.paint_at_uv(uv);
                        }
                    }
                } else {
                    ui.colored_label(
                        Color32::LIGHT_RED,
                        "Não foi possível carregar os pixels desta textura.",
                    );
                }
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show_inside(ui, |ui| {
                self.viewport(ui, true);
            });
    }
}
