//! Objects as stable values (design §3.9): a light userdata whose value is a pure function of
//! the object's ID, so `t[obj]` hashes and orders the same on every machine, in every process
//! and after any restore. The 8 high bits are a mark that gives views of the same object
//! without allocating (`obj.attr` is the object with the "attributes" mark); `typeof` is
//! "Object" for all of them. The one metatable of light userdata has `__index`, `__newindex`,
//! `__namecall`, `__tostring` and `__len` written in C through `mlua::ffi`: the key is read
//! with `lua_tolstring` and the method with `lua_namecallatom`, without creating mlua values.
//!
//! `self.state` is the object with the "memory" mark (see `memory`): its fields go through the
//! same C functions, and a table read from it or assigned to it stays in a table of touched
//! tables (an upvalue of `__index` and `__newindex`) until the call ends and the host writes it
//! back. That table is emptied by the first table a call touches, so nothing of an earlier call
//! is read from it.
//!
//! What an object has for scripts is the registry's (`registry`): its properties ([`Property`])
//! are resolved here by name, `obj:exists()` and `obj:has_attribute(name)` run in C
//! ([`FastMethod`]), and the other methods (the timers of `timers`, `obj:send` of `messages`)
//! are mlua functions in the table of methods, made from the registry's rows.
//! - `obj.attr`: its attributes, `obj.attr.Vida` (nil when it lacks one) and `obj.attr.Vida = 3`
//!   (it must have it, with a value of the same type; a Vetor2 takes a vector's x and y);
//! - `obj.position` and `obj.world_position`: where it stands, in metres, as a vector: in its
//!   parent's axes, as Properties shows it, and its pivot in the world (a 3D character's
//!   origin); writing one moves it as Mover / girar / escalar objeto does, refusing what that
//!   refuses (a 3D character, an object a playing clip poses);
//! - `self.state`, the memory of the script running (see `memory`).
use super::errors::closest;
use super::host::{Limit, Phase, Shared, door, locate_raw, peek};
use super::memory::{self, Key, Memory, ScriptValue};
use super::registry::{self, Imp};
use super::value_kind;
use crate::document::Value;
use crate::runtime::{Runtime, TransformPart, TransformSpace};
use glam::Vec3;
use mlua::{Function, LightUserData, Lua, Table, ffi};
use std::ffi::CStr;
use std::os::raw::{c_char, c_int, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

// The mark sits above a 56-bit key in the pointer's bits.
const _: () = assert!(usize::BITS == 64);

pub(crate) const MARK_OBJECT: u64 = 1;
pub(crate) const MARK_ATTRIBUTES: u64 = 2;
pub(crate) const MARK_STATE: u64 = 3;
const KEY_MASK: u64 = (1 << 56) - 1;

/// What a handle with `mark` is, in words.
pub(crate) fn mark_kind(mark: u64) -> &'static str {
    match mark {
        MARK_OBJECT => "objeto",
        MARK_ATTRIBUTES => "os atributos de um objeto",
        MARK_STATE => "a memória de um objeto",
        _ => "valor desconhecido da engine",
    }
}

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

/// The properties of objects, which `__index` and `__newindex` resolve.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Property {
    Name,
    Attr,
    State,
    Position,
    WorldPosition,
}

impl Property {
    /// Whether a script may assign it.
    pub(crate) fn writable(self) -> bool {
        matches!(self, Self::Position | Self::WorldPosition)
    }
}

/// The methods of objects that run in C, without a call of their own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FastMethod {
    Exists = 1,
    HasAttribute = 2,
}

/// Why `self.state` cannot be measured, walked or copied.
const STATE_SHAPE: &str = "self.state não se mede, não se percorre e não se copia: o primeiro \
                           nível da memória é um registro com os campos declarados em \
                           Modulo.state; guarde uma lista num campo (self.state.itens) para \
                           usar #, pairs ou table.clone (veja “Memória do script” no Guia)";

/// Installs the metatable of light userdata, frozen; the metatable and the table of touched
/// tables of script memory.
pub(crate) fn install(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<(Table, Table)> {
    let me = LightUserData(Rc::as_ptr(shared) as *mut c_void);
    let methods = lua.create_table()?;
    let touched = lua.create_table()?;
    for f in registry::API {
        let method: Function = match f.imp {
            Imp::Method(make) => make(lua, shared)?,
            // SAFETY: the two arguments become the closure's upvalues and the closure is left
            // as the only value. `Shared` outlives the VM (the host closes the VM first).
            Imp::FastMethod(method) => unsafe {
                lua.exec_raw((me, method as i32), |l| {
                    ffi::lua_pushcclosurek(l, object_method, c"Object method".as_ptr(), 2, None);
                })?
            },
            _ => continue,
        };
        methods.raw_set(f.member(), method)?;
    }
    methods.set_readonly(true);
    // The tests read what scripts reach.
    #[cfg(test)]
    lua.set_named_registry_value("oxy.methods", &methods)?;
    // SAFETY: as above.
    let (index, newindex, namecall, tostring, len): (
        Function,
        Function,
        Function,
        Function,
        Function,
    ) = unsafe {
        (
            lua.exec_raw((me, methods.clone(), touched.clone()), |l| {
                ffi::lua_pushcclosurek(l, object_index, c"Object.__index".as_ptr(), 3, None);
            })?,
            lua.exec_raw((me, touched.clone()), |l| {
                ffi::lua_pushcclosurek(l, object_newindex, c"Object.__newindex".as_ptr(), 2, None);
            })?,
            lua.exec_raw((me, methods), |l| {
                ffi::lua_pushcclosurek(l, object_namecall, c"Object.__namecall".as_ptr(), 2, None);
            })?,
            lua.exec_raw(me, |l| {
                ffi::lua_pushcclosurek(l, object_tostring, c"Object.__tostring".as_ptr(), 1, None);
            })?,
            lua.exec_raw(me, |l| {
                ffi::lua_pushcclosurek(l, object_len, c"Object.__len".as_ptr(), 1, None);
            })?,
        )
    };
    let mt = lua.create_table()?;
    mt.raw_set("__index", index)?;
    mt.raw_set("__newindex", newindex)?;
    mt.raw_set("__namecall", namecall)?;
    mt.raw_set("__tostring", tostring)?;
    mt.raw_set("__len", len)?;
    mt.raw_set("__type", "Object")?;
    mt.raw_set("__metatable", "travado")?;
    mt.set_readonly(true);
    lua.set_type_metatable::<LightUserData>(Some(mt.clone()));
    Ok((mt, touched))
}

/// What an access gives back, computed in Rust before anything touches the Lua stack.
enum Out {
    Nil,
    Bool(bool),
    Number(f64),
    Vector([f32; 3]),
    Text(String),
    Handle(u64, u64),
    /// The method under the key, from the table of methods.
    Method,
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
            In::Handle(mark, _) => mark_kind(*mark),
            In::Other(kind) => kind,
        }
    }
}

/// The property objects have under `name`, none for a method; why not when they have neither.
fn property(name: &[u8]) -> Result<Option<Property>, String> {
    match registry::object_member(name) {
        Some(f) => Ok(match f.imp {
            Imp::Property(property) => Some(property),
            _ => None,
        }),
        None => Err(unknown_member(name)),
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
            let Some(property) = property(name.unwrap_or_default())? else {
                return Ok(Out::Method);
            };
            Ok(match property {
                Property::Attr => Out::Handle(MARK_ATTRIBUTES, key),
                Property::State => {
                    // Its fields count the units; outside a call the door says why not.
                    if shared.phase.get() != Phase::Running {
                        door(shared)?;
                    }
                    state_module(shared, key)?;
                    Out::Handle(MARK_STATE, key)
                }
                Property::Name => {
                    let runtime = door(shared)?;
                    let handles = shared.handles.borrow();
                    let id = handles.get(&key).ok_or_else(gone)?;
                    Out::Text(runtime.entity(id).ok_or_else(gone)?.name.clone())
                }
                Property::Position | Property::WorldPosition => {
                    let space = if property == Property::Position {
                        TransformSpace::Local
                    } else {
                        TransformSpace::World
                    };
                    let runtime = door(shared)?;
                    let handles = shared.handles.borrow();
                    let id = handles.get(&key).ok_or_else(gone)?;
                    if runtime.entity(id).is_none() {
                        return Err(gone());
                    }
                    let (position, _) = runtime.object_pose(id, space)?;
                    Out::Vector(position.to_array())
                }
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

/// Only the object running reaches its memory, that of the module running, which declares one.
fn state_module(shared: &Shared, key: u64) -> Result<Rc<super::Module>, String> {
    if shared.caller_key() != Some(key) {
        return Err(
            "só self.state existe: a memória de outro objeto não é acessível (use atributos ou \
             mensagens)"
                .into(),
        );
    }
    let module = shared
        .module()
        .ok_or("memória fora de uma chamada de script")?;
    if module.state.is_none() {
        return Err(
            "este script não declara memória; declare os campos no topo, como Modulo.state = { \
             vida = 3 }"
                .into(),
        );
    }
    Ok(module)
}

/// The memory of the object running for the module running, made with its defaults the first
/// time.
fn state_door(shared: &Shared, key: u64) -> Result<&mut Memory, String> {
    let runtime = door(shared)?;
    let module = state_module(shared, key)?;
    let schema = module.state.as_ref().expect("checked by state_module");
    let handles = shared.handles.borrow();
    let owner = handles
        .get(&key)
        .filter(|id| runtime.entity(id).is_some())
        .ok_or_else(gone)?;
    Ok(runtime.script_memory(owner, &module.path, schema))
}

/// What a field of `self.state` reads as: a scalar, or a table to build from the engine's.
enum StateOut {
    Value(Out),
    /// Into the memory of the `Runtime`, which nothing changes before the table is built.
    Table(*const [(Key, ScriptValue)]),
}

fn state_read(shared: &Shared, key: u64, name: &[u8]) -> Result<StateOut, String> {
    let name = utf8(name)?;
    let memory = state_door(shared, key)?;
    let out = match memory.field(name).map_err(|r| r.message())? {
        ScriptValue::Table(entries) => return Ok(StateOut::Table(entries.as_slice())),
        ScriptValue::Bool(b) => Out::Bool(*b),
        ScriptValue::Number(n) => Out::Number(*n),
        ScriptValue::Text(text) => Out::Text(text.to_string()),
        ScriptValue::Vector(v) => Out::Vector(*v),
        ScriptValue::Object(id) => {
            let id = id.clone();
            Out::Handle(
                MARK_OBJECT,
                shared.register(&id).unwrap_or_else(|_| self::key(&id)),
            )
        }
    };
    Ok(StateOut::Value(out))
}

/// A field of `self.state` takes a scalar: checked here and written in the engine.
fn state_write(shared: &Shared, key: u64, name: &[u8], value: In) -> Result<(), String> {
    let name = String::from_utf8_lossy(name);
    let refused = |what: &str| format!("self.state.{name}: não aceita {what}");
    let value = match value {
        In::Bool(b) => ScriptValue::Bool(b),
        In::Number(n) if n.is_finite() => ScriptValue::Number(n),
        In::Number(_) => return Err(refused("infinito nem NaN")),
        In::Text(text) if text.len() > memory::MAX_TEXT => {
            return Err(refused(&format!(
                "texto de mais de {} bytes",
                memory::MAX_TEXT
            )));
        }
        In::Text(text) => ScriptValue::Text(text.into()),
        In::Vector(v) if v.iter().all(|c| c.is_finite()) => ScriptValue::Vector(v),
        In::Vector(_) => return Err(refused("vetor com infinito ou NaN")),
        In::Handle(MARK_OBJECT, k) => ScriptValue::Object(shared.id(k).ok_or_else(gone)?),
        other => {
            let memory = state_door(shared, key)?;
            let kind = memory.field(&name).map_err(|r| r.message())?.kind();
            return Err(format!(
                "self.state.{name}: guarda {kind}, não {}",
                other.kind()
            ));
        }
    };
    let memory = state_door(shared, key)?;
    memory.set(&name, value).map_err(|refusal| {
        if refusal.limit {
            shared.overran(Limit::ScriptMemory, None);
        }
        refusal.message()
    })
}

/// A field of `self.state` takes a table, which the call then touches: only a table field.
fn state_assign(shared: &Shared, key: u64, name: &[u8]) -> Result<(), String> {
    let name = String::from_utf8_lossy(name);
    let memory = state_door(shared, key)?;
    memory
        .check(&name, &ScriptValue::Table(Vec::new()))
        .map_err(|r| r.message())
}

/// The member of objects nearest to `name`, as a hint: `; quis dizer 'position'?`.
fn nearest_member(name: &str) -> String {
    closest(name, registry::object_members().filter(|m| *m != name))
        .map(|near| format!("; quis dizer '{near}'?"))
        .unwrap_or_default()
}

fn unknown_member(name: &[u8]) -> String {
    let name = String::from_utf8_lossy(name);
    format!("Object não tem '{name}'{}", nearest_member(&name))
}

/// `obj:name()` of a name that is no method.
fn not_a_method(name: &[u8]) -> String {
    match registry::object_member(name) {
        Some(f) => {
            let property = f.member();
            format!("{property} é uma propriedade, não um método (use obj.{property})")
        }
        None => unknown_member(name),
    }
}

fn newindex(shared: &Shared, mark: u64, key: u64, name: &[u8], value: In) -> Result<(), String> {
    if mark == MARK_OBJECT {
        let member = registry::object_member(name);
        let name = String::from_utf8_lossy(name);
        return match member.map(|f| f.imp) {
            Some(Imp::Property(property)) if property.writable() => {
                let space = if property == Property::Position {
                    TransformSpace::Local
                } else {
                    TransformSpace::World
                };
                write_position(shared, key, &name, space, value)
            }
            Some(Imp::Method(_) | Imp::FastMethod(_)) => {
                Err(format!("{name} é um método e é só leitura"))
            }
            Some(_) => Err(format!("{name} é só leitura")),
            None => Err(format!(
                "self não aceita campos soltos ({name}{}); use self.state ou um atributo",
                nearest_member(&name)
            )),
        };
    }
    let name = String::from_utf8_lossy(name);
    if mark != MARK_ATTRIBUTES {
        return Err("este valor é só leitura".into());
    }
    let runtime = door(shared)?;
    let handles = shared.handles.borrow();
    let id = handles.get(&key).ok_or_else(gone)?;
    let old = runtime
        .attribute(id, &name)
        .map_err(|_| gone())?
        .ok_or_else(|| missing_attribute(runtime, id, &name, shared.caller_key() == Some(key)))?;
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

/// Why `obj.attr.name = v` fails on the object `id`, which lacks the attribute: with the nearest
/// name it has, and how it gets one, for `self` (`own`) through the script's declaration.
fn missing_attribute(runtime: &Runtime, id: &str, name: &str, own: bool) -> String {
    let entity = runtime.entity(id);
    let near = entity
        .and_then(|e| closest(name, e.attributes.keys().map(String::as_str)))
        .map_or_else(String::new, |near| format!(" (quis dizer {near}?)"));
    if own {
        format!(
            "o atributo {name} não existe neste objeto{near}; declare-o em Modulo.attributes \
             (anexar o script de novo cria o que falta) ou crie-o no objeto"
        )
    } else {
        let object = entity.map_or("o objeto", |e| e.name.as_str());
        format!(
            "o atributo {name} não existe em {object}{near}; crie-o nele ou declare-o em \
             Modulo.attributes de um script que ele anexa"
        )
    }
}

/// `obj.position = v` or `obj.world_position = v`: the object moves there, as Mover / girar /
/// escalar objeto moves it.
fn write_position(
    shared: &Shared,
    key: u64,
    name: &str,
    space: TransformSpace,
    value: In,
) -> Result<(), String> {
    let In::Vector(position) = value else {
        return Err(format!(
            "{name} recebe um vetor, como vector.create(0, 1, 0), não {}",
            value.kind()
        ));
    };
    let runtime = door(shared)?;
    let handles = shared.handles.borrow();
    let id = handles.get(&key).ok_or_else(gone)?;
    if runtime.entity(id).is_none() {
        return Err(gone());
    }
    runtime.write_transform(
        id,
        TransformPart::Position,
        space,
        Vec3::from(position),
        false,
        false,
    )
}

fn run(
    shared: &Shared,
    method: FastMethod,
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
        FastMethod::Exists => Ok(entity.is_some()),
        FastMethod::HasAttribute => {
            let name = arg.ok_or("has_attribute precisa do nome do atributo (texto)")?;
            let name = utf8(name)?;
            Ok(entity.ok_or_else(gone)?.attributes.contains_key(name))
        }
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
            Out::Method => {
                // The key at 2 names a method of the registry, which the table has.
                ffi::lua_pushvalue(l, 2);
                ffi::lua_rawget(l, ffi::lua_upvalueindex(2));
            }
        }
    }
}

/// Puts the value at `value` in the table of touched tables (upvalue `touched`) under the key at
/// `name`, emptying it first when it holds nothing of this call.
///
/// # Safety
/// `name` and `value` are valid indexes of `l`, run by a C function whose upvalue `touched` is
/// that table.
unsafe fn touch(
    l: *mut ffi::lua_State,
    shared: &Shared,
    name: c_int,
    value: c_int,
    touched: c_int,
) {
    unsafe {
        let touched = ffi::lua_upvalueindex(touched);
        if !shared.touched.replace(true) {
            ffi::lua_cleartable(l, touched);
        }
        ffi::lua_pushvalue(l, name);
        ffi::lua_pushvalue(l, value);
        ffi::lua_rawset(l, touched);
    }
}

/// A field of `self.state`: a table touched in this call, else the engine's value.
///
/// # Safety
/// Called from `object_index`, with the memory's handle at 1 and the key at 2.
unsafe fn state_index(l: *mut ffi::lua_State, shared: &Shared, key: u64) -> c_int {
    unsafe {
        let Some(name) = string_at(l, 2) else {
            raise(
                l,
                shared,
                "os campos de self.state têm nomes de texto".into(),
            )
        };
        if shared.touched.get() {
            ffi::lua_pushvalue(l, 2);
            if ffi::lua_rawget(l, ffi::lua_upvalueindex(3)) != ffi::LUA_TNIL {
                return 1;
            }
            ffi::lua_settop(l, -2);
        }
        match guarded(shared, "Object.state", || state_read(shared, key, name)) {
            Ok(StateOut::Value(out)) => push(l, out),
            Ok(StateOut::Table(entries)) => {
                // SAFETY: the entries are the Runtime's, unchanged since `state_read`.
                let pushed = memory::push_table(l, shared, &*entries);
                shared.count_converted(pushed);
                touch(l, shared, 2, ffi::lua_gettop(l), 3);
            }
            Err(message) => raise(l, shared, message),
        }
        1
    }
}

/// Upvalues: the `Shared`, the table of methods and the table of touched tables.
unsafe extern "C-unwind" fn object_index(l: *mut ffi::lua_State) -> c_int {
    unsafe {
        let shared = shared_of(l);
        let (mark, key) = handle_at(l, 1).unwrap_or_default();
        if mark == MARK_STATE {
            return state_index(l, shared, key);
        }
        let name = string_at(l, 2);
        match guarded(shared, "Object.__index", || index(shared, mark, key, name)) {
            Ok(out) => push(l, out),
            Err(message) => raise(l, shared, message),
        }
        1
    }
}

/// The value at `idx` as a script writes it.
///
/// # Safety
/// `idx` is a valid index of `l`.
unsafe fn read_in(l: *mut ffi::lua_State, idx: c_int) -> In {
    unsafe {
        match ffi::lua_type(l, idx) {
            ffi::LUA_TNIL => In::Nil,
            ffi::LUA_TBOOLEAN => In::Bool(ffi::lua_toboolean(l, idx) != 0),
            ffi::LUA_TNUMBER => In::Number(ffi::lua_tonumberx(l, idx, std::ptr::null_mut())),
            ffi::LUA_TVECTOR => {
                let v = ffi::lua_tovector(l, idx);
                In::Vector([*v, *v.add(1), *v.add(2)])
            }
            ffi::LUA_TLIGHTUSERDATA => {
                let (mark, key) = decode(ffi::lua_tolightuserdata(l, idx));
                In::Handle(mark, key)
            }
            ffi::LUA_TSTRING => match string_at(l, idx).map(|b| std::str::from_utf8(b)) {
                Some(Ok(text)) => In::Text(text.to_owned()),
                _ => In::Other("texto que não é UTF-8"),
            },
            ffi::LUA_TTABLE => In::Other("tabela"),
            ffi::LUA_TFUNCTION => In::Other("função"),
            ffi::LUA_TTHREAD => In::Other("corrotina"),
            _ => In::Other("userdata"),
        }
    }
}

/// Upvalues: the `Shared` and the table of touched tables.
unsafe extern "C-unwind" fn object_newindex(l: *mut ffi::lua_State) -> c_int {
    unsafe {
        let shared = shared_of(l);
        let (mark, key) = handle_at(l, 1).unwrap_or_default();
        if mark == MARK_STATE {
            let Some(name) = string_at(l, 2) else {
                raise(
                    l,
                    shared,
                    "os campos de self.state têm nomes de texto".into(),
                )
            };
            if ffi::lua_type(l, 3) == ffi::LUA_TTABLE {
                match guarded(shared, "Object.state", || state_assign(shared, key, name)) {
                    Ok(()) => touch(l, shared, 2, 3, 2),
                    Err(message) => raise(l, shared, message),
                }
                return 0;
            }
            let value = read_in(l, 3);
            if let Err(message) = guarded(shared, "Object.state", || {
                state_write(shared, key, name, value)
            }) {
                raise(l, shared, message);
            }
            return 0;
        }
        let name = string_at(l, 2).unwrap_or(b"?");
        let value = read_in(l, 3);
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
unsafe fn run_method(l: *mut ffi::lua_State, shared: &Shared, method: FastMethod) -> c_int {
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
/// Called from a C function Luau runs, with nothing of Rust to drop; `name` is the method's
/// name, NUL-terminated, which the table has.
unsafe fn call_listed(l: *mut ffi::lua_State, name: *const c_char) -> c_int {
    unsafe {
        let n = ffi::lua_gettop(l);
        ffi::lua_rawgetfield(l, ffi::lua_upvalueindex(2), name);
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
        match registry::object_member(name).map(|f| f.imp) {
            Some(Imp::Method(_)) => call_listed(l, atom),
            Some(Imp::FastMethod(method)) => run_method(l, shared, method),
            _ => raise(l, shared, not_a_method(name)),
        }
    }
}

/// A method read as a value (`local f = obj.exists`). Upvalues: the `Shared` and the method.
unsafe extern "C-unwind" fn object_method(l: *mut ffi::lua_State) -> c_int {
    unsafe {
        let shared = shared_of(l);
        let number = ffi::lua_tonumberx(l, ffi::lua_upvalueindex(2), std::ptr::null_mut());
        let method = if number == FastMethod::Exists as i32 as f64 {
            FastMethod::Exists
        } else {
            FastMethod::HasAttribute
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
                MARK_STATE => format!("memória de {name}"),
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

/// `#` of a handle: objects, their attributes and their memory have no length. Upvalue: the
/// `Shared`.
unsafe extern "C-unwind" fn object_len(l: *mut ffi::lua_State) -> c_int {
    unsafe {
        let shared = shared_of(l);
        let (mark, _) = handle_at(l, 1).unwrap_or_default();
        let message = match mark {
            MARK_STATE => STATE_SHAPE,
            MARK_ATTRIBUTES => "os atributos de um objeto não têm tamanho (#)",
            _ => "um objeto não tem tamanho (#)",
        };
        raise(l, shared, message.to_owned())
    }
}
