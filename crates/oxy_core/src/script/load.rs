//! Loading a script (design §3.5, steps 1–3): compile it with the engine's fixed compiler and
//! check the bytecode (R8); run the module's top with the host "loading", where nothing reaches
//! the world (R2); freeze in depth everything the module reaches (R1). The module is kept for
//! the life of the VM: nothing of the game lives in it.
use super::bytecode;
use super::errors::{ScriptError, describe};
use super::host::{Module, Phase, Shared, suspended_error};
use super::sandbox::compiler;
use mlua::chunk::ChunkMode;
use mlua::{Function, Lua, Table, Value as LuaValue, ffi};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::ffi::CStr;
use std::os::raw::c_int;
use std::rc::Rc;

/// The bytecode of `source` and what its checks read, or why it cannot load: a syntax error or
/// a top-level local some function writes.
pub(crate) fn check(path: &str, source: &str) -> Result<(Vec<u8>, bytecode::Report), ScriptError> {
    let bytes = compiler(1).compile(source).map_err(|e| syntax(path, &e))?;
    let module = bytecode::parse(&bytes).map_err(|e| ScriptError::new(path, 0, e))?;
    let report = bytecode::analyze(&module).map_err(|e| ScriptError::new(path, 0, e))?;
    if let Some(write) = report.top_level_writes.first() {
        // The name comes from a second compilation that keeps the locals' names.
        let name = compiler(2)
            .compile(source)
            .ok()
            .and_then(|bytes: Vec<u8>| bytecode::parse(&bytes).ok())
            .and_then(|full| bytecode::local_name(&full, write))
            .unwrap_or_else(|| "?".into());
        return Err(ScriptError::new(
            path,
            write.line.unwrap_or(0),
            format!(
                "`{name}` é um local do topo do script alterado dentro de uma função: o valor se \
                 perderia ao recarregar e na rede; use self.state ou um atributo. Se ele só \
                 recebe uma função ou um valor fixo depois de declarado, dê o valor já na \
                 declaração (local function antes de usá-la, const ou uma expressão só) ou use \
                 um campo do módulo (M.{name})"
            ),
        ));
    }
    Ok((bytes, report))
}

/// A syntax error, as the compiler gives it (`3: Expected …`).
fn syntax(path: &str, error: &mlua::Error) -> ScriptError {
    let mlua::Error::SyntaxError { message, .. } = error else {
        return describe(error, path, 0);
    };
    let digits = message.bytes().take_while(u8::is_ascii_digit).count();
    let line = message[..digits].parse().unwrap_or(0);
    let rest = message[digits..].trim_start_matches(':').trim_start();
    ScriptError::new(path, line, format!("erro de sintaxe: {rest}"))
}

/// The module `path` of the project, loading it the first time (its error is kept as well).
pub(crate) fn module(lua: &Lua, shared: &Shared, path: &str) -> Result<Rc<Module>, ScriptError> {
    if let Some(loaded) = shared.modules.borrow().get(path) {
        return loaded.clone();
    }
    if shared.loading.borrow().iter().any(|p| p == path) {
        let mut chain = shared.loading.borrow().clone();
        chain.push(path.to_owned());
        return Err(ScriptError::new(
            path,
            0,
            format!("require circular: {}", chain.join(" → ")),
        ));
    }
    let result = load(lua, shared, path);
    shared
        .modules
        .borrow_mut()
        .insert(path.to_owned(), result.clone());
    result
}

fn load(lua: &Lua, shared: &Shared, path: &str) -> Result<Rc<Module>, ScriptError> {
    let source = shared
        .sources
        .get(path)
        .ok_or_else(|| ScriptError::new(path, 0, "o script não existe na pasta do projeto"))?;
    let (bytes, report) = check(path, &source.text)?;
    let chunk = format!("@{path}");
    shared
        .stateful
        .borrow_mut()
        .insert(chunk.clone(), report.stateful_protos);
    let top = lua
        .load(&bytes[..])
        .set_name(&chunk)
        .set_mode(ChunkMode::Binary)
        .into_function()
        .map_err(|e| describe(&e, path, 0))?;
    // A require inside another top or inside a call goes on counting for it; the outermost
    // top counts as a call of its own.
    let outer = shared.phase.get();
    if outer == Phase::Idle {
        shared.start_count();
    }
    let result = {
        let _top = Top::enter(shared, path);
        top.call::<LuaValue>(())
    };
    if outer == Phase::Idle {
        let suspended = shared.suspended_coroutine();
        if let Some(error) = shared
            .engine_error(path, || 0)
            .or_else(|| shared.overrun_error(path, || 0))
        {
            return Err(error);
        }
        if result.is_ok() && suspended {
            return Err(suspended_error(path, 0));
        }
    }
    let LuaValue::Table(table) = result.map_err(|e| shared.failure(&e, path, 0))? else {
        return Err(ScriptError::new(
            path,
            0,
            "o script precisa devolver a tabela do módulo (return Modulo)",
        ));
    };
    freeze(lua, shared, &table).map_err(|e| ScriptError::new(path, 0, e))?;
    let functions = table
        .pairs::<LuaValue, LuaValue>()
        .filter_map(|pair| match pair.ok()? {
            (LuaValue::String(name), LuaValue::Function(f)) => {
                Some((name.to_str().ok()?.to_owned(), f))
            }
            _ => None,
        })
        .collect::<BTreeMap<String, Function>>();
    Ok(Rc::new(Module { table, functions }))
}

/// A module's top running: the host "loading" and the module on the stack of requires, both
/// put back when it ends, in the unwinding of a panic too.
struct Top<'a> {
    shared: &'a Shared,
    outer: Phase,
}

impl<'a> Top<'a> {
    fn enter(shared: &'a Shared, path: &str) -> Self {
        shared.loading.borrow_mut().push(path.to_owned());
        Self {
            shared,
            outer: shared.phase.replace(Phase::Loading),
        }
    }
}

impl Drop for Top<'_> {
    fn drop(&mut self) {
        self.shared.loading.borrow_mut().pop();
        self.shared.phase.set(self.outer);
    }
}

/// Freezes everything `table` reaches (tables, metatables, keys, values and the upvalues of
/// Luau functions) and refuses what the engine cannot vouch for: coroutines, userdata, buffers,
/// 64-bit integers, host functions with state of their own, closures that share a mutable
/// upvalue (a factory run at the top) and any kind of value the engine does not know, so that
/// a Luau update cannot open a hole unseen.
fn freeze(lua: &Lua, shared: &Shared, table: &Table) -> Result<(), String> {
    let known = shared.known_functions.borrow();
    let stateful = shared.stateful.borrow();
    let mut walk = Walk {
        known: &known,
        stateful: &stateful,
        path: Vec::new(),
        error: None,
    };
    // SAFETY: the walk keeps the stack balanced and leaves it empty; `exec_raw` runs it
    // protected, so a memory error unwinds as a Lua error to mlua. A refusal leaves the keys of
    // its path on the stack, where its text reads them.
    unsafe {
        lua.exec_raw::<()>(table, |l| {
            ffi::lua_createtable(l, 0, 0);
            if let Err(reason) = walk_value(l, 1, 2, &mut walk) {
                walk.error = Some(format!("{}: {reason}", path_text(l, &walk.path)));
            }
            ffi::lua_settop(l, 0);
        })
        .map_err(|e| describe(&e, "", 0).message)?;
    }
    walk.error.map_or(Ok(()), Err)
}

/// How deep the freeze goes from the module (an entry, a metatable or an upvalue is one level):
/// data modules stay far below it, and the walk's recursion stays bounded.
const MAX_DEPTH: usize = 200;

struct Walk<'a> {
    known: &'a HashSet<usize>,
    stateful: &'a HashMap<String, BTreeSet<u32>>,
    /// The steps from the module to the value being walked.
    path: Vec<Step>,
    error: Option<String>,
}

#[derive(Clone, Copy)]
enum Step {
    /// A table's entry, by the stack index of its key.
    Key(c_int),
    Metatable,
    Upvalue(c_int),
}

/// `módulo.dados.[1] (metatabela)`: the first steps of `path`, read only for a refusal.
///
/// # Safety
/// The keys of `path` are still at their stack indexes.
unsafe fn path_text(l: *mut ffi::lua_State, path: &[Step]) -> String {
    const SHOWN: usize = 12;
    let mut text = String::from("módulo");
    for step in path.iter().take(SHOWN) {
        match *step {
            Step::Key(idx) => {
                text.push('.');
                text.push_str(&unsafe { key_text(l, idx) });
            }
            Step::Metatable => text.push_str(" (metatabela)"),
            Step::Upvalue(n) => text.push_str(&format!(" (upvalue {n})")),
        }
    }
    if path.len() > SHOWN {
        text.push_str(" …");
    }
    text
}

/// Whether the value at `idx` is in the table of values seen at `seen`, marking it.
///
/// # Safety
/// `idx` and `seen` are valid absolute indexes.
unsafe fn seen_before(l: *mut ffi::lua_State, idx: c_int, seen: c_int) -> bool {
    unsafe {
        ffi::lua_pushvalue(l, idx);
        ffi::lua_rawget(l, seen);
        let was = ffi::lua_type(l, -1) != ffi::LUA_TNIL;
        ffi::lua_settop(l, -2);
        if !was {
            ffi::lua_pushvalue(l, idx);
            ffi::lua_pushboolean(l, 1);
            ffi::lua_rawset(l, seen);
        }
        was
    }
}

/// # Safety
/// `idx` is a valid absolute index.
unsafe fn key_text(l: *mut ffi::lua_State, idx: c_int) -> String {
    unsafe {
        match ffi::lua_type(l, idx) {
            ffi::LUA_TSTRING => {
                let mut len = 0usize;
                let p = ffi::lua_tolstring(l, idx, &mut len);
                String::from_utf8_lossy(std::slice::from_raw_parts(p as *const u8, len))
                    .into_owned()
            }
            ffi::LUA_TNUMBER => format!("[{}]", ffi::lua_tonumberx(l, idx, std::ptr::null_mut())),
            _ => "[?]".into(),
        }
    }
}

/// The value at `idx`, one step below the value walking it.
///
/// # Safety
/// As for [`walk_value`]; on success `step` is off the path again.
unsafe fn walk_child(
    l: *mut ffi::lua_State,
    idx: c_int,
    seen: c_int,
    w: &mut Walk,
    step: Step,
) -> Result<(), String> {
    w.path.push(step);
    unsafe { walk_value(l, idx, seen, w)? };
    w.path.pop();
    Ok(())
}

/// Refuses with a reason and leaves the stack and `w.path` as they were at the refusal, for
/// its text; otherwise leaves the stack as it was.
///
/// # Safety
/// `idx` and `seen` are valid absolute indexes.
unsafe fn walk_value(
    l: *mut ffi::lua_State,
    idx: c_int,
    seen: c_int,
    w: &mut Walk,
) -> Result<(), String> {
    unsafe {
        if w.path.len() > MAX_DEPTH {
            return Err(format!(
                "tabelas aninhadas demais (mais de {MAX_DEPTH} níveis)"
            ));
        }
        if ffi::lua_checkstack(l, 6) == 0 {
            return Err("tabelas aninhadas demais".into());
        }
        match ffi::lua_type(l, idx) {
            ffi::LUA_TNIL
            | ffi::LUA_TBOOLEAN
            | ffi::LUA_TNUMBER
            | ffi::LUA_TVECTOR
            | ffi::LUA_TSTRING
            | ffi::LUA_TLIGHTUSERDATA => Ok(()),
            ffi::LUA_TTABLE => {
                if seen_before(l, idx, seen) {
                    return Ok(());
                }
                ffi::lua_setreadonly(l, idx, 1);
                if ffi::lua_getmetatable(l, idx) != 0 {
                    let top = ffi::lua_gettop(l);
                    walk_child(l, top, seen, w, Step::Metatable)?;
                    ffi::lua_settop(l, top - 1);
                }
                ffi::lua_pushnil(l);
                while ffi::lua_next(l, idx) != 0 {
                    let top = ffi::lua_gettop(l);
                    walk_child(l, top - 1, seen, w, Step::Key(top - 1))?;
                    walk_child(l, top, seen, w, Step::Key(top - 1))?;
                    ffi::lua_settop(l, top - 1);
                }
                Ok(())
            }
            ffi::LUA_TFUNCTION => {
                if seen_before(l, idx, seen) {
                    return Ok(());
                }
                if ffi::lua_iscfunction(l, idx) != 0 {
                    return if w.known.contains(&(ffi::lua_topointer(l, idx) as usize)) {
                        Ok(())
                    } else {
                        Err("função do host com estado próprio (coroutine.wrap, \
                             string.gmatch...) não pode ficar guardada no módulo"
                            .into())
                    };
                }
                let mut ar: ffi::lua_Debug = std::mem::zeroed();
                ffi::lua_pushvalue(l, idx);
                ffi::lua_getinfo(l, -1, c"sp".as_ptr(), &mut ar);
                ffi::lua_settop(l, -2);
                let source = if ar.source.is_null() {
                    Default::default()
                } else {
                    CStr::from_ptr(ar.source).to_string_lossy()
                };
                if w.stateful
                    .get(source.as_ref())
                    .is_some_and(|protos| protos.contains(&(ar.bytecodeid as u32)))
                {
                    return Err(format!(
                        "função criada ao carregar o script (linha {}) guarda uma variável \
                         local que muda; o valor se perderia ao recarregar e na rede; use \
                         self.state ou um atributo",
                        ar.linedefined
                    ));
                }
                let mut n = 1;
                while !ffi::lua_getupvalue(l, idx, n).is_null() {
                    let top = ffi::lua_gettop(l);
                    walk_child(l, top, seen, w, Step::Upvalue(n))?;
                    ffi::lua_settop(l, top - 1);
                    n += 1;
                }
                Ok(())
            }
            ffi::LUA_TUSERDATA => Err("userdata que não é um valor imutável da engine".into()),
            ffi::LUA_TTHREAD => Err("corrotina guardada no módulo".into()),
            ffi::LUA_TBUFFER => Err(
                "buffer guardado no módulo (estado mutável que o congelamento não alcança)".into(),
            ),
            ffi::LUA_TINTEGER => {
                Err("inteiro de 64 bits (integer) não é aceito nos scripts".into())
            }
            other => Err(format!("tipo de valor {other} desconhecido pela engine")),
        }
    }
}
