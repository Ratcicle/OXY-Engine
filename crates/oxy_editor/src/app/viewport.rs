use super::*;

fn frame_camera(camera: &mut CameraState, scene: &Scene, selection: &[Id], size: [u32; 2]) {
    if selection.is_empty() {
        *camera = CameraState::for_scene(scene);
        return;
    }
    let view = oxy_core::scene_view::SceneView::new(scene);
    let ids: std::collections::HashSet<_> = selection
        .iter()
        .flat_map(|id| view.descendants(id))
        .collect();
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for entity in scene
        .entities
        .iter()
        .filter(|e| ids.contains(&e.id) && e.has_geometry())
    {
        if let Ok(world) = view.world_matrix(&entity.id) {
            if let Some(mesh) = &entity.mesh {
                for vertex in &mesh.data().vertices {
                    let p = world.transform_point3(Vec3::from(vertex.position));
                    min = min.min(p);
                    max = max.max(p);
                }
                continue;
            }
            let half = Vec3::from_array(entity.dimensions) * 0.5;
            for x in [-1., 1.] {
                for y in [-1., 1.] {
                    for z in [-1., 1.] {
                        let point = world.transform_point3(half * Vec3::new(x, y, z));
                        min = min.min(point);
                        max = max.max(point);
                    }
                }
            }
        }
    }
    if min.is_finite() && max.is_finite() {
        camera.focus((min + max) * 0.5);
        let aspect = size[0].max(1) as f32 / size[1].max(1) as f32;
        let radius = ((max - min).length() * 0.5).max(0.25);
        let vertical = camera.fov.to_radians() * 0.5;
        let half_angle = vertical.min((vertical.tan() * aspect).atan());
        camera.distance = (radius / half_angle.sin() * 1.25).clamp(0.1, 1000.);
        camera.orthographic_size =
            ((max.y - min.y).max((max.x - min.x) / aspect) * 0.65).clamp(0.5, 1000.);
    } else {
        camera.focus(editing::selection_center(scene, selection));
    }
}

impl Editor {
    pub(crate) fn frame_selection(&mut self) {
        let scene = if self.tab == Tab::Studio && self.studio.tab == StudioTab::Animation {
            self.animation_preview()
        } else {
            self.scene().clone()
        };
        frame_camera(
            &mut self.camera,
            &scene,
            &self.selection.ids,
            self.editor_size,
        );
    }

    pub(super) fn focus_object_click(
        &mut self,
        id: Option<Id>,
        double: bool,
        modifiers: egui::Modifiers,
    ) {
        let plain = !modifiers.ctrl && !modifiers.command && !modifiers.shift;
        if plain && double && id.is_some() && id == self.last_object_click {
            self.frame_selection();
        }
        self.last_object_click = if plain { id } else { None };
    }
    pub fn viewport(&mut self, ui: &mut egui::Ui, painting: bool) {
        if !painting {
            self.viewport_tools_row(ui);
        }
        let scene = if self.tab == Tab::Studio && self.studio.tab == StudioTab::Animation {
            self.animation_preview()
        } else {
            self.scene().clone()
        };
        let (rect, response) = ui.allocate_exact_size(
            ui.available_size().max(Vec2::splat(1.)),
            Sense::click_and_drag(),
        );
        let size = [rect.width().max(1.) as u32, rect.height().max(1.) as u32];
        self.editor_size = size;
        let blocked = self.modeling.creation.is_some() || self.modeling.help;
        if response.hovered() && !self.spatial_active_drag() && !self.camera_dragging() && !blocked
        {
            self.camera.zoom(ui.input(|i| i.smooth_scroll_delta.y));
        }
        if response.dragged_by(egui::PointerButton::Middle)
            && !self.spatial_active_drag()
            && !self.camera_dragging()
            && !blocked
        {
            let d = ui.input(|i| i.pointer.delta());
            self.camera.pan([d.x, d.y], size);
        }
        if self.scene().kind == SceneKind::ThreeD
            && response.dragged_by(egui::PointerButton::Secondary)
            && !self.spatial_active_drag()
            && !self.camera_dragging()
            && !blocked
        {
            let d = ui.input(|i| i.pointer.delta());
            self.camera.orbit([d.x, d.y]);
        }
        let physical = [
            (rect.width() * ui.ctx().pixels_per_point()).max(1.) as u32,
            (rect.height() * ui.ctx().pixels_per_point()).max(1.) as u32,
        ];
        self.renderer.show_grid = self.grid;
        let texture = self.renderer.render(
            &self.render_state,
            &self.state.project,
            &scene,
            &self.root(),
            &self.camera,
            physical,
            self.selected.clone(),
            self.debug,
        );
        ui.painter().image(
            texture,
            rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1., 1.)),
            Color32::WHITE,
        );
        if blocked {
            return;
        }
        let point = ui
            .input(|i| i.pointer.interact_pos())
            .filter(|p| rect.contains(*p));
        let object_point = point.filter(|_| !self.components_active());
        let pick = object_point.and_then(|p| {
            oxy_render::pick(
                &scene,
                &self.camera,
                size,
                [p.x - rect.min.x, p.y - rect.min.y],
            )
        });
        let ui_pick = object_point.and_then(|p| oxy_render::pick_ui(&scene, rect, p));
        let overlays = self.renderer.draw_colliders(
            ui,
            &scene,
            &self.camera,
            rect,
            &self.selection.ids,
            self.show_disabled_colliders,
        );
        let contour_pick = point.and_then(|p| overlays.pick(p, &self.selection.ids));
        let camera_owned = !painting && self.camera_guides(ui, &scene, rect);
        let handle_owned = camera_owned || (!painting && self.spatial_handles(ui, &scene, rect));
        if !painting && !camera_owned && self.mesh_viewport(ui, &scene, rect, &response) {
            return;
        }
        if painting {
            self.draw_paint_faces(ui, rect, &scene);
            if (response.dragged_by(egui::PointerButton::Primary) || response.clicked())
                && let Some(hit) = pick.as_ref()
            {
                if self.selected.as_ref() == Some(&hit.entity) {
                    if self.paint_selecting() {
                        if response.clicked() {
                            self.select_paint_face(
                                hit.face,
                                hit.uv,
                                ui.input(|i| i.modifiers.ctrl),
                            );
                        }
                    } else {
                        self.paint_at_uv(hit.uv);
                    }
                } else if response.clicked() {
                    self.select(Some(hit.entity.clone()));
                }
            }
            if let Some(hit) = &pick
                && self.selected.as_ref() == Some(&hit.entity)
            {
                self.studio.hover_uv = Some(hit.uv);
                if let Some(p) = point {
                    ui.painter()
                        .circle_stroke(p, 8., egui::Stroke::new(1., Color32::WHITE));
                }
            }
        } else {
            if response.clicked() && !handle_owned {
                let id = contour_pick.clone().or(ui_pick
                    .clone()
                    .or_else(|| pick.as_ref().map(|h| h.entity.clone())));
                self.select_click(id.clone(), ui.input(|i| i.modifiers), false);
                self.focus_object_click(id, response.double_clicked(), ui.input(|i| i.modifiers));
            }
            if response.secondary_clicked() {
                self.context_target = contour_pick
                    .or(ui_pick)
                    .or_else(|| pick.as_ref().map(|h| h.entity.clone()));
            }
            response.context_menu(|ui| {
                if let Some(id) = self.context_target.clone() {
                    self.object_context(ui, &id);
                } else {
                    self.creation_menu(ui);
                }
            });
            self.selection_outlines(ui, &scene, rect, size);
            if self.spatial.mode == Tool::Object
                && !handle_owned
                && !self
                    .selected
                    .as_deref()
                    .and_then(|id| scene.entity(id))
                    .and_then(|e| e.camera_rig.as_ref())
                    .is_some_and(|r| r.mode != oxy_core::character::CameraMode::Fixed)
            {
                self.gizmo_ui(ui, rect, size);
            }
            let clicks = self
                .game_ui
                .draw(ui, &self.state.project, &scene, &self.root(), rect);
            if let Some(id) = clicks.first() {
                self.select_click(Some(id.clone()), ui.input(|i| i.modifiers), false);
            }
            if let Some(element) = self
                .selected
                .as_deref()
                .and_then(|id| scene.entity(id))
                .and_then(|e| e.ui.as_ref())
            {
                let outline = oxy_render::game_ui::element_rect(element, rect);
                ui.painter().rect_stroke(
                    outline,
                    0.,
                    egui::Stroke::new(1.5, Color32::GOLD),
                    egui::StrokeKind::Inside,
                );
            }
        }
        for error in overlays.errors {
            if self.messages.last() != Some(&error) {
                self.log(error);
            }
        }
    }
    pub(super) fn gizmo_ui(&mut self, ui: &mut egui::Ui, rect: Rect, size: [u32; 2]) {
        let Some(id) = self.selected.clone() else {
            return;
        };
        let displayed = if self.tab == Tab::Studio && self.studio.tab == StudioTab::Animation {
            self.animation_preview()
        } else {
            self.scene().clone()
        };
        let Some(entity) = displayed.entity(&id).cloned() else {
            return;
        };
        if entity.ui.is_some() {
            return;
        }
        let Ok(world) = displayed.world_matrix(&id) else {
            return;
        };
        let multi = self.selection.ids.len() > 1;
        let pivot = if multi {
            editing::selection_center(&displayed, &self.selection.ids)
        } else {
            world.transform_point3(Vec3::from(entity.transform.pivot))
        };
        let Some(origin) = self.camera.world_to_screen(pivot, size) else {
            return;
        };
        let origin = rect.min + Vec2::from(origin);
        for (axis, color) in [
            (0, Color32::from_rgb(239, 113, 117)),
            (1, Color32::from_rgb(127, 215, 153)),
            (2, Color32::from_rgb(122, 169, 240)),
            (3, Color32::WHITE),
        ] {
            let uniform = axis == 3;
            if uniform && self.gizmo != Gizmo::Scale {
                continue;
            }
            if self.scene().kind == SceneKind::TwoD && axis == 2 && self.gizmo != Gizmo::Rotate {
                continue;
            }
            let vector = if uniform {
                Vec3::ONE
            } else {
                [Vec3::X, Vec3::Y, Vec3::Z][axis]
            };
            let parent = if multi {
                glam::Mat4::IDENTITY
            } else {
                entity
                    .parent
                    .as_deref()
                    .and_then(|p| displayed.world_matrix(p).ok())
                    .unwrap_or(glam::Mat4::IDENTITY)
            };
            let world_axis = parent.transform_vector3(vector);
            let projected = self
                .camera
                .world_to_screen(pivot + world_axis, size)
                .map(|p| rect.min + Vec2::from(p) - origin)
                .unwrap_or(Vec2::ZERO);
            let direction = if !uniform && projected.length() > 0.1 {
                projected.normalized()
            } else {
                Vec2::new(0.7, -0.7)
            };
            let endpoint = if uniform {
                origin
            } else {
                origin + direction * (70. + axis as f32 * 8.)
            };
            let painter = ui.painter_at(rect);
            painter.line_segment([origin, endpoint], egui::Stroke::new(2., color));
            painter.circle_filled(endpoint, 6., color);
            if !uniform {
                painter.text(
                    endpoint + Vec2::new(8., -8.),
                    egui::Align2::LEFT_BOTTOM,
                    ["X", "Y", "Z"][axis],
                    egui::FontId::proportional(12.),
                    color,
                );
            }
            let _response = ui.interact(
                Rect::from_center_size(endpoint, Vec2::splat(20.)),
                ui.id().with(("gizmo", axis)),
                Sense::drag(),
            );
            let pressed = ui.input(|i| {
                i.events.iter().find_map(|event| match event {
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        ..
                    } => Some(*pos),
                    _ => None,
                })
            });
            if let Some(pos) =
                pressed.filter(|p| Rect::from_center_size(endpoint, Vec2::splat(22.)).contains(*p))
            {
                if uniform && ui.input(|i| i.modifiers.alt) {
                    self.warn("Alt em Escalar exige escolher a alça X, Y ou Z.");
                    continue;
                }
                self.gizmo_drag = Some(GizmoDrag {
                    entity: id.clone(),
                    base: entity.transform.clone(),
                    axis,
                    origin: pos,
                    direction,
                    pixels_per_unit: projected.length().max(1.),
                    originals: editing::selection_roots(&displayed, &self.selection.ids)
                        .iter()
                        .filter_map(|id| {
                            displayed
                                .entity(id)
                                .map(|e| (e.id.clone(), e.transform.clone()))
                        })
                        .collect(),
                    center: pivot,
                });
            }
            if let Some(pos) = ui.input(|i| i.pointer.latest_pos())
                && let Some(drag) = self.gizmo_drag.clone()
            {
                if drag.entity != id || drag.axis != axis {
                    continue;
                }
                let mut next = drag.base.clone();
                if uniform && ui.input(|i| i.modifiers.alt) {
                    continue;
                }
                let amount = (pos - drag.origin).dot(drag.direction);
                if drag.originals.len() > 1 {
                    let mut working = displayed.clone();
                    let roots: Vec<_> = drag.originals.iter().map(|(id, _)| id.clone()).collect();
                    for (id, transform) in &drag.originals {
                        if let Some(e) = working.entity_mut(id) {
                            e.transform = transform.clone();
                        }
                    }
                    let delta = match self.gizmo {
                        Gizmo::Move => {
                            let mut distance = amount / drag.pixels_per_unit;
                            if self.snap_grid && !ui.input(|i| i.modifiers.alt) {
                                distance = (distance / self.grid_size).round() * self.grid_size;
                            }
                            glam::Mat4::from_translation(vector * distance)
                        }
                        Gizmo::Rotate => {
                            glam::Mat4::from_translation(drag.center)
                                * glam::Mat4::from_quat(glam::Quat::from_axis_angle(
                                    vector,
                                    amount * 0.012,
                                ))
                                * glam::Mat4::from_translation(-drag.center)
                        }
                        Gizmo::Scale => {
                            let mut factor = Vec3::ONE;
                            if uniform {
                                factor = Vec3::splat((amount * 0.01).exp());
                            } else {
                                factor[axis] = (amount * 0.01).exp();
                            }
                            glam::Mat4::from_translation(drag.center)
                                * glam::Mat4::from_scale(factor)
                                * glam::Mat4::from_translation(-drag.center)
                        }
                    };
                    if let Err(error) = editing::transform_selection(&mut working, &roots, delta) {
                        if self.messages.last() != Some(&error) {
                            self.log(error);
                        }
                    } else {
                        for root in roots {
                            if let Some(e) = working.entity(&root) {
                                self.apply_pose(&root, e.transform.clone());
                            }
                        }
                    }
                    continue;
                }
                match self.gizmo {
                    Gizmo::Move => {
                        next.position[axis] += amount / drag.pixels_per_unit;
                        if self.snap_grid && !ui.input(|i| i.modifiers.alt) {
                            next.position[axis] =
                                (next.position[axis] / self.grid_size).round() * self.grid_size;
                        }
                    }
                    Gizmo::Rotate => {
                        next.rotation[axis] += amount * 0.012;
                    }
                    Gizmo::Scale => {
                        if uniform {
                            if !ui.input(|i| i.modifiers.alt) {
                                next.scale = drag.base.scale.map(|v| v * (amount * 0.01).exp());
                            }
                        } else {
                            next.scale[axis] = drag.base.scale[axis] * (amount * 0.01).exp();
                        }
                    }
                }
                self.apply_pose(&id, next);
            }
        }
        if ui.input(|i| !i.pointer.primary_down()) {
            self.gizmo_drag = None;
        }
    }

    pub(crate) fn apply_pose(&mut self, id: &str, transform: Transform) {
        if self.tab == Tab::Studio && self.studio.tab == StudioTab::Animation {
            self.set_animation_draft_transform(id, transform);
        } else if let Some(entity) = self.scene_mut().entity_mut(id) {
            entity.transform = transform;
        }
    }

    fn selection_outlines(&self, ui: &egui::Ui, scene: &Scene, rect: Rect, size: [u32; 2]) {
        if self.selection.ids.is_empty() {
            return;
        }
        let view = oxy_core::scene_view::SceneView::new(scene);
        let painter = ui.painter_at(rect);
        let ids: std::collections::HashSet<_> = self
            .selection
            .ids
            .iter()
            .flat_map(|id| view.descendants(id))
            .collect();
        for entity in scene
            .entities
            .iter()
            .filter(|e| ids.contains(&e.id) && e.visible)
        {
            if let Some(element) = &entity.ui {
                painter.rect_stroke(
                    oxy_render::game_ui::element_rect(element, rect),
                    0.,
                    egui::Stroke::new(1.5, Color32::GOLD),
                    egui::StrokeKind::Inside,
                );
            }
            if !entity.has_geometry() || self.selected.as_ref() == Some(&entity.id) {
                continue;
            }
            let Ok(world) = view.world_matrix(&entity.id) else {
                continue;
            };
            let half = Vec3::from(entity.dimensions) * 0.5;
            let (min, max) = entity
                .mesh
                .as_ref()
                .and_then(|m| m.prepared().bounds)
                .unwrap_or((-half, half));
            let points: Vec<_> = (0..8)
                .map(|i| {
                    let local = Vec3::new(
                        if i & 1 == 0 { min.x } else { max.x },
                        if i & 2 == 0 { min.y } else { max.y },
                        if i & 4 == 0 { min.z } else { max.z },
                    );
                    self.camera
                        .world_to_screen(world.transform_point3(local), size)
                        .map(|p| rect.min + Vec2::from(p))
                })
                .collect();
            for i in 0..8 {
                for axis in [1, 2, 4] {
                    let j = i ^ axis;
                    if i < j
                        && let (Some(a), Some(b)) = (points[i], points[j])
                    {
                        painter.line_segment(
                            [a, b],
                            egui::Stroke::new(1., Color32::from_rgb(216, 184, 96)),
                        );
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn focus_frames_hierarchy_in_portrait_and_landscape_2d_and_3d() {
        for kind in [SceneKind::TwoD, SceneKind::ThreeD] {
            let mut scene = Scene::new("Enquadramento", kind);
            let group = Entity::new("Articulação", None);
            let root = group.id.clone();
            let mut child = Entity::new(
                "Peça",
                Some(if kind == SceneKind::TwoD {
                    Primitive::Rectangle
                } else {
                    Primitive::Cube
                }),
            );
            child.parent = Some(root.clone());
            child.transform.position = [7., 3., 0.];
            child.dimensions = [6., 2., 1.];
            scene.entities.extend([group, child]);
            for size in [[1000, 500], [300, 900]] {
                let mut camera = CameraState::for_scene(&scene);
                frame_camera(&mut camera, &scene, std::slice::from_ref(&root), size);
                assert!(camera.target.abs_diff_eq(Vec3::new(7., 3., 0.), 1e-5));
                for point in [Vec3::new(4., 2., 0.), Vec3::new(10., 4., 0.)] {
                    let screen = camera.world_to_screen(point, size).unwrap();
                    assert!(
                        screen[0] >= 0.
                            && screen[0] <= size[0] as f32
                            && screen[1] >= 0.
                            && screen[1] <= size[1] as f32
                    );
                }
            }
        }
    }
}
