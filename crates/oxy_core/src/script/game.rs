//! The game as a whole for scripts (design §5.4), through the `Runtime`'s operations as the
//! nodes: the global tables `game` (the game pause, the cursor, quitting), `save` (the saved
//! progress), `input` (the input actions by name and the step's axes, of the one player in 0.7:
//! the argument `player` comes in 0.8), `time` (the game clock, the step and the text of a time)
//! and `random` (the engine's draws, with a sequence per object and script and named ones,
//! `random.stream`, whose methods are here too).
use super::api::{finite, text};
use super::errors::closest;
use super::handles::{self, In};
use super::host::{Shared, api, located};
use super::registry::View;
use crate::document::{Id, Project};
use crate::input_actions::{self, MovementActions};
use crate::runtime::Runtime;
use mlua::{Function, Lua, Value as LuaValue};
use std::rc::Rc;

/// Actions a message lists before it counts the rest.
const LISTED: usize = 12;

/// The formats of `time.format`, as Formatar tempo lists them.
pub(crate) const FORMATS: [&str; 6] = [
    "mm:ss",
    "mm:ss.cc",
    "m:ss.cc",
    "ss.cc",
    "mm:ss.mmm",
    "h:mm:ss",
];

/// `game.pause()` and, `paused` false, `game.resume()`.
fn pause(
    lua: &Lua,
    shared: &Rc<Shared>,
    name: &'static str,
    paused: bool,
) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(move |lua, ()| {
        api(lua, &s, name, |runtime| {
            runtime.set_game_paused(paused);
            Ok(())
        })
    })
}

pub(crate) fn game_pause(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    pause(lua, shared, "game.pause", true)
}

pub(crate) fn game_resume(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    pause(lua, shared, "game.resume", false)
}

/// `game.paused()`.
pub(crate) fn game_paused(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(move |lua, ()| {
        api(lua, &s, "game.paused", |runtime| Ok(runtime.game_paused()))
    })
}

/// `game.show_cursor()` and, `shown` false, `game.hide_cursor()`.
fn cursor(
    lua: &Lua,
    shared: &Rc<Shared>,
    name: &'static str,
    shown: bool,
) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(move |lua, ()| {
        api(lua, &s, name, |runtime| {
            runtime.set_cursor_shown(shown);
            Ok(())
        })
    })
}

pub(crate) fn game_show_cursor(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    cursor(lua, shared, "game.show_cursor", true)
}

pub(crate) fn game_hide_cursor(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    cursor(lua, shared, "game.hide_cursor", false)
}

/// `game.quit()`.
pub(crate) fn game_quit(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(move |lua, ()| {
        api(lua, &s, "game.quit", |runtime| {
            let owner = s
                .caller()
                .ok_or("game.quit fora de uma chamada de script")?;
            runtime.quit_game(&owner);
            Ok(())
        })
    })
}

/// `save.write()`.
pub(crate) fn save_write(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(move |lua, ()| {
        api(lua, &s, "save.write", |runtime| {
            runtime.save_progress();
            Ok(())
        })
    })
}

/// `save.clear()`.
pub(crate) fn save_clear(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(move |lua, ()| {
        api(lua, &s, "save.clear", |runtime| {
            runtime.clear_progress();
            Ok(())
        })
    })
}

/// `save.exists()`.
pub(crate) fn save_exists(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(move |lua, ()| {
        api(lua, &s, "save.exists", |runtime| {
            Ok(runtime.has_saved_progress())
        })
    })
}

/// Why `function` takes no player in 0.7, when a script passes one.
fn no_player(player: &LuaValue, function: &str) -> Result<(), String> {
    match player {
        LuaValue::Nil => Ok(()),
        _ => Err(format!(
            "{function}: o argumento player chega na 0.8, com a rede e vários jogadores; na 0.7 \
             a entrada é a do único jogador (tire o argumento)"
        )),
    }
}

/// The input action `name` names, by its name in Lógica > Ações de entrada (any case) or its
/// ID; one the project lacks is refused with the nearest name and the names it has, and a name
/// several actions share without their IDs, which the editor never shows.
fn action(project: &Project, name: &str, function: &str) -> Result<Id, String> {
    let wanted = name.trim().to_lowercase();
    let sharing = project
        .input_bindings
        .keys()
        .filter(|id| input_actions::label(project, id).to_lowercase() == wanted)
        .count();
    if sharing > 1 && !project.input_bindings.contains_key(name) {
        return Err(format!(
            "{function}: há {sharing} ações de entrada chamadas \"{name}\"; renomeie uma delas em \
             Lógica > Ações de entrada"
        ));
    }
    if let Some(id) =
        input_actions::find_named(project, name).map_err(|e| format!("{function}: {e}"))?
    {
        return Ok(id);
    }
    let names: Vec<String> = project
        .input_bindings
        .keys()
        .map(|id| input_actions::label(project, id).into_owned())
        .collect();
    if names.is_empty() {
        return Err(format!(
            "{function}: a ação de entrada \"{name}\" não existe: o projeto não tem ações (crie \
             em Lógica > Ações de entrada)"
        ));
    }
    let hint = closest(name, names.iter().map(String::as_str))
        .map_or_else(|| ".".to_owned(), |near| format!("; quis dizer {near}?"));
    let mut listed: Vec<&str> = names.iter().take(LISTED).map(String::as_str).collect();
    let more = format!("e mais {}", names.len().saturating_sub(LISTED));
    if names.len() > LISTED {
        listed.push(&more);
    }
    Err(format!(
        "{function}: a ação de entrada \"{name}\" não existe no projeto{hint} Ações: {}",
        listed.join(", ")
    ))
}

/// `input.pressed`, `input.held` and `input.released`: `read` tells of the action.
fn action_state(
    lua: &Lua,
    shared: &Rc<Shared>,
    function: &'static str,
    read: fn(&Runtime, &str) -> bool,
) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(move |lua, (name, player): (LuaValue, LuaValue)| {
        api(lua, &s, function, |runtime| {
            let name = text(
                &name,
                &format!("{function} precisa do nome da ação de entrada (um texto)"),
            )?;
            no_player(&player, function)?;
            let id = action(runtime.project(), &name, function)?;
            Ok(read(runtime, &id))
        })
    })
}

pub(crate) fn input_pressed(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    action_state(lua, shared, "input.pressed", Runtime::input_pressed)
}

pub(crate) fn input_held(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    action_state(lua, shared, "input.held", Runtime::input_held)
}

pub(crate) fn input_released(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    action_state(lua, shared, "input.released", Runtime::input_released)
}

/// `input.movement()`: through the movement actions of the object's 3D character, else the
/// project's own.
pub(crate) fn input_movement(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(move |lua, player: LuaValue| {
        let axis = api(lua, &s, "input.movement", |runtime| {
            no_player(&player, "input.movement")?;
            let owner = s
                .caller()
                .ok_or("input.movement fora de uma chamada de script")?;
            if runtime
                .entity(&owner)
                .is_some_and(|e| e.character3d.is_some())
            {
                return runtime.input_movement(&owner);
            }
            Ok(crate::character::movement_axis(
                runtime.step_input(),
                &MovementActions::default(),
            ))
        })?;
        Ok(mlua::Vector::new(axis.x, axis.y, 0.))
    })
}

/// `input.look()`.
pub(crate) fn input_look(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(move |lua, player: LuaValue| {
        let look = api(lua, &s, "input.look", |runtime| {
            no_player(&player, "input.look")?;
            Ok(runtime.input_look())
        })?;
        Ok(mlua::Vector::new(look.x, look.y, 0.))
    })
}

/// `input.wheel()`.
pub(crate) fn input_wheel(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(move |lua, player: LuaValue| {
        api(lua, &s, "input.wheel", |runtime| {
            no_player(&player, "input.wheel")?;
            Ok(f64::from(runtime.input_wheel()))
        })
    })
}

/// `time.now()`.
pub(crate) fn time_now(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(move |lua, ()| api(lua, &s, "time.now", |runtime| Ok(runtime.game_time())))
}

/// `time.format(seconds, format)`: pure, so a module's top may use it.
pub(crate) fn time_format(lua: &Lua, _: &Rc<Shared>) -> mlua::Result<Function> {
    lua.create_function(|lua, (seconds, format): (LuaValue, LuaValue)| {
        let formatted = (|| {
            let seconds = finite(&seconds, "time.format precisa dos segundos (um número)")?;
            let format = text(
                &format,
                "time.format precisa do formato (um texto), como \"mm:ss\"",
            )?;
            if !FORMATS.contains(&format.as_str()) {
                let hint = closest(&format, FORMATS)
                    .map_or_else(|| ".".to_owned(), |near| format!("; quis dizer {near}?"));
                return Err(format!(
                    "time.format não conhece o formato \"{format}\"{hint} Formatos: {}",
                    FORMATS.join(", ")
                ));
            }
            Runtime::format_time(seconds, &format).map_err(|e| format!("time.format: {e}"))
        })();
        formatted.map_err(|e| located(lua, e))
    })
}

/// The object running and its script, whose sequences draw.
fn drawer(s: &Shared) -> Result<(Id, String), String> {
    let key = s
        .caller_key()
        .ok_or("sorteio fora de uma chamada de script")?;
    let owner = s.id(key).ok_or_else(|| s.gone(key))?;
    let script = s.module().ok_or("sorteio fora de uma chamada de script")?;
    Ok((owner, script.path.to_string()))
}

/// A draw from `min` to `max` of the sequence `stream` ("" for the script's own).
fn draw(
    s: &Shared,
    runtime: &mut Runtime,
    function: &str,
    stream: &str,
    (min, max): (&LuaValue, &LuaValue),
    integer: bool,
) -> Result<f64, String> {
    let min = finite(min, &format!("{function}: min é um número"))?;
    let max = finite(max, &format!("{function}: max é um número"))?;
    let (owner, script) = drawer(s)?;
    runtime
        .script_random_number(&owner, &script, stream, min, max, integer)
        .map_err(|e| format!("{function}: {e}"))
}

/// `random.number(min, max)` and, `integer`, `random.integer(min, max)`.
fn number(
    lua: &Lua,
    shared: &Rc<Shared>,
    function: &'static str,
    integer: bool,
) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(move |lua, (min, max): (LuaValue, LuaValue)| {
        api(lua, &s, function, |runtime| {
            draw(&s, runtime, function, "", (&min, &max), integer)
        })
    })
}

pub(crate) fn random_number(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    number(lua, shared, "random.number", false)
}

pub(crate) fn random_integer(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    number(lua, shared, "random.integer", true)
}

/// `random.seed(seed)`.
pub(crate) fn random_seed(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(move |lua, seed: LuaValue| {
        api(lua, &s, "random.seed", |runtime| {
            let seed = finite(&seed, "random.seed recebe a semente, um número inteiro")?;
            runtime
                .set_random_seed(seed)
                .map_err(|e| format!("random.seed: {e}"))
        })
    })
}

/// The longest name of a sequence of draws, in bytes.
const MAX_STREAM: usize = 256;

/// `random.stream(name)`: pure, so a module's top may keep one.
pub(crate) fn random_stream(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(move |lua, name: LuaValue| {
        let key = (|| {
            let name = text(
                &name,
                "random.stream precisa do nome da sequência (um texto)",
            )?;
            if name.is_empty() || name.len() > MAX_STREAM {
                return Err(format!(
                    "random.stream: o nome da sequência tem de 1 a {MAX_STREAM} bytes"
                ));
            }
            s.register_stream(&name)
        })()
        .map_err(|e| located(lua, e))?;
        Ok(LuaValue::LightUserData(mlua::LightUserData(
            handles::handle(View::Stream.mark(), key),
        )))
    })
}

/// `stream:number(min, max)` and, `integer`, `stream:integer(min, max)`.
fn stream_draw(
    lua: &Lua,
    shared: &Rc<Shared>,
    method: &'static str,
    integer: bool,
) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(move |lua, (me, min, max): (LuaValue, LuaValue, LuaValue)| {
        api(lua, &s, method, |runtime| {
            let stream = match In::of(&me) {
                In::Handle(mark, key) if mark == View::Stream.mark() => s
                    .stream(key)
                    .ok_or("sequência de sorteios desconhecida da engine")?,
                _ => {
                    return Err(format!(
                        "{method} é um método de uma sequência de sorteios: \
                             random.stream(\"nome\"):{method}(min, max)"
                    ));
                }
            };
            let function = format!("random.stream(\"{stream}\"):{method}");
            draw(&s, runtime, &function, &stream, (&min, &max), integer)
        })
    })
}

pub(crate) fn stream_number(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    stream_draw(lua, shared, "number", false)
}

pub(crate) fn stream_integer(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    stream_draw(lua, shared, "integer", true)
}
