//! Device-owned buffers. Geometry is immutable; per-object state has its own instance stream.
use crate::mesh::{self, MeshKey};
use egui_wgpu::{RenderState, wgpu};
use std::{collections::VecDeque, sync::Arc};
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct MeshVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct InstanceData {
    pub world: [[f32; 4]; 4],
    pub normal: [[f32; 4]; 4],
    pub color: [f32; 4],
    pub parameters: [f32; 4],
}

pub(crate) struct GpuMesh {
    pub vertices: wgpu::Buffer,
    pub indices: wgpu::Buffer,
    pub vertex_count: u32,
    pub index_count: u32,
    bytes: usize,
}

/// A bounded LRU prevents dragging the segment count from retaining every intermediate mesh.
/// Draws retain an Arc so eviction cannot invalidate geometry already queued in the same frame.
#[derive(Default)]
pub(crate) struct GeometryCache {
    entries: VecDeque<(MeshKey, Arc<GpuMesh>)>,
    bytes: usize,
}

impl GeometryCache {
    pub fn get(
        &mut self,
        device: &wgpu::Device,
        entity: &oxy_core::document::Entity,
    ) -> (Arc<GpuMesh>, bool) {
        let key = MeshKey::for_entity(entity).expect("Filtered geometry");
        if let Some(index) = self.entries.iter().position(|(stored, _)| *stored == key) {
            let entry = self.entries.remove(index).expect("Known cache entry");
            let mesh = Arc::clone(&entry.1);
            self.entries.push_back(entry);
            return (mesh, true);
        }
        let mesh = mesh::cached_entity(entity).expect("Filtered geometry");
        let vertices: Vec<_> = mesh
            .vertices
            .iter()
            .map(|vertex| MeshVertex {
                position: vertex.position.to_array(),
                normal: vertex.normal.to_array(),
                uv: vertex.uv.to_array(),
            })
            .collect();
        let bytes = std::mem::size_of_val(vertices.as_slice())
            + std::mem::size_of_val(mesh.indices.as_slice());
        let gpu = Arc::new(GpuMesh {
            vertices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("OXY cached unit primitive vertices"),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            }),
            indices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("OXY cached primitive indices"),
                contents: bytemuck::cast_slice(&mesh.indices),
                usage: wgpu::BufferUsages::INDEX,
            }),
            vertex_count: vertices.len() as u32,
            index_count: mesh.indices.len() as u32,
            bytes,
        });
        const MAX_BYTES: usize = 64 * 1024 * 1024;
        while !self.entries.is_empty()
            && (self.entries.len() >= 64 || self.bytes + bytes > MAX_BYTES)
        {
            self.bytes -= self.entries.pop_front().expect("Non-empty cache").1.bytes;
        }
        self.entries.push_back((key, Arc::clone(&gpu)));
        self.bytes += bytes;
        (gpu, false)
    }

    pub fn counts(&self) -> (usize, u64, u64) {
        self.entries.iter().fold(
            (self.entries.len(), 0, 0),
            |(count, vertices, triangles), (_, mesh)| {
                (
                    count,
                    vertices + u64::from(mesh.vertex_count),
                    triangles + u64::from(mesh.index_count / 3),
                )
            },
        )
    }
}

pub(crate) struct DynamicBuffer {
    pub buffer: wgpu::Buffer,
    capacity: usize,
    previous: Vec<u8>,
    label: &'static str,
}

impl DynamicBuffer {
    pub fn new(device: &wgpu::Device, label: &'static str) -> Self {
        Self {
            buffer: Self::allocate(device, label, 4),
            capacity: 4,
            previous: Vec::new(),
            label,
        }
    }

    fn allocate(device: &wgpu::Device, label: &str, capacity: usize) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: capacity as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    /// Returns (allocated, uploaded). Equal data does not produce a queue write.
    pub fn update(&mut self, rs: &RenderState, bytes: &[u8]) -> (bool, bool) {
        if self.previous == bytes {
            return (false, false);
        }
        let allocated = bytes.len() > self.capacity;
        if allocated {
            self.capacity = bytes.len().next_power_of_two();
            self.buffer = Self::allocate(&rs.device, self.label, self.capacity);
        }
        if !bytes.is_empty() {
            rs.queue.write_buffer(&self.buffer, 0, bytes);
        }
        self.previous.clear();
        self.previous.extend_from_slice(bytes);
        (allocated, !bytes.is_empty())
    }
}
