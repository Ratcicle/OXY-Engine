use super::*;

fn frame_camera(camera: &mut CameraState, scene: &Scene, selection: &[Id], size: [u32; 2]) {
    if selection.is_empty() {
        *camera = CameraState::for_scene(scene);
        return;
    }
    let ids: std::collections::HashSet<_> = selection
        .iter()
        .flat_map(|id| scene.descendants(id))
        .collect();
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for entity in scene
        .entities
        .iter()
        .filter(|e| ids.contains(&e.id) && e.primitive.is_some())
    {
        if let Ok(world) = scene.world_matrix(&entity.id) {
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
        let compact_tools = ui.available_width() < 550.;
        if !painting {
            ui.horizontal_wrapped(|ui| {
                for (tool,label) in [(Gizmo::Move,"Mover (W)"),(Gizmo::Rotate,"Girar (E)"),(Gizmo::Scale,"Escalar (R)")] { if ui.selectable_label(self.spatial.mode==Tool::Object&&self.gizmo==tool,label).clicked(){self.set_spatial_tool(Tool::Object);self.gizmo=tool;} }
                if ui.selectable_label(self.spatial.mode==Tool::Collider,"Editar colisor (C)").on_hover_text("Arraste bordas para ajustar a caixa real; o centro desloca apenas o colisor. Alt suspende o encaixe; Esc cancela o gesto ou sai da ferramenta.").clicked(){self.set_spatial_tool(Tool::Collider);}
                if ui.selectable_label(self.spatial.mode==Tool::Pivot,"Editar pivô (P)").on_hover_text("Reposiciona o ponto de giro sem mover a peça e seus filhos. Alt suspende o encaixe; Esc cancela o gesto ou sai da ferramenta.").clicked(){self.set_spatial_tool(Tool::Pivot);}
                if !compact_tools {ui.separator();
                ui.checkbox(&mut self.grid, "Grade");
                ui.add(
                    egui::DragValue::new(&mut self.grid_size)
                        .range(0.01..=10.)
                        .speed(0.01)
                        .prefix("Encaixe "),
                );}
                ui.checkbox(&mut self.debug, "Colisores");
                if self.debug && !compact_tools { ui.checkbox(&mut self.show_disabled_colliders,"Mostrar desativados"); }
                if ui.button("Enquadrar").clicked() {
                    self.frame_selection();
                }
                if self.spatial.mode!=Tool::Object || compact_tools {ui.menu_button(if compact_tools{"Opções"}else{"Valores da ferramenta"},|ui|{
                    if compact_tools {ui.checkbox(&mut self.grid,"Grade");ui.add(egui::DragValue::new(&mut self.grid_size).range(0.01..=10.).speed(0.01).prefix("Encaixe "));ui.checkbox(&mut self.show_disabled_colliders,"Mostrar desativados");ui.separator();}
                    if let Some(id)=self.selected.clone() && let Some(entity)=self.scene().entity(&id).cloned() {
                        if self.spatial.mode==Tool::Pivot {
                            let mut pivot=entity.transform.pivot;vector3(ui,"Pivô — ponto de giro",&mut pivot,0.05,false);
                            if pivot!=entity.transform.pivot && let Err(error)=oxy_core::spatial::move_pivot(self.scene_mut(),&id,Vec3::from(pivot)){self.log(error);self.console=true;}
                        }else if self.spatial.mode==Tool::Collider && let Some(mut collider)=entity.collider.clone(){
                            vector3(ui,"Tamanho da caixa",&mut collider.size,0.05,true);vector3(ui,"Deslocamento",&mut collider.offset,0.05,false);
                            if Some(&collider)!=entity.collider.as_ref(){let mut candidate=self.scene().clone();candidate.entity_mut(&id).unwrap().collider=Some(collider);match oxy_core::spatial::collider_bounds(&candidate,&id){Ok(_)=>*self.scene_mut()=candidate,Err(e)=>self.log(e)}}
                        }
                    }
                });}
            });
            if self.spatial.mode == Tool::Pivot {
                ui.horizontal_wrapped(|ui| {
                    ui.label("Mover pivô sem mover a peça");
                    if ui.button("Centralizar na peça").clicked() {
                        self.center_pivot(false);
                    }
                    if ui.button("Centralizar no conjunto").clicked() {
                        self.center_pivot(true);
                    }
                });
            }
            if self.spatial.base_pose {
                ui.colored_label(
                    Color32::LIGHT_YELLOW,
                    "Editando a pose-base · W/E/R ou Esc retorna à animação",
                );
            }
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
        if response.hovered() && !self.spatial_active_drag() {
            self.camera.zoom(ui.input(|i| i.smooth_scroll_delta.y));
        }
        if response.dragged_by(egui::PointerButton::Middle) && !self.spatial_active_drag() {
            let d = ui.input(|i| i.pointer.delta());
            self.camera.pan([d.x, d.y], size);
        }
        if self.scene().kind == SceneKind::ThreeD
            && response.dragged_by(egui::PointerButton::Secondary)
            && !self.spatial_active_drag()
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
        let point = ui
            .input(|i| i.pointer.interact_pos())
            .filter(|p| rect.contains(*p));
        let pick = point.and_then(|p| {
            oxy_render::pick(
                &scene,
                &self.camera,
                size,
                [p.x - rect.min.x, p.y - rect.min.y],
            )
        });
        let ui_pick = point.and_then(|p| oxy_render::pick_ui(&scene, rect, p));
        let overlays = self.renderer.draw_colliders(
            ui,
            &scene,
            &self.camera,
            rect,
            &self.selection.ids,
            self.show_disabled_colliders,
        );
        let contour_pick = point.and_then(|p| overlays.pick(p, &self.selection.ids));
        let handle_owned = !painting && self.spatial_handles(ui, &scene, rect);
        if painting {
            if (response.dragged_by(egui::PointerButton::Primary) || response.clicked())
                && let Some(hit) = pick.as_ref()
            {
                if self.selected.as_ref() == Some(&hit.entity) {
                    self.paint_at_uv(hit.uv);
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
            if self.spatial.mode == Tool::Object {
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
        let hint = if rect.width() < 550. {
            if self.spatial.mode != Tool::Object {
                "Alt: sem encaixe · Esc: cancelar"
            } else {
                "Pan: central · Zoom: roda"
            }
        } else if !painting && self.spatial.mode != Tool::Object {
            "Alças: arrastar · centro: plano da câmera · Alt: sem encaixe · Esc: cancelar"
        } else if painting {
            "Pincel: esquerdo · órbita: direito · pan: central · zoom: roda"
        } else if self.scene().kind == SceneKind::TwoD {
            "Selecionar: esquerdo · pan: central · zoom: roda · arraste os eixos para transformar"
        } else {
            "Selecionar: esquerdo · órbita: direito · pan: central · zoom: roda"
        };
        if rect.height() > 240. {
            ui.painter().text(
                rect.left_bottom() + Vec2::new(12., -14.),
                egui::Align2::LEFT_BOTTOM,
                hint,
                egui::FontId::proportional(12.),
                Color32::from_gray(170),
            );
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
        ] {
            if self.scene().kind == SceneKind::TwoD && axis == 2 && self.gizmo != Gizmo::Rotate {
                continue;
            }
            let vector = [Vec3::X, Vec3::Y, Vec3::Z][axis];
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
            let direction = if projected.length() > 0.1 {
                projected.normalized()
            } else {
                Vec2::new(0.7, -0.7)
            };
            let endpoint = origin + direction * (70. + axis as f32 * 8.);
            let painter = ui.painter_at(rect);
            painter.line_segment([origin, endpoint], egui::Stroke::new(2., color));
            painter.circle_filled(endpoint, 6., color);
            painter.text(
                endpoint + Vec2::new(8., -8.),
                egui::Align2::LEFT_BOTTOM,
                ["X", "Y", "Z"][axis],
                egui::FontId::proportional(12.),
                color,
            );
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
                            if self.grid && !ui.input(|i| i.modifiers.alt) {
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
                            glam::Mat4::from_translation(drag.center)
                                * glam::Mat4::from_scale(Vec3::splat((amount * 0.01).exp()))
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
                        if self.grid && !ui.input(|i| i.modifiers.alt) {
                            next.position[axis] =
                                (next.position[axis] / self.grid_size).round() * self.grid_size;
                        }
                    }
                    Gizmo::Rotate => {
                        next.rotation[axis] += amount * 0.012;
                    }
                    Gizmo::Scale => {
                        next.scale[axis] = drag.base.scale[axis] * (amount * 0.01).exp();
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
        let painter = ui.painter_at(rect);
        let ids: std::collections::HashSet<_> = self
            .selection
            .ids
            .iter()
            .flat_map(|id| scene.descendants(id))
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
            if entity.primitive.is_none() || self.selected.as_ref() == Some(&entity.id) {
                continue;
            }
            let Ok(world) = scene.world_matrix(&entity.id) else {
                continue;
            };
            let half = Vec3::from(entity.dimensions) * 0.5;
            let points: Vec<_> = (0..8)
                .map(|i| {
                    let local = Vec3::new(
                        if i & 1 == 0 { -half.x } else { half.x },
                        if i & 2 == 0 { -half.y } else { half.y },
                        if i & 4 == 0 { -half.z } else { half.z },
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
