//! Native overlay widgets shared unchanged between the editor's game tab and player.
use egui::{
    Align, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, TextureHandle,
    TextureOptions, Vec2, text::LayoutJob,
};
use oxy_core::{
    document::{Id, Project, Scene, UiElement, UiKind, UiTextAlign},
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
        self.paint(ui, project, scene, root, viewport, false)
    }

    /// `draw` for the editor's scene view: an image element without an image also gets a dashed
    /// outline, so a missing image shows even when its colour is see-through.
    pub fn draw_authoring(
        &mut self,
        ui: &mut egui::Ui,
        project: &Project,
        scene: &Scene,
        root: &Path,
        viewport: Rect,
    ) -> Vec<Id> {
        self.paint(ui, project, scene, root, viewport, true)
    }

    fn paint(
        &mut self,
        ui: &mut egui::Ui,
        project: &Project,
        scene: &Scene,
        root: &Path,
        viewport: Rect,
        authoring: bool,
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
                            let image = oxy_core::names::resolve(root, &asset.path)
                                .map_err(|error| error.to_string())
                                .and_then(|path| image::open(path).map_err(|e| e.to_string()))
                                .map_err(|error| format!("Imagem {}: {error}", asset.name))?;
                            let limit = ui.ctx().input(|input| input.max_texture_side);
                            if image.width().max(image.height()) as usize > limit {
                                return Err(format!(
                                    "Imagem {} excede o limite de {limit} pixels da GPU",
                                    asset.name
                                ));
                            }
                            Ok(image)
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
            let font = game_ui::font_size(element, rect.height(), texture.is_some())
                .map(FontId::proportional)
                .unwrap_or_default();
            match element.kind {
                UiKind::Text => aligned_text(&painter, rect, element.text_align, text, font, color),
                UiKind::Image => {
                    if let Some(texture) = texture {
                        painter.image(
                            texture.id(),
                            fit_image(rect, texture.size_vec2()),
                            Rect::from_min_max(Pos2::ZERO, Pos2::new(1., 1.)),
                            color,
                        );
                    } else {
                        // Without an image, a panel in its colour: the backdrop of a menu.
                        painter.rect_filled(rect, 0., color);
                        if authoring {
                            let corners = [
                                rect.left_top(),
                                rect.right_top(),
                                rect.right_bottom(),
                                rect.left_bottom(),
                                rect.left_top(),
                            ];
                            painter.extend(egui::Shape::dashed_line(
                                &corners,
                                Stroke::new(1., MISSING_IMAGE_OUTLINE),
                                5.,
                                4.,
                            ));
                        }
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
                            font,
                            color,
                        );
                    } else {
                        painter.text(rect.center(), Align2::CENTER_CENTER, text, font, color);
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
                        font,
                        Color32::WHITE,
                    );
                }
            }
        }
        clicks
    }
}

/// `text` across `rect` as `align` asks, each line on its own, the whole block centred on the
/// rectangle's height.
fn aligned_text(
    painter: &egui::Painter,
    rect: Rect,
    align: UiTextAlign,
    text: String,
    font: FontId,
    color: Color32,
) {
    let (halign, x) = match align {
        UiTextAlign::Left => (Align::LEFT, rect.left()),
        UiTextAlign::Center => (Align::Center, rect.center().x),
        UiTextAlign::Right => (Align::RIGHT, rect.right()),
    };
    let mut job = LayoutJob::simple(text, font, color, f32::INFINITY);
    job.halign = halign;
    let galley = painter.layout_job(job);
    let top = rect.center().y - galley.size().y / 2.;
    painter.galley(Pos2::new(x, top), galley, color);
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

/// The dashed outline of an image element without an image, in the editor's scene view.
const MISSING_IMAGE_OUTLINE: Color32 = Color32::from_rgba_premultiplied(190, 190, 196, 220);

/// The game button under `pointer`, the topmost as `draw` lays them out: a mouse press there
/// belongs to the button, not to the actions bound to the mouse button.
pub fn button_at(scene: &Scene, viewport: Rect, pointer: Pos2) -> Option<Id> {
    let viewport = game_ui::ScreenRect {
        min: viewport.min.into(),
        max: viewport.max.into(),
    };
    game_ui::button_at(scene, viewport, pointer.into())
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

    /// The shapes `draw` (or `draw_authoring`) paints for `project`'s first scene in a 640×480
    /// view, flattened.
    fn painted(project: &Project, authoring: bool) -> Vec<egui::Shape> {
        fn flatten(shape: egui::Shape, output: &mut Vec<egui::Shape>) {
            match shape {
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        flatten(shape, output);
                    }
                }
                shape => output.push(shape),
            }
        }
        let ctx = egui::Context::default();
        let mut overlay = GameUi::new();
        let viewport = Rect::from_min_size(Pos2::ZERO, Vec2::new(640., 480.));
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(viewport),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let root = Path::new("sem-pasta");
                    if authoring {
                        overlay.draw_authoring(ui, project, &project.scenes[0], root, viewport);
                    } else {
                        overlay.draw(ui, project, &project.scenes[0], root, viewport);
                    }
                });
            },
        );
        assert!(overlay.take_errors().is_empty());
        let mut shapes = Vec::new();
        for clipped in output.shapes {
            flatten(clipped.shape, &mut shapes);
        }
        shapes
    }

    #[test]
    fn an_image_without_an_image_is_a_panel_in_its_colour() {
        use oxy_core::document::{Entity, UiAnchor};
        let mut project = Project::new("Painel");
        let mut backdrop = Entity::new("Fundo do menu", None);
        let element = UiElement {
            kind: UiKind::Image,
            anchor: UiAnchor::Center,
            position: [0., 0.],
            size: [360., 300.],
            color: [0.06, 0.08, 0.11, 0.86],
            ..Default::default()
        };
        let viewport = Rect::from_min_size(Pos2::ZERO, Vec2::new(640., 480.));
        let rect = element_rect(&element, viewport);
        let fill = rgba(element.color);
        backdrop.ui = Some(element);
        project.scenes[0].entities.push(backdrop);
        for authoring in [false, true] {
            let shapes = painted(&project, authoring);
            let panels: Vec<_> = shapes
                .iter()
                .filter_map(|shape| match shape {
                    egui::Shape::Rect(panel) if panel.fill == fill => Some(panel.rect),
                    _ => None,
                })
                .collect();
            assert_eq!(panels, [rect], "the colour, transparency kept");
            assert!(fill.a() < 255);
            assert!(
                !shapes
                    .iter()
                    .any(|shape| matches!(shape, egui::Shape::Text(_))),
                "no Imagem placeholder"
            );
            // Only the editor's scene view outlines the missing image, dashed, on its border.
            let dashes: Vec<_> = shapes
                .iter()
                .filter_map(|shape| match shape {
                    egui::Shape::LineSegment { points, stroke }
                        if stroke.color == MISSING_IMAGE_OUTLINE =>
                    {
                        Some(points)
                    }
                    _ => None,
                })
                .collect();
            if authoring {
                assert!(dashes.len() > 20, "{} dashes", dashes.len());
            } else {
                assert!(dashes.is_empty(), "{} dashes", dashes.len());
            }
            for [a, b] in dashes {
                let on_border = |p: &Pos2| {
                    (p.x - rect.left()).abs() < 0.01
                        || (p.x - rect.right()).abs() < 0.01
                        || (p.y - rect.top()).abs() < 0.01
                        || (p.y - rect.bottom()).abs() < 0.01
                };
                assert!(on_border(a) && on_border(b));
            }
        }
    }

    /// The texts `draw` paints for `elements` in a 640×480 view: each with its rows in the view
    /// (logical rects, one per line) and its font size.
    fn painted_texts(elements: Vec<UiElement>) -> Vec<(Vec<Rect>, f32)> {
        use oxy_core::document::Entity;
        let mut project = Project::new("Textos");
        for element in elements {
            let mut entity = Entity::new("Texto", None);
            entity.ui = Some(element);
            project.scenes[0].entities.push(entity);
        }
        painted(&project, false)
            .into_iter()
            .filter_map(|shape| match shape {
                egui::Shape::Text(text) => Some((
                    text.galley
                        .rows
                        .iter()
                        .map(|row| row.rect().translate(text.pos.to_vec2()))
                        .collect(),
                    text.galley.job.sections[0].format.font_id.size,
                )),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn texts_sit_left_centred_or_right_at_their_own_size() {
        use oxy_core::document::{UiAnchor, UiTextAlign};
        let title = |text_align, text_size| UiElement {
            anchor: UiAnchor::Center,
            position: [0., -110.],
            size: [520., 56.],
            text: "Chegada!".into(),
            text_align,
            text_size,
            ..Default::default()
        };
        let viewport = Rect::from_min_size(Pos2::ZERO, Vec2::new(640., 480.));
        let rect = element_rect(&title(UiTextAlign::Left, None), viewport);
        let near = |a: f32, b: f32| (a - b).abs() <= 1.;
        let drawn = painted_texts(vec![
            title(UiTextAlign::Left, None),
            title(UiTextAlign::Center, Some(48.)),
            title(UiTextAlign::Right, Some(48.)),
            title(UiTextAlign::Center, None),
        ]);
        let [left, center, right, small] = drawn.as_slice() else {
            panic!("{} texts", drawn.len());
        };
        // Automatic, as before: half of the 56 px height, from the left edge.
        assert_eq!(left.1, 28.);
        assert!(near(left.0[0].left(), rect.left()), "{left:?}");
        assert_eq!((center.1, right.1, small.1), (48., 48., 28.));
        assert!(near(center.0[0].center().x, rect.center().x), "{center:?}");
        assert!(near(small.0[0].center().x, rect.center().x), "{small:?}");
        assert!(near(right.0[0].right(), rect.right()), "{right:?}");
        // Every size stays centred on the rectangle's height.
        for (rows, _) in &drawn {
            assert!(near(rows[0].center().y, rect.center().y), "{rows:?}");
        }
        assert!(center.0[0].width() > small.0[0].width() * 1.5);

        // Each line of a longer text sits on its own; the block stays centred on the height.
        let lines = |text_align| UiElement {
            text: "Seu tempo 00:55.80\nRecorde".into(),
            ..title(text_align, Some(20.))
        };
        let drawn = painted_texts(vec![lines(UiTextAlign::Center), lines(UiTextAlign::Right)]);
        let [(center, _), (right, _)] = drawn.as_slice() else {
            panic!("{} texts", drawn.len());
        };
        assert_eq!((center.len(), right.len()), (2, 2));
        assert!(center[0].width() > center[1].width() + 20.);
        for row in center {
            assert!(near(row.center().x, rect.center().x), "{center:?}");
        }
        for row in right {
            assert!(near(row.right(), rect.right()), "{right:?}");
        }
        assert!(near(
            (center[0].top() + center[1].bottom()) / 2.,
            rect.center().y
        ));
    }

    #[test]
    fn a_fixed_size_holds_on_button_labels_and_bars_keep_theirs() {
        use oxy_core::document::UiTextAlign;
        let element = |kind, text_size| UiElement {
            kind,
            size: [240., 42.],
            text: "Jogar de novo".into(),
            // Only texts take the alignment: a button keeps its label centred.
            text_align: UiTextAlign::Right,
            text_size,
            ..Default::default()
        };
        let viewport = Rect::from_min_size(Pos2::ZERO, Vec2::new(640., 480.));
        let rect = element_rect(&element(UiKind::Button, None), viewport);
        let drawn = painted_texts(vec![
            element(UiKind::Button, None),
            element(UiKind::Button, Some(30.)),
            element(UiKind::Bar, Some(30.)),
        ]);
        let sizes: Vec<_> = drawn.iter().map(|(_, size)| *size).collect();
        assert_eq!(sizes, [16., 30., 22.]);
        for (rows, _) in &drawn[..2] {
            assert!(
                (rows[0].center().x - rect.center().x).abs() <= 1.,
                "{rows:?}"
            );
        }
    }

    #[test]
    fn an_image_past_the_gpu_limit_is_a_panel_and_an_error() {
        use oxy_core::document::{Asset, AssetKind, Entity};
        let root =
            std::env::temp_dir().join(format!("oxy-ui-limit-{}", oxy_core::document::new_id()));
        std::fs::create_dir_all(&root).unwrap();
        image::RgbaImage::from_pixel(1100, 2, image::Rgba([255; 4]))
            .save(root.join("fundo.png"))
            .unwrap();
        let mut project = Project::new("Limite");
        project.assets.push(Asset {
            id: "fundo".into(),
            name: "Fundo largo".into(),
            path: "fundo.png".into(),
            kind: AssetKind::Texture,
            model: None,
        });
        let mut backdrop = Entity::new("Fundo do menu", None);
        backdrop.ui = Some(UiElement {
            kind: UiKind::Image,
            texture: Some("fundo".into()),
            ..Default::default()
        });
        project.scenes[0].entities.push(backdrop);
        let ctx = egui::Context::default();
        let mut overlay = GameUi::new();
        for limit in [1024, 2048] {
            let input = egui::RawInput {
                max_texture_side: Some(limit),
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    overlay.draw(ui, &project, &project.scenes[0], &root, ui.max_rect());
                });
            });
            if limit < 1100 {
                // Drawn as a panel in its colour instead of reaching the GPU.
                assert!(overlay.textures.is_empty());
                assert_eq!(
                    overlay.take_errors(),
                    ["Imagem Fundo largo excede o limite de 1024 pixels da GPU"]
                );
                overlay.invalidate_texture("fundo");
            } else {
                assert!(overlay.textures.contains_key("fundo"));
                assert!(overlay.take_errors().is_empty());
            }
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn presses_belong_to_the_topmost_visible_button() {
        use oxy_core::document::{Entity, SceneKind};
        let mut scene = Scene::new("UI", SceneKind::TwoD);
        let button = |name: &str, layer: i32| {
            let mut entity = Entity::new(name, None);
            entity.ui = Some(UiElement {
                kind: UiKind::Button,
                ..Default::default()
            });
            entity.layer = layer;
            entity
        };
        let lower = button("Continuar", 0);
        let upper = button("Sair", 3);
        let mut image = Entity::new("Fundo", None);
        image.ui = Some(UiElement {
            kind: UiKind::Image,
            size: [600., 400.],
            ..Default::default()
        });
        image.layer = 9;
        let (lower_id, upper_id) = (lower.id.clone(), upper.id.clone());
        scene.entities.extend([lower, upper, image]);
        let viewport = Rect::from_min_size(Pos2::new(10., 20.), Vec2::new(640., 480.));
        let inside = Pos2::new(60., 60.);
        // An image drawn over the buttons, or beside them, takes no press: it is no control.
        assert_eq!(button_at(&scene, viewport, inside), Some(upper_id.clone()));
        assert_eq!(button_at(&scene, viewport, Pos2::new(400., 300.)), None);
        assert_eq!(button_at(&scene, viewport, Pos2::new(5., 5.)), None);
        scene.entity_mut(&upper_id).unwrap().visible = false;
        assert_eq!(button_at(&scene, viewport, inside), Some(lower_id.clone()));
        let hidden = Entity::new("Menu de pausa", None);
        let group = hidden.id.clone();
        scene.entities.push(hidden);
        scene.entity_mut(&group).unwrap().visible = false;
        scene.entity_mut(&lower_id).unwrap().parent = Some(group);
        assert_eq!(button_at(&scene, viewport, inside), None);
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
