//! The game camera for scripts (design §5.4): the global table `camera`, what Ler câmera de jogo
//! reads and what the camera nodes do, through the same operations of the `Runtime`. The
//! commands take the camera object; one without Câmera, or without Controle de câmera for what
//! needs it, is refused by name before the operation runs.
use super::api::{finite, text};
use super::errors::closest;
use super::handles::{self, In, MARK_OBJECT};
use super::host::{Shared, api, object};
use crate::character::{CameraLookAt, CameraMode};
use crate::document::Id;
use crate::runtime::{Runtime, look_of};
use glam::Vec3;
use mlua::{Function, Lua, Value as LuaValue};
use std::rc::Rc;

/// The transition the nodes start from, in seconds.
const TRANSITION: f64 = 0.25;
/// The smoothing of a framing the nodes start from, in seconds.
const SMOOTHING: f64 = 0.15;

/// The modes `camera.set_mode` names, as the node `camera.mode` lists them.
pub(crate) const MODES: [&str; 4] = ["first_person", "third_person", "fixed", "fixed_object"];

/// The settings `camera.set` names, as the node `camera.setting` lists them.
pub(crate) const SETTINGS: [&str; 3] = ["fov", "distance", "shoulder"];

/// The object `value` is, in the scene: its ID; why not, in `function`'s words.
fn object_arg(
    s: &Shared,
    runtime: &Runtime,
    value: &LuaValue,
    function: &str,
    what: &str,
) -> Result<Id, String> {
    match In::of(value) {
        In::Handle(MARK_OBJECT, key) => s
            .id(key)
            .filter(|id| runtime.entity(id).is_some())
            .ok_or_else(|| format!("{function}: {what} não existe mais")),
        other => Err(format!(
            "{function} recebe {what}, um objeto, não {}",
            other.kind()
        )),
    }
}

/// The camera `value` is: an object with Câmera and, with `rig`, Controle de câmera.
fn camera(
    s: &Shared,
    runtime: &Runtime,
    value: &LuaValue,
    function: &str,
    rig: bool,
) -> Result<Id, String> {
    let id = object_arg(s, runtime, value, function, "a câmera")?;
    let e = runtime.entity(&id).expect("checked by object_arg");
    if e.camera.is_none() {
        return Err(format!("{function}: {} não tem Câmera", e.name));
    }
    if rig && e.camera_rig.is_none() {
        return Err(format!(
            "{function}: {} não tem Controle de câmera (só uma câmera de personagem troca de \
             modo, acompanha e olha para um alvo)",
            e.name
        ));
    }
    Ok(id)
}

/// A transition in seconds, `default` when left out.
fn seconds(value: &LuaValue, default: f64, function: &str) -> Result<f32, String> {
    let seconds = match value {
        LuaValue::Nil => default,
        value => finite(
            value,
            &format!("{function}: seconds é a transição em segundos"),
        )?,
    };
    if seconds < 0. {
        return Err(format!(
            "{function}: seconds não pode ser negativo; recebeu {seconds}"
        ));
    }
    Ok(seconds as f32)
}

/// The mode of a camera as scripts name it.
fn mode_name(mode: CameraMode) -> &'static str {
    match mode {
        CameraMode::FirstPerson => "first_person",
        CameraMode::ThirdPerson => "third_person",
        _ => "fixed",
    }
}

/// `camera.active()`.
pub(crate) fn active(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(move |lua, ()| {
        let key = api(lua, &s, "camera.active", |runtime| {
            let Some(id) = runtime.active_camera().map(str::to_owned) else {
                return Ok(None);
            };
            s.register(&id)
                .map(Some)
                .map_err(|first| handles::collision(runtime, &first, &id))
        })?;
        Ok(key.map_or(LuaValue::Nil, object))
    })
}

/// The direction the active camera looks, in the simulation.
fn facing(runtime: &Runtime, function: &str) -> Result<Vec3, String> {
    runtime
        .camera_orientation()
        .map(|rotation| rotation * -Vec3::Z)
        .map_err(|_| {
            format!(
                "{function}: não há câmera de jogo ativa, ou o Controle de câmera dela ainda não \
                 começou (ele começa no primeiro passo)"
            )
        })
}

/// `camera.look()`.
pub(crate) fn look(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(move |lua, ()| {
        let v = api(lua, &s, "camera.look", |runtime| {
            facing(runtime, "camera.look").map(look_of)
        })?;
        Ok(mlua::Vector::new(v[0], v[1], 0.))
    })
}

/// `camera.forward()`.
pub(crate) fn forward(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(move |lua, ()| {
        let f = api(lua, &s, "camera.forward", |runtime| {
            facing(runtime, "camera.forward")
        })?;
        Ok(mlua::Vector::new(f.x, f.y, f.z))
    })
}

/// `camera.mode()`.
pub(crate) fn mode(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(move |lua, ()| {
        api(lua, &s, "camera.mode", |runtime| {
            Ok(runtime
                .active_camera()
                .map(|id| mode_name(runtime.camera_mode(id).unwrap_or(CameraMode::Fixed))))
        })
    })
}

/// `camera.activate(camera, seconds?)`.
pub(crate) fn activate(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(move |lua, (which, time): (LuaValue, LuaValue)| {
        api(lua, &s, "camera.activate", |runtime| {
            let id = camera(&s, runtime, &which, "camera.activate", false)?;
            let seconds = seconds(&time, TRANSITION, "camera.activate")?;
            runtime.activate_camera(&id, seconds)
        })
    })
}

/// `camera.set_mode(camera, mode, seconds?)`.
pub(crate) fn set_mode(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(
        move |lua, (which, mode, time): (LuaValue, LuaValue, LuaValue)| {
            api(lua, &s, "camera.set_mode", |runtime| {
                let id = camera(&s, runtime, &which, "camera.set_mode", true)?;
                let mode = text(&mode, "camera.set_mode precisa do modo (um texto)")?;
                let seconds = seconds(&time, TRANSITION, "camera.set_mode")?;
                match mode.as_str() {
                    "first_person" => {
                        runtime.set_camera_mode(&id, CameraMode::FirstPerson, seconds)
                    }
                    "third_person" => {
                        runtime.set_camera_mode(&id, CameraMode::ThirdPerson, seconds)
                    }
                    "fixed" => runtime.set_camera_mode(&id, CameraMode::Fixed, seconds),
                    "fixed_object" => runtime.fix_camera_on_object(&id, seconds),
                    other => {
                        let hint = closest(other, MODES)
                            .map_or_else(|| ".".to_owned(), |near| format!("; quis dizer {near}?"));
                        Err(format!(
                            "camera.set_mode não conhece o modo \"{other}\"{hint} Modos: {}",
                            MODES.join(", ")
                        ))
                    }
                }
            })
        },
    )
}

/// `camera.follow(camera, target)`.
pub(crate) fn follow(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(move |lua, (which, target): (LuaValue, LuaValue)| {
        api(lua, &s, "camera.follow", |runtime| {
            let id = camera(&s, runtime, &which, "camera.follow", true)?;
            let target = object_arg(&s, runtime, &target, "camera.follow", "o alvo")?;
            if target == id {
                return Err("camera.follow: uma câmera não acompanha a si mesma".into());
            }
            runtime.set_camera_target(&id, &target)
        })
    })
}

/// `camera.look_at(camera, target)`, a point or an object.
pub(crate) fn look_at(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(move |lua, (which, target): (LuaValue, LuaValue)| {
        api(lua, &s, "camera.look_at", |runtime| {
            let id = camera(&s, runtime, &which, "camera.look_at", true)?;
            let focus = match &target {
                LuaValue::Vector(v) => {
                    let point = Vec3::new(v.x(), v.y(), v.z());
                    if !point.is_finite() {
                        return Err("camera.look_at: o ponto tem infinito ou NaN".into());
                    }
                    CameraLookAt::Point(point)
                }
                LuaValue::LightUserData(_) => CameraLookAt::Object(object_arg(
                    &s,
                    runtime,
                    &target,
                    "camera.look_at",
                    "o alvo",
                )?),
                other => {
                    return Err(format!(
                        "camera.look_at recebe um ponto do mundo (um vetor) ou um objeto, não {}",
                        In::of(other).kind()
                    ));
                }
            };
            look_command(runtime, &id, Some(focus), "camera.look_at")
        })
    })
}

/// `camera.release_look(camera)`.
pub(crate) fn release_look(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(move |lua, which: LuaValue| {
        api(lua, &s, "camera.release_look", |runtime| {
            let id = camera(&s, runtime, &which, "camera.release_look", true)?;
            look_command(runtime, &id, None, "camera.release_look")
        })
    })
}

/// Olhar assistido / liberar olhar on the camera `id`, which must be active.
fn look_command(
    runtime: &mut Runtime,
    id: &str,
    focus: Option<CameraLookAt>,
    function: &str,
) -> Result<(), String> {
    if runtime.active_camera() != Some(id) {
        let name = &runtime.entity(id).expect("checked by camera").name;
        return Err(format!(
            "{function}: {name} não é a câmera ativa; ative-a antes (camera.activate)"
        ));
    }
    runtime.set_camera_look_at(id, focus)
}

/// `camera.set(camera, setting, value, seconds?)`.
pub(crate) fn set(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(
        move |lua, (which, setting, value, time): (LuaValue, LuaValue, LuaValue, LuaValue)| {
            api(lua, &s, "camera.set", |runtime| {
                let setting = text(&setting, "camera.set precisa do ajuste (um texto)")?;
                if !SETTINGS.contains(&setting.as_str()) {
                    let hint = closest(&setting, SETTINGS)
                        .map_or_else(|| ".".to_owned(), |near| format!("; quis dizer {near}?"));
                    return Err(format!(
                        "camera.set não conhece o ajuste \"{setting}\"{hint} Ajustes: {}",
                        SETTINGS.join(", ")
                    ));
                }
                let id = camera(&s, runtime, &which, "camera.set", setting != "fov")?;
                let value = finite(
                    &value,
                    &format!("camera.set: o valor de {setting} é um número"),
                )?;
                let seconds = seconds(&time, SMOOTHING, "camera.set")?;
                runtime
                    .set_camera_setting(&id, &setting, value as f32, seconds)
                    .map_err(|e| format!("camera.set(\"{setting}\"): {e}"))
            })
        },
    )
}
