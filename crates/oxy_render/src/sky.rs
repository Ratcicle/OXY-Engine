//! Céu da OXY, the engine's sky: a panorama drawn behind a scene by the direction the camera
//! looks along, one full-screen triangle over one texture. Orthographic views look along a
//! single direction, so there the sky is its horizon color: captures from above or from the
//! side of a 3D scene (2D scenes take a color, not the sky).
use super::*;
use glam::Vec4;

/// The day sky of Kenney's Skyboxes (CC0, licenses/kenney-skyboxes-cc0.txt), 2048×1024 and
/// equirectangular: the horizon across the middle row.
const PANORAMA: &[u8] = include_bytes!("../assets/ceu-oxy.png");

/// The sky's color at the horizon, the middle row of the panorama averaged: what orthographic
/// views of a scene with the sky show.
pub const HORIZON: [f32; 4] = [0.6808, 0.766, 0.9488, 1.];

/// The matrix taking a point of the far plane in normalized device coordinates to the direction
/// `camera` looks along through it: the inverse of the projection times the view's rotation.
/// None for an orthographic camera.
pub fn directions(camera: &CameraState, size: [u32; 2]) -> Option<Mat4> {
    if camera.kind != SceneKind::ThreeD {
        return None;
    }
    let mut rotation = camera.view();
    rotation.w_axis = Vec4::W;
    let matrix = (camera.projection(size) * rotation).inverse();
    matrix.is_finite().then_some(matrix)
}

pub(crate) struct SkyPass {
    pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    uniform_group: wgpu::BindGroup,
    _texture: wgpu::Texture,
    texture_group: wgpu::BindGroup,
    last: Option<[f32; 16]>,
}

impl SkyPass {
    pub(crate) fn new(
        rs: &RenderState,
        texture_layout: &wgpu::BindGroupLayout,
    ) -> Result<Self, String> {
        let device = &rs.device;
        let image = image::load_from_memory(PANORAMA)
            .map_err(|e| format!("Não foi possível abrir o Céu da OXY: {e}"))?
            .to_rgba8();
        let (width, height) = image.dimensions();
        let limit = device.limits().max_texture_dimension_2d;
        if width > limit || height > limit {
            return Err(format!(
                "O Céu da OXY excede o limite de {limit} pixels da placa de vídeo"
            ));
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("OXY sky"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        write_texture_pixels(rs, &texture, width, height, image.as_raw());
        let view = texture.create_view(&Default::default());
        // Around the horizon the panorama wraps; up and down it ends at the poles.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("OXY sky sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let texture_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("OXY sky texture"),
            layout: texture_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        let uniform_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("OXY sky directions layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("OXY sky directions"),
            size: std::mem::size_of::<[f32; 16]>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let uniform_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("OXY sky directions group"),
            layout: &uniform_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("OXY sky layout"),
            bind_group_layouts: &[&uniform_layout, texture_layout],
            push_constant_ranges: &[],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("OXY sky shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("sky.wgsl").into()),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("OXY sky pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_sky"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            // Behind everything: drawn first, it neither tests nor writes depth.
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Always,
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_sky"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        });
        Ok(Self {
            pipeline,
            uniform,
            uniform_group,
            _texture: texture,
            texture_group,
            last: None,
        })
    }

    /// Points the sky along `directions` ([`directions`]); true when that wrote the uniform.
    pub(crate) fn aim(&mut self, rs: &RenderState, directions: Mat4) -> bool {
        let matrix = directions.to_cols_array();
        if self.last == Some(matrix) {
            return false;
        }
        rs.queue
            .write_buffer(&self.uniform, 0, bytemuck::cast_slice(&matrix));
        self.last = Some(matrix);
        true
    }

    pub(crate) fn draw(&self, pass: &mut wgpu::RenderPass) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.uniform_group, &[]);
        pass.set_bind_group(1, &self.texture_group, &[]);
        pass.draw(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_panorama_is_embedded_and_its_horizon_is_the_one_documented() {
        let image = image::load_from_memory(PANORAMA).unwrap().to_rgba8();
        assert_eq!(image.dimensions(), (2048, 1024));
        let row = image.height() / 2;
        let mut sum = [0u64; 3];
        for x in 0..image.width() {
            let pixel = image.get_pixel(x, row).0;
            for (total, channel) in sum.iter_mut().zip(pixel) {
                *total += u64::from(channel);
            }
        }
        let average = sum.map(|total| total as f32 / image.width() as f32 / 255.);
        for (channel, horizon) in average.iter().zip(HORIZON) {
            assert!((channel - horizon).abs() < 0.5 / 255., "{average:?}");
        }
    }

    #[test]
    fn directions_follow_the_view_and_only_perspective_has_them() {
        let scene = Scene::new("Céu", SceneKind::ThreeD);
        let mut camera = CameraState::for_scene(&scene);
        camera.target = Vec3::new(40., -3., 12.);
        let size = [800, 400];
        let directions = directions(&camera, size).unwrap();
        let along = |x: f32, y: f32| {
            let far = directions * Vec4::new(x, y, 1., 1.);
            (far.truncate() / far.w).normalize()
        };
        // The middle of the screen looks forward, wherever the camera is; a point of the
        // screen looks through what the camera shows there.
        assert!(along(0., 0.).abs_diff_eq(camera.forward(), 1e-5));
        let half = (camera.fov.to_radians() * 0.5).tan();
        let through = camera
            .view()
            .inverse()
            .transform_vector3(Vec3::new(0.75 * half * 2., 0.75 * half, -1.))
            .normalize();
        assert!(along(0.75, 0.75).abs_diff_eq(through, 1e-5));
        // Picking rounds the camera's place into its rays; the sky leaves it out.
        let (_, ray) = camera.ray(size, [700., 50.]);
        assert!(along(0.75, 0.75).abs_diff_eq(ray, 1e-3), "{ray}");
        camera.kind = SceneKind::TwoD;
        assert!(super::directions(&camera, size).is_none());
    }
}
