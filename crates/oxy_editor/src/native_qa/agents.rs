//! Live agent channel in the real editor: a client finds the editor through its discovery
//! file, edits the open project, reads it back, captures the viewport, is refused while the
//! game is running, and undoes its own step.
use super::*;
use oxy_core::document::Value;
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
                std::thread::spawn(move || {
                    client(
                        dir,
                        vec![
                            ("status", json!({})),
                            (
                                "apply",
                                json!({"commands": [
                                    {"op": "create_entity", "kind": {"type": "group"},
                                     "name": "Grupo do agente"},
                                    {"op": "set_attribute", "entity": "$0", "name": "vida",
                                     "value": {"Number": 3.0}},
                                ]}),
                            ),
                            ("find", json!({"text": "grupo do agente"})),
                            ("screenshot", json!({})),
                            (
                                "apply",
                                json!({"commands": [{"op": "rename_entity",
                                    "entity": "inexistente", "name": "X"}]}),
                            ),
                        ],
                    )
                });
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
                let id = applied["applied"][0]["created"][0]
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
                if self.editor.qa_undo_label() != Some("Agente: 2 comandos") {
                    return Err(format!(
                        "Passo de desfazer inesperado: {:?}",
                        self.editor.qa_undo_label()
                    ));
                }
                if get("find", 0)??[0]["id"] != json!(id) {
                    return Err("Busca não encontrou o objeto do agente".into());
                }
                let shot = get("screenshot", 0)??;
                if shot["png_base64"].as_str().is_none_or(|s| s.len() < 1000) {
                    return Err("Captura da vista vazia".into());
                }
                let error = get("apply", 1)?
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
                        vec![(
                            "apply",
                            json!({"commands": [{"op": "create_entity",
                                "kind": {"type": "group"}}]}),
                        )],
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
                if undo["label"] != "Agente: 2 comandos" {
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
                Action::Wait(60),
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
