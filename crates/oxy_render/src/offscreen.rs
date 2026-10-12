//! Rendering without a window: a GPU device of its own and pixel readback, for scene
//! screenshots requested by tools (the MCP server) and for GPU tests.
use super::*;
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

/// The game view of `runtime` at its current time, as PNG: what the player would show,
/// without the game interface (text, buttons), which is drawn by egui.
pub fn game_view_png(
    rs: &RenderState,
    renderer: &mut Renderer,
    runtime: &mut oxy_core::runtime::Runtime,
    root: &Path,
    size: [u32; 2],
) -> Result<Vec<u8>, String> {
    if size.iter().any(|v| !(16..=4096).contains(v)) {
        return Err("A imagem deve ter entre 16 e 4096 pixels de lado".into());
    }
    runtime.set_viewport_aspect(size[0] as f32 / size[1] as f32);
    let camera = crate::camera::CameraState::for_runtime(runtime);
    renderer.show_grid = false;
    renderer.render(
        rs,
        runtime.project(),
        runtime.scene(),
        root,
        &camera,
        size,
        None,
        false,
    );
    let (pixels, size) = renderer.read_pixels(rs)?;
    encode_png(pixels, size)
}
