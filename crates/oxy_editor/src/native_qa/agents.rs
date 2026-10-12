//! Live agent channel in the real editor: a client finds the editor through its discovery
//! file, edits the open project (objects, logic and an input action, with the warning the
//! batch added, which an empty batch then does not repeat), reads it back (with read options
//! answering as they do on files), captures the viewport (with the collider view too, also
//! while the game runs), is refused edits while the game is running, and undoes its own step.
use super::*;
use oxy_core::{document::Value, query};
use serde_json::{Value as Json, json};
use std::{
    io::{BufRead, BufReader, Write},
    net::TcpStream,
    sync::OnceLock,
    time::{Duration, Instant},
};

type Results = Vec<(String, Result<Json, String>)>;
fn results() -> &'static Mutex<Option<Results>> {
    static RESULTS: OnceLock<Mutex<Option<Results>>> = OnceLock::new();
    RESULTS.get_or_init(|| Mutex::new(None))
}

/// Runs `requests` against the editor announced in `dir`, storing each answer.
fn client(dir: PathBuf, requests: Vec<(&'static str, Json)>) {
    let run = || -> Result<Results, String> {
        let deadline = Instant::now() + Duration::from_secs(10);
        let info: Json = loop {
            let file = std::fs::read_dir(&dir)
                .ok()
                .and_then(|entries| entries.filter_map(Result::ok).next())
                .map(|entry| entry.path());
            if let Some(file) = file
                && let Ok(bytes) = std::fs::read(file)
                && let Ok(info) = serde_json::from_slice(&bytes)
            {
                break info;
            }
            if Instant::now() > deadline {
                return Err("O editor não anunciou o canal".into());
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        let port = info["port"].as_u64().ok_or("Porta ausente")? as u16;
        let stream = TcpStream::connect(("127.0.0.1", port)).map_err(|e| e.to_string())?;
        stream
            .set_read_timeout(Some(Duration::from_secs(30)))
            .map_err(|e| e.to_string())?;
        let mut writer = stream.try_clone().map_err(|e| e.to_string())?;
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        writeln!(writer, "{}", json!({"token": "errado"})).map_err(|e| e.to_string())?;
        reader.read_line(&mut line).map_err(|e| e.to_string())?;
        let mut answers = vec![(
            "wrong_token".to_owned(),
            serde_json::from_str::<Json>(&line).map_err(|e| e.to_string()),
        )];
        let stream = TcpStream::connect(("127.0.0.1", port)).map_err(|e| e.to_string())?;
        stream
            .set_read_timeout(Some(Duration::from_secs(30)))
            .map_err(|e| e.to_string())?;
        let mut writer = stream.try_clone().map_err(|e| e.to_string())?;
        let mut reader = BufReader::new(stream);
        writeln!(writer, "{}", json!({"token": info["token"]})).map_err(|e| e.to_string())?;
        line.clear();
        reader.read_line(&mut line).map_err(|e| e.to_string())?;
        for (index, (method, params)) in requests.into_iter().enumerate() {
            writeln!(
                writer,
                "{}",
                json!({"id": index, "method": method, "params": params})
            )
            .map_err(|e| e.to_string())?;
            line.clear();
            reader.read_line(&mut line).map_err(|e| e.to_string())?;
            let response: Json = serde_json::from_str(&line).map_err(|e| e.to_string())?;
            let answer = match response.get("error").and_then(Json::as_str) {
                Some(error) => Err(error.to_owned()),
                None => Ok(response["result"].clone()),
            };
            answers.push((method.to_owned(), answer));
        }
        Ok(answers)
    };
    let answers = run().unwrap_or_else(|error| vec![("client".into(), Err(error))]);
    *results().lock().unwrap() = Some(answers);
}

/// Reads with options (paging, world boxes, parts of objects, brief catalog). Answered live,
/// they must equal the same reads on the editor's document, as the MCP server makes on files.
fn live_reads(editor: &Editor) -> Vec<(&'static str, Json)> {
    let scene = editor.scene();
    let first = scene
        .entities
        .first()
        .map(|e| e.id.clone())
        .unwrap_or_default();
    let scripted = scene
        .entities
        .iter()
        .find(|e| !e.graph.nodes.is_empty())
        .map_or_else(|| first.clone(), |e| e.id.clone());
    vec![
        ("find", json!({"text": "grupo do agente", "limit": 1})),
        (
            "scene_tree",
            json!({"fields": ["world_position", "bounds"], "depth": 1, "limit": 3}),
        ),
        (
            "entity",
            json!({"ids": [first, scripted],
                   "fields": ["world", "graph", "collider_bounds", "visual_bounds"]}),
        ),
        ("catalog", json!({"operations": ["event."]})),
    ]
}

fn take_results() -> Result<Results, String> {
    results()
        .lock()
        .unwrap()
        .take()
        .ok_or_else(|| "O cliente ainda não terminou".to_owned())
}

impl NativeQa {
    fn agent_dir(&self) -> PathBuf {
        self.output.join("agents")
    }
    pub(super) fn check_agents(&mut self, label: &str) -> Result<(), String> {
        match label {
            "agent_enable" => {
                let dir = self.agent_dir();
                let _ = std::fs::remove_dir_all(&dir);
                // SAFETY: single-threaded at this point of the native test process.
                unsafe { std::env::set_var("OXY_AGENT_DIR", &dir) };
                self.editor.qa_allow_agents(true);
                self.base = Some(self.editor.state.clone());
            }
            "agent_edit" => {
                let dir = self.agent_dir();
                let mut requests = vec![
                    ("status", json!({})),
                    (
                        "apply",
                        json!({"commands": [
                            {"op": "create_entity", "kind": {"type": "group"},
                             "name": "Grupo do agente"},
                            {"op": "set_attribute", "entity": "$0", "name": "vida",
                             "value": {"Number": 3.0}},
                            // Project-level data in the same undo step, and a warning.
                            {"op": "create_input_action", "name": "Abrir porta", "key": "o"},
                            {"op": "add_node", "entity": "$0", "operation": "event.input",
                             "params": {"action": {"Text": "$2"}}},
                            {"op": "add_node", "entity": "$0", "operation": "input.read",
                             "params": {"action": {"Text": "inexistente"}}},
                        ]}),
                    ),
                    // Nothing to apply: no undo step, and the warning above is not repeated.
                    ("apply", json!({"commands": []})),
                ];
                requests.extend(live_reads(&self.editor));
                requests.extend([
                    ("screenshot", json!({})),
                    ("screenshot", json!({"colliders": true})),
                    (
                        "apply",
                        json!({"commands": [{"op": "rename_entity",
                            "entity": "inexistente", "name": "X"}]}),
                    ),
                ]);
                std::thread::spawn(move || client(dir, requests));
            }
            "agent_edited" => {
                let answers = take_results()?;
                let get = |method: &str, index: usize| {
                    answers
                        .iter()
                        .filter(|(m, _)| m == method)
                        .nth(index)
                        .map(|(_, a)| a.clone())
                        .ok_or_else(|| format!("Sem resposta para {method}"))
                };
                let refused = get("wrong_token", 0)??;
                if refused["error"].as_str().is_none() {
                    return Err("Token errado foi aceito".into());
                }
                let status = get("status", 0)??;
                if status["project"]["name"] != json!(self.editor.state.project.name) {
                    return Err(format!("Status inesperado: {status}"));
                }
                let applied = get("apply", 0)??;
                let id = applied["created"]["0"][0]["id"]
                    .as_str()
                    .ok_or("Lote não devolveu o ID criado")?
                    .to_owned();
                let entity = self
                    .editor
                    .scene()
                    .entity(&id)
                    .ok_or("Objeto do agente não está na cena aberta")?;
                if entity.name != "Grupo do agente"
                    || entity.attributes.get("vida") != Some(&Value::Number(3.))
                {
                    return Err("Lote do agente aplicado de forma incompleta".into());
                }
                let action = applied["created"]["2"][0]["id"]
                    .as_str()
                    .unwrap_or_default();
                if applied["created"]["2"][0]["name"] != "Abrir porta"
                    || self
                        .editor
                        .state
                        .project
                        .input_bindings
                        .get(action)
                        .map(String::as_str)
                        != Some("O")
                {
                    return Err(format!("Ação de entrada do agente ausente: {applied}"));
                }
                if applied["warnings"].as_array().is_none_or(|w| {
                    w.len() != 1 || !w[0].as_str().unwrap_or_default().contains("inexistente")
                }) {
                    return Err(format!("Lote sem o aviso esperado: {applied}"));
                }
                let empty = get("apply", 1)??;
                if empty != json!({"created": {}}) {
                    return Err(format!(
                        "Lote vazio respondeu diferente dos arquivos: {empty}"
                    ));
                }
                if self.editor.qa_undo_label() != Some("Agente: 5 comandos") {
                    return Err(format!(
                        "Passo de desfazer inesperado: {:?}",
                        self.editor.qa_undo_label()
                    ));
                }
                if get("find", 0)??["objects"][0]["id"] != json!(id) {
                    return Err("Busca não encontrou o objeto do agente".into());
                }
                let scene = self.editor.scene().id.clone();
                for (method, params) in live_reads(&self.editor) {
                    let live = get(method, 0)??;
                    let local = query::answer(&self.editor.state.project, &scene, method, &params)?;
                    // Through text, as the live answer came over the channel.
                    let local: Json =
                        serde_json::from_str(&local.to_string()).map_err(|e| e.to_string())?;
                    if live != local {
                        return Err(format!(
                            "Leitura ao vivo de {method} difere da leitura do documento: \
                             {live} / {local}"
                        ));
                    }
                }
                let tree = get("scene_tree", 0)??;
                if tree["objects"][0].get("world_position").is_none() || tree["total"].is_null() {
                    return Err(format!("Árvore sem as opções pedidas: {tree}"));
                }
                let shot = get("screenshot", 0)??;
                if shot["png_base64"].as_str().is_none_or(|s| s.len() < 1000) {
                    return Err("Captura da vista vazia".into());
                }
                let colliders = get("screenshot", 1)??;
                if colliders["colliders"] != true
                    || colliders["view"] != "editor_viewport"
                    || [&colliders["width"], &colliders["height"]]
                        != [&shot["width"], &shot["height"]]
                {
                    return Err(format!("Captura com colisores inesperada: {colliders:?}"));
                }
                if colliders["png_base64"] == shot["png_base64"] {
                    return Err("A captura com colisores não desenhou os colisores".into());
                }
                let error = get("apply", 2)?
                    .err()
                    .ok_or("Comando inválido foi aceito")?;
                if !error.contains("Nada foi alterado") {
                    return Err(format!("Erro sem explicação: {error}"));
                }
                if !self
                    .editor
                    .console
                    .messages
                    .iter()
                    .any(|m| m.starts_with("Agente · "))
                {
                    return Err("Console não registrou a ação do agente".into());
                }
            }
            "agent_while_playing" => {
                let dir = self.agent_dir();
                std::thread::spawn(move || {
                    client(
                        dir,
                        vec![
                            (
                                "apply",
                                json!({"commands": [{"op": "create_entity",
                                    "kind": {"type": "group"}}]}),
                            ),
                            ("screenshot", json!({"colliders": true})),
                        ],
                    )
                });
            }
            "agent_refused_while_playing" => {
                let answers = take_results()?;
                let error = answers
                    .iter()
                    .find(|(m, _)| m == "apply")
                    .and_then(|(_, a)| a.clone().err())
                    .ok_or("Edição durante o teste foi aceita")?;
                if !error.contains("teste") {
                    return Err(format!("Recusa sem motivo claro: {error}"));
                }
                let shot = answers
                    .iter()
                    .find(|(m, _)| m == "screenshot")
                    .ok_or("Sem resposta para screenshot")?
                    .1
                    .clone()?;
                if shot["view"] != "game"
                    || shot["colliders"] != true
                    || shot["png_base64"].as_str().is_none_or(|s| s.len() < 1000)
                {
                    return Err("Captura do jogo com colisores falhou".into());
                }
            }
            "agent_undo" => {
                let dir = self.agent_dir();
                std::thread::spawn(move || client(dir, vec![("undo", json!({}))]));
            }
            "agent_undone" => {
                let answers = take_results()?;
                let undo = answers
                    .iter()
                    .find(|(m, _)| m == "undo")
                    .ok_or("Sem resposta para undo")?
                    .1
                    .clone()?;
                if undo["label"] != "Agente: 5 comandos" {
                    return Err(format!("Desfez outro passo: {undo}"));
                }
                if self.editor.state.project != self.base.as_ref().ok_or("Sem referência")?.project
                {
                    return Err("Desfazer do agente não restaurou o documento".into());
                }
            }
            "agent_disable" => {
                self.editor.qa_allow_agents(false);
            }
            "agent_disabled" => {
                if std::fs::read_dir(self.agent_dir())
                    .map(|entries| entries.count())
                    .unwrap_or(0)
                    != 0
                {
                    return Err("Arquivo de descoberta ficou após desligar".into());
                }
            }
            _ => return Err(format!("Verificação de agentes desconhecida: {label}")),
        }
        Ok(())
    }
}

#[test]
#[ignore = "Opens real native WGPU editor and a local TCP client"]
#[cfg(target_os = "windows")]
fn native_agents_live_channel() {
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = oxy_core::qa::output_dir("v0.5/agents");
    std::fs::create_dir_all(&output).unwrap();
    let report = Arc::new(Mutex::new(Report::default()));
    let shared = report.clone();
    let artifacts = output.clone();
    let options = eframe::NativeOptions {
        persist_window: false,
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default()
            .with_title("OXY Engine — QA agentes")
            .with_inner_size([1440., 900.])
            .with_active(false),
        event_loop_builder: Some(Box::new(|b| {
            b.with_any_thread(true);
        })),
        ..Default::default()
    };
    eframe::run_native(
        "OXY Engine — QA agentes",
        options,
        Box::new(move |cc| {
            let qa = NativeQa::new(
                cc,
                workspace.join("examples/validacao/project.oxy.json"),
                artifacts,
                shared,
            );
            let mut qa = qa;
            qa.actions = VecDeque::from([
                Action::Check("agent_enable"),
                Action::Wait(10),
                Action::Check("agent_edit"),
                Action::Wait(90),
                Action::Check("agent_edited"),
                Action::Screenshot("agent-connected.png"),
                Action::Click("Jogar"),
                Action::Check("agent_while_playing"),
                Action::Wait(90),
                Action::Check("agent_refused_while_playing"),
                Action::Click("Parar"),
                Action::Check("agent_undo"),
                Action::Wait(60),
                Action::Check("agent_undone"),
                Action::Check("agent_disable"),
                Action::Wait(10),
                Action::Check("agent_disabled"),
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
    std::fs::write(output.join("native-agents.txt"), &text).unwrap();
    assert!(r.done && r.error.is_none(), "{text}");
}
