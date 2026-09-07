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
    let first_stats = renderer.stats();
    assert_eq!(
        first_stats.meshes, 1,
        "Both cubes share the same GPU buffers"
    );
    assert_eq!(first_stats.mesh_uploads, 1);
    assert_eq!(first_stats.mesh_cache_hits, 1);
    assert_eq!(first_stats.visible_objects, 2);
    assert_eq!(first_stats.draw_calls, 2);
    assert_eq!(first_stats.vertices, 48);
    assert_eq!(first_stats.triangles, 24);
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
    assert_eq!(
        read_pixels(&renderer, &rs),
        pixels,
        "Cached redraw preserves pixels"
    );
    let repeat_stats = renderer.stats();
    assert_eq!(
        repeat_stats.buffer_allocations,
        first_stats.buffer_allocations
    );
    assert_eq!(repeat_stats.mesh_uploads, first_stats.mesh_uploads);
    assert_eq!(repeat_stats.instance_uploads, first_stats.instance_uploads);
    assert_eq!(repeat_stats.uniform_uploads, first_stats.uniform_uploads);
    assert_eq!(
        repeat_stats.mesh_cache_hits,
        first_stats.mesh_cache_hits + 2
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
    assert_eq!(
        renderer.stats().mesh_uploads,
        first_stats.mesh_uploads,
        "Changing material and resizing never regenerate geometry"
    );
    assert_eq!(
        renderer.stats().instance_uploads,
        first_stats.instance_uploads + 1
    );
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qa/v0.1.2/native-depth-proof.png");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    image::save_buffer(&path, &pixels, 512, 256, image::ColorType::Rgba8).unwrap();
    println!("GPU depth readback evidence: {}", path.display());
    let key = ("painted-in-memory".to_owned(), true);
    let texture = renderer.textures[&key]._texture.clone();
    let uploads = renderer.stats().texture_uploads;
    renderer
        .set_texture_pixels(&rs, &key.0, 1, 1, &[0, 0, 255, 255], true)
        .unwrap();
    assert_eq!(
        renderer.stats().texture_uploads,
        uploads,
        "Identical pixels skip upload"
    );
    renderer
        .set_texture_pixels(&rs, &key.0, 1, 1, &[0, 0, 255, 255], false)
        .unwrap();
    let smooth_texture = renderer.textures[&(key.0.clone(), false)]._texture.clone();
    renderer
        .set_texture_pixels(&rs, &key.0, 1, 1, &[0, 255, 0, 255], true)
        .unwrap();
    assert_eq!(
        texture, renderer.textures[&key]._texture,
        "A brush reuses texture storage"
    );
    assert_eq!(
        smooth_texture,
        renderer.textures[&(key.0.clone(), false)]._texture
    );
    assert_eq!(
        renderer.stats().texture_uploads,
        uploads + 3,
        "Both sampling modes receive painted pixels"
    );
    project.scenes[0].entities[0].material.nearest = false;
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
    let painted = read_pixels(&renderer, &rs);
    assert!(
        painted[center + 1] > 100 && painted[center + 2] < 5,
        "The reused smooth texture reflects new pixels"
    );
    let before_release = renderer.stats();
    assert_eq!(renderer.texture_override_bytes(), 4);
    assert_eq!(renderer.release_saved_texture_pixels(), 4);
    assert_eq!(renderer.texture_override_bytes(), 0);
    assert_eq!(texture, renderer.textures[&key]._texture);
    assert_eq!(
        smooth_texture,
        renderer.textures[&(key.0.clone(), false)]._texture
    );
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
    assert_eq!(
        read_pixels(&renderer, &rs),
        painted,
        "Releasing saved CPU copies must preserve the rendered pixels"
    );
    assert_eq!(
        renderer.stats().texture_uploads,
        before_release.texture_uploads
    );
    project.scenes[0].entities[0].material.nearest = true;
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
    let before = renderer.stats();
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
    assert_eq!(read_pixels(&renderer, &rs), pixels);
    let after = renderer.stats();
    assert_eq!(before.meshes, 4);
    assert_eq!(
        before.draw_calls, 5,
        "Four objects plus one non-empty grid draw"
    );
    assert_eq!(after.buffer_allocations, before.buffer_allocations);
    assert_eq!(after.line_uploads, before.line_uploads);
    assert_eq!(after.instance_uploads, before.instance_uploads);
    assert_eq!(after.texture_uploads, before.texture_uploads);
    println!("Cached render stats: {after:?}");

    // Many independent entities vary transform/dimensions/material, while retaining one cube
    // topology. This exercises the indexed per-instance stream beyond a single object.
    let scene = &mut project.scenes[0];
    scene.entities.clear();
    for index in 0..128 {
        let mut entity = Entity::new("Independent cube", Some(Primitive::Cube));
        entity.transform.position = [(index % 16) as f32 - 8., (index / 16) as f32 - 4., 0.];
        entity.dimensions = [0.5 + index as f32 * 0.001, 0.5, 0.5];
        entity.transform.rotation = [0.1, index as f32 * 0.1, 0.];
        entity.material.color = [0.3, 0.7, 0.8, 1.];
        scene.entities.push(entity);
    }
    renderer.show_grid = false;
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
    let many_stats = renderer.stats();
    assert_eq!(many_stats.mesh_uploads, after.mesh_uploads);
    assert_eq!(many_stats.mesh_cache_hits, after.mesh_cache_hits + 128);
    assert_eq!(many_stats.draw_calls, 128);
    assert_eq!(many_stats.triangles, 128 * 12);
    assert_eq!(many_stats.vertices, 128 * 24);
    project.scenes[0].entities[0].primitive = Some(Primitive::Sphere);
    project.scenes[0].entities[0].segments = 31;
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
    assert_eq!(
        renderer.stats().mesh_uploads,
        many_stats.mesh_uploads + 1,
        "A new segment count uploads exactly one new primitive"
    );
    let scene = &mut project.scenes[0];
    scene.kind = SceneKind::TwoD;
    let mut front = Entity::new("Alpha foreground", Some(Primitive::Rectangle));
    front.layer = 1;
    front.material.color = [1., 0., 0., 0.5];
    let mut back = Entity::new("Sprite background", Some(Primitive::Sprite));
    back.material.color = [0., 1., 0., 1.];
    let mut hidden = Entity::new("Hidden circle", Some(Primitive::Circle));
    hidden.visible = false;
    scene.entities = vec![front, back, hidden];
    camera = CameraState::for_scene(scene);
    camera.target = Vec3::ZERO;
    camera.orthographic_size = 2.;
    let uploads = renderer.stats().mesh_uploads;
    renderer.render(
        &rs,
        &project,
        &project.scenes[0],
        Path::new(""),
        &camera,
        [128, 128],
        None,
        false,
    );
    let pixels = read_pixels(&renderer, &rs);
    let center = (64 * 128 + 64) * 4;
    assert!(
        (126..=129).contains(&pixels[center]) && (126..=129).contains(&pixels[center + 1]),
        "2D layer order and alpha blending survive mesh reuse: {:?}",
        &pixels[center..center + 4]
    );
    assert_eq!(
        renderer.stats().mesh_uploads,
        uploads + 1,
        "Sprite and rectangle share one quad; hidden geometry is skipped"
    );
    assert_eq!(renderer.stats().draw_calls, 2);
    assert_eq!(renderer.stats().visible_objects, 2);
    assert_eq!(renderer.stats().triangles, 4);
    assert!(renderer.take_errors().is_empty());
}
