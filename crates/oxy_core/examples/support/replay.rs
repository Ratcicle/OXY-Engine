use super::movement;
use oxy_core::{
    character::*,
    document::*,
    graph::{Edge, Node},
    input_timeline::{InputChange, TimedInput},
    runtime::{FIXED_DT, InputFrame, Runtime},
};
use serde::Serialize;
#[derive(Debug, Clone, Serialize)]
pub struct Sample {
    pub tick: u64,
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub look: [f32; 2],
    pub grounded: bool,
    pub posture: String,
    pub support: Option<String>,
}
#[derive(Debug, Serialize)]
pub struct Replay {
    pub hz: usize,
    pub mode: String,
    pub samples: Vec<Sample>,
    pub events: Vec<(u64, String)>,
    pub apex_sampled_m: f32,
    pub peak_speed_sampled_mps: f32,
    pub retained_after_stop: [usize; 4],
}
fn connect(e: &mut Entity, a: &Node, output: &str, b: &Node, input: &str) {
    e.graph.edges.push(Edge {
        from_node: a.id.clone(),
        from_port: output.into(),
        to_node: b.id.clone(),
        to_port: input.into(),
    });
}
pub fn project(mode: &str) -> Project {
    let mut p = movement::fixture(1, mode);
    p.scenes[0].entities.retain(|e| {
        e.id == movement::id(0)
            || e.id == movement::id(1000)
            || e.id == movement::id(2000)
            || e.id == movement::id(3000)
    });
    let body = p.scenes[0].entity_mut(&movement::id(0)).unwrap();
    let c = body.character3d.as_mut().unwrap();
    c.apply_profile(MovementProfile::ChainedJumps);
    c.jump_mode = JumpMode::Automatic;
    for (i, kind) in ["jump", "land", "leave", "posture", "surface"]
        .into_iter()
        .enumerate()
    {
        body.attributes.insert(kind.into(), Value::Number(0.));
        let mut event = Node::new("event.character", [0.; 2]);
        event.id = movement::id(7000 + i * 2);
        event.params.insert("kind".into(), Value::Text(kind.into()));
        let mut set = Node::new("attribute.set", [0.; 2]);
        set.id = movement::id(7001 + i * 2);
        set.params
            .insert("attribute".into(), Value::Text(kind.into()));
        set.params.insert("value".into(), Value::Number(1.));
        connect(body, &event, "exec", &set, "exec");
        body.graph.nodes.extend([event, set]);
    }
    let mut event = Node::new("event.input", [0.; 2]);
    event.id = movement::id(7100);
    event
        .params
        .insert("action".into(), Value::Text("reiniciar".into()));
    let mut teleport = Node::new("character.teleport", [0.; 2]);
    teleport.id = movement::id(7101);
    teleport.params.insert(
        "position".into(),
        Value::Vector3([-0.8, if mode == "platforms" { 0.52 } else { 0.02 }, 0.]),
    );
    teleport
        .params
        .insert("restore_look".into(), Value::Bool(true));
    teleport
        .params
        .insert("look".into(), Value::Vector2([0., 0.]));
    connect(body, &event, "exec", &teleport, "exec");
    body.graph.nodes.extend([event, teleport]);
    validate_project(&p).unwrap();
    p
}
pub fn run(hz: usize, mode: &str) -> Replay {
    let p = project(mode);
    let original = p.clone();
    let id = movement::id(0);
    let mut rt = Runtime::new(&p, &p.start_scene).unwrap();
    let mut inputs = vec![
        (0.21, InputChange::Movement([0., 1.])),
        (0.5, InputChange::Press("correr".into())),
        (1.113, InputChange::Press("pular".into())),
        (4.29, InputChange::Release("pular".into())),
        (4.7, InputChange::Press("agachar".into())),
        (5.3, InputChange::Release("agachar".into())),
        (5.33, InputChange::Press("alternar_camera".into())),
        (5.36, InputChange::Release("alternar_camera".into())),
        (6.317, InputChange::Press("reiniciar".into())),
        (6.35, InputChange::Release("reiniciar".into())),
        (7.31, InputChange::Press("pular".into())),
        (8.2, InputChange::Release("pular".into())),
        (9.1, InputChange::Movement([0., 0.])),
        (9.2, InputChange::Release("correr".into())),
    ];
    for i in 0..120 {
        inputs.push((
            0.23 + i as f64 * 0.071,
            InputChange::Look([
                if i < 60 { 1.2 } else { -0.8 },
                if i % 2 == 0 { 0.5 } else { -0.5 },
            ]),
        ));
    }
    inputs.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (at, change) in inputs {
        rt.queue_timed_input(TimedInput { at, change }).unwrap();
    }
    let mut out = Replay {
        hz,
        mode: mode.into(),
        samples: Vec::new(),
        events: Vec::new(),
        apex_sampled_m: 0.,
        peak_speed_sampled_mps: 0.,
        retained_after_stop: [0; 4],
    };
    for frame in 1..=hz * 12 {
        rt.advance(1. / hz as f32, &InputFrame::default());
        for trace in rt.traces.drain(..) {
            out.events.push((
                (trace.time / f64::from(FIXED_DT)).round() as u64,
                trace.node,
            ));
        }
        let s = rt.character_state(&id).unwrap();
        out.apex_sampled_m = out.apex_sampled_m.max(s.position.y);
        out.peak_speed_sampled_mps = out
            .peak_speed_sampled_mps
            .max(s.total_velocity().with_y(0.).length());
        if frame % (hz / 2) == 0 {
            out.samples.push(Sample {
                tick: (rt.time / f64::from(FIXED_DT)).round() as u64,
                position: s.position.to_array(),
                velocity: s.total_velocity().to_array(),
                look: [s.yaw, s.pitch],
                grounded: s.grounded,
                posture: format!("{:?}", s.posture),
                support: s.support.as_ref().map(|s| s.object.clone()),
            });
        }
    }
    assert!(rt.logs.is_empty(), "{mode}/{hz}: {:?}", rt.logs);
    assert!((rt.time - 12.).abs() < 1e-5);
    assert_eq!(p, original, "Play changed authoring data");
    rt.stop();
    out.retained_after_stop = rt.retained_counts();
    out
}
pub fn compare(a: &Replay, b: &Replay) {
    assert_eq!(a.events, b.events, "event ordering/timing at {} Hz", b.hz);
    assert_eq!(a.samples.len(), b.samples.len());
    for (a, b) in a.samples.iter().zip(&b.samples) {
        assert_eq!(
            (a.tick, a.grounded, &a.posture, &a.support),
            (b.tick, b.grounded, &b.posture, &b.support)
        );
        for (x, y) in a
            .position
            .iter()
            .chain(&a.velocity)
            .chain(&a.look)
            .zip(b.position.iter().chain(&b.velocity).chain(&b.look))
        {
            assert!(
                (x - y).abs() <= 2e-4,
                "fixed state changed at tick {}: {x} != {y}",
                a.tick
            );
        }
    }
}
