use super::*;
use oxy_core::document::{Asset, AssetKind, Entity, Layout, Project, Value};

/// The project in folders of the dialogs' flow, its name as the root of the PASTAS column.
const NAME: &str = "Diálogos QA";
/// The project of the old layout of the conversion's flow.
const OLD: &str = "Organizar QA";

/// A mono 16-bit PCM WAV of `samples` silent samples.
fn wav(samples: u32) -> Vec<u8> {
    let data = samples * 2;
    let mut bytes = Vec::new();
    bytes.extend(b"RIFF");
    bytes.extend((36 + data).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16_u32.to_le_bytes());
    bytes.extend(1_u16.to_le_bytes());
    bytes.extend(1_u16.to_le_bytes());
    bytes.extend(8000_u32.to_le_bytes());
    bytes.extend(16000_u32.to_le_bytes());
    bytes.extend(2_u16.to_le_bytes());
    bytes.extend(16_u16.to_le_bytes());
    bytes.extend(b"data");
    bytes.extend(data.to_le_bytes());
    bytes.extend(vec![0_u8; data as usize]);
    bytes
}

fn png() -> Vec<u8> {
    oxy_core::painting::PaintImage::new(2, 2, [90, 90, 90, 255])
        .unwrap()
        .to_png()
        .unwrap()
}

/// Writes `files` under `root`.
fn put(root: &Path, files: &[(&str, Vec<u8>)]) {
    for (path, bytes) in files {
        let file = root.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, bytes).unwrap();
    }
}

/// The files in the project's trash, its index left out.
fn trashed(root: &Path) -> usize {
    std::fs::read_dir(root.join(".oxy/lixeira")).map_or(0, |listing| {
        listing
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name() != "indice.json")
            .count()
    })
}

impl NativeQa {
    pub(super) fn check_library_dialogs(&mut self, label: &str) -> Result<(), String> {
        let root = self
            .editor
            .path
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_owned)
            .or_else(|| self.qa_root.clone())
            .ok_or("O projeto não tem pasta")?;
        self.qa_root = Some(root.clone());
        let visible = |text: &str| self.surface.texts.iter().any(|t| t.text == text);
        let contains = |text: &str| self.surface.texts.iter().any(|t| t.text.contains(text));
        let undo = self.editor.qa_undo_label();
        let undo_is = |expected: &str| {
            if undo == Some(expected) {
                Ok(())
            } else {
                Err(format!("Desfazer nomeia {undo:?}, não {expected}"))
            }
        };
        let all_visible = |texts: &[&str], what: &str| -> Result<(), String> {
            match texts.iter().find(|text| !visible(text)) {
                Some(text) => Err(format!("{what} não mostra {text}")),
                None => Ok(()),
            }
        };
        let file = |path: &str| root.join(path).is_file();
        let code = self.editor.qa_code_path().map(str::to_owned);
        let attached = self
            .editor
            .scene()
            .entities
            .iter()
            .filter(|e| {
                e.scripts
                    .iter()
                    .any(|s| s.path.ends_with("checkpoint.luau"))
            })
            .count();
        match label {
            "lib8_open" => all_visible(&["Bipe", "contagem", "Checkpoints"], "Pista")?,
            "lib8_drag_green" => {
                all_visible(&["Mover para Sons", "Bipe"], "O arrasto sobre Sons")?;
            }
            "lib8_dropped" => {
                if !file("Sons/Bipe.wav") || file("Pista/Bipe.wav") {
                    return Err("Soltar em Sons não levou Pista/Bipe.wav".into());
                }
                undo_is("Mover Bipe para Sons")?;
            }
            "lib8_drop_undone" => {
                if !file("Pista/Bipe.wav") || file("Sons/Bipe.wav") {
                    return Err("Ctrl+Z não trouxe Bipe de volta para Pista".into());
                }
            }
            "lib8_drag_red" => all_visible(
                &["Sons já tem um script chamado contagem. Renomeie um dos dois antes."],
                "O arrasto recusado",
            )?,
            "lib8_refused" => {
                if !file("Pista/contagem.luau") || !file("Sons/contagem.luau") {
                    return Err("O arrasto recusado mexeu nos arquivos".into());
                }
                if undo.is_some_and(|u| u.contains("contagem")) {
                    return Err(format!("O arrasto recusado virou passo: {undo:?}"));
                }
            }
            "lib8_root" => {
                if !file("Bipe.wav") {
                    return Err("Soltar no nome do projeto não levou Bipe para a raiz".into());
                }
                undo_is(&format!("Mover Bipe para {NAME}"))?;
            }
            "lib8_code_open" => {
                if code.as_deref() != Some("Pista/contagem.luau") {
                    return Err(format!("Ver código abriu {code:?}"));
                }
            }
            "lib8_move_dialog" => all_visible(
                &[
                    "Mover “contagem” para…",
                    "Pista (onde estão)",
                    "+ Nova pasta em Pista",
                    "Mover",
                    "Cancelar",
                ],
                "Mover para…",
            )?,
            "lib8_code_followed" => {
                if !file("Menu/contagem.luau") {
                    return Err("Mover para… não levou contagem para Menu".into());
                }
                undo_is("Mover contagem para Menu")?;
                if code.as_deref() != Some("Menu/contagem.luau") {
                    return Err(format!("A aba do código não seguiu o script: {code:?}"));
                }
            }
            "lib8_code_back" => {
                if code.as_deref() != Some("Pista/contagem.luau") {
                    return Err(format!(
                        "Desfazer não levou a aba do código junto: {code:?}"
                    ));
                }
            }
            "lib8_delete_sound" => {
                all_visible(
                    &[
                        "Excluir o som “Bipe”?",
                        "Este som está em uso. Se ele sair, quem o usa fica sem som:",
                        "Pista/Checkpoints/checkpoint.luau",
                        ", linha 3 (toca “Bipe”)",
                        "Mandar para a lixeira",
                    ],
                    "Excluir som usado por script",
                )?;
            }
            "lib8_sound_trashed" => {
                if file("Pista/Bipe.wav") || trashed(&root) != 1 {
                    return Err("Bipe não foi para a lixeira do projeto".into());
                }
                undo_is("Excluir Bipe")?;
            }
            "lib8_trash_menu" => {
                if !contains("Esvaziar lixeira do projeto (1 arquivo, 1 KB)") {
                    return Err("O menu Projeto não conta o que a lixeira tem".into());
                }
            }
            "lib8_empty_trash" => all_visible(
                &[
                    "Esvaziar a lixeira do projeto?",
                    "1 passo de desfazer deixa de valer: o Ctrl+Z para antes do passo mais novo \
                     que precisa de um arquivo da lixeira.",
                    "Esvaziar lixeira",
                ],
                "Esvaziar lixeira",
            )?,
            "lib8_trash_emptied" => {
                if trashed(&root) != 0 {
                    return Err("A lixeira do projeto não ficou vazia".into());
                }
                if undo.is_some() {
                    return Err(format!("O passo que precisava da lixeira ficou: {undo:?}"));
                }
            }
            "lib8_delete_script" => all_visible(
                &[
                    "Excluir o script “checkpoint”?",
                    "Ele está anexado a 3 objetos (Checkpoint 1 a Checkpoint 3). Para excluir, a \
                     OXY tira o script de todos eles antes.",
                    "Os atributos que ele pediu (Ordem) continuam nos objetos. Tudo volta com \
                     Ctrl+Z.",
                    "Tirar dos 3 e excluir",
                ],
                "Excluir script anexado",
            )?,
            "lib8_script_trashed" => {
                if file("Pista/Checkpoints/checkpoint.luau") || attached != 0 {
                    return Err("O script não saiu dos objetos e da pasta".into());
                }
                undo_is("Excluir checkpoint")?;
            }
            "lib8_script_back" => {
                if !file("Pista/Checkpoints/checkpoint.luau") || attached != 3 {
                    return Err("Ctrl+Z não devolveu o script aos 3 objetos".into());
                }
            }
            "lib8_delete_many" => all_visible(
                &["Excluir 2 itens?", "Mandar para a lixeira"],
                "Excluir 2 itens",
            )?,
            "lib8_delete_folder" => all_visible(
                &[
                    "Excluir a pasta “Menu”?",
                    "Nenhum recurso é apagado: os 2 itens da pasta vão para a raiz do projeto, e \
                     só a pasta some.",
                    "A raiz já tem um script “menu”: este vai como “menu 2”.",
                    "Excluir pasta",
                ],
                "Excluir pasta",
            )?,
            "lib8_folder_deleted" => {
                if root.join("Menu").exists() || !file("menu 2.luau") || !file("Clique.wav") {
                    return Err("Excluir pasta não levou os itens para a raiz".into());
                }
                undo_is("Excluir pasta Menu")?;
            }
            "lib8_folder_back" => {
                if !file("Menu/menu.luau") || !file("Menu/Clique.wav") || file("menu 2.luau") {
                    return Err("Ctrl+Z não refez a pasta Menu".into());
                }
            }
            "lib8_discard" => all_visible(
                &[
                    "Descartar as alterações?",
                    "1 arquivo movido volta para Pista/",
                    "Descartar tudo",
                ],
                "Descartar alterações",
            )?,
            "lib8_discarded" => {
                if !file("Pista/contagem.luau") || file("contagem.luau") {
                    return Err("Descartar não trouxe contagem de volta para Pista".into());
                }
            }
            "lib8o_variant" => {
                all_visible(
                    &[
                        "Organizar o projeto em pastas",
                        "Organizar numa pasta nova…",
                        "Organizar aqui e incluir o arquivo",
                        "Cancelar",
                    ],
                    "A conversão de uma pasta com outros arquivos",
                )?;
                if !contains("Fotos/praia.png") {
                    return Err("A conversão não listou Fotos/praia.png".into());
                }
            }
            "lib8o_cancelled" => {
                if self.editor.state.project.layout != Layout::Legacy
                    || root.join("Sons").exists()
                    || root.join(".oxy/backup").exists()
                {
                    return Err("Cancelar a conversão mexeu no projeto".into());
                }
            }
            "lib8o_photo_moved_out" => {
                // The person takes the photo out of the project's folder, as the dialog says.
                std::fs::remove_file(root.join("Fotos/praia.png"))
                    .map_err(|error| format!("A foto não saiu da pasta: {error}"))?;
                let _ = std::fs::remove_dir(root.join("Fotos"));
            }
            "lib8o_dialog" => {
                all_visible(
                    &[
                        "Organizar o projeto em pastas",
                        "1 imagem vai para Imagens/ com o nome dela",
                        "1 som vai para Sons/ com o nome dele",
                        "Modelos virariam arquivos em Modelos/",
                        "A pasta scripts/ fica como está",
                        "1 script",
                        "Organizar e salvar",
                        "Cancelar",
                    ],
                    "Organizar o projeto em pastas",
                )?;
                if !contains("cópia completa do projeto") || !contains("→ Sons/Bipe.wav") {
                    return Err("A conversão não disse o que vai para onde e o backup".into());
                }
            }
            "lib8o_organized" => {
                if self.editor.state.project.layout != Layout::Tree {
                    return Err("Organizar e salvar não organizou o projeto".into());
                }
                if !file("Sons/Bipe.wav") || !file("Imagens/Pedra.png") {
                    return Err("Os arquivos não foram para Sons/ e Imagens/".into());
                }
                if !root.join(".oxy/backup").is_dir() {
                    return Err("A conversão não guardou a cópia do projeto".into());
                }
                if !self
                    .editor
                    .console
                    .messages
                    .iter()
                    .any(|m| m.starts_with("Projeto organizado em pastas ao salvar"))
                {
                    return Err("O Console não contou a conversão".into());
                }
            }
            _ => return Err(format!("Verificação dos diálogos desconhecida: {label}")),
        }
        Ok(())
    }
}

/// Runs the flow `actions` on the project at `path` in a 1440 × 900 window with a dock tall
/// enough for the folder's rows; answers what it told and the screenshots taken.
fn run_flow(title: &'static str, path: PathBuf, output: PathBuf, actions: Vec<Action>) -> Report {
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let report = Arc::new(Mutex::new(Report::default()));
    let shared = report.clone();
    eframe::run_native(
        title,
        eframe::NativeOptions {
            persist_window: false,
            renderer: eframe::Renderer::Wgpu,
            viewport: egui::ViewportBuilder::default()
                .with_title(title)
                .with_inner_size([1440., 900.])
                .with_active(false),
            event_loop_builder: Some(Box::new(|b| {
                b.with_any_thread(true);
            })),
            ..Default::default()
        },
        Box::new(move |cc| {
            cc.egui_ctx.data_mut(|data| {
                data.insert_persisted(
                    egui::Id::new("dock"),
                    egui::containers::panel::PanelState {
                        rect: Rect::from_min_size(egui::pos2(0., 520.), Vec2::new(900., 330.)),
                    },
                )
            });
            let mut qa = NativeQa::new(cc, path, output, shared);
            qa.actions = VecDeque::from(actions);
            Ok(Box::new(qa))
        }),
    )
    .unwrap();
    std::mem::take(&mut *report.lock().unwrap())
}

#[test]
#[ignore = "Real native WGPU Library dialogs: drag between folders, move, delete, trash, discard"]
#[cfg(target_os = "windows")]
fn native_library_dialogs() {
    let output = oxy_core::qa::output_dir("v0.7/library-dialogs");
    std::fs::create_dir_all(&output).unwrap();
    let root = std::env::temp_dir().join(format!("oxy-library-dialogs-{}", new_id()));
    put(
        &root,
        &[
            ("Pista/Bipe.wav", wav(4)),
            ("Pista/contagem.luau", b"return {}\n".to_vec()),
            (
                "Pista/Checkpoints/checkpoint.luau",
                b"local M = {}\nfunction M.on_start(self)\n    audio.play(\"Bipe\")\nend\n\
                  M.attributes = { Ordem = 1 }\nreturn M\n"
                    .to_vec(),
            ),
            // A script of the same name in Sons: dragging contagem there is refused.
            ("Sons/contagem.luau", b"return {}\n".to_vec()),
            ("Sons/Largada.wav", wav(6)),
            ("Menu/Clique.wav", wav(8)),
            ("Menu/menu.luau", b"return {}\n".to_vec()),
            // The root has a menu.luau too: deleting Menu brings its own as menu 2.
            ("menu.luau", b"return {}\n".to_vec()),
        ],
    );
    let path = root.join("project.oxy.json");
    let mut project = oxy_core::editing::blank_project(NAME, SceneKind::ThreeD).unwrap();
    project.layout = Layout::Tree;
    for (name, file) in [
        ("Bipe", "Pista/Bipe.wav"),
        ("Largada", "Sons/Largada.wav"),
        ("Clique", "Menu/Clique.wav"),
    ] {
        project.assets.push(Asset {
            id: new_id(),
            name: name.into(),
            path: file.into(),
            kind: AssetKind::Audio,
            model: None,
        });
    }
    for n in 1..=3 {
        let mut checkpoint = Entity::new(format!("Checkpoint {n}"), None);
        checkpoint.scripts.push(oxy_core::script::ScriptRef::new(
            "Pista/Checkpoints/checkpoint.luau",
        ));
        checkpoint
            .attributes
            .insert("Ordem".into(), Value::Number(n as f64));
        project.scenes[0].entities.push(checkpoint);
    }
    save(&path, &project);
    let report = run_flow(
        "OXY Engine — QA diálogos da Biblioteca",
        path,
        output.clone(),
        vec![
            Action::Wait(6),
            Action::Click("Pasta: Pista"),
            Action::Wait(4),
            Action::Check("lib8_open"),
            // Dragging between folders: green, dropped, undone; red with why; the root.
            Action::DragHold("Recurso: Bipe", "Pasta: Sons"),
            Action::Wait(4),
            Action::Check("lib8_drag_green"),
            Action::Screenshot("drag-green.png"),
            Action::Release,
            Action::Wait(4),
            Action::Check("lib8_dropped"),
            Action::Key(Key::Z, true),
            Action::Wait(4),
            Action::Check("lib8_drop_undone"),
            Action::DragHold("Recurso: contagem", "Pasta: Sons"),
            Action::Wait(4),
            Action::Check("lib8_drag_red"),
            Action::Screenshot("drag-red.png"),
            Action::Release,
            Action::Wait(4),
            Action::Check("lib8_refused"),
            Action::DragHold("Recurso: Bipe", "Pasta: Diálogos QA"),
            Action::Wait(4),
            Action::Release,
            Action::Wait(4),
            Action::Check("lib8_root"),
            Action::Key(Key::Z, true),
            Action::Wait(4),
            // Mover para…, with the code tab following the script and coming back with Ctrl+Z.
            Action::Click("Ver código"),
            Action::Wait(4),
            Action::Check("lib8_code_open"),
            Action::Click("Biblioteca"),
            Action::Wait(4),
            Action::Context("Recurso: contagem"),
            Action::Click("Mover para…"),
            Action::Wait(4),
            Action::Check("lib8_move_dialog"),
            Action::Screenshot("move-dialog.png"),
            Action::Click("Menu"),
            Action::Wait(2),
            Action::Click("Mover"),
            Action::Wait(4),
            Action::Check("lib8_code_followed"),
            Action::Key(Key::Z, true),
            Action::Wait(4),
            Action::Check("lib8_code_back"),
            // A sound a script plays, sent to the trash; the trash emptied from the menu.
            Action::Click("Pasta: Pista"),
            Action::Wait(4),
            Action::Click("Recurso: Bipe"),
            Action::Key(Key::Delete, false),
            Action::Wait(4),
            Action::Check("lib8_delete_sound"),
            Action::Screenshot("delete-sound.png"),
            Action::Click("Mandar para a lixeira"),
            Action::Wait(4),
            Action::Check("lib8_sound_trashed"),
            Action::Click("Projeto"),
            Action::Wait(2),
            Action::Check("lib8_trash_menu"),
            Action::Click("Esvaziar lixeira do projeto (1 arquivo, 1 KB)"),
            Action::Wait(4),
            Action::Check("lib8_empty_trash"),
            Action::Screenshot("empty-trash.png"),
            Action::Click("Esvaziar lixeira"),
            Action::Wait(4),
            Action::Check("lib8_trash_emptied"),
            // A script attached to three objects.
            Action::Click("Pasta: Checkpoints"),
            Action::Wait(4),
            Action::Click("Recurso: checkpoint"),
            Action::Key(Key::Delete, false),
            Action::Wait(4),
            Action::Check("lib8_delete_script"),
            Action::Screenshot("delete-script.png"),
            Action::Click("Tirar dos 3 e excluir"),
            Action::Wait(4),
            Action::Check("lib8_script_trashed"),
            Action::Key(Key::Z, true),
            Action::Wait(4),
            Action::Check("lib8_script_back"),
            // Several items, then the folder whose script the root already has.
            Action::Click("Pasta: Menu"),
            Action::Wait(4),
            Action::Click("Recurso: Clique"),
            Action::Key(Key::A, true),
            Action::Key(Key::Delete, false),
            Action::Wait(4),
            Action::Check("lib8_delete_many"),
            Action::Click("Cancelar"),
            Action::Wait(4),
            Action::Context("Pasta: Menu"),
            Action::Click("Excluir pasta…"),
            Action::Wait(4),
            Action::Check("lib8_delete_folder"),
            Action::Screenshot("delete-folder.png"),
            Action::Click("Excluir pasta"),
            Action::Wait(4),
            Action::Check("lib8_folder_deleted"),
            Action::Key(Key::Z, true),
            Action::Wait(4),
            Action::Check("lib8_folder_back"),
            // Descartar alterações brings the files back to the last save.
            Action::Click("Pasta: Pista"),
            Action::Wait(4),
            Action::DragHold("Recurso: contagem", "Pasta: Diálogos QA"),
            Action::Wait(4),
            Action::Release,
            Action::Wait(4),
            Action::Click("Projeto"),
            Action::Click("Tela inicial"),
            Action::Wait(4),
            Action::Click("Descartar alterações"),
            Action::Wait(4),
            Action::Check("lib8_discard"),
            Action::Screenshot("discard.png"),
            Action::Click("Descartar tudo"),
            Action::Wait(10),
            Action::Check("lib8_discarded"),
        ],
    );
    let text = format!(
        "Concluído: {}\nErro: {:?}\n{}\nCapturas: {:?}",
        report.done,
        report.error,
        report.steps.join("\n"),
        report.screenshots
    );
    std::fs::write(output.join("native-library-dialogs.txt"), &text).unwrap();
    let _ = std::fs::remove_dir_all(&root);
    assert!(report.done && report.error.is_none(), "{text}");
    assert_eq!(report.screenshots.len(), 8, "{text}");
}

#[test]
#[ignore = "Real native WGPU conversion of an old project on its first save"]
#[cfg(target_os = "windows")]
fn native_library_organize() {
    let output = oxy_core::qa::output_dir("v0.7/library-organize");
    std::fs::create_dir_all(&output).unwrap();
    let root = std::env::temp_dir().join(format!("oxy-library-organize-{}", new_id()));
    let mut project: Project = oxy_core::editing::blank_project(OLD, SceneKind::ThreeD).unwrap();
    project.layout = Layout::Legacy;
    let (bipe, pedra) = (new_id(), new_id());
    put(
        &root,
        &[
            (&format!("assets/{bipe}.wav"), wav(4)),
            (&format!("assets/{pedra}.png"), png()),
            ("scripts/porta.luau", b"return {}\n".to_vec()),
            // An image the project does not name: the dialog offers its variant.
            ("Fotos/praia.png", png()),
        ],
    );
    for (id, name, kind, extension) in [
        (&bipe, "Bipe", AssetKind::Audio, "wav"),
        (&pedra, "Pedra", AssetKind::Texture, "png"),
    ] {
        project.assets.push(Asset {
            id: id.clone(),
            name: name.into(),
            path: format!("assets/{id}.{extension}"),
            kind,
            model: None,
        });
    }
    let path = root.join("project.oxy.json");
    save(&path, &project);
    let report = run_flow(
        "OXY Engine — QA organizar o projeto em pastas",
        path,
        output.clone(),
        vec![
            Action::Wait(6),
            Action::Key(Key::S, true),
            Action::Wait(4),
            Action::Check("lib8o_variant"),
            Action::Screenshot("organize-variant.png"),
            Action::Click("Cancelar"),
            Action::Wait(4),
            Action::Check("lib8o_cancelled"),
            Action::Check("lib8o_photo_moved_out"),
            Action::Key(Key::S, true),
            Action::Wait(4),
            Action::Check("lib8o_dialog"),
            Action::Screenshot("organize.png"),
            Action::Click("Organizar e salvar"),
            Action::Wait(10),
            Action::Check("lib8o_organized"),
            Action::Screenshot("organized.png"),
        ],
    );
    let text = format!(
        "Concluído: {}\nErro: {:?}\n{}\nCapturas: {:?}",
        report.done,
        report.error,
        report.steps.join("\n"),
        report.screenshots
    );
    std::fs::write(output.join("native-library-organize.txt"), &text).unwrap();
    let _ = std::fs::remove_dir_all(&root);
    assert!(report.done && report.error.is_none(), "{text}");
    assert_eq!(report.screenshots.len(), 3, "{text}");
}

/// Saves `project` at `path` as the project file of its folder.
fn save(path: &Path, project: &Project) {
    oxy_core::persistence::save_project(path, project).unwrap();
}
