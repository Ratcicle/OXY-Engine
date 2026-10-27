use super::*;
use oxy_core::document::{Asset, AssetKind, Entity, Layout, Primitive, Project, Value};

/// The project of the Library's flow, its name as the root of the PASTAS column.
const NAME: &str = "Pastas QA";

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

impl NativeQa {
    pub(super) fn check_library(&mut self, label: &str) -> Result<(), String> {
        let root = self
            .editor
            .path
            .as_deref()
            .and_then(Path::parent)
            .ok_or("O projeto não tem pasta")?
            .to_owned();
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
        let place = self.editor.qa_library_place();
        let selection = self.editor.qa_library_selection();
        let sounds = |name: &str| {
            self.editor
                .state
                .project
                .assets
                .iter()
                .filter(|a| a.kind == AssetKind::Audio && a.name == name)
                .cloned()
                .collect::<Vec<Asset>>()
        };
        match label {
            "lib7_open" => {
                for text in [
                    "PASTAS",
                    "Todos os recursos",
                    NAME,
                    "Imagens",
                    "Menu",
                    "Pista",
                    "Sons",
                    "1 outro arquivo (notas.txt) · ",
                    "Mostrar no Explorer",
                ] {
                    if !visible(text) {
                        return Err(format!("A Biblioteca não mostra {text}"));
                    }
                }
                if place != Some("") || !visible(&format!("Biblioteca: {NAME}")) {
                    return Err(format!("A Biblioteca não abriu na raiz: {place:?}"));
                }
            }
            "lib7_pista" => {
                if place != Some("Pista") {
                    return Err(format!("Clicar em Pista abriu {place:?}"));
                }
                for text in [
                    "Checkpoints",
                    "Bipe",
                    "contagem",
                    "Largada",
                    "Localizar arquivo…",
                    "ausente: Pista/Largada.wav não está na pasta",
                    "tocado por checkpoint.luau",
                    "1 outro arquivo (pista.psd) · ",
                    "Tudo 3",
                    "Sons 2",
                    "Scripts 1",
                    "Biblioteca: Pista",
                ] {
                    if !visible(text) {
                        return Err(format!("Pista não mostra {text}"));
                    }
                }
            }
            "lib7_sounds" => {
                if visible("contagem") || !visible("Bipe") || !visible("Largada") {
                    return Err("A ficha Sons não mostrou só os sons da pasta".into());
                }
            }
            "lib7_search" => {
                if !visible("checkpoint") || !visible(&format!("{NAME} › Pista › Checkpoints"))
                {
                    return Err("A busca não achou checkpoint com a pasta dele".into());
                }
                if visible("Bipe") || !contains("Busca no projeto") {
                    return Err("A busca não listou só o que tem o nome buscado".into());
                }
            }
            "lib7_locate" => {
                for text in [
                    "Localizar arquivo de “Largada”",
                    "Sons/Largada.wav",
                    "Religar",
                ] {
                    if !visible(text) {
                        return Err(format!("Localizar arquivo… não mostra {text}"));
                    }
                }
            }
            "lib7_relinked" => {
                let largada = sounds("Largada");
                if largada.len() != 1 || largada[0].path != "Sons/Largada.wav" {
                    return Err(format!("Religar deixou {largada:?}"));
                }
                undo_is("Religar arquivo")?;
                if visible("Localizar arquivo…") {
                    return Err("A linha ausente continuou na Biblioteca".into());
                }
            }
            "lib7_folder_made" => {
                if !root.join("Pista/Fases").is_dir() {
                    return Err("+ Nova pasta não criou Pista/Fases".into());
                }
                undo_is("Criar pasta Fases")?;
                if selection != ["Pista/Fases"] {
                    return Err(format!("A pasta nova não ficou selecionada: {selection:?}"));
                }
            }
            "lib7_renamed" => {
                if !root.join("Pista/Bip.wav").is_file() || root.join("Pista/Bipe.wav").exists() {
                    return Err("F2 na Biblioteca não renomeou Pista/Bipe.wav".into());
                }
                undo_is("Renomear Bipe")?;
                if sounds("Bip").len() != 1 {
                    return Err("O som não se chama Bip".into());
                }
            }
            "lib7_rule" => {
                if !contains("não aceita em nomes") {
                    return Err("O nome com : não disse a regra do Windows".into());
                }
                if !root.join("Pista/Bip.wav").is_file() {
                    return Err("O nome recusado mexeu no arquivo".into());
                }
            }
            "lib7_delete_focus" => {
                if !self
                    .editor
                    .scene()
                    .entities
                    .iter()
                    .any(|e| e.name == "Porta")
                {
                    return Err("Delete com o foco na Biblioteca apagou o objeto".into());
                }
                if !self.editor.qa_library_dialog() || !visible("Excluir o som “Bip”?") {
                    return Err("Delete com o foco na Biblioteca não pediu para excluir Bip".into());
                }
            }
            "lib7_select_all" => {
                if !self.editor.qa_library_focus() {
                    return Err("A Biblioteca perdeu o foco das teclas".into());
                }
                let mut sorted = selection.clone();
                sorted.sort();
                if sorted != ["Bip", "Pista/contagem.luau"] {
                    return Err(format!("Ctrl+A na Biblioteca selecionou {selection:?}"));
                }
            }
            "lib7_no_duplicate" => {
                if self.editor.scene().entities.len() != self.initial_entities {
                    return Err("Ctrl+D com o foco na Biblioteca duplicou objetos".into());
                }
                if sounds("Bip").len() != 1 {
                    return Err("Ctrl+D com o foco na Biblioteca duplicou o som".into());
                }
            }
            "lib7_move_dialog" => {
                for text in [
                    "Mover 2 itens para…",
                    "Mover",
                    "Cancelar",
                    "Pista (onde estão)",
                ] {
                    if !visible(text) {
                        return Err(format!("Mover para… não mostra {text}"));
                    }
                }
            }
            "lib7_moved" => {
                for path in ["Sons/Bip.wav", "Sons/contagem.luau"] {
                    if !root.join(path).is_file() {
                        return Err(format!("Mover para… não levou {path}"));
                    }
                }
                undo_is("Mover 2 itens para Sons")?;
                if place != Some("Sons") {
                    return Err(format!("A Biblioteca não abriu Sons: {place:?}"));
                }
            }
            "lib7_delete_folder_dialog" => {
                if !visible("Excluir a pasta “Fases”?")
                    || !visible("A pasta está vazia: só ela some.")
                {
                    return Err("Excluir pasta… não explicou o que acontece".into());
                }
            }
            "lib7_folder_deleted" => {
                if root.join("Pista/Fases").exists() {
                    return Err("Excluir pasta não tirou Pista/Fases".into());
                }
                undo_is("Excluir pasta Fases")?;
            }
            "lib7_folder_back" => {
                if !root.join("Pista/Fases").is_dir() {
                    return Err("Ctrl+Z não trouxe a pasta Fases de volta".into());
                }
            }
            "lib7_playing" => {
                if !self.editor.playing() {
                    return Err("O teste não está rodando".into());
                }
                if self.editor.qa_library_dialog() || !root.join("Pista/Fases").is_dir() {
                    return Err("Organizar funcionou durante o teste".into());
                }
                if !visible("Bip") || !visible("contagem") {
                    return Err("A Biblioteca parou de mostrar a pasta durante o teste".into());
                }
            }
            _ => return Err(format!("Verificação da Biblioteca desconhecida: {label}")),
        }
        Ok(())
    }
}

#[test]
#[ignore = "Real native WGPU Library with the project's folders; RawInput only"]
#[cfg(target_os = "windows")]
fn native_library_tree() {
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let output = oxy_core::qa::output_dir("v0.7/library-tree");
    std::fs::create_dir_all(&output).unwrap();
    let root = std::env::temp_dir().join(format!("oxy-library-tree-{}", new_id()));
    let png = oxy_core::painting::PaintImage::new(2, 2, [90, 90, 90, 255])
        .unwrap()
        .to_png()
        .unwrap();
    for (path, bytes) in [
        ("Pista/Bipe.wav", wav(4)),
        ("Pista/contagem.luau", b"return {}\n".to_vec()),
        (
            "Pista/Checkpoints/checkpoint.luau",
            b"local M = {}\nfunction M.on_start(self)\n    audio.play(\"Bipe\")\nend\nreturn M\n"
                .to_vec(),
        ),
        ("Pista/pista.psd", b"camadas".to_vec()),
        // Largada was saved in Pista and moved to Sons outside OXY.
        ("Sons/Largada.wav", wav(6)),
        ("Imagens/Pedra.png", png),
        ("Menu/menu.luau", b"return {}\n".to_vec()),
        ("notas.txt", b"notas".to_vec()),
    ] {
        let file = root.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, bytes).unwrap();
    }
    let path = root.join("project.oxy.json");
    let mut project = oxy_core::editing::blank_project(NAME, SceneKind::ThreeD).unwrap();
    project.layout = Layout::Tree;
    let (largada, pedra) = (new_id(), new_id());
    for (id, name, file, kind) in [
        (&largada, "Largada", "Pista/Largada.wav", AssetKind::Audio),
        (&pedra, "Pedra", "Imagens/Pedra.png", AssetKind::Texture),
    ] {
        project.assets.push(Asset {
            id: id.clone(),
            name: name.into(),
            path: file.into(),
            kind,
            model: None,
        });
    }
    let mut porta = Entity::new("Porta", None);
    let mut sound = oxy_core::graph::Node::new("action.sound", [0.; 2]);
    sound.params.insert("asset".into(), Value::Text(largada));
    porta.graph.nodes.push(sound);
    let mut chao = Entity::new("Chão", Some(Primitive::Cube));
    chao.material.texture = Some(pedra);
    project.scenes[0].entities.extend([porta, chao]);
    save(&path, &project);
    let report = Arc::new(Mutex::new(Report::default()));
    let shared = report.clone();
    let artifacts = output.clone();
    eframe::run_native(
        "OXY Engine — QA Biblioteca com pastas",
        eframe::NativeOptions {
            persist_window: false,
            renderer: eframe::Renderer::Wgpu,
            viewport: egui::ViewportBuilder::default()
                .with_title("OXY Engine — QA Biblioteca com pastas")
                .with_inner_size([1440., 900.])
                .with_active(false),
            event_loop_builder: Some(Box::new(|b| {
                b.with_any_thread(true);
            })),
            ..Default::default()
        },
        Box::new(move |cc| {
            // A dock tall enough for the folder's rows, as the prototype shows it.
            cc.egui_ctx.data_mut(|data| {
                data.insert_persisted(
                    egui::Id::new("dock"),
                    egui::containers::panel::PanelState {
                        rect: Rect::from_min_size(egui::pos2(0., 520.), Vec2::new(900., 330.)),
                    },
                )
            });
            let mut qa = NativeQa::new(cc, path, artifacts, shared);
            qa.actions = VecDeque::from([
                Action::Wait(6),
                Action::Check("lib7_open"),
                Action::Screenshot("library-root.png"),
                Action::Click("Pasta: Pista"),
                Action::Wait(4),
                Action::Check("lib7_pista"),
                Action::Screenshot("library-pista.png"),
                Action::Click("Sons 2"),
                Action::Wait(2),
                Action::Check("lib7_sounds"),
                Action::Click("Tudo 3"),
                Action::Wait(2),
                Action::Click("Buscar no projeto…"),
                Action::Text("check"),
                Action::Wait(4),
                Action::Check("lib7_search"),
                Action::Key(Key::A, true),
                Action::Key(Key::Backspace, false),
                Action::Wait(4),
                Action::Click("Localizar arquivo…"),
                Action::Wait(4),
                Action::Check("lib7_locate"),
                Action::Screenshot("library-locate.png"),
                Action::Click("Religar"),
                Action::Wait(4),
                Action::Check("lib7_relinked"),
                Action::Click("Nova pasta"),
                Action::Wait(4),
                Action::Key(Key::A, true),
                Action::Text("Fases"),
                Action::Key(Key::Enter, false),
                Action::Wait(4),
                Action::Check("lib7_folder_made"),
                Action::Click("Recurso: Bipe"),
                Action::Key(Key::F2, false),
                Action::Wait(4),
                Action::Key(Key::A, true),
                Action::Text("Bip"),
                Action::Key(Key::Enter, false),
                Action::Wait(4),
                Action::Check("lib7_renamed"),
                Action::Click("Recurso: Bip"),
                Action::Key(Key::F2, false),
                Action::Wait(4),
                Action::Key(Key::A, true),
                Action::Text("Bi:p"),
                Action::Key(Key::Enter, false),
                Action::Wait(4),
                Action::Check("lib7_rule"),
                Action::Screenshot("library-rename-rule.png"),
                Action::Key(Key::Escape, false),
                Action::Wait(2),
                Action::SelectEntity("Porta"),
                Action::Click("Recurso: Bip"),
                Action::Key(Key::Delete, false),
                Action::Wait(4),
                Action::Check("lib7_delete_focus"),
                Action::Click("Cancelar"),
                Action::Wait(4),
                Action::Key(Key::A, true),
                Action::Wait(2),
                Action::Check("lib7_select_all"),
                Action::Key(Key::D, true),
                Action::Wait(2),
                Action::Check("lib7_no_duplicate"),
                Action::Context("Recurso: contagem"),
                Action::Click("Mover 2 itens para…"),
                Action::Wait(4),
                Action::Check("lib7_move_dialog"),
                Action::Click("Sons"),
                Action::Wait(2),
                Action::Screenshot("library-move.png"),
                Action::Click("Mover"),
                Action::Wait(4),
                Action::Check("lib7_moved"),
                Action::Context("Pasta: Fases"),
                Action::Click("Excluir pasta…"),
                Action::Wait(4),
                Action::Check("lib7_delete_folder_dialog"),
                Action::Screenshot("library-delete-folder.png"),
                Action::Click("Excluir pasta"),
                Action::Wait(4),
                Action::Check("lib7_folder_deleted"),
                Action::Key(Key::Z, true),
                Action::Wait(4),
                Action::Check("lib7_folder_back"),
                Action::Click("Jogar"),
                Action::Wait(20),
                Action::Context("Pasta: Fases"),
                Action::Wait(2),
                Action::Click("Excluir pasta…"),
                Action::Wait(4),
                Action::Check("lib7_playing"),
                Action::Screenshot("library-playing.png"),
                Action::Key(Key::Escape, false),
                Action::Click("Parar"),
            ]);
            Ok(Box::new(qa))
        }),
    )
    .unwrap();
    let report = report.lock().unwrap();
    let text = format!(
        "Concluído: {}\nErro: {:?}\n{}\nCapturas: {:?}",
        report.done,
        report.error,
        report.steps.join("\n"),
        report.screenshots
    );
    std::fs::write(output.join("native-library-tree.txt"), &text).unwrap();
    let _ = std::fs::remove_dir_all(&root);
    assert!(report.done && report.error.is_none(), "{text}");
    assert_eq!(report.screenshots.len(), 7, "{text}");
}

/// Saves `project` at `path` as the project file of its folder.
fn save(path: &Path, project: &Project) {
    oxy_core::persistence::save_project(path, project).unwrap();
}
