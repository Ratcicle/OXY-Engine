//! Opt-in native integration test. Inputs enter only this eframe application's RawInput.
//! It never sends OS keyboard/mouse input and does not require foreground ownership.
use crate::app::{Editor, Snapshot, Tab};
use egui::{Color32, Event, Key, Modifiers, PointerButton, Pos2, Rect, Vec2};
use oxy_core::document::{Id, SceneKind, new_id};
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[derive(Clone)]
enum Action {
    Click(&'static str),
    SelectEntity(&'static str),
    ControlSelect(&'static str),
    DoubleEntity(&'static str),
    Context(&'static str),
    Reparent(&'static str, &'static str),
    EditValue(&'static str, &'static str),
    Scroll(&'static str, f32),
    DeleteTextureAsset,
    OptionalClick(&'static str),
    Text(&'static str),
    Key(Key, bool),
    Hold(Key, usize),
    Wait(usize),
    Card,
    Resize(Vec2),
    Screenshot(&'static str),
    Check(&'static str),
    DragNode(&'static str),
    ConnectPorts,
    Paint,
    GizmoX,
    GizmoDelta(f32),
    Idle,
}

#[derive(Default)]
struct Report {
    done: bool,
    error: Option<String>,
    steps: Vec<String>,
    screenshots: Vec<String>,
}
struct TextTarget {
    text: String,
    rect: Rect,
}
#[derive(Default)]
struct Surface {
    texts: Vec<TextTarget>,
    canvas: Option<Rect>,
    png: Option<Rect>,
    native_viewport: Option<Rect>,
    ports: Vec<Pos2>,
    gizmo_x: Option<Pos2>,
}

struct IdleProbe {
    deadline: Instant,
    measuring: bool,
    frames: usize,
}

fn wake_after(ctx: &egui::Context, duration: Duration) {
    // An inactive Windows viewport may defer repaint_after timers. This one-shot event-loop
    // wake is excluded from the measured frame count and never touches OS input or focus.
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        std::thread::sleep(duration);
        ctx.request_repaint();
    });
}

struct NativeQa {
    editor: Editor,
    actions: VecDeque<Action>,
    events: VecDeque<Vec<Event>>,
    surface: Surface,
    report: Arc<Mutex<Report>>,
    output: PathBuf,
    start: Instant,
    wait: usize,
    missing: usize,
    pending_shot: Option<String>,
    base: Option<Snapshot>,
    before_paint: Option<Snapshot>,
    after_paint: Option<Snapshot>,
    created: Option<Id>,
    initial_entities: usize,
    hierarchy_base: Option<Snapshot>,
    camera_before: Option<oxy_render::CameraState>,
    idle: Option<IdleProbe>,
    texture_original: Option<(Id, usize, oxy_core::painting::PaintImage)>,
    texture_base: Option<Snapshot>,
    saved_before_draft: Option<Vec<u8>>,
    finished: bool,
}

impl NativeQa {
    fn new(
        cc: &eframe::CreationContext<'_>,
        project: PathBuf,
        output: PathBuf,
        report: Arc<Mutex<Report>>,
    ) -> Self {
        let mut editor = Editor::new(cc);
        editor.open(project);
        let initial_entities = editor.scene().entities.len();
        let mut actions = VecDeque::from([
            Action::Check("lazy_open"),
            Action::Screenshot("editor-scene.png"),
            Action::Click("+ Objeto"),
            Action::Click("Retângulo"),
            Action::Check("created"),
            Action::Check("gizmo_baseline"),
            Action::GizmoX,
            Action::Check("gizmo_moved"),
            Action::Key(Key::Z, true),
            Action::Check("gizmo_undo"),
            Action::Click("Lógica"),
            Action::Click("Ao iniciar cena"),
            Action::Click("Pesquisar ação…"),
            Action::Text("Mensagem"),
            Action::Click("Mensagem de diagnóstico"),
            Action::DragNode("Mensagem de diagnóstico"),
            Action::ConnectPorts,
            Action::Check("graph_connected"),
            Action::Screenshot("graph.png"),
            Action::Key(Key::Delete, false),
            Action::Check("node_deleted"),
            Action::Key(Key::Z, true),
            Action::Check("node_undo"),
            Action::Click("Cena"),
            Action::Check("play_baseline"),
            Action::Click("▶ Jogar"),
            Action::Check("playing"),
            Action::Hold(Key::D, 24),
            Action::Check("game_moved"),
            Action::Screenshot("game.png"),
            Action::Click("Cena"),
            Action::Check("left_game_paused"),
            Action::Click("Jogo"),
            Action::Check("return_still_paused"),
            Action::Click("▶ Retomar"),
            Action::Check("playing"),
            Action::Key(Key::Escape, false),
            Action::Check("escape_paused"),
            Action::Click("■ Parar"),
            Action::Check("stop_isolated"),
            Action::Resize(Vec2::new(920., 600.)),
            Action::Screenshot("editor-small.png"),
            Action::Resize(Vec2::new(1440., 900.)),
            Action::Click("A · Sala de plataforma 2D"),
            Action::Click("B · Oficina 3D e golpe articulado"),
            Action::Check("scene_3d"),
            Action::SelectEntity("Tronco · textura pintável"),
            Action::Click("Estúdio"),
            Action::Click("Pintura"),
            Action::Click("Enquadrar peça"),
            Action::Check("texture_copy_baseline"),
            Action::OptionalClick("Criar cópia independente"),
            Action::Check("texture_copied"),
            Action::Check("paint_baseline"),
            Action::Paint,
            Action::Check("paint_changed"),
            Action::Screenshot("studio-paint.png"),
            Action::Key(Key::Z, true),
            Action::Check("paint_undo"),
            Action::Key(Key::Y, true),
            Action::Check("paint_redo"),
            Action::Key(Key::S, true),
            Action::Check("saved_roundtrip"),
            Action::Check("texture_controls_baseline"),
            Action::Click("Localizar na biblioteca"),
            Action::DeleteTextureAsset,
            Action::Check("texture_reference_blocked"),
            Action::Screenshot("texture-reference.png"),
            Action::Click("Cancelar"),
            Action::Click("Remover textura"),
            Action::Check("texture_unlinked"),
            Action::Key(Key::Z, true),
            Action::Check("texture_unlink_undo"),
            Action::Click("Modelagem"),
            Action::Screenshot("studio-3d.png"),
            Action::SelectEntity("Boneco · modelo por peças"),
            Action::Click("Animação"),
            Action::Check("animation_baseline"),
            Action::Click("▶ Reproduzir"),
            Action::Wait(16),
            Action::Check("animation_preview"),
            Action::Screenshot("animation.png"),
            Action::Resize(Vec2::new(920., 600.)),
            Action::Click("Enquadrar"),
            Action::Screenshot("animation-small.png"),
            Action::Check("animation_small_layout"),
            Action::Click("+ Keyframe"),
            Action::Check("key_created"),
            Action::Key(Key::Z, true),
            Action::Check("animation_preview"),
            Action::Resize(Vec2::new(1440., 900.)),
            Action::Click("Pose-base"),
            Action::Click("+ Keyframe"),
            Action::Check("key_created"),
            Action::Scroll("QUADRO-CHAVE", -460.),
            Action::Click("Copiar quadro"),
            Action::SelectEntity("Cabeça"),
            Action::Click("Colar quadro"),
            Action::Check("key_pasted"),
            Action::Key(Key::S, true),
            Action::Check("saved_roundtrip"),
            Action::Click("Cena"),
            Action::Click("B · Oficina 3D e golpe articulado"),
            Action::Click("C · Carta, custo, energia e alvo"),
            Action::Check("play_baseline"),
            Action::Click("▶ Jogar"),
            Action::Check("playing"),
            Action::Card,
            Action::Check("card_one"),
            Action::Card,
            Action::Check("card_two"),
            Action::Card,
            Action::Check("card_three"),
            Action::Card,
            Action::Check("card_four"),
            Action::Screenshot("card.png"),
            Action::Click("■ Parar"),
            Action::Check("stop_isolated"),
        ]);
        actions.extend([
            Action::Click("+ Cena"),
            Action::Click("3D"),
            Action::Click("+ Cena"),
            Action::Click("Criar cena"),
            Action::Click("+ Objeto"),
            Action::Click("Cubo"),
            Action::Key(Key::F2, false),
            Action::Key(Key::A, true),
            Action::Text("Peça A"),
            Action::Key(Key::S, true),
            Action::Check("rename_saved_inline"),
            Action::Key(Key::Enter, false),
            Action::Check("renamed_a"),
            Action::Key(Key::Z, true),
            Action::Check("rename_undo"),
            Action::Key(Key::Y, true),
            Action::Check("renamed_a"),
            Action::GizmoDelta(75.),
            Action::Click("+ Objeto"),
            Action::Click("Cubo"),
            Action::Key(Key::F2, false),
            Action::Key(Key::A, true),
            Action::Text("Peça B"),
            Action::Key(Key::Enter, false),
            Action::GizmoDelta(-75.),
            Action::Click("+ Objeto"),
            Action::Click("Grupo vazio"),
            Action::Key(Key::F2, false),
            Action::Key(Key::A, true),
            Action::Text("Articulação"),
            Action::Key(Key::Enter, false),
            Action::Click("+ Objeto"),
            Action::Click("Grupo vazio"),
            Action::Key(Key::F2, false),
            Action::Key(Key::A, true),
            Action::Text("Montagem"),
            Action::Key(Key::Enter, false),
            Action::Check("hierarchy_baseline"),
            Action::Reparent("Peça A", "Articulação"),
            Action::Reparent("Peça B", "Articulação"),
            Action::Check("pieces_parented"),
            Action::Reparent("Articulação", "Montagem"),
            Action::Check("group_parented"),
            Action::Reparent("Articulação", "Raiz da cena · solte aqui"),
            Action::Check("group_root"),
            Action::SelectEntity("Peça A"),
            Action::ControlSelect("Peça B"),
            Action::Check("multi_baseline"),
            Action::Key(Key::W, false),
            Action::GizmoX,
            Action::Check("multi_move"),
            Action::Key(Key::Z, true),
            Action::Check("multi_undo"),
            Action::Key(Key::E, false),
            Action::GizmoX,
            Action::Check("multi_rotate"),
            Action::Key(Key::Z, true),
            Action::Check("multi_undo"),
            Action::Key(Key::R, false),
            Action::GizmoX,
            Action::Check("multi_scale"),
            Action::Key(Key::Z, true),
            Action::Check("multi_undo"),
            Action::Screenshot("multi-selection.png"),
            Action::Check("focus_baseline"),
            Action::DoubleEntity("Articulação"),
            Action::Check("focused_group"),
            Action::Click("Estúdio"),
            Action::Click("Animação"),
            Action::Click("+ Nova animação"),
            Action::Key(Key::A, true),
            Action::Text("Parado"),
            Action::Key(Key::Enter, false),
            Action::Click("+ Keyframe"),
            Action::SelectEntity("Peça A"),
            Action::Click("+ Keyframe"),
            Action::SelectEntity("Articulação"),
            Action::Click("+ Nova animação"),
            Action::Key(Key::A, true),
            Action::Text("Ataque"),
            Action::Key(Key::Enter, false),
            Action::Click("+ Keyframe"),
            Action::Check("authored_clips"),
            Action::EditValue("Cursor", "0.5"),
            Action::Click("Mover (W)"),
            Action::Check("draft_baseline"),
            Action::GizmoDelta(35.),
            Action::Check("group_draft"),
            Action::Key(Key::S, true),
            Action::Check("draft_save_refused"),
            Action::Click("Nova cena 3D"),
            Action::Click("A · Sala de plataforma 2D"),
            Action::Check("draft_scene_refused"),
            Action::Click("Console"),
            Action::Click("+ Keyframe"),
            Action::Check("group_key"),
            Action::Screenshot("group-animation.png"),
            Action::Context("Ataque"),
            Action::Click("Duplicar animação"),
            Action::Check("clip_duplicated"),
            Action::Key(Key::S, true),
            Action::Check("saved_roundtrip"),
            Action::Click("Cena"),
            Action::Click("Nova cena 3D"),
            Action::Click("B · Oficina 3D e golpe articulado"),
            Action::SelectEntity("Boneco · modelo por peças"),
            Action::Click("Estúdio"),
            Action::Click("Animação"),
            Action::Check("clip_reference_baseline"),
            Action::Context("Ataque"),
            Action::Click("Excluir animação"),
            Action::Check("clip_reference_blocked"),
            Action::Screenshot("animation-reference.png"),
            Action::Click("Entendi"),
            Action::Click("Cena"),
            Action::Idle,
        ]);
        Self {
            editor,
            actions,
            events: VecDeque::new(),
            surface: Surface::default(),
            report,
            output,
            start: Instant::now(),
            wait: 15,
            missing: 0,
            pending_shot: None,
            base: None,
            before_paint: None,
            after_paint: None,
            created: None,
            initial_entities,
            hierarchy_base: None,
            camera_before: None,
            idle: None,
            texture_original: None,
            texture_base: None,
            saved_before_draft: None,
            finished: false,
        }
    }

    fn fail(&mut self, ctx: &egui::Context, message: String) {
        let labels = self
            .surface
            .texts
            .iter()
            .map(|text| format!("{:?}: {}", text.rect, text.text))
            .collect::<Vec<_>>()
            .join("\n");
        let _ = std::fs::write(self.output.join("native-qa-labels.txt"), labels);
        self.report.lock().unwrap().error = Some(message);
        self.finished = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    fn find(&self, label: &str, canvas: bool) -> Option<Pos2> {
        self.surface
            .texts
            .iter()
            .filter(|target| target.text == label || target.text.ends_with(&format!(" {label}")))
            .filter(|target| {
                !canvas
                    || self
                        .surface
                        .canvas
                        .is_some_and(|canvas| canvas.contains(target.rect.center()))
            })
            .min_by(|a, b| a.rect.top().total_cmp(&b.rect.top()))
            .map(|target| target.rect.center())
    }

    fn click(&mut self, position: Pos2) {
        self.click_with(position, PointerButton::Primary, Modifiers::NONE);
    }

    fn click_with(&mut self, position: Pos2, button: PointerButton, modifiers: Modifiers) {
        self.events.push_back(vec![Event::PointerMoved(position)]);
        self.events.push_back(vec![Event::PointerButton {
            pos: position,
            button,
            pressed: true,
            modifiers,
        }]);
        self.events.push_back(vec![Event::PointerButton {
            pos: position,
            button,
            pressed: false,
            modifiers,
        }]);
        self.events.push_back(vec![]);
    }

    fn entity_position(&self, label: &str) -> Option<Pos2> {
        self.surface
            .texts
            .iter()
            .find(|target| {
                target.text.ends_with(&format!(" {label}"))
                    && target.text.starts_with(['▾', '◇', '▤', '◉'])
            })
            .map(|target| target.rect.center())
    }

    fn key(&mut self, key: Key, control: bool) {
        let modifiers = Modifiers {
            ctrl: control,
            command: control,
            ..Modifiers::NONE
        };
        for pressed in [true, false] {
            self.events.push_back(vec![Event::Key {
                key,
                physical_key: Some(key),
                pressed,
                repeat: false,
                modifiers,
            }]);
        }
    }

    fn drag(&mut self, from: Pos2, to: Pos2) {
        self.events.push_back(vec![Event::PointerMoved(from)]);
        self.events.push_back(vec![Event::PointerButton {
            pos: from,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        }]);
        for index in 1..=6 {
            self.events
                .push_back(vec![Event::PointerMoved(from.lerp(to, index as f32 / 6.))]);
        }
        self.events.push_back(vec![Event::PointerButton {
            pos: to,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        }]);
    }

    fn check(&mut self, label: &str) -> Result<(), String> {
        let ensure = |condition: bool, message: &str| {
            if condition {
                Ok(())
            } else {
                Err(format!("{label}: {message}"))
            }
        };
        match label {
            "animation_small_layout" => {
                let viewport = self
                    .surface
                    .native_viewport
                    .ok_or("Viewport nativo da animação não encontrado")?;
                self.report.lock().unwrap().steps.push(format!(
                    "Animação 920×600: viewport {:.0}×{:.0} pontos",
                    viewport.width(),
                    viewport.height()
                ));
                ensure(
                    viewport.width() >= 160. && viewport.height() >= 90.,
                    &format!(
                        "Viewport pequeno demais para editar: {:.0}×{:.0}",
                        viewport.width(),
                        viewport.height()
                    ),
                )?;
                ensure(
                    self.find("LINHA DO TEMPO", false).is_some()
                        && self.find("+ Keyframe", false).is_some()
                        && self.find("Cursor", false).is_some(),
                    "Linha do tempo, cursor e criação de quadro devem permanecer acessíveis em 920×600",
                )
            }
            "rename_saved_inline" => {
                let id = self
                    .editor
                    .selected
                    .as_deref()
                    .ok_or("Seleção de rename ausente")?;
                let project = oxy_core::persistence::load_project(
                    self.editor.path.as_deref().ok_or("Arquivo ausente")?,
                )?;
                ensure(
                    project
                        .scene(&self.editor.scene_id)
                        .and_then(|scene| scene.entity(id))
                        .is_some_and(|entity| entity.name == "Peça A"),
                    "Ctrl+S durante F2 deve gravar o nome digitado antes de Enter",
                )
            }
            "rename_undo" => ensure(
                self.editor.scene().entities.len() == 1
                    && self
                        .editor
                        .selected
                        .as_deref()
                        .and_then(|id| self.editor.scene().entity(id))
                        .is_some_and(|entity| entity.name == "Cubo"),
                "Undo após salvar rename deve recuperar o nome anterior sem remover a peça",
            ),
            "lazy_open" => ensure(
                self.editor.state.images.is_empty() && self.editor.state.images.decode_count() == 0,
                "Abrir projeto não deve decodificar todos os PNGs para a cache de pintura",
            ),
            "texture_copy_baseline" => {
                let id = self
                    .editor
                    .selected
                    .as_deref()
                    .and_then(|id| self.editor.scene().entity(id))
                    .and_then(|entity| entity.material.texture.as_ref())
                    .ok_or("Textura compartilhada ausente")?;
                let image = self
                    .editor
                    .state
                    .images
                    .get(id)
                    .ok_or("Abrir Pintura deve carregar apenas os pixels necessários")?
                    .clone();
                ensure(
                    oxy_core::editing::asset_references(&self.editor.state.project, id).len() > 1,
                    "A textura original deve possuir vínculos compartilhados",
                )?;
                self.texture_original =
                    Some((id.clone(), self.editor.state.project.assets.len(), image));
                Ok(())
            }
            "texture_copied" => {
                let (original, count, pixels) = self
                    .texture_original
                    .as_ref()
                    .ok_or("Baseline de textura ausente")?;
                let id = self
                    .editor
                    .selected
                    .as_deref()
                    .and_then(|id| self.editor.scene().entity(id))
                    .and_then(|entity| entity.material.texture.as_ref())
                    .ok_or("Cópia de textura ausente")?;
                ensure(
                    id != original && self.editor.state.project.assets.len() == count + 1,
                    "Cópia independente precisa de novo recurso/ID",
                )?;
                ensure(
                    self.editor.state.images.get(id) == Some(pixels),
                    "Cópia independente deve preservar os pixels originais",
                )?;
                ensure(
                    self.editor.state.project.asset(original).is_some(),
                    "Cópia não pode remover o recurso original",
                )
            }
            "texture_controls_baseline" => {
                self.texture_base = Some(self.editor.state.clone());
                if let Some((original, _, pixels)) = &self.texture_original {
                    let asset = self
                        .editor
                        .state
                        .project
                        .asset(original)
                        .ok_or("Original perdido")?;
                    let root = self
                        .editor
                        .path
                        .as_ref()
                        .and_then(|path| path.parent())
                        .ok_or("Pasta ausente")?;
                    let saved = oxy_core::painting::PaintImage::load(&root.join(&asset.path))?;
                    ensure(
                        &saved == pixels,
                        "Pintar/salvar a cópia não pode modificar o PNG original",
                    )?;
                }
                Ok(())
            }
            "texture_reference_blocked" => {
                ensure(
                    self.texture_base.as_ref() == Some(&self.editor.state),
                    "Excluir textura usada não pode remover o recurso",
                )?;
                ensure(
                    self.find(
                        "Este recurso ainda está em uso. Remova os vínculos antes de excluí-lo.",
                        false,
                    )
                    .is_some(),
                    "Biblioteca deve explicar bloqueio e listar os vínculos",
                )
            }
            "texture_unlinked" => {
                let current = self
                    .editor
                    .selected
                    .as_deref()
                    .and_then(|id| self.editor.scene().entity(id))
                    .ok_or("Objeto ausente")?;
                ensure(
                    current.material.texture.is_none(),
                    "Remover textura deve limpar o vínculo do objeto",
                )?;
                let before = self.texture_base.as_ref().ok_or("Baseline ausente")?;
                ensure(
                    self.editor.state.project.assets == before.project.assets,
                    "Remover vínculo preserva recursos da biblioteca e arquivos",
                )
            }
            "texture_unlink_undo" => ensure(
                self.texture_base.as_ref() == Some(&self.editor.state),
                "Undo deve restaurar o vínculo e os pixels exatos",
            ),
            "renamed_a" => ensure(
                self.editor
                    .selected
                    .as_deref()
                    .and_then(|id| self.editor.scene().entity(id))
                    .is_some_and(|entity| entity.name == "Peça A"),
                "F2/Enter deve renomear o objeto selecionado na própria hierarquia",
            ),
            "hierarchy_baseline" => {
                ensure(
                    self.editor.scene().entities.len() == 4,
                    "A nova cena deve conter duas peças e dois grupos",
                )?;
                for name in ["Peça A", "Peça B", "Articulação", "Montagem"] {
                    ensure(
                        self.editor
                            .scene()
                            .entities
                            .iter()
                            .any(|entity| entity.name == name),
                        &format!("Renomear não preservou {name}"),
                    )?;
                }
                self.hierarchy_base = Some(self.editor.state.clone());
                Ok(())
            }
            "pieces_parented" | "group_parented" | "group_root" => {
                let scene = self.editor.scene();
                let previous = self
                    .hierarchy_base
                    .as_ref()
                    .and_then(|snapshot| snapshot.project.scene(&scene.id))
                    .ok_or("Baseline de hierarquia ausente")?;
                let group = scene
                    .entities
                    .iter()
                    .find(|entity| entity.name == "Articulação")
                    .ok_or("Grupo ausente")?;
                for name in ["Peça A", "Peça B"] {
                    let piece = scene
                        .entities
                        .iter()
                        .find(|entity| entity.name == name)
                        .ok_or("Peça ausente")?;
                    ensure(
                        piece.parent.as_deref() == Some(&group.id),
                        &format!("DnD não definiu o pai de {name}"),
                    )?;
                    ensure(
                        scene
                            .world_matrix(&piece.id)?
                            .abs_diff_eq(previous.world_matrix(&piece.id)?, 0.0001),
                        &format!("DnD deslocou {name} no mundo"),
                    )?;
                }
                if label == "group_parented" {
                    let outer = scene
                        .entities
                        .iter()
                        .find(|entity| entity.name == "Montagem")
                        .ok_or("Grupo externo ausente")?;
                    ensure(
                        group.parent.as_deref() == Some(&outer.id),
                        "Grupo deve aceitar outro grupo como pai",
                    )?;
                }
                if label == "group_root" {
                    ensure(
                        group.parent.is_none(),
                        "Soltar na raiz deve remover o pai do grupo",
                    )?;
                }
                Ok(())
            }
            "multi_baseline" => {
                ensure(
                    self.editor.selection.ids.len() == 2,
                    "Ctrl+clique deve manter duas peças selecionadas",
                )?;
                self.base = Some(self.editor.state.clone());
                Ok(())
            }
            "multi_move" | "multi_rotate" | "multi_scale" => {
                let scene = self.editor.scene();
                let before = self
                    .base
                    .as_ref()
                    .and_then(|snapshot| snapshot.project.scene(&scene.id))
                    .ok_or("Baseline de seleção ausente")?;
                ensure(
                    self.editor.selection.ids.len() == 2,
                    "Transformar não deve perder seleção múltipla",
                )?;
                let mut translation = None;
                for id in &self.editor.selection.ids {
                    let entity = scene.entity(id).ok_or("Selecionado ausente")?;
                    let old = before
                        .entity(id)
                        .ok_or("Selecionado não estava no baseline")?;
                    match label {
                        "multi_move" => {
                            let delta = glam::Vec3::from(entity.transform.position)
                                - glam::Vec3::from(old.transform.position);
                            ensure(delta.x > 0.01, "W + gizmo deve mover ambas as peças")?;
                            if let Some(expected) = translation {
                                ensure(
                                    delta.abs_diff_eq(expected, 0.0001),
                                    "Deslocamento das peças deve ser igual",
                                )?;
                            }
                            translation = Some(delta);
                        }
                        "multi_rotate" => ensure(
                            (entity.transform.rotation[0] - old.transform.rotation[0]).abs() > 0.01,
                            "E + gizmo deve girar ambas as peças",
                        )?,
                        _ => ensure(
                            entity
                                .transform
                                .scale
                                .iter()
                                .zip(old.transform.scale)
                                .all(|(new, old)| new > &(old * 1.01)),
                            "R + gizmo deve escalar o conjunto proporcionalmente",
                        )?,
                    }
                }
                Ok(())
            }
            "multi_undo" => ensure(
                self.base.as_ref() == Some(&self.editor.state),
                "Undo deve restaurar todo o gesto da seleção múltipla",
            ),
            "focus_baseline" => {
                self.camera_before = Some(self.editor.camera.clone());
                self.base = Some(self.editor.state.clone());
                Ok(())
            }
            "focused_group" => {
                ensure(
                    self.base.as_ref() == Some(&self.editor.state),
                    "Enquadrar não pode alterar documentos",
                )?;
                let previous = self
                    .camera_before
                    .as_ref()
                    .ok_or("Câmera anterior ausente")?;
                ensure(
                    !self
                        .editor
                        .camera
                        .matrix([700, 500])
                        .abs_diff_eq(previous.matrix([700, 500]), 0.0001),
                    "Duplo clique deve enquadrar o grupo",
                )?;
                ensure(
                    self.editor.selection.ids.len() == 1
                        && self
                            .editor
                            .selected
                            .as_deref()
                            .and_then(|id| self.editor.scene().entity(id))
                            .is_some_and(|entity| entity.name == "Articulação"),
                    "Duplo clique deve selecionar o grupo",
                )
            }
            "authored_clips" => {
                let owner = self
                    .editor
                    .studio
                    .owner
                    .as_deref()
                    .and_then(|id| self.editor.scene().entity(id))
                    .ok_or("Modelo de animação ausente")?;
                ensure(
                    owner.name == "Articulação",
                    "O grupo deve possuir as animações criadas",
                )?;
                ensure(
                    owner.clips.len() == 2,
                    "A lista deve conter duas animações novas",
                )?;
                let idle = owner
                    .clips
                    .iter()
                    .find(|clip| clip.name == "Parado")
                    .ok_or("Animação Parado ausente")?;
                ensure(
                    idle.tracks.len() == 2,
                    "Parado deve conter quadros da raiz e de uma peça",
                )?;
                ensure(
                    owner
                        .clips
                        .iter()
                        .any(|clip| clip.name == "Ataque" && clip.tracks.len() == 1),
                    "Ataque deve conter um quadro da raiz",
                )
            }
            "draft_baseline" => {
                ensure(
                    (self.editor.studio.animation.time - 0.5).abs() < 0.001,
                    "Editar o cursor numérico deve produzir 0,5 s",
                )?;
                self.base = Some(self.editor.state.clone());
                self.saved_before_draft = Some(
                    std::fs::read(self.editor.path.as_ref().ok_or("Arquivo ausente")?)
                        .map_err(|error| error.to_string())?,
                );
                Ok(())
            }
            "draft_save_refused" => {
                let current = std::fs::read(self.editor.path.as_ref().ok_or("Arquivo ausente")?)
                    .map_err(|error| error.to_string())?;
                ensure(
                    self.saved_before_draft.as_ref() == Some(&current),
                    "Salvar deve preservar o arquivo anterior enquanto há pose provisória",
                )?;
                ensure(
                    !self.editor.studio.animation.drafts.is_empty()
                        && self
                            .editor
                            .messages
                            .iter()
                            .any(|message| message.contains("pose provisória")),
                    "Recusa de salvamento precisa explicar a pose provisória pendente",
                )
            }
            "draft_scene_refused" => ensure(
                self.editor.scene().name == "Nova cena 3D"
                    && !self.editor.studio.animation.drafts.is_empty(),
                "Trocar de cena deve preservar o rascunho e manter a cena atual",
            ),
            "group_draft" => {
                ensure(
                    self.base.as_ref() == Some(&self.editor.state),
                    "A pose provisória não pode modificar a pose-base ou o clip",
                )?;
                ensure(
                    !self.editor.studio.animation.drafts.is_empty(),
                    "Gizmo no Estúdio deve criar pose provisória",
                )?;
                let preview = self.editor.animation_preview();
                for name in ["Peça A", "Peça B"] {
                    let entity = self
                        .editor
                        .scene()
                        .entities
                        .iter()
                        .find(|entity| entity.name == name)
                        .ok_or("Filho ausente")?;
                    ensure(
                        !preview
                            .world_matrix(&entity.id)?
                            .abs_diff_eq(self.editor.scene().world_matrix(&entity.id)?, 0.001),
                        "Filhos devem acompanhar o grupo na pose provisória",
                    )?;
                }
                Ok(())
            }
            "group_key" => {
                ensure(
                    self.editor.studio.animation.drafts.is_empty(),
                    "Gravar quadro deve consumir a pose provisória",
                )?;
                let owner = self
                    .editor
                    .studio
                    .owner
                    .as_deref()
                    .and_then(|id| self.editor.scene().entity(id))
                    .ok_or("Modelo ausente")?;
                let clip = owner
                    .clips
                    .iter()
                    .find(|clip| clip.name == "Ataque")
                    .ok_or("Clip ausente")?;
                ensure(
                    clip.tracks.iter().any(|track| {
                        track.target == owner.id
                            && track.keyframes.len() == 2
                            && track
                                .keyframes
                                .iter()
                                .any(|key| (key.time - 0.5).abs() < 0.001)
                    }),
                    "Ataque deve salvar os dois quadros da articulação",
                )?;
                let before = self
                    .base
                    .as_ref()
                    .and_then(|snapshot| snapshot.project.scene(&self.editor.scene_id))
                    .and_then(|scene| scene.entity(&owner.id))
                    .ok_or("Pose-base ausente")?;
                ensure(
                    owner.transform == before.transform,
                    "Gravar animação preserva transform da pose-base",
                )
            }
            "clip_duplicated" => {
                let owner = self
                    .editor
                    .studio
                    .owner
                    .as_deref()
                    .and_then(|id| self.editor.scene().entity(id))
                    .ok_or("Modelo ausente")?;
                let original = owner
                    .clips
                    .iter()
                    .find(|clip| clip.name == "Ataque")
                    .ok_or("Original ausente")?;
                let copy = owner
                    .clips
                    .iter()
                    .find(|clip| clip.name == "Ataque (cópia)")
                    .ok_or("Cópia ausente")?;
                ensure(
                    owner.clips.len() == 3
                        && original.id != copy.id
                        && original.tracks == copy.tracks
                        && original.events == copy.events,
                    "Duplicar animação deve preservar quadros/eventos em novo ID",
                )
            }
            "clip_reference_baseline" => {
                self.base = Some(self.editor.state.clone());
                Ok(())
            }
            "clip_reference_blocked" => {
                ensure(
                    self.base.as_ref() == Some(&self.editor.state),
                    "Excluir animação referenciada não pode alterar projeto",
                )?;
                ensure(
                    self.find("Animação em uso", false).is_some(),
                    "A exclusão bloqueada deve explicar as referências ao usuário",
                )
            }
            "created" => {
                ensure(
                    self.editor.scene().entities.len() == self.initial_entities + 1,
                    "+ Objeto não criou uma entidade",
                )?;
                self.created = self.editor.selected.clone();
                ensure(self.created.is_some(), "Objeto criado não selecionado")
            }
            "graph_connected" | "node_undo" => {
                let entity = self
                    .editor
                    .scene()
                    .entity(self.created.as_deref().ok_or("Objeto de QA ausente")?)
                    .ok_or("Objeto removido")?;
                ensure(
                    entity.graph.nodes.len() == 2 && entity.graph.edges.len() == 1,
                    "Conexão normal de portas deve produzir 2 nós / 1 aresta",
                )
            }
            "node_deleted" => {
                let entity = self
                    .editor
                    .scene()
                    .entity(self.created.as_deref().ok_or("Objeto de QA ausente")?)
                    .ok_or("Delete excluiu o objeto inteiro")?;
                ensure(
                    entity.graph.nodes.len() == 1 && entity.graph.edges.is_empty(),
                    "Delete deve excluir só o nó selecionado",
                )
            }
            "play_baseline" | "gizmo_baseline" | "animation_baseline" => {
                self.base = Some(self.editor.state.clone());
                Ok(())
            }
            "animation_preview" => ensure(
                self.base.as_ref() == Some(&self.editor.state),
                "Prévia do clip existente não pode escrever a pose-base ou alterar o projeto",
            ),
            "key_created" | "key_pasted" => {
                let owner = self
                    .editor
                    .studio
                    .owner
                    .as_deref()
                    .ok_or("Proprietário de clip ausente")?;
                let current = self
                    .editor
                    .scene()
                    .entity(owner)
                    .ok_or("Objeto animado ausente")?;
                let previous = self
                    .base
                    .as_ref()
                    .and_then(|base| base.project.scene(&self.editor.scene_id))
                    .and_then(|scene| scene.entity(owner))
                    .ok_or("Baseline de animação ausente")?;
                let count = |entity: &oxy_core::document::Entity| {
                    entity
                        .clips
                        .iter()
                        .flat_map(|clip| &clip.tracks)
                        .map(|track| track.keyframes.len())
                        .sum::<usize>()
                };
                let added = if label == "key_created" { 1 } else { 2 };
                ensure(
                    count(current) >= count(previous) + added,
                    "Inserir/copiar/colar keyframes deve alterar dados normais do clip",
                )
            }
            "card_one" | "card_two" | "card_three" | "card_four" => {
                let scene = self
                    .editor
                    .runtime
                    .as_ref()
                    .ok_or("Runtime de carta ausente")?
                    .scene();
                let energy = scene.entities.iter().find_map(|entity| {
                    entity
                        .attributes
                        .get("Energia")
                        .and_then(oxy_core::document::Value::number)
                });
                let health = scene.entities.iter().find_map(|entity| {
                    entity
                        .attributes
                        .get("Vida")
                        .and_then(oxy_core::document::Value::number)
                });
                let expected = match label {
                    "card_one" => (2., 75.),
                    "card_two" => (1., 50.),
                    _ => (0., 25.),
                };
                ensure(
                    energy == Some(expected.0) && health == Some(expected.1),
                    &format!(
                        "Clique da carta: energia/vida {energy:?}/{health:?}, esperado {expected:?}"
                    ),
                )
            }
            "gizmo_moved" => {
                let entity = self
                    .editor
                    .scene()
                    .entity(self.created.as_deref().ok_or("Objeto de QA ausente")?)
                    .ok_or("Objeto removido")?;
                ensure(
                    entity.transform.position[0] > 0.1,
                    "Arrasto visual do eixo X deve mover o objeto",
                )
            }
            "gizmo_undo" => ensure(
                self.base.as_ref() == Some(&self.editor.state),
                "Undo deve restaurar o arrasto inteiro de transformação",
            ),
            "playing" => ensure(
                self.editor.tab == Tab::Game
                    && self.editor.capture
                    && self
                        .editor
                        .runtime
                        .as_ref()
                        .is_some_and(|runtime| !runtime.paused),
                "Jogo deveria estar executando e capturando entrada",
            ),
            "game_moved" => {
                let runtime = self.editor.runtime.as_ref().ok_or("Runtime não iniciou")?;
                let original = self
                    .base
                    .as_ref()
                    .and_then(|base| base.project.scene(&self.editor.scene_id))
                    .and_then(|scene| {
                        scene.entities.iter().find(|entity| {
                            entity
                                .controller
                                .as_ref()
                                .is_some_and(|controller| controller.enabled)
                        })
                    })
                    .ok_or("Controlador da cena de validação ausente")?;
                let played = runtime
                    .scene()
                    .entity(&original.id)
                    .ok_or("Controlador removido")?;
                ensure(
                    played.transform.position[0] > original.transform.position[0] + 0.01,
                    "Entrada D remapeada deve mover o controlador no runtime",
                )
            }
            "left_game_paused" | "return_still_paused" | "escape_paused" => ensure(
                !self.editor.capture
                    && self
                        .editor
                        .runtime
                        .as_ref()
                        .is_some_and(|runtime| runtime.paused),
                "Jogo deveria pausar e liberar entrada",
            ),
            "stop_isolated" => ensure(
                self.editor.runtime.is_none() && self.base.as_ref() == Some(&self.editor.state),
                "Parar deve descartar runtime e preservar documento exato",
            ),
            "scene_3d" => ensure(
                self.editor.scene().kind == SceneKind::ThreeD,
                "Seletor de cena não abriu 3D",
            ),
            "paint_baseline" => {
                self.before_paint = Some(self.editor.state.clone());
                Ok(())
            }
            "paint_changed" => {
                ensure(
                    self.before_paint
                        .as_ref()
                        .is_some_and(|before| before.images != self.editor.state.images),
                    "Pincelada não mudou pixels reais",
                )?;
                self.after_paint = Some(self.editor.state.clone());
                Ok(())
            }
            "paint_undo" => ensure(
                self.before_paint.as_ref() == Some(&self.editor.state),
                "Undo deve desfazer o gesto inteiro",
            ),
            "paint_redo" => ensure(
                self.after_paint.as_ref() == Some(&self.editor.state),
                "Redo deve recuperar exatamente os pixels",
            ),
            "saved_roundtrip" => {
                let path = self
                    .editor
                    .path
                    .as_ref()
                    .ok_or("Arquivo de projeto ausente")?;
                let project = oxy_core::persistence::load_project(path)?;
                ensure(
                    project == self.editor.state.project,
                    "Salvar deve preservar os dados editados",
                )?;
                // Lazy residency is not document state. Only resident (including dirty) buffers
                // can be compared, and no missing CPU entry implies a missing PNG reference.
                for (id, image) in self.editor.state.images.iter() {
                    let asset = project.asset(id).ok_or("Textura sem referência estável")?;
                    let read = oxy_core::painting::PaintImage::load(
                        &path.parent().unwrap().join(&asset.path),
                    )?;
                    ensure(&read == image, "PNG reaberto diverge dos pixels em memória")?;
                }
                Ok(())
            }
            _ => Err(format!("Verificação desconhecida: {label}")),
        }
    }

    fn step(&mut self, ctx: &egui::Context) -> Result<bool, String> {
        let Some(action) = self.actions.front().cloned() else {
            return Ok(true);
        };
        let description;
        match action {
            Action::SelectEntity(label)
            | Action::ControlSelect(label)
            | Action::DoubleEntity(label) => {
                let position = self
                    .entity_position(label)
                    .ok_or_else(|| format!("Objeto da hierarquia não encontrado: {label}"))?;
                if matches!(action, Action::ControlSelect(_)) {
                    self.click_with(
                        position,
                        PointerButton::Primary,
                        Modifiers {
                            ctrl: true,
                            command: true,
                            ..Modifiers::NONE
                        },
                    );
                } else {
                    self.click(position);
                    if matches!(action, Action::DoubleEntity(_)) {
                        self.click(position);
                    }
                }
                description = format!("Selecionar hierarquia: {label}");
            }
            Action::Context(label) => {
                let point = self
                    .find(label, false)
                    .ok_or_else(|| format!("Alvo de menu de contexto ausente: {label}"))?;
                self.click_with(point, PointerButton::Secondary, Modifiers::NONE);
                description = format!("Abrir menu de contexto: {label}");
            }
            Action::Reparent(from, to) => {
                let source = self
                    .entity_position(from)
                    .ok_or_else(|| format!("Origem de DnD ausente: {from}"))?;
                let target = self
                    .entity_position(to)
                    .or_else(|| self.find(to, false))
                    .ok_or_else(|| format!("Destino de DnD ausente: {to}"))?;
                self.drag(source, target);
                description = format!("Arrastar hierarquia: {from} → {to}");
            }
            Action::EditValue(label, value) => {
                let anchor = self
                    .find(label, false)
                    .ok_or_else(|| format!("Rótulo do campo ausente: {label}"))?;
                let point = self
                    .surface
                    .texts
                    .iter()
                    .filter(|text| {
                        let p = text.rect.center();
                        p.x > anchor.x && (p.y - anchor.y).abs() < 8.
                    })
                    .min_by(|a, b| a.rect.left().total_cmp(&b.rect.left()))
                    .map(|text| text.rect.center())
                    .ok_or_else(|| format!("Valor editável após {label} ausente"))?;
                self.click(point);
                self.click(point);
                self.key(Key::A, true);
                self.events.push_back(vec![Event::Text(value.into())]);
                self.key(Key::Enter, false);
                description = format!("Editar campo visível: {label} = {value}");
            }
            Action::Scroll(label, delta) => {
                let anchor = self
                    .find(label, false)
                    .ok_or_else(|| format!("Área para rolagem não encontrada: {label}"))?;
                self.events
                    .push_back(vec![Event::PointerMoved(anchor + Vec2::new(0., 110.))]);
                self.events.push_back(vec![Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: Vec2::new(0., delta),
                    modifiers: Modifiers::NONE,
                }]);
                description = format!("Rolar painel {label}");
            }
            Action::DeleteTextureAsset => {
                let id = self
                    .editor
                    .selected
                    .as_deref()
                    .and_then(|id| self.editor.scene().entity(id))
                    .and_then(|entity| entity.material.texture.as_deref())
                    .ok_or("Textura selecionada ausente")?;
                let asset = self
                    .editor
                    .state
                    .project
                    .asset(id)
                    .ok_or("Recurso selecionado ausente")?;
                let library = self
                    .find("BIBLIOTECA DO PROJETO", false)
                    .ok_or("Biblioteca não visível")?;
                let title = self
                    .surface
                    .texts
                    .iter()
                    .find(|target| target.text == asset.name && target.rect.center().y > library.y)
                    .map(|target| target.rect.center())
                    .ok_or("Localizar na biblioteca não revelou o recurso")?;
                let point = self
                    .surface
                    .texts
                    .iter()
                    .filter(|target| {
                        target.text == "Excluir recurso do projeto"
                            && (target.rect.center().x - title.x).abs() < 130.
                            && target.rect.center().y > title.y
                    })
                    .min_by(|a, b| a.rect.top().total_cmp(&b.rect.top()))
                    .map(|target| target.rect.center())
                    .ok_or("Ação da biblioteca não está visível no cartão localizado")?;
                self.click(point);
                description = "Excluir recurso pelo cartão de sua própria textura".into();
            }
            Action::Click(label) | Action::OptionalClick(label) => {
                let Some(position) = self.find(label, false) else {
                    if matches!(action, Action::OptionalClick(_)) {
                        self.actions.pop_front();
                        return Ok(false);
                    }
                    self.missing += 1;
                    if self.missing < 70 {
                        return Ok(false);
                    }
                    return Err(format!("Controle visível não encontrado: {label}"));
                };
                self.click(position);
                description = format!("Clique: {label}");
            }
            Action::Text(text) => {
                self.events.push_back(vec![Event::Text(text.into())]);
                description = format!("Digitar: {text}");
            }
            Action::Key(key, control) => {
                self.key(key, control);
                description = format!("Tecla {key:?}, Ctrl={control}");
            }
            Action::Hold(key, frames) => {
                self.events.push_back(vec![Event::Key {
                    key,
                    physical_key: Some(key),
                    pressed: true,
                    repeat: false,
                    modifiers: Modifiers::NONE,
                }]);
                for _ in 0..frames {
                    self.events.push_back(vec![]);
                }
                self.events.push_back(vec![Event::Key {
                    key,
                    physical_key: Some(key),
                    pressed: false,
                    repeat: false,
                    modifiers: Modifiers::NONE,
                }]);
                description = format!("Manter {key:?} por {frames} quadros");
            }
            Action::Wait(frames) => {
                self.wait = frames;
                description = format!("Reproduzir {frames} quadros sem entrada");
            }
            Action::Card => {
                let position = self
                    .surface
                    .texts
                    .iter()
                    .find(|text| text.text.contains("Clique para jogar"))
                    .map(|text| text.rect.center())
                    .ok_or("Texto do botão da carta não encontrado")?;
                self.click(position);
                description = "Clique real no botão da carta".into();
            }
            Action::Resize(size) => {
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
                self.wait = 20;
                description = format!("Redimensionar: {size:?}");
            }
            Action::Screenshot(name) => {
                self.pending_shot = Some(name.into());
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(
                    name.to_owned(),
                )));
                description = format!("Captura GPU da janela: {name}");
            }
            Action::Check(label) => {
                self.check(label)?;
                description = format!("Verificado: {label}");
            }
            Action::DragNode(label) => {
                let from = self
                    .find(label, true)
                    .ok_or("Título do nó não está visível no canvas")?;
                self.drag(from, from + Vec2::new(290., 120.));
                description = "Arrastar nó pelo título".into();
            }
            Action::ConnectPorts => {
                let mut ports = self.surface.ports.clone();
                ports.sort_by(|a, b| a.y.total_cmp(&b.y).then(a.x.total_cmp(&b.x)));
                if ports.len() != 3 {
                    return Err(format!(
                        "Esperadas 3 portas de execução visíveis; encontradas {}",
                        ports.len()
                    ));
                }
                self.click(ports[0]);
                self.click(ports[1]);
                description = "Conectar saída e entrada pelas portas visuais".into();
            }
            Action::Paint => {
                let rect = self
                    .surface
                    .png
                    .ok_or("Prévia PNG pintável não encontrada")?;
                self.drag(rect.min + rect.size() * 0.33, rect.min + rect.size() * 0.63);
                description = "Pincelada real no PNG".into();
            }
            Action::GizmoX | Action::GizmoDelta(_) => {
                let from = self
                    .surface
                    .gizmo_x
                    .ok_or("Controle visual vermelho do eixo X não encontrado")?;
                let delta = if let Action::GizmoDelta(delta) = action {
                    delta
                } else {
                    75.
                };
                self.drag(from, from + Vec2::new(delta, 0.));
                description = "Arrastar eixo X pelo controle visual".into();
            }
            Action::Idle => {
                self.idle = Some(IdleProbe {
                    deadline: Instant::now() + Duration::from_millis(300),
                    measuring: false,
                    frames: 0,
                });
                wake_after(ctx, Duration::from_millis(300));
                description =
                    "Medir repouso sem request_repaint do harness (300 ms estabilização + 500 ms)"
                        .into();
            }
        }
        self.report.lock().unwrap().steps.push(description);
        self.actions.pop_front();
        self.missing = 0;
        self.wait = self.wait.max(5);
        Ok(false)
    }
}

impl eframe::App for NativeQa {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, input: &mut egui::RawInput) {
        // Preserve native screenshot responses; discard external input so other apps are untouched.
        input
            .events
            .retain(|event| matches!(event, Event::Screenshot { .. }));
        let scripted = self.events.pop_front().unwrap_or_default();
        input.modifiers = scripted
            .iter()
            .find_map(|event| {
                if let Event::Key { modifiers, .. } | Event::PointerButton { modifiers, .. } = event
                {
                    Some(*modifiers)
                } else {
                    None
                }
            })
            .unwrap_or_default();
        input.events.extend(scripted);
        input.focused = true;
        input.hovered_files.clear();
        input.dropped_files.clear();
        if let Some(viewport) = input.viewports.get_mut(&egui::ViewportId::ROOT) {
            viewport.focused = Some(true);
        }
    }

    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if self.finished {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if self.start.elapsed() > Duration::from_secs(100) {
            self.fail(ctx, "QA nativo excedeu 100 segundos".into());
            return;
        }
        let screenshots = ctx.input(|input| {
            input
                .events
                .iter()
                .filter_map(|event| {
                    if let Event::Screenshot {
                        user_data, image, ..
                    } = event
                    {
                        user_data
                            .data
                            .as_ref()
                            .and_then(|data| data.downcast_ref::<String>())
                            .map(|name| (name.clone(), image.clone()))
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
        });
        for (name, pixels) in screenshots {
            let rgba: Vec<u8> = pixels
                .pixels
                .iter()
                .flat_map(|pixel| pixel.to_array())
                .collect();
            let path = self.output.join(&name);
            if let Err(error) = image::save_buffer(
                &path,
                &rgba,
                pixels.width() as u32,
                pixels.height() as u32,
                image::ColorType::Rgba8,
            ) {
                self.fail(ctx, error.to_string());
                return;
            }
            self.report.lock().unwrap().screenshots.push(name);
            self.pending_shot = None;
        }
        self.editor.update(ctx, frame);
        self.surface = capture_surface(ctx);
        if let Some(idle) = &mut self.idle {
            if Instant::now() < idle.deadline {
                if idle.measuring {
                    idle.frames += 1;
                }
                return;
            }
            if !idle.measuring {
                idle.measuring = true;
                idle.frames = 0;
                idle.deadline = Instant::now() + Duration::from_millis(500);
                wake_after(ctx, Duration::from_millis(500));
                return;
            }
            let frames = idle.frames;
            self.idle = None;
            self.report.lock().unwrap().steps.push(format!(
                "Repouso medido: {frames} atualizações espontâneas em 500 ms"
            ));
            if frames > 10 {
                self.fail(
                    ctx,
                    format!("Editor em repouso redesenhou {frames} vezes em 500 ms"),
                );
                return;
            }
        }
        if !self.events.is_empty() || self.pending_shot.is_some() {
            ctx.request_repaint();
            return;
        }
        if self.wait > 0 {
            self.wait -= 1;
            ctx.request_repaint();
            return;
        }
        match self.step(ctx) {
            Ok(true) => {
                let mut report = self.report.lock().unwrap();
                report.done = true;
                self.finished = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Ok(false) => {}
            Err(error) => self.fail(ctx, error),
        }
        if self.idle.is_none() {
            ctx.request_repaint();
        }
    }
}

fn capture_surface(ctx: &egui::Context) -> Surface {
    let shapes = ctx.graphics(|graphics| graphics.clone().drain(&[], &Default::default()));
    let mut surface = Surface::default();
    fn inspect(shape: &egui::Shape, clip: Rect, surface: &mut Surface) {
        match shape {
            egui::Shape::Text(text) => {
                let rect = text.visual_bounding_rect().intersect(clip);
                if rect.is_positive() {
                    surface.texts.push(TextTarget {
                        text: text.galley.text().to_owned(),
                        rect,
                    });
                }
            }
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    inspect(shape, clip, surface);
                }
            }
            egui::Shape::Rect(rect) if rect.fill == Color32::from_rgb(20, 24, 31) => {
                surface.canvas = Some(rect.rect.intersect(clip))
            }
            egui::Shape::Circle(circle)
                if circle.fill == Color32::WHITE && circle.radius > 2. && circle.radius < 10. =>
            {
                surface.ports.push(circle.center)
            }
            egui::Shape::Circle(circle)
                if circle.fill == Color32::from_rgb(239, 113, 117)
                    && (circle.radius - 6.).abs() < 0.1 =>
            {
                surface.gizmo_x = Some(circle.center)
            }
            egui::Shape::Mesh(mesh) if matches!(mesh.texture_id, egui::TextureId::Managed(_)) => {
                let rect = mesh.calc_bounds().intersect(clip);
                if rect.width() > 100.
                    && rect.height() > 100.
                    && surface.png.is_none_or(|old| rect.area() > old.area())
                {
                    surface.png = Some(rect);
                }
            }
            egui::Shape::Mesh(mesh) if matches!(mesh.texture_id, egui::TextureId::User(_)) => {
                let rect = mesh.calc_bounds().intersect(clip);
                if rect.is_positive()
                    && surface
                        .native_viewport
                        .is_none_or(|old| rect.area() > old.area())
                {
                    surface.native_viewport = Some(rect);
                }
            }
            _ => {}
        }
    }
    for shape in shapes {
        inspect(&shape.shape, shape.clip_rect, &mut surface);
    }
    if let Some(canvas) = surface.canvas {
        surface.ports.retain(|point| canvas.contains(*point));
    }
    surface
}

fn copy_directory(source: &Path, destination: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(destination)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let target = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_directory(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

#[test]
#[ignore = "Opens an inactive native WGPU window; injects input only into this app"]
#[cfg(target_os = "windows")]
fn native_editor_workflow() {
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = workspace.join("qa/v0.1.1");
    std::fs::create_dir_all(&output).unwrap();
    let fixture = std::env::temp_dir().join(format!("oxy-native-qa-{}", new_id()));
    copy_directory(&workspace.join("examples/validacao"), &fixture).unwrap();
    let report = Arc::new(Mutex::new(Report::default()));
    let result = report.clone();
    let artifacts = output.clone();
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default()
            .with_title("OXY Engine — QA nativo isolado")
            .with_inner_size([1440., 900.])
            .with_min_inner_size([920., 600.])
            .with_active(false),
        event_loop_builder: Some(Box::new(|builder| {
            builder.with_any_thread(true);
        })),
        ..Default::default()
    };
    eframe::run_native(
        "OXY Engine — QA nativo isolado",
        options,
        Box::new(move |cc| {
            Ok(Box::new(NativeQa::new(
                cc,
                fixture.join("project.oxy.json"),
                artifacts,
                result,
            )))
        }),
    )
    .unwrap();
    let report = report.lock().unwrap();
    let summary = format!(
        "Concluído: {}\nErro: {:?}\n\n{}\n\nCapturas: {:?}\n",
        report.done,
        report.error,
        report.steps.join("\n"),
        report.screenshots
    );
    std::fs::write(output.join("native-qa.txt"), &summary).unwrap();
    println!("{summary}");
    assert!(
        report.done && report.error.is_none(),
        "{}",
        report
            .error
            .as_deref()
            .unwrap_or("Janela fechada antes do fim")
    );
    assert_eq!(report.screenshots.len(), 13);
}
