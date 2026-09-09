//! Numeric drafts retain invalid intermediate text, but never silently accept the
//! last valid number when a user finishes an invalid field. Geometry rollback is caller-owned.
use std::{cell::Cell, ops::RangeInclusive};

#[derive(Clone, Default)]
struct Draft {
    valid: bool,
    editing: bool,
}

fn parse(text: &str, range: Option<(f64, f64)>, integer: bool) -> Option<f64> {
    let normalized: String = text
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| match c {
            '−' => '-',
            ',' => '.',
            _ => c,
        })
        .collect();
    let number = normalized.parse::<f64>().ok()?;
    if !number.is_finite()
        || !(number as f32).is_finite()
        || integer && number.fract() != 0.
        || range.is_some_and(|(min, max)| number < min || number > max)
    {
        None
    } else {
        Some(number)
    }
}

fn finish(response: &egui::Response, parsed: Option<bool>, invalid: &mut bool) {
    let key = response.id.with("oxy_numeric_draft");
    let mut draft = response
        .ctx
        .data_mut(|data| data.get_temp::<Draft>(key))
        .unwrap_or(Draft {
            valid: true,
            editing: false,
        });
    if response.gained_focus() {
        draft.valid = true;
    }
    if let Some(valid) = parsed {
        draft.valid = valid;
    }
    let escape = response.ctx.input(|i| i.key_pressed(egui::Key::Escape));
    let enter = response.ctx.input(|i| i.key_pressed(egui::Key::Enter));
    let finished = response.lost_focus() || enter && (response.has_focus() || draft.editing);
    if !escape && finished && !draft.valid {
        *invalid = true;
    }
    draft.editing = response.has_focus();
    response.ctx.data_mut(|data| {
        if finished || escape || !draft.editing {
            data.remove::<Draft>(key);
        } else {
            data.insert_temp(key, draft);
        }
    });
}

/// Mouse dragging respects `limits`; typed values are validated before egui can clamp them.
/// `invalid` becomes true only when a bad draft is completed, never for a transient "-".
pub(super) fn float(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut f32,
    speed: f64,
    limits: Option<RangeInclusive<f32>>,
    invalid: &mut bool,
) -> egui::Response {
    let before = *value;
    let parsed = Cell::new(None);
    let bounds = limits
        .as_ref()
        .map(|r| (f64::from(*r.start()), f64::from(*r.end())));
    let mut response = ui
        .push_id(label, |ui| {
            let mut widget = egui::DragValue::new(value)
                .speed(speed)
                .custom_parser(|text| {
                    let result = parse(text, bounds, false);
                    parsed.set(Some(result.is_some()));
                    result
                });
            if !label.is_empty() {
                widget = widget.prefix(format!("{label} "));
            }
            if let Some(range) = limits {
                widget = widget.range(range);
            }
            ui.add(widget)
        })
        .inner;
    // DragValue embeds TextEdit, whose text can change without yielding a valid number.
    // Consumers begin geometry gestures only when the numeric value actually changes.
    response
        .flags
        .set(egui::response::Flags::CHANGED, *value != before);
    finish(&response, parsed.get(), invalid);
    response
}

pub(super) fn integer(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut u32,
    limits: RangeInclusive<u32>,
    invalid: &mut bool,
) -> egui::Response {
    let before = *value;
    let parsed = Cell::new(None);
    let bounds = Some((f64::from(*limits.start()), f64::from(*limits.end())));
    let mut response = ui
        .push_id(label, |ui| {
            let mut widget = egui::DragValue::new(value)
                .range(limits)
                .custom_parser(|text| {
                    let result = parse(text, bounds, true);
                    parsed.set(Some(result.is_some()));
                    result
                });
            if !label.is_empty() {
                widget = widget.prefix(format!("{label} "));
            }
            ui.add(widget)
        })
        .inner;
    response
        .flags
        .set(egui::response::Flags::CHANGED, *value != before);
    finish(&response, parsed.get(), invalid);
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Field {
        ctx: egui::Context,
        value: f32,
        invalid: bool,
        rect: egui::Rect,
        time: f64,
    }
    impl Field {
        fn new() -> Self {
            let mut field = Self {
                ctx: egui::Context::default(),
                value: 70.,
                invalid: false,
                rect: egui::Rect::NOTHING,
                time: 0.,
            };
            field.frame(vec![]);
            field.frame(vec![]);
            field
        }
        fn frame(&mut self, events: Vec<egui::Event>) {
            self.time += 1. / 60.;
            self.invalid = false;
            let ctx = self.ctx.clone();
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(500., 300.),
                    )),
                    time: Some(self.time),
                    events,
                    focused: true,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        self.rect = float(
                            ui,
                            "Tamanho",
                            &mut self.value,
                            0.5,
                            Some(0.01..=100.),
                            &mut self.invalid,
                        )
                        .rect;
                        let _ = ui
                            .button("Outro campo")
                            .on_hover_text("Alvo de perda de foco.");
                    });
                },
            );
        }
        fn click(&mut self, p: egui::Pos2) -> bool {
            self.frame(vec![
                egui::Event::PointerMoved(p),
                egui::Event::PointerButton {
                    pos: p,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
            let pressed_invalid = self.invalid;
            self.frame(vec![egui::Event::PointerButton {
                pos: p,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }]);
            pressed_invalid || self.invalid
        }
        fn edit(&mut self, text: &str) {
            self.click(self.rect.center());
            self.frame(vec![]);
            self.frame(vec![egui::Event::Text(text.into())]);
        }
        fn enter(&mut self) {
            self.frame(vec![egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }]);
        }
    }

    #[test]
    fn parser_checks_original_text_before_clamp_and_supports_brazilian_decimal_comma() {
        for text in ["0", "101", "NaN", "inf", "-", "", "1e100"] {
            assert_eq!(parse(text, Some((0.01, 100.)), false), None, "{text}");
        }
        assert_eq!(parse("70", Some((0.01, 100.)), false), Some(70.));
        assert_eq!(parse("−0,25", None, false), Some(-0.25));
        assert_eq!(parse("1.5", Some((1., 16.)), true), None);
        assert_eq!(parse("16", Some((1., 16.)), true), Some(16.));
    }
    #[test]
    fn invalid_intermediate_text_only_reports_when_enter_finishes_the_field() {
        let mut field = Field::new();
        field.edit("-");
        assert!(!field.invalid);
        assert_eq!(field.value, 70.);
        field.enter();
        assert!(field.invalid);
        assert_eq!(field.value, 70.);
    }
    #[test]
    fn lost_focus_rejects_out_of_range_instead_of_clamping_or_using_previous_number() {
        let mut field = Field::new();
        field.edit("101");
        assert!(!field.invalid);
        assert_eq!(field.value, 70.);
        let p = egui::pos2(400., 250.);
        let clicked_invalid = field.click(p);
        // Focus changes can be observed one UI pass later; retain either observation.
        let first = field.invalid;
        field.frame(vec![]);
        assert!(clicked_invalid || first || field.invalid);
        assert_eq!(field.value, 70.);
    }
    #[test]
    fn corrected_draft_can_finish_without_stale_invalid_state() {
        let mut field = Field::new();
        field.edit("-");
        assert!(!field.invalid);
        field.frame(vec![
            egui::Event::Key {
                key: egui::Key::A,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers {
                    ctrl: true,
                    command: true,
                    ..egui::Modifiers::NONE
                },
            },
            egui::Event::Text("35,5".into()),
        ]);
        assert!(!field.invalid);
        assert_eq!(field.value, 35.5);
        field.enter();
        assert!(!field.invalid);
        assert_eq!(field.value, 35.5);
    }
}
