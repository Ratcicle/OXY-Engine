use super::*;
use oxy_render::collider_debug::{project, segment_distance};

#[derive(Default)]
pub(super) struct BodyTools {
    pub active: Option<Id>,
    drag: Option<BodyDrag>,
}
struct BodyDrag {
    id: Id,
    scene: Id,
    original: [f32; 3],
    axis_index: usize,
    axis: Vec3,
    units: f32,
    origin: Vec3,
    start: Vec3,
    normal: Vec3,
}
fn frame(scene: &Scene, id: &str) -> Result<(Vec3, [Vec3; 3], f32), String> {
    let entity = scene.entity(id).ok_or("Personagem ausente")?;
    let config = entity
        .character3d
        .as_ref()
        .ok_or("Selecione um Personagem 3D")?;
    let matrix = scene.world_matrix(id)?;
    let (_, rotation, scale) = oxy_core::physics3d::world_pose(matrix)?;
    oxy_core::character::character_scale(rotation, scale)?;
    let collider = config.body.collider(false, config.crouch_height)?;
    Ok((
        matrix.transform_point3(Vec3::from(collider.center)),
        [rotation * Vec3::X, rotation * Vec3::Y, rotation * Vec3::Z],
        scale.x,
    ))
}
fn dragged_offset(drag: &BodyDrag, at: Vec3, snap: Option<f32>) -> [f32; 3] {
    let mut result = drag.original;
    let mut delta = (at - drag.start).dot(drag.axis);
    if let Some(grid) = snap.filter(|g| g.is_finite() && *g > 0.) {
        delta = (delta / grid).round() * grid;
    }
    result[drag.axis_index] += delta / drag.units;
    result
}
impl Editor {
    #[cfg(test)]
    pub(crate) fn patch_tool_is_move(&self) -> bool {
        self.gizmo == Gizmo::Move && self.spatial.mode == Tool::Object
    }
    #[cfg(test)]
    pub(crate) fn patch_tool_is_rotate(&self) -> bool {
        self.gizmo == Gizmo::Rotate && self.spatial.mode == Tool::Object
    }
    #[cfg(test)]
    pub(crate) fn patch_tool_is_collider(&self) -> bool {
        self.spatial.mode == Tool::Collider
    }
    pub(super) fn body_dragging(&self) -> bool {
        self.body_tools.drag.is_some()
    }
    pub(super) fn cancel_body_drag(&mut self) -> bool {
        if let Some(drag) = self.body_tools.drag.take() {
            if let Some(config) = self
                .state
                .project
                .scene_mut(&drag.scene)
                .and_then(|s| s.entity_mut(&drag.id))
                .and_then(|e| e.character3d.as_mut())
            {
                config.body.offset = drag.original;
            }
            true
        } else {
            false
        }
    }
    pub(super) fn edit_body_position(&mut self, id: &str) {
        if self.tab != Tab::Scene || self.selection.ids.len() != 1 || !self.structural_ready() {
            self.warn("Selecione somente um Personagem 3D na Cena para editar a posição do corpo.");
            return;
        }
        if let Err(error) = frame(self.scene(), id) {
            self.warn(error);
            return;
        }
        if self.mesh_operation_active() || self.gizmo_drag.is_some() {
            return;
        }
        self.set_spatial_tool(Tool::Object);
        self.modeling.selection.mode = oxy_core::geometry::selection::Mode::Object;
        self.body_tools.active = Some(id.into());
    }
    pub(super) fn body_handles(&mut self, ui: &egui::Ui, scene: &Scene, rect: Rect) -> bool {
        let Some(id) = self.body_tools.active.clone() else {
            return false;
        };
        if self.tab != Tab::Scene
            || self.selected.as_deref() != Some(&id)
            || self.selection.ids.len() != 1
        {
            self.cancel_body_drag();
            self.body_tools.active = None;
            return false;
        }
        let Ok((center, axes, units)) = frame(scene, &id) else {
            self.cancel_body_drag();
            self.body_tools.active = None;
            return false;
        };
        let Some(origin) = project(&self.camera, rect, center) else {
            return true;
        };
        let editable = !self.navigation.active
            && !self.capture
            && !crate::graph_ui::text_input_active(ui.ctx())
            && !egui::Popup::is_any_open(ui.ctx())
            && self.pending_preferences.is_none()
            && self.pending.is_none()
            && ui.input(|i| i.focused && !i.pointer.secondary_down());
        if !editable {
            self.cancel_body_drag();
        }
        let pointer = ui
            .input(|i| i.pointer.interact_pos())
            .filter(|p| rect.contains(*p));
        let painter = ui.painter_at(rect);
        let mut hovered = None;
        let mut nearest = 9.;
        for (i, axis) in axes.into_iter().enumerate() {
            let probe = project(&self.camera, rect, center + axis * units);
            let Some(probe) = probe.filter(|p| p.distance(origin) > 2.) else {
                continue;
            };
            let length = units * 65. / probe.distance(origin);
            let Some(end) = project(&self.camera, rect, center + axis * length) else {
                continue;
            };
            let color = [
                Color32::LIGHT_RED,
                Color32::LIGHT_GREEN,
                Color32::LIGHT_BLUE,
            ][i];
            painter.line_segment([origin, end], egui::Stroke::new(4., Color32::BLACK));
            painter.line_segment([origin, end], egui::Stroke::new(2., color));
            painter.circle_filled(end, 5., color);
            painter.text(
                end + Vec2::new(8., -8.),
                egui::Align2::LEFT_BOTTOM,
                ["X", "Y", "Z"][i],
                egui::FontId::proportional(13.),
                color,
            );
            if editable
                && let Some(distance) =
                    pointer.map(|p| segment_distance(p, origin + (end - origin) * 0.2, end))
                && distance < nearest
            {
                nearest = distance;
                hovered = Some((i, axis));
            }
        }
        painter.circle_stroke(origin, 6., egui::Stroke::new(2., Color32::WHITE));
        painter.text(
            origin + Vec2::new(12., 20.),
            egui::Align2::LEFT_TOP,
            "Posição do corpo · Esc cancela/sai",
            egui::FontId::proportional(12.),
            Color32::WHITE,
        );
        if self.body_tools.drag.is_none()
            && ui.input(|i| i.pointer.primary_pressed())
            && let (Some((axis_index, axis)), Some(pointer)) = (hovered, pointer)
        {
            let view = self.camera.forward();
            let normal = (view - axis * view.dot(axis)).normalize_or_zero();
            if let Some(start) =
                spatial_tools::ray_plane(&self.camera, rect, pointer, center, normal)
            {
                self.body_tools.drag = Some(BodyDrag {
                    id: id.clone(),
                    scene: self.scene_id.clone(),
                    original: scene
                        .entity(&id)
                        .unwrap()
                        .character3d
                        .as_ref()
                        .unwrap()
                        .body
                        .offset,
                    axis_index,
                    axis,
                    units,
                    origin: center,
                    start,
                    normal,
                });
            }
        }
        if let Some(drag) = &self.body_tools.drag
            && let Some(pointer) = ui.input(|i| i.pointer.interact_pos())
            && let Some(at) =
                spatial_tools::ray_plane(&self.camera, rect, pointer, drag.origin, drag.normal)
        {
            let snap = (self.snap_grid && !ui.input(|i| i.modifiers.alt)).then_some(self.grid_size);
            let offset = dragged_offset(drag, at, snap);
            if Vec3::from(offset).is_finite() {
                self.scene_mut()
                    .entity_mut(&id)
                    .unwrap()
                    .character3d
                    .as_mut()
                    .unwrap()
                    .body
                    .offset = offset;
            }
        }
        if ui.input(|i| i.pointer.primary_released()) {
            self.body_tools.drag = None;
        }
        // This explicit tool owns its viewport while active: no simultaneous object transform.
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_axes_and_offset_deltas_keep_parent_child_transforms_and_undo() {
        let mut p = Project::new("Corpo");
        p.scenes[0].kind = SceneKind::ThreeD;
        let mut parent = Entity::new("Pai", None);
        parent.transform.scale = [2.; 3];
        parent.transform.rotation = [0., 0.7, 0.];
        let mut actor = Entity::new("Personagem", None);
        actor.parent = Some(parent.id.clone());
        actor.character3d = Some(Default::default());
        let id = actor.id.clone();
        let mut child = Entity::new("Visual", Some(Primitive::Cube));
        child.parent = Some(id.clone());
        child.transform.position = [1., 1., 0.];
        let child_id = child.id.clone();
        p.scenes[0].entities = vec![parent, actor, child];
        let before = p.clone();
        let visual = p.scenes[0].world_matrix(&child_id).unwrap();
        let (center, axes, units) = frame(&p.scenes[0], &id).unwrap();
        let mut history = CommandHistory::default();
        let mut images = TextureCache::default();
        history.begin("Posição do corpo", &p, &images);
        let drag = BodyDrag {
            id: id.clone(),
            scene: p.start_scene.clone(),
            original: [0.; 3],
            axis_index: 2,
            axis: axes[2],
            units,
            origin: center,
            start: center,
            normal: Vec3::Y,
        };
        let offset = dragged_offset(&drag, center + axes[2] * 0.83, Some(0.5));
        assert!((offset[2] - 0.5).abs() < 1e-5);
        p.scenes[0]
            .entity_mut(&id)
            .unwrap()
            .character3d
            .as_mut()
            .unwrap()
            .body
            .offset = offset;
        assert_eq!(p.scenes[0].world_matrix(&child_id).unwrap(), visual);
        assert!((frame(&p.scenes[0], &id).unwrap().0 - center).abs_diff_eq(axes[2], 1e-5));
        history.commit(&p, &mut images).unwrap();
        let edited = p.clone();
        history.undo(&mut p, &mut images).unwrap();
        assert_eq!(p, before);
        history.redo(&mut p, &mut images).unwrap();
        assert_eq!(p, edited);
        p.scenes[0].entity_mut(&id).unwrap().transform.scale = [1., 2., 1.];
        assert!(frame(&p.scenes[0], &id).is_err());
    }
}
