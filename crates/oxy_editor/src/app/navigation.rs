//! Temporary RMB input ownership. No document, runtime, physics or history state lives here.
use super::*;
use egui::{Event, Key, Modifiers, PointerButton};
use std::collections::HashSet;
#[cfg(all(test, target_os = "windows"))]
mod native;
#[cfg(test)]
mod tests;

pub(super) const MIN_SPEED: f32 = 0.1;
pub(super) const MAX_SPEED: f32 = 100.;
pub(super) const MIN_SENSITIVITY: f32 = 0.0005;
pub(super) const MAX_SENSITIVITY: f32 = 0.02;
pub(super) const HELP: &str =
    "RMB + mouse olhar · RMB + WASD mover · Shift rápido · Ctrl preciso · RMB + roda velocidade";

#[derive(Default)]
pub(super) struct Navigation {
    pub active: bool,
    pub owns_rmb: bool,
    viewport: Option<(Rect, egui::LayerId)>,
    owner: Option<(Id, Tab, StudioTab)>,
    pointer: Option<Pos2>,
    held: HashSet<Key>,
    // Repeats/releases belonging to a finished gesture must not become editor shortcuts.
    consumed: HashSet<Key>,
    look: Vec2,
    wheel: f32,
    modifiers: Modifiers,
    started: bool,
    pub notice_until: Option<Instant>,
    preference_changed: bool,
}

fn multiplier(m: Modifiers) -> f32 {
    match (m.shift, m.ctrl) {
        (true, false) => 4.,
        (false, true) => 0.25,
        _ => 1.,
    }
}
fn scroll_speed(speed: f32, steps: f32) -> f32 {
    if !steps.is_finite() {
        return speed;
    }
    (speed * 1.2_f32.powf(steps.clamp(-32., 32.))).clamp(MIN_SPEED, MAX_SPEED)
}

impl Navigation {
    fn stop(&mut self) {
        self.active = false;
        self.held.clear();
        self.look = Vec2::ZERO;
        self.wheel = 0.;
    }

    /// Runs before egui sees input: Ctrl+wheel cannot leak into UI scrolling/zoom,
    /// and Ctrl+S/Ctrl+D during navigation cannot save or duplicate scene objects.
    fn route(
        &mut self,
        input: &mut egui::RawInput,
        pixels_per_point: f32,
        can_start: bool,
        initially_held: [Option<Key>; 4],
        over_viewport: impl Fn(Pos2) -> bool,
    ) {
        self.look = Vec2::ZERO;
        self.wheel = 0.;
        self.started = false;
        self.modifiers = input.modifiers;
        if !can_start || !input.focused {
            self.stop();
        }
        let mut relative = Vec2::ZERO;
        let mut absolute = Vec2::ZERO;
        let mut has_relative = false;
        let mut priority_pointer = false;
        input.events.retain(|event| {
            match event {
                Event::WindowFocused(false) | Event::PointerGone => self.stop(),
                Event::PointerMoved(pos) => {
                    if self.active {
                        if !over_viewport(*pos) {
                            self.stop();
                        } else if let Some(old) = self.pointer {
                            absolute += (*pos - old) * pixels_per_point;
                        }
                    }
                    self.pointer = Some(*pos);
                }
                Event::PointerButton {
                    pos,
                    button: PointerButton::Secondary,
                    pressed,
                    ..
                } => {
                    self.pointer = Some(*pos);
                    if *pressed
                        && can_start
                        && input.focused
                        && !priority_pointer
                        && over_viewport(*pos)
                    {
                        self.active = true;
                        self.owns_rmb = true;
                        self.started = true;
                        self.held.clear();
                        self.held.extend(initially_held.into_iter().flatten());
                        self.consumed.extend(self.held.iter().copied());
                    } else {
                        self.stop();
                        if !pressed {
                            self.owns_rmb = false;
                        }
                    }
                }
                Event::PointerButton {
                    button: PointerButton::Primary | PointerButton::Middle,
                    pressed,
                    ..
                } => {
                    if self.active {
                        return false;
                    }
                    priority_pointer = *pressed;
                }
                Event::MouseMoved(delta) if self.active => {
                    relative += *delta;
                    has_relative = true;
                }
                Event::MouseWheel { unit, delta, .. } if self.active => {
                    self.wheel += match unit {
                        egui::MouseWheelUnit::Line | egui::MouseWheelUnit::Page => delta.y,
                        egui::MouseWheelUnit::Point => delta.y * pixels_per_point / 40.,
                    };
                    return false;
                }
                Event::Key {
                    key,
                    pressed,
                    repeat,
                    ..
                } => {
                    if *pressed && !repeat && !self.active {
                        self.consumed.remove(key);
                    }
                    let was_consumed = self.consumed.contains(key);
                    if !pressed {
                        self.held.remove(key);
                        self.consumed.remove(key);
                    } else if self.active {
                        if (!repeat || !was_consumed)
                            && matches!(key, Key::W | Key::A | Key::S | Key::D)
                        {
                            self.held.insert(*key);
                        }
                        self.consumed.insert(*key);
                    }
                    if self.active || was_consumed {
                        return false;
                    }
                }
                Event::Text(_) | Event::Copy | Event::Cut | Event::Paste(_) if self.active => {
                    return false;
                }
                _ => {}
            }
            true
        });
        if self.active && self.pointer.is_none_or(|p| !over_viewport(p)) {
            self.stop();
        }
        if self.active {
            self.look = if has_relative { relative } else { absolute };
        }
    }

    fn apply(
        &mut self,
        camera: &mut CameraState,
        preferences: &mut preferences::Preferences,
        dt: f32,
    ) {
        if !self.active {
            return;
        }
        camera.look_from_eye(
            [self.look.x, self.look.y],
            preferences.navigation_sensitivity,
        );
        let speed = scroll_speed(preferences.navigation_speed, self.wheel);
        if speed != preferences.navigation_speed {
            preferences.navigation_speed = speed;
            self.preference_changed = true;
            self.notice_until = Some(Instant::now() + std::time::Duration::from_millis(1400));
        }
        let down = |key| if self.held.contains(&key) { 1. } else { 0. };
        camera.move_in_view(
            down(Key::D) - down(Key::A),
            down(Key::W) - down(Key::S),
            speed * multiplier(self.modifiers),
            if self.started { 0. } else { dt },
        );
        // An egui sizing pass may call update again with the same RawInput.
        self.look = Vec2::ZERO;
        self.wheel = 0.;
    }
}

impl Editor {
    pub(super) fn stop_navigation(&mut self, ctx: &egui::Context) {
        self.navigation.stop();
        self.finish_navigation(ctx);
    }
    pub(super) fn navigation_blocked(&self, ctx: &egui::Context) -> bool {
        self.home.visible
            || !matches!(self.tab, Tab::Scene | Tab::Studio)
            || self.scene().kind != SceneKind::ThreeD
            || self.capture
            || self.modeling.creation.is_some()
            || self.modeling.help
            || self.mesh_operation_active()
            || self.gizmo_drag.is_some()
            || self.spatial_active_drag()
            || self.camera_dragging()
            || self.pending_preferences.is_some()
            || self.new_project.is_some()
            || self.scene_dialog.is_some()
            || self.pending.is_some()
            || self.delete_asset.is_some()
            || self.spatial.fit.is_some()
            || self.logic_ui.inputs
            || self.logic_ui.guide
            || crate::graph_ui::text_input_active(ctx)
            || egui::Popup::is_any_open(ctx)
            || ctx.memory(|m| m.top_modal_layer().is_some())
    }

    pub(super) fn navigation_input(&mut self, ctx: &egui::Context, input: &mut egui::RawInput) {
        let same_context = self
            .navigation
            .owner
            .as_ref()
            .is_some_and(|(scene, tab, studio)| {
                scene == &self.scene_id && *tab == self.tab && *studio == self.studio.tab
            });
        let allowed = same_context
            && !self.navigation_blocked(ctx)
            && !ctx.input(|i| {
                i.pointer.primary_down() || i.pointer.button_down(PointerButton::Middle)
            });
        let viewport = self.navigation.viewport;
        ctx.input_mut(|i| {
            i.keys_down
                .retain(|key| !self.navigation.consumed.contains(key))
        });
        let initially_held =
            ctx.input(|i| [Key::W, Key::A, Key::S, Key::D].map(|k| i.key_down(k).then_some(k)));
        self.navigation.route(
            input,
            ctx.pixels_per_point(),
            allowed,
            initially_held,
            |p| {
                viewport.is_some_and(|(rect, layer)| {
                    rect.contains(p) && ctx.layer_id_at(p) == Some(layer)
                })
            },
        );
        ctx.input_mut(|i| {
            i.keys_down.retain(|key| {
                !self.navigation.consumed.contains(key)
                    && (!self.navigation.started || !initially_held.contains(&Some(*key)))
            })
        });
    }

    pub(super) fn navigate_viewport(&mut self, ui: &egui::Ui, rect: Rect) {
        let context = (self.scene_id.clone(), self.tab, self.studio.tab);
        if self.navigation.owner.as_ref() != Some(&context) {
            self.navigation.stop();
            self.navigation.owner = Some(context);
        }
        self.navigation.viewport = Some((rect, ui.layer_id()));
        if self.navigation_blocked(ui.ctx())
            || !ui.input(|i| i.focused && i.pointer.secondary_down())
            || !self.navigation.pointer.is_some_and(|p| rect.contains(p))
        {
            self.navigation.stop();
        }
        self.navigation.apply(
            &mut self.camera,
            &mut self.preferences,
            self.frame_interval_ms / 1000.,
        );
        if self.navigation.active {
            ui.ctx().request_repaint();
        }
    }

    pub(super) fn navigation_notice(&mut self, ui: &egui::Ui, rect: Rect) {
        if let Some(until) = self.navigation.notice_until {
            if let Some(delay) = until.checked_duration_since(Instant::now()) {
                let label = format!("Navegação: {:.1} m/s", self.preferences.navigation_speed)
                    .replace('.', ",");
                let painter = ui.painter_at(rect);
                let galley =
                    painter.layout_no_wrap(label, egui::FontId::proportional(14.), Color32::WHITE);
                let pos = rect.left_top() + Vec2::splat(12.);
                painter.rect_filled(
                    Rect::from_min_size(pos - Vec2::splat(6.), galley.size() + Vec2::splat(12.)),
                    5.,
                    Color32::from_black_alpha(210),
                );
                painter.galley(pos, galley, Color32::WHITE);
                ui.ctx().request_repaint_after(delay);
            } else {
                self.navigation.notice_until = None;
            }
        }
    }

    pub(super) fn finish_navigation(&mut self, ctx: &egui::Context) {
        if self.navigation_blocked(ctx) || !ctx.input(|i| i.focused && i.pointer.secondary_down()) {
            self.navigation.stop();
        }
        if !self.navigation.active
            && std::mem::take(&mut self.navigation.preference_changed)
            && let Err(error) = self.preferences.save()
        {
            self.warn(format!(
                "Não foi possível guardar a velocidade de navegação: {error}"
            ));
            self.notice_last(true);
        }
    }
}
