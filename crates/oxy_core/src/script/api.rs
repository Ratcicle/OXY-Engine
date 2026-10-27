//! The global tables of the API (design §5.3, §5.4) and the functions of the base, installed
//! from the registry (`registry`): only its rows exist, with the values of a table (`time.dt`).
//! Their functions reach the world through the `Runtime`'s shared operations layer, so they
//! check, refuse and act as the nodes do, and name what the person named (sounds and game
//! variables by their names, never their IDs). The tables are frozen, and reading a name one
//! lacks is an error with the nearest name it has, or says it comes in 0.8 (`game.online`).
//! Here are `audio` (sounds, loops and volumes), `vars` and `param`.
//!
//! `param.object()`, `param.surface(name?)` and `param.vector2(x, y)` are the markers of
//! `Module.attributes` and `Module.state` for the types with no literal (design §3.4, §4.3):
//! frozen tables with the engine's locked metatable, which loading reads as the default.
use super::errors::closest;
use super::handles::{self, MARK_OBJECT};
use super::host::{Shared, api, located};
use super::registry::{self, Imp};
use crate::audio::{Channel, MAX_FADE};
use crate::document::{AssetKind, Id, Project, Value};
use crate::variables;
use mlua::{Function, Lua, Table, Value as LuaValue, ffi};
use std::os::raw::c_int;
use std::rc::Rc;

/// Sounds a message lists before it counts the rest.
const LISTED: usize = 10;

/// Puts in the base the registry's functions: those of the base, and the global tables with
/// theirs and their values (`time.dt`). The base turns read-only after it.
pub(crate) fn install(lua: &Lua, base: &Table, shared: &Rc<Shared>) -> mlua::Result<()> {
    let mut tables: Vec<(&'static str, Table, Vec<&'static str>)> = Vec::new();
    for f in registry::API {
        let value = match f.imp {
            Imp::Function(make) => LuaValue::Function(make(lua, shared)?),
            Imp::Constant(n) => LuaValue::Number(n),
            _ => continue,
        };
        let holder = f.holder();
        if holder.is_empty() {
            base.raw_set(f.name, value)?;
            continue;
        }
        let at = match tables.iter().position(|(name, ..)| *name == holder) {
            Some(at) => at,
            None => {
                tables.push((holder, lua.create_table()?, Vec::new()));
                tables.len() - 1
            }
        };
        let (_, table, members) = &mut tables[at];
        table.raw_set(f.member(), value)?;
        members.push(f.member());
    }
    for (name, table, members) in tables {
        lock(lua, &table, name, members)?;
        base.raw_set(name, table)?;
    }
    Ok(())
}

/// `audio.play(name, options?)`.
pub(crate) fn audio_play(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(move |lua, (name, options): (LuaValue, LuaValue)| {
        // The arguments first, outside the panic barrier: the options are a table.
        let name = text(&name, "audio.play precisa do nome do som (um texto)");
        let options = play_options(&options);
        api(lua, &s, "audio.play", |runtime| {
            let name = name?;
            let (volume, audience) = options?;
            if let Some(key) = audience
                && s.id(key).and_then(|id| runtime.entity(&id)).is_none()
            {
                return Err("audio.play: o objeto de audience não existe mais".into());
            }
            let asset = sound(runtime.project(), &name)?;
            runtime.play_sound(&asset, volume)
        })
    })
}

/// A channel of loops as scripts name it; `all` for both, where `function` takes it.
fn loop_channel(value: &LuaValue, function: &str, all: bool) -> Result<Option<Channel>, String> {
    let known = if all {
        "\"music\", \"effects\" ou \"all\""
    } else {
        "\"music\" ou \"effects\""
    };
    let refused = || format!("{function}: channel é {known}");
    match text(value, &refused())?.as_str() {
        "music" => Ok(Some(Channel::Music)),
        "effects" => Ok(Some(Channel::Effects)),
        "all" if all => Ok(None),
        other => Err(format!("{}, não \"{other}\"", refused())),
    }
}

/// A fade in seconds, from 0 to [`MAX_FADE`].
fn fade_of(value: &LuaValue, function: &str) -> Result<f64, String> {
    let fade = finite(
        value,
        &format!("{function}: fade é a transição em segundos"),
    )?;
    if !(0.0..=MAX_FADE).contains(&fade) {
        return Err(format!(
            "{function}: fade vai de 0 a {MAX_FADE} segundos; recebeu {fade}"
        ));
    }
    Ok(fade)
}

/// The options of `audio.loop`, read before the call reaches the world: the channel, the volume
/// and the fade.
fn loop_options(options: &LuaValue) -> Result<(Channel, f64, f64), String> {
    let mut found = (Channel::Music, 1., 0.);
    let table = match options {
        LuaValue::Nil => return Ok(found),
        LuaValue::Table(table) => table,
        other => {
            return Err(format!(
                "as opções de audio.loop são uma tabela, como {{ channel = \"effects\" }}, não {}",
                handles::In::of(other).kind()
            ));
        }
    };
    for pair in table.pairs::<LuaValue, LuaValue>() {
        let (key, value) = pair.map_err(|e| e.to_string())?;
        match option_name(&key, "audio.loop")?.as_str() {
            "channel" => {
                found.0 = loop_channel(&value, "audio.loop", false)?.expect("not all");
            }
            "volume" => found.1 = finite(&value, "audio.loop: volume é um número de 0 a 1")?,
            "fade" => found.2 = fade_of(&value, "audio.loop")?,
            other => {
                return Err(format!(
                    "audio.loop não conhece a opção {other}; use channel, volume ou fade"
                ));
            }
        }
    }
    Ok(found)
}

/// `audio.loop(name, options?)`.
pub(crate) fn audio_loop(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(move |lua, (name, options): (LuaValue, LuaValue)| {
        let name = text(&name, "audio.loop precisa do nome do som (um texto)");
        let options = loop_options(&options);
        api(lua, &s, "audio.loop", |runtime| {
            let (channel, volume, fade) = options?;
            let asset = sound(runtime.project(), &name?)?;
            runtime.play_sound_loop(&asset, channel, volume, fade)
        })
    })
}

/// The options of `audio.stop`: the sound's name, the channel (none for both) and the fade.
type StopOptions = (Option<String>, Option<Channel>, f64);

fn stop_options(options: &LuaValue) -> Result<StopOptions, String> {
    let mut found = (None, None, 0.);
    let table = match options {
        LuaValue::Nil => return Ok(found),
        LuaValue::Table(table) => table,
        other => {
            return Err(format!(
                "as opções de audio.stop são uma tabela, como {{ channel = \"music\" }}, não {}",
                handles::In::of(other).kind()
            ));
        }
    };
    for pair in table.pairs::<LuaValue, LuaValue>() {
        let (key, value) = pair.map_err(|e| e.to_string())?;
        match option_name(&key, "audio.stop")?.as_str() {
            "sound" => {
                found.0 = Some(text(
                    &value,
                    "audio.stop: sound é o nome do som (um texto)",
                )?);
            }
            "channel" => found.1 = loop_channel(&value, "audio.stop", true)?,
            "fade" => found.2 = fade_of(&value, "audio.stop")?,
            other => {
                return Err(format!(
                    "audio.stop não conhece a opção {other}; use sound, channel ou fade"
                ));
            }
        }
    }
    Ok(found)
}

/// `audio.stop(options?)`.
pub(crate) fn audio_stop(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(move |lua, options: LuaValue| {
        let options = stop_options(&options);
        api(lua, &s, "audio.stop", |runtime| {
            let (name, channel, fade) = options?;
            let asset = match name {
                Some(name) => Some(sound(runtime.project(), &name)?),
                None => None,
            };
            runtime.stop_sounds(asset.as_deref(), channel, fade)
        })
    })
}

/// The volume channel `value` names: master, music or effects.
fn volume_channel(value: &LuaValue, function: &str) -> Result<&'static str, String> {
    let refused = format!("{function}: o canal é \"master\", \"music\" ou \"effects\"");
    let channel = text(value, &refused)?;
    ["master", "music", "effects"]
        .into_iter()
        .find(|known| *known == channel)
        .ok_or_else(|| format!("{refused}, não \"{channel}\""))
}

/// `audio.set_volume(channel, volume)`.
pub(crate) fn audio_set_volume(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(move |lua, (channel, volume): (LuaValue, LuaValue)| {
        api(lua, &s, "audio.set_volume", |runtime| {
            let channel = volume_channel(&channel, "audio.set_volume")?;
            let volume = finite(&volume, "audio.set_volume: o volume é um número de 0 a 1")?;
            runtime.set_volume(channel, volume)
        })
    })
}

/// `audio.volume(channel)`.
pub(crate) fn audio_volume(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(move |lua, channel: LuaValue| {
        api(lua, &s, "audio.volume", |runtime| {
            let channel = volume_channel(&channel, "audio.volume")?;
            Ok(runtime.audio().settings.get(channel).unwrap_or(1.))
        })
    })
}

/// `vars.get(name, key?)`.
pub(crate) fn vars_get(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(move |lua, (name, key): (LuaValue, LuaValue)| {
        let name = text(
            &name,
            "vars.get precisa do nome da variável do jogo (um texto)",
        );
        let key = key_of(&key, "vars.get");
        // A key never written reads the variable's initial value, as Ler variável do jogo.
        let value = api(lua, &s, "vars.get", |runtime| {
            let id = variable(runtime.project(), &name?)?;
            Ok(runtime.variable(&id, &key?).cloned())
        })?;
        match value {
            Some(value) => to_lua(lua, &value),
            None => Ok(LuaValue::Nil),
        }
    })
}

/// `vars.set(name, value, key?)`.
pub(crate) fn vars_set(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(
        move |lua, (name, value, key): (LuaValue, LuaValue, LuaValue)| {
            let name = text(
                &name,
                "vars.set precisa do nome da variável do jogo (um texto)",
            );
            let key = key_of(&key, "vars.set");
            let given = Given::of(&value);
            api(lua, &s, "vars.set", |runtime| {
                let id = variable(runtime.project(), &name?)?;
                let variable = variables::get(runtime.project(), &id).expect("found above");
                let value = given.as_value(&variable.value).ok_or_else(|| {
                    format!(
                        "vars.set: a variável {} guarda {}, não {}",
                        variable.name,
                        super::value_kind(&variable.value),
                        given.kind()
                    )
                })?;
                runtime.set_variable(&id, &key?, value)
            })
        },
    )
}

/// The name of the Lua registry's entry with the markers' metatable.
const MARKER: &str = "oxy.param";
const MARKER_C: &std::ffi::CStr = c"oxy.param";

/// What a marker of `param` declares.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Marker {
    /// An Objeto, empty.
    Object,
    /// A Vetor2 with this default.
    Vector2([f32; 2]),
    /// A Superfície: the default one, or the project's of this name.
    Surface(Option<String>),
}

/// The metatable of `param` markers, made the first time.
fn marker_meta(lua: &Lua) -> mlua::Result<Table> {
    if let Ok(meta) = lua.named_registry_value::<Table>(MARKER) {
        return Ok(meta);
    }
    let meta = lua.create_table()?;
    meta.raw_set("__metatable", "travado")?;
    meta.set_readonly(true);
    lua.set_named_registry_value(MARKER, &meta)?;
    Ok(meta)
}

/// A frozen marker with `fields`.
fn marker(lua: &Lua, fields: &[(&str, LuaValue)]) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    for (name, value) in fields {
        table.raw_set(*name, value.clone())?;
    }
    table.set_metatable(Some(marker_meta(lua)?))?;
    table.set_readonly(true);
    Ok(table)
}

/// `param.object()`: an Object attribute, empty by default.
pub(crate) fn param_object(lua: &Lua, _: &Rc<Shared>) -> mlua::Result<Function> {
    lua.create_function(|lua, ()| {
        marker(
            lua,
            &[("param", LuaValue::String(lua.create_string("object")?))],
        )
    })
}

/// `param.vector2(x, y)`: a Vetor2 attribute with that default.
pub(crate) fn param_vector2(lua: &Lua, _: &Rc<Shared>) -> mlua::Result<Function> {
    lua.create_function(|lua, (x, y): (LuaValue, LuaValue)| {
        let (Some(x), Some(y)) = (number(&x), number(&y)) else {
            return Err(located(
                lua,
                "param.vector2 precisa de dois números, como param.vector2(1, 1)".into(),
            ));
        };
        // As the Vetor2 keeps them: a finite number past ±3.4e38 is infinite there.
        let (x, y) = (x as f32, y as f32);
        if !x.is_finite() || !y.is_finite() {
            return Err(located(
                lua,
                "param.vector2 não aceita infinito, NaN nem números além do que um Vetor2 guarda \
                 (±3,4e38)"
                    .into(),
            ));
        }
        marker(
            lua,
            &[
                ("param", LuaValue::String(lua.create_string("vector2")?)),
                ("x", LuaValue::Number(f64::from(x))),
                ("y", LuaValue::Number(f64::from(y))),
            ],
        )
    })
}

/// `param.surface(name?)`: a Superfície attribute, the default surface or the project's of that
/// name, which attaching looks up.
pub(crate) fn param_surface(lua: &Lua, _: &Rc<Shared>) -> mlua::Result<Function> {
    lua.create_function(|lua, name: LuaValue| {
        let name = match name {
            LuaValue::Nil => LuaValue::Nil,
            LuaValue::String(name) if name.to_str().is_ok_and(|n| !n.trim().is_empty()) => {
                LuaValue::String(name)
            }
            _ => {
                return Err(located(
                    lua,
                    "param.surface recebe o nome de uma superfície do projeto (um texto), como \
                     param.surface(\"Gelo\"), ou nada para a superfície padrão"
                        .into(),
                ));
            }
        };
        marker(
            lua,
            &[
                ("param", LuaValue::String(lua.create_string("surface")?)),
                ("name", name),
            ],
        )
    })
}

/// What a `param` marker declares; none for any other value.
pub(crate) fn marker_of(lua: &Lua, value: &LuaValue) -> Option<Marker> {
    let LuaValue::Table(table) = value else {
        return None;
    };
    let meta = lua.named_registry_value::<Table>(MARKER).ok()?;
    if table.metatable()?.to_pointer() != meta.to_pointer() {
        return None;
    }
    let field = |name: &str| table.raw_get::<LuaValue>(name).ok();
    let coordinate = |name: &str| field(name).as_ref().and_then(number).map(|n| n as f32);
    match field("param")?.to_string().ok()?.as_str() {
        "object" => Some(Marker::Object),
        "vector2" => {
            let v = [coordinate("x")?, coordinate("y")?];
            Value::Vector2(v).is_finite().then_some(Marker::Vector2(v))
        }
        "surface" => Some(Marker::Surface(match field("name") {
            Some(LuaValue::String(name)) => Some(name.to_str().ok()?.to_owned()),
            _ => None,
        })),
        _ => None,
    }
}

/// [`marker_of`] for the value at `idx` of a C function or a protected call, leaving the stack
/// as it was.
///
/// # Safety
/// `idx` is a valid absolute index of `l`, which runs protected (a memory error unwinds).
pub(crate) unsafe fn marker_at(l: *mut ffi::lua_State, idx: c_int) -> Option<Marker> {
    unsafe {
        if ffi::lua_type(l, idx) != ffi::LUA_TTABLE {
            return None;
        }
        ffi::lua_checkstack(l, 3);
        if ffi::lua_getmetatable(l, idx) == 0 {
            return None;
        }
        ffi::lua_rawgetfield(l, ffi::LUA_REGISTRYINDEX, MARKER_C.as_ptr());
        let marked = ffi::lua_rawequal(l, -1, -2) != 0;
        ffi::lua_settop(l, -3);
        if !marked {
            return None;
        }
        let text = |field: &std::ffi::CStr| {
            ffi::lua_rawgetfield(l, idx, field.as_ptr());
            let mut len = 0usize;
            let found = (ffi::lua_type(l, -1) == ffi::LUA_TSTRING).then(|| {
                let p = ffi::lua_tolstring(l, -1, &mut len);
                String::from_utf8_lossy(std::slice::from_raw_parts(p as *const u8, len))
                    .into_owned()
            });
            ffi::lua_settop(l, -2);
            found
        };
        let coordinate = |field: &std::ffi::CStr| {
            ffi::lua_rawgetfield(l, idx, field.as_ptr());
            let n = ffi::lua_tonumberx(l, -1, std::ptr::null_mut()) as f32;
            ffi::lua_settop(l, -2);
            n
        };
        match text(c"param")?.as_str() {
            "object" => Some(Marker::Object),
            "vector2" => Some(Marker::Vector2([coordinate(c"x"), coordinate(c"y")])),
            "surface" => Some(Marker::Surface(text(c"name"))),
            _ => None,
        }
    }
}

/// Gives the table `name` a locked metatable whose `__index` refuses a name it lacks with the
/// nearest of `names`, or says when it comes, for a name kept for 0.8 (`game.online`). The
/// table itself turns read-only with the base.
fn lock(
    lua: &Lua,
    table: &Table,
    name: &'static str,
    names: Vec<&'static str>,
) -> mlua::Result<()> {
    let index = lua.create_function(move |lua, (_, key): (LuaValue, LuaValue)| {
        let key = key.to_string().unwrap_or_default();
        Err::<(), _>(located(
            lua,
            unknown_member(name, &key, names.iter().copied()),
        ))
    })?;
    let meta = lua.create_table()?;
    meta.raw_set("__index", index)?;
    meta.raw_set("__metatable", "travado")?;
    meta.set_readonly(true);
    table.set_metatable(Some(meta))
}

/// Why the global table `table` has no `key`: a name of 0.8 (`game.online`), or one it lacks,
/// with the nearest of `names` (its own). The lint of names says the same of a path a script
/// imports, Luau's tables (`math.flor`) included.
pub(crate) fn unknown_member<'a>(
    table: &str,
    key: &str,
    names: impl IntoIterator<Item = &'a str>,
) -> String {
    let path = format!("{table}.{key}");
    if registry::reserved(&path).is_some() {
        return registry::later(&path);
    }
    let hint = closest(key, names)
        .map(|near| format!("; quis dizer {near}?"))
        .unwrap_or_default();
    format!("{table} não tem {key}{hint}")
}

/// The game variable `name` names, by its ID or its name in any case (see `variables::find`); one
/// the project lacks is refused with the nearest name it has, when one is close enough to be a
/// typo, and the names it has.
fn variable(project: &Project, name: &str) -> Result<Id, String> {
    if let Some(id) = variables::find_named(project, name)? {
        return Ok(id);
    }
    let hint = closest(name, project.variables.iter().map(|v| v.name.as_str()))
        .map_or_else(|| ".".to_owned(), |near| format!("; quis dizer {near}?"));
    Err(format!(
        "A variável do jogo \"{name}\" não existe no projeto{hint} Variáveis: {}",
        variables::summary(project)
    ))
}

/// A text argument, or `refused`.
pub(crate) fn text(value: &LuaValue, refused: &str) -> Result<String, String> {
    match value {
        LuaValue::String(text) => text
            .to_str()
            .map(|text| text.to_owned())
            .map_err(|_| format!("{refused}; este não é UTF-8")),
        _ => Err(refused.into()),
    }
}

/// The name of an option in the table `function` takes: options have text names, and another
/// key is refused by its kind (its text would be an address, which changes from run to run).
pub(crate) fn option_name(key: &LuaValue, function: &str) -> Result<String, String> {
    match key {
        LuaValue::String(_) => text(key, &format!("as opções de {function} têm nomes de texto")),
        other => Err(format!(
            "as opções de {function} têm nomes de texto, não {}",
            handles::In::of(other).kind()
        )),
    }
}

/// The key of a variable's value: none is the variable's own value.
fn key_of(value: &LuaValue, function: &str) -> Result<String, String> {
    match value {
        LuaValue::Nil => Ok(String::new()),
        value => text(value, &format!("a chave de {function} é um texto")),
    }
}

/// A number as mlua gives it: whole ones may come as `Integer`.
fn number(value: &LuaValue) -> Option<f64> {
    match value {
        LuaValue::Number(n) => Some(*n),
        LuaValue::Integer(i) => Some(*i as f64),
        _ => None,
    }
}

/// A finite number argument, or `refused` (with what came).
pub(crate) fn finite(value: &LuaValue, refused: &str) -> Result<f64, String> {
    match number(value) {
        Some(n) if n.is_finite() => Ok(n),
        Some(_) => Err(format!("{refused}; este é infinito ou NaN")),
        None => Err(format!("{refused}, não {}", handles::In::of(value).kind())),
    }
}

/// A boolean argument, `default` when left out, or `refused` (with what came).
pub(crate) fn flag(value: &LuaValue, default: Option<bool>, refused: &str) -> Result<bool, String> {
    match (value, default) {
        (LuaValue::Boolean(b), _) => Ok(*b),
        (LuaValue::Nil, Some(default)) => Ok(default),
        (other, _) => Err(format!("{refused}, não {}", handles::In::of(other).kind())),
    }
}

/// `audio.play`'s options: the volume (1 by default) and the handle key of the object whose
/// player hears the sound (from 0.8, when there are many; in 0.7 the one player hears it).
fn play_options(options: &LuaValue) -> Result<(f64, Option<u64>), String> {
    let table = match options {
        LuaValue::Nil => return Ok((1., None)),
        LuaValue::Table(table) => table,
        _ => {
            return Err("as opções de audio.play são uma tabela, como { volume = 0.5 }".into());
        }
    };
    let (mut volume, mut audience) = (1., None);
    for pair in table.pairs::<LuaValue, LuaValue>() {
        let (key, value) = pair.map_err(|e| e.to_string())?;
        match option_name(&key, "audio.play")?.as_str() {
            "volume" => {
                volume = number(&value).ok_or("audio.play: volume é um número de 0 a 1")?;
            }
            "audience" => {
                audience = match &value {
                    LuaValue::Nil => None,
                    LuaValue::LightUserData(handle) => match handles::decode(handle.0) {
                        (MARK_OBJECT, key) => Some(key),
                        _ => return Err("audio.play: audience é um objeto".into()),
                    },
                    _ => return Err("audio.play: audience é um objeto".into()),
                };
            }
            other => {
                return Err(format!(
                    "audio.play não conhece a opção {other}; use volume ou audience"
                ));
            }
        }
    }
    Ok((volume, audience))
}

/// The audio asset `name` names: by its exact name, else ignoring case and the spaces at its
/// ends, as the Biblioteca lists it.
fn sound(project: &Project, name: &str) -> Result<Id, String> {
    let sounds: Vec<_> = project
        .assets
        .iter()
        .filter(|asset| asset.kind == AssetKind::Audio)
        .collect();
    let mut named: Vec<_> = sounds.iter().filter(|asset| asset.name == name).collect();
    if named.is_empty() {
        let wanted = name.trim().to_lowercase();
        named = sounds
            .iter()
            .filter(|asset| asset.name.trim().to_lowercase() == wanted)
            .collect();
    }
    match named.as_slice() {
        [one] => Ok(one.id.clone()),
        [] if sounds.is_empty() => Err(format!(
            "o som \"{name}\" não existe: o projeto não tem sons (importe um WAV na Biblioteca)"
        )),
        [] => {
            let mut listed: Vec<String> = sounds
                .iter()
                .take(LISTED)
                .map(|asset| asset.name.clone())
                .collect();
            if sounds.len() > LISTED {
                listed.push(format!("e mais {}", sounds.len() - LISTED));
            }
            Err(format!(
                "o som \"{name}\" não existe no projeto; sons: {}",
                listed.join(", ")
            ))
        }
        many => Err(format!(
            "{} sons se chamam \"{name}\"; renomeie um deles na Biblioteca",
            many.len()
        )),
    }
}

/// A value a script gives for a game variable, read before the variable's type is known.
enum Given {
    Number(f64),
    Text(String),
    Bool(bool),
    Vector([f32; 3]),
    Other(&'static str),
}

impl Given {
    fn of(value: &LuaValue) -> Self {
        match value {
            LuaValue::Boolean(b) => Self::Bool(*b),
            LuaValue::Number(_) | LuaValue::Integer(_) => {
                Self::Number(number(value).expect("a number"))
            }
            LuaValue::Vector(v) => Self::Vector([v.x(), v.y(), v.z()]),
            LuaValue::String(text) => match text.to_str() {
                Ok(text) => Self::Text(text.to_owned()),
                Err(_) => Self::Other("texto que não é UTF-8"),
            },
            LuaValue::Nil => Self::Other("nil"),
            LuaValue::Table(_) => Self::Other("tabela"),
            LuaValue::Function(_) => Self::Other("função"),
            LuaValue::LightUserData(handle) => {
                Self::Other(handles::mark_kind(handles::decode(handle.0).0))
            }
            _ => Self::Other("userdata"),
        }
    }

    fn kind(&self) -> &'static str {
        match self {
            Self::Number(_) => "número",
            Self::Text(_) => "texto",
            Self::Bool(_) => "booleano",
            Self::Vector(_) => "vetor",
            Self::Other(kind) => kind,
        }
    }

    /// The value as a variable that starts at `declared` keeps it; none of another type. A
    /// Vetor2 takes a vector's x and y.
    fn as_value(&self, declared: &Value) -> Option<Value> {
        Some(match (declared, self) {
            (Value::Number(_), Self::Number(n)) => Value::Number(*n),
            (Value::Text(_), Self::Text(text)) => Value::Text(text.clone()),
            (Value::Bool(_), Self::Bool(b)) => Value::Bool(*b),
            (Value::Vector3(_), Self::Vector(v)) => Value::Vector3(*v),
            (Value::Vector2(_), Self::Vector([x, y, _])) => Value::Vector2([*x, *y]),
            _ => return None,
        })
    }
}

/// A game variable's value in Luau: a Vetor2 as a vector with z 0.
fn to_lua(lua: &Lua, value: &Value) -> mlua::Result<LuaValue> {
    Ok(match value {
        Value::Number(n) => LuaValue::Number(*n),
        Value::Bool(b) => LuaValue::Boolean(*b),
        Value::Text(text) => LuaValue::String(lua.create_string(text)?),
        Value::Vector3([x, y, z]) => LuaValue::Vector(mlua::Vector::new(*x, *y, *z)),
        Value::Vector2([x, y]) => LuaValue::Vector(mlua::Vector::new(*x, *y, 0.)),
        // Variables hold neither (see `variables::check_value`).
        Value::Object(_) | Value::Surface(_) => LuaValue::Nil,
    })
}
