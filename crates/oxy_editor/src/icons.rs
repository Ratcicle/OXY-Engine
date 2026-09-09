//! Small vector pictograms: no platform glyphs, emoji or texture assets.
use egui::{Color32, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2};
#[cfg(test)]
static QA_CONTROLS_ENABLED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
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
    Inset,
    View,
    Frame,
    More,
    Help,
    Brush,
    Fill,
    Sample,
    PaintSelect,
    TextureNew,
    Import,
    Export,
}
pub fn width(ui: &mut Ui, label: &str, names: bool) -> f32 {
    if names {
        40. + ui
            .fonts_mut(|f| {
                f.layout_no_wrap(
                    label.into(),
                    egui::FontId::proportional(13.),
                    Color32::WHITE,
                )
            })
            .size()
            .x
    } else {
        32.
    }
}
pub fn menu_button(
    ui: &mut Ui,
    icon: Icon,
    label: &str,
    tooltip: &str,
    names: bool,
    contents: impl FnOnce(&mut Ui),
) {
    if egui::containers::menu::is_in_menu(ui) {
        // A nested root popup would close its parent. Use egui's real submenu state.
        let response = button(ui, icon, label, tooltip, false, true);
        egui::containers::menu::SubMenu::default().show(ui, &response, |ui| {
            egui::ScrollArea::vertical()
                .id_salt(("tool_menu", label))
                .max_height((ui.ctx().content_rect().height() - 40.).max(120.))
                .show(ui, contents);
        });
        return;
    }
    let response = button(ui, icon, label, tooltip, false, names);
    egui::Popup::menu(&response).show(|ui| {
        ui.set_max_width(330.);
        egui::ScrollArea::vertical()
            .id_salt(("tool_menu", label))
            .max_height((ui.ctx().content_rect().height() - 40.).max(120.))
            .show(ui, contents);
    });
}
pub fn button(
    ui: &mut Ui,
    icon: Icon,
    label: &str,
    tooltip: &str,
    selected: bool,
    names: bool,
) -> Response {
    let width = width(ui, label, names);
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 30.), Sense::click());
    #[cfg(test)]
    if QA_CONTROLS_ENABLED.load(std::sync::atomic::Ordering::Relaxed) {
        let frame = ui.ctx().cumulative_frame_nr();
        ui.ctx().data_mut(|data| {
            let controls = data.get_temp_mut_or_default::<(u64, Vec<(String, Rect)>)>(
                egui::Id::new("oxy_native_icon_controls"),
            );
            if controls.0 != frame {
                controls.0 = frame;
                controls.1.clear();
            }
            controls.1.push((label.to_owned(), rect));
        });
    }
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
/// Native QA uses the real response allocation, including unlabeled accessible icons.
#[cfg(test)]
pub(crate) fn qa_control_rect(ctx: &egui::Context, label: &str) -> Option<Rect> {
    // Benchmarks use a different host and never enable the semantic QA registry.
    QA_CONTROLS_ENABLED.store(true, std::sync::atomic::Ordering::Relaxed);
    ctx.data(|data| {
        data.get_temp::<(u64, Vec<(String, Rect)>)>(egui::Id::new("oxy_native_icon_controls"))
    })
    .filter(|(frame, _)| *frame == ctx.cumulative_frame_nr())
    .and_then(|(_, controls)| {
        controls
            .into_iter()
            .rev()
            .find(|(name, _)| name == label)
            .map(|(_, rect)| rect)
    })
}
fn draw(ui: &Ui, r: Rect, icon: Icon, color: Color32) {
    let p = ui.painter();
    let at = |x: f32, y: f32| r.min + Vec2::new(x, y) * r.size();
    let line = |a, b| {
        p.line_segment([a, b], Stroke::new(1.3, color));
    };
    match icon {
        Icon::Inset => {
            for inset in [0.08, 0.31] {
                p.rect_stroke(
                    Rect::from_min_max(at(inset, inset), at(1. - inset, 1. - inset)),
                    0.,
                    Stroke::new(1.4, color),
                    egui::StrokeKind::Inside,
                );
            }
            for (x, y) in [(0.08, 0.08), (0.92, 0.08), (0.92, 0.92), (0.08, 0.92)] {
                line(
                    at(x, y),
                    at(
                        if x < 0.5 { 0.31 } else { 0.69 },
                        if y < 0.5 { 0.31 } else { 0.69 },
                    ),
                );
            }
        }
        Icon::View => {
            p.add(egui::Shape::line(
                vec![
                    at(0.02, 0.5),
                    at(0.26, 0.25),
                    at(0.5, 0.15),
                    at(0.74, 0.25),
                    at(0.98, 0.5),
                    at(0.74, 0.75),
                    at(0.5, 0.85),
                    at(0.26, 0.75),
                    at(0.02, 0.5),
                ],
                Stroke::new(1.3, color),
            ));
            p.circle_stroke(r.center(), 3.2, Stroke::new(1.5, color));
        }
        Icon::Frame => {
            for (x, y, sx, sy) in [
                (0.1, 0.1, 1., 1.),
                (0.9, 0.1, -1., 1.),
                (0.1, 0.9, 1., -1.),
                (0.9, 0.9, -1., -1.),
            ] {
                line(at(x, y), at(x + sx * 0.25, y));
                line(at(x, y), at(x, y + sy * 0.25));
            }
            p.circle_stroke(r.center(), 3., Stroke::new(1.2, color));
        }
        Icon::More => {
            for x in [0.2, 0.5, 0.8] {
                p.circle_filled(at(x, 0.5), 1.8, color);
            }
        }
        Icon::Help => {
            p.circle_stroke(r.center(), 8., Stroke::new(1.4, color));
            p.text(
                r.center(),
                egui::Align2::CENTER_CENTER,
                "?",
                egui::FontId::proportional(15.),
                color,
            );
        }
        Icon::Brush => {
            line(at(0.35, 0.67), at(0.8, 0.08));
            line(at(0.49, 0.77), at(0.94, 0.2));
            line(at(0.8, 0.08), at(0.94, 0.2));
            p.add(egui::Shape::convex_polygon(
                vec![
                    at(0.35, 0.62),
                    at(0.57, 0.79),
                    at(0.36, 0.94),
                    at(0.04, 0.96),
                    at(0.2, 0.83),
                ],
                color,
                Stroke::NONE,
            ));
        }
        Icon::Fill => {
            p.add(egui::Shape::closed_line(
                vec![
                    at(0.12, 0.48),
                    at(0.46, 0.14),
                    at(0.81, 0.48),
                    at(0.46, 0.84),
                ],
                Stroke::new(1.4, color),
            ));
            line(at(0.19, 0.5), at(0.74, 0.5));
            line(at(0.34, 0.26), at(0.2, 0.08));
            p.add(egui::Shape::convex_polygon(
                vec![at(0.88, 0.53), at(0.76, 0.85), at(0.88, 0.96), at(1., 0.85)],
                color,
                Stroke::NONE,
            ));
        }
        Icon::Sample => {
            p.add(egui::Shape::closed_line(
                vec![
                    at(0.13, 0.83),
                    at(0.63, 0.27),
                    at(0.77, 0.4),
                    at(0.25, 0.95),
                    at(0.05, 0.97),
                ],
                Stroke::new(1.4, color),
            ));
            line(at(0.54, 0.22), at(0.84, 0.49));
            p.circle_filled(at(0.84, 0.16), 3.3, color);
        }
        Icon::PaintSelect => {
            p.rect_stroke(
                Rect::from_min_max(at(0.08, 0.1), at(0.78, 0.76)),
                0.,
                Stroke::new(1.2, color),
                egui::StrokeKind::Inside,
            );
            p.add(egui::Shape::convex_polygon(
                vec![
                    at(0.44, 0.43),
                    at(0.62, 0.96),
                    at(0.73, 0.76),
                    at(0.98, 0.72),
                ],
                color,
                Stroke::new(1., color),
            ));
        }
        Icon::TextureNew => {
            p.rect_stroke(
                Rect::from_min_max(at(0.08, 0.08), at(0.76, 0.78)),
                0.,
                Stroke::new(1.3, color),
                egui::StrokeKind::Inside,
            );
            for (x, y) in [(0.13, 0.13), (0.43, 0.43)] {
                p.rect_filled(
                    Rect::from_min_size(at(x, y), r.size() * 0.27),
                    0.,
                    color.gamma_multiply(0.5),
                );
            }
            line(at(0.68, 0.82), at(1., 0.82));
            line(at(0.84, 0.66), at(0.84, 0.98));
        }
        Icon::Import | Icon::Export => {
            p.add(egui::Shape::line(
                vec![at(0.1, 0.62), at(0.1, 0.91), at(0.9, 0.91), at(0.9, 0.62)],
                Stroke::new(1.4, color),
            ));
            let (a, b) = if matches!(icon, Icon::Import) {
                (at(0.5, 0.04), at(0.5, 0.71))
            } else {
                (at(0.5, 0.71), at(0.5, 0.04))
            };
            p.arrow(a, b - a, Stroke::new(1.6, color));
        }
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
