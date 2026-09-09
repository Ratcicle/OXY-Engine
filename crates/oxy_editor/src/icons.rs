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
