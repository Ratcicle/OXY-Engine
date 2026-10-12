//! Variáveis do jogo: the project's game variables as a table. Every change goes through the
//! project commands, as agents' do, and is one undo step named after the command.
use super::*;
use crate::{theme, widgets};
use oxy_core::{
    command::{self, Command, ParamValue},
    graph,
    input_actions::Use,
    variables::{self, GameVariable},
};
use std::collections::HashSet;

const TYPE_WIDTH: f32 = 96.;
const VALUE_WIDTH: f32 = 92.;
const PERSIST_WIDTH: f32 = 86.;
const USED_WIDTH: f32 = 200.;
const DELETE_WIDTH: f32 = 24.;
const GAP: f32 = 8.;
const ROW_HEIGHT: f32 = 28.;
const WARNING_TEXT: Color32 = Color32::from_rgb(232, 210, 160);

#[derive(Default)]
pub(super) struct VariablesUi {
    pub(super) open: bool,
    /// The variable about to be created: name, type (an index in [`kinds`]), initial value
    /// as typed and Salva entre partidas.
    name: String,
    kind: usize,
    value: String,
    persistent: bool,
    /// The variable whose deletion is asked under its row.
    remove: Option<Id>,
    /// A name or initial value being typed in a row.
    draft: Option<Draft>,
    /// Why the last change was refused.
    error: Option<String>,
}
impl VariablesUi {
    /// Opens the dialog with an empty new variable.
    pub(super) fn show(&mut self) {
        *self = Self {
            open: true,
            value: value_text(&kinds()[0]),
            ..Self::default()
        };
    }
}
/// What a row is typing: it becomes a command once its field lets go.
struct Draft {
    variable: Id,
    field: Field,
    text: String,
    widget: egui::Id,
}
#[derive(Clone, Copy, PartialEq)]
enum Field {
    Name,
    Value,
}

/// The types a variable can have, as their default values, in the order the dialog offers them.
fn kinds() -> [Value; 5] {
    [
        Value::Number(0.),
        Value::Text(String::new()),
        Value::Bool(false),
        Value::Vector2([0.; 2]),
        Value::Vector3([0.; 3]),
    ]
}
fn kind_index(value: &Value) -> usize {
    kinds()
        .iter()
        .position(|kind| std::mem::discriminant(kind) == std::mem::discriminant(value))
        .unwrap_or(0)
}
fn kind_label(value: &Value) -> &'static str {
    graph::value_type(value).label()
}
fn bool_label(on: bool) -> &'static str {
    if on { "Verdadeiro" } else { "Falso" }
}

/// An initial value as the dialog writes it: numbers with a point, vectors with ";" between
/// their numbers.
pub(crate) fn value_text(value: &Value) -> String {
    let vector = |numbers: &[f32]| {
        numbers
            .iter()
            .map(f32::to_string)
            .collect::<Vec<_>>()
            .join("; ")
    };
    match value {
        Value::Number(number) => number.to_string(),
        Value::Text(text) => text.clone(),
        Value::Bool(on) => bool_label(*on).into(),
        Value::Vector2(v) => vector(v),
        Value::Vector3(v) => vector(v),
        Value::Object(_) | Value::Surface(_) => String::new(),
    }
}

/// `text` read as a value of the type of `like`: a decimal comma is a point, and vectors take
/// ";" between their numbers. `None` when it does not fit.
fn parse_value(text: &str, like: &Value) -> Option<Value> {
    let number = |text: &str| {
        text.trim()
            .replace(',', ".")
            .parse::<f64>()
            .ok()
            .filter(|n| n.is_finite())
    };
    let vector = || -> Option<Vec<f32>> {
        text.split(';')
            .map(|part| number(part).map(|n| n as f32).filter(|n| n.is_finite()))
            .collect()
    };
    match like {
        Value::Number(_) => number(text).map(Value::Number),
        Value::Text(_) => Some(Value::Text(text.into())),
        Value::Bool(_) => match text.trim().to_lowercase().as_str() {
            "verdadeiro" | "sim" | "true" => Some(Value::Bool(true)),
            "falso" | "não" | "nao" | "false" => Some(Value::Bool(false)),
            _ => None,
        },
        Value::Vector2(_) => match vector()?.as_slice() {
            &[x, y] => Some(Value::Vector2([x, y])),
            _ => None,
        },
        Value::Vector3(_) => match vector()?.as_slice() {
            &[x, y, z] => Some(Value::Vector3([x, y, z])),
            _ => None,
        },
        Value::Object(_) | Value::Surface(_) => None,
    }
}

/// What an initial value of the type of `like` looks like, for a refused one.
fn expected(like: &Value) -> &'static str {
    match like {
        Value::Vector2(_) => "dois números separados por ponto e vírgula, como 0; 1.5",
        Value::Vector3(_) => "três números separados por ponto e vírgula, como 0; 1.5; 0",
        _ => "um número, como 0.5",
    }
}

/// Where a variable is used, short: the scenes, then the objects, with the node count when
/// an object has more than one; "não usada" without uses.
fn uses_summary(uses: &[Use]) -> String {
    if uses.is_empty() {
        return "não usada".into();
    }
    let mut scopes: Vec<&str> = Vec::new();
    let mut names: Vec<&str> = Vec::new();
    let mut objects = HashSet::new();
    for found in uses {
        if !scopes.contains(&found.scope.as_str()) {
            scopes.push(&found.scope);
        }
        if !names.contains(&found.name.as_str()) {
            names.push(&found.name);
        }
        objects.insert((found.scope.as_str(), found.entity.as_str()));
    }
    let mut summary = format!("{} → {}", scopes.join(", "), names.join(", "));
    if uses.len() > objects.len() {
        summary.push_str(&format!(" ({} nós)", uses.len()));
    }
    summary
}

/// The nodes that use a variable, one line per object: "Cena → Objeto → Nó, Nó".
fn uses_detail(uses: &[Use]) -> Vec<String> {
    let mut objects: Vec<(&Use, Vec<&str>)> = Vec::new();
    for found in uses {
        let label = graph::definition(&found.role).map_or(found.role.as_str(), |d| d.label);
        match objects
            .iter_mut()
            .find(|(first, _)| first.scope == found.scope && first.entity == found.entity)
        {
            Some((_, labels)) => labels.push(label),
            None => objects.push((found, vec![label])),
        }
    }
    objects
        .into_iter()
        .map(|(found, labels)| format!("{} → {} → {}", found.scope, found.name, labels.join(", ")))
        .collect()
}

/// Column widths for the room the table has: Usada por and the Sim/Não beside the switch
/// leave first when it is narrow.
struct Columns {
    name: f32,
    persist: f32,
    used: Option<f32>,
}
impl Columns {
    fn new(width: f32) -> Self {
        let used = (width >= 720.).then_some(USED_WIDTH);
        let persist = if width >= 520. { PERSIST_WIDTH } else { 34. };
        let cells = if used.is_some() { 5. } else { 4. };
        let fixed =
            TYPE_WIDTH + VALUE_WIDTH + persist + used.unwrap_or(0.) + DELETE_WIDTH + GAP * cells;
        Self {
            name: (width - fixed).max(70.),
            persist,
            used,
        }
    }
    fn persist_text(&self) -> bool {
        self.persist >= PERSIST_WIDTH
    }
}
/// A table cell of `width`, its content centred vertically.
fn cell<R>(ui: &mut egui::Ui, width: f32, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.allocate_ui_with_layout(
        Vec2::new(width, ROW_HEIGHT),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_width(width);
            add(ui)
        },
    )
    .inner
}
/// The titles of the table's columns, in capitals, each wrapped within its column and centred
/// on the tallest.
fn column_titles(ui: &mut egui::Ui, titles: &[(&str, f32)]) {
    let galleys: Vec<_> = titles
        .iter()
        .map(|(text, width)| {
            ui.painter().layout(
                text.to_uppercase(),
                theme::medium(10.5),
                theme::TEXT_MUTED,
                *width,
            )
        })
        .collect();
    let height = galleys.iter().map(|g| g.size().y).fold(18., f32::max);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = GAP;
        for (galley, (_, width)) in galleys.into_iter().zip(titles) {
            let (rect, _) = ui.allocate_exact_size(Vec2::new(*width, height), Sense::hover());
            let top = rect.center().y - galley.size().y / 2.;
            ui.painter()
                .galley(egui::pos2(rect.left(), top), galley, theme::TEXT_MUTED);
        }
    });
}
fn muted(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).size(12.).color(theme::TEXT_MUTED));
}
fn kind_choice(ui: &mut egui::Ui, kind: &mut usize, salt: &str) -> bool {
    let before = *kind;
    egui::ComboBox::from_id_salt(salt)
        .width(ui.available_width())
        .selected_text(kind_label(&kinds()[*kind]))
        .show_ui(ui, |ui| {
            for (index, value) in kinds().iter().enumerate() {
                ui.selectable_value(kind, index, kind_label(value));
            }
        });
    *kind != before
}
fn bool_choice(ui: &mut egui::Ui, on: &mut bool, salt: &str) -> bool {
    let before = *on;
    egui::ComboBox::from_id_salt(salt)
        .width(ui.available_width())
        .selected_text(bool_label(*on))
        .show_ui(ui, |ui| {
            ui.selectable_value(on, false, bool_label(false));
            ui.selectable_value(on, true, bool_label(true));
        });
    *on != before
}

impl Editor {
    /// The project's game variables: name, type, initial value, Salva entre partidas, where
    /// each is used and deletion; a new one; the test's saved progress.
    pub(super) fn variables_dialog(&mut self, ctx: &egui::Context) {
        let mut close = false;
        let content = ctx.content_rect();
        let width = (content.width() - 48.).clamp(160., 860.);
        let response = egui::Modal::new("game_variables".into()).show(ctx, |ui| {
            ui.set_width(width);
            close = widgets::dialog_header(ui, "Variáveis do jogo");
            egui::ScrollArea::vertical()
                .max_height((content.height() - 190.).max(90.))
                .show(ui, |ui| {
                    widgets::hint(
                        ui,
                        "Valem para o jogo inteiro e continuam valendo quando a cena muda. As \
                         marcadas como salvas voltam na próxima partida, depois de um nó Salvar \
                         progresso.",
                    );
                    ui.add_space(6.);
                    let columns = Columns::new(ui.available_width());
                    let mut titles = vec![
                        ("Nome", columns.name),
                        ("Tipo", TYPE_WIDTH),
                        ("Valor inicial", VALUE_WIDTH),
                        (
                            if columns.persist_text() {
                                "Salva entre partidas"
                            } else {
                                "Salva"
                            },
                            columns.persist,
                        ),
                    ];
                    if let Some(width) = columns.used {
                        titles.push(("Usada por", width));
                    }
                    column_titles(ui, &titles);
                    for variable in self.state.project.variables.clone() {
                        ui.push_id(&variable.id, |ui| {
                            self.variable_row(ui, &variable, &columns);
                        });
                    }
                    if self.state.project.variables.is_empty() {
                        widgets::hint(
                            ui,
                            "Nenhuma variável do jogo ainda. Crie a primeira abaixo.",
                        );
                    }
                    if let Some(error) = &self.logic_ui.variables.error {
                        ui.add_space(2.);
                        widgets::callout(ui, true, error);
                    }
                    ui.add_space(4.);
                    ui.separator();
                    self.new_variable_row(ui, &columns);
                    widgets::hint(
                        ui,
                        "Tipos: Número, Texto, Booleano, Vetor2 e Vetor3; vetores levam ponto e \
                         vírgula entre os números, como 0; 1.5; 0. Com uma chave nos nós Ler e \
                         Alterar variável do jogo, a mesma variável guarda um valor para cada \
                         chave, por exemplo um por cena.",
                    );
                    ui.add_space(6.);
                    self.test_progress(ui);
                });
            widgets::dialog_footer(ui, |ui| {
                let done = widgets::primary_button(ui, "Fechar", true);
                icons::register_qa(ui, "Fechar variáveis do jogo", done.rect);
                close |= done.clicked();
            });
        });
        let close = close || response.should_close();
        // A name or value being typed applies once its field lets go, or as the dialog closes.
        let typing = self
            .logic_ui
            .variables
            .draft
            .as_ref()
            .is_some_and(|draft| ctx.memory(|m| m.has_focus(draft.widget)));
        if close || !typing {
            self.commit_variable_draft();
        }
        if close {
            self.logic_ui.variables.open = false;
            self.logic_ui.variables.remove = None;
            self.logic_ui.variables.error = None;
        }
    }
    fn variable_row(&mut self, ui: &mut egui::Ui, variable: &GameVariable, columns: &Columns) {
        let uses = variables::uses(&self.state.project, &variable.id);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            cell(ui, columns.name, |ui| {
                self.draft_field(ui, variable, Field::Name);
            });
            cell(ui, TYPE_WIDTH, |ui| {
                let mut kind = kind_index(&variable.value);
                // Another type starts from that type's default.
                if kind_choice(ui, &mut kind, "type") {
                    self.update_variable(&variable.id, Some(kinds()[kind].clone()), None);
                }
            });
            cell(ui, VALUE_WIDTH, |ui| match variable.value {
                Value::Bool(on) => {
                    let mut on = on;
                    if bool_choice(ui, &mut on, "value") {
                        self.update_variable(&variable.id, Some(Value::Bool(on)), None);
                    }
                }
                _ => self.draft_field(ui, variable, Field::Value),
            });
            cell(ui, columns.persist, |ui| {
                let mut on = variable.persistent;
                widgets::toggle(
                    ui,
                    &mut on,
                    &format!("Salva entre partidas: {}", variable.name),
                );
                if on != variable.persistent {
                    self.update_variable(&variable.id, None, Some(on));
                }
                if columns.persist_text() {
                    muted(ui, if on { "Sim" } else { "Não" });
                }
            });
            if let Some(width) = columns.used {
                cell(ui, width, |ui| {
                    let color = if uses.is_empty() {
                        theme::TEXT_MUTED
                    } else {
                        Color32::from_rgb(168, 168, 174)
                    };
                    let label = ui.add(
                        egui::Label::new(
                            egui::RichText::new(uses_summary(&uses))
                                .size(12.)
                                .color(color),
                        )
                        .truncate(),
                    );
                    if !uses.is_empty() {
                        label.on_hover_text(uses_detail(&uses).join("\n"));
                    }
                });
            }
            let delete = icons::small(ui, Icon::Trash, "Excluir variável", DELETE_WIDTH);
            icons::register_qa(
                ui,
                &format!("Excluir variável {}", variable.name),
                delete.rect,
            );
            if delete.clicked() {
                self.logic_ui.variables.remove = Some(variable.id.clone());
            }
        });
        if self.logic_ui.variables.remove.as_deref() != Some(variable.id.as_str()) {
            return;
        }
        // Deleting asks in place, under its own row; a variable in use lists its nodes.
        egui::Frame::new()
            .fill(Color32::from_rgb(46, 39, 24))
            .stroke(egui::Stroke::new(1., Color32::from_rgb(74, 61, 34)))
            .corner_radius(6)
            .inner_margin(egui::Margin::symmetric(12, 10))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                if uses.is_empty() {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(format!(
                                "Excluir a variável {}? Nenhum nó a usa.",
                                variable.name
                            ))
                            .size(12.5)
                            .color(WARNING_TEXT),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let confirm = widgets::secondary_button(
                                ui,
                                egui::RichText::new("Excluir variável")
                                    .color(Color32::from_rgb(240, 160, 160)),
                            );
                            icons::register_qa(ui, "Confirmar exclusão da variável", confirm.rect);
                            if widgets::secondary_button(ui, "Cancelar").clicked() {
                                self.logic_ui.variables.remove = None;
                            }
                            if confirm.clicked()
                                && self.apply_variable_command(Command::DeleteVariable {
                                    variable: variable.id.clone(),
                                })
                            {
                                self.logic_ui.variables.remove = None;
                            }
                        });
                    });
                } else {
                    ui.label(
                        egui::RichText::new("Altere estes nós antes de excluir a variável:")
                            .size(12.5)
                            .color(WARNING_TEXT),
                    );
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(uses_detail(&uses).join("; "))
                                .size(12.)
                                .color(Color32::from_rgb(201, 184, 142)),
                        )
                        .wrap(),
                    );
                    if widgets::secondary_button(ui, "Cancelar exclusão").clicked() {
                        self.logic_ui.variables.remove = None;
                    }
                }
            });
    }
    /// A row's name or initial value: typed into a draft, applied as one command when the
    /// field lets go.
    fn draft_field(&mut self, ui: &mut egui::Ui, variable: &GameVariable, field: Field) {
        let state = &mut self.logic_ui.variables;
        let draft = state
            .draft
            .as_mut()
            .filter(|draft| draft.variable == variable.id && draft.field == field);
        let mut text = match (&draft, field) {
            (Some(draft), _) => draft.text.clone(),
            (None, Field::Name) => variable.name.clone(),
            (None, Field::Value) => value_text(&variable.value),
        };
        let mut edit = egui::TextEdit::singleline(&mut text).desired_width(f32::INFINITY);
        if field == Field::Value {
            edit = edit.font(egui::TextStyle::Monospace);
        }
        let response = ui.add(edit);
        if let Some(draft) = draft {
            draft.text = text;
        } else if response.has_focus() {
            self.commit_variable_draft();
            self.logic_ui.variables.draft = Some(Draft {
                variable: variable.id.clone(),
                field,
                text,
                widget: response.id,
            });
        }
    }
    fn commit_variable_draft(&mut self) {
        let Some(draft) = self.logic_ui.variables.draft.take() else {
            return;
        };
        let Some(variable) = variables::get(&self.state.project, &draft.variable).cloned() else {
            return;
        };
        let command = match draft.field {
            Field::Name if draft.text.trim() == variable.name => return,
            Field::Name => Command::UpdateVariable {
                variable: variable.id,
                name: Some(draft.text),
                value: None,
                persistent: None,
            },
            Field::Value => match parse_value(&draft.text, &variable.value) {
                Some(value) if value == variable.value => return,
                Some(value) => Command::UpdateVariable {
                    variable: variable.id,
                    name: None,
                    value: Some(ParamValue::Tagged(value)),
                    persistent: None,
                },
                None => {
                    self.refuse_variable_edit(format!(
                        "O valor inicial de {} precisa ser {}",
                        variable.name,
                        expected(&variable.value)
                    ));
                    return;
                }
            },
        };
        self.apply_variable_command(command);
    }
    fn update_variable(&mut self, id: &str, value: Option<Value>, persistent: Option<bool>) {
        self.apply_variable_command(Command::UpdateVariable {
            variable: id.into(),
            name: None,
            value: value.map(ParamValue::Tagged),
            persistent,
        });
    }
    /// One project command as its own undo step, named as the command. A refused one changes
    /// nothing and says why in the dialog.
    fn apply_variable_command(&mut self, command: Command) -> bool {
        self.finish_history(true);
        self.history
            .begin(command.label(), &self.state.project, &self.state.images);
        match command::apply(
            &mut self.state.project,
            &self.scene_id,
            std::slice::from_ref(&command),
        ) {
            Ok(_) => {
                self.finish_history(true);
                self.logic_ui.variables.error = None;
                true
            }
            Err(error) => {
                self.history
                    .cancel(&mut self.state.project, &mut self.state.images);
                self.refuse_variable_edit(error.message);
                false
            }
        }
    }
    fn refuse_variable_edit(&mut self, message: String) {
        let message = format!("{}.", message.trim_end().trim_end_matches('.'));
        self.log(message.clone());
        self.logic_ui.variables.error = Some(message);
    }
    fn new_variable_row(&mut self, ui: &mut egui::Ui, columns: &Columns) {
        let mut create = false;
        let state = &mut self.logic_ui.variables;
        let ready = !state.name.trim().is_empty();
        let button = |ui: &mut egui::Ui| {
            ui.add_enabled_ui(ready, |ui| widgets::secondary_button(ui, "Criar variável"))
                .inner
                .clicked()
        };
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            cell(ui, columns.name, |ui| {
                let name = ui.add(
                    egui::TextEdit::singleline(&mut state.name)
                        .hint_text("Nome da nova variável")
                        .desired_width(f32::INFINITY),
                );
                create |=
                    ready && name.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            });
            cell(ui, TYPE_WIDTH, |ui| {
                if kind_choice(ui, &mut state.kind, "new_type") {
                    state.value = value_text(&kinds()[state.kind]);
                }
            });
            cell(ui, VALUE_WIDTH, |ui| {
                if matches!(kinds()[state.kind], Value::Bool(_)) {
                    let mut on =
                        parse_value(&state.value, &Value::Bool(false)) == Some(Value::Bool(true));
                    if bool_choice(ui, &mut on, "new_value") {
                        state.value = bool_label(on).into();
                    }
                } else {
                    ui.add(
                        egui::TextEdit::singleline(&mut state.value)
                            .font(egui::TextStyle::Monospace)
                            .desired_width(f32::INFINITY),
                    );
                }
            });
            cell(ui, columns.persist, |ui| {
                widgets::toggle(
                    ui,
                    &mut state.persistent,
                    "Salva entre partidas: nova variável",
                );
                if columns.persist_text() {
                    muted(ui, if state.persistent { "Sim" } else { "Não" });
                }
            });
            if let Some(width) = columns.used {
                create |= cell(ui, width, button);
            }
        });
        if columns.used.is_none() {
            create |= button(ui);
        }
        if !create {
            return;
        }
        let state = &self.logic_ui.variables;
        let like = kinds()[state.kind].clone();
        let Some(value) = parse_value(&state.value, &like) else {
            let message = format!("O valor inicial precisa ser {}", expected(&like));
            self.refuse_variable_edit(message);
            return;
        };
        let command = Command::CreateVariable {
            name: state.name.clone(),
            value: ParamValue::Tagged(value),
            persistent: state.persistent,
        };
        if self.apply_variable_command(command) {
            let state = &mut self.logic_ui.variables;
            state.name.clear();
            state.value = value_text(&kinds()[state.kind]);
            state.persistent = false;
        }
    }
    /// What Salvar progresso recorded in this editor session's tests, with Apagar progresso do
    /// teste.
    fn test_progress(&mut self, ui: &mut egui::Ui) {
        let saved = self.play.test_save.exists;
        let rows =
            super::super::test_progress::saved_rows(&self.state.project, &self.play.test_save);
        let mut clear = false;
        egui::Frame::new()
            .fill(Color32::from_rgb(37, 37, 40))
            .stroke(egui::Stroke::new(1., theme::CARD_BORDER))
            .corner_radius(8)
            .inner_margin(egui::Margin::same(12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("Progresso do teste")
                            .font(theme::medium(13.))
                            .color(theme::TEXT),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        clear = ui
                            .add_enabled_ui(saved, |ui| {
                                widgets::secondary_button(ui, "Apagar progresso do teste")
                            })
                            .inner
                            .on_disabled_hover_text("Nenhum progresso gravado no teste.")
                            .clicked();
                    });
                });
                widgets::hint(
                    ui,
                    "O que Salvar progresso gravou nos testes desta sessão do editor. O próximo \
                     Jogar começa daqui. O jogo exportado guarda o próprio progresso, em \
                     %APPDATA%\\OXY Engine\\Jogos.",
                );
                let empty = if !saved {
                    "Nenhum progresso gravado no teste. O próximo Jogar começa com os valores \
                     iniciais."
                } else if rows.is_empty() {
                    "O teste gravou progresso, mas nenhuma variável é salva entre partidas."
                } else {
                    ""
                };
                if !empty.is_empty() {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(empty)
                                .size(12.)
                                .color(Color32::from_rgb(168, 168, 174)),
                        )
                        .wrap(),
                    );
                    return;
                }
                let label_width = (ui.available_width() * 0.4).clamp(120., 220.);
                egui::Grid::new("test_progress")
                    .num_columns(2)
                    .min_col_width(label_width)
                    .spacing([12., 4.])
                    .show(ui, |ui| {
                        for (name, key, value) in &rows {
                            let mut label = egui::text::LayoutJob::default();
                            let font = egui::FontId::proportional(12.5);
                            label.append(
                                name,
                                0.,
                                egui::TextFormat::simple(font.clone(), theme::TEXT),
                            );
                            if !key.is_empty() {
                                label.append(
                                    &format!(" · {key}"),
                                    0.,
                                    egui::TextFormat::simple(font, theme::TEXT_MUTED),
                                );
                            }
                            ui.add(egui::Label::new(label).truncate());
                            ui.add(
                                egui::Label::new(egui::RichText::new(value).monospace().size(12.))
                                    .truncate(),
                            );
                            ui.end_row();
                        }
                    });
            });
        if clear {
            self.play.test_save = Default::default();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_values_read_back_as_written_with_commas_or_points() {
        for value in [
            Value::Number(0.8),
            Value::Number(-12.),
            Value::Text("Fase 1 · início".into()),
            Value::Bool(true),
            Value::Vector2([0., 1.5]),
            Value::Vector3([0., 1.5, -2.25]),
        ] {
            assert_eq!(
                parse_value(&value_text(&value), &value),
                Some(value.clone())
            );
        }
        assert_eq!(value_text(&Value::Vector3([0., 1.5, 0.])), "0; 1.5; 0");
        assert_eq!(
            parse_value(" 1,5 ", &Value::Number(0.)),
            Some(Value::Number(1.5))
        );
        assert_eq!(
            parse_value("0; 1,5; 0", &Value::Vector3([0.; 3])),
            Some(Value::Vector3([0., 1.5, 0.]))
        );
        assert_eq!(
            parse_value("Sim", &Value::Bool(false)),
            Some(Value::Bool(true))
        );
        for (text, like) in [
            ("", Value::Number(0.)),
            ("um", Value::Number(0.)),
            ("inf", Value::Number(0.)),
            ("1; 2", Value::Vector3([0.; 3])),
            ("1; 2; 3", Value::Vector2([0.; 2])),
            ("talvez", Value::Bool(false)),
        ] {
            assert_eq!(parse_value(text, &like), None, "{text}");
        }
    }

    #[test]
    fn uses_read_by_scene_and_object_with_node_counts_only_when_they_add() {
        let found = |scope: &str, entity: &str, name: &str, role: &str| Use {
            scope: scope.into(),
            entity: entity.into(),
            name: name.into(),
            role: role.into(),
            model: None,
        };
        assert_eq!(uses_summary(&[]), "não usada");
        assert!(uses_detail(&[]).is_empty());
        let one = [found("Menu", "a", "Botão Continuar", "variable.get")];
        assert_eq!(uses_summary(&one), "Menu → Botão Continuar");
        let two_objects = [
            found("Fase 1", "b", "Cronômetro", "variable.set"),
            found("Fase 1", "c", "Linha de chegada", "variable.get"),
        ];
        assert_eq!(
            uses_summary(&two_objects),
            "Fase 1 → Cronômetro, Linha de chegada"
        );
        let shared = [
            found("Fase 1", "c", "Linha de chegada", "variable.get"),
            found("Fase 1", "c", "Linha de chegada", "variable.set"),
            found("Fase 2", "d", "Linha de chegada", "variable.get"),
            found("Fase 2", "d", "Linha de chegada", "variable.set"),
        ];
        assert_eq!(
            uses_summary(&shared),
            "Fase 1, Fase 2 → Linha de chegada (4 nós)"
        );
        assert_eq!(
            uses_detail(&shared),
            [
                "Fase 1 → Linha de chegada → Ler variável do jogo, Alterar variável do jogo",
                "Fase 2 → Linha de chegada → Ler variável do jogo, Alterar variável do jogo",
            ]
        );
    }

    #[test]
    fn narrow_tables_drop_usada_por_then_the_switch_text() {
        let wide = Columns::new(820.);
        assert_eq!(wide.used, Some(USED_WIDTH));
        assert!(wide.persist_text());
        assert_eq!(
            wide.name
                + TYPE_WIDTH
                + VALUE_WIDTH
                + PERSIST_WIDTH
                + USED_WIDTH
                + DELETE_WIDTH
                + 5. * GAP,
            820.
        );
        let medium = Columns::new(600.);
        assert_eq!(medium.used, None);
        assert!(medium.persist_text());
        let narrow = Columns::new(400.);
        assert!(!narrow.persist_text());
        assert!(narrow.name >= 70.);
    }
}
