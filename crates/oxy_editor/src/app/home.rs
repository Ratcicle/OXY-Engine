use super::*;

const RECENT_LIMIT: usize = 12;

pub(super) struct Home {
    pub visible: bool,
    pub example_copy: bool,
    recent: Vec<PathBuf>,
    settings: Option<PathBuf>,
    error: Option<String>,
    logo: egui::TextureHandle,
}

impl Home {
    pub fn load(ctx: &egui::Context) -> Self {
        // Native QA must never read or write the user's recent-project list.
        let settings = if cfg!(test) {
            None
        } else {
            std::env::var_os("LOCALAPPDATA")
                .map(|p| PathBuf::from(p).join("OXY Engine/recent-projects.json"))
        };
        let (recent, error) = match settings.as_deref().filter(|p| p.exists()).map(read_recent) {
            Some(Ok(paths)) => (paths, None),
            Some(Err(error)) => (Vec::new(), Some(error)),
            None => (Vec::new(), None),
        };
        let icon = crate::window_icon();
        let logo = ctx.load_texture(
            "OXY Engine",
            egui::ColorImage::from_rgba_unmultiplied(
                [icon.width as usize, icon.height as usize],
                &icon.rgba,
            ),
            egui::TextureOptions::LINEAR,
        );
        Self {
            visible: true,
            example_copy: false,
            recent,
            settings,
            error,
            logo,
        }
    }

    fn persist(&mut self) {
        if let Some(path) = &self.settings {
            let result = serde_json::to_vec_pretty(
                &serde_json::json!({"schema_version": 1, "projects": self.recent}),
            )
            .map_err(|e| e.to_string())
            .and_then(|bytes| persistence::safe_write(path, &bytes));
            if let Err(error) = result {
                self.error = Some(format!(
                    "Não foi possível salvar a lista de recentes: {error}"
                ));
            }
        }
    }
}

fn read_recent(path: &Path) -> Result<Vec<PathBuf>, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|e| format!("Lista de recentes inválida: {e}"))?;
    if value["schema_version"] != 1 {
        return Err("Versão incompatível da lista de projetos recentes.".into());
    }
    let paths: Vec<PathBuf> =
        serde_json::from_value(value["projects"].clone()).map_err(|e| e.to_string())?;
    let mut result = Vec::new();
    for path in paths.into_iter().filter(|p| p.is_absolute()) {
        if !result.contains(&path) {
            result.push(path);
        }
        if result.len() == RECENT_LIMIT {
            break;
        }
    }
    Ok(result)
}

fn remember(recent: &mut Vec<PathBuf>, path: PathBuf) {
    recent.retain(|p| p != &path);
    recent.insert(0, path);
    recent.truncate(RECENT_LIMIT);
}

pub(super) fn bundled_example(exe: &Path) -> Option<PathBuf> {
    let dir = exe.parent()?;
    let mut candidates = vec![dir.join("data/project.oxy.json")];
    // Development-only convenience; release packages use executable-relative data exclusively.
    if cfg!(debug_assertions) {
        candidates.push(dir.join("../../examples/validacao/project.oxy.json"));
        candidates.push(dir.join("../../../examples/validacao/project.oxy.json"));
    }
    candidates.into_iter().find(|p| p.is_file())
}

impl Editor {
    pub(super) fn remember_project(&mut self) {
        if let Some(path) = self.path.as_ref().and_then(|p| p.canonicalize().ok()) {
            remember(&mut self.home.recent, path);
            self.home.persist();
        }
    }

    fn open_example(&mut self) {
        let source = std::env::current_exe()
            .ok()
            .and_then(|exe| bundled_example(&exe));
        let Some(source) = source else {
            self.home.error = Some("Exemplo não encontrado. Extraia o ZIP inteiro, mantendo a pasta data junto do executável.".into());
            return;
        };
        // Work in an independent temporary bundle from the beginning, so even
        // asset imports cannot write into the distributed or repository example.
        let result = (|| {
            let project = persistence::load_project_lazy(&source)?;
            let root = source.parent().ok_or("Exemplo sem pasta de origem")?;
            let mut images = TextureCache::default();
            images.configure(root, &project);
            let copy = std::env::temp_dir()
                .join(format!("oxy-example-{}", new_id()))
                .join(persistence::PROJECT_FILE);
            persistence::save_cached_copy(&copy, &project, &mut images, root)?;
            Ok::<_, String>(copy)
        })();
        let copy = match result {
            Ok(copy) => copy,
            Err(error) => {
                self.home.error = Some(error);
                return;
            }
        };
        self.open_with_recent(copy, false);
        if !self.home.visible {
            self.home.example_copy = true;
            self.log("Exemplo aberto como cópia. Salvar solicitará uma nova pasta; o exemplo original será preservado.");
        }
    }

    pub(super) fn home_ui(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(&ctx.style()).inner_margin(28))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.add_space(24.);
                    ui.horizontal(|ui| {
                        ui.image((self.home.logo.id(), Vec2::splat(72.)));
                        ui.vertical(|ui| {
                            ui.heading(egui::RichText::new("OXY Engine").size(32.));
                            ui.label(concat!(
                                "Versão ",
                                env!("CARGO_PKG_VERSION"),
                                " · Crie do seu jeito."
                            ));
                        });
                    });
                    ui.horizontal_wrapped(|ui| {
                        self.interface_button(ui);
                        self.notices_button(ui);
                        if ui.button("Guia de lógica visual").clicked() {
                            self.open_guide("");
                        }
                        if ui.button("Console").clicked() {
                            self.console = !self.console;
                        }
                    });
                    ui.horizontal_wrapped(|ui| {
                        if ui
                            .add(
                                egui::Button::new("Novo projeto")
                                    .min_size(Vec2::new(170., 44.))
                                    .fill(Color32::from_rgb(43, 103, 100)),
                            )
                            .clicked()
                        {
                            self.new_project_dialog();
                        }
                        if ui
                            .add(egui::Button::new("Abrir projeto").min_size(Vec2::new(170., 44.)))
                            .clicked()
                            && let Some(path) = rfd::FileDialog::new()
                                .set_title("Abrir projeto OXY")
                                .add_filter("Projeto OXY", &["json"])
                                .pick_file()
                        {
                            self.transition(Transition::Open(path));
                        }
                        if ui
                        .add(egui::Button::new("Projeto de exemplo").min_size(Vec2::new(170., 44.)))
                        .on_hover_text(
                            "Explore as três cenas. Ao salvar, escolha uma pasta para sua cópia.",
                        )
                        .clicked()
                    {
                        self.open_example();
                    }
                    });
                    ui.label("Cenas 2D e 3D · Modelagem · Pintura · Animação · Lógica visual");
                    ui.add_space(24.);
                    ui.heading("Projetos recentes");
                    if self.home.recent.is_empty() {
                        ui.weak("Seus projetos aparecerão aqui depois de abrir ou salvar.");
                    }
                    for path in self.home.recent.clone() {
                        ui.push_id(&path, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                let name = path
                                    .parent()
                                    .and_then(Path::file_name)
                                    .unwrap_or_default()
                                    .to_string_lossy();
                                if ui
                                    .button(name)
                                    .on_hover_text(path.display().to_string())
                                    .clicked()
                                {
                                    self.transition(Transition::Open(path.clone()));
                                }
                                if ui.small_button("Remover da lista").clicked() {
                                    self.home.recent.retain(|p| p != &path);
                                    self.home.persist();
                                }
                                if !path.is_file() {
                                    ui.colored_label(Color32::YELLOW, "Arquivo não encontrado");
                                }
                            });
                            ui.weak(path.display().to_string());
                            ui.separator();
                        });
                    }
                    if let Some(error) = &self.home.error {
                        ui.colored_label(Color32::LIGHT_RED, error);
                    }
                    if self.console
                        && let Some(message) = self.messages.last()
                    {
                        ui.colored_label(Color32::LIGHT_RED, message);
                    }
                    ui.add_space(16.);
                    ui.weak(
                    "Tudo no seu computador. Seus projetos e assets permanecem em pastas locais.",
                );
                });
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recents_are_bounded_deduplicated_and_reordered() {
        let mut paths = Vec::new();
        for i in 0..20 {
            remember(&mut paths, PathBuf::from(format!("project-{i}")));
        }
        let path = paths[5].clone();
        remember(&mut paths, path.clone());
        assert_eq!(paths.len(), RECENT_LIMIT);
        assert_eq!(paths[0], path);
        assert_eq!(paths.iter().filter(|p| *p == &path).count(), 1);
    }
    #[test]
    fn recent_settings_roundtrip_and_invalid_version() {
        let dir = std::env::temp_dir().join(format!("oxy-recents-{}", new_id()));
        let path = dir.join("recent.json");
        let projects = vec![dir.join("project.oxy.json")];
        persistence::safe_write(
            &path,
            &serde_json::to_vec(&serde_json::json!({"schema_version":1,"projects":projects}))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(read_recent(&path).unwrap(), projects);
        persistence::safe_write(&path, br#"{"schema_version":2,"projects":[]}"#).unwrap();
        assert!(read_recent(&path).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
