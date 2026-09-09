use super::*;
use oxy_core::geometry::{
    primitives,
    selection::{Mode, Selection},
};

fn adapter() -> RenderState {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let adapter =
        block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).unwrap();
    println!("Native component overlay adapter: {:?}", adapter.get_info());
    let (device, queue) =
        block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
    let renderer =
        egui_wgpu::Renderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, Default::default());
    RenderState {
        adapter,
        available_adapters: vec![],
        device,
        queue,
        target_format: wgpu::TextureFormat::Rgba8Unorm,
        renderer: Arc::new(egui::epaint::mutex::RwLock::new(renderer)),
    }
}
fn save(pixels: &[u8], name: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../qa/v0.2.1/m5")
        .join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    image::save_buffer(path, pixels, 512, 384, image::ColorType::Rgba8).unwrap();
}
fn maximum_delta(base: &[u8], painted: &[u8]) -> u8 {
    (170..210)
        .flat_map(|y| (235..275).map(move |x| ((y * 512 + x) * 4) as usize))
        .map(|i| painted[i].saturating_sub(base[i]))
        .max()
        .unwrap()
}
#[test]
#[ignore = "Native WGPU component overlay proof, including depth, hidden highlights and cache uploads"]
fn native_gpu_component_overlay_depth_and_cache() {
    let rs = adapter();
    let mut renderer = Renderer::new(&rs);
    renderer.show_grid = false;
    let mut project = Project::new("Componentes");
    project.scenes[0].kind = SceneKind::ThreeD;
    let mesh = primitives::generate(Primitive::Cube, 8, Default::default()).unwrap();
    let front = mesh
        .data()
        .faces
        .iter()
        .find(|f| {
            f.corners
                .iter()
                .all(|c| mesh.position(c.vertex).unwrap().z > 0.49)
        })
        .unwrap()
        .id;
    let back = mesh
        .data()
        .faces
        .iter()
        .find(|f| {
            f.corners
                .iter()
                .all(|c| mesh.position(c.vertex).unwrap().z < -0.49)
        })
        .unwrap()
        .id;
    let mut entity = Entity::new("Cubo", None);
    entity.mesh = Some(mesh.clone());
    entity.material.color = [0.18, 0.2, 0.22, 1.];
    let id = entity.id.clone();
    project.scenes[0].entities = vec![entity];
    let mut camera = CameraState::for_scene(&project.scenes[0]);
    camera.target = Vec3::ZERO;
    camera.yaw = 0.;
    camera.pitch = 0.;
    camera.distance = 3.;
    let render = |renderer: &mut Renderer, p: &Project, c: &CameraState| {
        renderer.render(
            &rs,
            p,
            &p.scenes[0],
            Path::new(""),
            c,
            [512, 384],
            Some(id.clone()),
            false,
        );
    };
    render(&mut renderer, &project, &camera);
    let base = read_pixels(&renderer, &rs);
    let mut selection = Selection {
        mode: Mode::Face,
        ids: vec![front],
        through: false,
    };
    renderer.draw_mesh_components(&rs, &mesh, Mat4::IDENTITY, &camera, 1., &selection, false);
    let visible = read_pixels(&renderer, &rs);
    save(&visible, "components-visible-3d.png");
    render(&mut renderer, &project, &camera);
    selection.ids = vec![back];
    renderer.draw_mesh_components(&rs, &mesh, Mat4::IDENTITY, &camera, 1., &selection, false);
    let hidden = read_pixels(&renderer, &rs);
    save(&hidden, "components-hidden-3d.png");
    assert!(
        maximum_delta(&base, &visible) > maximum_delta(&base, &hidden) + 20,
        "Hidden selection must remain visibly attenuated"
    );
    assert!(
        maximum_delta(&base, &hidden) > 5,
        "Hidden selected surfaces must not disappear"
    );
    let before = renderer.stats();
    for step in 0..4 {
        camera.orbit([step as f32, 0.5]);
        render(&mut renderer, &project, &camera);
        renderer.draw_mesh_components(&rs, &mesh, Mat4::IDENTITY, &camera, 1., &selection, false);
    }
    let after = renderer.stats();
    assert_eq!(before.mesh_uploads, after.mesh_uploads);
    assert_eq!(before.component_mesh_uploads, after.component_mesh_uploads);
    assert_eq!(
        before.component_selection_uploads,
        after.component_selection_uploads
    );
    selection.mode = Mode::Vertex;
    selection.ids = mesh.data().vertices.iter().map(|v| v.id).collect();
    render(&mut renderer, &project, &camera);
    renderer.draw_mesh_components(&rs, &mesh, Mat4::IDENTITY, &camera, 1.6, &selection, true);
    save(
        &read_pixels(&renderer, &rs),
        "components-vertices-through-3d.png",
    );
    assert_eq!(
        before.component_mesh_uploads,
        renderer.stats().component_mesh_uploads
    );
    let mut occluder = Entity::new("Oclusor", Some(Primitive::Cube));
    occluder.transform.position = [0., 0., 1.];
    occluder.dimensions = [0.65, 0.65, 0.1];
    occluder.material.color = [0.05, 0.5, 0.1, 1.];
    project.scenes[0].entities.push(occluder);
    camera.yaw = 0.;
    camera.pitch = 0.;
    selection.mode = Mode::Face;
    selection.ids = vec![front];
    render(&mut renderer, &project, &camera);
    let covered_base = read_pixels(&renderer, &rs);
    renderer.draw_mesh_components(&rs, &mesh, Mat4::IDENTITY, &camera, 1., &selection, false);
    let covered = read_pixels(&renderer, &rs);
    save(&covered, "components-current-occluder-3d.png");
    assert!(
        maximum_delta(&covered_base, &covered) < maximum_delta(&base, &visible),
        "New current-frame occluder must hide the selected cap"
    );
    assert_eq!(
        before.component_mesh_uploads,
        renderer.stats().component_mesh_uploads
    );

    // In 2D depth comes from a private layer-ordered replay, never from geometric Z.
    project.scenes[0].kind = SceneKind::TwoD;
    let rectangle = primitives::generate(Primitive::Rectangle, 8, Default::default()).unwrap();
    project.scenes[0].entities[0].mesh = Some(rectangle.clone());
    project.scenes[0].entities[0].layer = 0;
    project.scenes[0].entities[1].primitive = Some(Primitive::Rectangle);
    project.scenes[0].entities[1].transform.position = [0., 0., -2.];
    project.scenes[0].entities[1].layer = 1;
    camera = CameraState::for_scene(&project.scenes[0]);
    camera.orthographic_size = 1.;
    selection.ids = vec![rectangle.data().faces[0].id];
    render(&mut renderer, &project, &camera);
    let before_overlay = read_pixels(&renderer, &rs);
    renderer.draw_mesh_components(
        &rs,
        &rectangle,
        Mat4::IDENTITY,
        &camera,
        1.,
        &selection,
        false,
    );
    let layer_hidden = read_pixels(&renderer, &rs);
    save(&layer_hidden, "components-layer-2d.png");
    project.scenes[0].entities[1].layer = -1;
    render(&mut renderer, &project, &camera);
    let before_visible = read_pixels(&renderer, &rs);
    renderer.draw_mesh_components(
        &rs,
        &rectangle,
        Mat4::IDENTITY,
        &camera,
        1.,
        &selection,
        false,
    );
    let layer_visible = read_pixels(&renderer, &rs);
    assert!(
        maximum_delta(&before_visible, &layer_visible)
            > maximum_delta(&before_overlay, &layer_hidden) + 20
    );
    // Flattening 2D depth for layer order must not resurrect geometry outside the camera.
    project.scenes[0].entities[0].transform.position[2] = 10_000.;
    render(&mut renderer, &project, &camera);
    let clipped_base = read_pixels(&renderer, &rs);
    renderer.draw_mesh_components(
        &rs,
        &rectangle,
        Mat4::from_translation(Vec3::Z * 10_000.),
        &camera,
        1.,
        &selection,
        false,
    );
    assert_eq!(clipped_base, read_pixels(&renderer, &rs));
    println!("Component overlay counters: {:?}", renderer.stats());
}
