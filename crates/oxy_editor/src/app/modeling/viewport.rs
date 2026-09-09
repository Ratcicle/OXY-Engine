use super::*;
use oxy_render::{
    ScenePicker,
    collider_debug::{project, segment_distance},
};
mod interaction;

pub(super) fn plane(
    camera: &CameraState,
    rect: Rect,
    p: Pos2,
    origin: Vec3,
    normal: Vec3,
) -> Option<Vec3> {
    let (a, d) = camera.ray(
        [rect.width().max(1.) as u32, rect.height().max(1.) as u32],
        [p.x - rect.min.x, p.y - rect.min.y],
    );
    let denominator = d.dot(normal);
    if denominator.abs() < 1e-6 {
        return None;
    }
    let t = (origin - a).dot(normal) / denominator;
    (t >= 0.).then_some(a + d * t)
}
impl Editor {
    pub(crate) fn mesh_viewport(
        &mut self,
        ui: &mut egui::Ui,
        scene: &Scene,
        rect: Rect,
        response: &egui::Response,
    ) -> bool {
        self.mesh_component_interaction(ui, scene, rect, response)
    }
    fn mesh_gizmo(
        &mut self,
        ui: &mut egui::Ui,
        rect: Rect,
        mesh: &EditableMesh,
        world: Mat4,
    ) -> bool {
        if self.direct_topology_gizmo(ui, rect) {
            return true;
        }
        let ids = self.modeling.selection.vertices(mesh);
        if ids.is_empty() {
            return false;
        }
        if self
            .modeling
            .preview
            .as_ref()
            .is_some_and(|p| !matches!(p.operation, Operation::Transform(_)))
        {
            return false;
        }
        let center = self.modeling.preview.as_ref().map_or_else(
            || ids.iter().filter_map(|id| mesh.position(*id)).sum::<Vec3>() / ids.len() as f32,
            |p| p.center,
        );
        let center_world = world.transform_point3(center);
        let Some(origin) = project(&self.camera, rect, center_world) else {
            return false;
        };
        let global = self
            .modeling
            .preview
            .as_ref()
            .map_or(self.modeling.global, |p| p.global);
        let tool = self
            .modeling
            .preview
            .as_ref()
            .and_then(|p| {
                if let Operation::Transform(t) = p.operation {
                    Some(t)
                } else {
                    None
                }
            })
            .unwrap_or(self.gizmo);
        let pressed = ui.input(|i| {
            i.pointer
                .primary_pressed()
                .then_some(!i.modifiers.ctrl)
                .filter(|v| *v)
                .is_some()
                .then(|| i.pointer.interact_pos())
                .flatten()
        });
        let mut owned = false;
        let axes = if self.scene().kind == SceneKind::TwoD {
            2
        } else {
            3
        };
        for axis in 0..=axes {
            let uniform = axis == axes;
            if uniform && tool != Gizmo::Scale {
                continue;
            }
            let local_axis = if uniform {
                Vec3::ZERO
            } else {
                [Vec3::X, Vec3::Y, Vec3::Z][axis]
            };
            let world_axis = if global {
                local_axis
            } else {
                world.transform_vector3(local_axis)
            };
            let projected = project(&self.camera, rect, center_world + world_axis)
                .map_or(Vec2::ZERO, |p| p - origin);
            if !uniform && projected.length() < 0.5 {
                continue;
            }
            let direction = if uniform {
                Vec2::new(0.7, -0.7)
            } else {
                projected.normalized()
            };
            let endpoint = if uniform {
                origin
            } else {
                origin + direction * (60. + axis as f32 * 7.)
            };
            let color = if uniform {
                Color32::WHITE
            } else {
                [
                    Color32::LIGHT_RED,
                    Color32::LIGHT_GREEN,
                    Color32::LIGHT_BLUE,
                ][axis]
            };
            ui.painter_at(rect)
                .line_segment([origin, endpoint], egui::Stroke::new(2., color));
            ui.painter_at(rect).rect_filled(
                Rect::from_center_size(endpoint, Vec2::splat(9.)),
                1.,
                color,
            );
            if !uniform {
                ui.painter_at(rect).text(
                    endpoint + Vec2::new(7., -7.),
                    egui::Align2::LEFT_BOTTOM,
                    ["X", "Y", "Z"][axis],
                    egui::FontId::proportional(12.),
                    color,
                );
            }
            let handle = Rect::from_center_size(endpoint, Vec2::splat(20.));
            let response = ui.interact(handle, ui.id().with(("mesh_axis", axis)), Sense::drag());
            let response = response.on_hover_text(if uniform {
                "Escala uniforme. Alt exige escolher uma alça de eixo."
            } else {
                "Arraste neste eixo; soltar aplica. Esc cancela o gesto inteiro."
            });
            if let Some(p) = pressed.filter(|p| handle.contains(*p) && response.contains_pointer())
            {
                owned = true;
                if uniform && ui.input(|i| i.modifiers.alt) {
                    self.warn("Alt em Escalar exige escolher a alça X, Y ou Z.");
                    continue;
                }
                self.begin_mesh_operation(Operation::Transform(tool));
                let (eye, view) = self.camera.ray(
                    [rect.width() as u32, rect.height() as u32],
                    [(origin.x - rect.min.x), (origin.y - rect.min.y)],
                );
                let _ = eye;
                let axis_unit = world_axis.normalize_or_zero();
                let normal = if uniform {
                    view
                } else {
                    (view - axis_unit * view.dot(axis_unit)).normalize_or_zero()
                };
                if let Some(start_world) = plane(&self.camera, rect, p, center_world, normal)
                    && let Some(preview) = &mut self.modeling.preview
                {
                    preview.drag = Some(Drag {
                        axis: (!uniform).then_some(axis),
                        start: p,
                        direction,
                        base: preview.values,
                        plane_origin: center_world,
                        plane_normal: normal,
                        world_axis,
                        start_world,
                    });
                }
            }
        }
        let mut release = false;
        if let Some(preview) = self.modeling.preview.as_mut()
            && let Some(drag) = &preview.drag
            && let Some(p) = ui.input(|i| i.pointer.latest_pos())
        {
            owned = true;
            let amount = (p - drag.start).dot(drag.direction);
            let axis = drag.axis.unwrap_or(0);
            match tool {
                Gizmo::Move => {
                    if let Some(hit) =
                        plane(&self.camera, rect, p, drag.plane_origin, drag.plane_normal)
                    {
                        let mut distance = (hit - drag.start_world).dot(drag.world_axis)
                            / drag.world_axis.length_squared().max(1e-12);
                        if self.snap_grid && !ui.input(|i| i.modifiers.alt) {
                            distance = (distance / self.grid_size).round() * self.grid_size;
                        }
                        preview.values[axis] = drag.base[axis] + distance;
                    }
                }
                Gizmo::Rotate => preview.values[axis] = drag.base[axis] + amount * 0.7,
                Gizmo::Scale => {
                    let factor = (amount * 0.01).exp();
                    if drag.axis.is_none() {
                        if !ui.input(|i| i.modifiers.alt) {
                            preview.values = drag.base.map(|v| v * factor);
                        }
                    } else {
                        preview.values[axis] = drag.base[axis] * factor;
                    }
                }
            }
            if !ui.input(|i| i.pointer.primary_down()) {
                release = true;
                preview.drag = None;
            }
        }
        if owned {
            self.update_mesh_preview();
        }
        if release {
            self.confirm_mesh_operation();
        }
        owned
    }
}
