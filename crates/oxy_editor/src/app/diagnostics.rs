use super::*;
use std::time::Duration;

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
        if !self.diagnostics {
            return;
        }
        let stats = self.renderer.stats();
        let scene = self
            .runtime
            .as_ref()
            .filter(|_| self.tab == Tab::Game)
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
            .runtime
            .as_ref()
            .map_or([0; 4], |r| r.retained_counts());
        let counters = oxy_core::metrics::take();
        egui::Window::new("Diagnóstico de desempenho").open(&mut self.diagnostics).default_width(340.).show(ctx,|ui| {
            ui.label(format!("FPS de redesenho: {:.1}",1000./self.frame_interval_ms.max(0.001))).on_hover_text("Frequência observada entre as duas últimas atualizações. O editor em repouso redesenha apenas quando necessário.");
            ui.label(format!("Intervalo: {:.2} ms · trabalho CPU: {:.2} ms",self.frame_interval_ms,self.frame_cpu_ms));
            ui.separator();ui.label(format!("Objetos: {count} · visíveis: {visible}"));
            ui.label(format!("Geometrias desenhadas: {} · chamadas de desenho: {}",stats.visible_objects,stats.draw_calls));
            ui.label(format!("Vértices: {} · triângulos: {}",stats.vertices,stats.triangles));
            ui.label(format!("Malhas em cache: {} · envios de malha: {}",stats.meshes,stats.mesh_uploads));
            ui.label(format!("Texturas GPU: {} · pixels editáveis em memória: {}",stats.textures,self.state.images.len()));
            ui.label(format!("Imagens CPU: {:.2} MiB · histórico estimado: {:.2} MiB",self.state.images.resident_bytes() as f64/1048576.,self.history.estimated_bytes() as f64/1048576.));
            ui.label(format!("Nós: {nodes} · tarefas pendentes: {}",self.runtime.as_ref().map_or(0,|r|r.pending_tasks())));
            ui.label(format!("Acertos vivos: {} · áreas ativas: {} · prontas: {} · em espera: {}", retained[0], retained[1], retained[2], retained[3]));
            if oxy_core::metrics::ENABLED {
                ui.separator();
                ui.small("Instrumentação ativa: totais desde a última atualização deste painel. Tempos CPU, sem GPU/VSync.");
                ui.label(format!("Passos: {} · movimento: {:.3} ms · áreas: {:.3} ms · ações: {:.3} ms", counters.steps, counters.movement_ns as f64/1e6, counters.areas_ns as f64/1e6, counters.tasks_ns as f64/1e6));
                ui.label(format!("Consultas: {} · visitas de hierarquia: {} · matrizes: {} · reutilizadas: {}", counters.entity_queries,counters.hierarchy_visits,counters.matrices,counters.matrix_hits));
                ui.label(format!("Índices construídos: {} · candidatos: {} · sobreposições: {} · ações: {}",counters.index_builds,counters.candidates,counters.overlaps,counters.actions));
            }
            ui.small("A janela atualiza duas vezes por segundo em repouso. Não inclui medição de RAM total ou VRAM.");
        });
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
