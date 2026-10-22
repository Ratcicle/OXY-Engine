//! The 3D character's commands for scripts (design §5.4): the methods of `obj.character`, which
//! do what the nodes of Personagem 3D do through the same operations of the `Runtime`, with the
//! names a script gives (`"crouch"`, `"agile"`, `"speed"`) checked before them, so a wrong one
//! is refused with the names there are. Its state reads through `views`. A teleport that finds
//! no room is no error: it returns false and why, as Teleportar com segurança tells it.
use super::api::{finite, flag, option_name, text};
use super::errors::closest;
use super::handles::In;
use super::host::{Shared, api};
use super::objects::vector;
use super::registry::View;
use super::views::receiver;
use crate::character::{MAX_TELEPORT_SEARCH, MovementProfile, TeleportOptions};
use crate::document::Id;
use crate::runtime::Runtime;
use glam::Vec2;
use mlua::{Function, Lua, Table, Value as LuaValue};
use std::rc::Rc;

/// The values of the motor `set_param` and `get_param` name, as Alterar parâmetro do personagem
/// lists them.
pub(crate) const PARAMETERS: [&str; 20] = [
    "speed",
    "sprint_speed",
    "crouch_speed",
    "gravity",
    "jump_speed",
    "ground_acceleration",
    "ground_braking",
    "ground_friction",
    "air_acceleration",
    "air_projected_limit",
    "air_resistance",
    "horizontal_limit",
    "jump_retention",
    "landing_retention",
    "coyote_ms",
    "jump_buffer_ms",
    "slide_duration",
    "slide_friction",
    "slide_control",
    "angular_speed",
];

/// The options `set_option` names, as Alterar opção do personagem lists them.
pub(crate) const OPTIONS: [&str; 5] = [
    "automatic_input",
    "slide_enabled",
    "crouch_toggle",
    "automatic_jump",
    "face_look",
];

/// The postures `set_posture` names, as the node `character.posture` lists them.
pub(crate) const POSTURES: [&str; 3] = ["sprint", "crouch", "slide"];

/// The channels `block` and `block_reasons` name, as the node `character.block` lists them.
pub(crate) const CHANNELS: [&str; 2] = ["movement", "look"];

/// The profiles `apply_profile` names, as the node `character.profile` lists them, and
/// `"agile"`, the generic name of `"parkour"` (design §5.1), which projects keep.
pub(crate) const PROFILES: [&str; 4] = ["direct", "agile", "chained", "parkour"];

/// The character before `:`.
fn me(s: &Shared, runtime: &Runtime, me: &LuaValue, method: &str) -> Result<Id, String> {
    receiver(s, runtime, me, View::Character, method)
}

/// `name`, one of `known` (what `method` takes as `what`); why not, with the nearest and the list.
fn known<'a>(
    name: &LuaValue,
    known: &[&'a str],
    method: &str,
    what: &str,
) -> Result<&'a str, String> {
    let name = text(name, &format!("{method} precisa d{what} (um texto)"))?;
    known.iter().copied().find(|k| *k == name).ok_or_else(|| {
        let hint = closest(&name, known.iter().copied())
            .map_or_else(|| ".".to_owned(), |near| format!("; quis dizer {near}?"));
        format!(
            "{method} não conhece \"{name}\"{hint} Use {}",
            known.join(", ")
        )
    })
}

/// The channel of `block` and `block_reasons`: true for the look.
fn channel(value: &LuaValue, method: &str) -> Result<bool, String> {
    Ok(known(value, &CHANNELS, method, "o canal")? == "look")
}

/// `obj.character:block_reasons(channel)`.
pub(crate) fn block_reasons(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(move |lua, (who, which): (LuaValue, LuaValue)| {
        let reasons = api(lua, &s, "block_reasons", |runtime| {
            let id = me(&s, runtime, &who, "block_reasons")?;
            let look = channel(&which, "block_reasons")?;
            let state = runtime.inspect_character(&id)?;
            let reasons = if look {
                &state.look_blocks
            } else {
                &state.movement_blocks
            };
            Ok(reasons.iter().cloned().collect::<Vec<_>>())
        })?;
        lua.create_sequence_from(reasons)
    })
}

/// `obj.character:move(axis)`.
pub(crate) fn intent(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(move |lua, (who, axis): (LuaValue, LuaValue)| {
        api(lua, &s, "move", |runtime| {
            let id = me(&s, runtime, &who, "move")?;
            let axis = vector(
                &axis,
                "move recebe o eixo do movimento, um vetor com x para a direita e y para a \
                 frente, como vector.create(0, 1, 0)",
            )?;
            runtime.set_movement_intent(&id, Vec2::new(axis.x, axis.y))
        })
    })
}

/// `obj.character:jump()`.
pub(crate) fn jump(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(move |lua, who: LuaValue| {
        api(lua, &s, "jump", |runtime| {
            let id = me(&s, runtime, &who, "jump")?;
            runtime.request_jump(&id)
        })
    })
}

/// `obj.character:set_posture(kind, on)`.
pub(crate) fn posture(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(move |lua, (who, kind, on): (LuaValue, LuaValue, LuaValue)| {
        api(lua, &s, "set_posture", |runtime| {
            let id = me(&s, runtime, &who, "set_posture")?;
            let kind = known(&kind, &POSTURES, "set_posture", "a postura")?;
            let on = flag(&on, None, "set_posture: on é true (liga) ou false (desliga)")?;
            match (kind, on) {
                ("sprint", on) => runtime.request_sprint(&id, on),
                ("slide", true) => {
                    let e = runtime.entity(&id).expect("checked by receiver");
                    if e.character3d.as_ref().is_some_and(|c| !c.slide_enabled) {
                        return Err(format!(
                            "{} não pode deslizar: o deslize está desligado no personagem (ligue \
                             com set_option(\"slide_enabled\", true))",
                            e.name
                        ));
                    }
                    runtime.request_slide(&id)
                }
                (_, on) => runtime.request_crouch(&id, on),
            }
        })
    })
}

/// `obj.character:add_velocity(v)` and, `set`, `obj.character:set_velocity(v)`.
fn velocity(
    lua: &Lua,
    shared: &Rc<Shared>,
    method: &'static str,
    set: bool,
) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(move |lua, (who, v): (LuaValue, LuaValue)| {
        api(lua, &s, method, |runtime| {
            let id = me(&s, runtime, &who, method)?;
            let v = vector(
                &v,
                &format!(
                    "{method} recebe uma velocidade em m/s, um vetor como vector.create(0, 8, 0)"
                ),
            )?;
            if set {
                runtime.set_character_velocity(&id, v)
            } else {
                runtime.add_character_velocity(&id, v)
            }
        })
    })
}

pub(crate) fn add_velocity(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    velocity(lua, shared, "add_velocity", false)
}

pub(crate) fn set_velocity(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    velocity(lua, shared, "set_velocity", true)
}

/// `obj.character:block(channel, reason, on)`.
pub(crate) fn block(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(
        move |lua, (who, which, reason, on): (LuaValue, LuaValue, LuaValue, LuaValue)| {
            api(lua, &s, "block", |runtime| {
                let id = me(&s, runtime, &who, "block")?;
                let look = channel(&which, "block")?;
                let reason = text(
                    &reason,
                    "block precisa do motivo (um texto de 1 a 256 bytes)",
                )?;
                if reason.trim().is_empty() || reason.len() > 256 {
                    return Err("block: o motivo é um texto de 1 a 256 bytes".into());
                }
                let on = flag(&on, None, "block: on é true (bloqueia) ou false (libera)")?;
                runtime.block_character_input(&id, look, &reason, on)
            })
        },
    )
}

/// `obj.character:apply_profile(profile, allow_limit?)`.
pub(crate) fn profile(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(
        move |lua, (who, profile, allow): (LuaValue, LuaValue, LuaValue)| {
            api(lua, &s, "apply_profile", |runtime| {
                let id = me(&s, runtime, &who, "apply_profile")?;
                let profile = match known(&profile, &PROFILES, "apply_profile", "o perfil")? {
                    "direct" => MovementProfile::Direct,
                    "chained" => MovementProfile::ChainedJumps,
                    _ => MovementProfile::Parkour,
                };
                let allow = flag(
                    &allow,
                    Some(false),
                    "apply_profile: allow_limit é true ou false",
                )?;
                runtime.apply_movement_profile(&id, profile, allow)
            })
        },
    )
}

/// `obj.character:set_param(name, value, allow_limit?)`.
pub(crate) fn set_param(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(
        move |lua, (who, name, value, allow): (LuaValue, LuaValue, LuaValue, LuaValue)| {
            api(lua, &s, "set_param", |runtime| {
                let id = me(&s, runtime, &who, "set_param")?;
                let name = known(&name, &PARAMETERS, "set_param", "o nome do parâmetro")?;
                let value = finite(&value, &format!("set_param: o valor de {name} é um número"))?;
                let value = value as f32;
                if !value.is_finite() {
                    return Err(format!(
                        "set_param: o valor de {name} passa do que o motor guarda"
                    ));
                }
                let allow = flag(
                    &allow,
                    Some(false),
                    "set_param: allow_limit é true ou false",
                )?;
                runtime
                    .set_character_parameter(&id, name, value, allow)
                    .map_err(|e| format!("set_param(\"{name}\"): {e}"))
            })
        },
    )
}

/// `obj.character:get_param(name)`.
pub(crate) fn get_param(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(move |lua, (who, name): (LuaValue, LuaValue)| {
        api(lua, &s, "get_param", |runtime| {
            let id = me(&s, runtime, &who, "get_param")?;
            let name = known(&name, &PARAMETERS, "get_param", "o nome do parâmetro")?;
            runtime.character_parameter(&id, name).map(f64::from)
        })
    })
}

/// `obj.character:set_option(name, on)`.
pub(crate) fn option(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(
        move |lua, (who, name, on): (LuaValue, LuaValue, LuaValue)| {
            api(lua, &s, "set_option", |runtime| {
                let id = me(&s, runtime, &who, "set_option")?;
                let name = known(&name, &OPTIONS, "set_option", "o nome da opção")?;
                let on = flag(&on, None, "set_option: on é true (liga) ou false (desliga)")?;
                runtime.set_character_option(&id, name, on)
            })
        },
    )
}

/// `obj.character:set_yaw(degrees)`.
pub(crate) fn set_yaw(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(move |lua, (who, degrees): (LuaValue, LuaValue)| {
        api(lua, &s, "set_yaw", |runtime| {
            let id = me(&s, runtime, &who, "set_yaw")?;
            let degrees = finite(&degrees, "set_yaw recebe o giro no mundo, em graus")?;
            runtime
                .set_character_yaw(&id, (degrees as f32).to_radians())
                .map_err(|e| {
                    let name = &runtime.entity(&id).expect("checked by receiver").name;
                    format!("set_yaw não girou {name}: {e}")
                })
        })
    })
}

/// The options of `teleport`, read before the call reaches the world.
fn teleport_options(options: &LuaValue) -> Result<TeleportOptions, String> {
    let mut found = TeleportOptions {
        keep_velocity: false,
        yaw: None,
        look: None,
        search_radius: 0.,
    };
    let table: &Table = match options {
        LuaValue::Nil => return Ok(found),
        LuaValue::Table(table) => table,
        other => {
            return Err(format!(
                "as opções de teleport são uma tabela, como {{ search = 1 }}, não {}",
                In::of(other).kind()
            ));
        }
    };
    for pair in table.pairs::<LuaValue, LuaValue>() {
        let (key, value) = pair.map_err(|e| e.to_string())?;
        match option_name(&key, "teleport")?.as_str() {
            "search" => {
                let search = finite(
                    &value,
                    &format!("teleport: search é um número de 0 a {MAX_TELEPORT_SEARCH} metros"),
                )? as f32;
                if !(0. ..=MAX_TELEPORT_SEARCH).contains(&search) {
                    return Err(format!(
                        "teleport: search vai de 0 a {MAX_TELEPORT_SEARCH} metros; recebeu {search}"
                    ));
                }
                found.search_radius = search;
            }
            "keep_velocity" => {
                found.keep_velocity =
                    flag(&value, None, "teleport: keep_velocity é true ou false")?;
            }
            "yaw" => {
                let yaw = finite(&value, "teleport: yaw é o giro do corpo em graus")?;
                found.yaw = Some((yaw as f32).to_radians());
            }
            "look" => {
                let look = vector(
                    &value,
                    "teleport: look é o olhar em graus, um vetor com x o giro e y a inclinação \
                     (como character.look dá)",
                )?;
                found.look = Some([look.x.to_radians(), look.y.to_radians()]);
            }
            other => {
                return Err(format!(
                    "teleport não conhece a opção {other}; use search, keep_velocity, yaw ou look"
                ));
            }
        }
    }
    Ok(found)
}

/// `obj.character:teleport(feet, options?)`: ok, and why not or where the feet went.
pub(crate) fn teleport(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(
        move |lua, (who, feet, options): (LuaValue, LuaValue, LuaValue)| {
            let options = teleport_options(&options);
            let result = api(lua, &s, "teleport", |runtime| {
                let id = me(&s, runtime, &who, "teleport")?;
                let feet = vector(
                    &feet,
                    "teleport recebe onde os pés ficam, no mundo, um vetor como vector.create(0, \
                     1, 0)",
                )?;
                Ok(runtime.teleport_character(&id, feet, options?))
            })?;
            Ok(match result {
                Ok(at) => (true, None, Some(mlua::Vector::new(at.x, at.y, at.z))),
                Err(reason) => (false, Some(reason), None),
            })
        },
    )
}
