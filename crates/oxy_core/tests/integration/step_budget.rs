//! The logic work budget (`MAX_NODE_WORK`) belongs to each fixed step, not to a frame: a game
//! whose logic runs out of it does the same whether the host's frames take one step each or
//! several, as they do in the editor and the player at different frame rates.
use oxy_core::{
    document::*,
    graph::{Edge, Node},
    runtime::{FIXED_DT, InputFrame, MAX_NODE_WORK, MAX_STEPS, Runtime, StateDigest, StatePart},
};

fn node(entity: &mut Entity, operation: &str, params: &[(&str, Value)]) -> Id {
    let mut node = Node::new(operation, [0.; 2]);
    for (key, value) in params {
        node.params.insert((*key).into(), value.clone());
    }
    let id = node.id.clone();
    entity.graph.nodes.push(node);
    id
}
fn wire(entity: &mut Entity, from: &str, from_port: &str, to: &str, to_port: &str) {
    entity.graph.edges.push(Edge {
        from_node: from.into(),
        from_port: from_port.into(),
        to_node: to.into(),
        to_port: to_port.into(),
    });
}
fn text(value: &str) -> Value {
    Value::Text(value.into())
}

/// Logic that asks for more than a step's work, in a 2D scene where a box falls: every step
/// starts a flow longer than the budget, so flows pile up and each step runs out of work; a
/// counter adds one a step, when its turn comes behind them; and a click starts a sum that
/// reads more data than a step allows, which stops its object. Returns the sum's object.
fn busy_project() -> (Project, Id) {
    let mut project = Project::new("Orçamento por passo");
    let mut long = Entity::new("Fluxo longo", None);
    let mut previous = node(&mut long, "event.step", &[]);
    let mut port = "exec";
    for _ in 0..MAX_NODE_WORK + MAX_NODE_WORK / 2 {
        let next = node(&mut long, "control.sequence", &[]);
        wire(&mut long, &previous, port, &next, "exec");
        (previous, port) = (next, "first");
    }
    let mut counter = Entity::new("Contador", None);
    counter
        .attributes
        .insert("Passos".into(), Value::Number(0.));
    let step = node(&mut counter, "event.step", &[]);
    let read = node(
        &mut counter,
        "attribute.get",
        &[("attribute", text("Passos"))],
    );
    let add = node(&mut counter, "math.binary", &[("operator", text("+"))]);
    let set = node(
        &mut counter,
        "attribute.set",
        &[("attribute", text("Passos"))],
    );
    wire(&mut counter, &step, "exec", &set, "exec");
    wire(&mut counter, &read, "value", &add, "a");
    wire(&mut counter, &add, "value", &set, "value");
    // Each sum reads the one below it twice: 2^12 - 1 reads in all.
    let mut heavy = Entity::new("Soma pesada", None);
    heavy.attributes.insert("Total".into(), Value::Number(0.));
    let click = node(&mut heavy, "event.click", &[]);
    let set = node(&mut heavy, "attribute.set", &[("attribute", text("Total"))]);
    wire(&mut heavy, &click, "exec", &set, "exec");
    let mut below = node(&mut heavy, "value.number", &[("value", Value::Number(1.))]);
    for _ in 0..11 {
        let sum = node(&mut heavy, "math.binary", &[("operator", text("+"))]);
        wire(&mut heavy, &below, "value", &sum, "a");
        wire(&mut heavy, &below, "value", &sum, "b");
        below = sum;
    }
    wire(&mut heavy, &below, "value", &set, "value");
    let mut falling = Entity::new("Caixa", Some(Primitive::Rectangle));
    falling.controller = Some(Controller::default());
    falling.collider = Some(Collider::default());
    falling.transform.position[1] = 50.;
    let heavy_id = heavy.id.clone();
    project.scenes[0].kind = SceneKind::TwoD;
    project.scenes[0]
        .entities
        .extend([long, counter, heavy, falling]);
    validate_project(&project).unwrap();
    (project, heavy_id)
}

/// Plays `steps` steps in frames of `per_frame` steps each, clicking `clicked` after the
/// eighth: the clock and the state after each frame, and the runtime at the end.
fn play(
    project: &Project,
    clicked: &str,
    per_frame: usize,
    steps: usize,
) -> (Vec<(f64, StateDigest)>, Runtime) {
    let mut runtime = Runtime::new(project, &project.start_scene).unwrap();
    let mut states = Vec::new();
    for frame in 0..steps / per_frame {
        if frame * per_frame == 8 {
            runtime.click(clicked);
        }
        runtime.advance(FIXED_DT * per_frame as f32, &InputFrame::default());
        states.push((runtime.time, runtime.state_digest()));
    }
    (states, runtime)
}

/// Every part but the clock's, whose leftover of a frame (`accumulator`, an f32) is not
/// always zero after frames of several steps.
fn without_clock(digest: &StateDigest) -> Vec<&StatePart> {
    digest
        .parts
        .iter()
        .filter(|part| part.object.is_some() || part.name != "time")
        .collect()
}

#[test]
fn logic_out_of_work_does_the_same_at_any_frame_rate() {
    let (project, heavy) = busy_project();
    let steps = 6 * MAX_STEPS;
    let (one, runtime) = play(&project, &heavy, 1, steps);
    let (two, _) = play(&project, &heavy, 2, steps);
    let (eight, _) = play(&project, &heavy, MAX_STEPS, steps);
    assert_eq!(one.len(), steps);
    for (i, (time, digest)) in two.iter().enumerate() {
        let (same_time, same) = &one[2 * i + 1];
        assert_eq!(time, same_time);
        assert_eq!(
            digest.hash(),
            same.hash(),
            "step {}: {:?}",
            2 * i + 2,
            digest.first_difference(same)
        );
    }
    for (i, (time, digest)) in eight.iter().enumerate() {
        let (same_time, same) = &one[MAX_STEPS * (i + 1) - 1];
        assert_eq!(time, same_time);
        assert_eq!(
            without_clock(digest),
            without_clock(same),
            "step {}: {:?}",
            MAX_STEPS * (i + 1),
            digest.first_difference(same)
        );
    }
    // Every step ran out of work and said so once, and the sum stopped its object.
    let texts: Vec<&str> = runtime.logs.iter().map(|l| l.text.as_str()).collect();
    let exhausted = "Limite de trabalho por atualização atingido; demais tarefas retomam no \
                     próximo passo.";
    assert_eq!(
        texts.iter().filter(|text| **text == exhausted).count(),
        steps,
        "{texts:?}"
    );
    assert!(
        texts.iter().any(|text| text.ends_with(&format!(
            "Limite de {MAX_NODE_WORK} operações por atualização. Comportamento interrompido."
        ))),
        "{texts:?}"
    );
    let total = &runtime.scene().entity(&heavy).unwrap().attributes["Total"];
    assert_eq!(total, &Value::Number(0.));
    assert!(
        one.last()
            .unwrap()
            .1
            .parts
            .iter()
            .any(|part| part.object.as_ref() == Some(&heavy) && part.name == "behavior")
    );
}

#[test]
fn each_step_of_a_frame_has_the_whole_budget() {
    let mut project = Project::new("Fluxo longo");
    let mut entity = Entity::new("Fluxo", None);
    let mut previous = node(&mut entity, "event.click", &[]);
    let mut port = "exec";
    for _ in 0..3 * MAX_NODE_WORK {
        let next = node(&mut entity, "control.sequence", &[]);
        wire(&mut entity, &previous, port, &next, "exec");
        (previous, port) = (next, "first");
    }
    let id = entity.id.clone();
    project.scenes[0].entities.push(entity);
    let mut runtime = Runtime::new(&project, &project.start_scene).unwrap();
    runtime.click(&id);
    runtime.advance(2. * FIXED_DT, &InputFrame::default());
    assert_eq!(runtime.traces_written(), 2 * MAX_NODE_WORK as u64);
    assert_eq!(runtime.pending_tasks(), 1);
    runtime.advance(FIXED_DT, &InputFrame::default());
    assert_eq!(runtime.traces_written(), 3 * MAX_NODE_WORK as u64);
    assert_eq!(
        runtime.pending_tasks(),
        1,
        "the last node waits for the next step"
    );
    runtime.advance(FIXED_DT, &InputFrame::default());
    assert_eq!(runtime.traces_written(), 3 * MAX_NODE_WORK as u64 + 1);
    assert_eq!(runtime.pending_tasks(), 0);
}
