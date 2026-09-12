//! CPU-only M7 harness; no FPS/GPU time is inferred from these measurements.
#[path = "../../oxy_core/examples/support/movement.rs"]
pub mod movement;
#[allow(dead_code)]
#[path = "performance.rs"]
mod standard;
use oxy_core::{
    document::*,
    metrics,
    runtime::{FIXED_DT, InputFrame, Runtime},
};
use std::{
    collections::BTreeMap,
    hint::black_box,
    time::{Duration, Instant},
};
fn distribution(mut values: Vec<u64>) -> serde_json::Value {
    values.sort_unstable();
    let n = values.len();
    let q = |fraction: f64| {
        values
            .get(((n.saturating_sub(1)) as f64 * fraction).ceil() as usize)
            .copied()
    };
    serde_json::json!({"samples":n,"median_ns":q(0.5),"p95_ns":if n>=100{q(0.95)}else{None},"p99_ns":if n>=100{q(0.99)}else{None},"limit_exceeded":n<301})
}
fn input(tick: usize) -> InputFrame {
    InputFrame {
        movement: [
            0.,
            if (tick / 60).is_multiple_of(2) {
                1.
            } else {
                -1.
            },
        ],
        ..Default::default()
    }
}
pub fn measure(p: &Project, steps: usize) -> serde_json::Value {
    let mut rt = Runtime::new(p, &p.start_scene).unwrap();
    for tick in 0..120 {
        rt.advance(FIXED_DT, &input(tick));
    }
    let before = rt.physics_world().unwrap().counters();
    let before_camera = rt.camera_physics_counters();
    let mut phases: BTreeMap<&str, Vec<u64>> = BTreeMap::new();
    let mut work = None;
    let mut checksum = 0u64;
    let start = Instant::now();
    for i in 0..301 {
        if start.elapsed() > Duration::from_secs(8) {
            break;
        }
        metrics::take();
        let time = rt.time;
        let begin = Instant::now();
        rt.advance(FIXED_DT * steps as f32, &input(120 + i * steps));
        let elapsed = begin.elapsed().as_nanos() as u64;
        let c = metrics::take();
        assert_eq!(c.steps, steps as u64, "Run with oxy_core/profiling");
        assert!(((rt.time - time) / f64::from(FIXED_DT) - steps as f64).abs() < 1e-6);
        for (name, value) in [
            ("advance", elapsed),
            ("fixed_steps", c.fixed_step_ns),
            ("input", c.input_ns),
            ("prepare_queries_platforms", c.character_prepare_ns),
            ("motor_inclusive", c.character_motor_ns),
            ("resolve_subset_of_motor", c.character_resolve_ns),
            ("sensors", c.character_sensors_ns),
            ("tasks", c.tasks_ns),
            ("animation", c.animation_ns),
            ("presentation_camera", c.presentation_ns),
        ] {
            phases.entry(name).or_default().push(value);
        }
        checksum = checksum.wrapping_add(black_box(
            rt.character_state(&movement::id(0))
                .unwrap()
                .position
                .z
                .to_bits(),
        ) as u64);
        work = Some(c);
    }
    assert!(rt.logs.is_empty(), "{:?}", rt.logs);
    let after = rt.physics_world().unwrap().counters();
    let after_camera = rt.camera_physics_counters();
    let mut result = serde_json::json!({"steps_per_advance":steps,"checksum":checksum,"last_sample_work":work,"physics_before":before,"physics_after":after,"camera_before":before_camera,"camera_after":after_camera,"retained":rt.retained_counts()});
    for (name, values) in phases {
        result[name] = distribution(values);
    }
    rt.stop();
    result["retained_after_stop"] = serde_json::json!(rt.retained_counts());
    result
}
fn main() {
    let counts: Vec<usize> = std::env::args()
        .nth(1)
        .unwrap_or("1,10,50".into())
        .split(',')
        .map(|s| s.parse().unwrap())
        .collect();
    let mut rows = Vec::new();
    for count in counts {
        for mode in ["sparse", "dense", "triangles", "platforms", "corridor"] {
            eprintln!("{mode}: {count} controllers / 400 total");
            let p = movement::fixture(count, mode);
            let s = &p.scenes[0];
            let bytes = serde_json::to_vec(&p).unwrap();
            let limit = Duration::from_secs(3);
            let load = standard::samples(
                || {
                    let p: Project = serde_json::from_slice(&bytes).unwrap();
                    validate_project(&p).unwrap();
                    black_box(p).scenes[0].entities.len()
                },
                limit,
            );
            let create = standard::samples(
                || {
                    black_box(Runtime::new(&p, &p.start_scene).unwrap())
                        .scene()
                        .entities
                        .len()
                },
                limit,
            );
            let mut rt = Runtime::new(&p, &p.start_scene).unwrap();
            rt.advance(FIXED_DT, &input(0));
            let camera = oxy_render::CameraState::for_runtime(&rt);
            let render = standard::samples(
                || black_box(oxy_render::prepare_scene(rt.scene(), &camera)).len(),
                limit,
            );
            rows.push(serde_json::json!({"scenario":mode,"controllers":count,"entities":s.entities.len(),"sensors":s.entities.iter().filter(|e|e.physics3d.as_ref().is_some_and(|p|p.sensor)).count(),"physical_shapes":s.entities.iter().filter(|e|e.physics3d.is_some() || e.character3d.is_some()).count(),"static_physics_triangles":if mode=="triangles"{3200}else{0},"json_bytes":bytes.len(),"load_validate":load,"runtime_create":create,"render_cpu":render,"single":measure(&p,1),"four_steps":measure(&p,4)}));
        }
    }
    println!("{}",serde_json::to_string_pretty(&serde_json::json!({"method":"CPU only, seed 0, fixed UUIDs, release locked profiling; 120 warmup ticks then 301 samples (8s soft limit each); exactly 1 or 4 fixed steps; presentation outside step; resolve is included in motor; 101 samples/3s for create/load/render; counters cumulative between before/after; no GPU/RAM estimate","rows":rows})).unwrap());
}
