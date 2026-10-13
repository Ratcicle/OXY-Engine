//! Small vector pictograms: no platform glyphs, emoji or texture assets.
use crate::theme;
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
    // Object kinds shown in the hierarchy and the inspector header.
    Person,
    Camera,
    Group,
    Cube,
    Sphere,
    Plane,
    Sprite,
    Interface,
    Sound,
    // Shell controls.
    Play,
    Pause,
    Stop,
    Undo,
    Redo,
    Eye,
    EyeOff,
    ChevronRight,
    ChevronDown,
    ChevronUp,
    Plus,
    // Animation.
    SkipStart,
    Keyframe,
    Flag,
    Clip,
    // Logic: node categories and the Lógica toolbar.
    Bolt,
    Data,
    Branch,
    Action,
    Flow,
    Game,
    Keyboard,
    Variables,
    Book,
    Check,
    Minus,
    Logic,
    Close,
    Trash,
    // Home screen.
    Folder,
    Search,
    Examples,
    Track,
    Console,
    // The test an agent plays.
    Agent,
}
pub fn width(ui: &mut Ui, label: &str, names: bool) -> f32 {
    if names {
        34. + ui
            .fonts_mut(|f| {
                f.layout_no_wrap(
                    label.into(),
                    egui::FontId::proportional(theme::LABEL_SIZE),
                    Color32::WHITE,
                )
            })
            .size()
            .x
    } else {
        theme::CONTROL_HEIGHT + 2.
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
    // A fixed side keeps the menu from jumping between alignments near the window edge:
    // buttons on the right half open towards the left.
    let align = if response.rect.center().x > ui.ctx().content_rect().center().x {
        egui::RectAlign::BOTTOM_END
    } else {
        egui::RectAlign::BOTTOM_START
    };
    // The menu scrolls within the space below its button, so it never has to be pushed
    // back inside the window and stays where it first opened.
    let below = ui.ctx().content_rect().bottom() - response.rect.bottom() - 28.;
    egui::Popup::menu(&response)
        .align(align)
        .align_alternatives(&[])
        .show(|ui| {
            ui.set_max_width(330.);
            egui::ScrollArea::vertical()
                .id_salt(("tool_menu", label))
                .max_height(below.max(120.))
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
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(width, theme::CONTROL_HEIGHT), Sense::click());
    register_qa(ui, label, rect);
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
        3.,
        visuals.bg_fill,
        visuals.bg_stroke,
        egui::StrokeKind::Inside,
    );
    let r = Rect::from_center_size(
        Pos2::new(rect.left() + 13., rect.center().y),
        Vec2::splat(theme::ICON_SIZE),
    );
    draw(ui, r, icon, visuals.fg_stroke.color);
    if names {
        ui.painter().text(
            Pos2::new(rect.left() + 26., rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(theme::LABEL_SIZE),
            visuals.fg_stroke.color,
        );
    }
    response.on_hover_text(tooltip)
}
/// Records an unlabeled control under its accessible name, so native QA can click it
/// by the same text a labelled button would show.
pub(crate) fn register_qa(ui: &Ui, label: &str, rect: Rect) {
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
    #[cfg(not(test))]
    let _ = (ui, label, rect);
}
/// Paints `icon` without interaction, for rows and headers that own their own response.
pub fn paint(ui: &Ui, rect: Rect, icon: Icon, color: Color32) {
    draw(ui, rect, icon, color);
}
/// Icon-only button of `size` points with an accessible name and tooltip.
pub fn small(ui: &mut Ui, icon: Icon, label: &str, size: f32) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    register_qa(ui, label, rect);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    let visuals = ui.style().interact(&response);
    if response.hovered() || response.has_focus() {
        ui.painter().rect_filled(rect, 4., visuals.weak_bg_fill);
    }
    let inner = Rect::from_center_size(rect.center(), Vec2::splat((size - 8.).max(10.)));
    draw(ui, inner, icon, visuals.fg_stroke.color);
    response.on_hover_text(label)
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
                p.line_segment([points[1], points[2]], Stroke::new(2.5, theme::HIGHLIGHT));
            }
            if matches!(icon, Icon::Vertex) {
                p.circle_filled(points[1], 2.6, theme::HIGHLIGHT);
            }
        }
        Icon::Person => {
            p.circle_stroke(at(0.5, 0.28), r.width() * 0.19, Stroke::new(1.3, color));
            p.add(egui::Shape::line(
                (0..=12)
                    .map(|i| {
                        let a = std::f32::consts::PI * (1. + i as f32 / 12.);
                        at(0.5 + 0.34 * a.cos(), 0.94 + 0.4 * a.sin())
                    })
                    .collect(),
                Stroke::new(1.3, color),
            ));
        }
        Icon::Camera => {
            p.rect_stroke(
                Rect::from_min_max(at(0.06, 0.3), at(0.66, 0.78)),
                1.5,
                Stroke::new(1.3, color),
                egui::StrokeKind::Inside,
            );
            p.add(egui::Shape::closed_line(
                vec![
                    at(0.68, 0.46),
                    at(0.94, 0.3),
                    at(0.94, 0.78),
                    at(0.68, 0.62),
                ],
                Stroke::new(1.3, color),
            ));
        }
        Icon::Group => {
            p.add(egui::Shape::closed_line(
                vec![
                    at(0.06, 0.24),
                    at(0.4, 0.24),
                    at(0.5, 0.36),
                    at(0.94, 0.36),
                    at(0.94, 0.84),
                    at(0.06, 0.84),
                ],
                Stroke::new(1.3, color),
            ));
        }
        Icon::Cube => {
            let v = [
                at(0.5, 0.06),
                at(0.92, 0.28),
                at(0.92, 0.72),
                at(0.5, 0.94),
                at(0.08, 0.72),
                at(0.08, 0.28),
                at(0.5, 0.5),
            ];
            p.add(egui::Shape::closed_line(
                v[..6].to_vec(),
                Stroke::new(1.3, color),
            ));
            for i in [5, 1, 3] {
                line(v[6], v[i]);
            }
        }
        Icon::Sphere => {
            p.circle_stroke(r.center(), r.width() * 0.42, Stroke::new(1.3, color));
            p.add(egui::Shape::line(
                (0..=12)
                    .map(|i| {
                        let a = std::f32::consts::PI * i as f32 / 12.;
                        at(0.5 - 0.42 * a.cos(), 0.5 + 0.14 * a.sin())
                    })
                    .collect(),
                Stroke::new(1.1, color),
            ));
        }
        Icon::Plane => {
            p.add(egui::Shape::closed_line(
                vec![at(0.04, 0.72), at(0.3, 0.32), at(0.96, 0.32), at(0.7, 0.72)],
                Stroke::new(1.3, color),
            ));
        }
        Icon::Sprite => {
            p.rect_stroke(
                Rect::from_min_max(at(0.08, 0.12), at(0.92, 0.88)),
                1.5,
                Stroke::new(1.3, color),
                egui::StrokeKind::Inside,
            );
            p.circle_filled(at(0.34, 0.38), r.width() * 0.08, color);
            p.add(egui::Shape::line(
                vec![at(0.12, 0.8), at(0.42, 0.56), at(0.62, 0.7), at(0.88, 0.48)],
                Stroke::new(1.2, color),
            ));
        }
        Icon::Interface => {
            p.rect_stroke(
                Rect::from_min_max(at(0.06, 0.16), at(0.94, 0.84)),
                1.5,
                Stroke::new(1.3, color),
                egui::StrokeKind::Inside,
            );
            p.rect_filled(Rect::from_min_max(at(0.2, 0.42), at(0.62, 0.58)), 1., color);
        }
        Icon::Sound => {
            for (x, h) in [
                (0.12, 0.16),
                (0.32, 0.42),
                (0.52, 0.7),
                (0.72, 0.36),
                (0.9, 0.14),
            ] {
                line(at(x, 0.5 - h / 2.), at(x, 0.5 + h / 2.));
            }
        }
        Icon::Play => {
            p.add(egui::Shape::convex_polygon(
                vec![at(0.22, 0.1), at(0.9, 0.5), at(0.22, 0.9)],
                color,
                Stroke::NONE,
            ));
        }
        Icon::Pause => {
            for x in [0.22, 0.6] {
                p.rect_filled(
                    Rect::from_min_max(at(x, 0.12), at(x + 0.18, 0.88)),
                    1.,
                    color,
                );
            }
        }
        Icon::Stop => {
            p.rect_filled(
                Rect::from_min_max(at(0.16, 0.16), at(0.84, 0.84)),
                1.5,
                color,
            );
        }
        Icon::Undo | Icon::Redo => {
            let undo = matches!(icon, Icon::Undo);
            let flip = |x: f32| if undo { x } else { 1. - x };
            let arc: Vec<_> = (0..=12)
                .map(|i| {
                    let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * i as f32 / 12.;
                    at(flip(0.55 + 0.32 * a.cos()), 0.62 + 0.28 * a.sin())
                })
                .collect();
            p.add(egui::Shape::line(arc, Stroke::new(1.4, color)));
            line(at(flip(0.55), 0.34), at(flip(0.1), 0.34));
            line(at(flip(0.1), 0.34), at(flip(0.3), 0.14));
            line(at(flip(0.1), 0.34), at(flip(0.3), 0.54));
            line(at(flip(0.55), 0.9), at(flip(0.3), 0.9));
        }
        Icon::Eye | Icon::EyeOff => {
            let upper: Vec<_> = (0..=12)
                .map(|i| {
                    let t = i as f32 / 12.;
                    at(
                        0.04 + 0.92 * t,
                        0.5 - 0.3 * (std::f32::consts::PI * t).sin(),
                    )
                })
                .collect();
            let lower: Vec<_> = upper
                .iter()
                .map(|q| Pos2::new(q.x, 2. * r.center().y - q.y))
                .collect();
            p.add(egui::Shape::line(upper, Stroke::new(1.3, color)));
            p.add(egui::Shape::line(lower, Stroke::new(1.3, color)));
            p.circle_stroke(r.center(), r.width() * 0.13, Stroke::new(1.3, color));
            if matches!(icon, Icon::EyeOff) {
                p.line_segment([at(0.12, 0.88), at(0.88, 0.12)], Stroke::new(1.5, color));
            }
        }
        Icon::ChevronRight => {
            p.add(egui::Shape::line(
                vec![at(0.36, 0.2), at(0.66, 0.5), at(0.36, 0.8)],
                Stroke::new(1.5, color),
            ));
        }
        Icon::ChevronDown => {
            p.add(egui::Shape::line(
                vec![at(0.2, 0.36), at(0.5, 0.66), at(0.8, 0.36)],
                Stroke::new(1.5, color),
            ));
        }
        Icon::ChevronUp => {
            p.add(egui::Shape::line(
                vec![at(0.2, 0.64), at(0.5, 0.34), at(0.8, 0.64)],
                Stroke::new(1.5, color),
            ));
        }
        Icon::SkipStart => {
            p.line_segment([at(0.2, 0.15), at(0.2, 0.85)], Stroke::new(1.6, color));
            p.add(egui::Shape::convex_polygon(
                vec![at(0.86, 0.15), at(0.86, 0.85), at(0.3, 0.5)],
                color,
                Stroke::NONE,
            ));
        }
        Icon::Keyframe => {
            p.add(egui::Shape::convex_polygon(
                vec![at(0.5, 0.1), at(0.9, 0.5), at(0.5, 0.9), at(0.1, 0.5)],
                color,
                Stroke::NONE,
            ));
        }
        Icon::Flag => {
            p.line_segment([at(0.22, 0.08), at(0.22, 0.95)], Stroke::new(1.4, color));
            p.add(egui::Shape::convex_polygon(
                vec![at(0.22, 0.1), at(0.88, 0.32), at(0.22, 0.54)],
                color.gamma_multiply(0.45),
                Stroke::new(1.2, color),
            ));
        }
        Icon::Clip => {
            p.rect_stroke(
                Rect::from_min_max(at(0.06, 0.18), at(0.94, 0.82)),
                2.,
                Stroke::new(1.3, color),
                egui::StrokeKind::Inside,
            );
            p.add(egui::Shape::convex_polygon(
                vec![at(0.4, 0.34), at(0.66, 0.5), at(0.4, 0.66)],
                color,
                Stroke::NONE,
            ));
        }
        Icon::Plus => {
            p.line_segment([at(0.5, 0.14), at(0.5, 0.86)], Stroke::new(1.5, color));
            p.line_segment([at(0.14, 0.5), at(0.86, 0.5)], Stroke::new(1.5, color));
        }
        Icon::Minus => {
            p.line_segment([at(0.14, 0.5), at(0.86, 0.5)], Stroke::new(1.5, color));
        }
        Icon::Close => {
            p.line_segment([at(0.2, 0.2), at(0.8, 0.8)], Stroke::new(1.5, color));
            p.line_segment([at(0.8, 0.2), at(0.2, 0.8)], Stroke::new(1.5, color));
        }
        Icon::Check => {
            p.add(egui::Shape::line(
                vec![at(0.12, 0.52), at(0.4, 0.8), at(0.9, 0.2)],
                Stroke::new(1.6, color),
            ));
        }
        Icon::Bolt => {
            p.add(egui::Shape::closed_line(
                vec![
                    at(0.6, 0.04),
                    at(0.16, 0.56),
                    at(0.46, 0.56),
                    at(0.38, 0.96),
                    at(0.84, 0.42),
                    at(0.54, 0.42),
                ],
                Stroke::new(1.3, color),
            ));
        }
        Icon::Data => {
            use std::f32::consts::{PI, TAU};
            let ellipse = |y: f32, from: f32, to: f32| -> Vec<Pos2> {
                (0..=16)
                    .map(|i| {
                        let a = from + (to - from) * i as f32 / 16.;
                        at(0.5 + 0.38 * a.cos(), y + 0.13 * a.sin())
                    })
                    .collect()
            };
            p.add(egui::Shape::closed_line(
                ellipse(0.22, 0., TAU),
                Stroke::new(1.3, color),
            ));
            for y in [0.5, 0.78] {
                p.add(egui::Shape::line(
                    ellipse(y, 0., PI),
                    Stroke::new(1.3, color),
                ));
            }
            line(at(0.12, 0.22), at(0.12, 0.78));
            line(at(0.88, 0.22), at(0.88, 0.78));
        }
        Icon::Branch => {
            line(at(0.5, 0.95), at(0.5, 0.55));
            for x in [0.14, 0.86] {
                p.arrow(
                    at(0.5, 0.55),
                    at(x, 0.1) - at(0.5, 0.55),
                    Stroke::new(1.3, color),
                );
            }
        }
        Icon::Action => {
            p.rect_stroke(
                Rect::from_min_max(at(0.08, 0.08), at(0.92, 0.92)),
                3.,
                Stroke::new(1.3, color),
                egui::StrokeKind::Inside,
            );
            p.add(egui::Shape::closed_line(
                vec![at(0.4, 0.3), at(0.7, 0.5), at(0.4, 0.7)],
                Stroke::new(1.3, color),
            ));
        }
        Icon::Flow => {
            line(at(0.14, 0.1), at(0.14, 0.9));
            for y in [0.2, 0.5, 0.8] {
                p.arrow(
                    at(0.14, y),
                    at(0.92, y) - at(0.14, y),
                    Stroke::new(1.3, color),
                );
            }
        }
        Icon::Game => {
            // A gamepad drawn on a 14-unit grid: body and grips, a cross and a button.
            let unit = |x: f32, y: f32| at(x / 14., y / 14.);
            let arc = |cx: f32, cy: f32, radius: f32, from: f32, to: f32| -> Vec<Pos2> {
                (0..=6)
                    .map(|i| {
                        let a = (from + (to - from) * i as f32 / 6.).to_radians();
                        unit(cx + radius * a.cos(), cy + radius * a.sin())
                    })
                    .collect()
            };
            let mut body = arc(10., 7.5, 3., -90., 0.);
            body.extend(arc(11.2, 8.67, 1.8, 0., 141.));
            body.extend([unit(8.8, 8.5), unit(5.2, 8.5)]);
            body.extend(arc(2.8, 8.67, 1.8, 39., 180.));
            body.extend(arc(4., 7.5, 3., 180., 270.));
            p.add(egui::Shape::closed_line(body, Stroke::new(1.3, color)));
            line(unit(4.3, 6.), unit(4.3, 7.8));
            line(unit(3.4, 6.9), unit(5.2, 6.9));
            p.circle_filled(unit(9.8, 6.8), r.width() * 0.08, color);
        }
        Icon::Keyboard => {
            p.rect_stroke(
                Rect::from_min_max(at(0.04, 0.22), at(0.96, 0.8)),
                2.,
                Stroke::new(1.3, color),
                egui::StrokeKind::Inside,
            );
            for x in [0.24, 0.41, 0.58, 0.75] {
                p.circle_filled(at(x, 0.42), 0.9, color);
            }
            line(at(0.3, 0.62), at(0.7, 0.62));
        }
        Icon::Variables => {
            // Curly braces on a 16-unit grid; the right one mirrors the left.
            let left = [
                (5.5, 2.5),
                (4.3, 2.8),
                (3.7, 3.5),
                (3.5, 4.5),
                (3.5, 6.1),
                (3.1, 7.3),
                (2.1, 8.),
                (3.1, 8.7),
                (3.5, 9.9),
                (3.5, 11.5),
                (3.7, 12.5),
                (4.3, 13.2),
                (5.5, 13.5),
            ];
            for mirrored in [false, true] {
                let points = left
                    .iter()
                    .map(|&(x, y)| at(if mirrored { 16. - x } else { x } / 16., y / 16.))
                    .collect();
                p.add(egui::Shape::line(points, Stroke::new(1.3, color)));
            }
        }
        Icon::Book => {
            p.add(egui::Shape::closed_line(
                vec![at(0.5, 0.22), at(0.5, 0.9), at(0.06, 0.8), at(0.06, 0.12)],
                Stroke::new(1.3, color),
            ));
            p.add(egui::Shape::closed_line(
                vec![at(0.5, 0.22), at(0.94, 0.12), at(0.94, 0.8), at(0.5, 0.9)],
                Stroke::new(1.3, color),
            ));
        }
        Icon::Logic => {
            for rect in [
                Rect::from_min_max(at(0.04, 0.12), at(0.4, 0.42)),
                Rect::from_min_max(at(0.6, 0.58), at(0.96, 0.88)),
            ] {
                p.rect_stroke(rect, 1.5, Stroke::new(1.3, color), egui::StrokeKind::Inside);
            }
            p.add(egui::epaint::CubicBezierShape::from_points_stroke(
                [at(0.4, 0.27), at(0.62, 0.27), at(0.38, 0.73), at(0.6, 0.73)],
                false,
                Color32::TRANSPARENT,
                Stroke::new(1.3, color),
            ));
        }
        Icon::Trash => {
            line(at(0.1, 0.22), at(0.9, 0.22));
            p.add(egui::Shape::line(
                vec![at(0.36, 0.22), at(0.4, 0.08), at(0.6, 0.08), at(0.64, 0.22)],
                Stroke::new(1.3, color),
            ));
            p.add(egui::Shape::line(
                vec![at(0.2, 0.22), at(0.26, 0.94), at(0.74, 0.94), at(0.8, 0.22)],
                Stroke::new(1.3, color),
            ));
            line(at(0.42, 0.4), at(0.42, 0.78));
            line(at(0.58, 0.4), at(0.58, 0.78));
        }
        Icon::Folder => {
            p.add(egui::Shape::closed_line(
                vec![
                    at(0.06, 0.2),
                    at(0.38, 0.2),
                    at(0.48, 0.32),
                    at(0.94, 0.32),
                    at(0.94, 0.86),
                    at(0.06, 0.86),
                ],
                Stroke::new(1.3, color),
            ));
        }
        Icon::Search => {
            p.circle_stroke(at(0.42, 0.42), r.width() * 0.3, Stroke::new(1.4, color));
            p.line_segment([at(0.64, 0.64), at(0.92, 0.92)], Stroke::new(1.6, color));
        }
        Icon::Examples => {
            for (x, y) in [(0.08, 0.08), (0.56, 0.08), (0.08, 0.56), (0.56, 0.56)] {
                p.rect_stroke(
                    Rect::from_min_max(at(x, y), at(x + 0.36, y + 0.36)),
                    1.,
                    Stroke::new(1.3, color),
                    egui::StrokeKind::Inside,
                );
            }
        }
        Icon::Track => {
            p.add(egui::Shape::line(
                vec![
                    at(0.06, 0.8),
                    at(0.28, 0.8),
                    at(0.5, 0.46),
                    at(0.7, 0.46),
                    at(0.94, 0.16),
                ],
                Stroke::new(1.3, color),
            ));
            p.circle_stroke(at(0.3, 0.58), r.width() * 0.09, Stroke::new(1.3, color));
            line(at(0.66, 0.8), at(0.94, 0.8));
        }
        Icon::Console => {
            p.rect_stroke(
                Rect::from_min_max(at(0.06, 0.14), at(0.94, 0.86)),
                1.5,
                Stroke::new(1.3, color),
                egui::StrokeKind::Inside,
            );
            p.add(egui::Shape::line(
                vec![at(0.24, 0.36), at(0.4, 0.5), at(0.24, 0.64)],
                Stroke::new(1.3, color),
            ));
            line(at(0.5, 0.66), at(0.74, 0.66));
        }
        Icon::Agent => {
            // A robot head on a 14-unit grid: the face, an antenna and two eyes.
            let unit = |x: f32, y: f32| at(x / 14., y / 14.);
            p.rect_stroke(
                Rect::from_min_max(unit(2.5, 4.), unit(11.5, 11.)),
                2.,
                Stroke::new(1.3, color),
                egui::StrokeKind::Middle,
            );
            line(unit(7., 1.8), unit(7., 4.));
            for x in [5., 9.] {
                p.circle_filled(unit(x, 7.2), r.width() * 0.07, color);
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
