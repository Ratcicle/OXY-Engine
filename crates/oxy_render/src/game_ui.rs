//! Native overlay widgets shared unchanged between the editor's game tab and player.
use egui::{
    Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, TextureHandle, TextureOptions,
    Vec2,
};
use oxy_core::{
    document::{Id, Project, Scene, UiElement, UiKind},
    game_ui,
};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

#[derive(Default)]
pub struct GameUi {
    textures: HashMap<Id, TextureHandle>,
    overrides: HashMap<Id, (u32, u32, Vec<u8>, bool)>,
    failed: Vec<Id>,
    errors: Vec<String>,
    root: PathBuf,
}

impl GameUi {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn invalidate_texture(&mut self, id: &str) {
        self.textures.remove(id);
        self.failed.retain(|failed| failed != id);
    }

    pub fn clear_texture_override(&mut self, id: &str) {
        self.overrides.remove(id);
        self.invalidate_texture(id);
    }

    /// After a successful project save, release CPU painting copies while keeping
    /// egui texture handles alive. Failed saves must retain these overrides.
    pub fn release_saved_texture_pixels(&mut self) -> usize {
        let released = self.texture_override_bytes();
        self.overrides.clear();
        released
    }

    pub fn texture_override_bytes(&self) -> usize {
        self.overrides
            .values()
            .map(|(_, _, pixels, _)| pixels.capacity())
            .sum()
    }

    pub fn set_texture_pixels(
        &mut self,
        ctx: &egui::Context,
        id: &str,
        width: u32,
        height: u32,
        rgba: &[u8],
        nearest: bool,
    ) -> Result<(), String> {
        self.load_texture_pixels(ctx, id, width, height, rgba, nearest)?;
        self.overrides
            .insert(id.to_owned(), (width, height, rgba.to_vec(), nearest));
        Ok(())
    }

    fn load_texture_pixels(
        &mut self,
        ctx: &egui::Context,
        id: &str,
        width: u32,
        height: u32,
        rgba: &[u8],
        nearest: bool,
    ) -> Result<(), String> {
        if width == 0 || height == 0 || width as usize * height as usize * 4 != rgba.len() {
            return Err("Pixels da interface inválidos".into());
        }
        let options = if nearest {
            TextureOptions::NEAREST
        } else {
            TextureOptions::LINEAR
        };
        let image =
            egui::ColorImage::from_rgba_unmultiplied([width as usize, height as usize], rgba);
        self.textures.insert(
            id.to_owned(),
            ctx.load_texture(format!("OXY UI {id}"), image, options),
        );
        Ok(())
    }

    pub fn take_errors(&mut self) -> Vec<String> {
        std::mem::take(&mut self.errors)
    }

    /// Returns stable IDs of clicked buttons; the host forwards them to Runtime::click.
    pub fn draw(
        &mut self,
        ui: &mut egui::Ui,
        project: &Project,
        scene: &Scene,
        root: &Path,
        viewport: Rect,
    ) -> Vec<Id> {
        if self.root != root {
            self.textures.clear();
            self.failed.clear();
            self.root = root.to_owned();
        }
        let mut clicks = Vec::new();
        if !scene.entities.iter().any(|e| e.ui.is_some()) {
            return clicks;
        }
        let view = oxy_core::scene_view::SceneView::new(scene);
        let mut elements: Vec<_> = scene
            .entities
            .iter()
            .filter(|entity| entity.ui.is_some() && view.visible(entity))
            .collect();
        elements.sort_by_key(|entity| entity.layer);
        let painter = ui.painter().with_clip_rect(viewport);
        for entity in elements {
            let element = entity.ui.as_ref().unwrap();
            let rect = element_rect(element, viewport);
            let color = rgba(element.color);
            let value = game_ui::bound_value(&view, entity, element);
            let text = game_ui::shown_text(element, value, scene, &project.surfaces);
            let texture = element.texture.as_deref().and_then(|id| {
                if !self.textures.contains_key(id) && !self.failed.iter().any(|failed| failed == id)
                {
                    if let Some((width, height, pixels, nearest)) = self.overrides.get(id).cloned()
                    {
                        let _ =
                            self.load_texture_pixels(ui.ctx(), id, width, height, &pixels, nearest);
                        return self.textures.get(id);
                    }
                    let loaded = project
                        .asset(id)
                        .ok_or_else(|| format!("Imagem de interface ausente: {id}"))
                        .and_then(|asset| {
                            image::open(root.join(&asset.path))
                                .map_err(|error| format!("Imagem {}: {error}", asset.name))
                        });
                    match loaded {
                        Ok(image) => {
                            let pixels = image.to_rgba8();
                            let _ = self.load_texture_pixels(
                                ui.ctx(),
                                id,
                                pixels.width(),
                                pixels.height(),
                                pixels.as_raw(),
                                entity.material.nearest,
                            );
                        }
                        Err(error) => {
                            self.failed.push(id.to_owned());
                            self.errors.push(error);
                        }
                    }
                }
                self.textures.get(id)
            });
            match element.kind {
                UiKind::Text => {
                    painter.text(
                        rect.left_center(),
                        Align2::LEFT_CENTER,
                        text,
                        FontId::proportional((rect.height() * 0.5).clamp(12., 32.)),
                        color,
                    );
                }
                UiKind::Image => {
                    if let Some(texture) = texture {
                        painter.image(
                            texture.id(),
                            fit_image(rect, texture.size_vec2()),
                            Rect::from_min_max(Pos2::ZERO, Pos2::new(1., 1.)),
                            color,
                        );
                    } else {
                        painter.rect_filled(rect, 3., Color32::from_rgb(65, 45, 65));
                        painter.text(
                            rect.center(),
                            Align2::CENTER_CENTER,
                            "Imagem",
                            FontId::proportional(14.),
                            color,
                        );
                    }
                }
                UiKind::Button => {
                    let response = ui.interact(
                        rect.intersect(viewport),
                        egui::Id::new(("oxy_game_button", &entity.id)),
                        Sense::click(),
                    );
                    let background = if response.is_pointer_button_down_on() {
                        Color32::from_rgb(45, 67, 75)
                    } else if response.hovered() {
                        Color32::from_rgb(56, 79, 90)
                    } else {
                        Color32::from_rgb(30, 44, 54)
                    };
                    painter.rect_filled(rect, 5., background);
                    painter.rect_stroke(
                        rect,
                        5.,
                        Stroke::new(1., color.gamma_multiply(0.7)),
                        StrokeKind::Inside,
                    );
                    if let Some(texture) = texture {
                        let image_rect = Rect::from_min_max(
                            rect.min + Vec2::splat(6.),
                            Pos2::new(rect.max.x - 6., rect.min.y + rect.height() * 0.65),
                        );
                        painter.image(
                            texture.id(),
                            fit_image(image_rect, texture.size_vec2()),
                            Rect::from_min_max(Pos2::ZERO, Pos2::new(1., 1.)),
                            Color32::WHITE,
                        );
                        painter.text(
                            Pos2::new(rect.center().x, rect.max.y - 8.),
                            Align2::CENTER_BOTTOM,
                            text,
                            FontId::proportional(15.),
                            color,
                        );
                    } else {
                        painter.text(
                            rect.center(),
                            Align2::CENTER_CENTER,
                            text,
                            FontId::proportional(16.),
                            color,
                        );
                    }
                    if response.clicked() {
                        clicks.push(entity.id.clone());
                    }
                }
                UiKind::Bar => {
                    let fraction = game_ui::bar_fraction(element, value) as f32;
                    painter.rect_filled(rect, 4., Color32::from_rgb(24, 28, 34));
                    painter.rect_filled(
                        Rect::from_min_size(
                            rect.min,
                            Vec2::new(rect.width() * fraction, rect.height()),
                        ),
                        4.,
                        color,
                    );
                    painter.rect_stroke(
                        rect,
                        4.,
                        Stroke::new(1., Color32::from_gray(110)),
                        StrokeKind::Inside,
                    );
                    painter.text(
                        rect.center(),
                        Align2::CENTER_CENTER,
                        text,
                        FontId::proportional((rect.height() * 0.55).clamp(11., 22.)),
                        Color32::WHITE,
                    );
                }
            }
        }
        clicks
    }
}

/// `oxy_core::game_ui::element_rect` on an egui rectangle.
pub fn element_rect(element: &UiElement, viewport: Rect) -> Rect {
    let rect = game_ui::element_rect(
        element,
        game_ui::ScreenRect {
            min: viewport.min.into(),
            max: viewport.max.into(),
        },
    );
    Rect::from_min_max(rect.min.into(), rect.max.into())
}

/// Editor selection for the overlay, using the very same anchors and layer ordering as drawing.
pub fn pick_ui(scene: &Scene, viewport: Rect, pointer: Pos2) -> Option<Id> {
    if !viewport.contains(pointer) {
        return None;
    }
    let view = oxy_core::scene_view::SceneView::new(scene);
    let mut elements: Vec<_> = scene
        .entities
        .iter()
        .filter(|entity| entity.ui.is_some() && view.visible(entity))
        .collect();
    elements.sort_by_key(|entity| entity.layer);
    elements
        .into_iter()
        .rev()
        .find(|entity| element_rect(entity.ui.as_ref().unwrap(), viewport).contains(pointer))
        .map(|entity| entity.id.clone())
}

fn fit_image(rect: Rect, size: Vec2) -> Rect {
    let scale = (rect.width() / size.x.max(1.)).min(rect.height() / size.y.max(1.));
    Rect::from_center_size(rect.center(), size * scale)
}

fn rgba(color: [f32; 4]) -> Color32 {
    Color32::from_rgba_unmultiplied(
        (color[0].clamp(0., 1.) * 255.) as u8,
        (color[1].clamp(0., 1.) * 255.) as u8,
        (color[2].clamp(0., 1.) * 255.) as u8,
        (color[3].clamp(0., 1.) * 255.) as u8,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portuguese_attribute_binding_and_unsaved_pixels_survive_first_game_draw() {
        use oxy_core::{
            document::{Entity, Value},
            game_ui::display_text,
        };
        let element = UiElement {
            text: "Vida {valor}".into(),
            ..Default::default()
        };
        assert_eq!(
            display_text(
                &element,
                Some(&Value::Number(42.)),
                &Project::new("Vínculo").scenes[0],
                &[]
            ),
            "Vida 42"
        );
        let ctx = egui::Context::default();
        let mut overlay = GameUi::new();
        overlay
            .set_texture_pixels(&ctx, "temporary-paint", 1, 1, &[20, 40, 60, 255], true)
            .unwrap();
        let mut project = Project::new("Preview");
        let mut entity = Entity::new("Imagem", None);
        entity.ui = Some(UiElement {
            kind: UiKind::Image,
            texture: Some("temporary-paint".into()),
            ..Default::default()
        });
        project.scenes[0].entities.push(entity);
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                overlay.draw(
                    ui,
                    &project,
                    &project.scenes[0],
                    Path::new("new-project-root"),
                    ui.max_rect(),
                );
            });
        });
        assert!(overlay.textures.contains_key("temporary-paint"));
        assert!(overlay.take_errors().is_empty());
        let texture_id = overlay.textures["temporary-paint"].id();
        assert_eq!(overlay.texture_override_bytes(), 4);
        assert_eq!(overlay.release_saved_texture_pixels(), 4);
        assert_eq!(overlay.texture_override_bytes(), 0);
        let output = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                overlay.draw(
                    ui,
                    &project,
                    &project.scenes[0],
                    Path::new("new-project-root"),
                    ui.max_rect(),
                );
            });
        });
        assert_eq!(overlay.textures["temporary-paint"].id(), texture_id);
        assert!(
            !output
                .textures_delta
                .set
                .iter()
                .any(|(id, _)| *id == texture_id)
        );
        assert!(overlay.take_errors().is_empty());
    }

    #[test]
    fn right_anchor_keeps_margin_on_resize() {
        use oxy_core::document::UiAnchor;
        let element = UiElement {
            anchor: UiAnchor::TopRight,
            position: [12., 8.],
            size: [100., 40.],
            ..Default::default()
        };
        for width in [300., 1000.] {
            let viewport = Rect::from_min_size(Pos2::new(10., 20.), Vec2::new(width, 400.));
            let rect = element_rect(&element, viewport);
            assert_eq!(viewport.right() - rect.right(), 12.);
            assert_eq!(rect.top() - viewport.top(), 8.);
        }
    }

    #[test]
    fn overlay_picking_matches_layers_and_hidden_hierarchy() {
        use oxy_core::document::{Entity, SceneKind};
        let mut scene = Scene::new("UI", SceneKind::TwoD);
        let mut bottom = Entity::new("Base", None);
        bottom.ui = Some(UiElement::default());
        let bottom_id = bottom.id.clone();
        let mut top = Entity::new("Topo", None);
        top.ui = Some(UiElement::default());
        top.layer = 4;
        let top_id = top.id.clone();
        scene.entities.extend([bottom, top]);
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(640., 480.));
        let pointer = Pos2::new(30., 30.);
        assert_eq!(pick_ui(&scene, rect, pointer), Some(top_id.clone()));
        scene.entity_mut(&top_id).unwrap().visible = false;
        assert_eq!(pick_ui(&scene, rect, pointer), Some(bottom_id.clone()));
        scene.entity_mut(&bottom_id).unwrap().parent = Some(top_id);
        assert_eq!(pick_ui(&scene, rect, pointer), None);
    }
}
