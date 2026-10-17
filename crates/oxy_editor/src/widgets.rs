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
            .font(theme::medium(11.))
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

/// Segmented choice whose items carry an icon. Without `names` only icons are shown; the
/// label stays the accessible name and the tooltip carries the full description.
pub fn segmented_icons<T: PartialEq + Copy>(
    ui: &mut Ui,
    current: &mut T,
    items: &[(T, Icon, &str, &str)],
    names: bool,
) -> bool {
    let mut changed = false;
    egui::Frame::new()
        .fill(theme::BG_BASE)
        .stroke(Stroke::new(1., theme::BORDER))
        .corner_radius(7)
        .inner_margin(2)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 2.;
            ui.horizontal(|ui| {
                for (value, icon, label, tooltip) in items {
                    let active = *current == *value;
                    let font = egui::FontId::proportional(theme::LABEL_SIZE);
                    let text_width = if names {
                        ui.painter()
                            .layout_no_wrap((*label).into(), font.clone(), Color32::WHITE)
                            .size()
                            .x
                    } else {
                        0.
                    };
                    let width = if names {
                        8. + 14. + 6. + text_width + 10.
                    } else {
                        theme::CONTROL_HEIGHT + 2.
                    };
                    let (rect, response) = ui.allocate_exact_size(
                        Vec2::new(width, theme::CONTROL_HEIGHT),
                        Sense::click(),
                    );
                    icons::register_qa(ui, label, rect);
                    response.widget_info(|| {
                        egui::WidgetInfo::selected(
                            egui::WidgetType::SelectableLabel,
                            ui.is_enabled(),
                            active,
                            *label,
                        )
                    });
                    let color = if active {
                        Color32::WHITE
                    } else if response.hovered() && ui.is_enabled() {
                        theme::TEXT
                    } else {
                        theme::TEXT_MUTED
                    };
                    if active {
                        ui.painter().rect_filled(rect, 5., theme::BG_HEADER);
                    } else if response.hovered() && ui.is_enabled() {
                        ui.painter().rect_filled(rect, 5., theme::BG_RAISED);
                    }
                    let icon_center = if names {
                        egui::pos2(rect.left() + 15., rect.center().y)
                    } else {
                        rect.center()
                    };
                    icons::paint(
                        ui,
                        Rect::from_center_size(icon_center, Vec2::splat(14.)),
                        *icon,
                        color,
                    );
                    if names {
                        ui.painter().text(
                            egui::pos2(rect.left() + 28., rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            *label,
                            font,
                            color,
                        );
                    }
                    if response.on_hover_text(*tooltip).clicked() && !active {
                        *current = *value;
                        changed = true;
                    }
                }
            });
        });
    changed
}

/// Width of a `segmented_icons` control, to decide between names and icons before drawing.
pub fn segmented_icons_width(ui: &Ui, labels: &[&str], names: bool) -> f32 {
    let items: f32 = labels
        .iter()
        .map(|label| {
            if names {
                8. + 14.
                    + 6.
                    + ui.painter()
                        .layout_no_wrap(
                            (*label).into(),
                            egui::FontId::proportional(theme::LABEL_SIZE),
                            Color32::WHITE,
                        )
                        .size()
                        .x
                    + 10.
            } else {
                theme::CONTROL_HEIGHT + 2.
            }
        })
        .sum();
    items + 2. * (labels.len().max(1) as f32 - 1.) + 6.
}

/// Width of a text `segmented` control.
pub fn segmented_width(ui: &Ui, labels: &[&str]) -> f32 {
    let padding = 2. * ui.spacing().button_padding.x;
    let items: f32 = labels
        .iter()
        .map(|label| {
            ui.painter()
                .layout_no_wrap(
                    (*label).into(),
                    egui::TextStyle::Button.resolve(ui.style()),
                    Color32::WHITE,
                )
                .size()
                .x
                + padding
        })
        .sum();
    items + 2. * (labels.len().max(1) as f32 - 1.) + 6.
}

/// Fixed-height toolbar row. Draw the right-aligned controls into the returned `Ui`
/// first, then call `toolbar_left` for the width that remains.
pub fn toolbar_right(ui: &mut Ui, height: f32) -> (Rect, Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    let right = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::right_to_left(Align::Center)),
    );
    (rect, right)
}

/// Left part of a `toolbar_right` row, ending before the controls already on the right.
pub fn toolbar_left(ui: &mut Ui, rect: Rect, right: &Ui) -> Ui {
    let used = right.min_rect().width();
    let max_x = if used > 0. {
        rect.max.x - used - 8.
    } else {
        rect.max.x
    };
    ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.with_max_x(max_x.max(rect.min.x)))
            .layout(Layout::left_to_right(Align::Center)),
    )
}

/// Warning strip across a work area, with its actions at the right.
pub fn banner(ui: &mut Ui, text: &str, actions: impl FnOnce(&mut Ui)) {
    egui::Frame::new()
        .fill(Color32::from_rgb(46, 39, 24))
        .stroke(Stroke::new(1., Color32::from_rgb(74, 61, 34)))
        .inner_margin(egui::Margin::symmetric(10, 5))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::containers::Sides::new()
                .height(theme::CONTROL_HEIGHT + 2.)
                .shrink_left()
                .truncate()
                .show(
                    ui,
                    |ui| {
                        let (icon, _) = ui.allocate_exact_size(Vec2::splat(16.), Sense::hover());
                        warning_icon(ui, icon, theme::WARNING);
                        ui.label(
                            RichText::new(text)
                                .size(12.5)
                                .color(Color32::from_rgb(232, 210, 160)),
                        );
                    },
                    actions,
                );
        });
}

/// Triangle with an exclamation mark, for warnings painted beside text.
pub fn warning_icon(ui: &Ui, rect: Rect, color: Color32) {
    let p = ui.painter();
    let at = |x: f32, y: f32| rect.min + Vec2::new(x, y) * rect.size();
    p.add(egui::Shape::closed_line(
        vec![at(0.5, 0.08), at(0.96, 0.9), at(0.04, 0.9)],
        Stroke::new(1.4, color),
    ));
    p.line_segment([at(0.5, 0.38), at(0.5, 0.62)], Stroke::new(1.5, color));
    p.circle_filled(at(0.5, 0.76), 1., color);
}

/// Centered card over an empty work area: what is missing and how to start.
pub fn empty_state(
    ui: &mut Ui,
    area: Rect,
    icon: Icon,
    title: &str,
    text: &str,
    actions: impl FnOnce(&mut Ui),
) {
    let width = (area.width() - 32.).min(360.);
    if width < 200. || area.height() < 160. {
        // Short areas get one centered line with the same title and actions.
        let mut row = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(area.shrink2(Vec2::new(12., 8.)))
                .layout(Layout::top_down(Align::Center)),
        );
        row.add_space(((area.height() - 70.) / 2.).max(0.));
        row.label(
            RichText::new(title)
                .font(theme::medium(13.))
                .color(theme::TEXT),
        );
        row.add_space(4.);
        actions(&mut row);
        return;
    }
    let rect = Rect::from_center_size(area.center(), Vec2::new(width, 10.));
    let mut card = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(Rect::from_min_size(
                rect.min - Vec2::new(0., 80.),
                Vec2::new(width, area.height()),
            ))
            .layout(Layout::top_down(Align::Center)),
    );
    egui::Frame::new()
        .fill(theme::BG_PANEL)
        .stroke(Stroke::new(1., theme::BORDER))
        .corner_radius(10)
        .inner_margin(egui::Margin::symmetric(20, 18))
        .shadow(egui::epaint::Shadow {
            offset: [0, 8],
            blur: 24,
            spread: 0,
            color: Color32::from_black_alpha(90),
        })
        .show(&mut card, |ui| {
            ui.set_width(width - 40.);
            ui.vertical_centered(|ui| {
                let (tile, _) = ui.allocate_exact_size(Vec2::splat(38.), Sense::hover());
                ui.painter().rect_filled(tile, 9., theme::ACCENT_SOFT);
                icons::paint(
                    ui,
                    Rect::from_center_size(tile.center(), Vec2::splat(20.)),
                    icon,
                    theme::ACCENT_BRIGHT,
                );
                ui.add_space(6.);
                ui.label(
                    RichText::new(title)
                        .font(theme::medium(14.))
                        .color(theme::TEXT),
                );
                ui.add_space(2.);
                ui.label(
                    RichText::new(text)
                        .size(12.5)
                        .color(Color32::from_rgb(168, 168, 174)),
                );
                ui.add_space(8.);
                actions(ui);
            });
        });
}

/// Thin vertical line between toolbar groups.
pub fn divider(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(9., 18.), Sense::hover());
    ui.painter().vline(
        rect.center().x,
        rect.y_range(),
        Stroke::new(1., theme::BORDER),
    );
}

/// Uppercase caption that separates groups inside a menu.
pub fn menu_label(ui: &mut Ui, text: &str) {
    ui.add_space(2.);
    ui.label(
        RichText::new(text.to_uppercase())
            .font(theme::medium(10.5))
            .color(theme::TEXT_MUTED),
    );
}

/// Menu entry with its keyboard shortcut aligned to the right.
pub fn menu_item(ui: &mut Ui, label: &str, shortcut: &str) -> Response {
    ui.add(egui::Button::new(label).shortcut_text(shortcut))
}

/// Status line over a viewport's lower-left corner: what is selected and the next step.
/// Painted only; it never takes input from the view below.
pub fn viewport_chip(ui: &Ui, area: Rect, dot: Color32, title: &str, hint: &str) {
    let painter = ui.painter_at(area);
    let font = egui::FontId::proportional(12.);
    let title = painter.layout_no_wrap(title.to_owned(), font.clone(), theme::TEXT);
    let hint = painter.layout_no_wrap(hint.to_owned(), font, theme::TEXT_MUTED);
    let base = 10. + 7. + 8. + title.size().x + 10.;
    let room = area.width() - 24.;
    let show_hint = !hint.is_empty() && base + 14. + hint.size().x <= room;
    let width = if show_hint {
        base + 14. + hint.size().x
    } else {
        base
    };
    if width > room || area.height() < 60. {
        return;
    }
    let rect = Rect::from_min_size(
        egui::pos2(area.left() + 12., area.bottom() - 12. - 26.),
        Vec2::new(width, 26.),
    );
    painter.rect(
        rect,
        6.,
        theme::BG_PANEL.gamma_multiply(0.94),
        Stroke::new(1., theme::BORDER),
        egui::StrokeKind::Inside,
    );
    let y = rect.center().y;
    painter.circle_filled(egui::pos2(rect.left() + 13.5, y), 3.5, dot);
    let x = rect.left() + 25.;
    painter.galley(
        egui::pos2(x, y - title.size().y / 2.),
        title.clone(),
        theme::TEXT,
    );
    if show_hint {
        let x = x + title.size().x + 7.;
        painter.circle_filled(egui::pos2(x, y), 1.2, theme::TEXT_MUTED);
        painter.galley(
            egui::pos2(x + 7., y - hint.size().y / 2.),
            hint,
            theme::TEXT_MUTED,
        );
    }
}

/// Always-open block with an icon tile, a title and optional trailing text. `accent`
/// marks an operation in progress.
pub fn panel_card<R>(
    ui: &mut Ui,
    icon: Icon,
    title: &str,
    trailing: &str,
    accent: bool,
    body: impl FnOnce(&mut Ui) -> R,
) -> R {
    let stroke = if accent {
        Stroke::new(1., theme::ACCENT)
    } else {
        Stroke::new(1., theme::CARD_BORDER)
    };
    egui::Frame::new()
        .fill(theme::CARD_FILL)
        .stroke(stroke)
        .corner_radius(theme::CARD_RADIUS)
        .inner_margin(egui::Margin {
            left: 8,
            right: 8,
            top: 6,
            bottom: 8,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.set_min_height(26.);
                let (tile, _) = ui.allocate_exact_size(Vec2::splat(22.), Sense::hover());
                let (fill, color) = if accent {
                    (theme::ACCENT_SOFT, theme::ACCENT_BRIGHT)
                } else {
                    (theme::BG_HEADER, theme::TEXT_MUTED)
                };
                ui.painter().rect_filled(tile, 5., fill);
                icons::paint(
                    ui,
                    Rect::from_center_size(tile.center(), Vec2::splat(13.)),
                    icon,
                    color,
                );
                ui.add(egui::Label::new(RichText::new(title).font(theme::medium(13.))).truncate());
                if !trailing.is_empty() {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(trailing).size(11.5).color(if accent {
                            theme::ACCENT_BRIGHT
                        } else {
                            theme::TEXT_MUTED
                        }));
                    });
                }
            });
            ui.add_space(2.);
            body(ui)
        })
        .inner
}

/// Label column wide enough for every label in a block, never narrower than the default.
pub fn label_column(ui: &Ui, labels: &[&str]) -> f32 {
    labels
        .iter()
        .map(|label| {
            ui.painter()
                .layout_no_wrap(
                    (*label).into(),
                    egui::FontId::proportional(12.),
                    Color32::WHITE,
                )
                .size()
                .x
                + 8.
        })
        .fold(theme::LABEL_COLUMN, f32::max)
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
    card_with(ui, id_salt, title, false, enabled, menu, body)
}

/// `card` that starts open the first time it is shown.
pub fn open_card<R>(
    ui: &mut Ui,
    id_salt: impl Hash,
    title: &str,
    body: impl FnOnce(&mut Ui) -> R,
) -> Option<R> {
    card_with(ui, id_salt, title, true, None, None, body)
}

/// Opens the card titled `title` when it is next shown, e.g. a component just added.
/// A request that no card answers within two frames is dropped.
pub fn request_card_open(ctx: &egui::Context, title: &str) {
    let frame = ctx.cumulative_frame_nr();
    ctx.data_mut(|d| d.insert_temp(card_open_request(), (title.to_owned(), frame)));
}

fn card_open_request() -> egui::Id {
    egui::Id::new("card_open_request")
}

fn card_with<R>(
    ui: &mut Ui,
    id_salt: impl Hash,
    title: &str,
    default_open: bool,
    enabled: Option<&mut bool>,
    menu: Option<&mut dyn FnMut(&mut Ui)>,
    body: impl FnOnce(&mut Ui) -> R,
) -> Option<R> {
    let id = ui.make_persistent_id(id_salt);
    let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(
        ui.ctx(),
        id,
        default_open,
    );
    let request = ui
        .ctx()
        .data(|d| d.get_temp::<(String, u64)>(card_open_request()));
    if let Some((requested, frame)) = request {
        if ui.ctx().cumulative_frame_nr() > frame + 2 {
            ui.ctx()
                .data_mut(|d| d.remove::<(String, u64)>(card_open_request()));
        } else if requested == title {
            state.set_open(true);
            ui.ctx()
                .data_mut(|d| d.remove::<(String, u64)>(card_open_request()));
        }
    }
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
                    .add(
                        egui::Label::new(
                            RichText::new(title)
                                .font(theme::medium(13.))
                                .color(theme::TEXT),
                        )
                        .selectable(false)
                        .sense(Sense::click()),
                    )
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
    property_row_with(ui, theme::LABEL_COLUMN, label, hint, add)
}

/// `property_row` with a label column of `column` points, shared by every row of a block.
pub fn property_row_with<R>(
    ui: &mut Ui,
    column: f32,
    label: &str,
    hint: &str,
    add: impl FnOnce(&mut Ui) -> R,
) -> R {
    ui.horizontal(|ui| {
        ui.set_min_height(theme::CONTROL_HEIGHT);
        let label_size = Vec2::new(column, theme::CONTROL_HEIGHT);
        let response = ui
            .allocate_ui_with_layout(label_size, Layout::left_to_right(Align::Center), |ui| {
                ui.set_width(column);
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

/// `property_row_with` while the label fits the column and `width` points of field fit beside
/// it; otherwise the label takes its own line over the field, since a wider row would grow the
/// side panel. Returns whether the field sits beside the label.
pub fn property_row_fit(
    ui: &mut Ui,
    column: f32,
    label: &str,
    hint: &str,
    width: f32,
    add: impl FnOnce(&mut Ui),
) -> bool {
    let beside = label_column(ui, &[label]) <= column
        && ui.available_width() >= column + ui.spacing().item_spacing.x + width;
    if beside {
        property_row_with(ui, column, label, hint, add);
    } else {
        let response = ui.label(RichText::new(label).size(12.).color(theme::TEXT_MUTED));
        if !hint.is_empty() {
            response.on_hover_text(hint);
        }
        ui.horizontal(|ui| {
            ui.set_min_height(theme::CONTROL_HEIGHT);
            add(ui)
        });
    }
    beside
}

/// X/Y/Z fields in one row, each marked with its axis colour. Returns the field responses.
pub fn vec3_fields(
    ui: &mut Ui,
    label: &str,
    hint: &str,
    values: &mut [f32; 3],
    speed: f64,
    nonzero: bool,
) -> [Response; 3] {
    property_row(ui, label, hint, |ui| {
        let gap = 4.;
        ui.spacing_mut().item_spacing.x = gap;
        // Never wider than the column: a wider row would grow the side panel.
        let width = ((ui.available_width() - 2. * gap) / 3.).max(12.);
        let mut field = |axis: usize, value: &mut f32| {
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
            if nonzero && value.abs() < 0.0001 {
                *value = 0.0001;
            }
            response
        };
        let [x, y, z] = values;
        [field(0, x), field(1, y), field(2, z)]
    })
}

/// X/Y/Z fields in one row. Returns whether any value changed.
pub fn vec3_row(
    ui: &mut Ui,
    label: &str,
    hint: &str,
    values: &mut [f32; 3],
    speed: f64,
    nonzero: bool,
) -> bool {
    vec3_fields(ui, label, hint, values, speed, nonzero)
        .iter()
        .any(Response::changed)
}

/// Number field filling the field column, with the unit after the value.
pub fn number_row<N: egui::emath::Numeric>(
    ui: &mut Ui,
    label: &str,
    hint: &str,
    value: &mut N,
    range: std::ops::RangeInclusive<N>,
    speed: f64,
    unit: &str,
) -> Response {
    property_row(ui, label, hint, |ui| {
        ui.add_sized(
            [ui.available_width(), theme::CONTROL_HEIGHT],
            egui::DragValue::new(value)
                .range(range)
                .speed(speed)
                .suffix(unit),
        )
    })
}

/// On/off setting with its switch in the field column.
pub fn toggle_row(ui: &mut Ui, label: &str, hint: &str, value: &mut bool) -> Response {
    property_row(ui, label, hint, |ui| toggle(ui, value, label))
}

/// Drop-down filling the field column.
pub fn combo_row<R>(
    ui: &mut Ui,
    label: &str,
    hint: &str,
    id_salt: impl Hash,
    selected: &str,
    options: impl FnOnce(&mut Ui) -> R,
) {
    property_row(ui, label, hint, |ui| {
        combo(ui, id_salt, selected, ui.available_width(), options)
    });
}

/// Drop-down whose whole button, arrow included, is `width` wide.
pub fn combo<R>(
    ui: &mut Ui,
    id_salt: impl Hash,
    selected: &str,
    width: f32,
    options: impl FnOnce(&mut Ui) -> R,
) {
    egui::ComboBox::from_id_salt(id_salt)
        .width(combo_inner_width(ui, width))
        .selected_text(selected)
        .truncate()
        .show_ui(ui, options);
}

/// Inner width for a ComboBox whose button, arrow included, spans `outer` points.
pub fn combo_inner_width(ui: &Ui, outer: f32) -> f32 {
    let padding = 2. * ui.spacing().button_padding.x + ui.spacing().icon_width + 4.;
    (outer - padding).max(24.)
}

/// Collapsible group inside a card, closed until opened; remembered per `id_salt`.
pub fn sub_section(ui: &mut Ui, id_salt: impl Hash, title: &str, body: impl FnOnce(&mut Ui)) {
    let id = ui.make_persistent_id(id_salt);
    let mut state =
        egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false);
    let open = state.is_open();
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 24.), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::CollapsingHeader, true, open, title)
    });
    let color = if response.hovered() {
        theme::TEXT
    } else {
        theme::TEXT_MUTED
    };
    icons::paint(
        ui,
        Rect::from_center_size(
            egui::pos2(rect.left() + 8., rect.center().y),
            Vec2::splat(12.),
        ),
        if open {
            Icon::ChevronDown
        } else {
            Icon::ChevronRight
        },
        color,
    );
    ui.painter().text(
        egui::pos2(rect.left() + 20., rect.center().y),
        egui::Align2::LEFT_CENTER,
        title,
        egui::FontId::proportional(12.5),
        color,
    );
    if response.clicked() {
        state.set_open(!open);
    }
    if state.is_open() {
        egui::Frame::new()
            .inner_margin(egui::Margin {
                left: 8,
                right: 0,
                top: 0,
                bottom: 4,
            })
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                body(ui)
            });
    }
    state.store(ui.ctx());
}

/// Muted explanatory text for empty states and short hints.
pub fn hint(ui: &mut Ui, text: &str) {
    ui.add(egui::Label::new(RichText::new(text).size(12.).color(theme::TEXT_MUTED)).wrap());
}

/// Title row of a dialog with its close button; true when the button was clicked.
pub fn dialog_header(ui: &mut Ui, title: &str) -> bool {
    let mut close = false;
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(title)
                .font(theme::medium(15.))
                .color(theme::TEXT),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            close = icons::small(ui, Icon::Close, "Fechar", 26.).clicked();
        });
    });
    ui.add_space(8.);
    close
}

/// Small muted caption above a dialog field.
pub fn field_label(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(12.).color(theme::TEXT_MUTED));
}

/// Bottom row of a dialog: `add` lays out the buttons from the right edge, so the main
/// action goes first.
pub fn dialog_footer<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.add_space(12.);
    let row = Vec2::new(ui.available_width(), 30.);
    ui.allocate_ui_with_layout(row, Layout::right_to_left(Align::Center), add)
        .inner
}

/// Accent button for the main action of a dialog or block.
pub fn primary_button(ui: &mut Ui, text: &str, enabled: bool) -> Response {
    ui.add_enabled(
        enabled,
        egui::Button::new(RichText::new(text).color(Color32::WHITE))
            .fill(theme::ACCENT)
            .min_size(Vec2::new(0., 28.)),
    )
}

/// Framed neutral button, such as Cancelar.
pub fn secondary_button(ui: &mut Ui, text: impl Into<egui::WidgetText>) -> Response {
    ui.add(
        egui::Button::new(text)
            .fill(theme::BG_RAISED)
            .stroke(Stroke::new(1., theme::BORDER))
            .min_size(Vec2::new(0., 28.)),
    )
}

/// Destructive action. `filled` paints it red; otherwise only the text is red, for a
/// button that sits away from the main action.
pub fn danger_button(ui: &mut Ui, text: &str, filled: bool) -> Response {
    let button = if filled {
        egui::Button::new(RichText::new(text).color(Color32::WHITE))
            .fill(Color32::from_rgb(182, 75, 75))
    } else {
        egui::Button::new(RichText::new(text).color(theme::ERROR)).frame(false)
    };
    ui.add(button.min_size(Vec2::new(0., 28.)))
}

/// Tinted box with a short message inside a dialog or block.
pub fn callout(ui: &mut Ui, warning: bool, text: &str) {
    let (fill, stroke, color) = if warning {
        (
            Color32::from_rgb(46, 39, 24),
            Color32::from_rgb(74, 61, 34),
            Color32::from_rgb(232, 210, 160),
        )
    } else {
        (theme::BG_PANEL, theme::CARD_BORDER, theme::TEXT_MUTED)
    };
    egui::Frame::new()
        .fill(fill)
        .stroke(Stroke::new(1., stroke))
        .corner_radius(6)
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(egui::Label::new(RichText::new(text).size(12.).color(color)).wrap());
        });
}

/// Cancelar/Aplicar row that closes a block with an operation in preview.
/// Returns Some(true) to apply and Some(false) to cancel.
pub fn preview_footer(
    ui: &mut Ui,
    can_apply: bool,
    disabled_hint: &str,
    extra: impl FnOnce(&mut Ui),
) -> Option<bool> {
    let mut action = None;
    ui.add_space(4.);
    ui.separator();
    let row = Vec2::new(ui.available_width(), theme::CONTROL_HEIGHT + 4.);
    ui.allocate_ui_with_layout(row, Layout::right_to_left(Align::Center), |ui| {
        if ui
            .add_enabled(
                can_apply,
                egui::Button::new(RichText::new("Aplicar").color(Color32::WHITE))
                    .fill(theme::ACCENT)
                    .shortcut_text(RichText::new("Enter").color(Color32::from_rgb(214, 228, 245))),
            )
            .on_disabled_hover_text(disabled_hint)
            .clicked()
        {
            action = Some(true);
        }
        if ui
            .add(egui::Button::new("Cancelar").shortcut_text("Esc"))
            .clicked()
        {
            action = Some(false);
        }
        ui.with_layout(Layout::left_to_right(Align::Center), extra);
    });
    action
}

/// One place that still uses something being deleted: an icon, the object's name and,
/// at the right, where it is and how it uses it.
pub fn reference_row(ui: &mut Ui, icon: Icon, object: &str, detail: &str) {
    egui::Frame::new()
        .fill(theme::BG_PANEL)
        .corner_radius(5)
        .inner_margin(egui::Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(14.), Sense::hover());
                icons::paint(ui, rect, icon, theme::ACCENT_BRIGHT);
                ui.label(RichText::new(object).size(12.5).color(theme::TEXT));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.add(
                        egui::Label::new(RichText::new(detail).size(12.).color(theme::TEXT_MUTED))
                            .truncate(),
                    );
                });
            });
        });
}
