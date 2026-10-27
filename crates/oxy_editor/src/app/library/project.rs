//! The project's files as a whole: "Organizar o projeto em pastas" on the person's first save of
//! a project of the old layout (what goes where and the full copy made before, or, for a folder
//! with other images, sounds and models, organizing it in a new folder or taking them in),
//! "Descartar alterações" listing what comes back in the folders, and emptying the project's
//! trash, which tells how many undo steps stop working. The work itself is the core's.
use super::*;
use crate::widgets;
use oxy_core::{fs_ops, journal};

/// The fill of the boxes inside the dialogs.
const BOX_FILL: Color32 = Color32::from_rgb(30, 30, 32);
/// The muted text at the right of a row of the conversion.
const EXAMPLE: Color32 = Color32::from_rgb(120, 120, 125);

/// "Organizar o projeto em pastas", planned when it opened.
pub(crate) struct Organize {
    /// What goes where: the kind (none for the scripts), the sentence, and an example or count.
    rows: Vec<(Option<Kind>, String, String)>,
    /// The names fitted or numbered.
    notes: Vec<String>,
    /// The images, sounds and models of the folder the project does not name.
    others: Vec<String>,
    /// The project file is of a format the OXY 0.6 opens.
    old_format: bool,
    /// What waited for the save (the project closing, another one opening).
    then: Option<Transition>,
}

/// "Descartar as alterações?" before `then`, with what comes back in the folders.
pub(crate) struct Discard {
    lines: Vec<String>,
    /// When the project was last saved: "há 12 minutos".
    when: Option<String>,
    then: Transition,
}

/// "Esvaziar a lixeira do projeto?", with what it holds and what stops working.
pub(crate) struct EmptyTrash {
    held: fs_ops::Emptied,
    /// The undo and redo steps that need a file of the trash.
    steps: usize,
    /// The files deleted since the save that "Descartar alterações" would bring back.
    discarded: usize,
}

/// How long ago something happened, `seconds` ago: "há 12 minutos", "ontem".
fn ago(seconds: u64) -> String {
    let past =
        |n: u64, one: &str, many: &str| format!("há {}", model::count(n as usize, one, many));
    match seconds {
        0..60 => "há poucos segundos".into(),
        60..3600 => past(seconds / 60, "minuto", "minutos"),
        3600..86_400 => past(seconds / 3600, "hora", "horas"),
        86_400..172_800 => "ontem".into(),
        _ => past(seconds / 86_400, "dia", "dias"),
    }
}

/// An old file name as the conversion shows it: "12aec132….wav" for a name made of an ID.
fn short_name(path: &str) -> String {
    let name = model::file_name(path);
    let stem = model::stem(name);
    let extension = &name[stem.len()..];
    if stem.chars().count() > 12 {
        format!("{}…{extension}", stem.chars().take(8).collect::<String>())
    } else {
        name.to_owned()
    }
}

/// What the conversion dialog says of `plan`, each kind in its row: the images, sounds and models
/// it has, with an example of where one goes, then the scripts, then the kinds it has none of.
fn organize_rows(plan: &conversion::Plan) -> Vec<(Option<Kind>, String, String)> {
    let mut rows = Vec::new();
    let mut none = Vec::new();
    for (kind, folder) in [
        (AssetKind::Texture, "Imagens"),
        (AssetKind::Audio, "Sons"),
        (AssetKind::Model, "Modelos"),
    ] {
        let shown = Kind::of(kind);
        let n = plan.count(kind);
        let example = if kind == AssetKind::Model {
            plan.model_paths().into_iter().next().unwrap_or_default()
        } else {
            plan.moved
                .iter()
                .find(|moved| moved.to.starts_with(&format!("{folder}/")))
                .map(|moved| format!("{} → {}", short_name(&moved.from), moved.to))
                .unwrap_or_default()
        };
        let row = match (kind, n) {
            (AssetKind::Texture, 0) => (
                format!("Imagens iriam para {folder}/"),
                "nenhuma neste projeto".to_owned(),
            ),
            (AssetKind::Audio, 0) => (
                format!("Sons iriam para {folder}/"),
                "nenhum neste projeto".to_owned(),
            ),
            (AssetKind::Model, 0) => (
                format!("Modelos virariam arquivos em {folder}/"),
                "nenhum neste projeto".to_owned(),
            ),
            (AssetKind::Texture, 1) => (
                format!("1 imagem vai para {folder}/ com o nome dela"),
                example,
            ),
            (AssetKind::Texture, n) => (
                format!("{n} imagens vão para {folder}/ com o nome de cada uma"),
                example,
            ),
            (AssetKind::Audio, 1) => (format!("1 som vai para {folder}/ com o nome dele"), example),
            (AssetKind::Audio, n) => (
                format!("{n} sons vão para {folder}/ com o nome de cada um"),
                example,
            ),
            (AssetKind::Model, 1) => (format!("1 modelo vira arquivo em {folder}/"), example),
            (AssetKind::Model, n) => (format!("{n} modelos viram arquivos em {folder}/"), example),
        };
        let list = if n > 0 { &mut rows } else { &mut none };
        list.push((Some(shown), row.0, row.1));
    }
    let scripts = &plan.scripts;
    let sentence = if scripts.is_empty() {
        "Os scripts ficariam onde estão"
    } else if scripts
        .iter()
        .all(|path| path.to_ascii_lowercase().starts_with("scripts/"))
    {
        "A pasta scripts/ fica como está"
    } else {
        "Os scripts ficam onde estão"
    };
    let count = if scripts.is_empty() {
        "nenhum neste projeto".to_owned()
    } else {
        Kind::Script.count(scripts.len())
    };
    rows.push((None, sentence.to_owned(), count));
    rows.extend(none);
    rows
}

/// "Organizou o projeto em pastas ao gravá-lo: …" as the Console shows what an agent's
/// conversion told, under the Agente mark.
pub(crate) fn agent_note_line(line: &str) -> String {
    let line = line
        .strip_prefix("O agente o")
        .map_or_else(|| line.to_owned(), |rest| format!("O{rest}"));
    format!("{}{line}", super::super::agent::AGENT_PREFIX)
}

/// A sentence whose folder names are in the code font: "9 sons vão para `Sons/` …".
fn with_folders(text: &str, color: Color32) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let plain = egui::TextFormat::simple(egui::FontId::proportional(12.5), color);
    let code = egui::TextFormat::simple(egui::FontId::monospace(12.), color);
    for (index, word) in text.split(' ').enumerate() {
        if index > 0 {
            job.append(" ", 0., plain.clone());
        }
        let format = if word.ends_with('/') {
            code.clone()
        } else {
            plain.clone()
        };
        job.append(word, 0., format);
    }
    job
}

impl Editor {
    /// Before the person's first save of a project of the old layout: opens "Organizar o
    /// projeto em pastas", and answers whether it is open (the save waits for it). A project that
    /// cannot be organized where it is saves in the old layout, telling why, as before.
    pub(crate) fn organize_first(&mut self) -> bool {
        if self.library.organize.is_some() {
            return true;
        }
        let Some(path) = self.path.clone() else {
            return false;
        };
        if self.home.example_copy
            || self.state.project.layout != Layout::Legacy
            || path.file_name() != Some(persistence::PROJECT_FILE.as_ref())
            || !path.exists()
        {
            return false;
        }
        let root = path.parent().unwrap_or(Path::new(".")).to_owned();
        let painted: std::collections::HashSet<Id> = self
            .state
            .images
            .dirty_images()
            .map(|(id, _)| id.clone())
            .collect();
        let Ok(plan) = conversion::plan_all(&root, &self.state.project, &painted) else {
            return false;
        };
        let old_format = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .and_then(|json| json["schema_version"].as_u64())
            .is_some_and(|format| format <= 5);
        self.library.organize = Some(Organize {
            rows: organize_rows(&plan),
            notes: plan.notes.clone(),
            others: plan.others.clone(),
            old_format,
            then: None,
        });
        true
    }

    /// What the save that "Organizar o projeto em pastas" waits for lets go on once it saves.
    pub(in crate::app) fn organize_then(&mut self, then: Option<Transition>) {
        if let Some(organize) = &mut self.library.organize {
            organize.then = then;
        }
    }

    /// "Descartar alterações" before `then`: asks first, listing what comes back in the
    /// folders, when OXY touched the project's files since the save; else discards at once.
    pub(in crate::app) fn discard_before(&mut self, then: Transition) {
        if self.known_files.journal().is_clean() {
            self.discard_and_go(then);
            return;
        }
        let folders: BTreeSet<String> = self
            .tree_scan
            .as_ref()
            .map(|tree| tree.folders.iter().cloned().collect())
            .unwrap_or_default();
        let lines = model::discard_lines(self.known_files.journal(), &|path| {
            folders
                .iter()
                .any(|folder| oxy_core::names::same_name(folder, path))
        });
        let when = self
            .path
            .as_deref()
            .and_then(|path| std::fs::metadata(path).ok()?.modified().ok())
            .map(|saved| {
                ago(std::time::SystemTime::now()
                    .duration_since(saved)
                    .unwrap_or_default()
                    .as_secs())
            });
        self.library.discard = Some(Discard { lines, when, then });
    }

    /// The Projeto menu's "Esvaziar lixeira do projeto (N arquivos, X MB)".
    pub(crate) fn trash_menu_item(&mut self, ui: &mut egui::Ui) {
        let held = self
            .project_root()
            .filter(|_| self.state.project.layout == Layout::Tree)
            .and_then(|root| fs_ops::trash_contents(&root).ok())
            .unwrap_or_default();
        let label = if held.files > 0 {
            format!(
                "Esvaziar lixeira do projeto ({}, {})",
                model::count(held.files, "arquivo", "arquivos"),
                model::size_line(held.bytes)
            )
        } else {
            "Esvaziar lixeira do projeto".to_owned()
        };
        let why = if self.path.is_none() {
            Some(UNSAVED)
        } else if self.state.project.layout == Layout::Legacy {
            Some(OLD_LAYOUT)
        } else if held.files == 0 {
            Some("A lixeira do projeto está vazia.")
        } else {
            None
        };
        let item = ui.add_enabled(why.is_none(), egui::Button::new(label));
        let item = match why {
            Some(why) => item.on_disabled_hover_text(why),
            None => item.on_hover_text(
                "Apaga de vez o que foi excluído ou trocado no projeto (.oxy/lixeira).",
            ),
        };
        if item.clicked() {
            self.library.empty_trash = Some(EmptyTrash {
                held,
                steps: self.history.trash_steps(),
                discarded: self.known_files.journal().trash.len(),
            });
            ui.close();
        }
    }

    /// The dialogs of the project's files, when one is open.
    pub(super) fn file_dialogs(&mut self, ctx: &egui::Context) {
        self.organize_dialog(ctx);
        self.discard_dialog(ctx);
        self.empty_trash_dialog(ctx);
    }

    fn organize_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.library.organize.take() else {
            return;
        };
        enum Choice {
            Here,
            Including,
            Elsewhere,
        }
        let mut choice = None;
        let mut close = false;
        let mixed = !dialog.others.is_empty();
        let response = egui::Modal::new("organize_project".into()).show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 48.).clamp(200., 640.));
            close = widgets::dialog_header(ui, "Organizar o projeto em pastas");
            ui.add(
                egui::Label::new(
                    RichText::new(
                        "Este projeto foi feito numa versão antiga da OXY, em que os arquivos \
                         tinham nomes de código. Ao salvar, a OXY dá a cada arquivo o nome que \
                         aparece na Biblioteca e o põe numa pasta. Os objetos, os scripts e os nós \
                         continuam funcionando: nada muda dentro do jogo.",
                    )
                    .size(12.5)
                    .color(Color32::from_rgb(180, 180, 182)),
                )
                .wrap(),
            );
            ui.add_space(10.);
            egui::Frame::new()
                .fill(BOX_FILL)
                .stroke(egui::Stroke::new(1., theme::CARD_BORDER))
                .corner_radius(6)
                .inner_margin(egui::Margin::symmetric(14, 10))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing.y = 6.;
                    for (kind, text, example) in &dialog.rows {
                        ui.horizontal(|ui| {
                            let (icon, _) =
                                ui.allocate_exact_size(Vec2::new(18., 18.), Sense::hover());
                            match kind {
                                Some(kind) => {
                                    paint_icon(ui, icon, *kind, Color32::from_rgb(180, 180, 182))
                                }
                                None => paint_braces(ui, icon, theme::ACCENT_BRIGHT),
                            }
                            ui.label(with_folders(text, theme::TEXT));
                            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                                let mut example = RichText::new(example).color(EXAMPLE);
                                example = if example.text().contains('/') {
                                    example.monospace().size(11.5)
                                } else {
                                    example.size(12.)
                                };
                                ui.add(egui::Label::new(example).truncate());
                            });
                        });
                    }
                    for note in &dialog.notes {
                        ui.add(
                            egui::Label::new(RichText::new(note).size(12.).color(theme::WARNING))
                                .wrap(),
                        );
                    }
                });
            if mixed {
                ui.add_space(12.);
                let shown: Vec<String> = dialog.others.iter().take(6).cloned().collect();
                let more = dialog.others.len() - shown.len();
                let list = if more > 0 {
                    format!("{} e mais {more}", shown.join(", "))
                } else {
                    model::listed(&shown)
                };
                widgets::callout(
                    ui,
                    true,
                    &format!(
                        "A pasta do projeto tem {} que ele não usa: {list}. Organizado aqui, o \
                         projeto passa a ter esses arquivos na Biblioteca. Para deixar esta \
                         pasta como está, a OXY organiza o projeto numa pasta nova e passa a \
                         editar a cópia.",
                        match dialog.others.len() {
                            1 => "1 imagem, som ou modelo".to_owned(),
                            n => format!("{n} imagens, sons ou modelos"),
                        }
                    ),
                );
            }
            ui.add_space(12.);
            let opens = if dialog.old_format {
                ", que ainda abre na OXY 0.6"
            } else {
                ""
            };
            note_row(
                ui,
                "✓",
                theme::SUCCESS,
                &format!(
                    "Antes de mexer em qualquer arquivo, a OXY guarda uma cópia completa do \
                     projeto em .oxy/backup/antes-das-pastas-…/{opens}. Se algo for interrompido \
                     no meio, a conversão continua de onde parou na próxima vez."
                ),
            );
            ui.add_space(6.);
            note_row(
                ui,
                "i",
                theme::ACCENT_BRIGHT,
                "Depois você reorganiza como quiser: arrastar, renomear e criar pastas na \
                 Biblioteca muda as pastas do disco.",
            );
            widgets::dialog_footer(ui, |ui| {
                if mixed {
                    if widgets::primary_button(ui, "Organizar numa pasta nova…", true).clicked() {
                        choice = Some(Choice::Elsewhere);
                    }
                    let here = format!(
                        "Organizar aqui e incluir {}",
                        match dialog.others.len() {
                            1 => "o arquivo".to_owned(),
                            n => format!("os {n} arquivos"),
                        }
                    );
                    if widgets::secondary_button(ui, here).clicked() {
                        choice = Some(Choice::Including);
                    }
                } else if widgets::primary_button(ui, "Organizar e salvar", true).clicked() {
                    choice = Some(Choice::Here);
                }
                close |= widgets::secondary_button(ui, "Cancelar").clicked();
            });
        });
        let Some(choice) = choice else {
            if !(close || response.should_close()) {
                self.library.organize = Some(dialog);
            }
            return;
        };
        match choice {
            Choice::Here => {}
            Choice::Including => self.library.save_including = std::mem::take(&mut dialog.others),
            Choice::Elsewhere => self.library.save_elsewhere = true,
        }
        if self.save_from("")
            && let Some(then) = dialog.then
        {
            self.apply_transition(then);
        }
    }

    fn discard_dialog(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.library.discard.take() else {
            return;
        };
        let mut confirm = false;
        let mut close = false;
        let response = egui::Modal::new("discard_changes".into()).show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 48.).clamp(160., 460.));
            close = widgets::dialog_header(ui, "Descartar as alterações?");
            let when = dialog
                .when
                .as_ref()
                .map_or_else(String::new, |when| format!(" ({when})"));
            ui.add(
                egui::Label::new(
                    RichText::new(format!(
                        "Tudo volta ao último Salvar{when}, inclusive o que você fez nas pastas:"
                    ))
                    .size(12.5)
                    .color(Color32::from_rgb(180, 180, 182)),
                )
                .wrap(),
            );
            ui.add_space(6.);
            egui::Frame::new()
                .fill(BOX_FILL)
                .stroke(egui::Stroke::new(1., theme::CARD_BORDER))
                .corner_radius(6)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing.y = 4.;
                    for line in &dialog.lines {
                        ui.label(RichText::new(line).size(12.5).color(theme::TEXT));
                    }
                });
            ui.add_space(6.);
            widgets::hint(
                ui,
                "Se algum arquivo estiver aberto em outro programa, a OXY avisa qual e não mexe \
                 nele.",
            );
            widgets::dialog_footer(ui, |ui| {
                confirm = widgets::danger_button(ui, "Descartar tudo", true).clicked();
                close |= widgets::secondary_button(ui, "Cancelar").clicked();
            });
        });
        if confirm {
            self.discard_and_go(dialog.then);
        } else if !(close || response.should_close()) {
            self.library.discard = Some(dialog);
        }
    }

    fn empty_trash_dialog(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.library.empty_trash.take() else {
            return;
        };
        let mut confirm = false;
        let mut close = false;
        let response = egui::Modal::new("empty_trash".into()).show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 48.).clamp(160., 460.));
            close = widgets::dialog_header(ui, "Esvaziar a lixeira do projeto?");
            widgets::hint(
                ui,
                &format!(
                    "{} ({}) da lixeira do projeto (.oxy/lixeira) saem do disco para sempre.",
                    match dialog.held.files {
                        1 => "O arquivo".to_owned(),
                        n => format!("Os {n} arquivos"),
                    },
                    model::size_line(dialog.held.bytes)
                ),
            );
            if dialog.steps > 0 {
                ui.add_space(6.);
                widgets::callout(
                    ui,
                    true,
                    &format!(
                        "{} de desfazer {} de valer: o Ctrl+Z para antes do passo mais novo que \
                         precisa de um arquivo da lixeira.",
                        model::count(dialog.steps, "passo", "passos"),
                        if dialog.steps == 1 { "deixa" } else { "deixam" }
                    ),
                );
            }
            if dialog.discarded > 0 {
                ui.add_space(6.);
                widgets::hint(
                    ui,
                    &format!(
                        "Descartar alterações também deixa de trazer de volta {} desde o último \
                         Salvar.",
                        model::count(dialog.discarded, "arquivo excluído", "arquivos excluídos")
                    ),
                );
            }
            widgets::dialog_footer(ui, |ui| {
                confirm = widgets::danger_button(ui, "Esvaziar lixeira", true).clicked();
                close |= widgets::secondary_button(ui, "Cancelar").clicked();
            });
        });
        if confirm {
            self.empty_project_trash();
        } else if !(close || response.should_close()) {
            self.library.empty_trash = Some(dialog);
        }
    }

    /// Empties the project's trash for good; the undo steps that need a file of it go, unless
    /// nothing left it.
    fn empty_project_trash(&mut self) {
        let Some(root) = self.project_root() else {
            return;
        };
        let before = fs_ops::trash_contents(&root).ok().map(|held| held.files);
        let emptied = journal::empty_trash(&root, &mut self.known_files);
        let went = emptied.is_ok()
            || match (before, fs_ops::trash_contents(&root)) {
                (Some(before), Ok(after)) => after.files < before,
                _ => true,
            };
        let gone = if went { self.history.forget_trash() } else { 0 };
        match emptied {
            Ok(emptied) => self.log(format!(
                "Lixeira do projeto esvaziada: {} ({}).",
                model::count(emptied.files, "arquivo", "arquivos"),
                model::size_line(emptied.bytes)
            )),
            Err(error) => self.warn(error),
        }
        if gone > 0 {
            self.log(format!(
                "{} de desfazer {}: precisava{} dos arquivos da lixeira.",
                model::count(gone, "passo", "passos"),
                if gone == 1 { "saiu" } else { "saíram" },
                if gone == 1 { "" } else { "m" }
            ));
        }
    }
}

/// A line of the conversion dialog after a mark: "✓ Antes de mexer…".
fn note_row(ui: &mut egui::Ui, mark: &str, color: Color32, text: &str) {
    ui.horizontal_top(|ui| {
        ui.add_sized(
            Vec2::new(14., 16.),
            egui::Label::new(RichText::new(mark).strong().color(color)),
        );
        ui.add(
            egui::Label::new(
                RichText::new(text)
                    .size(12.5)
                    .color(Color32::from_rgb(180, 180, 182)),
            )
            .wrap(),
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_last_save_reads_as_how_long_ago() {
        assert_eq!(ago(5), "há poucos segundos");
        assert_eq!(ago(60), "há 1 minuto");
        assert_eq!(ago(12 * 60 + 5), "há 12 minutos");
        assert_eq!(ago(2 * 3600), "há 2 horas");
        assert_eq!(ago(30 * 3600), "ontem");
        assert_eq!(ago(3 * 86_400), "há 3 dias");
    }

    #[test]
    fn an_old_name_made_of_an_id_is_shortened() {
        assert_eq!(
            short_name("assets/12aec132-1b2c-4d5e-8f90-123456789abc.wav"),
            "12aec132….wav"
        );
        assert_eq!(short_name("assets/Bipe.wav"), "Bipe.wav");
    }

    #[test]
    fn the_conversion_says_what_goes_where_the_kinds_it_has_first() {
        use oxy_core::document::{Asset, Primitive, Project};
        let mut project = Project::new("Antigo");
        for (id, name) in [
            ("12aec132-1b2c-4d5e-8f90-123456789abc", "Bipe"),
            ("18574072-dc72-480d-8930-3fd3eb480700", "Clique"),
        ] {
            project.assets.push(Asset {
                id: id.into(),
                name: name.into(),
                path: format!("assets/{id}.wav"),
                kind: AssetKind::Audio,
                model: None,
            });
        }
        project.assets.push(Asset {
            id: "1c0389ef-8535-4c71-b16d-d94d1bbc2160".into(),
            name: "Checkpoint".into(),
            path: String::new(),
            kind: AssetKind::Model,
            model: Some(vec![Entity::new("Base", Some(Primitive::Cube))]),
        });
        let mut plan = conversion::in_folders(&project);
        plan.scripts = vec!["scripts/porta.luau".into(), "scripts/chave.luau".into()];
        let row = |kind: Option<Kind>, text: &str, example: &str| {
            (kind, text.to_owned(), example.to_owned())
        };
        assert_eq!(
            organize_rows(&plan),
            [
                row(
                    Some(Kind::Sound),
                    "2 sons vão para Sons/ com o nome de cada um",
                    "12aec132….wav → Sons/Bipe.wav"
                ),
                row(
                    Some(Kind::Model),
                    "1 modelo vira arquivo em Modelos/",
                    "Modelos/Checkpoint.modelo.json"
                ),
                row(None, "A pasta scripts/ fica como está", "2 scripts"),
                row(
                    Some(Kind::Image),
                    "Imagens iriam para Imagens/",
                    "nenhuma neste projeto"
                ),
            ]
        );
        // One of a kind, and scripts in other folders.
        project.assets.truncate(1);
        let mut plan = conversion::in_folders(&project);
        plan.scripts = vec!["Pista/porta.luau".into()];
        let rows = organize_rows(&plan);
        assert_eq!(
            rows[0],
            row(
                Some(Kind::Sound),
                "1 som vai para Sons/ com o nome dele",
                "12aec132….wav → Sons/Bipe.wav"
            )
        );
        assert_eq!(
            rows[1],
            row(None, "Os scripts ficam onde estão", "1 script")
        );
        assert_eq!(
            rows[3],
            row(
                Some(Kind::Model),
                "Modelos virariam arquivos em Modelos/",
                "nenhum neste projeto"
            )
        );
    }

    #[test]
    fn the_agent_conversion_reads_under_the_agent_mark() {
        let line =
            agent_note_line("O agente organizou o projeto em pastas ao gravá-lo: 9 sons em Sons/.");
        assert_eq!(
            line,
            format!(
                "{}Organizou o projeto em pastas ao gravá-lo: 9 sons em Sons/.",
                super::super::super::agent::AGENT_PREFIX
            )
        );
        use super::super::super::console_panel::{Mark, message_row};
        let row = message_row(&line, false);
        assert!(matches!(row.mark, Mark::Agente));
        assert_eq!(
            row.text,
            "Organizou o projeto em pastas ao gravá-lo: 9 sons em Sons/."
        );
    }
}
