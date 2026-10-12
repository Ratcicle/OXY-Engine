//! The number and text nodes run by the runtime: arithmetic, functions, limits, time
//! formatting and random draws.
use oxy_core::{
    document::*,
    graph::*,
    random::MAX_SEED,
    runtime::{FIXED_DT, InputFrame, Runtime, RuntimeOptions},
};

fn node(op: &str, params: &[(&str, Value)]) -> Node {
    let mut n = Node::new(op, [0., 0.]);
    n.params
        .extend(params.iter().map(|(k, v)| ((*k).into(), v.clone())));
    n
}
fn wire(g: &mut Graph, a: &Node, out: &str, b: &Node, input: &str) {
    g.edges.push(Edge {
        from_node: a.id.clone(),
        from_port: out.into(),
        to_node: b.id.clone(),
        to_port: input.into(),
    });
}
fn text(value: &str) -> Value {
    Value::Text(value.into())
}
fn n(value: f64) -> Value {
    Value::Number(value)
}

/// A project whose Painel writes the first output of `data` into its attribute Resultado
/// when `event` runs, and the Painel's ID.
fn panel(event: &str, data: &Node, extra: &[Node]) -> (Project, Id) {
    let output = &definition(&data.operation).unwrap().outputs[0];
    let mut project = Project::new("Contas");
    let mut owner = Entity::new("Painel", None);
    owner.attributes.insert(
        "Resultado".into(),
        match output.kind {
            PortType::Text => text(""),
            _ => n(0.),
        },
    );
    let start = node(event, &[]);
    let write = node("attribute.set", &[("attribute", text("Resultado"))]);
    owner.graph.nodes = vec![start.clone(), data.clone(), write.clone()];
    owner.graph.nodes.extend(extra.iter().cloned());
    wire(&mut owner.graph, &start, "exec", &write, "exec");
    wire(&mut owner.graph, data, output.id, &write, "value");
    let id = owner.id.clone();
    project.scenes[0].entities.push(owner);
    validate_project(&project).unwrap();
    (project, id)
}

/// What a node of `operation` with `params` gives when the scene starts, or the error the
/// runtime logged for it.
fn eval(operation: &str, params: &[(&str, Value)]) -> Result<Value, String> {
    let (project, id) = panel("event.scene_start", &node(operation, params), &[]);
    let runtime = Runtime::new(&project, &project.start_scene).unwrap();
    match runtime.logs.as_slice() {
        [] => Ok(runtime.scene().entity(&id).unwrap().attributes["Resultado"].clone()),
        [error] => Err(error.clone()),
        logs => panic!("{operation}: {logs:?}"),
    }
}
fn number(operation: &str, params: &[(&str, Value)]) -> f64 {
    match eval(operation, params) {
        Ok(Value::Number(value)) => value,
        other => panic!("{operation} {params:?}: {other:?}"),
    }
}
fn binary(a: f64, operator: &str, b: f64) -> Result<Value, String> {
    eval(
        "math.binary",
        &[("a", n(a)), ("b", n(b)), ("operator", text(operator))],
    )
}
fn unary(operator: &str, value: f64) -> Result<Value, String> {
    eval(
        "math.unary",
        &[("value", n(value)), ("operator", text(operator))],
    )
}
fn time(seconds: f64, format: &str) -> Result<String, String> {
    eval(
        "text.time",
        &[("seconds", n(seconds)), ("format", text(format))],
    )
    .map(|value| match value {
        Value::Text(text) => text,
        other => panic!("text.time gave {other:?}"),
    })
}
fn refused(result: Result<impl std::fmt::Debug, String>, message: &str) {
    let error = result.unwrap_err();
    assert!(
        error.contains(message) && error.ends_with("Comportamento interrompido."),
        "{error}"
    );
}

#[test]
fn every_number_of_the_math_nodes_also_comes_through_a_port() {
    for id in ["math.binary", "math.unary", "math.clamp", "text.time"] {
        let def = definition(id).unwrap();
        assert_eq!(def.category, "Dados", "{id}");
        for param in &def.params {
            let port = def.inputs.iter().find(|port| port.id == param.id);
            match &param.default {
                Value::Number(_) => assert_eq!(port.unwrap().kind, PortType::Number, "{id}"),
                _ => assert!(port.is_none(), "{id}.{}", param.id),
            }
        }
    }
    assert_eq!(
        definition("math.binary").unwrap().params[2].label,
        "Operação"
    );
}

#[test]
fn arithmetic_takes_the_remainder_the_minimum_and_the_maximum() {
    // The remainder is never negative, whatever the signs.
    for (a, b, expected) in [
        (7., 3., 1.),
        (-7., 3., 2.),
        (7., -3., 1.),
        (-7., -3., 2.),
        (-1., 3., 2.),
        (5.5, 2., 1.5),
        (6., 3., 0.),
    ] {
        assert_eq!(binary(a, "%", b), Ok(n(expected)), "{a} mod {b}");
    }
    refused(binary(7., "%", 0.), "Resto por zero");
    refused(binary(7., "/", 0.), "Divisão por zero");
    assert_eq!(binary(3., "min", -2.), Ok(n(-2.)));
    assert_eq!(binary(3., "max", -2.), Ok(n(3.)));
    assert_eq!(binary(4., "-", 6.), Ok(n(-2.)));
    refused(binary(1e308, "*", 10.), "Resultado aritmético não finito");
    for (operator, _) in ARITHMETIC {
        binary(9., operator, 4.).unwrap_or_else(|error| panic!("{operator}: {error}"));
    }
}

#[test]
fn functions_round_and_measure_numbers() {
    let table = [
        ("floor", 2.7, 2.),
        ("floor", -2.7, -3.),
        ("ceil", 2.1, 3.),
        ("ceil", -2.1, -2.),
        ("round", 2.5, 3.),
        ("round", -2.5, -3.),
        ("round", 2.4, 2.),
        ("trunc", 2.7, 2.),
        ("trunc", -2.7, -2.),
        ("abs", -3.5, 3.5),
        ("abs", 3.5, 3.5),
        ("sign", -4., -1.),
        ("sign", 0., 0.),
        ("sign", 7., 1.),
        ("sqrt", 9., 3.),
        ("sqrt", 0., 0.),
    ];
    for (operator, value, expected) in table {
        assert_eq!(
            unary(operator, value),
            Ok(n(expected)),
            "{operator}({value})"
        );
    }
    for (operator, _) in choices("math.unary", "operator").unwrap() {
        assert!(
            table.iter().any(|(tested, ..)| tested == operator),
            "{operator}"
        );
    }
    refused(unary("sqrt", -1.), "Raiz quadrada de número negativo (-1)");
}

#[test]
fn a_zero_result_is_never_negative_zero() {
    // -0 equals 0, but the interface writes it as -0.
    let unary = |operator: &str, value: f64| {
        number(
            "math.unary",
            &[("value", n(value)), ("operator", text(operator))],
        )
    };
    let binary = |a: f64, operator: &str, b: f64| {
        number(
            "math.binary",
            &[("a", n(a)), ("b", n(b)), ("operator", text(operator))],
        )
    };
    for zero in [
        unary("ceil", -0.4),
        unary("round", -0.4),
        unary("trunc", -0.7),
        unary("sqrt", -0.),
        unary("sign", -0.),
        binary(-0., "%", 3.),
        binary(-3., "%", 3.),
        binary(0., "*", -1.),
        number(
            "math.clamp",
            &[("value", n(-0.)), ("min", n(0.)), ("max", n(1.))],
        ),
    ] {
        assert!(zero == 0. && zero.is_sign_positive(), "{zero}");
    }
}

#[test]
fn clamp_keeps_a_number_between_the_limits() {
    let clamp = |value: f64, min: f64, max: f64| {
        eval(
            "math.clamp",
            &[("value", n(value)), ("min", n(min)), ("max", n(max))],
        )
    };
    for (value, min, max, expected) in [
        (5., 0., 1., 1.),
        (-2., 0., 1., 0.),
        (0.25, 0., 1., 0.25),
        (3., 3., 3., 3.),
        (-150., -100., 100., -100.),
    ] {
        assert_eq!(clamp(value, min, max), Ok(n(expected)), "{value}");
    }
    refused(clamp(0., 2., 1.), "Mínimo 2 maior que o máximo 1");
    assert_eq!(number("math.clamp", &[]), 0.);
}

#[test]
fn time_is_truncated_and_written_with_a_dot() {
    for (seconds, format, expected) in [
        (0., "mm:ss.cc", "00:00.00"),
        (1.15, "mm:ss.cc", "00:01.15"),
        (59.999, "mm:ss.cc", "00:59.99"),
        (61.5, "mm:ss.cc", "01:01.50"),
        (3725., "mm:ss.cc", "62:05.00"),
        (-1., "mm:ss.cc", "-00:01.00"),
        (-0.001, "mm:ss.cc", "00:00.00"),
        (3725., "h:mm:ss", "1:02:05"),
        (59.999, "h:mm:ss", "0:00:59"),
        (61.5, "m:ss.cc", "1:01.50"),
        (3725., "m:ss.cc", "62:05.00"),
        (61.5, "mm:ss", "01:01"),
        (59.999, "mm:ss", "00:59"),
        // ss.cc counts every second, with two digits at least.
        (0.5, "ss.cc", "00.50"),
        (125., "ss.cc", "125.00"),
        (-0.5, "ss.cc", "-00.50"),
        (1.15, "mm:ss.mmm", "00:01.150"),
        (59.9999, "mm:ss.mmm", "00:59.999"),
    ] {
        assert_eq!(
            time(seconds, format).as_deref(),
            Ok(expected),
            "{seconds} {format}"
        );
    }
    // Each choice shows how it writes 83.456 s, and the node writes it the same way.
    for (format, label) in choices("text.time", "format").unwrap() {
        let (shown, example) = label.split_once(" · ").unwrap();
        assert_eq!(shown, *format);
        let example = example.split(' ').next().unwrap();
        assert_eq!(time(83.456, format).as_deref(), Ok(example), "{format}");
    }
    refused(time(1e300, "mm:ss.cc"), "Tempo grande demais");
}

#[test]
fn time_formats_a_number_read_through_its_port_and_refuses_one_not_finite() {
    let read = node("attribute.get", &[("attribute", text("Tempo"))]);
    let format = node("text.time", &[("format", text("mm:ss.cc"))]);
    let (mut project, id) = panel("event.step", &format, std::slice::from_ref(&read));
    let owner = project.scenes[0].entity_mut(&id).unwrap();
    owner.attributes.insert("Tempo".into(), n(83.456));
    wire(&mut owner.graph, &read, "value", &format, "seconds");
    validate_project(&project).unwrap();
    let mut runtime = Runtime::new(&project, &project.start_scene).unwrap();
    runtime.advance(FIXED_DT, &InputFrame::default());
    assert!(runtime.logs.is_empty(), "{:?}", runtime.logs);
    let attributes = |runtime: &Runtime| runtime.scene().entity(&id).unwrap().attributes.clone();
    assert_eq!(attributes(&runtime)["Resultado"], text("01:23.45"));

    // Saved projects refuse a value that is not finite, so only a running game has one.
    runtime
        .scene_mut()
        .entity_mut(&id)
        .unwrap()
        .attributes
        .insert("Tempo".into(), n(f64::NAN));
    runtime.advance(FIXED_DT, &InputFrame::default());
    assert_eq!(runtime.logs.len(), 1, "{:?}", runtime.logs);
    assert!(
        runtime.logs[0].contains("Segundos precisam ser um número finito"),
        "{:?}",
        runtime.logs
    );
    assert_eq!(attributes(&runtime)["Resultado"], text("01:23.45"));
}

/// A Dado that draws with `draw` on every step and writes the number into its attribute A;
/// with `before`, another draw runs first and writes B. Pressing "semear" runs `reseed`.
fn dice(draw: &Node, before: Option<&Node>, reseed: Option<&Node>) -> Project {
    let mut project = Project::new("Sorteios");
    let mut owner = Entity::new("Dado", None);
    // The same object in every project, as the draws depend on it.
    owner.id = "dado".into();
    owner.attributes.insert("A".into(), n(0.));
    owner.attributes.insert("B".into(), n(0.));
    let step = node("event.step", &[]);
    let write = node("attribute.set", &[("attribute", text("A"))]);
    owner.graph.nodes = vec![step.clone(), draw.clone(), write.clone()];
    wire(&mut owner.graph, draw, "exec", &write, "exec");
    wire(&mut owner.graph, draw, "value", &write, "value");
    if let Some(before) = before {
        let other = node("attribute.set", &[("attribute", text("B"))]);
        owner.graph.nodes.extend([before.clone(), other.clone()]);
        wire(&mut owner.graph, &step, "exec", before, "exec");
        wire(&mut owner.graph, before, "exec", &other, "exec");
        wire(&mut owner.graph, before, "value", &other, "value");
        wire(&mut owner.graph, &other, "exec", draw, "exec");
    } else {
        wire(&mut owner.graph, &step, "exec", draw, "exec");
    }
    if let Some(reseed) = reseed {
        let press = node("event.input", &[("action", text("semear"))]);
        owner.graph.nodes.extend([press.clone(), reseed.clone()]);
        wire(&mut owner.graph, &press, "exec", reseed, "exec");
    }
    project.scenes[0].entities.push(owner);
    validate_project(&project).unwrap();
    project
}
fn seeded(project: &Project, seed: u64) -> Runtime {
    Runtime::with_options(
        project,
        &project.start_scene,
        RuntimeOptions {
            seed,
            ..Default::default()
        },
    )
    .unwrap()
}
fn step(runtime: &mut Runtime, press: bool) {
    let input = InputFrame {
        pressed: if press {
            ["semear".into()].into()
        } else {
            Default::default()
        },
        ..Default::default()
    };
    runtime.advance(FIXED_DT, &input);
}
/// The number A holds after each of `steps` steps of a run started with `seed`, pressing
/// "semear" on the steps in `press`.
fn draws(project: &Project, seed: u64, steps: usize, press: &[usize]) -> Vec<f64> {
    let mut runtime = seeded(project, seed);
    (0..steps)
        .map(|i| {
            step(&mut runtime, press.contains(&i));
            assert!(runtime.logs.is_empty(), "{:?}", runtime.logs);
            match runtime.scene().entity("dado").unwrap().attributes["A"] {
                Value::Number(value) => value,
                ref other => panic!("{other:?}"),
            }
        })
        .collect()
}
fn draw_node(min: f64, max: f64, mode: &str) -> Node {
    node(
        "random.number",
        &[("min", n(min)), ("max", n(max)), ("mode", text(mode))],
    )
}

#[test]
fn the_random_nodes_are_actions_with_their_numbers_also_through_ports() {
    for id in ["random.number", "random.seed"] {
        let def = definition(id).unwrap();
        assert_eq!(def.category, "Ações", "{id}");
        assert_eq!(def.inputs[0].kind, PortType::Exec, "{id}");
        assert_eq!(def.outputs[0].kind, PortType::Exec, "{id}");
        for param in &def.params {
            let port = def.inputs.iter().find(|port| port.id == param.id);
            match &param.default {
                Value::Number(_) => assert_eq!(port.unwrap().kind, PortType::Number, "{id}"),
                _ => assert!(port.is_none(), "{id}.{}", param.id),
            }
        }
    }
    let number = definition("random.number").unwrap();
    assert_eq!(number.outputs[1].id, "value");
    assert_eq!(number.outputs[1].kind, PortType::Number);
    let modes: Vec<_> = choices("random.number", "mode").unwrap().to_vec();
    assert_eq!(
        modes,
        [
            ("real", "Real entre mínimo e máximo"),
            ("integer", "Inteiro, extremos incluídos")
        ]
    );
    // After Mudar de cena, before the Jogo nodes.
    let order: Vec<_> = registry().iter().map(|def| def.id).collect();
    let scene = order.iter().position(|id| *id == "action.scene").unwrap();
    assert_eq!(
        order[scene..scene + 3],
        ["action.scene", "random.number", "random.seed"]
    );
}

#[test]
fn each_draw_node_repeats_its_numbers_for_a_seed_whatever_other_draws_do() {
    let draw = draw_node(1., 6., "integer");
    let alone = dice(&draw, None, None);
    let with_other = dice(&draw, Some(&draw_node(0., 1., "real")), None);
    let first = draws(&alone, 5, 120, &[]);
    assert_eq!(first, draws(&alone, 5, 120, &[]));
    assert_eq!(
        first,
        draws(&with_other, 5, 120, &[]),
        "another draw node, run first, leaves these numbers alone"
    );
    assert_ne!(first, draws(&alone, 6, 120, &[]));
    // Both ends included, only integers.
    for face in 1..=6 {
        assert!(first.contains(&f64::from(face)), "{face}: {first:?}");
    }
    assert!(
        first
            .iter()
            .all(|v| v.fract() == 0. && (1. ..=6.).contains(v))
    );
    // Bounds with decimals keep the integers between them.
    let between = draws(
        &dice(&draw_node(0.5, 3.5, "integer"), None, None),
        0,
        120,
        &[],
    );
    for k in [1., 2., 3.] {
        assert!(between.contains(&k), "{k}: {between:?}");
    }
    assert!(
        between.iter().all(|v| [1., 2., 3.].contains(v)),
        "{between:?}"
    );
    // Real numbers never reach the maximum.
    let real = draws(&dice(&draw_node(-2., 3., "real"), None, None), 0, 120, &[]);
    assert!(real.iter().all(|v| (-2. ..3.).contains(v)), "{real:?}");
    assert!(real.iter().any(|v| v.fract() != 0.), "{real:?}");
    let same = draws(&dice(&draw_node(4., 4., "real"), None, None), 0, 3, &[]);
    assert_eq!(same, [4.; 3]);
    let wide = draws(
        &dice(&draw_node(-1e308, 1e308, "real"), None, None),
        0,
        3,
        &[],
    );
    assert!(wide.iter().all(|v| v.is_finite()), "{wide:?}");
}

#[test]
fn setting_the_seed_starts_every_draw_over() {
    let draw = draw_node(1., 1000., "integer");
    let project = dice(&draw, None, Some(&node("random.seed", &[("seed", n(42.))])));
    let plain = draws(&project, 42, 5, &[]);
    // On step 3 the draw comes first (the step event runs before inputs), then the seed
    // starts the count over: steps 4 and 5 draw as steps 0 and 1.
    let again = draws(&project, 42, 6, &[3]);
    assert_eq!(again[..4], plain[..4]);
    assert_eq!(again[4..], plain[..2]);
    // Given 42 by logic, a run started with 0 draws as one started with 42.
    let late = draws(&project, 0, 6, &[0]);
    assert_eq!(late[1..], plain[..]);
    assert_ne!(late[0], plain[0]);
    // The largest seed is accepted.
    let largest = dice(
        &draw,
        None,
        Some(&node("random.seed", &[("seed", n(MAX_SEED as f64))])),
    );
    assert_eq!(
        draws(&largest, 0, 3, &[0])[1..],
        draws(&largest, MAX_SEED, 2, &[])
    );
}

#[test]
fn draws_refuse_bounds_without_numbers_and_a_value_read_before_the_draw() {
    let errors = |project: &Project| {
        let mut runtime = seeded(project, 0);
        step(&mut runtime, true);
        runtime.logs.join("\n")
    };
    for (draw, message) in [
        (draw_node(2., 1., "real"), "Mínimo 2 maior que o máximo 1"),
        (
            draw_node(0.2, 0.8, "integer"),
            "Nenhum inteiro entre o mínimo 0.2 e o máximo 0.8",
        ),
        (
            draw_node(-1e300, 1., "integer"),
            "No modo Inteiro, mínimo e máximo precisam estar entre -9007199254740991 e \
             9007199254740991",
        ),
    ] {
        let error = errors(&dice(&draw, None, None));
        assert!(error.contains(message), "{error}");
    }
    for seed in [-1., 1.5, (MAX_SEED + 1) as f64] {
        let reseed = node("random.seed", &[("seed", n(seed))]);
        let error = errors(&dice(&draw_node(0., 1., "real"), None, Some(&reseed)));
        assert!(
            error.contains("A semente precisa ser um inteiro de 0 a 9007199254740991"),
            "{seed}: {error}"
        );
    }
    // The number exists once the node runs: reading it from a flow that did not run it fails.
    let mut project = dice(&draw_node(0., 1., "real"), None, None);
    let owner = &mut project.scenes[0].entities[0];
    let draw = owner.graph.nodes[1].clone();
    let press = node("event.input", &[("action", text("semear"))]);
    let write = node("attribute.set", &[("attribute", text("B"))]);
    owner.graph.nodes.extend([press.clone(), write.clone()]);
    wire(&mut owner.graph, &press, "exec", &write, "exec");
    wire(&mut owner.graph, &draw, "value", &write, "value");
    let error = errors(&project);
    assert!(
        error.contains("Dados da saída value indisponíveis; execute a ação produtora antes"),
        "{error}"
    );
}
