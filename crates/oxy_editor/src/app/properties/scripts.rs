//! The Scripts card of the Properties: the scripts the selected object attaches, in the order
//! they run, each with its Ligado switch, how many objects use it and whether it fails, Ver
//! código and a menu (Desanexar, Mover para cima, Mover para baixo); then "+ Anexar script",
//! the project's scripts by folder. The person does not write code here: an agent writes the
//! scripts, and the card shows, attaches, switches and opens them. Each change is a command
//! with its own undo step.
use super::*;
use crate::app::script_status::{self, file_name};
use crate::icons::{self, Icon};
use crate::theme;
use egui::RichText;
use oxy_core::command::{self, Command};
use std::collections::BTreeMap;

/// What the Scripts card asked for, done once the panel's edits are stored.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ScriptAction {
    Attach(String),
    Detach(String),
    Switch(String, bool),
    /// Moves the script to that place of the object's list.
    Move(String, usize),
    /// Opens the script at that line.
    View(String, u32),
}

/// A script of the card, as drawn.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ScriptRow {
    pub(crate) path: String,
    pub(crate) enabled: bool,
    /// How many objects of the project attach it.
    pub(crate) users: usize,
    /// How it fails, in red; none when it does not.
    pub(crate) failure: Option<String>,
    /// Where Ver código opens: the script, or the one it requires where it fails first, and
    /// the line.
    pub(crate) view: (String, u32),
}

impl Editor {
    /// The rows of the Scripts card for `entity`.
    fn script_rows(&self, entity: &Entity) -> Vec<ScriptRow> {
        entity
            .scripts
            .iter()
            .map(|s| {
                let errors = self.script_status.errors(entity, &s.path);
                let places: Vec<(&str, u32)> =
                    errors.iter().map(|(at, line, _)| (*at, *line)).collect();
                let missing = self.script_status.missing(&s.path);
                let (at, line) = script_status::first_line(&places).unwrap_or((&s.path, 1));
                ScriptRow {
                    path: s.path.clone(),
                    enabled: s.enabled,
                    users: oxy_core::script::attached_by(&self.state.project, &s.path),
                    failure: script_status::failure(&s.path, missing, &places),
                    view: (at.to_owned(), line),
                }
            })
            .collect()
    }

    /// The Scripts card of `entity`; what it asked for.
    pub(super) fn scripts_card(
        &mut self,
        ui: &mut egui::Ui,
        entity: &Entity,
    ) -> Option<ScriptAction> {
        let rows = self.script_rows(entity);
        let failing = rows.iter().any(|row| row.failure.is_some());
        let mut action = None;
        let has_graph = !entity.graph.nodes.is_empty();
        let saved = self.path.is_some();
        let root = self.state.project.name.clone();
        let files = &self.script_status.files;
        let search = &mut self.script_search;
        let mut opened = false;
        crate::widgets::open_card_with(
            ui,
            ("scripts", &entity.id),
            "Scripts",
            &rows.len().to_string(),
            failing.then_some(theme::ERROR),
            |ui| {
                if rows.is_empty() {
                    crate::widgets::hint(
                        ui,
                        if has_graph {
                            "Nenhum script. A lógica deste objeto está em nós (legado), na aba \
                             Lógica."
                        } else {
                            "Nenhum script."
                        },
                    );
                }
                action = script_list(ui, &rows);
                let menu = AttachMenu {
                    files,
                    attached: &entity.scripts,
                    root: &root,
                    saved,
                };
                let (clicked, chosen) = attach_menu(ui, &menu, search);
                opened = clicked;
                if let Some(path) = chosen {
                    action = Some(ScriptAction::Attach(path));
                }
            },
        );
        // The list offered is the folder as it is now.
        if opened {
            self.script_status.look_again();
        }
        action
    }

    /// Does what the Scripts card of the object `entity` asked for, as its own undo step named
    /// after it; what it warns, and a refusal, go to the Console and Avisos.
    pub(super) fn run_script_action(&mut self, entity: &Id, action: ScriptAction) {
        let command = match action {
            ScriptAction::View(path, line) => {
                self.open_code(&path, line);
                return;
            }
            ScriptAction::Move(path, to) => {
                self.move_script(entity, &path, to);
                return;
            }
            ScriptAction::Attach(path) => Command::AttachScript {
                scene: None,
                entity: entity.clone(),
                path,
                index: None,
            },
            ScriptAction::Detach(path) => Command::DetachScript {
                scene: None,
                entity: entity.clone(),
                path,
            },
            ScriptAction::Switch(path, enabled) => Command::SetScriptEnabled {
                scene: None,
                entity: entity.clone(),
                path,
                enabled,
            },
        };
        let label = command.label();
        let op = command.op();
        let commands = [command];
        let scripts = match self.project_root() {
            Some(root) if command::needs_scripts(&commands) => {
                match persistence::load_scripts(&root) {
                    Ok(scripts) => scripts,
                    Err(error) => {
                        self.warn(format!("{label}: {error}"));
                        return;
                    }
                }
            }
            _ => ScriptSources::default(),
        };
        self.finish_history(true);
        self.history
            .begin(label, &self.state.project, &self.state.images);
        let scene = self.scene_id.clone();
        let attributes = |editor: &Self| {
            editor
                .scene()
                .entity(entity)
                .map_or(0, |e| e.attributes.len())
        };
        let before = attributes(self);
        match command::apply_with_scripts(&mut self.state.project, &scene, &commands, &scripts) {
            Ok(applied) => {
                if let Err(error) = self
                    .history
                    .commit(&self.state.project, &mut self.state.images)
                {
                    self.log(error);
                }
                // The attributes the script declared show in their card, each with its note.
                if attributes(self) > before {
                    crate::widgets::request_card_open(&self.context, "Atributos");
                }
                // Said as in the agents' answers, without the batch's place.
                let place = format!("Comando 0 ({op}, o 1º do lote): ");
                for warning in applied.into_iter().flat_map(|a| a.warnings) {
                    let warning = warning.strip_prefix(&place).unwrap_or(&warning).to_owned();
                    self.warn(warning);
                }
            }
            Err(error) => {
                self.history
                    .cancel(&mut self.state.project, &mut self.state.images);
                self.warn(format!("{label}: {}", error.message));
            }
        }
        // What the panel changes next is another edit.
        self.history
            .begin("Editar projeto", &self.state.project, &self.state.images);
    }

    /// Moves the script `path` of the object `entity` to the place `to` of its list, as its own
    /// undo step.
    fn move_script(&mut self, entity: &Id, path: &str, to: usize) {
        self.finish_history(true);
        self.history
            .begin("Reordenar scripts", &self.state.project, &self.state.images);
        if let Some(e) = self.scene_mut().entity_mut(entity)
            && let Some(at) = e.scripts.iter().position(|s| s.path == path)
            && to < e.scripts.len()
        {
            let script = e.scripts.remove(at);
            e.scripts.insert(to, script);
        }
        if let Err(error) = self
            .history
            .commit(&self.state.project, &mut self.state.images)
        {
            self.log(error);
        }
        self.history
            .begin("Editar projeto", &self.state.project, &self.state.images);
    }

    /// For the Atributos card: the note under each attribute an attached script of `entity`
    /// declares, "pedido por porta.luau", by attribute.
    pub(super) fn requested_attributes(&self, entity: &Entity) -> BTreeMap<String, String> {
        entity
            .attributes
            .keys()
            .filter_map(|name| {
                let scripts: Vec<&str> = self
                    .script_status
                    .declaring(&entity.scripts, name)
                    .into_iter()
                    .map(file_name)
                    .collect();
                let by = match scripts.as_slice() {
                    [] => return None,
                    [one] => (*one).to_owned(),
                    [first @ .., last] => format!("{} e {last}", first.join(", ")),
                };
                Some((name.clone(), format!("pedido por {by}")))
            })
            .collect()
    }
}

/// The scripts of the card, one block each; what was asked.
pub(crate) fn script_list(ui: &mut egui::Ui, rows: &[ScriptRow]) -> Option<ScriptAction> {
    let mut action = None;
    for (index, row) in rows.iter().enumerate() {
        ui.push_id(("script", &row.path), |ui| {
            if let Some(asked) = script_block(ui, row, index, rows.len()) {
                action = Some(asked);
            }
        });
    }
    action
}

/// One script of the card: { }, its name and the Ligado switch; who uses it and whether it
/// fails; Ver código and the menu.
fn script_block(
    ui: &mut egui::Ui,
    row: &ScriptRow,
    index: usize,
    count: usize,
) -> Option<ScriptAction> {
    let mut action = None;
    let name = file_name(&row.path);
    egui::Frame::new()
        .fill(theme::BG_PANEL)
        .stroke(egui::Stroke::new(1., theme::CARD_BORDER))
        .corner_radius(5)
        .inner_margin(8)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 6.;
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("{ }")
                        .monospace()
                        .size(12.)
                        .color(theme::ACCENT_BRIGHT),
                );
                let switch = 30. + 2. * ui.spacing().item_spacing.x + 44.;
                ui.allocate_ui(
                    Vec2::new((ui.available_width() - switch).max(30.), 20.),
                    |ui| {
                        ui.add(
                            egui::Label::new(RichText::new(name).monospace().size(12.5)).truncate(),
                        )
                        .on_hover_text(&row.path);
                    },
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let mut on = row.enabled;
                    // Off, the object keeps the script but the game runs it without it.
                    if crate::widgets::toggle(ui, &mut on, &format!("{name} ligado")).clicked() {
                        action = Some(ScriptAction::Switch(row.path.clone(), on));
                    }
                    ui.label(RichText::new("Ligado").size(12.).color(theme::TEXT_MUTED));
                });
            });
            match &row.failure {
                Some(failure) => {
                    ui.label(RichText::new(failure).size(12.).color(theme::ERROR));
                }
                None => {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.;
                        ui.label(
                            RichText::new(format!("{} · ", script_status::users(row.users)))
                                .size(12.)
                                .color(theme::TEXT_MUTED),
                        );
                        ui.label(RichText::new("sem erros").size(12.).color(theme::SUCCESS));
                    });
                }
            }
            ui.horizontal(|ui| {
                let more = 26. + ui.spacing().item_spacing.x;
                let view = ui.add_sized(
                    [
                        (ui.available_width() - more).max(40.),
                        theme::CONTROL_HEIGHT,
                    ],
                    egui::Button::new("Ver código"),
                );
                if view.clicked() {
                    let (path, line) = row.view.clone();
                    action = Some(ScriptAction::View(path, line));
                }
                let response = icons::small(ui, Icon::More, &format!("Mais ações de {name}"), 26.);
                egui::Popup::menu(&response).show(|ui| {
                    ui.set_min_width(180.);
                    if ui.button("Desanexar").clicked() {
                        action = Some(ScriptAction::Detach(row.path.clone()));
                        ui.close();
                    }
                    if ui
                        .add_enabled(index > 0, egui::Button::new("Mover para cima"))
                        .clicked()
                    {
                        action = Some(ScriptAction::Move(row.path.clone(), index - 1));
                        ui.close();
                    }
                    if ui
                        .add_enabled(index + 1 < count, egui::Button::new("Mover para baixo"))
                        .clicked()
                    {
                        action = Some(ScriptAction::Move(row.path.clone(), index + 1));
                        ui.close();
                    }
                });
            });
        });
    action
}

/// What "+ Anexar script" offers.
pub(crate) struct AttachMenu<'a> {
    /// The project's scripts, by path.
    pub(crate) files: &'a [String],
    /// What the object attaches already.
    pub(crate) attached: &'a [oxy_core::script::ScriptRef],
    /// The caption of the scripts at the project's root.
    pub(crate) root: &'a str,
    /// Whether the project has a folder, where scripts are.
    pub(crate) saved: bool,
}

/// "+ Anexar script" and its list, with `search`: whether it was clicked (the list opens or
/// closes) and the script chosen.
pub(crate) fn attach_menu(
    ui: &mut egui::Ui,
    menu: &AttachMenu<'_>,
    search: &mut String,
) -> (bool, Option<String>) {
    let mut chosen = None;
    let text = egui::WidgetText::from(
        RichText::new("+ Anexar script")
            .size(12.5)
            .color(theme::ACCENT_BRIGHT),
    );
    let galley = text.into_galley(ui, None, f32::INFINITY, egui::TextStyle::Button);
    let size = galley.size() + Vec2::new(18., 8.);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    icons::register_qa(ui, "+ Anexar script", rect);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), "+ Anexar script")
    });
    let color = if response.hovered() {
        Color32::WHITE
    } else {
        theme::ACCENT_BRIGHT
    };
    let top = egui::pos2(rect.left(), rect.center().y - galley.size().y / 2.);
    ui.painter().galley(top, galley.clone(), color);
    let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&response));
    icons::paint(
        ui,
        Rect::from_center_size(
            egui::pos2(rect.left() + galley.size().x + 9., rect.center().y),
            Vec2::splat(11.),
        ),
        if open {
            Icon::ChevronUp
        } else {
            Icon::ChevronDown
        },
        color,
    );
    let clicked = response.clicked();
    egui::Popup::from_toggle_button_response(&response)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.set_width(240.);
            if !menu.saved {
                crate::widgets::hint(
                    ui,
                    "Salve o projeto numa pasta: os scripts ficam nela, e um agente de IA os \
                     escreve pelo MCP.",
                );
                return;
            }
            if menu.files.is_empty() {
                crate::widgets::hint(
                    ui,
                    "O projeto ainda não tem scripts. Um agente de IA pode escrevê-los pelo MCP.",
                );
                return;
            }
            ui.add(
                egui::TextEdit::singleline(search)
                    .hint_text("Buscar script…")
                    .desired_width(f32::INFINITY),
            );
            ui.add_space(4.);
            let groups = script_status::by_folder(menu.files, search, menu.root);
            egui::ScrollArea::vertical()
                .id_salt("attach_script")
                .max_height((ui.ctx().content_rect().height() * 0.5).max(160.))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 1.;
                    if groups.is_empty() {
                        crate::widgets::hint(ui, "Nenhum script com esse nome.");
                    }
                    for (folder, paths) in groups {
                        ui.add_space(4.);
                        ui.label(RichText::new(folder).size(11.).color(theme::TEXT_MUTED));
                        for path in paths {
                            let stem = file_name(path)
                                .strip_suffix(".luau")
                                .unwrap_or(file_name(path));
                            let attached = menu.attached.iter().any(|s| s.path == path);
                            let item = ui
                                .add_enabled(
                                    !attached,
                                    egui::Button::selectable(
                                        false,
                                        RichText::new(stem).monospace().size(12.),
                                    ),
                                )
                                .on_hover_text(path)
                                .on_disabled_hover_text("Já anexado a este objeto.");
                            if item.clicked() {
                                chosen = Some(path.to_owned());
                                ui.close();
                            }
                        }
                    }
                });
        });
    if chosen.is_some() {
        search.clear();
    }
    (clicked, chosen)
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Pos2, Rect, Shape, vec2};

    /// Every place `label` is drawn, top first.
    fn drawn(output: &egui::FullOutput, label: &str) -> Vec<Pos2> {
        fn find(shape: &Shape, label: &str, found: &mut Vec<Pos2>) {
            match shape {
                Shape::Text(text) if text.galley.text() == label => {
                    found.push(text.pos + text.galley.size() * 0.5)
                }
                Shape::Vec(shapes) => shapes.iter().for_each(|s| find(s, label, found)),
                _ => {}
            }
        }
        let mut found = Vec::new();
        for shape in &output.shapes {
            find(&shape.shape, label, &mut found);
        }
        found.sort_by(|a, b| a.y.total_cmp(&b.y));
        found
    }

    fn rows() -> Vec<ScriptRow> {
        vec![
            ScriptRow {
                path: "Pista/Checkpoints/checkpoint.luau".into(),
                enabled: true,
                users: 6,
                failure: None,
                view: ("Pista/Checkpoints/checkpoint.luau".into(), 1),
            },
            ScriptRow {
                path: "Pista/contagem.luau".into(),
                enabled: false,
                users: 1,
                failure: Some("1 erro na linha 25".into()),
                view: ("Pista/contagem.luau".into(), 25),
            },
        ]
    }

    /// The card's list, 260 points wide, one frame with `events`: what it drew and asked.
    fn frame(
        ctx: &egui::Context,
        rows: &[ScriptRow],
        events: Vec<egui::Event>,
    ) -> (egui::FullOutput, Option<ScriptAction>) {
        let mut action = None;
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800., 600.))),
            events,
            ..Default::default()
        };
        let output = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.allocate_ui(vec2(260., 560.), |ui| action = script_list(ui, rows));
            });
        });
        (output, action)
    }

    fn click(ctx: &egui::Context, rows: &[ScriptRow], at: Pos2) -> Option<ScriptAction> {
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        let (_, pressed) = frame(ctx, rows, vec![egui::Event::PointerMoved(at), button(true)]);
        let (_, released) = frame(ctx, rows, vec![button(false)]);
        pressed.or(released)
    }

    #[test]
    fn each_script_shows_its_name_who_uses_it_and_where_it_fails() {
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        let rows = rows();
        frame(&ctx, &rows, vec![]);
        let (output, _) = frame(&ctx, &rows, vec![]);
        for label in [
            "checkpoint.luau",
            "contagem.luau",
            "Usado por 6 objetos · ",
            "sem erros",
            "1 erro na linha 25",
            "Ligado",
            "Ver código",
        ] {
            assert!(!drawn(&output, label).is_empty(), "{label}");
        }
        // A failing script says only how it fails.
        assert!(drawn(&output, "Usado por 1 objeto · ").is_empty());
        // Ver código opens each at its line.
        let views = drawn(&output, "Ver código");
        assert_eq!(
            click(&ctx, &rows, views[1]),
            Some(ScriptAction::View("Pista/contagem.luau".into(), 25))
        );
        // The switch asks for the other state.
        let (output, _) = frame(&ctx, &rows, vec![]);
        let switches = drawn(&output, "Ligado");
        // At the right of the block, past its margin.
        let off = egui::pos2(236., switches[0].y);
        assert_eq!(
            click(&ctx, &rows, off),
            Some(ScriptAction::Switch(
                "Pista/Checkpoints/checkpoint.luau".into(),
                false
            ))
        );
        let on = egui::pos2(236., switches[1].y);
        assert_eq!(
            click(&ctx, &rows, on),
            Some(ScriptAction::Switch("Pista/contagem.luau".into(), true))
        );
    }

    #[test]
    fn the_menu_detaches_and_moves_within_the_list() {
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        let rows = rows();
        frame(&ctx, &rows, vec![]);
        // The second script's ⋯, at the right of its Ver código.
        let (output, _) = frame(&ctx, &rows, vec![]);
        let view = drawn(&output, "Ver código")[1];
        let more = egui::pos2(260. - 10., view.y);
        assert_eq!(click(&ctx, &rows, more), None);
        let (output, _) = frame(&ctx, &rows, vec![]);
        assert!(!drawn(&output, "Mover para baixo").is_empty());
        let up = drawn(&output, "Mover para cima")[0];
        assert_eq!(
            click(&ctx, &rows, up),
            Some(ScriptAction::Move("Pista/contagem.luau".into(), 0))
        );
        assert_eq!(click(&ctx, &rows, more), None);
        let (output, _) = frame(&ctx, &rows, vec![]);
        let detach = drawn(&output, "Desanexar")[0];
        assert_eq!(
            click(&ctx, &rows, detach),
            Some(ScriptAction::Detach("Pista/contagem.luau".into()))
        );
    }

    #[test]
    fn the_list_to_attach_offers_the_project_scripts_by_folder() {
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        let files: Vec<String> = ["Pista/contagem.luau", "Pista/Checkpoints/checkpoint.luau"]
            .map(String::from)
            .to_vec();
        let attached = [oxy_core::script::ScriptRef::new("Pista/contagem.luau")];
        let mut search = String::new();
        let run = |events: Vec<egui::Event>, search: &mut String| {
            let mut answer = (false, None);
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800., 600.))),
                events,
                ..Default::default()
            };
            let output = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let menu = AttachMenu {
                        files: &files,
                        attached: &attached,
                        root: "Parkour",
                        saved: true,
                    };
                    answer = attach_menu(ui, &menu, search);
                });
            });
            (output, answer)
        };
        let press = |at: Pos2| {
            let button = |pressed| egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            [
                vec![egui::Event::PointerMoved(at), button(true)],
                vec![button(false)],
            ]
        };
        run(vec![], &mut search);
        let (output, _) = run(vec![], &mut search);
        let button = drawn(&output, "+ Anexar script")[0];
        let [down, up] = press(button);
        run(down, &mut search);
        let (_, (clicked, _)) = run(up, &mut search);
        assert!(clicked);
        let (output, _) = run(vec![], &mut search);
        for label in ["Pista", "Pista › Checkpoints", "contagem", "checkpoint"] {
            assert!(!drawn(&output, label).is_empty(), "{label}");
        }
        // What the object attaches already cannot be chosen again.
        let attached = drawn(&output, "contagem")[0];
        let [down, up] = press(attached);
        run(down, &mut search);
        assert_eq!(run(up, &mut search).1, (false, None));
        let (output, _) = run(vec![], &mut search);
        let item = drawn(&output, "checkpoint")[0];
        let [down, up] = press(item);
        let (_, (_, pressed)) = run(down, &mut search);
        let (_, (_, released)) = run(up, &mut search);
        assert_eq!(
            pressed.or(released).as_deref(),
            Some("Pista/Checkpoints/checkpoint.luau")
        );
    }
}
