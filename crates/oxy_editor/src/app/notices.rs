//! Bounded UI-only notices. Logging runtime diagnostics never opens a panel.
use super::*;

#[derive(Clone)]
struct Notice {
    text: String,
    count: u32,
    persistent: bool,
    unread: bool,
    expires: f64,
    last_seen: f64,
}
#[derive(Default)]
pub(super) struct Notices {
    entries: Vec<Notice>,
}
impl Notices {
    /// Texts of the notices still in the history.
    pub(super) fn texts(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|e| e.text.as_str())
    }
    pub(super) fn unread(&self) -> usize {
        self.entries.iter().filter(|e| e.unread).count()
    }
    /// The notice shown at `now`: the last unread one, a timed one only until it expires.
    fn shown(&self, now: f64) -> Option<usize> {
        self.entries
            .iter()
            .rposition(|e| e.unread && (e.persistent || now < e.expires))
    }
    fn push(&mut self, text: String, persistent: bool, now: f64) {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.text == text) {
            if now - entry.last_seen > 1. {
                entry.expires = now + 7.;
            }
            entry.last_seen = now;
            entry.count = entry.count.saturating_add(1);
            entry.persistent |= persistent;
            entry.unread = true;
            // A held invalid gesture must not renew a toast indefinitely.
        } else {
            self.entries.push(Notice {
                text,
                count: 1,
                persistent,
                unread: true,
                expires: now + 7.,
                last_seen: now,
            });
            if self.entries.len() > 64 {
                self.entries.remove(0);
            }
        }
    }
}
impl Editor {
    pub(super) fn notice_last(&mut self, persistent: bool) {
        if let Some(text) = self.console.messages.last() {
            let now = raised_at(self.context.input(|i| i.time), self.frame.started);
            self.notices.push(text.clone(), persistent, now);
        }
    }
    /// Notices not yet seen in the Avisos tab.
    pub(crate) fn unread_notices(&self) -> usize {
        self.notices.unread()
    }
    pub(crate) fn warn(&mut self, text: impl Into<String>) {
        self.log(text);
        self.notice_last(false);
    }
    /// What a rename or a deletion of the person's checks the scripts against: the project as
    /// it is before the edit and the scripts of its folder; none without a folder or scripts.
    pub(crate) fn script_names_before(&self) -> Option<(Project, ScriptSources)> {
        let root = self.project_root()?;
        let sources = persistence::load_scripts(&root).ok()?;
        (!sources.files.is_empty()).then(|| (self.state.project.clone(), sources))
    }
    /// Warns in the Console of the names of `kinds` the scripts give with literal texts that the
    /// edit since `before` (see [`Self::script_names_before`]) took away, with the line and the
    /// new name, as an agent's batch tells them: the game would stop those calls.
    pub(crate) fn warn_script_names(
        &mut self,
        before: Option<(Project, ScriptSources)>,
        kinds: &[oxy_core::script::NameKind],
    ) {
        let Some((before, sources)) = before else {
            return;
        };
        let broken =
            oxy_core::script::broken_citations(&before, &self.state.project, &sources, kinds);
        for broken in broken {
            self.warn(broken.warning.to_string());
        }
    }
    /// Notice history, shown in the dock's Avisos tab. Viewing it marks entries as read.
    pub(crate) fn notices_body(&mut self, ui: &mut egui::Ui) {
        if self.notices.entries.is_empty() {
            crate::widgets::hint(
                ui,
                "Nenhum aviso. Operações recusadas e falhas aparecem aqui com o motivo.",
            );
            return;
        }
        egui::ScrollArea::vertical().show(ui, |ui| {
            for entry in self.notices.entries.iter_mut().rev() {
                entry.unread = false;
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(
                        if entry.persistent {
                            crate::theme::ERROR
                        } else {
                            crate::theme::WARNING
                        },
                        "●",
                    );
                    ui.label(&entry.text);
                    if entry.count > 1 {
                        ui.weak(format!("{} ocorrências", entry.count));
                    }
                });
            }
        });
    }
    pub(super) fn notices_ui(&mut self, ctx: &egui::Context) {
        let (now, frame) = ctx.input(|i| (i.time, i.predicted_dt));
        if let Some(index) = self.notices.shown(now) {
            let entry = self.notices.entries[index].clone();
            if !entry.persistent {
                ctx.request_repaint_after(until_expiry(entry.expires - now, frame));
            }
            {
                use crate::theme;
                // Keep the buttons registered on both press and release. Omitting the area
                // while a pointer is down prevents Dismiss/Details from receiving a click.
                egui::Area::new("operation_notice".into())
                    // Above the scene, below modal dialogs and their input-blocking backdrop.
                    // A long notice must never cover Cancel/Apply in a small scaled window,
                    // nor the status bar: it sits just above it.
                    .order(egui::Order::Middle)
                    .anchor(
                        egui::Align2::RIGHT_BOTTOM,
                        [-12., -(theme::STATUS_HEIGHT + 10.)],
                    )
                    .show(ctx, |ui| {
                        egui::Frame::new()
                            .fill(theme::BG_RAISED)
                            .stroke(egui::Stroke::new(1., theme::BORDER))
                            .corner_radius(8)
                            .inner_margin(egui::Margin::same(10))
                            .shadow(egui::epaint::Shadow {
                                offset: [0, 6],
                                blur: 20,
                                spread: 0,
                                color: egui::Color32::from_black_alpha(110),
                            })
                            .show(ui, |ui| {
                                let width = (ctx.content_rect().width() - 40.).clamp(100., 340.);
                                ui.set_max_width(width);
                                ui.horizontal_top(|ui| {
                                    let (icon, _) =
                                        ui.allocate_exact_size(Vec2::splat(16.), Sense::hover());
                                    let color = if entry.persistent {
                                        theme::ERROR
                                    } else {
                                        theme::WARNING
                                    };
                                    crate::widgets::warning_icon(ui, icon, color);
                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new(&entry.text)
                                                .size(12.5)
                                                .color(theme::TEXT),
                                        )
                                        .wrap(),
                                    );
                                });
                                ui.add_space(4.);
                                ui.horizontal(|ui| {
                                    if entry.count > 1 {
                                        ui.label(
                                            egui::RichText::new(format!(
                                                "{} ocorrências",
                                                entry.count
                                            ))
                                            .size(11.5)
                                            .color(theme::TEXT_MUTED),
                                        );
                                    }
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if ui.button("Ver detalhes").clicked() {
                                                self.show_dock(DockTab::Notices);
                                                self.notices.entries[index].unread = false;
                                            }
                                            if ui
                                                .add(egui::Button::new("Dispensar").frame(false))
                                                .clicked()
                                            {
                                                self.notices.entries[index].unread = false;
                                            }
                                        },
                                    );
                                });
                            });
                    });
            }
        }
    }
}
/// The egui time of a notice raised now in the frame whose time is `time`, taken when it
/// `started`: a file dialog or a project loading earlier in the frame (Abrir projeto…) must not
/// take a timed notice's seconds.
fn raised_at(time: f64, started: Instant) -> f64 {
    time + started.elapsed().as_secs_f64()
}
/// The wait for the frame that takes away a timed notice `remaining` seconds from its expiry.
/// egui brings a timed repaint one predicted frame (`frame`) early; asked for the expiry
/// itself, the last frame before it would be drawn again and again until it passes.
fn until_expiry(remaining: f64, frame: f32) -> std::time::Duration {
    std::time::Duration::from_secs_f64(remaining.max(0.01) + f64::from(frame.max(0.)))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn notices_group_expire_and_bound_retention() {
        let mut notices = Notices::default();
        for i in 0..100 {
            notices.push("Recusada".into(), false, i as f64);
        }
        assert_eq!(notices.entries.len(), 1);
        assert_eq!(notices.entries[0].count, 100);
        assert_eq!(notices.entries[0].expires, 7.);
        notices.push("Falha ao salvar".into(), true, 200.);
        assert!(notices.entries[1].persistent);
        for i in 0..80 {
            notices.push(format!("Aviso {i}"), false, 300.);
        }
        assert_eq!(notices.entries.len(), 64);
    }

    /// A timed notice wakes the editor once, at its expiry: egui's repaint comes no earlier, so
    /// the frames before it are not drawn back to back until the expiry passes.
    #[test]
    fn a_timed_notice_wakes_the_editor_at_its_expiry() {
        let ctx = egui::Context::default();
        let frame = 1. / 60.;
        let delay = |remaining: Option<f64>| {
            let input = egui::RawInput {
                predicted_dt: frame,
                ..Default::default()
            };
            let output = ctx.run(input, |ctx| {
                if let Some(remaining) = remaining {
                    ctx.request_repaint_after(until_expiry(remaining, frame));
                }
            });
            output.viewport_output[&egui::ViewportId::ROOT].repaint_delay
        };
        // The first frames settle.
        assert!((0..8).any(|_| delay(None) == std::time::Duration::MAX));
        for remaining in [6.5, 0.5, 0.004] {
            let after = delay(Some(remaining)).as_secs_f64();
            assert!(after >= remaining.max(0.01) - 1e-6, "{remaining}: {after}");
        }
    }

    /// Opening a project of the old layout tells what its first save will do in a notice that
    /// goes away by itself, as the other timed notices, while the Library's banner keeps saying
    /// it; why a project stays in the old layout waits to be dismissed. Both stay in Avisos.
    #[test]
    fn the_old_layout_notice_goes_by_itself_unless_the_project_stays_old() {
        let root = std::env::temp_dir().join(format!("oxy-editor-old-layout-{}", new_id()));
        std::fs::create_dir_all(&root).unwrap();
        let project = editing::blank_project("Antigo", SceneKind::TwoD).unwrap();
        for (file, stays) in [(persistence::PROJECT_FILE, false), ("meu.json", true)] {
            let told = conversion::preview(&root.join(file), &project);
            assert!(told.text.contains("formato antigo"), "{told:?}");
            assert_eq!(told.informational, !stays, "{told:?}");
            let mut notices = Notices::default();
            // As opening the project raises it.
            notices.push(told.text, !told.informational, 10.);
            assert_eq!(notices.shown(16.9), Some(0));
            assert_eq!(notices.shown(17.1), stays.then_some(0), "{file}");
            assert_eq!(notices.unread(), 1);
            assert_eq!(notices.texts().count(), 1);
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A notice raised after a file dialog held the frame (Abrir projeto…, then the old layout's
    /// notice) counts its seconds from when it is raised, not from when the frame started: the
    /// next frame still shows it, for the whole 7 s.
    #[test]
    fn a_notice_raised_late_in_a_frame_keeps_its_seconds() {
        let held = std::time::Duration::from_secs(9);
        let Some(started) = Instant::now().checked_sub(held) else {
            return;
        };
        let now = raised_at(10., started);
        assert!(now >= 19., "{now}");
        let mut notices = Notices::default();
        notices.push("Formato antigo".into(), false, now);
        // The frame after the dialog.
        assert_eq!(notices.shown(19.02), Some(0));
        assert_eq!(notices.shown(now + 6.9), Some(0));
        assert_eq!(notices.shown(now + 7.1), None);
    }
}
