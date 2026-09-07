use super::*;
use oxy_core::{collision::Aabb, spatial};
use oxy_render::collider_debug::{self, project};

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ray_plane_drag_tracks_camera_pan_zoom_resizing_and_logical_dpi() {
        for kind in [SceneKind::TwoD, SceneKind::ThreeD] {
            let scene = Scene::new("Câmera", kind);
            for size in [
                Vec2::new(900., 500.),
                Vec2::new(350., 240.),
                Vec2::new(400., 300.) / 1.2,
            ] {
                let rect = Rect::from_min_size(Pos2::new(228., 154.), size);
                let mut camera = CameraState::for_scene(&scene);
                camera.zoom(100.);
                camera.pan([25., -15.], [size.x as u32, size.y as u32]);
                let origin = camera.target;
                let (_, normal) =
                    camera.ray([size.x as u32, size.y as u32], [size.x * 0.5, size.y * 0.5]);
                let axis = Vec3::X;
                let delta = (axis - normal * axis.dot(normal)).normalize_or_zero() * 0.6;
                let a = project(&camera, rect, origin).unwrap();
                let b = project(&camera, rect, origin + delta).unwrap();
                let start = ray_plane(&camera, rect, a, origin, normal).unwrap();
                let end = ray_plane(&camera, rect, b, origin, normal).unwrap();
                assert!(
                    (end - start).abs_diff_eq(delta, 0.001),
                    "{kind:?}: {start:?} -> {end:?}"
                );
            }
        }
    }
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tool {
    #[default]
    Object,
    Collider,
    Pivot,
}
struct Drag {
    id: Id,
    transform: Transform,
    collider: Option<Collider>,
    bounds: Option<Aabb>,
    world: glam::Mat4,
    origin: Vec3,
    normal: Vec3,
    start: Vec3,
    axis: Option<usize>,
    edges: [i8; 3],
}
pub(super) struct Fit {
    id: Id,
    pieces: Vec<(Id, bool)>,
    margin: f32,
}
#[derive(Default)]
pub(crate) struct SpatialTools {
    pub mode: Tool,
    drag: Option<Drag>,
    pub(super) fit: Option<Fit>,
    pub base_pose: bool,
    previous_preview: bool,
}

fn ray_plane(
    camera: &CameraState,
    rect: Rect,
    pointer: Pos2,
    origin: Vec3,
    normal: Vec3,
) -> Option<Vec3> {
    let (o, d) = camera.ray(
        [rect.width().max(1.) as u32, rect.height().max(1.) as u32],
        [pointer.x - rect.min.x, pointer.y - rect.min.y],
    );
    let denominator = d.dot(normal);
    if denominator.abs() < 1e-5 {
        return None;
    }
    let t = (origin - o).dot(normal) / denominator;
    (t.is_finite() && t >= 0.).then_some(o + d * t)
}
impl Editor {
    pub(crate) fn structural_ready(&mut self) -> bool {
        if self.studio.playing || !self.studio.animation.drafts.is_empty() {
            self.log("Pause a animação e grave ou descarte as poses alteradas antes de editar pivô, caixa ou parentesco.");
            self.console = true;
            false
        } else {
            true
        }
    }
    pub(crate) fn set_spatial_tool(&mut self, mode: Tool) {
        if mode != Tool::Object {
            if self.selection.ids.len() != 1 {
                self.log("Escolha somente um objeto para editar seu colisor ou pivô.");
                self.console = true;
                return;
            }
            if !self.structural_ready() {
                return;
            }
            let Some(id) = self.selected.clone() else {
                return;
            };
            let valid = if mode == Tool::Collider {
                spatial::collider_bounds(self.scene(), &id).map(|_| ())
            } else {
                let mut scene = self.scene().clone();
                let p = scene.entity(&id).unwrap().transform.pivot;
                spatial::move_pivot(&mut scene, &id, Vec3::from(p))
            };
            if let Err(error) = valid {
                self.log(error);
                self.console = true;
                return;
            }
        }
        self.cancel_spatial_drag();
        if self.spatial.base_pose {
            self.studio.animation.preview = self.spatial.previous_preview;
        }
        self.spatial.base_pose = false;
        self.spatial.mode = mode;
        if mode != Tool::Object
            && self.tab == Tab::Studio
            && self.studio.tab == StudioTab::Animation
        {
            self.spatial.previous_preview = self.studio.animation.preview;
            self.spatial.base_pose = true;
        }
    }
    pub(crate) fn cancel_spatial_drag(&mut self) -> bool {
        if let Some(drag) = self.spatial.drag.take() {
            if let Some(e) = self.scene_mut().entity_mut(&drag.id) {
                e.transform = drag.transform;
                e.collider = drag.collider;
            }
            true
        } else {
            false
        }
    }
    pub(super) fn spatial_active_drag(&self) -> bool {
        self.spatial.drag.is_some()
    }
    pub(super) fn start_fit(&mut self, children: bool) {
        if !self.structural_ready() {
            return;
        }
        let Some(id) = self.selected.clone() else {
            return;
        };
        let ids = if children {
            self.scene().descendants(&id)
        } else {
            vec![id.clone()]
        };
        let pieces = ids
            .into_iter()
            .filter(|id| {
                self.scene()
                    .entity(id)
                    .is_some_and(|e| e.primitive.is_some() && e.camera.is_none() && e.ui.is_none())
            })
            .map(|id| (id, true))
            .collect();
        self.spatial.fit = Some(Fit {
            id,
            pieces,
            margin: 0.,
        });
    }
    fn fit_bounds(&self, fit: &Fit) -> Result<Aabb, String> {
        let mut bounds = spatial::visual_bounds(
            self.scene(),
            &fit.pieces
                .iter()
                .filter(|(_, yes)| *yes)
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>(),
        )?;
        bounds.min -= Vec3::splat(fit.margin);
        bounds.max += Vec3::splat(fit.margin);
        Ok(bounds)
    }
    pub(super) fn fit_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut fit) = self.spatial.fit.take() else {
            return;
        };
        let mut keep = true;
        egui::Window::new("Ajustar caixa · prévia na cena")
            .default_width(310.)
            .resizable(true)
            .show(ctx, |ui| {
                ui.label("Uma caixa na pose-base. Sprites usam seu retângulo completo.");
                ui.add(
                    egui::DragValue::new(&mut fit.margin)
                        .range(0.0..=100.)
                        .speed(0.01)
                        .prefix("Margem "),
                );
                egui::ScrollArea::vertical()
                    .max_height(190.)
                    .show(ui, |ui| {
                        for (id, included) in &mut fit.pieces {
                            if let Some(e) = self.scene().entity(id) {
                                ui.checkbox(included, &e.name);
                            }
                        }
                    });
                let proposal = self.fit_bounds(&fit);
                if let Err(error) = &proposal {
                    ui.label(error);
                }
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(proposal.is_ok(), egui::Button::new("Confirmar ajuste"))
                        .clicked()
                    {
                        if let Err(e) = proposal.and_then(|b| {
                            spatial::set_collider_bounds(self.scene_mut(), &fit.id, b)
                        }) {
                            self.log(e);
                        } else {
                            keep = false;
                        }
                    }
                    if ui.button("Cancelar").clicked() {
                        keep = false;
                    }
                });
            });
        if keep {
            self.spatial.fit = Some(fit);
        }
    }
    pub(super) fn center_pivot(&mut self, children: bool) {
        if !self.structural_ready() {
            return;
        }
        let Some(id) = self.selected.clone() else {
            return;
        };
        let ids = if children {
            self.scene().descendants(&id)
        } else {
            vec![id.clone()]
        };
        let result = spatial::visual_bounds(self.scene(), &ids).and_then(|b| {
            let pivot = spatial::invertible_world(self.scene(), &id)?
                .inverse()
                .transform_point3((b.min + b.max) * 0.5);
            spatial::move_pivot(self.scene_mut(), &id, pivot)
        });
        if let Err(e) = result {
            self.log(e);
            self.console = true;
        }
    }
    /// Returns true while a handle owns the pointer, including the release frame.
    pub(super) fn spatial_handles(&mut self, ui: &mut egui::Ui, scene: &Scene, rect: Rect) -> bool {
        if let Some(fit) = &self.spatial.fit
            && let Ok(b) = self.fit_bounds(fit)
        {
            let (shapes, _) = collider_debug::box_shapes(
                &self.camera,
                rect,
                b,
                scene.kind == SceneKind::TwoD,
                Color32::WHITE,
                true,
                true,
            );
            ui.painter().extend(shapes);
        }
        if self.spatial.mode == Tool::Object || self.selection.ids.len() != 1 {
            return false;
        }
        let Some(id) = self.selected.clone() else {
            return false;
        };
        let Some(entity) = scene.entity(&id) else {
            return false;
        };
        let Ok(world) = spatial::invertible_world(scene, &id) else {
            return false;
        };
        let collider = self.spatial.mode == Tool::Collider;
        let bounds = collider
            .then(|| spatial::collider_bounds(scene, &id).ok())
            .flatten();
        let center = if collider {
            let Some(b) = bounds else { return false };
            (b.min + b.max) * 0.5
        } else {
            world.transform_point3(Vec3::from(entity.transform.pivot))
        };
        let flat = scene.kind == SceneKind::TwoD;
        let mut handles: Vec<(Vec3, [i8; 3], Option<usize>)> = vec![(center, [0; 3], None)];
        if let Some(b) = bounds {
            if flat {
                for x in -1i8..=1 {
                    for y in -1i8..=1 {
                        if x != 0 || y != 0 {
                            handles.push((
                                Vec3::new(
                                    if x < 0 {
                                        b.min.x
                                    } else if x > 0 {
                                        b.max.x
                                    } else {
                                        center.x
                                    },
                                    if y < 0 {
                                        b.min.y
                                    } else if y > 0 {
                                        b.max.y
                                    } else {
                                        center.y
                                    },
                                    center.z,
                                ),
                                [x, y, 0],
                                None,
                            ));
                        }
                    }
                }
            } else {
                for axis in 0..3 {
                    for sign in [-1, 1] {
                        let mut p = center;
                        p[axis] = if sign < 0 { b.min[axis] } else { b.max[axis] };
                        let mut edges = [0; 3];
                        edges[axis] = sign;
                        handles.push((p, edges, Some(axis)));
                    }
                }
            }
        }
        // Translation axes have constant projected length. Center uses a view-facing plane.
        if !flat {
            for axis in 0..3 {
                let mut unit = Vec3::ZERO;
                unit[axis] = 1.;
                if let (Some(a), Some(b)) = (
                    project(&self.camera, rect, center),
                    project(&self.camera, rect, center + unit),
                ) {
                    let pixels = (b - a).length();
                    if pixels > 0.1 {
                        handles.push((center + unit * (55. / pixels), [0; 3], Some(axis)));
                    }
                }
            }
        }
        let pointer = ui.input(|i| i.pointer.interact_pos());
        let color = if collider {
            Color32::from_rgb(100, 245, 170)
        } else {
            Color32::from_rgb(230, 130, 255)
        };
        let mut hovered = None;
        for (index, (p, edges, axis)) in handles.iter().enumerate() {
            let Some(screen) = project(&self.camera, rect, *p) else {
                continue;
            };
            if !rect.contains(screen) {
                continue;
            }
            if *edges == [0; 3]
                && axis.is_some()
                && let Some(c) = project(&self.camera, rect, center)
            {
                ui.painter()
                    .line_segment([c, screen], egui::Stroke::new(2., color));
                ui.painter().text(
                    screen + Vec2::new(8., 0.),
                    egui::Align2::LEFT_CENTER,
                    ["X", "Y", "Z"][axis.unwrap()],
                    egui::FontId::proportional(12.),
                    color,
                );
            }
            let hot =
                ui.rect_contains_pointer(rect) && pointer.is_some_and(|p| p.distance(screen) < 10.);
            if hot && hovered.is_none() {
                hovered = Some(index);
            }
            ui.painter().rect_filled(
                Rect::from_center_size(screen, Vec2::splat(if hot { 12. } else { 9. })),
                2.,
                Color32::BLACK,
            );
            ui.painter().rect_stroke(
                Rect::from_center_size(screen, Vec2::splat(9.)),
                2.,
                egui::Stroke::new(2., color),
                egui::StrokeKind::Middle,
            );
        }
        if let Some(p) = project(&self.camera, rect, center) {
            ui.painter().text(
                p + Vec2::new(12., 22.),
                egui::Align2::LEFT_TOP,
                if collider {
                    "Centro da caixa"
                } else {
                    "Pivô — ponto de giro"
                },
                egui::FontId::proportional(13.),
                color,
            );
        }
        let owned = self.spatial.drag.is_some();
        if self.spatial.drag.is_none()
            && ui.input(|i| i.pointer.primary_pressed())
            && let (Some(index), Some(pointer)) = (hovered, pointer)
        {
            let (origin, edges, axis) = handles[index];
            let (_, view) = self.camera.ray(
                [rect.width().max(1.) as u32, rect.height().max(1.) as u32],
                [rect.width() * 0.5, rect.height() * 0.5],
            );
            let normal = if let Some(axis) = axis {
                let mut unit = Vec3::ZERO;
                unit[axis] = 1.;
                (view - unit * view.dot(unit)).normalize_or_zero()
            } else {
                view
            };
            if let Some(start) = ray_plane(&self.camera, rect, pointer, origin, normal) {
                self.spatial.drag = Some(Drag {
                    id: id.clone(),
                    transform: entity.transform.clone(),
                    collider: entity.collider.clone(),
                    bounds,
                    world,
                    origin,
                    normal,
                    start,
                    axis,
                    edges,
                });
            }
        }
        if let Some(drag) = &self.spatial.drag
            && let Some(pointer) = pointer
            && ui.input(|i| i.pointer.primary_down())
            && let Some(hit) = ray_plane(&self.camera, rect, pointer, drag.origin, drag.normal)
        {
            let mut delta = hit - drag.start;
            if let Some(axis) = drag.axis {
                let amount = delta[axis];
                delta = Vec3::ZERO;
                delta[axis] = amount;
            }
            let snap = (self.grid && !ui.input(|i| i.modifiers.alt)).then_some(self.grid_size);
            let result = if collider {
                let mut b = drag.bounds.unwrap();
                if drag.edges != [0; 3] {
                    b = spatial::resize_box(b, drag.edges, delta, snap);
                } else {
                    let center = (b.min + b.max) * 0.5;
                    if let Some(s) = snap {
                        let mut next = center + delta;
                        for a in 0..3 {
                            if (!flat || a < 2) && (drag.axis.is_none() || drag.axis == Some(a)) {
                                next[a] = (next[a] / s).round() * s;
                            }
                        }
                        delta = next - center;
                    }
                    b.min += delta;
                    b.max += delta;
                }
                spatial::set_collider_bounds(self.scene_mut(), &id, b)
            } else {
                let mut target = drag
                    .world
                    .transform_point3(Vec3::from(drag.transform.pivot))
                    + delta;
                if let Some(s) = snap {
                    for a in 0..3 {
                        if (!flat || a < 2) && (drag.axis.is_none() || drag.axis == Some(a)) {
                            target[a] = (target[a] / s).round() * s;
                        }
                    }
                }
                let pivot = drag.world.inverse().transform_point3(target);
                spatial::move_pivot(self.scene_mut(), &id, pivot)
            };
            if let Err(e) = result {
                self.log(e);
                self.cancel_spatial_drag();
            }
        }
        if ui.input(|i| i.pointer.primary_released()) {
            self.spatial.drag = None;
        }
        owned || self.spatial.drag.is_some() || hovered.is_some()
    }
}
