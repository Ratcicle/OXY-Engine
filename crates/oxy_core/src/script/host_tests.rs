//! The host's contract: sandbox, loading, handles, budgets and the policy of errors.
use super::host::{LIMITS, Limits, Phase, Shared, api, peek};
use super::*;
use crate::document::{Entity, Id, Project, Value};
use crate::runtime::{FIXED_DT, InputFrame, Runtime, RuntimeOptions, ScriptLocation};
use mlua::{LightUserData, Lua, ffi};
use std::os::raw::c_int;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::sync::Arc;

const PATH: &str = "scripts/teste.luau";

fn sources(files: &[(&str, &str)]) -> Arc<ScriptSources> {
    Arc::new(ScriptSources::from_texts(files.iter().copied()))
}

/// An object with a text attribute `Resultado` that runs `scripts`.
fn object(name: &str, scripts: &[&str]) -> Entity {
    let mut e = Entity::new(name, None);
    e.attributes
        .insert("Resultado".into(), Value::Text(String::new()));
    e.scripts = scripts.iter().copied().map(ScriptRef::new).collect();
    e
}

fn start(objects: Vec<Entity>, files: &[(&str, &str)]) -> Runtime {
    let mut project = Project::new("Scripts");
    project.scenes[0].entities = objects;
    let scene = project.start_scene.clone();
    let options = RuntimeOptions {
        scripts: sources(files),
        ..Default::default()
    };
    Runtime::with_options(&project, &scene, options).unwrap()
}

/// One object, "Porta", running `source` as `scripts/teste.luau`.
fn single(source: &str) -> (Runtime, Id) {
    let door = object("Porta", &[PATH]);
    let id = door.id.clone();
    (start(vec![door], &[(PATH, source)]), id)
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

fn result(rt: &Runtime, id: &str) -> String {
    match &rt.scene().entity(id).unwrap().attributes["Resultado"] {
        Value::Text(text) => text.clone(),
        other => panic!("{other:?}"),
    }
}

fn limits(rt: &Runtime, limits: Limits) {
    rt.script_host().unwrap().set_limits(limits);
}

unsafe extern "C-unwind" fn push_integer(l: *mut ffi::lua_State) -> c_int {
    unsafe {
        ffi::lua_pushinteger64(l, 7);
        1
    }
}

/// A host with a `test` table: values scripts cannot make (a buffer, a 64-bit integer, a
/// userdata) and functions that panic inside and outside the engine's barrier.
fn test_host(files: &[(&str, &str)]) -> ScriptHost {
    ScriptHost::with_extra(sources(files), &|lua: &Lua, base, shared: &Rc<Shared>| {
        let test = lua.create_table()?;
        test.raw_set(
            "buffer",
            lua.create_function(|lua, ()| lua.create_buffer(b"abc"))?,
        )?;
        // SAFETY: pushes one value.
        test.raw_set("integer", unsafe { lua.create_c_function(push_integer)? })?;
        test.raw_set(
            "userdata",
            lua.create_function(|lua, ()| lua.create_any_userdata(7u8))?,
        )?;
        let s = shared.clone();
        test.raw_set(
            "boom",
            lua.create_function(move |lua, ()| {
                api(lua, &s, "test.boom", |_| -> Result<(), String> {
                    panic!("defeito na engine")
                })
            })?,
        )?;
        test.raw_set(
            "raw_panic",
            lua.create_function(|_, ()| -> mlua::Result<()> { panic!("pânico cru") })?,
        )?;
        base.raw_set("test", test)
    })
    .unwrap()
}

fn use_test_host(rt: &mut Runtime, files: &[(&str, &str)]) {
    let host = test_host(files);
    host.begin_scene(rt.scene().entities.iter().map(|e| e.id.as_str()));
    rt.replace_script_host(host);
}

#[test]
fn the_bytecode_reader_knows_the_compilers_format() {
    let bytes = sandbox::compiler(1).compile("return 1").unwrap();
    assert_eq!(
        (bytes[0], bytes[1]),
        (bytecode::VERSION, bytecode::TYPES_VERSION),
        "o mlua mudou o formato do bytecode: atualize o leitor da conferência"
    );
    let source = "local M = {}\nlocal K = { 1, 2, a = 'x' }\n\
                  function M.f(self, n) local t = {} for i = 1, n do t[i] = K[1] + math.floor(n) \
                  end return function() return #t end end\nreturn M\n";
    for level in [1, 2] {
        let bytes = sandbox::compiler(level).compile(source).unwrap();
        let module = bytecode::parse(&bytes).unwrap();
        let report = bytecode::analyze(&module).unwrap();
        assert!(report.top_level_writes.is_empty() && report.stateful_protos.is_empty());
        assert!(
            report.imports.contains("math.floor"),
            "{:?}",
            report.imports
        );
    }
    let mut wrong = bytes.clone();
    wrong[0] = 10;
    assert!(
        bytecode::parse(&wrong)
            .unwrap_err()
            .contains("versão de bytecode 10")
    );
    assert!(bytecode::parse(&bytes[..bytes.len() - 1]).is_err());
}

#[test]
fn the_globals_are_the_engines_list_read_only_and_frozen() {
    let host = ScriptHost::new(sources(&[])).unwrap();
    let names: String = host
        .lua()
        .load(
            "local out = {} for k, v in pairs(_G) do table.insert(out, k .. ':' .. type(v)) end \
             table.sort(out) return table.concat(out, ' ')",
        )
        .eval()
        .unwrap();
    let mut expected = vec![
        "_G:table",
        "_VERSION:string",
        "assert:function",
        "bit32:table",
        "coroutine:table",
        "error:function",
        "getmetatable:function",
        "ipairs:function",
        "math:table",
        "next:function",
        "pairs:function",
        "pcall:function",
        "print:function",
        "rawequal:function",
        "rawget:function",
        "rawlen:function",
        "rawset:function",
        "require:function",
        "select:function",
        "setmetatable:function",
        "string:table",
        "table:table",
        "tonumber:function",
        "tostring:function",
        "type:function",
        "typeof:function",
        "unpack:function",
        "utf8:table",
        "vector:table",
        "warn:function",
        "xpcall:function",
    ];
    expected.sort();
    assert_eq!(names, expected.join(" "), "os globais da partida mudaram");
    assert!(check(PATH, "local x = 1i return {}").is_err());
    let frozen: String = host
        .lua()
        .load(
            "return tostring(table.isfrozen(getmetatable(vector.create(0, 0, 0)))) .. ' ' .. \
             tostring(table.isfrozen(getmetatable(''))) .. ' ' .. tostring(table.isfrozen(_G)) \
             .. ' ' .. tostring(table.isfrozen(math)) .. ' ' .. tostring(table.isfrozen(string))",
        )
        .eval()
        .unwrap();
    assert_eq!(frozen, "true true true true true");
    let handles = host.lua().type_metatable::<LightUserData>().unwrap();
    assert!(handles.is_readonly());
    // A function scripts call, kept and called from Rust outside a call, refuses.
    let print: mlua::Function = host.lua().globals().get("print").unwrap();
    let e = print.call::<()>("oi").unwrap_err().to_string();
    assert!(e.contains("fora de uma chamada de script"), "{e}");
}

#[test]
fn nothing_hidden_is_reachable_and_globals_cannot_be_written() {
    let names = [
        "os",
        "io",
        "debug",
        "buffer",
        "integer",
        "loadstring",
        "load",
        "dofile",
        "loadfile",
        "getfenv",
        "setfenv",
        "gcinfo",
        "newproxy",
        "collectgarbage",
        "package",
    ];
    let mut source = String::from("local M = {}\n");
    for (i, name) in names.iter().enumerate() {
        source.push_str(&format!("function M.n{i}(self) return {name} end\n"));
    }
    source.push_str(
        "function M.random(self) return math.random(1, 6) end\n\
         function M.seed(self) math.randomseed(1) end\n\
         function M.write(self) contador = 1 end\n\
         function M.raw(self) rawset(_G, 'x', 1) end\n\
         function M.field(self) _G.x = 1 end\n\
         function M.meta(self) setmetatable(_G, {}) end\n\
         function M.typo(self) prnt('x') end\n\
         function M.weak(self) setmetatable({}, { __mode = 'k' }) end\n\
         return M\n",
    );
    let (mut rt, id) = single(&source);
    for (i, name) in names.iter().enumerate() {
        let e = failed(call(&mut rt, &id, &format!("n{i}")));
        assert_eq!(
            e.message,
            format!("{name} não existe nos scripts"),
            "{name}"
        );
        assert_eq!(e.line, i as u32 + 2, "{name}");
    }
    for function in ["random", "seed"] {
        let e = failed(call(&mut rt, &id, function));
        assert!(e.message.contains("os sorteios são da engine"), "{e}");
    }
    let e = failed(call(&mut rt, &id, "write"));
    assert_eq!(
        e.message,
        "variáveis globais não existem nos scripts (contador); use local, self.state ou um atributo"
    );
    for function in ["raw", "field", "meta"] {
        let e = failed(call(&mut rt, &id, function));
        assert!(e.message.contains("tabela travada"), "{function}: {e}");
    }
    let e = failed(call(&mut rt, &id, "typo"));
    assert_eq!(
        e.message,
        "variável global desconhecida: prnt; quis dizer print?"
    );
    let e = failed(call(&mut rt, &id, "weak"));
    assert!(e.message.contains("tabelas fracas"), "{e}");
    // A global written at the top refuses the script.
    let (rt, _) = single("contador = 0\nreturn {}");
    assert!(
        rt.logs[0].text.starts_with(
            "scripts/teste.luau:1: variáveis globais não existem nos scripts (contador)"
        )
    );
}

#[test]
fn no_address_shows_in_text_formats_or_errors_of_any_value() {
    let source = r#"local M = {}
local function finished()
    local co = coroutine.create(function() end)
    coroutine.resume(co)
    return co
end
local function values(self)
    return {
        tabela = {},
        funcao = function() end,
        funcao_c = print,
        corrotina = finished(),
        userdata = test.userdata(),
        buffer = test.buffer(),
        objeto = self,
        erro = select(2, pcall(setmetatable, 1)),
    }
end
function M.texts(self)
    local out = {}
    for name, value in values(self) do
        local _, caught = pcall(error, value)
        table.insert(out, name .. "=" .. tostring(value) .. "|" .. string.format("%*", value)
            .. "|" .. tostring(caught))
        print(value)
    end
    table.sort(out)
    self.attr.Resultado = table.concat(out, "\n")
end
function M.raise_table(self) error({}) end
function M.raise_function(self) error(print) end
function M.raise_thread(self) error(coroutine.running()) end
function M.raise_buffer(self) error(test.buffer()) end
function M.raise_userdata(self) error(test.userdata()) end
return M
"#;
    let (mut rt, id) = single("return {}");
    use_test_host(&mut rt, &[(PATH, source)]);
    assert_eq!(call(&mut rt, &id, "texts"), ScriptCall::Done);
    let texts = result(&rt, &id);
    assert!(!texts.contains("0x"), "{texts}");
    assert!(texts.contains("tabela=tabela|tabela|tabela"), "{texts}");
    assert!(texts.contains("objeto=Porta|Porta|Porta"), "{texts}");
    assert!(
        texts.contains("corrotina=corrotina|corrotina|corrotina"),
        "{texts}"
    );
    assert!(texts.contains("buffer=buffer|buffer|buffer"), "{texts}");
    for line in &rt.logs {
        assert!(!line.text.contains("0x"), "{}", line.text);
    }
    for function in [
        "raise_table",
        "raise_function",
        "raise_thread",
        "raise_buffer",
        "raise_userdata",
    ] {
        let e = failed(call(&mut rt, &id, function));
        assert!(!e.message.contains("0x"), "{function}: {e}");
        assert_eq!(e.path, PATH);
        assert!(e.line > 0, "{function}: {e}");
    }
    for line in &rt.logs {
        assert!(!line.text.contains("0x"), "{}", line.text);
    }
}

#[test]
fn loading_refuses_bad_scripts_with_clear_messages() {
    let refused = |source: &str| check(PATH, source).unwrap_err().to_string();
    assert!(
        refused("local = 1").starts_with("scripts/teste.luau:1: erro de sintaxe: "),
        "{}",
        refused("local = 1")
    );
    let e = refused(
        "local M = {}\nlocal contador = 0\nfunction M.on_step(self)\n    contador += 1\nend\nreturn M\n",
    );
    assert!(
        e.starts_with(
            "scripts/teste.luau:3: `contador` é um local do topo do script alterado dentro de uma \
             função"
        ),
        "{e}"
    );
    assert!(e.contains("local function antes de usá-la, const"), "{e}");
    // The known false positive, a forward declaration, gets the same hint.
    let e = refused(
        "local M = {} local ajuda function M.f() return ajuda() end ajuda = function() return 1 \
         end return M",
    );
    assert!(e.contains("`ajuda`") && e.contains("M.ajuda"), "{e}");
    // A local factory inlined into the top is the same case.
    assert!(
        refused(
            "local function novo() local n = 0 return function() n += 1 return n end end return { \
         proximo = novo() }"
        )
        .contains("`n`")
    );
    for good in [
        "local M = {} function M.f() local n = 0 local function inc() n += 1 end inc() return n end return M",
        "local M = {} local function fat(n) return if n <= 1 then 1 else n * fat(n - 1) end local K = { 1, 2 } function M.f() return fat(5) + K[1] end return M",
        "local x = 1 x = 2 local M = { x = x } return M",
        "const LIMITE = 3 local M = {} function M.f() return LIMITE end return M",
    ] {
        check(PATH, good).unwrap_or_else(|e| panic!("{good}: {e}"));
    }
    let report = check(
        PATH,
        "local M = {} function M.f(x) return math.floor(x) end return M",
    );
    assert_eq!(report.unwrap().imports, ["math.floor"]);

    // At load: what the top does and what the module keeps.
    let load_error = |source: &str| {
        let (mut rt, id) = single("return {}");
        use_test_host(&mut rt, &[(PATH, source)]);
        assert_eq!(call(&mut rt, &id, "f"), ScriptCall::Skipped, "{source}");
        rt.logs.last().map(|l| l.text.clone()).unwrap_or_default()
    };
    for (source, message) in [
        (
            "return 1",
            "scripts/teste.luau: o script precisa devolver a tabela do módulo",
        ),
        (
            "\nprint('oi') return {}",
            "scripts/teste.luau:2: o topo do script não pode mexer no mundo; faça isso em on_start",
        ),
        (
            "return { b = test.buffer() }",
            "módulo.b: buffer guardado no módulo",
        ),
        (
            "return { i = test.integer() }",
            "módulo.i: inteiro de 64 bits (integer)",
        ),
        (
            "return { u = test.userdata() }",
            "módulo.u: userdata que não é um valor imutável",
        ),
        (
            "local co = coroutine.create(function() end) coroutine.resume(co) return { c = co }",
            "módulo.c: corrotina guardada no módulo",
        ),
        (
            "local w = coroutine.wrap(function() end) w() return { w = w }",
            "módulo.w: função do host com estado próprio",
        ),
        (
            "return { it = string.gmatch('a b', '%a') }",
            "módulo.it: função do host com estado próprio",
        ),
        (
            "local F = {} function F.novo() local n = 0 return function() n += 1 return n end end \
             return { proximo = F.novo() }",
            "função criada ao carregar o script (linha 1) guarda uma variável local que muda",
        ),
        (
            "local co = coroutine.create(function() coroutine.yield() end) coroutine.resume(co) \
             return {}",
            "uma corrotina criada nesta chamada ficou suspensa",
        ),
        (
            "while true do end return {}",
            "o script trabalhou demais numa chamada",
        ),
        (
            "local t = {} for i = 1, 100000 do t = { t } end return { t = t }",
            "módulo.t.[1].[1].[1]",
        ),
        (
            "local t = {} for i = 1, 100000 do t = { t } end return { t = t }",
            " …: tabelas aninhadas demais (mais de 200 níveis)",
        ),
        (
            "\nprnt('oi') return {}",
            "scripts/teste.luau:2: variável global desconhecida: prnt; quis dizer print? Os \
             objetos com este script ficam sem ele.",
        ),
    ] {
        let line = load_error(source);
        assert!(line.contains(message), "{source}\n{line}");
        assert!(
            line.ends_with("Os objetos com este script ficam sem ele."),
            "{line}"
        );
    }
    // What the module reaches is frozen after the load.
    let (mut rt, id) = single(
        "local M = {}\nM.dados = { lista = { 1, 2 } }\nlocal cache = {}\n\
         setmetatable(M.dados, { __index = function() return 0 end })\n\
         function M.lista(self) M.dados.lista[1] = 9 end\n\
         function M.cache(self) cache.x = 1 end\n\
         function M.meta(self) getmetatable(M.dados).__index = nil end\n\
         function M.modulo(self) M.novo = 1 end\n\
         function M.ok(self) self.attr.Resultado = tostring(M.dados.lista[2] + M.dados.outro) end\n\
         return M",
    );
    for function in ["lista", "cache", "meta", "modulo"] {
        let e = failed(call(&mut rt, &id, function));
        assert!(e.message.contains("tabela travada"), "{function}: {e}");
    }
    assert_eq!(call(&mut rt, &id, "ok"), ScriptCall::Done);
    assert_eq!(result(&rt, &id), "2");
    // Deep data below the limit loads.
    let (mut rt, id) = single(
        "local t = {} for i = 1, 150 do t = { t } end\nreturn { t = t, f = function(self) end }",
    );
    assert_eq!(call(&mut rt, &id, "f"), ScriptCall::Done);
}

#[test]
fn a_script_that_does_not_load_is_told_once_and_its_objects_run_without_it() {
    let (a, b) = (object("A", &[PATH]), object("B", &[PATH]));
    let ids = [a.id.clone(), b.id.clone()];
    let mut rt = start(
        vec![a, b],
        &[(PATH, "local M = {}\nfunction M.f(\nreturn M")],
    );
    assert_eq!(rt.logs.len(), 1, "{:?}", rt.logs);
    let line = &rt.logs[0];
    assert!(
        line.text
            .starts_with("scripts/teste.luau:3: erro de sintaxe:"),
        "{}",
        line.text
    );
    assert_eq!(
        line.script,
        Some(ScriptLocation {
            path: PATH.into(),
            line: 3
        })
    );
    for id in &ids {
        assert_eq!(call(&mut rt, id, "f"), ScriptCall::Skipped);
    }
    let missing = object("C", &["scripts/falta.luau"]);
    let rt = start(vec![missing], &[]);
    assert_eq!(
        rt.logs[0].text,
        "scripts/falta.luau: o script não existe na pasta do projeto. Os objetos com este script \
         ficam sem ele."
    );
    assert_eq!(rt.logs[0].script, None);
}

#[test]
fn projects_without_scripts_create_no_vm_and_keep_their_state_parts() {
    let mut project = Project::new("Só grafo");
    project.scenes[0].entities.push(Entity::new("Caixa", None));
    let scene = project.start_scene.clone();
    let mut rt = Runtime::new(&project, &scene).unwrap();
    rt.advance(FIXED_DT * 3., &InputFrame::default());
    assert!(!rt.has_script_host());
    let names: Vec<String> = rt
        .state_digest()
        .parts
        .into_iter()
        .map(|p| p.name)
        .collect();
    assert!(
        !names.iter().any(|n| n == "script_errors" || n == "scripts"),
        "{names:?}"
    );
}

#[test]
fn objects_are_stable_values_with_marks() {
    let source = r#"local M = {}
function M.f(self)
    local a, b = self, self.attr
    assert(typeof(a) == "Object" and typeof(b) == "Object")
    assert(a == self and a ~= b and rawequal(a, self) and not rawequal(a, b))
    assert(tostring(self) == "Porta" and tostring(b) == "atributos de Porta")
    assert(getmetatable(self) == "travado")
    local t = { [self] = 1, [b] = 2 }
    assert(t[self] == 1 and t[self.attr] == 2)
    assert(self.attr.Ausente == nil and self.attr.Numero == 1)
    self.attr.Numero += 1.5
    self.attr.Ponto = vector.create(1, 2, 3)
    self.attr.Plano = vector.create(4, 5, 6)
    local outro = self.attr.Alvo
    self.attr.Resultado = self.name .. " " .. outro.name .. " " .. tostring(outro:exists())
        .. " " .. tostring(self:has_attribute("Numero")) .. " " .. tostring(self:has_attribute("x"))
    local exists = self.exists
    assert(exists(self) and rawequal(exists, self.exists))
    self.attr.Alvo = nil
    self.attr.Alvo = outro
end
function M.gone(self, outro)
    self.attr.Resultado = tostring(outro:exists())
    return outro.name
end
function M.loose(self) self.x = 1 end
function M.name(self) self.name = "x" end
function M.typo(self) return self.nmae end
function M.method(self) self:exist() end
function M.property(self) return self:name() end
function M.kind(self) self.attr.Numero = "texto" end
function M.infinite(self) self.attr.Numero = 1 / 0 end
function M.missing(self) self.attr.Novo = 1 end
return M
"#;
    let other = Entity::new("Alvo", None);
    let other_id = other.id.clone();
    let mut door = object("Porta", &[PATH]);
    for (name, value) in [
        ("Numero", Value::Number(1.)),
        ("Ponto", Value::Vector3([0.; 3])),
        ("Plano", Value::Vector2([0.; 2])),
        ("Alvo", Value::Object(Some(other_id.clone()))),
    ] {
        door.attributes.insert(name.into(), value);
    }
    let id = door.id.clone();
    let mut rt = start(vec![door, other], &[(PATH, source)]);
    assert_eq!(call(&mut rt, &id, "f"), ScriptCall::Done);
    assert_eq!(result(&rt, &id), "Porta Alvo true true false");
    let attributes = &rt.scene().entity(&id).unwrap().attributes;
    assert_eq!(attributes["Numero"], Value::Number(2.5));
    assert_eq!(attributes["Ponto"], Value::Vector3([1., 2., 3.]));
    assert_eq!(attributes["Plano"], Value::Vector2([4., 5.]));
    assert_eq!(attributes["Alvo"], Value::Object(Some(other_id.clone())));
    for (function, message) in [
        (
            "loose",
            "self não aceita campos soltos (x); use self.state ou um atributo",
        ),
        ("name", "name é só leitura"),
        ("typo", "Object não tem 'nmae'; quis dizer 'name'?"),
        ("method", "Object não tem 'exist'; quis dizer 'exists'?"),
        (
            "property",
            "name é uma propriedade, não um método (use obj.name)",
        ),
        ("kind", "o atributo Numero guarda número, não texto"),
        ("infinite", "o atributo Numero não aceita infinito nem NaN"),
        (
            "missing",
            "o atributo Novo não existe neste objeto; crie-o no painel de propriedades",
        ),
    ] {
        let e = failed(call(&mut rt, &id, function));
        assert_eq!(e.message, message, "{function}");
        assert!(e.line > 0, "{function}");
    }
    // A removed object: `exists()` is false, anything else fails clearly.
    rt.remove_object(&other_id);
    let gone = [Value::Object(Some(other_id.clone()))];
    let e = failed(rt.call_script(&id, PATH, "gone", &gone));
    assert_eq!(e.message, "o objeto não existe mais");
    assert_eq!(result(&rt, &id), "false");
}

/// The order `pairs` gives over a table keyed by the handles of 48 objects with fixed IDs,
/// after removals and insertions.
fn pairs_order() -> String {
    let mut controller = object("Controle", &[PATH]);
    controller.id = "controle".into();
    let mut objects = Vec::new();
    for i in 1..=48 {
        let mut e = Entity::new(format!("Objeto {i}"), None);
        e.id = format!("objeto-{i}-{:08x}", (i as u32).wrapping_mul(2_654_435_761));
        controller
            .attributes
            .insert(format!("Alvo {i}"), Value::Object(Some(e.id.clone())));
        objects.push(e);
    }
    objects.insert(0, controller);
    let source = r#"local M = {}
function M.order(self)
    local targets = {}
    for i = 1, 48 do targets[i] = self.attr["Alvo " .. i] end
    local t = {}
    for i, h in ipairs(targets) do t[h] = i end
    for i = 1, #targets, 3 do t[targets[i]] = nil end
    for i = 1, #targets, 6 do t[targets[i]] = true end
    local names = {}
    for h in pairs(t) do table.insert(names, h.name) end
    self.attr.Resultado = table.concat(names, ",")
end
return M
"#;
    let mut rt = start(objects, &[(PATH, source)]);
    assert_eq!(
        rt.call_script("controle", PATH, "order", &[]),
        ScriptCall::Done
    );
    result(&rt, "controle")
}

#[test]
#[ignore = "processo filho de pairs_over_handles_is_the_same_in_every_process"]
fn pairs_order_child() {
    println!("ORDEM {}", pairs_order());
}

#[test]
fn pairs_over_handles_is_the_same_in_every_process() {
    let here = pairs_order();
    assert_eq!(here.split(',').count(), 40, "{here}");
    let exe = std::env::current_exe().unwrap();
    for _ in 0..2 {
        let output = std::process::Command::new(&exe)
            .args([
                "--exact",
                "script::host_tests::pairs_order_child",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let order = stdout
            .lines()
            .find_map(|line| line.split_once("ORDEM ").map(|(_, order)| order))
            .unwrap_or_else(|| panic!("o processo filho não respondeu: {stdout}"));
        assert_eq!(order, here);
    }
}

#[test]
fn the_unit_limit_of_a_call_stops_the_script_on_its_object() {
    let source = "local M = {}\nfunction M.spend(self)\n    for i = 1, 100 do\n        local _ = \
                  self.attr.Resultado\n    end\nend\nfunction M.caught(self)\n    pcall(M.spend, \
                  self)\nend\nfunction M.write(self) self.attr.Resultado = 'ok' end\nreturn M\n";
    let (a, b) = (object("A", &[PATH]), object("B", &[PATH]));
    let ids = [a.id.clone(), b.id.clone()];
    let mut rt = start(vec![a, b], &[(PATH, source)]);
    limits(
        &rt,
        Limits {
            call_units: 50,
            ..LIMITS
        },
    );
    let written = rt.logs_written();
    let e = failed(call(&mut rt, &ids[0], "spend"));
    assert_eq!(e.kind, ScriptErrorKind::Limit);
    assert_eq!(
        e.to_string(),
        "scripts/teste.luau:4: Limite de 50 operações por chamada"
    );
    assert_eq!(rt.logs_written(), written + 1);
    let line = rt.logs.last().unwrap();
    assert_eq!(
        line.text,
        "A · scripts/teste.luau:4: Limite de 50 operações por chamada. Script interrompido neste \
         objeto."
    );
    assert_eq!(line.script.as_ref().map(|s| s.line), Some(4));
    assert_eq!(call(&mut rt, &ids[0], "write"), ScriptCall::Skipped);
    assert_eq!(
        rt.disabled_scripts().collect::<Vec<_>>(),
        [(ids[0].as_str(), PATH)]
    );
    let parts = rt.state_digest().parts;
    assert!(
        parts
            .iter()
            .any(|p| p.name == "scripts" && p.object.as_deref() == Some(ids[0].as_str()))
    );
    // A pcall does not get past it: the call still fails and the script stops.
    let e = failed(call(&mut rt, &ids[1], "caught"));
    assert_eq!(e.kind, ScriptErrorKind::Limit);
    assert_eq!(call(&mut rt, &ids[1], "write"), ScriptCall::Skipped);
    // The object's removal and a scene change forget the stop.
    rt.remove_object(&ids[0]);
    assert_eq!(rt.disabled_scripts().count(), 1);
    let scene = rt.scene_id().to_owned();
    rt.change_scene(&scene).unwrap();
    assert_eq!(rt.disabled_scripts().count(), 0);
    assert_eq!(call(&mut rt, &ids[1], "write"), ScriptCall::Done);
}

#[test]
fn the_step_budget_is_checked_between_calls() {
    let source = "local M = {}\nfunction M.spend(self)\n    for i = 1, 20 do local _ = \
                  self.attr.Resultado end\nend\nreturn M\n";
    let objects: Vec<Entity> = (0..3).map(|i| object(&format!("O{i}"), &[PATH])).collect();
    let ids: Vec<Id> = objects.iter().map(|e| e.id.clone()).collect();
    let mut rt = start(objects, &[(PATH, source)]);
    limits(
        &rt,
        Limits {
            step_units: 30,
            ..LIMITS
        },
    );
    rt.advance(FIXED_DT, &InputFrame::default());
    // 21 units each: the second call starts below 30 and runs whole; the third waits.
    assert_eq!(call(&mut rt, &ids[0], "spend"), ScriptCall::Done);
    assert_eq!(call(&mut rt, &ids[1], "spend"), ScriptCall::Done);
    assert_eq!(call(&mut rt, &ids[2], "spend"), ScriptCall::Deferred);
    rt.advance(FIXED_DT, &InputFrame::default());
    assert_eq!(call(&mut rt, &ids[2], "spend"), ScriptCall::Done);
    assert_eq!(rt.disabled_scripts().count(), 0);
}

#[test]
fn the_interrupt_net_stops_endless_loops_even_under_pcall() {
    let source = "local M = {}\nfunction M.loop(self)\n    while true do end\nend\n\
                  function M.caught(self)\n    pcall(M.loop, self)\n    self.attr.Resultado = \
                  'escapou'\nend\nfunction M.spin(self)\n    for i = 1, 10000 do end\nend\n\
                  return M\n";
    let (a, b) = (object("A", &[PATH]), object("B", &[PATH]));
    let ids = [a.id.clone(), b.id.clone()];
    let mut rt = start(vec![a, b], &[(PATH, source)]);
    let e = failed(call(&mut rt, &ids[0], "loop"));
    assert_eq!(e.kind, ScriptErrorKind::Limit);
    assert_eq!(
        e.to_string(),
        "scripts/teste.luau:3: o script trabalhou demais numa chamada (um laço sem fim?)"
    );
    let e = failed(call(&mut rt, &ids[1], "caught"));
    assert_eq!(e.kind, ScriptErrorKind::Limit);
    assert_eq!(result(&rt, &ids[1]), "");
    assert_eq!(rt.disabled_scripts().count(), 2);

    // The step's net: the call that goes past it fails, the calls after it wait.
    let objects: Vec<Entity> = (0..8).map(|i| object(&format!("O{i}"), &[PATH])).collect();
    let ids: Vec<Id> = objects.iter().map(|e| e.id.clone()).collect();
    let mut rt = start(objects, &[(PATH, source)]);
    limits(
        &rt,
        Limits {
            step_interrupts: 30_000,
            ..LIMITS
        },
    );
    rt.advance(FIXED_DT, &InputFrame::default());
    let calls: Vec<ScriptCall> = ids.iter().map(|id| call(&mut rt, id, "spin")).collect();
    let failed_at = calls
        .iter()
        .position(|c| matches!(c, ScriptCall::Failed(_)))
        .unwrap();
    assert!(failed_at >= 1 && calls[..failed_at].iter().all(|c| *c == ScriptCall::Done));
    let ScriptCall::Failed(e) = &calls[failed_at] else {
        unreachable!()
    };
    assert_eq!(
        e.message,
        "os scripts trabalharam demais neste passo (um laço sem fim?)"
    );
    assert!(
        calls[failed_at + 1..]
            .iter()
            .all(|c| *c == ScriptCall::Deferred)
    );
    rt.advance(FIXED_DT, &InputFrame::default());
    assert_eq!(call(&mut rt, &ids[failed_at + 1], "spin"), ScriptCall::Done);
}

#[test]
fn a_memory_error_inside_the_c_index_path_stops_the_script() {
    let source = "local M = {}\nfunction M.read(self)\n    local big = self.attr.Grande\nend\n\
                  function M.fill(self)\n    local t = {}\n    for i = 1, 1e8 do t[i] = i end\nend\n\
                  function M.write(self) self.attr.Resultado = 'ok' end\nreturn M\n";
    let mut a = object("A", &[PATH]);
    a.attributes
        .insert("Grande".into(), Value::Text("x".repeat(4 << 20)));
    let b = object("B", &[PATH]);
    let ids = [a.id.clone(), b.id.clone()];
    let mut rt = start(vec![a, b], &[(PATH, source)]);
    assert_eq!(call(&mut rt, &ids[1], "write"), ScriptCall::Done);
    let used = rt.script_host().unwrap().lua().used_memory();
    limits(
        &rt,
        Limits {
            memory: used + (1 << 20),
            ..LIMITS
        },
    );
    let e = failed(call(&mut rt, &ids[0], "read"));
    assert_eq!(e.kind, ScriptErrorKind::Limit);
    assert!(
        e.message
            .starts_with("os scripts passaram do limite de memória"),
        "{e}"
    );
    assert_eq!(
        rt.disabled_scripts().collect::<Vec<_>>(),
        [(ids[0].as_str(), PATH)]
    );
    let host = rt.script_host().unwrap();
    assert!(peek(host.shared()).is_none());
    assert_eq!(host.shared().phase.get(), Phase::Idle);
    // A script that fills the VM stops as well, and the VM goes on.
    let e = failed(call(&mut rt, &ids[1], "fill"));
    assert_eq!(e.kind, ScriptErrorKind::Limit);
    limits(&rt, LIMITS);
    let lua = rt.script_host().unwrap().lua();
    assert_eq!(lua.load("return 2 * 21").eval::<i32>().unwrap(), 42);
}

#[test]
fn a_panic_in_a_host_function_is_an_engine_error_whatever_the_script_does() {
    let source = "local M = {}\nfunction M.boom(self)\n    local ok = pcall(test.boom)\n    \
                  self.attr.Resultado = tostring(ok)\nend\nfunction M.raw(self)\n    \
                  pcall(test.raw_panic)\nend\nfunction M.write(self) self.attr.Resultado = 'ok' \
                  end\nreturn M\n";
    let (mut rt, id) = single("return {}");
    use_test_host(&mut rt, &[(PATH, source)]);
    let e = failed(call(&mut rt, &id, "boom"));
    assert_eq!(e.kind, ScriptErrorKind::Engine);
    assert_eq!(
        e.message,
        "erro interno da engine em test.boom: defeito na engine"
    );
    // What the call did after its pcall stays; the script goes on.
    assert_eq!(result(&rt, &id), "false");
    assert_eq!(rt.disabled_scripts().count(), 0);
    assert_eq!(call(&mut rt, &id, "write"), ScriptCall::Done);
    // Without the barrier, mlua carries the panic as a Lua error, the script's pcall rethrows
    // it and it resumes in Rust: the guard clears the pointer as it unwinds.
    let host = test_host(&[(PATH, source)]);
    host.begin_scene(rt.scene().entities.iter().map(|e| e.id.as_str()));
    let panic = catch_unwind(AssertUnwindSafe(|| {
        host.call(&mut rt, &id, PATH, "raw", &[])
    }))
    .unwrap_err();
    assert_eq!(panic.downcast_ref::<&str>(), Some(&"pânico cru"));
    assert!(peek(host.shared()).is_none());
    assert_eq!(host.shared().phase.get(), Phase::Idle);
    assert_eq!(
        host.call(&mut rt, &id, PATH, "write", &[]),
        ScriptCall::Done
    );
}

#[test]
fn an_error_stops_only_its_call_and_is_told_once_with_a_count() {
    let source = "local M = {}\nfunction M.open(self)\n    self.attr.Resultado = 'antes'\n    \
                  local door = nil\n    door.abrir()\nend\nfunction M.fail(self, n)\n    \
                  error('n=' .. n)\nend\nfunction M.spend(self, n)\n    for i = 1, n do if i == \
                  3 and n < 10 then error('cedo') end local _ = self.attr.Resultado end\nend\n\
                  return M\n";
    let (mut rt, id) = single(source);
    let written = rt.logs_written();
    for _ in 0..3 {
        let e = failed(call(&mut rt, &id, "open"));
        assert_eq!(e.kind, ScriptErrorKind::Error);
        assert_eq!(
            e.to_string(),
            "scripts/teste.luau:5: tentou ler 'abrir' de um valor nil"
        );
    }
    assert_eq!(result(&rt, &id), "antes");
    assert_eq!(rt.logs_written(), written + 1);
    let line = rt.logs.last().unwrap();
    assert_eq!(
        line.text,
        "Porta · scripts/teste.luau:5: tentou ler 'abrir' de um valor nil."
    );
    assert_eq!(
        line.script,
        Some(ScriptLocation {
            path: PATH.into(),
            line: 5
        })
    );
    assert_eq!(
        rt.script_errors().map(|(_, _, n)| n).collect::<Vec<_>>(),
        [3]
    );
    assert_eq!(rt.disabled_scripts().count(), 0);
    assert!(
        rt.state_digest()
            .parts
            .iter()
            .any(|p| p.name == "script_errors")
    );
    // The same place with other values in its message is the same error.
    for n in 1..=3 {
        let e = failed(rt.call_script(&id, PATH, "fail", &[Value::Number(n as f64)]));
        assert_eq!(e.to_string(), format!("scripts/teste.luau:8: n={n}"));
    }
    assert_eq!(rt.logs_written(), written + 2);
    assert_eq!(
        rt.logs.last().unwrap().text,
        "Porta · scripts/teste.luau:8: n=1."
    );
    assert_eq!(
        rt.script_errors()
            .map(|(_, text, n)| (text, n))
            .collect::<Vec<_>>(),
        [
            (
                "Porta · scripts/teste.luau:5: tentou ler 'abrir' de um valor nil.",
                3
            ),
            ("Porta · scripts/teste.luau:8: n=1.", 3)
        ]
    );
    // A function the module lacks.
    let e = failed(call(&mut rt, &id, "close"));
    assert_eq!(
        e.to_string(),
        "scripts/teste.luau: o script não tem a função close"
    );
    // A limit at a line that already failed is told: it stops the script.
    limits(
        &rt,
        Limits {
            call_units: 50,
            ..LIMITS
        },
    );
    let e = failed(rt.call_script(&id, PATH, "spend", &[Value::Number(5.)]));
    assert_eq!(e.to_string(), "scripts/teste.luau:11: cedo");
    let e = failed(rt.call_script(&id, PATH, "spend", &[Value::Number(100.)]));
    assert_eq!(e.kind, ScriptErrorKind::Limit);
    assert_eq!(rt.logs_written(), written + 5);
    assert_eq!(
        rt.logs.last().unwrap().text,
        "Porta · scripts/teste.luau:11: Limite de 50 operações por chamada. Script interrompido \
         neste objeto."
    );
}

#[test]
fn print_and_warn_write_the_object_and_the_line() {
    let source = "local M = {}\nfunction M.f(self)\n    print('olá', 5, 1.5, true, nil, { 1, 2, \
                  a = 3, { 4 } }, self, vector.create(1, 2, 3))\n    warn('cuidado')\nend\nreturn \
                  M\n";
    let (mut rt, id) = single(source);
    assert_eq!(call(&mut rt, &id, "f"), ScriptCall::Done);
    let lines: Vec<_> = rt.logs.iter().rev().take(2).rev().collect();
    assert_eq!(
        lines[0].text,
        "Porta · olá 5 1.5 true nil {1, 2, {…}, a = 3} Porta 1, 2, 3"
    );
    assert_eq!(lines[0].script.as_ref().map(|s| s.line), Some(3));
    assert_eq!(lines[1].text, "Porta · aviso: cuidado");
    assert_eq!(lines[1].script.as_ref().map(|s| s.line), Some(4));
}

#[test]
fn string_format_writes_star_like_the_engine() {
    let source = "local M = {}\nfunction M.f(self)\n    self.attr.Resultado = \
                  string.format('%5.1f|%*|%*|%%|%*|%s|%*', 3.14159, {}, 7, self, 'x', 0.1) .. \
                  ('%*'):format(print)\nend\nreturn M\n";
    let (mut rt, id) = single(source);
    assert_eq!(call(&mut rt, &id, "f"), ScriptCall::Done);
    assert_eq!(result(&rt, &id), "  3.1|tabela|7|%|Porta|x|0.1função");
}

#[test]
fn require_serves_frozen_modules_from_the_scripts_folder() {
    let main = "local M = {}\nlocal cards = require('./dados/cartas')\nM.cost = cards[1].custo\n\
                function M.f(self)\n    local again = require('./dados/cartas')\n    \
                self.attr.Resultado = tostring(M.cost) .. ' ' .. tostring(table.isfrozen(cards)) \
                .. ' ' .. tostring(rawequal(again, cards))\nend\nfunction M.change(self) \
                cards[1].custo = 9 end\nreturn M\n";
    let level = "local M = {}\nM.cards = require('../dados/cartas')\nreturn M\n";
    let files = [
        (PATH, main),
        (
            "scripts/dados/cartas.luau",
            "return { { nome = 'Golpe', custo = 2 } }",
        ),
        ("scripts/fases/nivel.luau", level),
        ("scripts/a.luau", "return { b = require('./b') }"),
        ("scripts/b.luau", "return { a = require('./a') }"),
    ];
    let (mut rt, id) = single("return {}");
    rt.replace_script_host(ScriptHost::new(sources(&files)).unwrap());
    rt.script_host()
        .unwrap()
        .begin_scene(rt.scene().entities.iter().map(|e| e.id.as_str()));
    assert_eq!(call(&mut rt, &id, "f"), ScriptCall::Done);
    assert_eq!(result(&rt, &id), "2 true true");
    assert!(
        failed(call(&mut rt, &id, "change"))
            .message
            .contains("tabela travada")
    );
    let host = rt.script_host().unwrap();
    host.load("scripts/fases/nivel.luau").0.unwrap();
    let e = host.load("scripts/a.luau").0.unwrap_err();
    assert!(
        e.message
            .contains("require circular: scripts/a.luau → scripts/b.luau → scripts/a.luau"),
        "{e}"
    );
    for (name, message) in [
        (
            "@native/fisica",
            "módulos Rust por jogo ainda não existem (@native/)",
        ),
        ("@oxy", "esse apelido não existe"),
        ("dados/cartas", "o caminho começa com ./ ou ../"),
        ("../fora", "fora da pasta scripts"),
        ("./nada", "scripts/nada.luau não existe"),
    ] {
        let path = format!("scripts/req{}.luau", name.len());
        let source = format!("\nreturn require('{name}')");
        let host = ScriptHost::new(sources(&[(&path, &source)])).unwrap();
        let e = host.load(&path).0.unwrap_err();
        assert!(e.message.contains(message), "{name}: {e}");
        assert_eq!(e.line, 2, "{name}: {e}");
    }
}

#[test]
fn coroutines_must_finish_within_their_call() {
    let source = "local M = {}\nfunction M.generator(self)\n    local next = coroutine.wrap(function() \
                  coroutine.yield(1) coroutine.yield(2) end)\n    local sum = next() + next()\n    \
                  next()\n    self.attr.Resultado = tostring(sum)\nend\nfunction M.left(self)\n    \
                  local co = coroutine.create(function() coroutine.yield() end)\n    \
                  coroutine.resume(co)\nend\nreturn M\n";
    let (mut rt, id) = single(source);
    assert_eq!(call(&mut rt, &id, "generator"), ScriptCall::Done);
    assert_eq!(result(&rt, &id), "3");
    let e = failed(call(&mut rt, &id, "left"));
    assert!(
        e.message
            .starts_with("uma corrotina criada nesta chamada ficou suspensa"),
        "{e}"
    );
    assert_eq!(e.kind, ScriptErrorKind::Error);
}

#[test]
fn handle_keys_that_meet_are_refused_by_name() {
    let host = ScriptHost::new(sources(&[])).unwrap();
    let shared = host.shared();
    // Forge the meeting: the key of "a" already names "b".
    shared
        .handles
        .borrow_mut()
        .insert(handles::key("a"), "b".into());
    assert_eq!(shared.register("a"), Err("b".to_owned()));
    assert_eq!(shared.register("c"), Ok(handles::key("c")));
    let mut project = Project::new("Chaves");
    let (mut x, mut y) = (Entity::new("Caixa", None), Entity::new("Bola", None));
    x.id = "a".into();
    y.id = "b".into();
    project.scenes[0].entities = vec![x, y];
    let scene = project.start_scene.clone();
    let rt = Runtime::new(&project, &scene).unwrap();
    assert_eq!(
        collision(&rt, "b", "a"),
        "os objetos Bola e Caixa têm a mesma chave nos scripts; duplique um deles de novo"
    );
    // A handle is a pure function of the ID, never 0, with the mark above it.
    let pointer = handles::handle(handles::MARK_OBJECT, handles::key("a"));
    assert_eq!(
        handles::decode(pointer),
        (handles::MARK_OBJECT, handles::key("a"))
    );
    assert_ne!(handles::key(""), 0);
}
