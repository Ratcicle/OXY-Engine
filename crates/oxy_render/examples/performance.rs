//! CPU benchmark; never interprets these durations as GPU time or game FPS.
use oxy_core::{
    document::*,
    edit_history::CommandHistory,
    graph::{Edge, Node},
    metrics,
    runtime::{FIXED_DT, InputFrame, Runtime},
    scene_view::SceneView,
    texture_cache::TextureCache,
};
use std::{
    hint::black_box,
    time::{Duration, Instant},
};
fn id(n: usize) -> String {
    format!("00000000-0000-4000-8000-{n:012x}")
}
pub(crate) fn fixture(n: usize, mode: &str) -> Project {
    let mut p = Project::new("Benchmark OXY");
    p.id = id(1);
    p.scenes[0].id = id(2);
    p.start_scene = id(2);
    for i in 0..n {
        let mut e = Entity::new(format!("Peça {i}"), Some(Primitive::Rectangle));
        e.id = id(100 + i);
        e.transform.position = [(i % 40) as f32 * 4., (i / 40) as f32 * 4., 0.];
        if mode == "hierarchy" && i % 16 != 0 {
            e.parent = Some(id(100 + i - 1));
            e.transform.position = [0.1, 0.2, 0.];
            e.transform.pivot = [0.05, -0.1, 0.];
            e.transform.scale = [if i % 3 == 0 { -1. } else { 1. }, 1.01, 1.];
            e.transform.rotation[2] = 0.1;
        }
        if mode.starts_with("move") || mode.starts_with("area") {
            if i < 10 {
                e.controller = Some(Controller {
                    gravity: 0.,
                    ..Default::default()
                });
            }
            let physical = if mode == "move_decor" { 40 } else { n / 2 };
            if i < physical {
                e.collider = Some(Collider::default());
            }
            if mode.starts_with("area") && (10..20).contains(&i) {
                e.collider = Some(Collider {
                    is_trigger: true,
                    size: [12., 12., 1.],
                    ..Default::default()
                });
            }
            if mode == "area_dense" {
                e.transform.position = [(i % 4) as f32 * 0.1, 0., 0.];
            }
        }
        if mode.starts_with("graph") && i == 0 {
            e.attributes.insert("Vida".into(), Value::Number(1e12));
            let count = if mode == "graph_large" { 64 } else { 8 };
            for j in 0..count {
                let op = if j == 0 {
                    "event.input"
                } else if j == count / 2 {
                    "control.wait"
                } else {
                    "action.damage"
                };
                let mut node = Node::new(op, [j as f32 * 10., 0.]);
                node.id = id(10000 + j);
                if op == "action.damage" {
                    node.params
                        .insert("attribute".into(), Value::Text("Vida".into()));
                }
                if op == "control.wait" {
                    node.params.insert("seconds".into(), Value::Number(0.025));
                }
                if j > 0 {
                    e.graph.edges.push(Edge {
                        from_node: id(10000 + j - 1),
                        from_port: "exec".into(),
                        to_node: node.id.clone(),
                        to_port: "exec".into(),
                    });
                }
                e.graph.nodes.push(node);
            }
        }
        p.scenes[0].entities.push(e);
    }
    p
}
pub(crate) fn samples(mut work: impl FnMut() -> usize, limit: Duration) -> serde_json::Value {
    for _ in 0..3 {
        black_box(work());
    }
    metrics::take();
    let start = Instant::now();
    let mut times = Vec::new();
    let mut checksum = 0usize;
    for _ in 0..101 {
        if start.elapsed() > limit {
            break;
        }
        let t = Instant::now();
        checksum = checksum.wrapping_add(black_box(work()));
        times.push(t.elapsed().as_nanos() as u64);
    }
    let counters = metrics::take();
    times.sort_unstable();
    let percentile = |q: f64| {
        times
            .get(((times.len().saturating_sub(1)) as f64 * q).ceil() as usize)
            .copied()
    };
    serde_json::json!({"samples":times.len(),"median_ns":percentile(0.5),"p95_ns":if times.len()>=100 {percentile(0.95)}else{None},"p99_ns":if times.len()>=100 {percentile(0.99)}else{None},"limit_exceeded":times.len()<101,"checksum":checksum,"counters":counters})
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let sizes: Vec<usize> = args
        .get(1)
        .map(|s| s.split(',').map(|v| v.parse().unwrap()).collect())
        .unwrap_or(vec![100, 200, 400, 800, 1600]);
    let mut rows = Vec::new();
    for n in sizes {
        for mode in [
            "static",
            "move",
            "move_decor",
            "area_sparse",
            "area_dense",
            "hierarchy",
            "graph_small",
            "graph_large",
        ] {
            eprintln!("{mode} / {n}");
            let p = fixture(n, mode);
            let s = &p.scenes[0];
            let json = serde_json::to_vec(&p).unwrap();
            let limit = Duration::from_secs(3);
            let mut row = serde_json::json!({"scenario":mode,"entities":n,"controllers":s.entities.iter().filter(|e|e.controller.is_some()).count(),"colliders":s.entities.iter().filter(|e|e.collider.is_some()).count(),"areas":s.entities.iter().filter(|e|e.collider.as_ref().is_some_and(|c|c.is_trigger)).count(),"nodes":s.entities.iter().map(|e|e.graph.nodes.len()).sum::<usize>(),"edges":s.entities.iter().map(|e|e.graph.edges.len()).sum::<usize>(),"json_bytes":json.len()});
            row["create"] = samples(
                || black_box(fixture(n, mode)).scenes[0].entities.len(),
                limit,
            );
            row["load_validate"] = samples(
                || {
                    let p: Project = serde_json::from_slice(&json).unwrap();
                    validate_project(&p).unwrap();
                    p.scenes[0].entities.len()
                },
                limit,
            );
            if mode == "static" || mode == "hierarchy" {
                row["queries"] = samples(
                    || {
                        let view = SceneView::new(s);
                        s.entities
                            .iter()
                            .map(|e| black_box(view.entity(&e.id)).unwrap().name.len())
                            .sum()
                    },
                    limit,
                );
                row["transforms"] = samples(
                    || {
                        let view = SceneView::new(s);
                        s.entities
                            .iter()
                            .map(|e| {
                                black_box(view.world_matrix(&e.id).unwrap()).to_cols_array()[12]
                                    .to_bits() as usize
                            })
                            .fold(0, usize::wrapping_add)
                    },
                    limit,
                );
                row["subtrees"] = samples(
                    || {
                        let view = SceneView::new(s);
                        s.entities
                            .iter()
                            .step_by(16)
                            .map(|e| view.descendants(&e.id).len())
                            .sum()
                    },
                    limit,
                );
                row["render_cpu"] = samples(
                    || oxy_render::prepare_scene(s, &oxy_render::CameraState::for_scene(s)).len(),
                    limit,
                );
                let mut edit = p.clone();
                let mut history = CommandHistory::new();
                let mut pixels = TextureCache::default();
                row["gesture_undo_redo"] = samples(
                    || {
                        history.begin("Mover", &edit, &pixels);
                        edit.scenes[0].entities[0].transform.position[0] += 1.;
                        history.commit(&edit, &mut pixels).unwrap();
                        history.undo(&mut edit, &mut pixels).unwrap();
                        history.redo(&mut edit, &mut pixels).unwrap();
                        history.estimated_bytes()
                    },
                    limit,
                );
            }
            let input = InputFrame {
                released: Default::default(),
                held: ["mover_direita".into()].into(),
                pressed: if mode.starts_with("graph") {
                    ["atacar".into()].into()
                } else {
                    Default::default()
                },
            };
            for (label, steps) in [("fixed_step", 1), ("accumulated_4_steps", 4)] {
                let mut runtime = Runtime::new(&p, &p.start_scene).unwrap();
                row[label] = samples(
                    || {
                        let before = runtime.time;
                        runtime.advance(FIXED_DT * steps as f32, &input);
                        assert!(
                            ((runtime.time - before) / f64::from(FIXED_DT) - steps as f64).abs()
                                < 0.001
                        );
                        black_box(runtime.scene());
                        runtime.pending_tasks() + runtime.retained_counts()[0]
                    },
                    limit,
                );
                row[format!("{label}_retained")] = serde_json::json!(runtime.retained_counts());
            }
            rows.push(row);
        }
    }
    let p = fixture(100, "graph_large");
    let mut runtime = Runtime::new(&p, &p.start_scene).unwrap();
    let mut retention = Vec::new();
    for i in 0..2000 {
        runtime.advance(
            FIXED_DT,
            &InputFrame {
                released: Default::default(),
                pressed: ["atacar".into()].into(),
                ..Default::default()
            },
        );
        if i % 200 == 0 {
            retention.push(serde_json::json!({"step":i,"retained":runtime.retained_counts()}));
        }
    }
    for _ in 0..10 {
        runtime.advance(FIXED_DT, &InputFrame::default());
    }
    retention.push(serde_json::json!({"phase":"drained","retained":runtime.retained_counts()}));
    runtime.stop();
    retention.push(serde_json::json!({"phase":"stop","retained":runtime.retained_counts()}));
    println!("{}",serde_json::to_string_pretty(&serde_json::json!({"method":"CPU only; 3 warmups + up to 101 samples; 3 seconds soft limit per measurement; deterministic ID layout; seed 0; exact steps asserted; profiling feature","rows":rows,"retention":retention})).unwrap());
}
