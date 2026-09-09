//! Authored mesh loads, CPU interaction and history. No GPU/FPS inference.
#[allow(dead_code)]
#[path = "performance.rs"]
mod standard;
use glam::Vec3;
use oxy_core::{
    document::*,
    edit_history::CommandHistory,
    geometry::{
        self,
        primitives::{self, Parameters},
    },
    texture_cache::TextureCache,
};
use std::{
    hint::black_box,
    time::{Duration, Instant},
};
fn main() {
    let sizes: Vec<u32> = std::env::args()
        .nth(1)
        .unwrap_or("22,70,158".into())
        .split(',')
        .map(|s| s.parse().unwrap())
        .collect();
    let mut rows = Vec::new();
    for divisions in sizes {
        let params = Parameters {
            plane_divisions: [divisions; 2],
            ..Default::default()
        };
        let generate = || primitives::generate(Primitive::Plane, 8, params).unwrap();
        let mesh = generate();
        let mut p = Project::new("Malha medida");
        p.id = "00000000-0000-4000-8000-000000000001".into();
        p.scenes[0].id = "00000000-0000-4000-8000-000000000002".into();
        p.start_scene = p.scenes[0].id.clone();
        p.scenes[0].kind = SceneKind::ThreeD;
        let mut e = Entity::new("Plano editável", None);
        e.id = "00000000-0000-4000-8000-000000000003".into();
        e.mesh = Some(mesh.clone());
        p.scenes[0].entities.push(e);
        let json = serde_json::to_vec(&p).unwrap();
        let limit = Duration::from_secs(3);
        let camera = oxy_render::CameraState::for_scene(&p.scenes[0]);
        let picker = oxy_render::ScenePicker::new(&p.scenes[0]);
        let creation = standard::samples(|| black_box(generate()).data().faces.len(), limit);
        let loading = standard::samples(
            || {
                black_box(serde_json::from_slice::<Project>(&json).unwrap()).scenes[0]
                    .entities
                    .len()
            },
            limit,
        );
        let picking = standard::samples(
            || {
                (0..100)
                    .filter(|i| {
                        picker
                            .ray(
                                Vec3::new(
                                    (*i % 10) as f32 / 12. - 0.4,
                                    1.,
                                    (*i / 10) as f32 / 12. - 0.4,
                                ),
                                -Vec3::Y,
                            )
                            .is_some()
                    })
                    .count()
            },
            limit,
        );
        let prepare = standard::samples(
            || black_box(oxy_render::prepare_scene(&p.scenes[0], &camera)).len(),
            limit,
        );
        let encode = standard::samples(
            || {
                black_box(oxy_render::entity_mesh(&p.scenes[0].entities[0]))
                    .vertices
                    .len()
            },
            limit,
        );
        let build_buffers = standard::samples(
            || {
                black_box(oxy_render::mesh::from_editable(&mesh))
                    .vertices
                    .len()
            },
            limit,
        );
        let overlay = standard::samples(
            || {
                let vp = camera.matrix([920, 600]);
                black_box(
                    mesh.data()
                        .edges
                        .iter()
                        .map(|edge| {
                            edge.vertices
                                .map(|id| vp.project_point3(mesh.position(id).unwrap()))
                        })
                        .collect::<Vec<_>>(),
                )
                .len()
            },
            limit,
        );
        let candidate = mesh.with_shading(geometry::Shading::Smooth).unwrap();
        let operator = standard::samples(
            || {
                black_box(mesh.with_shading(geometry::Shading::Smooth).unwrap())
                    .data()
                    .faces
                    .len()
            },
            limit,
        );
        // Each cycle applies the same complete shading operation, then undo/redo. Reset between
        // samples outside timed phases; retain only this operation, never lower the mesh load.
        let mut times: [Vec<u64>; 4] = Default::default();
        let started = Instant::now();
        let mut retained = 0;
        for iteration in 0..104 {
            if started.elapsed() > Duration::from_secs(12) {
                break;
            }
            let mut doc = p.clone();
            let mut images = TextureCache::default();
            let mut history = CommandHistory::new();
            let mut ns = [0; 4];
            let t = Instant::now();
            history.begin("Sombreamento", &doc, &images);
            ns[0] = t.elapsed().as_nanos() as u64;
            doc.scenes[0].entities[0].mesh = Some(candidate.clone());
            let t = Instant::now();
            history.commit(&doc, &mut images).unwrap();
            ns[1] = t.elapsed().as_nanos() as u64;
            let t = Instant::now();
            history.undo(&mut doc, &mut images).unwrap();
            ns[2] = t.elapsed().as_nanos() as u64;
            let t = Instant::now();
            history.redo(&mut doc, &mut images).unwrap();
            ns[3] = t.elapsed().as_nanos() as u64;
            retained = history.estimated_bytes();
            black_box(&doc);
            if iteration >= 3 {
                for (samples, ns) in times.iter_mut().zip(ns) {
                    samples.push(ns);
                }
            }
        }
        let mut history =
            serde_json::json!({"retained_estimated_bytes":retained,"limit_seconds":12});
        for (name, mut values) in ["begin", "commit", "undo", "redo"].into_iter().zip(times) {
            values.sort_unstable();
            let n = values.len();
            history[name] = serde_json::json!({"samples":n,"median_ns":values.get(n/2),"p95_ns":if n>=100{values.get(((n-1) as f64*0.95).ceil() as usize)}else{None},"p99_ns":if n>=100{values.get(((n-1) as f64*0.99).ceil() as usize)}else{None},"limit_exceeded":n<101});
        }
        rows.push(serde_json::json!({"divisions":divisions,"vertices":mesh.data().vertices.len(),"triangles":mesh.prepared().triangles.len(),"geometry_estimated_bytes":mesh.estimated_bytes(),"serialized_bytes":json.len(),"generate_validate":creation,"deserialize_validate":loading,"pick_100_rays":picking,"prepare_one_entity":prepare,"cpu_cached_mesh_copy":encode,"cpu_build_render_vertices":build_buffers,"project_all_edges":overlay,"shading_operator":operator,"history":history}));
    }
    println!("{}",serde_json::to_string_pretty(&serde_json::json!({"method":"release locked, opt-level=2, deterministic regular plane, 3 warmups, 101 samples or 3 seconds per phase, history 12 seconds; CPU only; estimated bytes exclude allocator/GPU; no random IDs in serialized fixture", "rows":rows})).unwrap());
}
