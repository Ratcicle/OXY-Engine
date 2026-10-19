//! The first surface of the scripts' API, the vertical slice of 0.7 (design §11, stage 3):
//! `obj.attr`, `obj.position`, `obj.world_position`, `audio.play`, `vars.get`/`vars.set`, the
//! callbacks `on_start`, `on_area_enter` and `on_input`, `self:after`, `self:sequence` and
//! `print`, with the names of decision Q5 and the person's names for sounds, variables and
//! actions; then the parkour's checkpoint and countdown as the design writes them.
use oxy_core::{
    character::CharacterConfig,
    command::{self, Command},
    document::*,
    input_actions,
    runtime::{FIXED_DT, InputFrame, Runtime, RuntimeOptions},
    script::{ScriptCall, ScriptError, ScriptRef, ScriptSources},
    variables,
};
use std::sync::Arc;

const PATH: &str = "scripts/teste.luau";

fn run(project: &Project, files: &[(&str, &str)]) -> Runtime {
    let options = RuntimeOptions {
        scripts: Arc::new(ScriptSources::from_texts(files.iter().copied())),
        ..Default::default()
    };
    Runtime::with_options(project, &project.start_scene, options).unwrap()
}

fn step(rt: &mut Runtime, steps: usize) {
    for _ in 0..steps {
        rt.advance(FIXED_DT, &InputFrame::default());
    }
}

/// An object named `name` with a text attribute `Resultado` that runs `scripts/teste.luau`.
fn object(name: &str) -> Entity {
    let mut e = Entity::new(name, None);
    e.attributes
        .insert("Resultado".into(), Value::Text(String::new()));
    e.scripts = vec![ScriptRef::new(PATH)];
    e
}

fn attribute(rt: &Runtime, id: &str, name: &str) -> Value {
    rt.scene().entity(id).unwrap().attributes[name].clone()
}

fn result(rt: &Runtime, id: &str) -> String {
    match attribute(rt, id, "Resultado") {
        Value::Text(text) => text,
        other => panic!("{other:?}"),
    }
}

fn failed(call: ScriptCall) -> ScriptError {
    match call {
        ScriptCall::Failed(error) => error,
        other => panic!("esperava um erro, veio {other:?}"),
    }
}

/// The log lines written since `from`.
fn lines_since(rt: &Runtime, from: u64) -> Vec<String> {
    let new = (rt.logs_written() - from) as usize;
    rt.logs[rt.logs.len() - new..]
        .iter()
        .map(|l| l.text.clone())
        .collect()
}

fn sound(project: &mut Project, name: &str) -> Id {
    let id = new_id();
    project.assets.push(Asset {
        id: id.clone(),
        name: name.into(),
        path: format!("assets/{}.wav", name.to_lowercase()),
        kind: AssetKind::Audio,
        model: None,
    });
    id
}

#[test]
fn attributes_read_nil_when_missing_and_take_only_their_type() {
    let mut door = object("Porta");
    door.attributes.insert("Vida".into(), Value::Number(3.));
    door.attributes
        .insert("Ponto".into(), Value::Vector2([0., 0.]));
    let id = door.id.clone();
    let mut project = Project::new("Atributos");
    project.scenes[0].entities.push(door);
    let source = r#"local M = {}
function M.read(self)
    self.attr.Resultado = tostring(self.attr.Vida) .. " " .. tostring(self.attr.Nada)
end
function M.write(self) self.attr.Vida -= 1 end
function M.wrong(self) self.attr.Vida = "muita" end
function M.missing(self) self.attr.Nada = 1 end
function M.flat(self) self.attr.Ponto = vector.create(1, 2, 3) end
return M
"#;
    let mut rt = run(&project, &[(PATH, source)]);
    let call = |rt: &mut Runtime, f: &str| rt.call_script(&id, PATH, f, &[]);
    assert_eq!(call(&mut rt, "read"), ScriptCall::Done);
    assert_eq!(result(&rt, &id), "3 nil");
    assert_eq!(call(&mut rt, "write"), ScriptCall::Done);
    assert_eq!(attribute(&rt, &id, "Vida"), Value::Number(2.));
    let e = failed(call(&mut rt, "wrong"));
    assert_eq!(
        (e.line, e.message.as_str()),
        (6, "o atributo Vida guarda número, não texto")
    );
    let e = failed(call(&mut rt, "missing"));
    assert_eq!(
        e.message,
        "o atributo Nada não existe neste objeto; crie-o no painel de propriedades"
    );
    // A Vetor2 takes a vector's x and y.
    assert_eq!(call(&mut rt, "flat"), ScriptCall::Done);
    assert_eq!(attribute(&rt, &id, "Ponto"), Value::Vector2([1., 2.]));
}

#[test]
fn positions_read_and_move_objects_in_the_parents_axes_and_in_the_world() {
    let mut project = Project::new("Posições");
    project.scenes[0].kind = SceneKind::ThreeD;
    let mut base = Entity::new("Base", None);
    base.transform.position = [10., 0., 0.];
    let mut crate_ = object("Caixa");
    crate_.parent = Some(base.id.clone());
    crate_.transform.position = [1., 2., 3.];
    let mut runner = object("Corredor");
    runner.character3d = Some(CharacterConfig::default());
    let (crate_id, runner_id) = (crate_.id.clone(), runner.id.clone());
    project.scenes[0].entities.extend([base, crate_, runner]);
    let source = r#"local M = {}
function M.read(self)
    local p, w = self.position, self.world_position
    self.attr.Resultado = string.format("%g,%g,%g %g,%g,%g", p.x, p.y, p.z, w.x, w.y, w.z)
end
function M.lift(self) self.position += vector.create(0, 1, 0) end
function M.place(self) self.world_position = vector.create(0, 0, 0) end
function M.wrong(self) self.position = 3 end
return M
"#;
    let mut rt = run(&project, &[(PATH, source)]);
    let call = |rt: &mut Runtime, id: &str, f: &str| rt.call_script(id, PATH, f, &[]);
    assert_eq!(call(&mut rt, &crate_id, "read"), ScriptCall::Done);
    assert_eq!(result(&rt, &crate_id), "1,2,3 11,2,3");
    assert_eq!(call(&mut rt, &crate_id, "lift"), ScriptCall::Done);
    let position = |rt: &Runtime| rt.scene().entity(&crate_id).unwrap().transform.position;
    assert_eq!(position(&rt), [1., 3., 3.]);
    // In the world: the parent's place is taken off.
    assert_eq!(call(&mut rt, &crate_id, "place"), ScriptCall::Done);
    assert_eq!(position(&rt), [-10., 0., 0.]);
    let e = failed(call(&mut rt, &crate_id, "wrong"));
    assert_eq!(
        e.message,
        "position recebe um vetor, como vector.create(0, 1, 0), não número"
    );
    // A 3D character moves by itself: the same refusal as Mover / girar / escalar objeto.
    let e = failed(call(&mut rt, &runner_id, "lift"));
    assert!(e.message.starts_with("Corredor é um personagem 3D"), "{e}");
    assert_eq!(call(&mut rt, &runner_id, "read"), ScriptCall::Done);
}

#[test]
fn audio_play_plays_a_sound_by_its_name() {
    let mut project = Project::new("Sons");
    let checkpoint = sound(&mut project, "Checkpoint");
    let beep = sound(&mut project, "Bipe");
    let speaker = object("Alto-falante");
    let id = speaker.id.clone();
    let mut top = object("Topo");
    top.scripts = vec![ScriptRef::new("scripts/topo.luau")];
    project.scenes[0].entities.extend([speaker, top]);
    let source = r#"local M = {}
local A = audio
function M.play(self) audio.play("Checkpoint") end
function M.quiet(self) A.play("bipe", { volume = 0.25, audience = self }) end
function M.loud(self) audio.play("Bipe", { volume = 7 }) end
function M.unknown(self) audio.play("Checkpoit") end
function M.option(self) audio.play("Bipe", { volum = 1 }) end
function M.audience(self) audio.play("Bipe", { audience = "Jogador" }) end
function M.typo(self) audio.paly("Bipe") end
return M
"#;
    let mut rt = run(
        &project,
        &[
            (PATH, source),
            ("scripts/topo.luau", "audio.play('Bipe')\nreturn {}\n"),
        ],
    );
    // The top of a script reaches nothing of the world.
    assert!(
        rt.logs[0].text.starts_with(
            "scripts/topo.luau:1: o topo do script não pode mexer no mundo; faça isso em \
             on_start"
        ),
        "{:?}",
        rt.logs
    );
    let call = |rt: &mut Runtime, f: &str| rt.call_script(&id, PATH, f, &[]);
    for f in ["play", "quiet", "loud"] {
        assert_eq!(call(&mut rt, f), ScriptCall::Done, "{f}");
    }
    let played: Vec<(String, f32)> = rt
        .sounds
        .iter()
        .map(|s| (s.asset.clone(), s.volume))
        .collect();
    assert_eq!(played, [(checkpoint, 1.), (beep.clone(), 0.25), (beep, 1.)]);
    for (f, message) in [
        (
            "unknown",
            "o som \"Checkpoit\" não existe no projeto; sons: Checkpoint, Bipe",
        ),
        (
            "option",
            "audio.play não conhece a opção volum; use volume ou audience",
        ),
        ("audience", "audio.play: audience é um objeto"),
        ("typo", "audio não tem paly; quis dizer play?"),
    ] {
        let e = failed(call(&mut rt, f));
        assert_eq!(e.message, message, "{f}");
        assert!(e.line > 1, "{f}: {e}");
    }
    assert_eq!(rt.sounds.len(), 3);
}

#[test]
fn game_variables_are_read_and_written_by_name_with_their_types_and_keys() {
    let mut project = Project::new("Variáveis");
    let start =
        variables::create(&mut project, "Largada liberada", Value::Bool(false), false).unwrap();
    let count = variables::create(&mut project, "Contagem", Value::Number(0.), false).unwrap();
    let record = variables::create(&mut project, "Recorde", Value::Number(99.), true).unwrap();
    let name = variables::create(&mut project, "Nome", Value::Text(String::new()), false).unwrap();
    let point = variables::create(&mut project, "Ponto", Value::Vector2([0.; 2]), false).unwrap();
    let goal = variables::create(&mut project, "Destino", Value::Vector3([0.; 3]), false).unwrap();
    let manager = object("Controle");
    let id = manager.id.clone();
    project.scenes[0].entities.push(manager);
    let source = r#"local M = {}
function M.go(self)
    vars.set("Largada liberada", true)
    vars.set("Contagem", 3)
    vars.set("Recorde", 42.5, "Pista")
    vars.set("nome", "Ana")
    vars.set("Ponto", vector.create(1, 2, 3))
    vars.set("Destino", vector.create(4, 5, 6))
    local p = vars.get("Ponto")
    self.attr.Resultado = string.format("%s %g %g %g %s %g,%g,%g",
        tostring(vars.get("Largada liberada")), vars.get("Contagem"), vars.get("Recorde", "Pista"),
        vars.get("Recorde", "Outra"), vars.get("Nome"), p.x, p.y, p.z)
end
function M.unknown(self) vars.get("Largada") end
function M.wrong(self) vars.set("Contagem", "três") end
function M.key(self) vars.get("Contagem", 1) end
function M.typo(self) vars.sett("Contagem", 1) end
return M
"#;
    let mut rt = run(&project, &[(PATH, source)]);
    let call = |rt: &mut Runtime, f: &str| rt.call_script(&id, PATH, f, &[]);
    assert_eq!(call(&mut rt, "go"), ScriptCall::Done);
    // A key never written reads the initial value; a Vetor2 reads with z 0.
    assert_eq!(result(&rt, &id), "true 3 42.5 99 Ana 1,2,0");
    for (variable, key, value) in [
        (&start, "", Value::Bool(true)),
        (&count, "", Value::Number(3.)),
        (&record, "Pista", Value::Number(42.5)),
        (&record, "", Value::Number(99.)),
        (&name, "", Value::Text("Ana".into())),
        (&point, "", Value::Vector2([1., 2.])),
        (&goal, "", Value::Vector3([4., 5., 6.])),
    ] {
        assert_eq!(rt.variable(variable, key), Some(&value), "{variable} {key}");
    }
    let e = failed(call(&mut rt, "unknown"));
    assert!(
        e.message.starts_with(
            "A variável do jogo \"Largada\" não existe no projeto. Variáveis: Largada liberada, \
             Contagem"
        ),
        "{e}"
    );
    // Names, never IDs.
    assert!(!e.message.contains(&start), "{e}");
    let e = failed(call(&mut rt, "wrong"));
    assert_eq!(
        e.message,
        "vars.set: a variável Contagem guarda número, não texto"
    );
    let e = failed(call(&mut rt, "key"));
    assert_eq!(e.message, "a chave de vars.get é um texto");
    let e = failed(call(&mut rt, "typo"));
    assert_eq!(e.message, "vars não tem sett; quis dizer set?");
    assert_eq!(rt.variable(&count, ""), Some(&Value::Number(3.)));
}

#[test]
fn callbacks_hear_the_start_areas_and_actions_by_their_names() {
    let mut project = Project::new("Eventos");
    let jump = input_actions::create(&mut project, "Pular alto", "Space").unwrap();
    let mut listener = object("Ouvinte");
    listener.scripts = vec![ScriptRef::new("scripts/ouvinte.luau")];
    let mut area = object("Área");
    area.collider = Some(Collider {
        is_trigger: true,
        ..Default::default()
    });
    area.scripts = vec![ScriptRef::new("scripts/area.luau")];
    let mut crate_ = Entity::new("Caixa", None);
    crate_.collider = Some(Collider::default());
    project.scenes[0].entities.extend([listener, area, crate_]);
    let listener_source = r#"local M = {}
function M.on_start(self) print("começou") end
function M.on_input(self, action, phase) print(action, phase) end
return M
"#;
    let area_source = r#"local M = {}
function M.on_area_enter(self, other) print("entrou", other.name) end
return M
"#;
    let mut rt = run(
        &project,
        &[
            ("scripts/ouvinte.luau", listener_source),
            ("scripts/area.luau", area_source),
        ],
    );
    assert_eq!(lines_since(&rt, 0), ["Ouvinte · começou"]);
    let from = rt.logs_written();
    let press = InputFrame {
        pressed: [jump.clone()].into(),
        held: [jump.clone()].into(),
        ..Default::default()
    };
    rt.advance(FIXED_DT, &press);
    let release = InputFrame {
        released: [jump.clone()].into(),
        ..Default::default()
    };
    rt.advance(FIXED_DT, &release);
    // The action by the name the person gave it, never its ID.
    assert_eq!(
        lines_since(&rt, from),
        [
            "Ouvinte · Pular alto pressed",
            "Área · entrou Caixa",
            "Ouvinte · Pular alto released",
        ]
    );
    assert_eq!(rt.script_errors().count(), 0);
}

/// The checkpoint of the parkour, as design §5.8 writes it.
const CHECKPOINT: &str = r#"--!strict
-- Área de checkpoint: 1 arquivo para os 6 checkpoints e o modelo.
local Checkpoint = {}
Checkpoint.attributes = { Ordem = 1 }      -- anexar cria o atributo que falta [Q6]
local FEET = vector.create(0, 1.5, 0)      -- do centro da área até o chão

function Checkpoint.on_area_enter(self: Object, other: Object)
    if not other:has_attribute("Checkpoint atual") then
        return                             -- uma caixa de física também entra na área [Q4]
    end
    local order = self.attr.Ordem
    if other.attr["Checkpoint atual"] < order then
        other.attr["Ponto de retorno"] = self.world_position - FEET
        other.attr["Checkpoint atual"] = order
        audio.play("Checkpoint", { volume = 0.9, audience = other })
    end
end

return Checkpoint
"#;

/// The countdown of design §5.8, without the panel the slice cannot hide yet (`obj.visible`
/// and `param.object()` come with the full API).
const RACE: &str = r#"--!strict
-- Contagem da largada 3, 2, 1, Já! O HUD mostra o atributo Aviso.
local Race = {}
Race.attributes = { Aviso = "" }

function Race.on_start(self: Object)
    vars.set("Largada liberada", false)
    self:sequence(1, Race.show, { 3, 2, 1, 0 })   -- show(self, 3) no passo seguinte, depois 1 por segundo
    self:after(4, Race.hide_banner)
end

function Race.show(self: Object, n: number)
    vars.set("Contagem da largada", n)
    self.attr.Aviso = if n > 0 then tostring(n) else "Já!"
    audio.play(if n > 0 then "Bipe" else "Largada")
    print("contagem", n)
    if n == 0 then vars.set("Largada liberada", true) end
end

function Race.hide_banner(self: Object)
    self.attr.Aviso = ""
end

return Race
"#;

#[test]
fn the_parkour_checkpoint_and_countdown_run_as_the_design_writes_them() {
    let mut project = Project::new("Pista");
    let start =
        variables::create(&mut project, "Largada liberada", Value::Bool(true), false).unwrap();
    let count = variables::create(
        &mut project,
        "Contagem da largada",
        Value::Number(0.),
        false,
    )
    .unwrap();
    let checkpoint_sound = sound(&mut project, "Checkpoint");
    let beep = sound(&mut project, "Bipe");
    let go = sound(&mut project, "Largada");
    let control = Entity::new("Controle da corrida", None);
    let mut area = Entity::new("Checkpoint 2", None);
    area.collider = Some(Collider {
        is_trigger: true,
        ..Default::default()
    });
    area.transform.position = [5., 1.5, 0.];
    let mut runner = Entity::new("Jogador", None);
    runner.collider = Some(Collider::default());
    runner.transform.position = [5., 1.5, 0.];
    runner
        .attributes
        .insert("Checkpoint atual".into(), Value::Number(1.));
    runner
        .attributes
        .insert("Ponto de retorno".into(), Value::Vector3([0.; 3]));
    // A physics box enters too, and the checkpoint leaves it alone.
    let mut crate_ = Entity::new("Caixa", None);
    crate_.collider = Some(Collider::default());
    crate_.transform.position = [5., 1.5, 0.];
    let (control_id, area_id, runner_id) = (control.id.clone(), area.id.clone(), runner.id.clone());
    project.scenes[0]
        .entities
        .extend([control, area, runner, crate_]);
    let files = [
        ("scripts/checkpoint.luau", CHECKPOINT),
        ("scripts/controle_da_corrida.luau", RACE),
    ];
    let sources = ScriptSources::from_texts(files);
    // Attached as the person or an agent attaches them: the declared attributes appear.
    let scene = project.start_scene.clone();
    let attach = |entity: &Id, path: &str| Command::AttachScript {
        scene: None,
        entity: entity.clone(),
        path: path.into(),
        index: None,
    };
    command::apply_with_scripts(
        &mut project,
        &scene,
        &[
            attach(&area_id, "scripts/checkpoint.luau"),
            attach(&control_id, "scripts/controle_da_corrida.luau"),
        ],
        &sources,
    )
    .unwrap();
    let declared = |project: &Project, id: &Id, name: &str| {
        project.scenes[0].entity(id).unwrap().attributes[name].clone()
    };
    assert_eq!(
        declared(&project, &control_id, "Aviso"),
        Value::Text(String::new())
    );
    assert_eq!(declared(&project, &area_id, "Ordem"), Value::Number(1.));
    // The second checkpoint of the course.
    project.scenes[0]
        .entity_mut(&area_id)
        .unwrap()
        .attributes
        .insert("Ordem".into(), Value::Number(2.));
    let mut rt = run(&project, &files);
    assert_eq!(rt.variable(&start, ""), Some(&Value::Bool(false)));
    step(&mut rt, 1);
    let aviso = |rt: &Runtime| attribute(rt, &control_id, "Aviso");
    assert_eq!(aviso(&rt), Value::Text("3".into()));
    assert_eq!(
        attribute(&rt, &runner_id, "Checkpoint atual"),
        Value::Number(2.)
    );
    assert_eq!(
        attribute(&rt, &runner_id, "Ponto de retorno"),
        Value::Vector3([5., 0., 0.])
    );
    step(&mut rt, 69);
    assert_eq!(aviso(&rt), Value::Text("2".into()));
    step(&mut rt, 120);
    assert_eq!(aviso(&rt), Value::Text("Já!".into()));
    assert_eq!(rt.variable(&start, ""), Some(&Value::Bool(true)));
    assert_eq!(rt.variable(&count, ""), Some(&Value::Number(0.)));
    step(&mut rt, 60);
    assert_eq!(aviso(&rt), Value::Text(String::new()));
    let played: Vec<(&str, f32)> = rt
        .sounds
        .iter()
        .map(|s| (s.asset.as_str(), s.volume))
        .collect();
    let mut expected = vec![(beep.as_str(), 1.), (checkpoint_sound.as_str(), 0.9)];
    expected.extend([(beep.as_str(), 1.), (beep.as_str(), 1.), (go.as_str(), 1.)]);
    assert_eq!(played, expected);
    let counted: Vec<&str> = rt
        .logs
        .iter()
        .map(|l| l.text.as_str())
        .filter(|l| l.contains("contagem"))
        .collect();
    assert_eq!(
        counted,
        [
            "Controle da corrida · contagem 3",
            "Controle da corrida · contagem 2",
            "Controle da corrida · contagem 1",
            "Controle da corrida · contagem 0",
        ]
    );
    assert_eq!(rt.script_errors().count(), 0, "{:?}", rt.logs);
}
