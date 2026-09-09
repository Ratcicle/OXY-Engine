//! Additional CPU operations; same source can run against the immutable baseline.
#[allow(dead_code)]
#[path = "performance.rs"]
mod standard;
use oxy_core::{animation::*, document::*};
use oxy_render::{CameraState, collider_debug};
use std::{hint::black_box, time::Duration};
fn history_phases(project: &Project) -> serde_json::Value {
    let mut p = project.clone();
    let mut images = oxy_core::texture_cache::TextureCache::default();
    let mut history = oxy_core::edit_history::CommandHistory::new();
    let mut times: [Vec<u64>; 4] = Default::default();
    let started = std::time::Instant::now();
    for iteration in 0..104 {
        if started.elapsed() > Duration::from_secs(12) {
            break;
        }
        let mut one = [0; 4];
        let start = std::time::Instant::now();
        history.begin("Mover", &p, &images);
        one[0] = start.elapsed().as_nanos() as u64;
        p.scenes[0].entities[0].transform.position[0] += 1.;
        let start = std::time::Instant::now();
        history.commit(&p, &mut images).unwrap();
        one[1] = start.elapsed().as_nanos() as u64;
        let start = std::time::Instant::now();
        history.undo(&mut p, &mut images).unwrap();
        one[2] = start.elapsed().as_nanos() as u64;
        let start = std::time::Instant::now();
        history.redo(&mut p, &mut images).unwrap();
        one[3] = start.elapsed().as_nanos() as u64;
        black_box(&p);
        if iteration >= 3 {
            for (samples, time) in times.iter_mut().zip(one) {
                samples.push(time);
            }
        }
    }
    let mut result = serde_json::json!({"retained_estimated_bytes":history.estimated_bytes(),"limit_seconds":12});
    for (name, mut values) in ["begin", "commit", "undo", "redo"].into_iter().zip(times) {
        values.sort_unstable();
        let n = values.len();
        result[name] = serde_json::json!({"samples":n,"median_ns":values.get(n/2),"p95_ns":if n>=100 {values.get(((n-1) as f32*0.95).ceil() as usize)}else{None},"p99_ns":if n>=100 {values.get(((n-1) as f32*0.99).ceil() as usize)}else{None},"limit_exceeded":n<101});
    }
    result
}
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
        let history = history_phases(&project);
        rows.push(serde_json::json!({"entities":n,"pick":pick,"overlay":overlay,"preview_clone_sample_prepare":preview,"reparent_roundtrip":reparent,"history_phases":history}));
    }
    let mut cycles = Vec::new();
    for n in [100, 400] {
        let p = cycle_project(n);
        let mut runtime = oxy_core::runtime::Runtime::new(&p, &p.start_scene).unwrap();
        let input = oxy_core::runtime::InputFrame {
            released: Default::default(),
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
    let mut soak_project = cycle_project(100);
    {
        use oxy_core::graph::{Edge, Node};
        let target = &mut soak_project.scenes[0].entities[3];
        target.collider = Some(Collider::default());
        target.transform.position = [0.; 3];
        target.attributes.insert("Vida".into(), Value::Number(1e9));
        let target_id = target.id.clone();
        let area = &mut soak_project.scenes[0].entities[2];
        area.collider = Some(Collider {
            is_trigger: true,
            ..Default::default()
        });
        area.transform.position = [0.; 3];
        let mut nodes: Vec<_> = [
            "event.area_enter",
            "action.damage",
            "control.wait",
            "action.damage",
        ]
        .iter()
        .enumerate()
        .map(|(i, op)| {
            let mut node = Node::new(op, [0.; 2]);
            node.id = format!("00000000-0000-4000-8000-{:012x}", 30000 + i);
            node
        })
        .collect();
        for i in [1, 3] {
            nodes[i]
                .params
                .insert("target".into(), Value::Object(Some(target_id.clone())));
        }
        nodes[2].params.insert("seconds".into(), Value::Number(0.1));
        for i in 0..3 {
            area.graph.edges.push(Edge {
                from_node: nodes[i].id.clone(),
                from_port: "exec".into(),
                to_node: nodes[i + 1].id.clone(),
                to_port: "exec".into(),
            });
        }
        area.graph.nodes = nodes;
    }
    {
        use oxy_core::graph::{Edge, Node};
        let area = soak_project.scenes[0].entities[2].id.clone();
        let graph = &mut soak_project.scenes[0].entities[0].graph;
        for (i, action, enabled) in [(0, "ativar_area", true), (1, "desativar_area", false)] {
            let mut event = Node::new("event.input", [0.; 2]);
            event.id = format!("00000000-0000-4000-8000-{:012x}", 40000 + i * 2);
            event
                .params
                .insert("action".into(), Value::Text(action.into()));
            let mut node = Node::new("action.component", [0.; 2]);
            node.id = format!("00000000-0000-4000-8000-{:012x}", 40001 + i * 2);
            node.params
                .insert("target".into(), Value::Object(Some(area.clone())));
            node.params
                .insert("component".into(), Value::Text("collider".into()));
            node.params.insert("enabled".into(), Value::Bool(enabled));
            graph.edges.push(Edge {
                from_node: event.id.clone(),
                from_port: "exec".into(),
                to_node: node.id.clone(),
                to_port: "exec".into(),
            });
            graph.nodes.extend([event, node]);
        }
    }
    let mut rt = oxy_core::runtime::Runtime::new(&soak_project, &soak_project.start_scene).unwrap();
    let mut soak = Vec::new();
    let start = std::time::Instant::now();
    for step in 0..2000 {
        let mut pressed = std::collections::BTreeSet::from(["atacar".into()]);
        if step % 64 == 0 {
            pressed.insert("desativar_area".into());
        }
        if step % 64 == 1 {
            pressed.insert("ativar_area".into());
        }
        rt.advance(
            oxy_core::runtime::FIXED_DT,
            &oxy_core::runtime::InputFrame {
                released: Default::default(),
                pressed,
                ..Default::default()
            },
        );
        if step % 200 == 0 {
            soak.push(serde_json::json!({"step":step,"objects":rt.scene().entities.len(),"retained":rt.retained_counts()}));
        }
        assert!(
            start.elapsed() < Duration::from_secs(30),
            "Soak exceeded 30 seconds"
        );
    }
    rt.advance(
        oxy_core::runtime::FIXED_DT,
        &oxy_core::runtime::InputFrame {
            released: Default::default(),
            pressed: ["desativar_area".into()].into(),
            ..Default::default()
        },
    );
    for _ in 0..10 {
        rt.advance(oxy_core::runtime::FIXED_DT, &Default::default());
    }
    assert_eq!(rt.scene().entities.len(), 100);
    assert!(rt.logs.is_empty(), "{:?}", rt.logs);
    assert_eq!(
        rt.scene().entities[0].attributes["Contador"],
        Value::Number(4000.)
    );
    soak.push(serde_json::json!({"phase":"drained","retained":rt.retained_counts(),"objects":rt.scene().entities.len(),"owner_attributes":rt.scene().entities[0].attributes,"target_attributes":rt.scene().entities[3].attributes,"duration_ns":start.elapsed().as_nanos()}));
    rt.stop();
    soak.push(serde_json::json!({"phase":"stop","retained":rt.retained_counts()}));
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
                    released: Default::default(),
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
    println!("{}",serde_json::to_string_pretty(&serde_json::json!({"method":"CPU only; seed 0; 16-piece chains; all pieces collidable and animated; 3 warmups, up to 101 samples, 3s soft cap; picking and overlays at 1280x720 logical points; preview includes scene clone and actual sampling/preparation; no GPU timing","rows":rows,"cycles":cycles,"soak":soak,"equivalence":equivalence})).unwrap());
}
