//! The VM scripts run in (design §3.10): one set of globals for every script, read-only behind
//! a proxy whose `__index` refuses unknown names at the line that reads them and whose
//! `__newindex` refuses globals with a hint; frozen metatables; no clock, disk, `buffer`,
//! `integer`, debug library, global draws or addresses in text.
use super::errors::closest;
use super::handles;
use super::host::{Extra, Shared, api, locate, located};
use super::{FOLDER, load};
use mlua::chunk::Compiler;
use mlua::{
    Function, Lua, LuaOptions, LuaString, MultiValue, StdLib, Table, Value as LuaValue, VmState,
    ffi,
};
use std::collections::HashSet;
use std::os::raw::c_int;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

/// No `os`, `debug`, `buffer` (mutable state the freeze cannot reach) or `integer` (a second
/// kind of number: `t[1]` and `t[1i]` would be different keys).
pub(crate) fn libraries() -> StdLib {
    StdLib::MATH
        | StdLib::STRING
        | StdLib::TABLE
        | StdLib::BIT
        | StdLib::UTF8
        | StdLib::VECTOR
        | StdLib::COROUTINE
}

/// Base functions scripts never see: loading code, environments, the collector and mlua's
/// `require`, which reads the disk.
const REMOVED: [&str; 7] = [
    "loadstring",
    "getfenv",
    "setfenv",
    "gcinfo",
    "collectgarbage",
    "newproxy",
    "require",
];

/// Names of Lua and Luau that scripts lack on purpose: no hint of a near name for them.
const ABSENT: [&str; 15] = [
    "os",
    "io",
    "debug",
    "buffer",
    "integer",
    "package",
    "load",
    "dofile",
    "loadfile",
    "loadstring",
    "getfenv",
    "setfenv",
    "gcinfo",
    "newproxy",
    "collectgarbage",
];

/// The engine's fixed compiler: optimization 2, line information, no types, and the builtin of
/// `setmetatable` off (it would set the metatable without the engine's check).
pub(crate) fn compiler(debug_level: u8) -> Compiler {
    Compiler::new()
        .set_optimization_level(2)
        .set_debug_level(debug_level)
        .set_type_info_level(0)
        .set_disabled_builtins(["setmetatable"])
}

/// A new VM with the sandbox, the object handles and the safety net of interrupts; with it, the
/// table of touched tables of script memory (see `handles`).
pub(crate) fn create(shared: &Rc<Shared>, extra: Extra) -> mlua::Result<(Lua, Table)> {
    let lua = Lua::new_with(libraries(), LuaOptions::new().catch_rust_panics(false))?;
    let base = lua.globals();
    for name in REMOVED {
        base.raw_set(name, LuaValue::Nil)?;
    }
    install_base(&lua, &base, shared)?;
    super::math::install(&lua)?;
    super::api::install(&lua, &base, shared)?;
    let (handles, touched) = handles::install(&lua, shared)?;
    extra(&lua, &base, shared)?;
    // The tests read what scripts reach.
    #[cfg(test)]
    lua.set_named_registry_value("oxy.base", &base)?;
    // Every host function the base reaches may sit in a module; any other (a coroutine.wrap,
    // a string.gmatch iterator) carries state of its own.
    {
        let mut known = shared.known_functions.borrow_mut();
        collect_functions(&LuaValue::Table(base.clone()), &mut known, 3);
        collect_functions(&LuaValue::Table(handles), &mut known, 3);
    }
    lua.sandbox(true)?;
    install_proxy(&lua, &base)?;
    lua.set_compiler(compiler(1));
    lua.set_memory_limit(shared.limits.get().memory)?;
    let s = shared.clone();
    lua.set_interrupt(move |lua| match s.interrupt() {
        None => Ok(VmState::Continue),
        Some(limit) => {
            if !s.has_overrun() {
                s.overran(limit, locate(lua));
            }
            Err(mlua::Error::runtime(limit.message(&s.limits.get())))
        }
    });
    Ok((lua, touched))
}

fn collect_functions(value: &LuaValue, known: &mut HashSet<usize>, depth: u32) {
    match value {
        LuaValue::Function(f) => {
            known.insert(f.to_pointer() as usize);
        }
        LuaValue::Table(t) if depth > 0 => {
            // The API's tables refuse unknown names through their metatable.
            if let Some(meta) = t.metatable() {
                collect_functions(&LuaValue::Table(meta), known, depth - 1);
            }
            for (_, v) in t.pairs::<LuaValue, LuaValue>().flatten() {
                collect_functions(&v, known, depth - 1);
            }
        }
        _ => {}
    }
}

/// Puts a proxy in place of the main thread's globals (mlua refuses `set_globals` once
/// sandboxed): an empty read-only table, `safeenv`, whose locked metatable reads the base and
/// refuses unknown names and writes. Then freezes the metatable of `vector`, which
/// `luaL_sandbox` leaves open (FFlag::LuauSandboxFreezesVectorMetatable is off).
fn install_proxy(lua: &Lua, base: &Table) -> mlua::Result<()> {
    let names: Vec<String> = base
        .pairs::<String, LuaValue>()
        .filter_map(|pair| pair.ok().map(|(name, _)| name))
        .collect();
    let read = {
        let base = base.clone();
        lua.create_function(move |lua, (_, key): (LuaValue, LuaValue)| {
            let value: LuaValue = base.raw_get(key.clone())?;
            if !value.is_nil() {
                return Ok(value);
            }
            let name = key.to_string().unwrap_or_default();
            if ABSENT.contains(&name.as_str()) {
                return Err(located(lua, format!("{name} não existe nos scripts")));
            }
            let hint = closest(&name, names.iter().map(String::as_str))
                .map(|near| format!("; quis dizer {near}?"))
                .unwrap_or_default();
            Err(located(
                lua,
                format!("variável global desconhecida: {name}{hint}"),
            ))
        })?
    };
    let write = lua.create_function(|lua, (_, key, _): (LuaValue, LuaValue, LuaValue)| {
        Err::<(), _>(located(
            lua,
            format!(
                "variáveis globais não existem nos scripts ({}); use local, self.state ou um \
                 atributo",
                key.to_string().unwrap_or_default()
            ),
        ))
    })?;
    let meta = lua.create_table()?;
    meta.raw_set("__index", read)?;
    meta.raw_set("__newindex", write)?;
    meta.raw_set("__metatable", "travado")?;
    meta.set_readonly(true);
    let proxy = lua.create_table()?;
    proxy.set_metatable(Some(meta))?;
    proxy.set_readonly(true);
    proxy.set_safeenv(true);
    // SAFETY: replaces the globals of the main thread with the argument and leaves the stack
    // empty; runs protected, so an allocation failure is an mlua error.
    unsafe {
        lua.exec_raw::<()>(proxy, |l| {
            ffi::lua_replace(l, ffi::LUA_GLOBALSINDEX);
            ffi::lua_pushvector(l, 0., 0., 0.);
            if ffi::lua_getmetatable(l, -1) != 0 {
                ffi::lua_setreadonly(l, -1, 1);
            }
            ffi::lua_settop(l, 0);
        })
    }
}

/// `print(...)`: a line in the log with the object's name and the script line.
pub(crate) fn print(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    logger(lua, shared, "print", false)
}

/// `warn(...)`: as `print`, marked as a warning.
pub(crate) fn warn(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    logger(lua, shared, "warn", true)
}

fn logger(
    lua: &Lua,
    shared: &Rc<Shared>,
    name: &'static str,
    warning: bool,
) -> mlua::Result<Function> {
    // SAFETY: pushes one C function.
    let tostring = unsafe { lua.create_c_function(engine_tostring)? };
    let s = shared.clone();
    lua.create_function(move |lua, args: MultiValue| {
        // The text first: a handle's `__tostring` reads the lent Runtime, which the door below
        // then holds.
        let mut parts = Vec::with_capacity(args.len());
        for value in &args {
            parts.push(show(&tostring, value, 0)?);
        }
        let at = locate(lua);
        api(lua, &s, name, |runtime| {
            let object = s
                .caller()
                .and_then(|id| runtime.entity(&id).map(|e| e.name.clone()))
                .unwrap_or_default();
            let text = parts.join(" ");
            let text = if warning {
                format!("{object} · aviso: {text}")
            } else {
                format!("{object} · {text}")
            };
            runtime.log_at(text, at);
            Ok(())
        })
    })
}

/// The standard functions `install_base` replaces: `tostring` and `string.format` without
/// addresses, `setmetatable` that keeps protected metatables and refuses weak tables, the draws
/// of `math` refused (they are the engine's) and the coroutines a call must finish. The engine's
/// own functions are the registry's; a test holds both lists to what scripts reach.
#[cfg(test)]
pub(crate) const SANDBOXED: [&str; 7] = [
    "tostring",
    "setmetatable",
    "math.random",
    "math.randomseed",
    "string.format",
    "coroutine.create",
    "coroutine.wrap",
];

fn install_base(lua: &Lua, base: &Table, shared: &Rc<Shared>) -> mlua::Result<()> {
    // SAFETY: pushes one C function.
    let tostring = unsafe { lua.create_c_function(engine_tostring)? };
    base.raw_set("tostring", tostring)?;
    let s = shared.clone();
    base.raw_set(
        "setmetatable",
        lua.create_function(move |lua, (table, meta): (Table, Option<Table>)| {
            guard(lua, &s, "setmetatable", || {
                if let Some(old) = table.metatable()
                    && !old.raw_get::<LuaValue>("__metatable")?.is_nil()
                {
                    return Err(located(lua, "a metatabela desta tabela é protegida".into()));
                }
                if let Some(meta) = &meta
                    && !meta.raw_get::<LuaValue>("__mode")?.is_nil()
                {
                    return Err(located(
                        lua,
                        "tabelas fracas (__mode) não existem nos scripts: elas dependem do \
                         coletor de lixo"
                            .into(),
                    ));
                }
                table.set_metatable(meta)?;
                Ok(table)
            })
        })?,
    )?;
    let math: Table = base.raw_get("math")?;
    let no_random = lua.create_function(|lua, _: MultiValue| {
        Err::<(), _>(located(
            lua,
            "math.random não existe nos scripts: os sorteios são da engine (random)".into(),
        ))
    })?;
    math.raw_set("random", no_random.clone())?;
    math.raw_set("randomseed", no_random)?;
    let string: Table = base.raw_get("string")?;
    let format: Function = string.raw_get("format")?;
    // SAFETY: the original format becomes the only upvalue and the closure is left alone.
    let format: Function = unsafe {
        lua.exec_raw(format, |l| {
            ffi::lua_pushcclosurek(l, engine_format, c"format".as_ptr(), 1, None);
        })?
    };
    string.raw_set("format", format)?;
    let coroutine: Table = base.raw_get("coroutine")?;
    let s = shared.clone();
    coroutine.raw_set(
        "create",
        lua.create_function(move |lua, f: Function| {
            let thread = lua.create_thread(f)?;
            s.threads.borrow_mut().push(thread.clone());
            Ok(thread)
        })?,
    )?;
    let s = shared.clone();
    coroutine.raw_set(
        "wrap",
        lua.create_function(move |lua, f: Function| {
            let thread = lua.create_thread(f)?;
            s.threads.borrow_mut().push(thread.clone());
            lua.create_function(move |_, args: MultiValue| thread.resume::<MultiValue>(args))
        })?,
    )?;
    Ok(())
}

/// `require(path)`: a module of the scripts' folder (see `resolve`), loaded the first time and
/// frozen.
pub(crate) fn require(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(move |lua, name: LuaString| {
        let name = name.to_string_lossy();
        guard(lua, &s, "require", || {
            let from = locate(lua).map(|at| at.path);
            let path = resolve(from.as_deref(), &name).map_err(|e| located(lua, e))?;
            // Noted before the file is looked for: a reload that takes away a module still
            // required finds which text to keep.
            if let Some(from) = from {
                s.requires
                    .borrow_mut()
                    .entry(from)
                    .or_default()
                    .insert(path.clone());
            }
            if s.sources.get(&path).is_none() {
                return Err(located(
                    lua,
                    format!("require(\"{name}\"): {path} não existe"),
                ));
            }
            match load::module(lua, &s, &path) {
                Ok(module) => Ok(module.table.clone()),
                Err(e) => Err(located(lua, format!("require(\"{name}\"): {e}"))),
            }
        })
    })
}

/// A pure host function behind the panic barrier (it reaches nothing of the world). `f` uses
/// only mlua's safe API.
fn guard<R>(
    lua: &Lua,
    shared: &Shared,
    name: &str,
    f: impl FnOnce() -> mlua::Result<R>,
) -> mlua::Result<R> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(result) => result,
        Err(panic) => {
            shared.poison(name, &*panic);
            Err(located(lua, format!("erro interno da engine em {name}")))
        }
    }
}

/// The path of the module `name` names from the script `from`, as Luau's require-by-string
/// reads it: `./` from the script's folder, `../` from the folder above, never out of
/// `scripts`; the file `<name>.luau`, whether or not it exists. `@native/` is reserved for Rust
/// modules of a game (0.7 has none) and other aliases do not exist.
fn resolve(from: Option<&str>, name: &str) -> Result<String, String> {
    if name.starts_with("@native/") {
        return Err(format!(
            "require(\"{name}\"): módulos Rust por jogo ainda não existem (@native/)"
        ));
    }
    if name.starts_with('@') {
        return Err(format!("require(\"{name}\"): esse apelido não existe"));
    }
    if !name.starts_with("./") && !name.starts_with("../") {
        return Err(format!(
            "require(\"{name}\"): o caminho começa com ./ ou ../, a partir deste script (como \
             require(\"./dados/cartas\"))"
        ));
    }
    let from = from.ok_or_else(|| format!("require(\"{name}\") fora de um script"))?;
    let mut parts: Vec<&str> = from.split('/').collect();
    parts.pop();
    for part in name.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.len() <= 1 {
                    return Err(format!("require(\"{name}\"): fora da pasta {FOLDER}"));
                }
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    Ok(format!("{}.luau", parts.join("/")))
}

/// What `print` writes of a value: text as it is, a table's first level, the rest as the
/// engine's `tostring`.
fn show(tostring: &Function, value: &LuaValue, depth: usize) -> mlua::Result<String> {
    Ok(match value {
        LuaValue::String(s) => s.to_string_lossy(),
        LuaValue::Table(t) if depth == 0 => {
            let mut items = Vec::new();
            let length = t.raw_len();
            for i in 1..=length.min(16) {
                items.push(show(tostring, &t.raw_get(i)?, 1)?);
            }
            let mut keyed = Vec::new();
            for pair in t.pairs::<LuaValue, LuaValue>() {
                let (key, value) = pair?;
                match &key {
                    LuaValue::Integer(i) if *i >= 1 && *i as usize <= length => {}
                    LuaValue::Number(n) if n.fract() == 0. && *n >= 1. && *n as usize <= length => {
                    }
                    LuaValue::String(s) => keyed.push((s.to_string_lossy(), value)),
                    _ => keyed.push((format!("[{}]", show(tostring, &key, 1)?), value)),
                }
            }
            keyed.sort_by(|a, b| a.0.cmp(&b.0));
            for (key, value) in keyed.iter().take(16usize.saturating_sub(items.len())) {
                items.push(format!("{key} = {}", show(tostring, value, 1)?));
            }
            if length + keyed.len() > items.len() {
                items.push("…".into());
            }
            format!("{{{}}}", items.join(", "))
        }
        LuaValue::Table(_) => "{…}".into(),
        other => tostring.call::<LuaString>(other.clone())?.to_string_lossy(),
    })
}

/// The engine's text of the value at `idx`: a `__tostring` when there is one, the kind for
/// tables, functions, coroutines, buffers and userdata (Luau would show the address), and
/// Luau's own text for the rest, so numbers read the same through the builtin and here.
///
/// # Safety
/// `idx` is a valid absolute index; pushes one string.
unsafe fn push_engine_text(l: *mut ffi::lua_State, idx: c_int) {
    unsafe {
        let kind = match ffi::lua_type(l, idx) {
            ffi::LUA_TTABLE => Some(c"tabela"),
            ffi::LUA_TFUNCTION => Some(c"função"),
            ffi::LUA_TTHREAD => Some(c"corrotina"),
            ffi::LUA_TBUFFER => Some(c"buffer"),
            ffi::LUA_TUSERDATA => Some(c"userdata"),
            _ => None,
        };
        match kind {
            Some(kind) if ffi::luaL_callmeta(l, idx, c"__tostring".as_ptr()) == 0 => {
                ffi::lua_pushstring(l, kind.as_ptr());
            }
            Some(_) => {
                if ffi::lua_type(l, -1) != ffi::LUA_TSTRING {
                    ffi::luaL_error(l, c"'__tostring' must return a string".as_ptr());
                }
            }
            None => {
                ffi::luaL_tolstring_(l, idx, std::ptr::null_mut());
            }
        }
    }
}

/// `tostring` without addresses (R6).
unsafe extern "C-unwind" fn engine_tostring(l: *mut ffi::lua_State) -> c_int {
    unsafe {
        ffi::luaL_checkany(l, 1);
        push_engine_text(l, 1);
        1
    }
}

/// `string.format` whose `%*` writes values as the engine's `tostring` does: Luau's own `%*`
/// shows the address of a table. The arguments `%*` takes are replaced by their text, then the
/// original (upvalue 1) formats. The scan follows Luau's `str_format`.
unsafe extern "C-unwind" fn engine_format(l: *mut ffi::lua_State) -> c_int {
    unsafe {
        let top = ffi::lua_gettop(l);
        let mut len = 0usize;
        let format = ffi::luaL_checklstring(l, 1, &mut len);
        let format = std::slice::from_raw_parts(format as *const u8, len);
        let mut arg: c_int = 1;
        let mut i = 0;
        while i < format.len() {
            if format[i] != b'%' {
                i += 1;
                continue;
            }
            i += 1;
            match format.get(i) {
                Some(b'%') => i += 1,
                Some(b'*') => {
                    i += 1;
                    arg += 1;
                    if arg <= top
                        && !matches!(
                            ffi::lua_type(l, arg),
                            ffi::LUA_TNIL
                                | ffi::LUA_TBOOLEAN
                                | ffi::LUA_TNUMBER
                                | ffi::LUA_TSTRING
                                | ffi::LUA_TVECTOR
                        )
                    {
                        push_engine_text(l, arg);
                        ffi::lua_replace(l, arg);
                    }
                }
                _ => {
                    arg += 1;
                    while i < format.len() && b"-+ #0".contains(&format[i]) {
                        i += 1;
                    }
                    while i < format.len() && (format[i].is_ascii_digit() || format[i] == b'.') {
                        i += 1;
                    }
                    i += 1;
                }
            }
        }
        ffi::lua_pushvalue(l, ffi::lua_upvalueindex(1));
        ffi::lua_insert(l, 1);
        ffi::lua_call(l, top, 1);
        1
    }
}
