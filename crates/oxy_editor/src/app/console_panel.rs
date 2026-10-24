//! The Console panel: the editor's messages, the agents', the test's log and its last logic
//! actions, each marked by where it came from (Editor, Agente, Jogo, Lógica, Script). A
//! script's lines (what it prints, its errors, the reloads) carry the Script mark or say what
//! failed, with the file and line as a link that opens the code tab there; an error a script
//! repeats is told once by the runtime and shows how many times it happened, and a script's
//! lines that repeat one after the other show once with their count. "Mostrar" picks which
//! lines show.
use super::*;
use crate::theme;
use egui::RichText;
use oxy_core::runtime::LogLine;
use std::collections::{HashMap, HashSet};

/// Where a line of the Console came from, or what it tells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mark {
    Editor,
    Agente,
    Jogo,
    Logica,
    Script,
    Erro,
    Aviso,
}

impl Mark {
    fn label(self) -> &'static str {
        match self {
            Self::Editor => "Editor",
            Self::Agente => "Agente",
            Self::Jogo => "Jogo",
            Self::Logica => "Lógica",
            Self::Script => "Script",
            Self::Erro => "Erro",
            Self::Aviso => "Aviso",
        }
    }

    fn color(self) -> Color32 {
        match self {
            Self::Editor => theme::TEXT_MUTED,
            Self::Agente => Color32::from_rgb(190, 160, 240),
            Self::Jogo => theme::SUCCESS,
            Self::Logica | Self::Script => theme::ACCENT_BRIGHT,
            Self::Erro => Color32::from_rgb(240, 154, 154),
            Self::Aviso => theme::WARNING,
        }
    }
}

/// Which lines the Console shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Show {
    #[default]
    All,
    Scripts,
    Problems,
    Game,
    Logic,
    Editor,
    Agents,
}

const SHOWS: [(Show, &str); 7] = [
    (Show::All, "Tudo"),
    (Show::Scripts, "Scripts"),
    (Show::Problems, "Erros e avisos"),
    (Show::Game, "Jogo"),
    (Show::Logic, "Lógica"),
    (Show::Editor, "Editor"),
    (Show::Agents, "Agente"),
];

/// A line of the Console as it shows.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Row {
    pub(crate) mark: Mark,
    pub(crate) text: String,
    /// The script line it tells of, which its link opens: the file and the line (0 when the
    /// error names none).
    pub(crate) place: Option<(String, u32)>,
    /// Whether it comes from a script or tells of one.
    pub(crate) script: bool,
    /// How many times it happened.
    pub(crate) count: u64,
    /// Whether the count shows at 1 too: a script's error.
    pub(crate) counted: bool,
}

impl Row {
    fn new(mark: Mark, text: String) -> Self {
        Self {
            mark,
            text,
            place: None,
            script: false,
            count: 1,
            counted: false,
        }
    }

    /// Whether `show` shows it.
    pub(crate) fn shows(&self, show: Show) -> bool {
        match show {
            Show::All => true,
            Show::Scripts => self.script,
            Show::Problems => matches!(self.mark, Mark::Erro | Mark::Aviso),
            Show::Game => self.mark == Mark::Jogo,
            Show::Logic => self.mark == Mark::Logica,
            Show::Editor => self.mark == Mark::Editor,
            Show::Agents => self.mark == Mark::Agente,
        }
    }
}

/// The row of an editor message: an agent's, one about a reload of scripts, or the editor's
/// own; a warning when it also became a notice.
pub(crate) fn message_row(message: &str, warning: bool) -> Row {
    let (mark, text) = match message.strip_prefix(agent::AGENT_PREFIX) {
        Some(text) => (Mark::Agente, text),
        None => (Mark::Editor, message),
    };
    let mut row = Row::new(if warning { Mark::Aviso } else { mark }, text.to_owned());
    if mark == Mark::Editor
        && let Some(place) = script_watch::reload_place(message)
    {
        if !warning {
            row.mark = Mark::Script;
        }
        row.place = place;
        row.script = true;
    }
    row
}

/// The script errors a test's runtime told, by the text its log has of each: the most times it
/// happened and where.
pub(crate) type ScriptErrors = HashMap<String, (u64, Option<(String, u32)>)>;

/// Keeps in `seen` the script errors the test `runtime` told so far. The runtime forgets an
/// error when its script reloads or its object goes, and the Console still tells it as it was.
pub(crate) fn remember_script_errors(seen: &mut ScriptErrors, runtime: &Runtime) {
    for (_, text, count) in runtime.script_errors() {
        match seen.get_mut(text) {
            Some((most, _)) => *most = (*most).max(count),
            None => {
                seen.insert(text.to_owned(), (count, None));
            }
        }
    }
    for (_, path, line, text) in runtime.script_error_lines() {
        if let Some((_, place)) = seen.get_mut(text) {
            place.get_or_insert_with(|| (path.to_owned(), line));
        }
    }
}

/// The row of a line of the test's log: a script's error (one of `errors`), what a script
/// printed or warned, or the game's own.
pub(crate) fn log_row(line: &LogLine, errors: &ScriptErrors) -> Row {
    let at = line.script.as_ref().map(|s| (s.path.clone(), s.line));
    if let Some((count, place)) = errors.get(line.text.as_str()) {
        let place = at.or_else(|| place.clone());
        // The link says where: the text goes on without it.
        let text = match &place {
            Some((path, number)) if *number > 0 => {
                line.text.replacen(&format!("{path}:{number}: "), "", 1)
            }
            _ => line.text.clone(),
        };
        return Row {
            mark: Mark::Erro,
            text,
            place,
            script: true,
            count: *count,
            counted: true,
        };
    }
    match at {
        Some(place) => {
            let warns = line
                .text
                .split_once(" · ")
                .is_some_and(|(_, said)| said.starts_with("aviso: "));
            let mark = if warns { Mark::Aviso } else { Mark::Script };
            let mut row = Row::new(mark, line.text.clone());
            row.place = Some(place);
            row.script = true;
            row
        }
        None => Row::new(Mark::Jogo, line.text.clone()),
    }
}

/// `rows` with the scripts' that repeat one after the other as one, counting them. An error
/// already counts every time it happened.
pub(crate) fn grouped(rows: Vec<Row>) -> Vec<Row> {
    let mut grouped: Vec<Row> = Vec::with_capacity(rows.len());
    for row in rows {
        match grouped.last_mut() {
            Some(last)
                if row.script
                    && last.mark == row.mark
                    && last.text == row.text
                    && last.place == row.place =>
            {
                last.count = if row.counted {
                    last.count.max(row.count)
                } else {
                    last.count + row.count
                };
            }
            _ => grouped.push(row),
        }
    }
    grouped
}

/// The Console's rows: the editor's `messages` (those also in `warnings` are warnings), then
/// the log of the test `runtime`, whose script errors are among `errors`, and its last logic
/// actions.
pub(crate) fn rows(
    messages: &[String],
    warnings: &HashSet<&str>,
    runtime: Option<&Runtime>,
    errors: &ScriptErrors,
) -> Vec<Row> {
    let mut rows: Vec<Row> = messages
        .iter()
        .map(|message| message_row(message, warnings.contains(message.as_str())))
        .collect();
    if let Some(rt) = runtime {
        rows.extend(rt.logs.iter().map(|line| log_row(line, errors)));
        let operations = oxy_core::graph::registry();
        for trace in rt.traces.iter().rev().take(5).rev() {
            let operation = operations
                .iter()
                .find(|op| op.id == trace.operation)
                .map_or("Ação não reconhecida", |op| op.label);
            let object = rt
                .scene()
                .entity(&trace.object)
                .map_or(trace.object.as_str(), |e| e.name.as_str());
            rows.push(Row::new(
                Mark::Logica,
                format!("{:.3} s · {object} · {operation}", trace.time),
            ));
        }
    }
    grouped(rows)
}

/// The link of a script line: its file name and line, with an arrow for what failed.
fn link_text(place: &(String, u32), mark: Mark) -> String {
    let name = script_status::file_name(&place.0);
    let at = if place.1 > 0 {
        format!("{name}:{}", place.1)
    } else {
        name.to_owned()
    };
    if matches!(mark, Mark::Erro | Mark::Aviso) {
        format!("{at} →")
    } else {
        at
    }
}

impl Editor {
    /// The Console's "Mostrar" menu, in the dock's row of tabs.
    pub(crate) fn console_show_menu(&mut self, ui: &mut egui::Ui) {
        let current = SHOWS
            .iter()
            .find(|(show, _)| *show == self.console_show)
            .map_or("Tudo", |(_, label)| label);
        ui.menu_button(format!("Mostrar: {current}"), |ui| {
            for (show, label) in SHOWS {
                if ui
                    .selectable_label(self.console_show == show, label)
                    .clicked()
                {
                    self.console_show = show;
                    ui.close();
                }
            }
        });
    }

    /// Editor messages, game logs and the last logic actions, each marked by its source.
    /// Messages that also became notices are shown as warnings; a script line's link opens
    /// the code tab at it.
    pub(crate) fn console_body(&mut self, ui: &mut egui::Ui) {
        let warnings: HashSet<&str> = self.notices.texts().collect();
        let rows = rows(
            &self.console.messages,
            &warnings,
            self.play.runtime.as_ref(),
            &self.console_errors,
        );
        if rows.is_empty() {
            crate::widgets::hint(
                ui,
                "Nada no console. Mensagens do editor e do jogo aparecem aqui.",
            );
            return;
        }
        // The Console of the home screen and the Animation's have no "Mostrar" and show
        // everything; no dock there shows the code tab, so their places are no links.
        let docked = !self.home.visible && !self.animating();
        let show = if docked { self.console_show } else { Show::All };
        if !rows.iter().any(|row| row.shows(show)) {
            crate::widgets::hint(ui, "Nenhuma linha para mostrar com este filtro.");
            return;
        }
        let mut open = None;
        egui::ScrollArea::vertical()
            .stick_to_bottom(true)
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.;
                for (index, row) in rows.iter().enumerate() {
                    if !row.shows(show) {
                        continue;
                    }
                    ui.push_id(index, |ui| {
                        if let Some(place) = row_ui(ui, row, docked) {
                            open = Some(place);
                        }
                    });
                }
            });
        if let Some((path, line)) = open {
            self.open_code(&path, line.max(1));
        }
    }
}

/// One line of the Console: its mark, its text, how many times and its script line, a link
/// when `links`; a script's error or warning stands out in its colour. The place, when its
/// link was clicked.
fn row_ui(ui: &mut egui::Ui, row: &Row, links: bool) -> Option<(String, u32)> {
    let problem = row.script && matches!(row.mark, Mark::Erro | Mark::Aviso);
    let (fill, edge, color) = match row.mark {
        Mark::Erro => (code_view::ERROR_FILL, theme::ERROR, code_view::ERROR_TEXT),
        _ if problem => (
            Color32::from_rgb(58, 51, 34),
            theme::WARNING,
            Color32::from_rgb(240, 224, 184),
        ),
        Mark::Aviso => (
            Color32::TRANSPARENT,
            Color32::TRANSPARENT,
            Color32::from_rgb(232, 210, 160),
        ),
        _ => (Color32::TRANSPARENT, Color32::TRANSPARENT, theme::TEXT),
    };
    let mut clicked = None;
    let frame = egui::Frame::new()
        .fill(fill)
        .corner_radius(3)
        .inner_margin(if problem {
            egui::Margin::symmetric(6, 4)
        } else {
            egui::Margin::ZERO
        })
        .show(ui, |ui| {
            ui.horizontal_top(|ui| {
                let (rect, _) = ui.allocate_exact_size(Vec2::new(52., 18.), Sense::hover());
                ui.painter().text(
                    rect.left_center(),
                    egui::Align2::LEFT_CENTER,
                    row.mark.label(),
                    theme::medium(11.),
                    row.mark.color(),
                );
                let count = (row.counted || row.count > 1).then(|| format!("×{}", row.count));
                let link = row.place.as_ref().map(|place| link_text(place, row.mark));
                let font = egui::FontId::monospace(12.);
                let measure = |text: &str| {
                    ui.painter()
                        .layout_no_wrap(text.to_owned(), font.clone(), theme::TEXT)
                        .size()
                        .x
                };
                let spacing = ui.spacing().item_spacing.x;
                let right = count.as_deref().map_or(0., |c| measure(c) + 14. + spacing)
                    + link.as_deref().map_or(0., |l| measure(l) + spacing);
                let width = (ui.available_width() - right).max(80.);
                ui.allocate_ui(Vec2::new(width, 0.), |ui| {
                    ui.set_width(width);
                    ui.add(
                        egui::Label::new(
                            RichText::new(&row.text).monospace().size(12.).color(color),
                        )
                        .wrap(),
                    );
                });
                if let Some(count) = &count {
                    let chip = if row.mark == Mark::Erro {
                        (code_view::ERROR_CHIP, code_view::ERROR_TEXT)
                    } else {
                        (theme::BG_HEADER, theme::TEXT_MUTED)
                    };
                    egui::Frame::new()
                        .fill(chip.0)
                        .corner_radius(8)
                        .inner_margin(egui::Margin::symmetric(7, 0))
                        .show(ui, |ui| {
                            ui.label(RichText::new(count).size(11.).color(chip.1));
                        });
                }
                match (&link, &row.place) {
                    (Some(link), Some(_)) if !links => {
                        ui.label(
                            RichText::new(link)
                                .monospace()
                                .size(12.)
                                .color(theme::TEXT_MUTED),
                        );
                    }
                    (Some(link), Some(place)) => {
                        let response = ui
                            .add(egui::Link::new(
                                RichText::new(link)
                                    .monospace()
                                    .size(12.)
                                    .color(theme::ACCENT_BRIGHT),
                            ))
                            .on_hover_text(format!("Ver {} na linha {}", place.0, place.1.max(1)));
                        if response.clicked() {
                            clicked = Some(place.clone());
                        }
                    }
                    _ => {}
                }
            });
        });
    if problem {
        let rect = frame.response.rect;
        ui.painter().rect_filled(
            Rect::from_min_size(rect.min, Vec2::new(3., rect.height())),
            0.,
            edge,
        );
    }
    clicked
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxy_core::runtime::{FIXED_DT, InputFrame, ScriptLocation};
    use oxy_core::script::ScriptRef;

    fn log(text: &str, script: Option<(&str, u32)>) -> LogLine {
        LogLine {
            text: text.into(),
            script: script.map(|(path, line)| ScriptLocation {
                path: path.into(),
                line,
            }),
        }
    }

    #[test]
    fn messages_are_marked_by_who_wrote_them_and_reloads_as_scripts() {
        let row = message_row("Projeto aberto e validado.", false);
        assert_eq!(
            (row.mark, row.script, row.place),
            (Mark::Editor, false, None)
        );
        let row = message_row(&format!("{}Criar objeto", agent::AGENT_PREFIX), false);
        assert_eq!(
            (row.mark, row.text.as_str()),
            (Mark::Agente, "Criar objeto")
        );
        let reloaded = "Script recarregado: scripts/porta.luau (2 objetos)";
        let row = message_row(reloaded, false);
        assert_eq!(
            (row.mark, row.text.as_str(), row.script, row.place),
            (Mark::Script, reloaded, true, None),
            "a reload keeps its text"
        );
        let failed = "Script não recarregado: scripts/porta.luau:16: erro de sintaxe. O teste \
                      segue com a versão anterior.";
        let row = message_row(failed, true);
        assert_eq!(
            (row.mark, row.text.as_str(), row.script),
            (Mark::Aviso, failed, true)
        );
        assert_eq!(row.place, Some(("scripts/porta.luau".into(), 16)));
        assert_eq!(
            link_text(row.place.as_ref().unwrap(), row.mark),
            "porta.luau:16 →"
        );
        // An agent's warning stays the agent's, whatever it says.
        let row = message_row(
            &format!("{}Script recarregado: x.luau", agent::AGENT_PREFIX),
            true,
        );
        assert_eq!((row.mark, row.script), (Mark::Aviso, false));
    }

    #[test]
    fn log_lines_of_scripts_carry_their_place_and_errors_their_count() {
        let mut errors = ScriptErrors::new();
        let error = "Porta · scripts/porta.luau:6: o atributo Nada não existe";
        errors.insert(error.into(), (37, Some(("scripts/porta.luau".into(), 6))));
        let row = log_row(&log(error, Some(("scripts/porta.luau", 6))), &errors);
        assert_eq!(
            row,
            Row {
                mark: Mark::Erro,
                text: "Porta · o atributo Nada não existe".into(),
                place: Some(("scripts/porta.luau".into(), 6)),
                script: true,
                count: 37,
                counted: true,
            }
        );
        // An error the log has without a place takes the runtime's.
        let unloaded = "o script scripts/sumiu.luau não foi encontrado";
        errors.insert(unloaded.into(), (1, Some(("scripts/sumiu.luau".into(), 0))));
        let row = log_row(&log(unloaded, None), &errors);
        assert_eq!((row.mark, row.text.as_str()), (Mark::Erro, unloaded));
        assert_eq!(
            link_text(row.place.as_ref().unwrap(), row.mark),
            "sumiu.luau →"
        );
        let printed = log(
            "Porta · entrou Jogador",
            Some(("Pista/checkpoint.luau", 12)),
        );
        let row = log_row(&printed, &errors);
        assert_eq!((row.mark, row.script, row.count), (Mark::Script, true, 1));
        assert_eq!(
            link_text(row.place.as_ref().unwrap(), row.mark),
            "checkpoint.luau:12"
        );
        let warned = log("Porta · aviso: sem chave", Some(("scripts/porta.luau", 3)));
        assert_eq!(log_row(&warned, &errors).mark, Mark::Aviso);
        let node = log("[porta / no] Abriu", None);
        let row = log_row(&node, &errors);
        assert_eq!((row.mark, row.script, row.place), (Mark::Jogo, false, None));
    }

    #[test]
    fn repeats_group_and_filters_pick_lines() {
        let printed = log("Porta · passo", Some(("scripts/porta.luau", 4)));
        let rows = grouped(vec![
            message_row("Salvo.", false),
            message_row("Salvo.", false),
            message_row("Salvo.", true),
            message_row("Script recarregado: scripts/a.luau", false),
            message_row("Script recarregado: scripts/a.luau", false),
            log_row(&printed, &ScriptErrors::new()),
            log_row(&printed, &ScriptErrors::new()),
            log_row(&printed, &ScriptErrors::new()),
            message_row("Salvo.", false),
        ]);
        let shown: Vec<(Mark, u64)> = rows.iter().map(|r| (r.mark, r.count)).collect();
        // The editor's own lines stay as they were; the scripts' group.
        assert_eq!(
            shown,
            [
                (Mark::Editor, 1),
                (Mark::Editor, 1),
                (Mark::Aviso, 1),
                (Mark::Script, 2),
                (Mark::Script, 3),
                (Mark::Editor, 1)
            ]
        );
        let marks = |show: Show| -> Vec<Mark> {
            rows.iter()
                .filter(|r| r.shows(show))
                .map(|r| r.mark)
                .collect()
        };
        assert_eq!(marks(Show::All).len(), 6);
        assert_eq!(marks(Show::Scripts), [Mark::Script, Mark::Script]);
        assert_eq!(marks(Show::Problems), [Mark::Aviso]);
        assert_eq!(
            marks(Show::Editor),
            [Mark::Editor, Mark::Editor, Mark::Editor]
        );
        assert!(marks(Show::Game).is_empty() && marks(Show::Agents).is_empty());
        // An error told again after a reload has the same text and count: it shows once, with
        // the times it happened, not twice as many.
        let mut errors = ScriptErrors::new();
        let error = "Porta · scripts/porta.luau:6: quebrou";
        errors.insert(error.into(), (5, Some(("scripts/porta.luau".into(), 6))));
        let failed = log(error, Some(("scripts/porta.luau", 6)));
        let rows = grouped(vec![log_row(&failed, &errors), log_row(&failed, &errors)]);
        let shown: Vec<(Mark, u64)> = rows.iter().map(|r| (r.mark, r.count)).collect();
        assert_eq!(shown, [(Mark::Erro, 5)]);
    }

    #[test]
    fn a_tests_script_lines_show_with_their_place_and_how_often_they_failed() {
        let root = std::env::temp_dir().join(format!("oxy-editor-console-{}", new_id()));
        std::fs::create_dir_all(root.join("scripts")).unwrap();
        std::fs::write(
            root.join("scripts/porta.luau"),
            "local Porta = {}\nfunction Porta.on_start(self)\n    print(\"pronta\")\nend\n\
             function Porta.on_step(self)\n    self.attr.Nada = 1\nend\nreturn Porta\n",
        )
        .unwrap();
        let mut project = Project::new("Console");
        let mut door = Entity::new("Porta", None);
        door.scripts = vec![ScriptRef::new("scripts/porta.luau")];
        project.scenes[0].entities.push(door);
        let options = RuntimeOptions {
            scripts: std::sync::Arc::new(persistence::load_scripts(&root).unwrap()),
            ..Default::default()
        };
        let mut runtime = Runtime::with_options(&project, &project.start_scene, options).unwrap();
        let mut errors = ScriptErrors::new();
        for _ in 0..5 {
            runtime.advance(FIXED_DT, &InputFrame::default());
            remember_script_errors(&mut errors, &runtime);
        }
        let messages = vec!["Teste iniciado.".to_owned()];
        let shown = rows(&messages, &HashSet::new(), Some(&runtime), &errors);
        let printed = shown.iter().find(|r| r.mark == Mark::Script).unwrap();
        assert_eq!(printed.text, "Porta · pronta");
        assert_eq!(printed.place, Some(("scripts/porta.luau".into(), 3)));
        let failed: Vec<&Row> = shown.iter().filter(|r| r.mark == Mark::Erro).collect();
        assert_eq!(failed.len(), 1, "told once: {shown:?}");
        assert_eq!(failed[0].place, Some(("scripts/porta.luau".into(), 6)));
        let count = failed[0].count;
        assert!(count >= 2, "repeated: {:?}", failed[0]);
        assert!(
            failed[0].text.starts_with("Porta · ") && !failed[0].text.contains("porta.luau:6"),
            "{:?}",
            failed[0]
        );
        // An agent fixes the script and it reloads: the runtime forgets the error, and the
        // Console still tells it as it was, with its place and count.
        let fixed = "local Porta = {}\nfunction Porta.on_start(self)\n    print(\"consertada\")\n\
                     end\nreturn Porta\n";
        std::fs::write(root.join("scripts/porta.luau"), fixed).unwrap();
        let sources = std::sync::Arc::new(persistence::load_scripts(&root).unwrap());
        let report = runtime.reload_scripts(sources);
        assert_eq!(report.reloaded, ["scripts/porta.luau"], "{report:?}");
        assert_eq!(runtime.script_errors().count(), 0);
        remember_script_errors(&mut errors, &runtime);
        let shown = rows(&messages, &HashSet::new(), Some(&runtime), &errors);
        let failed: Vec<&Row> = shown.iter().filter(|r| r.mark == Mark::Erro).collect();
        assert_eq!(failed.len(), 1, "{shown:?}");
        assert_eq!(failed[0].count, count);
        assert_eq!(
            link_text(failed[0].place.as_ref().unwrap(), failed[0].mark),
            "porta.luau:6 →"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
