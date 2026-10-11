//! Modeling help: topics at the left, the shortcuts of the chosen topic at the right.
use super::*;
use crate::{theme, widgets};
use egui::{Align, Layout, RichText, Stroke};

struct Topic {
    title: &'static str,
    lead: &'static str,
    rows: &'static [(&'static [&'static str], &'static str)],
    note: &'static str,
}

const TOPICS: &[Topic] = &[
    Topic {
        title: "Selecionar",
        lead: "Escolha o que editar: a peça inteira ou partes dela.",
        rows: &[
            (&["1", "2", "3", "4"], "Objeto · Face · Aresta · Vértice"),
            (&["Clique"], "Seleciona um componente"),
            (&["Ctrl", "Clique"], "Adiciona ou tira da seleção"),
            (&["Arrastar"], "Caixa: substitui pelos componentes visíveis"),
            (
                &["Ctrl", "Arrastar"],
                "Caixa: soma todos os tocados, inclusive ocultos",
            ),
            (&["Shift", "X"], "Selecionar através da peça"),
            (&["Shift", "I"], "Inverter a seleção"),
        ],
        note: "A tecla Ctrl é lida no início do gesto de caixa.",
    },
    Topic {
        title: "Transformar",
        lead: "Mover, girar e escalar o que está selecionado.",
        rows: &[
            (&["W", "E", "R"], "Mover · Girar · Escalar"),
            (&["Arrastar alça"], "Mostra a prévia; soltar aplica"),
            (
                &["Enter"],
                "Aplica um valor digitado (sair do campo também aplica)",
            ),
            (&["Esc"], "Cancela o gesto inteiro, mesmo com Alt ou Shift"),
            (&["Ctrl", "Z"], "Durante o gesto, só cancela"),
        ],
        note: "Armar uma ferramenta não altera a peça. Faces não planas precisam ser \
               trianguladas antes de deformar.",
    },
    Topic {
        title: "Extrudir e borda interna",
        lead: "Criar volume a partir das faces selecionadas.",
        rows: &[
            (&["Shift", "E"], "Extrudir"),
            (&["Shift", "U"], "Criar borda interna"),
            (
                &["Tamanho interno"],
                "Reduz em torno do centro: 50% é metade do tamanho",
            ),
            (
                &["Distância"],
                "Positiva ou negativa; Normal da face segue a orientação",
            ),
        ],
        note: "A moldura mantém o plano original. Funciona numa região plana e convexa ou em \
               cada face independente; buracos, regiões côncavas e interseções são recusados.",
    },
    Topic {
        title: "Corte e bisturi",
        lead: "Adicionar arestas na malha.",
        rows: &[
            (
                &["Corte em loop"],
                "Clique numa aresta, arraste para posicionar e solte",
            ),
            (&["Bisturi"], "Trace entre bordas da malha"),
            (&["Duplo clique"], "Termina o bisturi (Enter também)"),
        ],
        note: "Inverter orientação, triangular, criar face ou aresta e excluir são aplicados \
               na hora.",
    },
    Topic {
        title: "Última operação",
        lead: "Ajustar a operação que acabou de ser feita.",
        rows: &[
            (
                &["Propriedades"],
                "Recalcula a partir da origem e substitui o mesmo comando",
            ),
            (&["Ctrl", "Z"], "Volta ao estado anterior à operação"),
        ],
        note: "Outra edição ou a troca de peça encerra esse ajuste. UVs antigos mantêm a \
               pintura; se faltar espaço para as faces novas, escolha Criar cópia ampliada \
               (2×) ou cancele.",
    },
    Topic {
        title: "Forma e malha editável",
        lead: "Como uma forma pronta vira malha.",
        rows: &[
            (
                &["Modo Objeto"],
                "Mantém a forma paramétrica (lados, raio…)",
            ),
            (
                &["Primeira edição"],
                "Converte em malha editável; cancelar restaura os parâmetros",
            ),
            (
                &["Visualização"],
                "Grade, encaixe, seleção através e enquadrar",
            ),
        ],
        note: "A conversão mantém aparência, pivô, filhos, colisor e animações. Esconder a \
               grade não desliga o encaixe.",
    },
];

/// Key drawn as a small raised cap.
fn key_cap(ui: &mut egui::Ui, text: &str) {
    let font = egui::FontId::monospace(11.);
    let galley = ui
        .painter()
        .layout_no_wrap(text.into(), font, Color32::from_rgb(230, 230, 232));
    let (rect, _) = ui.allocate_exact_size(Vec2::new(galley.size().x + 12., 20.), Sense::hover());
    ui.painter().rect(
        rect,
        4.,
        theme::BG_RAISED,
        Stroke::new(1., Color32::from_rgb(74, 74, 80)),
        egui::StrokeKind::Inside,
    );
    ui.painter()
        .galley(rect.center() - galley.size() / 2., galley, theme::TEXT);
}

impl Editor {
    pub(in crate::app) fn mesh_help(&mut self, ctx: &egui::Context) {
        if !self.modeling.help {
            return;
        }
        let id = egui::Id::new("mesh_help_topic");
        let mut topic = ctx.data(|d| d.get_temp::<usize>(id)).unwrap_or(0);
        let mut close = false;
        let response = egui::Modal::new(egui::Id::new("mesh_help")).show(ctx, |ui| {
            let width = (ctx.content_rect().width() - 48.).clamp(260., 720.);
            ui.set_width(width);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Ajuda da modelagem")
                        .font(theme::medium(15.))
                        .color(theme::TEXT),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    close =
                        crate::icons::small(ui, crate::icons::Icon::Close, "Fechar", 26.).clicked();
                    ui.label(
                        RichText::new("Abra a qualquer momento com F1")
                            .size(12.)
                            .color(theme::TEXT_MUTED),
                    );
                });
            });
            ui.add_space(6.);
            let height = (ctx.content_rect().height() - 170.).clamp(140., 380.);
            let narrow = width < 520.;
            let body = |ui: &mut egui::Ui, topic: usize| {
                let current = &TOPICS[topic];
                egui::ScrollArea::vertical()
                    .id_salt("mesh_help_body")
                    .max_height(height)
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(current.title)
                                .font(theme::medium(14.))
                                .color(theme::TEXT),
                        );
                        widgets::hint(ui, current.lead);
                        ui.add_space(6.);
                        let keys = 168f32.min(ui.available_width() * 0.4);
                        for (caps, text) in current.rows {
                            ui.horizontal_top(|ui| {
                                ui.allocate_ui_with_layout(
                                    Vec2::new(keys, 22.),
                                    Layout::left_to_right(Align::Center).with_main_wrap(true),
                                    |ui| {
                                        ui.spacing_mut().item_spacing.x = 4.;
                                        for cap in *caps {
                                            key_cap(ui, cap);
                                        }
                                    },
                                );
                                ui.add(egui::Label::new(RichText::new(*text).size(12.5)).wrap());
                            });
                            let y = ui.cursor().top();
                            ui.painter().hline(
                                ui.max_rect().x_range(),
                                y,
                                Stroke::new(1., Color32::from_rgb(52, 52, 58)),
                            );
                            ui.add_space(4.);
                        }
                        ui.add_space(6.);
                        widgets::callout(ui, false, current.note);
                    });
            };
            if narrow {
                ui.horizontal_wrapped(|ui| {
                    for (i, t) in TOPICS.iter().enumerate() {
                        if ui.selectable_label(i == topic, t.title).clicked() {
                            topic = i;
                        }
                    }
                });
                ui.separator();
                body(ui, topic);
            } else {
                // Fixed rectangles: inside a modal the available height is unbounded, so a
                // vertical separator or a growing column would stretch to the whole screen.
                let (area, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
                let nav = Rect::from_min_size(area.min, Vec2::new(190., height));
                ui.painter().vline(
                    nav.right() + 8.,
                    area.y_range(),
                    Stroke::new(1., theme::CARD_BORDER),
                );
                let content =
                    Rect::from_min_max(egui::pos2(nav.right() + 20., area.top()), area.max);
                let mut right = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(content)
                        .layout(Layout::top_down(Align::Min)),
                );
                body(&mut right, topic);
                let mut left = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(nav)
                        .layout(Layout::top_down(Align::Min)),
                );
                {
                    let ui = &mut left;
                    {
                        for (i, t) in TOPICS.iter().enumerate() {
                            let selected = i == topic;
                            let text = RichText::new(t.title).size(12.5).color(if selected {
                                Color32::WHITE
                            } else {
                                theme::TEXT_MUTED
                            });
                            if ui
                                .add_sized(
                                    [190., 28.],
                                    egui::Button::new(text)
                                        .fill(if selected {
                                            theme::ACCENT_SOFT
                                        } else {
                                            Color32::TRANSPARENT
                                        })
                                        .stroke(Stroke::NONE),
                                )
                                .clicked()
                            {
                                topic = i;
                            }
                        }
                    }
                }
            }
            widgets::dialog_footer(ui, |ui| {
                close |= widgets::secondary_button(ui, "Fechar ajuda").clicked();
            });
        });
        ctx.data_mut(|d| d.insert_temp(id, topic));
        if close || response.should_close() || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.modeling.help = false;
        }
    }
}
