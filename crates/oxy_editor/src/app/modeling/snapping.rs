use super::*;
use oxy_core::geometry::snap::{self, Method};
use oxy_render::{ScenePicker, collider_debug::project};
struct Point {
    entity: Id,
    position: Vec3,
    moving: bool,
}
pub(super) struct Snap {
    pub selection: Selection,
    pub selected: Option<Id>,
    base: Vec<(Id, Transform)>,
    points: Vec<Point>,
    source: Option<usize>,
    target: Option<usize>,
    axis: Option<usize>,
    pivot: Vec3,
    error: Option<String>,
}
impl Editor {
    pub(super) fn begin_snap(&mut self) {
        if self.mesh_operation_active() {
            return;
        }
        if self.components_active() {
            self.warn("Encaixar vértices move a peça inteira. Use o modo Objeto (1).");
            return;
        }
        if self.selection.ids.is_empty() {
            self.warn("Selecione a peça ou grupo que será alinhado.");
            return;
        }
        if !self.studio.animation.drafts.is_empty() || self.studio.playing {
            self.warn("Pause e grave ou descarte a pose antes de alinhar a montagem.");
            return;
        }
        // Only what is on screen can be a target: the isolated object in the Studio.
        let shown = self.view_scene();
        let scene = &shown;
        let view = oxy_core::scene_view::SceneView::new(scene);
        let moving = self
            .selection
            .ids
            .iter()
            .flat_map(|id| view.descendants(id))
            .chain(self.selection.ids.iter().cloned())
            .collect::<std::collections::HashSet<_>>();
        let roots = editing::selection_roots(scene, &self.selection.ids);
        let base = roots
            .iter()
            .filter_map(|id| scene.entity(id).map(|e| (id.clone(), e.transform.clone())))
            .collect::<Vec<_>>();
        let pivot = if roots.len() == 1 {
            let e = scene.entity(&roots[0]).unwrap();
            view.world_matrix(&e.id)
                .unwrap_or_default()
                .transform_point3(Vec3::from(e.transform.pivot))
        } else {
            editing::selection_center(scene, &roots)
        };
        let mut points = Vec::new();
        for entity in &scene.entities {
            if !entity.has_geometry() || !view.visible(entity) {
                continue;
            }
            let Ok(world) = view.world_matrix(&entity.id) else {
                continue;
            };
            let mesh = if let Some(mesh) = &entity.mesh {
                mesh.clone()
            } else {
                let mut copy = entity.clone();
                if primitives::convert(&mut copy).is_err() {
                    continue;
                }
                copy.mesh.unwrap()
            };
            for v in &mesh.data().vertices {
                points.push(Point {
                    entity: entity.id.clone(),
                    position: world.transform_point3(Vec3::from(v.position)),
                    moving: moving.contains(&entity.id),
                });
            }
        }
        if !points.iter().any(|p| p.moving) || !points.iter().any(|p| !p.moving) {
            self.warn("É necessário um vértice na seleção e outro em uma peça externa a ela.");
            return;
        }
        self.finish_history(true);
        self.history.begin(
            "Encaixar peças por vértices",
            &self.state.project,
            &self.state.images,
        );
        self.modeling.snap = Some(Snap {
            selection: self.selection.clone(),
            selected: self.selected.clone(),
            base,
            points,
            source: None,
            target: None,
            axis: None,
            pivot,
            error: None,
        });
    }
    fn update_snap(&mut self) {
        let Some(mut state) = self.modeling.snap.take() else {
            return;
        };
        for (id, t) in &state.base {
            if let Some(e) = self.scene_mut().entity_mut(id) {
                e.transform = t.clone();
            }
        }
        if let (Some(a), Some(b)) = (state.source, state.target) {
            let method = state.axis.map_or(Method::Move, |axis| Method::Scale {
                axis,
                pivot: state.pivot,
            });
            state.error = snap::apply(
                self.scene_mut(),
                &state.selection.ids,
                state.points[a].position,
                state.points[b].position,
                method,
            )
            .err();
        }
        self.modeling.snap = Some(state);
    }
    pub(crate) fn snap_panel(&mut self, ui: &mut egui::Ui) -> bool {
        let Some(state) = self.modeling.snap.as_mut() else {
            return false;
        };
        let mut confirm = false;
        let mut cancel = false;
        let mut changed = false;
        let mut reset = false;
        use crate::{theme, widgets};
        let status = if state.source.is_none() {
            "Escolha a origem"
        } else if state.target.is_none() {
            "Escolha o destino"
        } else {
            "Em prévia"
        };
        let ready = state.target.is_some() && state.error.is_none();
        egui::ScrollArea::vertical().show(ui, |ui| {
            widgets::panel_card(
                ui,
                crate::icons::Icon::Snap,
                "Encaixar vértices",
                status,
                true,
                |ui| {
                    let steps = [
                        (
                            state.source,
                            "Origem",
                            "Clique num vértice da peça selecionada",
                        ),
                        (state.target, "Destino", "Clique num vértice de outra peça"),
                    ];
                    let active = usize::from(state.source.is_some());
                    for (i, (point, title, wait)) in steps.into_iter().enumerate() {
                        let detail = match point {
                            Some(index) => {
                                let p = state.points[index].position;
                                format!("{:.3}, {:.3}, {:.3}", p.x, p.y, p.z)
                            }
                            None if i == active => wait.to_owned(),
                            None => "—".to_owned(),
                        };
                        snap_step(ui, i + 1, title, &detail, point.is_some(), i == active);
                    }
                    ui.add_space(4.);
                    widgets::property_row(ui, "Ajuste", "", |ui| {
                        let mut axis = state.axis;
                        changed |= widgets::segmented(
                            ui,
                            &mut axis,
                            &[
                                (None, "Mover"),
                                (Some(0), "Escalar X"),
                                (Some(1), "Escalar Y"),
                                (Some(2), "Escalar Z"),
                            ],
                        );
                        state.axis = axis;
                    });
                    widgets::hint(
                        ui,
                        if state.axis.is_none() {
                            "Move a seleção até a origem coincidir com o destino."
                        } else {
                            "Escala só no eixo escolhido, a partir do pivô da peça (ou do \
                             centro do conjunto)."
                        },
                    );
                    if let Some(error) = &state.error {
                        ui.colored_label(theme::WARNING, error);
                    }
                    match widgets::preview_footer(
                        ui,
                        ready,
                        "Escolha a origem e o destino primeiro.",
                        |ui| {
                            reset = ui
                                .add(egui::Button::new("Outros pontos").frame(false))
                                .on_hover_text("Escolher outros pontos")
                                .clicked();
                        },
                    ) {
                        Some(true) => confirm = true,
                        Some(false) => cancel = true,
                        None => {}
                    }
                },
            );
        });
        confirm |= ready && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if reset {
            state.source = None;
            state.target = None;
            state.error = None;
            changed = true;
        }
        if changed {
            self.update_snap();
        }
        if cancel {
            self.cancel_mesh_operation();
        } else if confirm {
            self.modeling.snap = None;
            self.finish_history(true);
        }
        true
    }
    pub(super) fn snap_viewport(
        &mut self,
        ui: &mut egui::Ui,
        scene: &Scene,
        rect: Rect,
        response: &egui::Response,
    ) -> bool {
        let Some(state) = self.modeling.snap.as_ref() else {
            return false;
        };
        let picker = ScenePicker::new(scene);
        let size = [rect.width().max(1.) as u32, rect.height().max(1.) as u32];
        let painter = ui.painter_at(rect);
        let pointer = ui
            .input(|i| i.pointer.hover_pos())
            .filter(|p| rect.contains(*p));
        let mut nearest = 11.;
        let mut hovered = None;
        for (i, p) in state.points.iter().enumerate() {
            if (state.source.is_none()) != p.moving {
                continue;
            }
            let Some(screen) = project(&self.camera, rect, p.position) else {
                continue;
            };
            let (o, d) = self
                .camera
                .ray(size, [screen.x - rect.min.x, screen.y - rect.min.y]);
            let depth = (p.position - o).dot(d);
            if picker
                .ray(o, d)
                .is_some_and(|hit| hit.distance < depth - 0.0001 * (1. + depth.abs()))
            {
                continue;
            }
            painter.circle_filled(screen, 3., crate::theme::ACCENT_BRIGHT);
            if let Some(pointer) = pointer {
                let distance = screen.distance(pointer);
                if distance < nearest {
                    nearest = distance;
                    hovered = Some(i);
                }
            }
        }
        for index in [state.source, state.target].into_iter().flatten() {
            if let Some(screen) = project(&self.camera, rect, state.points[index].position) {
                painter.circle_stroke(screen, 8., egui::Stroke::new(2., crate::theme::HIGHLIGHT));
            }
        }
        if let Some(index) = hovered {
            let p = &state.points[index];
            if let Some(screen) = project(&self.camera, rect, p.position) {
                painter.circle_stroke(screen, 6., egui::Stroke::new(2., Color32::WHITE));
                painter.text(
                    screen + Vec2::new(10., -12.),
                    egui::Align2::LEFT_BOTTOM,
                    scene.entity(&p.entity).map_or("Peça", |e| e.name.as_str()),
                    egui::FontId::proportional(13.),
                    Color32::WHITE,
                );
            }
        }
        if response.clicked()
            && let Some(index) = hovered
        {
            let state = self.modeling.snap.as_mut().unwrap();
            if state.source.is_none() {
                state.source = Some(index);
            } else {
                state.target = Some(index);
            }
            self.update_snap();
        }
        true
    }
}

/// Numbered step of the snap: done, waiting for a click, or still ahead.
fn snap_step(
    ui: &mut egui::Ui,
    number: usize,
    title: &str,
    detail: &str,
    done: bool,
    active: bool,
) {
    use crate::theme;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 40.), Sense::hover());
    if active && !done {
        ui.painter()
            .rect_filled(rect, 6., Color32::from_rgb(38, 53, 72));
    }
    let center = egui::pos2(rect.left() + 18., rect.center().y);
    if done {
        ui.painter().circle_filled(center, 10., theme::ACCENT);
        crate::icons::paint(
            ui,
            Rect::from_center_size(center, Vec2::splat(10.)),
            crate::icons::Icon::Check,
            Color32::WHITE,
        );
    } else {
        let color = if active {
            theme::ACCENT_BRIGHT
        } else {
            theme::TEXT_MUTED
        };
        ui.painter()
            .circle_stroke(center, 9.5, egui::Stroke::new(1.3, color));
        ui.painter().text(
            center,
            egui::Align2::CENTER_CENTER,
            number.to_string(),
            theme::medium(11.),
            color,
        );
    }
    let left = rect.left() + 38.;
    ui.painter().text(
        egui::pos2(left, rect.center().y - 1.),
        egui::Align2::LEFT_BOTTOM,
        title,
        theme::medium(13.),
        if done || active {
            theme::TEXT
        } else {
            theme::TEXT_MUTED
        },
    );
    ui.painter().text(
        egui::pos2(left, rect.center().y + 1.),
        egui::Align2::LEFT_TOP,
        detail,
        egui::FontId::monospace(11.5),
        theme::TEXT_MUTED,
    );
}
