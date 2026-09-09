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
        let scene = self.scene();
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
        egui::ScrollArea::vertical().show(ui,|ui|{
            ui.heading("Encaixar vértices");
            ui.label(if state.source.is_none(){"Escolha o ponto de origem na seleção."}else if state.target.is_none(){"Escolha o ponto de destino em outra peça."}else{"Alinhamento em prévia"});
            ui.horizontal_wrapped(|ui|{changed|=ui.selectable_value(&mut state.axis,None,"Mover").changed();for(i,label)in ["Escalar X","Escalar Y","Escalar Z"].iter().enumerate(){changed|=ui.selectable_value(&mut state.axis,Some(i),*label).changed();}});
            ui.small("Escala usa o pivô da peça; seleções com várias raízes usam o centro do conjunto. Apenas o eixo escolhido muda.");
            for(index,label)in [(state.source,"Origem"),(state.target,"Destino")]{if let Some(index)=index{let p=&state.points[index];ui.label(format!("{label}: {:.3}, {:.3}, {:.3}",p.position.x,p.position.y,p.position.z));}}
            if let Some(error)=&state.error{ui.colored_label(Color32::LIGHT_YELLOW,error);}
            reset=ui.button("Escolher outros pontos").clicked();
            confirm=ui.add_enabled(state.target.is_some()&&state.error.is_none(),egui::Button::new("Confirmar encaixe (Enter)")).clicked()||ui.input(|i|i.key_pressed(egui::Key::Enter))&&state.target.is_some()&&state.error.is_none();
            cancel=ui.button("Cancelar (Esc)").clicked();
        });
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
            painter.circle_filled(screen, 3., Color32::from_rgb(105, 224, 208));
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
                painter.circle_stroke(screen, 8., egui::Stroke::new(2., Color32::GOLD));
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
