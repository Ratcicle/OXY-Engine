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
    open: bool,
}
impl Notices {
    pub(super) fn has_unread(&self) -> bool {
        self.entries.iter().any(|e| e.unread)
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
        if let Some(text) = self.messages.last() {
            self.notices
                .push(text.clone(), persistent, self.context.input(|i| i.time));
        }
    }
    pub(crate) fn warn(&mut self, text: impl Into<String>) {
        self.log(text);
        self.notice_last(false);
    }
    pub(super) fn notices_button(&mut self, ui: &mut egui::Ui) {
        let unread = self.notices.entries.iter().filter(|e| e.unread).count();
        if ui
            .add_sized(
                [96., ui.spacing().interact_size.y],
                egui::Button::new(format!("Avisos ({unread})")),
            )
            .on_hover_text("Histórico de avisos e operações recusadas.")
            .clicked()
        {
            self.notices.open = !self.notices.open;
        }
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
            if !ctx.input(|i| i.pointer.any_down()) {
                egui::Area::new("operation_notice".into())
                    .order(egui::Order::Foreground)
                    .anchor(egui::Align2::RIGHT_BOTTOM, [-12., -12.])
                    .show(ctx, |ui| {
                        egui::Frame::popup(ui.style()).show(ui, |ui| {
                            ui.set_max_width((ctx.content_rect().width() - 40.).clamp(100., 360.));
                            ui.colored_label(
                                if entry.persistent {
                                    Color32::LIGHT_RED
                                } else {
                                    Color32::LIGHT_YELLOW
                                },
                                &entry.text,
                            );
                            ui.horizontal(|ui| {
                                if entry.count > 1 {
                                    ui.weak(format!("{} ocorrências", entry.count));
                                }
                                if ui.button("Ver detalhes").clicked() {
                                    self.console = true;
                                    self.notices.entries[index].unread = false;
                                }
                                if ui.small_button("Dispensar").clicked() {
                                    self.notices.entries[index].unread = false;
                                }
                            });
                        });
                    });
            }
        }
        if self.notices.open {
            egui::Window::new("Avisos")
                .open(&mut self.notices.open)
                .default_width(380.)
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(ctx.content_rect().height() * 0.6)
                        .show(ui, |ui| {
                            for entry in self.notices.entries.iter_mut().rev() {
                                entry.unread = false;
                                ui.label(&entry.text);
                                if entry.count > 1 {
                                    ui.weak(format!("{} ocorrências", entry.count));
                                }
                                ui.separator();
                            }
                        });
                    if ui.button("Ver detalhes no Console").clicked() {
                        self.console = true;
                    }
                });
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
