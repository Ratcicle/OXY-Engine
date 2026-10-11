use super::*;
use std::time::Duration;

/// One diagnostics row: label, value and an optional explanation.
type Stat = (&'static str, String, &'static str);

pub(super) fn repaint_delay(
    game: bool,
    animation: bool,
    interacting: bool,
    diagnostics: bool,
) -> Option<Duration> {
    if game || animation || interacting {
        Some(Duration::from_millis(16))
    } else if diagnostics {
        Some(Duration::from_millis(500))
    } else {
        None
    }
}
impl Editor {
    pub(super) fn diagnostics_ui(&mut self, ctx: &egui::Context) {
        if !self.frame.visible {
            return;
        }
        let stats = self.renderer.stats();
        let scene = self
            .play
            .runtime
            .as_ref()
            .filter(|_| self.showing_game())
            .map_or(self.scene(), |r| r.scene());
        let count = scene.entities.len();
        let view = oxy_core::scene_view::SceneView::new(scene);
        let visible = scene.entities.iter().filter(|e| view.visible(e)).count();
        let nodes = scene
            .entities
            .iter()
            .map(|e| e.graph.nodes.len())
            .sum::<usize>();
        let retained = self
            .play
            .runtime
            .as_ref()
            .map_or([0; 4], |r| r.retained_counts());
        let counters = oxy_core::metrics::take();
        let (component_queries, component_cpu_bytes) = self.component_selection_stats();
        let mb = |bytes: f64| format!("{:.2} MiB", bytes / 1_048_576.);
        let ms = |ns: u64| format!("{:.3} ms", ns as f64 / 1e6);
        let mut sections: Vec<(&str, Vec<Stat>)> = vec![
            (
                "QUADRO",
                vec![
                    (
                        "Redesenho",
                        format!("{:.1} FPS", 1000. / self.frame.interval_ms.max(0.001)),
                        "Frequência observada entre as duas últimas atualizações. O editor em \
                         repouso redesenha apenas quando necessário.",
                    ),
                    ("Intervalo", format!("{:.2} ms", self.frame.interval_ms), ""),
                    ("Trabalho CPU", format!("{:.2} ms", self.frame.cpu_ms), ""),
                ],
            ),
            (
                "CENA",
                vec![
                    ("Objetos", format!("{count} · {visible} visíveis"), ""),
                    (
                        "Geometrias",
                        format!("{} · {} chamadas", stats.visible_objects, stats.draw_calls),
                        "Geometrias desenhadas e chamadas de desenho.",
                    ),
                    (
                        "Vértices",
                        format!("{} · {} triângulos", stats.vertices, stats.triangles),
                        "",
                    ),
                    (
                        "Malhas",
                        format!("{} em cache · {} envios", stats.meshes, stats.mesh_uploads),
                        "",
                    ),
                    (
                        "Componentes",
                        format!(
                            "{} chamadas · {} geometria · {} seleção",
                            stats.component_draw_calls,
                            stats.component_mesh_uploads,
                            stats.component_selection_uploads
                        ),
                        "Chamadas de desenho e envios de geometria e de seleção dos componentes.",
                    ),
                    (
                        "Projeções",
                        format!(
                            "{} criadas / {} reutilizadas",
                            component_queries.projection_builds, component_queries.projection_hits
                        ),
                        "",
                    ),
                    (
                        "Oclusão",
                        format!(
                            "{} criada / {} reutilizada",
                            component_queries.occlusion_builds, component_queries.occlusion_hits
                        ),
                        "",
                    ),
                ],
            ),
            (
                "MEMÓRIA",
                vec![
                    (
                        "Componentes",
                        format!(
                            "{} CPU · {} GPU",
                            mb(component_cpu_bytes as f64),
                            mb(stats.component_buffer_bytes as f64)
                        ),
                        "CPU estimada e buffers GPU alocados para os componentes.",
                    ),
                    (
                        "Texturas",
                        format!(
                            "{} GPU · {} editáveis",
                            stats.textures,
                            self.state.images.len()
                        ),
                        "Texturas na GPU e imagens com pixels editáveis em memória.",
                    ),
                    (
                        "Imagens CPU",
                        mb(self.state.images.resident_bytes() as f64),
                        "",
                    ),
                    (
                        "Histórico",
                        mb(self.history.estimated_bytes() as f64),
                        "Estimativa do que Desfazer guarda.",
                    ),
                ],
            ),
            (
                "LÓGICA E JOGO",
                vec![
                    (
                        "Nós",
                        format!(
                            "{nodes} · {} tarefas pendentes",
                            self.play.runtime.as_ref().map_or(0, |r| r.pending_tasks())
                        ),
                        "",
                    ),
                    (
                        "Áreas",
                        format!(
                            "{} acertos · {} ativas · {} prontas · {} em espera",
                            retained[0], retained[1], retained[2], retained[3]
                        ),
                        "Acertos vivos e áreas ativas, prontas e em espera.",
                    ),
                ],
            ),
        ];
        if oxy_core::metrics::ENABLED {
            let mut rows = vec![
                ("Passos", counters.steps.to_string(), ""),
                ("Movimento", ms(counters.movement_ns), ""),
                ("Áreas", ms(counters.areas_ns), ""),
                ("Ações", ms(counters.tasks_ns), ""),
                ("Passos completos", ms(counters.fixed_step_ns), ""),
                (
                    "Apresentação",
                    ms(counters.presentation_ns),
                    "Apresentação e câmera.",
                ),
                ("Preparação 3D", ms(counters.character_prepare_ns), ""),
                ("Motor", ms(counters.character_motor_ns), ""),
                ("Sensores", ms(counters.character_sensors_ns), ""),
                (
                    "Resolução",
                    ms(counters.character_resolve_ns),
                    "Já incluída no motor. Não somar novamente.",
                ),
            ];
            if let Some(world) = self.play.runtime.as_ref().and_then(|r| r.physics_world()) {
                let p = world.counters();
                rows.push((
                    "Física 3D",
                    format!(
                        "{} formas · {} preparações · {} consultas · {} candidatos",
                        p.colliders, p.shapes_prepared, p.queries, p.candidate_tests
                    ),
                    "Formas vivas; as demais contagens são acumuladas.",
                ));
            }
            rows.extend([
                (
                    "Consultas",
                    format!(
                        "{} · {} visitas de hierarquia",
                        counters.entity_queries, counters.hierarchy_visits
                    ),
                    "",
                ),
                (
                    "Matrizes",
                    format!(
                        "{} · {} reutilizadas",
                        counters.matrices, counters.matrix_hits
                    ),
                    "",
                ),
                (
                    "Índices",
                    format!(
                        "{} construídos · {} candidatos · {} sobreposições · {} ações",
                        counters.index_builds,
                        counters.candidates,
                        counters.overlaps,
                        counters.actions
                    ),
                    "",
                ),
            ]);
            sections.push(("INSTRUMENTAÇÃO", rows));
        }
        let mut open = self.frame.visible;
        egui::Window::new("Desempenho")
            .id(egui::Id::new("performance_diagnostics"))
            .open(&mut open)
            .default_width(360.)
            .collapsible(false)
            .resizable(false)
            .frame(
                egui::Frame::window(&ctx.style())
                    .fill(crate::theme::BG_WINDOW)
                    .inner_margin(12),
            )
            .show(ctx, |ui| {
                use crate::theme;
                egui::ScrollArea::vertical()
                    .max_height((ctx.content_rect().height() - 160.).max(160.))
                    .show(ui, |ui| {
                        for (index, (title, rows)) in sections.iter().enumerate() {
                            if index > 0 {
                                ui.add_space(8.);
                            }
                            ui.label(
                                egui::RichText::new(*title)
                                    .font(theme::medium(11.))
                                    .color(theme::TEXT_MUTED),
                            );
                            egui::Grid::new(("performance_section", index))
                                .num_columns(2)
                                .min_col_width(110.)
                                .spacing([12., 4.])
                                .show(ui, |ui| {
                                    for (label, value, hint) in rows {
                                        let response = ui.label(
                                            egui::RichText::new(*label)
                                                .size(12.)
                                                .color(theme::TEXT_MUTED),
                                        );
                                        if !hint.is_empty() {
                                            response.on_hover_text(*hint);
                                        }
                                        ui.label(egui::RichText::new(value).monospace().size(12.));
                                        ui.end_row();
                                    }
                                });
                        }
                        ui.add_space(8.);
                        crate::widgets::hint(
                            ui,
                            "Atualiza duas vezes por segundo em repouso. Tempos de CPU, sem \
                             GPU nem VSync; não mede RAM total nem VRAM.",
                        );
                    });
            });
        self.frame.visible = open;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn idle_editor_has_no_continuous_repaint() {
        assert_eq!(repaint_delay(false, false, false, false), None);
        assert_eq!(
            repaint_delay(false, false, false, true),
            Some(Duration::from_millis(500))
        );
        for active in [
            (true, false, false),
            (false, true, false),
            (false, false, true),
        ] {
            assert_eq!(
                repaint_delay(active.0, active.1, active.2, false),
                Some(Duration::from_millis(16))
            );
        }
    }
}
