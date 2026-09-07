use crate::app::{Editor, vector3};
use egui::{Color32, Pos2, Rect, Sense, Vec2};
use oxy_core::{
    animation::{AnimationEvent, Clip, Interpolation, Keyframe, sample_clip},
    document::*,
    painting::PaintImage,
};

#[derive(Clone, Copy, PartialEq)]
pub enum StudioTab {
    Model,
    Paint,
    Animation,
}
#[derive(Clone, Copy, PartialEq)]
enum PaintTool {
    Brush,
    Fill,
    Sample,
}
pub struct Studio {
    pub tab: StudioTab,
    pub owner: Option<Id>,
    clip: Option<Id>,
    time: f32,
    pub playing: bool,
    preview: bool,
    key: Option<(Id, f32)>,
    clipboard: Option<Keyframe>,
    color: [u8; 4],
    radius: f32,
    tool: PaintTool,
    pub shared_edit: Option<Id>,
    pub hover_uv: Option<[f32; 2]>,
    last_pixel: Option<[f32; 2]>,
    texture: Option<egui::TextureHandle>,
    marker_name: String,
}
impl Default for Studio {
    fn default() -> Self {
        Self {
            tab: StudioTab::Model,
            owner: None,
            clip: None,
            time: 0.,
            playing: false,
            preview: false,
            key: None,
            clipboard: None,
            color: [198, 82, 63, 255],
            radius: 7.,
            tool: PaintTool::Brush,
            shared_edit: None,
            hover_uv: None,
            last_pixel: None,
            texture: None,
            marker_name: "Impacto".into(),
        }
    }
}
impl Editor {
    pub fn studio_ui(&mut self, ui: &mut egui::Ui, dt: f32) {
        ui.horizontal_wrapped(|ui| {
            ui.heading("Estúdio");
            ui.separator();
            ui.selectable_value(&mut self.studio.tab, StudioTab::Model, "Modelagem");
            ui.selectable_value(&mut self.studio.tab, StudioTab::Paint, "Pintura");
            ui.selectable_value(&mut self.studio.tab, StudioTab::Animation, "Animação");
        });
        if self.selected.is_none() {
            ui.label("Selecione uma peça ou use + Objeto na hierarquia. Todas as peças continuam editáveis.");
        }
        match self.studio.tab {
            StudioTab::Model => {
                ui.label("Adicione formas em + Objeto. No Estúdio, a peça criada recebe a peça selecionada como pai. Dimensões, pivô e parentesco ficam nas propriedades.");
                self.viewport(ui, false);
            }
            StudioTab::Paint => self.paint_ui(ui),
            StudioTab::Animation => self.animation_ui(ui, dt),
        }
    }
    fn texture_id(&self) -> Option<Id> {
        self.selected
            .as_deref()
            .and_then(|id| self.scene().entity(id))
            .and_then(|e| e.material.texture.clone())
    }
    fn texture_uses(&self, id: &str) -> usize {
        self.state
            .project
            .scenes
            .iter()
            .flat_map(|s| &s.entities)
            .chain(
                self.state
                    .project
                    .assets
                    .iter()
                    .filter_map(|a| a.model.as_ref())
                    .flatten(),
            )
            .filter(|e| {
                e.material.texture.as_deref() == Some(id)
                    || e.ui.as_ref().and_then(|u| u.texture.as_deref()) == Some(id)
            })
            .count()
    }
    fn create_texture(&mut self, size: u32) {
        let Some(entity_id) = self.selected.clone() else {
            return;
        };
        let id = new_id();
        let name = format!(
            "Pintura {size} · {}",
            self.scene()
                .entity(&entity_id)
                .map(|e| e.name.as_str())
                .unwrap_or("peça")
        );
        match PaintImage::new(size, size, [235, 231, 218, 255]) {
            Ok(image) => {
                self.state.images.insert(id.clone(), image);
                self.state.project.assets.push(Asset {
                    id: id.clone(),
                    name,
                    path: format!("assets/{id}.png"),
                    kind: AssetKind::Texture,
                    model: None,
                });
                if let Some(e) = self.scene_mut().entity_mut(&entity_id) {
                    e.material.texture = Some(id.clone());
                    e.material.color = [1.; 4];
                }
                self.refresh_texture(&id);
                self.studio.shared_edit = Some(id);
            }
            Err(e) => self.log(e),
        }
    }
    fn copy_texture(&mut self, id: &str) {
        let Some(image) = self.state.images.get(id).cloned() else {
            return;
        };
        let Some(entity_id) = self.selected.clone() else {
            return;
        };
        let copy = new_id();
        let name = self
            .state
            .project
            .asset(id)
            .map(|a| format!("{} (cópia)", a.name))
            .unwrap_or("Textura independente".into());
        self.state.project.assets.push(Asset {
            id: copy.clone(),
            name,
            path: format!("assets/{copy}.png"),
            kind: AssetKind::Texture,
            model: None,
        });
        self.state.images.insert(copy.clone(), image);
        if let Some(e) = self.scene_mut().entity_mut(&entity_id) {
            e.material.texture = Some(copy.clone());
        }
        self.refresh_texture(&copy);
        self.studio.shared_edit = Some(copy);
    }
    pub fn paint_at_uv(&mut self, uv: [f32; 2]) {
        let Some(id) = self.texture_id() else { return };
        if self.texture_uses(&id) > 1 && self.studio.shared_edit.as_ref() != Some(&id) {
            return;
        }
        let Some(image) = self.state.images.get_mut(&id) else {
            return;
        };
        let [x, y] = image.uv_pixel(uv);
        let point = [x as f32 + 0.5, y as f32 + 0.5];
        let changed = match self.studio.tool {
            PaintTool::Brush => {
                let previous = self.studio.last_pixel.unwrap_or(point);
                self.studio.last_pixel = Some(point);
                if (previous[0] - point[0]).abs() > image.width as f32 * 0.3
                    || (previous[1] - point[1]).abs() > image.height as f32 * 0.3
                {
                    image.brush(point, self.studio.radius, self.studio.color)
                } else {
                    image.stroke(previous, point, self.studio.radius, self.studio.color)
                }
            }
            PaintTool::Fill => image.flood_fill(x, y, self.studio.color),
            PaintTool::Sample => {
                self.studio.color = image.pixel(x, y);
                false
            }
        };
        if changed {
            self.refresh_texture(&id);
        }
    }
    fn paint_ui(&mut self, ui: &mut egui::Ui) {
        if !ui.input(|i| i.pointer.primary_down()) {
            self.studio.last_pixel = None;
        }
        let Some(_entity) = self.selected.clone() else {
            self.viewport(ui, false);
            return;
        };
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.studio.tool, PaintTool::Brush, "Pincel");
            ui.selectable_value(&mut self.studio.tool, PaintTool::Fill, "Preencher região");
            ui.selectable_value(&mut self.studio.tool, PaintTool::Sample, "Conta-gotas");
            ui.color_edit_button_srgba_unmultiplied(&mut self.studio.color);
            ui.add(egui::Slider::new(&mut self.studio.radius, 0.5..=64.).text("Raio px"));
            if ui.button("Enquadrar peça").clicked() {
                self.frame_selection();
            }
        });
        ui.horizontal_wrapped(|ui| {
            for color in [
                [235, 231, 218, 255],
                [38, 44, 55, 255],
                [184, 64, 53, 255],
                [213, 165, 77, 255],
                [91, 168, 152, 255],
                [86, 127, 181, 255],
                [122, 84, 152, 255],
                [0, 0, 0, 0],
            ] {
                let c = Color32::from_rgba_unmultiplied(color[0], color[1], color[2], color[3]);
                if ui.add(egui::Button::new("   ").fill(c)).clicked() {
                    self.studio.color = color;
                }
            }
            if self.texture_id().is_none() {
                if ui.button("Criar 256×256").clicked() {
                    self.create_texture(256)
                }
                if ui.button("Criar 512×512").clicked() {
                    self.create_texture(512)
                }
            }
            if ui.button("Importar PNG").clicked() {
                self.import(AssetKind::Texture);
            }
            if ui
                .add_enabled(
                    self.texture_id().is_some(),
                    egui::Button::new("Exportar PNG"),
                )
                .clicked()
                && let Some(id) = self.texture_id()
                && let Some(image) = self.state.images.get(&id)
                && let Some(path) = rfd::FileDialog::new()
                    .add_filter("PNG", &["png"])
                    .set_file_name("pintura.png")
                    .save_file()
            {
                let result = image.save(&path);
                if let Err(e) = result {
                    self.log(e)
                } else {
                    self.log("PNG exportado com pixels reais.");
                }
            }
        });
        let Some(id) = self.texture_id() else {
            ui.label("Crie ou importe uma textura para começar a pintar.");
            self.viewport(ui, true);
            return;
        };
        let uses = self.texture_uses(&id);
        if uses > 1 && self.studio.shared_edit.as_ref() != Some(&id) {
            ui.group(|ui|{ui.colored_label(Color32::from_rgb(232,190,113),format!("Esta textura está vinculada a {uses} peças ou imagens. A pintura afetará todas essas referências."));ui.horizontal(|ui|{if ui.button("Criar cópia independente").clicked(){self.copy_texture(&id)}
if ui.button("Editar textura compartilhada").clicked(){self.studio.shared_edit=Some(id.clone());}});});
        }
        ui.label("Esquerda: pixels do PNG e regiões UV. Direita: pintura direta na peça selecionada. Ctrl+Z desfaz a pincelada inteira.");
        let width = (ui.available_width() * 0.48).max(100.);
        egui::SidePanel::left("paint_pixels")
            .resizable(true)
            .default_width(width)
            .width_range(100.0..=1200.0)
            .show_inside(ui, |ui| {
                if let Some(image) = self.state.images.get(&id) {
                    let color_image = egui::ColorImage::from_rgba_unmultiplied(
                        [image.width as usize, image.height as usize],
                        &image.pixels,
                    );
                    if let Some(handle) = &mut self.studio.texture {
                        handle.set(color_image, egui::TextureOptions::NEAREST);
                    } else {
                        self.studio.texture = Some(ui.ctx().load_texture(
                            "oxy_paint",
                            color_image,
                            egui::TextureOptions::NEAREST,
                        ));
                    }
                    ui.label(format!("{} × {} · RGBA", image.width, image.height));
                    let ratio = image.width as f32 / image.height as f32;
                    let h = (ui.available_width() / ratio).min(ui.available_height().max(10.));
                    let size = Vec2::new(h * ratio, h);
                    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
                    for y in 0..((rect.height() / 16.).ceil() as usize) {
                        for x in 0..((rect.width() / 16.).ceil() as usize) {
                            let r = Rect::from_min_size(
                                rect.min + Vec2::new(x as f32 * 16., y as f32 * 16.),
                                Vec2::splat(16.),
                            )
                            .intersect(rect);
                            ui.painter().rect_filled(
                                r,
                                0.,
                                if (x + y) % 2 == 0 {
                                    Color32::from_gray(62)
                                } else {
                                    Color32::from_gray(90)
                                },
                            );
                        }
                    }
                    ui.painter().image(
                        self.studio.texture.as_ref().unwrap().id(),
                        rect,
                        Rect::from_min_max(Pos2::ZERO, Pos2::new(1., 1.)),
                        Color32::WHITE,
                    );
                    let primitive = self
                        .selected
                        .as_deref()
                        .and_then(|i| self.scene().entity(i))
                        .and_then(|e| e.primitive);
                    let uv_color = Color32::from_rgba_unmultiplied(70, 235, 216, 160);
                    let rows = if primitive == Some(Primitive::Cube)
                        || primitive == Some(Primitive::Cylinder)
                    {
                        2
                    } else {
                        1
                    };
                    let cols = if primitive == Some(Primitive::Cube) {
                        3
                    } else {
                        1
                    };
                    for row in 0..rows {
                        for col in 0..cols {
                            let uv_rect = Rect::from_min_size(
                                rect.min
                                    + Vec2::new(
                                        rect.width() * col as f32 / cols as f32,
                                        rect.height() * row as f32 / rows as f32,
                                    ),
                                Vec2::new(rect.width() / cols as f32, rect.height() / rows as f32),
                            );
                            ui.painter().rect_stroke(
                                uv_rect,
                                0.,
                                egui::Stroke::new(1., uv_color),
                                egui::StrokeKind::Inside,
                            );
                        }
                    }
                    if let Some(uv) = self.studio.hover_uv {
                        let p = rect.min + Vec2::new(uv[0] * rect.width(), uv[1] * rect.height());
                        ui.painter()
                            .circle_stroke(p, 5., egui::Stroke::new(1.5, Color32::WHITE));
                    }
                    let press = ui.input(|i| {
                        i.events.iter().find_map(|event| match event {
                            egui::Event::PointerButton {
                                pos,
                                button: egui::PointerButton::Primary,
                                pressed: true,
                                ..
                            } if rect.contains(*pos) => Some(*pos),
                            _ => None,
                        })
                    });
                    if let Some(p) = press {
                        self.studio.last_pixel = Some([
                            (p.x - rect.left()) / rect.width() * image.width as f32,
                            (p.y - rect.top()) / rect.height() * image.height as f32,
                        ]);
                    }
                    if (response.dragged_by(egui::PointerButton::Primary)
                        || response.clicked()
                        || press.is_some())
                        && let Some(p) = response.interact_pointer_pos()
                    {
                        let uv = [
                            (p.x - rect.min.x) / rect.width(),
                            (p.y - rect.min.y) / rect.height(),
                        ];
                        self.paint_at_uv(uv);
                    }
                } else {
                    ui.colored_label(
                        Color32::LIGHT_RED,
                        "Não foi possível carregar os pixels desta textura.",
                    );
                }
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show_inside(ui, |ui| {
                self.viewport(ui, true);
            });
    }
    fn chosen_clip(&self) -> Option<&Clip> {
        self.studio
            .owner
            .as_deref()
            .and_then(|id| self.scene().entity(id))
            .and_then(|e| {
                e.clips
                    .iter()
                    .find(|c| Some(&c.id) == self.studio.clip.as_ref())
            })
    }
    pub fn animation_preview(&self) -> Scene {
        let mut scene = self.scene().clone();
        if self.studio.preview
            && let Some(clip) = self.chosen_clip()
        {
            sample_clip(&mut scene, clip, self.studio.time);
        }
        scene
    }
    fn animation_ui(&mut self, ui: &mut egui::Ui, dt: f32) {
        if self
            .studio
            .owner
            .as_ref()
            .is_none_or(|id| self.scene().entity(id).is_none())
        {
            self.studio.owner = self.selected.clone();
        }
        let objects: Vec<_> = self
            .scene()
            .entities
            .iter()
            .map(|e| (e.id.clone(), e.name.clone()))
            .collect();
        ui.horizontal(|ui| {
            ui.label("Objeto que possui o clip");
            crate::graph_ui::object_picker(ui, &mut self.studio.owner, &objects, "clip_owner");
            if ui.button("+ Clip").clicked()
                && let Some(id) = self.studio.owner.clone()
            {
                let clip = Clip::new("Novo clip");
                self.studio.clip = Some(clip.id.clone());
                if let Some(e) = self.scene_mut().entity_mut(&id) {
                    e.clips.push(clip);
                }
            }
            if ui.button("Pose-base").clicked() {
                self.studio.preview = false;
                self.studio.playing = false;
            }
        });
        let Some(owner) = self.studio.owner.clone() else {
            self.viewport(ui, false);
            return;
        };
        let clips = self
            .scene()
            .entity(&owner)
            .map(|e| e.clips.clone())
            .unwrap_or_default();
        if self
            .studio
            .clip
            .as_ref()
            .is_none_or(|id| !clips.iter().any(|c| &c.id == id))
        {
            self.studio.clip = clips.first().map(|c| c.id.clone());
        }
        let Some(mut clip) = clips
            .iter()
            .find(|c| Some(&c.id) == self.studio.clip.as_ref())
            .cloned()
        else {
            ui.label("Crie um clip, ajuste a pose e insira keyframes. As trilhas podem apontar para qualquer peça da hierarquia.");
            self.viewport(ui, false);
            return;
        };
        ui.horizontal_wrapped(|ui| {
            egui::ComboBox::from_id_salt("clip_select")
                .selected_text(&clip.name)
                .show_ui(ui, |ui| {
                    for c in &clips {
                        ui.selectable_value(&mut self.studio.clip, Some(c.id.clone()), &c.name);
                    }
                });
            ui.add(egui::TextEdit::singleline(&mut clip.name).desired_width(140.));
            let end = clip
                .tracks
                .iter()
                .flat_map(|t| t.keyframes.iter().map(|k| k.time))
                .chain(clip.events.iter().map(|e| e.time))
                .fold(0.1, f32::max);
            ui.add(
                egui::DragValue::new(&mut clip.duration)
                    .range(end..=600.)
                    .speed(0.05)
                    .suffix(" s"),
            );
            ui.checkbox(&mut clip.looping, "Repetir");
            if ui
                .button(if self.studio.playing {
                    "Ⅱ Pausar"
                } else {
                    "▶ Prévia"
                })
                .clicked()
            {
                self.studio.playing = !self.studio.playing;
                self.studio.preview = true;
                if self.studio.time >= clip.duration {
                    self.studio.time = 0.;
                }
            }
            if ui.button("Keyframe da peça").clicked()
                && let Some(id) = &self.selected
                && let Some(e) = self.scene().entity(id)
            {
                clip.insert_key(
                    id,
                    Keyframe {
                        time: self.studio.time.min(clip.duration),
                        transform: e.transform.clone(),
                        interpolation: Interpolation::Linear,
                    },
                );
                self.studio.key = Some((id.clone(), self.studio.time));
            }
            if ui
                .add_enabled(
                    self.studio.clipboard.is_some() && self.selected.is_some(),
                    egui::Button::new("Colar keyframe"),
                )
                .clicked()
                && let (Some(mut key), Some(id)) =
                    (self.studio.clipboard.clone(), self.selected.clone())
            {
                key.time = self.studio.time.min(clip.duration);
                clip.insert_key(&id, key);
            }
        });
        if self.studio.playing {
            self.studio.time += dt;
            if self.studio.time > clip.duration {
                if clip.looping {
                    self.studio.time %= clip.duration;
                } else {
                    self.studio.time = clip.duration;
                    self.studio.playing = false;
                }
            }
        }
        if ui
            .add(egui::Slider::new(&mut self.studio.time, 0.0..=clip.duration).text("Cursor (s)"))
            .changed()
        {
            self.studio.preview = true;
            self.studio.playing = false;
        }
        egui::TopBottomPanel::bottom("timeline").resizable(true).default_height(240.).height_range(140.0..=500.0).show_inside(ui,|ui| {
            ui.horizontal(|ui|{ui.strong("LINHA DO TEMPO");ui.small("Arraste um quadro para mudar o instante. A prévia nunca grava a pose-base.");});
            let ids=self.scene().descendants(&owner);
            let lanes:Vec<_>=objects.iter().filter(|(id,_)|ids.contains(id)).cloned().collect();
            egui::ScrollArea::vertical().max_height(130.).show(ui,|ui|{
                for (target,name) in lanes {
                    ui.horizontal(|ui|{
                        let label=ui.add_sized([110.,22.],egui::Button::selectable(self.selected.as_ref()==Some(&target),&name));if label.clicked(){self.select(Some(target.clone()));}
                        let (rect,response)=ui.allocate_exact_size(Vec2::new(ui.available_width(),25.),Sense::click());ui.painter().rect_filled(rect,2.,Color32::from_rgb(34,41,49));
                        for i in 0..=10{let x=rect.left()+rect.width()*i as f32/10.;ui.painter().line_segment([Pos2::new(x,rect.top()),Pos2::new(x,rect.bottom())],egui::Stroke::new(1.,Color32::from_gray(61)));}
                        let cursor=rect.left()+rect.width()*self.studio.time/clip.duration;ui.painter().line_segment([Pos2::new(cursor,rect.top()),Pos2::new(cursor,rect.bottom())],egui::Stroke::new(2.,Color32::from_rgb(122,224,204)));
                        if response.clicked()&& let Some(pos)=response.interact_pointer_pos(){self.studio.time=((pos.x-rect.left())/rect.width()*clip.duration).clamp(0.,clip.duration);self.studio.preview=true;self.studio.playing=false;}
                        if let Some(track)=clip.tracks.iter_mut().find(|t|t.target==target){
                            for (index,key) in track.keyframes.iter_mut().enumerate(){let pos=Pos2::new(rect.left()+rect.width()*key.time/clip.duration,rect.center().y);let selected=self.studio.key.as_ref().is_some_and(|(id,time)|id==&target&&(*time-key.time).abs()<0.00001);ui.painter().circle_filled(pos,if selected{6.}else{4.5},if selected{Color32::GOLD}else{Color32::from_rgb(138,202,221)});
                                let r=ui.interact(Rect::from_center_size(pos,Vec2::splat(16.)),ui.id().with((&target,index)),Sense::click_and_drag());
                                if r.clicked()||r.drag_started(){self.studio.key=Some((target.clone(),key.time));self.studio.time=key.time;self.studio.preview=true;self.studio.playing=false;}
                                if r.dragged()&& let Some(pos)=r.interact_pointer_pos(){key.time=(((pos.x-rect.left())/rect.width()*clip.duration*60.).round()/60.).clamp(0.,clip.duration);self.studio.key=Some((target.clone(),key.time));self.studio.time=key.time;}
                            }
                            if !ui.input(|i|i.pointer.primary_down()){track.keyframes.sort_by(|a,b|a.time.total_cmp(&b.time));track.keyframes.dedup_by(|a,b|(a.time-b.time).abs()<0.00001);}
                        }
                    });
                }
            });
            ui.horizontal_wrapped(|ui|{
                ui.label("Evento");ui.add(egui::TextEdit::singleline(&mut self.studio.marker_name).desired_width(120.));if ui.button("+ Marcador no cursor").clicked()&&!self.studio.marker_name.trim().is_empty(){clip.events.push(AnimationEvent{time:self.studio.time,name:self.studio.marker_name.clone()});}
                let mut delete=None;for (i,event) in clip.events.iter_mut().enumerate(){ui.push_id(i,|ui|{ui.label(&event.name);ui.add(egui::DragValue::new(&mut event.time).range(0.0..=clip.duration).speed(0.01));if ui.small_button("×").clicked(){delete=Some(i);}});}
if let Some(i)=delete{clip.events.remove(i);}
            });
        });
        if let Some((target, time)) = self.studio.key.clone() {
            let mut remove = false;
            egui::SidePanel::left("key_properties")
                .resizable(true)
                .default_width(235.)
                .width_range(210.0..=400.0)
                .show_inside(ui, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        ui.strong("POSE DO KEYFRAME");
                        if let Some(key) = clip
                            .tracks
                            .iter_mut()
                            .find(|t| t.target == target)
                            .and_then(|t| {
                                t.keyframes
                                    .iter_mut()
                                    .find(|k| (k.time - time).abs() < 0.00001)
                            })
                        {
                            ui.label(format!("Instante {:.3} s", key.time));
                            vector3(ui, "Posição", &mut key.transform.position, 0.02, false);
                            let mut degrees = key.transform.rotation.map(f32::to_degrees);
                            let original_degrees = degrees;
                            vector3(ui, "Rotação °", &mut degrees, 0.5, false);
                            if degrees != original_degrees {
                                key.transform.rotation = degrees.map(f32::to_radians);
                            }
                            vector3(ui, "Escala", &mut key.transform.scale, 0.02, true);
                            ui.selectable_value(
                                &mut key.interpolation,
                                Interpolation::Linear,
                                "Linear / rotação slerp",
                            );
                            ui.selectable_value(
                                &mut key.interpolation,
                                Interpolation::Hold,
                                "Manter pose",
                            );
                            if ui.button("Copiar keyframe").clicked() {
                                self.studio.clipboard = Some(key.clone());
                            }
                            if ui.button("Excluir keyframe").clicked() {
                                remove = true;
                            }
                        }
                    });
                });
            if remove {
                if let Some(track) = clip.tracks.iter_mut().find(|t| t.target == target) {
                    track.keyframes.retain(|k| (k.time - time).abs() > 0.00001);
                }
                self.studio.key = None;
            }
        }
        if let Some(e) = self.scene_mut().entity_mut(&owner)
            && let Some(existing) = e.clips.iter_mut().find(|c| c.id == clip.id)
        {
            *existing = clip;
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show_inside(ui, |ui| {
                self.viewport(ui, false);
            });
    }
}
