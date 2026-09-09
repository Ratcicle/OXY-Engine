//! GPU component overlays. One immutable local instance stream; depth is from this frame.
//! No visibility rays, readback, per-camera geometry or egui shape per edge.
use super::*;
use oxy_core::geometry::{
    EditableMesh,
    selection::{Mode, Selection},
};

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct ComponentInstance {
    a: [f32; 3],
    b: [f32; 3],
    c: [f32; 3],
    ordinal: u32,
}
#[repr(C)]
#[derive(Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniform {
    mvp: [f32; 16],
    viewport: [f32; 4],
    ranges: [u32; 4],
    layer: [f32; 4],
}
struct Geometry {
    revision: u64,
    instances: wgpu::Buffer,
    flags: wgpu::Buffer,
    group: wgpu::BindGroup,
    count: u32,
    bytes: u64,
    selection: Option<(Mode, Vec<u32>)>,
}
pub(crate) struct ComponentOverlay {
    layout: wgpu::BindGroupLayout,
    uniform: wgpu::Buffer,
    previous: Option<Uniform>,
    visible: wgpu::RenderPipeline,
    hidden: wgpu::RenderPipeline,
    depth_2d: wgpu::RenderPipeline,
    geometry: Option<Geometry>,
}
impl ComponentOverlay {
    pub fn new(
        device: &wgpu::Device,
        camera: &wgpu::BindGroupLayout,
        texture: &wgpu::BindGroupLayout,
    ) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("OXY component overlay resources"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("OXY component camera and settings"),
            size: std::mem::size_of::<Uniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("OXY component pipeline layout"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("OXY component shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("component_overlay.wgsl").into()),
        });
        let visible = component_pipeline(device, &pipeline_layout, &shader, false);
        let hidden = component_pipeline(device, &pipeline_layout, &shader, true);
        let depth_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("OXY private 2D depth layout"),
            bind_group_layouts: &[camera, texture, &layout],
            push_constant_ranges: &[],
        });
        let depth_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("OXY private 2D depth shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("component_depth_2d.wgsl").into()),
        });
        let depth_2d = depth_pipeline(device, &depth_layout, &depth_shader);
        Self {
            layout,
            uniform,
            previous: None,
            visible,
            hidden,
            depth_2d,
            geometry: None,
        }
    }
    pub fn clear(&mut self) {
        self.geometry = None;
        self.previous = None;
    }
    pub fn bytes(&self) -> u64 {
        self.geometry.as_ref().map_or(0, |g| g.bytes)
    }
    fn prepare(
        &mut self,
        rs: &RenderState,
        mesh: &EditableMesh,
        selection: &Selection,
        uniform: Uniform,
        stats: &mut RendererStats,
    ) {
        if self
            .geometry
            .as_ref()
            .is_none_or(|g| g.revision != mesh.revision())
        {
            let data = mesh.data();
            let mut instances = Vec::with_capacity(
                mesh.prepared().triangles.len() + data.edges.len() + data.vertices.len(),
            );
            for triangle in &mesh.prepared().triangles {
                let [a, b, c] = mesh.triangle_points(triangle).map(|p| p.to_array());
                instances.push(ComponentInstance {
                    a,
                    b,
                    c,
                    ordinal: mesh.prepared().faces[&triangle.face] as u32,
                });
            }
            let edges_offset = data.faces.len() as u32;
            for (i, edge) in data.edges.iter().enumerate() {
                let [a, b] = edge
                    .vertices
                    .map(|id| mesh.position(id).expect("Validated edge").to_array());
                instances.push(ComponentInstance {
                    a,
                    b,
                    c: a,
                    ordinal: edges_offset + i as u32,
                });
            }
            let vertices_offset = edges_offset + data.edges.len() as u32;
            for (i, vertex) in data.vertices.iter().enumerate() {
                instances.push(ComponentInstance {
                    a: vertex.position,
                    b: vertex.position,
                    c: vertex.position,
                    ordinal: vertices_offset + i as u32,
                });
            }
            let flags_size = (data.faces.len() + data.edges.len() + data.vertices.len()).max(1) * 4;
            let flags = rs.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("OXY component selection flags"),
                size: flags_size as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let instances_buffer =
                rs.device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("OXY cached component instances"),
                        contents: if instances.is_empty() {
                            &[0; 40]
                        } else {
                            bytemuck::cast_slice(&instances)
                        },
                        usage: wgpu::BufferUsages::VERTEX,
                    });
            let group = rs.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("OXY component resources"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.uniform.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: flags.as_entire_binding(),
                    },
                ],
            });
            self.geometry = Some(Geometry {
                revision: mesh.revision(),
                bytes: instances_buffer.size() + flags.size(),
                instances: instances_buffer,
                flags,
                group,
                count: instances.len() as u32,
                selection: None,
            });
            stats.component_mesh_uploads += 1;
            stats.buffer_allocations += 2;
        }
        let geometry = self.geometry.as_mut().expect("Prepared component stream");
        if geometry
            .selection
            .as_ref()
            .is_none_or(|(mode, ids)| *mode != selection.mode || ids != &selection.ids)
        {
            let d = mesh.data();
            let mut flags = vec![0u32; (d.faces.len() + d.edges.len() + d.vertices.len()).max(1)];
            for id in &selection.ids {
                let index = match selection.mode {
                    Mode::Object => None,
                    Mode::Face => mesh.prepared().faces.get(id).copied(),
                    Mode::Edge => mesh.prepared().edges.get(id).map(|i| d.faces.len() + i),
                    Mode::Vertex => mesh
                        .prepared()
                        .vertices
                        .get(id)
                        .map(|i| d.faces.len() + d.edges.len() + i),
                };
                if let Some(index) = index {
                    flags[index] = 1;
                }
            }
            rs.queue
                .write_buffer(&geometry.flags, 0, bytemuck::cast_slice(&flags));
            geometry.selection = Some((selection.mode, selection.ids.clone()));
            stats.component_selection_uploads += 1;
        }
        if self.previous != Some(uniform) {
            rs.queue
                .write_buffer(&self.uniform, 0, bytemuck::bytes_of(&uniform));
            self.previous = Some(uniform);
            stats.uniform_uploads += 1;
        }
    }
}

impl Renderer {
    /// Batched overlays on the current color/depth target. Must follow render for this scene.
    /// Selection updates only flags; camera changes only uniforms. The game never calls this API.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_mesh_components(
        &mut self,
        rs: &RenderState,
        mesh: &EditableMesh,
        world: Mat4,
        camera: &CameraState,
        pixels_per_point: f32,
        selection: &Selection,
        temporary_through: bool,
    ) {
        let Some(selected_instance) = self.last_selected_instance else {
            return;
        };
        let size = self.target.size;
        let data = mesh.data();
        let is_2d = self.last_kind == SceneKind::TwoD;
        let uniform = Uniform {
            mvp: (camera.matrix(size) * world).to_cols_array(),
            viewport: [
                size[0] as f32,
                size[1] as f32,
                pixels_per_point,
                u8::from(selection.through || temporary_through) as f32,
            ],
            ranges: [
                match selection.mode {
                    Mode::Object => 0,
                    Mode::Face => 1,
                    Mode::Edge => 2,
                    Mode::Vertex => 3,
                },
                data.faces.len() as u32,
                data.edges.len() as u32,
                data.vertices.len() as u32,
            ],
            layer: [
                u8::from(is_2d) as f32,
                1. - (selected_instance + 1) as f32 / (self.last_entity_count + 1) as f32,
                self.last_entity_count as f32,
                0.,
            ],
        };
        self.components
            .prepare(rs, mesh, selection, uniform, &mut self.stats);
        let geometry = self.components.geometry.as_ref().expect("Prepared overlay");
        let mut encoder = rs
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("OXY current frame component overlay"),
            });
        if is_2d {
            let colors = [Some(wgpu::RenderPassColorAttachment {
                view: &self.target.view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })];
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("OXY layer ordered component depth"),
                color_attachments: &colors,
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
            pass.set_pipeline(&self.components.depth_2d);
            pass.set_bind_group(0, &self.uniform_group, &[]);
            pass.set_bind_group(2, &geometry.group, &[]);
            pass.set_vertex_buffer(1, self.instances.buffer.slice(..));
            for draw in &self.last_draws {
                pass.set_vertex_buffer(0, draw.mesh.vertices.slice(..));
                pass.set_index_buffer(draw.mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.set_bind_group(1, &draw.texture, &[]);
                pass.draw_indexed(
                    0..draw.mesh.index_count,
                    0,
                    draw.instance..draw.instance + 1,
                );
            }
            self.stats.component_draw_calls += self.last_draws.len() as u64;
        }
        {
            let colors = [Some(wgpu::RenderPassColorAttachment {
                view: &self.target.view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })];
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("OXY visible and hidden components"),
                color_attachments: &colors,
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.target.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_bind_group(0, &geometry.group, &[]);
            pass.set_vertex_buffer(0, geometry.instances.slice(..));
            pass.set_pipeline(&self.components.hidden);
            pass.draw(0..6, 0..geometry.count);
            pass.set_pipeline(&self.components.visible);
            pass.draw(0..6, 0..geometry.count);
            self.stats.component_draw_calls += 2;
        }
        rs.queue.submit([encoder.finish()]);
    }
    /// Drop the single active mesh stream when leaving component editing or changing project.
    pub fn clear_mesh_components(&mut self) {
        self.components.clear();
    }
}

fn component_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    hidden: bool,
) -> wgpu::RenderPipeline {
    const ATTR: [wgpu::VertexAttribute; 4] =
        wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x3,2=>Float32x3,3=>Uint32];
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(if hidden {
            "OXY hidden component pass"
        } else {
            "OXY visible component pass"
        }),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_component"),
            compilation_options: Default::default(),
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: 40,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &ATTR,
            }],
        },
        primitive: wgpu::PrimitiveState {
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: false,
            depth_compare: if hidden {
                wgpu::CompareFunction::Greater
            } else {
                wgpu::CompareFunction::LessEqual
            },
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(if hidden { "fs_hidden" } else { "fs_visible" }),
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
fn depth_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
) -> wgpu::RenderPipeline {
    const VERT: [wgpu::VertexAttribute; 3] =
        wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x3,2=>Float32x2];
    const INST: [wgpu::VertexAttribute; 10] = wgpu::vertex_attr_array![3=>Float32x4,4=>Float32x4,5=>Float32x4,6=>Float32x4,7=>Float32x4,8=>Float32x4,9=>Float32x4,10=>Float32x4,11=>Float32x4,12=>Float32x4];
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("OXY private 2D layer depth"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[
                wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<MeshVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &VERT,
                },
                wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<InstanceData>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &INST,
                },
            ],
        },
        primitive: Default::default(),
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: true,
            depth_compare: wgpu::CompareFunction::Always,
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
                blend: None,
                write_mask: wgpu::ColorWrites::empty(),
            })],
        }),
        multiview: None,
        cache: None,
    })
}
