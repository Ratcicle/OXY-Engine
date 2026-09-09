use super::*;
use oxy_core::scene_view::SceneView;

impl Editor {
    /// Components own the entire primary gesture; Ctrl reserves it before any gizmo.
    pub(super) fn mesh_component_interaction(
        &mut self,
        ui: &mut egui::Ui,
        scene: &Scene,
        rect: Rect,
        response: &egui::Response,
    ) -> bool {
        let primary_on_viewport = ui.input(|i| i.pointer.primary_pressed())
            && ui.input(|i| i.pointer.interact_pos()).is_some_and(|p| {
                rect.contains(p) && ui.ctx().layer_id_at(p) == Some(ui.layer_id())
            });
        // A same-layer gizmo must not hide the initial Ctrl press from selection.
        // Popups/notices live on other layers and continue to own their pointer.
        if self.components_active() && primary_on_viewport && ui.input(|i| i.modifiers.ctrl) {
            self.cancel_mesh_operation();
        }
        if self.snap_viewport(ui, scene, rect, response) {
            return true;
        }
        if self.knife_viewport(ui, scene, rect, response) {
            return true;
        }
        if self.loop_viewport(ui, scene, rect, response) {
            return true;
        }
        if !self.components_active() {
            self.cancel_box_selection();
            self.modeling.projection_cache.clear();
            self.renderer.clear_mesh_components();
            return false;
        }
        if !ui.input(|i| i.focused) || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.cancel_box_selection();
        }
        let mesh = match self.model_source() {
            Ok(mesh) => mesh,
            Err(error) => {
                ui.painter_at(rect).text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    error,
                    egui::FontId::proportional(14.),
                    Color32::LIGHT_GRAY,
                );
                return true;
            }
        };
        let id = self.selected.clone().unwrap();
        let view = SceneView::new(scene);
        let Some(entity) = view.entity(&id) else {
            return true;
        };
        if !view.visible(entity) {
            self.modeling.projection_cache.clear();
            return true;
        }
        let Ok(world) = view.world_matrix(&id) else {
            return true;
        };
        let scale = ui.ctx().pixels_per_point();
        let pointer = ui
            .input(|i| i.pointer.interact_pos())
            .filter(|p| rect.contains(*p));
        let press = primary_on_viewport && pointer.is_some();
        let ctrl_press = press && ui.input(|i| i.modifiers.ctrl);
        let navigating = ui.input(|i| {
            i.pointer.button_down(egui::PointerButton::Middle)
                || i.pointer.button_down(egui::PointerButton::Secondary)
        });
        let hovering = !navigating
            && !self
                .modeling
                .selection_gesture
                .as_ref()
                .is_some_and(|g| g.dragging)
            && (press || !ui.input(|i| i.pointer.primary_down()));
        if hovering && pointer.is_some() || self.modeling.selection_gesture.is_some() {
            self.modeling.projection_cache.prepare(
                &id,
                &mesh,
                world,
                &self.camera,
                rect,
                scale,
                scene,
                &view,
            );
        }
        let mut hovered = None;
        if hovering && let Some(p) = pointer {
            let mode = self.modeling.selection.mode;
            let through = self.modeling.selection.through;
            let (hit, edge) = if let Some(cached) = self
                .modeling
                .projection_cache
                .cached_hover(p, mode, through)
            {
                cached
            } else {
                let projected = self.modeling.projection_cache.projected();
                let size = [
                    (rect.width() * scale).max(1.) as u32,
                    (rect.height() * scale).max(1.) as u32,
                ];
                let inverse = self.camera.matrix(size).inverse();
                let ray = |p: Pos2| {
                    let x = (p.x - rect.min.x) / rect.width() * 2. - 1.;
                    let y = 1. - (p.y - rect.min.y) / rect.height() * 2.;
                    let near = inverse.project_point3(Vec3::new(x, y, 0.));
                    let far = inverse.project_point3(Vec3::new(x, y, 1.));
                    (near, (far - near).normalize_or_zero())
                };
                let picker = (!through).then(|| ScenePicker::from_view(&view));
                let visible = |point: glam::DVec3| {
                    if picker.is_none() {
                        return true;
                    }
                    let (origin, direction) = ray(Pos2::new(point.x as f32, point.y as f32));
                    let ndc = Vec3::new(
                        (point.x as f32 - rect.min.x) / rect.width() * 2. - 1.,
                        1. - (point.y as f32 - rect.min.y) / rect.height() * 2.,
                        point.z as f32,
                    );
                    let world_point = inverse.project_point3(ndc);
                    let distance = (world_point - origin).dot(direction);
                    picker
                        .as_ref()
                        .unwrap()
                        .ray(origin, direction)
                        .is_none_or(|hit| {
                            if scene.kind == SceneKind::TwoD {
                                hit.entity == id
                            } else {
                                hit.distance >= distance - 0.0001 * (1. + distance.abs())
                            }
                        })
                };
                let mut nearest_edge = 10f32;
                let mut edge = None;
                for i in projected.edge_candidates(p, 10.) {
                    let Some([a, b]) = projected.edges[i] else {
                        continue;
                    };
                    let a2 = Pos2::new(a.x as f32, a.y as f32);
                    let b2 = Pos2::new(b.x as f32, b.y as f32);
                    let distance = segment_distance(p, a2, b2);
                    if distance < nearest_edge {
                        let delta = b2 - a2;
                        let t =
                            ((p - a2).dot(delta) / delta.length_sq().max(0.000001)).clamp(0., 1.);
                        if visible(a.lerp(b, t as f64)) {
                            nearest_edge = distance;
                            edge = Some(mesh.data().edges[i].id);
                        }
                    }
                }
                let hit = match mode {
                    Mode::Object => None,
                    Mode::Edge => edge,
                    Mode::Vertex => {
                        let mut nearest = 10f32;
                        let mut found = None;
                        for i in projected.vertex_candidates(p, 10.) {
                            let Some(v) = projected.vertices[i] else {
                                continue;
                            };
                            let distance = p.distance(Pos2::new(v.x as f32, v.y as f32));
                            if distance < nearest && visible(v) {
                                nearest = distance;
                                found = Some(mesh.data().vertices[i].id);
                            }
                        }
                        found
                    }
                    Mode::Face => {
                        let (origin, direction) = ray(p);
                        let local = world.inverse();
                        mesh.prepared()
                            .acceleration
                            .hit(
                                local.transform_point3(origin),
                                local.transform_vector3(direction),
                                |i| mesh.triangle_points(&mesh.prepared().triangles[i]),
                            )
                            .filter(|(_, distance, _)| {
                                picker.as_ref().is_none_or(|picker| {
                                    picker.ray(origin, direction).is_none_or(|hit| {
                                        if scene.kind == SceneKind::TwoD {
                                            hit.entity == id
                                        } else {
                                            hit.distance + 0.0001 * (1. + distance.abs())
                                                >= *distance
                                        }
                                    })
                                })
                            })
                            .map(|(i, _, _)| mesh.prepared().triangles[i].face)
                    }
                };
                self.modeling
                    .projection_cache
                    .remember_hover(p, mode, through, hit, edge);
                (hit, edge)
            };
            hovered = hit;
            if self.modeling.preview.is_none() {
                self.modeling.hovered_edge = edge;
            }
        }
        let reserve_box = ctrl_press || self.modeling.selection_gesture.is_some();
        let gizmo_owned = !reserve_box && self.mesh_gizmo(ui, rect, &mesh, world);
        if !gizmo_owned && press && self.modeling.selection_gesture.is_none() {
            if self.modeling.preview.is_some() {
                self.cancel_mesh_operation();
            }
            self.modeling.selection_gesture = Some(selection::BoxGesture::new(
                pointer.unwrap(),
                self.modeling.selection.clone(),
                ctrl_press,
                hovered,
            ));
        }
        if let Some(mut gesture) = self.modeling.selection_gesture.take() {
            let end = ui
                .input(|i| i.pointer.latest_pos())
                .unwrap_or(gesture.start);
            let bounds = Rect::from_two_pos(gesture.start, end).intersect(rect);
            let recognized =
                gesture.dragging || gesture.start.distance(end) >= selection::BOX_THRESHOLD;
            if recognized {
                let candidates = self.modeling.projection_cache.select(
                    gesture.initial.mode,
                    bounds,
                    gesture.through,
                    &id,
                    scene,
                    &view,
                    &self.camera,
                    rect,
                    scale,
                );
                gesture.update(end, candidates, &mut self.modeling.selection);
            }
            let painter = ui.painter_at(rect);
            painter.rect(
                bounds,
                0.,
                Color32::from_rgba_unmultiplied(82, 177, 185, 25),
                egui::Stroke::new(1., Color32::from_rgb(112, 209, 207)),
                egui::StrokeKind::Inside,
            );
            if gesture.through {
                painter.text(
                    (bounds.min + Vec2::new(6., -6.)).max(rect.min + Vec2::new(4., 16.)),
                    egui::Align2::LEFT_BOTTOM,
                    "Seleção através",
                    egui::FontId::proportional(12.),
                    Color32::from_rgb(112, 209, 207),
                );
            }
            if ui.input(|i| i.pointer.primary_down()) {
                self.modeling.selection_gesture = Some(gesture);
            } else {
                gesture.finish(&mut self.modeling.selection);
            }
        }
        self.renderer.draw_mesh_components(
            &self.render_state,
            &mesh,
            world,
            &self.camera,
            scale,
            &self.modeling.selection,
            self.modeling
                .selection_gesture
                .as_ref()
                .is_some_and(|g| g.through),
        );
        if !gizmo_owned && self.modeling.selection_gesture.is_none() {
            response.context_menu(|ui| {
                self.topology_menu(ui);
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
        }
        true
    }
}
