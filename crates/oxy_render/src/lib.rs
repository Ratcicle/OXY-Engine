//! Shared native WGPU rendering for OXY Engine, Studio and the standalone player.
//! No editor state is read here. Meshes use real perspective/depth and stable UVs.
mod cache;
pub mod camera;
pub mod collider_debug;
pub mod game_ui;
pub mod input;
pub mod labels;
pub mod mesh;
pub use camera::CameraState;
pub use game_ui::GameUi;
pub use game_ui::pick_ui;
#[cfg(test)]
mod gpu_test;

use cache::{DynamicBuffer, GeometryCache, GpuMesh, InstanceData, MeshVertex};
use egui_wgpu::{RenderState, wgpu};
use glam::{Mat4, Vec2, Vec3};
#[cfg(test)]
use oxy_core::document::Primitive;
use oxy_core::document::{Entity, Id, Project, Scene, SceneKind};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 3],
    normal: [f32; 3],
    uv: [f32; 2],
    color: [f32; 4],
    lit: f32,
}

impl Vertex {
    fn line(position: Vec3, color: [f32; 4]) -> Self {
        Self {
            position: position.to_array(),
            normal: [0., 1., 0.],
            uv: [0.; 2],
            color,
            lit: 0.,
        }
    }
}

struct TextureEntry {
    _texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
}
struct TextureOverride {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}
struct Target {
    _color: wgpu::Texture,
    view: wgpu::TextureView,
    _depth: wgpu::Texture,
    depth_view: wgpu::TextureView,
    size: [u32; 2],
    id: egui::TextureId,
}
struct Draw {
    mesh: Arc<GpuMesh>,
    instance: u32,
    texture: wgpu::BindGroup,
}

/// Frame counts describe actual submitted draws, including non-empty debug line batches.
/// Upload/hit/allocation counters are cumulative since this renderer was created.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RendererStats {
    pub visible_objects: u64,
    pub draw_calls: u64,
    pub vertices: u64,
    pub triangles: u64,
    pub meshes: usize,
    /// Asset texture resources; distinct filtering modes each occupy one cache entry.
    pub textures: usize,
    pub cached_vertices: u64,
    pub cached_triangles: u64,
    pub mesh_cache_hits: u64,
    pub mesh_uploads: u64,
    pub texture_cache_hits: u64,
    pub texture_uploads: u64,
    pub instance_uploads: u64,
    pub line_uploads: u64,
    pub uniform_uploads: u64,
    /// Vertex, index, instance, line and camera buffers; excludes egui and readback buffers.
    pub buffer_allocations: u64,
}

pub struct Renderer {
    pipeline_3d: wgpu::RenderPipeline,
    pipeline_2d: wgpu::RenderPipeline,
    pipeline_lines: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    last_view: [f32; 16],
    uniform_group: wgpu::BindGroup,
    geometry: GeometryCache,
    instances: DynamicBuffer,
    grid: DynamicBuffer,
    overlays: DynamicBuffer,
    stats: RendererStats,
    texture_layout: wgpu::BindGroupLayout,
    textures: HashMap<(Id, bool), TextureEntry>,
    overrides: HashMap<Id, TextureOverride>,
    white: TextureEntry,
    target: Target,
    project_root: PathBuf,
    errors: Vec<String>,
    /// Grid is normally visible for editing and disabled for gameplay.
    pub show_grid: bool,
    debug_colliders: bool,
}

impl Renderer {
    pub fn new(rs: &RenderState) -> Self {
        let device = &rs.device;
        let uniform_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("OXY camera layout"),
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
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("OXY texture layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("OXY camera"),
            contents: bytemuck::cast_slice(&Mat4::IDENTITY.to_cols_array()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let uniform_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("OXY camera group"),
            layout: &uniform_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("OXY shared graphics layout"),
            bind_group_layouts: &[&uniform_layout, &texture_layout],
            push_constant_ranges: &[],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("OXY material shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let pipeline_3d = make_pipeline(device, &layout, &shader, true, false);
        let pipeline_2d = make_pipeline(device, &layout, &shader, false, false);
        let pipeline_lines = make_pipeline(device, &layout, &shader, false, true);
        let white = upload_texture(rs, &texture_layout, 1, 1, &[255; 4], true);
        let target = make_target(rs, [1, 1], None);
        Self {
            pipeline_3d,
            pipeline_2d,
            pipeline_lines,
            uniform,
            last_view: Mat4::IDENTITY.to_cols_array(),
            uniform_group,
            geometry: GeometryCache::default(),
            instances: DynamicBuffer::new(device, "OXY object instance stream"),
            grid: DynamicBuffer::new(device, "OXY persistent grid vertices"),
            overlays: DynamicBuffer::new(device, "OXY persistent overlay vertices"),
            stats: RendererStats {
                buffer_allocations: 4,
                ..Default::default()
            },
            texture_layout,
            textures: HashMap::new(),
            overrides: HashMap::new(),
            white,
            target,
            project_root: PathBuf::new(),
            errors: Vec::new(),
            show_grid: true,
            debug_colliders: false,
        }
    }

    pub fn stats(&self) -> RendererStats {
        let (meshes, cached_vertices, cached_triangles) = self.geometry.counts();
        RendererStats {
            meshes,
            textures: self.textures.len(),
            cached_vertices,
            cached_triangles,
            ..self.stats
        }
    }

    pub fn invalidate_texture(&mut self, id: &str) {
        self.textures.retain(|(key, _), _| key != id);
    }

    pub fn clear_texture_override(&mut self, id: &str) {
        self.overrides.remove(id);
        self.invalidate_texture(id);
    }

    /// Call only after the entire project save succeeds. Cached GPU textures stay
    /// intact; a later cache miss reads the now-current PNG from the project.
    pub fn release_saved_texture_pixels(&mut self) -> usize {
        let released = self.texture_override_bytes();
        self.overrides.clear();
        released
    }

    /// CPU RGBA allocations retained for unsaved painting, excluding GPU resources.
    pub fn texture_override_bytes(&self) -> usize {
        self.overrides
            .values()
            .map(|entry| entry.pixels.capacity())
            .sum()
    }

    pub fn clear_textures(&mut self) {
        self.textures.clear();
        self.overrides.clear();
    }

    /// In-memory painting never writes the source PNG. A document save commits pixels separately.
    pub fn set_texture_pixels(
        &mut self,
        rs: &RenderState,
        id: &str,
        width: u32,
        height: u32,
        rgba: &[u8],
        nearest: bool,
    ) -> Result<(), String> {
        let limit = rs.device.limits().max_texture_dimension_2d;
        if width == 0
            || height == 0
            || width > limit
            || height > limit
            || width as usize * height as usize * 4 != rgba.len()
        {
            return Err("Dimensões ou pixels da textura inválidos para a GPU".into());
        }
        let unchanged = self.overrides.get(id).is_some_and(|pixels| {
            pixels.width == width && pixels.height == height && pixels.pixels == rgba
        });
        if !unchanged {
            // Brush strokes update existing allocations, including both cached sampling modes.
            self.textures.retain(|(key, _), entry| {
                key != id || (entry._texture.width() == width && entry._texture.height() == height)
            });
            for ((key, _), entry) in &self.textures {
                if key == id {
                    write_texture_pixels(rs, &entry._texture, width, height, rgba);
                    self.stats.texture_uploads += 1;
                }
            }
            self.overrides.insert(
                id.to_owned(),
                TextureOverride {
                    width,
                    height,
                    pixels: rgba.to_vec(),
                },
            );
        }
        self.textures
            .entry((id.to_owned(), nearest))
            .or_insert_with(|| {
                self.stats.texture_uploads += 1;
                upload_texture(rs, &self.texture_layout, width, height, rgba, nearest)
            });
        Ok(())
    }

    pub fn take_errors(&mut self) -> Vec<String> {
        std::mem::take(&mut self.errors)
    }

    fn texture(
        &mut self,
        rs: &RenderState,
        project: &Project,
        root: &Path,
        id: Option<&str>,
        nearest: bool,
    ) -> wgpu::BindGroup {
        let Some(id) = id else {
            return self.white.bind_group.clone();
        };
        let key = (id.to_owned(), nearest);
        if !self.textures.contains_key(&key) {
            let loaded = if let Some(pixels) = self.overrides.get(id) {
                Ok((pixels.width, pixels.height, pixels.pixels.clone()))
            } else {
                project
                    .asset(id)
                    .ok_or_else(|| format!("Referência de textura ausente: {id}"))
                    .and_then(|asset| {
                        image::open(root.join(&asset.path))
                            .map_err(|error| format!("Textura {}: {error}", asset.name))
                    })
                    .map(|image| {
                        let image = image.to_rgba8();
                        (image.width(), image.height(), image.into_raw())
                    })
            };
            let pixels = loaded.and_then(|(width, height, pixels)| {
                let limit = rs.device.limits().max_texture_dimension_2d;
                if width > limit || height > limit {
                    Err(format!(
                        "Textura {id} excede o limite de {limit} pixels da GPU"
                    ))
                } else {
                    Ok((width, height, pixels))
                }
            });
            let entry = match pixels {
                Ok((width, height, pixels)) => {
                    upload_texture(rs, &self.texture_layout, width, height, &pixels, nearest)
                }
                Err(error) => {
                    self.errors.push(error);
                    upload_texture(
                        rs,
                        &self.texture_layout,
                        2,
                        2,
                        &[
                            255, 0, 190, 255, 30, 30, 30, 255, 30, 30, 30, 255, 255, 0, 190, 255,
                        ],
                        nearest,
                    )
                }
            };
            self.textures.insert(key.clone(), entry);
            self.stats.texture_uploads += 1;
        } else {
            self.stats.texture_cache_hits += 1;
        }
        self.textures[&key].bind_group.clone()
    }

    /// Draw into a color/depth target, then expose it as a native egui texture.
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        rs: &RenderState,
        project: &Project,
        scene: &Scene,
        root: &Path,
        camera: &CameraState,
        size: [u32; 2],
        selected: Option<Id>,
        debug_colliders: bool,
    ) -> egui::TextureId {
        self.debug_colliders = debug_colliders;
        if self.project_root != root {
            self.textures.clear();
            self.project_root = root.to_owned();
        }
        let limit = rs.device.limits().max_texture_dimension_2d.min(8192);
        let factor = (limit as f32 / size[0].max(size[1]).max(1) as f32).min(1.);
        let size = [
            (size[0] as f32 * factor).round().max(1.) as u32,
            (size[1] as f32 * factor).round().max(1.) as u32,
        ];
        if self.target.size != size {
            self.target = make_target(rs, size, Some(self.target.id));
        }
        let view = camera.matrix(size).to_cols_array();
        if self.last_view != view {
            rs.queue
                .write_buffer(&self.uniform, 0, bytemuck::cast_slice(&view));
            self.last_view = view;
            self.stats.uniform_uploads += 1;
        }
        self.stats.visible_objects = 0;
        self.stats.draw_calls = 0;
        self.stats.vertices = 0;
        self.stats.triangles = 0;
        let mut instances = Vec::new();
        let mut draws = Vec::new();
        let mut entities: Vec<_> = scene
            .entities
            .iter()
            .filter(|entity| {
                entity.primitive.is_some() && entity.ui.is_none() && is_visible(scene, entity)
            })
            .collect();
        if scene.kind == SceneKind::TwoD {
            entities.sort_by_key(|entity| entity.layer);
        } else {
            // Opaque geometry first; alpha materials far-to-near. Depth still handles occlusion.
            entities.sort_by(|a, b| {
                let aa = a.material.color[3] < 1.;
                let ba = b.material.color[3] < 1.;
                aa.cmp(&ba).then_with(|| {
                    if !aa {
                        return std::cmp::Ordering::Equal;
                    }
                    let distance = |e: &Entity| {
                        scene
                            .world_matrix(&e.id)
                            .map(|w| {
                                w.transform_point3(Vec3::ZERO)
                                    .distance_squared(camera.eye())
                            })
                            .unwrap_or(0.)
                    };
                    distance(b).total_cmp(&distance(a))
                })
            });
        }
        for entity in &entities {
            let Ok(world) = scene.world_matrix(&entity.id) else {
                continue;
            };
            let world = world * Mat4::from_scale(Vec3::from(entity.dimensions));
            let key = mesh::MeshKey::for_entity(entity).expect("Filtered primitive");
            let (mesh, hit) = self.geometry.get(&rs.device, key);
            self.stats.mesh_cache_hits += u64::from(hit);
            self.stats.mesh_uploads += u64::from(!hit);
            self.stats.buffer_allocations += 2 * u64::from(!hit);
            self.stats.visible_objects += 1;
            self.stats.vertices += u64::from(mesh.vertex_count);
            self.stats.triangles += u64::from(mesh.index_count / 3);
            let instance = instances.len() as u32;
            instances.push(InstanceData {
                world: world.to_cols_array_2d(),
                normal: world.inverse().transpose().to_cols_array_2d(),
                color: entity.material.color,
                parameters: [
                    if scene.kind == SceneKind::ThreeD {
                        1.
                    } else {
                        0.
                    },
                    0.,
                    0.,
                    0.,
                ],
            });
            draws.push(Draw {
                mesh,
                instance,
                texture: self.texture(
                    rs,
                    project,
                    root,
                    entity.material.texture.as_deref(),
                    entity.material.nearest,
                ),
            });
        }
        let mut grid = Vec::new();
        if self.show_grid {
            grid_lines(&mut grid, camera, scene.kind);
        }
        let mut overlays = Vec::new();
        if let Some(selected) = selected.as_deref() {
            for entity in &entities {
                if entity.id == selected
                    && let Ok(world) = scene.world_matrix(&entity.id)
                {
                    let world = world * Mat4::from_scale(Vec3::from(entity.dimensions));
                    box_lines(
                        &mut overlays,
                        Vec3::splat(-0.5),
                        Vec3::splat(0.5),
                        world,
                        [1., 0.7, 0.24, 1.],
                        scene.kind == SceneKind::TwoD,
                    );
                }
            }
        }
        let (allocated, uploaded) = self.instances.update(rs, bytemuck::cast_slice(&instances));
        self.stats.buffer_allocations += u64::from(allocated);
        self.stats.instance_uploads += u64::from(uploaded);
        for (buffer, vertices) in [(&mut self.grid, &grid), (&mut self.overlays, &overlays)] {
            let (allocated, uploaded) = buffer.update(rs, bytemuck::cast_slice(vertices));
            self.stats.buffer_allocations += u64::from(allocated);
            self.stats.line_uploads += u64::from(uploaded);
            if !vertices.is_empty() {
                self.stats.draw_calls += 1;
                self.stats.vertices += vertices.len() as u64;
            }
        }
        self.stats.draw_calls += draws.len() as u64;
        let mut encoder = rs
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("OXY viewport encoder"),
            });
        {
            let attachments = [Some(wgpu::RenderPassColorAttachment {
                view: &self.target.view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.048,
                        g: 0.058,
                        b: 0.074,
                        a: 1.,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })];
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("OXY real 2D/3D viewport"),
                color_attachments: &attachments,
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.target.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_bind_group(0, &self.uniform_group, &[]);
            pass.set_bind_group(1, &self.white.bind_group, &[]);
            if !grid.is_empty() {
                pass.set_pipeline(&self.pipeline_lines);
                pass.set_vertex_buffer(0, self.grid.buffer.slice(..));
                pass.draw(0..grid.len() as u32, 0..1);
            }
            pass.set_pipeline(if scene.kind == SceneKind::ThreeD {
                &self.pipeline_3d
            } else {
                &self.pipeline_2d
            });
            pass.set_vertex_buffer(1, self.instances.buffer.slice(..));
            for draw in &draws {
                pass.set_vertex_buffer(0, draw.mesh.vertices.slice(..));
                pass.set_index_buffer(draw.mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.set_bind_group(1, &draw.texture, &[]);
                // Keep material/layer ordering. Adjacent compatible draws can later be batched
                // simply by widening this instance range, without another vertex format change.
                pass.draw_indexed(
                    0..draw.mesh.index_count,
                    0,
                    draw.instance..draw.instance + 1,
                );
            }
            if !overlays.is_empty() {
                pass.set_pipeline(&self.pipeline_lines);
                pass.set_bind_group(1, &self.white.bind_group, &[]);
                pass.set_vertex_buffer(0, self.overlays.buffer.slice(..));
                pass.draw(0..overlays.len() as u32, 0..1);
            }
        }
        rs.queue.submit([encoder.finish()]);
        self.target.id
    }
    /// Complete the debug_colliders path after the viewport image/game UI is drawn.
    /// Stroke widths are logical screen points, independent of zoom and scene depth.
    pub fn draw_colliders(
        &self,
        ui: &egui::Ui,
        scene: &Scene,
        camera: &CameraState,
        rect: egui::Rect,
        selected: &[Id],
        show_disabled: bool,
    ) -> collider_debug::OverlayFrame {
        collider_debug::draw(
            ui,
            scene,
            camera,
            rect,
            self.debug_colliders,
            selected,
            show_disabled,
        )
    }
}

fn make_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    depth: bool,
    lines: bool,
) -> wgpu::RenderPipeline {
    const MESH_ATTRIBUTES: [wgpu::VertexAttribute; 3] =
        wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2];
    const INSTANCE_ATTRIBUTES: [wgpu::VertexAttribute; 10] = wgpu::vertex_attr_array![
        3 => Float32x4, 4 => Float32x4, 5 => Float32x4, 6 => Float32x4,
        7 => Float32x4, 8 => Float32x4, 9 => Float32x4, 10 => Float32x4,
        11 => Float32x4, 12 => Float32x4
    ];
    const LINE_ATTRIBUTES: [wgpu::VertexAttribute; 5] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2, 3 => Float32x4, 4 => Float32];
    let mesh_layouts = [
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<MeshVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &MESH_ATTRIBUTES,
        },
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<InstanceData>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &INSTANCE_ATTRIBUTES,
        },
    ];
    let line_layouts = [wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<Vertex>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &LINE_ATTRIBUTES,
    }];
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("OXY primitive pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some(if lines { "vs_line" } else { "vs_main" }),
            compilation_options: Default::default(),
            buffers: if lines { &line_layouts } else { &mesh_layouts },
        },
        primitive: wgpu::PrimitiveState {
            topology: if lines {
                wgpu::PrimitiveTopology::LineList
            } else {
                wgpu::PrimitiveTopology::TriangleList
            },
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: depth,
            depth_compare: if depth {
                wgpu::CompareFunction::LessEqual
            } else {
                wgpu::CompareFunction::Always
            },
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba8Unorm,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview: None,
        cache: None,
    })
}

fn upload_texture(
    rs: &RenderState,
    layout: &wgpu::BindGroupLayout,
    width: u32,
    height: u32,
    rgba: &[u8],
    nearest: bool,
) -> TextureEntry {
    let size = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let texture = rs.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("OXY editable PNG"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    write_texture_pixels(rs, &texture, width, height, rgba);
    let view = texture.create_view(&Default::default());
    let filter = if nearest {
        wgpu::FilterMode::Nearest
    } else {
        wgpu::FilterMode::Linear
    };
    let sampler = rs.device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("OXY PNG sampler"),
        mag_filter: filter,
        min_filter: filter,
        ..Default::default()
    });
    let bind_group = rs.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("OXY material texture"),
        layout,
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
    TextureEntry {
        _texture: texture,
        bind_group,
    }
}

fn write_texture_pixels(
    rs: &RenderState,
    texture: &wgpu::Texture,
    width: u32,
    height: u32,
    rgba: &[u8],
) {
    rs.queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        rgba,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width * 4),
            rows_per_image: Some(height),
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
}

fn make_target(rs: &RenderState, size: [u32; 2], id: Option<egui::TextureId>) -> Target {
    let extent = wgpu::Extent3d {
        width: size[0],
        height: size[1],
        depth_or_array_layers: 1,
    };
    let color = rs.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("OXY viewport color"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = color.create_view(&Default::default());
    let depth = rs.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("OXY viewport depth"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let depth_view = depth.create_view(&Default::default());
    let mut renderer = rs.renderer.write();
    let id = if let Some(id) = id {
        renderer.update_egui_texture_from_wgpu_texture(
            &rs.device,
            &view,
            wgpu::FilterMode::Linear,
            id,
        );
        id
    } else {
        renderer.register_native_texture(&rs.device, &view, wgpu::FilterMode::Linear)
    };
    Target {
        _color: color,
        view,
        _depth: depth,
        depth_view,
        size,
        id,
    }
}

pub fn entity_mesh(entity: &Entity) -> mesh::Mesh {
    mesh::MeshKey::for_entity(entity)
        .map(|key| (*mesh::cached_primitive(key)).clone())
        .unwrap_or_default()
}

fn is_visible(scene: &Scene, entity: &Entity) -> bool {
    let mut current = Some(entity);
    let mut remaining = scene.entities.len() + 1;
    while let Some(entity) = current {
        if !entity.visible || remaining == 0 {
            return false;
        }
        remaining -= 1;
        current = entity.parent.as_deref().and_then(|id| scene.entity(id));
    }
    true
}

#[derive(Clone, Debug)]
pub struct PickHit {
    pub entity: Id,
    pub uv: [f32; 2],
    pub position: Vec3,
    pub distance: f32,
}

pub fn pick(
    scene: &Scene,
    camera: &CameraState,
    size: [u32; 2],
    pixel: [f32; 2],
) -> Option<PickHit> {
    let (origin, direction) = camera.ray(size, pixel);
    let mut best: Option<PickHit> = None;
    let mut best_layer = i32::MIN;
    for entity in &scene.entities {
        if entity.primitive.is_none() || entity.ui.is_some() || !is_visible(scene, entity) {
            continue;
        }
        let Ok(world) = scene.world_matrix(&entity.id) else {
            continue;
        };
        let world = world * Mat4::from_scale(Vec3::from(entity.dimensions));
        let mesh = mesh::cached_primitive(mesh::MeshKey::for_entity(entity)?);
        for triangle in mesh.indices.chunks_exact(3) {
            let vertices = triangle.map_indices(&mesh.vertices);
            let points = vertices.map(|vertex| world.transform_point3(vertex.position));
            if let Some((distance, barycentric)) = mesh::ray_triangle(origin, direction, points) {
                let closer = if scene.kind == SceneKind::TwoD {
                    entity.layer >= best_layer
                } else {
                    best.as_ref().is_none_or(|hit| distance < hit.distance)
                };
                if closer {
                    let uv = vertices
                        .iter()
                        .zip(barycentric)
                        .fold(Vec2::ZERO, |sum, (vertex, weight)| sum + vertex.uv * weight);
                    best = Some(PickHit {
                        entity: entity.id.clone(),
                        uv: uv.to_array(),
                        position: origin + direction * distance,
                        distance,
                    });
                    best_layer = entity.layer;
                }
            }
        }
    }
    best
}

trait TriangleVertices {
    fn map_indices<'a>(&self, vertices: &'a [mesh::MeshVertex]) -> [&'a mesh::MeshVertex; 3];
}
impl TriangleVertices for [u32] {
    fn map_indices<'a>(&self, vertices: &'a [mesh::MeshVertex]) -> [&'a mesh::MeshVertex; 3] {
        [
            &vertices[self[0] as usize],
            &vertices[self[1] as usize],
            &vertices[self[2] as usize],
        ]
    }
}

fn grid_lines(vertices: &mut Vec<Vertex>, camera: &CameraState, kind: SceneKind) {
    let span = if kind == SceneKind::TwoD {
        camera.orthographic_size * 3.
    } else {
        camera.distance * 2.
    };
    let step = 10f32.powf((span / 40.).log10().ceil()).max(0.1);
    let center = (camera.target / step).round() * step;
    let extent = step * 40.;
    for index in -40..=40 {
        let offset = index as f32 * step;
        let color = [0.115, 0.135, 0.16, 1.];
        if kind == SceneKind::TwoD {
            line(
                vertices,
                Vec3::new(center.x + offset, center.y - extent, 0.),
                Vec3::new(center.x + offset, center.y + extent, 0.),
                color,
            );
            line(
                vertices,
                Vec3::new(center.x - extent, center.y + offset, 0.),
                Vec3::new(center.x + extent, center.y + offset, 0.),
                color,
            );
        } else {
            line(
                vertices,
                Vec3::new(center.x + offset, 0., center.z - extent),
                Vec3::new(center.x + offset, 0., center.z + extent),
                color,
            );
            line(
                vertices,
                Vec3::new(center.x - extent, 0., center.z + offset),
                Vec3::new(center.x + extent, 0., center.z + offset),
                color,
            );
        }
    }
    line(
        vertices,
        Vec3::NEG_X * extent,
        Vec3::X * extent,
        [0.48, 0.22, 0.24, 1.],
    );
    let axis = if kind == SceneKind::TwoD {
        Vec3::Y
    } else {
        Vec3::Z
    };
    line(
        vertices,
        -axis * extent,
        axis * extent,
        [0.24, 0.38, 0.48, 1.],
    );
}
fn line(vertices: &mut Vec<Vertex>, a: Vec3, b: Vec3, color: [f32; 4]) {
    vertices.extend([Vertex::line(a, color), Vertex::line(b, color)]);
}
fn box_lines(
    vertices: &mut Vec<Vertex>,
    min: Vec3,
    max: Vec3,
    world: Mat4,
    color: [f32; 4],
    flat: bool,
) {
    let z = if flat { (min.z + max.z) * 0.5 } else { min.z };
    let points = [
        Vec3::new(min.x, min.y, z),
        Vec3::new(max.x, min.y, z),
        Vec3::new(max.x, max.y, z),
        Vec3::new(min.x, max.y, z),
        Vec3::new(min.x, min.y, max.z),
        Vec3::new(max.x, min.y, max.z),
        Vec3::new(max.x, max.y, max.z),
        Vec3::new(min.x, max.y, max.z),
    ]
    .map(|p| world.transform_point3(p));
    for [a, b] in [[0, 1], [1, 2], [2, 3], [3, 0]] {
        line(vertices, points[a], points[b], color);
    }
    if !flat {
        for [a, b] in [
            [4, 5],
            [5, 6],
            [6, 7],
            [7, 4],
            [0, 4],
            [1, 5],
            [2, 6],
            [3, 7],
        ] {
            line(vertices, points[a], points[b], color);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn picking_obeys_2d_layers_and_returns_texture_coordinates() {
        let mut scene = Scene::new("Teste", SceneKind::TwoD);
        let mut back = Entity::new("Fundo", Some(Primitive::Rectangle));
        back.layer = -1;
        let mut front = Entity::new("Frente", Some(Primitive::Rectangle));
        front.layer = 10;
        let id = front.id.clone();
        scene.entities = vec![front, back];
        let camera = CameraState::for_scene(&scene);
        let hit = pick(&scene, &camera, [640, 480], [320., 240.]).unwrap();
        assert_eq!(hit.entity, id);
        assert!((hit.uv[0] - 0.5).abs() < 0.001 && (hit.uv[1] - 0.5).abs() < 0.001);
    }
}
