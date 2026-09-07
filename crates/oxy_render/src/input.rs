//! One native input boundary shared by editor Play and the standalone player.
//! The core receives named actions and never reads egui or editing state.
use egui::{Context, Event, Key, Modifiers};
use oxy_core::runtime::InputFrame;
use std::collections::{BTreeMap, HashSet};

#[derive(Clone, Default)]
struct CaptureState {
    suppressed: HashSet<Key>,
    enabled: bool,
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
pub fn collect_input(
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
            }
        }
    }
    let capture = enabled && focused && !text_input_active(ctx) && !command(modifiers);
    if !capture {
        state.suppressed.extend(down.iter().copied());
        state.suppressed.extend(ordinary_pressed.iter().copied());
        state.enabled = false;
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
    }
    ctx.data_mut(|data| data.insert_temp(state_id, state));
    frame
}

#[cfg(test)]
mod tests {
    use super::*;

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
