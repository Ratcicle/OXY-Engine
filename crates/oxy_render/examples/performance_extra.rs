//! Additional CPU operations; same source can run against the immutable baseline.
#[allow(dead_code)]
#[path = "performance.rs"]
mod standard;
use oxy_core::{animation::*, document::*};
use oxy_render::{CameraState, collider_debug};
use std::{hint::black_box, time::Duration};
fn cycle_project(n: usize) -> Project {
    use oxy_core::graph::{Edge, Node};
    let mut p = standard::fixture(n, "static");
    let template = p.scenes[0].entities[1].id.clone();
    let owner = &mut p.scenes[0].entities[0];
    owner
        .attributes
        .insert("Contador".into(), Value::Number(0.));
    owner.attributes.insert("Vida".into(), Value::Number(1e9));
    let mut nodes: Vec<_> = [
        "event.input",
        "attribute.set",
        "attribute.set",
        "action.spawn",
        "action.damage",
        "control.wait",
        "action.damage",
        "action.remove",
        "attribute.get",
        "math.binary",
    ]
    .iter()
    .enumerate()
    .map(|(i, op)| {
        let mut node = Node::new(op, [0.; 2]);
        node.id = format!("00000000-0000-4000-8000-{:012x}", 20000 + i);
        node
    })
    .collect();
    for i in [1, 2, 8] {
        nodes[i]
            .params
            .insert("attribute".into(), Value::Text("Contador".into()));
    }
    nodes[3]
        .params
        .insert("target".into(), Value::Object(Some(template)));
    nodes[5]
        .params
        .insert("seconds".into(), Value::Number(0.025));
    let mut connect = |a: usize, output: &str, b: usize, input: &str| {
        owner.graph.edges.push(Edge {
            from_node: nodes[a].id.clone(),
            from_port: output.into(),
            to_node: nodes[b].id.clone(),
            to_port: input.into(),
        })
    };
    for i in 0..7 {
        connect(i, "exec", i + 1, "exec");
    }
    connect(8, "value", 9, "a");
    connect(9, "value", 1, "value");
    connect(9, "value", 2, "value");
    connect(3, "created", 7, "target");
    owner.graph.nodes = nodes;
    p
}
fn main() {
    let sizes: Vec<usize> = std::env::args()
        .nth(1)
        .unwrap_or("100,200,400,800,1600".into())
        .split(',')
        .map(|v| v.parse().unwrap())
        .collect();
    let mut rows = Vec::new();
    for n in sizes {
        let mut project = standard::fixture(n, "hierarchy");
        let scene = &mut project.scenes[0];
        for e in &mut scene.entities {
            e.collider = Some(Collider::default());
        }
        let camera = CameraState::for_scene(scene);
        let limit = Duration::from_secs(3);
        let pick = standard::samples(
            || {
                black_box(oxy_render::pick(scene, &camera, [1280, 720], [640., 360.])).is_some()
                    as usize
            },
            limit,
        );
        let ctx = egui::Context::default();
        let overlay = standard::samples(
            || {
                let output = ctx.run(egui::RawInput::default(), |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        black_box(collider_debug::draw(
                            ui,
                            scene,
                            &camera,
                            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280., 720.)),
                            true,
                            &[],
                            true,
                        ));
                    });
                });
                black_box(output.shapes).len()
            },
            limit,
        );
        let mut clip = Clip::new("Prévia");
        clip.id = "00000000-0000-4000-9000-000000000001".into();
        for e in &scene.entities {
            let mut end = e.transform.clone();
            end.rotation[2] += 0.5;
            clip.tracks.push(Track {
                target: e.id.clone(),
                keyframes: vec![
                    Keyframe {
                        time: 0.,
                        transform: e.transform.clone(),
                        interpolation: Interpolation::Linear,
                    },
                    Keyframe {
                        time: 1.,
                        transform: end,
                        interpolation: Interpolation::Linear,
                    },
                ],
            });
        }
        let preview = standard::samples(
            || {
                let mut preview = scene.clone();
                sample_clip(&mut preview, &clip, 0.37);
                black_box(oxy_render::prepare_scene(&preview, &camera)).len()
            },
            limit,
        );
        let mut structural = scene.clone();
        for e in &mut structural.entities {
            e.transform.scale = [1.; 3];
        }
        let scene = &mut structural;
        let id = scene.entities[15].id.clone();
        let old_parent = scene.entities[15].parent.clone();
        let reparent = standard::samples(
            || {
                scene.reparent(&id, None, true).unwrap();
                scene.reparent(&id, old_parent.clone(), true).unwrap();
                black_box(scene.world_matrix(&id)).unwrap().to_cols_array()[12].to_bits() as usize
            },
            limit,
        );
        rows.push(serde_json::json!({"entities":n,"pick":pick,"overlay":overlay,"preview_clone_sample_prepare":preview,"reparent_roundtrip":reparent}));
    }
    let mut cycles = Vec::new();
    for n in [100, 400] {
        let p = cycle_project(n);
        let mut runtime = oxy_core::runtime::Runtime::new(&p, &p.start_scene).unwrap();
        let input = oxy_core::runtime::InputFrame {
            pressed: ["atacar".into()].into(),
            ..Default::default()
        };
        let timing = standard::samples(
            || {
                runtime.advance(oxy_core::runtime::FIXED_DT, &input);
                black_box(runtime.scene()).entities.len()
            },
            Duration::from_secs(3),
        );
        for _ in 0..8 {
            runtime.advance(oxy_core::runtime::FIXED_DT, &Default::default());
        }
        assert_eq!(runtime.scene().entities.len(), n);
        assert!(runtime.logs.is_empty(), "{:?}", runtime.logs);
        cycles.push(serde_json::json!({"entities":n,"nodes":10,"edges":11,"sampled_step":timing,"after_drain":runtime.retained_counts(),"attributes":runtime.scene().entities[0].attributes}));
    }
    let mut equivalence = Vec::new();
    for mode in [
        "move",
        "area_sparse",
        "area_dense",
        "graph_small",
        "graph_large",
    ] {
        let p = standard::fixture(200, mode);
        let mut runtime = oxy_core::runtime::Runtime::new(&p, &p.start_scene).unwrap();
        for _ in 0..60 {
            runtime.advance(
                oxy_core::runtime::FIXED_DT,
                &oxy_core::runtime::InputFrame {
                    held: ["mover_direita".into()].into(),
                    pressed: if mode.starts_with("graph") {
                        ["atacar".into()].into()
                    } else {
                        Default::default()
                    },
                },
            );
        }
        equivalence.push(serde_json::json!({"scenario":mode,"entities":runtime.scene().entities.iter().map(|e|(&e.id,&e.parent,&e.transform,&e.attributes)).collect::<Vec<_>>(),"logs":runtime.logs,"trace":runtime.traces.iter().map(|t|(&t.object,&t.node,&t.operation,t.time)).collect::<Vec<_>>()}));
    }
    println!("{}",serde_json::to_string_pretty(&serde_json::json!({"method":"CPU only; seed 0; 16-piece chains; all pieces collidable and animated; 3 warmups, up to 101 samples, 3s soft cap; picking and overlays at 1280x720 logical points; preview includes scene clone and actual sampling/preparation; no GPU timing","rows":rows,"cycles":cycles,"equivalence":equivalence})).unwrap());
}
