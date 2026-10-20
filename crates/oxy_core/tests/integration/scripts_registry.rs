//! The registry of the scripts' API (design §5.7, stage 4): every row's example, as the whole
//! script the reference and the Guide show, loads, is attached as the person attaches it and
//! runs without an error in a small project with the sound, the variable, the module and the
//! attributes the examples use, callbacks included; and the markers of `param` declare the
//! attributes that have no literal.
use oxy_core::{
    character::{CharacterConfig, MovementEvent},
    command::{self, Command},
    document::*,
    physics3d::{Collider3d, CollisionShape},
    runtime::{FIXED_DT, InputFrame, Runtime, RuntimeOptions},
    script::{self, ScriptCall, ScriptRef, ScriptSources, registry},
    variables,
};
use std::sync::Arc;

const PATH: &str = "scripts/exemplo.luau";

/// The module `require`'s example loads, next to the example.
const MODULE: (&str, &str) = ("scripts/dados.luau", "return { custo = 2 }\n");

/// A project with the door every example runs on: its attributes, the sound `Bipe` and the
/// game variable `Pontos`.
fn project() -> (Project, Id) {
    let mut project = Project::new("API");
    variables::create(&mut project, "Pontos", Value::Number(0.), false).unwrap();
    project.assets.push(Asset {
        id: new_id(),
        name: "Bipe".into(),
        path: "assets/bipe.wav".into(),
        kind: AssetKind::Audio,
        model: None,
    });
    let mut door = Entity::new("Porta", Some(Primitive::Cube));
    door.attributes.insert("Vida".into(), Value::Number(3.));
    door.attributes
        .insert("Aviso".into(), Value::Text(String::new()));
    let id = door.id.clone();
    project.scenes[0].entities.push(door);
    (project, id)
}

/// `project` with `text` attached to the door, as attach_script attaches it; the warnings.
fn attach(project: &mut Project, door: &Id, sources: &ScriptSources) -> Vec<String> {
    let scene = project.start_scene.clone();
    let applied = command::apply_with_scripts(
        project,
        &scene,
        &[Command::AttachScript {
            scene: None,
            entity: door.clone(),
            path: PATH.into(),
            index: None,
        }],
        sources,
    )
    .unwrap();
    applied.into_iter().flat_map(|a| a.warnings).collect()
}

fn run(project: &Project, sources: ScriptSources, steps: usize) -> Runtime {
    let options = RuntimeOptions {
        scripts: Arc::new(sources),
        ..Default::default()
    };
    let mut rt = Runtime::with_options(project, &project.start_scene, options).unwrap();
    for _ in 0..steps {
        rt.advance(FIXED_DT, &InputFrame::default());
    }
    rt
}

/// What a callback's example gets after `self`, by its parameters' types: the door for an
/// object, the first of the texts a parameter allows, an input action's name for a text.
fn callback_args(f: &registry::ApiFn, door: &Id) -> Vec<Value> {
    f.params
        .iter()
        .map(|p| match p.ty {
            "Object" => Value::Object(Some(door.clone())),
            "string" => Value::Text("Pular".into()),
            "boolean" => Value::Bool(true),
            "number" => Value::Number(f64::from(FIXED_DT)),
            texts if texts.starts_with('"') => Value::Text(texts.split('"').nth(1).unwrap().into()),
            _ => Value::Number(1.),
        })
        .collect()
}

/// `project` with the door a 3D character above a floor, which lands on it.
fn character(project: &mut Project, door: &Id) {
    let scene = &mut project.scenes[0];
    scene.kind = SceneKind::ThreeD;
    let mut floor = Entity::new("Chão", Some(Primitive::Cube));
    floor.dimensions = [40., 1., 40.];
    floor.transform.position = [0., -0.5, 0.];
    floor.physics3d = Some(Collider3d {
        shape: CollisionShape::Box {
            size: floor.dimensions,
        },
        ..Default::default()
    });
    scene.entities.push(floor);
    let body = scene.entity_mut(door).unwrap();
    body.character3d = Some(CharacterConfig::default());
    body.transform.position[1] = 3.;
}

#[test]
fn every_row_of_the_api_runs_its_example() {
    for f in registry::API {
        let text = f.example_script();
        let sources = ScriptSources::from_texts([(PATH, text.as_str()), MODULE]);
        let report = &script::check_loading(&sources, [PATH], |_| true)[0];
        assert!(
            report.error.is_none() && report.warnings.is_empty(),
            "{}: {report:?}\n{text}",
            f.name
        );
        let (mut project, door) = project();
        let on_character = f.name == "on_character";
        if on_character {
            character(&mut project, &door);
        }
        let warnings = attach(&mut project, &door, &sources);
        assert!(warnings.is_empty(), "{}: {warnings:?}", f.name);
        // Three seconds: the examples' timers go off, and the character lands.
        let mut rt = run(&project, sources, 0);
        let mut landed = false;
        for _ in 0..180 {
            rt.advance(FIXED_DT, &InputFrame::default());
            landed |= rt
                .movement_records()
                .iter()
                .any(|r| r.object == door && matches!(r.event, MovementEvent::Landed { .. }));
        }
        if on_character {
            // Its example logs the landing.
            let logged = rt.logs.iter().any(|l| l.text.starts_with("Porta · pousou"));
            assert!(landed && logged, "{}: {:?}", f.name, rt.logs);
        } else if f.kind == registry::ApiKind::Callback
            && !["on_start", "on_step"].contains(&f.name)
        {
            // The steps ran on_start and on_step; the other callbacks are called as their
            // events call them.
            let args = callback_args(f, &door);
            assert_eq!(
                rt.call_script(&door, PATH, f.name, &args),
                ScriptCall::Done,
                "{}",
                f.name
            );
        }
        let errors: Vec<&str> = rt.script_errors().map(|(_, text, _)| text).collect();
        assert!(errors.is_empty(), "{}: {errors:?}\n{text}", f.name);
    }
}

#[test]
fn param_markers_declare_the_attributes_that_have_no_literal() {
    let text = "local M = {}\nM.attributes = { Alvo = param.object(), Tamanho = \
                param.vector2(1, 2), Vida = 3 }\nfunction M.on_start(self)\n    \
                self.attr.Aviso = tostring(self.attr.Alvo) .. ' ' .. \
                tostring(self.attr.Tamanho)\nend\nfunction M.apontar(self, outro)\n    \
                self.attr.Alvo = outro\n    self.attr.Tamanho = vector.create(3, 4, 5)\n    \
                self.attr.Aviso = self.attr.Alvo.name\nend\nreturn M\n";
    let sources = ScriptSources::from_texts([(PATH, text)]);
    let declared = script::declared_attributes(&sources, [PATH]);
    assert_eq!(
        declared[0].1.clone().unwrap(),
        [
            ("Alvo".to_owned(), Value::Object(None)),
            ("Tamanho".to_owned(), Value::Vector2([1., 2.])),
            ("Vida".to_owned(), Value::Number(3.)),
        ]
    );
    let (mut project, door) = project();
    // The door has Vida already, as a number: it stays.
    project.scenes[0]
        .entity_mut(&door)
        .unwrap()
        .attributes
        .insert("Vida".into(), Value::Number(7.));
    assert_eq!(attach(&mut project, &door, &sources), Vec::<String>::new());
    let attributes = &project.scenes[0].entity(&door).unwrap().attributes;
    assert_eq!(attributes["Alvo"], Value::Object(None));
    assert_eq!(attributes["Tamanho"], Value::Vector2([1., 2.]));
    assert_eq!(attributes["Vida"], Value::Number(7.));
    let target = Entity::new("Alvo certo", None);
    let target_id = target.id.clone();
    project.scenes[0].entities.push(target);
    let mut rt = run(&project, sources, 1);
    let aviso = |rt: &Runtime| rt.scene().entity(&door).unwrap().attributes["Aviso"].clone();
    // An empty object reads nil; a Vetor2, a vector with z 0.
    assert_eq!(aviso(&rt), Value::Text("nil 1, 2, 0".into()));
    let other = [Value::Object(Some(target_id.clone()))];
    assert_eq!(
        rt.call_script(&door, PATH, "apontar", &other),
        ScriptCall::Done
    );
    let attributes = &rt.scene().entity(&door).unwrap().attributes;
    assert_eq!(attributes["Alvo"], Value::Object(Some(target_id)));
    assert_eq!(attributes["Tamanho"], Value::Vector2([3., 4.]));
    assert_eq!(attributes["Aviso"], Value::Text("Alvo certo".into()));
}

#[test]
fn param_markers_refuse_what_they_cannot_declare() {
    let refused = |text: &str| {
        let sources = ScriptSources::from_texts([(PATH, text)]);
        script::check_loading(&sources, [PATH], |_| true)
            .remove(0)
            .error
            .map(|e| e.to_string())
    };
    assert_eq!(
        refused("local M = {}\nM.attributes = { Tamanho = param.vector2('1', 2) }\nreturn M\n"),
        Some(
            "scripts/exemplo.luau:2: param.vector2 precisa de dois números, como \
             param.vector2(1, 1)"
                .into()
        )
    );
    // A finite number past what a Vetor2 keeps, in either coordinate, and NaN.
    for args in ["1, 1e39", "-1e39, 1", "0/0, 1"] {
        assert_eq!(
            refused(&format!(
                "local M = {{}}\nM.attributes = {{ Tamanho = param.vector2({args}) }}\nreturn M\n"
            )),
            Some(
                "scripts/exemplo.luau:2: param.vector2 não aceita infinito, NaN nem números além \
                 do que um Vetor2 guarda (±3,4e38)"
                    .into()
            ),
            "{args}"
        );
    }
    assert_eq!(
        refused("local M = {}\nM.state = { alvo = param.object() }\nreturn M\n"),
        Some(
            "scripts/exemplo.luau: Modulo.state.alvo: os marcadores param servem em \
             Modulo.attributes; a memória ainda não tem campos que começam vazios (para um \
             objeto escolhido depois, declare um atributo Objeto: Modulo.attributes = { alvo = \
             param.object() })"
                .into()
        )
    );
    // A marker cannot be changed or faked: its metatable is the engine's, locked.
    assert!(
        refused(
            "local M = {}\nlocal p = param.object()\nsetmetatable(p, nil)\nM.attributes = { A = \
             p }\nreturn M\n"
        )
        .unwrap()
        .contains("a metatabela desta tabela é protegida")
    );
    let sources = ScriptSources::from_texts([(
        PATH,
        "local M = {}\nM.attributes = { Alvo = { param = 'object' } }\nreturn M\n",
    )]);
    let declared = script::declared_attributes(&sources, [PATH]);
    assert!(
        declared[0]
            .1
            .as_ref()
            .unwrap_err()
            .message
            .contains("Modulo.attributes.Alvo: uma tabela não serve de valor padrão")
    );
}

#[test]
fn the_api_names_its_objects_and_suggests_its_own_names() {
    let text = "local M = {}\nfunction M.on_start(self)\n    audio.paly('Bipe')\nend\nreturn M\n";
    let (mut project, door) = project();
    project.scenes[0].entity_mut(&door).unwrap().scripts = vec![ScriptRef::new(PATH)];
    let rt = run(&project, ScriptSources::from_texts([(PATH, text)]), 1);
    let errors: Vec<&str> = rt.script_errors().map(|(_, text, _)| text).collect();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        errors[0].contains("scripts/exemplo.luau:3: audio não tem paly; quis dizer play?"),
        "{errors:?}"
    );
}
