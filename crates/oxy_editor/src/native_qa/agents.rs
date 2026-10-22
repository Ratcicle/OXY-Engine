//! Live agent channel in the real editor: a client finds the editor through its discovery
//! file, edits the open project (objects, logic and an input action, with the warning the
//! batch added, which an empty batch then does not repeat), reads it back (with read options
//! answering as they do on files), captures the viewport (with the collider view too, also
//! while the game runs), imports a PNG (applied to a sprite, its pixels in the texture cache)
//! and a WAV, reads the console (its own edits, then the game's logs and logic of the person's
//! test, still there after the test stops) and the play state, is refused edits and imports
//! while the game is running, and undoes its own steps. It also plays a test of its own with
//! the window in the background (the game goes on, the mouse stays free, its inputs and Esc
//! reach the game), is refused saving until the person allows it in Preferências do editor
//! and then saves with a backup (never an example copy, which would ask for a folder), hands a
//! first-person test to the person through Assumir, and then may pause the person's test but
//! not stop it, resume it or send it inputs. Turning agents off turns their saving off.
use super::*;
use crate::app::Driver;
use oxy_core::{document::Value, encoding, painting::PaintImage, query, random};
use serde_json::{Value as Json, json};
use std::{
    io::{BufRead, BufReader, Write},
    net::TcpStream,
    sync::OnceLock,
    time::{Duration, Instant},
};

pub(super) type Results = Vec<(String, Result<Json, String>)>;
fn results() -> &'static Mutex<Option<Results>> {
    static RESULTS: OnceLock<Mutex<Option<Results>>> = OnceLock::new();
    RESULTS.get_or_init(|| Mutex::new(None))
}

/// Runs `requests` against the editor announced in `dir`, storing each answer.
pub(super) fn client(dir: PathBuf, requests: Vec<(&'static str, Json)>) {
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

/// Whether the client answered every request, or gave up.
pub(super) fn client_finished() -> bool {
    results().lock().unwrap().is_some()
}

pub(super) fn take_results() -> Result<Results, String> {
    results()
        .lock()
        .unwrap()
        .take()
        .ok_or_else(|| "O cliente ainda não terminou".to_owned())
}

/// The answer to the `index`-th request of `method`.
pub(super) fn answer(
    answers: &Results,
    method: &str,
    index: usize,
) -> Result<Result<Json, String>, String> {
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

/// The input action the agent's batch creates, and its logic listens to.
const DOOR_ACTION: &str = "Abrir porta";

/// Fails with `message` and what the editor's test looks like when `condition` does not hold.
fn ensure(condition: bool, message: &str, state: &str) -> Result<(), String> {
    if condition {
        Ok(())
    } else {
        Err(format!("{message} ({state})"))
    }
}

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
    /// The editor's test as the checks see it, for their messages.
    fn agent_test_state(&self) -> String {
        let play = &self.editor.play;
        format!(
            "teste {:?}, condutor {:?}, captura {}, mouse preso {}, chip {:?}, foco {}",
            play.runtime.as_ref().map(|rt| (rt.time, rt.paused)),
            play.driver,
            play.capture,
            self.cursor_captured,
            self.test_chip(),
            !self.unfocused
        )
    }
    fn visible(&self, text: &str) -> bool {
        self.surface.texts.iter().any(|t| t.text == text)
    }
    /// Clicks the switch named `label`; its row label may repeat the name.
    fn click_switch(&mut self, label: &str) -> Result<(), String> {
        let rect = self
            .editor
            .qa_icon_rect(label)
            .ok_or_else(|| format!("Chave ausente: {label}"))?;
        self.click(rect.center());
        Ok(())
    }
    /// Checks of the agent's own test, its saving and the hand-over to the person.
    fn check_agent_play(&mut self, label: &str) -> Result<(), String> {
        let dir = self.agent_dir();
        let state = self.agent_test_state();
        let time = self.editor.play.runtime.as_ref().map(|rt| rt.time);
        match label {
            "agent_play" => {
                // The person works in another window: the agent's test must not pause for it.
                self.unfocused = true;
                std::thread::spawn(move || client(dir, vec![("play", json!({"action": "start"}))]));
            }
            "agent_playing" => {
                let answers = take_results()?;
                let start = answer(&answers, "play", 0)??;
                if start["running"] != true
                    || start["driven_by"] != "agent"
                    || start["mouse_captured"] != false
                    || start["advancing"] != true
                    || start["paused"] != false
                    || start["scene"]["id"] != json!(self.editor.scene_id)
                {
                    return Err(format!("Início do teste pelo agente inesperado: {start}"));
                }
                ensure(
                    self.editor.play.driver == Driver::Agent && !self.editor.play.capture,
                    "O teste do agente deveria deixar o mouse e o teclado com a pessoa",
                    &state,
                )?;
                ensure(
                    self.editor
                        .play
                        .runtime
                        .as_ref()
                        .is_some_and(|rt| !rt.paused && rt.time > 0.),
                    "O teste do agente deveria correr com a janela sem foco",
                    &state,
                )?;
                ensure(
                    !self.cursor_captured
                        && self.test_chip() == Some("Agente jogando")
                        && self.visible("Assumir")
                        && self.visible("Um agente está jogando"),
                    "A barra deveria mostrar Agente jogando e Assumir, com o mouse livre",
                    &state,
                )?;
                ensure(
                    self.editor
                        .console
                        .messages
                        .iter()
                        .any(|m| m.starts_with("Agente · Teste iniciado em ")),
                    "O Console não diz que o agente iniciou o teste",
                    &state,
                )?;
                // Esc waits in the editor for its game time, as clicks do.
                let press = json!([
                    {"type": "press", "at": 0.0, "action": DOOR_ACTION},
                    {"type": "escape", "at": 0.05},
                    {"type": "release", "at": 0.1, "action": DOOR_ACTION},
                ]);
                let requests = vec![
                    ("status", json!({})),
                    ("play", json!({"action": "input", "inputs": press})),
                    (
                        "play",
                        json!({"action": "input", "inputs":
                            [{"type": "press", "at": 0.0, "action": "voar"}]}),
                    ),
                    ("play", json!({"action": "start"})),
                    ("save", json!({})),
                    ("status", json!({})),
                ];
                std::thread::spawn(move || client(dir, requests));
            }
            "agent_play_input" => {
                let answers = take_results()?;
                let get = |method: &str, index: usize| answer(&answers, method, index);
                let status = get("status", 0)??;
                let before = status["play"]["time"].as_f64().ok_or("Status sem tempo")?;
                if status["play"]["driven_by"] != "agent" || status["busy"].is_null() {
                    return Err(format!("Status do teste do agente inesperado: {status}"));
                }
                let input = get("play", 0)??;
                let queued = input["queued_until"].as_f64().ok_or("Entradas sem fila")?;
                let at = input["time"].as_f64().ok_or("Resposta sem tempo")?;
                if (queued - at - 0.1).abs() > 1e-6 {
                    return Err(format!("Entradas fora do relógio do jogo: {input}"));
                }
                let error = get("play", 1)?.err().ok_or("Ação inexistente aceita")?;
                if !error.contains("\"voar\" não existe") {
                    return Err(format!("Recusa da ação sem motivo claro: {error}"));
                }
                let error = get("play", 2)?.err().ok_or("Segundo teste aceito")?;
                if !error.contains("Já há um teste") {
                    return Err(format!("Recusa do segundo teste sem motivo claro: {error}"));
                }
                let error = get("save", 0)?
                    .err()
                    .ok_or("Salvou sem a permissão da pessoa")?;
                if !error.contains("Permitir que agentes salvem") {
                    return Err(format!("Recusa ao salvar sem motivo claro: {error}"));
                }
                let status = get("status", 1)??;
                if status["can_save"] != false
                    || !status["save_refusal"]
                        .as_str()
                        .is_some_and(|r| r.contains("Permitir que agentes salvem"))
                {
                    return Err(format!("Status sem a recusa de salvar: {status}"));
                }
                ensure(
                    time.is_some_and(|now| now > before + 0.05)
                        && self
                            .editor
                            .play
                            .runtime
                            .as_ref()
                            .is_some_and(|rt| !rt.paused),
                    "O jogo deveria seguir com a janela sem foco",
                    &state,
                )?;
                ensure(
                    self.editor.play.agent_clicks.is_empty(),
                    "O Esc do agente deveria ter chegado ao jogo na hora dele",
                    &state,
                )?;
                let requests = vec![
                    (
                        "console",
                        json!({"origin": ["jogo", "logica"], "limit": 300}),
                    ),
                    ("play", json!({"action": "pause"})),
                    (
                        "play",
                        json!({"action": "input", "inputs": [{"type": "escape", "at": 0.0}]}),
                    ),
                    ("play", json!({"action": "resume"})),
                    ("play", json!({"action": "stop"})),
                    ("status", json!({})),
                ];
                std::thread::spawn(move || client(dir, requests));
            }
            "agent_play_stopped" => {
                let answers = take_results()?;
                let get = |method: &str, index: usize| answer(&answers, method, index);
                let console = get("console", 0)??;
                if !lines(&console).iter().any(|l| {
                    l["operation"] == "event.input" && l["object"]["name"] == "Grupo do agente"
                }) {
                    return Err(format!(
                        "A entrada do agente não chegou à lógica: {console}"
                    ));
                }
                if !lines(&console).iter().any(|l| {
                    l["origin"] == "jogo"
                        && l["text"]
                            .as_str()
                            .is_some_and(|t| t.starts_with("Esc: a cena não tem menu de pausa"))
                }) {
                    return Err(format!("O Esc do agente não chegou ao jogo: {console}"));
                }
                let paused = get("play", 0)??;
                if paused["paused"] != true || paused["advancing"] != false {
                    return Err(format!("Pausa do agente inesperada: {paused}"));
                }
                let error = get("play", 1)?
                    .err()
                    .ok_or("Entrada aceita no teste pausado")?;
                if !error.contains("pausado") {
                    return Err(format!("Recusa da entrada sem motivo claro: {error}"));
                }
                let resumed = get("play", 2)??;
                if resumed["paused"] != false || resumed["driven_by"] != "agent" {
                    return Err(format!("O agente não retomou o teste: {resumed}"));
                }
                let stopped = get("play", 3)??;
                let seed = self.editor.play.seed.ok_or("Teste sem semente")?;
                if stopped != json!({"running": false, "seed": seed}) {
                    return Err(format!("O agente não parou o teste: {stopped}"));
                }
                ensure(
                    self.editor.play.runtime.is_none()
                        && self.editor.play.driver == Driver::Person
                        && self.editor.tab == crate::app::Tab::Scene,
                    "Parar pelo agente deveria encerrar o teste e voltar à Cena",
                    &state,
                )?;
                for line in ["Agente · Teste pausado", "Agente · Teste retomado"] {
                    ensure(
                        self.editor.console.messages.iter().any(|m| m == line),
                        &format!("O Console não diz {line}"),
                        &state,
                    )?;
                }
                if get("status", 0)??["play"] != json!({"running": false, "seed": seed}) {
                    return Err("O status ainda mostra o teste do agente".into());
                }
                self.unfocused = false;
            }
            "agent_preferences" => {
                // Saving starts off, under agents that are on.
                ensure(
                    self.surface.modal.is_some()
                        && self.visible("Permitir que agentes salvem")
                        && self.visible("desligado")
                        && self.surface.texts.iter().any(|t| {
                            t.text
                                .starts_with("Quando ligado, o agente pode salvar o projeto")
                        })
                        && !self.visible("ligue Permitir agentes primeiro")
                        && self.editor.qa_agent_preferences() == (true, false),
                    "Preferências sem a opção de salvar pelos agentes, desligada",
                    &state,
                )?;
            }
            "agent_agents_switch" => self.click_switch("Permitir agentes de IA")?,
            "agent_preferences_locked" => {
                // With agents off, saving cannot be turned on.
                ensure(
                    self.visible("ligue Permitir agentes primeiro"),
                    "Com os agentes desligados, a opção de salvar deveria pedir para ligá-los",
                    &state,
                )?;
            }
            "agent_save_switch" => {
                ensure(
                    !self.visible("ligue Permitir agentes primeiro"),
                    "Religar os agentes deveria liberar a opção de salvar",
                    &state,
                )?;
                self.click_switch("Permitir que agentes salvem")?;
            }
            "agent_save" => {
                // The person allowed it: everything unsaved is written, the agent's edits and
                // imports too.
                ensure(
                    self.surface.modal.is_none()
                        && self.editor.qa_agent_preferences() == (true, true),
                    "Aplicar deveria permitir que agentes salvem",
                    &state,
                )?;
                std::thread::spawn(move || {
                    client(
                        dir,
                        vec![
                            ("status", json!({})),
                            ("save", json!({})),
                            ("status", json!({})),
                        ],
                    )
                });
            }
            "agent_saved" => {
                let answers = take_results()?;
                let get = |method: &str, index: usize| answer(&answers, method, index);
                let before = get("status", 0)??;
                if before["can_save"] != true
                    || before.get("save_refusal").is_some()
                    || before["project"]["unsaved_changes"] != true
                {
                    return Err(format!("Status antes de salvar inesperado: {before}"));
                }
                let saved = get("save", 0)??;
                let path = self.editor.path.clone().ok_or("Projeto sem arquivo")?;
                let backup = path.with_extension("json.agente.bak");
                if saved["saved"] != json!(path.display().to_string())
                    || saved["previous_version"] != json!(backup.display().to_string())
                {
                    return Err(format!("Resposta de salvar inesperada: {saved}"));
                }
                let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
                let original = std::fs::read(workspace.join("examples/validacao/project.oxy.json"))
                    .map_err(|e| e.to_string())?;
                if std::fs::read(&backup).map_err(|e| e.to_string())? != original {
                    return Err("A versão anterior não foi guardada em .agente.bak".into());
                }
                let written = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
                if !written.contains("Grupo do agente") || !written.contains("Placa do agente") {
                    return Err("O arquivo salvo não tem as edições ainda não salvas".into());
                }
                let after = get("status", 1)??;
                if after["project"]["unsaved_changes"] != false {
                    return Err(format!("O projeto continua com alterações: {after}"));
                }
                if !self
                    .editor
                    .console
                    .messages
                    .iter()
                    .any(|m| *m == format!("Agente · Salvo: {}", path.display()))
                {
                    return Err("O Console não diz que o agente salvou".into());
                }
            }
            "agent_example_copy" => {
                // Salvar would ask the person for a folder, in a dialog the agent cannot answer.
                self.editor.qa_example_copy(true);
                std::thread::spawn(move || {
                    client(dir, vec![("status", json!({})), ("save", json!({}))])
                });
            }
            "agent_example_refused" => {
                let answers = take_results()?;
                let status = answer(&answers, "status", 0)??;
                if status["can_save"] != false
                    || !status["save_refusal"]
                        .as_str()
                        .is_some_and(|r| r.contains("cópia de exemplo"))
                {
                    return Err(format!("Status sem a recusa da cópia de exemplo: {status}"));
                }
                let error = answer(&answers, "save", 0)?
                    .err()
                    .ok_or("Salvou a cópia de exemplo")?;
                if !error.contains("cópia de exemplo") {
                    return Err(format!(
                        "Recusa da cópia de exemplo sem motivo claro: {error}"
                    ));
                }
                self.editor.qa_example_copy(false);
            }
            "agent_takeover" => {
                // A first-person game, whose camera would hold the person's mouse.
                let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
                let project = self.output.join("laboratorio");
                let _ = std::fs::remove_dir_all(&project);
                copy_folder(&workspace.join("examples/laboratorio-3d"), &project)
                    .map_err(|e| e.to_string())?;
                self.editor.open(project.join("project.oxy.json"));
                self.unfocused = true;
                std::thread::spawn(move || client(dir, vec![("play", json!({"action": "start"}))]));
            }
            "agent_driving" => {
                let answers = take_results()?;
                let start = answer(&answers, "play", 0)??;
                if start["driven_by"] != "agent" || start["mouse_captured"] != false {
                    return Err(format!(
                        "Teste em primeira pessoa do agente inesperado: {start}"
                    ));
                }
                ensure(
                    self.editor
                        .play
                        .runtime
                        .as_ref()
                        .is_some_and(|rt| rt.wants_relative_mouse() && !rt.paused)
                        && !self.editor.play.capture
                        && !self.cursor_captured
                        && self.test_chip() == Some("Agente jogando")
                        && self.visible("Assumir"),
                    "A câmera em primeira pessoa do agente não deveria prender o mouse",
                    &state,
                )?;
                // The person comes back to the editor to take over.
                self.unfocused = false;
            }
            "agent_taken_over" => {
                ensure(
                    self.editor.play.driver == Driver::Person
                        && self.editor.play.capture
                        && self.cursor_captured
                        && self
                            .editor
                            .play
                            .runtime
                            .as_ref()
                            .is_some_and(|rt| !rt.paused)
                        && self.test_chip() == Some("Jogando")
                        && !self.visible("Assumir")
                        && !self.visible("Um agente está jogando"),
                    "Assumir deveria passar o teste, o mouse e o teclado para a pessoa",
                    &state,
                )?;
            }
            "agent_person_test" => {
                // The agent pauses the person's test, and asks for what is not its own.
                let escape = json!([{"type": "escape", "at": 0.0}]);
                std::thread::spawn(move || {
                    client(
                        dir,
                        vec![
                            ("status", json!({})),
                            ("play", json!({"action": "stop"})),
                            ("play", json!({"action": "input", "inputs": escape})),
                            ("play", json!({"action": "pause"})),
                            ("play", json!({"action": "resume"})),
                        ],
                    )
                });
            }
            "agent_person_test_kept" => {
                let answers = take_results()?;
                let status = answer(&answers, "status", 0)??;
                if status["play"]["driven_by"] != "person"
                    || status["play"]["mouse_captured"] != true
                {
                    return Err(format!("Status depois de Assumir inesperado: {status}"));
                }
                let error = answer(&answers, "play", 0)?
                    .err()
                    .ok_or("O agente parou o teste da pessoa")?;
                if !error.contains("da pessoa") {
                    return Err(format!("Recusa de parar sem motivo claro: {error}"));
                }
                let error = answer(&answers, "play", 1)?
                    .err()
                    .ok_or("O agente mandou entradas ao teste da pessoa")?;
                if !error.contains("da pessoa") {
                    return Err(format!("Recusa das entradas sem motivo claro: {error}"));
                }
                let paused = answer(&answers, "play", 2)??;
                if paused["paused"] != true || paused["driven_by"] != "person" {
                    return Err(format!("Pausa do teste da pessoa inesperada: {paused}"));
                }
                let error = answer(&answers, "play", 3)?
                    .err()
                    .ok_or("O agente retomou o teste da pessoa")?;
                if !error.contains("Retomar") {
                    return Err(format!("Recusa de retomar sem motivo claro: {error}"));
                }
                ensure(
                    self.editor
                        .play
                        .runtime
                        .as_ref()
                        .is_some_and(|rt| rt.paused)
                        && self.editor.play.driver == Driver::Person,
                    "O teste da pessoa deveria continuar dela, pausado pelo agente",
                    &state,
                )?;
            }
            _ => return Err(format!("Verificação de agentes desconhecida: {label}")),
        }
        Ok(())
    }
    pub(super) fn check_agents(&mut self, label: &str) -> Result<(), String> {
        if [
            "agent_play",
            "agent_playing",
            "agent_play_input",
            "agent_play_stopped",
            "agent_preferences",
            "agent_agents_switch",
            "agent_preferences_locked",
            "agent_save_switch",
            "agent_save",
            "agent_saved",
            "agent_example_copy",
            "agent_example_refused",
            "agent_takeover",
            "agent_driving",
            "agent_taken_over",
            "agent_person_test",
            "agent_person_test_kept",
        ]
        .contains(&label)
        {
            return self.check_agent_play(label);
        }
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
                            // The person's test is theirs to stop.
                            ("play", json!({"action": "stop"})),
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
                let error = get("play", 0)?
                    .err()
                    .ok_or("O agente parou o teste da pessoa")?;
                if !error.contains("da pessoa") {
                    return Err(format!("Recusa de parar sem motivo claro: {error}"));
                }
                let status = get("status", 0)??;
                let play = &status["play"];
                if play["running"] != true
                    || play["game_paused"] != false
                    || play["driven_by"] != "person"
                    || play["advancing"] != true
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
                // Through Preferências, as the person would, with saving still on.
                if self.editor.qa_agent_preferences() != (true, true) {
                    return Err(
                        "Os agentes deveriam estar ligados e com permissão de salvar".into(),
                    );
                }
                self.click_switch("Permitir agentes de IA")?;
            }
            "agent_disabled" => {
                if self.editor.qa_agent_preferences() != (false, false) {
                    return Err("Desligar os agentes deveria desligar também o salvamento".into());
                }
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
#[expect(
    clippy::disallowed_methods,
    reason = "QA copies its fixtures into folders of its own"
)]
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
                // The agent's own test, with the editor window in the background.
                Action::Check("agent_play"),
                Action::Wait(60),
                Action::Check("agent_playing"),
                Action::Screenshot("agent-playing.png"),
                Action::Wait(60),
                Action::Check("agent_play_input"),
                Action::Wait(90),
                Action::Check("agent_play_stopped"),
                // The person lets agents save, in Preferências do editor.
                Action::Click("Interface: 100%"),
                Action::Check("agent_preferences"),
                Action::Screenshot("agent-preferences.png"),
                Action::Check("agent_agents_switch"),
                Action::Wait(10),
                Action::Check("agent_preferences_locked"),
                Action::Screenshot("agent-preferences-locked.png"),
                Action::Check("agent_agents_switch"),
                Action::Wait(10),
                Action::Check("agent_save_switch"),
                Action::Wait(10),
                Action::Screenshot("agent-preferences-on.png"),
                Action::Click("Aplicar"),
                Action::Wait(5),
                Action::Check("agent_save"),
                Action::Wait(60),
                Action::Check("agent_saved"),
                Action::Check("agent_example_copy"),
                Action::Wait(60),
                Action::Check("agent_example_refused"),
                Action::Click("Jogar"),
                Action::Check("agent_while_playing"),
                Action::Wait(90),
                Action::Check("agent_refused_while_playing"),
                Action::Click("Parar"),
                Action::Check("agent_undo"),
                Action::Wait(60),
                Action::Check("agent_undone"),
                // A first-person test the agent plays, which the person takes over.
                Action::Check("agent_takeover"),
                Action::Wait(90),
                Action::Check("agent_driving"),
                Action::Screenshot("agent-driving.png"),
                Action::Click("Assumir"),
                Action::Check("agent_taken_over"),
                Action::Screenshot("agent-taken-over.png"),
                Action::Check("agent_person_test"),
                Action::Wait(60),
                Action::Check("agent_person_test_kept"),
                Action::Click("Parar"),
                // Turning agents off in Preferências turns their saving off too.
                Action::Click("Interface: 100%"),
                Action::Check("agent_disable"),
                Action::Wait(10),
                Action::Check("agent_preferences_locked"),
                Action::Click("Aplicar"),
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
