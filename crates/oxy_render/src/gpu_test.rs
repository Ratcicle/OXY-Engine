//! Opt-in tests really submit triangles and read GPU pixels back; no screenshots are faked.
use super::*;
mod components;
use std::{sync::Arc, time::Duration};

use crate::offscreen::block_on;

#[test]
#[ignore = "Measures CPU buffer creation and native submission for authored meshes; requires WGPU"]
fn native_mesh_upload_measurements() {
    use std::{hint::black_box, time::Instant};
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let adapter =
        block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).unwrap();
    let info = format!("{:?}", adapter.get_info());
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
    let summary = |mut times: Vec<u64>| {
        times.sort_unstable();
        let n = times.len();
        serde_json::json!({"samples":n,"median_ns":times.get(n/2),"p95_ns":if n>=100{times.get(((n-1) as f64*0.95).ceil() as usize)}else{None},"p99_ns":if n>=100{times.get(((n-1) as f64*0.99).ceil() as usize)}else{None}})
    };
    let mut rows = Vec::new();
    for divisions in [22, 70, 158] {
        let mut p = Project::new("Carga GPU");
        p.scenes[0].kind = SceneKind::ThreeD;
        let mesh = oxy_core::geometry::primitives::generate(
            Primitive::Plane,
            8,
            oxy_core::geometry::primitives::Parameters {
                plane_divisions: [divisions; 2],
                ..Default::default()
            },
        )
        .unwrap();
        let triangles = mesh.prepared().triangles.len();
        let mut e = Entity::new("Malha", None);
        e.mesh = Some(mesh);
        p.scenes[0].entities.push(e);
        let mut renderer = Renderer::new(&rs);
        renderer.show_grid = false;
        let camera = CameraState::for_scene(&p.scenes[0]);
        let mut cold = Vec::new();
        let mut warm = Vec::new();
        let mut bytes = 0;
        let start = Instant::now();
        for sample in 0..104 {
            if start.elapsed() > Duration::from_secs(10) {
                break;
            }
            // The CPU mesh encoding is warm, GPU vertex/index buffers are deliberately fresh.
            let mut cache = GeometryCache::default();
            let t = Instant::now();
            let (gpu, hit) = cache.get(&rs.device, &p.scenes[0].entities[0]);
            let upload = t.elapsed().as_nanos() as u64;
            assert!(!hit);
            bytes = gpu.vertices.size() + gpu.indices.size();
            black_box(gpu);
            let before = renderer.stats().mesh_uploads;
            let t = Instant::now();
            renderer.render(
                &rs,
                &p,
                &p.scenes[0],
                Path::new(""),
                &camera,
                [920, 600],
                None,
                false,
            );
            let submission = t.elapsed().as_nanos() as u64;
            // Drain outside measurement. This is CPU API time, not GPU elapsed time or VSync.
            rs.device
                .poll(wgpu::PollType::Wait {
                    submission_index: None,
                    timeout: Some(Duration::from_secs(10)),
                })
                .unwrap();
            if sample >= 3 {
                assert_eq!(before, renderer.stats().mesh_uploads);
                cold.push(upload);
                warm.push(submission);
            }
        }
        assert!(renderer.take_errors().is_empty());
        let pixels = read_pixels(&renderer, &rs);
        assert!(pixels.chunks_exact(4).any(|p| p[0] > 30));
        rows.push(serde_json::json!({"triangles":triangles,"buffer_size_bytes":bytes,"cpu_create_vertex_index_buffers":summary(cold),"cpu_cached_render_submission":summary(warm),"warm_mesh_uploads":0,"phase_limit_seconds":10}));
    }
    let output = oxy_core::qa::output_dir("v0.2.1/m6");
    std::fs::create_dir_all(&output).unwrap();
    std::fs::write(output.join("mesh-upload.json"),serde_json::to_vec_pretty(&serde_json::json!({"gpu":info,"physical_offscreen_resolution":[920,600],"method":"Release locked; 3 warmups then 101 samples. CPU Instant around WGPU creation/submission; device drain outside measured interval; no VSync and no GPU timestamps. Buffer sizes are actual descriptor bytes, not VRAM residency. CPU mesh cache warm, GPU buffers cold per sample.","rows":rows})).unwrap()).unwrap();
}

fn read_pixels(renderer: &Renderer, rs: &RenderState) -> Vec<u8> {
    renderer.read_pixels(rs).unwrap().0
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
    let path = oxy_core::qa::output_dir("v0.2.1/m3/native-depth-proof.png");
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

#[test]
#[ignore = "Native GPU comparison of primitive conversion, editable mesh cache and loose topology"]
fn native_gpu_editable_mesh_equivalence() {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let adapter =
        block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).unwrap();
    println!("GPU: {:?}", adapter.get_info());
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
    let mut project = Project::new("Conversão GPU");
    project.scenes[0].kind = SceneKind::ThreeD;
    let pixels: Vec<u8> = (0..64 * 64)
        .flat_map(|i| [(i % 64 * 4) as u8, (i / 64 * 4) as u8, 160, 255])
        .collect();
    renderer
        .set_texture_pixels(&rs, "conversion-texture", 64, 64, &pixels, true)
        .unwrap();
    let mut camera = CameraState::for_scene(&project.scenes[0]);
    camera.target = Vec3::ZERO;
    camera.distance = 5.;
    for primitive in [
        Primitive::Cube,
        Primitive::Sphere,
        Primitive::Cylinder,
        Primitive::Plane,
    ] {
        let mut entity = Entity::new("Forma", Some(primitive));
        entity.dimensions = [1.3, 2.1, 0.7];
        entity.transform.pivot = [0.1, 0.2, 0.];
        entity.transform.rotation = [0.2, 0.4, 0.1];
        entity.material.color = [1.; 4];
        entity.material.texture = Some("conversion-texture".into());
        project.scenes[0].entities = vec![entity];
        renderer.render(
            &rs,
            &project,
            &project.scenes[0],
            Path::new(""),
            &camera,
            [640, 480],
            None,
            false,
        );
        let before = read_pixels(&renderer, &rs);
        oxy_core::geometry::primitives::convert(&mut project.scenes[0].entities[0]).unwrap();
        renderer.render(
            &rs,
            &project,
            &project.scenes[0],
            Path::new(""),
            &camera,
            [640, 480],
            None,
            false,
        );
        let after = read_pixels(&renderer, &rs);
        let different = before
            .chunks_exact(4)
            .zip(after.chunks_exact(4))
            .filter(|(a, b)| a.iter().zip(*b).any(|(a, b)| a.abs_diff(*b) > 2))
            .count();
        assert!(
            different < 50,
            "{primitive:?}: {different} pixels mudaram além da tolerância de rasterização (2/255; no máximo 50 pixels de borda em 307.200)."
        );
        let stats = renderer.stats();
        camera.yaw += 0.05;
        renderer.render(
            &rs,
            &project,
            &project.scenes[0],
            Path::new(""),
            &camera,
            [640, 480],
            None,
            false,
        );
        assert_eq!(stats.mesh_uploads, renderer.stats().mesh_uploads);
        let entity = &mut project.scenes[0].entities[0];
        let mut data = entity.mesh.as_ref().unwrap().data().clone();
        for v in &mut data.vertices {
            v.position[0] += 0.15;
        }
        entity.mesh = Some(oxy_core::geometry::EditableMesh::new(data).unwrap());
        renderer.render(
            &rs,
            &project,
            &project.scenes[0],
            Path::new(""),
            &camera,
            [640, 480],
            None,
            false,
        );
        assert_eq!(stats.mesh_uploads + 1, renderer.stats().mesh_uploads);
    }
    let mut data = oxy_core::geometry::MeshData::default();
    data.add_vertex(Vec3::ZERO).unwrap();
    let entity = &mut project.scenes[0].entities[0];
    entity.mesh = Some(oxy_core::geometry::EditableMesh::new(data).unwrap());
    renderer.render(
        &rs,
        &project,
        &project.scenes[0],
        Path::new(""),
        &camera,
        [640, 480],
        None,
        false,
    );
    assert_eq!(renderer.stats().triangles, 0);
    assert!(renderer.take_errors().is_empty());
}

#[test]
#[ignore = "Requires a native WGPU adapter; run explicitly for graphics validation"]
fn native_gpu_capture_collider_view_and_free_cameras() {
    use crate::offscreen::{ShotView, framed_bounds, headless_state, view_png};
    use oxy_core::{
        document::{UiAnchor, UiElement, UiKind, Value},
        physics3d::{Collider3d, CollisionShape},
    };
    let rs = headless_state().expect("Native WGPU adapter");
    let mut renderer = Renderer::new(&rs);
    let mut project = Project::new("Captura");
    let scene = &mut project.scenes[0];
    scene.kind = SceneKind::ThreeD;
    let mut floor = Entity::new("Chão", Some(Primitive::Cube));
    floor.dimensions = [10., 1., 10.];
    floor.transform.position = [0., -0.5, 0.];
    floor.material.color = [0.2, 0.2, 0.8, 1.];
    floor.physics3d = Some(Collider3d {
        shape: CollisionShape::Box {
            size: floor.dimensions,
        },
        ..Default::default()
    });
    // An invisible checkpoint: only the collider view shows it.
    let mut checkpoint = Entity::new("Ponto de controle", None);
    checkpoint.transform.position = [2., 1., 2.];
    checkpoint.physics3d = Some(Collider3d {
        shape: CollisionShape::Box { size: [2.; 3] },
        sensor: true,
        ..Default::default()
    });
    // The game interface: a white text and a bar three quarters full of white.
    let mut score = Entity::new("Placar", None);
    score.ui = Some(UiElement {
        text: "Pontos".into(),
        position: [16., 16.],
        ..Default::default()
    });
    let mut health = Entity::new("Vida", None);
    health.attributes.insert("vida".into(), Value::Number(75.));
    health.ui = Some(UiElement {
        kind: UiKind::Bar,
        anchor: UiAnchor::BottomRight,
        position: [16., 16.],
        size: [200., 28.],
        text: String::new(),
        color: [0.95, 0.95, 0.95, 1.],
        binding_attribute: "vida".into(),
        ..Default::default()
    });
    scene.entities = vec![floor, checkpoint, score, health];
    let mut runtime = oxy_core::runtime::Runtime::new(&project, &project.start_scene).unwrap();
    // What the editor shows must survive a capture.
    renderer.render(
        &rs,
        &project,
        &project.scenes[0],
        Path::new(""),
        &CameraState::for_scene(&project.scenes[0]),
        [64, 48],
        None,
        false,
    );
    let shown = read_pixels(&renderer, &rs);
    let bounds = framed_bounds(runtime.scene(), &[]).unwrap();
    let interface = oxy_core::game_ui::ui_state(runtime.project(), runtime.scene(), [320., 240.]);
    let decode = |png: &[u8]| image::load_from_memory(png).unwrap().to_rgba8();
    let mut shot = |view: &ShotView, interface: bool, colliders: bool| {
        let (png, camera) = view_png(
            &rs,
            &mut renderer,
            &mut runtime,
            Path::new(""),
            [320, 240],
            view,
            interface,
            colliders,
        )
        .unwrap();
        (decode(&png), camera)
    };
    let (plain, camera) = shot(&ShotView::Top(bounds), false, false);
    assert_eq!(camera.kind, SceneKind::TwoD, "orthographic top view");
    let (with, _) = shot(&ShotView::Top(bounds), false, true);
    assert_eq!(plain.dimensions(), (320, 240));
    let count = |image: &image::RgbaImage, test: fn(&[u8]) -> bool| {
        image.pixels().filter(|p| test(&p.0)).count()
    };
    let yellow: fn(&[u8]) -> bool = |p| p[0] > 200 && p[1] > 200 && p[2] < 90;
    let green: fn(&[u8]) -> bool = |p| p[1] > 200 && p[0] < 180 && p[2] < 180;
    let blue: fn(&[u8]) -> bool = |p| p[2] > p[0] + 40 && p[2] > p[1] + 40;
    assert_eq!(count(&plain, yellow), 0);
    assert_eq!(count(&plain, green), 0);
    assert!(count(&with, yellow) > 20, "sensor outline");
    assert!(count(&with, green) > 20, "solid collider outline");
    assert!(
        blue(&plain.get_pixel(160, 120).0),
        "the floor fills the top view"
    );
    let (free, camera) = shot(
        &ShotView::Free {
            eye: Vec3::new(8., 6., 8.),
            target: Vec3::ZERO,
            fov: 60.,
        },
        false,
        false,
    );
    assert_eq!(camera.kind, SceneKind::ThreeD);
    assert!(blue(&free.get_pixel(160, 120).0), "looking at the floor");
    assert!(!blue(&free.get_pixel(160, 2).0), "background above it");

    // The game camera with the interface: drawn as the player draws it, inside the rects the
    // simulation reports for the same size, and only when asked.
    let (drawn, _) = shot(&ShotView::Game, true, false);
    let (bare, _) = shot(&ShotView::Game, false, false);
    let bright: fn(&[u8]) -> bool = |p| p[0] > 200 && p[1] > 200 && p[2] > 200;
    let inside = |rect: [f64; 4], x: u32, y: u32| {
        let (x, y) = (f64::from(x) + 0.5, f64::from(y) + 0.5);
        x > rect[0] && x < rect[0] + rect[2] && y > rect[1] && y < rect[1] + rect[3]
    };
    let lit = |image: &image::RgbaImage, rect: [f64; 4]| {
        image
            .enumerate_pixels()
            .filter(|(x, y, p)| inside(rect, *x, *y) && bright(&p.0))
            .count()
    };
    assert_eq!(interface.len(), 2);
    for element in &interface {
        assert!(
            lit(&drawn, element.rect) > 40,
            "{} drawn at {:?}",
            element.name,
            element.rect
        );
        assert_eq!(lit(&bare, element.rect), 0, "{} left out", element.name);
    }
    assert_eq!(count(&bare, bright), 0);
    let stray = drawn
        .enumerate_pixels()
        .filter(|(x, y, p)| bright(&p.0) && !interface.iter().any(|e| inside(e.rect, *x, *y)))
        .count();
    assert_eq!(stray, 0, "nothing bright outside the interface rects");
    // Past three quarters, the bar shows its dark track.
    let [x, y, width, height] = interface[1].rect;
    assert_eq!(interface[1].fraction, Some(0.75));
    assert_eq!(
        lit(
            &drawn,
            [x + width * 0.8, y + 3., width * 0.2 - 3., height - 6.]
        ),
        0
    );
    assert!(lit(&drawn, [x + 3., y + 3., width * 0.2, height - 6.]) > 40);
    let output = oxy_core::qa::output_dir("v0.6/captures");
    std::fs::create_dir_all(&output).unwrap();
    for (name, image) in [
        ("game-interface.png", &drawn),
        ("game-without-interface.png", &bare),
        ("top-colliders.png", &with),
    ] {
        image.save(output.join(name)).unwrap();
    }
    println!("Capture evidence: {}", output.display());
    assert_eq!(renderer.target_size(), [64, 48]);
    assert_eq!(read_pixels(&renderer, &rs), shown);
    assert!(renderer.take_errors().is_empty());
}

/// Captures `project`'s first scene through `camera`, with nothing painted over it.
fn background_shot(
    rs: &RenderState,
    renderer: &mut Renderer,
    project: &Project,
    camera: &CameraState,
) -> image::RgbaImage {
    let (pixels, size) = renderer
        .capture(
            rs,
            project,
            &project.scenes[0],
            Path::new(""),
            camera,
            [320, 240],
            None,
            crate::offscreen::Overlays {
                interface: false,
                colliders: None,
                pixels_per_point: 1.,
            },
        )
        .unwrap();
    image::RgbaImage::from_raw(size[0], size[1], pixels).unwrap()
}

#[test]
#[ignore = "Requires a native WGPU adapter; run explicitly for graphics validation"]
fn native_gpu_scene_backgrounds() {
    use crate::offscreen::{ShotView, framed_bounds, headless_state, view_png};
    use oxy_core::document::Background;
    let rs = headless_state().expect("Native WGPU adapter");
    let mut renderer = Renderer::new(&rs);
    renderer.show_grid = false;
    let mut project = Project::new("Fundos");
    let scene = &mut project.scenes[0];
    scene.kind = SceneKind::ThreeD;
    scene.background = Some(Background::Sky);
    let mut floor = Entity::new("Chão", Some(Primitive::Cube));
    floor.dimensions = [6., 1., 6.];
    floor.transform.position = [0., -0.5, 0.];
    floor.material.color = [0.15, 0.6, 0.2, 1.];
    scene.entities = vec![floor];
    // Level with the horizon, 8 m from the floor's middle, along -Z turned by `yaw`.
    let level = |scene: &Scene, yaw: f32| {
        let mut camera = CameraState::for_scene(scene);
        camera.target = Vec3::new(0., 1., 0.);
        camera.distance = 8.;
        camera.yaw = yaw;
        camera.pitch = 0.;
        camera
    };
    let sky: fn(&[u8]) -> bool = |p| p[2] > 200 && p[2] > p[0] + 25 && p[1] > 120;
    let close = |p: &[u8], color: [f32; 4]| {
        p.iter()
            .zip(color)
            .all(|(p, c)| p.abs_diff((c * 255.).round() as u8) <= 1)
    };
    let output = oxy_core::qa::output_dir("v0.6/backgrounds");
    std::fs::create_dir_all(&output).unwrap();

    // Looking along -Z: the sky above the horizon, the floor drawn over it.
    let forward = background_shot(&rs, &mut renderer, &project, &level(&project.scenes[0], 0.));
    forward.save(output.join("sky-forward.png")).unwrap();
    for x in [0, 160, 319] {
        for y in [0, 40, 100] {
            let p = forward.get_pixel(x, y).0;
            assert!(sky(&p), "sky at ({x}, {y}): {p:?}");
        }
    }
    let ground = forward.get_pixel(160, 150).0;
    assert!(
        ground[1] > ground[0] + 40,
        "the floor over the sky: {ground:?}"
    );
    assert_ne!(
        forward.get_pixel(160, 0).0,
        forward.get_pixel(160, 100).0,
        "the panorama, not one color"
    );
    assert_eq!(renderer.stats().draw_calls, 2, "the sky and the floor");
    // The same view again: the same image, nothing new to upload.
    let stats = renderer.stats();
    let again = background_shot(&rs, &mut renderer, &project, &level(&project.scenes[0], 0.));
    assert_eq!(again, forward);
    assert_eq!(renderer.stats().uniform_uploads, stats.uniform_uploads);

    // Looking along +Z, where the panorama wraps around: no seam down the middle.
    let back = background_shot(
        &rs,
        &mut renderer,
        &project,
        &level(&project.scenes[0], std::f32::consts::PI),
    );
    back.save(output.join("sky-wrap.png")).unwrap();
    for y in 0..100 {
        let [a, b] = [back.get_pixel(159, y).0, back.get_pixel(160, y).0];
        assert!(
            a.iter().zip(b).all(|(a, b)| a.abs_diff(b) <= 8),
            "seam at row {y}: {a:?} {b:?}"
        );
        assert!(sky(&a), "sky behind at row {y}: {a:?}");
    }

    // Looking up, wide: every pixel is sky, clouds and the sun included.
    let mut up = level(&project.scenes[0], 2.);
    up.target.y = 30.;
    up.pitch = -0.8;
    up.fov = 100.;
    let above = background_shot(&rs, &mut renderer, &project, &up);
    above.save(output.join("sky-up.png")).unwrap();
    assert!(
        above
            .pixels()
            .all(|p| p[2] > 180 && p[2] >= p[0] && p[2] >= p[1])
    );

    // A capture from above is orthographic: around the floor, the sky's horizon color.
    let mut runtime = oxy_core::runtime::Runtime::new(&project, &project.start_scene).unwrap();
    let mut bounds = framed_bounds(runtime.scene(), &[]).unwrap();
    bounds.min -= Vec3::splat(4.);
    bounds.max += Vec3::splat(4.);
    let (png, camera) = view_png(
        &rs,
        &mut renderer,
        &mut runtime,
        Path::new(""),
        [320, 240],
        &ShotView::Top(bounds),
        false,
        false,
    )
    .unwrap();
    assert_eq!(camera.kind, SceneKind::TwoD);
    let top = image::load_from_memory(&png).unwrap().to_rgba8();
    top.save(output.join("sky-top.png")).unwrap();
    assert!(close(&top.get_pixel(2, 2).0, crate::sky::HORIZON));
    let middle = top.get_pixel(160, 120).0;
    assert!(
        middle[1] > middle[0] + 40,
        "the floor in the middle: {middle:?}"
    );

    // A color clears to itself, in 3D and in 2D; without a background, the dark color.
    let orange = [0.9, 0.45, 0.1, 1.];
    project.scenes[0].background = Some(Background::Color(orange));
    let colored = background_shot(&rs, &mut renderer, &project, &level(&project.scenes[0], 0.));
    colored.save(output.join("color-3d.png")).unwrap();
    assert!(close(&colored.get_pixel(5, 5).0, orange));
    assert_eq!(renderer.stats().draw_calls, 1, "the floor alone");
    project.scenes[0].kind = SceneKind::TwoD;
    project.scenes[0].entities[0].primitive = Some(Primitive::Rectangle);
    let camera = CameraState::for_scene(&project.scenes[0]);
    let flat = background_shot(&rs, &mut renderer, &project, &camera);
    flat.save(output.join("color-2d.png")).unwrap();
    assert!(close(&flat.get_pixel(5, 5).0, orange));
    project.scenes[0].background = None;
    let dark = background_shot(&rs, &mut renderer, &project, &camera);
    assert!(close(&dark.get_pixel(5, 5).0, Background::DARK));
    println!("Background evidence: {}", output.display());
    assert!(renderer.take_errors().is_empty());
}
