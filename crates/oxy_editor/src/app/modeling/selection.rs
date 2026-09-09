//! A press owns one immutable selection contract. Pointer movement never edits the document.
use super::*;
use oxy_core::scene_view::SceneView;
use oxy_render::mesh_selection::{OccluderKey, Occlusion, ProjectedMesh, occluder_keys};

pub(super) const BOX_THRESHOLD: f32 = 4.;
pub(super) struct BoxGesture {
    pub start: Pos2,
    pub initial: Components,
    pub additive: bool,
    pub through: bool,
    pub dragging: bool,
    pub hit: Option<u32>,
}
impl BoxGesture {
    pub fn new(start: Pos2, initial: Components, ctrl: bool, hit: Option<u32>) -> Self {
        let through = ctrl || initial.through;
        Self {
            start,
            initial,
            additive: ctrl,
            through,
            dragging: false,
            hit,
        }
    }
    pub fn update(
        &mut self,
        end: Pos2,
        candidates: impl IntoIterator<Item = u32>,
        selection: &mut Components,
    ) {
        self.dragging |= self.start.distance(end) >= BOX_THRESHOLD;
        if !self.dragging {
            return;
        }
        let mut ids = if self.additive {
            self.initial.ids.clone()
        } else {
            Vec::new()
        };
        let mut seen: std::collections::HashSet<_> = ids.iter().copied().collect();
        ids.extend(candidates.into_iter().filter(|id| seen.insert(*id)));
        selection.ids = ids;
    }
    pub fn finish(self, selection: &mut Components) {
        if !self.dragging {
            selection.click(self.hit, self.additive);
        }
    }
}
#[derive(Clone, PartialEq)]
struct Key {
    entity: Id,
    revision: u64,
    kind: SceneKind,
    matrix: [f32; 16],
    rect: Rect,
    scale: f32,
}
#[derive(Clone, Copy, Default)]
pub(crate) struct Stats {
    pub projection_builds: u64,
    pub projection_hits: u64,
    pub occlusion_builds: u64,
    pub occlusion_hits: u64,
    pub hover_queries: u64,
    pub rectangle_queries: u64,
    pub through_queries: u64,
}
#[derive(Default)]
pub(super) struct ProjectionCache {
    key: Option<Key>,
    projected: Option<ProjectedMesh>,
    scene_keys: Vec<OccluderKey>,
    occlusion: Option<Occlusion>,
    hover: Option<Hover>,
    pub stats: Stats,
}
#[derive(Clone, Copy)]
struct Hover {
    position: Pos2,
    mode: Mode,
    through: bool,
    hit: Option<u32>,
    edge: Option<u32>,
}
impl ProjectionCache {
    pub fn clear(&mut self) {
        self.key = None;
        self.projected = None;
        self.scene_keys.clear();
        self.occlusion = None;
        self.hover = None;
    }
    #[allow(clippy::too_many_arguments)]
    pub fn prepare(
        &mut self,
        entity: &str,
        mesh: &EditableMesh,
        world: Mat4,
        camera: &CameraState,
        rect: Rect,
        scale: f32,
        scene: &Scene,
        view: &SceneView<'_>,
    ) {
        let physical = [
            (rect.width() * scale).max(1.) as u32,
            (rect.height() * scale).max(1.) as u32,
        ];
        let matrix = camera.matrix(physical) * world;
        let key = Key {
            entity: entity.to_owned(),
            revision: mesh.revision(),
            kind: scene.kind,
            matrix: matrix.to_cols_array(),
            rect,
            scale,
        };
        if self.key.as_ref() != Some(&key) {
            self.projected = Some(ProjectedMesh::new(mesh, matrix, rect));
            self.key = Some(key);
            self.occlusion = None;
            self.hover = None;
            self.stats.projection_builds += 1;
        } else {
            self.stats.projection_hits += 1;
        }
        let keys = occluder_keys(scene, view);
        if self.scene_keys != keys {
            self.scene_keys = keys;
            self.occlusion = None;
            self.hover = None;
        }
    }
    pub fn projected(&self) -> &ProjectedMesh {
        self.projected
            .as_ref()
            .expect("Prepared component projection")
    }
    pub fn cached_hover(
        &self,
        p: Pos2,
        mode: Mode,
        through: bool,
    ) -> Option<(Option<u32>, Option<u32>)> {
        self.hover
            .filter(|h| h.position == p && h.mode == mode && h.through == through)
            .map(|h| (h.hit, h.edge))
    }
    pub fn remember_hover(
        &mut self,
        p: Pos2,
        mode: Mode,
        through: bool,
        hit: Option<u32>,
        edge: Option<u32>,
    ) {
        self.hover = Some(Hover {
            position: p,
            mode,
            through,
            hit,
            edge,
        });
        self.stats.hover_queries += 1;
    }
    #[allow(clippy::too_many_arguments)]
    pub fn select(
        &mut self,
        mode: Mode,
        rect: Rect,
        through: bool,
        entity: &str,
        scene: &Scene,
        view: &SceneView<'_>,
        camera: &CameraState,
        viewport: Rect,
        scale: f32,
    ) -> Vec<u32> {
        self.stats.rectangle_queries += 1;
        if through {
            self.stats.through_queries += 1;
            return self.projected().select(mode, rect, None);
        }
        if self.occlusion.is_none() {
            self.occlusion = Some(Occlusion::with_target(
                scene,
                view,
                camera,
                viewport,
                scale,
                (entity, self.projected()),
            ));
            self.stats.occlusion_builds += 1;
        } else {
            self.stats.occlusion_hits += 1;
        }
        self.projected()
            .select(mode, rect, Some((self.occlusion.as_ref().unwrap(), entity)))
    }
    pub fn bytes(&self) -> usize {
        self.projected
            .as_ref()
            .map_or(0, ProjectedMesh::estimated_bytes)
            + self
                .occlusion
                .as_ref()
                .map_or(0, Occlusion::estimated_bytes)
    }
}
impl Editor {
    pub(in crate::app) fn cancel_box_selection(&mut self) -> bool {
        let Some(gesture) = self.modeling.selection_gesture.take() else {
            return false;
        };
        self.modeling.selection = gesture.initial;
        true
    }
    pub(crate) fn component_selection_stats(&self) -> (Stats, usize) {
        (
            self.modeling.projection_cache.stats,
            self.modeling.projection_cache.bytes(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn selection() -> Components {
        Components {
            mode: Mode::Face,
            ids: vec![9],
            through: false,
        }
    }
    #[test]
    fn ctrl_box_is_union_of_initial_and_current_not_accumulated_or_toggled() {
        let mut selected = selection();
        let mut gesture = BoxGesture::new(Pos2::ZERO, selected.clone(), true, Some(1));
        gesture.update(Pos2::new(20., 20.), [1, 9, 2, 1], &mut selected);
        assert_eq!(selected.ids, vec![9, 1, 2]);
        gesture.update(Pos2::new(7., 7.), [2], &mut selected);
        assert_eq!(selected.ids, vec![9, 2]);
        assert!(gesture.through);
        assert!(!selected.through);
        gesture.finish(&mut selected);
        assert_eq!(selected.ids, vec![9, 2]);
    }
    fn cancellation_after_drag(ctrl: bool) {
        let mut selected = selection();
        let mut gesture = BoxGesture::new(Pos2::ZERO, selected.clone(), ctrl, Some(1));
        gesture.update(Pos2::new(10., 10.), [3, 4], &mut selected);
        selected = gesture.initial;
        assert_eq!(selected, selection());
    }
    #[test]
    fn escape_and_focus_loss_restore_the_press_selection() {
        cancellation_after_drag(true);
        cancellation_after_drag(false);
    }
    #[test]
    fn ctrl_click_toggles_one_and_modifier_release_does_not_change_press_contract() {
        let mut selected = selection();
        BoxGesture::new(Pos2::ZERO, selected.clone(), true, Some(9)).finish(&mut selected);
        assert!(selected.ids.is_empty());
        let gesture = BoxGesture::new(Pos2::ZERO, selected.clone(), true, Some(4));
        assert!(gesture.through);
        gesture.finish(&mut selected);
        assert_eq!(selected.ids, vec![4]);
        assert!(!selected.through);
    }
    #[test]
    fn normal_drag_replaces_and_threshold_uses_logical_points() {
        let mut selected = selection();
        let mut gesture = BoxGesture::new(Pos2::ZERO, selected.clone(), false, Some(1));
        gesture.update(Pos2::new(2., 1.), [3], &mut selected);
        assert_eq!(selected.ids, vec![9]);
        gesture.update(Pos2::new(4., 0.), [3], &mut selected);
        assert_eq!(selected.ids, vec![3]);
        assert!(!gesture.through);
    }
    #[test]
    fn projection_cache_invalidation_and_through_selection_skip_occlusion() {
        let mut scene = Scene::new("Teste", SceneKind::ThreeD);
        let mesh = primitives::generate(Primitive::Cube, 8, Default::default()).unwrap();
        let mut entity = Entity::new("Peça", None);
        entity.mesh = Some(mesh.clone());
        let id = entity.id.clone();
        scene.entities.push(entity);
        let mut camera = CameraState::for_scene(&scene);
        camera.target = Vec3::ZERO;
        camera.distance = 3.;
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(600., 400.));
        let mut cache = ProjectionCache::default();
        for _ in 0..2 {
            cache.prepare(
                &id,
                &mesh,
                Mat4::IDENTITY,
                &camera,
                rect,
                1.,
                &scene,
                &SceneView::new(&scene),
            );
        }
        assert_eq!(cache.stats.projection_builds, 1);
        assert_eq!(cache.stats.projection_hits, 1);
        let through = cache.select(
            Mode::Face,
            rect,
            true,
            &id,
            &scene,
            &SceneView::new(&scene),
            &camera,
            rect,
            1.,
        );
        assert_eq!(through.len(), 6);
        assert_eq!(cache.stats.occlusion_builds, 0);
        let visible = cache.select(
            Mode::Face,
            rect,
            false,
            &id,
            &scene,
            &SceneView::new(&scene),
            &camera,
            rect,
            1.,
        );
        assert!(visible.len() < through.len());
        assert_eq!(cache.stats.occlusion_builds, 1);
        cache.select(
            Mode::Face,
            rect,
            false,
            &id,
            &scene,
            &SceneView::new(&scene),
            &camera,
            rect,
            1.,
        );
        assert_eq!(cache.stats.occlusion_builds, 1);
        assert_eq!(cache.stats.occlusion_hits, 1);
        camera.orbit([10., 0.]);
        cache.prepare(
            &id,
            &mesh,
            Mat4::IDENTITY,
            &camera,
            rect,
            1.,
            &scene,
            &SceneView::new(&scene),
        );
        assert_eq!(cache.stats.projection_builds, 2);
        cache.prepare(
            &id,
            &mesh,
            Mat4::from_translation(Vec3::X),
            &camera,
            rect,
            1.,
            &scene,
            &SceneView::new(&scene),
        );
        assert_eq!(cache.stats.projection_builds, 3);
        cache.prepare(
            &id,
            &mesh,
            Mat4::IDENTITY,
            &camera,
            rect,
            1.6,
            &scene,
            &SceneView::new(&scene),
        );
        assert_eq!(cache.stats.projection_builds, 4);
        let small = rect.shrink(40.);
        cache.prepare(
            &id,
            &mesh,
            Mat4::IDENTITY,
            &camera,
            small,
            1.6,
            &scene,
            &SceneView::new(&scene),
        );
        assert_eq!(cache.stats.projection_builds, 5);
        let replaced = mesh
            .with_shading(oxy_core::geometry::Shading::Smooth)
            .unwrap();
        cache.prepare(
            &id,
            &replaced,
            Mat4::IDENTITY,
            &camera,
            small,
            1.6,
            &scene,
            &SceneView::new(&scene),
        );
        assert_eq!(cache.stats.projection_builds, 6);
        assert!(cache.bytes() > 0);
        cache.clear();
        assert_eq!(cache.bytes(), 0);
    }
}
