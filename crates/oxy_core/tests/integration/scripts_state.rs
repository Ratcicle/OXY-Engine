//! What scripts keep in the engine besides their tasks (design §4.3, §4.6, §4.8, §4.11): their
//! memory per object and script, the messages between objects, the reload of their modules
//! with the state kept, and the check of a VM made anew, all in the state hash and through a
//! replay.
use oxy_core::{
    document::*,
    headless::{self, SimulationRequest},
    persistence,
    runtime::{FIXED_DT, InputFrame, Runtime, RuntimeOptions, SCRIPT_ENTRIES_WARNING},
    script::{ScriptCall, ScriptError, ScriptErrorKind, ScriptRef, ScriptSources},
};
use serde_json::json;
use std::path::Path;
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

fn project(objects: Vec<Entity>) -> Project {
    let mut project = Project::new("Estado");
    project.scenes[0].entities = objects;
    project
}

fn start(objects: Vec<Entity>, files: &[(&str, &str)]) -> Runtime {
    let project = project(objects);
    let options = RuntimeOptions {
        scripts: sources(files),
        ..Default::default()
    };
    Runtime::with_options(&project, &project.start_scene, options).unwrap()
}

fn step(rt: &mut Runtime, steps: usize) {
    for _ in 0..steps {
        rt.advance(FIXED_DT, &InputFrame::default());
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

fn attribute(rt: &Runtime, id: &str, name: &str) -> Value {
    rt.scene().entity(id).unwrap().attributes[name].clone()
}

fn result(rt: &Runtime, id: &str) -> String {
    match attribute(rt, id, "Resultado") {
        Value::Text(text) => text,
        other => panic!("{other:?}"),
    }
}

fn memory(rt: &Runtime, id: &str, path: &str) -> Vec<(String, String)> {
    rt.script_memory_text(id, path).unwrap_or_default()
}

fn field(name: &str, value: &str) -> (String, String) {
    (name.to_owned(), value.to_owned())
}

const MEMORY: &str = r#"local M = {}
M.state = { count = 0, label = "", spot = vector.create(0, 0, 0), done = false, items = {}, a = {}, b = {} }
function M.bump(self) self.state.count += 1 self.state.label ..= "x" self.state.spot += vector.create(1, 2, 3) end
function M.fill(self) local list = {} self.state.items = list table.insert(list, 7) end
function M.alias(self) self.state.b = self.state.a self.state.a.x = 1 end
function M.read(self) self.attr.Resultado = #self.state.items .. " " .. tostring(self.state.a.x) .. " " .. tostring(self.state.b.x) end
function M.apart(self) self.state.b.y = 2 self.attr.Resultado = tostring(self.state.a.y) end
function M.show(self) self.attr.Resultado = string.format("%d %s %s %s", self.state.count, self.state.label, tostring(self.state.spot), tostring(self.state.done)) end
function M.mixed(self) self.state.items = { [3] = "c", z = 1, [1] = "a", b = 2, [2] = "b", a = 3, nested = { [2] = self, [1] = true } } end
function M.walk(self) local out = {} for k, v in pairs(self.state.items) do table.insert(out, tostring(k)) end self.attr.Resultado = table.concat(out, " ") end
function M.fail_after(self) table.insert(self.state.items, "guardado") error("depois") end
function M.last(self) self.attr.Resultado = tostring(self.state.items[#self.state.items]) end
function M.same(self) self.attr.Resultado = tostring(self.state.items == self.state.items) .. " " .. tostring(rawequal(self.state, self.state)) .. " " .. typeof(self.state) .. " " .. tostring(self.state) end
return M
"#;

#[test]
fn memory_keeps_its_fields_and_writes_back_tables_read_or_assigned() {
    let door = object("Porta", &[PATH]);
    let id = door.id.clone();
    let mut rt = start(vec![door], &[(PATH, MEMORY)]);
    // No call touched the memory yet: the object has none, the declared defaults read alike.
    assert!(rt.script_memory_text(&id, PATH).is_none());
    assert_eq!(call(&mut rt, &id, "show"), ScriptCall::Done);
    assert_eq!(result(&rt, &id), "0  0, 0, 0 false");
    // The two cases of the stage 0 review: a list filled after it went into memory, and an
    // alias written after the assignment. Without the fix they read "0 1 nil".
    for function in ["fill", "alias", "read"] {
        assert_eq!(call(&mut rt, &id, function), ScriptCall::Done, "{function}");
    }
    assert_eq!(result(&rt, &id), "1 1 1");
    // An alias lasts only to the end of its call: a and b are two tables in the next one.
    assert_eq!(call(&mut rt, &id, "apart"), ScriptCall::Done);
    assert_eq!(result(&rt, &id), "nil");
    // Scalars go straight to the engine.
    for _ in 0..2 {
        assert_eq!(call(&mut rt, &id, "bump"), ScriptCall::Done);
    }
    assert_eq!(call(&mut rt, &id, "show"), ScriptCall::Done);
    assert_eq!(result(&rt, &id), "2 xx 2, 4, 6 false");
    assert_eq!(call(&mut rt, &id, "same"), ScriptCall::Done);
    assert_eq!(result(&rt, &id), "true true Object memória de Porta");
    // What a call did to a table before its error stays, as what it did to the world.
    assert_eq!(call(&mut rt, &id, "fill"), ScriptCall::Done);
    let e = failed(call(&mut rt, &id, "fail_after"));
    assert_eq!((e.line, e.message.as_str()), (11, "depois"));
    assert_eq!(call(&mut rt, &id, "last"), ScriptCall::Done);
    assert_eq!(result(&rt, &id), "guardado");
    assert_eq!(
        memory(&rt, &id, PATH),
        [
            field("a", "{x = 1}"),
            field("b", "{x = 1, y = 2}"),
            field("count", "2"),
            field("done", "false"),
            field("items", "{[1] = 7, [2] = \"guardado\"}"),
            field("label", "\"xx\""),
            field("spot", "vector(2, 4, 6)"),
        ]
    );
    // Whole keys and texts, nested tables and objects: kept in the canonical order, and walked
    // in the same order in every runtime.
    assert_eq!(call(&mut rt, &id, "mixed"), ScriptCall::Done);
    assert_eq!(
        memory(&rt, &id, PATH)[4],
        field(
            "items",
            &format!(
                "{{[1] = \"a\", [2] = \"b\", [3] = \"c\", a = 3, b = 2, nested = {{[1] = true, \
                 [2] = objeto {id}}}, z = 1}}"
            )
        )
    );
    assert_eq!(call(&mut rt, &id, "walk"), ScriptCall::Done);
    let walked = result(&rt, &id);
    assert!(walked.starts_with("1 2 3 "), "{walked}");
    let door = object("Porta", &[PATH]);
    let other = door.id.clone();
    let mut again = start(vec![door], &[(PATH, MEMORY)]);
    for function in ["mixed", "walk"] {
        assert_eq!(call(&mut again, &other, function), ScriptCall::Done);
    }
    assert_eq!(result(&again, &other), walked);
}

#[test]
fn memory_refuses_what_it_cannot_keep_with_the_line_and_a_hint() {
    let source = r#"local M = {}
M.state = { count = 0, items = { 1 } }
function M.typo(self) self.state.cout = 1 end
function M.kind(self) self.state.count = "x" end
function M.none(self) self.state.count = nil end
function M.inf(self) self.state.count = 1 / 0 end
function M.long(self) self.state.items = { string.rep("x", 5000) } end
function M.func(self) self.state.items = { 1, { f = print } } end
function M.meta(self) self.state.items = setmetatable({}, {}) end
function M.key(self) self.state.items = { [1.5] = 1 } end
function M.boolkey(self) self.state.items = { [true] = 1 } end
function M.walk(self) for k in pairs(self.state) do end end
function M.len(self) return #self.state end
function M.clone(self) return table.clone(self.state) end
function M.other(self) return self.attr.Outro.state end
function M.table_scalar(self) self.state.items = 3 end
function M.scalar_table(self) self.state.count = {} end
function M.attr(self) self.state.items = { self.attr } end
function M.forin(self) for k in self.state do end end
function M.refused_then_fail(self) self.state.items = { print } error("depois") end
return M
"#;
    let mut door = object("Porta", &[PATH]);
    let outro = object("Outro", &[PATH]);
    door.attributes
        .insert("Outro".into(), Value::Object(Some(outro.id.clone())));
    let id = door.id.clone();
    let mut rt = start(vec![door, outro], &[(PATH, source)]);
    let guide = "veja “Memória do script” no Guia";
    for (function, line, message) in [
        (
            "typo",
            3,
            "self.state.cout: não é um campo da memória; declare-o no topo, em Modulo.state; \
             quis dizer count?",
        ),
        ("kind", 4, "self.state.count: guarda número, não texto"),
        ("none", 5, "self.state.count: guarda número, não nil"),
        ("inf", 6, "self.state.count: não aceita infinito nem NaN"),
        (
            "long",
            7,
            "self.state.items[1]: não aceita texto de mais de 4096 bytes",
        ),
        (
            "func",
            8,
            "self.state.items[2].f: não aceita função; só booleanos, números, textos, vetores, \
             objetos, superfícies e tabelas deles",
        ),
        (
            "meta",
            9,
            "self.state.items: não aceita tabela com metatabela",
        ),
        (
            "key",
            10,
            "self.state.items: não aceita a chave 1.5; as chaves são números inteiros ou textos",
        ),
        (
            "boolkey",
            11,
            "self.state.items: não aceita chave do tipo booleano",
        ),
        ("walk", 12, guide),
        (
            "len",
            13,
            "self.state não se mede, não se percorre e não se copia",
        ),
        ("clone", 14, guide),
        (
            "other",
            15,
            "só self.state existe: a memória de outro objeto não é acessível",
        ),
        (
            "table_scalar",
            16,
            "self.state.items: guarda tabela, não número",
        ),
        (
            "scalar_table",
            17,
            "self.state.count: guarda número, não tabela",
        ),
        (
            "attr",
            18,
            "self.state.items[1]: não aceita os atributos de um objeto",
        ),
        ("forin", 19, guide),
        // The call's error, and the table it left that memory refused.
        (
            "refused_then_fail",
            20,
            "depois (a memória também recusou: self.state.items[1]: não aceita função",
        ),
    ] {
        let e = failed(call(&mut rt, &id, function));
        assert!(e.message.contains(message), "{function}: {e}");
        assert_eq!(e.line, line, "{function}: {e}");
        assert_eq!(e.kind, ScriptErrorKind::Error, "{function}: {e}");
    }
    // Nothing refused reached the memory, and no error stopped the script.
    assert_eq!(
        memory(&rt, &id, PATH),
        [field("count", "0"), field("items", "{[1] = 1}")]
    );
    assert_eq!(rt.disabled_scripts().count(), 0);

    // A script without memory, and declarations that cannot be memory, which do not load.
    let none = "local M = {}\nfunction M.read(self) return self.state end\nreturn M\n";
    let (mut rt, id) = {
        let door = object("Porta", &[PATH]);
        let id = door.id.clone();
        (start(vec![door], &[(PATH, none)]), id)
    };
    let e = failed(call(&mut rt, &id, "read"));
    assert!(
        e.message.starts_with("este script não declara memória"),
        "{e}"
    );
    for (declared, message) in [
        (
            "M.state = 5",
            "Modulo.state precisa ser uma tabela de campos, como { vida = 3 }",
        ),
        (
            "M.state = { f = print }",
            "Modulo.state.f: não aceita função",
        ),
        (
            "M.state = { 1, 2 }",
            "os campos de Modulo.state têm nomes de texto",
        ),
        (
            "M.state = { lista = { { { { { 1 } } } } } }",
            "Modulo.state.lista[1][1][1][1]: passou de 4 níveis de tabela",
        ),
    ] {
        let source = format!("local M = {{}}\n{declared}\nreturn M\n");
        let rt = start(vec![object("Porta", &[PATH])], &[(PATH, &source)]);
        let told = &rt.logs[0].text;
        assert!(
            told.starts_with("scripts/teste.luau: ") && told.contains(message),
            "{declared}: {told}"
        );
    }
}

#[test]
fn memory_past_its_limits_stops_the_script_on_its_object() {
    let mut fields = String::new();
    for i in 1..=17 {
        fields.push_str(&format!("t{i} = \"\", "));
    }
    let source = format!(
        r#"local M = {{}}
M.state = {{ items = {{}}, notes = {{}}, {fields} }}
function M.many(self) local t = {{}} for i = 1, 1025 do t[i] = i end self.state.items = t end
function M.deep(self) self.state.items = {{ {{ {{ {{ {{ 1 }} }} }} }} }} end
function M.cycle(self) local t = {{}} t.again = t self.state.items = t end
function M.big(self) for i = 1, 20 do self.state.notes[i] = string.rep("x", 4000) end end
function M.texts(self)
    for i = 1, 17 do self.state["t" .. i] = string.rep("y", 4000) end
end
function M.fits(self) self.state.items = {{ {{ {{ {{ 1 }} }} }} }} local t = {{}} for i = 1, 1024 do t[i] = i end self.state.notes = t end
function M.many_then_fail(self) local t = {{}} for i = 1, 2000 do t[i] = i end self.state.items = t error("depois") end
return M
"#
    );
    let names = [
        "Muitos", "Fundo", "Ciclo", "Grande", "Textos", "Cabe", "Falha",
    ];
    let objects: Vec<Entity> = names.iter().map(|n| object(n, &[PATH])).collect();
    let ids: Vec<Id> = objects.iter().map(|e| e.id.clone()).collect();
    let mut rt = start(objects, &[(PATH, &source)]);
    for (i, (function, line, message)) in [
        (
            "many",
            3,
            "self.state.items: uma tabela passou de 1024 entradas",
        ),
        ("deep", 4, "passou de 4 níveis de tabela"),
        (
            "cycle",
            5,
            "passou de 4 níveis de tabela (ou uma tabela contém a si mesma)",
        ),
        ("big", 6, "self.state.notes[17]: passou de 64 KB"),
        (
            "texts",
            8,
            "a memória do script passou de 64 KB neste objeto",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let e = failed(call(&mut rt, &ids[i], function));
        assert!(e.message.contains(message), "{function}: {e}");
        assert_eq!(e.line, line, "{function}: {e}");
        assert!(e.stops_script(), "{function}: {e}");
    }
    let stopped: Vec<&str> = rt.disabled_scripts().map(|(owner, _)| owner).collect();
    assert_eq!(stopped.len(), 5);
    // The refused tables never reached the memory; the 16 texts that fit did.
    assert_eq!(memory(&rt, &ids[0], PATH)[0], field("items", "{}"));
    let texts = memory(&rt, &ids[4], PATH);
    assert_eq!(texts.iter().filter(|(_, v)| v.len() > 4000).count(), 16);
    // Four levels and 1024 entries fit.
    assert_eq!(call(&mut rt, &ids[5], "fits"), ScriptCall::Done);
    // A table past a limit stops the script even when the call failed after keeping it.
    let e = failed(call(&mut rt, &ids[6], "many_then_fail"));
    assert_eq!(
        (e.line, e.message.as_str()),
        (
            11,
            "self.state.items: uma tabela passou de 1024 entradas; a chamada também parou num \
             erro: scripts/teste.luau:11: depois"
        )
    );
    assert!(e.stops_script());
    assert!(rt.disabled_scripts().any(|(owner, _)| owner == ids[6]));
}

#[test]
fn memory_is_kept_per_object_and_script_in_the_state_and_goes_with_its_scene() {
    let counter = |field: &str| {
        format!(
            "local M = {{}}\nM.state = {{ n = 0 }}\nfunction M.on_step(self)\n    self.state.n \
             += {field}\nend\nreturn M\n"
        )
    };
    let (one, ten) = (counter("1"), counter("10"));
    let files = [
        ("scripts/um.luau", one.as_str()),
        ("scripts/dez.luau", ten.as_str()),
    ];
    let a = object("A", &["scripts/um.luau", "scripts/dez.luau"]);
    let b = object("B", &["scripts/um.luau"]);
    let plain = object("Sem script", &[]);
    let ids = [a.id.clone(), b.id.clone(), plain.id.clone()];
    let mut project = project(vec![a, b, plain]);
    project.scenes.push(Scene::new("Outra", SceneKind::ThreeD));
    let other_scene = project.scenes[1].id.clone();
    let options = RuntimeOptions {
        scripts: sources(&files),
        ..Default::default()
    };
    let mut rt = Runtime::with_options(&project, &project.start_scene, options).unwrap();
    step(&mut rt, 3);
    assert_eq!(memory(&rt, &ids[0], "scripts/um.luau"), [field("n", "3")]);
    assert_eq!(memory(&rt, &ids[0], "scripts/dez.luau"), [field("n", "30")]);
    assert_eq!(memory(&rt, &ids[1], "scripts/um.luau"), [field("n", "3")]);
    // One part per (object, script), only where there is memory.
    let parts = |rt: &Runtime| -> Vec<(String, String)> {
        rt.state_digest()
            .parts
            .iter()
            .filter(|p| p.name.starts_with("script:"))
            .map(|p| (p.object.clone().unwrap(), p.name.clone()))
            .collect()
    };
    assert_eq!(
        parts(&rt),
        [
            (ids[0].clone(), "script:scripts/dez.luau".to_owned()),
            (ids[0].clone(), "script:scripts/um.luau".to_owned()),
            (ids[1].clone(), "script:scripts/um.luau".to_owned()),
        ]
    );
    let before = rt.state_digest();
    step(&mut rt, 1);
    let difference = before.first_difference(&rt.state_digest()).unwrap();
    assert_eq!(difference.part, "time");
    // An object that goes takes its memory; a scene change drops all of it.
    rt.remove_object(&ids[1]);
    assert!(rt.script_memory_text(&ids[1], "scripts/um.luau").is_none());
    assert_eq!(parts(&rt).len(), 2);
    rt.change_scene(&other_scene).unwrap();
    assert!(parts(&rt).is_empty());
    assert!(!rt.scene_state_in_use().contains(&"script_memory"));
}

const SENDER: &str = r#"local M = {}
M.state = { sent = 0 }
function M.on_step(self)
    self.state.sent += 1
    if self.state.sent == 1 then
        local data = { n = 1, list = { "a" } }
        self.attr.B:send("oi", data)
        data.n = 99
        self.attr.C:send("oi", 2)
        self:send("eco")
        self.attr.B:send("ping", 1)
    end
    if self.state.sent <= 3 then
        self.attr.Mudo:send("nada")
    end
end
function M.on_message(self, name, value, from)
    local log = self.attr.Registro
    log.attr.Resultado ..= self.name .. "<" .. name .. ":" .. tostring(value) .. "@" .. self.state.sent .. " "
    if name == "pong" and value < 4 then
        from:send("ping", value + 1)
    end
end
return M
"#;

const RECEIVER: &str = r#"local M = {}
M.state = { steps = 0 }
function M.on_step(self) self.state.steps += 1 end
function M.on_message(self, name, value, from)
    local shown = type(value) == "table" and (value.n .. value.list[1]) or tostring(value)
    local log = self.attr.Registro
    log.attr.Resultado ..= self.name .. "<" .. name .. ":" .. shown .. ":" .. from.name .. "@" .. self.state.steps .. " "
    if name == "ping" then
        from:send("pong", value)
    end
end
return M
"#;

#[test]
fn messages_arrive_in_the_next_logic_phase_in_the_order_they_were_sent() {
    let mut log = object("Registro", &[]);
    log.id = "registro".into();
    // C comes first in the scene: messages keep the order they were sent in, not the scene's.
    let mut c = object("C", &["scripts/recebe.luau"]);
    let mut a = object("A", &["scripts/manda.luau"]);
    let mut b = object("B", &["scripts/recebe.luau"]);
    let mute = object("Mudo", &[]);
    for (name, target) in [("B", &b.id), ("C", &c.id), ("Mudo", &mute.id)] {
        a.attributes
            .insert(name.into(), Value::Object(Some(target.clone())));
    }
    for e in [&mut a, &mut b, &mut c] {
        e.attributes
            .insert("Registro".into(), Value::Object(Some(log.id.clone())));
    }
    let files = [
        ("scripts/manda.luau", SENDER),
        ("scripts/recebe.luau", RECEIVER),
    ];
    let project = project(vec![log, c, a, b, mute]);
    let run = || {
        let options = RuntimeOptions {
            scripts: sources(&files),
            ..Default::default()
        };
        let mut rt = Runtime::with_options(&project, &project.start_scene, options).unwrap();
        step(&mut rt, 6);
        rt
    };
    let mut rt = run();
    // Sent in the first phase of step 1, they arrive in its second; B's pong, sent there,
    // arrives in the first phase of step 2, after A's on_step, and so on, one message a phase.
    // The table was copied when sent.
    assert_eq!(
        result(&rt, "registro"),
        "B<oi:1a:A@1 C<oi:2:A@1 A<eco:nil@1 B<ping:1:A@1 A<pong:1@2 B<ping:2:A@2 A<pong:2@3 \
         B<ping:3:A@3 A<pong:3@4 B<ping:4:A@4 A<pong:4@5 "
    );
    // The target without on_message is told once, at the line that sent to it, and counted.
    let told: Vec<(String, u64)> = rt
        .script_errors()
        .map(|(_, text, count)| (text.to_owned(), count))
        .collect();
    assert_eq!(
        told,
        [(
            "scripts/manda.luau:14: Mudo não tem on_message em nenhum script: a mensagem \"nada\" \
             não chega a ele."
                .to_owned(),
            3
        )]
    );
    // A replay does the same.
    assert_eq!(rt.state_hash(), run().state_hash());

    // A message on its way is part of the state.
    let ids: Vec<Id> = rt.scene().entities.iter().map(|e| e.id.clone()).collect();
    let before = rt.state_digest();
    let sent = rt.call_script(
        &ids[3],
        "scripts/recebe.luau",
        "on_message",
        &[
            Value::Text("ping".into()),
            Value::Number(9.),
            Value::Object(Some(ids[2].clone())),
        ],
    );
    assert_eq!(sent, ScriptCall::Done);
    let difference = before.first_difference(&rt.state_digest()).unwrap();
    assert_eq!(
        (difference.object, difference.part.as_str()),
        (None, "tasks")
    );
}

#[test]
fn a_flood_of_messages_stops_only_the_script_that_sends_it() {
    let source = r#"local M = {}
function M.on_start(self) self:send("mais") end
function M.on_message(self, name)
    self:send("mais")
    self:send("mais")
end
return M
"#;
    let e = object("Eco", &[PATH]);
    let id = e.id.clone();
    let mut rt = start(vec![e], &[(PATH, source)]);
    step(&mut rt, 12);
    let told: Vec<String> = rt.script_errors().map(|(_, t, _)| t.to_owned()).collect();
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(
        told[0].contains(
            "scripts/teste.luau:4: o script mandou mais de 1024 mensagens deste objeto neste passo"
        ) && told[0].ends_with("Script interrompido neste objeto."),
        "{}",
        told[0]
    );
    assert_eq!(
        rt.disabled_scripts().collect::<Vec<_>>(),
        [(id.as_str(), PATH)]
    );

    // Scripts that each send a lot stop no one: past the scene's messages, a send fails.
    let crowd = "local M = {}\nfunction M.on_start(self)\n    for i = 1, 1000 do self:send(\"oi\") \
                 end\nend\nfunction M.on_message(self) end\nreturn M\n";
    let objects = (1..=5)
        .map(|i| object(&format!("Coro {i}"), &[PATH]))
        .collect();
    let mut rt = start(objects, &[(PATH, crowd)]);
    let told: Vec<(String, u64)> = rt
        .script_errors()
        .map(|(_, text, count)| (text.to_owned(), count))
        .collect();
    assert_eq!(
        told,
        [(
            "Coro 5 · scripts/teste.luau:3: os scripts da cena já mandaram 4096 mensagens neste \
             passo; esta não foi enviada."
                .to_owned(),
            1
        )]
    );
    step(&mut rt, 1);
    assert_eq!(rt.disabled_scripts().count(), 0);
}

#[test]
fn a_target_without_on_message_is_told_once_per_line_whatever_the_names() {
    let source = r#"local M = {}
function M.on_step(self)
    self.attr.Alvo:send("onda " .. self.attr.Contagem)
    self.attr.Contagem += 1
end
return M
"#;
    let mut sender = object("Gerente", &[PATH]);
    let mute = object("Mudo", &[]);
    sender
        .attributes
        .insert("Alvo".into(), Value::Object(Some(mute.id.clone())));
    let mut rt = start(vec![sender, mute], &[(PATH, source)]);
    step(&mut rt, 10);
    let told: Vec<(String, u64)> = rt
        .script_errors()
        .map(|(_, text, count)| (text.to_owned(), count))
        .collect();
    assert_eq!(
        told,
        [(
            "scripts/teste.luau:3: Mudo não tem on_message em nenhum script: a mensagem \"onda 0\" \
             não chega a ele."
                .to_owned(),
            10
        )]
    );
    assert_eq!(rt.logs.len(), 1);
}

const V1: &str = r#"local M = {}
M.state = { n = 0, keep = "a", gone = 1, retype = 0 }
function M.on_start(self)
    self:every(0.5, M.tick)
    self:after(1, M.later)
end
function M.tick(self)
    self.state.n += 1
    self.attr.Contagem = self.state.n
end
function M.later(self) self.attr.Resultado = "v1" end
return M
"#;

const V2: &str = r#"local M = {}
M.state = { n = 0, keep = "a", added = true, retype = "" }
function M.tick(self)
    self.state.n += 10
    self.attr.Contagem = self.state.n
end
return M
"#;

#[test]
fn a_reload_swaps_the_functions_and_keeps_the_state() {
    let e = object("Relógio", &[PATH]);
    let id = e.id.clone();
    let mut rt = start(vec![e], &[(PATH, V1)]);
    step(&mut rt, 31);
    assert_eq!(attribute(&rt, &id, "Contagem"), Value::Number(1.));
    let logs = rt.logs_written();
    // The same texts: nothing changes, not even the state.
    let hash = rt.state_hash();
    let same = rt.reload_scripts(sources(&[(PATH, V1)]));
    assert!(same.reloaded.is_empty() && same.errors.is_empty() && same.warnings.is_empty());
    assert_eq!(rt.state_hash(), hash);
    // A new text: the timers go on calling by name, the memory follows the new declaration.
    let report = rt.reload_scripts(sources(&[(PATH, V2)]));
    assert_eq!(report.reloaded, [PATH]);
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(
        report.warnings,
        [
            "scripts/teste.luau: o campo retype da memória mudou de tipo e voltou ao valor \
             declarado",
            "scripts/teste.luau: o campo gone saiu de Modulo.state e da memória dos objetos",
        ]
    );
    assert_eq!(
        memory(&rt, &id, PATH),
        [
            field("added", "true"),
            field("keep", "\"a\""),
            field("n", "1"),
            field("retype", "\"\""),
        ]
    );
    // Nothing of the reload went to the runtime's log.
    assert_eq!(rt.logs_written(), logs);
    step(&mut rt, 30);
    assert_eq!(attribute(&rt, &id, "Contagem"), Value::Number(11.));
    // The timer of a function the new text lacks fell, told once.
    let told: Vec<String> = rt.script_errors().map(|(_, t, _)| t.to_owned()).collect();
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(
        told[0].contains("o script não tem a função later"),
        "{}",
        told[0]
    );
    assert_eq!(rt.script_timers().len(), 1);
    assert_eq!(result(&rt, &id), "");

    // A text that does not load keeps the old one running and says why: a syntax error, a
    // top that fails and a local of the top a function writes.
    for (broken, message) in [
        (
            "local M = {}\nfunction M.tick(self)\n    self.state.n +=\nend\nreturn M\n",
            "scripts/teste.luau:4: erro de sintaxe",
        ),
        (
            "local M = {}\nerror(\"quebrado\")\nreturn M\n",
            "scripts/teste.luau:2: quebrado",
        ),
        (
            "local M = {}\nlocal n = 0\nfunction M.tick(self) n += 1 end\nreturn M\n",
            "scripts/teste.luau:3: `n` é um local do topo do script alterado dentro de uma função",
        ),
    ] {
        let report = rt.reload_scripts(sources(&[(PATH, broken)]));
        assert!(report.reloaded.is_empty(), "{report:?}");
        assert_eq!(report.errors.len(), 1, "{report:?}");
        assert!(
            report.errors[0].to_string().starts_with(message),
            "{}",
            report.errors[0]
        );
        step(&mut rt, 30);
    }
    assert_eq!(attribute(&rt, &id, "Contagem"), Value::Number(41.));
    assert_eq!(rt.logs_written(), logs + 1);
}

#[test]
fn a_reload_brings_back_a_script_a_limit_stopped_and_spares_scripts_that_require_the_old_text() {
    let greedy = r#"local M = {}
M.state = { items = {} }
function M.on_step(self)
    local t = {}
    for i = 1, 2000 do t[i] = i end
    self.state.items = t
end
return M
"#;
    let fixed = r#"local M = {}
M.state = { items = {} }
function M.on_step(self)
    self.attr.Contagem += 1
end
return M
"#;
    let helper = "local H = {}\nH.step = 1\nreturn H\n";
    let user = r#"local H = require("./ajuda")
local STEP = H.step + 1
local M = {}
function M.on_step(self) self.attr.Contagem += STEP end
return M
"#;
    let a = object("Guloso", &[PATH]);
    let b = object("Usuário", &["scripts/usa.luau"]);
    let ids = [a.id.clone(), b.id.clone()];
    let files = [
        (PATH, greedy),
        ("scripts/ajuda.luau", helper),
        ("scripts/usa.luau", user),
    ];
    let mut rt = start(vec![a, b], &files);
    step(&mut rt, 2);
    assert_eq!(rt.disabled_scripts().count(), 1);
    assert_eq!(rt.script_errors().count(), 1);
    assert_eq!(attribute(&rt, &ids[1], "Contagem"), Value::Number(4.));
    // The fixed text runs again where the limit stopped the old one.
    let report = rt.reload_scripts(sources(&[
        (PATH, fixed),
        ("scripts/ajuda.luau", helper),
        ("scripts/usa.luau", user),
    ]));
    assert_eq!(report.reloaded, [PATH]);
    assert_eq!(rt.disabled_scripts().count(), 0);
    // Its errors will be told again, as new ones.
    assert_eq!(rt.script_errors().count(), 0);
    step(&mut rt, 2);
    assert_eq!(attribute(&rt, &ids[0], "Contagem"), Value::Number(2.));
    // A helper whose new text makes a script that ran fail goes back to its old text.
    let report = rt.reload_scripts(sources(&[
        (PATH, fixed),
        (
            "scripts/ajuda.luau",
            "local H = {}\nH.step = \"um\"\nreturn H\n",
        ),
        ("scripts/usa.luau", user),
    ]));
    assert!(report.reloaded.is_empty(), "{report:?}");
    assert_eq!(report.errors.len(), 1);
    let error = report.errors[0].to_string();
    assert!(
        error.starts_with("scripts/ajuda.luau: a nova versão faz scripts/usa.luau falhar: ")
            && error.contains("scripts/usa.luau:2: tentou fazer uma conta"),
        "{error}"
    );
    step(&mut rt, 1);
    assert_eq!(attribute(&rt, &ids[1], "Contagem"), Value::Number(10.));
    // A helper whose new text works runs, and so do the tops that require it: all of them run
    // again in the new VM.
    let report = rt.reload_scripts(sources(&[
        (PATH, fixed),
        ("scripts/ajuda.luau", "local H = {}\nH.step = 9\nreturn H\n"),
        ("scripts/usa.luau", user),
    ]));
    assert_eq!(report.reloaded, ["scripts/ajuda.luau"]);
    step(&mut rt, 1);
    assert_eq!(attribute(&rt, &ids[1], "Contagem"), Value::Number(20.));
}

#[test]
fn a_reload_lets_a_module_go_unless_a_script_that_runs_requires_it() {
    let helper = "local H = {}\nH.step = 1\nreturn H\n";
    let user = r#"local H = require("./ajuda")
local M = {}
function M.on_step(self) self.attr.Contagem += H.step + 1 end
return M
"#;
    let alone = "local M = {}\nfunction M.on_step(self) self.attr.Contagem += 10 end\nreturn M\n";
    let e = object("Usuário", &["scripts/usa.luau"]);
    let id = e.id.clone();
    let mut rt = start(
        vec![e],
        &[("scripts/ajuda.luau", helper), ("scripts/usa.luau", user)],
    );
    step(&mut rt, 1);
    assert_eq!(attribute(&rt, &id, "Contagem"), Value::Number(2.));
    // A module a script that runs still requires stays, in its old text, and says why.
    let report = rt.reload_scripts(sources(&[("scripts/usa.luau", user)]));
    assert!(report.reloaded.is_empty(), "{report:?}");
    let errors: Vec<String> = report.errors.iter().map(ToString::to_string).collect();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        errors[0].starts_with("scripts/ajuda.luau: a nova versão faz scripts/usa.luau falhar: ")
            && errors[0].contains("scripts/ajuda.luau não existe"),
        "{}",
        errors[0]
    );
    step(&mut rt, 1);
    assert_eq!(attribute(&rt, &id, "Contagem"), Value::Number(4.));
    // Once nothing requires it, it goes for good: the next reloads have nothing to say.
    let files = [("scripts/usa.luau", alone)];
    let report = rt.reload_scripts(sources(&files));
    assert_eq!(report.reloaded, ["scripts/usa.luau"]);
    assert!(
        report.errors.is_empty() && report.warnings.is_empty(),
        "{report:?}"
    );
    for _ in 0..2 {
        let again = rt.reload_scripts(sources(&files));
        assert!(
            again.reloaded.is_empty() && again.errors.is_empty() && again.warnings.is_empty(),
            "{again:?}"
        );
    }
    step(&mut rt, 1);
    assert_eq!(attribute(&rt, &id, "Contagem"), Value::Number(14.));
}

#[test]
fn a_new_text_of_a_module_renews_the_scripts_that_require_it() {
    let helper = |start: &str, count: u32| {
        format!(
            "local H = {{}}\nH.START = {start}\nH.COUNT = {count}\nfunction H.items()\n    local \
             t = {{}}\n    for i = 1, H.COUNT do t[i] = i end\n    return t\nend\nreturn H\n"
        )
    };
    let user = r#"local H = require("./ajuda")
local M = {}
M.state = { n = H.START, items = {} }
function M.on_step(self)
    self.state.items = H.items()
    self.attr.Contagem = #self.state.items
end
return M
"#;
    let e = object("Usuário", &["scripts/usa.luau"]);
    let id = e.id.clone();
    let greedy = helper("0", 2000);
    let mut rt = start(
        vec![e],
        &[("scripts/ajuda.luau", &greedy), ("scripts/usa.luau", user)],
    );
    // The helper's list is past the limit: the script that keeps it stops.
    step(&mut rt, 1);
    assert_eq!(rt.disabled_scripts().count(), 1);
    assert_eq!(rt.script_errors().count(), 1);
    assert_eq!(memory(&rt, &id, "scripts/usa.luau")[1], field("n", "0"));
    // The fix is in the helper: the script that requires it runs again, its errors will be told
    // again, and its memory follows what its top declares now.
    let fixed = helper("\"\"", 3);
    let report = rt.reload_scripts(sources(&[
        ("scripts/ajuda.luau", &fixed),
        ("scripts/usa.luau", user),
    ]));
    assert_eq!(report.reloaded, ["scripts/ajuda.luau"]);
    assert!(report.errors.is_empty(), "{report:?}");
    assert_eq!(
        report.warnings,
        ["scripts/usa.luau: o campo n da memória mudou de tipo e voltou ao valor declarado"]
    );
    assert_eq!(rt.disabled_scripts().count(), 0);
    assert_eq!(rt.script_errors().count(), 0);
    step(&mut rt, 1);
    assert_eq!(attribute(&rt, &id, "Contagem"), Value::Number(3.));
    assert_eq!(
        memory(&rt, &id, "scripts/usa.luau"),
        [
            field("items", "{[1] = 1, [2] = 2, [3] = 3}"),
            field("n", "\"\"")
        ]
    );
}

/// A project whose scripts use memory, timers and messages.
fn busy_project() -> (Project, Arc<ScriptSources>) {
    let mut log = object("Registro", &[]);
    log.id = "registro".into();
    let mut a = object("A", &["scripts/manda.luau"]);
    let mut b = object("B", &["scripts/recebe.luau"]);
    let mut c = object("C", &["scripts/recebe.luau", "scripts/relogio.luau"]);
    let mute = object("Mudo", &[]);
    for (name, target) in [("B", &b.id), ("C", &c.id), ("Mudo", &mute.id)] {
        a.attributes
            .insert(name.into(), Value::Object(Some(target.clone())));
    }
    for e in [&mut a, &mut b, &mut c] {
        e.attributes
            .insert("Registro".into(), Value::Object(Some(log.id.clone())));
    }
    let clock = r#"local M = {}
M.state = { marks = {}, total = 0 }
function M.on_start(self) self:every(0.2, M.mark) end
function M.mark(self)
    table.insert(self.state.marks, self.state.total)
    self.state.total += #self.state.marks
    self.attr.Contagem = self.state.total
end
return M
"#;
    let project = project(vec![log, a, b, c, mute]);
    let files = sources(&[
        ("scripts/manda.luau", SENDER),
        ("scripts/recebe.luau", RECEIVER),
        ("scripts/relogio.luau", clock),
    ]);
    (project, files)
}

#[test]
fn a_replay_and_a_vm_made_anew_with_memory_and_messages_stay_identical() {
    let (project, files) = busy_project();
    let request: SimulationRequest = serde_json::from_value(json!({
        "seconds": 3,
        "check_replay": true,
        "check_vm_reset": true,
        "actions": "none",
        "ui": false
    }))
    .unwrap();
    let (report, rt) = headless::run_with_scripts(&project, files.clone(), &request).unwrap();
    for check in [report.replay.as_ref(), report.vm_reset.as_ref()] {
        let check = check.unwrap();
        assert!(check.identical, "{check:?}");
        assert_eq!(check.steps, 180);
    }
    assert_eq!(
        report.state_hash,
        headless::run_with_scripts(&project, files, &request)
            .unwrap()
            .0
            .state_hash
    );
    // The run did what the scripts say.
    let ids: Vec<Id> = rt.scene().entities.iter().map(|e| e.id.clone()).collect();
    assert_eq!(
        memory(&rt, &ids[3], "scripts/relogio.luau"),
        [
            field(
                "marks",
                "{[1] = 0, [2] = 1, [3] = 3, [4] = 6, [5] = 10, [6] = 15, [7] = 21, [8] = 28, \
                 [9] = 36, [10] = 45, [11] = 55, [12] = 66, [13] = 78, [14] = 91, [15] = 105}"
            ),
            field("total", "120"),
        ]
    );
    assert!(result(&rt, "registro").contains("A<pong:4@5"));
    // Each script file's cost: the clock touches a list of 15 at most, under the warning.
    let clock = report
        .script_time
        .iter()
        .find(|t| t.path == "scripts/relogio.luau")
        .unwrap();
    // on_start in the scene start, then each mark.
    assert_eq!((clock.calls, clock.steps), (16, 16));
    assert!(clock.most_entries > 0 && clock.most_entries < SCRIPT_ENTRIES_WARNING);
    assert!(clock.warning.is_none() && clock.max_us >= clock.p50_us);
}

#[test]
fn big_tables_touched_every_step_are_warned_about() {
    let source = r#"local M = {}
M.state = { items = {}, n = 0 }
function M.on_start(self)
    local t = {}
    for i = 1, 600 do t[i] = i end
    self.state.items = t
end
function M.on_step(self)
    self.state.n += self.state.items[1]
end
return M
"#;
    let light = "local M = {}\nM.state = { n = 0 }\nfunction M.on_step(self) self.state.n += 1 \
                 end\nreturn M\n";
    let project = project(vec![
        object("Pesado", &[PATH]),
        object("Leve", &["scripts/leve.luau"]),
    ]);
    let request: SimulationRequest =
        serde_json::from_value(json!({ "seconds": 0.5, "actions": "none", "ui": false })).unwrap();
    let (report, _) = headless::run_with_scripts(
        &project,
        sources(&[(PATH, source), ("scripts/leve.luau", light)]),
        &request,
    )
    .unwrap();
    let [light, heavy] = report.script_time.as_slice() else {
        panic!("{:?}", report.script_time);
    };
    // 601 entries built in Luau and 601 read back each step.
    assert_eq!((heavy.path.as_str(), heavy.most_entries), (PATH, 1202));
    assert!(
        heavy
            .warning
            .as_deref()
            .is_some_and(|w| w.starts_with("scripts/teste.luau converteu até 1202 entradas")),
        "{heavy:?}"
    );
    assert_eq!((light.most_entries, light.warning.as_deref()), (0, None));
}

#[test]
fn graph_projects_stay_as_they_were_under_the_check_of_a_vm_made_anew() {
    let file = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/parkour")
        .join(persistence::PROJECT_FILE);
    let project = persistence::load_project(&file).unwrap();
    let request = |vm: bool| -> SimulationRequest {
        serde_json::from_value(json!({
            "seconds": 8,
            "check_vm_reset": vm,
            "actions": "none",
            "ui": false,
            "inputs": [{ "type": "click", "at": 0.5, "entity": "Jogar" }]
        }))
        .unwrap()
    };
    let (checked, rt) = headless::run(&project, &request(true)).unwrap();
    let check = checked.vm_reset.as_ref().unwrap();
    assert!(check.identical && check.steps == 480, "{check:?}");
    assert!(!rt.has_script_host() && checked.script_time.is_empty());
    let plain = headless::simulate(&project, &request(false)).unwrap();
    assert_eq!(checked.state_hash, plain.state_hash);
    assert!(plain.vm_reset.is_none());
    // The request reads and writes as before when the check is off.
    let written = serde_json::to_value(request(false)).unwrap();
    assert!(written.get("check_vm_reset").is_none(), "{written}");
}

#[test]
fn objects_in_memory_read_as_gone_once_removed_and_messages_to_them_drop() {
    let source = r#"local M = {}
M.state = { target = {}, alive = "" }
function M.keep(self) self.state.target = { self.attr.Alvo } end
function M.check(self)
    local t = self.state.target[1]
    self.attr.Resultado = tostring(t:exists())
end
function M.tell(self) self.attr.Alvo:send("oi") end
return M
"#;
    let mut keeper = object("Guarda", &[PATH]);
    let target = object("Alvo", &["scripts/alvo.luau"]);
    keeper
        .attributes
        .insert("Alvo".into(), Value::Object(Some(target.id.clone())));
    let (kid, tid) = (keeper.id.clone(), target.id.clone());
    let heard = "local M = {}\nfunction M.on_message(self) self.attr.Contagem += 1 end\nreturn M\n";
    let mut rt = start(
        vec![keeper, target],
        &[(PATH, source), ("scripts/alvo.luau", heard)],
    );
    assert_eq!(call(&mut rt, &kid, "keep"), ScriptCall::Done);
    assert_eq!(call(&mut rt, &kid, "check"), ScriptCall::Done);
    assert_eq!(result(&rt, &kid), "true");
    assert_eq!(call(&mut rt, &kid, "tell"), ScriptCall::Done);
    rt.remove_object(&tid);
    // The message to the object gone dropped with it; the memory still names it, as gone.
    step(&mut rt, 1);
    assert_eq!(call(&mut rt, &kid, "check"), ScriptCall::Done);
    assert_eq!(result(&rt, &kid), "false");
    assert_eq!(
        memory(&rt, &kid, PATH)[1],
        field("target", &format!("{{[1] = objeto {tid}}}"))
    );
}
