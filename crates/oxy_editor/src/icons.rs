//! Small vector pictograms: no platform glyphs, emoji or texture assets.
use egui::{Color32, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2};
#[derive(Clone, Copy)]
pub enum Icon {
    Object,
    Face,
    Edge,
    Vertex,
    Move,
    Rotate,
    Scale,
    Extrude,
    Create,
    Flip,
    Snap,
    Loop,
    Knife,
    Bevel,
    Collider,
    Pivot,
}
pub fn button(
    ui: &mut Ui,
    icon: Icon,
    label: &str,
    tooltip: &str,
    selected: bool,
    names: bool,
) -> Response {
    let width = if names {
        32. + ui
            .fonts_mut(|f| {
                f.layout_no_wrap(
                    label.into(),
                    egui::FontId::proportional(13.),
                    Color32::WHITE,
                )
            })
            .size()
            .x
            + 8.
    } else {
        32.
    };
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 30.), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            ui.is_enabled(),
            selected,
            label,
        )
    });
    let visuals = ui.style().interact_selectable(&response, selected);
    ui.painter().rect(
        rect,
        4.,
        visuals.bg_fill,
        visuals.bg_stroke,
        egui::StrokeKind::Inside,
    );
    let r = Rect::from_center_size(
        Pos2::new(rect.left() + 16., rect.center().y),
        Vec2::splat(20.),
    );
    draw(ui, r, icon, visuals.fg_stroke.color);
    if names {
        ui.painter().text(
            Pos2::new(rect.left() + 31., rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(13.),
            visuals.fg_stroke.color,
        );
    }
    response.on_hover_text(tooltip)
}
fn draw(ui: &Ui, r: Rect, icon: Icon, color: Color32) {
    let p = ui.painter();
    let at = |x: f32, y: f32| r.min + Vec2::new(x, y) * r.size();
    let line = |a, b| {
        p.line_segment([a, b], Stroke::new(1.3, color));
    };
    match icon {
        Icon::Collider => {
            let box_rect = Rect::from_min_max(at(0.1, 0.15), at(0.9, 0.85));
            p.rect_stroke(
                box_rect,
                0.,
                Stroke::new(1.5, color),
                egui::StrokeKind::Inside,
            );
            for q in [
                box_rect.left_top(),
                box_rect.left_bottom(),
                box_rect.right_top(),
                box_rect.right_bottom(),
            ] {
                p.rect_filled(Rect::from_center_size(q, Vec2::splat(4.)), 0., color);
            }
            line(at(0.36, 0.5), at(0.64, 0.5));
            line(at(0.5, 0.36), at(0.5, 0.64));
        }
        Icon::Pivot => {
            p.circle_stroke(r.center(), 5., Stroke::new(1.4, color));
            p.circle_filled(r.center(), 2., color);
            p.arrow(
                at(0.5, 0.9),
                at(0.5, 0.04) - at(0.5, 0.9),
                Stroke::new(1.3, color),
            );
            p.arrow(
                at(0.1, 0.5),
                at(0.96, 0.5) - at(0.1, 0.5),
                Stroke::new(1.3, color),
            );
        }
        Icon::Loop => {
            for x in [0.1, 0.9] {
                line(at(x, 0.1), at(x, 0.9));
            }
            for y in [0.1, 0.9] {
                line(at(0.1, y), at(0.9, y));
            }
            p.line_segment([at(0., 0.5), at(1., 0.5)], Stroke::new(2., color));
        }
        Icon::Knife => {
            p.add(egui::Shape::convex_polygon(
                vec![at(0.1, 0.95), at(0.38, 0.25), at(0.62, 0.5)],
                color.gamma_multiply(0.6),
                Stroke::new(1., color),
            ));
            line(at(0.5, 0.35), at(0.86, 0.05));
            line(at(0.58, 0.43), at(0.96, 0.1));
        }
        Icon::Bevel => {
            line(at(0.1, 0.95), at(0.1, 0.1));
            line(at(0.1, 0.1), at(0.95, 0.1));
            let points = (0..=12)
                .map(|i| {
                    let a = i as f32 / 12. * std::f32::consts::FRAC_PI_2;
                    at(0.6 - 0.5 * a.cos(), 0.6 - 0.5 * a.sin())
                })
                .collect();
            p.add(egui::Shape::line(points, Stroke::new(2., color)));
        }
        Icon::Extrude => {
            for y in [0.45, 0.9] {
                p.rect_stroke(
                    Rect::from_min_max(at(0.1, y - 0.3), at(0.7, y)),
                    0.,
                    Stroke::new(1.3, color),
                    egui::StrokeKind::Inside,
                );
            }
            line(at(0.1, 0.45), at(0.1, 0.9));
            line(at(0.7, 0.45), at(0.7, 0.9));
            p.arrow(
                at(0.9, 0.85),
                at(0.9, 0.05) - at(0.9, 0.85),
                Stroke::new(1.5, color),
            );
        }
        Icon::Create => {
            let v = [at(0.1, 0.85), at(0.8, 0.85), at(0.7, 0.1)];
            for i in 0..3 {
                line(v[i], v[(i + 1) % 3]);
                p.circle_filled(v[i], 2., color);
            }
            line(at(0.1, 0.2), at(0.4, 0.2));
            line(at(0.25, 0.05), at(0.25, 0.35));
        }
        Icon::Flip => {
            line(at(0.5, 0.15), at(0.5, 0.85));
            for (a, b) in [(at(0.1, 0.3), at(0.9, 0.3)), (at(0.9, 0.7), at(0.1, 0.7))] {
                p.arrow(a, b - a, Stroke::new(1.4, color));
            }
        }
        Icon::Snap => {
            p.circle_stroke(at(0.15, 0.75), 3., Stroke::new(1.4, color));
            p.circle_filled(at(0.8, 0.25), 3., color);
            p.arrow(
                at(0.3, 0.65),
                at(0.7, 0.35) - at(0.3, 0.65),
                Stroke::new(1.4, color),
            );
        }
        Icon::Object | Icon::Face | Icon::Edge | Icon::Vertex => {
            let points = [
                at(0.05, 0.25),
                at(0.65, 0.4),
                at(0.65, 0.95),
                at(0.05, 0.8),
                at(0.38, 0.02),
                at(0.98, 0.18),
                at(0.98, 0.72),
            ];
            if matches!(icon, Icon::Face) {
                p.add(egui::Shape::convex_polygon(
                    vec![points[0], points[1], points[2], points[3]],
                    color.gamma_multiply(0.65),
                    Stroke::NONE,
                ));
            }
            for [a, b] in [
                [0, 1],
                [1, 2],
                [2, 3],
                [3, 0],
                [0, 4],
                [4, 5],
                [5, 6],
                [6, 2],
                [5, 1],
            ] {
                line(points[a], points[b]);
            }
            if matches!(icon, Icon::Edge) {
                p.line_segment([points[1], points[2]], Stroke::new(3., Color32::GOLD));
            }
            if matches!(icon, Icon::Vertex) {
                p.circle_filled(points[1], 3.2, Color32::GOLD);
            }
        }
        Icon::Move => {
            for end in [at(0.5, 0.), at(1., 0.5), at(0.5, 1.), at(0., 0.5)] {
                p.arrow(r.center(), end - r.center(), Stroke::new(1.5, color));
            }
        }
        Icon::Rotate => {
            let points: Vec<_> = (0..=18)
                .map(|i| {
                    let a = i as f32 / 18. * 5.3;
                    r.center() + Vec2::angled(a) * 8.
                })
                .collect();
            p.add(egui::Shape::line(points.clone(), Stroke::new(1.5, color)));
            p.arrow(points[17], points[18] - points[17], Stroke::new(1.5, color));
        }
        Icon::Scale => {
            p.rect_stroke(
                Rect::from_min_max(at(0.03, 0.55), at(0.45, 0.97)),
                0.,
                Stroke::new(1.3, color),
                egui::StrokeKind::Inside,
            );
            p.arrow(
                at(0.4, 0.6),
                at(0.93, 0.07) - at(0.4, 0.6),
                Stroke::new(1.5, color),
            );
        }
    }
}
