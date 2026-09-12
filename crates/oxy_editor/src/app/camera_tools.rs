use super::*;
use oxy_core::{
    camera_authoring::{self, CameraField, CameraHandle},
    character::{CameraMode, CameraRig},
    runtime::{CameraDebug, CameraPreview},
};
use oxy_render::collider_debug::{dashed, project};

struct CameraDrag {
    id: Id,
    field: CameraField,
    original: CameraRig,
    origin: Vec3,
    axis: Vec3,
    normal: Vec3,
    start: Vec3,
    units: f32,
}
pub(crate) struct CameraTools {
    pub icons: bool,
    pub frustum: bool,
    pub eyes: bool,
    pub orbit: bool,
    pub obstruction: bool,
    pub preview: bool,
    pinned: Option<Id>,
    medium: bool,
    crouched: bool,
    source: Option<Scene>,
    evaluated: std::collections::HashMap<Id, Result<CameraDebug, String>>,
    query: CameraPreview,
    aspect: f32,
    drag: Option<CameraDrag>,
    image: Option<egui::TextureId>,
    image_key: Option<(Id, [u32; 2], bool)>,
    pub evaluations: u64,
    pub preview_draws: u64,
}
impl Default for CameraTools {
    fn default() -> Self {
        Self {
            icons: true,
            frustum: false,
            eyes: false,
            orbit: false,
            obstruction: false,
            preview: false,
            pinned: None,
            medium: false,
            crouched: false,
            source: None,
            evaluated: Default::default(),
            query: Default::default(),
            aspect: 16. / 9.,
            drag: None,
            image: None,
            image_key: None,
            evaluations: 0,
            preview_draws: 0,
        }
    }
}
impl CameraTools {
    pub(super) fn invalidate_image(&mut self) {
        self.image_key = None;
    }
    fn sync(&mut self, scene: &Scene) {
        if self.source.as_ref() != Some(scene) {
            self.source = Some(scene.clone());
            self.evaluated.clear();
            self.image_key = None;
        }
    }
    fn evaluate(&mut self, scene: &Scene, id: &str) -> Result<CameraDebug, String> {
        self.sync(scene);
        self.evaluated
            .entry(id.into())
            .or_insert_with(|| {
                self.evaluations += 1;
                self.query.evaluate(scene, id, self.aspect, self.crouched)
            })
            .clone()
    }
}
fn line(
    ui: &egui::Ui,
    camera: &CameraState,
    rect: Rect,
    a: Vec3,
    b: Vec3,
    color: Color32,
    width: f32,
) {
    if let (Some(a), Some(b)) = (project(camera, rect, a), project(camera, rect, b)) {
        ui.painter()
            .with_clip_rect(rect)
            .line_segment([a, b], egui::Stroke::new(width, color));
    }
}
fn ring(
    ui: &egui::Ui,
    camera: &CameraState,
    rect: Rect,
    center: Vec3,
    a: Vec3,
    b: Vec3,
    color: Color32,
) {
    for i in 0..48 {
        let t = i as f32 * std::f32::consts::TAU / 48.;
        let u = (i + 1) as f32 * std::f32::consts::TAU / 48.;
        line(
            ui,
            camera,
            rect,
            center + a * t.cos() + b * t.sin(),
            center + a * u.cos() + b * u.sin(),
            color,
            1.,
        );
    }
}
fn guide_label(
    ui: &egui::Ui,
    rect: Rect,
    origin: Pos2,
    offset: Vec2,
    text: String,
    color: Color32,
    occupied: &mut Vec<Rect>,
) {
    let painter = ui.painter().with_clip_rect(rect);
    let galley = painter.layout_no_wrap(text, egui::FontId::proportional(12.), color);
    let size = galley.size() + Vec2::splat(4.);
    let x = (origin.x + offset.x - if offset.x < 0. { size.x } else { 0. }).clamp(
        rect.min.x + 4.,
        (rect.max.x - size.x - 4.).max(rect.min.x + 4.),
    );
    let mut at = Rect::from_min_size(
        Pos2::new(
            x,
            (origin.y + offset.y).clamp(
                rect.min.y + 4.,
                (rect.max.y - size.y - 4.).max(rect.min.y + 4.),
            ),
        ),
        size,
    );
    for _ in 0..12 {
        if occupied.iter().all(|r| !r.expand(3.).intersects(at)) {
            break;
        }
        at = at.translate(Vec2::new(0., size.y + 4.));
    }
    occupied.push(at);
    painter.line_segment(
        [origin, at.center()],
        egui::Stroke::new(0.8, color.gamma_multiply(0.7)),
    );
    painter.rect_filled(at, 2., Color32::from_black_alpha(185));
    painter.galley(at.min + Vec2::splat(2.), galley, color);
}
impl Editor {
    #[cfg(test)]
    pub(crate) fn camera_test_metadata(&self) -> serde_json::Value {
        serde_json::json!({"size":self.editor_size,"ui_scale":self.scale,"adapter":format!("{:?}",self.render_state.adapter.get_info())})
    }
    pub(super) fn camera_display_menu(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        ui.label("Câmeras");
        for (value, label) in [
            (&mut self.camera_tools.icons, "Mostrar ícones de câmera"),
            (&mut self.camera_tools.frustum, "Mostrar frustum"),
            (&mut self.camera_tools.eyes, "Mostrar alvo/olhos"),
            (
                &mut self.camera_tools.orbit,
                "Mostrar órbita de terceira pessoa",
            ),
            (
                &mut self.camera_tools.obstruction,
                "Mostrar proteção contra obstáculos",
            ),
            (&mut self.camera_tools.preview, "Mostrar prévia da câmera"),
        ] {
            let response = ui.checkbox(value, label);
            if label == "Mostrar frustum" {
                response.on_hover_text("Contorno do campo de visão: direção, plano próximo e abertura definidos pela câmera.");
            }
        }
    }
    pub(super) fn cancel_camera_drag(&mut self) -> bool {
        if let Some(drag) = self.camera_tools.drag.take() {
            if let Some(entity) = self.scene_mut().entity_mut(&drag.id) {
                entity.camera_rig = Some(drag.original);
            }
            return true;
        }
        false
    }
    pub(super) fn camera_dragging(&self) -> bool {
        self.camera_tools.drag.is_some()
    }
    pub(super) fn camera_guides(&mut self, ui: &mut egui::Ui, scene: &Scene, rect: Rect) -> bool {
        if scene.kind != SceneKind::ThreeD {
            return false;
        }
        let mut hovered: Option<(Id, CameraHandle)> = None;
        let mut nearest_handle = 10_f32;
        let pointer = ui
            .input(|i| i.pointer.interact_pos())
            .filter(|p| rect.contains(*p));
        for entity in scene.entities.iter().filter(|e| e.camera.is_some()) {
            let selected = self.selected.as_ref() == Some(&entity.id);
            if !selected
                && !self.camera_tools.icons
                && !self.camera_tools.frustum
                && !self.camera_tools.eyes
                && !self.camera_tools.orbit
            {
                continue;
            }
            let evaluated = match self.camera_tools.evaluate(scene, &entity.id) {
                Ok(pose) => pose,
                Err(error) => {
                    if selected {
                        ui.painter().text(
                            rect.left_top() + Vec2::new(12., 12.),
                            egui::Align2::LEFT_TOP,
                            error,
                            egui::FontId::proportional(13.),
                            Color32::YELLOW,
                        );
                    }
                    continue;
                }
            };
            let color = if selected {
                Color32::from_rgb(120, 200, 255)
            } else {
                Color32::from_gray(130)
            };
            let pose = &evaluated.pose;
            let mut labels = Vec::new();
            if let Some(p) = project(&self.camera, rect, pose.position)
                && rect.contains(p)
            {
                ui.painter().rect_stroke(
                    Rect::from_center_size(p, Vec2::new(14., 10.)),
                    2.,
                    egui::Stroke::new(1.5, color),
                    egui::StrokeKind::Middle,
                );
                ui.painter().line_segment(
                    [p + Vec2::new(7., 0.), p + Vec2::new(12., -4.)],
                    egui::Stroke::new(1.5, color),
                );
                if selected {
                    ui.painter().text(
                        rect.min + Vec2::new(12., 12.),
                        egui::Align2::LEFT_TOP,
                        &entity.name,
                        egui::FontId::proportional(12.),
                        color,
                    );
                }
            }
            if selected || self.camera_tools.frustum {
                for distance in [oxy_core::character::CAMERA_NEAR, 1.5] {
                    let corners =
                        camera_authoring::frustum(pose, self.camera_tools.aspect, distance);
                    for i in 0..4 {
                        line(
                            ui,
                            &self.camera,
                            rect,
                            corners[i],
                            corners[(i + 1) % 4],
                            color,
                            1.4,
                        );
                        line(ui, &self.camera, rect, pose.position, corners[i], color, 1.);
                    }
                }
            }
            let handles =
                camera_authoring::handles(scene, &entity.id, &evaluated).unwrap_or_default();
            let rig = entity.camera_rig.as_ref();
            if let Some(rig) = rig
                && rig.mode != CameraMode::Fixed
            {
                if (selected || self.camera_tools.eyes)
                    && let Some(target) = rig
                        .target
                        .as_deref()
                        .and_then(|id| scene.world_matrix(id).ok())
                {
                    let feet = target.w_axis.truncate();
                    for h in handles
                        .iter()
                        .filter(|h| matches!(h.field, CameraField::Eye | CameraField::CrouchedEye))
                    {
                        line(ui, &self.camera, rect, feet, h.position, color, 1.);
                    }
                }
                if rig.mode == CameraMode::ThirdPerson && (selected || self.camera_tools.orbit) {
                    for (radius, shade, label) in [
                        (rig.min_distance, 0.35, "Mínima"),
                        (rig.distance, 0.85, "Atual"),
                        (rig.max_distance, 0.25, "Máxima"),
                    ] {
                        ring(
                            ui,
                            &self.camera,
                            rect,
                            evaluated.anchor,
                            Vec3::X * radius,
                            Vec3::Z * radius,
                            color.gamma_multiply(shade),
                        );
                        if selected
                            && label != "Atual"
                            && let Some(p) =
                                project(&self.camera, rect, evaluated.anchor - Vec3::X * radius)
                                    .filter(|p| rect.contains(*p))
                        {
                            guide_label(
                                ui,
                                rect,
                                p,
                                Vec2::new(8., 12.),
                                format!("{label} · {radius:.2} m"),
                                color,
                                &mut labels,
                            );
                        }
                    }
                    let back = pose.rotation * Vec3::Z;
                    let horizontal = back.with_y(0.).normalize_or_zero();
                    let limit = rig.pitch_limit.to_radians();
                    for i in 0..32 {
                        let a = -limit + 2. * limit * i as f32 / 32.;
                        let b = -limit + 2. * limit * (i + 1) as f32 / 32.;
                        line(
                            ui,
                            &self.camera,
                            rect,
                            evaluated.anchor
                                + (horizontal * a.cos() + Vec3::Y * a.sin()) * rig.distance,
                            evaluated.anchor
                                + (horizontal * b.cos() + Vec3::Y * b.sin()) * rig.distance,
                            color.gamma_multiply(0.5),
                            1.,
                        );
                    }
                    line(
                        ui,
                        &self.camera,
                        rect,
                        evaluated.anchor,
                        evaluated.desired,
                        color,
                        1.5,
                    );
                }
            }
            if selected && self.camera_tools.obstruction {
                line(
                    ui,
                    &self.camera,
                    rect,
                    evaluated.anchor,
                    evaluated.desired,
                    Color32::YELLOW,
                    2.,
                );
                line(
                    ui,
                    &self.camera,
                    rect,
                    evaluated.desired,
                    pose.position,
                    Color32::LIGHT_RED,
                    2.,
                );
                for (a, b) in [(Vec3::X, Vec3::Y), (Vec3::X, Vec3::Z), (Vec3::Y, Vec3::Z)] {
                    ring(
                        ui,
                        &self.camera,
                        rect,
                        pose.position,
                        a * evaluated.radius,
                        b * evaluated.radius,
                        Color32::LIGHT_GREEN,
                    );
                }
                if let Some(hit) = &evaluated.contact {
                    line(
                        ui,
                        &self.camera,
                        rect,
                        hit.point,
                        hit.point + hit.normal * 0.5,
                        Color32::LIGHT_RED,
                        2.,
                    );
                }
                if let (Some(a), Some(b)) = (
                    project(&self.camera, rect, evaluated.anchor),
                    project(&self.camera, rect, evaluated.desired),
                ) {
                    let mut shapes = vec![];
                    dashed(&mut shapes, a, b, egui::Stroke::new(1., Color32::WHITE));
                    ui.painter().extend(shapes);
                }
            }
            if selected && self.selection.ids.len() == 1 {
                for h in handles {
                    let Some(p) =
                        project(&self.camera, rect, h.position).filter(|p| rect.contains(*p))
                    else {
                        continue;
                    };
                    let hot = pointer.is_some_and(|mouse| mouse.distance(p) < 10.);
                    if let Some(distance) = pointer
                        .map(|mouse| mouse.distance(p))
                        .filter(|distance| *distance < nearest_handle)
                    {
                        nearest_handle = distance;
                        hovered = Some((entity.id.clone(), h.clone()));
                    }
                    let c = if h.field == CameraField::CrouchedEye {
                        Color32::from_rgb(220, 165, 255)
                    } else {
                        color
                    };
                    ui.painter()
                        .circle_filled(p, if hot { 6. } else { 4.5 }, Color32::BLACK);
                    ui.painter().circle_stroke(p, 5., egui::Stroke::new(2., c));
                    let offset = match h.field {
                        CameraField::Eye => Vec2::new(18., -40.),
                        CameraField::CrouchedEye => Vec2::new(18., 24.),
                        CameraField::Shoulder => Vec2::new(-18., -24.),
                        _ => Vec2::new(18., 0.),
                    };
                    guide_label(
                        ui,
                        rect,
                        p,
                        offset,
                        format!("{} · {:.2} m", h.field.label(), h.field.get(rig.unwrap())),
                        c,
                        &mut labels,
                    );
                }
            }
        }
        let owned = self.camera_tools.drag.is_some() || hovered.is_some();
        if self.camera_tools.drag.is_none()
            && ui.input(|i| i.pointer.primary_pressed())
            && let (Some((id, h)), Some(pointer)) = (hovered, pointer)
        {
            if !self.structural_ready() {
                return true;
            }
            let view = (self.camera.target - self.camera.eye()).normalize_or_zero();
            let normal = (view - h.axis * view.dot(h.axis)).normalize_or_zero();
            if let Some(start) =
                super::spatial_tools::ray_plane(&self.camera, rect, pointer, h.position, normal)
            {
                self.camera_tools.drag = Some(CameraDrag {
                    id,
                    field: h.field,
                    original: scene
                        .entity(self.selected.as_deref().unwrap())
                        .unwrap()
                        .camera_rig
                        .clone()
                        .unwrap(),
                    origin: h.position,
                    axis: h.axis,
                    normal,
                    start,
                    units: h.units,
                });
            }
        }
        if let Some(drag) = &self.camera_tools.drag
            && let Some(pointer) = ui.input(|i| i.pointer.interact_pos())
            && let Some(at) = super::spatial_tools::ray_plane(
                &self.camera,
                rect,
                pointer,
                drag.origin,
                drag.normal,
            )
        {
            let id = drag.id.clone();
            let field = drag.field;
            let value = field.get(&drag.original) + (at - drag.start).dot(drag.axis) / drag.units;
            let mut rig = drag.original.clone();
            if field.set(&mut rig, value).is_ok()
                && let Some(e) = self.scene_mut().entity_mut(&id)
            {
                e.camera_rig = Some(rig);
            }
        }
        if ui.input(|i| i.pointer.primary_released()) {
            self.camera_tools.drag = None;
        }
        owned
    }
    pub(super) fn camera_preview_window(&mut self, ctx: &egui::Context) {
        if !self.camera_tools.preview || self.tab == Tab::Game {
            self.renderer.close_preview(&self.render_state);
            self.camera_tools.image = None;
            self.camera_tools.image_key = None;
            return;
        }
        let scene = if self.tab == Tab::Studio && self.studio.tab == StudioTab::Animation {
            self.animation_preview()
        } else {
            self.scene().clone()
        };
        let id = self.camera_tools.pinned.clone().or_else(|| {
            self.selected
                .clone()
                .filter(|id| scene.entity(id).is_some_and(|e| e.camera.is_some()))
        });
        let mut open = true;
        egui::Window::new("Prévia da câmera")
            .open(&mut open)
            .resizable(false)
            .default_pos(Pos2::new(380., 130.))
            .show(ctx, |ui| {
                let Some(id) = id.as_deref().filter(|id| scene.entity(id).is_some()) else {
                    ui.label("Selecione uma câmera na Hierarquia.");
                    if self.camera_tools.pinned.is_some()
                        && ui.button("Desafixar câmera ausente").clicked()
                    {
                        self.camera_tools.pinned = None;
                    }
                    return;
                };
                ui.horizontal(|ui| {
                    ui.label(&scene.entity(id).unwrap().name);
                    let mut pinned = self.camera_tools.pinned.is_some();
                    if ui.checkbox(&mut pinned, "Fixar").changed() {
                        self.camera_tools.pinned = pinned.then(|| id.to_owned());
                    }
                    ui.checkbox(&mut self.camera_tools.medium, "Média");
                });
                ui.horizontal(|ui| {
                    if ui
                        .checkbox(&mut self.camera_tools.crouched, "Prévia agachada")
                        .changed()
                    {
                        self.camera_tools.evaluated.clear();
                        self.camera_tools.image_key = None;
                    }
                    let before = self.camera_tools.aspect;
                    egui::ComboBox::from_id_salt("camera_preview_aspect")
                        .selected_text(if before > 1.5 { "16:9" } else { "4:3" })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.camera_tools.aspect, 16. / 9., "16:9");
                            ui.selectable_value(&mut self.camera_tools.aspect, 4. / 3., "4:3");
                        });
                    if before != self.camera_tools.aspect {
                        self.camera_tools.evaluated.clear();
                        self.camera_tools.image_key = None;
                    }
                });
                match self.camera_tools.evaluate(&scene, id) {
                    Ok(evaluated) => {
                        let width = (if self.camera_tools.medium {
                            480_f32
                        } else {
                            300.
                        })
                        .min(ctx.content_rect().width() - 60.);
                        let size = Vec2::new(width, width / self.camera_tools.aspect);
                        let physical = [
                            (size.x * ctx.pixels_per_point()) as u32,
                            (size.y * ctx.pixels_per_point()) as u32,
                        ];
                        let key = (id.to_owned(), physical, self.camera_tools.crouched);
                        if self.camera_tools.image_key.as_ref() != Some(&key)
                            || ui.input(|i| i.pointer.any_down())
                        {
                            let camera = CameraState::from_pose(&scene, &evaluated.pose);
                            self.camera_tools.image = Some(self.renderer.render_preview(
                                &self.render_state,
                                &self.state.project,
                                &scene,
                                &self.root(),
                                &camera,
                                physical,
                            ));
                            self.camera_tools.preview_draws += 1;
                            self.camera_tools.image_key = Some(key);
                        }
                        if let Some(texture) = self.camera_tools.image {
                            ui.image((texture, size));
                        }
                        ui.small("Prévia da cena em edição · não inicia a simulação.");
                    }
                    Err(error) => {
                        ui.colored_label(Color32::YELLOW, error);
                    }
                }
            });
        self.camera_tools.preview = open;
    }
}
