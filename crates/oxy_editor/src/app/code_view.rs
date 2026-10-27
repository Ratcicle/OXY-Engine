//! The read-only viewer of a script, a tab of the bottom dock that "Ver código" and the
//! Console's links open at a line: the file as the disk has it, with its line numbers and
//! colours, the line asked for highlighted and, under each line where the script fails, what
//! failed. It reads the file again when its text changes (the check of the scripts finds another
//! text, and the viewer itself looks at the file's size and time while it shows) and says so
//! when the file is gone. Editing stays outside OXY (an agent, or VS Code).
mod highlight;

use super::*;
use crate::icons::{self, Icon};
use crate::theme;
use egui::RichText;
use egui::text::{LayoutJob, TextFormat};
use highlight::{Piece, Pieces};
use std::ops::Range;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

/// Height of a line of code.
const LINE_HEIGHT: f32 = 19.;
/// Width of the line numbers' column, left of the code.
const GUTTER: f32 = 60.;
/// How often, in seconds, the viewer looks at its file's size and time while it shows.
const LOOK_EVERY: f64 = 0.5;
/// The red of a script's error: the block's fill, its text and the chip that counts errors.
pub(crate) const ERROR_FILL: Color32 = Color32::from_rgb(58, 34, 36);
pub(crate) const ERROR_TEXT: Color32 = Color32::from_rgb(246, 201, 201);
pub(crate) const ERROR_CHIP: Color32 = Color32::from_rgb(107, 47, 47);
/// The colours of the code: keywords, texts, comments and numbers.
const KEYWORD: Color32 = Color32::from_rgb(197, 163, 240);
const TEXT: Color32 = Color32::from_rgb(232, 184, 134);
const COMMENT: Color32 = Color32::from_rgb(126, 126, 134);
const NUMBER: Color32 = Color32::from_rgb(143, 211, 200);
const LINE_NUMBER: Color32 = Color32::from_rgb(94, 94, 100);

pub(crate) struct CodeView {
    /// The script, by its path in the project.
    pub(crate) path: String,
    /// The line asked for, from 1: scrolled to when asked and highlighted.
    pub(crate) line: u32,
    scroll: bool,
    /// The text as last read, or why it could not be read.
    code: Result<Code, String>,
    /// The text was read and then the file left the project's folder.
    deleted: bool,
    /// The hash the check of the scripts had for the file when it was read.
    hash: Option<u64>,
    /// The file's size and time when the viewer last looked, and when that was.
    stamp: Option<(u64, Option<SystemTime>)>,
    looked: f64,
    /// The first and last lines drawn, from 1, for the native checks.
    #[cfg(test)]
    shown: (u32, u32),
}

/// A script's text with what the code tab needs to draw it.
struct Code {
    text: String,
    /// Each line's bytes in the text and pieces.
    lines: Vec<(Range<usize>, Pieces)>,
    /// The most characters a line has, a tab counting as four.
    widest: usize,
}

impl Code {
    fn new(text: String) -> Self {
        // The lines as the highlighter splits them, by where each starts in the text.
        let base = text.as_ptr() as usize;
        let ranges = text.lines().map(|line| {
            let start = line.as_ptr() as usize - base;
            start..start + line.len()
        });
        let lines: Vec<(Range<usize>, Pieces)> = ranges.zip(highlight::pieces(&text)).collect();
        let widest = lines
            .iter()
            .map(|(range, _)| {
                text[range.clone()]
                    .chars()
                    .map(|c| if c == '\t' { 4 } else { 1 })
                    .sum()
            })
            .max()
            .unwrap_or(0);
        Self {
            text,
            lines,
            widest,
        }
    }
}

/// What the script `path` of the project saved in `root` says: its text, none when the folder
/// lacks it, or why it cannot be read.
fn read_code(root: Option<&Path>, path: &str) -> Result<Option<String>, String> {
    let Some(root) = root else {
        return Err("O projeto ainda não foi salvo numa pasta.".into());
    };
    persistence::read_script(root, path).map_err(|error| error.to_string())
}

/// The size and time of the script `path` of the project in `root`; none when it is not there.
fn file_stamp(root: &Path, path: &str) -> Option<(u64, Option<SystemTime>)> {
    let file = persistence::resolve_asset_path(root, path).ok()?;
    let meta = std::fs::metadata(file).ok()?;
    Some((meta.len(), meta.modified().ok()))
}

impl CodeView {
    /// Reads the file again: a text that was read and is now missing was deleted.
    fn read(&mut self, root: Option<&Path>) {
        let read = read_code(root, &self.path);
        self.deleted = matches!(read, Ok(None)) && (self.code.is_ok() || self.deleted);
        self.code = match read {
            Ok(Some(text)) => Ok(Code::new(text)),
            Ok(None) => Err(format!(
                "O arquivo {} não está na pasta do projeto.",
                self.path
            )),
            Err(error) => Err(error),
        };
    }
}

/// How the code tab says who uses a script: the `objects` that attach it, by name, or else the
/// attached `scripts` that require it.
pub(crate) fn used_by(objects: &[&str], scripts: &[&str]) -> String {
    match (objects, scripts) {
        ([one], _) => format!("Usado por {one}"),
        ([first, second], _) if first != second => format!("Usado por {first} e {second}"),
        ([], []) => "Nenhum objeto usa".into(),
        ([], [one]) => format!("Usado por {}", script_status::file_name(one)),
        ([], [first, second]) => format!(
            "Usado por {} e {}",
            script_status::file_name(first),
            script_status::file_name(second)
        ),
        ([], scripts) => format!("Usado por {} scripts", scripts.len()),
        (objects, _) => format!("Usado por {} objetos", objects.len()),
    }
}

/// A row of the code tab: a line, by its index, or the block of an error, by its place in the
/// list of the file's errors.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Row {
    Line(usize),
    Error(usize),
}

/// The rows of a file of `lines` lines whose errors are at `errors` (their lines, from 1; 0 for
/// one that names none): each error's block under its line, those of no line first and those
/// past the last line at the end.
fn rows(lines: usize, errors: &[u32]) -> Vec<Row> {
    let at = |line: usize| {
        errors
            .iter()
            .enumerate()
            .filter(move |(_, l)| **l as usize == line)
            .map(|(index, _)| Row::Error(index))
    };
    let mut rows: Vec<Row> = at(0).collect();
    for index in 0..lines {
        rows.push(Row::Line(index));
        rows.extend(at(index + 1));
    }
    rows.extend(
        errors
            .iter()
            .enumerate()
            .filter(|(_, l)| **l as usize > lines)
            .map(|(index, _)| Row::Error(index)),
    );
    rows
}

impl Editor {
    /// Opens the script `path` in the dock's code tab, at `line` (from 1).
    pub(crate) fn open_code(&mut self, path: &str, line: u32) {
        let root = self.project_root();
        let mut view = match self.code_view.take() {
            Some(view) if view.path == path => view,
            _ => CodeView {
                path: path.to_owned(),
                line: 1,
                scroll: true,
                code: Err(String::new()),
                deleted: false,
                hash: None,
                stamp: None,
                looked: 0.,
                #[cfg(test)]
                shown: (0, 0),
            },
        };
        view.line = line.max(1);
        view.scroll = true;
        view.read(root.as_deref());
        view.hash = self.script_status.hash(path);
        view.stamp = root.as_deref().and_then(|root| file_stamp(root, path));
        view.looked = self.context.input(|i| i.time);
        self.code_view = Some(view);
        // A narrow window shows the dock in the place of the side panels.
        if Self::compact_layout(&self.context) {
            self.compact_panel = CompactPanel::Library;
        }
        self.show_dock(DockTab::Code);
    }

    /// The script the code tab shows was moved or renamed by OXY to `to`: the tab follows it,
    /// and the dock stays on the tab it shows.
    pub(crate) fn code_moved(&mut self, to: &str) {
        let root = self.project_root();
        let Some(view) = self.code_view.as_mut() else {
            return;
        };
        view.path = to.to_owned();
        view.read(root.as_deref());
        view.hash = self.script_status.hash(to);
        view.stamp = root.as_deref().and_then(|root| file_stamp(root, to));
    }

    /// Closes the code tab.
    pub(crate) fn close_code(&mut self) {
        self.code_view = None;
        if self.dock.tab == DockTab::Code {
            self.dock.tab = DockTab::Library;
        }
    }

    /// The code tab: the file's path, that it is read only, who uses it, its errors and Copiar
    /// caminho, then its lines.
    pub(crate) fn code_body(&mut self, ui: &mut egui::Ui) {
        let root = self.project_root();
        let now = ui.input(|i| i.time);
        let Some(view) = self.code_view.as_mut() else {
            return;
        };
        // Written by another program (an agent, VS Code) while it shows: the check of the
        // scripts reads the folder again, and its hash tells whether the text changed.
        if now - view.looked >= LOOK_EVERY {
            view.looked = now;
            let stamp = root
                .as_deref()
                .and_then(|root| file_stamp(root, &view.path));
            if stamp != view.stamp {
                view.stamp = stamp;
                self.script_status.look_again();
            }
        }
        ui.ctx()
            .request_repaint_after(Duration::from_secs_f64(LOOK_EVERY));
        let hash = self.script_status.hash(&view.path);
        if hash != view.hash {
            view.read(root.as_deref());
            view.hash = hash;
        }
        // The lines where it fails on any object, run by it or by a script that requires it,
        // with what was told.
        let mut failing: Vec<(u32, String)> = Vec::new();
        let mut objects: Vec<&str> = Vec::new();
        for e in self.state.project.scenes.iter().flat_map(|s| &s.entities) {
            for s in &e.scripts {
                if s.path == view.path {
                    objects.push(&e.name);
                }
                for (at, line, message) in self.script_status.errors(e, &s.path) {
                    if at == view.path && !failing.iter().any(|(l, _)| *l == line) {
                        failing.push((line, shown_message(&view.path, line, message)));
                    }
                }
            }
        }
        if view.deleted {
            failing.clear();
        }
        let requiring = self.script_status.required_by(&view.path);
        let users = used_by(&objects, &requiring);
        let flag = if view.deleted {
            Some("Arquivo excluído".to_owned())
        } else {
            (!failing.is_empty()).then(|| error_count(failing.len()))
        };
        let mut copy = false;
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                copy = crate::widgets::secondary_button(ui, "Copiar caminho")
                    .on_hover_text("Copia o caminho do arquivo no projeto.")
                    .clicked();
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 10.;
                    // The chips always show: the path, then who uses it, give way to them.
                    let path_font = egui::FontId::monospace(12.5);
                    let users_font = egui::FontId::proportional(12.);
                    let gaps = if flag.is_some() { 3. } else { 2. };
                    let free = ui.available_width()
                        - chip_width(ui, "Somente leitura")
                        - flag.as_deref().map_or(0., |flag| chip_width(ui, flag))
                        - gaps * ui.spacing().item_spacing.x;
                    let path_width = text_width(ui, &view.path, path_font);
                    let users_width = text_width(ui, &users, users_font)
                        .min((free - path_width).max(free / 2.))
                        .max(0.);
                    bounded(ui, (free - users_width).max(0.), |ui| {
                        ui.add(
                            egui::Label::new(
                                RichText::new(&view.path)
                                    .monospace()
                                    .size(12.5)
                                    .color(theme::TEXT_MUTED),
                            )
                            .truncate(),
                        );
                    });
                    chip(ui, "Somente leitura", theme::BG_HEADER, theme::TEXT_MUTED);
                    bounded(ui, users_width, |ui| {
                        let users = ui.add(
                            egui::Label::new(
                                RichText::new(&users).size(12.).color(theme::TEXT_MUTED),
                            )
                            .truncate(),
                        );
                        if objects.len() > 2 {
                            users.on_hover_text(objects.join(", "));
                        }
                    });
                    if let Some(flag) = &flag {
                        chip(ui, flag, ERROR_CHIP, ERROR_TEXT);
                    }
                });
            });
        });
        if copy {
            ui.ctx().copy_text(view.path.clone());
        }
        ui.add_space(4.);
        let code = match &view.code {
            Ok(code) => code,
            Err(error) => {
                let error = if view.deleted {
                    "O arquivo foi excluído da pasta do projeto. O código volta a aparecer aqui se \
                     ele voltar."
                } else {
                    error
                };
                crate::widgets::hint(ui, error);
                return;
            }
        };
        let font = egui::FontId::monospace(12.5);
        let glyph = ui
            .painter()
            .layout_no_wrap("0".into(), font.clone(), theme::TEXT)
            .size()
            .x;
        let visible = ui.available_size();
        let width = (GUTTER + code.widest as f32 * glyph + 16.).max(visible.x - 12.);
        // Each error's message, wrapped to what the tab shows.
        let wrap = (visible.x - GUTTER - 48.).max(120.);
        let messages: Vec<Arc<egui::Galley>> = failing
            .iter()
            .map(|(_, message)| {
                ui.painter().layout(
                    message.clone(),
                    egui::FontId::proportional(12.5),
                    ERROR_TEXT,
                    wrap,
                )
            })
            .collect();
        let lines: Vec<u32> = failing.iter().map(|(line, _)| *line).collect();
        let layout: Vec<(Row, f32, f32)> = rows(code.lines.len(), &lines)
            .into_iter()
            .scan(0., |top, row| {
                let height = match row {
                    Row::Line(_) => LINE_HEIGHT,
                    Row::Error(index) => messages[index].size().y + 18.,
                };
                let at = *top;
                *top += height;
                Some((row, at, height))
            })
            .collect();
        let total = layout.last().map_or(0., |(_, top, height)| top + height);
        let mut area = egui::ScrollArea::both()
            .id_salt(("code_view", &view.path))
            .auto_shrink([false, false]);
        if std::mem::take(&mut view.scroll) {
            let asked = view.line as usize - 1;
            let top = layout
                .iter()
                .find(|(row, _, _)| *row == Row::Line(asked))
                .map_or(0., |(_, top, _)| *top);
            area = area.scroll_offset(Vec2::new(0., (top - visible.y * 0.3).max(0.)));
        }
        let asked = view.line;
        let mut shown = (0, 0);
        egui::Frame::new()
            .fill(Color32::from_rgb(28, 28, 30))
            .corner_radius(4)
            .show(ui, |ui| {
                area.show_viewport(ui, |ui, viewport| {
                    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, total), Sense::hover());
                    let painter = ui.painter();
                    let first =
                        layout.partition_point(|(_, top, height)| top + height < viewport.min.y);
                    for (row, top, height) in &layout[first..] {
                        if *top > viewport.max.y {
                            break;
                        }
                        let row_rect = Rect::from_min_size(
                            rect.min + Vec2::new(0., *top),
                            Vec2::new(width, *height),
                        );
                        match *row {
                            Row::Line(index) => {
                                let number = index as u32 + 1;
                                if shown.0 == 0 {
                                    shown.0 = number;
                                }
                                shown.1 = number;
                                let fails = lines.contains(&number);
                                let look = Look {
                                    font: &font,
                                    gutter: GUTTER,
                                    numbers: true,
                                };
                                paint_line(
                                    painter,
                                    row_rect,
                                    code,
                                    index,
                                    number == asked,
                                    fails,
                                    &look,
                                );
                            }
                            Row::Error(index) => paint_error(painter, row_rect, &messages[index]),
                        }
                    }
                });
            });
        #[cfg(test)]
        {
            view.shown = shown;
        }
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

    /// The code tab's text as last read, the first and last lines it drew, and whether its file
    /// was deleted.
    pub(crate) fn qa_code_state(&self) -> Option<(Option<&str>, (u32, u32), bool)> {
        self.code_view.as_ref().map(|view| {
            let text = view.code.as_ref().ok().map(|code| code.text.as_str());
            (text, view.shown, view.deleted)
        })
    }
}

/// How lines of code are drawn: their font, where the text starts and whether their numbers
/// show, right-aligned before it.
struct Look<'f> {
    font: &'f egui::FontId,
    gutter: f32,
    numbers: bool,
}

/// One line of code in `rect`: its number, then its text in colours. The line asked for is
/// highlighted, and the number of one that fails is red.
fn paint_line(
    painter: &egui::Painter,
    rect: Rect,
    code: &Code,
    index: usize,
    asked: bool,
    fails: bool,
    look: &Look,
) {
    let font = look.font;
    if asked {
        painter.rect_filled(rect, 0., theme::ACCENT_FILL);
        painter.rect_filled(
            Rect::from_min_size(rect.min, Vec2::new(3., rect.height())),
            0.,
            theme::ACCENT,
        );
    }
    if look.numbers {
        painter.text(
            egui::pos2(rect.left() + look.gutter - 14., rect.center().y),
            egui::Align2::RIGHT_CENTER,
            (index + 1).to_string(),
            font.clone(),
            if fails { theme::ERROR } else { LINE_NUMBER },
        );
    }
    let (range, pieces) = &code.lines[index];
    let line = &code.text[range.clone()];
    let mut job = LayoutJob::default();
    for (piece_range, piece) in pieces {
        let color = match piece {
            Piece::Plain => theme::TEXT,
            Piece::Keyword => KEYWORD,
            Piece::Text => TEXT,
            Piece::Comment => COMMENT,
            Piece::Number => NUMBER,
        };
        job.append(
            &line[piece_range.clone()].replace('\t', "    "),
            0.,
            TextFormat {
                font_id: font.clone(),
                color,
                italics: *piece == Piece::Comment,
                ..Default::default()
            },
        );
    }
    let galley = painter.layout_job(job);
    let top = rect.center().y - galley.size().y / 2.;
    painter.galley(
        egui::pos2(rect.left() + look.gutter, top),
        galley,
        theme::TEXT,
    );
}

/// A script's text as the Guide shows it, read only: in a dark block, in the code tab's colours,
/// with its line numbers when `numbers`; a line wider than the page scrolls sideways.
pub(crate) fn snippet(ui: &mut egui::Ui, id: impl std::hash::Hash, text: &str, numbers: bool) {
    let code = Code::new(text.trim_end().to_owned());
    let font = egui::FontId::monospace(12.);
    let glyph = ui
        .painter()
        .layout_no_wrap("0".into(), font.clone(), theme::TEXT)
        .size()
        .x;
    let look = Look {
        font: &font,
        gutter: if numbers { 42. } else { 10. },
        numbers,
    };
    let height = code.lines.len() as f32 * LINE_HEIGHT;
    egui::Frame::new()
        .fill(Color32::from_rgb(30, 30, 32))
        .stroke(egui::Stroke::new(1., theme::CARD_BORDER))
        .corner_radius(6)
        .inner_margin(egui::Margin::symmetric(0, 6))
        .show(ui, |ui| {
            let width = (look.gutter + code.widest as f32 * glyph + 12.).max(ui.available_width());
            egui::ScrollArea::horizontal()
                .id_salt(id)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    let (rect, _) =
                        ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
                    for index in 0..code.lines.len() {
                        let row = Rect::from_min_size(
                            rect.min + Vec2::new(0., index as f32 * LINE_HEIGHT),
                            Vec2::new(width, LINE_HEIGHT),
                        );
                        paint_line(ui.painter(), row, &code, index, false, false, &look);
                    }
                });
        });
}

/// The block under a line that fails, in `rect`: what failed, with a red edge.
fn paint_error(painter: &egui::Painter, rect: Rect, message: &Arc<egui::Galley>) {
    let block = Rect::from_min_size(
        rect.min + Vec2::new(GUTTER, 2.),
        Vec2::new(message.size().x + 20., rect.height() - 6.),
    );
    painter.rect_filled(block, 3., ERROR_FILL);
    painter.rect_filled(
        Rect::from_min_size(block.min, Vec2::new(3., block.height())),
        0.,
        theme::ERROR,
    );
    painter.galley(block.min + Vec2::new(10., 6.), message.clone(), ERROR_TEXT);
}

/// "1 erro", "3 erros".
fn error_count(count: usize) -> String {
    if count == 1 {
        "1 erro".into()
    } else {
        format!("{count} erros")
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

/// How wide [`chip`] draws `text`.
fn chip_width(ui: &egui::Ui, text: &str) -> f32 {
    text_width(ui, text, egui::FontId::proportional(11.5)) + 16.
}

/// How wide `text` is in `font`, on one line.
fn text_width(ui: &egui::Ui, text: &str, font: egui::FontId) -> f32 {
    ui.painter()
        .layout_no_wrap(text.to_owned(), font, theme::TEXT)
        .size()
        .x
}

/// `add_contents` in at most `width`, taking only what it uses.
fn bounded(ui: &mut egui::Ui, width: f32, add_contents: impl FnOnce(&mut egui::Ui)) {
    ui.allocate_ui_with_layout(
        Vec2::new(width, ui.available_height()),
        egui::Layout::left_to_right(egui::Align::Center),
        add_contents,
    );
}

/// The code tab in the dock's row of tabs: the file's name and its close ×, underlined while it
/// shows. Whether the tab and the × were clicked.
pub(crate) fn code_tab(ui: &mut egui::Ui, name: &str, selected: bool) -> (bool, bool) {
    let font = egui::FontId::monospace(12.5);
    let width = ui
        .painter()
        .layout_no_wrap(name.into(), font.clone(), theme::TEXT)
        .size()
        .x;
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(12. + width + 4., theme::PANEL_HEADER_HEIGHT),
        Sense::click(),
    );
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, name)
    });
    let color = if selected || response.hovered() {
        theme::TEXT
    } else {
        theme::TEXT_MUTED
    };
    ui.painter().text(
        egui::pos2(rect.left() + 12., rect.center().y),
        egui::Align2::LEFT_CENTER,
        name,
        font,
        color,
    );
    let close = icons::small(ui, Icon::Close, "Fechar código", 20.);
    if selected {
        ui.painter().hline(
            rect.left() + 6.0..=close.rect.right(),
            rect.bottom() - 1.,
            egui::Stroke::new(2., theme::ACCENT),
        );
    }
    (response.clicked(), close.clicked())
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
    use super::*;

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

    #[test]
    fn who_uses_a_script_is_said_by_name_then_by_count() {
        assert_eq!(used_by(&["Porta"], &[]), "Usado por Porta");
        assert_eq!(
            used_by(&["Porta", "Luz"], &["scripts/x.luau"]),
            "Usado por Porta e Luz"
        );
        // Two of the same name, or more than two, by how many.
        assert_eq!(
            used_by(&["Checkpoint", "Checkpoint"], &[]),
            "Usado por 2 objetos"
        );
        assert_eq!(used_by(&["A", "B", "C"], &[]), "Usado por 3 objetos");
        // A module no object attaches: the scripts that require it.
        assert_eq!(
            used_by(&[], &["scripts/porta.luau"]),
            "Usado por porta.luau"
        );
        assert_eq!(
            used_by(&[], &["scripts/porta.luau", "Pista/luz.luau"]),
            "Usado por porta.luau e luz.luau"
        );
        assert_eq!(
            used_by(&[], &["a.luau", "b.luau", "c.luau"]),
            "Usado por 3 scripts"
        );
        assert_eq!(used_by(&[], &[]), "Nenhum objeto usa");
        assert_eq!(error_count(1), "1 erro");
        assert_eq!(error_count(4), "4 erros");
    }

    #[test]
    fn errors_show_under_their_line() {
        use Row::*;
        // Errors at lines 2, 0 (none named), 2 again from another object, and 9 of a file of 3.
        assert_eq!(
            rows(3, &[2, 0, 2, 9]),
            [
                Error(1),
                Line(0),
                Line(1),
                Error(0),
                Error(2),
                Line(2),
                Error(3)
            ]
        );
        assert_eq!(rows(2, &[]), [Line(0), Line(1)]);
    }

    #[test]
    fn the_text_keeps_each_lines_place_and_pieces() {
        let code = Code::new("local a = 1\r\n\tif a then\n\nreturn a".into());
        let lines: Vec<&str> = code
            .lines
            .iter()
            .map(|(range, _)| &code.text[range.clone()])
            .collect();
        assert_eq!(lines, ["local a = 1", "\tif a then", "", "return a"]);
        assert_eq!(code.widest, 13, "a tab counts as four");
        assert_eq!(code.lines[1].1[1], (1..3, Piece::Keyword));
        assert!(code.lines[2].1.is_empty());
        // A last line that ends the text with its break is no extra line.
        assert_eq!(Code::new("a\nb\n".into()).lines.len(), 2);
    }

    #[test]
    fn a_file_read_then_gone_is_deleted_and_one_never_read_is_not() {
        let root = std::env::temp_dir().join(format!("oxy-editor-code-view-{}", new_id()));
        std::fs::create_dir_all(root.join("scripts")).unwrap();
        std::fs::write(root.join("scripts/porta.luau"), "return {}\n").unwrap();
        let mut view = CodeView {
            path: "scripts/porta.luau".into(),
            line: 1,
            scroll: false,
            code: Err(String::new()),
            deleted: false,
            hash: None,
            stamp: None,
            looked: 0.,
            #[cfg(test)]
            shown: (0, 0),
        };
        view.read(Some(&root));
        assert!(view.code.is_ok() && !view.deleted);
        let stamp = file_stamp(&root, &view.path);
        assert!(stamp.is_some_and(|(len, _)| len == 10), "{stamp:?}");
        std::fs::remove_file(root.join("scripts/porta.luau")).unwrap();
        assert_eq!(file_stamp(&root, &view.path), None);
        view.read(Some(&root));
        assert!(view.deleted && view.code.is_err());
        // Read again while gone, it stays deleted; back, it shows again.
        view.read(Some(&root));
        assert!(view.deleted);
        std::fs::write(root.join("scripts/porta.luau"), "return 1\n").unwrap();
        view.read(Some(&root));
        assert!(!view.deleted && view.code.as_ref().is_ok_and(|c| c.text == "return 1\n"));
        // A script never there is missing, not deleted.
        view.path = "scripts/nunca.luau".into();
        view.code = Err(String::new());
        view.deleted = false;
        view.read(Some(&root));
        assert!(!view.deleted);
        assert_eq!(
            view.code.as_ref().err().map(String::as_str),
            Some("O arquivo scripts/nunca.luau não está na pasta do projeto.")
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
