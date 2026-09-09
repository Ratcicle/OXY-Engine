use super::PaintTool;
use crate::app::Editor;
use egui::{Color32, Rect, Vec2};
use oxy_core::{
    document::Scene,
    geometry::{EditableMesh, selection::Mode},
};
impl Editor {
    pub(crate) fn paint_selecting(&self) -> bool {
        self.studio.tool == PaintTool::Select
    }
    pub(crate) fn select_paint_face(&mut self, face: Option<u32>, uv: [f32; 2], toggle: bool) {
        let Ok(mesh) = self.model_source() else {
            return;
        };
        let face = face.or_else(|| {
            oxy_core::geometry::uv::pick(&mesh, glam::Vec2::from(uv), &self.mesh_components().ids)
        });
        self.select_mesh_face(face, toggle);
    }
    pub(super) fn draw_uv_islands(&self, ui: &egui::Ui, rect: Rect, mesh: &EditableMesh) {
        let painter = ui.painter_at(rect);
        let point = |uv: [f32; 2]| rect.min + Vec2::from(uv) * rect.size();
        let selected = self
            .mesh_components()
            .ids
            .iter()
            .copied()
            .collect::<std::collections::HashSet<_>>();
        for face in &mesh.data().faces {
            let active = self.mesh_components().mode == Mode::Face && selected.contains(&face.id);
            for (a, b) in face
                .corners
                .iter()
                .zip(face.corners.iter().cycle().skip(1))
                .take(face.corners.len())
            {
                painter.line_segment(
                    [point(a.uv), point(b.uv)],
                    egui::Stroke::new(
                        if active { 2. } else { 1. },
                        if active {
                            Color32::GOLD
                        } else {
                            Color32::from_rgba_unmultiplied(70, 235, 216, 140)
                        },
                    ),
                );
            }
        }
        for t in mesh
            .prepared()
            .triangles
            .iter()
            .filter(|t| self.mesh_components().mode == Mode::Face && selected.contains(&t.face))
        {
            let face = mesh.face(t.face).unwrap();
            painter.add(egui::Shape::convex_polygon(
                t.corners.map(|i| point(face.corners[i].uv)).to_vec(),
                Color32::from_rgba_unmultiplied(245, 175, 70, 45),
                egui::Stroke::NONE,
            ));
        }
    }
    pub(crate) fn draw_paint_faces(&mut self, ui: &egui::Ui, rect: Rect, scene: &Scene) {
        if self.mesh_components().mode != Mode::Face || self.mesh_components().ids.is_empty() {
            return;
        }
        let Ok(mesh) = self.model_source() else {
            return;
        };
        let Some(id) = self.selected.as_ref() else {
            return;
        };
        let Ok(world) = scene.world_matrix(id) else {
            return;
        };
        let picker = oxy_render::ScenePicker::new(scene);
        let size = [rect.width().max(1.) as u32, rect.height().max(1.) as u32];
        let mut visible = std::collections::HashSet::new();
        for &id in &self.mesh_components().ids {
            if let Some(face) = mesh.face(id) {
                let center = face
                    .corners
                    .iter()
                    .map(|c| mesh.position(c.vertex).unwrap())
                    .sum::<glam::Vec3>()
                    / face.corners.len() as f32;
                let p = world.transform_point3(center);
                if let Some(pixel) = oxy_render::collider_debug::project(&self.camera, rect, p) {
                    let (o, d) = self
                        .camera
                        .ray(size, [pixel.x - rect.min.x, pixel.y - rect.min.y]);
                    let depth = (p - o).dot(d);
                    if picker
                        .ray(o, d)
                        .is_none_or(|hit| hit.distance >= depth - 0.0001 * (1. + depth.abs()))
                    {
                        visible.insert(id);
                    }
                }
            }
        }
        let painter = ui.painter_at(rect);
        for triangle in mesh
            .prepared()
            .triangles
            .iter()
            .filter(|t| visible.contains(&t.face))
        {
            let positions: Option<Vec<_>> = mesh
                .triangle_points(triangle)
                .into_iter()
                .map(|p| {
                    oxy_render::collider_debug::project(
                        &self.camera,
                        rect,
                        world.transform_point3(p),
                    )
                })
                .collect();
            if let Some(positions) = positions {
                painter.add(egui::Shape::convex_polygon(
                    positions,
                    Color32::from_rgba_unmultiplied(245, 175, 70, 45),
                    egui::Stroke::NONE,
                ));
            }
        }
    }
}
