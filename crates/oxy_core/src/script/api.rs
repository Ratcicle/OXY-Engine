//! The global tables of the API (design §5.3, §5.4): their functions reach the world through
//! the `Runtime`'s shared operations layer, so they check, refuse and act as the nodes do, and
//! name what the person named (sounds and game variables by their names, never their IDs). The
//! first surface, the vertical slice of 0.7, has `audio.play` and `vars.get`/`vars.set`. The
//! tables are frozen, and reading a name one lacks is an error with the nearest name it has.
use super::errors::closest;
use super::handles::{self, MARK_OBJECT};
use super::host::{Shared, api, located};
use crate::document::{AssetKind, Id, Project, Value};
use crate::variables;
use mlua::{Lua, Table, Value as LuaValue};
use std::rc::Rc;

/// Sounds a message lists before it counts the rest.
const LISTED: usize = 10;

/// Puts the API's tables in the base, which turns read-only after it.
pub(crate) fn install(lua: &Lua, base: &Table, shared: &Rc<Shared>) -> mlua::Result<()> {
    let audio = lua.create_table()?;
    let s = shared.clone();
    audio.raw_set(
        "play",
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
        })?,
    )?;
    lock(lua, &audio, "audio", &["play"])?;
    base.raw_set("audio", audio)?;

    let vars = lua.create_table()?;
    let s = shared.clone();
    vars.raw_set(
        "get",
        lua.create_function(move |lua, (name, key): (LuaValue, LuaValue)| {
            let name = text(
                &name,
                "vars.get precisa do nome da variável do jogo (um texto)",
            );
            let key = key_of(&key, "vars.get");
            let value = api(lua, &s, "vars.get", |runtime| {
                let id = variables::find(runtime.project(), &name?)?;
                Ok(runtime.variable(&id, &key?).cloned())
            })?;
            match value {
                Some(value) => to_lua(lua, &value),
                None => Ok(LuaValue::Nil),
            }
        })?,
    )?;
    let s = shared.clone();
    vars.raw_set(
        "set",
        lua.create_function(
            move |lua, (name, value, key): (LuaValue, LuaValue, LuaValue)| {
                let name = text(
                    &name,
                    "vars.set precisa do nome da variável do jogo (um texto)",
                );
                let key = key_of(&key, "vars.set");
                let given = Given::of(&value);
                api(lua, &s, "vars.set", |runtime| {
                    let id = variables::find(runtime.project(), &name?)?;
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
        )?,
    )?;
    lock(lua, &vars, "vars", &["get", "set"])?;
    base.raw_set("vars", vars)?;
    Ok(())
}

/// Gives the table `name` a locked metatable whose `__index` refuses a name it lacks with the
/// nearest of `names`. The table itself turns read-only with the base.
fn lock(
    lua: &Lua,
    table: &Table,
    name: &'static str,
    names: &'static [&'static str],
) -> mlua::Result<()> {
    let index = lua.create_function(move |lua, (_, key): (LuaValue, LuaValue)| {
        let key = key.to_string().unwrap_or_default();
        let hint = closest(&key, names.iter().copied())
            .map(|near| format!("; quis dizer {near}?"))
            .unwrap_or_default();
        Err::<(), _>(located(lua, format!("{name} não tem {key}{hint}")))
    })?;
    let meta = lua.create_table()?;
    meta.raw_set("__index", index)?;
    meta.raw_set("__metatable", "travado")?;
    meta.set_readonly(true);
    table.set_metatable(Some(meta))
}

/// A text argument, or `refused`.
fn text(value: &LuaValue, refused: &str) -> Result<String, String> {
    match value {
        LuaValue::String(text) => text
            .to_str()
            .map(|text| text.to_owned())
            .map_err(|_| format!("{refused}; este não é UTF-8")),
        _ => Err(refused.into()),
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
        match key.to_string().unwrap_or_default().as_str() {
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
