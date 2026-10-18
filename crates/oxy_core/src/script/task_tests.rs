//! Scripts as tasks of the runtime, seen from inside the crate: what the engine keeps for them
//! (draws, copies, stopped scripts, deferred calls) and what timers refuse.
use super::host::{LIMITS, Limits, Shared, api};
use super::*;
use crate::document::{Entity, Id, Project, Value};
use crate::runtime::{FIXED_DT, InputFrame, Runtime, RuntimeOptions};
use mlua::{LightUserData, Lua};
use std::rc::Rc;
use std::sync::Arc;

const PATH: &str = "scripts/teste.luau";

fn sources(files: &[(&str, &str)]) -> Arc<ScriptSources> {
    Arc::new(ScriptSources::from_texts(files.iter().copied()))
}

/// An object with a text attribute `Resultado` and a number `Contagem` that runs `scripts`.
fn object(name: &str, scripts: &[&str]) -> Entity {
    let mut e = Entity::new(name, None);
    e.attributes
        .insert("Resultado".into(), Value::Text(String::new()));
    e.attributes.insert("Contagem".into(), Value::Number(0.));
    e.scripts = scripts.iter().copied().map(ScriptRef::new).collect();
    e
}

fn start_seeded(objects: Vec<Entity>, files: &[(&str, &str)], seed: u64) -> Runtime {
    let mut project = Project::new("Tarefas");
    project.scenes[0].entities = objects;
    let scene = project.start_scene.clone();
    let options = RuntimeOptions {
        scripts: sources(files),
        seed,
        ..Default::default()
    };
    Runtime::with_options(&project, &scene, options).unwrap()
}

fn start(objects: Vec<Entity>, files: &[(&str, &str)]) -> Runtime {
    start_seeded(objects, files, 0)
}

fn step(rt: &mut Runtime, steps: usize) {
    for _ in 0..steps {
        rt.advance(FIXED_DT, &InputFrame::default());
    }
}

fn attribute(rt: &Runtime, id: &str, name: &str) -> Value {
    rt.scene().entity(id).unwrap().attributes[name].clone()
}

fn failed(call: ScriptCall) -> ScriptError {
    match call {
        ScriptCall::Failed(error) => error,
        other => panic!("esperava um erro, veio {other:?}"),
    }
}

/// A host whose `test` table reaches the engine's draws and copies for the running script.
fn engine_host(files: &[(&str, &str)]) -> ScriptHost {
    ScriptHost::with_extra(sources(files), &|lua: &Lua, base, shared: &Rc<Shared>| {
        let test = lua.create_table()?;
        let s = shared.clone();
        test.raw_set(
            "roll",
            lua.create_function(move |lua, (stream, min, max): (String, f64, f64)| {
                api(lua, &s, "test.roll", |runtime| {
                    let owner = s.caller().unwrap();
                    let script = s.module().unwrap().path.clone();
                    runtime.script_random_number(&owner, &script, &stream, min, max, true)
                })
            })?,
        )?;
        let s = shared.clone();
        test.raw_set(
            "copy",
            lua.create_function(move |lua, template: LightUserData| {
                api(lua, &s, "test.copy", |runtime| {
                    let owner = s.caller().unwrap();
                    let script = s.module().unwrap().path.clone();
                    let template = s.id(handles::decode(template.0).1).unwrap();
                    let copy = runtime.script_spawn_copy(
                        &template,
                        glam::Vec3::new(2., 0., 0.),
                        &owner,
                        &script,
                    )?;
                    Ok(runtime.entity(&copy).unwrap().name.clone())
                })
            })?,
        )?;
        base.raw_set("test", test)
    })
    .unwrap()
}

fn use_engine_host(rt: &mut Runtime, files: &[(&str, &str)]) {
    let host = engine_host(files);
    host.begin_scene(rt.scene().entities.iter().map(|e| e.id.as_str()));
    rt.replace_script_host(host);
}

#[test]
fn scripts_draw_apart_from_the_graph_each_other_and_their_streams() {
    let a = "local M = {}\nfunction M.draw(self, stream)\n    local n = test.roll(stream, 1, \
             1000000)\n    self.attr.Resultado ..= tostring(n) .. ' '\nend\nreturn M\n";
    let b = "local M = {}\nfunction M.draw(self)\n    test.roll('', 1, 1000000)\nend\nreturn M\n";
    let files = [("scripts/a.luau", a), ("scripts/b.luau", b)];
    let run = |between: bool, seed: u64| {
        let mut e = object("Dado", &["scripts/a.luau", "scripts/b.luau"]);
        e.id = "dado".into();
        let id = e.id.clone();
        let mut rt = start_seeded(vec![e], &files, seed);
        use_engine_host(&mut rt, &files);
        let text = Value::Text(String::new());
        let draw = |rt: &mut Runtime, stream: &str| {
            let call = rt.call_script(&id, "scripts/a.luau", "draw", &[Value::Text(stream.into())]);
            assert_eq!(call, ScriptCall::Done);
        };
        draw(&mut rt, "");
        if between {
            assert_eq!(
                rt.call_script(&id, "scripts/b.luau", "draw", &[]),
                ScriptCall::Done
            );
            draw(&mut rt, "outra");
        }
        draw(&mut rt, "");
        let drawn = attribute(&rt, &id, "Resultado");
        assert_ne!(drawn, text);
        (rt, id, drawn)
    };
    let (rt, id, alone) = run(false, 7);
    // The same seed gives the same numbers; another script's draws and another stream's leave
    // the sequence of `scripts/a.luau` as it was.
    assert_eq!(run(false, 7).2, alone);
    let (busy, _, mixed) = run(true, 7);
    let Value::Text(mixed) = mixed else { panic!() };
    let words: Vec<&str> = mixed.split_whitespace().collect();
    assert_eq!(
        Value::Text(format!("{} {} ", words[0], words[2])),
        alone,
        "{mixed}"
    );
    assert_ne!(run(false, 8).2, alone);
    let state = busy.run_state_debug();
    assert!(state.contains("rolls: {}"), "{state}");
    assert!(
        state.contains(&format!(
            "script_rolls: {{(\"{id}\", \"scripts/a.luau\", \"\"): 2, (\"{id}\", \
             \"scripts/a.luau\", \"outra\"): 1, (\"{id}\", \"scripts/b.luau\", \"\"): 1}}"
        )),
        "{state}"
    );
    // The draws are in the state's random part, and a new seed starts the counts over.
    let mut rt = rt;
    let before = rt.state_digest();
    rt.set_random_seed(3.).unwrap();
    assert!(rt.run_state_debug().contains("script_rolls: {}"));
    let difference = before.first_difference(&rt.state_digest()).unwrap();
    assert_eq!(
        (difference.object, difference.part.as_str()),
        (None, "random")
    );
}

#[test]
fn scripts_copy_with_ids_of_their_own_and_copies_start_their_scripts() {
    let factory = "local M = {}\nfunction M.copy(self)\n    self.attr.Resultado = \
                   test.copy(self.attr.Molde)\nend\nreturn M\n";
    let mould = "local M = {}\nfunction M.on_start(self)\n    self.attr.Contagem += 1\nend\n\
                 return M\n";
    let files = [
        ("scripts/fabrica.luau", factory),
        ("scripts/molde.luau", mould),
    ];
    let run = || {
        let mut maker = object("Fábrica", &["scripts/fabrica.luau"]);
        let mut template = object("Molde", &["scripts/molde.luau"]);
        (maker.id, template.id) = ("fabrica".into(), "molde".into());
        maker
            .attributes
            .insert("Molde".into(), Value::Object(Some(template.id.clone())));
        let (maker_id, template_id) = (maker.id.clone(), template.id.clone());
        let mut rt = start(vec![maker, template], &files);
        use_engine_host(&mut rt, &files);
        assert_eq!(attribute(&rt, &template_id, "Contagem"), Value::Number(1.));
        for _ in 0..2 {
            assert_eq!(
                rt.call_script(&maker_id, "scripts/fabrica.luau", "copy", &[]),
                ScriptCall::Done
            );
        }
        assert_eq!(
            attribute(&rt, &maker_id, "Resultado"),
            Value::Text("Molde (cópia)".into())
        );
        // The copies' on_start waits for the queue: the next step runs it.
        step(&mut rt, 1);
        let copies: Vec<Id> = rt
            .scene()
            .entities
            .iter()
            .filter(|e| e.name == "Molde (cópia)")
            .map(|e| e.id.clone())
            .collect();
        assert_eq!(copies.len(), 2);
        for copy in &copies {
            // The copy carried the template's count, then its own on_start ran once.
            assert_eq!(attribute(&rt, copy, "Contagem"), Value::Number(2.));
        }
        (rt, maker_id, copies)
    };
    let (rt, maker, copies) = run();
    assert_eq!(run().2, copies, "os IDs das cópias se repetem");
    let state = rt.run_state_debug();
    assert!(state.contains("spawns: {}"), "{state}");
    assert!(
        state.contains(&format!(
            "script_spawns: {{(\"{maker}\", \"scripts/fabrica.luau\"): 2}}"
        )),
        "{state}"
    );
}

#[test]
fn a_limit_stops_the_scripts_tasks_on_its_object_and_spares_the_others() {
    let heavy = "local M = {}\nfunction M.on_start(self)\n    self:every(0.1, M.spend)\nend\n\
                 function M.spend(self)\n    for i = 1, 100 do local _ = self.attr.Contagem \
                 end\nend\nreturn M\n";
    let light = "local M = {}\nfunction M.on_start(self)\n    self:every(0.1, M.tick)\nend\n\
                 function M.tick(self)\n    self.attr.Contagem += 1\nend\nreturn M\n";
    let files = [("scripts/pesado.luau", heavy), ("scripts/leve.luau", light)];
    let e = object("Torre", &["scripts/pesado.luau", "scripts/leve.luau"]);
    let id = e.id.clone();
    let mut rt = start(vec![e], &files);
    rt.script_host().unwrap().set_limits(Limits {
        call_units: 50,
        ..LIMITS
    });
    assert_eq!(
        rt.script_timers(),
        [
            (id.as_str(), "scripts/pesado.luau", 1),
            (id.as_str(), "scripts/leve.luau", 2)
        ]
    );
    step(&mut rt, 7);
    assert_eq!(
        rt.disabled_scripts().collect::<Vec<_>>(),
        [(id.as_str(), "scripts/pesado.luau")]
    );
    assert_eq!(rt.script_timers(), [(id.as_str(), "scripts/leve.luau", 2)]);
    step(&mut rt, 60);
    assert_eq!(attribute(&rt, &id, "Contagem"), Value::Number(11.));
    assert_eq!(rt.script_errors().count(), 1);
}

#[test]
fn the_step_budget_defers_script_tasks_in_their_order() {
    let source = "local M = {}\nfunction M.on_step(self)\n    for i = 1, 19 do local _ = \
                  self.attr.Contagem end\n    self.attr.Contagem += 1\nend\nreturn M\n";
    let objects: Vec<Entity> = ["A", "B", "C"]
        .into_iter()
        .map(|name| object(name, &[PATH]))
        .collect();
    let ids: Vec<Id> = objects.iter().map(|e| e.id.clone()).collect();
    let mut rt = start(objects, &[(PATH, source)]);
    rt.script_host().unwrap().set_limits(Limits {
        step_units: 30,
        ..LIMITS
    });
    let counts = |rt: &Runtime| -> Vec<Value> {
        ids.iter().map(|id| attribute(rt, id, "Contagem")).collect()
    };
    let n = |v: f64| Value::Number(v);
    // 22 units a call: the second starts below 30 and runs whole; the third waits, also in the
    // step's second logic phase, and the next step starts with it.
    step(&mut rt, 1);
    assert_eq!(counts(&rt), [n(1.), n(1.), n(0.)]);
    step(&mut rt, 1);
    assert_eq!(counts(&rt), [n(2.), n(1.), n(1.)]);
    let told: Vec<_> = rt
        .script_errors()
        .map(|(owner, text, count)| (owner.to_owned(), text.to_owned(), count))
        .collect();
    assert_eq!(
        told,
        [(
            String::new(),
            "Limite de trabalho dos scripts no passo atingido; demais tarefas retomam no \
             próximo passo."
                .to_owned(),
            4
        )]
    );
    assert_eq!(rt.disabled_scripts().count(), 0);
}

#[test]
fn timers_refuse_what_they_cannot_keep_at_the_line_that_asks() {
    let source = r#"local M = {}
function M.target(self) end
function M.made(self) self:after(1, function() end) end
function M.table(self) self:after(1, M.target, {}) end
function M.func(self) self:after(1, M.target, M.target) end
function M.negative(self) self:after(-1, M.target) end
function M.nan(self) self:after(0/0, M.target) end
function M.text(self) self:after("1", M.target) end
function M.other(self) self.attr.Outro:after(1, M.target) end
function M.items(self) self:sequence(1, M.target, 5) end
function M.step(self) self:schedule({ { 1 } }) end
function M.cancel(self) self:cancel("1") end
function M.attr(self) self:after(1, M.target, self.attr) end
function M.huge(self) self:after(1, M.target, math.huge) end
function M.ok(self) self.attr.Resultado = tostring(self:after(1, M.target, nil, 2, "x", true, vector.create(1, 2, 3), self)) end
return M
"#;
    let mut door = object("Porta", &[PATH]);
    let other = object("Outro", &[]);
    door.attributes
        .insert("Outro".into(), Value::Object(Some(other.id.clone())));
    let id = door.id.clone();
    let mut rt = start(vec![door, other], &[(PATH, source)]);
    for (function, line, message) in [
        (
            "made",
            3,
            "passe uma função da tabela do módulo, como Modulo.contar, não uma criada aqui",
        ),
        ("table", 4, "tabela não pode ser dado de temporizador"),
        ("func", 5, "função não pode ser dado de temporizador"),
        (
            "negative",
            6,
            "o tempo do temporizador deve estar entre 0 e 86400 segundos",
        ),
        (
            "nan",
            7,
            "o tempo do temporizador deve estar entre 0 e 86400 segundos",
        ),
        ("text", 8, "after precisa do tempo em segundos (um número)"),
        (
            "other",
            9,
            "temporizadores são do próprio objeto: use self:after(...)",
        ),
        ("items", 10, "sequence precisa de uma lista de itens"),
        ("step", 11, "schedule precisa de uma função"),
        ("cancel", 12, "cancel precisa do número que after"),
        (
            "attr",
            13,
            "os atributos de um objeto não pode ser dado de temporizador",
        ),
        (
            "huge",
            14,
            "infinito ou NaN não pode ser dado de temporizador",
        ),
    ] {
        let e = failed(rt.call_script(&id, PATH, function, &[]));
        assert!(e.message.contains(message), "{function}: {e}");
        assert_eq!(e.line, line, "{function}: {e}");
    }
    assert!(rt.script_timers().is_empty());
    assert_eq!(rt.call_script(&id, PATH, "ok", &[]), ScriptCall::Done);
    assert_eq!(attribute(&rt, &id, "Resultado"), Value::Text("1".into()));
    assert_eq!(rt.script_timers(), [(id.as_str(), PATH, 1)]);
}

#[test]
fn timers_past_the_queues_room_stop_the_script_and_give_the_room_back() {
    let source = r#"local M = {}
function M.target(self) end
function M.calls(self)
    for i = 1, 10000 do
        pcall(function() self:sequence(1, M.target, { 1, 2 }) end)
    end
    self.attr.Resultado = "passou"
end
function M.text(self)
    self:sequence(1, M.target, table.create(1024, string.rep("x", 4096)))
    self:sequence(1, M.target, { "y" })
end
function M.some(self)
    self:sequence(0.1, M.target, { "a", "b", "c" })
    self.attr.Contagem = self:every(0.1, M.target, "zz")
end
function M.stop(self)
    self:cancel(self.attr.Contagem)
end
return M
"#;
    let objects: Vec<Entity> = ["Laço", "Texto", "Calmo"]
        .into_iter()
        .map(|name| object(name, &[PATH]))
        .collect();
    let ids: Vec<Id> = objects.iter().map(|e| e.id.clone()).collect();
    let mut rt = start(objects, &[(PATH, source)]);
    let limit = "a fila não tem lugar para mais temporizadores (até 16384 chamadas e 4 MB de texto";
    // 8192 sequences of two calls fill the queue; the next one is past the limit, which no
    // pcall gets past: the write after the loop is refused too.
    let e = failed(rt.call_script(&ids[0], PATH, "calls", &[]));
    assert!(e.message.contains(limit) && e.stops_script(), "{e}");
    assert_eq!(e.line, 5);
    assert_eq!(
        attribute(&rt, &ids[0], "Resultado"),
        Value::Text(String::new())
    );
    // The script stopped on its object and its timers went, with what they held.
    assert_eq!(rt.disabled_scripts().count(), 1);
    assert!(rt.script_timers().is_empty());
    assert_eq!(rt.timer_room(), (0, 0));
    // 4 MB of text fit; one byte more does not.
    let e = failed(rt.call_script(&ids[1], PATH, "text", &[]));
    assert!(e.message.contains(limit), "{e}");
    assert_eq!(e.line, 11);
    assert_eq!(rt.timer_room(), (0, 0));
    // What a sequence holds comes back call by call, and a repetition's with its cancel.
    assert_eq!(rt.call_script(&ids[2], PATH, "some", &[]), ScriptCall::Done);
    assert_eq!(rt.timer_room(), (2, 5));
    step(&mut rt, 1);
    assert_eq!(rt.timer_room(), (1, 4));
    step(&mut rt, 12);
    assert_eq!(rt.timer_room(), (0, 2));
    assert_eq!(rt.call_script(&ids[2], PATH, "stop", &[]), ScriptCall::Done);
    assert_eq!(rt.timer_room(), (0, 0));
    assert!(rt.script_timers().is_empty());
}

#[test]
fn a_function_named_like_a_callback_that_is_none_is_told_once_with_the_closest_name() {
    let source = "local M = {}\nfunction M.on_aera_enter(self) end\nfunction M.on_mesage(self) \
                  end\nfunction M.on_message(self) end\nfunction M.on_zzz(self) end\nreturn M\n";
    let (a, b) = (object("A", &[PATH]), object("B", &[PATH]));
    let mut rt = start(vec![a, b], &[(PATH, source)]);
    step(&mut rt, 2);
    let lines: Vec<&str> = rt.logs.iter().map(|l| l.text.as_str()).collect();
    assert_eq!(
        lines,
        [
            "scripts/teste.luau:2: on_aera_enter não é um evento dos scripts e nunca roda \
             sozinha; quis dizer on_area_enter?",
            "scripts/teste.luau:3: on_mesage não é um evento dos scripts e nunca roda sozinha; \
             quis dizer on_message?",
            "scripts/teste.luau:5: on_zzz não é um evento dos scripts e nunca roda sozinha.",
        ]
    );
    assert_eq!(rt.logs[0].script.as_ref().map(|s| s.line), Some(2));
}

/// Runs `function` of `source` for `id` in a VM of its own, with `args` after `self`.
fn call_with(rt: &mut Runtime, id: &str, source: &str, args: &[ScriptArg]) -> ScriptCall {
    let host = engine_host(&[(PATH, source)]);
    host.begin_scene([id]);
    let module = host.load(PATH).unwrap();
    host.call(rt, id, &module, "show", args)
}

#[test]
fn callbacks_receive_nil_objects_and_records_as_tables() {
    let door = object("Porta", &[]);
    let id = door.id.clone();
    let mut rt = start(vec![door], &[]);
    let record = ScriptArg::Record(vec![
        ("kind", Value::Text("land".into())),
        ("impact", Value::Number(4.5)),
    ]);
    let me = ScriptArg::Value(Value::Object(Some(id.clone())));
    let shown = "local M = {}
function M.show(self, a, b, c)
    self.attr.Resultado =                  typeof(a) .. ' ' .. tostring(b) .. ' ' .. tostring(rawequal(self, c))
end
                 return M
";
    let args = [record.clone(), ScriptArg::Nil, me];
    assert_eq!(call_with(&mut rt, &id, shown, &args), ScriptCall::Done);
    assert_eq!(
        attribute(&rt, &id, "Resultado"),
        Value::Text("table nil true".into())
    );
    let read = "local M = {}
function M.show(self, event)
    self.attr.Resultado = event.kind                 .. ' ' .. tostring(event.impact)
end
return M
";
    assert_eq!(call_with(&mut rt, &id, read, &[record]), ScriptCall::Done);
    assert_eq!(
        attribute(&rt, &id, "Resultado"),
        Value::Text("land 4.5".into())
    );
}
