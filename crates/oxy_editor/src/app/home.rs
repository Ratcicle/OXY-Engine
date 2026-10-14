use super::*;
use crate::icons::{self, Icon};
use crate::{theme, widgets};
use egui::{Align, Layout, RichText, Stroke};

const RECENT_LIMIT: usize = 12;

pub(super) struct Home {
    pub visible: bool,
    pub example_copy: bool,
    recent: Vec<PathBuf>,
    settings: Option<PathBuf>,
    error: Option<String>,
    logo: egui::TextureHandle,
    query: String,
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
        let icon = oxy_render::branding::oxy_icon();
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
            query: String::new(),
        }
    }

    fn persist(&mut self) {
        if let Some(path) = &self.settings {
            let result = serde_json::to_vec_pretty(
                &serde_json::json!({"schema_version": 1, "projects": self.recent}),
            )
            .map_err(persistence::PersistenceError::from)
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

    fn open_laboratory(&mut self) {
        let result = (|| {
            let project = oxy_core::guide_recipes::movement_laboratory()?;
            let path = std::env::temp_dir()
                .join(format!("oxy-laboratory-{}", new_id()))
                .join(persistence::PROJECT_FILE);
            persistence::save_project(&path, &project)?;
            Ok::<_, String>(path)
        })();
        match result {
            Ok(path) => self.transition(Transition::Laboratory(path)),
            Err(e) => self.home.error = Some(e),
        }
    }

    /// Home screen: the same top and status bars as the editor, the ways to start at the
    /// left and the recent projects at the right (stacked in narrow windows).
    pub(super) fn home_ui(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("home_bar")
            .exact_height(theme::TOPBAR_HEIGHT)
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.add_space(4.);
                    ui.image((self.home.logo.id(), Vec2::splat(24.)));
                    ui.label(
                        RichText::new("OXY Engine")
                            .font(theme::medium(14.))
                            .color(theme::TEXT),
                    );
                    ui.label(
                        RichText::new(env!("OXY_APP_VERSION"))
                            .monospace()
                            .size(11.5)
                            .color(theme::TEXT_MUTED),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add_space(4.);
                        if widgets::icon_text_button(ui, Icon::Console, "Console", false, false)
                            .clicked()
                        {
                            if self.console_visible() {
                                self.set_dock_open(false);
                            } else {
                                self.show_dock(DockTab::Console);
                            }
                        }
                        if widgets::icon_text_button(
                            ui,
                            Icon::Book,
                            "Guia de lógica visual",
                            false,
                            false,
                        )
                        .clicked()
                        {
                            self.open_guide("");
                        }
                    });
                });
            });
        egui::TopBottomPanel::bottom("home_status")
            .exact_height(theme::STATUS_HEIGHT)
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(30, 30, 32))
                    .inner_margin(egui::Margin::symmetric(8, 0)),
            )
            .show(ctx, |ui| {
                let muted = |text: &str| RichText::new(text).size(12.).color(theme::TEXT_MUTED);
                ui.horizontal_centered(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(muted(concat!("OXY Engine ", env!("OXY_APP_VERSION"))));
                        self.interface_button(ui);
                        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                            ui.add(
                                egui::Label::new(muted(
                                    "Tudo no seu computador. Seus projetos e assets ficam em pastas locais.",
                                ))
                                .truncate(),
                            );
                        });
                    });
                });
            });
        self.home_console(ctx);
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::BG_BASE))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let full = ui.available_width();
                    let wide = full >= 1000.;
                    let (margin_x, margin_y) = if wide { (40., 48.) } else { (20., 24.) };
                    let width = (full - 2. * margin_x).clamp(200., 1180.);
                    ui.add_space(margin_y);
                    ui.horizontal_top(|ui| {
                        ui.add_space((full - width) / 2.);
                        ui.vertical(|ui| {
                            ui.set_width(width);
                            if let Some(error) = &self.home.error {
                                egui::Frame::new()
                                    .fill(Color32::from_rgb(52, 30, 30))
                                    .stroke(Stroke::new(1., Color32::from_rgb(92, 48, 48)))
                                    .corner_radius(6)
                                    .inner_margin(egui::Margin::symmetric(12, 8))
                                    .show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        ui.colored_label(theme::ERROR, error);
                                    });
                                ui.add_space(16.);
                            }
                            if wide {
                                ui.horizontal_top(|ui| {
                                    ui.vertical(|ui| {
                                        ui.set_width(400.);
                                        self.home_start(ui, false);
                                    });
                                    ui.add_space(32.);
                                    ui.vertical(|ui| {
                                        ui.set_width(ui.available_width());
                                        self.home_recent(ui);
                                    });
                                });
                            } else {
                                self.home_start(ui, true);
                                ui.add_space(20.);
                                self.home_recent(ui);
                            }
                        });
                    });
                    ui.add_space(margin_y);
                });
            });
    }

    /// Title, the two ways to start and the bundled examples. `pairs` puts the cards two
    /// by two, for narrow windows.
    fn home_start(&mut self, ui: &mut egui::Ui, pairs: bool) {
        ui.label(
            RichText::new("Crie do seu jeito.")
                .font(theme::medium(22.))
                .color(theme::TEXT),
        );
        ui.label(
            RichText::new("Cenas 2D e 3D · Modelagem · Pintura · Animação · Lógica visual")
                .size(12.5)
                .color(theme::TEXT_MUTED),
        );
        ui.add_space(16.);
        let start = [
            (
                Icon::Plus,
                "Novo projeto",
                "Comece com uma cena 2D ou 3D vazia.",
            ),
            (
                Icon::Folder,
                "Abrir projeto",
                "Escolha o arquivo project.oxy.json de uma pasta.",
            ),
        ];
        let examples = [
            (
                Icon::Examples,
                "Projeto de exemplo",
                "Três cenas para explorar. Abre como cópia; ao salvar, você escolhe a pasta.",
            ),
            (
                Icon::Track,
                "Laboratório 3D · movimento",
                "Pista com rampas, gelo, plataformas e checkpoints feitos com nós.",
            ),
        ];
        let clicked = |ui: &mut egui::Ui, cards: &[(Icon, &str, &str); 2], first_primary: bool| {
            let mut clicked = None;
            if pairs {
                let gap = 8.;
                let width = (ui.available_width() - gap) / 2.;
                let height = cards
                    .iter()
                    .map(|(_, _, text)| action_card_height(ui, width, text))
                    .fold(0., f32::max);
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    for (i, (icon, title, text)) in cards.iter().enumerate() {
                        let primary = first_primary && i == 0;
                        if action_card(ui, *icon, title, text, primary, width, height).clicked() {
                            clicked = Some(i);
                        }
                    }
                });
            } else {
                let width = ui.available_width();
                for (i, (icon, title, text)) in cards.iter().enumerate() {
                    let height = action_card_height(ui, width, text);
                    let primary = first_primary && i == 0;
                    if action_card(ui, *icon, title, text, primary, width, height).clicked() {
                        clicked = Some(i);
                    }
                    ui.add_space(8. - ui.spacing().item_spacing.y);
                }
            }
            clicked
        };
        match clicked(ui, &start, true) {
            Some(0) => self.new_project_dialog(),
            Some(_) => {
                if let Some(path) = rfd::FileDialog::new()
                    .set_title("Abrir projeto OXY")
                    .add_filter("Projeto OXY", &["json"])
                    .pick_file()
                {
                    self.transition(Transition::Open(path));
                }
            }
            None => {}
        }
        ui.add_space(20.);
        ui.label(
            RichText::new("EXEMPLOS")
                .font(theme::medium(11.))
                .color(theme::TEXT_MUTED),
        );
        ui.add_space(4.);
        match clicked(ui, &examples, false) {
            Some(0) => self.open_example(),
            Some(_) => self.open_laboratory(),
            None => {}
        }
    }

    /// Recent projects in one block, with search, the file's age and a "⋯" per row.
    fn home_recent(&mut self, ui: &mut egui::Ui) {
        let mut open = None;
        let mut remove = None;
        egui::Frame::new()
            .fill(theme::BG_PANEL)
            .stroke(Stroke::new(1., theme::CARD_BORDER))
            .corner_radius(theme::CARD_RADIUS)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let count = self.home.recent.len();
                let (header, _) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 44.), Sense::hover());
                let mut row = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(header.shrink2(Vec2::new(12., 0.)))
                        .layout(Layout::left_to_right(Align::Center)),
                );
                row.label(
                    RichText::new("PROJETOS RECENTES")
                        .font(theme::medium(11.))
                        .color(theme::TEXT_MUTED),
                );
                if count > 0 {
                    let galley = row.painter().layout_no_wrap(
                        count.to_string(),
                        egui::FontId::proportional(11.),
                        Color32::from_rgb(180, 180, 186),
                    );
                    let (badge, _) = row
                        .allocate_exact_size(Vec2::new(galley.size().x + 12., 16.), Sense::hover());
                    row.painter()
                        .rect_filled(badge, 8., Color32::from_rgb(52, 52, 58));
                    row.painter().galley(
                        badge.center() - galley.size() / 2.,
                        galley,
                        theme::TEXT_MUTED,
                    );
                    row.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let width = ui.available_width().clamp(80., 200.);
                        let response = ui.add(
                            egui::TextEdit::singleline(&mut self.home.query)
                                .hint_text("Buscar projeto")
                                .desired_width(width - 32.)
                                .margin(egui::Margin {
                                    left: 24,
                                    right: 6,
                                    top: 4,
                                    bottom: 4,
                                }),
                        );
                        icons::paint(
                            ui,
                            Rect::from_center_size(
                                egui::pos2(response.rect.left() + 13., response.rect.center().y),
                                Vec2::splat(12.),
                            ),
                            Icon::Search,
                            theme::TEXT_MUTED,
                        );
                    });
                }
                ui.painter().hline(
                    header.x_range(),
                    header.bottom() - 0.5,
                    Stroke::new(1., theme::CARD_BORDER),
                );
                if count == 0 {
                    ui.vertical_centered(|ui| {
                        ui.add_space(40.);
                        let (tile, _) = ui.allocate_exact_size(Vec2::splat(44.), Sense::hover());
                        ui.painter().rect_filled(tile, 10., theme::BG_RAISED);
                        icons::paint(
                            ui,
                            Rect::from_center_size(tile.center(), Vec2::splat(22.)),
                            Icon::Folder,
                            theme::TEXT_MUTED,
                        );
                        ui.add_space(6.);
                        ui.label(
                            RichText::new("Nenhum projeto ainda")
                                .font(theme::medium(13.))
                                .color(theme::TEXT),
                        );
                        let width = ui.available_width().min(320.);
                        ui.allocate_ui_with_layout(
                            Vec2::new(width, 0.),
                            Layout::top_down(Align::Center),
                            |ui| {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(
                                            "Seus projetos aparecerão aqui depois de abrir ou \
                                             salvar. Para conhecer o editor, comece por um dos \
                                             exemplos.",
                                        )
                                        .size(12.5)
                                        .color(theme::TEXT_MUTED),
                                    )
                                    .wrap(),
                                );
                            },
                        );
                        ui.add_space(40.);
                    });
                    return;
                }
                let query = self.home.query.trim().to_lowercase();
                egui::Frame::new().inner_margin(6).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 2.;
                    let mut shown = 0;
                    for path in &self.home.recent {
                        let name = project_name(path);
                        if !query.is_empty() && !name.to_lowercase().contains(&query) {
                            continue;
                        }
                        shown += 1;
                        match recent_row(ui, path, &name) {
                            Some(RowAction::Open) => open = Some(path.clone()),
                            Some(RowAction::Remove) => remove = Some(path.clone()),
                            None => {}
                        }
                    }
                    if shown == 0 {
                        ui.add_space(10.);
                        ui.label(
                            RichText::new("  Nenhum projeto recente com esse nome.")
                                .size(12.5)
                                .color(theme::TEXT_MUTED),
                        );
                        ui.add_space(10.);
                    }
                });
            });
        if let Some(path) = remove {
            self.home.recent.retain(|p| p != &path);
            self.home.persist();
        }
        if let Some(path) = open {
            self.transition(Transition::Open(path));
        }
    }
}

enum RowAction {
    Open,
    Remove,
}

/// Project folder name shown for a recent `project.oxy.json`.
fn project_name(path: &Path) -> String {
    path.parent()
        .and_then(Path::file_name)
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

/// Path as people write it, without the `\\?\` prefix of canonical Windows paths.
fn display_path(path: &Path) -> String {
    let text = path.display().to_string();
    text.strip_prefix(r"\\?\")
        .map(str::to_owned)
        .unwrap_or(text)
}

/// "hoje", "ontem", "há 3 dias"… from the time the project file was last saved.
fn age(path: &Path) -> Option<String> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    let days = std::time::SystemTime::now()
        .duration_since(modified)
        .unwrap_or_default()
        .as_secs()
        / 86_400;
    let plural =
        |n: u64, one: &str, many: &str| format!("há {n} {}", if n == 1 { one } else { many });
    Some(match days {
        0 => "hoje".into(),
        1 => "ontem".into(),
        2..=6 => plural(days, "dia", "dias"),
        7..=29 => plural(days / 7, "semana", "semanas"),
        30..=364 => plural(days / 30, "mês", "meses"),
        _ => plural(days / 365, "ano", "anos"),
    })
}

fn recent_row(ui: &mut egui::Ui, path: &Path, name: &str) -> Option<RowAction> {
    let mut action = None;
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 52.), Sense::click());
    let response = response.on_hover_text(display_path(path));
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name));
    if response.hovered() {
        ui.painter().rect_filled(rect, 5., theme::BG_RAISED);
    }
    if response.clicked() {
        action = Some(RowAction::Open);
    }
    let missing = !path.is_file();
    let mut row = ui.new_child(
        egui::UiBuilder::new()
            .id_salt(path)
            .max_rect(rect.shrink2(Vec2::new(8., 0.)))
            .layout(Layout::right_to_left(Align::Center)),
    );
    let more = icons::small(
        &mut row,
        Icon::More,
        &format!("Mais ações para {name}"),
        26.,
    );
    egui::Popup::menu(&more).show(|ui| {
        if ui.button("Abrir").clicked() {
            action = Some(RowAction::Open);
        }
        ui.separator();
        if ui.button("Remover da lista").clicked() {
            action = Some(RowAction::Remove);
        }
    });
    row.add_space(4.);
    if missing {
        egui::Frame::new()
            .fill(Color32::from_rgba_unmultiplied(224, 184, 104, 30))
            .corner_radius(10)
            .inner_margin(egui::Margin::symmetric(7, 2))
            .show(&mut row, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 5.;
                    let (icon, _) = ui.allocate_exact_size(Vec2::splat(12.), Sense::hover());
                    widgets::warning_icon(ui, icon, theme::WARNING);
                    ui.label(
                        RichText::new("Arquivo não encontrado")
                            .size(11.5)
                            .color(theme::WARNING),
                    );
                });
            });
    } else if let Some(age) = age(path) {
        row.label(RichText::new(age).size(12.).color(theme::TEXT_MUTED));
    }
    row.add_space(8.);
    // Folder icon, name and path take what is left at the left.
    let text = Rect::from_min_max(
        rect.min + Vec2::new(12., 0.),
        egui::pos2(row.min_rect().left().min(rect.right()), rect.bottom()),
    );
    let painter = ui.painter_at(text);
    icons::paint(
        ui,
        Rect::from_center_size(
            egui::pos2(text.left() + 9., rect.center().y),
            Vec2::splat(16.),
        ),
        Icon::Folder,
        theme::TEXT_MUTED,
    );
    let max_width = (text.width() - 30.).max(10.);
    let galley = |text: RichText| {
        egui::WidgetText::from(text).into_galley(
            ui,
            Some(egui::TextWrapMode::Truncate),
            max_width,
            egui::TextStyle::Body,
        )
    };
    let title = galley(
        RichText::new(name)
            .font(theme::medium(13.))
            .color(if missing {
                Color32::from_rgb(160, 160, 166)
            } else {
                theme::TEXT
            }),
    );
    let location = galley(
        RichText::new(display_path(path))
            .monospace()
            .size(11.5)
            .color(Color32::from_rgb(138, 138, 144)),
    );
    let left = text.left() + 30.;
    painter.galley(
        egui::pos2(left, rect.center().y - title.size().y),
        title,
        theme::TEXT,
    );
    painter.galley(
        egui::pos2(left, rect.center().y + 1.),
        location,
        theme::TEXT_MUTED,
    );
    action
}

const CARD_PADDING: f32 = 14.;
const CARD_ICON: f32 = 36.;

fn action_card_text(ui: &egui::Ui, width: f32, text: &str) -> std::sync::Arc<egui::Galley> {
    let wrap = width - 2. * CARD_PADDING - CARD_ICON - 12.;
    ui.painter().layout(
        text.into(),
        egui::FontId::proportional(12.),
        Color32::from_rgb(160, 160, 166),
        wrap.max(40.),
    )
}

fn action_card_height(ui: &egui::Ui, width: f32, text: &str) -> f32 {
    let body = 18. + 2. + action_card_text(ui, width, text).size().y;
    body.max(CARD_ICON) + 2. * 12.
}

/// Large button with an icon tile, a title and one explaining sentence.
fn action_card(
    ui: &mut egui::Ui,
    icon: Icon,
    title: &str,
    text: &str,
    primary: bool,
    width: f32,
    height: f32,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());
    icons::register_qa(ui, title, rect);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, title));
    let hovered = response.hovered();
    let (fill, border) = match (primary, hovered) {
        (true, false) => (
            Color32::from_rgb(48, 74, 108),
            Color32::from_rgb(74, 127, 192),
        ),
        (true, true) => (Color32::from_rgb(54, 84, 121), theme::ACCENT),
        (false, false) => (theme::CARD_FILL, theme::CARD_BORDER),
        (false, true) => (Color32::from_rgb(50, 50, 54), Color32::from_rgb(74, 74, 80)),
    };
    let painter = ui.painter();
    painter.rect(
        rect,
        theme::CARD_RADIUS,
        fill,
        Stroke::new(1., border),
        egui::StrokeKind::Inside,
    );
    let tile = Rect::from_min_size(
        egui::pos2(rect.left() + CARD_PADDING, rect.center().y - CARD_ICON / 2.),
        Vec2::splat(CARD_ICON),
    );
    let (tile_fill, ink) = if primary {
        (Color32::from_rgb(74, 127, 192), Color32::WHITE)
    } else {
        (
            Color32::from_rgb(31, 31, 34),
            Color32::from_rgb(196, 196, 202),
        )
    };
    painter.rect_filled(tile, 6., tile_fill);
    icons::paint(
        ui,
        Rect::from_center_size(tile.center(), Vec2::splat(18.)),
        icon,
        ink,
    );
    let body = action_card_text(ui, width, text);
    let left = tile.right() + 12.;
    let top = rect.center().y - (18. + 2. + body.size().y) / 2.;
    painter.text(
        egui::pos2(left, top),
        egui::Align2::LEFT_TOP,
        title,
        theme::medium(13.),
        Color32::from_rgb(240, 240, 242),
    );
    painter.galley(egui::pos2(left, top + 20.), body, theme::TEXT_MUTED);
    response
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
    fn recent_rows_show_plain_paths_and_the_file_age() {
        assert_eq!(
            display_path(Path::new(r"\\?\C:\Jogos\Corrida\project.oxy.json")),
            r"C:\Jogos\Corrida\project.oxy.json"
        );
        assert_eq!(age(Path::new("nao-existe.oxy.json")), None);
        let dir = std::env::temp_dir().join(format!("oxy-age-{}", new_id()));
        let path = dir.join("project.oxy.json");
        persistence::safe_write(&path, b"{}").unwrap();
        assert_eq!(age(&path).as_deref(), Some("hoje"));
        std::fs::remove_dir_all(dir).unwrap();
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
