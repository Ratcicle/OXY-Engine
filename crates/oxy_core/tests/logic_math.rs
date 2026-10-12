//! The number and text data nodes run by the runtime: arithmetic, functions, limits and
//! time formatting.
use oxy_core::{
    document::*,
    graph::*,
    runtime::{FIXED_DT, InputFrame, Runtime},
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
