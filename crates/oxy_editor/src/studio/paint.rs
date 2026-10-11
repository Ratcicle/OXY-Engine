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
    /// Paint tools in the Studio's first row.
    pub(crate) fn paint_tool_choice(&mut self, ui: &mut egui::Ui) {
        use crate::icons::Icon;
        let tools = [
            (
                PaintTool::Brush,
                Icon::Brush,
                "Pincel",
                "Pinta pixels reais; arraste para fazer uma pincelada e Ctrl+Z desfaz o traço inteiro.",
            ),
            (
                PaintTool::Fill,
                Icon::Fill,
                "Preencher região",
                "Preenche a região de pixels conectados com a cor escolhida.",
            ),
            (
                PaintTool::Sample,
                Icon::Sample,
                "Conta-gotas",
                "Copia a cor do pixel apontado para o pincel, sem alterar a imagem.",
            ),
            (
                PaintTool::Select,
                Icon::PaintSelect,
                "Selecionar faces",
                "Escolha uma face na peça ou em sua ilha de textura. Ctrl alterna a seleção sem pintar.",
            ),
        ];
        let labels = tools.map(|(_, _, label, _)| label);
        let names =
            ui.available_width() >= crate::widgets::segmented_icons_width(ui, &labels, true);
        if names
            || ui.available_width() >= crate::widgets::segmented_icons_width(ui, &labels, false)
        {
            crate::widgets::segmented_icons(ui, &mut self.studio.tool, &tools, names);
        } else {
            let (_, icon, label, _) = tools
                .into_iter()
                .find(|(tool, ..)| *tool == self.studio.tool)
                .unwrap_or(tools[0]);
            crate::icons::menu_button(
                ui,
                icon,
                "Pintar",
                &format!("Ferramenta: {label}. Abra para escolher outra ferramenta de pintura."),
                false,
                |ui| {
                    for (tool, icon, label, tip) in tools {
                        if crate::icons::button(
                            ui,
                            icon,
                            label,
                            tip,
                            self.studio.tool == tool,
                            true,
                        )
                        .clicked()
                        {
                            self.studio.tool = tool;
                            ui.close();
                        }
                    }
                },
            );
        }
    }
    /// Brush colour: a swatch that opens the picker with the palette below it.
    fn paint_color_button(&mut self, ui: &mut egui::Ui) {
        use crate::{icons, theme};
        let [r, g, b, a] = self.studio.color;
        let text = if a == 0 {
            "Transparente".to_owned()
        } else {
            format!("#{r:02X}{g:02X}{b:02X}")
        };
        let font = egui::FontId::monospace(12.);
        let text_width = ui
            .painter()
            .layout_no_wrap(text.clone(), font.clone(), Color32::WHITE)
            .size()
            .x;
        let size = Vec2::new(
            8. + 18. + 8. + text_width + 8. + 12. + 6.,
            theme::CONTROL_HEIGHT + 2.,
        );
        let (rect, response) = ui.allocate_exact_size(size, Sense::click());
        icons::register_qa(ui, "Cor do pincel", rect);
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), "Cor do pincel")
        });
        let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&response));
        if response.hovered() || open {
            ui.painter()
                .rect_filled(rect, 5., ui.style().interact(&response).weak_bg_fill);
        }
        let swatch = Rect::from_min_size(
            egui::pos2(rect.left() + 8., rect.center().y - 9.),
            Vec2::splat(18.),
        );
        paint_swatch(ui, swatch, self.studio.color, false);
        ui.painter().text(
            egui::pos2(swatch.right() + 8., rect.center().y),
            egui::Align2::LEFT_CENTER,
            text,
            font,
            theme::TEXT,
        );
        icons::paint(
            ui,
            Rect::from_center_size(
                egui::pos2(rect.right() - 12., rect.center().y),
                Vec2::splat(11.),
            ),
            icons::Icon::ChevronDown,
            theme::TEXT_MUTED,
        );
        let response = response.on_hover_text(
            "Cor do pincel e do preenchimento; o último valor controla a transparência.",
        );
        egui::Popup::from_toggle_button_response(&response)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                ui.set_width(220.);
                let mut color = Color32::from_rgba_unmultiplied(r, g, b, a);
                if egui::color_picker::color_picker_color32(
                    ui,
                    &mut color,
                    egui::color_picker::Alpha::OnlyBlend,
                ) {
                    self.studio.color = color.to_srgba_unmultiplied();
                }
                ui.add_space(4.);
                crate::widgets::menu_label(ui, "Paleta");
                self.paint_palette(ui);
            });
    }
    fn paint_palette(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::splat(4.);
            for (color, name) in [
                ([235, 231, 218, 255], "Creme"),
                ([38, 44, 55, 255], "Ardósia escura"),
                ([184, 64, 53, 255], "Vermelho"),
                ([213, 165, 77, 255], "Ocre"),
                ([91, 168, 152, 255], "Verde-água"),
                ([86, 127, 181, 255], "Azul"),
                ([122, 84, 152, 255], "Roxo"),
                ([0, 0, 0, 0], "Transparente: apaga pixels"),
            ] {
                let (rect, response) = ui.allocate_exact_size(Vec2::splat(22.), Sense::click());
                crate::icons::register_qa(ui, name, rect);
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), name)
                });
                paint_swatch(
                    ui,
                    rect,
                    color,
                    response.hovered() || self.studio.color == color,
                );
                if response
                    .on_hover_text(format!(
                        "{name} · RGBA {}, {}, {}, {}",
                        color[0], color[1], color[2], color[3]
                    ))
                    .clicked()
                {
                    self.studio.color = color;
                }
            }
        });
    }
    fn paint_texture_actions(&mut self, ui: &mut egui::Ui) {
        use crate::icons::{self, Icon};
        ui.set_min_width(240.);
        ui.add_enabled_ui(self.selected.is_some(), |ui| {
            icons::menu_button(
                ui,
                Icon::TextureNew,
                "Criar textura",
                "Cria uma imagem editável e a associa à peça selecionada.",
                true,
                |ui| {
                    if ui.button("Criar 256×256").clicked() {
                        self.create_texture(256);
                        ui.close();
                    }
                    if ui.button("Criar 512×512").clicked() {
                        self.create_texture(512);
                        ui.close();
                    }
                },
            );
        });
        if icons::button(
            ui,
            Icon::Import,
            "Importar PNG",
            "Importa uma cópia do PNG para o projeto, preservando o arquivo original.",
            false,
            true,
        )
        .clicked()
        {
            self.import(AssetKind::Texture);
        }
        let id = self.texture_id();
        if ui
            .add_enabled_ui(id.is_some(), |ui| {
                icons::button(
                    ui,
                    Icon::Export,
                    "Exportar PNG",
                    "Salva os pixels atuais em uma imagem PNG.",
                    false,
                    true,
                )
            })
            .inner
            .on_disabled_hover_text("Aplique ou crie uma textura primeiro.")
            .clicked()
            && let Some(id) = id
            && let Some(image) = self.state.images.get(&id)
            && let Some(path) = rfd::FileDialog::new()
                .add_filter("PNG", &["png"])
                .set_file_name("pintura.png")
                .save_file()
        {
            match image.save(&path) {
                Ok(_) => self.log("PNG exportado com pixels reais."),
                Err(error) => self.log(error),
            }
        }
        if let Some(id) = self.selected.clone()
            && let Some(mut entity) = self.scene().entity(&id).cloned()
        {
            ui.separator();
            let interface = !entity.has_geometry() && entity.ui.is_some();
            self.texture_controls(ui, &mut entity, interface);
            if let Some(original) = self.scene_mut().entity_mut(&id) {
                *original = entity;
            }
        }
    }
    /// Second Pintura row: colour and radius at the left, texture actions at the right.
    fn paint_toolbar(&mut self, ui: &mut egui::Ui) {
        use crate::icons::{self, Icon};
        let (rect, mut right) = crate::widgets::toolbar_right(ui, super::ROW_HEIGHT);
        icons::menu_button(
            &mut right,
            Icon::TextureNew,
            "Textura",
            "Criar, importar, exportar, substituir ou remover o vínculo da textura.",
            true,
            |ui| self.paint_texture_actions(ui),
        );
        let mut left = crate::widgets::toolbar_left(ui, rect, &right);
        self.paint_color_button(&mut left);
        if left.available_width() > 190. {
            left.add_space(6.);
            left.label(
                egui::RichText::new("Raio")
                    .size(12.5)
                    .color(crate::theme::TEXT_MUTED),
            );
            left.spacing_mut().slider_width = 110.;
            left.add(
                egui::Slider::new(&mut self.studio.radius, 0.5..=64.)
                    .logarithmic(true)
                    .max_decimals(1)
                    .suffix(" px"),
            )
            .on_hover_text("Raio do pincel em pixels da textura, independente do zoom da câmera.");
        } else if left.available_width() > 90. {
            left.add(
                egui::DragValue::new(&mut self.studio.radius)
                    .speed(0.25)
                    .range(0.5..=64.)
                    .prefix("Raio ")
                    .suffix(" px"),
            )
            .on_hover_text("Raio do pincel em pixels da textura, independente do zoom da câmera.");
        }
        #[cfg(test)]
        {
            self.studio.header_bottom = ui.cursor().top();
        }
    }
    pub(super) fn paint_ui(&mut self, ui: &mut egui::Ui) {
        let compact = ui.available_width() < 620. || ui.available_height() < 340.;
        if !ui.input(|i| i.pointer.primary_down()) {
            self.studio.last_pixel = None;
        }
        if let Some(id) = self.texture_id() {
            self.ensure_texture(&id);
        }
        self.paint_toolbar(ui);
        if self.selected.is_none() {
            self.viewport(ui, true);
            return;
        }
        let Some(id) = self.texture_id() else {
            let area = ui.available_rect_before_wrap();
            self.viewport(ui, true);
            self.paint_missing_texture(ui, area);
            return;
        };
        let uses = self.texture_uses(&id);
        if uses > 1 && self.studio.shared_edit.as_ref() != Some(&id) {
            let text = format!(
                "Esta textura está vinculada a {uses} peças ou imagens. A pintura afetará todas essas referências."
            );
            crate::widgets::banner(ui, &text, |ui| {
                if ui.button("Editar textura compartilhada").clicked() {
                    self.studio.shared_edit = Some(id.clone());
                }
                if ui.button("Criar cópia independente").clicked() {
                    self.copy_texture(&id)
                }
            });
        }
        let texture_name = self
            .state
            .project
            .asset(&id)
            .map_or_else(|| "Textura".to_owned(), |a| a.name.clone());
        let width = (ui.available_width() * if compact { 0.35 } else { 0.48 }).max(100.);
        let max_width = if compact {
            width
        } else {
            (ui.available_width() * 0.65).max(100.)
        };
        let mesh = self.model_source().ok();
        let key = self
            .state
            .images
            .buffer_identity(&id)
            .map(|identity| (id.clone(), identity));
        egui::SidePanel::left("paint_pixels")
            .resizable(true)
            .default_width(width)
            .width_range(100.0..=max_width)
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
                    let size_text = format!("{} × {} · RGBA", image.width, image.height);
                    let show_uv = &mut self.studio.show_uv;
                    egui::containers::Sides::new()
                        .height(crate::theme::CONTROL_HEIGHT + 4.)
                        .shrink_left()
                        .truncate()
                        .show(
                            ui,
                            |ui| {
                                ui.label(
                                    egui::RichText::new(&texture_name)
                                        .font(crate::theme::medium(12.5)),
                                );
                                ui.label(
                                    egui::RichText::new(size_text)
                                        .size(12.)
                                        .color(crate::theme::TEXT_MUTED),
                                );
                            },
                            |ui| {
                                crate::widgets::toggle(ui, show_uv, "Ilhas UV");
                                ui.label(
                                    egui::RichText::new("Ilhas UV")
                                        .size(12.)
                                        .color(crate::theme::TEXT_MUTED),
                                );
                            },
                        );
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
                    if self.studio.show_uv
                        && let Some(mesh) = &mesh
                    {
                        self.draw_uv_islands(ui, rect, mesh);
                    }
                    if let Some(uv) = self.studio.hover_uv {
                        let p = rect.min + Vec2::new(uv[0] * rect.width(), uv[1] * rect.height());
                        ui.painter()
                            .circle_stroke(p, 5., egui::Stroke::new(1.5, Color32::WHITE));
                    }
                    // Menus may overlap the UV image. Only the image's own hit target can
                    // start a stroke, so a popup click cannot leave a hidden brush origin.
                    let press = response
                        .hovered()
                        .then(|| {
                            ui.input(|i| {
                                i.events.iter().find_map(|event| match event {
                                    egui::Event::PointerButton {
                                        pos,
                                        button: egui::PointerButton::Primary,
                                        pressed: true,
                                        ..
                                    } if rect.contains(*pos) => Some(*pos),
                                    _ => None,
                                })
                            })
                        })
                        .flatten();
                    if press.is_some() {
                        // A press that closes an open menu must not paint under it.
                        self.studio.stroke_blocked = egui::Popup::is_any_open(ui.ctx());
                    }
                    if let Some(p) = press.filter(|_| !self.studio.stroke_blocked) {
                        self.studio.last_pixel = Some([
                            (p.x - rect.left()) / rect.width() * image.width as f32,
                            (p.y - rect.top()) / rect.height() * image.height as f32,
                        ]);
                    }
                    if (response.dragged_by(egui::PointerButton::Primary)
                        || response.clicked()
                        || press.is_some())
                        && !self.studio.stroke_blocked
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
                        crate::theme::ERROR,
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
    /// Tool, colour and next step for the viewport status line.
    pub(crate) fn paint_status(&self) -> (Color32, String, &'static str) {
        let [r, g, b, a] = self.studio.color;
        let brush = if a == 0 {
            crate::theme::TEXT_MUTED
        } else {
            Color32::from_rgb(r, g, b)
        };
        let Some(id) = self.texture_id() else {
            return (
                crate::theme::TEXT_MUTED,
                "Sem textura".into(),
                "crie uma textura para pintar esta peça",
            );
        };
        if self.texture_uses(&id) > 1 && self.studio.shared_edit.as_ref() != Some(&id) {
            return (
                crate::theme::WARNING,
                "Pintura pausada".into(),
                "escolha uma opção no aviso acima",
            );
        }
        match self.studio.tool {
            PaintTool::Brush => (
                brush,
                format!("Pincel · raio {:.1} px", self.studio.radius),
                "arraste na peça ou na textura · Ctrl+Z desfaz o traço inteiro",
            ),
            PaintTool::Fill => (
                brush,
                "Preenchimento".into(),
                "clique numa área de pixels conectados",
            ),
            PaintTool::Sample => (
                crate::theme::ACCENT_BRIGHT,
                "Conta-gotas ativo".into(),
                "clique num pixel para copiar a cor",
            ),
            PaintTool::Select => (
                crate::theme::HIGHLIGHT,
                "Seleção de faces".into(),
                "clique numa face na peça ou na ilha · Ctrl alterna",
            ),
        }
    }
    /// A piece without a texture shows how to get one instead of an empty view.
    fn paint_missing_texture(&mut self, ui: &mut egui::Ui, area: Rect) {
        let Some(name) = self
            .selected
            .as_deref()
            .and_then(|id| self.scene().entity(id))
            .map(|e| e.name.clone())
        else {
            return;
        };
        crate::widgets::empty_state(
            ui,
            area,
            crate::icons::Icon::Sprite,
            &format!("{name} ainda não tem textura"),
            "Crie uma imagem para pintar pixels nesta peça ou aplique uma textura da Biblioteca.",
            |ui| {
                let width = 180.;
                if ui
                    .add_sized(
                        [width, 28.],
                        egui::Button::new(
                            egui::RichText::new("Criar 256×256").color(Color32::WHITE),
                        )
                        .fill(crate::theme::ACCENT),
                    )
                    .clicked()
                {
                    self.create_texture(256);
                }
                if ui
                    .add_sized([width, 28.], egui::Button::new("Criar 512×512"))
                    .clicked()
                {
                    self.create_texture(512);
                }
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new("Escolher na Biblioteca")
                                .color(crate::theme::ACCENT_BRIGHT),
                        )
                        .frame(false),
                    )
                    .clicked()
                {
                    self.show_dock(crate::app::DockTab::Library);
                }
            },
        );
    }
    /// Texture block of the inspector while painting.
    pub(crate) fn paint_texture_card(&mut self, ui: &mut egui::Ui) {
        let id = self.texture_id();
        crate::widgets::open_card(ui, "paint_texture", "Textura", |ui| {
            let Some(id) = id else {
                crate::widgets::hint(ui, "Sem textura. A peça usa só a cor do material.");
                ui.horizontal(|ui| {
                    if ui.button("Criar 256×256").clicked() {
                        self.create_texture(256);
                    }
                    if ui.button("Criar 512×512").clicked() {
                        self.create_texture(512);
                    }
                });
                return;
            };
            let name = self
                .state
                .project
                .asset(&id)
                .map_or_else(|| "Textura ausente".to_owned(), |a| a.name.clone());
            let size = self
                .state
                .images
                .get(&id)
                .map(|image| format!("{} × {} · RGBA", image.width, image.height))
                .unwrap_or_default();
            let uses = self.texture_uses(&id);
            ui.horizontal(|ui| {
                self.texture_thumbnail_sized(ui, &id, 52.);
                ui.vertical(|ui| {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&name).font(crate::theme::medium(13.)),
                        )
                        .truncate(),
                    );
                    ui.label(
                        egui::RichText::new(size)
                            .size(12.)
                            .color(crate::theme::TEXT_MUTED),
                    );
                    let (text, color) = if uses > 1 {
                        (format!("Usada por {uses} peças"), crate::theme::WARNING)
                    } else {
                        (
                            "Usada só por esta peça".to_owned(),
                            crate::theme::TEXT_MUTED,
                        )
                    };
                    ui.label(egui::RichText::new(text).size(12.).color(color));
                });
            });
            ui.horizontal(|ui| {
                let width = ((ui.available_width() - ui.spacing().item_spacing.x) / 2.).max(40.);
                if ui
                    .add_sized([width, 26.], egui::Button::new("Exportar PNG"))
                    .clicked()
                {
                    self.export_texture(&id);
                }
                if ui
                    .add_sized([width, 26.], egui::Button::new("Substituir…"))
                    .on_hover_text("Importe uma cópia de outro PNG e aplique somente a esta peça.")
                    .clicked()
                    && let Some(entity) = self.selected.clone()
                    && let Some(updated) = self.import_selected(AssetKind::Texture)
                    && let Some(e) = self.scene_mut().entity_mut(&entity)
                {
                    if !e.has_geometry()
                        && let Some(element) = &mut e.ui
                    {
                        element.texture = Some(updated);
                    } else {
                        e.material.texture = Some(updated);
                    }
                }
            });
        });
    }
    fn export_texture(&mut self, id: &str) {
        if let Some(image) = self.state.images.get(id)
            && let Some(path) = rfd::FileDialog::new()
                .add_filter("PNG", &["png"])
                .set_file_name("pintura.png")
                .save_file()
        {
            match image.save(&path) {
                Ok(_) => self.log("PNG exportado com pixels reais."),
                Err(error) => self.log(error),
            }
        }
    }
}

/// Colour square; transparency shows over a checkerboard.
fn paint_swatch(ui: &egui::Ui, rect: Rect, color: [u8; 4], highlight: bool) {
    let painter = ui.painter();
    if color[3] < 255 {
        let half = rect.width() / 2.;
        for (i, offset) in [(0., 0.), (half, 0.), (0., half), (half, half)]
            .into_iter()
            .enumerate()
        {
            painter.rect_filled(
                Rect::from_min_size(rect.min + Vec2::new(offset.0, offset.1), Vec2::splat(half)),
                0.,
                if i == 0 || i == 3 {
                    Color32::from_gray(138)
                } else {
                    Color32::from_gray(90)
                },
            );
        }
    }
    painter.rect_filled(
        rect,
        4.,
        Color32::from_rgba_unmultiplied(color[0], color[1], color[2], color[3]),
    );
    painter.rect_stroke(
        rect,
        4.,
        egui::Stroke::new(
            if highlight { 2. } else { 1. },
            if highlight {
                crate::theme::ACCENT_BRIGHT
            } else {
                Color32::from_gray(86)
            },
        ),
        egui::StrokeKind::Inside,
    );
}
