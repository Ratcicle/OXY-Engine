//! Edits requested by AI agents. Each batch of project commands, and each import of PNGs or WAVs,
//! is one undo step labelled "Agente: …", logged in the console with the Agente origin, and
//! refused while a person is in the middle of something the edit could collide with. Agents
//! also play tests of the game without taking the mouse (see [`Driver`]), and save when the
//! person allowed it in Preferências do editor.
use super::*;
use oxy_core::{
    asset_import::{self, ImportBatch, ImportKind},
    command::{self, Command},
    headless::{self, InputEvent},
    script_files,
};
use serde_json::{Value as Json, json};

/// Console prefix of messages about agent edits; the console shows it as the line's origin.
pub(crate) const AGENT_PREFIX: &str = "Agente · ";

const NO_TEST: &str = "Não há teste do jogo em andamento no editor.";

/// The scripts an agent's batch for the project saved at `path` reads: those of its tree, read
/// from the disk again, when a command attaches one (see [`command::needs_scripts`]), so a
/// script the agent just wrote with its own file tools is there; else none. A project never
/// saved has no folder for them.
fn agent_scripts(path: Option<&Path>, commands: &[Command]) -> Result<ScriptSources, String> {
    if !command::needs_scripts(commands) {
        return Ok(ScriptSources::default());
    }
    match path.and_then(Path::parent) {
        Some(root) => persistence::load_scripts(root).map_err(|error| error.to_string()),
        None => Err(
            "O projeto ainda não foi salvo numa pasta, onde ficam os seus scripts; \
             salve-o antes de anexar um script."
                .into(),
        ),
    }
}

impl PlaySession {
    /// The game time the inputs an agent sent still wait until, when some wait.
    pub(crate) fn queued_until(&self) -> Option<f64> {
        let timed = self.runtime.as_ref().and_then(Runtime::timed_input_end);
        let direct = self.agent_clicks.back().map(|(at, _)| *at);
        timed.into_iter().chain(direct).reduce(f64::max)
    }
    /// Queues an agent's inputs, in oxy_simulate's form, with `at` counted from the game's clock
    /// or, while inputs sent before still wait, from the last of them: batches follow one
    /// another. Presses, releases, movement and look go to the runtime's timed inputs; clicks
    /// and Esc wait here for their time. Actions are found in the game that runs, which undo
    /// in the editor does not change. All of the batch is queued, or none of it; refused while
    /// the test is paused, which drops them.
    pub(crate) fn queue_agent_inputs(&mut self, inputs: &[InputEvent]) -> Result<(), String> {
        let waiting = self.queued_until();
        let Some(runtime) = &mut self.runtime else {
            return Err(NO_TEST.into());
        };
        if runtime.paused {
            return Err("O teste está pausado: retome-o (resume) antes de mandar entradas.".into());
        }
        let start = waiting.map_or(runtime.time, |at| at.max(runtime.time));
        let (timed, directs) = headless::plan_inputs(runtime.project(), inputs, &[], start, None)?;
        runtime.queue_timed_inputs(timed)?;
        self.agent_clicks.extend(directs);
        Ok(())
    }
}

impl Editor {
    /// Why the editor cannot take agent edits right now, if it cannot.
    pub(crate) fn agent_busy(&self) -> Option<&'static str> {
        if self.home.visible {
            Some("Nenhum projeto aberto no editor.")
        } else if self.playing() {
            Some("O jogo está em teste no editor. Pare o teste para editar.")
        } else if self.mesh_operation_active() {
            Some("Há uma operação de modelagem em prévia no editor.")
        } else if self.studio.playing || !self.studio.animation.drafts.is_empty() {
            Some("Há uma animação tocando ou uma pose provisória no Estúdio.")
        } else if self.pending.is_some() {
            Some("O editor está esperando uma decisão sobre alterações não salvas.")
        } else {
            None
        }
    }
    /// A person is dragging, typing or has an edit in progress: agent edits wait for it.
    pub(crate) fn person_editing(&self, ctx: &egui::Context) -> bool {
        self.history.is_pending()
            || ctx.input(|i| i.pointer.any_down())
            || crate::graph_ui::text_input_active(ctx)
    }
    /// Applies an agent's batch as one undo step and answers with [`command::report`], as the
    /// MCP server does on files. On failure nothing changes.
    pub(crate) fn execute_agent(&mut self, commands: &[Command]) -> Result<Json, String> {
        if let Some(reason) = self.agent_busy() {
            return Err(reason.into());
        }
        if commands.is_empty() {
            // No undo step for nothing; the answer leaves out the project's warnings, as on
            // files.
            let before = command::batch_warnings(&self.state.project);
            return Ok(command::report(&self.state.project, &[], &before));
        }
        let label = match commands {
            [one] => format!("Agente: {}", one.label()),
            many => format!("Agente: {} comandos", many.len()),
        };
        let root = self.project_root();
        // A project never saved has no folder: attaching a script is refused, and the tree's
        // commands say they need one.
        let scripts = match root {
            Some(_) => ScriptSources::default(),
            None => agent_scripts(None, commands)?,
        };
        // A script or a file the agent just wrote, moved or deleted with its own tools is
        // where the batch looks for it.
        if root.is_some() && command::needs_files(&self.state.project, commands) {
            self.scan_project();
        }
        let known = root.as_deref().map(|root| {
            persistence::missing_scripts(&self.state.project, root).unwrap_or_default()
        });
        self.history
            .begin(label.clone(), &self.state.project, &self.state.images);
        let applied = match &root {
            Some(root) => command::apply_on_files(
                &mut self.state.project,
                &self.scene_id,
                commands,
                root,
                &mut self.known_files,
                &label,
            )
            .map(|done| (done.report, done.files)),
            None => command::apply_and_report_with_scripts(
                &mut self.state.project,
                &self.scene_id,
                commands,
                &scripts,
            )
            .map(|report| (report, Vec::new()))
            .map_err(|error| command::FilesError {
                message: error.to_string(),
                kept: Vec::new(),
            }),
        };
        match applied {
            Ok((mut report, files)) => {
                if let (Some(root), Some(known)) = (&root, &known) {
                    let added =
                        persistence::added_script_warnings(known, &self.state.project, root);
                    command::add_warnings(&mut report, added);
                }
                let moved = !files.is_empty();
                if let Err(error) = self.history.commit_with_files(
                    &self.state.project,
                    &mut self.state.images,
                    files,
                ) {
                    self.log(error);
                }
                if moved {
                    self.rebase_scan();
                }
                for command in commands {
                    self.log(format!("{AGENT_PREFIX}{}", command.label()));
                }
                // Whoever writes the scripts in VS Code gets the API's types.
                if let Some(root) = root.as_deref().filter(|_| command::needs_scripts(commands)) {
                    match script_files::setup_vscode_when_due(root) {
                        Ok(Some(setup)) => {
                            report["vscode"] = setup.json();
                            for line in setup.console() {
                                self.log(format!("{AGENT_PREFIX}{line}"));
                            }
                        }
                        Ok(None) => {}
                        Err(error) => command::add_warnings(&mut report, vec![error]),
                    }
                }
                self.after_external_edit();
                Ok(report)
            }
            Err(command::FilesError { message, kept }) => {
                self.keep_files_or_cancel(kept);
                self.log(format!("{AGENT_PREFIX}{message}"));
                Err(message)
            }
        }
    }
    /// Ends the step begun for a batch that failed: cancelled, or, when files stayed moved
    /// because undoing them failed too (the document follows them), kept with them, so undoing
    /// puts them back.
    pub(crate) fn keep_files_or_cancel(&mut self, kept: Vec<oxy_core::edit_history::FileOp>) {
        if kept.is_empty() {
            self.history
                .cancel(&mut self.state.project, &mut self.state.images);
            return;
        }
        if let Err(error) =
            self.history
                .commit_with_files(&self.state.project, &mut self.state.images, kept)
        {
            self.log(error);
        }
        self.rebase_scan();
        self.after_external_edit();
    }
    /// An agent's oxy_scripts on the project open in the editor, whose folder holds its scripts.
    /// The disk is the truth: a write is no undo step, waits for nothing and reaches a test
    /// running through its watcher; writes and setting VS Code up are told in the Console.
    pub(crate) fn agent_script_files(&mut self, params: &Json) -> Result<Json, String> {
        if self.home.visible {
            return Err("Nenhum projeto aberto no editor.".into());
        }
        let request = script_files::Request::parse(params)?;
        let Some(root) = self
            .path
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_owned)
        else {
            return Err(
                "O projeto ainda não foi salvo numa pasta, onde ficam os seus scripts; peça \
                 para a pessoa salvá-lo antes."
                    .into(),
            );
        };
        let project = Some(&self.state.project);
        match script_files::answer(&root, project, &request, &mut self.agent_scripts_kept) {
            Ok(answer) => {
                // Written through OXY: undoing a move of the file takes its new stamp.
                if let script_files::Request::Write { path, .. } = &request
                    && self.known_files.stamp(path).is_some()
                {
                    self.known_files.note_stamp(&root, path);
                }
                for line in answer.console {
                    self.log(format!("{AGENT_PREFIX}{line}"));
                }
                Ok(answer.json)
            }
            Err(error) => {
                if matches!(request, script_files::Request::Write { .. }) {
                    self.log(format!("{AGENT_PREFIX}{error}"));
                }
                Err(error)
            }
        }
    }
    /// Imports PNGs or WAVs for an agent, as the editor's Importar does but without the dialog:
    /// one undo step, for every file of the batch, that carries the images, so the texture
    /// cache holds their pixels, the viewport and the game interface show them at once, and
    /// undo takes them away again. `scene` is where `apply_to` is looked for.
    pub(crate) fn import_agent(
        &mut self,
        batch: &ImportBatch,
        scene: &str,
    ) -> Result<Json, String> {
        if let Some(reason) = self.agent_busy() {
            return Err(reason.into());
        }
        if self.path.is_none() {
            return Err(
                "O projeto ainda não tem pasta: peça para a pessoa salvá-lo antes de importar \
                 recursos."
                    .into(),
            );
        }
        let sources = match batch.read() {
            Ok(sources) => sources,
            Err(error) => {
                self.log(format!("{AGENT_PREFIX}{error}"));
                return Err(error);
            }
        };
        let root = self.root();
        let label = match batch.requests.len() {
            1 => "Agente: Importar recurso".to_owned(),
            count => format!("Agente: Importar {count} recursos"),
        };
        // What changed outside OXY is told before the walk takes the import's files.
        self.scan_project();
        self.history
            .begin(label.clone(), &self.state.project, &self.state.images);
        // In a project in folders each file it makes goes with the step (see
        // `journal::Creating`).
        let mut creating = (self.state.project.layout == Layout::Tree)
            .then(|| oxy_core::journal::Creating::new(&root, &mut self.known_files, &label));
        let imported = asset_import::import_batch_with(
            &mut self.state.project,
            &root,
            scene,
            batch,
            &sources,
            &mut |path, id| creating.as_mut().map_or(Ok(()), |c| c.before(path, id)),
        );
        let files = creating
            .map(|creating| creating.finish(imported.is_ok()))
            .unwrap_or_default();
        match imported {
            Ok(imported) => {
                for (done, source) in imported.iter().zip(sources) {
                    if let Some(image) = source.image {
                        self.state.images.insert(done.asset.clone(), image);
                    }
                }
                let made = !files.is_empty();
                if let Err(error) = self.history.commit_with_files(
                    &self.state.project,
                    &mut self.state.images,
                    files,
                ) {
                    self.log(error);
                }
                if made {
                    self.rebase_scan();
                }
                for done in &imported {
                    self.log(format!(
                        "{AGENT_PREFIX}Recurso importado como cópia local: {}",
                        done.name
                    ));
                }
                self.after_external_edit();
                for done in &imported {
                    if done.kind == ImportKind::Texture {
                        self.refresh_texture(&done.asset);
                    }
                }
                batch.answer(&imported)
            }
            Err(error) => {
                self.history
                    .cancel(&mut self.state.project, &mut self.state.images);
                self.log(format!("{AGENT_PREFIX}{error}"));
                Err(error)
            }
        }
    }
    /// Keeps the active scene, selection and textures consistent after an edit that did not
    /// come from the panels.
    fn after_external_edit(&mut self) {
        if self.state.project.scene(&self.scene_id).is_none() {
            let start = self.state.project.start_scene.clone();
            self.set_scene(start);
        }
        if self
            .selected
            .as_ref()
            .is_some_and(|id| self.scene().entity(id).is_none())
        {
            self.select(None);
        }
        let scene = self.scene().clone();
        self.selection.retain_scene(&scene);
        self.state
            .images
            .configure(&self.root(), &self.state.project);
    }
    /// An agent's `play` request: start (in `scene`, by its ID or name, else the open one, as
    /// Jogar), stop, pause, resume or input. Its test never takes the mouse or the keyboard. The
    /// agent may pause any test, but stops, resumes and sends inputs only to the one it plays:
    /// the person's, theirs from the start or since Assumir or Retomar, stays theirs. Answers
    /// the play state, as `status` gives it.
    pub(crate) fn agent_play(&mut self, params: &Json) -> Result<Json, String> {
        let action = params["action"].as_str().unwrap_or_default();
        if self.home.visible {
            return Err("Nenhum projeto aberto no editor.".into());
        }
        let paused = self.play.runtime.as_ref().map(|rt| rt.paused);
        let person = paused.is_some() && self.play.driver == Driver::Person;
        let refusal = match action {
            "stop" if person => {
                Some("O teste é da pessoa: só ela pode pará-lo (Parar, na barra do editor).")
            }
            "resume" if person => {
                Some("O teste é da pessoa: só ela pode retomá-lo (Retomar, na barra do editor).")
            }
            "input" if person => Some("O teste é da pessoa: só ela manda entradas a ele."),
            _ => None,
        };
        if let Some(refusal) = refusal {
            return Err(refusal.into());
        }
        match (action, paused) {
            ("start", Some(_)) => {
                return Err("Já há um teste do jogo em andamento no editor.".into());
            }
            ("start", None) => {
                if let Some(reason) = self.agent_busy() {
                    return Err(reason.into());
                }
                let scene = params["scene"].as_str().unwrap_or(&self.scene_id);
                let found = self.state.project.find_scene(scene, "scene")?;
                let (scene, name) = (found.id.clone(), found.name.clone());
                self.start_with(&scene, Driver::Agent)?;
                self.log(format!("{AGENT_PREFIX}Teste iniciado em {name}"));
            }
            ("stop" | "pause" | "resume" | "input", None) => return Err(NO_TEST.into()),
            ("stop", Some(_)) => {
                self.stop();
                self.log(format!("{AGENT_PREFIX}Teste parado"));
            }
            ("pause", Some(paused)) => {
                if !paused {
                    self.pause();
                    self.log(format!("{AGENT_PREFIX}Teste pausado"));
                }
            }
            ("resume", Some(false)) => return Err("O teste não está pausado.".into()),
            ("resume", Some(true)) => {
                self.resume_for(Driver::Agent);
                self.log(format!("{AGENT_PREFIX}Teste retomado"));
            }
            ("input", Some(_)) => {
                let inputs: Vec<InputEvent> = serde_json::from_value(params["inputs"].clone())
                    .map_err(|e| format!("Entradas inválidas: {e}"))?;
                self.play.queue_agent_inputs(&inputs)?;
            }
            _ => {
                return Err(format!(
                    "action `{action}` desconhecida; use start, stop, pause, resume ou input."
                ));
            }
        }
        Ok(self.play_status())
    }
    /// Why an agent may not save now, if it may not: the person did not allow it, the editor is
    /// busy, or saving would ask the person for a folder.
    pub(crate) fn save_refusal(&self) -> Option<&'static str> {
        if !self.preferences.allow_agents || !self.preferences.allow_agent_save {
            Some(
                "A pessoa não permitiu que agentes salvem: Permitir que agentes salvem está \
                 desligado em Preferências do editor.",
            )
        } else if let Some(reason) = self.agent_busy() {
            Some(reason)
        } else if self.path.is_none() {
            Some("O projeto ainda não tem pasta: a pessoa precisa salvá-lo pela primeira vez.")
        } else if self.home.example_copy {
            Some("O projeto é uma cópia de exemplo: a pessoa precisa salvá-lo numa pasta.")
        } else {
            None
        }
    }
    /// Saves for an agent, as Ctrl+S does, everything unsaved included, after keeping the
    /// file as it was before this save as project.oxy.json.agente.bak (the MCP server on files
    /// keeps the one from before its session's first write).
    #[expect(
        clippy::disallowed_methods,
        reason = "the agent's copy of the project file is OXY's own, kept fresh at each save"
    )]
    pub(crate) fn agent_save(&mut self) -> Result<Json, String> {
        if let Some(reason) = self.save_refusal() {
            return Err(reason.into());
        }
        let path = self.path.clone().expect("Projeto com pasta");
        let backup = path.with_extension("json.agente.bak");
        std::fs::copy(&path, &backup)
            .map_err(|e| format!("Não foi possível guardar a versão anterior: {e}"))?;
        let written = self.console.written();
        if !self.save_from(AGENT_PREFIX) {
            // Save says why in the Console.
            return Err(self
                .console
                .messages
                .last()
                .filter(|_| self.console.written() > written)
                .cloned()
                .unwrap_or_else(|| "Não foi possível salvar.".into()));
        }
        let mut answer = json!({
            "saved": path.display().to_string(),
            "previous_version": backup.display().to_string(),
        });
        // The first save of a project of the old layout organized it in folders, or why not.
        if let Some(converted) = self.converted.take() {
            answer["converted"] = converted;
        }
        if let Some(kept) = self.kept_layout.take() {
            answer["warnings"] = json!([kept]);
        }
        Ok(answer)
    }
    /// Undo or redo on an agent's request, with the same rules as Ctrl+Z.
    pub(crate) fn agent_undo(&mut self, redo: bool) -> Result<Option<String>, String> {
        if let Some(reason) = self.agent_busy() {
            return Err(reason.into());
        }
        let label = if redo {
            self.history.redo_label()
        } else {
            self.history.undo_label()
        }
        .map(str::to_owned);
        if label.is_none() {
            return Ok(None);
        }
        // Moving files back can be refused (a file open in another program, one changed
        // outside OXY): the agent hears it.
        self.undo_step(redo)?;
        self.log(format!(
            "{AGENT_PREFIX}{} {}",
            if redo { "Refazer" } else { "Desfazer" },
            label.as_deref().unwrap_or_default()
        ));
        Ok(label)
    }
}

impl Editor {
    /// Status bar mark: agents allowed, or an agent connected right now.
    pub(crate) fn agent_indicator(&self, ui: &mut egui::Ui) {
        if !self.preferences.allow_agents {
            return;
        }
        let connected = self.agent_connected();
        let playing = self.playing() && self.play.driver == Driver::Agent;
        let color = if connected || playing {
            Color32::from_rgb(190, 160, 240)
        } else {
            crate::theme::TEXT_MUTED
        };
        let text = if playing {
            "Agente jogando"
        } else if connected {
            "Agente conectado"
        } else {
            "Agentes ligados"
        };
        ui.add_space(4.);
        ui.label(egui::RichText::new(text).size(12.).color(color))
            .on_hover_text(if playing {
                "Um agente de IA está testando o jogo e envia as entradas. Clique em Assumir, na \
                 barra de cima, para jogar você mesmo."
            } else if connected {
                "Um agente de IA está conectado pelo servidor MCP. As mudanças dele aparecem                  no Console e podem ser desfeitas com Ctrl+Z."
            } else {
                "Agentes de IA podem se conectar pelo servidor MCP. Desligue em Preferências do                  editor."
            });
        let (dot, _) = ui.allocate_exact_size(Vec2::splat(10.), Sense::hover());
        ui.painter().circle_filled(dot.center(), 3.5, color);
    }
}

#[cfg(test)]
impl Editor {
    pub(crate) fn qa_allow_agents(&mut self, on: bool) {
        self.preferences.allow_agents = on;
    }
    /// Permitir agentes and Permitir que agentes salvem, as applied.
    pub(crate) fn qa_agent_preferences(&self) -> (bool, bool) {
        (
            self.preferences.allow_agents,
            self.preferences.allow_agent_save,
        )
    }
    /// Treats the open project as an example copy, which Salvar would ask a folder for.
    pub(crate) fn qa_example_copy(&mut self, on: bool) {
        self.home.example_copy = on;
    }
    pub(crate) fn qa_undo_label(&self) -> Option<&str> {
        self.history.undo_label()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxy_core::{headless::Direct, input_actions, runtime::InputFrame};
    #[test]
    fn agent_messages_carry_the_console_origin() {
        assert!(format!("{AGENT_PREFIX}Criar objeto").starts_with("Agente · "));
    }

    #[test]
    fn only_a_batch_that_attaches_a_script_reads_the_folder() {
        let root = std::env::temp_dir().join(format!("oxy-editor-agent-scripts-{}", new_id()));
        std::fs::create_dir_all(root.join("scripts")).unwrap();
        std::fs::write(root.join("scripts/porta.luau"), "return {}\n").unwrap();
        let path = root.join(persistence::PROJECT_FILE);
        let door = new_id();
        let attach = [Command::AttachScript {
            scene: None,
            entity: door.clone(),
            path: "scripts/porta.luau".into(),
            index: None,
        }];
        let detach = [Command::DetachScript {
            scene: None,
            entity: door,
            path: "scripts/porta.luau".into(),
        }];
        let read = agent_scripts(Some(&path), &attach).unwrap();
        assert_eq!(
            read.files.keys().collect::<Vec<_>>(),
            ["scripts/porta.luau"]
        );
        assert!(
            agent_scripts(Some(&path), &detach)
                .unwrap()
                .files
                .is_empty()
        );
        // A project never saved has no folder: attaching is refused, the rest goes on.
        let error = agent_scripts(None, &attach).unwrap_err();
        assert!(error.contains("ainda não foi salvo"), "{error}");
        assert!(agent_scripts(None, &detach).unwrap().files.is_empty());
        // A file that cannot be read keeps none of the others from the batch: attaching it says
        // why; and a script the agent wrote in any folder is read at once.
        std::fs::write(root.join("scripts/binario.luau"), [0xff, 0xfe]).unwrap();
        std::fs::create_dir_all(root.join("Pista")).unwrap();
        std::fs::write(root.join("Pista/marco.luau"), "return {}\n").unwrap();
        let read = agent_scripts(Some(&path), &attach).unwrap();
        assert_eq!(
            read.files.keys().collect::<Vec<_>>(),
            ["Pista/marco.luau", "scripts/porta.luau"]
        );
        assert_eq!(
            read.absent("scripts/binario.luau"),
            "o script scripts/binario.luau não é texto UTF-8"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn agent_inputs_queue_up_from_the_game_clock_and_need_a_running_test() {
        let mut project = Project::new("Entradas");
        input_actions::create(&mut project, "Abrir porta", "O").unwrap();
        let door = Entity::new("Porta", None);
        let door_id = door.id.clone();
        project.scenes[0].entities.push(door);
        let scene = project.start_scene.clone();
        let inputs = |value: Json| serde_json::from_value::<Vec<InputEvent>>(value).unwrap();
        let mut play = PlaySession {
            mixer: oxy_audio::Mixer::silent(),
            ..Default::default()
        };
        let escape = inputs(json!([{"type": "escape", "at": 0.0}]));
        let error = play.queue_agent_inputs(&escape).unwrap_err();
        assert!(error.contains("Não há teste"), "{error}");
        play.runtime = Some(Runtime::with_options(&project, &scene, play.options(1)).unwrap());
        play.driver = Driver::Agent;
        for _ in 0..30 {
            let rt = play.runtime.as_mut().unwrap();
            rt.advance(oxy_core::runtime::FIXED_DT, &InputFrame::default());
        }
        let now = play.runtime.as_ref().unwrap().time;
        assert!(now > 0.4);
        // The door by name, as oxy_simulate takes it: queued by its ID.
        play.queue_agent_inputs(&inputs(json!([
            {"type": "press", "at": 0.25, "action": "abrir porta"},
            {"type": "click", "at": 0.5, "entity": "porta"},
        ])))
        .unwrap();
        assert_eq!(play.queued_until(), Some(now + 0.5));
        assert_eq!(
            play.runtime.as_ref().unwrap().timed_input_end(),
            Some(now + 0.25)
        );
        // A later batch follows what still waits, rather than going back to now.
        play.queue_agent_inputs(&inputs(
            json!([{"type": "release", "at": 0.0, "action": "Abrir porta"}]),
        ))
        .unwrap();
        assert_eq!(
            play.runtime.as_ref().unwrap().timed_input_end(),
            Some(now + 0.5)
        );
        assert_eq!(
            play.agent_clicks.iter().cloned().collect::<Vec<_>>(),
            [(now + 0.5, Direct::Click(door_id.clone()))]
        );
        let error = play
            .queue_agent_inputs(&inputs(
                json!([{"type": "press", "at": 0.0, "action": "voar"}]),
            ))
            .unwrap_err();
        assert!(error.contains("\"voar\" não existe"), "{error}");
        let error = play
            .queue_agent_inputs(&inputs(
                json!([{"type": "click", "at": 0.0, "entity": "Janela"}]),
            ))
            .unwrap_err();
        assert!(
            error.contains("\"Janela\" não é o ID nem o nome de um objeto"),
            "{error}"
        );
        // Actions are those of the game that runs, whatever the document gains meanwhile.
        input_actions::create(&mut project, "Acenar", "K").unwrap();
        let error = play
            .queue_agent_inputs(&inputs(
                json!([{"type": "press", "at": 0.0, "action": "Acenar"}]),
            ))
            .unwrap_err();
        assert!(error.contains("\"Acenar\" não existe"), "{error}");
        // A batch with one bad input queues none of it.
        let error = play
            .queue_agent_inputs(&inputs(json!([
                {"type": "click", "at": 0.0, "entity": door_id},
                {"type": "press", "at": 0.1, "action": "Abrir porta"},
                {"type": "movement", "at": 0.2, "value": [1e39, 0.0]},
            ])))
            .unwrap_err();
        assert!(error.contains("finitos"), "{error}");
        assert_eq!(play.queued_until(), Some(now + 0.5));
        assert_eq!(play.agent_clicks.len(), 1);
        // A pause drops what waits in the runtime, and inputs wait for the test to resume.
        play.runtime.as_mut().unwrap().set_paused(true);
        assert_eq!(play.runtime.as_ref().unwrap().timed_input_end(), None);
        let error = play.queue_agent_inputs(&escape).unwrap_err();
        assert!(error.contains("pausado"), "{error}");
        // Parar ends the agent's test.
        play.end();
        assert!(play.agent_clicks.is_empty() && play.driver == Driver::Person);
        assert_eq!(play.queued_until(), None);
    }
}
