use super::*;
// The player's pause QA plays the same game.
#[path = "../../../oxy_core/examples/support/pause_menu.rs"]
mod pause_menu;

/// The line the Console gets when Sair do jogo ends the test.
const QUIT_LINE: &str = "O jogo pediu para sair; o teste foi encerrado.";

impl NativeQa {
    /// The status chip of the test in the top bar: Jogando, Pausado or Pausa do jogo.
    pub(super) fn test_chip(&self) -> Option<&str> {
        self.surface
            .texts
            .iter()
            .filter(|t| t.rect.max.y <= crate::theme::TOPBAR_HEIGHT)
            .map(|t| t.text.as_str())
            .find(|text| ["Jogando", "Pausado", "Pausa do jogo"].contains(text))
    }
    pub(super) fn check_pause(&mut self, label: &str) -> Result<(), String> {
        let visible = |text: &str| self.surface.texts.iter().any(|t| t.text == text);
        let in_game = |text: &str| {
            self.surface.texts.iter().any(|t| {
                t.text == text
                    && self
                        .surface
                        .native_viewport
                        .is_some_and(|view| view.contains(t.rect.center()))
            })
        };
        let menu = in_game("Continuar") && in_game("Sair do jogo");
        let card = visible("O jogo pausou a si mesmo");
        let chip = self.test_chip();
        let captured = self.cursor_captured;
        let capture = self.editor.play.capture;
        let runtime = self.editor.play.runtime.as_ref();
        let (paused, game_paused) =
            runtime.map_or((false, false), |rt| (rt.paused, rt.game_paused()));
        let fired = runtime.map_or(0, |rt| {
            rt.logs.iter().filter(|l| l.ends_with("] disparo")).count()
        });
        let state = format!(
            "teste pausado {paused}, jogo pausado {game_paused}, captura {capture}, mouse preso \
             {captured}, menu {menu}, chip {chip:?}, Disparar {fired}"
        );
        let ensure = |condition: bool, message: &str| {
            if condition {
                Ok(())
            } else {
                Err(format!("{label}: {message} ({state})"))
            }
        };
        match label {
            "pause_playing" => ensure(
                self.editor.showing_game()
                    && runtime.is_some_and(|rt| rt.has_pause_menu())
                    && capture
                    && !paused
                    && !game_paused
                    && captured
                    && !menu
                    && chip == Some("Jogando"),
                "Jogar deveria jogar com o mouse preso, sem menu",
            ),
            "pause_game_menu" => ensure(
                game_paused
                    && !paused
                    && capture
                    && !captured
                    && menu
                    && card
                    && chip == Some("Pausa do jogo"),
                "Esc deveria abrir o menu do jogo, com o teste correndo e o mouse livre",
            ),
            "pause_test_over_menu" => ensure(
                paused
                    && game_paused
                    && !capture
                    && menu
                    && !card
                    && chip == Some("Pausado")
                    && visible("Retomar")
                    && fired == 0,
                "Pausar deveria pausar o teste por cima do menu do jogo",
            ),
            "pause_resumed" => {
                ensure(
                    !game_paused
                        && !paused
                        && capture
                        && captured
                        && !menu
                        && !card
                        && chip == Some("Jogando")
                        && fired == 0,
                    "Continuar deveria retomar e prender o mouse, sem Disparar",
                )?;
                // With the cursor held again, a click in the game is the game's.
                let view = self
                    .surface
                    .native_viewport
                    .ok_or("Imagem do jogo ausente")?;
                self.click(view.center());
                Ok(())
            }
            "pause_shot_fired" => ensure(
                fired == 1,
                "Um clique no jogo com o mouse preso deveria Disparar uma vez",
            ),
            "pause_quit_stopped" => {
                ensure(
                    runtime.is_none() && !capture,
                    "Sair do jogo deveria encerrar o teste",
                )?;
                ensure(
                    self.editor.console.messages.iter().any(|m| m == QUIT_LINE),
                    "O Console não diz que o jogo pediu para sair",
                )?;
                ensure(
                    self.base.as_ref() == Some(&self.editor.state),
                    "Encerrar o teste deve preservar o documento",
                )?;
                self.editor.show_dock(crate::app::DockTab::Console);
                Ok(())
            }
            _ => Err(format!("Verificação desconhecida: {label}")),
        }
    }
}

#[test]
#[ignore = "Native WGPU and isolated RawInput; does not certify physical mouse capture"]
fn native_game_pause_workflow() {
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let output = oxy_core::qa::output_dir("v0.6/editor-pause");
    let folder = std::env::temp_dir().join(format!("oxy-editor-pause-{}", new_id()));
    std::fs::create_dir_all(&output).unwrap();
    let path = folder.join("project.oxy.json");
    oxy_core::persistence::save_project(&path, &pause_menu::fixture()).unwrap();
    let report = Arc::new(Mutex::new(Report::default()));
    let shared = report.clone();
    let artifacts = output.clone();
    eframe::run_native(
        "OXY Engine — pausa do jogo",
        eframe::NativeOptions {
            persist_window: false,
            renderer: eframe::Renderer::Wgpu,
            viewport: egui::ViewportBuilder::default()
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
                Action::Check("play_baseline"),
                Action::Click("Jogar"),
                Action::Wait(12),
                Action::Check("pause_playing"),
                Action::Key(Key::Escape, false),
                Action::Wait(4),
                Action::Check("pause_game_menu"),
                Action::Screenshot("editor-game-menu.png"),
                Action::Click("Pausar"),
                Action::Check("pause_test_over_menu"),
                Action::Screenshot("editor-test-paused-over-menu.png"),
                Action::Click("Retomar"),
                Action::Wait(4),
                Action::Check("pause_game_menu"),
                Action::Click("Continuar"),
                Action::Wait(10),
                Action::Check("pause_resumed"),
                Action::Wait(6),
                Action::Check("pause_shot_fired"),
                Action::Key(Key::Escape, false),
                Action::Wait(4),
                Action::Check("pause_game_menu"),
                Action::Click("Sair do jogo"),
                Action::Wait(6),
                Action::Check("pause_quit_stopped"),
                Action::Wait(4),
                Action::Screenshot("editor-quit-console.png"),
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
    std::fs::write(output.join("native-pause.txt"), &text).unwrap();
    println!("{text}");
    assert!(report.done && report.error.is_none(), "{text}");
    assert_eq!(report.screenshots.len(), 3, "{text}");
}
