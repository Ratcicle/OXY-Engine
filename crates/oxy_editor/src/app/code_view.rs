//! The read-only viewer of a script, a tab of the bottom dock that "Ver código" opens at a line:
//! the file as the disk has it, read again when the check of the scripts finds it changed, with
//! its line numbers and the lines where it fails marked. Editing stays outside OXY (an agent, or
//! VS Code).
use super::*;
use crate::theme;
use egui::RichText;

/// Height of a line of code.
const LINE_HEIGHT: f32 = 19.;

pub(crate) struct CodeView {
    /// The script, by its path in the project.
    pub(crate) path: String,
    /// The line it opened at, shown once.
    pub(crate) line: u32,
    scroll: bool,
    /// The text as last read, or why it could not be read.
    text: Result<String, String>,
    /// The hash the check of the scripts had for the file when it was read.
    hash: Option<u64>,
}

/// The script `path` of the project saved in `root`, or why it cannot be shown.
fn read_code(root: Option<&Path>, path: &str) -> Result<String, String> {
    let Some(root) = root else {
        return Err("O projeto ainda não foi salvo numa pasta.".into());
    };
    match persistence::read_script(root, path) {
        Ok(Some(text)) => Ok(text),
        Ok(None) => Err(format!("O arquivo {path} não está na pasta do projeto.")),
        Err(error) => Err(error.to_string()),
    }
}

impl Editor {
    /// Opens the script `path` in the dock's code tab, at `line` (from 1).
    pub(crate) fn open_code(&mut self, path: &str, line: u32) {
        let root = self.project_root();
        self.code_view = Some(CodeView {
            path: path.to_owned(),
            line: line.max(1),
            scroll: true,
            text: read_code(root.as_deref(), path),
            hash: self.script_status.hash(path),
        });
        // A narrow window shows the dock in the place of the side panels.
        if Self::compact_layout(&self.context) {
            self.compact_panel = CompactPanel::Library;
        }
        self.show_dock(DockTab::Code);
    }

    /// Closes the code tab.
    pub(crate) fn close_code(&mut self) {
        self.code_view = None;
        if self.dock.tab == DockTab::Code {
            self.dock.tab = DockTab::Library;
        }
    }

    /// The code tab: the file's path and that it is read only, then its lines.
    pub(crate) fn code_body(&mut self, ui: &mut egui::Ui) {
        let root = self.project_root();
        let Some(view) = self.code_view.as_mut() else {
            return;
        };
        // Changed since it was read (an agent wrote it): the new text, where it was scrolled.
        let hash = self.script_status.hash(&view.path);
        if hash != view.hash {
            view.text = read_code(root.as_deref(), &view.path);
            view.hash = hash;
        }
        // The lines where it fails on any object, run by it or by a script that requires it,
        // with what was told.
        let mut failing: Vec<(u32, String)> = Vec::new();
        for e in self.state.project.scenes.iter().flat_map(|s| &s.entities) {
            for s in &e.scripts {
                for (at, line, message) in self.script_status.errors(e, &s.path) {
                    if at == view.path && !failing.iter().any(|(l, _)| *l == line) {
                        failing.push((line, shown_message(&view.path, line, message)));
                    }
                }
            }
        }
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(&view.path)
                    .monospace()
                    .size(12.5)
                    .color(theme::TEXT_MUTED),
            );
            chip(ui, "Somente leitura", theme::BG_HEADER, theme::TEXT_MUTED);
            let errors = failing.len();
            if errors > 0 {
                let text = if errors == 1 {
                    "1 erro".to_owned()
                } else {
                    format!("{errors} erros")
                };
                chip(
                    ui,
                    &text,
                    Color32::from_rgb(107, 47, 47),
                    Color32::from_rgb(246, 201, 201),
                );
            }
        });
        ui.add_space(4.);
        let text = match &view.text {
            Ok(text) => text,
            Err(error) => {
                crate::widgets::hint(ui, error);
                return;
            }
        };
        let lines: Vec<&str> = text.lines().collect();
        // Wide enough for the longest line, which scrolls sideways.
        let widest = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0);
        let width = 70. + widest as f32 * 7.6;
        let mut area = egui::ScrollArea::both()
            .id_salt(("code_view", &view.path))
            .auto_shrink([false, false]);
        if std::mem::take(&mut view.scroll) {
            let top = (view.line.saturating_sub(4)) as f32 * LINE_HEIGHT;
            area = area.vertical_scroll_offset(top);
        }
        egui::Frame::new()
            .fill(Color32::from_rgb(28, 28, 30))
            .corner_radius(4)
            .show(ui, |ui| {
                // Rows touch, so the scroll finds each line at its height.
                ui.spacing_mut().item_spacing.y = 0.;
                area.show_rows(ui, LINE_HEIGHT, lines.len(), |ui, range| {
                    for index in range {
                        let number = index as u32 + 1;
                        let error = failing.iter().find(|(line, _)| *line == number);
                        code_line(
                            ui,
                            width,
                            number,
                            lines[index],
                            error.map(|(_, m)| m.as_str()),
                        );
                    }
                });
            });
    }
}

#[cfg(test)]
impl Editor {
    /// The script the code tab shows and the line it opened at, while the dock shows it.
    pub(crate) fn qa_code_view(&self) -> Option<(&str, u32)> {
        self.code_view
            .as_ref()
            .filter(|_| self.dock_open() && self.dock.tab == DockTab::Code)
            .map(|view| (view.path.as_str(), view.line))
    }
}

/// One line of code, at least `width` wide: its number, its text and, where it fails, a red mark
/// whose message shows on hover.
fn code_line(ui: &mut egui::Ui, width: f32, number: u32, text: &str, error: Option<&str>) {
    let width = ui.available_width().max(width);
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, LINE_HEIGHT), Sense::hover());
    let painter = ui.painter();
    if error.is_some() {
        painter.rect_filled(rect, 0., Color32::from_rgb(58, 34, 36));
        painter.rect_filled(
            Rect::from_min_size(rect.min, Vec2::new(3., rect.height())),
            0.,
            theme::ERROR,
        );
    }
    let font = egui::FontId::monospace(12.5);
    painter.text(
        egui::pos2(rect.left() + 40., rect.center().y),
        egui::Align2::RIGHT_CENTER,
        number.to_string(),
        font.clone(),
        Color32::from_rgb(94, 94, 100),
    );
    painter.text(
        egui::pos2(rect.left() + 54., rect.center().y),
        egui::Align2::LEFT_CENTER,
        text.replace('\t', "    "),
        font,
        theme::TEXT,
    );
    if let Some(error) = error {
        response.on_hover_text(error);
    }
}

/// A rounded label in `fill`, such as Somente leitura.
fn chip(ui: &mut egui::Ui, text: &str, fill: Color32, color: Color32) {
    egui::Frame::new()
        .fill(fill)
        .corner_radius(9)
        .inner_margin(egui::Margin::symmetric(8, 1))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(11.5).color(color));
        });
}

/// What failed at `line` of `path`, without the file and line the message may start with.
fn shown_message(path: &str, line: u32, message: &str) -> String {
    let place = if line > 0 {
        format!("{path}:{line}: ")
    } else {
        format!("{path}: ")
    };
    let message = message.split_once(&place).map_or(message, |(_, rest)| rest);
    if line > 0 {
        format!("Linha {line}: {message}")
    } else {
        message.to_owned()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn messages_drop_the_place_they_repeat() {
        assert_eq!(
            super::shown_message(
                "scripts/porta.luau",
                3,
                "Porta: scripts/porta.luau:3: quebrou."
            ),
            "Linha 3: quebrou."
        );
        assert_eq!(
            super::shown_message(
                "scripts/porta.luau",
                0,
                "o script scripts/porta.luau tem mais de 1 MB"
            ),
            "o script scripts/porta.luau tem mais de 1 MB"
        );
        assert_eq!(
            super::shown_message("scripts/porta.luau", 2, "faltou fechar a tabela"),
            "Linha 2: faltou fechar a tabela"
        );
    }
}
