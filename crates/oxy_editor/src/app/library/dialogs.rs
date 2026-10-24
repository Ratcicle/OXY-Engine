//! The Library's dialogs: "Mover para…" (the project's folders, a new one made on the way),
//! "Excluir pasta…" (what goes to the root, told before) and deleting scripts or several items
//! at once (to the project's trash, a script taken off the objects that attach it first). Each
//! runs the tree's commands as one undo step.
use super::*;
use crate::widgets;

/// The fill of the boxes inside the dialogs.
const BOX_FILL: Color32 = Color32::from_rgb(30, 30, 32);

/// "Mover para…" for `items`.
pub(super) struct MoveDialog {
    items: Vec<Key>,
    /// The folder chosen ("" is the root).
    target: Option<String>,
    /// The name of a new folder being typed in the target.
    new_folder: Option<String>,
    error: Option<String>,
}

/// "Excluir pasta…" for `folder`, with what the deletion would tell, planned when it opened.
pub(super) struct DeleteFolder {
    folder: String,
    told: Result<Vec<String>, String>,
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
            items,
            target,
            new_folder: None,
            error: None,
        });
    }

    /// Opens "Excluir pasta…" for `folder`, telling first where its items go.
    pub(super) fn open_delete_folder(&mut self, folder: &str) {
        let Some(root) = self.project_root() else {
            return;
        };
        let commands = [Command::DeleteFolder {
            folder: folder.to_owned(),
        }];
        let told = command::TreeView::read(&root)
            .map_err(|error| error.to_string())
            .and_then(|mut view| {
                command::plan_with_files(&self.state.project, &self.scene_id, &commands, &mut view)
                    .map_err(|error| plain(&error.to_string()))
            })
            .map(|(_, applied)| {
                applied
                    .into_iter()
                    .flat_map(|a| a.warnings)
                    .map(|w| plain(&w))
                    .collect()
            });
        self.library.delete_folder = Some(DeleteFolder {
            folder: folder.to_owned(),
            told,
        });
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
        let title = match (dialog.items.as_slice(), names.as_slice()) {
            ([Key::Folder(folder)], _) => {
                format!("Mover a pasta “{}” para…", model::file_name(folder))
            }
            ([_], [name]) => format!(
                "Mover “{}” para…",
                name.rsplit_once(" (").map_or(name.as_str(), |(n, _)| n)
            ),
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
                                let inside = moving.iter().any(|m| {
                                    folder.path == *m || folder.path.starts_with(&format!("{m}/"))
                                });
                                let mut text = RichText::new(&folder.name);
                                if current.as_deref() == Some(folder.path.as_str()) {
                                    text = RichText::new(format!("{} (onde estão)", folder.name));
                                }
                                ui.horizontal(|ui| {
                                    ui.add_space(folder.depth as f32 * 14.);
                                    let chosen =
                                        dialog.target.as_deref() == Some(folder.path.as_str());
                                    let row = ui
                                        .add_enabled(
                                            !inside,
                                            egui::Button::selectable(chosen, text),
                                        )
                                        .on_disabled_hover_text(
                                            "Uma pasta não vai para dentro dela mesma.",
                                        );
                                    if row.clicked() {
                                        dialog.target = Some(folder.path.clone());
                                        dialog.error = None;
                                    }
                                });
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
                ui.add_space(4.);
                ui.label(RichText::new(error).size(12.).color(theme::ERROR));
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
            let items: Vec<String> = dialog
                .items
                .iter()
                .map(|key| match key {
                    Key::Asset(id) => id.clone(),
                    Key::Script(path) | Key::Folder(path) => path.clone(),
                })
                .collect();
            let label = match names.as_slice() {
                [one] => format!(
                    "Mover {} para {}",
                    one.rsplit_once(" (").map_or(one.as_str(), |(n, _)| n),
                    self.folder_label(&target)
                ),
                _ => format!(
                    "Mover {} itens para {}",
                    items.len(),
                    self.folder_label(&target)
                ),
            };
            let commands = vec![Command::Move {
                items,
                to: target.clone(),
                kind: None,
            }];
            match self.library_commands(&label, commands) {
                Ok(_) => self.open_place(Place::Folder(target)),
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
        let Some(dialog) = self.library.delete_folder.take() else {
            return;
        };
        let name = model::file_name(&dialog.folder).to_owned();
        let subfolders = lib.subfolders(&dialog.folder);
        let items: Vec<String> = subfolders
            .iter()
            .map(|f| format!("{} (pasta)", f.name))
            .chain(
                lib.shown(Some(&dialog.folder), "", None)
                    .into_iter()
                    .map(labelled),
            )
            .collect();
        let others = lib.others.get(&dialog.folder).map_or(0, Vec::len);
        let mut close = false;
        let mut confirm = false;
        let response = egui::Modal::new("library_delete_folder".into()).show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 48.).clamp(160., 440.));
            close = widgets::dialog_header(ui, &format!("Excluir a pasta “{name}”?"));
            let what = match items.len() + others {
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
            if !items.is_empty() || dialog.told.as_ref().is_ok_and(|t| !t.is_empty()) {
                ui.add_space(6.);
                egui::Frame::new()
                    .fill(BOX_FILL)
                    .stroke(egui::Stroke::new(1., theme::CARD_BORDER))
                    .corner_radius(6)
                    .inner_margin(egui::Margin::symmetric(10, 8))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        if !items.is_empty() {
                            let mut shown: Vec<String> = items.iter().take(8).cloned().collect();
                            if items.len() > 8 {
                                shown.push(format!("e mais {}", items.len() - 8));
                            }
                            ui.add(
                                egui::Label::new(RichText::new(shown.join(" · ")).size(12.5))
                                    .wrap(),
                            );
                        }
                        if let Ok(told) = &dialog.told {
                            for line in told {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(line).size(12.).color(theme::WARNING),
                                    )
                                    .wrap(),
                                );
                            }
                        }
                    });
            }
            if let Err(error) = &dialog.told {
                ui.add_space(6.);
                widgets::callout(ui, true, error);
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
            if let Err(error) = self.library_commands(&label, commands) {
                self.warn(error);
            } else if matches!(&self.library.place, Place::Folder(open) if open == &dialog.folder || open.starts_with(&format!("{}/", dialog.folder)))
            {
                self.library.place = Place::default();
            }
            return;
        }
        if !(close || response.should_close()) {
            self.library.delete_folder = Some(dialog);
        }
    }

    fn delete_items_dialog(&mut self, ctx: &egui::Context, lib: &Library) {
        let Some(keys) = self.library.delete_items.take() else {
            return;
        };
        let items: Vec<&Item> = keys.iter().filter_map(|key| lib.item(key)).collect();
        if items.is_empty() {
            return;
        }
        // An image, sound or model in use stays; a script attached leaves its objects first.
        let in_use = |item: &Item| match &item.key {
            Key::Asset(id) => !oxy_core::editing::asset_uses(&self.state.project, id).is_empty(),
            _ => false,
        };
        let attached = |item: &Item| match &item.key {
            Key::Script(path) => oxy_core::script::attached_by(&self.state.project, path),
            _ => 0,
        };
        let kept: Vec<&Item> = items.iter().copied().filter(|i| in_use(i)).collect();
        let going: Vec<&Item> = items.iter().copied().filter(|i| !in_use(i)).collect();
        let objects: usize = going.iter().map(|i| attached(i)).sum();
        let title = match items.as_slice() {
            [one] => format!("Excluir {} “{}”?", one.kind.article(), one.name),
            _ => format!("Excluir {} itens?", items.len()),
        };
        let tree = self.state.project.layout == Layout::Tree;
        let mut close = false;
        let mut confirm = false;
        let response = egui::Modal::new("library_delete_items".into()).show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 48.).clamp(160., 460.));
            close = widgets::dialog_header(ui, &title);
            egui::ScrollArea::vertical()
                .id_salt("library_delete_items")
                .max_height(220.)
                .show(ui, |ui| {
                    for item in &items {
                        let detail = if in_use(item) {
                            "em uso: fica".to_owned()
                        } else {
                            match attached(item) {
                                0 => item.path.clone(),
                                n => format!("anexado a {}", model::count(n, "objeto", "objetos")),
                            }
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
            if !kept.is_empty() {
                ui.add_space(6.);
                widgets::callout(
                    ui,
                    true,
                    &format!(
                        "{} em uso {}: remova os vínculos antes de excluir.",
                        model::count(kept.len(), "recurso", "recursos"),
                        if kept.len() == 1 { "fica" } else { "ficam" }
                    ),
                );
            }
            if objects > 0 {
                ui.add_space(6.);
                widgets::hint(
                    ui,
                    "Para excluir um script anexado, a OXY o tira dos objetos antes. Os atributos \
                     que ele pediu continuam nos objetos.",
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
            widgets::dialog_footer(ui, |ui| {
                let label = if objects > 0 {
                    format!("Tirar dos {objects} e excluir")
                } else if tree {
                    "Mandar para a lixeira".to_owned()
                } else {
                    // The old layout keeps the files on disk.
                    "Excluir".to_owned()
                };
                confirm = !going.is_empty() && widgets::danger_button(ui, &label, true).clicked();
                close |= widgets::secondary_button(ui, "Cancelar").clicked();
            });
        });
        if confirm {
            let commands: Vec<Command> = going
                .iter()
                .map(|item| Command::DeleteAsset {
                    asset: match &item.key {
                        Key::Asset(id) => id.clone(),
                        Key::Script(path) | Key::Folder(path) => path.clone(),
                    },
                    kind: Some(item.kind.command()),
                    detach: (attached(item) > 0).then_some(true),
                })
                .collect();
            let removed: Vec<Id> = going
                .iter()
                .filter_map(|item| match &item.key {
                    Key::Asset(id) => Some(id.clone()),
                    _ => None,
                })
                .collect();
            let label = match going.as_slice() {
                [one] => format!("Excluir {}", one.name),
                _ => format!("Excluir {} itens", going.len()),
            };
            match self.library_commands(&label, commands) {
                Ok(_) => {
                    for id in removed {
                        self.state.images.remove(&id);
                        self.renderer.clear_texture_override(&id);
                        self.play.game_ui.clear_texture_override(&id);
                    }
                    self.library.selection.clear();
                }
                Err(error) => self.warn(error),
            }
            return;
        }
        if !(close || response.should_close()) {
            self.library.delete_items = Some(keys);
        }
    }
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
