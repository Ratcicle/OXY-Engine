//! Script memory (design §4.3, decision Q2): the fields a script declares at its top in
//! `Module.state`, kept by the engine for each (object, script), so that nothing of the game
//! lives in the VM. `self.state` is a handle whose first level is a fixed record of those
//! fields: a scalar is read and written in the engine at once, checked at the line; a table is
//! built in Luau the first time a call reads it and is then "touched", as is a table assigned to
//! a field, and every touched table is checked and written back when the call ends, with or
//! without an error. A table is copied in and out: two fields that share one table in a call
//! hold two tables in the next.
//!
//! Tables go between Luau and the engine in one pass through `mlua::ffi`, under the C++
//! exception rule of [`crate::script`]: inside `exec_raw` or a C function Luau runs, never
//! inside a `catch_unwind`. Their keys are whole numbers or texts, and Luau gets them in the
//! canonical order (whole numbers rising, then texts in byte order), so `pairs` runs the same
//! on every machine. The values are those attributes hold (booleans, numbers, texts, vectors,
//! objects and surfaces) and tables of them. A field may start empty or hold a Vetor2 when
//! `Module.state` declares it with a marker of `param` (`alvo = param.object()`), as
//! `Module.attributes` does: an empty object or surface reads nil and takes nil, and a Vetor2
//! reads as a vector with z 0 and keeps a vector's x and y.
use super::api::{self, Marker};
use super::handles::{self, MARK_OBJECT, MARK_SURFACE};
use super::host::Shared;
use crate::document::Id;
use mlua::{Lua, Table, Value as LuaValue, ffi};
use std::os::raw::c_int;
use std::sync::Arc;

/// Levels of tables under a field.
pub(crate) const MAX_DEPTH: usize = 4;
/// Entries of one table.
pub(crate) const MAX_ENTRIES: usize = 1024;
/// What the memory of one (object, script) may hold, as [`ScriptValue::size`] counts it.
pub(crate) const MAX_BYTES: usize = 64 * 1024;
/// The longest text, in bytes.
pub(crate) const MAX_TEXT: usize = 4096;

/// A key of a table: a whole number or a text. The order is the canonical one.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Key {
    Int(i64),
    Text(Box<str>),
}

/// A value of script memory or of a message.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ScriptValue {
    Bool(bool),
    Number(f64),
    Text(Box<str>),
    Vector([f32; 3]),
    /// A field `param.vector2(x, y)` declares: a vector with z 0 in Luau.
    Vector2([f32; 2]),
    /// Kept by ID: it reads as an object that does not exist once the object is gone. Empty only
    /// in a field `param.object()` declares, where it reads nil.
    Object(Option<Id>),
    /// A surface of the project, by ID; empty only in a field `param.surface()` declares.
    Surface(Option<Id>),
    /// Entries in the order of their keys.
    Table(Vec<(Key, ScriptValue)>),
}

impl ScriptValue {
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Self::Bool(_) => "booleano",
            Self::Number(_) => "número",
            Self::Text(_) => "texto",
            Self::Vector(_) => "vetor",
            Self::Vector2(_) => "vetor 2D",
            Self::Object(_) => "objeto",
            Self::Surface(_) => "superfície",
            Self::Table(_) => "tabela",
        }
    }

    /// What the value weighs against [`MAX_BYTES`]: the bytes of its texts, IDs and text keys,
    /// 8 for a number, a whole key or a 2D vector, 12 for a vector and 1 for a boolean.
    pub(crate) fn size(&self) -> usize {
        match self {
            Self::Bool(_) => 1,
            Self::Number(_) | Self::Vector2(_) => 8,
            Self::Text(text) => text.len(),
            Self::Vector(_) => 12,
            Self::Object(id) | Self::Surface(id) => id.as_ref().map_or(0, |id| id.len()),
            Self::Table(entries) => entries
                .iter()
                .map(|(key, value)| key_size(key) + value.size())
                .sum(),
        }
    }
}

fn key_size(key: &Key) -> usize {
    match key {
        Key::Int(_) => 8,
        Key::Text(text) => text.len(),
    }
}

/// The fields of one (object, script), in the order of their names; also what `Module.state`
/// declares, with each field's default and kind.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Memory {
    fields: Vec<(Arc<str>, ScriptValue)>,
    /// The fields' names and sizes, against [`MAX_BYTES`].
    size: usize,
}

/// Why a value cannot go into script memory or a message: past a limit (the script stops on
/// its object) or of a kind memory does not keep. The place is the path from the value read to
/// the value refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Refusal {
    pub limit: bool,
    reason: String,
    place: String,
}

impl Refusal {
    fn kind(reason: impl Into<String>) -> Self {
        Self {
            limit: false,
            reason: reason.into(),
            place: String::new(),
        }
    }
    fn limit(reason: impl Into<String>) -> Self {
        Self {
            limit: true,
            reason: reason.into(),
            place: String::new(),
        }
    }
    /// The refusal of a value found under `key`.
    fn under(mut self, key: &Key) -> Self {
        let step = match key {
            Key::Int(i) => format!("[{i}]"),
            Key::Text(text) => format!(".{text}"),
        };
        self.place.insert_str(0, &step);
        self
    }
    /// The refusal of a value of the field `name` of `self.state`.
    fn in_field(mut self, name: &str) -> Self {
        self.place.insert_str(0, &format!("self.state.{name}"));
        self
    }
    /// The refusal as a sentence, its place first: `self.state.items[3]: não aceita função…`.
    pub(crate) fn message(&self) -> String {
        if self.place.is_empty() {
            self.reason.clone()
        } else {
            format!("{}: {}", self.place, self.reason)
        }
    }
    /// The refusal of a value read from `root`, an expression.
    pub(crate) fn at(&self, root: &str) -> String {
        format!("{root}{}: {}", self.place, self.reason)
    }
}

impl Memory {
    /// The fields with their values, in the order of their names.
    pub(crate) fn fields(&self) -> &[(Arc<str>, ScriptValue)] {
        &self.fields
    }

    fn position(&self, name: &str) -> Option<usize> {
        self.fields.binary_search_by(|(n, _)| (**n).cmp(name)).ok()
    }

    /// The value of the field `name`, or why it has none.
    pub(crate) fn field(&self, name: &str) -> Result<&ScriptValue, Refusal> {
        match self.position(name) {
            Some(i) => Ok(&self.fields[i].1),
            None => Err(self.undeclared(name)),
        }
    }

    /// Whether `name` is a field of the kind of `value`, or why not.
    pub(crate) fn check(&self, name: &str, value: &ScriptValue) -> Result<(), Refusal> {
        let old = self.field(name)?;
        if std::mem::discriminant(old) != std::mem::discriminant(value) {
            return Err(
                Refusal::kind(format!("guarda {}, não {}", old.kind(), value.kind()))
                    .in_field(name),
            );
        }
        Ok(())
    }

    /// Writes a scalar to the declared field `name`, of its kind.
    pub(crate) fn set(&mut self, name: &str, value: ScriptValue) -> Result<(), Refusal> {
        self.check(name, &value)?;
        let i = self.position(name).expect("checked above");
        let size = self.size - self.fields[i].1.size() + value.size();
        if size > MAX_BYTES {
            return Err(over_bytes(name));
        }
        self.size = size;
        self.fields[i].1 = value;
        Ok(())
    }

    /// Writes back the tables a call touched, by field: those memory keeps, or, past the limit
    /// of bytes, none of them. The first refusal in the order of the fields.
    pub(crate) fn write_back(
        &mut self,
        tables: Vec<(String, Result<ScriptValue, Refusal>)>,
    ) -> Result<(), Refusal> {
        let mut first = None;
        let mut size = self.size;
        let mut accepted = Vec::with_capacity(tables.len());
        for (name, value) in tables {
            let Some(i) = self.position(&name) else {
                continue;
            };
            match value {
                Ok(value) => {
                    size = size - self.fields[i].1.size() + value.size();
                    accepted.push((i, value));
                }
                Err(refusal) => {
                    first.get_or_insert_with(|| refusal.in_field(&name));
                }
            }
        }
        if size > MAX_BYTES {
            let names: Vec<&str> = accepted.iter().map(|(i, _)| &*self.fields[*i].0).collect();
            return Err(over_bytes(&names.join(", ")));
        }
        self.size = size;
        for (i, value) in accepted {
            if self.fields[i].1 != value {
                self.fields[i].1 = value;
            }
        }
        first.map_or(Ok(()), Err)
    }

    /// Brings this memory to the shape of `schema` after a reload: a new field starts at its
    /// default, a field of another kind goes back to it, and a field no longer declared goes.
    /// The names reset and dropped.
    pub(crate) fn reconcile(&mut self, schema: &Memory) -> (Vec<Arc<str>>, Vec<Arc<str>>) {
        let mut reset = Vec::new();
        let dropped = self
            .fields
            .iter()
            .filter(|(name, _)| schema.position(name).is_none())
            .map(|(name, _)| name.clone())
            .collect();
        let fields = schema
            .fields
            .iter()
            .map(|(name, default)| match self.field(name) {
                Ok(value) if std::mem::discriminant(value) == std::mem::discriminant(default) => {
                    (name.clone(), value.clone())
                }
                Ok(_) => {
                    reset.push(name.clone());
                    (name.clone(), default.clone())
                }
                Err(_) => (name.clone(), default.clone()),
            })
            .collect();
        *self = Self::from_fields(fields);
        (reset, dropped)
    }

    /// `fields` in the order of their names, with their size.
    fn from_fields(mut fields: Vec<(Arc<str>, ScriptValue)>) -> Self {
        fields.sort_by(|a, b| a.0.cmp(&b.0));
        let size = fields.iter().map(|(n, v)| n.len() + v.size()).sum();
        Self { fields, size }
    }

    fn undeclared(&self, name: &str) -> Refusal {
        let hint = match super::errors::closest(name, self.fields.iter().map(|(n, _)| &**n)) {
            Some(near) => format!("; quis dizer {near}?"),
            None => String::new(),
        };
        Refusal::kind(format!(
            "não é um campo da memória; declare-o no topo, em Modulo.state{hint}"
        ))
        .in_field(name)
    }
}

/// The memory went past its bytes when `fields` were written.
fn over_bytes(fields: &str) -> Refusal {
    Refusal::limit(format!(
        "a memória do script passou de {} KB neste objeto",
        MAX_BYTES / 1024
    ))
    .in_field(fields)
}

/// What `Module.state` declares: its fields by name with their defaults, checked as memory
/// checks what it keeps; a field a marker of `param` declares starts empty or as a Vetor2.
pub(crate) fn schema(lua: &Lua, shared: &Shared, table: &Table) -> Result<Memory, String> {
    let mut out: Result<Vec<(Arc<str>, ScriptValue)>, String> = Ok(Vec::new());
    // SAFETY: reads the argument and leaves the stack empty; `exec_raw` runs it protected.
    unsafe {
        lua.exec_raw::<()>(table, |l| {
            out = read_fields(l, shared);
            ffi::lua_settop(l, 0);
        })
        .map_err(|e| super::errors::describe(&e, "", 0).message)?;
    }
    let memory = Memory::from_fields(out?);
    if memory.size > MAX_BYTES {
        return Err(format!(
            "Modulo.state passa de {} KB, o limite da memória de um objeto",
            MAX_BYTES / 1024
        ));
    }
    Ok(memory)
}

/// # Safety
/// The table of fields is at index 1.
unsafe fn read_fields(
    l: *mut ffi::lua_State,
    shared: &Shared,
) -> Result<Vec<(Arc<str>, ScriptValue)>, String> {
    unsafe {
        let mut fields = Vec::new();
        let mut reader = Reader::new(shared, MAX_BYTES);
        ffi::lua_pushnil(l);
        while ffi::lua_next(l, 1) != 0 {
            let top = ffi::lua_gettop(l);
            let Some(name) = text_at(l, top - 1).and_then(|bytes| std::str::from_utf8(bytes).ok())
            else {
                return Err(
                    "os campos de Modulo.state têm nomes de texto, como { vida = 3 }".into(),
                );
            };
            let name: Arc<str> = name.into();
            let value = match api::marker_at(l, top) {
                Some(marker) => declared(&name, marker)?,
                None => reader
                    .value(l, top, 0)
                    .map_err(|refusal| refusal.at(&format!("Modulo.state.{name}")))?,
            };
            fields.push((name, value));
            ffi::lua_settop(l, top - 1);
        }
        Ok(fields)
    }
}

/// What a marker of `param` declares as the field `name` of `Module.state`: an empty object or
/// surface, or a Vetor2. A surface starts empty: memory names no surface of the project.
fn declared(name: &str, marker: Marker) -> Result<ScriptValue, String> {
    Ok(match marker {
        Marker::Object => ScriptValue::Object(None),
        Marker::Vector2(v) => ScriptValue::Vector2(v),
        Marker::Surface(None) => ScriptValue::Surface(None),
        Marker::Surface(Some(surface)) => {
            return Err(format!(
                "Modulo.state.{name}: na memória, param.surface() começa vazio e não leva o nome \
                 de uma superfície (\"{surface}\"); para começar com uma superfície do projeto, \
                 declare um atributo: Modulo.attributes = {{ {name} = param.surface(\"{surface}\") \
                 }}"
            ));
        }
    })
}

/// Reads values off the Luau stack into memory's form, counting their bytes against a limit.
pub(crate) struct Reader<'a> {
    shared: &'a Shared,
    bytes: usize,
    max_bytes: usize,
    /// Table entries read, the tables themselves included.
    pub entries: u64,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(shared: &'a Shared, max_bytes: usize) -> Self {
        Self {
            shared,
            bytes: 0,
            max_bytes,
            entries: 0,
        }
    }

    fn spend(&mut self, bytes: usize) -> Result<(), Refusal> {
        self.bytes += bytes;
        if self.bytes > self.max_bytes {
            return Err(Refusal::limit(format!(
                "passou de {} KB",
                self.max_bytes / 1024
            )));
        }
        Ok(())
    }

    /// The value at `idx`, `depth` tables down from where reading started.
    ///
    /// # Safety
    /// `idx` is a valid absolute index of `l`; the stack is as it was when this returns `Ok`.
    pub(crate) unsafe fn value(
        &mut self,
        l: *mut ffi::lua_State,
        idx: c_int,
        depth: usize,
    ) -> Result<ScriptValue, Refusal> {
        unsafe {
            let value = match ffi::lua_type(l, idx) {
                ffi::LUA_TBOOLEAN => ScriptValue::Bool(ffi::lua_toboolean(l, idx) != 0),
                ffi::LUA_TNUMBER => {
                    let n = ffi::lua_tonumberx(l, idx, std::ptr::null_mut());
                    if !n.is_finite() {
                        return Err(Refusal::kind("não aceita infinito nem NaN"));
                    }
                    ScriptValue::Number(n)
                }
                ffi::LUA_TSTRING => {
                    let text = text_at(l, idx)
                        .and_then(|bytes| std::str::from_utf8(bytes).ok())
                        .ok_or_else(|| Refusal::kind("não aceita texto que não é UTF-8"))?;
                    if text.len() > MAX_TEXT {
                        return Err(Refusal::kind(format!(
                            "não aceita texto de mais de {MAX_TEXT} bytes"
                        )));
                    }
                    ScriptValue::Text(text.into())
                }
                ffi::LUA_TVECTOR => {
                    let v = ffi::lua_tovector(l, idx);
                    let v = [*v, *v.add(1), *v.add(2)];
                    if !v.iter().all(|c| c.is_finite()) {
                        return Err(Refusal::kind("não aceita vetor com infinito ou NaN"));
                    }
                    ScriptValue::Vector(v)
                }
                ffi::LUA_TLIGHTUSERDATA => {
                    match handles::decode(ffi::lua_tolightuserdata(l, idx)) {
                        (MARK_OBJECT, key) => {
                            ScriptValue::Object(Some(self.shared.id(key).ok_or_else(|| {
                                Refusal::kind("não aceita um objeto desconhecido")
                            })?))
                        }
                        (MARK_SURFACE, key) => {
                            ScriptValue::Surface(Some(self.shared.surface_id(key).ok_or_else(
                                || Refusal::kind("não aceita uma superfície desconhecida"),
                            )?))
                        }
                        (mark, _) => {
                            return Err(Refusal::kind(format!(
                                "não aceita {}",
                                handles::mark_kind(mark)
                            )));
                        }
                    }
                }
                ffi::LUA_TTABLE => self.table(l, idx, depth + 1)?,
                ffi::LUA_TFUNCTION => return Err(Refusal::kind(not_kept("função"))),
                ffi::LUA_TTHREAD => return Err(Refusal::kind(not_kept("corrotina"))),
                ffi::LUA_TBUFFER => return Err(Refusal::kind(not_kept("buffer"))),
                ffi::LUA_TNIL => return Err(Refusal::kind(not_kept("nil"))),
                ffi::LUA_TUSERDATA if ffi::lua_userdatatag(l, idx) == super::math::QUAT_TAG => {
                    return Err(Refusal::kind(
                        "não aceita um quatérnio; guarde quat.to_euler(q), um vetor, e refaça-o \
                         com quat.from_euler",
                    ));
                }
                _ => return Err(Refusal::kind(not_kept("userdata"))),
            };
            match &value {
                ScriptValue::Table(_) => {}
                scalar => self.spend(scalar.size())?,
            }
            Ok(value)
        }
    }

    /// # Safety
    /// As for [`Reader::value`], with a table at `idx`.
    unsafe fn table(
        &mut self,
        l: *mut ffi::lua_State,
        idx: c_int,
        depth: usize,
    ) -> Result<ScriptValue, Refusal> {
        unsafe {
            if depth > MAX_DEPTH {
                return Err(Refusal::limit(format!(
                    "passou de {MAX_DEPTH} níveis de tabela (ou uma tabela contém a si mesma)"
                )));
            }
            if api::marker_at(l, idx).is_some() {
                return Err(Refusal::kind(
                    "não aceita um marcador param aqui; os marcadores declaram um campo de \
                     Modulo.state ou um atributo de Modulo.attributes, no primeiro nível",
                ));
            }
            if ffi::lua_getmetatable(l, idx) != 0 {
                ffi::lua_settop(l, -2);
                return Err(Refusal::kind(not_kept("tabela com metatabela")));
            }
            ffi::lua_checkstack(l, 4);
            let mut entries = Vec::new();
            ffi::lua_pushnil(l);
            while ffi::lua_next(l, idx) != 0 {
                let top = ffi::lua_gettop(l);
                let key = match ffi::lua_type(l, top - 1) {
                    ffi::LUA_TNUMBER => {
                        let n = ffi::lua_tonumberx(l, top - 1, std::ptr::null_mut());
                        if n.fract() != 0. || n.abs() > MAX_WHOLE {
                            return Err(Refusal::kind(format!(
                                "não aceita a chave {n}; as chaves são números inteiros ou textos"
                            )));
                        }
                        Key::Int(n as i64)
                    }
                    ffi::LUA_TSTRING => Key::Text(
                        text_at(l, top - 1)
                            .and_then(|bytes| std::str::from_utf8(bytes).ok())
                            .ok_or_else(|| Refusal::kind("não aceita chave que não é UTF-8"))?
                            .into(),
                    ),
                    other => {
                        return Err(Refusal::kind(format!(
                            "não aceita chave do tipo {}; as chaves são números inteiros ou \
                             textos",
                            kind_name(other)
                        )));
                    }
                };
                self.spend(key_size(&key))?;
                let value = self.value(l, top, depth).map_err(|r| r.under(&key))?;
                entries.push((key, value));
                if entries.len() > MAX_ENTRIES {
                    return Err(Refusal::limit(format!(
                        "uma tabela passou de {MAX_ENTRIES} entradas"
                    )));
                }
                ffi::lua_settop(l, top - 1);
            }
            self.entries += entries.len() as u64 + 1;
            entries.sort_unstable_by(|a, b| a.0.cmp(&b.0));
            Ok(ScriptValue::Table(entries))
        }
    }
}

/// Whole keys go up to 2^53, where every whole number is a number of Luau.
const MAX_WHOLE: f64 = 9_007_199_254_740_992.;

fn not_kept(kind: &str) -> String {
    format!(
        "não aceita {kind}; só booleanos, números, textos, vetores, objetos, superfícies e \
         tabelas deles"
    )
}

fn kind_name(kind: c_int) -> &'static str {
    match kind {
        ffi::LUA_TBOOLEAN => "booleano",
        ffi::LUA_TVECTOR => "vetor",
        ffi::LUA_TLIGHTUSERDATA => "objeto",
        ffi::LUA_TTABLE => "tabela",
        ffi::LUA_TFUNCTION => "função",
        _ => "desconhecido",
    }
}

/// The bytes of the string at `idx`, if it is one.
///
/// # Safety
/// `idx` is a valid index of `l`; the slice lives while the value stays on the stack.
unsafe fn text_at<'a>(l: *mut ffi::lua_State, idx: c_int) -> Option<&'a [u8]> {
    unsafe {
        if ffi::lua_type(l, idx) != ffi::LUA_TSTRING {
            return None;
        }
        let mut len = 0usize;
        let p = ffi::lua_tolstring(l, idx, &mut len);
        Some(std::slice::from_raw_parts(p as *const u8, len))
    }
}

/// Pushes `value`, registering an object's handle so that a script can hand it back even after
/// the object went. The table entries pushed, the tables included.
///
/// # Safety
/// `l` runs protected (`exec_raw`) or a C function Luau called: a memory error unwinds from here.
pub(crate) unsafe fn push(l: *mut ffi::lua_State, shared: &Shared, value: &ScriptValue) -> u64 {
    unsafe {
        match value {
            ScriptValue::Bool(b) => ffi::lua_pushboolean(l, c_int::from(*b)),
            ScriptValue::Number(n) => ffi::lua_pushnumber(l, *n),
            ScriptValue::Text(text) => {
                ffi::lua_pushlstring(l, text.as_ptr().cast(), text.len());
            }
            ScriptValue::Vector([x, y, z]) => ffi::lua_pushvector(l, *x, *y, *z),
            ScriptValue::Vector2([x, y]) => ffi::lua_pushvector(l, *x, *y, 0.),
            ScriptValue::Object(Some(id)) => {
                // A key another object holds keeps it (one chance in ~10⁹ for 10 thousand
                // objects): the handle names that object.
                let key = shared.register(id).unwrap_or_else(|_| handles::key(id));
                ffi::lua_pushlightuserdatatagged(l, handles::handle(MARK_OBJECT, key), 0);
            }
            ScriptValue::Surface(Some(id)) => {
                let key = shared.register_surface(id);
                ffi::lua_pushlightuserdatatagged(l, handles::handle(MARK_SURFACE, key), 0);
            }
            ScriptValue::Object(None) | ScriptValue::Surface(None) => ffi::lua_pushnil(l),
            ScriptValue::Table(entries) => return push_table(l, shared, entries),
        }
        0
    }
}

/// Pushes a table with `entries`, inserted in their (canonical) order. The entries pushed, the
/// tables included.
///
/// # Safety
/// As for [`push`].
pub(crate) unsafe fn push_table(
    l: *mut ffi::lua_State,
    shared: &Shared,
    entries: &[(Key, ScriptValue)],
) -> u64 {
    unsafe {
        ffi::lua_checkstack(l, 4);
        let whole = entries
            .iter()
            .take_while(|(k, _)| matches!(k, Key::Int(_)))
            .count();
        ffi::lua_createtable(l, whole as c_int, (entries.len() - whole) as c_int);
        let mut pushed = entries.len() as u64 + 1;
        for (key, value) in entries {
            match key {
                Key::Int(i) => ffi::lua_pushnumber(l, *i as f64),
                Key::Text(text) => {
                    ffi::lua_pushlstring(l, text.as_ptr().cast(), text.len());
                }
            }
            pushed += push(l, shared, value);
            ffi::lua_rawset(l, -3);
        }
        pushed
    }
}

/// `value` as a Luau value.
pub(crate) fn to_lua(lua: &Lua, shared: &Shared, value: &ScriptValue) -> mlua::Result<LuaValue> {
    let mut pushed = 0;
    // SAFETY: pushes exactly one value, which `exec_raw` returns.
    let value = unsafe {
        lua.exec_raw::<LuaValue>((), |l| {
            pushed = push(l, shared, value);
        })?
    };
    shared.count_converted(pushed);
    Ok(value)
}

/// A Luau value in memory's form, at most `max_bytes` of it: none for nil.
pub(crate) fn from_lua(
    lua: &Lua,
    shared: &Shared,
    value: &LuaValue,
    max_bytes: usize,
) -> Result<Option<ScriptValue>, Refusal> {
    if value.is_nil() {
        return Ok(None);
    }
    let mut out = Err(Refusal::kind(String::new()));
    let mut read = 0;
    // SAFETY: reads the argument and leaves the stack empty; `exec_raw` runs it protected.
    let ran = unsafe {
        lua.exec_raw::<()>(value, |l| {
            let mut reader = Reader::new(shared, max_bytes);
            out = reader.value(l, 1, 0);
            read = reader.entries;
            ffi::lua_settop(l, 0);
        })
    };
    shared.count_converted(read);
    ran.map_err(|e| Refusal::kind(super::errors::describe(&e, "", 0).message))?;
    out.map(Some)
}

/// Reads the tables of `touched` (field name → table) into memory's form, in the order of the
/// fields, and empties it. Each field has its own allowance of bytes; the record's limit is
/// [`Memory::write_back`]'s.
pub(crate) fn read_touched(
    lua: &Lua,
    shared: &Shared,
    touched: &Table,
) -> mlua::Result<Vec<(String, Result<ScriptValue, Refusal>)>> {
    let mut tables = Vec::new();
    let mut read = 0;
    // SAFETY: walks the argument with `lua_next`, leaving each pair as it found it, empties it
    // and leaves the stack empty; `exec_raw` runs it protected.
    let ran = unsafe {
        lua.exec_raw::<()>(touched, |l| {
            ffi::lua_pushnil(l);
            while ffi::lua_next(l, 1) != 0 {
                let top = ffi::lua_gettop(l);
                if let Some(name) =
                    text_at(l, top - 1).and_then(|bytes| std::str::from_utf8(bytes).ok())
                {
                    let mut reader = Reader::new(shared, MAX_BYTES);
                    let value = reader.value(l, top, 0);
                    read += reader.entries;
                    tables.push((name.to_owned(), value));
                }
                ffi::lua_settop(l, top - 1);
            }
            ffi::lua_cleartable(l, 1);
            ffi::lua_settop(l, 0);
        })
    };
    shared.count_converted(read);
    ran?;
    tables.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(tables)
}
