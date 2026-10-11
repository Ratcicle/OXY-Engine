use super::*;
use crate::icons::Icon;
use crate::widgets;

pub(super) enum SceneDialog {
    Create { name: String, kind: SceneKind },
    Rename { id: Id, name: String },
    Delete(Id),
}
pub(super) struct NewProject {
    pub name: String,
    pub kind: SceneKind,
}
impl Default for NewProject {
    fn default() -> Self {
        Self {
            name: "Meu projeto OXY".into(),
            kind: SceneKind::TwoD,
        }
    }
}
pub(super) fn kind_label(kind: SceneKind) -> &'static str {
    if kind == SceneKind::TwoD { "2D" } else { "3D" }
}

impl Editor {
    pub(super) fn new_project_dialog(&mut self) {
        self.pause();
        self.new_project = Some(NewProject::default());
    }
    /// Scene picker at the top of the Hierarchy, with create and scene actions beside it.
    pub(super) fn scene_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.;
            let buttons = 2. * 26. + 2. * ui.spacing().item_spacing.x;
            let combo_padding = 2. * ui.spacing().button_padding.x + ui.spacing().icon_width + 4.;
            let width = (ui.available_width() - buttons - combo_padding).max(60.);
            self.scene_picker(ui, width);
            if crate::icons::small(ui, crate::icons::Icon::Plus, "Criar cena", 26.).clicked() {
                self.scene_dialog = Some(SceneDialog::Create {
                    name: "Nova cena".into(),
                    kind: self.scene().kind,
                });
            }
            let more = crate::icons::small(ui, crate::icons::Icon::More, "Ações da cena", 26.);
            egui::Popup::menu(&more).show(|ui| self.scene_actions(ui));
        });
    }
    fn scene_picker(&mut self, ui: &mut egui::Ui, width: f32) {
        let mut scene_id = self.scene_id.clone();
        egui::ComboBox::from_id_salt("scene")
            .width(width)
            .selected_text(format!(
                "{} · {}",
                self.scene().name,
                kind_label(self.scene().kind)
            ))
            .show_ui(ui, |ui| {
                for scene in &self.state.project.scenes {
                    let start = scene.id == self.state.project.start_scene;
                    ui.selectable_value(
                        &mut scene_id,
                        scene.id.clone(),
                        format!(
                            "{}{} · {}",
                            if start { "• " } else { "" },
                            scene.name,
                            kind_label(scene.kind)
                        ),
                    )
                    .on_hover_text(if start {
                        "Cena inicial aberta pelo jogo independente."
                    } else {
                        "Abrir esta cena para editar."
                    });
                }
            });
        if scene_id != self.scene_id {
            self.set_scene(scene_id);
        }
    }
    fn scene_actions(&mut self, ui: &mut egui::Ui) {
        {
            if ui.button("Renomear cena").clicked() {
                self.scene_dialog = Some(SceneDialog::Rename {
                    id: self.scene_id.clone(),
                    name: self.scene().name.clone(),
                });
                ui.close();
            }
            if ui.button("Duplicar cena").clicked() {
                if self.structural_ready() {
                    match editing::duplicate_scene(&mut self.state.project, &self.scene_id) {
                        Ok(id) => self.set_scene(id),
                        Err(e) => self.warn(e),
                    }
                }
                ui.close();
            }
            if ui
                .add_enabled(
                    self.state.project.scenes.len() > 1,
                    egui::Button::new("Excluir cena"),
                )
                .on_disabled_hover_text("O projeto precisa manter pelo menos uma cena.")
                .clicked()
            {
                self.scene_dialog = Some(SceneDialog::Delete(self.scene_id.clone()));
                ui.close();
            }
            if ui.button("Usar como cena inicial do jogo").clicked() {
                self.state.project.start_scene = self.scene_id.clone();
                ui.close();
            }
        }
    }
    pub(super) fn project_dialogs(&mut self, ctx: &egui::Context) {
        if let Some(mut new) = self.new_project.take() {
            let mut create = false;
            let mut cancel = false;
            let response = egui::Modal::new("new_project".into()).show(ctx, |ui| {
                ui.set_width((ctx.content_rect().width() - 48.).clamp(160., 460.));
                cancel = widgets::dialog_header(ui, "Novo projeto");
                widgets::field_label(ui, "Nome do projeto");
                ui.add(
                    egui::TextEdit::singleline(&mut new.name)
                        .desired_width(f32::INFINITY)
                        .margin(egui::Margin::symmetric(8, 6)),
                );
                ui.add_space(10.);
                widgets::field_label(ui, "Primeira cena");
                let gap = 10.;
                let width = (ui.available_width() - gap) / 2.;
                let height = KINDS
                    .iter()
                    .map(|(_, _, _, text)| kind_text(ui, width, text).size().y)
                    .fold(0., f32::max)
                    + 92.;
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    for (kind, icon, title, text) in KINDS {
                        if kind_card(ui, icon, title, text, new.kind == kind, width, height)
                            .clicked()
                        {
                            new.kind = kind;
                        }
                    }
                });
                widgets::hint(ui, "Você poderá adicionar cenas 2D e 3D depois.");
                widgets::dialog_footer(ui, |ui| {
                    create =
                        widgets::primary_button(ui, "Criar projeto", !new.name.trim().is_empty())
                            .clicked();
                    cancel |= widgets::secondary_button(ui, "Cancelar").clicked();
                });
            });
            cancel |= response.should_close();
            create |= response.is_top_modal
                && ctx.input(|i| i.key_pressed(egui::Key::Enter))
                && !new.name.trim().is_empty();
            if !cancel {
                if create {
                    self.transition(Transition::Create(new.name, new.kind));
                } else {
                    self.new_project = Some(new);
                }
            }
        }
        if let Some(mut dialog) = self.scene_dialog.take() {
            let mut confirm = false;
            let mut cancel = false;
            let response = egui::Modal::new("scene_dialog".into()).show(ctx, |ui| {
                ui.set_width((ctx.content_rect().width() - 48.).clamp(160., 420.));
                match &mut dialog {
                    SceneDialog::Create { name, kind } => {
                        cancel = widgets::dialog_header(ui, "Nova cena");
                        widgets::field_label(ui, "Nome da cena");
                        ui.add(
                            egui::TextEdit::singleline(name)
                                .desired_width(f32::INFINITY)
                                .margin(egui::Margin::symmetric(8, 6)),
                        );
                        ui.add_space(10.);
                        widgets::field_label(ui, "Tipo");
                        let gap = 8.;
                        let width = (ui.available_width() - gap) / 2.;
                        ui.horizontal_top(|ui| {
                            ui.spacing_mut().item_spacing.x = gap;
                            for (value, icon, title, _) in KINDS {
                                if kind_row(ui, icon, title, *kind == value, width).clicked() {
                                    *kind = value;
                                }
                            }
                        });
                        let valid = !name.trim().is_empty();
                        widgets::dialog_footer(ui, |ui| {
                            confirm = widgets::primary_button(ui, "Criar cena", valid).clicked();
                            cancel |= widgets::secondary_button(ui, "Cancelar").clicked();
                        });
                        confirm |= valid && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    }
                    SceneDialog::Rename { id, name } => {
                        cancel = widgets::dialog_header(ui, "Renomear cena");
                        widgets::field_label(ui, "Nome da cena");
                        ui.add(
                            egui::TextEdit::singleline(name)
                                .desired_width(f32::INFINITY)
                                .margin(egui::Margin::symmetric(8, 6)),
                        );
                        if let Some(scene) = self.state.project.scene(id) {
                            let mut context = format!(
                                "{} · {} objetos",
                                kind_label(scene.kind),
                                scene.entities.len()
                            );
                            if *id == self.state.project.start_scene {
                                context.push_str(" · cena inicial do jogo");
                            }
                            widgets::hint(ui, &context);
                        }
                        let valid = !name.trim().is_empty();
                        widgets::dialog_footer(ui, |ui| {
                            confirm =
                                widgets::primary_button(ui, "Confirmar nome", valid).clicked();
                            cancel |= widgets::secondary_button(ui, "Cancelar").clicked();
                        });
                        confirm |= valid && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    }
                    SceneDialog::Delete(id) => {
                        let (name, objects) = self
                            .state
                            .project
                            .scene(id)
                            .map_or((String::new(), 0), |s| (s.name.clone(), s.entities.len()));
                        let references = editing::scene_references(&self.state.project, id);
                        let start = id == &self.state.project.start_scene;
                        let free = !start && references.is_empty();
                        let title = if free {
                            format!("Excluir “{name}”?")
                        } else {
                            format!("Não é possível excluir “{name}”")
                        };
                        cancel = widgets::dialog_header(ui, &title);
                        if start {
                            widgets::hint(
                                ui,
                                "Esta é a cena inicial do jogo. Escolha outra cena inicial \
                                 em Ações da cena antes de excluí-la.",
                            );
                        } else if !references.is_empty() {
                            widgets::hint(
                                ui,
                                "Estes nós levam para esta cena. Mude o destino deles antes \
                                 de excluir:",
                            );
                            ui.add_space(4.);
                            egui::ScrollArea::vertical()
                                .max_height(180.)
                                .show(ui, |ui| {
                                    for reference in &references {
                                        reference_row(ui, reference);
                                    }
                                });
                        } else {
                            widgets::hint(
                                ui,
                                &format!(
                                    "A cena e seus {objects} objetos saem do projeto. Você \
                                     poderá desfazer com Ctrl+Z."
                                ),
                            );
                        }
                        widgets::dialog_footer(ui, |ui| {
                            if free {
                                confirm =
                                    widgets::danger_button(ui, "Excluir cena", true).clicked();
                                cancel |= widgets::secondary_button(ui, "Cancelar").clicked();
                            } else {
                                cancel |= widgets::secondary_button(ui, "Fechar").clicked();
                            }
                        });
                    }
                }
            });
            if cancel || response.should_close() {
                return;
            }
            if confirm {
                if !self.structural_ready() {
                    return;
                }
                match dialog {
                    SceneDialog::Create { name, kind } => {
                        let scene = Scene::new(name.trim(), kind);
                        let id = scene.id.clone();
                        self.state.project.scenes.push(scene);
                        self.set_scene(id);
                    }
                    SceneDialog::Rename { id, name } => {
                        if let Some(scene) = self.state.project.scene_mut(&id) {
                            scene.name = name.trim().into();
                        }
                    }
                    SceneDialog::Delete(id) => {
                        match editing::delete_scene(&mut self.state.project, &id) {
                            Ok(()) => self.set_scene(self.state.project.start_scene.clone()),
                            Err(e) => self.warn(e),
                        }
                    }
                }
            } else {
                self.scene_dialog = Some(dialog);
            }
        }
    }
}

fn kind_text(ui: &egui::Ui, width: f32, text: &str) -> std::sync::Arc<egui::Galley> {
    ui.painter().layout(
        text.into(),
        egui::FontId::proportional(12.),
        crate::theme::TEXT_MUTED,
        width - 28.,
    )
}

/// Selectable card for the first scene of a new project.
fn kind_card(
    ui: &mut egui::Ui,
    icon: crate::icons::Icon,
    title: &str,
    text: &str,
    selected: bool,
    width: f32,
    height: f32,
) -> egui::Response {
    use crate::theme;
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());
    crate::icons::register_qa(ui, title, rect);
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, title)
    });
    let hovered = response.hovered();
    let (fill, border) = match (selected, hovered) {
        (true, false) => (Color32::from_rgb(38, 53, 72), theme::ACCENT),
        (true, true) => (Color32::from_rgb(42, 59, 80), theme::ACCENT_BRIGHT),
        (false, false) => (theme::BG_PANEL, theme::BORDER),
        (false, true) => (theme::CARD_FILL, Color32::from_rgb(86, 86, 92)),
    };
    let painter = ui.painter();
    painter.rect(
        rect,
        8.,
        fill,
        egui::Stroke::new(1., border),
        egui::StrokeKind::Inside,
    );
    let ink = if selected {
        Color32::from_rgb(183, 210, 245)
    } else {
        Color32::from_rgb(196, 196, 202)
    };
    crate::icons::paint(
        ui,
        Rect::from_min_size(rect.min + Vec2::splat(14.), Vec2::splat(32.)),
        icon,
        ink,
    );
    if selected {
        let center = egui::pos2(rect.right() - 19., rect.top() + 19.);
        painter.circle_filled(center, 9., theme::ACCENT);
        crate::icons::paint(
            ui,
            Rect::from_center_size(center, Vec2::splat(10.)),
            crate::icons::Icon::Check,
            Color32::WHITE,
        );
    }
    let top = rect.top() + 56.;
    painter.text(
        egui::pos2(rect.left() + 14., top),
        egui::Align2::LEFT_TOP,
        title,
        theme::medium(13.),
        Color32::from_rgb(240, 240, 242),
    );
    painter.galley(
        egui::pos2(rect.left() + 14., top + 20.),
        kind_text(ui, width, text),
        theme::TEXT_MUTED,
    );
    response
}

const KINDS: [(SceneKind, Icon, &str, &str); 2] = [
    (
        SceneKind::TwoD,
        Icon::Sprite,
        "Cena 2D",
        "Imagens, formas e uma câmera 2D.",
    ),
    (
        SceneKind::ThreeD,
        Icon::Cube,
        "Cena 3D",
        "Modelos, profundidade e uma câmera 3D.",
    ),
];

/// Compact selectable card for the kind of a new scene: icon at the left, then the title.
fn kind_row(
    ui: &mut egui::Ui,
    icon: Icon,
    title: &str,
    selected: bool,
    width: f32,
) -> egui::Response {
    use crate::theme;
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 44.), Sense::click());
    crate::icons::register_qa(ui, title, rect);
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, title)
    });
    let (fill, border) = match (selected, response.hovered()) {
        (true, _) => (Color32::from_rgb(38, 53, 72), theme::ACCENT),
        (false, false) => (theme::BG_PANEL, theme::BORDER),
        (false, true) => (theme::CARD_FILL, Color32::from_rgb(86, 86, 92)),
    };
    ui.painter().rect(
        rect,
        8.,
        fill,
        egui::Stroke::new(1., border),
        egui::StrokeKind::Inside,
    );
    let ink = if selected {
        Color32::from_rgb(183, 210, 245)
    } else {
        Color32::from_rgb(196, 196, 202)
    };
    crate::icons::paint(
        ui,
        Rect::from_center_size(
            egui::pos2(rect.left() + 24., rect.center().y),
            Vec2::splat(20.),
        ),
        icon,
        ink,
    );
    ui.painter().text(
        egui::pos2(rect.left() + 44., rect.center().y),
        egui::Align2::LEFT_CENTER,
        title,
        theme::medium(13.),
        Color32::from_rgb(240, 240, 242),
    );
    response
}

/// One logic node that points at a scene, from "scope → object → action".
fn reference_row(ui: &mut egui::Ui, reference: &str) {
    use crate::theme;
    let parts: Vec<_> = reference.split(" → ").collect();
    let (object, detail) = match parts.as_slice() {
        [scope, object, action] => (*object, format!("{scope} · {action}")),
        _ => (reference, String::new()),
    };
    egui::Frame::new()
        .fill(theme::BG_PANEL)
        .corner_radius(5)
        .inner_margin(egui::Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (icon, _) = ui.allocate_exact_size(Vec2::splat(14.), Sense::hover());
                crate::icons::paint(ui, icon, Icon::Logic, theme::ACCENT_BRIGHT);
                ui.label(egui::RichText::new(object).size(12.5).color(theme::TEXT));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(detail)
                                .size(12.)
                                .color(theme::TEXT_MUTED),
                        )
                        .truncate(),
                    );
                });
            });
        });
}
