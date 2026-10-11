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
            self.notices
                .push(text.clone(), persistent, self.context.input(|i| i.time));
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
        let now = ctx.input(|i| i.time);
        let index = self
            .notices
            .entries
            .iter()
            .rposition(|e| e.unread && (e.persistent || now < e.expires));
        if let Some(index) = index {
            let entry = self.notices.entries[index].clone();
            if !entry.persistent {
                ctx.request_repaint_after(std::time::Duration::from_secs_f64(
                    (entry.expires - now).max(0.01),
                ));
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
}
