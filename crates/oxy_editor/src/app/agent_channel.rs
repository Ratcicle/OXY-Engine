//! Local channel that lets the OXY MCP server (oxy-mcp) talk to this editor while it is open.
//!
//! When "Permitir agentes de IA" is on, the editor listens on 127.0.0.1 at a random port and
//! writes `editor-<pid>.json` (port, token, project) to the agents folder under
//! `%LOCALAPPDATA%\OXY Engine`. A client sends one JSON line `{"token": "..."}`, then
//! requests `{"id": 1, "method": "...", "params": {...}}`, one per line, and receives
//! `{"id": 1, "result": ...}` or `{"id": 1, "error": "..."}`. Requests run on the UI thread
//! in the next frame, and edits wait while a person is dragging or typing. Besides edits and
//! reads, an agent can import a PNG or WAV (`import`) and read the console (`console`), and
//! `status` says whether the person is testing the game.
use super::*;
use oxy_core::{command, query};
use serde_json::{Value as Json, json};
use std::{
    collections::VecDeque,
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    time::Duration,
};

/// How long a connection waits for the editor to answer one request.
const REPLY_TIMEOUT: Duration = Duration::from_secs(60);

pub(crate) struct Incoming {
    method: String,
    params: Json,
    reply: mpsc::Sender<Result<Json, String>>,
}

pub(crate) struct AgentChannel {
    requests: mpsc::Receiver<Incoming>,
    queue: VecDeque<Incoming>,
    connections: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    port: u16,
    token: String,
    discovery: Option<PathBuf>,
    advertised: Option<Json>,
}

/// Folder where open editors announce their channel. Tests point it elsewhere.
pub(crate) fn discovery_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("OXY_AGENT_DIR") {
        return Some(PathBuf::from(dir));
    }
    std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("OXY Engine").join("agents"))
}

impl AgentChannel {
    pub(crate) fn start(ctx: &egui::Context) -> std::io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        let token = new_id();
        let (sender, requests) = mpsc::channel();
        let connections = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        {
            let (ctx, token, connections, stop) = (
                ctx.clone(),
                token.clone(),
                connections.clone(),
                stop.clone(),
            );
            std::thread::Builder::new()
                .name("oxy-agent-listener".into())
                .spawn(move || {
                    while !stop.load(Ordering::Relaxed) {
                        match listener.accept() {
                            Ok((stream, _)) => {
                                let (ctx, token, connections, sender, stop) = (
                                    ctx.clone(),
                                    token.clone(),
                                    connections.clone(),
                                    sender.clone(),
                                    stop.clone(),
                                );
                                let _ = std::thread::Builder::new()
                                    .name("oxy-agent-connection".into())
                                    .spawn(move || {
                                        serve(stream, &token, &ctx, &connections, &sender, &stop)
                                    });
                            }
                            Err(_) => std::thread::sleep(Duration::from_millis(100)),
                        }
                    }
                })?;
        }
        Ok(Self {
            requests,
            queue: VecDeque::new(),
            connections,
            stop,
            port,
            token,
            discovery: None,
            advertised: None,
        })
    }
    pub(crate) fn connected(&self) -> bool {
        self.connections.load(Ordering::Relaxed) > 0
    }
    /// Writes or refreshes the discovery file when the open project changes.
    fn advertise(&mut self, project: Option<(&str, Option<&Path>)>) {
        let info = json!({
            "pid": std::process::id(),
            "port": self.port,
            "token": self.token,
            "version": env!("OXY_APP_VERSION"),
            "project_name": project.map(|(name, _)| name),
            "project_path": project.and_then(|(_, path)| path).map(|p| p.display().to_string()),
        });
        if self.advertised.as_ref() == Some(&info) {
            return;
        }
        let Some(dir) = discovery_dir() else {
            return;
        };
        let path = dir.join(format!("editor-{}.json", std::process::id()));
        let written = serde_json::to_vec_pretty(&info)
            .map_err(|e| e.to_string())
            .and_then(|bytes| persistence::safe_write(&path, &bytes).map_err(|e| e.to_string()));
        if written.is_ok() {
            self.discovery = Some(path);
            self.advertised = Some(info);
        }
    }
}
impl Drop for AgentChannel {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(path) = &self.discovery {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// One client connection: token handshake, then request/response lines.
fn serve(
    stream: TcpStream,
    token: &str,
    ctx: &egui::Context,
    connections: &AtomicUsize,
    sender: &mpsc::Sender<Incoming>,
    stop: &AtomicBool,
) {
    let _ = stream.set_nonblocking(false);
    let Ok(mut writer) = stream.try_clone() else {
        return;
    };
    let mut lines = BufReader::new(stream).lines();
    let authorized = lines
        .next()
        .and_then(Result::ok)
        .and_then(|line| serde_json::from_str::<Json>(&line).ok())
        .is_some_and(|hello| hello["token"].as_str() == Some(token));
    if !authorized {
        let _ = writeln!(writer, "{}", json!({"error": "Token inválido."}));
        return;
    }
    let _ = writeln!(
        writer,
        "{}",
        json!({"ok": true, "editor": env!("OXY_APP_VERSION")})
    );
    connections.fetch_add(1, Ordering::Relaxed);
    ctx.request_repaint();
    for line in lines {
        let Ok(line) = line else {
            break;
        };
        if stop.load(Ordering::Relaxed) {
            break;
        }
        if line.trim().is_empty() {
            continue;
        }
        let request: Json = match serde_json::from_str(&line) {
            Ok(request) => request,
            Err(error) => {
                let _ = writeln!(
                    writer,
                    "{}",
                    json!({"error": format!("JSON inválido: {error}")})
                );
                continue;
            }
        };
        let id = request["id"].clone();
        let (reply, answer) = mpsc::channel();
        let incoming = Incoming {
            method: request["method"].as_str().unwrap_or_default().to_owned(),
            params: request.get("params").cloned().unwrap_or(Json::Null),
            reply,
        };
        if sender.send(incoming).is_err() {
            break;
        }
        ctx.request_repaint();
        let response = match answer.recv_timeout(REPLY_TIMEOUT) {
            Ok(Ok(result)) => json!({"id": id, "result": result}),
            Ok(Err(error)) => json!({"id": id, "error": error}),
            Err(_) => json!({"id": id, "error": "O editor não respondeu a tempo."}),
        };
        if writeln!(writer, "{response}").is_err() {
            break;
        }
    }
    connections.fetch_sub(1, Ordering::Relaxed);
    ctx.request_repaint();
}

/// Requests that change the project, and therefore wait for a person's gesture to end.
fn edits(method: &str) -> bool {
    matches!(
        method,
        "apply" | "import" | "undo" | "redo" | "select" | "frame"
    )
}

impl Editor {
    /// Starts or stops the channel with the preference, advertises the open project and
    /// answers queued requests. Called once per frame.
    pub(crate) fn agent_frame(&mut self, ctx: &egui::Context) {
        // Before anything else this frame, so the lines of the last frame's test are kept.
        self.sync_console_feed();
        if self.preferences.allow_agents != self.agents.is_some() {
            self.agents = if self.preferences.allow_agents {
                match AgentChannel::start(ctx) {
                    Ok(channel) => Some(channel),
                    Err(error) => {
                        self.warn(format!(
                            "Não foi possível abrir o canal para agentes de IA: {error}"
                        ));
                        self.preferences.allow_agents = false;
                        None
                    }
                }
            } else {
                None
            };
        }
        let project = (!self.home.visible).then(|| {
            (
                self.state.project.name.clone(),
                self.path.clone().filter(|_| !self.home.example_copy),
            )
        });
        let Some(channel) = self.agents.as_mut() else {
            return;
        };
        channel.advertise(project.as_ref().map(|(n, p)| (n.as_str(), p.as_deref())));
        while let Ok(request) = channel.requests.try_recv() {
            channel.queue.push_back(request);
        }
        loop {
            let Some(front) = self.agents.as_ref().and_then(|c| c.queue.front()) else {
                break;
            };
            if edits(&front.method) && self.person_editing(ctx) {
                ctx.request_repaint_after(Duration::from_millis(100));
                break;
            }
            let request = self.agents.as_mut().unwrap().queue.pop_front().unwrap();
            let result = self.agent_request(ctx, &request.method, &request.params);
            let _ = request.reply.send(result);
        }
    }
    pub(crate) fn agent_connected(&self) -> bool {
        self.agents.as_ref().is_some_and(AgentChannel::connected)
    }
    fn agent_scene(&self, params: &Json) -> Result<Id, String> {
        if self.home.visible {
            return Err("Nenhum projeto aberto no editor.".into());
        }
        Ok(params["scene"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| self.scene_id.clone()))
    }
    fn agent_request(
        &mut self,
        ctx: &egui::Context,
        method: &str,
        params: &Json,
    ) -> Result<Json, String> {
        match method {
            "status" => Ok(json!({
                "editor": env!("OXY_APP_VERSION"),
                "project": (!self.home.visible).then(|| json!({
                    "name": self.state.project.name,
                    "path": self.path.as_ref().map(|p| p.display().to_string()),
                    "unsaved_changes": self.dirty(),
                })),
                "active_scene": (!self.home.visible).then(|| self.scene_id.clone()),
                "mode": match self.tab {
                    Tab::Scene => "scene",
                    Tab::Studio => "studio",
                    Tab::Logic => "logic",
                },
                "selection": self.selection.ids,
                "busy": self.agent_busy(),
                "undo": self.history.undo_label(),
                "redo": self.history.redo_label(),
                // A test the person is running: edits are refused until it stops.
                "play": match &self.play.runtime {
                    Some(runtime) => json!({
                        "running": true,
                        "paused": runtime.paused,
                        "scene": {"id": runtime.scene_id(), "name": runtime.scene().name},
                        "time": runtime.time,
                        "seed": self.play.seed,
                    }),
                    // The last test's seed stays for the person's report after it stops.
                    None => match self.play.seed {
                        Some(seed) => json!({"running": false, "seed": seed}),
                        None => json!({"running": false}),
                    },
                },
            })),
            "console" => self.console_answer(params),
            "document" => {
                self.agent_scene(params)?;
                serde_json::to_value(&self.state.project).map_err(|e| e.to_string())
            }
            // The same answers, with the same arguments, as the MCP server gives on files.
            "summary" | "scene_tree" | "entity" | "find" => {
                let scene = self.agent_scene(params)?;
                query::answer(&self.state.project, &scene, method, params)
            }
            "catalog" => query::catalog_query(params),
            "validate" => {
                self.agent_scene(params)?;
                Ok(query::validation(&self.state.project))
            }
            "apply" => {
                let commands = command::parse(&params["commands"])?;
                self.execute_agent(&commands)
            }
            "import" => {
                let scene = self.agent_scene(params)?;
                let request = oxy_core::asset_import::ImportRequest::parse(params)?;
                self.import_agent(&request, &scene)
            }
            "undo" | "redo" => {
                let label = self.agent_undo(method == "redo")?;
                Ok(json!({"done": label.is_some(), "label": label}))
            }
            "select" => {
                let scene = self.agent_scene(params)?;
                if let Some(reason) = self.agent_busy() {
                    return Err(reason.into());
                }
                let ids: Vec<Id> = serde_json::from_value(params["ids"].clone())
                    .map_err(|_| "Informe ids como uma lista de IDs".to_owned())?;
                if scene != self.scene_id {
                    if self.state.project.scene(&scene).is_none() {
                        return Err(format!("Cena {scene} não existe"));
                    }
                    self.set_scene(scene);
                }
                if let Some(missing) = ids.iter().find(|id| self.scene().entity(id).is_none()) {
                    return Err(format!("Objeto {missing} não existe nesta cena"));
                }
                self.select(ids.last().cloned());
                self.selection.ids = ids;
                Ok(json!({"selected": self.selection.ids}))
            }
            "screenshot" => {
                self.agent_scene(params)?;
                let colliders = params["colliders"].as_bool() == Some(true);
                let (pixels, size) = if colliders {
                    self.agent_capture_colliders(ctx.pixels_per_point())?
                } else {
                    self.renderer.read_pixels(&self.render_state)?
                };
                let png = oxy_render::offscreen::encode_png(pixels, size)?;
                Ok(json!({
                    "png_base64": oxy_core::encoding::base64(&png),
                    "width": size[0],
                    "height": size[1],
                    "view": if self.showing_game() { "game" } else { "editor_viewport" },
                    "colliders": colliders,
                }))
            }
            "frame" => {
                self.agent_scene(params)?;
                self.frame_selection();
                Ok(json!({"done": true}))
            }
            other => Err(format!("Pedido desconhecido: {other}")),
        }
    }
    /// The view on screen drawn again with the collider view on top, as with the debug view
    /// on: the game camera while the game runs, the editor camera otherwise (without the
    /// Studio's modeling marks). The frame shown to the person is not touched.
    fn agent_capture_colliders(
        &mut self,
        pixels_per_point: f32,
    ) -> Result<(Vec<u8>, [u32; 2]), String> {
        use oxy_render::offscreen::ColliderOverlay;
        let size = self.renderer.target_size();
        let root = self.root();
        let show_disabled = self.view.show_disabled_colliders;
        if self.showing_game()
            && let Some(runtime) = &self.play.runtime
        {
            self.renderer.show_grid = false;
            return self.renderer.capture(
                &self.render_state,
                &self.state.project,
                runtime.scene(),
                &root,
                &CameraState::for_runtime(runtime),
                size,
                None,
                Some(ColliderOverlay {
                    selected: &[],
                    show_disabled,
                    runtime_world: runtime.physics_world(),
                    pixels_per_point,
                }),
            );
        }
        let scene = self.view_scene();
        self.renderer.show_grid = self.view.grid;
        self.renderer.capture(
            &self.render_state,
            &self.state.project,
            &scene,
            &root,
            &self.camera,
            size,
            self.selected.clone(),
            Some(ColliderOverlay {
                selected: &self.selection.ids,
                show_disabled,
                runtime_world: None,
                pixels_per_point,
            }),
        )
    }
}
