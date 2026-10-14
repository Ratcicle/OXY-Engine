//! Exportar jogo in the real editor, on a copy of the example: the dialog opens from the
//! Projeto menu, refuses a relative destination with the export's own message and Exportar
//! off, exports to a new folder with the game's title, window and full screen changed (one
//! undo step) while the editor goes on, shows the result and the Console line, then offers to
//! replace that export, keeps it while one of its files is held open (as a running game holds
//! its executable) and replaces it once the file is let go. The exported executable checked
//! itself each time. An agent exports through its channel too, answered when it is over.
use super::agents::{answer, client, client_finished, take_results};
use super::*;
use oxy_core::{export, persistence};
use serde_json::json;
use std::sync::{OnceLock, mpsc};

/// The title the dialog gives the game: its folder and executable.
const TITLE: &str = "Validação Exportada";
/// The scene the example starts in.
const START: &str = "A · Sala de plataforma 2D";

/// Where the games go: a new folder in the temporary folder.
fn destination() -> &'static Path {
    static FOLDER: OnceLock<PathBuf> = OnceLock::new();
    FOLDER.get_or_init(|| {
        std::env::temp_dir()
            .join(format!("oxy-native-export-{}", new_id()))
            .join("Jogos")
    })
}
fn game_folder() -> PathBuf {
    destination().join(TITLE)
}
/// A file of the exported game held open, as the running game holds its executable.
fn held() -> &'static Mutex<Option<std::fs::File>> {
    static HELD: OnceLock<Mutex<Option<std::fs::File>>> = OnceLock::new();
    HELD.get_or_init(|| Mutex::new(None))
}
/// What lets the held export start.
fn gate() -> &'static Mutex<Option<mpsc::Sender<()>>> {
    static GATE: OnceLock<Mutex<Option<mpsc::Sender<()>>>> = OnceLock::new();
    GATE.get_or_init(|| Mutex::new(None))
}

impl NativeQa {
    fn visible_text(&self, text: &str) -> bool {
        self.surface.texts.iter().any(|t| t.text == text)
    }
    pub(super) fn check_export(&mut self, label: &str) -> Result<(), String> {
        let dialog = self.editor.qa_export();
        let state = dialog.as_ref().map_or("fechado", |d| d.state);
        let ready = dialog.as_ref().is_some_and(|d| d.ready);
        let undo = self.editor.qa_undo_label().map(str::to_owned);
        let summary = format!(
            "diálogo {state}, Exportar {}, desfazer {undo:?}, falha {:?}",
            if ready { "ligado" } else { "desligado" },
            dialog.as_ref().and_then(|d| d.failure.clone())
        );
        let ensure = |condition: bool, message: &str| {
            if condition {
                Ok(())
            } else {
                Err(format!("{label}: {message} ({summary})"))
            }
        };
        let project = &self.editor.state.project;
        let exe = format!("{TITLE}{}", std::env::consts::EXE_SUFFIX);
        let exported = |count: usize| {
            self.editor
                .console
                .messages
                .iter()
                .filter(|m| **m == format!("Jogo exportado: {}", game_folder().display()))
                .count()
                == count
        };
        match label {
            "export_open" => {
                ensure(state == "configure", "Exportar jogo… não abriu o diálogo")?;
                for text in [
                    "Exportar jogo",
                    "JOGO",
                    "DESTINO",
                    "CONFERÊNCIA",
                    "Nome do jogo",
                    "Cena inicial",
                    "Tela cheia",
                    "No jogo, Alt+Enter alterna a tela cheia.",
                    "Escolha a pasta onde o jogo será criado.",
                    &format!("{START} · 2D"),
                ] {
                    ensure(self.visible_text(text), &format!("{text} não aparece"))?;
                }
                let executable = format!(
                    "Executável: {}{} · título da janela",
                    export::file_name(&project.name),
                    std::env::consts::EXE_SUFFIX
                );
                ensure(
                    self.visible_text(&executable),
                    "a dica não dá o executável pelo nome do projeto",
                )?;
                ensure(
                    self.visible_text(&format!(
                        "Player da OXY Engine {} encontrado ao lado do editor.",
                        export::engine_version()
                    )),
                    "o player não está ao lado do teste: compile-o antes, com \
                     ./scripts/cargo.ps1 build --release --locked -p oxy_player (o \
                     test-native.ps1 faz isso)",
                )?;
                ensure(
                    self.surface
                        .texts
                        .iter()
                        .any(|t| t.text.starts_with("Projeto válido: 3 cenas, ")),
                    "a Conferência não diz que o projeto é válido",
                )?;
                ensure(!ready, "Exportar ligado sem destino")?;
                self.base = Some(self.editor.state.clone());
            }
            "export_invalid" => {
                ensure(
                    self.visible_text("Informe o caminho completo da pasta (absoluto), não Jogos."),
                    "um destino relativo não mostra a recusa da exportação",
                )?;
                ensure(!ready, "Exportar ligado com destino inválido")?;
            }
            "export_ready" => {
                ensure(
                    self.visible_text(&format!("Será criada a pasta {}.", game_folder().display())),
                    "o destino não diz que pasta será criada",
                )?;
                ensure(
                    self.visible_text(&format!("Executável: {exe} · título da janela")),
                    "a dica não acompanha o nome do jogo",
                )?;
                ensure(self.visible_text("1600"), "a largura não mudou para 1600")?;
                ensure(ready, "Exportar desligado com tudo certo")?;
                ensure(
                    self.base.as_ref() == Some(&self.editor.state),
                    "o diálogo mudou o projeto antes de Exportar",
                )?;
                *gate().lock().unwrap() = Some(self.editor.qa_hold_next_export());
            }
            "export_running" => {
                ensure(state == "running", "Exportar não começou a exportação")?;
                ensure(
                    self.visible_text("Copiando arquivos…")
                        && self.visible_text(
                            "A exportação roda em segundo plano; o editor continua respondendo.",
                        ),
                    "o progresso não aparece",
                )?;
                ensure(
                    undo.as_deref() == Some("Configurar exportação"),
                    "as configurações não viraram um passo Configurar exportação",
                )?;
                let game = &project.game;
                ensure(
                    game.title == TITLE && game.window == [1600, 720] && game.fullscreen,
                    &format!("o projeto não recebeu as configurações: {game:?}"),
                )?;
                ensure(
                    self.editor.qa_export_folder() == Some(destination()),
                    "a pasta de destino não ficou nas preferências",
                )?;
                ensure(
                    !game_folder().exists(),
                    "o jogo apareceu antes da conferência",
                )?;
            }
            "export_go" => {
                let sender = gate()
                    .lock()
                    .unwrap()
                    .take()
                    .ok_or("Exportação sem trava")?;
                let _ = sender.send(());
            }
            "export_wait" => {
                if state == "running" {
                    self.actions.insert(1, Action::Wait(10));
                    self.actions.insert(2, Action::Check("export_wait"));
                }
            }
            "export_done" | "export_replaced" => {
                let replacing = label == "export_replaced";
                let report = dialog
                    .and_then(|d| d.report)
                    .ok_or_else(|| format!("{label}: a exportação não terminou ({summary})"))?;
                let folder = game_folder();
                ensure(report.folder == folder, "o jogo foi para outra pasta")?;
                ensure(
                    report.executable == folder.join(&exe) && report.executable.is_file(),
                    "o executável não tem o nome do jogo",
                )?;
                ensure(report.replaced == replacing, "replaced errado")?;
                let verify = &report.verify;
                ensure(
                    verify.ok
                        && verify.engine == export::engine_version()
                        && verify.game == TITLE
                        && verify.start_scene.as_deref() == Some(START)
                        && verify.seconds == export::VERIFY_SECONDS,
                    &format!("a conferência do executável não passou: {verify:?}"),
                )?;
                let marker: export::ExportMarker = serde_json::from_slice(
                    &std::fs::read(folder.join("data").join(export::MARKER_FILE))
                        .map_err(|e| format!("Marcador ausente: {e}"))?,
                )
                .map_err(|e| e.to_string())?;
                ensure(
                    marker.project_id == project.id && marker.exe == exe,
                    "o marcador não é deste projeto",
                )?;
                for file in ["LEIA-ME.txt", "licenses/OXY-Engine-MIT.txt"] {
                    ensure(folder.join(file).is_file(), &format!("{file} ausente"))?;
                }
                let game = persistence::load_project(&folder.join("data"))
                    .map_err(|e| format!("O jogo exportado não abre: {e}"))?
                    .game;
                ensure(
                    game.title == TITLE && game.window == [1600, 720] && game.fullscreen,
                    &format!("o jogo exportado não tem as configurações: {game:?}"),
                )?;
                let names: Vec<String> = std::fs::read_dir(destination())
                    .map_err(|e| e.to_string())?
                    .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                    .collect();
                ensure(
                    names == [TITLE],
                    &format!("sobrou algo no destino: {names:?}"),
                )?;
                for text in [
                    "Jogo exportado",
                    &format!("{}{}", folder.display(), std::path::MAIN_SEPARATOR),
                    "Para jogar em outro computador, copie a pasta inteira. O progresso salvo \
                     fica em %APPDATA%\\OXY Engine\\Jogos.",
                    "Abrir pasta",
                    "Jogar agora",
                    "Fechar",
                ] {
                    ensure(self.visible_text(text), &format!("{text} não aparece"))?;
                }
                let verified = format!("{} arquivos · ", report.files);
                ensure(
                    self.surface.texts.iter().any(|t| {
                        t.text.starts_with(&verified)
                            && t.text.ends_with(&format!(
                                "Conferido: o jogo exportado abre a cena {START} e roda 2 \
                                 segundos sem erros."
                            ))
                    }),
                    "o resultado não diz os arquivos e a conferência",
                )?;
                ensure(
                    exported(if replacing { 2 } else { 1 }),
                    "o Console não diz onde o jogo foi exportado",
                )?;
            }
            "export_replace" => {
                ensure(state == "configure", "o diálogo não abriu de novo")?;
                ensure(
                    self.visible_text(&format!(
                        "Substitui a exportação anterior deste jogo em {}. A anterior só é \
                         apagada depois que a nova for conferida.",
                        game_folder().display()
                    )),
                    "o destino não diz que substitui a exportação anterior",
                )?;
                ensure(
                    self.visible_text(&destination().display().to_string()),
                    "o diálogo não lembrou a pasta de destino",
                )?;
                ensure(self.visible_text(TITLE), "o nome do jogo não ficou")?;
                ensure(
                    self.visible_text(
                        "Alterações não salvas entram na exportação; o projeto continua não \
                         salvo.",
                    ),
                    "a Conferência não avisa das alterações não salvas",
                )?;
                ensure(ready, "Exportar desligado para substituir")?;
            }
            "export_hold" => {
                #[cfg(windows)]
                {
                    use std::os::windows::fs::OpenOptionsExt;
                    // Others may read it, as a running game lets them, but not move it.
                    let file = std::fs::OpenOptions::new()
                        .read(true)
                        .share_mode(1)
                        .open(game_folder().join("data").join(persistence::PROJECT_FILE))
                        .map_err(|e| e.to_string())?;
                    *held().lock().unwrap() = Some(file);
                }
            }
            "export_failed" => {
                ensure(
                    state == "failed",
                    "a exportação com o jogo aberto não falhou",
                )?;
                for text in [
                    "Não foi possível substituir a exportação anterior",
                    &format!(
                        "Feche o jogo exportado ({exe}) e tente de novo. Nada foi alterado no \
                         destino: a exportação anterior continua intacta."
                    ),
                    "Voltar",
                    "Tentar de novo",
                ] {
                    ensure(self.visible_text(text), &format!("{text} não aparece"))?;
                }
                let names: Vec<String> = std::fs::read_dir(destination())
                    .map_err(|e| e.to_string())?
                    .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                    .collect();
                ensure(
                    names == [TITLE] && game_folder().join(&exe).is_file(),
                    &format!("a exportação anterior não ficou intacta: {names:?}"),
                )?;
                ensure(exported(1), "o Console diz que exportou")?;
            }
            "export_release" => {
                held().lock().unwrap().take();
            }
            "export_closed" => {
                ensure(dialog.is_none(), "Fechar não fechou o diálogo")?;
            }
            "export_agent" => {
                let dir = self.output.join("agents");
                let _ = std::fs::remove_dir_all(&dir);
                // SAFETY: the native test's own process; no other thread reads it yet.
                unsafe { std::env::set_var("OXY_AGENT_DIR", &dir) };
                self.editor.qa_allow_agents(true);
                self.base = Some(self.editor.state.clone());
                let destination = destination().display().to_string();
                std::thread::spawn(move || {
                    client(
                        dir,
                        vec![
                            (
                                "export",
                                json!({"destination": destination, "replace": false}),
                            ),
                            ("export", json!({"destination": "Jogos"})),
                            (
                                "export",
                                json!({"destination": destination, "replace": true}),
                            ),
                        ],
                    )
                });
            }
            "export_agent_wait" => {
                if !client_finished() {
                    self.actions.insert(1, Action::Wait(10));
                    self.actions.insert(2, Action::Check("export_agent_wait"));
                }
            }
            "export_agent_done" => {
                let answers = take_results()?;
                let exists = answer(&answers, "export", 0)?;
                ensure(
                    exists
                        .as_ref()
                        .is_err_and(|e| e.contains("Já existe uma exportação deste jogo")),
                    &format!("sem replace, o agente não foi recusado: {exists:?}"),
                )?;
                let relative = answer(&answers, "export", 1)?;
                ensure(
                    relative
                        .as_ref()
                        .is_err_and(|e| e.contains("caminho completo da pasta")),
                    &format!("um destino relativo do agente não foi recusado: {relative:?}"),
                )?;
                let replaced = answer(&answers, "export", 2)??;
                ensure(
                    replaced["replaced"] == true
                        && replaced["verify"]["ok"] == true
                        && replaced["folder"] == json!(game_folder().display().to_string()),
                    &format!("o agente não substituiu a exportação: {replaced}"),
                )?;
                ensure(
                    self.editor.console.messages.iter().any(|m| {
                        *m == format!("Agente · Jogo exportado: {}", game_folder().display())
                    }),
                    "o Console não diz que o agente exportou",
                )?;
                ensure(
                    self.base.as_ref() == Some(&self.editor.state)
                        && undo.as_deref() == Some("Configurar exportação"),
                    "a exportação do agente mudou o projeto ou o histórico",
                )?;
            }
            _ => return Err(format!("Verificação de exportação desconhecida: {label}")),
        }
        Ok(())
    }
}

#[test]
#[ignore = "Opens the real native WGPU editor, exports the example and runs the exported game's check"]
#[cfg(target_os = "windows")]
fn native_export_workflow() {
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = oxy_core::qa::output_dir("v0.6/export");
    std::fs::create_dir_all(&output).unwrap();
    // The export reads the project's files: the example is copied, never touched.
    let project = std::env::temp_dir().join(format!("oxy-native-export-projeto-{}", new_id()));
    copy_directory(&workspace.join("examples/validacao"), &project).unwrap();
    let typed: &'static str = Box::leak(destination().display().to_string().into_boxed_str());
    let report = Arc::new(Mutex::new(Report::default()));
    let shared = report.clone();
    let artifacts = output.clone();
    let path = project.join("project.oxy.json");
    eframe::run_native(
        "OXY Engine — QA exportar jogo",
        eframe::NativeOptions {
            persist_window: false,
            renderer: eframe::Renderer::Wgpu,
            viewport: egui::ViewportBuilder::default()
                .with_title("OXY Engine — QA exportar jogo")
                .with_inner_size([1440., 900.])
                .with_active(false),
            event_loop_builder: Some(Box::new(|b| {
                b.with_any_thread(true);
            })),
            ..Default::default()
        },
        Box::new(move |cc| {
            let mut qa = NativeQa::new(cc, path, artifacts, shared);
            qa.actions = VecDeque::from([
                Action::Click("Projeto"),
                Action::Click("Exportar jogo…"),
                Action::Wait(6),
                Action::Check("export_open"),
                Action::Screenshot("export-dialog.png"),
                Action::Click("Pasta de destino"),
                Action::Key(Key::A, true),
                Action::Text("Jogos"),
                Action::Wait(4),
                Action::Check("export_invalid"),
                Action::Screenshot("export-invalid-destination.png"),
                Action::Click("Pasta de destino"),
                Action::Key(Key::A, true),
                Action::Text(typed),
                Action::EditValue("Nome do jogo", TITLE),
                Action::EditValue("Janela", "1600"),
                Action::Click("Começar em tela cheia"),
                Action::Wait(4),
                Action::Check("export_ready"),
                Action::Screenshot("export-ready.png"),
                Action::Click("Exportar"),
                Action::Wait(4),
                Action::Check("export_running"),
                Action::Screenshot("export-running.png"),
                Action::Check("export_go"),
                Action::Check("export_wait"),
                Action::Wait(4),
                Action::Check("export_done"),
                Action::Screenshot("export-done.png"),
                Action::Click("Fechar"),
                Action::Click("Projeto"),
                Action::Click("Exportar jogo…"),
                Action::Wait(6),
                Action::Check("export_replace"),
                Action::Screenshot("export-replace.png"),
                // The exported game is open: its folder cannot move, and the export says so.
                Action::Check("export_hold"),
                Action::Click("Exportar"),
                Action::Check("export_wait"),
                Action::Wait(4),
                Action::Check("export_failed"),
                Action::Screenshot("export-failed.png"),
                Action::Check("export_release"),
                Action::Click("Tentar de novo"),
                Action::Check("export_wait"),
                Action::Wait(4),
                Action::Check("export_replaced"),
                Action::Click("Fechar"),
                Action::Wait(4),
                Action::Check("export_closed"),
                // An agent exports the same game through its channel.
                Action::Check("export_agent"),
                Action::Wait(10),
                Action::Check("export_agent_wait"),
                Action::Check("export_agent_done"),
            ]);
            Ok(Box::new(qa))
        }),
    )
    .unwrap();
    held().lock().unwrap().take();
    let _ = std::fs::remove_dir_all(destination().parent().unwrap());
    let _ = std::fs::remove_dir_all(&project);
    let report = report.lock().unwrap();
    let text = format!(
        "Concluído: {}\nErro: {:?}\n{}\nCapturas: {:?}",
        report.done,
        report.error,
        report.steps.join("\n"),
        report.screenshots
    );
    std::fs::write(output.join("native-export.txt"), &text).unwrap();
    println!("{text}");
    assert!(report.done && report.error.is_none(), "{text}");
    assert_eq!(report.screenshots.len(), 7, "{text}");
}
