//! Objects as stable values (design §3.9): a light userdata whose value is a pure function of
//! the object's ID, so `t[obj]` hashes and orders the same on every machine, in every process
//! and after any restore. The 8 high bits are a mark that gives views of the same object
//! without allocating (`obj.attr` is the object with the "attributes" mark); `typeof` is
//! "Object" for all of them. The one metatable of light userdata has `__index`, `__newindex`,
//! `__namecall` and `__tostring` written in C through `mlua::ffi`: the key is read with
//! `lua_tolstring` and the method with `lua_namecallatom`, without creating mlua values.
//!
//! The surface scripts reach here is provisional (stage 2): the object's `name`, `attr`,
//! `exists()`, `has_attribute(name)` and the timers of `timers` (`self:after(...)` and the
//! others, written with mlua and called from the table of methods). The API of game scripts
//! comes with its own table.
use super::errors::closest;
use super::host::{Shared, door, locate_raw, peek};
use super::timers;
use crate::document::Value;
use crate::runtime::Runtime;
use mlua::{Function, LightUserData, Lua, Table, ffi};
use std::ffi::CStr;
use std::os::raw::{c_int, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

// The mark sits above a 56-bit key in the pointer's bits.
const _: () = assert!(usize::BITS == 64);

pub(crate) const MARK_OBJECT: u64 = 1;
pub(crate) const MARK_ATTRIBUTES: u64 = 2;
const KEY_MASK: u64 = (1 << 56) - 1;

/// The handle key of an object: its ID's portable hash in 56 bits, never 0.
pub(crate) fn key(id: &str) -> u64 {
    (crate::random::hash(&[id.as_bytes()]) & KEY_MASK).max(1)
}

pub(crate) fn handle(mark: u64, key: u64) -> *mut c_void {
    ((mark << 56) | key) as usize as *mut c_void
}

pub(crate) fn decode(pointer: *mut c_void) -> (u64, u64) {
    let raw = pointer as usize as u64;
    (raw >> 56, raw & KEY_MASK)
}

/// Two objects whose keys meet (one chance in ~10⁹ for 10 thousand objects): named, never by ID.
pub(crate) fn collision(runtime: &Runtime, a: &str, b: &str) -> String {
    let name = |id: &str| {
        runtime
            .entity(id)
            .map_or_else(|| "um objeto removido".to_owned(), |e| e.name.clone())
    };
    format!(
        "os objetos {} e {} têm a mesma chave nos scripts; duplique um deles de novo",
        name(a),
        name(b)
    )
}

const OBJECT_MEMBERS: [&str; 9] = [
    "name",
    "attr",
    "exists",
    "has_attribute",
    "after",
    "every",
    "sequence",
    "schedule",
    "cancel",
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Method {
    Exists = 1,
    HasAttribute = 2,
    /// One of `timers::METHODS`, an mlua function in the table of methods.
    Timer = 3,
}

const METHODS: [(&CStr, Method); 7] = [
    (c"exists", Method::Exists),
    (c"has_attribute", Method::HasAttribute),
    (c"after", Method::Timer),
    (c"every", Method::Timer),
    (c"sequence", Method::Timer),
    (c"schedule", Method::Timer),
    (c"cancel", Method::Timer),
];
const _: () = assert!(timers::METHODS.len() == 5);

fn method(name: &[u8]) -> Option<(&'static CStr, Method)> {
    METHODS.iter().find(|(n, _)| n.to_bytes() == name).copied()
}

/// Installs the metatable of light userdata, frozen.
pub(crate) fn install(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Table> {
    let me = LightUserData(Rc::as_ptr(shared) as *mut c_void);
    let methods = lua.create_table()?;
    timers::install(lua, shared, &methods)?;
    for (name, method) in METHODS {
        if method == Method::Timer {
            continue;
        }
        // SAFETY: the two arguments become the closure's upvalues and the closure is left as
        // the only value. `Shared` outlives the VM (the host closes the VM first).
        let f: Function = unsafe {
            lua.exec_raw((me, method as i32), |l| {
                ffi::lua_pushcclosurek(l, object_method, name.as_ptr(), 2, None);
            })?
        };
        methods.raw_set(name.to_string_lossy(), f)?;
    }
    methods.set_readonly(true);
    // SAFETY: as above.
    let (index, newindex, namecall, tostring): (Function, Function, Function, Function) = unsafe {
        (
            lua.exec_raw((me, methods.clone()), |l| {
                ffi::lua_pushcclosurek(l, object_index, c"Object.__index".as_ptr(), 2, None);
            })?,
            lua.exec_raw(me, |l| {
                ffi::lua_pushcclosurek(l, object_newindex, c"Object.__newindex".as_ptr(), 1, None);
            })?,
            lua.exec_raw((me, methods), |l| {
                ffi::lua_pushcclosurek(l, object_namecall, c"Object.__namecall".as_ptr(), 2, None);
            })?,
            lua.exec_raw(me, |l| {
                ffi::lua_pushcclosurek(l, object_tostring, c"Object.__tostring".as_ptr(), 1, None);
            })?,
        )
    };
    let mt = lua.create_table()?;
    mt.raw_set("__index", index)?;
    mt.raw_set("__newindex", newindex)?;
    mt.raw_set("__namecall", namecall)?;
    mt.raw_set("__tostring", tostring)?;
    mt.raw_set("__type", "Object")?;
    mt.raw_set("__metatable", "travado")?;
    mt.set_readonly(true);
    lua.set_type_metatable::<LightUserData>(Some(mt.clone()));
    Ok(mt)
}

/// What an access gives back, computed in Rust before anything touches the Lua stack.
enum Out {
    Nil,
    Bool(bool),
    Number(f64),
    Vector([f32; 3]),
    Text(String),
    Handle(u64, u64),
    Method(&'static CStr),
}

/// A value a script writes, read off the stack first.
enum In {
    Nil,
    Bool(bool),
    Number(f64),
    Vector([f32; 3]),
    Text(String),
    Handle(u64, u64),
    Other(&'static str),
}

impl In {
    fn kind(&self) -> &'static str {
        match self {
            In::Nil => "nil",
            In::Bool(_) => "booleano",
            In::Number(_) => "número",
            In::Vector(_) => "vetor",
            In::Text(_) => "texto",
            In::Handle(MARK_OBJECT, _) => "objeto",
            In::Handle(..) => "atributos de um objeto",
            In::Other(kind) => kind,
        }
    }
}

fn value_kind(value: &Value) -> &'static str {
    match value {
        Value::Number(_) => "número",
        Value::Text(_) => "texto",
        Value::Bool(_) => "booleano",
        Value::Vector3(_) => "vetor",
        Value::Vector2(_) => "vetor 2D",
        Value::Object(_) => "objeto",
        Value::Surface(_) => "superfície",
    }
}

fn gone() -> String {
    "o objeto não existe mais".into()
}

fn utf8(bytes: &[u8]) -> Result<&str, String> {
    std::str::from_utf8(bytes).map_err(|_| "nome que não é UTF-8".to_string())
}

fn index(shared: &Shared, mark: u64, key: u64, name: Option<&[u8]>) -> Result<Out, String> {
    match mark {
        MARK_OBJECT => {
            let name = name.unwrap_or_default();
            Ok(match name {
                b"attr" => Out::Handle(MARK_ATTRIBUTES, key),
                b"name" => {
                    let runtime = door(shared)?;
                    let handles = shared.handles.borrow();
                    let id = handles.get(&key).ok_or_else(gone)?;
                    Out::Text(runtime.entity(id).ok_or_else(gone)?.name.clone())
                }
                other => match method(other) {
                    Some((name, _)) => Out::Method(name),
                    None => return Err(unknown_member(other)),
                },
            })
        }
        MARK_ATTRIBUTES => {
            let name = utf8(name.ok_or("o nome de um atributo é um texto")?)?;
            let runtime = door(shared)?;
            let other = {
                let handles = shared.handles.borrow();
                let id = handles.get(&key).ok_or_else(gone)?;
                match runtime.attribute(id, name).map_err(|_| gone())? {
                    None | Some(Value::Object(None) | Value::Surface(_)) => return Ok(Out::Nil),
                    Some(Value::Number(n)) => return Ok(Out::Number(*n)),
                    Some(Value::Bool(b)) => return Ok(Out::Bool(*b)),
                    Some(Value::Text(text)) => return Ok(Out::Text(text.clone())),
                    Some(Value::Vector3(v)) => return Ok(Out::Vector(*v)),
                    Some(Value::Vector2([x, y])) => return Ok(Out::Vector([*x, *y, 0.])),
                    Some(Value::Object(Some(other))) => other.clone(),
                }
            };
            let key = shared
                .register(&other)
                .map_err(|first| collision(runtime, &first, &other))?;
            Ok(Out::Handle(MARK_OBJECT, key))
        }
        _ => Err("valor desconhecido da engine".into()),
    }
}

fn unknown_member(name: &[u8]) -> String {
    let name = String::from_utf8_lossy(name);
    match closest(&name, OBJECT_MEMBERS.into_iter().filter(|m| *m != name)) {
        Some(near) => format!("Object não tem '{name}'; quis dizer '{near}'?"),
        None => format!("Object não tem '{name}'"),
    }
}

/// `obj:name()` of a name that is no method.
fn not_a_method(name: &[u8]) -> String {
    match OBJECT_MEMBERS.into_iter().find(|m| m.as_bytes() == name) {
        Some(property) => {
            format!("{property} é uma propriedade, não um método (use obj.{property})")
        }
        None => unknown_member(name),
    }
}

fn newindex(shared: &Shared, mark: u64, key: u64, name: &[u8], value: In) -> Result<(), String> {
    let name = String::from_utf8_lossy(name);
    match mark {
        MARK_OBJECT if OBJECT_MEMBERS.contains(&name.as_ref()) => {
            return Err(format!("{name} é só leitura"));
        }
        MARK_OBJECT => {
            return Err(format!(
                "self não aceita campos soltos ({name}); use self.state ou um atributo"
            ));
        }
        MARK_ATTRIBUTES => {}
        _ => return Err("este valor é só leitura".into()),
    }
    let runtime = door(shared)?;
    let handles = shared.handles.borrow();
    let id = handles.get(&key).ok_or_else(gone)?;
    let old = runtime
        .attribute(id, &name)
        .map_err(|_| gone())?
        .ok_or_else(|| {
            format!("o atributo {name} não existe neste objeto; crie-o no painel de propriedades")
        })?;
    let new = match (old, value) {
        (Value::Number(_), In::Number(n)) => Value::Number(n),
        (Value::Text(_), In::Text(text)) => Value::Text(text),
        (Value::Bool(_), In::Bool(b)) => Value::Bool(b),
        (Value::Vector3(_), In::Vector(v)) => Value::Vector3(v),
        (Value::Vector2(_), In::Vector([x, y, _])) => Value::Vector2([x, y]),
        (Value::Object(_), In::Nil) => Value::Object(None),
        (Value::Surface(_), In::Nil) => Value::Surface(None),
        (Value::Object(_), In::Handle(MARK_OBJECT, key)) => {
            Value::Object(Some(handles.get(&key).ok_or_else(gone)?.clone()))
        }
        (old, value) => {
            return Err(format!(
                "o atributo {name} guarda {}, não {}",
                value_kind(old),
                value.kind()
            ));
        }
    };
    if !new.is_finite() {
        return Err(format!("o atributo {name} não aceita infinito nem NaN"));
    }
    runtime
        .set_attribute(id, &name, new)
        .map_err(|e| format!("atributo {name}: {e}"))
}

fn run(
    shared: &Shared,
    method: Method,
    mark: u64,
    key: u64,
    arg: Option<&[u8]>,
) -> Result<bool, String> {
    if mark != MARK_OBJECT {
        return Err("este método é de objetos".into());
    }
    let runtime = door(shared)?;
    let handles = shared.handles.borrow();
    let entity = handles.get(&key).and_then(|id| runtime.entity(id));
    match method {
        Method::Exists => Ok(entity.is_some()),
        Method::HasAttribute => {
            let name = arg.ok_or("has_attribute precisa do nome do atributo (texto)")?;
            let name = utf8(name)?;
            Ok(entity.ok_or_else(gone)?.attributes.contains_key(name))
        }
        Method::Timer => Err("método de temporizador fora da tabela de métodos".into()),
    }
}

/// Runs `f` behind a panic barrier: a panic marks the host, which reports it after the call.
/// Nothing in `f` touches the Lua stack, so no C++ exception can meet the barrier.
fn guarded<T>(
    shared: &Shared,
    place: &str,
    f: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(result) => result,
        Err(panic) => {
            shared.poison(place, &*panic);
            Err(format!("erro interno da engine em {place}"))
        }
    }
}

/// # Safety
/// Upvalue 1 of the running C closure is the host's `Shared`, alive while the VM is.
unsafe fn shared_of<'a>(l: *mut ffi::lua_State) -> &'a Shared {
    unsafe { &*(ffi::lua_tolightuserdata(l, ffi::lua_upvalueindex(1)) as *const Shared) }
}

/// The bytes of the string at `idx`, if it is one.
///
/// # Safety
/// `idx` is a valid index of `l`; the slice lives while the value stays on the stack.
unsafe fn string_at<'a>(l: *mut ffi::lua_State, idx: c_int) -> Option<&'a [u8]> {
    unsafe {
        if ffi::lua_type(l, idx) != ffi::LUA_TSTRING {
            return None;
        }
        let mut len = 0usize;
        let p = ffi::lua_tolstring(l, idx, &mut len);
        Some(std::slice::from_raw_parts(p as *const u8, len))
    }
}

/// # Safety
/// `l` is running a C function; a memory error unwinds from here.
unsafe fn push_text(l: *mut ffi::lua_State, text: &str) {
    unsafe {
        ffi::lua_pushlstring(l, text.as_ptr().cast(), text.len());
    }
}

/// Raises `message` as a Lua error at the script line running. Rust values are dropped before
/// `lua_error`, which unwinds as a C++ exception.
///
/// # Safety
/// Called from a C function Luau runs, outside any `catch_unwind`.
unsafe fn raise(l: *mut ffi::lua_State, shared: &Shared, message: String) -> ! {
    unsafe {
        let at = locate_raw(l);
        shared.place_overrun(at.clone());
        let message = match at {
            Some(at) if !message.starts_with(&format!("{}:", at.path)) => {
                format!("{}:{}: {message}", at.path, at.line)
            }
            _ => message,
        };
        push_text(l, &message);
        drop(message);
        ffi::lua_error(l)
    }
}

/// # Safety
/// `l` is running a C function with a light userdata at 1.
unsafe fn handle_at(l: *mut ffi::lua_State, idx: c_int) -> Option<(u64, u64)> {
    unsafe {
        (ffi::lua_type(l, idx) == ffi::LUA_TLIGHTUSERDATA)
            .then(|| decode(ffi::lua_tolightuserdata(l, idx)))
    }
}

/// # Safety
/// Pushes one value of `out`; a memory error unwinds from here.
unsafe fn push(l: *mut ffi::lua_State, out: Out) {
    unsafe {
        match out {
            Out::Nil => ffi::lua_pushnil(l),
            Out::Bool(b) => ffi::lua_pushboolean(l, c_int::from(b)),
            Out::Number(n) => ffi::lua_pushnumber(l, n),
            Out::Vector([x, y, z]) => ffi::lua_pushvector(l, x, y, z),
            Out::Text(text) => push_text(l, &text),
            Out::Handle(mark, key) => ffi::lua_pushlightuserdatatagged(l, handle(mark, key), 0),
            Out::Method(name) => {
                ffi::lua_rawgetfield(l, ffi::lua_upvalueindex(2), name.as_ptr());
            }
        }
    }
}

/// Upvalues: the `Shared` and the table of methods.
unsafe extern "C-unwind" fn object_index(l: *mut ffi::lua_State) -> c_int {
    unsafe {
        let shared = shared_of(l);
        let (mark, key) = handle_at(l, 1).unwrap_or_default();
        let name = string_at(l, 2);
        match guarded(shared, "Object.__index", || index(shared, mark, key, name)) {
            Ok(out) => push(l, out),
            Err(message) => raise(l, shared, message),
        }
        1
    }
}

/// Upvalue: the `Shared`.
unsafe extern "C-unwind" fn object_newindex(l: *mut ffi::lua_State) -> c_int {
    unsafe {
        let shared = shared_of(l);
        let (mark, key) = handle_at(l, 1).unwrap_or_default();
        let name = string_at(l, 2).unwrap_or(b"?");
        let value = match ffi::lua_type(l, 3) {
            ffi::LUA_TNIL => In::Nil,
            ffi::LUA_TBOOLEAN => In::Bool(ffi::lua_toboolean(l, 3) != 0),
            ffi::LUA_TNUMBER => In::Number(ffi::lua_tonumberx(l, 3, std::ptr::null_mut())),
            ffi::LUA_TVECTOR => {
                let v = ffi::lua_tovector(l, 3);
                In::Vector([*v, *v.add(1), *v.add(2)])
            }
            ffi::LUA_TLIGHTUSERDATA => {
                let (mark, key) = decode(ffi::lua_tolightuserdata(l, 3));
                In::Handle(mark, key)
            }
            ffi::LUA_TSTRING => match string_at(l, 3).map(|b| std::str::from_utf8(b)) {
                Some(Ok(text)) => In::Text(text.to_owned()),
                _ => In::Other("texto que não é UTF-8"),
            },
            ffi::LUA_TTABLE => In::Other("tabela"),
            ffi::LUA_TFUNCTION => In::Other("função"),
            ffi::LUA_TTHREAD => In::Other("corrotina"),
            _ => In::Other("userdata"),
        };
        if let Err(message) = guarded(shared, "Object.__newindex", || {
            newindex(shared, mark, key, name, value)
        }) {
            raise(l, shared, message);
        }
        0
    }
}

/// # Safety
/// Called from a C function Luau runs, with the object at 1 and the arguments after it.
unsafe fn run_method(l: *mut ffi::lua_State, shared: &Shared, method: Method) -> c_int {
    unsafe {
        let Some((mark, key)) = handle_at(l, 1) else {
            raise(
                l,
                shared,
                "o método precisa de um objeto antes de ':'".into(),
            )
        };
        let arg = string_at(l, 2);
        match guarded(shared, "Object", || run(shared, method, mark, key, arg)) {
            Ok(b) => ffi::lua_pushboolean(l, c_int::from(b)),
            Err(message) => raise(l, shared, message),
        }
        1
    }
}

/// A method of the table of methods (upvalue 2 of the running closure), called with the
/// object and the arguments as they came; its errors unwind from it.
///
/// # Safety
/// Called from a C function Luau runs, with nothing of Rust to drop.
unsafe fn call_listed(l: *mut ffi::lua_State, name: &CStr) -> c_int {
    unsafe {
        let n = ffi::lua_gettop(l);
        ffi::lua_rawgetfield(l, ffi::lua_upvalueindex(2), name.as_ptr());
        ffi::lua_insert(l, 1);
        ffi::lua_call(l, n, ffi::LUA_MULTRET);
        ffi::lua_gettop(l)
    }
}

/// `obj:method(...)`. Upvalues: the `Shared` and the table of methods.
unsafe extern "C-unwind" fn object_namecall(l: *mut ffi::lua_State) -> c_int {
    unsafe {
        let shared = shared_of(l);
        let atom = ffi::lua_namecallatom(l, std::ptr::null_mut());
        let name = if atom.is_null() {
            &[][..]
        } else {
            CStr::from_ptr(atom).to_bytes()
        };
        match method(name) {
            Some((name, Method::Timer)) => call_listed(l, name),
            Some((_, m)) => run_method(l, shared, m),
            None => raise(l, shared, not_a_method(name)),
        }
    }
}

/// A method read as a value (`local f = obj.exists`). Upvalues: the `Shared` and the method.
unsafe extern "C-unwind" fn object_method(l: *mut ffi::lua_State) -> c_int {
    unsafe {
        let shared = shared_of(l);
        let number = ffi::lua_tonumberx(l, ffi::lua_upvalueindex(2), std::ptr::null_mut());
        let method = if number == Method::Exists as i32 as f64 {
            Method::Exists
        } else {
            Method::HasAttribute
        };
        run_method(l, shared, method)
    }
}

/// The object's name; "objeto" outside a call. Upvalue: the `Shared`.
unsafe extern "C-unwind" fn object_tostring(l: *mut ffi::lua_State) -> c_int {
    unsafe {
        let shared = shared_of(l);
        let (mark, key) = handle_at(l, 1).unwrap_or_default();
        let text = guarded(shared, "Object.__tostring", || {
            let name = peek(shared)
                .zip(shared.id(key))
                .and_then(|(runtime, id)| runtime.entity(&id).map(|e| e.name.clone()))
                .unwrap_or_else(|| "objeto".into());
            Ok(match mark {
                MARK_ATTRIBUTES => format!("atributos de {name}"),
                _ => name,
            })
        });
        match text {
            Ok(text) => push_text(l, &text),
            Err(message) => raise(l, shared, message),
        }
        1
    }
}
