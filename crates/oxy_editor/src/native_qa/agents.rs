//! Live agent channel in the real editor: a client finds the editor through its discovery
//! file, edits the open project (objects, logic and an input action, with the warning the
//! batch added, which an empty batch then does not repeat), reads it back (with read options
//! answering as they do on files), captures the viewport (with the collider view too, also
//! while the game runs), imports a PNG (applied to a sprite, its pixels in the texture cache)
//! and a WAV, reads the console (its own edits, then the game's logs and logic of the person's
//! test, still there after the test stops) and the play state, is refused edits and imports
//! while the game is running, and undoes its own steps.
use super::*;
use oxy_core::{document::Value, encoding, painting::PaintImage, query, random};
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

/// The answer to the `index`-th request of `method`.
fn answer(answers: &Results, method: &str, index: usize) -> Result<Result<Json, String>, String> {
    answers
        .iter()
        .filter(|(m, _)| m == method)
        .nth(index)
        .map(|(_, a)| a.clone())
        .ok_or_else(|| format!("Sem resposta para {method}"))
}

/// The console lines of an answer to `console`.
fn lines(console: &Json) -> Vec<Json> {
    console["lines"].as_array().cloned().unwrap_or_default()
}

/// A mono 16-bit PCM WAV of `samples` silent samples.
fn wav(samples: usize, rate: u32) -> Vec<u8> {
    let data = samples * 2;
    let mut bytes = Vec::new();
    bytes.extend(b"RIFF");
    bytes.extend((36 + data as u32).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16u32.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(rate.to_le_bytes());
    bytes.extend((rate * 2).to_le_bytes());
    bytes.extend(2u16.to_le_bytes());
    bytes.extend(16u16.to_le_bytes());
    bytes.extend(b"data");
    bytes.extend((data as u32).to_le_bytes());
    bytes.extend(vec![0u8; data]);
    bytes
}

/// The message the agent's batch logs when the game starts.
const GREETING: &str = "Olá do agente";

impl NativeQa {
    fn agent_dir(&self) -> PathBuf {
        self.output.join("agents")
    }
    /// Files the agent imports from, outside the project.
    fn agent_sources(&self) -> PathBuf {
        self.output.join("agent-sources")
    }
    /// The sprite the imported image goes on.
    fn agent_sprite(&self) -> Option<&oxy_core::document::Entity> {
        self.editor
            .scene()
            .entities
            .iter()
            .find(|e| e.primitive == Some(oxy_core::document::Primitive::Sprite) && e.ui.is_none())
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
                            // A log line for the console while the game runs.
                            {"op": "add_node", "entity": "$0", "operation": "event.scene_start"},
                            {"op": "add_node", "entity": "$0", "operation": "debug.message",
                             "params": {"message": {"Text": GREETING}}},
                            {"op": "connect", "entity": "$0", "from_node": "$5",
                             "from_port": "exec", "to_node": "$6", "to_port": "exec"},
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
                    ("console", json!({"origin": "agente"})),
                ]);
                std::thread::spawn(move || client(dir, requests));
            }
            "agent_edited" => {
                let answers = take_results()?;
                let get = |method: &str, index: usize| answer(&answers, method, index);
                let refused = get("wrong_token", 0)??;
                if refused["error"].as_str().is_none() {
                    return Err("Token errado foi aceito".into());
                }
                let status = get("status", 0)??;
                if status["project"]["name"] != json!(self.editor.state.project.name)
                    || status["play"] != json!({"running": false})
                {
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
                if self.editor.qa_undo_label() != Some("Agente: 8 comandos") {
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
                // The agent reads its own steps and its error back, without the prefix.
                let console = get("console", 0)??;
                let mine = lines(&console);
                if mine.iter().any(|l| l["origin"] != "agente")
                    || !mine.iter().any(|l| l["text"] == "Criar objeto")
                    || !mine
                        .iter()
                        .any(|l| l["text"].as_str().unwrap_or_default().contains("Nada foi"))
                    || console["last_seq"].as_u64().is_none()
                {
                    return Err(format!("Console do agente inesperado: {console}"));
                }
            }
            "agent_import" => {
                let dir = self.agent_dir();
                let sources = self.agent_sources();
                std::fs::create_dir_all(&sources).map_err(|e| e.to_string())?;
                let image = sources.join("Placa do agente.png");
                let png = PaintImage::new(16, 8, [30, 160, 220, 255])?.to_png()?;
                std::fs::write(&image, png).map_err(|e| e.to_string())?;
                let sprite = self
                    .agent_sprite()
                    .ok_or("A cena aberta não tem sprite")?
                    .id
                    .clone();
                let requests = vec![
                    (
                        "import",
                        json!({"path": image.display().to_string(), "apply_to": sprite}),
                    ),
                    (
                        "import",
                        json!({"data_base64": encoding::base64(&wav(4000, 8000)),
                               "name": "Pulo"}),
                    ),
                    (
                        "import",
                        json!({"path": sources.join("leia.txt").display().to_string()}),
                    ),
                    (
                        "import",
                        json!({"path": image.display().to_string(), "kind": "audio"}),
                    ),
                    ("status", json!({})),
                ];
                std::thread::spawn(move || client(dir, requests));
            }
            "agent_imported" => {
                let answers = take_results()?;
                let get = |method: &str, index: usize| answer(&answers, method, index);
                let image = get("import", 0)??;
                let asset = image["asset"].as_str().ok_or("Importação sem asset")?;
                let stored = self
                    .editor
                    .state
                    .project
                    .asset(asset)
                    .ok_or("O recurso importado não está no projeto")?;
                if image["kind"] != "texture"
                    || image["name"] != "Placa do agente"
                    || [&image["width"], &image["height"]] != [&json!(16), &json!(8)]
                    || !self.editor.root().join(&stored.path).is_file()
                {
                    return Err(format!("Importação da imagem inesperada: {image}"));
                }
                let sprite = self.agent_sprite().ok_or("Sprite sumiu")?;
                if sprite.material.texture.as_deref() != Some(asset)
                    || (sprite.dimensions[0] - sprite.dimensions[1] * 2.).abs() > 1e-4
                {
                    return Err("A imagem não foi aplicada ao sprite com a proporção dela".into());
                }
                // The step carries the pixels: they are in the cache, waiting to be saved.
                if self
                    .editor
                    .state
                    .images
                    .get(asset)
                    .is_none_or(|p| (p.width, p.height) != (16, 8))
                    || !self.editor.state.images.is_dirty(asset)
                {
                    return Err("Os pixels importados não estão no cache de texturas".into());
                }
                let sound = get("import", 1)??;
                if sound["kind"] != "audio" || sound["seconds"] != json!(0.5) {
                    return Err(format!("Importação do som inesperada: {sound}"));
                }
                let error = get("import", 2)?.err().ok_or("Arquivo .txt foi aceito")?;
                if !error.contains(".png e .wav") {
                    return Err(format!("Recusa do .txt sem motivo claro: {error}"));
                }
                let error = get("import", 3)?.err().ok_or("Imagem aceita como som")?;
                if !error.contains("kind pede som WAV") {
                    return Err(format!("Recusa do tipo sem motivo claro: {error}"));
                }
                let status = get("status", 0)??;
                if status["undo"] != "Agente: Importar recurso" {
                    return Err(format!("Importação fora do histórico: {status}"));
                }
            }
            "agent_while_playing" => {
                let dir = self.agent_dir();
                let image = self.agent_sources().join("Placa do agente.png");
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
                            ("import", json!({"path": image.display().to_string()})),
                            ("status", json!({})),
                            ("console", json!({"origin": ["jogo", "logica"]})),
                        ],
                    )
                });
            }
            "agent_refused_while_playing" => {
                let answers = take_results()?;
                let get = |method: &str, index: usize| answer(&answers, method, index);
                let error = get("apply", 0)?
                    .err()
                    .ok_or("Edição durante o teste foi aceita")?;
                if !error.contains("teste") {
                    return Err(format!("Recusa sem motivo claro: {error}"));
                }
                let error = get("import", 0)?
                    .err()
                    .ok_or("Importação durante o teste foi aceita")?;
                if !error.contains("teste") {
                    return Err(format!("Recusa da importação sem motivo claro: {error}"));
                }
                let shot = get("screenshot", 0)??;
                if shot["view"] != "game"
                    || shot["colliders"] != true
                    || shot["png_base64"].as_str().is_none_or(|s| s.len() < 1000)
                {
                    return Err("Captura do jogo com colisores falhou".into());
                }
                let status = get("status", 0)??;
                let play = &status["play"];
                if play["running"] != true
                    || play["scene"]["id"] != json!(self.editor.scene_id)
                    || !play["time"].is_number()
                    || play["seed"]
                        .as_u64()
                        .is_none_or(|seed| seed > random::MAX_SEED)
                    || status["busy"].as_str().is_none_or(|b| !b.contains("teste"))
                {
                    return Err(format!("Status sem o teste em andamento: {status}"));
                }
                let console = get("console", 0)??;
                let lines = lines(&console);
                let greeting = lines
                    .iter()
                    .find(|l| l["origin"] == "jogo" && l["text"] == GREETING)
                    .ok_or_else(|| format!("Log do jogo ausente no console: {console}"))?;
                if greeting["object"]["name"] != "Grupo do agente" || !greeting["node"].is_string()
                {
                    return Err(format!("Log do jogo sem objeto e nó: {greeting}"));
                }
                if !lines
                    .iter()
                    .any(|l| l["origin"] == "logica" && l["operation"] == "debug.message")
                {
                    return Err(format!("Ações da lógica ausentes no console: {console}"));
                }
            }
            "agent_undo" => {
                let dir = self.agent_dir();
                std::thread::spawn(move || {
                    client(
                        dir,
                        vec![
                            // The test stopped: its lines are still there.
                            ("console", json!({"origin": "jogo"})),
                            ("status", json!({})),
                            ("undo", json!({})),
                            ("undo", json!({})),
                            ("undo", json!({})),
                        ],
                    )
                });
            }
            "agent_undone" => {
                let answers = take_results()?;
                let get = |method: &str, index: usize| answer(&answers, method, index);
                let console = get("console", 0)??;
                if !lines(&console).iter().any(|l| l["text"] == GREETING) {
                    return Err(format!("O log do teste sumiu ao parar: {console}"));
                }
                let status = get("status", 0)??;
                // Stopped, with the seed of the test that ran.
                let seed = self.editor.play.seed.ok_or("Teste sem semente")?;
                if status["play"] != json!({"running": false, "seed": seed}) {
                    return Err(format!("Teste ainda em andamento ou sem semente: {status}"));
                }
                let labels: Vec<Json> = (0..3)
                    .map(|i| get("undo", i).and_then(|a| a).map(|a| a["label"].clone()))
                    .collect::<Result<_, _>>()?;
                if labels
                    != [
                        json!("Agente: Importar recurso"),
                        json!("Agente: Importar recurso"),
                        json!("Agente: 8 comandos"),
                    ]
                {
                    return Err(format!("Desfez outros passos: {labels:?}"));
                }
                let base = &self.base.as_ref().ok_or("Sem referência")?.project;
                if self.editor.state.project != *base {
                    return Err("Desfazer do agente não restaurou o documento".into());
                }
                if self
                    .editor
                    .state
                    .images
                    .keys()
                    .any(|id| base.asset(id).is_none())
                {
                    return Err("A imagem importada ficou no cache depois de desfazer".into());
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

/// Copies a folder and everything in it.
fn copy_folder(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_folder(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

#[test]
#[ignore = "Opens real native WGPU editor and a local TCP client"]
#[cfg(target_os = "windows")]
fn native_agents_live_channel() {
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = oxy_core::qa::output_dir("v0.5/agents");
    std::fs::create_dir_all(&output).unwrap();
    // Imports write into the project's assets folder: the example is copied, never touched.
    let project = output.join("projeto");
    let _ = std::fs::remove_dir_all(&project);
    copy_folder(&workspace.join("examples/validacao"), &project).unwrap();
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
            let qa = NativeQa::new(cc, project.join("project.oxy.json"), artifacts, shared);
            let mut qa = qa;
            qa.actions = VecDeque::from([
                Action::Check("agent_enable"),
                Action::Wait(10),
                Action::Check("agent_edit"),
                Action::Wait(90),
                Action::Check("agent_edited"),
                Action::Screenshot("agent-connected.png"),
                Action::Check("agent_import"),
                Action::Wait(60),
                Action::Check("agent_imported"),
                Action::Screenshot("agent-imported.png"),
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
        "Concluído: {}\nErro: {:?}\nPassos:\n{}\nCapturas: {:?}",
        r.done,
        r.error,
        r.steps.join("\n"),
        r.screenshots
    );
    std::fs::write(output.join("native-agents.txt"), &text).unwrap();
    assert!(r.done && r.error.is_none(), "{text}");
}
