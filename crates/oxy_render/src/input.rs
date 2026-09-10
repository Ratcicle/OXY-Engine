//! One native input boundary shared by editor Play and the standalone player.
//! The core receives named actions and never reads egui or editing state.
use egui::{Context, Event, Key, Modifiers};
use oxy_core::runtime::InputFrame;
use std::collections::{BTreeMap, HashSet};

#[derive(Clone, Default)]
struct CaptureState {
    suppressed: HashSet<Key>,
    enabled: bool,
    active: HashSet<Key>,
}

pub fn text_input_active(ctx: &Context) -> bool {
    ctx.memory(|memory| memory.focused())
        .is_some_and(|id| egui::TextEdit::load_state(ctx, id).is_some())
}

fn command(modifiers: Modifiers) -> bool {
    modifiers.command || modifiers.ctrl || modifiers.alt || modifiers.mac_cmd
}

/// Escape and F3 belong to the native host (pause and diagnostics). Other keys are
/// resolved solely through project bindings. Text focus and command chords never
/// reach either `pressed` or `held`, including fast chords released in one frame.
fn collect_keyboard(
    ctx: &Context,
    bindings: &BTreeMap<String, String>,
    enabled: bool,
) -> InputFrame {
    let state_id = egui::Id::new("oxy_gameplay_input_capture");
    let mut state = ctx
        .data_mut(|data| data.get_temp::<CaptureState>(state_id))
        .unwrap_or_default();
    let (down, events, focused, modifiers) = ctx.input(|input| {
        (
            input.keys_down.clone(),
            input.events.clone(),
            input.focused,
            input.modifiers,
        )
    });
    state.suppressed.retain(|key| down.contains(key));
    let mut ordinary_pressed = HashSet::new();
    let mut ordinary_released = HashSet::new();
    for event in events {
        if let Event::Key {
            key,
            pressed,
            repeat,
            modifiers,
            ..
        } = event
        {
            if command(modifiers) {
                // Remember the key until its own release, not merely CTRL/ALT release.
                state.suppressed.insert(key);
            } else if pressed && !repeat {
                ordinary_pressed.insert(key);
            } else if !pressed {
                ordinary_released.insert(key);
            }
        }
    }
    let capture = enabled && focused && !text_input_active(ctx) && !command(modifiers);
    if !capture {
        state.suppressed.extend(down.iter().copied());
        state.suppressed.extend(ordinary_pressed.iter().copied());
        state.enabled = false;
        state.active.clear();
        ctx.data_mut(|data| data.insert_temp(state_id, state));
        return InputFrame::default();
    }
    if !state.enabled {
        // Retomar does not treat keys already held while editing/paused as input.
        state.suppressed.extend(
            down.iter()
                .filter(|key| !ordinary_pressed.contains(key))
                .copied(),
        );
    }
    state.enabled = true;
    let mut frame = InputFrame::default();
    for (action, name) in bindings {
        let Some(key) = Key::from_name(name) else {
            continue;
        };
        if matches!(key, Key::Escape | Key::F3) || state.suppressed.contains(&key) {
            continue;
        }
        if down.contains(&key) {
            frame.held.insert(action.clone());
        }
        if ordinary_pressed.contains(&key) {
            frame.pressed.insert(action.clone());
        }
        if ordinary_released.contains(&key)
            && (state.active.contains(&key) || ordinary_pressed.contains(&key))
        {
            frame.released.insert(action.clone());
        }
    }
    state.active = down
        .into_iter()
        .filter(|k| !state.suppressed.contains(k))
        .collect();
    ctx.data_mut(|data| data.insert_temp(state_id, state));
    frame
}

#[derive(Clone, Default)]
struct MouseCapture {
    enabled: bool,
    relative: bool,
    ignore_click: bool,
    active: Vec<egui::PointerButton>,
    suppressed: Vec<egui::PointerButton>,
}
/// Release also works outside the Game tab and on modal/home/error early returns.
pub fn release_cursor(ctx: &Context) {
    // Tabs that do not draw the game still suppress keys held while editing.
    collect_keyboard(ctx, &BTreeMap::new(), false);
    let id = egui::Id::new("oxy_relative_mouse");
    let mut state = ctx
        .data_mut(|d| d.get_temp::<MouseCapture>(id))
        .unwrap_or_default();
    if state.relative {
        ctx.send_viewport_cmd(egui::ViewportCommand::CursorGrab(egui::CursorGrab::None));
        ctx.send_viewport_cmd(egui::ViewportCommand::CursorVisible(true));
    }
    state.relative = false;
    state.enabled = false;
    state.active.clear();
    ctx.data_mut(|d| d.insert_temp(id, state));
}
pub fn accepts_game_click(ctx: &Context) -> bool {
    ctx.data_mut(|d| d.get_temp::<MouseCapture>(egui::Id::new("oxy_relative_mouse")))
        .is_some_and(|s| s.enabled && !s.ignore_click)
}
pub fn valid_binding(name: &str) -> bool {
    Key::from_name(name).is_some()
        || matches!(
            name,
            "MouseLeft"
                | "MouseRight"
                | "MouseMiddle"
                | "Mouse4"
                | "Mouse5"
                | "WheelUp"
                | "WheelDown"
        )
}
pub fn collect_input(
    ctx: &Context,
    bindings: &BTreeMap<String, String>,
    enabled: bool,
) -> InputFrame {
    collect_game_input(ctx, bindings, enabled, false)
}
pub fn collect_game_input(
    ctx: &Context,
    bindings: &BTreeMap<String, String>,
    enabled: bool,
    relative: bool,
) -> InputFrame {
    let mut frame = collect_keyboard(ctx, bindings, enabled);
    let id = egui::Id::new("oxy_relative_mouse");
    let mut state = ctx
        .data_mut(|d| d.get_temp::<MouseCapture>(id))
        .unwrap_or_default();
    let allowed = enabled && ctx.input(|i| i.focused) && !text_input_active(ctx);
    let capturing = allowed && relative;
    if capturing != state.relative {
        ctx.send_viewport_cmd(egui::ViewportCommand::CursorGrab(if capturing {
            egui::CursorGrab::Confined
        } else {
            egui::CursorGrab::None
        }));
        ctx.send_viewport_cmd(egui::ViewportCommand::CursorVisible(!capturing));
    }
    let entering = allowed && !state.enabled;
    let buttons = [
        egui::PointerButton::Primary,
        egui::PointerButton::Secondary,
        egui::PointerButton::Middle,
        egui::PointerButton::Extra1,
        egui::PointerButton::Extra2,
    ];
    let down: Vec<_> = ctx.input(|i| {
        buttons
            .into_iter()
            .filter(|b| i.pointer.button_down(*b))
            .collect()
    });
    state.suppressed.retain(|b| down.contains(b));
    if !allowed || entering {
        for b in &down {
            if !state.suppressed.contains(b) {
                state.suppressed.push(*b);
            }
        }
    }
    if allowed && !entering {
        let events = ctx.input(|i| i.events.clone());
        for event in events {
            match event {
                Event::MouseMoved(delta) if capturing => {
                    frame.look[0] += delta.x;
                    frame.look[1] += delta.y;
                }
                Event::PointerButton {
                    button,
                    pressed,
                    modifiers,
                    ..
                } if !command(modifiers) && !state.suppressed.contains(&button) => {
                    let name = match button {
                        egui::PointerButton::Primary => "MouseLeft",
                        egui::PointerButton::Secondary => "MouseRight",
                        egui::PointerButton::Middle => "MouseMiddle",
                        egui::PointerButton::Extra1 => "Mouse4",
                        egui::PointerButton::Extra2 => "Mouse5",
                    };
                    for (action, binding) in bindings {
                        if binding == name {
                            if pressed {
                                frame.pressed.insert(action.clone());
                            } else if state.active.contains(&button) {
                                frame.released.insert(action.clone());
                            }
                        }
                    }
                    if pressed && !state.active.contains(&button) {
                        state.active.push(button);
                    }
                }
                Event::MouseWheel {
                    unit,
                    delta,
                    modifiers,
                } if !command(modifiers) => {
                    let amount = match unit {
                        egui::MouseWheelUnit::Line => delta.y,
                        egui::MouseWheelUnit::Point => delta.y / 40.,
                        egui::MouseWheelUnit::Page => delta.y.signum(),
                    };
                    let name = if amount > 0. { "WheelUp" } else { "WheelDown" };
                    let mut assigned = false;
                    if amount != 0. {
                        for (action, binding) in bindings {
                            if binding == name {
                                frame.pressed.insert(action.clone());
                                frame.released.insert(action.clone());
                                assigned = true;
                            }
                        }
                    }
                    // An explicitly bound impulse (e.g. jump) wins over camera zoom.
                    if !assigned {
                        frame.wheel += amount;
                    }
                }
                _ => {}
            }
        }
        for (button, name) in
            buttons
                .into_iter()
                .zip(["MouseLeft", "MouseRight", "MouseMiddle", "Mouse4", "Mouse5"])
        {
            if down.contains(&button) && !state.suppressed.contains(&button) {
                for (action, binding) in bindings {
                    if binding == name {
                        frame.held.insert(action.clone());
                    }
                }
            }
        }
    }
    state.active = if allowed {
        down.into_iter()
            .filter(|b| !state.suppressed.contains(b))
            .collect()
    } else {
        Vec::new()
    };
    state.enabled = allowed;
    state.relative = capturing;
    state.ignore_click = entering && relative;
    ctx.data_mut(|d| d.insert_temp(id, state));
    frame
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mouse_frame(ctx: &Context, enabled: bool, events: Vec<Event>) -> InputFrame {
        let bindings = BTreeMap::from([
            ("atacar".into(), "MouseLeft".into()),
            ("pular".into(), "WheelUp".into()),
        ]);
        let mut output = InputFrame::default();
        let _ = ctx.run(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ctx| {
                output = collect_game_input(ctx, &bindings, enabled, true);
            },
        );
        output
    }
    #[test]
    fn relative_motion_is_not_doubled_by_absolute_pointer_or_ui_scale() {
        for scale in [1., 1.75, 2.5] {
            let ctx = Context::default();
            ctx.set_zoom_factor(scale);
            mouse_frame(&ctx, true, vec![]);
            let input = mouse_frame(
                &ctx,
                true,
                vec![
                    Event::MouseMoved(egui::vec2(20., -7.)),
                    Event::PointerMoved(egui::pos2(50., 50.)),
                ],
            );
            assert_eq!(input.look, [20., -7.]);
            assert_eq!(
                mouse_frame(&ctx, false, vec![Event::MouseMoved(egui::vec2(900., 900.))]).look,
                [0.; 2]
            );
            assert_eq!(mouse_frame(&ctx, true, vec![]).look, [0.; 2]);
        }
    }
    #[test]
    fn capture_click_is_swallowed_and_wheel_is_only_an_impulse() {
        let ctx = Context::default();
        let click = |pressed| Event::PointerButton {
            pos: egui::pos2(10., 10.),
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        let input = mouse_frame(&ctx, true, vec![click(true)]);
        assert!(input.pressed.is_empty() && !accepts_game_click(&ctx));
        assert!(
            mouse_frame(&ctx, true, vec![click(false)])
                .released
                .is_empty()
        );
        let input = mouse_frame(
            &ctx,
            true,
            vec![
                click(true),
                click(false),
                Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Line,
                    delta: egui::vec2(0., 1.),
                    modifiers: Modifiers::NONE,
                },
            ],
        );
        assert!(input.pressed("atacar") && input.released.contains("atacar"));
        assert!(input.pressed("pular") && input.released.contains("pular") && !input.held("pular"));
        assert_eq!(input.wheel, 0., "Explicit jump binding wins over zoom");
        assert!(mouse_frame(&ctx, true, vec![]).pressed.is_empty());
    }

    #[test]
    fn release_transition_is_once_and_inactive_keys_do_not_replay() {
        let ctx = Context::default();
        let keys = bindings();
        let pressed = frame(
            &ctx,
            &keys,
            true,
            Modifiers::NONE,
            vec![key(Key::J, true, Modifiers::NONE)],
        );
        assert!(pressed.pressed("atacar"));
        assert!(pressed.released.is_empty());
        let released = frame(
            &ctx,
            &keys,
            true,
            Modifiers::NONE,
            vec![key(Key::J, false, Modifiers::NONE)],
        );
        assert!(released.released.contains("atacar"));
        assert!(
            frame(&ctx, &keys, true, Modifiers::NONE, vec![])
                .released
                .is_empty()
        );
        frame(
            &ctx,
            &keys,
            false,
            Modifiers::NONE,
            vec![key(Key::J, true, Modifiers::NONE)],
        );
        let resumed = frame(
            &ctx,
            &keys,
            true,
            Modifiers::NONE,
            vec![key(Key::J, false, Modifiers::NONE)],
        );
        assert!(resumed.released.is_empty() && resumed.pressed.is_empty());
        let fast = frame(
            &ctx,
            &keys,
            true,
            Modifiers::NONE,
            vec![
                key(Key::J, true, Modifiers::NONE),
                key(Key::J, false, Modifiers::NONE),
            ],
        );
        assert!(fast.pressed("atacar") && fast.released.contains("atacar"));
    }

    fn key(key: Key, pressed: bool, modifiers: Modifiers) -> Event {
        Event::Key {
            key,
            physical_key: Some(key),
            pressed,
            repeat: false,
            modifiers,
        }
    }
    fn frame(
        ctx: &Context,
        bindings: &BTreeMap<String, String>,
        enabled: bool,
        modifiers: Modifiers,
        events: Vec<Event>,
    ) -> InputFrame {
        let mut output = InputFrame::default();
        let _ = ctx.run(
            egui::RawInput {
                modifiers,
                events,
                ..Default::default()
            },
            |ctx| {
                output = collect_input(ctx, bindings, enabled);
            },
        );
        output
    }
    fn bindings() -> BTreeMap<String, String> {
        BTreeMap::from([
            ("atacar".into(), "J".into()),
            ("interagir".into(), "E".into()),
            ("mover_direita".into(), "D".into()),
        ])
    }

    #[test]
    fn quick_command_chords_do_not_emit_pressed_or_later_held_actions() {
        let ctx = Context::default();
        let bindings = bindings();
        let chord = Modifiers {
            ctrl: true,
            command: true,
            ..Modifiers::NONE
        };
        let output = frame(
            &ctx,
            &bindings,
            true,
            Modifiers::NONE,
            vec![key(Key::J, true, chord), key(Key::E, true, chord)],
        );
        assert!(output.pressed.is_empty() && output.held.is_empty());
        // CTRL was released before the frame; J/E remain physically held.
        let output = frame(&ctx, &bindings, true, Modifiers::NONE, vec![]);
        assert!(output.pressed.is_empty() && output.held.is_empty());
        frame(
            &ctx,
            &bindings,
            true,
            Modifiers::NONE,
            vec![
                key(Key::J, false, Modifiers::NONE),
                key(Key::E, false, Modifiers::NONE),
            ],
        );
        let output = frame(
            &ctx,
            &bindings,
            true,
            Modifiers::NONE,
            vec![
                key(Key::J, true, Modifiers::NONE),
                key(Key::E, true, Modifiers::NONE),
            ],
        );
        assert!(output.pressed("atacar") && output.pressed("interagir"));
        assert!(output.held("atacar") && output.held("interagir"));
    }

    #[test]
    fn ordinary_gameplay_returns_after_resume_without_replaying_paused_keys() {
        let ctx = Context::default();
        let bindings = bindings();
        let paused = frame(
            &ctx,
            &bindings,
            false,
            Modifiers::NONE,
            vec![key(Key::D, true, Modifiers::NONE)],
        );
        assert!(paused.held.is_empty());
        let resumed = frame(&ctx, &bindings, true, Modifiers::NONE, vec![]);
        assert!(resumed.held.is_empty() && resumed.pressed.is_empty());
        frame(
            &ctx,
            &bindings,
            true,
            Modifiers::NONE,
            vec![key(Key::D, false, Modifiers::NONE)],
        );
        let movement = frame(
            &ctx,
            &bindings,
            true,
            Modifiers::NONE,
            vec![key(Key::D, true, Modifiers::NONE)],
        );
        assert!(movement.held("mover_direita") && movement.pressed("mover_direita"));
        let sustained = frame(&ctx, &bindings, true, Modifiers::NONE, vec![]);
        assert!(sustained.held("mover_direita") && !sustained.pressed("mover_direita"));
    }

    #[test]
    fn focused_resume_button_allows_gameplay_and_focused_text_suppresses_it() {
        for text_focus in [false, true] {
            let ctx = Context::default();
            let bindings = bindings();
            let mut text = String::new();
            let mut output = InputFrame::default();
            let _ = ctx.run(
                egui::RawInput {
                    events: vec![key(Key::J, true, Modifiers::NONE)],
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let resume = ui.button("Retomar");
                        let edit = ui.text_edit_singleline(&mut text);
                        if text_focus {
                            edit.request_focus();
                        } else {
                            resume.request_focus();
                        }
                        assert!(ctx.wants_keyboard_input());
                        output = collect_input(ctx, &bindings, true);
                    });
                },
            );
            assert_eq!(output.pressed("atacar"), !text_focus);
            assert_eq!(output.held("atacar"), !text_focus);
        }
    }

    #[test]
    fn complete_chord_in_one_frame_and_reserved_host_keys_are_filtered() {
        let ctx = Context::default();
        let mut bindings = bindings();
        bindings.insert("escape".into(), "Escape".into());
        bindings.insert("diagnostico".into(), "F3".into());
        let chord = Modifiers {
            ctrl: true,
            command: true,
            ..Modifiers::NONE
        };
        let output = frame(
            &ctx,
            &bindings,
            true,
            Modifiers::NONE,
            vec![
                key(Key::J, true, chord),
                key(Key::J, false, Modifiers::NONE),
                key(Key::Escape, true, Modifiers::NONE),
                key(Key::F3, true, Modifiers::NONE),
            ],
        );
        assert!(output.pressed.is_empty() && output.held.is_empty());
    }
}
