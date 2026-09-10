use glam::{Mat4, Vec3};
use oxy_core::document::{Scene, SceneKind};

/// Projection state independent of the editor. Screen measurements are local to the viewport.
#[derive(Clone, Debug)]
pub struct CameraState {
    pub kind: SceneKind,
    pub target: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    /// Half the visible vertical span in world units.
    pub orthographic_size: f32,
    pub fov: f32,
    game_view: Option<Mat4>,
    pub hidden: Vec<oxy_core::document::Id>,
}

impl CameraState {
    pub fn for_scene(scene: &Scene) -> Self {
        let mut camera = Self {
            kind: scene.kind,
            target: Vec3::new(0., 1., 0.),
            yaw: 0.65,
            pitch: 0.35,
            distance: 12.,
            orthographic_size: 7.5,
            fov: 60.,
            game_view: None,
            hidden: Vec::new(),
        };
        if scene.kind == SceneKind::TwoD {
            camera.target = Vec3::ZERO;
        }
        camera
    }

    pub fn for_game(scene: &Scene) -> Self {
        let mut result = Self::for_scene(scene);
        if let Some((entity, camera)) = scene
            .entities
            .iter()
            .filter_map(|e| e.camera.as_ref().map(|c| (e, c)))
            .find(|(_, c)| c.active)
            && let Ok(world) = scene.world_matrix(&entity.id)
        {
            // Scaling the parent must not distort the optical projection.
            let (_, rotation, position) = world.to_scale_rotation_translation();
            result.game_view = Some(Mat4::from_rotation_translation(rotation, position).inverse());
            result.target = position;
            result.orthographic_size = camera.orthographic_size.max(0.05);
            result.fov = camera.fov.clamp(10., 150.);
        }
        result
    }

    pub fn for_runtime(runtime: &oxy_core::runtime::Runtime) -> Self {
        let mut result = Self::for_game(runtime.scene());
        if let Some(pose) = runtime.game_camera_pose() {
            result.game_view =
                Some(Mat4::from_rotation_translation(pose.rotation, pose.position).inverse());
            result.target = pose.position;
            result.fov = pose.fov;
            result.hidden = pose.hidden;
        }
        result
    }

    pub fn eye(&self) -> Vec3 {
        if let Some(view) = self.game_view {
            return view.inverse().transform_point3(Vec3::ZERO);
        }
        if self.kind == SceneKind::TwoD {
            return self.target + Vec3::Z * 100.;
        }
        self.target
            + Vec3::new(
                self.pitch.cos() * self.yaw.sin(),
                self.pitch.sin(),
                self.pitch.cos() * self.yaw.cos(),
            ) * self.distance
    }

    pub fn view(&self) -> Mat4 {
        self.game_view
            .unwrap_or_else(|| Mat4::look_at_rh(self.eye(), self.target, Vec3::Y))
    }

    pub fn matrix(&self, size: [u32; 2]) -> Mat4 {
        let aspect = size[0].max(1) as f32 / size[1].max(1) as f32;
        let projection = if self.kind == SceneKind::TwoD {
            let half = self.orthographic_size;
            Mat4::orthographic_rh(-half * aspect, half * aspect, -half, half, -1000., 1000.)
        } else {
            Mat4::perspective_rh(self.fov.to_radians(), aspect, 0.02, 2000.)
        };
        projection * self.view()
    }

    pub fn ray(&self, size: [u32; 2], pixel: [f32; 2]) -> (Vec3, Vec3) {
        let inverse = self.matrix(size).inverse();
        let x = pixel[0] / size[0].max(1) as f32 * 2. - 1.;
        let y = 1. - pixel[1] / size[1].max(1) as f32 * 2.;
        let near = inverse.project_point3(Vec3::new(x, y, 0.));
        let far = inverse.project_point3(Vec3::new(x, y, 1.));
        (near, (far - near).normalize_or_zero())
    }

    pub fn world_to_screen(&self, position: Vec3, size: [u32; 2]) -> Option<[f32; 2]> {
        let clip = self.matrix(size) * position.extend(1.);
        if clip.w <= 0. {
            return None;
        }
        let ndc = clip.truncate() / clip.w;
        if !(0.0..=1.0).contains(&ndc.z) {
            return None;
        }
        Some([
            (ndc.x * 0.5 + 0.5) * size[0] as f32,
            (0.5 - ndc.y * 0.5) * size[1] as f32,
        ])
    }

    pub fn navigate_2d(&mut self, delta: [f32; 2], scroll: f32, size: [u32; 2]) {
        self.pan(delta, size);
        self.zoom(scroll);
    }

    pub fn orbit(&mut self, delta: [f32; 2]) {
        self.game_view = None;
        self.yaw -= delta[0] * 0.008;
        self.pitch = (self.pitch + delta[1] * 0.008).clamp(-1.5, 1.5);
    }

    pub fn pan(&mut self, delta: [f32; 2], size: [u32; 2]) {
        self.game_view = None;
        let half = if self.kind == SceneKind::TwoD {
            self.orthographic_size
        } else {
            self.distance * (self.fov.to_radians() * 0.5).tan()
        };
        let scale = 2. * half / size[1].max(1) as f32;
        let inverse = self.view().inverse();
        self.target += inverse.transform_vector3(Vec3::X) * -delta[0] * scale
            + inverse.transform_vector3(Vec3::Y) * delta[1] * scale;
    }

    pub fn zoom(&mut self, scroll: f32) {
        self.game_view = None;
        let factor = (-scroll * 0.002).exp();
        self.distance = (self.distance * factor).clamp(0.1, 1000.);
        self.orthographic_size = (self.orthographic_size * factor).clamp(0.05, 1000.);
    }

    pub fn focus(&mut self, target: Vec3) {
        self.game_view = None;
        self.target = target;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resized_projection_and_ray_stay_aligned() {
        let scene = Scene::new("Teste", SceneKind::ThreeD);
        let camera = CameraState::for_scene(&scene);
        for size in [[640, 480], [1920, 1080], [300, 900]] {
            let point = Vec3::new(0.3, 0.8, -0.4);
            let screen = camera.world_to_screen(point, size).unwrap();
            let (origin, direction) = camera.ray(size, screen);
            assert!((point - origin).cross(direction).length() < 0.003);
        }
    }
    #[test]
    fn orthographic_scale_is_uniform() {
        let scene = Scene::new("Teste", SceneKind::TwoD);
        let camera = CameraState::for_scene(&scene);
        let size = [1200, 400];
        let a = camera.world_to_screen(Vec3::ZERO, size).unwrap();
        let b = camera.world_to_screen(Vec3::X, size).unwrap();
        let c = camera.world_to_screen(Vec3::Y, size).unwrap();
        assert!(((b[0] - a[0]) - (a[1] - c[1])).abs() < 0.001);
    }
}
