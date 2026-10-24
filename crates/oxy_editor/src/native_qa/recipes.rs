use super::*;
use oxy_core::document::Value;
impl NativeQa {
    pub(super) fn check_recipes(&mut self, label: &str) -> Result<(), String> {
        if label == "r6_pause_quit" {
            // Sair do jogo stops the test, as Parar does.
            return if self.editor.play.runtime.is_none() && !self.editor.play.capture {
                Ok(())
            } else {
                Err("Sair do jogo deveria encerrar o teste da receita".into())
            };
        }
        let runtime = self
            .editor
            .play
            .runtime
            .as_ref()
            .ok_or("Runtime não iniciou")?;
        let id = |n: u32| format!("02000000-0000-4000-8000-{n:012x}");
        let shown = |n| runtime.scene().entity(&id(n)).is_some_and(|e| e.visible);
        let x = |n| {
            runtime
                .scene()
                .entity(&id(n))
                .map_or(f32::NAN, |e| e.transform.position[0])
        };
        let unset = Value::Text("--:--.--".into());
        let value = |n, attribute: &str| {
            runtime
                .scene()
                .entity(&id(n))
                .and_then(|e| e.attributes.get(attribute))
        };
        let pass = match label {
            "r6_door" => runtime
                .scene()
                .entity(&id(211))
                .is_some_and(|e| !e.visible && !e.collider.as_ref().unwrap().enabled),
            "r6_card" => {
                value(310, "Energia") == Some(&Value::Number(1.))
                    && value(311, "Vida") == Some(&Value::Number(30.))
            }
            "r6_attack" => {
                value(414, "Vida") == Some(&Value::Number(85.))
                    && !runtime
                        .scene()
                        .entity(&id(413))
                        .unwrap()
                        .collider
                        .as_ref()
                        .unwrap()
                        .enabled
            }
            "r6_passage" => runtime.scene_id() == id(502),
            // Walked, then Esc: the game's pause opens the menu.
            "r6_pause_menu" => runtime.game_paused() && shown(610) && x(616) > -2.5,
            "r6_pause_resumed" => !runtime.game_paused() && !shown(610),
            // Reiniciar opens the scene again, as it began.
            "r6_pause_restarted" => !runtime.game_paused() && !shown(610) && x(616) < -2.9,
            "r6_record" | "r6_record_kept" => {
                let best = runtime
                    .variable(&id(702), "Fase 1")
                    .and_then(Value::number)
                    .unwrap_or(0.);
                let record = value(711, "Recorde");
                // Played again, the test's progress brings the record back from the start.
                best > 0.
                    && record.is_some_and(|r| r != &unset)
                    && (label == "r6_record_kept" || runtime.save_slot().exists)
            }
            _ => false,
        };
        if !pass {
            return Err(format!(
                "Receita não atingiu resultado: {label}; logs {:?}",
                runtime.logs.iter().map(|l| &l.text).collect::<Vec<_>>()
            ));
        }
        if self
            .editor
            .path
            .as_ref()
            .is_none_or(|p| !p.starts_with(std::env::temp_dir()))
        {
            return Err("Guia não abriu cópia separada".into());
        }
        Ok(())
    }
}
#[test]
#[ignore = "Native guide opens and executes six editable recipes using real game input"]
#[cfg(target_os = "windows")]
fn native_guide_recipes_workflow() {
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let output = oxy_core::qa::output_dir("v0.2.1/m6");
    std::fs::create_dir_all(&output).unwrap();
    let root = std::env::temp_dir().join(format!("oxy-guide-recipes-{}", new_id()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("project.oxy.json");
    let p = oxy_core::editing::blank_project("Guia completo", SceneKind::TwoD).unwrap();
    oxy_core::persistence::save_project(&path, &p).unwrap();
    let report = Arc::new(Mutex::new(Report::default()));
    let shared = report.clone();
    let artifacts = output.clone();
    eframe::run_native(
        "OXY Engine — QA receitas",
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
                Action::Click("Lógica"),
                Action::Click("Guia"),
                Action::Scroll("Guia", -3000.),
                Action::Click("Receitas em nós"),
                Action::Click("Porta com chave"),
                Action::Screenshot("guide-door.png"),
                Action::Click("Abrir cópia da receita: porta com chave"),
                Action::Click("Jogar"),
                Action::HoldSeconds(Key::D, 2.),
                Action::Check("r6_door"),
                Action::Screenshot("recipe-door-game.png"),
                Action::Click("Parar"),
                Action::Click("Lógica"),
                Action::Click("Guia"),
                Action::Scroll("Guia", -3000.),
                Action::Click("Receitas em nós"),
                Action::Click("Carta e energia"),
                Action::Click("Abrir cópia da receita: carta e energia"),
                Action::Click("Jogar"),
                Action::Click("Raio · custo 2 · Clique para causar 20 de dano"),
                Action::Wait(30),
                Action::Check("r6_card"),
                Action::Click("Raio · custo 2 · Clique para causar 20 de dano"),
                Action::Wait(30),
                Action::Check("r6_card"),
                Action::Screenshot("recipe-card-game.png"),
                Action::Click("Parar"),
                Action::Click("Lógica"),
                Action::Click("Guia"),
                Action::Scroll("Guia", -3000.),
                Action::Click("Receitas em nós"),
                Action::Click("Ataque por marcador"),
                Action::Click("Abrir cópia da receita: ataque por marcador"),
                Action::Click("Jogar"),
                Action::HoldSeconds(Key::J, 1.),
                Action::Check("r6_attack"),
                Action::Screenshot("recipe-attack-game.png"),
                Action::Click("Parar"),
                Action::Click("Lógica"),
                Action::Click("Guia"),
                Action::Scroll("Guia", -3000.),
                Action::Click("Receitas em nós"),
                Action::Click("Passagem entre cenas"),
                Action::Click("Abrir cópia da receita: passagem entre cenas"),
                Action::Click("Jogar"),
                Action::HoldSeconds(Key::D, 2.),
                Action::Check("r6_passage"),
                Action::Screenshot("recipe-passage-game.png"),
                Action::Click("Parar"),
                Action::Click("Lógica"),
                Action::Click("Guia"),
                Action::Scroll("Guia", -3000.),
                Action::Click("Receitas em nós"),
                Action::Click("Menu de pausa"),
                Action::Click("Abrir cópia da receita: menu de pausa"),
                Action::Click("Jogar"),
                Action::HoldSeconds(Key::D, 0.6),
                Action::Key(Key::Escape, false),
                Action::Wait(4),
                Action::Check("r6_pause_menu"),
                Action::Screenshot("recipe-pause-game.png"),
                Action::Click("Continuar"),
                Action::Wait(10),
                Action::Check("r6_pause_resumed"),
                Action::Key(Key::Escape, false),
                Action::Wait(4),
                Action::Click("Reiniciar"),
                Action::Wait(10),
                Action::Check("r6_pause_restarted"),
                Action::Key(Key::Escape, false),
                Action::Wait(4),
                Action::Click("Sair do jogo"),
                Action::Wait(6),
                Action::Check("r6_pause_quit"),
                Action::Click("Lógica"),
                Action::Click("Guia"),
                Action::Scroll("Guia", -3000.),
                Action::Click("Receitas em nós"),
                Action::Click("Recorde salvo"),
                Action::Click("Abrir cópia da receita: recorde salvo"),
                Action::Click("Jogar"),
                Action::HoldSeconds(Key::D, 2.),
                Action::Check("r6_record"),
                Action::Screenshot("recipe-record-game.png"),
                Action::Click("Parar"),
                Action::Click("Jogar"),
                Action::Wait(10),
                Action::Check("r6_record_kept"),
                Action::Click("Parar"),
            ]);
            Ok(Box::new(qa))
        }),
    )
    .unwrap();
    let r = report.lock().unwrap();
    let text = format!(
        "Concluído: {}\nErro: {:?}\n{}\nCapturas: {:?}",
        r.done,
        r.error,
        r.steps.join("\n"),
        r.screenshots
    );
    std::fs::write(output.join("native-guide-recipes.txt"), &text).unwrap();
    assert!(r.done && r.error.is_none(), "{text}");
}
