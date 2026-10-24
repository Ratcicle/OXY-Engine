//! What the Library does besides drawing: importing into a folder, running the tree's commands
//! as one undo step (the selection, the open folder and the code tab following the files they
//! move), attaching a script to the selected objects, placing a model, applying an image,
//! hearing a sound, pointing a missing item at the file the person picks, and showing a file in
//! the Explorer.
use super::*;

impl Editor {
    /// Imports PNGs and WAVs the person picks into the folder `folder`, as one undo step; the
    /// imported items are selected.
    pub(crate) fn import_into(&mut self, folder: &str) {
        self.pause();
        if self.path.is_none() && !self.save() {
            return;
        }
        let Some(files) = rfd::FileDialog::new()
            .set_title("Importar imagens e sons")
            .add_filter("Imagem PNG ou som WAV", &["png", "wav"])
            .pick_files()
        else {
            return;
        };
        let sources: Vec<(PathBuf, AssetKind)> = files
            .into_iter()
            .filter_map(|file| {
                let extension = file.extension()?.to_string_lossy().to_ascii_lowercase();
                match extension.as_str() {
                    "png" => Some((file, AssetKind::Texture)),
                    "wav" => Some((file, AssetKind::Audio)),
                    _ => None,
                }
            })
            .collect();
        let imported = self.import_files(&sources, folder, false);
        if !imported.is_empty() {
            self.library.selection = imported.into_iter().map(Key::Asset).collect();
            self.library.reveal = self.library.selection.first().cloned();
        }
    }

    /// Runs `commands` from the Library as one undo step named `label`, with the files they
    /// move (see `command::apply_on_files`): what they told, or why they did not run (a batch
    /// that failed changes nothing).
    pub(crate) fn library_commands(
        &mut self,
        label: &str,
        commands: Vec<Command>,
    ) -> Result<Vec<String>, String> {
        if self.playing() && command::needs_files(&self.state.project, &commands) {
            return Err(PLAYING.into());
        }
        self.finish_history(true);
        let root = self.project_root();
        if root.is_some() {
            // What changed outside OXY is told before the step takes the files.
            self.scan_project();
        }
        self.history
            .begin(label, &self.state.project, &self.state.images);
        let applied = match &root {
            Some(root) => command::apply_on_files(
                &mut self.state.project,
                &self.scene_id,
                &commands,
                root,
                &mut self.known_files,
                label,
            )
            .map(|done| {
                let told: Vec<String> = done.report["warnings"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|w| w.as_str().map(plain))
                    .collect();
                (told, done.files)
            }),
            None => command::apply(&mut self.state.project, &self.scene_id, &commands)
                .map(|applied| {
                    let told = applied
                        .into_iter()
                        .flat_map(|a| a.warnings)
                        .map(|w| plain(&w))
                        .collect();
                    (told, Vec::new())
                })
                .map_err(|error| command::FilesError {
                    message: error.to_string(),
                    kept: Vec::new(),
                }),
        };
        match applied {
            Ok((told, files)) => {
                let moved = !files.is_empty();
                if let Err(error) = self.history.commit_with_files(
                    &self.state.project,
                    &mut self.state.images,
                    files.clone(),
                ) {
                    self.log(error);
                }
                if moved {
                    self.rebase_scan();
                    self.script_status.look_again();
                    self.follow_moves(&files);
                }
                self.state
                    .images
                    .configure(&self.root(), &self.state.project);
                self.asset_ui.thumbnail = None;
                for line in &told {
                    self.log(line.clone());
                }
                Ok(told)
            }
            Err(command::FilesError { message, kept }) => {
                self.keep_files_or_cancel(kept);
                Err(plain(&message))
            }
        }
    }

    /// The Library's places, its selection and the code tab follow what `files` moved.
    fn follow_moves(&mut self, files: &[FileOp]) {
        let follow = |key: &Key| -> Option<Key> {
            match key {
                Key::Asset(id) => Some(Key::Asset(id.clone())),
                Key::Script(path) => model::moved(path, files).map(Key::Script),
                Key::Folder(path) => model::moved(path, files).map(Key::Folder),
            }
        };
        self.library.selection = self.library.selection.iter().filter_map(follow).collect();
        self.library.anchor = self.library.anchor.as_ref().and_then(follow);
        self.library.new = self.library.new.iter().filter_map(follow).collect();
        if let Place::Folder(folder) = &self.library.place {
            self.library.place = Place::Folder(model::moved(folder, files).unwrap_or_default());
        }
        self.library.expanded = self
            .library
            .expanded
            .iter()
            .filter_map(|folder| model::moved(folder, files))
            .collect();
        let shown = self.code_view.as_ref().map(|view| view.path.clone());
        if let Some(path) = shown
            && let Some(to) = model::moved(&path, files)
            && to != path
        {
            self.code_moved(&to);
        }
    }

    /// Anexar: the script `path` on the selected objects that lack it, as one undo step.
    pub(super) fn attach_from_library(&mut self, path: &str) {
        self.library.focus = false;
        let name = model::file_name(path).to_owned();
        if self.selection.ids.is_empty() {
            self.warn(format!(
                "Selecione na Hierarquia os objetos que vão usar {name} e clique em Anexar de novo."
            ));
            return;
        }
        let targets: Vec<Id> = self
            .selection
            .ids
            .iter()
            .filter(|id| {
                self.scene()
                    .entity(id)
                    .is_some_and(|e| !e.scripts.iter().any(|s| s.path == path))
            })
            .cloned()
            .collect();
        if targets.is_empty() {
            self.warn(format!("{name} já está anexado aos objetos selecionados."));
            return;
        }
        let commands: Vec<Command> = targets
            .into_iter()
            .map(|entity| Command::AttachScript {
                scene: None,
                entity,
                path: path.to_owned(),
                index: None,
            })
            .collect();
        let scripts = match self.project_root() {
            Some(root) => match persistence::load_scripts(&root) {
                Ok(scripts) => scripts,
                Err(error) => {
                    self.warn(format!("Anexar script: {error}"));
                    return;
                }
            },
            None => ScriptSources::default(),
        };
        self.finish_history(true);
        self.history
            .begin("Anexar script", &self.state.project, &self.state.images);
        let scene = self.scene_id.clone();
        match command::apply_with_scripts(&mut self.state.project, &scene, &commands, &scripts) {
            Ok(applied) => {
                if let Err(error) = self
                    .history
                    .commit(&self.state.project, &mut self.state.images)
                {
                    self.log(error);
                }
                for warning in applied.into_iter().flat_map(|a| a.warnings) {
                    self.warn(plain(&warning));
                }
            }
            Err(error) => {
                self.history
                    .cancel(&mut self.state.project, &mut self.state.images);
                self.warn(format!("Anexar script: {}", error.message));
            }
        }
    }

    /// Colocar na cena: a placement of the model `id`, selected; the keys go back to the scene.
    pub(super) fn place_model(&mut self, id: &str) {
        self.library.focus = false;
        let scene_id = self.scene_id.clone();
        match self.state.project.instantiate_model(id, &scene_id) {
            Ok(id) => {
                if self.tab == Tab::Studio {
                    self.studio.focus = Some(id.clone());
                }
                self.select(Some(id));
            }
            Err(e) => self.log(e),
        }
    }

    /// Ouvir: the sound `id`, through the test's mixer, from the file the game plays.
    pub(super) fn hear_asset(&mut self, id: &str) {
        let Some(asset) = self.state.project.asset(id).cloned() else {
            return;
        };
        let heard = oxy_core::persistence::resolve_asset_path(&self.root(), &asset.path)
            .map_err(String::from)
            .and_then(|path| self.play.mixer.preview(&path, 0.6))
            .map_err(|error| format!("Som {}: {error}", asset.name));
        let errors: Vec<String> = heard
            .err()
            .into_iter()
            .chain(self.play.mixer.take_errors())
            .collect();
        for error in &errors {
            self.log(error.clone());
        }
        if !errors.is_empty() {
            self.notice_last(false);
        }
    }

    pub(super) fn apply_texture_asset(&mut self, asset: &Asset) {
        self.ensure_texture(&asset.id);
        if self.selected.is_none() {
            self.add_entity(Some(Primitive::Sprite), &asset.name);
        }
        let Some(id) = self.selected.clone() else {
            return;
        };
        if let Some(e) = self.scene_mut().entity_mut(&id) {
            e.material.texture = Some(asset.id.clone());
        }
        if let Some(p) = self.state.images.get(&asset.id) {
            let ratio = p.width as f32 / p.height as f32;
            if let Some(e) = self.scene_mut().entity_mut(&id)
                && e.primitive == Some(Primitive::Sprite)
            {
                e.dimensions[0] = e.dimensions[1] * ratio;
            }
        }
    }

    /// Religar: the missing item `key` takes the file `path` of the project's tree, as its own
    /// undo step.
    pub(super) fn relink(&mut self, key: &Key, path: &str) {
        let Some(root) = self.project_root() else {
            return;
        };
        if self.playing() {
            self.warn(PLAYING);
            return;
        }
        self.finish_history(true);
        self.history
            .begin("Religar arquivo", &self.state.project, &self.state.images);
        let relinked = match key {
            Key::Asset(id) => persistence::relink_asset(
                &mut self.state.project,
                &root,
                id,
                path,
                &mut self.known_files,
            ),
            Key::Script(from) => {
                persistence::relink_script(&mut self.state.project, &root, from, path)
            }
            Key::Folder(_) => Err(String::new()),
        };
        match relinked {
            Ok(line) => {
                if let Err(error) = self
                    .history
                    .commit(&self.state.project, &mut self.state.images)
                {
                    self.log(error);
                }
                self.log(line);
                match key {
                    Key::Asset(id) => {
                        self.library.near.remove(id);
                        self.library.report.remove(id);
                        self.state.images.remove(id);
                        self.state.images.configure(&root, &self.state.project);
                        if self.state.images.is_registered(id) {
                            self.renderer.clear_texture_override(id);
                            self.play.game_ui.clear_texture_override(id);
                            if self.ensure_texture(id) {
                                self.refresh_texture(id);
                            }
                        }
                        self.asset_ui.thumbnail = None;
                    }
                    _ => {
                        let path = oxy_core::names::disk_case(&root, path)
                            .unwrap_or_else(|_| path.to_owned());
                        self.library.selection = vec![Key::Script(path)];
                    }
                }
                self.script_status.look_again();
            }
            Err(error) => {
                self.history
                    .cancel(&mut self.state.project, &mut self.state.images);
                self.warn(error);
            }
        }
    }

    /// Escolher outro arquivo…: the person picks the file of the missing item `key` in the
    /// project's folder.
    pub(super) fn pick_relink(&mut self, key: &Key, kind: Kind) {
        let Some(root) = self.project_root() else {
            return;
        };
        let (filter, extension) = match kind {
            Kind::Image => ("Imagem PNG", "png"),
            Kind::Sound => ("Som WAV", "wav"),
            Kind::Script => ("Script Luau", "luau"),
            Kind::Model => ("Modelo", "json"),
        };
        let Some(file) = rfd::FileDialog::new()
            .set_title("Escolher o arquivo na pasta do projeto")
            .set_directory(&root)
            .add_filter(filter, &[extension])
            .pick_file()
        else {
            return;
        };
        let inside = std::fs::canonicalize(&root)
            .ok()
            .zip(std::fs::canonicalize(&file).ok())
            .and_then(|(root, file)| {
                file.strip_prefix(&root).ok().map(|rest| {
                    rest.components()
                        .map(|c| c.as_os_str().to_string_lossy().into_owned())
                        .collect::<Vec<_>>()
                        .join("/")
                })
            });
        match inside {
            Some(path) if !path.is_empty() => self.relink(key, &path),
            _ => self.warn(
                "O arquivo escolhido está fora da pasta do projeto; mova-o para dentro dela antes \
                 de religar.",
            ),
        }
    }

    /// Mostrar no Explorer: the folder `path` open, or the file `path` selected in its folder.
    pub(super) fn show_in_explorer(&mut self, path: &str, folder: bool) {
        let Some(root) = self.project_root() else {
            return;
        };
        let target = if path.is_empty() {
            root
        } else {
            root.join(path.replace('/', std::path::MAIN_SEPARATOR_STR))
        };
        let shown = if cfg!(windows) {
            let mut explorer = std::process::Command::new("explorer");
            if folder {
                explorer.arg(&target);
            } else {
                #[cfg(windows)]
                {
                    use std::os::windows::process::CommandExt;
                    explorer.raw_arg(format!("/select,\"{}\"", target.display()));
                }
            }
            explorer.spawn().map(drop)
        } else {
            let open = if folder {
                target.as_path()
            } else {
                target.parent().unwrap_or(&target)
            };
            std::process::Command::new("xdg-open")
                .arg(open)
                .spawn()
                .map(drop)
        };
        if let Err(error) = shown {
            self.warn(format!("Não foi possível abrir o Explorer: {error}"));
        }
    }
}
