//! Shared layout pieces: panel headers, cards, aligned property rows and segmented choices.
//! Panels build on these so spacing, alignment and emphasis stay identical across the editor.
use crate::icons::{self, Icon};
use crate::theme;
use egui::{Align, Color32, Layout, Rect, Response, RichText, Sense, Stroke, Ui, Vec2};
use std::hash::Hash;

/// Fixed-height title bar of a panel; `actions` are laid out from the right edge.
pub fn panel_header(ui: &mut Ui, title: &str, actions: impl FnOnce(&mut Ui)) {
    let size = Vec2::new(ui.available_width(), theme::PANEL_HEADER_HEIGHT);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let mut row = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(Vec2::new(4., 0.)))
            .layout(Layout::left_to_right(Align::Center)),
    );
    row.label(
        RichText::new(title.to_uppercase())
            .size(11.)
            .strong()
            .color(theme::TEXT_MUTED),
    );
    row.with_layout(Layout::right_to_left(Align::Center), actions);
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        Stroke::new(1., theme::BORDER),
    );
}

/// One choice among a few, drawn as a joined row of buttons.
pub fn segmented<T: PartialEq + Copy>(ui: &mut Ui, current: &mut T, items: &[(T, &str)]) -> bool {
    let mut changed = false;
    egui::Frame::new()
        .fill(theme::BG_BASE)
        .stroke(Stroke::new(1., theme::BORDER))
        .corner_radius(7)
        .inner_margin(2)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 2.;
            ui.horizontal(|ui| {
                for (value, label) in items {
                    let active = *current == *value;
                    let text = RichText::new(*label).color(if active {
                        Color32::WHITE
                    } else {
                        theme::TEXT_MUTED
                    });
                    let response = ui.add(
                        egui::Button::new(text)
                            .fill(if active {
                                theme::BG_HEADER
                            } else {
                                Color32::TRANSPARENT
                            })
                            .stroke(Stroke::NONE)
                            .corner_radius(5)
                            .min_size(Vec2::new(0., theme::CONTROL_HEIGHT)),
                    );
                    response.widget_info(|| {
                        egui::WidgetInfo::selected(
                            egui::WidgetType::SelectableLabel,
                            ui.is_enabled(),
                            active,
                            *label,
                        )
                    });
                    if response.clicked() && !active {
                        *current = *value;
                        changed = true;
                    }
                }
            });
        });
    changed
}

/// Underlined tabs with an optional count badge. Returns the tab clicked this frame,
/// including the active one.
pub fn tabs<T: PartialEq + Copy>(
    ui: &mut Ui,
    active: Option<T>,
    items: &[(T, &str, Option<usize>)],
) -> Option<T> {
    let mut clicked = None;
    ui.spacing_mut().item_spacing.x = 0.;
    for (value, label, count) in items {
        let selected = active == Some(*value);
        let color = if selected {
            theme::TEXT
        } else {
            theme::TEXT_MUTED
        };
        let font = egui::TextStyle::Button.resolve(ui.style());
        let text_width = ui
            .painter()
            .layout_no_wrap((*label).into(), font.clone(), color)
            .size()
            .x;
        let badge = count.map(|n| n.to_string());
        let badge_width = badge.as_ref().map_or(0., |b| 10. + 7. * b.len() as f32);
        let size = Vec2::new(
            12. + text_width + badge_width + 12.,
            theme::PANEL_HEADER_HEIGHT,
        );
        let (rect, response) = ui.allocate_exact_size(size, Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, *label)
        });
        let color = if response.hovered() {
            theme::TEXT
        } else {
            color
        };
        let y = rect.center().y;
        ui.painter().text(
            egui::pos2(rect.left() + 12., y),
            egui::Align2::LEFT_CENTER,
            *label,
            font,
            color,
        );
        if let Some(badge) = badge {
            let badge_rect = Rect::from_min_size(
                egui::pos2(rect.left() + 18. + text_width, y - 8.),
                Vec2::new(badge_width - 4., 16.),
            );
            ui.painter().rect_filled(badge_rect, 8., theme::BG_HEADER);
            ui.painter().text(
                badge_rect.center(),
                egui::Align2::CENTER_CENTER,
                badge,
                egui::FontId::proportional(11.),
                theme::TEXT_MUTED,
            );
        }
        if selected {
            ui.painter().hline(
                rect.x_range().shrink(6.),
                rect.bottom() - 1.,
                Stroke::new(2., theme::ACCENT),
            );
        }
        if response.clicked() {
            clicked = Some(*value);
        }
    }
    clicked
}

/// Button with an icon and a label. `primary` uses the accent fill; `compact` shows only
/// the icon, keeping the label as its accessible name and tooltip.
pub fn icon_text_button(
    ui: &mut Ui,
    icon: Icon,
    text: &str,
    primary: bool,
    compact: bool,
) -> Response {
    let font = egui::TextStyle::Button.resolve(ui.style());
    let text_width = ui
        .painter()
        .layout_no_wrap(text.into(), font.clone(), Color32::WHITE)
        .size()
        .x;
    let width = if compact { 30. } else { 34. + text_width + 12. };
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(width, theme::CONTROL_HEIGHT + 4.), Sense::click());
    icons::register_qa(ui, text, rect);
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), text));
    let visuals = ui.style().interact(&response);
    let (fill, stroke, fg) = if primary {
        let fill = if response.hovered() {
            theme::ACCENT.lerp_to_gamma(Color32::WHITE, 0.12)
        } else {
            theme::ACCENT
        };
        (fill, Stroke::NONE, Color32::WHITE)
    } else {
        (
            visuals.weak_bg_fill,
            visuals.bg_stroke,
            visuals.fg_stroke.color,
        )
    };
    ui.painter()
        .rect(rect, 6., fill, stroke, egui::StrokeKind::Inside);
    let icon_center = if compact {
        rect.center()
    } else {
        egui::pos2(rect.left() + 17., rect.center().y)
    };
    icons::paint(
        ui,
        Rect::from_center_size(icon_center, Vec2::splat(13.)),
        icon,
        fg,
    );
    if !compact {
        ui.painter().text(
            egui::pos2(rect.left() + 30., rect.center().y),
            egui::Align2::LEFT_CENTER,
            text,
            font,
            fg,
        );
        response
    } else {
        response
    }
}

/// Switch for on/off settings such as a component's "Ativo".
pub fn toggle(ui: &mut Ui, on: &mut bool, label: &str) -> Response {
    let size = Vec2::new(30., 18.);
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
    icons::register_qa(ui, label, rect);
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), *on, label)
    });
    let fill = if *on { theme::ACCENT } else { theme::BG_HEADER };
    ui.painter().rect_filled(rect, 9., fill);
    let x = if *on {
        rect.right() - 9.
    } else {
        rect.left() + 9.
    };
    ui.painter()
        .circle_filled(egui::pos2(x, rect.center().y), 6., Color32::WHITE);
    response.on_hover_text(label)
}

/// Collapsible inspector block, closed until opened. Open state is remembered per
/// `id_salt` for the session.
/// `enabled` adds a switch to the header; `menu` adds a "⋯" button with those actions.
pub fn card<R>(
    ui: &mut Ui,
    id_salt: impl Hash,
    title: &str,
    enabled: Option<&mut bool>,
    menu: Option<&mut dyn FnMut(&mut Ui)>,
    body: impl FnOnce(&mut Ui) -> R,
) -> Option<R> {
    let id = ui.make_persistent_id(id_salt);
    let mut state =
        egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false);
    let mut output = None;
    egui::Frame::new()
        .fill(theme::CARD_FILL)
        .stroke(Stroke::new(1., theme::CARD_BORDER))
        .corner_radius(theme::CARD_RADIUS)
        .inner_margin(egui::Margin {
            left: 6,
            right: 6,
            top: 2,
            bottom: 6,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.set_min_height(30.);
                let open = state.is_open();
                let chevron = if open {
                    Icon::ChevronDown
                } else {
                    Icon::ChevronRight
                };
                let toggle_label = if open {
                    format!("Recolher {title}")
                } else {
                    format!("Expandir {title}")
                };
                let mut clicked = icons::small(ui, chevron, &toggle_label, 20.).clicked();
                clicked |= ui
                    .add(egui::Label::new(RichText::new(title).strong()).sense(Sense::click()))
                    .clicked();
                if clicked {
                    state.set_open(!open);
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if let Some(menu) = menu {
                        let response =
                            icons::small(ui, Icon::More, &format!("Mais ações de {title}"), 24.);
                        egui::Popup::menu(&response).show(|ui| {
                            ui.set_min_width(200.);
                            menu(ui)
                        });
                    }
                    if let Some(enabled) = enabled {
                        toggle(ui, enabled, &format!("{title} ativo"));
                    }
                });
            });
            if state.is_open() {
                ui.add_space(2.);
                output = Some(body(ui));
            }
        });
    state.store(ui.ctx());
    output
}

/// Two-column row: a fixed label column and the field filling the rest.
pub fn property_row<R>(ui: &mut Ui, label: &str, hint: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.horizontal(|ui| {
        ui.set_min_height(theme::CONTROL_HEIGHT);
        let label_size = Vec2::new(theme::LABEL_COLUMN, theme::CONTROL_HEIGHT);
        let response = ui
            .allocate_ui_with_layout(label_size, Layout::left_to_right(Align::Center), |ui| {
                ui.set_width(theme::LABEL_COLUMN);
                ui.add(
                    egui::Label::new(RichText::new(label).size(12.).color(theme::TEXT_MUTED))
                        .truncate(),
                )
            })
            .inner;
        if !hint.is_empty() {
            response.on_hover_text(hint);
        }
        add(ui)
    })
    .inner
}

/// X/Y/Z fields in one row, each marked with its axis colour.
pub fn vec3_row(
    ui: &mut Ui,
    label: &str,
    hint: &str,
    values: &mut [f32; 3],
    speed: f64,
    nonzero: bool,
) -> bool {
    property_row(ui, label, hint, |ui| {
        let gap = 4.;
        ui.spacing_mut().item_spacing.x = gap;
        let width = ((ui.available_width() - 2. * gap) / 3.).max(36.);
        let mut changed = false;
        for (axis, value) in values.iter_mut().enumerate() {
            let response = ui.add_sized(
                [width, theme::CONTROL_HEIGHT],
                egui::DragValue::new(value)
                    .speed(speed)
                    .prefix(["X ", "Y ", "Z "][axis])
                    .max_decimals(3),
            );
            let strip = Rect::from_min_size(
                response.rect.left_top(),
                Vec2::new(2., response.rect.height()),
            );
            ui.painter().rect_filled(strip, 1., theme::AXES[axis]);
            changed |= response.changed();
            if nonzero && value.abs() < 0.0001 {
                *value = 0.0001;
            }
        }
        changed
    })
}

/// Muted explanatory text for empty states and short hints.
pub fn hint(ui: &mut Ui, text: &str) {
    ui.add(egui::Label::new(RichText::new(text).size(12.).color(theme::TEXT_MUTED)).wrap());
}
