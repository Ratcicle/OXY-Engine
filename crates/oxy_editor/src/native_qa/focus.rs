//! The test and the window's focus (design §6.2): losing it pauses the person's test, and the
//! Pausado chip says why and where to change it; with Preferências do editor › Teste ›
//! Continuar o teste quando a OXY perde o foco, the test goes on, lets go of the key held,
//! takes no input and reloads a script saved meanwhile, until the focus comes back; the click
//! that brings it back is the window's, not the game's.
use super::*;
use oxy_core::document::Value;

const COUNTER: &str = "scripts/contador.luau";
/// Counts the presses and releases of the input actions, the clicks of its button, and the
/// steps.
const COUNTING: &str = "local Contador = {}\nfunction Contador.on_input(self, action, phase)\n    \
                        if phase == \"pressed\" then\n        self.attr.Apertou += 1\n    else\n        \
                        self.attr.Soltou += 1\n    end\nend\nfunction Contador.on_click(self)\n    \
                        self.attr.Cliques += 1\nend\nfunction Contador.on_step(self)\n    \
                        self.attr.Passos += 1\nend\nreturn Contador\n";
const SWITCH: &str = "Continuar o teste quando a OXY perde o foco";
const ADVICE: &str = "Pausado porque a OXY perdeu o foco. Para seguir jogando enquanto usa outra \
                      janela, ligue em Preferências do editor › Teste.";

impl NativeQa {
    /// An attribute of Contador in the running test.
    fn counted(&self, name: &str) -> f64 {
        self.editor
            .play
            .runtime
            .as_ref()
            .and_then(|rt| rt.scene().entities.iter().find(|e| e.name == "Contador"))
            .and_then(|e| match e.attributes.get(name) {
                Some(Value::Number(n)) => Some(*n),
                _ => None,
            })
            .unwrap_or(-1.)
    }
    /// The middle of Contador's button in the game, Somar.
    fn game_button(&self) -> Result<Pos2, String> {
        self.surface
            .texts
            .iter()
            .find(|t| t.text == "Somar")
            .map(|t| t.rect.center())
            .ok_or_else(|| "Botão Somar do jogo ausente".to_owned())
    }
    fn press_d(&mut self, pressed: bool) {
        self.events.push_back(vec![Event::Key {
            key: Key::D,
            physical_key: Some(Key::D),
            pressed,
            repeat: false,
            modifiers: Modifiers::NONE,
        }]);
    }
    pub(super) fn check_focus(&mut self, label: &str) -> Result<(), String> {
        let visible = |text: &str| self.surface.texts.iter().any(|t| t.text == text);
        let play = &self.editor.play;
        let runtime = play.runtime.as_ref();
        let running = runtime.is_some_and(|rt| !rt.paused);
        let held = runtime.map(|rt| rt.step_input().held.clone());
        let state = format!(
            "teste {:?}, captura {}, mouse preso {}, fora de foco {}, pausado pelo foco {}, \
             clique da volta {}, chip {:?}, apertou {}, soltou {}, cliques {}, passos {}, \
             segurando {held:?}",
            runtime.map(|rt| (rt.time, rt.paused)),
            play.capture,
            self.cursor_captured,
            play.unfocused,
            play.focus_paused,
            play.refocus_click,
            self.test_chip(),
            self.counted("Apertou"),
            self.counted("Soltou"),
            self.counted("Cliques"),
            self.counted("Passos"),
        );
        let ensure = |condition: bool, message: &str| {
            if condition {
                Ok(())
            } else {
                Err(format!("{label}: {message} ({state})"))
            }
        };
        match label {
            "fc7_playing" => ensure(
                running
                    && play.capture
                    && !play.unfocused
                    && !play.focus_paused
                    && self.test_chip() == Some("Jogando")
                    && self.counted("Passos") > 0.,
                "Jogar deveria jogar com o teclado e o mouse da pessoa",
            ),
            "fc7_blur" | "fc7_blur_running" => {
                self.focus_time = runtime.map(|rt| rt.time);
                self.unfocused = true;
                Ok(())
            }
            "fc7_focus" => {
                self.unfocused = false;
                Ok(())
            }
            "fc7_refocus_click" => {
                let button = self.game_button()?;
                self.unfocused = false;
                // As Windows tells a click on a window in the background: the focus, then the
                // press, in one frame.
                let press = |pressed| Event::PointerButton {
                    pos: button,
                    button: PointerButton::Primary,
                    pressed,
                    modifiers: Modifiers::NONE,
                };
                self.events
                    .push_back(vec![Event::PointerMoved(button), press(true)]);
                self.events.push_back(vec![press(false)]);
                self.events.push_back(vec![]);
                Ok(())
            }
            "fc7_paused" => ensure(
                !self.editor.qa_play_unfocused()
                    && runtime.is_some_and(|rt| rt.paused)
                    && play.focus_paused
                    && !play.unfocused
                    && !play.capture
                    && !self.cursor_captured
                    && self.test_chip() == Some("Pausado"),
                "Desligada, a perda de foco deveria pausar o teste da pessoa",
            ),
            "fc7_hover_chip" => {
                let chip = self
                    .surface
                    .texts
                    .iter()
                    .find(|t| t.text == "Pausado" && t.rect.max.y <= crate::theme::TOPBAR_HEIGHT)
                    .map(|t| t.rect.center())
                    .ok_or("Chip Pausado ausente")?;
                self.events.push_back(vec![Event::PointerMoved(chip)]);
                Ok(())
            }
            "fc7_paused_tooltip" => ensure(
                runtime.is_some_and(|rt| rt.paused)
                    && play.focus_paused
                    && self.test_chip() == Some("Pausado")
                    && visible(ADVICE),
                "Com o foco de volta, o Pausado deveria dizer o motivo e onde ligar a opção",
            ),
            "fc7_no_advice" => ensure(
                runtime.is_some_and(|rt| rt.paused)
                    && play.focus_paused
                    && self.editor.qa_play_unfocused()
                    && self.test_chip() == Some("Pausado")
                    && !visible(ADVICE),
                "Com a opção ligada, o Pausado não deveria mais sugerir ligá-la",
            ),
            "fc7_preferences" => {
                let top = |text: &str| {
                    self.surface
                        .texts
                        .iter()
                        .find(|t| t.text == text)
                        .map(|t| t.rect.top())
                };
                ensure(
                    self.surface.modal.is_some()
                        && visible("Fora de foco")
                        && visible(SWITCH)
                        && self.surface.texts.iter().any(|t| {
                            t.text
                                .starts_with("O jogo segue rodando enquanto você usa outra janela")
                        })
                        && !self.editor.qa_play_unfocused(),
                    "Preferências sem a opção de teste fora de foco, desligada",
                )?;
                // Between Navegação 3D and Agentes de IA.
                let order = [top("NAVEGAÇÃO 3D"), top("TESTE"), top("AGENTES DE IA")];
                ensure(
                    matches!(order, [Some(a), Some(b), Some(c)] if a < b && b < c),
                    &format!("A seção Teste deveria ficar entre as outras duas: {order:?}"),
                )
            }
            "fc7_switch" => {
                let rect = self
                    .editor
                    .qa_icon_rect(SWITCH)
                    .ok_or_else(|| format!("Chave ausente: {SWITCH}"))?;
                self.click(rect.center());
                Ok(())
            }
            "fc7_on" => ensure(
                self.surface.modal.is_none() && self.editor.qa_play_unfocused(),
                "Aplicar deveria ligar a opção de teste fora de foco",
            ),
            "fc7_press" => {
                self.press_d(true);
                Ok(())
            }
            "fc7_pressed" => ensure(
                running
                    && self.counted("Apertou") == 1.
                    && self.counted("Soltou") == 0.
                    && held.as_ref().is_some_and(|h| h.contains("Andar")),
                "D deveria apertar e segurar Andar",
            ),
            "fc7_running_unfocused" => ensure(
                running
                    && runtime.map(|rt| rt.time) > self.focus_time
                    && play.unfocused
                    && !play.focus_paused
                    && !self.cursor_captured
                    && self.test_chip() == Some("Jogando · fora de foco")
                    && self.counted("Apertou") == 1.
                    && self.counted("Soltou") == 1.
                    && held.as_ref().is_some_and(|h| h.is_empty()),
                "Ligada, o teste deveria seguir fora de foco, com a tecla solta e sem entrada",
            ),
            "fc7_rewrite" => {
                let root = self
                    .editor
                    .path
                    .as_deref()
                    .and_then(Path::parent)
                    .ok_or("O projeto não tem pasta")?;
                let text = COUNTING.replace(
                    "self.attr.Passos += 1\n",
                    "self.attr.Passos += 1\n    self.attr.Versao = 2\n",
                );
                std::fs::write(root.join(COUNTER), text).map_err(|e| e.to_string())
            }
            "fc7_reloaded" => {
                let reloaded = "Script recarregado: scripts/contador.luau (1 objeto)";
                ensure(
                    self.editor.console.messages.iter().any(|m| m == reloaded),
                    "O Console não disse que o script recarregou fora de foco",
                )?;
                ensure(
                    running
                        && play.unfocused
                        && self.counted("Versao") == 2.
                        && self.test_chip() == Some("Jogando · fora de foco"),
                    "O teste fora de foco deveria rodar o script recarregado",
                )
            }
            "fc7_focused_again" => {
                ensure(
                    running
                        && play.capture
                        && !play.unfocused
                        && !play.refocus_click
                        && self.test_chip() == Some("Jogando"),
                    "Com o foco de volta, o chip deveria voltar a Jogando",
                )?;
                ensure(
                    self.counted("Cliques") == 0.
                        && self.counted("Apertou") == 1.
                        && self.counted("Soltou") == 1.,
                    "O clique que trouxe o foco de volta não deveria chegar ao botão do jogo",
                )?;
                // The key let go away from OXY, then a new press: only that one counts; and a
                // click on the game's button now is the game's.
                let button = self.game_button()?;
                self.press_d(false);
                self.press_d(true);
                self.press_d(false);
                self.click(button);
                Ok(())
            }
            "fc7_input_back" => ensure(
                running
                    && self.counted("Apertou") == 2.
                    && self.counted("Soltou") == 2.
                    && self.counted("Cliques") == 1.,
                "Com o foco de volta, o teclado e o mouse deveriam chegar ao jogo, sem soltar D \
                 duas vezes",
            ),
            _ => Err(format!("Verificação de foco desconhecida: {label}")),
        }
    }
}

#[test]
#[ignore = "Real native WGPU test with the focus taken away in RawInput only"]
#[cfg(target_os = "windows")]
fn native_test_unfocused() {
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let output = oxy_core::qa::output_dir("v0.7/test-unfocused");
    std::fs::create_dir_all(&output).unwrap();
    let root = std::env::temp_dir().join(format!("oxy-test-unfocused-{}", new_id()));
    std::fs::create_dir_all(root.join("scripts")).unwrap();
    std::fs::write(root.join(COUNTER), COUNTING).unwrap();
    let path = root.join("project.oxy.json");
    let mut project = oxy_core::editing::blank_project("Foco", SceneKind::ThreeD).unwrap();
    project.input_bindings.insert("Andar".into(), "D".into());
    let mut counter = oxy_core::document::Entity::new("Contador", None);
    for (name, value) in [
        ("Apertou", 0.),
        ("Soltou", 0.),
        ("Cliques", 0.),
        ("Passos", 0.),
        ("Versao", 1.),
    ] {
        counter.attributes.insert(name.into(), Value::Number(value));
    }
    counter.ui = Some(oxy_core::document::UiElement {
        kind: oxy_core::document::UiKind::Button,
        text: "Somar".into(),
        ..Default::default()
    });
    counter.scripts = vec![oxy_core::script::ScriptRef::new(COUNTER)];
    project.scenes[0].entities.push(counter);
    oxy_core::persistence::save_project(&path, &project).unwrap();
    let report = Arc::new(Mutex::new(Report::default()));
    let shared = report.clone();
    let artifacts = output.clone();
    eframe::run_native(
        "OXY Engine — QA teste fora de foco",
        eframe::NativeOptions {
            persist_window: false,
            renderer: eframe::Renderer::Wgpu,
            viewport: egui::ViewportBuilder::default()
                .with_title("OXY Engine — QA teste fora de foco")
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
                Action::Wait(4),
                // Off, as it starts: losing the focus pauses the person's test.
                Action::Click("Jogar"),
                Action::Wait(20),
                Action::Check("fc7_playing"),
                Action::Check("fc7_blur"),
                Action::Wait(6),
                Action::Check("fc7_paused"),
                // Back in OXY, the Pausado chip says why and where to change it.
                Action::Check("fc7_focus"),
                Action::Wait(4),
                Action::Check("fc7_hover_chip"),
                Action::Wait(60),
                Action::Check("fc7_paused_tooltip"),
                Action::Screenshot("test-paused-unfocused.png"),
                // The person turns the option on in Preferências do editor › Teste, with the
                // test still paused: the chip stops advising it.
                Action::Click("Interface: 100%"),
                Action::Wait(4),
                Action::Check("fc7_preferences"),
                Action::Screenshot("preferences-test.png"),
                Action::Check("fc7_switch"),
                Action::Wait(4),
                Action::Click("Aplicar"),
                Action::Wait(4),
                Action::Check("fc7_on"),
                Action::Check("fc7_hover_chip"),
                Action::Wait(60),
                Action::Check("fc7_no_advice"),
                Action::Click("Parar"),
                Action::Wait(4),
                // On: the test goes on away from OXY, lets go of D and takes no input, and a
                // script an agent saves meanwhile reloads with the game in view.
                Action::Click("Jogar"),
                Action::Wait(20),
                Action::Check("fc7_playing"),
                Action::Check("fc7_press"),
                Action::Wait(6),
                Action::Check("fc7_pressed"),
                Action::Check("fc7_blur_running"),
                Action::Wait(20),
                Action::Check("fc7_running_unfocused"),
                Action::Screenshot("test-running-unfocused.png"),
                Action::Check("fc7_rewrite"),
                Action::Wait(120),
                Action::Check("fc7_reloaded"),
                // Back in OXY with a click on the game's button, which the game does not take;
                // then the keyboard and the mouse reach the game again.
                Action::Check("fc7_refocus_click"),
                Action::Wait(6),
                Action::Check("fc7_focused_again"),
                Action::Wait(6),
                Action::Check("fc7_input_back"),
                Action::Screenshot("test-focused-again.png"),
                Action::Click("Parar"),
                Action::Wait(4),
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
    std::fs::write(output.join("native-test-unfocused.txt"), &text).unwrap();
    let _ = std::fs::remove_dir_all(&root);
    assert!(report.done && report.error.is_none(), "{text}");
    assert_eq!(report.screenshots.len(), 4, "{text}");
}
