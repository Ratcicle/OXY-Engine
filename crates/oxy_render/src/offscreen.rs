//! Rendering without a window: a GPU device of its own and pixel readback, for scene
//! screenshots requested by tools (the MCP server, the editor's agent channel) and for GPU
//! tests. Captures can carry the editor's collider view and look through a free camera.
use super::*;
use oxy_core::{character::GameCameraPose, collision::Aabb, runtime::Runtime};
use std::{
    future::Future,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
    time::Duration,
};

/// Runs a future to completion on this thread; wgpu's setup calls are the only users.
pub fn block_on<T>(future: impl Future<Output = T>) -> T {
    struct ThreadWake(std::thread::Thread);
    impl Wake for ThreadWake {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => return value,
            Poll::Pending => std::thread::park_timeout(Duration::from_secs(1)),
        }
    }
}

/// A render state backed by the default GPU adapter, with no window or surface.
pub fn headless_state() -> Result<RenderState, String> {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
        .map_err(|e| format!("Nenhuma placa de vídeo disponível para renderizar: {e}"))?;
    let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
        .map_err(|e| format!("Não foi possível abrir a placa de vídeo: {e}"))?;
    let egui_renderer =
        egui_wgpu::Renderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, Default::default());
    Ok(RenderState {
        adapter,
        available_adapters: Vec::new(),
        device,
        queue,
        target_format: wgpu::TextureFormat::Rgba8Unorm,
        renderer: Arc::new(egui::epaint::mutex::RwLock::new(egui_renderer)),
    })
}

/// The collider view drawn over a capture, as the editor paints it over the viewport: solid
/// colliders green, sensor areas yellow.
pub struct ColliderOverlay<'a> {
    /// Objects drawn as selected, with their name.
    pub selected: &'a [Id],
    pub show_disabled: bool,
    /// The running game's physics; without it, the scene's own colliders.
    pub runtime_world: Option<&'a oxy_core::physics3d::PhysicsWorld>,
    /// Pixels per egui point, which sets the stroke widths (1 off the editor).
    pub pixels_per_point: f32,
}

/// What `capture` draws the collider view with: an egui context and painter of its own, so
/// the editor's fonts and textures are never touched.
pub(crate) struct CapturePainter {
    context: egui::Context,
    painter: egui_wgpu::Renderer,
}

impl Renderer {
    /// RGBA pixels of the last frame drawn by `render`, row by row, and its size.
    pub fn read_pixels(&self, rs: &RenderState) -> Result<(Vec<u8>, [u32; 2]), String> {
        let [width, height] = self.target.size;
        let stride = (width * 4).div_ceil(256) * 256;
        let buffer = rs.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("OXY readback"),
            size: u64::from(stride * height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = rs.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.target._color,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(stride),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        rs.queue.submit([encoder.finish()]);
        let (sender, receiver) = std::sync::mpsc::channel();
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = sender.send(result);
            });
        rs.device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(Duration::from_secs(20)),
            })
            .map_err(|e| format!("A placa de vídeo não terminou a imagem: {e}"))?;
        receiver
            .recv_timeout(Duration::from_secs(2))
            .map_err(|_| "A leitura da imagem não terminou a tempo".to_owned())?
            .map_err(|e| format!("Não foi possível ler a imagem: {e}"))?;
        let mapped = buffer.slice(..).get_mapped_range();
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for row in mapped.chunks(stride as usize) {
            pixels.extend_from_slice(&row[..(width * 4) as usize]);
        }
        drop(mapped);
        buffer.unmap();
        Ok((pixels, [width, height]))
    }

    /// Size of the last frame drawn by `render`.
    pub fn target_size(&self) -> [u32; 2] {
        self.target.size
    }

    /// Renders `scene` into a target of its own, so the one the editor shows stays as it is,
    /// draws the collider view on top when given and reads the pixels back.
    #[allow(clippy::too_many_arguments)]
    pub fn capture(
        &mut self,
        rs: &RenderState,
        project: &Project,
        scene: &Scene,
        root: &Path,
        camera: &CameraState,
        size: [u32; 2],
        selected: Option<Id>,
        colliders: Option<ColliderOverlay>,
    ) -> Result<(Vec<u8>, [u32; 2]), String> {
        let mut target = make_target(rs, [size[0].max(1), size[1].max(1)], None);
        std::mem::swap(&mut self.target, &mut target);
        let debug = self.debug_colliders;
        self.render(
            rs,
            project,
            scene,
            root,
            camera,
            size,
            selected,
            colliders.is_some(),
        );
        if let Some(overlay) = &colliders {
            self.draw_overlay(rs, scene, camera, overlay);
        }
        let pixels = self.read_pixels(rs);
        self.debug_colliders = debug;
        std::mem::swap(&mut self.target, &mut target);
        rs.renderer.write().free_texture(&target.id);
        pixels
    }

    /// Paints the collider view over the target with the editor's own drawing code.
    fn draw_overlay(
        &mut self,
        rs: &RenderState,
        scene: &Scene,
        camera: &CameraState,
        overlay: &ColliderOverlay,
    ) {
        let size = self.target.size;
        let CapturePainter {
            context,
            mut painter,
        } = self
            .capture_painter
            .take()
            .unwrap_or_else(|| CapturePainter {
                context: egui::Context::default(),
                painter: egui_wgpu::Renderer::new(
                    &rs.device,
                    wgpu::TextureFormat::Rgba8Unorm,
                    egui_wgpu::RendererOptions {
                        dithering: false,
                        ..Default::default()
                    },
                ),
            });
        let pixels_per_point = if overlay.pixels_per_point.is_finite() {
            overlay.pixels_per_point.clamp(0.25, 8.)
        } else {
            1.
        };
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(size[0] as f32, size[1] as f32) / pixels_per_point,
            )),
            ..Default::default()
        };
        input
            .viewports
            .entry(egui::ViewportId::ROOT)
            .or_default()
            .native_pixels_per_point = Some(pixels_per_point);
        let output = context.run(input, |ctx| {
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(ctx, |ui| {
                    let rect = ui.max_rect();
                    self.draw_runtime_colliders(
                        ui,
                        scene,
                        camera,
                        rect,
                        overlay.selected,
                        overlay.show_disabled,
                        overlay.runtime_world,
                    );
                });
        });
        let primitives = context.tessellate(output.shapes, output.pixels_per_point);
        for (id, delta) in &output.textures_delta.set {
            painter.update_texture(&rs.device, &rs.queue, *id, delta);
        }
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: size,
            pixels_per_point: output.pixels_per_point,
        };
        let mut encoder = rs
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("OXY capture overlay encoder"),
            });
        let mut commands =
            painter.update_buffers(&rs.device, &rs.queue, &mut encoder, &primitives, &screen);
        {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("OXY capture overlay"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &self.target.view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                })
                .forget_lifetime();
            painter.render(&mut pass, &primitives, &screen);
        }
        commands.push(encoder.finish());
        rs.queue.submit(commands);
        for id in &output.textures_delta.free {
            painter.free_texture(id);
        }
        self.capture_painter = Some(CapturePainter { context, painter });
    }
}

/// PNG bytes of RGBA pixels.
pub fn encode_png(pixels: Vec<u8>, size: [u32; 2]) -> Result<Vec<u8>, String> {
    let image = image::RgbaImage::from_raw(size[0], size[1], pixels)
        .ok_or("Tamanho de imagem inconsistente")?;
    let mut bytes = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut bytes, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(bytes.into_inner())
}

/// How a capture of a running game looks at it.
#[derive(Clone, Debug, PartialEq)]
pub enum ShotView {
    /// The game camera, as the player shows it.
    Game,
    /// A perspective camera at `eye` looking at `target`, `fov` degrees high (3D scenes).
    Free { eye: Vec3, target: Vec3, fov: f32 },
    /// Orthographic, straight down on the bounds, -Z up in the image and +X to the right.
    Top(Aabb),
    /// Orthographic and level, across the bounds: along -Z (+X to the right), or along -X
    /// (-Z to the right) when they are deeper than wide, so their long side spans the image.
    Side(Aabb),
}

impl ShotView {
    /// The camera for an image of `size` pixels of the running scene. In 2D scenes top and
    /// side both show the scene plane, framed.
    pub fn camera(&self, runtime: &Runtime, size: [u32; 2]) -> Result<CameraState, String> {
        let scene = runtime.scene();
        let (pose, framed) = match self {
            Self::Game => return Ok(CameraState::for_runtime(runtime)),
            Self::Free { eye, target, fov } => {
                if scene.kind == SceneKind::TwoD {
                    return Err("Cenas 2D não têm vista livre; use view=top ou side.".into());
                }
                if !eye.is_finite() || !target.is_finite() || !fov.is_finite() {
                    return Err("eye, target e fov precisam ser números finitos".into());
                }
                if eye.distance(*target) < 1e-3 {
                    return Err("eye e target precisam ser pontos diferentes".into());
                }
                let pose = GameCameraPose {
                    position: *eye,
                    rotation: look_rotation(*target - *eye),
                    fov: fov.clamp(10., 150.),
                    hidden: Vec::new(),
                };
                (pose, None)
            }
            Self::Top(bounds) | Self::Side(bounds) => {
                let center = (bounds.min + bounds.max) * 0.5;
                let half = (bounds.max - bounds.min) * 0.5;
                // Forward, then the half extents across and up the image, and in depth.
                let (forward, across, up, depth) = match self {
                    _ if scene.kind == SceneKind::TwoD => (Vec3::NEG_Z, half.x, half.y, half.z),
                    Self::Top(_) => (Vec3::NEG_Y, half.x, half.z, half.y),
                    _ if half.z > half.x => (Vec3::NEG_X, half.z, half.y, half.x),
                    _ => (Vec3::NEG_Z, half.x, half.y, half.z),
                };
                let pose = GameCameraPose {
                    position: center - forward * (depth + 1.),
                    rotation: look_rotation(forward),
                    fov: 60.,
                    hidden: Vec::new(),
                };
                (pose, Some((across, up)))
            }
        };
        let mut camera = CameraState::from_pose(scene, &pose);
        if let Some((across, up)) = framed {
            let aspect = size[0].max(1) as f32 / size[1].max(1) as f32;
            // The kind only picks the projection here; the scene keeps its own pipeline.
            camera.kind = SceneKind::TwoD;
            camera.orthographic_size = up.max(across / aspect) * 1.08 + 0.25;
        }
        camera.worlds = runtime.presentation_worlds();
        Ok(camera)
    }
}

/// Rotation of a camera looking along `forward`, level with the horizon; straight up or down,
/// -Z is up in the image.
fn look_rotation(forward: Vec3) -> glam::Quat {
    let forward = forward.normalize();
    let up = if forward.y.abs() > 0.999 {
        Vec3::NEG_Z
    } else {
        Vec3::Y
    };
    let (_, rotation, _) = Mat4::look_at_rh(Vec3::ZERO, forward, up)
        .inverse()
        .to_scale_rotation_translation();
    rotation
}

/// World bounds of what `ids` and their children show in `scene`; with no IDs, of every
/// object with geometry, hidden ones included. Groups, cameras, interface elements and
/// objects squashed to nothing add nothing.
pub fn framed_bounds(scene: &Scene, ids: &[Id]) -> Result<Aabb, String> {
    let view = oxy_core::scene_view::SceneView::new(scene);
    let mut targets = Vec::new();
    if ids.is_empty() {
        targets.extend(scene.entities.iter().map(|e| e.id.clone()));
    }
    for id in ids {
        if view.entity(id).is_none() {
            return Err(format!("Objeto {id} não existe na cena {}", scene.name));
        }
        targets.extend(view.descendants(id));
    }
    targets.retain(|id| {
        view.entity(id).is_some_and(|e| {
            e.has_geometry()
                && e.camera.is_none()
                && e.ui.is_none()
                && view
                    .world_matrix(id)
                    .is_ok_and(|m| m.is_finite() && m.determinant().abs() >= 1e-8)
        })
    });
    if targets.is_empty() {
        return Err("Nada com forma para enquadrar".into());
    }
    oxy_core::spatial::visual_bounds(scene, &targets)
}

/// `runtime` at its current time seen through `view`, as PNG, with the collider view on top
/// when `colliders` is set, and the camera used. The game interface (text, buttons) is not
/// drawn: egui draws it.
pub fn view_png(
    rs: &RenderState,
    renderer: &mut Renderer,
    runtime: &mut Runtime,
    root: &Path,
    size: [u32; 2],
    view: &ShotView,
    colliders: bool,
) -> Result<(Vec<u8>, CameraState), String> {
    if size.iter().any(|v| !(16..=4096).contains(v)) {
        return Err("A imagem deve ter entre 16 e 4096 pixels de lado".into());
    }
    runtime.set_viewport_aspect(size[0] as f32 / size[1] as f32);
    let camera = view.camera(runtime, size)?;
    renderer.show_grid = false;
    let overlay = colliders.then(|| ColliderOverlay {
        selected: &[],
        show_disabled: false,
        runtime_world: runtime.physics_world(),
        pixels_per_point: 1.,
    });
    let (pixels, size) = renderer.capture(
        rs,
        runtime.project(),
        runtime.scene(),
        root,
        &camera,
        size,
        None,
        overlay,
    )?;
    Ok((encode_png(pixels, size)?, camera))
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxy_core::document::{Entity, Primitive};

    fn level(kind: SceneKind) -> Project {
        let mut project = Project::new("Enquadrar");
        project.scenes[0].kind = kind;
        let mut floor = Entity::new("Chão", Some(Primitive::Cube));
        floor.dimensions = [4., 1., 40.];
        floor.transform.position = [10., 0., -20.];
        let mut group = Entity::new("Grupo", None);
        group.transform.position = [0., 5., 0.];
        let mut child = Entity::new("Caixa", Some(Primitive::Cube));
        child.parent = Some(group.id.clone());
        let mut flat = Entity::new("Achatado", Some(Primitive::Cube));
        flat.transform.scale = [0., 1., 1.];
        project.scenes[0]
            .entities
            .extend([floor, group, child, flat]);
        project
    }

    #[test]
    fn framing_follows_children_and_skips_what_has_no_shape() {
        let project = level(SceneKind::ThreeD);
        let scene = &project.scenes[0];
        let all = framed_bounds(scene, &[]).unwrap();
        assert!(
            all.min.abs_diff_eq(Vec3::new(-0.5, -0.5, -40.), 1e-4),
            "{all:?}"
        );
        assert!(
            all.max.abs_diff_eq(Vec3::new(12., 5.5, 0.5), 1e-4),
            "{all:?}"
        );
        let group = scene.entities[1].id.clone();
        let only = framed_bounds(scene, &[group]).unwrap();
        assert!(only.min.abs_diff_eq(Vec3::new(-0.5, 4.5, -0.5), 1e-4));
        let flat = scene.entities[3].id.clone();
        assert!(framed_bounds(scene, &[flat]).is_err());
        let error = framed_bounds(scene, &["fantasma".into()]).unwrap_err();
        assert!(error.contains("fantasma"), "{error}");
    }

    #[test]
    fn free_top_and_side_cameras_frame_what_they_are_given() {
        let mut project = level(SceneKind::ThreeD);
        // The game refuses objects squashed to nothing.
        project.scenes[0].entities.pop();
        let runtime = Runtime::new(&project, &project.start_scene).unwrap();
        let size = [800, 400];
        let free = ShotView::Free {
            eye: Vec3::new(0., 10., 10.),
            target: Vec3::new(10., 0., -20.),
            fov: 50.,
        }
        .camera(&runtime, size)
        .unwrap();
        let center = free
            .world_to_screen(Vec3::new(10., 0., -20.), size)
            .unwrap();
        assert!((center[0] - 400.).abs() < 0.5 && (center[1] - 200.).abs() < 0.5);
        assert!(free.eye().abs_diff_eq(Vec3::new(0., 10., 10.), 1e-4));
        assert_eq!(free.fov, 50.);
        assert_eq!(free.kind, SceneKind::ThreeD, "perspective");
        // Straight down still has a usable orientation.
        let down = ShotView::Free {
            eye: Vec3::new(0., 10., 0.),
            target: Vec3::ZERO,
            fov: 60.,
        }
        .camera(&runtime, size)
        .unwrap();
        assert!(down.forward().abs_diff_eq(Vec3::NEG_Y, 1e-4));
        for view in [
            ShotView::Free {
                eye: Vec3::ONE,
                target: Vec3::ONE,
                fov: 60.,
            },
            ShotView::Free {
                eye: Vec3::new(f32::NAN, 0., 0.),
                target: Vec3::ONE,
                fov: 60.,
            },
        ] {
            assert!(view.camera(&runtime, size).is_err());
        }

        let bounds = framed_bounds(runtime.scene(), &[]).unwrap();
        let inside = |camera: &CameraState| {
            let corners = crate::collider_debug::corners(bounds, false);
            corners.iter().all(|corner| {
                camera
                    .world_to_screen(*corner, size)
                    .is_some_and(|[x, y]| (0.0..=800.).contains(&x) && (0.0..=400.).contains(&y))
            })
        };
        let top = ShotView::Top(bounds).camera(&runtime, size).unwrap();
        assert!(top.forward().abs_diff_eq(Vec3::NEG_Y, 1e-4));
        assert_eq!(top.kind, SceneKind::TwoD, "orthographic");
        assert!(inside(&top));
        // -Z is up in the image.
        let far = top.world_to_screen(Vec3::new(10., 0., -40.), size).unwrap();
        let near = top.world_to_screen(Vec3::new(10., 0., 0.), size).unwrap();
        assert!(far[1] < near[1]);
        // The level is deeper than wide: the side view looks along -X.
        let side = ShotView::Side(bounds).camera(&runtime, size).unwrap();
        assert!(side.forward().abs_diff_eq(Vec3::NEG_X, 1e-4));
        assert!(inside(&side));
        let wide = Aabb {
            min: Vec3::new(-20., 0., -1.),
            max: Vec3::new(20., 2., 1.),
        };
        let side = ShotView::Side(wide).camera(&runtime, size).unwrap();
        assert!(side.forward().abs_diff_eq(Vec3::NEG_Z, 1e-4));
        assert!(
            (side.orthographic_size - (20. / 2. * 1.08 + 0.25)).abs() < 1e-4,
            "the width sets the scale: {}",
            side.orthographic_size
        );

        let mut flat = Project::new("Plano");
        flat.scenes[0].kind = SceneKind::TwoD;
        let runtime = Runtime::new(&flat, &flat.start_scene).unwrap();
        let top = ShotView::Top(bounds).camera(&runtime, size).unwrap();
        assert!(top.forward().abs_diff_eq(Vec3::NEG_Z, 1e-4), "the 2D plane");
        assert!(
            ShotView::Free {
                eye: Vec3::Z,
                target: Vec3::ZERO,
                fov: 60.,
            }
            .camera(&runtime, size)
            .unwrap_err()
            .contains("2D")
        );
    }
}
