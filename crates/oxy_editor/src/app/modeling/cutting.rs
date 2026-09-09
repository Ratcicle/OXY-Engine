use super::*;
use oxy_core::geometry::cuts::{Point, Segment};
use oxy_render::{
    ScenePicker,
    collider_debug::{project, segment_distance},
};
#[derive(Clone, Default)]
pub(super) struct Path {
    pub segments: Vec<Segment>,
    anchor: Option<Point>,
}
fn local(mesh: &EditableMesh, p: Point) -> Option<Vec3> {
    let e = mesh.edge(p.edge)?;
    Some(
        mesh.position(e.vertices[0])?
            .lerp(mesh.position(e.vertices[1])?, p.factor),
    )
}
impl Editor {
    pub(super) fn knife_viewport(
        &mut self,
        ui: &mut egui::Ui,
        scene: &Scene,
        rect: Rect,
        response: &egui::Response,
    ) -> bool {
        let Some(preview) = self
            .modeling
            .preview
            .as_ref()
            .filter(|p| p.operation == Operation::Knife)
        else {
            return false;
        };
        let source = preview.source.clone();
        let world = preview.world;
        let anchor = preview.path.anchor;
        let painter = ui.painter_at(rect);
        let picker = ScenePicker::new(scene);
        let size = [rect.width().max(1.) as u32, rect.height().max(1.) as u32];
        let visible = |local: Vec3| {
            let p = world.transform_point3(local);
            let Some(pixel) = project(&self.camera, rect, p) else {
                return false;
            };
            let (o, d) = self
                .camera
                .ray(size, [pixel.x - rect.min.x, pixel.y - rect.min.y]);
            let depth = (p - o).dot(d);
            picker
                .ray(o, d)
                .is_none_or(|hit| hit.distance >= depth - 0.0001 * (1. + depth.abs()))
        };
        let pointer = ui
            .input(|i| i.pointer.hover_pos())
            .filter(|p| rect.contains(*p));
        let mut nearest = 12.;
        let mut hovered = None;
        for edge in &source.data().edges {
            let a = source.position(edge.vertices[0]).unwrap();
            let b = source.position(edge.vertices[1]).unwrap();
            let (Some(sa), Some(sb)) = (
                project(&self.camera, rect, world.transform_point3(a)),
                project(&self.camera, rect, world.transform_point3(b)),
            ) else {
                continue;
            };
            if visible((a + b) * 0.5) {
                painter.line_segment([sa, sb], egui::Stroke::new(1., Color32::GRAY));
            }
            if let Some(pointer) = pointer {
                let distance = segment_distance(pointer, sa, sb);
                if distance >= nearest {
                    continue;
                }
                let (o, d) = self
                    .camera
                    .ray(size, [pointer.x - rect.min.x, pointer.y - rect.min.y]);
                let aw = world.transform_point3(a);
                let bw = world.transform_point3(b);
                let v = bw - aw;
                let r = o - aw;
                let denominator = v.length_squared() - v.dot(d).powi(2);
                if denominator.abs() < 1e-9 {
                    continue;
                }
                let mut factor = ((v.dot(r) - v.dot(d) * d.dot(r)) / denominator).clamp(0., 1.);
                if pointer.distance(sa) < 7. {
                    factor = 0.;
                } else if pointer.distance(sb) < 7. {
                    factor = 1.;
                }
                if !visible(a.lerp(b, factor)) {
                    continue;
                }
                nearest = distance;
                hovered = Some(Point {
                    edge: edge.id,
                    factor,
                });
            }
        }
        for segment in &preview.path.segments {
            if let (Some(a), Some(b)) = (
                local(&source, segment.from)
                    .and_then(|p| project(&self.camera, rect, world.transform_point3(p))),
                local(&source, segment.to)
                    .and_then(|p| project(&self.camera, rect, world.transform_point3(p))),
            ) {
                painter.line_segment([a, b], egui::Stroke::new(2.5, Color32::GOLD));
            }
        }
        for p in [anchor, hovered].into_iter().flatten() {
            if let Some(screen) = local(&source, p)
                .and_then(|p| project(&self.camera, rect, world.transform_point3(p)))
            {
                painter.circle_filled(screen, 5., Color32::from_rgb(112, 239, 213));
            }
        }
        if let (Some(a), Some(b)) = (anchor, hovered)
            && let (Some(a), Some(b)) = (
                local(&source, a)
                    .and_then(|p| project(&self.camera, rect, world.transform_point3(p))),
                local(&source, b)
                    .and_then(|p| project(&self.camera, rect, world.transform_point3(p))),
            )
        {
            painter.line_segment([a, b], egui::Stroke::new(1., Color32::WHITE));
        }
        // egui can classify two quick finishing clicks as a triple click when
        // preceded by the path's last point. Only finish on the current anchor;
        // rapid clicks on distinct edges must continue constructing the path.
        let at_anchor = anchor
            .and_then(|a| local(&source, a))
            .and_then(|p| project(&self.camera, rect, world.transform_point3(p)))
            .zip(pointer)
            .is_some_and(|(a, p)| a.distance(p) < 7.);
        if (response.double_clicked() || response.triple_clicked())
            && at_anchor
            && !preview.path.segments.is_empty()
        {
            self.confirm_mesh_operation();
            return true;
        }
        if response.clicked() {
            let next = hovered.ok_or("Escolha uma borda ou vértice visível da peça para o corte.");
            let candidate=next.and_then(|next|{
                let Some(anchor)=anchor else{return Ok((next,None));};
                let a=local(&source,anchor).unwrap();let b=local(&source,next).unwrap();
                if a.distance(b)<1e-6{return Ok((next,None));}
                let a_faces=&source.prepared().incident_faces[source.prepared().edges[&anchor.edge]];let b_faces=&source.prepared().incident_faces[source.prepared().edges[&next.edge]];
                let common=a_faces.iter().filter_map(|&(fi,_)|b_faces.iter().any(|&(f,_)|f==fi).then_some(fi)).collect::<Vec<_>>();
                if common.len()!=1{return Err("Escolha uma borda que atravesse uma única face adjacente ao ponto anterior.");}
                if (1..8).any(|i|!visible(a.lerp(b,i as f32/8.))){return Err("Este trecho passa por uma superfície oculta. Gire a câmera ou escolha um percurso visível.");}
                Ok((next,Some(Segment{face:source.data().faces[common[0]].id,from:anchor,to:next})))
            });
            let preview = self.modeling.preview.as_mut().unwrap();
            match candidate {
                Ok((next, segment)) => {
                    if let Some(segment) = segment {
                        let mut segments = preview.path.segments.clone();
                        segments.push(segment);
                        match oxy_core::geometry::cuts::knife(&source, &segments) {
                            Ok(_) => {
                                preview.path.segments = segments;
                                preview.path.anchor = Some(next);
                                preview.previous = [f32::NAN; 3];
                            }
                            Err(e) => preview.error = Some(e),
                        }
                    } else {
                        preview.path.anchor = Some(next);
                        preview.error = preview.path.segments.is_empty().then(|| {
                            "Escolha a próxima borda para criar o primeiro trecho.".into()
                        });
                    }
                }
                Err(e) => preview.error = Some(e.into()),
            }
            if self
                .modeling
                .preview
                .as_ref()
                .is_some_and(|p| !p.path.segments.is_empty())
            {
                self.update_mesh_preview();
            }
        }
        true
    }
    pub(super) fn loop_viewport(
        &mut self,
        ui: &mut egui::Ui,
        scene: &Scene,
        rect: Rect,
        response: &egui::Response,
    ) -> bool {
        let Some(p) = self
            .modeling
            .preview
            .as_ref()
            .filter(|p| p.operation == Operation::Loop)
        else {
            return false;
        };
        let source = p.source.clone();
        let world = p.world;
        let pointer = ui
            .input(|i| i.pointer.latest_pos())
            .filter(|p| rect.contains(*p));
        let active = p.drag.is_some();
        let chosen = p.cut_edge;
        let picker = ScenePicker::new(scene);
        let mut nearest = 12.;
        let mut hit = None;
        let size = [rect.width().max(1.) as u32, rect.height().max(1.) as u32];
        if let Some(pointer) = pointer {
            let (origin, direction) = self
                .camera
                .ray(size, [pointer.x - rect.min.x, pointer.y - rect.min.y]);
            for edge in &source.data().edges {
                if active && chosen != Some(edge.id) {
                    continue;
                }
                let a = world.transform_point3(source.position(edge.vertices[0]).unwrap());
                let b = world.transform_point3(source.position(edge.vertices[1]).unwrap());
                let (Some(sa), Some(sb)) = (
                    project(&self.camera, rect, a),
                    project(&self.camera, rect, b),
                ) else {
                    continue;
                };
                let distance = segment_distance(pointer, sa, sb);
                if !active && distance >= nearest {
                    continue;
                }
                let v = b - a;
                let r = origin - a;
                let denominator = v.length_squared() - v.dot(direction).powi(2);
                if denominator.abs() < 1e-9 {
                    continue;
                }
                let factor = ((v.dot(r) - v.dot(direction) * direction.dot(r)) / denominator)
                    .clamp(0.001, 0.999);
                let point = a.lerp(b, factor);
                if !active
                    && picker
                        .ray(origin, direction)
                        .is_some_and(|h| h.distance < (point - origin).dot(direction) - 0.0001)
                {
                    continue;
                }
                nearest = distance;
                hit = Some((edge.id, factor, sa, sb));
            }
        }
        if let Some((_, factor, a, b)) = hit {
            let painter = ui.painter_at(rect);
            painter.line_segment([a, b], egui::Stroke::new(2., Color32::GOLD));
            painter.circle_filled(a.lerp(b, factor), 5., Color32::WHITE);
        }
        let press = response.contains_pointer()
            && ui.input(|i| i.pointer.primary_pressed() && !i.modifiers.ctrl);
        if (press || active)
            && let Some((edge, factor, _, _)) = hit
        {
            let p = self.modeling.preview.as_mut().unwrap();
            p.cut_edge = Some(edge);
            p.values[1] = ((factor - 0.5) * (p.count + 1) as f32 / 0.95).clamp(-1., 1.);
            p.previous = [f32::NAN; 3];
            if press {
                p.drag = Some(Drag {
                    axis: None,
                    start: pointer.unwrap(),
                    direction: Vec2::ZERO,
                    base: p.values,
                    plane_origin: Vec3::ZERO,
                    plane_normal: Vec3::Y,
                    world_axis: Vec3::X,
                    start_world: Vec3::ZERO,
                });
            }
            self.update_mesh_preview();
        }
        if active && !ui.input(|i| i.pointer.primary_down()) {
            self.modeling.preview.as_mut().unwrap().drag = None;
            self.confirm_mesh_operation();
        }
        true
    }
}
