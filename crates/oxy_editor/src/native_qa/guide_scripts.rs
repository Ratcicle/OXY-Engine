use super::*;
use oxy_core::guide_scripts;

impl NativeQa {
    /// The checks of the Guia's flow: it opens at Primeiro script with Scripts, the recipes in
    /// script and the reference over the nodes (legacy), the reference's pages and search, and a
    /// recipe in script opened as a copy with its script.
    pub(super) fn check_guide_scripts(&mut self, label: &str) -> Result<(), String> {
        let visible = |text: &str| self.surface.texts.iter().any(|t| t.text == text);
        let starts = |text: &str| self.surface.texts.iter().any(|t| t.text.starts_with(text));
        let top = |text: &str| {
            self.surface
                .texts
                .iter()
                .find(|t| t.text == text)
                .map(|t| t.rect.top())
        };
        let topic = self.editor.qa_guide_topic();
        match label {
            "gs7_first" => {
                if topic != Some("") {
                    return Err(format!("O Guia abriu em {topic:?}, não em Primeiro script"));
                }
                // Its title, in the modal: the Lógica's Guia button stays painted behind it.
                if self.surface.modal.is_none()
                    || !self
                        .surface
                        .texts
                        .iter()
                        .any(|t| t.text == "Guia" && !t.behind_modal)
                {
                    return Err("O Guia não abriu como diálogo com o título Guia".into());
                }
                let total = guide_scripts::functions().to_string();
                for text in [
                    "Seu primeiro script",
                    "Primeiro script",
                    "return Girar",
                    "Buscar assunto, função ou nó",
                    total.as_str(),
                ] {
                    if !visible(text) {
                        return Err(format!("O Guia não mostra {text}"));
                    }
                }
                let order: Vec<Option<f32>> =
                    ["SCRIPTS", "RECEITAS EM SCRIPT", "REFERÊNCIA DA API"]
                        .into_iter()
                        .map(top)
                        .collect();
                if order.iter().any(Option::is_none) || !order.is_sorted() {
                    return Err(format!("A lista do Guia não está na ordem: {order:?}"));
                }
            }
            "gs7_legacy" => {
                for text in [
                    "NÓS (LEGADO)",
                    "Primeiro comportamento",
                    "Receitas em nós",
                    "Nós por categoria",
                ] {
                    if !visible(text) {
                        return Err(format!("O fim da lista do Guia não mostra {text}"));
                    }
                }
                if top("Receitas em nós") < top("NÓS (LEGADO)") {
                    return Err("Os nós (legado) não ficam embaixo do título amarelo".into());
                }
            }
            "gs7_timers" => {
                if topic != Some("api:Temporizadores") {
                    return Err(format!("Temporizadores abriu {topic:?}"));
                }
                for text in [
                    "REFERÊNCIA DA API",
                    "5 funções",
                    "Esperar sem bloquear",
                    "0.8: do objeto dono",
                    "self:after(2, M.fechar)",
                ] {
                    if !visible(text) {
                        return Err(format!("A página Temporizadores não mostra {text}"));
                    }
                }
                if !starts("obj:after<X...>(seconds: number") {
                    return Err("A página não mostra a assinatura de after".into());
                }
            }
            "gs7_search" => {
                for text in [
                    "RESULTADOS",
                    "obj.character:teleport",
                    "Teleportar com segurança",
                ] {
                    if !visible(text) {
                        return Err(format!("A busca por teleport não mostra {text}"));
                    }
                }
            }
            "gs7_teleport" => {
                if topic != Some("api:Personagem 3D#Character:teleport") {
                    return Err(format!("O resultado abriu {topic:?}"));
                }
                if !starts("obj.character:teleport(") {
                    return Err("A página não rolou até teleport".into());
                }
            }
            "gs7_recipe" => {
                if topic != Some("script-recipe:0") {
                    return Err(format!("A receita em script abriu {topic:?}"));
                }
                for text in [
                    "RECEITA EM SCRIPT",
                    "Abrir cópia da receita: porta com chave",
                    "SCRIPT · area_da_porta.luau",
                    "16 linhas",
                    "local Porta = {}",
                ] {
                    if !visible(text) {
                        return Err(format!("A receita em script não mostra {text}"));
                    }
                }
                if visible("Ver a mesma receita em nós") || visible("CONEXÕES E VALORES") {
                    return Err("A receita em script mostra a versão em nós".into());
                }
            }
            "gs7_copy" => {
                if topic.is_some() {
                    return Err("O Guia continuou aberto depois de abrir a cópia".into());
                }
                let root = self
                    .editor
                    .path
                    .as_deref()
                    .and_then(Path::parent)
                    .filter(|root| root.starts_with(std::env::temp_dir()))
                    .ok_or("A receita não abriu como cópia numa pasta temporária")?;
                if !root.join("area_da_porta.luau").is_file() {
                    return Err("A cópia não levou o script da receita".into());
                }
                if self.editor.tab != crate::app::Tab::Scene {
                    return Err("A cópia da receita em script não abriu na Cena".into());
                }
                let selected = self
                    .editor
                    .selected
                    .as_deref()
                    .and_then(|id| self.editor.scene().entity(id))
                    .ok_or("A cópia não selecionou um objeto")?;
                let paths: Vec<&str> = selected.scripts.iter().map(|s| s.path.as_str()).collect();
                if paths != ["area_da_porta.luau"] {
                    return Err(format!("A cópia selecionou {} ({paths:?})", selected.name));
                }
            }
            "gs7_card" => {
                if !visible("area_da_porta.luau") {
                    return Err("O cartão Scripts não mostra area_da_porta.luau".into());
                }
            }
            _ => return Err(format!("Verificação do Guia desconhecida: {label}")),
        }
        Ok(())
    }
}

#[test]
#[ignore = "Real native WGPU Guia: Scripts, API reference, search and a recipe in script"]
#[cfg(target_os = "windows")]
fn native_guide_scripts() {
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let output = oxy_core::qa::output_dir("v0.7/guide-scripts");
    std::fs::create_dir_all(&output).unwrap();
    let root = std::env::temp_dir().join(format!("oxy-guide-scripts-{}", new_id()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("project.oxy.json");
    // The top bar shows the project's name above every button: named as a label the flow
    // clicks (or ending in one), it would take the click.
    let project = oxy_core::editing::blank_project("Guia de scripts", SceneKind::TwoD).unwrap();
    oxy_core::persistence::save_project(&path, &project).unwrap();
    let report = Arc::new(Mutex::new(Report::default()));
    let shared = report.clone();
    let artifacts = output.clone();
    eframe::run_native(
        "OXY Engine — QA Guia de scripts",
        eframe::NativeOptions {
            persist_window: false,
            renderer: eframe::Renderer::Wgpu,
            viewport: egui::ViewportBuilder::default()
                .with_title("OXY Engine — QA Guia de scripts")
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
                Action::Wait(4),
                Action::Check("gs7_first"),
                Action::Screenshot("guide-first-script.png"),
                Action::Scroll("Guia", -3000.),
                Action::Wait(20),
                Action::Check("gs7_legacy"),
                Action::Screenshot("guide-legacy.png"),
                Action::Scroll("Guia", 3000.),
                Action::Wait(20),
                Action::Click("Temporizadores"),
                Action::Wait(4),
                Action::Check("gs7_timers"),
                Action::Screenshot("guide-timers.png"),
                Action::Click("Buscar assunto, função ou nó"),
                Action::Text("teleport"),
                Action::Wait(4),
                Action::Check("gs7_search"),
                Action::Click("obj.character:teleport"),
                Action::Wait(60),
                Action::Check("gs7_teleport"),
                Action::Screenshot("guide-teleport.png"),
                Action::Click("teleport"),
                Action::Key(Key::A, true),
                Action::Key(Key::Backspace, false),
                Action::Wait(4),
                Action::Click("Porta com chave"),
                Action::Wait(4),
                Action::Check("gs7_recipe"),
                Action::Screenshot("guide-script-recipe.png"),
                Action::Click("Abrir cópia da receita: porta com chave"),
                Action::Wait(30),
                Action::Check("gs7_copy"),
                // The Scripts card comes after the components: the panel scrolls to it.
                Action::Scroll("Transformação", -3000.),
                Action::Wait(20),
                Action::Check("gs7_card"),
                Action::Screenshot("guide-recipe-copy.png"),
                Action::Click("Jogar"),
                Action::HoldSeconds(Key::D, 2.),
                Action::Check("r6_door"),
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
    std::fs::write(output.join("native-guide-scripts.txt"), &text).unwrap();
    let _ = std::fs::remove_dir_all(&root);
    assert!(report.done && report.error.is_none(), "{text}");
    assert_eq!(report.screenshots.len(), 6, "{text}");
}
