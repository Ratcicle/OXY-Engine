//! The Library's dialogs: "Mover para…" (the project's folders as a tree, a new one made on the
//! way), "Excluir pasta…" (its items go to the root, a name taken there told before) and
//! deleting items: an image, sound or model with who uses it (objects keep it from going, a
//! script that names it is listed with the line), a script with the objects and models it is
//! attached to ("Tirar dos N e excluir"), several items at once. What goes ends in the project's
//! trash; each runs the tree's commands as one undo step, and a refusal (a file open in another
//! program) stays in the dialog.
use super::*;
use crate::widgets;

/// The fill of the boxes inside the dialogs.
const BOX_FILL: Color32 = Color32::from_rgb(30, 30, 32);
/// The "(onde estão)" after the folder the items are in.
const WHERE: Color32 = Color32::from_rgb(120, 120, 125);
/// A folder of the tree that is not chosen.
const FOLDER: Color32 = Color32::from_rgb(180, 180, 182);

/// "Mover para…" for `items`.
pub(super) struct MoveDialog {
    items: Vec<Key>,
    /// The folder chosen ("" is the root).
    target: Option<String>,
    /// The folders shown open in the tree.
    expanded: BTreeSet<String>,
    /// The name of a new folder being typed in the target.
    new_folder: Option<String>,
    error: Option<String>,
}

/// "Excluir pasta…" for `folder`, with what the deletion would tell, planned when it opened.
pub(super) struct DeleteFolder {
    folder: String,
    told: Result<Vec<String>, String>,
    error: Option<String>,
}

/// Deleting the items `keys`.
pub(super) struct DeleteItems {
    keys: Vec<Key>,
    error: Option<String>,
}

/// Who uses an item about to be deleted.
#[derive(Default)]
struct Uses {
    /// The objects whose material, interface, placement or logic uses an image, sound or model,
    /// as `editing::asset_references` says them: they keep it from going.
    objects: Vec<String>,
    /// The scripts that name a sound or a model with a literal text, with the line.
    scripts: Vec<(String, u32)>,
    /// The objects of the scenes a script is attached to, one name for each.
    attached: Vec<String>,
    /// The models one of whose pieces a script is attached to.
    models: Vec<String>,
    /// The attributes a script attached declares.
    declared: Vec<String>,
    /// The scripts that require a script.
    required_by: Vec<String>,
}

impl Uses {
    /// How many objects and models a script leaves before it goes.
    fn holders(&self) -> usize {
        self.attached.len() + self.models.len()
    }
}

/// "Ele está anexado a 6 objetos (Checkpoint 1 a Checkpoint 6) e ao modelo Checkpoint. Para
/// excluir, a OXY tira o script de todos eles antes.": the objects (one name for each) and the
/// models a script is attached to.
fn attached_line(objects: &[String], models: &[String]) -> String {
    let all = if objects.len() + models.len() > 1 {
        "Para excluir, a OXY tira o script de todos eles antes."
    } else {
        "Para excluir, a OXY o tira dele antes."
    };
    let mut names: Vec<String> = Vec::new();
    for name in objects {
        if !names.contains(name) {
            names.push(name.clone());
        }
    }
    let objects = match objects.len() {
        0 => None,
        1 => Some(format!("ao objeto {}", objects[0])),
        n => Some(format!("a {n} objetos ({})", model::names_line(&names))),
    };
    let models = match models {
        [] => None,
        [one] => Some(format!("ao modelo {one}")),
        many => Some(format!("aos modelos {}", model::listed(many))),
    };
    let to = match (objects, models) {
        (Some(objects), Some(models)) => format!("{objects} e {models}"),
        (Some(one), None) | (None, Some(one)) => one,
        (None, None) => return String::new(),
    };
    format!("Ele está anexado {to}. {all}")
}

/// The folder all of `items` are in, when they share one.
fn common_folder(items: &[Key], lib: &Library) -> Option<String> {
    let mut folders = items.iter().map(|key| match key {
        Key::Folder(folder) => Some(model::parent(folder).to_owned()),
        key => lib.item(key).map(|item| item.folder.clone()),
    });
    let first = folders.next()??;
    folders
        .all(|folder| folder.as_deref() == Some(first.as_str()))
        .then_some(first)
}

/// How a label names `key`: the item's name, or the folder's.
fn key_name(key: &Key, lib: &Library) -> String {
    match key {
        Key::Folder(folder) => model::file_name(folder).to_owned(),
        key => lib
            .item(key)
            .map_or_else(String::new, |item| item.name.clone()),
    }
}

/// `folder` and every folder above it, the root included.
fn ancestors(folder: &str) -> Vec<String> {
    let mut found = vec![String::new()];
    let mut at = folder;
    while !at.is_empty() {
        found.push(at.to_owned());
        at = model::parent(at);
    }
    found
}

impl Editor {
    /// Opens "Mover para…" for `items`, the folder they are in chosen.
    pub(super) fn open_move(&mut self, items: Vec<Key>, lib: &Library) {
        if let Some(why) = self.organize_refusal() {
            self.warn(why);
            return;
        }
        if items.is_empty() {
            return;
        }
        let target = common_folder(&items, lib);
        self.library.move_to = Some(MoveDialog {
            expanded: ancestors(target.as_deref().unwrap_or_default())
                .into_iter()
                .collect(),
            items,
            target,
            new_folder: None,
            error: None,
        });
    }

    /// Moves `items` into the folder `to` as one undo step: "Mover Bipe para Sons".
    pub(super) fn move_keys(
        &mut self,
        items: &[Key],
        to: &str,
        lib: &Library,
    ) -> Result<(), String> {
        let label = match items {
            [key] => format!(
                "Mover {} para {}",
                key_name(key, lib),
                self.folder_label(to)
            ),
            _ => format!("Mover {} itens para {}", items.len(), self.folder_label(to)),
        };
        self.library_commands(&label, vec![drag::move_command(items, to)])
            .map(drop)
    }

    /// Opens "Excluir pasta…" for `folder`, telling first where its items go: a name the root
    /// already has is told as the Library names the item ("A raiz já tem um som “Clique”: este
    /// vai como “Clique 2”.").
    pub(super) fn open_delete_folder(&mut self, folder: &str) {
        let Some(root) = self.project_root() else {
            return;
        };
        let lib = self.library_model();
        let commands = [Command::DeleteFolder {
            folder: folder.to_owned(),
        }];
        let told = command::TreeView::read(&root)
            .map_err(|error| error.to_string())
            .and_then(|mut view| {
                let (_, applied) = command::plan_with_files(
                    &self.state.project,
                    &self.scene_id,
                    &commands,
                    &mut view,
                )
                .map_err(|error| plain(&error.to_string()))?;
                let mut told: Vec<String> = applied
                    .into_iter()
                    .flat_map(|a| a.warnings)
                    .map(|w| plain(&w))
                    .collect();
                for file in view.files() {
                    let FileOp::Move { from, to } = file else {
                        continue;
                    };
                    let (old, new) = (model::file_name(from), model::file_name(to));
                    if old == new {
                        continue;
                    }
                    let line = if let Some(item) = lib.items.iter().find(|i| i.path == *from) {
                        format!(
                            "A raiz já tem {} “{}”: {} vai como “{}”.",
                            item.kind.one(),
                            item.name,
                            item.kind.this(),
                            model::stem(new)
                        )
                    } else if lib.folder(from).is_some() {
                        format!("A raiz já tem uma pasta “{old}”: esta vai como “{new}”.")
                    } else {
                        continue;
                    };
                    told.retain(|w| !w.starts_with(&format!("{from} foi para a raiz")));
                    told.push(line);
                }
                Ok(told)
            });
        self.library.delete_folder = Some(DeleteFolder {
            folder: folder.to_owned(),
            told,
            error: None,
        });
    }

    /// Opens the dialog that deletes the items `keys`.
    pub(super) fn open_delete_items(&mut self, keys: Vec<Key>) {
        self.library.delete_items = Some(DeleteItems { keys, error: None });
    }

    /// The Library's open dialog, if any.
    pub(crate) fn library_dialogs(&mut self, ctx: &egui::Context) {
        if !self.library.dialog_open() {
            return;
        }
        let lib = self.library_model();
        self.move_dialog(ctx, &lib);
        self.delete_folder_dialog(ctx, &lib);
        self.delete_items_dialog(ctx, &lib);
        self.file_dialogs(ctx);
    }

    fn move_dialog(&mut self, ctx: &egui::Context, lib: &Library) {
        let Some(mut dialog) = self.library.move_to.take() else {
            return;
        };
        let names: Vec<String> = dialog
            .items
            .iter()
            .filter_map(|key| match key {
                Key::Folder(folder) => Some(format!("{} (pasta)", model::file_name(folder))),
                key => lib.item(key).map(labelled),
            })
            .collect();
        let title = match dialog.items.as_slice() {
            [Key::Folder(folder)] => {
                format!("Mover a pasta “{}” para…", model::file_name(folder))
            }
            [key] => format!("Mover “{}” para…", key_name(key, lib)),
            _ => format!("Mover {} itens para…", dialog.items.len()),
        };
        let current = common_folder(&dialog.items, lib);
        let moving: Vec<&str> = dialog
            .items
            .iter()
            .filter_map(|key| match key {
                Key::Folder(folder) => Some(folder.as_str()),
                _ => None,
            })
            .collect();
        let mut close = false;
        let mut confirm = false;
        let mut create = None;
        let response = egui::Modal::new("library_move".into()).show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 48.).clamp(160., 420.));
            close = widgets::dialog_header(ui, &title);
            widgets::hint(ui, &model::listed(&names));
            ui.add_space(8.);
            egui::Frame::new()
                .fill(BOX_FILL)
                .stroke(egui::Stroke::new(1., theme::CARD_BORDER))
                .corner_radius(6)
                .inner_margin(6)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    egui::ScrollArea::vertical()
                        .id_salt("library_move_folders")
                        .max_height(240.)
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = 1.;
                            for folder in &lib.folders {
                                let shown = ancestors(model::parent(&folder.path))
                                    .iter()
                                    .all(|above| dialog.expanded.contains(above));
                                if !folder.path.is_empty() && !shown {
                                    continue;
                                }
                                let inside = moving.iter().any(|m| {
                                    folder.path == *m || folder.path.starts_with(&format!("{m}/"))
                                });
                                let chosen = dialog.target.as_deref() == Some(folder.path.as_str());
                                let here = current.as_deref() == Some(folder.path.as_str());
                                let open = folder.path.is_empty()
                                    || dialog.expanded.contains(&folder.path);
                                let (row, toggled) =
                                    move_row(ui, folder, open, chosen, here, !inside);
                                if toggled {
                                    if open {
                                        dialog.expanded.remove(&folder.path);
                                    } else {
                                        dialog.expanded.insert(folder.path.clone());
                                    }
                                } else if row.clicked() {
                                    dialog.target = Some(folder.path.clone());
                                    dialog.error = None;
                                }
                            }
                        });
                });
            if let Some(name) = &mut dialog.new_folder {
                ui.add_space(6.);
                ui.horizontal(|ui| {
                    let field = ui.add(
                        egui::TextEdit::singleline(name)
                            .hint_text("Nome da pasta")
                            .desired_width(ui.available_width() - 70.),
                    );
                    field.request_focus();
                    let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if ui.button("Criar").clicked() || enter {
                        create = Some(name.clone());
                    }
                });
            }
            if let Some(error) = &dialog.error {
                ui.add_space(6.);
                widgets::callout(ui, true, error);
            }
            widgets::dialog_footer(ui, |ui| {
                let ready = dialog
                    .target
                    .as_ref()
                    .is_some_and(|target| current.as_ref() != Some(target));
                confirm = widgets::primary_button(ui, "Mover", ready).clicked();
                close |= widgets::secondary_button(ui, "Cancelar").clicked();
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    let place = dialog
                        .target
                        .as_deref()
                        .map_or(self.state.project.name.clone(), |t| self.folder_label(t));
                    let new = ui.add(
                        egui::Button::new(
                            RichText::new(format!("+ Nova pasta em {place}"))
                                .color(theme::ACCENT_BRIGHT),
                        )
                        .frame(false),
                    );
                    if new.clicked() {
                        dialog.new_folder = Some("Nova pasta".into());
                    }
                });
            });
        });
        if let Some(name) = create {
            let parent = dialog.target.clone().unwrap_or_default();
            let path = model::join(&parent, name.trim());
            let label = format!("Criar pasta {}", name.trim());
            match self.library_commands(&label, vec![Command::CreateFolder { path: path.clone() }])
            {
                Ok(_) => {
                    dialog.expanded.extend(ancestors(&parent));
                    dialog.target = Some(path);
                    dialog.new_folder = None;
                    dialog.error = None;
                }
                Err(error) => dialog.error = Some(error),
            }
            self.library.move_to = Some(dialog);
            return;
        }
        if confirm && let Some(target) = dialog.target.clone() {
            match self.move_keys(&dialog.items, &target, lib) {
                Ok(()) => self.open_place(Place::Folder(target)),
                Err(error) => {
                    dialog.error = Some(error);
                    self.library.move_to = Some(dialog);
                }
            }
            return;
        }
        if !(close || response.should_close()) {
            self.library.move_to = Some(dialog);
        }
    }

    fn delete_folder_dialog(&mut self, ctx: &egui::Context, lib: &Library) {
        let Some(mut dialog) = self.library.delete_folder.take() else {
            return;
        };
        let name = model::file_name(&dialog.folder).to_owned();
        let subfolders = lib.subfolders(&dialog.folder);
        let items = lib.shown(Some(&dialog.folder), "", None);
        let others = lib.others.get(&dialog.folder).map_or(0, Vec::len);
        let mut close = false;
        let mut confirm = false;
        let response = egui::Modal::new("library_delete_folder".into()).show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 48.).clamp(160., 440.));
            close = widgets::dialog_header(ui, &format!("Excluir a pasta “{name}”?"));
            let what = match subfolders.len() + items.len() + others {
                0 => "A pasta está vazia: só ela some.".to_owned(),
                1 => {
                    "Nenhum recurso é apagado: o item da pasta vai para a raiz do projeto, e só a \
                      pasta some."
                        .to_owned()
                }
                n => format!(
                    "Nenhum recurso é apagado: os {n} itens da pasta vão para a raiz do projeto, \
                     e só a pasta some."
                ),
            };
            widgets::hint(ui, &what);
            let told = dialog.told.as_ref().ok().filter(|t| !t.is_empty());
            if subfolders.len() + items.len() + others > 0 || told.is_some() {
                ui.add_space(6.);
                egui::Frame::new()
                    .fill(BOX_FILL)
                    .stroke(egui::Stroke::new(1., theme::CARD_BORDER))
                    .corner_radius(6)
                    .inner_margin(egui::Margin::symmetric(12, 8))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.;
                            let mut shown = 0;
                            let entries = subfolders
                                .iter()
                                .map(|f| (None, f.name.as_str()))
                                .chain(items.iter().map(|i| (Some(i.kind), i.name.as_str())));
                            for (kind, entry) in entries {
                                if shown == 8 {
                                    break;
                                }
                                if shown > 0 {
                                    ui.label(
                                        RichText::new("·").size(12.5).color(theme::TEXT_MUTED),
                                    );
                                }
                                inline_entry(ui, kind, entry);
                                shown += 1;
                            }
                            let more = subfolders.len() + items.len() - shown;
                            let mut rest = Vec::new();
                            if more > 0 {
                                rest.push(format!("e mais {more}"));
                            }
                            if others > 0 {
                                rest.push(model::count(others, "outro arquivo", "outros arquivos"));
                            }
                            if !rest.is_empty() {
                                if shown > 0 {
                                    ui.label(
                                        RichText::new("·").size(12.5).color(theme::TEXT_MUTED),
                                    );
                                }
                                ui.label(
                                    RichText::new(rest.join(" e "))
                                        .size(12.5)
                                        .color(theme::TEXT_MUTED),
                                );
                            }
                        });
                        for line in told.into_iter().flatten() {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(line).size(12.5).color(theme::WARNING),
                                )
                                .wrap(),
                            );
                        }
                    });
            }
            if let Err(error) = &dialog.told {
                ui.add_space(6.);
                widgets::callout(ui, true, error);
            }
            if let Some(error) = &dialog.error {
                ui.add_space(6.);
                widgets::callout(
                    ui,
                    true,
                    &format!("Não foi possível excluir a pasta: {error}."),
                );
            }
            widgets::dialog_footer(ui, |ui| {
                confirm =
                    widgets::primary_button(ui, "Excluir pasta", dialog.told.is_ok()).clicked();
                close |= widgets::secondary_button(ui, "Cancelar").clicked();
            });
        });
        if confirm {
            let label = format!("Excluir pasta {name}");
            let commands = vec![Command::DeleteFolder {
                folder: dialog.folder.clone(),
            }];
            match self.library_commands(&label, commands) {
                Ok(_) => {
                    if matches!(&self.library.place, Place::Folder(open) if open == &dialog.folder || open.starts_with(&format!("{}/", dialog.folder)))
                    {
                        self.library.place = Place::default();
                    }
                }
                Err(error) => {
                    dialog.error = Some(error);
                    self.library.delete_folder = Some(dialog);
                }
            }
            return;
        }
        if !(close || response.should_close()) {
            self.library.delete_folder = Some(dialog);
        }
    }

    /// Who uses `item`, from the project and what the scripts' check found (`facts`).
    fn uses_of(&self, item: &Item, facts: &BTreeMap<String, ScriptFacts>) -> Uses {
        let project = &self.state.project;
        match &item.key {
            Key::Asset(id) => Uses {
                objects: oxy_core::editing::asset_references(project, id),
                scripts: model::script_uses(facts, item.kind, &item.name),
                ..Uses::default()
            },
            Key::Script(path) => {
                let attaches = |e: &Entity| {
                    e.scripts
                        .iter()
                        .any(|s| oxy_core::names::same_name(&s.path, path))
                };
                Uses {
                    attached: project
                        .scenes
                        .iter()
                        .flat_map(|s| &s.entities)
                        .filter(|e| attaches(e))
                        .map(|e| e.name.clone())
                        .collect(),
                    models: project
                        .assets
                        .iter()
                        .filter(|a| a.model.as_ref().is_some_and(|p| p.iter().any(attaches)))
                        .map(|a| a.name.clone())
                        .collect(),
                    declared: self.script_status.declared(path).to_vec(),
                    required_by: self
                        .script_status
                        .required_by(path)
                        .into_iter()
                        .map(|p| model::file_name(p).to_owned())
                        .collect(),
                    ..Uses::default()
                }
            }
            Key::Folder(_) => Uses::default(),
        }
    }

    fn delete_items_dialog(&mut self, ctx: &egui::Context, lib: &Library) {
        let Some(mut dialog) = self.library.delete_items.take() else {
            return;
        };
        let items: Vec<&Item> = dialog.keys.iter().filter_map(|key| lib.item(key)).collect();
        if items.is_empty() {
            return;
        }
        let facts = self.script_status.library_facts();
        let uses: Vec<Uses> = items
            .iter()
            .map(|item| self.uses_of(item, &facts))
            .collect();
        let tree = self.state.project.layout == Layout::Tree;
        // An image, sound or model objects use stays; a script attached leaves them first.
        let going: Vec<usize> = (0..items.len())
            .filter(|&i| uses[i].objects.is_empty())
            .collect();
        let kept = items.len() - going.len();
        let holders: usize = going.iter().map(|&i| uses[i].holders()).sum();
        let trash_note = if tree {
            "O arquivo vai para a lixeira do projeto (.oxy/lixeira) e volta com Ctrl+Z."
        } else {
            "O recurso será retirado da biblioteca. O arquivo original no disco é preservado \
             para permitir desfazer com segurança."
        };
        let title = match items.as_slice() {
            [one] if kept == 1 => format!(
                "Não é possível excluir {} “{}”",
                one.kind.article(),
                one.name
            ),
            [one] => format!("Excluir {} “{}”?", one.kind.article(), one.name),
            _ => format!("Excluir {} itens?", items.len()),
        };
        let mut close = false;
        let mut confirm = false;
        let response = egui::Modal::new("library_delete_items".into()).show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 48.).clamp(160., 460.));
            close = widgets::dialog_header(ui, &title);
            match (items.as_slice(), uses.as_slice()) {
                ([item], [uses]) if !uses.objects.is_empty() => blocked(ui, item, uses),
                ([item], [uses]) if item.kind == Kind::Script => script_body(ui, uses, trash_note),
                ([item], [uses]) if !uses.scripts.is_empty() => {
                    let (he, it) = item.kind.pronouns();
                    let this = item.kind.this();
                    widgets::hint(
                        ui,
                        &format!(
                            "{}{} {} está em uso. Se {he} sair, quem {it} usa {}:",
                            this[..1].to_uppercase(),
                            &this[1..],
                            item.kind.label().to_lowercase(),
                            item.kind.without()
                        ),
                    );
                    ui.add_space(6.);
                    script_lines(ui, item, &uses.scripts);
                    ui.add_space(6.);
                    widgets::hint(ui, trash_note);
                }
                ([item], _) => {
                    widgets::hint(
                        ui,
                        if !tree {
                            trash_note
                        } else if matches!(item.state, State::Missing | State::Unsaved) {
                            // No file in the folder to send to the trash.
                            "O recurso sai da biblioteca; o arquivo dele não está na pasta do \
                             projeto. Ctrl+Z o traz de volta."
                        } else {
                            "O recurso sai da biblioteca, e o arquivo vai para a lixeira do \
                             projeto (.oxy/lixeira). Ctrl+Z traz os dois de volta."
                        },
                    );
                }
                _ => several_body(ui, &items, &uses, kept, holders, tree),
            }
            if let Some(error) = &dialog.error {
                ui.add_space(6.);
                widgets::callout(ui, true, &format!("Não foi possível excluir: {error}."));
            }
            widgets::dialog_footer(ui, |ui| {
                let label = if holders > 0 {
                    if holders == 1 {
                        "Tirar e excluir".to_owned()
                    } else {
                        format!("Tirar dos {holders} e excluir")
                    }
                } else if tree {
                    "Mandar para a lixeira".to_owned()
                } else {
                    // The old layout keeps the files on disk.
                    "Excluir recurso".to_owned()
                };
                confirm = !going.is_empty() && widgets::danger_button(ui, &label, true).clicked();
                close |= widgets::secondary_button(ui, "Cancelar").clicked();
            });
        });
        if confirm {
            let commands: Vec<Command> = going
                .iter()
                .map(|&i| Command::DeleteAsset {
                    asset: match &items[i].key {
                        Key::Asset(id) => id.clone(),
                        Key::Script(path) | Key::Folder(path) => path.clone(),
                    },
                    kind: Some(items[i].kind.command()),
                    detach: (uses[i].holders() > 0).then_some(true),
                })
                .collect();
            let removed: Vec<Id> = going
                .iter()
                .filter_map(|&i| match &items[i].key {
                    Key::Asset(id) => Some(id.clone()),
                    _ => None,
                })
                .collect();
            let label = match going.as_slice() {
                [one] => format!("Excluir {}", items[*one].name),
                _ => format!("Excluir {} itens", going.len()),
            };
            match self.library_commands(&label, commands) {
                Ok(_) => {
                    for id in removed {
                        self.state.images.remove(&id);
                        self.renderer.clear_texture_override(&id);
                        self.play.game_ui.clear_texture_override(&id);
                    }
                    self.asset_ui.thumbnail = None;
                    self.library.selection.clear();
                }
                Err(error) => {
                    dialog.error = Some(error);
                    self.library.delete_items = Some(dialog);
                }
            }
            return;
        }
        if !(close || response.should_close()) {
            self.library.delete_items = Some(dialog);
        }
    }
}

/// A folder of the tree in "Mover para…": its chevron (when it has subfolders), its name and,
/// for the folder the items are in, "(onde estão)"; the row's response and whether the chevron
/// was clicked.
fn move_row(
    ui: &mut egui::Ui,
    folder: &model::Folder,
    open: bool,
    chosen: bool,
    here: bool,
    enabled: bool,
) -> (egui::Response, bool) {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), FOLDER_ROW), sense);
    let fill = if chosen {
        theme::ACCENT_SOFT
    } else if enabled && response.hovered() {
        theme::BG_WINDOW
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 4., fill);
    let left = rect.left() + 6. + folder.depth as f32 * 14.;
    let mut toggled = false;
    if folder.children || folder.path.is_empty() {
        let at = Rect::from_center_size(egui::pos2(left + 7., rect.center().y), Vec2::splat(14.));
        let arrow = ui.interact(at, response.id.with("chevron"), Sense::click());
        icons::paint(
            ui,
            at.shrink(2.),
            if open {
                Icon::ChevronDown
            } else {
                Icon::ChevronRight
            },
            theme::TEXT_MUTED,
        );
        toggled = arrow.clicked() && !folder.path.is_empty();
    }
    let color = if !enabled {
        theme::TEXT_MUTED
    } else if chosen || folder.path.is_empty() {
        theme::TEXT
    } else {
        FOLDER
    };
    let mut job = egui::text::LayoutJob::default();
    let font = egui::FontId::proportional(13.);
    job.append(
        &folder.name,
        0.,
        egui::TextFormat::simple(font.clone(), color),
    );
    if here {
        job.append(" (onde estão)", 0., egui::TextFormat::simple(font, WHERE));
    }
    let galley = ui.painter().layout_job(job);
    ui.painter().galley(
        egui::pos2(left + 18., rect.center().y - galley.size().y / 2.),
        galley,
        Color32::PLACEHOLDER,
    );
    let response = if enabled {
        response
    } else {
        response.on_hover_text("Uma pasta não vai para dentro dela mesma.")
    };
    (response, toggled)
}

/// An item of a list inside a dialog: its icon and its name ("{ } menu_principal"), a folder's
/// with the folder icon.
fn inline_entry(ui: &mut egui::Ui, kind: Option<Kind>, name: &str) {
    let (icon, _) = ui.allocate_exact_size(Vec2::new(16., 18.), Sense::hover());
    match kind {
        Some(kind) => paint_icon(ui, icon, kind, FOLDER),
        None => icons::paint(
            ui,
            Rect::from_center_size(icon.center(), Vec2::splat(13.)),
            Icon::Folder,
            FOLDER,
        ),
    }
    let mut text = RichText::new(name).size(12.5).color(theme::TEXT);
    if kind == Some(Kind::Script) {
        text = text.monospace();
    }
    ui.label(text);
}

/// The scripts that name `item`, each with its line: "Pista/checkpoint.luau, linha 20 (toca
/// “Checkpoint”)".
fn script_lines(ui: &mut egui::Ui, item: &Item, scripts: &[(String, u32)]) {
    let verb = if item.kind == Kind::Sound {
        "toca"
    } else {
        "usa"
    };
    egui::Frame::new()
        .fill(BOX_FILL)
        .stroke(egui::Stroke::new(1., theme::CARD_BORDER))
        .corner_radius(6)
        .inner_margin(egui::Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 4.;
            for (path, line) in scripts {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.;
                    ui.label(RichText::new(path).monospace().size(12.).color(theme::TEXT));
                    let at = if *line > 0 {
                        format!(", linha {line}")
                    } else {
                        String::new()
                    };
                    ui.label(
                        RichText::new(format!("{at} ({verb} “{}”)", item.name))
                            .size(12.5)
                            .color(theme::TEXT),
                    );
                });
            }
        });
}

/// An image, sound or model objects use: who, and that those uses go first.
fn blocked(ui: &mut egui::Ui, item: &Item, uses: &Uses) {
    widgets::hint(
        ui,
        "Este recurso ainda está em uso. Remova os vínculos antes de excluí-lo.",
    );
    ui.add_space(4.);
    egui::ScrollArea::vertical()
        .id_salt("library_delete_uses")
        .max_height(240.)
        .show(ui, |ui| {
            for reference in &uses.objects {
                // "Scope → Object: use", as listed by editing::asset_references.
                let (place, usage) = reference
                    .split_once(": ")
                    .unwrap_or((reference.as_str(), ""));
                let (scope, object) = place.split_once(" → ").unwrap_or(("", place));
                let detail = [scope, usage]
                    .into_iter()
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(" · ");
                widgets::reference_row(ui, Icon::Object, object, &detail);
            }
        });
    if !uses.scripts.is_empty() {
        ui.add_space(6.);
        script_lines(ui, item, &uses.scripts);
    }
}

/// A script about to go: the objects and models it leaves first, and what stays.
fn script_body(ui: &mut egui::Ui, uses: &Uses, trash_note: &str) {
    if uses.holders() > 0 {
        widgets::hint(ui, &attached_line(&uses.attached, &uses.models));
        ui.add_space(4.);
        let attributes = if uses.declared.is_empty() {
            String::new()
        } else {
            format!(
                "Os atributos que ele pediu ({}) continuam nos objetos. ",
                model::listed(&uses.declared)
            )
        };
        widgets::hint(ui, &format!("{attributes}Tudo volta com Ctrl+Z."));
        return;
    }
    let users = if uses.required_by.is_empty() {
        "Nenhum objeto usa este script.".to_owned()
    } else {
        format!(
            "Nenhum objeto anexa este script, mas {} o {} com require: sem ele, deixa{} de \
             carregar.",
            model::listed(&uses.required_by),
            if uses.required_by.len() == 1 {
                "usa"
            } else {
                "usam"
            },
            if uses.required_by.len() == 1 { "" } else { "m" }
        )
    };
    widgets::hint(ui, &users);
    ui.add_space(4.);
    widgets::hint(ui, trash_note);
}

/// Several items about to go: each with what keeps it or who uses it.
fn several_body(
    ui: &mut egui::Ui,
    items: &[&Item],
    uses: &[Uses],
    kept: usize,
    holders: usize,
    tree: bool,
) {
    egui::ScrollArea::vertical()
        .id_salt("library_delete_items")
        .max_height(220.)
        .show(ui, |ui| {
            for (item, uses) in items.iter().zip(uses) {
                let detail = if !uses.objects.is_empty() {
                    "em uso: fica".to_owned()
                } else if uses.holders() > 0 {
                    format!(
                        "anexado a {}",
                        model::count(uses.holders(), "objeto ou modelo", "objetos e modelos")
                    )
                } else if !uses.scripts.is_empty() {
                    let by: Vec<String> = uses
                        .scripts
                        .iter()
                        .map(|(path, _)| model::file_name(path).to_owned())
                        .collect();
                    let verb = if item.kind == Kind::Sound {
                        "tocado"
                    } else {
                        "usado"
                    };
                    format!("{verb} por {}", model::listed(&by))
                } else {
                    item.path.clone()
                };
                let icon = match item.kind {
                    Kind::Image => Icon::Sprite,
                    Kind::Sound => Icon::Sound,
                    Kind::Model => Icon::Cube,
                    Kind::Script => Icon::Logic,
                };
                widgets::reference_row(ui, icon, &item.name, &detail);
            }
        });
    if kept > 0 {
        ui.add_space(6.);
        widgets::callout(
            ui,
            true,
            &format!(
                "{} em uso {}: remova os vínculos antes de excluir.",
                model::count(kept, "recurso", "recursos"),
                if kept == 1 { "fica" } else { "ficam" }
            ),
        );
    }
    if holders > 0 {
        ui.add_space(6.);
        widgets::hint(
            ui,
            "Para excluir um script anexado, a OXY o tira dos objetos e modelos antes. Os \
             atributos que ele pediu continuam nos objetos.",
        );
    }
    ui.add_space(6.);
    widgets::hint(
        ui,
        if tree {
            "Os arquivos vão para a lixeira do projeto (.oxy/lixeira) e voltam com Ctrl+Z."
        } else {
            "Os recursos saem da biblioteca; os arquivos ficam no disco para o desfazer."
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_script_attached_says_to_whom_as_the_prototype_does() {
        let objects: Vec<String> = (1..=6).map(|n| format!("Checkpoint {n}")).collect();
        assert_eq!(
            attached_line(&objects, &["Checkpoint".to_owned()]),
            "Ele está anexado a 6 objetos (Checkpoint 1 a Checkpoint 6) e ao modelo Checkpoint. \
             Para excluir, a OXY tira o script de todos eles antes."
        );
        assert_eq!(
            attached_line(&["Porta".to_owned()], &[]),
            "Ele está anexado ao objeto Porta. Para excluir, a OXY o tira dele antes."
        );
        assert_eq!(
            attached_line(&[], &["Porta".to_owned(), "Chave".to_owned()]),
            "Ele está anexado aos modelos Porta e Chave. Para excluir, a OXY tira o script de \
             todos eles antes."
        );
        assert_eq!(attached_line(&[], &[]), "");
    }

    #[test]
    fn the_tree_of_the_move_opens_the_way_to_a_folder() {
        assert_eq!(
            ancestors("Pista/Checkpoints"),
            ["", "Pista/Checkpoints", "Pista"]
        );
        assert_eq!(ancestors(""), [""]);
    }
}
