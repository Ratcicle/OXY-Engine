//! Editor and player appearance: neutral greys, one desaturated blue accent and compact controls.
//! UI code uses these semantic tokens instead of literal colours. Viewport overlays keep
//! pure white/black where they only need contrast against an arbitrary scene.
use egui::{Color32, CornerRadius, FontId, Stroke, TextStyle, Vec2};

// Surfaces, from the deepest (inputs, canvases) to the most raised (node titles).
pub const BG_BASE: Color32 = Color32::from_rgb(24, 24, 26);
pub const BG_PANEL: Color32 = Color32::from_rgb(36, 36, 38);
pub const BG_WINDOW: Color32 = Color32::from_rgb(42, 42, 45);
pub const BG_RAISED: Color32 = Color32::from_rgb(47, 47, 51);
pub const BG_HEADER: Color32 = Color32::from_rgb(58, 58, 63);
pub const BORDER: Color32 = Color32::from_rgb(62, 62, 67);
pub const GRID: Color32 = Color32::from_rgb(44, 44, 47);

pub const TEXT: Color32 = Color32::from_rgb(222, 222, 224);
pub const TEXT_MUTED: Color32 = Color32::from_rgb(150, 150, 156);

/// The single accent: selection, focus, primary action and editing overlays.
pub const ACCENT: Color32 = Color32::from_rgb(86, 140, 205);
/// Lighter accent for thin strokes and overlays drawn over the 3D scene.
pub const ACCENT_BRIGHT: Color32 = Color32::from_rgb(132, 178, 236);
pub const ACCENT_SOFT: Color32 = Color32::from_rgb(48, 74, 108);
pub const ACCENT_FILL: Color32 = Color32::from_rgba_unmultiplied_const(86, 140, 205, 28);

pub const WARNING: Color32 = Color32::from_rgb(224, 184, 104);
pub const ERROR: Color32 = Color32::from_rgb(232, 112, 112);
pub const SUCCESS: Color32 = Color32::from_rgb(122, 196, 140);
/// Hovered or active handle, marker or edge. Distinct from selection.
pub const HIGHLIGHT: Color32 = Color32::from_rgb(240, 196, 92);

// Spatial axes: X red, Y green, Z blue.
pub const AXIS_X: Color32 = Color32::from_rgb(232, 104, 108);
pub const AXIS_Y: Color32 = Color32::from_rgb(124, 206, 144);
pub const AXIS_Z: Color32 = Color32::from_rgb(110, 156, 236);
pub const AXES: [Color32; 3] = [AXIS_X, AXIS_Y, AXIS_Z];

/// Height of toolbar rows and icon buttons, in points before interface scale.
pub const CONTROL_HEIGHT: f32 = 24.;
pub const ICON_SIZE: f32 = 16.;
pub const LABEL_SIZE: f32 = 12.5;

pub fn apply(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    let v = &mut style.visuals;
    *v = egui::Visuals::dark();
    v.panel_fill = BG_PANEL;
    v.window_fill = BG_WINDOW;
    v.extreme_bg_color = BG_BASE;
    v.faint_bg_color = Color32::from_rgb(40, 40, 43);
    v.code_bg_color = BG_BASE;
    v.window_stroke = Stroke::new(1., BORDER);
    v.window_corner_radius = CornerRadius::same(4);
    v.menu_corner_radius = CornerRadius::same(4);
    v.hyperlink_color = ACCENT_BRIGHT;
    v.warn_fg_color = WARNING;
    v.error_fg_color = ERROR;
    v.selection.bg_fill = ACCENT_SOFT;
    v.selection.stroke = Stroke::new(1., ACCENT_BRIGHT);
    let radius = CornerRadius::same(3);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = BG_PANEL;
    w.noninteractive.weak_bg_fill = BG_PANEL;
    w.noninteractive.bg_stroke = Stroke::new(1., BORDER);
    w.noninteractive.fg_stroke = Stroke::new(1., TEXT_MUTED);
    w.inactive.bg_fill = Color32::from_rgb(52, 52, 56);
    w.inactive.weak_bg_fill = Color32::from_rgb(48, 48, 52);
    w.inactive.bg_stroke = Stroke::NONE;
    w.inactive.fg_stroke = Stroke::new(1., TEXT);
    w.hovered.bg_fill = Color32::from_rgb(64, 64, 69);
    w.hovered.weak_bg_fill = Color32::from_rgb(60, 60, 65);
    w.hovered.bg_stroke = Stroke::new(1., Color32::from_rgb(84, 84, 90));
    w.hovered.fg_stroke = Stroke::new(1.5, Color32::WHITE);
    w.active.bg_fill = Color32::from_rgb(72, 72, 78);
    w.active.weak_bg_fill = Color32::from_rgb(72, 72, 78);
    w.active.bg_stroke = Stroke::new(1., ACCENT);
    w.active.fg_stroke = Stroke::new(1.5, Color32::WHITE);
    w.open.bg_fill = BG_RAISED;
    w.open.weak_bg_fill = BG_RAISED;
    w.open.bg_stroke = Stroke::new(1., BORDER);
    w.open.fg_stroke = Stroke::new(1., TEXT);
    for widget in [
        &mut w.noninteractive,
        &mut w.inactive,
        &mut w.hovered,
        &mut w.active,
        &mut w.open,
    ] {
        widget.corner_radius = radius;
        widget.expansion = 0.;
    }
    style.spacing.item_spacing = Vec2::new(6., 4.);
    style.spacing.button_padding = Vec2::new(6., 3.);
    style.spacing.interact_size.y = 20.;
    style.spacing.menu_margin = egui::Margin::same(4);
    style.spacing.window_margin = egui::Margin::same(8);
    style
        .text_styles
        .insert(TextStyle::Body, FontId::proportional(13.));
    style
        .text_styles
        .insert(TextStyle::Button, FontId::proportional(13.));
    ctx.set_style(style);
}
