//! The objects, the scene, the transform, the math and the attributes of the scripts' API
//! (design §5.4, stage 4): each function on a small project, its errors named as the person
//! reads them, copies and removals that replay alike, and the parkour's countdown hiding its
//! panel by itself, without the graph of 3 nodes the vertical slice still needed.
use oxy_core::{
    character::CharacterConfig,
    command::{self, Command, ParamValue},
    document::*,
    graph::{Edge, Node},
    headless::{self, SimulationRequest},
    persistence,
    physics3d::{Collider3d, CollisionShape},
    runtime::{FIXED_DT, InputFrame, ObjectComponent, Runtime, RuntimeOptions},
    script::{ScriptCall, ScriptError, ScriptRef, ScriptSources},
    surface::{SurfaceMaterial, SurfacePreset},
    variables,
};
use serde_json::json;
use std::path::Path;
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

fn child(name: &str, parent: &Entity) -> Entity {
    let mut e = Entity::new(name, None);
    e.parent = Some(parent.id.clone());
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

fn call(rt: &mut Runtime, id: &str, function: &str) -> ScriptCall {
    rt.call_script(id, PATH, function, &[])
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

fn errors(rt: &Runtime) -> Vec<String> {
    rt.script_errors()
        .map(|(_, text, _)| text.to_owned())
        .collect()
}

#[test]
fn objects_read_their_hierarchy_and_switch_their_parts() {
    let mut door = object("Porta");
    door.collider = Some(Collider {
        is_trigger: true,
        ..Default::default()
    });
    let handle = child("Maçaneta", &door);
    let panel = child("Painel", &door);
    let text = child("Texto", &panel);
    let other = Entity::new("Porta", None);
    let (id, panel_id) = (door.id.clone(), panel.id.clone());
    let mut project = Project::new("Objetos");
    project.scenes[0]
        .entities
        .extend([door, handle, panel, text, other]);
    let source = r#"local M = {}
function M.read(self)
    local nomes = {}
    for _, filho in self:children() do
        table.insert(nomes, filho.name)
    end
    local fundo = self:find_child("Texto", true)
    self.attr.Resultado = table.concat(nomes, ",") .. " " .. self:get_child("Maçaneta").parent.name
        .. " " .. tostring(self:find_child("Texto")) .. " " .. fundo.name .. " "
        .. tostring(self.parent) .. " " .. tostring(fundo.parent == self:get_child("Painel"))
end
function M.missing(self) return self:get_child("Macaneta") end
function M.missing_deep(self) return self:get_child("Fundo", true) end
function M.hide(self) self:get_child("Painel").visible = false end
function M.parts(self)
    self.attr.Resultado = tostring(self:has_component("collider")) .. " "
        .. tostring(self:has_component("area")) .. " " .. tostring(self:has_component("character"))
        .. " " .. tostring(self:has_component("ui")) .. " " .. tostring(self.visible)
end
function M.unknown_kind(self) return self:has_component("colider") end
function M.id(self) self.attr.Resultado = self.id end
function M.write_id(self) self.id = "x" end
function M.write_visible(self) self.visible = 1 end
function M.write_parent(self) self.parent = nil end
function M.not_object(self) return self.children(5) end
return M
"#;
    let mut rt = run(&project, &[(PATH, source)]);
    assert_eq!(call(&mut rt, &id, "read"), ScriptCall::Done);
    assert_eq!(result(&rt, &id), "Maçaneta,Painel Porta nil Texto nil true");
    let e = failed(call(&mut rt, &id, "missing"));
    assert_eq!(
        e.message,
        "Porta não tem um filho chamado \"Macaneta\" (quis dizer Maçaneta?); filhos: Maçaneta, \
         Painel; para quando a ausência é esperada, use find_child, que dá nil"
    );
    assert_eq!(e.line, 12);
    let e = failed(call(&mut rt, &id, "missing_deep"));
    assert!(
        e.message
            .starts_with("Porta não tem nada chamado \"Fundo\" abaixo dele; abaixo dele: Maçaneta"),
        "{e}"
    );
    assert_eq!(call(&mut rt, &id, "hide"), ScriptCall::Done);
    assert!(!rt.scene().entity(&panel_id).unwrap().visible);
    assert_eq!(call(&mut rt, &id, "parts"), ScriptCall::Done);
    assert_eq!(result(&rt, &id), "true true false false true");
    assert_eq!(call(&mut rt, &id, "id"), ScriptCall::Done);
    assert_eq!(result(&rt, &id), id);
    for (function, message) in [
        (
            "unknown_kind",
            "has_component não conhece o componente \"colider\"; quis dizer collider? \
             Componentes: animation, area, camera, character, collider, controller, platform, ui",
        ),
        ("write_id", "id é só leitura"),
        ("write_visible", "visible recebe true ou false, não número"),
        ("write_parent", "parent é só leitura"),
        (
            "not_object",
            "children é um método de objetos: obj:children(...)",
        ),
    ] {
        let e = failed(call(&mut rt, &id, function));
        assert!(e.message.starts_with(message), "{function}: {e}");
    }
}

/// `event.scene_start → control.wait(seconds) → attribute.set(Marca = "grafo")` on `e`.
fn mark_later(e: &mut Entity, seconds: f64) {
    let start = Node::new("event.scene_start", [0.; 2]);
    let mut wait = Node::new("control.wait", [0.; 2]);
    wait.params.insert("seconds".into(), Value::Number(seconds));
    let mut set = Node::new("attribute.set", [0.; 2]);
    set.params
        .insert("attribute".into(), Value::Text("Marca".into()));
    set.params
        .insert("value".into(), Value::Text("grafo".into()));
    for (from, to) in [(&start, &wait), (&wait, &set)] {
        e.graph.edges.push(Edge {
            from_node: from.id.clone(),
            from_port: "exec".into(),
            to_node: to.id.clone(),
            to_port: "exec".into(),
        });
    }
    e.graph.nodes = vec![start, wait, set];
}

#[test]
fn logic_enabled_switches_the_graph_and_the_scripts_go_on() {
    let mut stopped = object("Parado");
    mark_later(&mut stopped, 0.5);
    let mut running = object("Ligado");
    mark_later(&mut running, 0.5);
    for e in [&mut stopped, &mut running] {
        e.attributes
            .insert("Marca".into(), Value::Text(String::new()));
        e.attributes.insert("Passos".into(), Value::Number(0.));
    }
    let (stopped_id, running_id) = (stopped.id.clone(), running.id.clone());
    let mut project = Project::new("Lógica");
    project.scenes[0].entities.extend([stopped, running]);
    let source = r#"local M = {}
function M.on_start(self)
    if self.name == "Parado" then
        self.logic_enabled = false
    end
    self.attr.Resultado = tostring(self.logic_enabled)
end
function M.on_step(self) self.attr.Passos += 1 end
return M
"#;
    let mut rt = run(&project, &[(PATH, source)]);
    assert_eq!(result(&rt, &stopped_id), "false");
    assert_eq!(result(&rt, &running_id), "true");
    assert!(!rt.logic_enabled(&stopped_id));
    step(&mut rt, 60);
    // The graph's wait went with it; the script hears every step.
    assert_eq!(
        attribute(&rt, &stopped_id, "Marca"),
        Value::Text(String::new())
    );
    assert_eq!(
        attribute(&rt, &running_id, "Marca"),
        Value::Text("grafo".into())
    );
    assert_eq!(attribute(&rt, &stopped_id, "Passos"), Value::Number(60.));
    assert!(errors(&rt).is_empty(), "{:?}", rt.logs);
}

#[test]
fn destroying_removes_the_hierarchy_and_the_object_itself_ends_its_call_quietly() {
    let mut target = Entity::new("Alvo", None);
    target.attributes.insert("Vida".into(), Value::Number(1.));
    let target_child = child("Filho do alvo", &target);
    let witness = object("Testemunha");
    let mut door = object("Porta");
    door.attributes
        .insert("Alvo".into(), Value::Object(Some(target.id.clone())));
    let (id, witness_id, target_id) = (door.id.clone(), witness.id.clone(), target.id.clone());
    let mut project = Project::new("Remover");
    project.scenes[0]
        .entities
        .extend([door, target, target_child, witness]);
    let source = r#"local M = {}
M.state = { alvo = param.object() }
function M.other(self)
    self.state.alvo = self.attr.Alvo
    self.attr.Alvo:destroy()
    self.attr.Resultado = tostring(self.state.alvo:exists()) .. " "
        .. tostring(scene.find("Filho do alvo")) .. " " .. tostring(self.attr.Alvo)
end
function M.gone(self) return self.state.alvo.name end
function M.vanish(self)
    self:destroy()
    scene.get("Testemunha").attr.Resultado = "depois"
    self.attr.Resultado = "nunca"
    scene.get("Testemunha").attr.Resultado = "nunca"
end
return M
"#;
    let mut rt = run(&project, &[(PATH, source)]);
    assert_eq!(call(&mut rt, &id, "other"), ScriptCall::Done);
    assert_eq!(result(&rt, &id), "false nil nil");
    assert!(rt.scene().entity(&target_id).is_none());
    let e = failed(call(&mut rt, &id, "gone"));
    assert_eq!(
        (e.message.as_str(), e.line),
        ("o objeto não existe mais", 9)
    );
    // Its own removal: what came before stays, the next use of self ends the call.
    let from = rt.logs_written();
    assert_eq!(call(&mut rt, &id, "vanish"), ScriptCall::Done);
    assert!(rt.scene().entity(&id).is_none());
    assert_eq!(result(&rt, &witness_id), "depois");
    assert!(lines_since(&rt, from).is_empty(), "{:?}", rt.logs);
}

#[test]
fn clones_take_options_start_after_the_call_and_get_the_same_ids_in_every_run() {
    let mut source_object = object("Fonte");
    source_object
        .attributes
        .insert("Copia".into(), Value::Bool(false));
    source_object.transform.position = [1., 0., 0.];
    let piece = child("Peça", &source_object);
    let group = Entity::new("Grupo", None);
    let id = source_object.id.clone();
    let mut project = Project::new("Cópias");
    project.scenes[0]
        .entities
        .extend([source_object, piece, group]);
    let source = r#"local M = {}
M.state = { feitos = 0 }
function M.on_start(self)
    if self.attr.Copia then
        print("cópia começou", self.name, self.state.feitos)
    end
end
function M.make(self)
    local a = self:clone({ offset = vector.create(2, 0, 0), attributes = { Copia = true } })
    local b = self:clone({ position = vector.create(0, 5, 0), rotation = vector.create(0, 90, 0),
        parent = scene.get("Grupo"), attributes = { Copia = true } })
    self.state.feitos += 2
    print("feito")
    self.attr.Resultado = a.name .. " " .. b.parent.name .. " " .. #a:children() .. " "
        .. tostring(a.position) .. " " .. tostring(b.position) .. " " .. a.id .. " " .. b.id
end
function M.bad_attribute(self) self:clone({ attributes = { Nada = 1 } }) end
function M.bad_type(self) self:clone({ attributes = { Copia = 1 } }) end
function M.bad_option(self) self:clone({ ofset = vector.zero }) end
function M.both(self) self:clone({ offset = vector.zero, position = vector.zero }) end
function M.table_key(self) self:clone({ [{}] = 1 }) end
function M.list(self) self:clone({ vector.zero }) end
return M
"#;
    // The copies' on_start runs once the call ended, in the logic phase that follows.
    let made = || {
        let mut rt = run(&project, &[(PATH, source)]);
        let from = rt.logs_written();
        assert_eq!(call(&mut rt, &id, "make"), ScriptCall::Done);
        step(&mut rt, 1);
        (rt, from)
    };
    let (mut rt, from) = made();
    let text = result(&rt, &id);
    let parts: Vec<&str> = text.split(' ').collect();
    assert_eq!(
        &parts[..8],
        ["Fonte", "(cópia)", "Grupo", "1", "3,", "0,", "0", "0,"],
        "{text}"
    );
    // The copies' on_start ran once the call ended, with their memory as declared.
    assert_eq!(
        lines_since(&rt, from),
        [
            "Fonte · feito",
            "Fonte (cópia) · cópia começou Fonte (cópia) 0",
            "Fonte (cópia) · cópia começou Fonte (cópia) 0",
        ]
    );
    let b = rt.scene().entity(parts[parts.len() - 1]).unwrap();
    assert_eq!(b.transform.position, [0., 5., 0.]);
    assert!((b.transform.rotation[1] - 90f32.to_radians()).abs() < 1e-6);
    // Another run makes the same copies with the same IDs.
    let (again, _) = made();
    assert_eq!(result(&again, &id), text);
    let objects = rt.scene().entities.len();
    for (function, message) in [
        (
            "bad_attribute",
            "clone: o atributo Nada não existe em Fonte (cópia); crie-o nele ou declare-o em \
             Modulo.attributes de um script que ele anexa",
        ),
        (
            "bad_type",
            "clone: o atributo Copia guarda booleano, não número",
        ),
        (
            "bad_option",
            "clone não conhece a opção ofset; use offset, position, rotation, parent, \
             attributes",
        ),
        ("both", "clone: use offset ou position, não os dois"),
        // A key that is no text is named by its kind, never by its address.
        (
            "table_key",
            "as opções de clone têm nomes de texto, não tabela",
        ),
        ("list", "as opções de clone têm nomes de texto, não número"),
    ] {
        let e = failed(call(&mut rt, &id, function));
        assert_eq!(e.message, message, "{function}");
    }
    // A refused option makes nothing.
    assert_eq!(rt.scene().entities.len(), objects);
}

#[test]
fn first_hit_counts_each_object_once_per_activation_of_the_area() {
    let mut area = object("Área");
    area.collider = Some(Collider {
        is_trigger: true,
        ..Default::default()
    });
    let mut crate_ = object("Caixa");
    crate_.collider = Some(Collider::default());
    let (area_id, crate_id) = (area.id.clone(), crate_.id.clone());
    let mut project = Project::new("Acertos");
    project.scenes[0].entities.extend([area, crate_]);
    let source = r#"local M = {}
function M.on_area_enter(self, other)
    local first, second = self:first_hit(other), self:first_hit(other)
    self.attr.Resultado ..= tostring(first) .. tostring(second) .. " "
end
function M.hit(self, other) self.attr.Resultado ..= tostring(self:first_hit(other)) .. " " end
function M.not_area(self) return scene.get("Caixa"):first_hit(self) end
return M
"#;
    let mut rt = run(&project, &[(PATH, source)]);
    step(&mut rt, 2);
    assert_eq!(result(&rt, &area_id), "truefalse ");
    let other = [Value::Object(Some(crate_id.clone()))];
    assert_eq!(
        rt.call_script(&area_id, PATH, "hit", &other),
        ScriptCall::Done
    );
    assert_eq!(result(&rt, &area_id), "truefalse false ");
    // Turned off and on, the area starts a new activation: the box is hit afresh.
    rt.set_component_enabled(&area_id, ObjectComponent::Collider, false)
        .unwrap();
    rt.set_component_enabled(&area_id, ObjectComponent::Collider, true)
        .unwrap();
    step(&mut rt, 2);
    assert_eq!(result(&rt, &area_id), "truefalse false truefalse ");
    let e = failed(call(&mut rt, &area_id, "not_area"));
    assert_eq!(
        e.message,
        "Caixa não é uma área: first_hit conta os acertos de cada ativação de uma área (um \
         colisor marcado como área)"
    );
}

/// `from.from_port → to.to_port` in `e`'s graph.
fn wire(e: &mut Entity, from: &Node, from_port: &str, to: &Node, to_port: &str) {
    e.graph.edges.push(Edge {
        from_node: from.id.clone(),
        from_port: from_port.into(),
        to_node: to.id.clone(),
        to_port: to_port.into(),
    });
}

#[test]
fn first_hit_shares_the_graphs_hits_and_its_events_activation() {
    // The area's graph damages whoever enters, once per activation, and turns the area off;
    // its script asks first_hit in the same event, after the graph.
    let mut area = object("Área");
    area.collider = Some(Collider {
        is_trigger: true,
        ..Default::default()
    });
    let enter = Node::new("event.area_enter", [0.; 2]);
    let damage = Node::new("action.damage", [0.; 2]);
    let mut off = Node::new("action.component", [0.; 2]);
    off.params
        .insert("component".into(), Value::Text("collider".into()));
    off.params.insert("enabled".into(), Value::Bool(false));
    wire(&mut area, &enter, "exec", &damage, "exec");
    wire(&mut area, &enter, "context", &damage, "target");
    wire(&mut area, &damage, "exec", &off, "exec");
    area.graph.nodes = vec![enter, damage, off];
    let mut target = Entity::new("Alvo", None);
    target.collider = Some(Collider::default());
    target.attributes.insert("Vida".into(), Value::Number(100.));
    let witness = Entity::new("Testemunha", None);
    let (area_id, target_id) = (area.id.clone(), target.id.clone());
    let mut project = Project::new("Acertos juntos");
    project.scenes[0].entities.extend([area, target, witness]);
    let source = r#"local M = {}
function M.on_area_enter(self, other)
    local testemunha = scene.get("Testemunha")
    self.attr.Resultado ..= tostring(self:first_hit(other)) .. tostring(self:first_hit(testemunha))
        .. tostring(self:first_hit(testemunha)) .. " "
end
function M.later(self, other) self.attr.Resultado ..= tostring(self:first_hit(other)) .. " " end
return M
"#;
    let mut rt = run(&project, &[(PATH, source)]);
    step(&mut rt, 2);
    // The graph hit the target first; the area went off, and the script still counts in the
    // event's activation.
    assert_eq!(result(&rt, &area_id), "falsetruefalse ");
    assert_eq!(attribute(&rt, &target_id, "Vida"), Value::Number(90.));
    let area_on = |rt: &Runtime| {
        rt.scene()
            .entity(&area_id)
            .and_then(|e| e.collider.as_ref())
            .is_some_and(|c| c.enabled)
    };
    assert!(!area_on(&rt));
    // Out of the event, an area that is off hits nothing.
    let other = [Value::Object(Some(target_id.clone()))];
    assert_eq!(
        rt.call_script(&area_id, PATH, "later", &other),
        ScriptCall::Done
    );
    assert_eq!(result(&rt, &area_id), "falsetruefalse false ");
    // Back on, a new activation: the graph hits again first, the witness is new to it.
    rt.set_component_enabled(&area_id, ObjectComponent::Collider, true)
        .unwrap();
    step(&mut rt, 2);
    assert_eq!(
        result(&rt, &area_id),
        "falsetruefalse false falsetruefalse "
    );
    assert_eq!(attribute(&rt, &target_id, "Vida"), Value::Number(80.));
    assert!(errors(&rt).is_empty(), "{:?}", rt.logs);
}

#[test]
fn the_scene_finds_objects_and_changes_when_the_function_ends() {
    let mut project = Project::new("Cenas");
    project.scenes[0].name = "Pista".into();
    let menu = Scene::new("Menu", SceneKind::TwoD);
    let menu_id = menu.id.clone();
    project.scenes.push(menu);
    let pista_id = project.start_scene.clone();
    let control = object("Controle");
    let mut first = Entity::new("Porta", None);
    first.attributes.insert("Vida".into(), Value::Number(3.));
    let second = Entity::new("Porta", None);
    let id = control.id.clone();
    project.scenes[0].entities.extend([control, first, second]);
    let source = r#"local M = {}
function M.find(self)
    local a = scene.find("Porta")
    self.attr.Resultado = a.attr.Vida .. " " .. #scene.find_all("Porta") .. " "
        .. #scene.with_attribute("Vida") .. " " .. tostring(scene.find("Nada")) .. " "
        .. scene.current() .. " " .. scene.get("Controle").name
end
function M.get(self) return scene.get("Portaa") end
function M.go(self)
    scene.change("menu")
    print("ainda", scene.current())
end
function M.nowhere(self) scene.change("Menuu") end
function M.again(self) scene.restart() end
return M
"#;
    let mut rt = run(&project, &[(PATH, source)]);
    assert_eq!(call(&mut rt, &id, "find"), ScriptCall::Done);
    assert_eq!(result(&rt, &id), "3 2 1 nil Pista Controle");
    let e = failed(call(&mut rt, &id, "get"));
    assert_eq!(
        e.message,
        "a cena Pista não tem um objeto chamado \"Portaa\" (quis dizer Porta?); para quando a \
         ausência é esperada, use scene.find, que dá nil"
    );
    let e = failed(call(&mut rt, &id, "nowhere"));
    assert_eq!(
        e.message,
        "a cena \"Menuu\" não existe no projeto; quis dizer Menu? Cenas: Pista, Menu"
    );
    let starts = rt.scene_starts.len();
    assert_eq!(call(&mut rt, &id, "again"), ScriptCall::Done);
    assert_eq!(rt.scene_starts.len(), starts + 1);
    assert_eq!(rt.scene_id(), pista_id);
    // The rest of the function still runs in the scene it is in.
    let from = rt.logs_written();
    assert_eq!(call(&mut rt, &id, "go"), ScriptCall::Done);
    assert_eq!(lines_since(&rt, from), ["Controle · ainda Pista"]);
    assert_eq!(rt.scene_id(), menu_id);
}

/// A project with the model `Bala` (a root with a child `Ponta`, the root attaching
/// `scripts/bala.luau` and an Objeto attribute `Dono`) and a spawner running `scripts/teste.luau`.
fn models_project() -> (Project, Id) {
    let mut project = Project::new("Modelos");
    project.scenes[0].kind = SceneKind::ThreeD;
    let scene = project.start_scene.clone();
    let mut bullet = Entity::new("Bala", None);
    bullet.attributes.insert("Dono".into(), Value::Object(None));
    bullet.scripts = vec![ScriptRef::new("scripts/bala.luau")];
    let tip = child("Ponta", &bullet);
    let bullet_id = bullet.id.clone();
    let mut cube = Entity::new("Cubo", Some(Primitive::Cube));
    cube.dimensions = [1.; 3];
    let cube_id = cube.id.clone();
    project.scenes[0].entities.extend([bullet, tip, cube]);
    project.save_model(&scene, &bullet_id, "Bala").unwrap();
    project.save_model(&scene, &cube_id, "Cubo").unwrap();
    project.scenes[0].entities.clear();
    let spawner = object("Lançador");
    let id = spawner.id.clone();
    project.scenes[0].entities.push(spawner);
    (project, id)
}

const BULLET: &str = r#"local M = {}
function M.on_start(self)
    print("surgiu", self.name, self.attr.Dono.name, #self:children())
end
return M
"#;

#[test]
fn models_spawn_with_their_scripts_and_the_same_ids_in_every_run() {
    let (mut project, id) = models_project();
    let source = r#"local M = {}
function M.fire(self)
    local bala = scene.spawn_model("bala", { position = vector.create(1, 2, 3),
        attributes = { Dono = self } })
    print("lançou")
    self.attr.Resultado = bala.name .. " " .. tostring(bala.position) .. " " .. bala.id
end
function M.unknown(self) scene.spawn_model("Balaa") end
function M.wrong_kind(self) scene.spawn_model("Cubo") end
function M.bad_attribute(self) scene.spawn_model("Bala", { attributes = { Vida = 1 } }) end
function M.offset(self) scene.spawn_model("Bala", { offset = vector.zero }) end
return M
"#;
    let files = [(PATH, source), ("scripts/bala.luau", BULLET)];
    let fired = |project: &Project| {
        let mut rt = run(project, &files);
        let from = rt.logs_written();
        assert_eq!(call(&mut rt, &id, "fire"), ScriptCall::Done);
        step(&mut rt, 1);
        (rt, from)
    };
    let (mut rt, from) = fired(&project);
    let text = result(&rt, &id);
    assert!(text.starts_with("Bala 1, 2, 3 "), "{text}");
    assert_eq!(
        lines_since(&rt, from),
        ["Lançador · lançou", "Bala · surgiu Bala Lançador 1"]
    );
    assert_eq!(result(&fired(&project).0, &id), text);
    let objects = rt.scene().entities.len();
    for (function, message) in [
        (
            "unknown",
            "O modelo \"Balaa\" não existe no projeto; quis dizer Bala? Modelos: Bala, Cubo",
        ),
        (
            "bad_attribute",
            "scene.spawn_model: o atributo Vida não existe em Bala; crie-o nele ou declare-o em \
             Modulo.attributes de um script que ele anexa",
        ),
        (
            "offset",
            "scene.spawn_model não conhece a opção offset; use position, rotation, parent, \
             attributes",
        ),
    ] {
        let e = failed(call(&mut rt, &id, function));
        assert_eq!(e.message, message, "{function}");
    }
    assert_eq!(rt.scene().entities.len(), objects);
    // A model of 3D shapes in a 2D scene.
    project.scenes[0].kind = SceneKind::TwoD;
    let mut rt = run(&project, &files);
    let e = failed(call(&mut rt, &id, "wrong_kind"));
    assert_eq!(e.message, "Cubo é uma forma 3D, e a cena Cena 2D é 2D");
}

#[test]
fn transforms_read_and_write_in_degrees_quaternions_and_directions() {
    let mut project = Project::new("Giros");
    project.scenes[0].kind = SceneKind::ThreeD;
    let mut box_ = object("Caixa");
    box_.transform.rotation = [0., 90f32.to_radians(), 0.];
    let mut runner = object("Corredor");
    runner.character3d = Some(Default::default());
    let (id, runner_id) = (box_.id.clone(), runner.id.clone());
    project.scenes[0].entities.extend([box_, runner]);
    let source = r#"local M = {}
local function r(v: number): number
    return math.round(v * 1000) / 1000 + 0
end
function M.read(self)
    local a, q, s = self.rotation, self.orientation, self.scale
    self.attr.Resultado = string.format("%g,%g,%g %g,%g,%g,%g %g,%g,%g %g", r(a.x), r(a.y), r(a.z),
        r(q.x), r(q.y), r(q.z), r(q.w), s.x, s.y, s.z, r(self.yaw))
end
function M.dirs(self)
    local f = self:to_world_direction(vector.create(0, 0, -1))
    local l = self:to_local_direction(f)
    self.attr.Resultado = string.format("%g,%g,%g %g,%g,%g", r(f.x), r(f.y), r(f.z), r(l.x), r(l.y),
        r(l.z))
end
function M.turn(self) self.rotation = vector.create(10, 20, 30) end
function M.orient(self) self.orientation = quat.from_euler(0, -60, 0) end
function M.grow(self) self.scale = vector.create(2, 3, 4) end
function M.flat(self) self.scale = vector.create(1, 0, 1) end
function M.jump(self) self:jump_to(vector.create(0, 7, 0)) end
function M.wrong_q(self) self.orientation = vector.zero end
return M
"#;
    let mut rt = run(&project, &[(PATH, source)]);
    let transform = |rt: &Runtime| rt.scene().entity(&id).unwrap().transform.clone();
    assert_eq!(call(&mut rt, &id, "read"), ScriptCall::Done);
    assert_eq!(result(&rt, &id), "0,90,0 0,0.707,0,0.707 1,1,1 90");
    assert_eq!(call(&mut rt, &id, "dirs"), ScriptCall::Done);
    assert_eq!(result(&rt, &id), "-1,0,0 0,0,-1");
    assert_eq!(call(&mut rt, &id, "turn"), ScriptCall::Done);
    assert_eq!(
        transform(&rt).rotation,
        [10f32, 20., 30.].map(f32::to_radians)
    );
    assert_eq!(call(&mut rt, &id, "orient"), ScriptCall::Done);
    let rotation = transform(&rt).rotation;
    assert!(
        rotation[0].abs() < 1e-5
            && (rotation[1] + 60f32.to_radians()).abs() < 1e-5
            && rotation[2].abs() < 1e-5,
        "{rotation:?}"
    );
    assert_eq!(call(&mut rt, &id, "grow"), ScriptCall::Done);
    assert_eq!(transform(&rt).scale, [2., 3., 4.]);
    assert_eq!(call(&mut rt, &id, "jump"), ScriptCall::Done);
    assert_eq!(transform(&rt).position, [0., 7., 0.]);
    let e = failed(call(&mut rt, &id, "flat"));
    assert!(
        e.message.starts_with("A escala de Caixa chegaria a zero"),
        "{e}"
    );
    let e = failed(call(&mut rt, &id, "wrong_q"));
    assert_eq!(
        e.message,
        "orientation recebe um quatérnio, como quat.from_euler(0, 90, 0), não vetor"
    );
    // A 3D character turns by its own yaw, as the node refuses.
    let e = failed(call(&mut rt, &runner_id, "turn"));
    assert!(e.message.starts_with("Corredor é um personagem 3D"), "{e}");
}

#[test]
fn vmath_and_quat_compute_as_the_nodes_and_quaternions_never_change() {
    let door = object("Porta");
    let id = door.id.clone();
    let mut project = Project::new("Contas");
    project.scenes[0].entities.push(door);
    let source = r#"local Q = quat.from_euler(0, 90, 0)
local function r(v: number): number
    return math.round(v * 1000) / 1000 + 0
end
local M = {}
M.state = { giro = 0, lista = {} }
function M.vmath(self)
    local a = vmath.lerp(vector.create(0, 0, 0), vector.create(10, 0, 0), 0.25)
    local n = vmath.normalize_or_zero(vector.zero)
    local m = vmath.normalize_or_zero(vector.create(3, 0, 4))
    self.attr.Resultado = string.format("%g %g %g %g,%g %g %g %g", a.x,
        vmath.distance(vector.create(1, 1, 1), vector.create(4, 5, 1)), n.x, m.x, m.z,
        vmath.flat(vector.create(1, 2, 3)).y, vmath.yaw_of(vector.create(-1, 5, 0)),
        vmath.yaw_of(vector.create(0, 1, 0)))
end
function M.quat(self)
    local v = quat.rotate(Q, vector.create(0, 0, -1))
    local w = Q * vector.create(0, 0, -1)
    local e = quat.to_euler(quat.multiply(quat.from_euler(0, 30, 0),
        quat.from_axis_angle(vector.create(0, 1, 0), 30)))
    local i = quat.multiply(Q, quat.inverse(Q))
    local l = quat.to_euler(quat.look_at(vector.create(-1, 0, 0)))
    local s = quat.to_euler(quat.slerp(quat.identity(), Q, 0.5))
    self.attr.Resultado = string.format("%g,%g,%g %s %g %g %g %g %s %s %s", r(v.x), r(v.y),
        r(v.z), tostring(w == v), r(e.y), r(i.w), r(l.y), r(s.y), typeof(Q),
        tostring(quat.identity()), tostring(Q * Q == quat.multiply(Q, Q)))
end
function M.immutable(self) Q.x = 1 end
function M.field(self) return Q.angle end
function M.keep(self) self.state.giro = Q end
function M.keep_list(self) self.state.lista = { Q } end
function M.timer(self) self:after(1, M.vmath, Q) end
function M.not_quat(self) return quat.rotate(vector.zero, vector.zero) end
function M.not_vector(self) return vmath.lerp(1, 2, 3) end
function M.no_axis(self) return quat.from_axis_angle(vector.zero, 90) end
function M.parallel(self) return quat.look_at(vector.create(0, 2, 0)) end
return M
"#;
    let mut rt = run(&project, &[(PATH, source)]);
    assert_eq!(call(&mut rt, &id, "vmath"), ScriptCall::Done);
    assert_eq!(result(&rt, &id), "2.5 5 0 0.6,0.8 0 90 0");
    assert_eq!(call(&mut rt, &id, "quat"), ScriptCall::Done);
    assert_eq!(
        result(&rt, &id),
        "-1,0,0 true 60 1 90 45 Quat quat(0, 0, 0, 1) true"
    );
    for (function, message) in [
        (
            "immutable",
            "um quatérnio não muda; faça outro com as funções de quat (quat.from_euler, \
             quat.multiply…)",
        ),
        (
            "field",
            "Quat não tem 'angle'; um quatérnio tem x, y, z e w",
        ),
        ("keep", "self.state.giro: guarda número, não quatérnio"),
        (
            "keep_list",
            "self.state.lista[1]: não aceita um quatérnio; guarde quat.to_euler(q), um vetor, e \
             refaça-o com quat.from_euler",
        ),
        (
            "timer",
            "userdata (um quatérnio, por exemplo) não pode ser dado de temporizador",
        ),
        (
            "not_quat",
            "quat.rotate: q precisa ser um quatérnio, como quat.identity(), não vetor",
        ),
        (
            "not_vector",
            "vmath.lerp: a precisa ser um vetor, como vector.create(0, 1, 0), não número",
        ),
        (
            "no_axis",
            "quat.from_axis_angle: axis precisa de uma direção (um vetor que não é zero)",
        ),
        (
            "parallel",
            "quat.look_at: forward e up não podem ter a mesma direção",
        ),
    ] {
        let e = failed(call(&mut rt, &id, function));
        assert!(e.message.starts_with(message), "{function}: {e}");
        assert!(e.line > 1, "{function}: {e}");
    }
}

#[test]
fn surfaces_objects_and_vetor2_declare_attributes_and_memory_fields() {
    let mut project = Project::new("Marcadores");
    let ice = SurfaceMaterial::preset(SurfacePreset::Ice);
    let ice_id = ice.id.clone();
    project.surfaces.push(ice);
    project
        .surfaces
        .push(SurfaceMaterial::preset(SurfacePreset::Mud));
    let mut door = Entity::new("Porta", None);
    door.attributes
        .insert("Resultado".into(), Value::Text(String::new()));
    let other = Entity::new("Outro", None);
    let mut granite = Entity::new("Granito", None);
    granite
        .attributes
        .insert("Resultado".into(), Value::Text(String::new()));
    let (id, granite_id) = (door.id.clone(), granite.id.clone());
    project.scenes[0].entities.extend([door, other, granite]);
    let source = r#"local M = {}
M.attributes = { Piso = param.surface("gelo"), Padrao = param.surface(), Alvo = param.object(),
    Tamanho = param.vector2(1, 2) }
M.state = { alvo = param.object(), piso = param.surface(), tamanho = param.vector2(3, 4) }
function M.read(self)
    local p = self.attr.Piso
    self.attr.Resultado = p.name .. " " .. tostring(p) .. " " .. tostring(self.attr.Padrao) .. " "
        .. tostring(self.state.alvo) .. " " .. tostring(self.state.piso) .. " "
        .. tostring(self.state.tamanho)
end
function M.write(self)
    self.state.alvo = scene.get("Outro")
    self.state.piso = self.attr.Piso
    self.state.tamanho = vector.create(5, 6, 7)
    self.attr.Padrao = self.attr.Piso
    self.attr.Piso = nil
end
function M.read_again(self)
    self.attr.Resultado = self.state.alvo.name .. " " .. self.state.piso.name .. " "
        .. tostring(self.state.tamanho) .. " " .. self.attr.Padrao.name .. " "
        .. tostring(self.attr.Piso)
end
function M.clear(self)
    self.state.alvo = nil
    self.state.piso = nil
end
function M.wrong(self) self.state.tamanho = nil end
function M.surface_field(self) return self.attr.Padrao.friction end
function M.pass(self)
    self:after(0, M.got, self.attr.Padrao)
    self:send("piso", { self.attr.Padrao })
end
function M.got(self, piso) print("temporizador", piso.name, piso == self.attr.Padrao) end
function M.on_message(self, name, value) print(name, value[1].name) end
return M
"#;
    let wrong_surface =
        "local M = {}\nM.attributes = { Piso = param.surface('Granito') }\nreturn M\n";
    let files = [(PATH, source), ("scripts/granito.luau", wrong_surface)];
    let sources = ScriptSources::from_texts(files);
    let scene = project.start_scene.clone();
    let attach = |entity: &Id, path: &str| Command::AttachScript {
        scene: None,
        entity: entity.clone(),
        path: path.into(),
        index: None,
    };
    let applied = command::apply_with_scripts(
        &mut project,
        &scene,
        &[
            attach(&id, PATH),
            attach(&granite_id, "scripts/granito.luau"),
        ],
        &sources,
    )
    .unwrap();
    let warnings: Vec<String> = applied.into_iter().flat_map(|a| a.warnings).collect();
    // The lint of names points at the line too.
    assert_eq!(
        warnings,
        [
            "Comando 1 (attach_script, o 2º do lote): Granito: os atributos que \
             scripts/granito.luau declara não foram criados (scripts/granito.luau: \
             Modulo.attributes.Piso: param.surface(\"Granito\") nomeia uma superfície que o \
             projeto não tem. Superfícies: Gelo, Lama); corrija o script e anexe-o de novo para \
             criá-los",
            "Comando 1 (attach_script, o 2º do lote): scripts/granito.luau:2: param.surface: a \
             superfície \"Granito\" não existe no projeto. Superfícies: Gelo, Lama"
        ]
    );
    let attributes = &project.scenes[0].entity(&id).unwrap().attributes;
    assert_eq!(attributes["Piso"], Value::Surface(Some(ice_id.clone())));
    assert_eq!(attributes["Padrao"], Value::Surface(None));
    assert_eq!(attributes["Alvo"], Value::Object(None));
    assert_eq!(attributes["Tamanho"], Value::Vector2([1., 2.]));
    let mut rt = run(&project, &files);
    assert_eq!(call(&mut rt, &id, "read"), ScriptCall::Done);
    assert_eq!(result(&rt, &id), "Gelo Gelo nil nil nil 3, 4, 0");
    assert_eq!(call(&mut rt, &id, "write"), ScriptCall::Done);
    assert_eq!(call(&mut rt, &id, "read_again"), ScriptCall::Done);
    assert_eq!(result(&rt, &id), "Outro Gelo 5, 6, 0 Gelo nil");
    assert_eq!(attribute(&rt, &id, "Padrao"), Value::Surface(Some(ice_id)));
    assert_eq!(call(&mut rt, &id, "clear"), ScriptCall::Done);
    let memory = rt.script_memory_text(&id, PATH).unwrap();
    assert_eq!(
        memory,
        [
            ("alvo".to_owned(), "nil".to_owned()),
            ("piso".to_owned(), "nil".to_owned()),
            ("tamanho".to_owned(), "vector(5, 6, 0)".to_owned()),
        ]
    );
    let e = failed(call(&mut rt, &id, "wrong"));
    assert_eq!(e.message, "self.state.tamanho: guarda vetor 2D, não nil");
    let e = failed(call(&mut rt, &id, "surface_field"));
    assert!(
        e.message
            .starts_with("Surface não tem 'friction'; uma superfície só tem name"),
        "{e}"
    );
    // A surface goes as a timer's data and in a message, and comes back as the same value.
    let from = rt.logs_written();
    assert_eq!(call(&mut rt, &id, "pass"), ScriptCall::Done);
    step(&mut rt, 2);
    assert_eq!(
        lines_since(&rt, from),
        ["Porta · temporizador Gelo true", "Porta · piso Gelo"]
    );
}

#[test]
fn character_events_pass_the_surfaces_by_name() {
    let mut project = Project::new("Pisos");
    let ice = SurfaceMaterial::preset(SurfacePreset::Ice);
    let mud = SurfaceMaterial::preset(SurfacePreset::Mud);
    let (ice_id, mud_id) = (ice.id.clone(), mud.id.clone());
    project.surfaces.extend([ice, mud]);
    let scene = &mut project.scenes[0];
    scene.kind = SceneKind::ThreeD;
    let mut floor = Entity::new("Chão", Some(Primitive::Cube));
    floor.dimensions = [40., 1., 40.];
    floor.transform.position = [0., -0.5, 0.];
    floor.physics3d = Some(Collider3d {
        shape: CollisionShape::Box {
            size: floor.dimensions,
        },
        surface: Some(ice_id),
        ..Default::default()
    });
    let floor_id = floor.id.clone();
    let mut body = object("Corredor");
    body.character3d = Some(CharacterConfig::default());
    body.transform.position[1] = 2.;
    scene.entities.extend([floor, body]);
    let source = r#"local M = {}
function M.on_character(self, event)
    if event.kind == "surface" then
        print(event.surface.name, tostring(event.previous_surface))
        if event.previous_surface then
            print("antes", event.previous_surface.name, event.previous_surface == event.surface)
        end
    end
end
return M
"#;
    let mut rt = run(&project, &[(PATH, source)]);
    step(&mut rt, 120);
    assert_eq!(
        lines_since(&rt, 0),
        ["Corredor · Gelo nil"],
        "{:?}",
        rt.logs
    );
    // The floor turns to mud under the character: the event names both surfaces.
    let from = rt.logs_written();
    rt.apply_surface(&floor_id, Some(mud_id)).unwrap();
    step(&mut rt, 2);
    assert_eq!(
        lines_since(&rt, from),
        ["Corredor · Lama Gelo", "Corredor · antes Gelo false"]
    );
    assert!(errors(&rt).is_empty(), "{:?}", rt.logs);
}

#[test]
fn an_error_about_nil_names_the_attribute_the_call_read_as_nil() {
    let mut door = object("Porta");
    door.attributes.insert("Alvo".into(), Value::Object(None));
    let handle = child("Maçaneta", &door);
    let other = Entity::new("Outro", None);
    let (id, other_id) = (door.id.clone(), other.id.clone());
    let mut project = Project::new("Nil");
    project.scenes[0].entities.extend([door, handle, other]);
    let source = r#"local M = {}
function M.compare(self, other)
    local ordem = other.attr["Ordem de chegada"]
    if ordem < 3 then
        print("antes")
    end
end
function M.empty(self) self.attr.Alvo.visible = false end
function M.unrelated(self) return nil + 1 end
function M.accented(self) return self.attr["Posição"] + 1 end
function M.refused(self) self.visible = self.attr.Mostrar end
function M.hint_only(self)
    if self.attr.Debug then
        print("depuração")
    end
    return self:get_child("Nada")
end
function M.scene_hint(self)
    local modo = self.attr.Debug
    return scene.get("Nada"), modo
end
return M
"#;
    let mut rt = run(&project, &[(PATH, source)]);
    let other = [Value::Object(Some(other_id))];
    let e = failed(rt.call_script(&id, PATH, "compare", &other));
    assert_eq!(
        (e.line, e.message.as_str()),
        (
            4,
            "tentou comparar um valor nil < um número (a linha 3 leu Outro.attr[\"Ordem de \
             chegada\"], que Outro não tem: um atributo ausente dá nil; confira com has_attribute \
             ou declare-o em Modulo.attributes)"
        )
    );
    let e = failed(call(&mut rt, &id, "empty"));
    assert_eq!(
        e.message,
        "tentou ler 'visible' de um valor nil (esta linha leu Porta.attr.Alvo, um atributo vazio, \
         que dá nil: escolha o que ele aponta nas Propriedades ou com o set_attribute do MCP)"
    );
    // A call that read no attribute as nil keeps Luau's error as it is.
    let e = failed(call(&mut rt, &id, "unrelated"));
    assert_eq!(
        e.message,
        "tentou fazer uma conta (add) com um valor nil e um número"
    );
    // A name that is no Luau identifier is written between brackets.
    let e = failed(call(&mut rt, &id, "accented"));
    assert_eq!(
        e.message,
        "tentou fazer uma conta (add) com um valor nil e um número (esta linha leu \
         Porta.attr[\"Posição\"], que Porta não tem: um atributo ausente dá nil; confira com \
         has_attribute ou declare-o em Modulo.attributes)"
    );
    // The API refusing a nil names it too.
    let e = failed(call(&mut rt, &id, "refused"));
    assert_eq!(
        e.message,
        "visible recebe true ou false, não nil (esta linha leu Porta.attr.Mostrar, que Porta não \
         tem: um atributo ausente dá nil; confira com has_attribute ou declare-o em \
         Modulo.attributes)"
    );
    // An error that mentions nil only in its advice keeps its text, whatever the call read.
    let e = failed(call(&mut rt, &id, "hint_only"));
    assert_eq!(
        e.message,
        "Porta não tem um filho chamado \"Nada\"; filhos: Maçaneta; para quando a ausência é \
         esperada, use find_child, que dá nil"
    );
    let e = failed(call(&mut rt, &id, "scene_hint"));
    assert!(e.message.ends_with("use scene.find, que dá nil"), "{e}");
}

#[test]
fn a_replay_with_copies_models_and_removals_stays_identical() {
    let (mut project, _) = models_project();
    project.scenes[0].entities[0].scripts = vec![ScriptRef::new("scripts/lancador.luau")];
    let spawner = r#"local M = {}
M.state = { feitos = 0 }
function M.on_start(self)
    self:every(0.25, M.fire)
end
function M.fire(self)
    self.state.feitos += 1
    local bala = scene.spawn_model("Bala", { position = vector.create(self.state.feitos, 0, 0),
        attributes = { Dono = self } })
    if self.state.feitos % 2 == 0 then
        bala:clone({ offset = vector.create(0, 1, 0) })
    end
end
return M
"#;
    let bullet = r#"local M = {}
function M.on_start(self)
    self:after(0.5, M.go)
end
function M.go(self)
    self:destroy()
end
return M
"#;
    let scripts = Arc::new(ScriptSources::from_texts([
        ("scripts/lancador.luau", spawner),
        ("scripts/bala.luau", bullet),
    ]));
    let request: SimulationRequest = serde_json::from_value(json!({
        "seconds": 3, "check_replay": true, "actions": "none", "ui": false
    }))
    .unwrap();
    let simulate = || headless::run_with_scripts(&project, scripts.clone(), &request).unwrap();
    let (report, rt) = simulate();
    let replay = report.replay.as_ref().unwrap();
    assert!(replay.identical, "{replay:?}");
    assert_eq!(report.state_hash, simulate().0.state_hash);
    // The bullets and their copies go by their own script, half a second after they appear.
    assert!(errors(&rt).is_empty(), "{:?}", rt.logs);
    let bullets = rt
        .scene()
        .entities
        .iter()
        .filter(|e| e.name.starts_with("Bala"))
        .count();
    assert!(bullets > 0 && bullets < 8, "{bullets}");
}

/// The countdown of the parkour in Luau, hiding the panel of the countdown with `obj.visible`
/// through an Objeto attribute (design §5.8): the graph of 3 nodes the vertical slice kept for
/// it is no longer needed.
const COUNTDOWN: &str = r#"--!strict
local Contagem = {}
Contagem.attributes = { Aviso = "", Painel = param.object() }

function Contagem.on_start(self: Object)
    vars.set("Largada liberada", false)
    Contagem.show(self, 3)
    self:sequence(1, Contagem.show, { 3, 2, 1, 0 })
end

function Contagem.show(self: Object, n: number)
    vars.set("Contagem da largada", n)
    if n > 0 then
        self.attr.Aviso = tostring(n)
    else
        self.attr.Aviso = "Já!"
        vars.set("Largada liberada", true)
        self:after(1, Contagem.hide)
    end
end

function Contagem.hide(self: Object)
    local painel = self.attr.Painel
    if painel then
        painel.visible = false
    end
    self.attr.Aviso = ""
end

return Contagem
"#;

#[test]
fn the_parkour_countdown_hides_its_panel_by_itself_step_for_step_with_the_graph() {
    let file = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/parkour")
        .join(persistence::PROJECT_FILE);
    let graph = persistence::load_project(&file).unwrap();
    let pista = graph
        .scenes
        .iter()
        .find(|s| s.name == "Pista")
        .unwrap()
        .id
        .clone();
    let find = |project: &Project, name: &str| {
        project
            .scene(&pista)
            .unwrap()
            .entities
            .iter()
            .find(|e| e.name == name)
            .unwrap()
            .id
            .clone()
    };
    let (control, panel) = (
        find(&graph, "Controle da corrida"),
        find(&graph, "Contagem"),
    );
    // The fixture: the graph of the countdown goes, the script comes, attached as an agent
    // attaches it, and its panel is picked.
    let mut scripted = graph.clone();
    scripted
        .scene_mut(&pista)
        .unwrap()
        .entity_mut(&control)
        .unwrap()
        .graph = Default::default();
    let files = [("scripts/contagem.luau", COUNTDOWN)];
    let sources = ScriptSources::from_texts(files);
    command::apply_with_scripts(
        &mut scripted,
        &pista,
        &[
            Command::AttachScript {
                scene: None,
                entity: control.clone(),
                path: "scripts/contagem.luau".into(),
                index: None,
            },
            Command::SetAttribute {
                scene: None,
                entity: control.clone(),
                name: "Painel".into(),
                value: Some(ParamValue::Tagged(Value::Object(Some(panel.clone())))),
            },
        ],
        &sources,
    )
    .unwrap();
    let start = |project: &Project, files: &[(&str, &str)]| {
        let options = RuntimeOptions {
            scripts: Arc::new(ScriptSources::from_texts(files.iter().copied())),
            ..Default::default()
        };
        Runtime::with_options(project, &pista, options).unwrap()
    };
    let mut by_graph = start(&graph, &[]);
    let mut by_script = start(&scripted, &files);
    let start_var = variables::find_named(&graph, "Largada liberada")
        .unwrap()
        .unwrap();
    let count_var = variables::find_named(&graph, "Contagem da largada")
        .unwrap()
        .unwrap();
    // Each step: the panel shown or not, the text of the countdown and the variables the
    // player reads.
    let seen = |rt: &Runtime| {
        (
            rt.scene().entity(&panel).unwrap().visible,
            attribute(rt, &control, "Aviso"),
            rt.variable(&start_var, "").cloned(),
            rt.variable(&count_var, "").cloned(),
        )
    };
    let mut hidden_at = None;
    for n in 0..=300 {
        if n > 0 {
            step(&mut by_graph, 1);
            step(&mut by_script, 1);
        }
        assert_eq!(seen(&by_script), seen(&by_graph), "passo {n}");
        if hidden_at.is_none() && !seen(&by_script).0 {
            hidden_at = Some(n);
        }
    }
    assert_eq!(hidden_at, Some(240));
    assert!(errors(&by_script).is_empty(), "{:?}", by_script.logs);
    // The sounds the player plays from the variables come out alike.
    let sounds =
        |rt: &Runtime| -> Vec<String> { rt.sounds.iter().map(|s| s.asset.clone()).collect() };
    assert_eq!(sounds(&by_script), sounds(&by_graph));
}
