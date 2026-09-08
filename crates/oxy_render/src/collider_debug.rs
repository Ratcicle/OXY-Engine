//! Screen-space debug drawing backed by the same bounds as the physics solver.
//! Egui batches the strokes/fills on top of the viewport without writing scene depth.
use crate::CameraState;
use egui::{Color32, Pos2, Rect, Shape, Stroke, Vec2};
use glam::Vec3;
use oxy_core::{collision::Aabb, document::*, scene_view::SceneView};

#[derive(Default)]
pub struct OverlayFrame {
    pub outlines: Vec<(Id, Vec<[Pos2; 2]>)>,
    pub errors: Vec<String>,
}
impl OverlayFrame {
    pub fn pick(&self, pointer: Pos2, selected: &[Id]) -> Option<Id> {
        self.outlines
            .iter()
            .filter(|(_, edges)| {
                edges
                    .iter()
                    .any(|[a, b]| segment_distance(pointer, *a, *b) < 7.)
            })
            .min_by_key(|(id, _)| !selected.contains(id))
            .map(|(id, _)| id.clone())
    }
}
pub fn segment_distance(point: Pos2, a: Pos2, b: Pos2) -> f32 {
    let delta = b - a;
    let t = ((point - a).dot(delta) / delta.length_sq().max(0.0001)).clamp(0., 1.);
    point.distance(a + delta * t)
}
pub fn project(camera: &CameraState, rect: Rect, point: Vec3) -> Option<Pos2> {
    camera
        .world_to_screen(
            point,
            [rect.width().max(1.) as u32, rect.height().max(1.) as u32],
        )
        .map(|p| rect.min + Vec2::from(p))
}
pub fn corners(bounds: Aabb, flat: bool) -> [Vec3; 8] {
    let z = if flat {
        (bounds.min.z + bounds.max.z) * 0.5
    } else {
        bounds.min.z
    };
    let (a, b) = (bounds.min, bounds.max);
    [
        Vec3::new(a.x, a.y, z),
        Vec3::new(b.x, a.y, z),
        Vec3::new(b.x, b.y, z),
        Vec3::new(a.x, b.y, z),
        Vec3::new(a.x, a.y, b.z),
        Vec3::new(b.x, a.y, b.z),
        Vec3::new(b.x, b.y, b.z),
        Vec3::new(a.x, b.y, b.z),
    ]
}
pub const EDGES: [[usize; 2]; 12] = [
    [0, 1],
    [1, 2],
    [2, 3],
    [3, 0],
    [4, 5],
    [5, 6],
    [6, 7],
    [7, 4],
    [0, 4],
    [1, 5],
    [2, 6],
    [3, 7],
];
pub fn dashed(shapes: &mut Vec<Shape>, a: Pos2, b: Pos2, stroke: Stroke) {
    let distance = a.distance(b);
    let direction = (b - a).normalized();
    for i in 0..(distance / 10.).ceil().min(4096.) as usize {
        let start = i as f32 * 10.;
        let end = (start + 5.).min(distance);
        if end > start {
            shapes.push(Shape::line_segment(
                [a + direction * start, a + direction * end],
                stroke,
            ));
        }
    }
}
pub fn box_shapes(
    camera: &CameraState,
    rect: Rect,
    bounds: Aabb,
    flat: bool,
    color: Color32,
    selected: bool,
    disabled: bool,
) -> (Vec<Shape>, Vec<[Pos2; 2]>) {
    let points = corners(bounds, flat).map(|p| project(camera, rect, p));
    let mut shapes = Vec::new();
    let mut edges = Vec::new();
    let faces: &[[usize; 4]] = if flat {
        &[[0, 1, 2, 3]]
    } else {
        &[
            [0, 1, 2, 3],
            [4, 5, 6, 7],
            [0, 1, 5, 4],
            [1, 2, 6, 5],
            [2, 3, 7, 6],
            [3, 0, 4, 7],
        ]
    };
    for face in faces {
        let polygon: Option<Vec<_>> = face.iter().map(|&i| points[i]).collect();
        if let Some(polygon) = polygon {
            shapes.push(Shape::convex_polygon(
                polygon,
                color.gamma_multiply(if selected { 0.09 } else { 0.025 }),
                Stroke::NONE,
            ));
        }
    }
    for [a, b] in EDGES.iter().take(if flat { 4 } else { 12 }) {
        if let (Some(a), Some(b)) = (points[*a], points[*b]) {
            // Clip oversized projected edges before tessellation (near-plane/large scenes).
            if !rect.expand(12.).intersects(Rect::from_two_pos(a, b)) {
                continue;
            }
            edges.push([a, b]);
            let stroke = Stroke::new(
                if selected { 2.5 } else { 1.7 },
                if disabled {
                    color.gamma_multiply(0.6)
                } else {
                    color
                },
            );
            if disabled {
                dashed(&mut shapes, a, b, stroke);
            } else {
                shapes.push(Shape::line_segment(
                    [a, b],
                    Stroke::new(stroke.width + 2., Color32::from_black_alpha(185)),
                ));
                shapes.push(Shape::line_segment([a, b], stroke));
            }
        }
    }
    if let Some(center) = project(camera, rect, (bounds.min + bounds.max) * 0.5) {
        for vector in [Vec2::X, Vec2::Y] {
            shapes.push(Shape::line_segment(
                [center - vector * 5., center + vector * 5.],
                Stroke::new(2., color),
            ));
        }
    }
    (shapes, edges)
}
pub fn draw(
    ui: &egui::Ui,
    scene: &Scene,
    camera: &CameraState,
    rect: Rect,
    global: bool,
    selected: &[Id],
    show_disabled: bool,
) -> OverlayFrame {
    let mut frame = OverlayFrame::default();
    if !global
        && !selected
            .iter()
            .any(|id| scene.entity(id).is_some_and(|e| e.collider.is_some()))
    {
        return frame;
    }
    let view = SceneView::new(scene);
    let painter = ui.painter_at(rect);
    let mut shapes = Vec::new();
    // Selected boxes are drawn last and win contour picking even when overlapping.
    let mut entities: Vec<_> = scene
        .entities
        .iter()
        .filter(|e| e.collider.is_some())
        .collect();
    entities.sort_by_key(|e| selected.contains(&e.id));
    for entity in entities {
        let active_selection = selected.contains(&entity.id);
        let collider = entity.collider.as_ref().unwrap();
        if !active_selection && (!global || (!collider.enabled && !show_disabled)) {
            continue;
        }
        let bounds = match view.collider_bounds(&entity.id) {
            Ok(b) => b,
            Err(error) => {
                frame.errors.push(format!("{}: {error}", entity.name));
                continue;
            }
        };
        let color = if collider.is_trigger {
            Color32::from_rgb(250, 221, 80)
        } else {
            Color32::from_rgb(88, 240, 151)
        };
        let (mut box_shapes, edges) = box_shapes(
            camera,
            rect,
            bounds,
            scene.kind == SceneKind::TwoD,
            color,
            active_selection,
            !collider.enabled,
        );
        shapes.append(&mut box_shapes);
        frame.outlines.push((entity.id.clone(), edges));
        if active_selection
            && let Some(center) = project(camera, rect, (bounds.min + bounds.max) * 0.5)
        {
            let label = format!(
                "{} · {}{}",
                entity.name,
                if collider.is_trigger {
                    "Área de detecção"
                } else {
                    "Colisor sólido"
                },
                if collider.enabled {
                    ""
                } else {
                    " · desativado"
                }
            );
            let galley = painter.layout_no_wrap(label, egui::FontId::proportional(13.), color);
            let width = galley.size().x;
            let pos = Pos2::new(
                (center.x + 12.).clamp(
                    rect.left() + 4.,
                    (rect.right() - width - 4.).max(rect.left() + 4.),
                ),
                (center.y - 28.).clamp(rect.top() + 4., (rect.bottom() - 22.).max(rect.top() + 4.)),
            );
            shapes.push(Shape::rect_filled(
                Rect::from_min_size(pos - Vec2::splat(3.), galley.size() + Vec2::splat(6.)),
                3.,
                Color32::from_black_alpha(210),
            ));
            shapes.push(Shape::galley(pos, galley, color));
        }
    }
    painter.extend(shapes);
    frame
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overlapping_contours_prefer_selection_and_fill_is_not_a_target() {
        let edges = vec![
            [Pos2::new(10., 10.), Pos2::new(110., 10.)],
            [Pos2::new(110., 10.), Pos2::new(110., 110.)],
            [Pos2::new(110., 110.), Pos2::new(10., 110.)],
            [Pos2::new(10., 110.), Pos2::new(10., 10.)],
        ];
        let frame = OverlayFrame {
            outlines: vec![("a".into(), edges.clone()), ("b".into(), edges)],
            errors: Vec::new(),
        };
        assert_eq!(
            frame.pick(Pos2::new(50., 10.), &["b".into()]),
            Some("b".into())
        );
        assert_eq!(frame.pick(Pos2::new(50., 50.), &["b".into()]), None);
    }
    #[test]
    fn invisible_group_is_drawn_when_selected_and_disabled_never_enables_physics() {
        let mut scene = Scene::new("Teste", SceneKind::TwoD);
        let mut e = Entity::new("Personagem", None);
        e.visible = false;
        e.collider = Some(Collider {
            enabled: false,
            ..Default::default()
        });
        let id = e.id.clone();
        scene.entities.push(e);
        let before = scene.clone();
        let ctx = egui::Context::default();
        let camera = CameraState::for_scene(&scene);
        let mut edges = 0;
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let frame = draw(
                    ui,
                    &scene,
                    &camera,
                    Rect::from_min_size(Pos2::ZERO, Vec2::new(600., 400.)),
                    false,
                    std::slice::from_ref(&id),
                    false,
                );
                edges = frame.outlines[0].1.len();
                assert_eq!(
                    frame.pick(frame.outlines[0].1[0][0], std::slice::from_ref(&id)),
                    Some(id.clone())
                );
            });
        });
        assert_eq!(edges, 4);
        assert_eq!(scene, before);
        assert!(oxy_core::runtime::collider_box(&scene, &id).is_none());
    }
}
