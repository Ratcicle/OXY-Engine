//! Direct gestures and bounded adjustment of the last committed operation.
use super::*;
use oxy_render::collider_debug::project;

#[derive(Clone, Copy, Default, PartialEq)]
pub(super) enum Direction {
    #[default]
    Normal,
    X,
    Y,
    Z,
    Vector,
}
pub(super) struct LastOperation {
    pub(super) command: u64,
    scene: Id,
    pub(super) source: Preview,
}
impl Preview {
    fn title(&self) -> &'static str {
        match self.operation {
            Operation::Transform(Gizmo::Move) => "Mover componentes",
            Operation::Transform(Gizmo::Rotate) => "Girar componentes",
            Operation::Transform(Gizmo::Scale) => "Escalar componentes",
            Operation::Delete => "Excluir componentes",
            Operation::Triangulate => "Triangular faces",
            Operation::Extrude => "Extrudir",
            Operation::Inset => "Borda interna",
            Operation::Create => "Criar face ou aresta",
            Operation::Flip => "Inverter orientação",
            Operation::Loop => "Corte em loop",
            Operation::Knife => "Bisturi",
            Operation::Bevel => "Arredondar",
        }
    }
    pub(super) fn neutral(&self) -> bool {
        match self.operation {
            Operation::Transform(Gizmo::Scale) => self.values == [1.; 3],
            Operation::Transform(_) => self.values == [0.; 3],
            Operation::Bevel => self.values[0] == 0.,
            Operation::Extrude => self.inner_size == 100. && self.displacement(None) == Vec3::ZERO,
            Operation::Inset => self.inner_size == 100.,
            _ => false,
        }
    }
    /// A local displacement, evaluated afresh from the immutable source. World
    /// normals use inverse-transpose, including reflected/nonuniform transforms.
    pub(super) fn displacement(&self, face: Option<u32>) -> Vec3 {
        self.displacement_with_values(face, self.values)
    }
    fn displacement_with_values(&self, face: Option<u32>, values: [f32; 3]) -> Vec3 {
        if self.operation == Operation::Inset {
            return Vec3::ZERO;
        }
        if self.direction == Direction::Vector {
            let v = Vec3::from(values);
            return if self.global {
                self.world.inverse().transform_vector3(v)
            } else {
                v
            };
        }
        let local = match self.direction {
            Direction::Normal => {
                let prepared = self.source.prepared();
                let n = if let Some(face) = face {
                    prepared
                        .faces
                        .get(&face)
                        .map_or(Vec3::ZERO, |&i| prepared.face_normals[i])
                } else {
                    self.selection
                        .ids
                        .iter()
                        .filter_map(|id| prepared.faces.get(id))
                        .map(|&i| prepared.face_normals[i])
                        .sum::<Vec3>()
                };
                if n.length_squared() > 1e-10 {
                    n.normalize()
                } else {
                    Vec3::Y
                }
            }
            Direction::X => Vec3::X,
            Direction::Y => Vec3::Y,
            Direction::Z => Vec3::Z,
            Direction::Vector => unreachable!(),
        };
        let axis = if !self.global {
            local
        } else if self.direction == Direction::Normal {
            let normal = self
                .world
                .inverse()
                .transpose()
                .transform_vector3(local)
                .normalize_or_zero();
            self.world.inverse().transform_vector3(normal)
        } else {
            self.world.inverse().transform_vector3(local)
        };
        axis * values[0]
    }
    fn extrusion_axis(&self) -> Vec3 {
        let values = if self.direction == Direction::Vector {
            self.drag.as_ref().map_or(self.values, |drag| drag.base)
        } else {
            [1., 0., 0.]
        };
        let local = self.displacement_with_values(None, values);
        if local.length_squared() <= 1e-12 {
            return if self.direction == Direction::Vector && self.global {
                Vec3::X
            } else {
                self.world.transform_vector3(Vec3::X).normalize_or_zero()
            };
        }
        self.world.transform_vector3(local).normalize_or_zero()
    }
}
impl Editor {
    pub(in crate::app) fn mesh_input_gesture_active(&self) -> bool {
        self.modeling
            .preview
            .as_ref()
            .is_some_and(|p| p.drag.is_some() || p.numeric_edit)
    }
    pub(in crate::app) fn forget_last_mesh_operation(&mut self) {
        self.modeling.last_operation = None;
    }
    pub(in crate::app) fn validate_last_mesh_operation(&mut self) {
        if self.modeling.last_operation.as_ref().is_some_and(|last| {
            self.history.last_command_id() != Some(last.command)
                || self.scene().id != last.scene
                || self.selected.as_ref() != Some(&last.source.entity)
                || (self
                    .history
                    .pending_changed(&self.state.project, &self.state.images)
                    && self.modeling.preview.is_none())
        }) {
            self.forget_last_mesh_operation();
        }
    }
    pub(super) fn start_mesh_gesture(&mut self) -> bool {
        let Some(p) = self.modeling.preview.as_ref() else {
            return false;
        };
        if p.started {
            return true;
        }
        let amendment = p.amendment;
        let title = p.title();
        let mut entity = p.original.clone();
        let texture = p.texture.as_ref().map(|(id, _)| id.clone());
        if let Some(id) = texture
            && !self.ensure_texture(&id)
        {
            return false;
        }
        if let Some(command) = amendment {
            self.modeling.preview.as_mut().unwrap().rollback_selection =
                Some(self.modeling.selection.clone());
            match self
                .history
                .begin_amend_last(command, &self.state.project, &self.state.images)
            {
                Ok(true) => (),
                Ok(false) => {
                    self.warn("A última operação já não pode ser ajustada.");
                    return false;
                }
                Err(error) => {
                    self.warn(error);
                    return false;
                }
            }
        } else {
            self.history
                .begin(title, &self.state.project, &self.state.images);
        }
        if let Err(error) = primitives::convert(&mut entity) {
            self.history
                .cancel(&mut self.state.project, &mut self.state.images);
            self.warn(error);
            return false;
        }
        let id = entity.id.clone();
        if let Some(target) = self.scene_mut().entity_mut(&id) {
            *target = entity;
        } else {
            self.cancel_mesh_operation();
            return false;
        }
        self.modeling.preview.as_mut().unwrap().started = true;
        true
    }
    pub(super) fn restore_neutral_mesh(&mut self) -> bool {
        let Some(p) = &self.modeling.preview else {
            return false;
        };
        if !p.neutral() {
            return false;
        }
        let original = p.original.clone();
        let selection = p.selection.clone();
        let mut temporary = p.expanded_texture.clone();
        self.remove_preview_texture(&mut temporary);
        *self.scene_mut().entity_mut(&original.id).unwrap() = original.clone();
        let p = self.modeling.preview.as_mut().unwrap();
        p.error = None;
        p.needs_space = false;
        p.previous = p.values;
        self.modeling.selection = selection;
        true
    }
    pub(super) fn confirm_mesh_operation(&mut self) {
        let Some(p) = self.modeling.preview.as_ref() else {
            return;
        };
        if p.needs_space && !p.allow_expansion {
            self.warn("Autorize a cópia ampliada da textura nas propriedades ou pressione Esc.");
            return;
        }
        if let Some(error) = p.error.clone() {
            self.cancel_mesh_operation();
            self.warn(format!("Operação cancelada: {error}"));
            return;
        }
        if !p.started {
            self.cancel_mesh_operation();
            return;
        }
        let mut p = self.modeling.preview.take().unwrap();
        self.modeling.source = None;
        self.finish_history(true);
        if let Some(command) = self.history.last_command_id()
            && !p.neutral()
            && matches!(
                p.operation,
                Operation::Transform(_)
                    | Operation::Extrude
                    | Operation::Inset
                    | Operation::Bevel
                    | Operation::Loop
            )
        {
            p.started = false;
            p.numeric_edit = false;
            p.drag = None;
            p.amendment = None;
            p.rollback_selection = None;
            p.previous = [f32::NAN; 3];
            self.modeling.last_operation = Some(LastOperation {
                command,
                scene: self.scene().id.clone(),
                source: p,
            });
        } else {
            self.forget_last_mesh_operation();
        }
        let _ = self.model_source();
    }
    pub(in crate::app) fn mesh_preview_panel(&mut self, ui: &mut egui::Ui) {
        self.validate_last_mesh_operation();
        let armed = self.modeling.preview.is_some();
        let candidate = if let Some(p) = self.modeling.preview.as_ref() {
            p
        } else if let Some(last) = &self.modeling.last_operation {
            &last.source
        } else {
            return;
        };
        // Only the small controls are copied during drawing. The immutable mesh,
        // original entity/graph and selected component IDs remain borrowed.
        let mut controls = Controls::from(candidate);
        let mut edit = Edit::default();
        let title = if armed {
            "Ferramenta"
        } else {
            "Última operação"
        };
        egui::CollapsingHeader::new(title).id_salt("direct_mesh_parameters").default_open(true).show(ui, |ui| {
            ui.strong(candidate.title());
            if armed && !candidate.started {
                ui.small("Arraste a alça na cena ou informe um valor.")
                    .on_hover_text("Soltar a alça aplica. Enter ou sair de um campo válido conclui sua edição. Esc cancela o gesto; a peça só é convertida quando houver alteração.");
            } else if !armed {
                ui.small("Ajusta o mesmo comando de desfazer.")
                    .on_hover_text("Recalculado a partir da peça anterior à operação. Outra edição encerra este ajuste; mover a câmera não o encerra.");
            }
            parameters(ui, candidate, &mut controls, &mut edit, !armed);
            for note in &candidate.notes { ui.small(note); }
            if candidate.needs_space || candidate.allow_expansion {
                ui.label("A ampliação preserva a pintura e cria uma textura independente para esta peça.");
                let response = ui.checkbox(&mut controls.allow_expansion, "Criar cópia ampliada (2×)");
                if response.changed() { edit.changed = true; edit.ended = true; }
            }
            if let Some(error) = &candidate.error { ui.colored_label(Color32::LIGHT_YELLOW, error); }
        });
        let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
        if edit.invalid {
            if armed {
                self.cancel_mesh_operation();
            }
            self.warn("Valor inválido. A operação foi restaurada; informe um número dentro dos limites do campo.");
            return;
        }
        if armed {
            let candidate = self.modeling.preview.as_mut().unwrap();
            controls.apply(candidate);
            if edit.changed {
                candidate.previous = [f32::NAN; 3];
                candidate.numeric_edit = true;
            }
            if edit.changed {
                self.update_mesh_preview();
            }
            if edit.ended || enter && self.modeling.preview.as_ref().is_some_and(|p| p.started) {
                self.confirm_mesh_operation();
            }
        } else if edit.changed {
            // A real adjustment needs its own mutable preview and rollback state.
            let mut candidate = self
                .modeling
                .last_operation
                .as_ref()
                .unwrap()
                .source
                .clone();
            controls.apply(&mut candidate);
            candidate.amendment = self.modeling.last_operation.as_ref().map(|p| p.command);
            candidate.numeric_edit = true;
            candidate.previous = [f32::NAN; 3];
            self.modeling.preview = Some(candidate);
            self.update_mesh_preview();
            if edit.ended || enter {
                self.confirm_mesh_operation();
            }
        }
    }
    /// Constant-screen-size operation handle; displacement comes from a world ray
    /// intersecting a plane through the source selection, never screen pixels as units.
    pub(super) fn direct_topology_gizmo(&mut self, ui: &mut egui::Ui, rect: Rect) -> bool {
        self.validate_last_mesh_operation();
        let from_last = self.modeling.preview.is_none();
        let Some(p) = self
            .modeling
            .preview
            .as_ref()
            .or_else(|| {
                self.modeling
                    .last_operation
                    .as_ref()
                    .map(|last| &last.source)
            })
            .filter(|p| {
                matches!(
                    p.operation,
                    Operation::Extrude | Operation::Inset | Operation::Bevel
                )
            })
        else {
            return false;
        };
        let center = p.world.transform_point3(p.center);
        let axis = if p.operation == Operation::Extrude {
            p.extrusion_axis()
        } else {
            p.world.transform_vector3(Vec3::X).normalize_or_zero()
        };
        let Some(origin) = project(&self.camera, rect, center) else {
            return false;
        };
        let projected = project(&self.camera, rect, center + axis).map_or(Vec2::X, |p| p - origin);
        let direction = if projected.length() > 0.5 {
            projected.normalized()
        } else {
            Vec2::new(0.7, -0.7)
        };
        // Keep the operation handle outside the transform handles' ring. Both
        // remain available after applying, without competing for egui hit testing.
        let endpoint = origin + direction * 104.;
        let color = Color32::from_rgb(112, 239, 213);
        let painter = ui.painter_at(rect);
        painter.line_segment([origin, endpoint], egui::Stroke::new(2., color));
        painter.circle_filled(endpoint, 6., color);
        painter.text(
            endpoint + Vec2::new(8., -7.),
            egui::Align2::LEFT_BOTTOM,
            p.title(),
            egui::FontId::proportional(12.),
            color,
        );
        let handle = Rect::from_center_size(endpoint, Vec2::splat(22.));
        let response=ui.interact(handle,ui.id().with("direct_topology_handle"),Sense::drag())
            .on_hover_text("Arraste para ajustar; solte para aplicar. Esc cancela o gesto. Alt suspende o encaixe.");
        let pressed = ui.input(|i| !i.modifiers.ctrl && i.pointer.primary_pressed());
        if pressed
            && response.contains_pointer()
            && let Some(pointer) = ui
                .input(|i| i.pointer.interact_pos())
                .filter(|p| handle.contains(*p))
        {
            let (_, view) = self.camera.ray(
                [rect.width() as u32, rect.height() as u32],
                [(origin.x - rect.min.x), (origin.y - rect.min.y)],
            );
            let normal = (view - axis * view.dot(axis)).normalize_or_zero();
            let parallel = projected.length() < 0.5 || normal.length_squared() < 0.5;
            let normal = if parallel { view } else { normal };
            let measure_axis = if parallel {
                super::viewport::plane(&self.camera, rect, origin + direction * 60., center, normal)
                    .map_or(axis, |point| (point - center).normalize_or_zero())
            } else {
                axis
            };
            if let Some(hit) = super::viewport::plane(&self.camera, rect, pointer, center, normal) {
                if from_last {
                    let last = self.modeling.last_operation.as_ref().unwrap();
                    let mut candidate = last.source.clone();
                    candidate.amendment = Some(last.command);
                    candidate.previous = [f32::NAN; 3];
                    candidate.rollback_selection = Some(self.modeling.selection.clone());
                    self.modeling.preview = Some(candidate);
                }
                let p = self.modeling.preview.as_mut().unwrap();
                p.drag = Some(Drag {
                    axis: None,
                    start: pointer,
                    direction,
                    base: if p.operation == Operation::Inset {
                        [p.inner_size, 0., 0.]
                    } else {
                        p.values
                    },
                    plane_origin: center,
                    plane_normal: normal,
                    world_axis: measure_axis,
                    start_world: hit,
                });
            }
        }
        let mut moved = false;
        let mut release = false;
        if let Some(p) = self.modeling.preview.as_mut()
            && let Some(drag) = &p.drag
        {
            let before = (p.values, p.inner_size);
            if let Some(pointer) = ui.input(|i| i.pointer.latest_pos()) {
                if p.operation == Operation::Inset {
                    p.inner_size = (drag.base[0]
                        - (pointer - drag.start).dot(drag.direction) * 0.5)
                        .clamp(0.01, 100.);
                } else if let Some(hit) = super::viewport::plane(
                    &self.camera,
                    rect,
                    pointer,
                    drag.plane_origin,
                    drag.plane_normal,
                ) {
                    let mut distance = (hit - drag.start_world).dot(drag.world_axis);
                    if !p.global || p.operation == Operation::Bevel {
                        distance /= p
                            .world
                            .transform_vector3(
                                p.world
                                    .inverse()
                                    .transform_vector3(axis)
                                    .normalize_or_zero(),
                            )
                            .length()
                            .max(1e-8);
                    }
                    if self.snap_grid && !ui.input(|i| i.modifiers.alt) {
                        distance = (distance / self.grid_size).round() * self.grid_size;
                    }
                    if p.operation == Operation::Extrude && p.direction == Direction::Vector {
                        p.values = vector_drag_values(p.world, p.global, drag.base, axis, distance);
                    } else {
                        p.values[0] = if p.operation == Operation::Bevel {
                            (drag.base[0] + distance).max(0.)
                        } else {
                            drag.base[0] + distance
                        };
                    }
                }
                moved = before != (p.values, p.inner_size);
                if moved {
                    p.previous = [f32::NAN; 3];
                }
            }
            release = !ui.input(|i| i.pointer.primary_down());
            if release {
                p.drag = None;
            }
        }
        if moved {
            self.update_mesh_preview();
        }
        if release {
            self.confirm_mesh_operation();
        }
        moved
            || release
            || self
                .modeling
                .preview
                .as_ref()
                .is_some_and(|p| p.drag.is_some())
    }
}

#[derive(Default)]
struct Edit {
    changed: bool,
    ended: bool,
    invalid: bool,
}
/// UI values have no mesh, pixels, graph, component arrays or gesture ownership.
#[derive(Clone, Copy, PartialEq)]
struct Controls {
    values: [f32; 3],
    inner_size: f32,
    count: u32,
    per_face: bool,
    direction: Direction,
    allow_expansion: bool,
}
impl From<&Preview> for Controls {
    fn from(p: &Preview) -> Self {
        Self {
            values: p.values,
            inner_size: p.inner_size,
            count: p.count,
            per_face: p.per_face,
            direction: p.direction,
            allow_expansion: p.allow_expansion,
        }
    }
}
impl Controls {
    fn apply(self, p: &mut Preview) {
        p.values = self.values;
        p.inner_size = self.inner_size;
        p.count = self.count;
        p.per_face = self.per_face;
        p.direction = self.direction;
        p.allow_expansion = self.allow_expansion;
    }
}
fn vector_direction(world: Mat4, global: bool, axis: Vec3) -> Vec3 {
    if global {
        axis
    } else {
        world.inverse().transform_vector3(axis).normalize_or_zero()
    }
}
fn vector_drag_values(
    world: Mat4,
    global: bool,
    base: [f32; 3],
    axis: Vec3,
    distance: f32,
) -> [f32; 3] {
    (Vec3::from(base) + vector_direction(world, global, axis) * distance).to_array()
}
impl Edit {
    fn numeric(&mut self, response: egui::Response) {
        self.changed |= response.changed();
        self.ended |= response.drag_stopped() || response.lost_focus();
    }
    fn option(&mut self, response: egui::Response, applied: bool) {
        if response.changed() {
            self.changed |= applied;
            self.ended |= applied;
        }
    }
}
fn parameters(
    ui: &mut egui::Ui,
    p: &Preview,
    controls: &mut Controls,
    edit: &mut Edit,
    adjusting: bool,
) {
    let applied = p.started || p.amendment.is_some() || adjusting;
    let mut invalid = false;
    match p.operation {
        Operation::Transform(tool) => {
            ui.horizontal_wrapped(|ui| {
                for (axis, label) in ["X", "Y", "Z"].into_iter().enumerate() {
                    edit.numeric(super::numeric::float(
                        ui,
                        label,
                        &mut controls.values[axis],
                        if tool == Gizmo::Rotate { 1. } else { 0.02 },
                        None,
                        &mut invalid,
                    ));
                }
            });
        }
        Operation::Extrude | Operation::Inset => {
            if p.selection.mode == Mode::Face {
                ui.horizontal(|ui|{
                    ui.label("Tamanho interno").on_hover_text("Proporção linear: 50% reduz cada dimensão pela metade. A moldura permanece no plano original. Use maior que 0% até 100%.");
                    edit.numeric(super::numeric::float(ui,"",&mut controls.inner_size,0.5,Some(0.001..=100.),&mut invalid)); ui.label("%");
                });
                edit.option(
                    ui.checkbox(&mut controls.per_face, "Cada face independente"),
                    applied,
                );
            }
            if p.operation == Operation::Extrude {
                if controls.direction != Direction::Vector {
                    ui.horizontal(|ui| {
                        ui.label("Distância");
                        edit.numeric(super::numeric::float(
                            ui,
                            "",
                            &mut controls.values[0],
                            0.02,
                            None,
                            &mut invalid,
                        ));
                    });
                }
                egui::ComboBox::from_id_salt("extrude_direction")
                    .selected_text(match controls.direction {
                        Direction::Normal => "Normal da face",
                        Direction::X => "Eixo X",
                        Direction::Y => "Eixo Y",
                        Direction::Z => "Eixo Z",
                        Direction::Vector => "Vetor (avançado)",
                    })
                    .show_ui(ui, |ui| {
                        for (value, label) in [
                            (Direction::Normal, "Normal da face"),
                            (Direction::X, "Eixo X"),
                            (Direction::Y, "Eixo Y"),
                            (Direction::Z, "Eixo Z"),
                            (Direction::Vector, "Vetor (avançado)"),
                        ] {
                            edit.option(
                                ui.selectable_value(&mut controls.direction, value, label),
                                applied,
                            );
                        }
                    });
                if controls.direction == Direction::Vector {
                    for (axis, label) in ["X", "Y", "Z"].into_iter().enumerate() {
                        edit.numeric(super::numeric::float(
                            ui,
                            label,
                            &mut controls.values[axis],
                            0.02,
                            None,
                            &mut invalid,
                        ));
                    }
                }
                ui.small(if p.global {
                    "Coordenadas globais"
                } else {
                    "Coordenadas locais"
                });
            }
        }
        Operation::Bevel => {
            ui.horizontal(|ui| {
                ui.label("Largura");
                edit.numeric(super::numeric::float(
                    ui,
                    "",
                    &mut controls.values[0],
                    0.01,
                    Some(0. ..=100.),
                    &mut invalid,
                ));
            });
            ui.horizontal(|ui| {
                ui.label("Segmentos");
                let r = super::numeric::integer(ui, "", &mut controls.count, 1..=16, &mut invalid);
                if applied {
                    edit.numeric(r);
                }
            });
        }
        Operation::Loop => {
            ui.horizontal(|ui| {
                ui.label("Quantidade de cortes");
                let r = super::numeric::integer(ui, "", &mut controls.count, 1..=64, &mut invalid);
                if applied {
                    edit.numeric(r);
                }
            });
            if applied {
                ui.horizontal(|ui| {
                    ui.label("Deslizamento");
                    edit.numeric(super::numeric::float(
                        ui,
                        "",
                        &mut controls.values[1],
                        0.01,
                        Some(-1. ..=1.),
                        &mut invalid,
                    ));
                });
            }
            if !p.started {
                ui.small("Clique na aresta apontada para inserir; arraste para deslizar.");
            }
        }
        Operation::Knife => {
            ui.small("Clique nas bordas para traçar. Duplo clique ou Enter aplica o percurso uma única vez.");
        }
        _ => (),
    }
    edit.invalid = invalid;
}

#[cfg(test)]
mod tests {
    use super::*;
    fn preview() -> Preview {
        let source = primitives::generate(Primitive::Cube, 8, Default::default()).unwrap();
        let mut original = Entity::new("Fonte", None);
        original.mesh = Some(source.clone());
        Preview {
            entity: original.id.clone(),
            original,
            source: source.clone(),
            selection: Components {
                mode: Mode::Face,
                ids: vec![source.data().faces[0].id],
                through: false,
            },
            operation: Operation::Extrude,
            values: [0.3, 0., 0.],
            previous: [f32::NAN; 3],
            world: Mat4::from_scale_rotation_translation(
                Vec3::new(-2., 3., 0.75),
                glam::Quat::from_rotation_z(0.7),
                Vec3::new(4., -2., 1.),
            ),
            center: Vec3::ZERO,
            global: false,
            keep_edges: true,
            error: None,
            drag: None,
            per_face: false,
            texture: None,
            expanded_texture: None,
            allow_expansion: false,
            needs_space: false,
            count: 1,
            cut_edge: None,
            path: Default::default(),
            notes: vec![],
            started: false,
            numeric_edit: false,
            amendment: None,
            rollback_selection: None,
            inner_size: 70.,
            direction: Direction::Normal,
        }
    }
    #[test]
    fn borrowed_displacement_keeps_source_and_uses_inverse_transpose_normal() {
        let mut p = preview();
        p.global = true;
        let normal = p.source.prepared().face_normals[0];
        let expected = p
            .world
            .inverse()
            .transpose()
            .transform_vector3(normal)
            .normalize();
        assert!(p.extrusion_axis().abs_diff_eq(expected, 1e-5));
        assert!(
            p.world
                .transform_vector3(p.displacement(None))
                .abs_diff_eq(expected * 0.3, 1e-5)
        );
        assert_eq!(p.values, [0.3, 0., 0.]);
        assert_eq!(
            p.original.mesh.as_ref().unwrap().revision(),
            p.source.revision()
        );
    }
    #[test]
    fn vector_handle_updates_all_coordinates_in_local_and_global_space() {
        let mut p = preview();
        p.direction = Direction::Vector;
        p.values = [1., 2., 3.];
        for global in [false, true] {
            p.global = global;
            let axis = p.extrusion_axis();
            let next = vector_drag_values(p.world, global, p.values, axis, 0.5);
            let expected = Vec3::from(p.values) + Vec3::from(p.values).normalize() * 0.5;
            assert!(Vec3::from(next).abs_diff_eq(expected, 1e-5));
            assert!(next.into_iter().zip(p.values).all(|(a, b)| a != b));
            let local = p.displacement(None);
            let expected_world = if global {
                Vec3::from(p.values)
            } else {
                p.world.transform_vector3(Vec3::from(p.values))
            };
            assert!(
                p.world
                    .transform_vector3(local)
                    .abs_diff_eq(expected_world, 1e-5)
            );
        }
    }
    #[test]
    fn vector_handle_keeps_press_axis_when_drag_crosses_zero() {
        let mut p = preview();
        p.direction = Direction::Vector;
        p.values = [1., 2., 3.];
        let axis = p.extrusion_axis();
        p.drag = Some(Drag {
            axis: None,
            start: Pos2::ZERO,
            direction: Vec2::X,
            base: p.values,
            plane_origin: Vec3::ZERO,
            plane_normal: Vec3::Z,
            world_axis: axis,
            start_world: Vec3::ZERO,
        });
        p.values = [-1., -2., -3.];
        assert!(p.extrusion_axis().abs_diff_eq(axis, 1e-5));
        p.drag = None;
        p.values = [0.; 3];
        p.global = true;
        assert_eq!(p.extrusion_axis(), Vec3::X);
    }
    #[test]
    fn drawing_controls_does_not_copy_or_change_preview_payload() {
        fn copy_type<T: Copy>(_: &T) {}
        let p = preview();
        let original = p.original.clone();
        let initial = Controls::from(&p);
        copy_type(&initial);
        assert!(std::mem::size_of::<Controls>() <= 32);
        let mut controls = initial;
        let ctx = egui::Context::default();
        for _ in 0..3 {
            let _ = ctx.run(Default::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let mut edit = Edit::default();
                    parameters(ui, &p, &mut controls, &mut edit, true);
                    assert!(!edit.changed);
                });
            });
        }
        assert!(controls == initial);
        assert_eq!(p.original, original);
        let mut candidate = p.clone();
        controls.values = [0.7, 0., 0.];
        controls.apply(&mut candidate);
        assert_eq!(candidate.values, [0.7, 0., 0.]);
        assert_eq!(candidate.source.revision(), p.source.revision());
        assert_eq!(candidate.selection, p.selection);
        assert_eq!(candidate.original, original);
    }
}
