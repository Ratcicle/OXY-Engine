use super::*;
use oxy_render::{
    ScenePicker,
    collider_debug::{project, segment_distance},
};
use std::collections::HashSet;

fn plane(camera: &CameraState, rect: Rect, p: Pos2, origin: Vec3, normal: Vec3) -> Option<Vec3> {
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
    /// Returns true when components own viewport interaction, including empty-space clicks.
    pub(crate) fn mesh_viewport(
        &mut self,
        ui: &mut egui::Ui,
        scene: &Scene,
        rect: Rect,
        response: &egui::Response,
    ) -> bool {
        if self.snap_viewport(ui, scene, rect, response) {
            return true;
        }
        if !self.components_active() {
            return false;
        }
        let mesh = match self.model_source() {
            Ok(mesh) => mesh,
            Err(e) => {
                ui.painter_at(rect).text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    e,
                    egui::FontId::proportional(14.),
                    Color32::LIGHT_GRAY,
                );
                return true;
            }
        };
        let id = self.selected.clone().unwrap();
        let Ok(world) = scene.world_matrix(&id) else {
            return true;
        };
        let picker = ScenePicker::new(scene);
        let size = [rect.width().max(1.) as u32, rect.height().max(1.) as u32];
        let screen = |p: Vec3| project(&self.camera, rect, world.transform_point3(p));
        let visible = |p: Vec3| {
            if self.modeling.selection.through {
                return true;
            }
            let world_p = world.transform_point3(p);
            let Some(pixel) = screen(p) else {
                return false;
            };
            let (origin, direction) = self
                .camera
                .ray(size, [pixel.x - rect.min.x, pixel.y - rect.min.y]);
            let distance = (world_p - origin).dot(direction);
            let tolerance = 0.0001 * (1. + distance.abs());
            picker
                .ray(origin, direction)
                .is_none_or(|hit| hit.distance >= distance - tolerance)
        };
        let selected: HashSet<_> = self.modeling.selection.ids.iter().copied().collect();
        let painter = ui.painter_at(rect);
        let mut points: Vec<(u32, Pos2, bool)> = Vec::new();
        let pointer = ui
            .input(|i| i.pointer.interact_pos())
            .filter(|p| rect.contains(*p));
        let mut hovered = None;
        let mut nearest = 10f32;
        let projected: std::collections::HashMap<_, _> = mesh
            .data()
            .vertices
            .iter()
            .map(|v| (v.id, screen(Vec3::from(v.position))))
            .collect();
        let mut highlighted = HashSet::new();
        for face in mesh
            .data()
            .faces
            .iter()
            .filter(|_| self.modeling.selection.mode == Mode::Face)
        {
            let center = face
                .corners
                .iter()
                .filter_map(|c| mesh.position(c.vertex))
                .sum::<Vec3>()
                / face.corners.len() as f32;
            let front = visible(center);
            if self.modeling.selection.mode == Mode::Face {
                if let Some(p) = screen(center) {
                    points.push((face.id, p, front));
                }
                if selected.contains(&face.id) && front {
                    highlighted.insert(face.id);
                }
            }
        }
        for triangle in mesh
            .prepared()
            .triangles
            .iter()
            .filter(|t| highlighted.contains(&t.face))
        {
            let face = mesh.face(triangle.face).unwrap();
            let positions: Option<Vec<_>> = triangle
                .corners
                .iter()
                .map(|i| projected[&face.corners[*i].vertex])
                .collect();
            if let Some(positions) = positions {
                painter.add(egui::Shape::convex_polygon(
                    positions,
                    Color32::from_rgba_unmultiplied(245, 175, 70, 75),
                    egui::Stroke::NONE,
                ));
            }
        }
        for edge in &mesh.data().edges {
            let [Some(a), Some(b)] = edge.vertices.map(|id| projected[&id]) else {
                continue;
            };
            let midpoint = (mesh.position(edge.vertices[0]).unwrap()
                + mesh.position(edge.vertices[1]).unwrap())
                * 0.5;
            let front = visible(midpoint);
            let active = self.modeling.selection.mode == Mode::Edge && selected.contains(&edge.id);
            let color = if active {
                Color32::GOLD
            } else {
                Color32::from_gray(if front { 155 } else { 55 })
            };
            if front || self.modeling.selection.through {
                painter.line_segment(
                    [a, b],
                    egui::Stroke::new(if active { 2.4 } else { 1. }, color),
                );
            }
            if self.modeling.selection.mode == Mode::Edge {
                points.push((edge.id, a + (b - a) * 0.5, front));
                if front && let Some(p) = pointer {
                    let d = segment_distance(p, a, b);
                    if d < nearest {
                        nearest = d;
                        hovered = Some(edge.id);
                    }
                }
            }
        }
        if self.modeling.selection.mode == Mode::Vertex {
            for vertex in &mesh.data().vertices {
                let Some(p) = projected[&vertex.id] else {
                    continue;
                };
                let front = visible(Vec3::from(vertex.position));
                points.push((vertex.id, p, front));
                if front {
                    painter.circle_filled(
                        p,
                        if selected.contains(&vertex.id) {
                            4.2
                        } else {
                            2.8
                        },
                        if selected.contains(&vertex.id) {
                            Color32::GOLD
                        } else {
                            Color32::LIGHT_GRAY
                        },
                    );
                    if let Some(pointer) = pointer {
                        let distance = pointer.distance(p);
                        if distance < nearest {
                            nearest = distance;
                            hovered = Some(vertex.id);
                        }
                    }
                }
            }
        }
        if self.modeling.selection.mode == Mode::Face
            && let Some(p) = pointer
        {
            let (origin, direction) = self.camera.ray(size, [p.x - rect.min.x, p.y - rect.min.y]);
            let inverse = world.inverse();
            if let Some((ti, d, _)) = mesh.prepared().acceleration.hit(
                inverse.transform_point3(origin),
                inverse.transform_vector3(direction),
                |i| mesh.triangle_points(&mesh.prepared().triangles[i]),
            ) && (self.modeling.selection.through
                || picker
                    .ray(origin, direction)
                    .is_none_or(|hit| hit.distance + 0.0001 * (1. + d.abs()) >= d))
            {
                hovered = Some(mesh.prepared().triangles[ti].face);
            }
        }
        if self.mesh_gizmo(ui, rect, &mesh, world) {
            return true;
        }
        if self.modeling.preview.is_some() {
            return true;
        }
        if response.drag_started_by(egui::PointerButton::Primary) {
            self.modeling.box_start = ui.input(|i| i.pointer.press_origin());
        }
        if let Some(start) = self.modeling.box_start
            && let Some(end) = ui.input(|i| i.pointer.latest_pos())
        {
            let bounds = Rect::from_two_pos(start, end).intersect(rect);
            painter.rect(
                bounds,
                0.,
                Color32::from_rgba_unmultiplied(82, 177, 185, 25),
                egui::Stroke::new(1., Color32::from_rgb(112, 209, 207)),
                egui::StrokeKind::Inside,
            );
            if !ui.input(|i| i.pointer.primary_down()) {
                if !ui.input(|i| i.modifiers.ctrl) {
                    self.modeling.selection.ids.clear();
                }
                for (id, p, front) in points {
                    if front && bounds.contains(p) && !self.modeling.selection.ids.contains(&id) {
                        self.modeling.selection.ids.push(id);
                    }
                }
                self.modeling.box_start = None;
            }
        } else if response.clicked() {
            self.modeling
                .selection
                .click(hovered, ui.input(|i| i.modifiers.ctrl));
        }
        response.context_menu(|ui| {
            self.topology_menu(ui);
            if ui.button("Transformar seleção").clicked() {
                self.begin_mesh_operation(Operation::Transform(self.gizmo));
                ui.close();
            }
            if ui.button("Excluir componentes").clicked() {
                self.begin_mesh_operation(Operation::Delete);
                ui.close();
            }
            if ui.button("Triangular faces").clicked() {
                self.begin_mesh_operation(Operation::Triangulate);
                ui.close();
            }
            if ui.button("Objeto (1)").clicked() {
                self.model_mode(Mode::Object);
                ui.close();
            }
        });
        true
    }
    fn mesh_gizmo(
        &mut self,
        ui: &mut egui::Ui,
        rect: Rect,
        mesh: &EditableMesh,
        world: Mat4,
    ) -> bool {
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
            response.on_hover_text(if uniform {
                "Escala uniforme. Alt exige escolher uma alça de eixo."
            } else {
                "Arraste neste eixo. Esc cancela toda a operação; Enter confirma."
            });
            if let Some(p) = pressed.filter(|p| handle.contains(*p)) {
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
                        if self.grid && !ui.input(|i| i.modifiers.alt) {
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
                preview.drag = None;
            }
        }
        if owned {
            self.update_mesh_preview();
        }
        owned
    }
}
