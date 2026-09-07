//! Opt-in tests really submit triangles and read GPU pixels back; no screenshots are faked.
use super::*;
use std::{
    future::Future,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
    time::Duration,
};

fn block_on<T>(future: impl Future<Output = T>) -> T {
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

fn read_pixels(renderer: &Renderer, rs: &RenderState) -> Vec<u8> {
    let [width, height] = renderer.target.size;
    let stride = (width * 4).div_ceil(256) * 256;
    let buffer = rs.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("OXY GPU evidence readback"),
        size: u64::from(stride * height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = rs.device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &renderer.target._color,
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
        .unwrap();
    receiver
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    let mapped = buffer.slice(..).get_mapped_range();
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for row in mapped.chunks(stride as usize) {
        pixels.extend_from_slice(&row[..(width * 4) as usize]);
    }
    drop(mapped);
    buffer.unmap();
    pixels
}

#[test]
#[ignore = "Requires a native WGPU adapter; run explicitly for graphics validation"]
fn native_gpu_depth_texture_and_resize() {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
        .expect("Native WGPU adapter");
    println!("Native GPU: {:?}", adapter.get_info());
    let (device, queue) =
        block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
    let egui_renderer =
        egui_wgpu::Renderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, Default::default());
    let rs = RenderState {
        adapter,
        available_adapters: Vec::new(),
        device,
        queue,
        target_format: wgpu::TextureFormat::Rgba8Unorm,
        renderer: Arc::new(egui::epaint::mutex::RwLock::new(egui_renderer)),
    };
    let mut renderer = Renderer::new(&rs);
    renderer.show_grid = false;
    let mut project = Project::new("GPU test");
    let scene = &mut project.scenes[0];
    scene.kind = SceneKind::ThreeD;
    let mut front = Entity::new("Front", Some(Primitive::Cube));
    front.material.color = [0., 1., 0., 1.];
    let mut back = Entity::new("Back", Some(Primitive::Cube));
    back.transform.position = [0., 0., -2.];
    back.dimensions = [2.; 3];
    back.material.color = [1., 0., 0., 1.];
    // Draw the back cube last, so this succeeds only with actual depth testing.
    scene.entities = vec![front, back];
    let mut camera = CameraState::for_scene(scene);
    camera.target = Vec3::ZERO;
    camera.yaw = 0.;
    camera.pitch = 0.;
    camera.distance = 4.;
    renderer.render(
        &rs,
        &project,
        &project.scenes[0],
        Path::new(""),
        &camera,
        [256, 256],
        None,
        false,
    );
    let pixels = read_pixels(&renderer, &rs);
    let center = (128 * 256 + 128) * 4;
    assert!(
        pixels[center + 1] > 100 && pixels[center] < 5,
        "Front green cube must occlude red back cube: {:?}",
        &pixels[center..center + 4]
    );
    project.scenes[0].entities[0].material.texture = Some("painted-in-memory".into());
    project.scenes[0].entities[0].material.color = [1.; 4];
    renderer
        .set_texture_pixels(&rs, "painted-in-memory", 1, 1, &[0, 0, 255, 255], true)
        .unwrap();
    renderer.render(
        &rs,
        &project,
        &project.scenes[0],
        Path::new(""),
        &camera,
        [512, 256],
        None,
        false,
    );
    let pixels = read_pixels(&renderer, &rs);
    let center = (128 * 512 + 256) * 4;
    assert!(
        pixels[center + 2] > 100 && pixels[center] < 5 && pixels[center + 1] < 5,
        "Live PNG override must be blue"
    );
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qa/native-depth-proof.png");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    image::save_buffer(&path, &pixels, 512, 256, image::ColorType::Rgba8).unwrap();
    println!("GPU depth readback evidence: {}", path.display());
    let mut atlas = vec![255; 96 * 64 * 4];
    let colors = [
        [190, 65, 65, 255],
        [60, 150, 220, 255],
        [90, 190, 90, 255],
        [150, 90, 205, 255],
        [235, 175, 60, 255],
        [60, 175, 180, 255],
    ];
    for y in 0..64 {
        for x in 0..96 {
            let color = colors[(y / 32) * 3 + x / 32];
            let offset = (y * 96 + x) * 4;
            atlas[offset..offset + 4].copy_from_slice(&color);
        }
    }
    renderer
        .set_texture_pixels(&rs, "painted-in-memory", 96, 64, &atlas, true)
        .unwrap();
    let scene = &mut project.scenes[0];
    scene.entities.truncate(1);
    scene.entities[0].transform.rotation[1] = 0.2;
    let mut sphere = Entity::new("Sphere", Some(Primitive::Sphere));
    sphere.transform.position = [-1.6, 0., 0.];
    sphere.material.color = [0.3, 0.7, 0.8, 1.];
    let mut cylinder = Entity::new("Cylinder", Some(Primitive::Cylinder));
    cylinder.transform.position = [1.6, 0., 0.];
    cylinder.material.color = [0.85, 0.5, 0.25, 1.];
    let mut plane = Entity::new("Plane", Some(Primitive::Plane));
    plane.transform.position = [0., -0.55, 0.];
    plane.dimensions = [6., 1., 4.];
    plane.material.color = [0.14, 0.17, 0.21, 1.];
    scene.entities.extend([sphere, cylinder, plane]);
    camera.yaw = 0.6;
    camera.pitch = 0.35;
    camera.distance = 5.5;
    renderer.show_grid = true;
    renderer.render(
        &rs,
        &project,
        &project.scenes[0],
        Path::new(""),
        &camera,
        [768, 512],
        None,
        false,
    );
    let pixels = read_pixels(&renderer, &rs);
    let path = path.with_file_name("native-gpu.png");
    image::save_buffer(&path, &pixels, 768, 512, image::ColorType::Rgba8).unwrap();
    println!("GPU primitive/UV readback evidence: {}", path.display());
    assert!(renderer.take_errors().is_empty());
}
