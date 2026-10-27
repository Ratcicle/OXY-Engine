//! The views of an object (design §5.4): `obj.character`, `obj.collider`, `obj.platform`,
//! `obj.controller`, `obj.animation` and `obj.ui`, the object's handle with the view's mark
//! (`handles`), which reading on an object without the component refuses, naming the object.
//! Their properties read and write here through the `Runtime`'s operations, as the nodes do: the
//! 3D character's state (Ler personagem 3D, with the aim and the look of the simulated input),
//! the switches of Ativar / desativar componente, the surface of the 3D collider or of the
//! character's body (Ler / Aplicar superfície ao colisor), the clip playing, and the text and
//! color of an interface element (new in 0.7). The character's commands are `character`'s;
//! `Animation:play` is here.
use super::api::text;
use super::handles::{self, In, MARK_SURFACE, Out, Property, object_id, unknown_member};
use super::host::{Shared, api, door};
use super::registry::{self, Imp, View};
use crate::character::Posture;
use crate::document::{Entity, Id};
use crate::runtime::{ObjectComponent, Runtime};
use glam::{Quat, Vec3};
use mlua::{Function, Lua, Value as LuaValue};
use std::rc::Rc;

/// Why `e` has no `view`; none when it has.
fn lacks(e: &Entity, view: View) -> Option<String> {
    let (has, what, kind) = match view {
        View::Character => (
            e.character3d.is_some(),
            "não tem Personagem 3D",
            "character",
        ),
        View::Collider => (
            e.collider.is_some() || e.physics3d.is_some(),
            "não tem colisor nem área",
            "collider",
        ),
        View::Platform => (e.platform.is_some(), "não tem plataforma móvel", "platform"),
        View::Controller => (e.controller.is_some(), "não tem Movimento 2D", "controller"),
        View::Animation => (!e.clips.is_empty(), "não tem animações", "animation"),
        View::Ui => (
            e.ui.is_some(),
            "não é um elemento de interface (texto, imagem, botão ou barra)",
            "ui",
        ),
        View::Object | View::Stream => return None,
    };
    (!has).then(|| {
        format!(
            "{} {what} (confira antes com obj:has_component(\"{kind}\"))",
            e.name
        )
    })
}

/// `obj.character` and the other views of `e`, whose handle has `key`: the handle with the
/// view's mark, when it has the component.
pub(crate) fn open(e: &Entity, property: Property, key: u64) -> Result<Out, String> {
    let view = match property {
        Property::Character => View::Character,
        Property::Collider => View::Collider,
        Property::Platform => View::Platform,
        Property::Controller => View::Controller,
        Property::Animation => View::Animation,
        Property::Ui => View::Ui,
        other => unreachable!("{other:?} is no property of objects"),
    };
    match lacks(e, view) {
        Some(why) => Err(why),
        None => Ok(Out::Handle(view.mark(), key)),
    }
}

/// The object the view whose handle has `key` belongs to, in the scene and with the component.
fn viewed(shared: &Shared, runtime: &Runtime, view: View, key: u64) -> Result<Id, String> {
    let id = object_id(shared, runtime, key)?;
    match lacks(runtime.entity(&id).expect("checked by object_id"), view) {
        Some(why) => Err(why),
        None => Ok(id),
    }
}

/// The object before `:` in `obj.character:method(...)` and the other views' methods.
pub(crate) fn receiver(
    s: &Shared,
    runtime: &Runtime,
    me: &LuaValue,
    view: View,
    method: &str,
) -> Result<Id, String> {
    match me {
        LuaValue::LightUserData(handle) if handles::decode(handle.0).0 == view.mark() => {
            viewed(s, runtime, view, handles::decode(handle.0).1)
        }
        _ => Err(format!(
            "{method} é um método de {}: {}:{method}(...)",
            view.name(),
            view.path()
        )),
    }
}

/// Posture as scripts name it.
pub(crate) fn posture_name(posture: Posture) -> &'static str {
    match posture {
        Posture::Standing => "standing",
        Posture::Crouched => "crouched",
        Posture::Sliding => "sliding",
    }
}

/// `view.name` of the view whose handle has `key`.
pub(crate) fn index(shared: &Shared, view: View, key: u64, name: &[u8]) -> Result<Out, String> {
    let f = registry::member(view, name).ok_or_else(|| unknown_member(view, name))?;
    let Imp::Property(property) = f.imp else {
        return Ok(Out::Method(view.mark()));
    };
    let runtime = door(shared)?;
    let id = viewed(shared, runtime, view, key)?;
    let e = runtime.entity(&id).expect("checked by viewed");
    Ok(match property {
        Property::CharacterState(output) => {
            let value = runtime.character_value(&id, output)?;
            Out::of(shared, runtime, value)?
        }
        Property::Posture => {
            Out::Text(posture_name(runtime.inspect_character(&id)?.posture).into())
        }
        Property::Aim | Property::Look => {
            let state = runtime.inspect_character(&id)?;
            let turn = Quat::from_rotation_y(state.look_yaw);
            if property == Property::Aim {
                let aim = turn * Quat::from_rotation_x(state.pitch) * -Vec3::Z;
                Out::Vector(aim.normalize_or_zero().to_array())
            } else {
                // Adding zero turns -0 into 0, which text would show as -0.
                let yaw = crate::runtime::yaw_of(turn * -Vec3::Z) + 0.;
                Out::Vector([yaw, state.pitch.to_degrees() + 0., 0.])
            }
        }
        Property::Enabled => Out::Bool(match view {
            View::Collider => {
                e.collider.as_ref().is_some_and(|c| c.enabled)
                    || e.physics3d.as_ref().is_some_and(|c| c.enabled)
            }
            View::Platform => e.platform.as_ref().is_some_and(|p| p.enabled),
            _ => e.controller.as_ref().is_some_and(|c| c.enabled),
        }),
        Property::IsArea => Out::Bool(
            e.collider.as_ref().is_some_and(|c| c.is_trigger)
                || e.physics3d.as_ref().is_some_and(|c| c.sensor),
        ),
        Property::Surface => {
            if view == View::Collider && e.physics3d.is_none() {
                return Err(flat_collider(e));
            }
            match runtime.object_surface(&id)? {
                Some(surface) => Out::Handle(MARK_SURFACE, shared.register_surface(&surface)),
                None => Out::Nil,
            }
        }
        Property::Playing => Out::Bool(runtime.animation_playing(&id).is_some_and(|(_, on)| on)),
        Property::Clip => match runtime.animation_playing(&id) {
            Some((clip, _)) => e
                .clips
                .iter()
                .find(|c| c.id == clip)
                .map_or(Out::Nil, |c| Out::Text(c.name.clone())),
            None => Out::Nil,
        },
        Property::Text => Out::Text(e.ui.as_ref().map(|ui| ui.text.clone()).unwrap_or_default()),
        Property::Color => {
            let [r, g, b, _] = e.ui.as_ref().map_or([1.; 4], |ui| ui.color);
            Out::Vector([r, g, b])
        }
        other => unreachable!("{other:?} is no property of {view:?}"),
    })
}

/// Why the collider of `e`, a 2D one, has no surface.
fn flat_collider(e: &Entity) -> String {
    format!(
        "o colisor de {} é 2D e não tem superfície: só um colisor 3D tem (a do corpo de um \
         personagem é obj.character.body_surface)",
        e.name
    )
}

/// `view.name = value` of a property a script may write.
pub(crate) fn write(
    shared: &Shared,
    view: View,
    key: u64,
    property: Property,
    name: &str,
    value: In,
) -> Result<(), String> {
    let refused =
        |what: &str| format!("{}.{name} recebe {what}, não {}", view.path(), value.kind());
    match property {
        Property::Enabled | Property::Playing => {
            let In::Bool(on) = value else {
                return Err(refused("true ou false"));
            };
            let component = match view {
                View::Collider => ObjectComponent::Collider,
                View::Platform => ObjectComponent::Platform,
                View::Controller => ObjectComponent::Controller,
                _ => ObjectComponent::Animation,
            };
            let runtime = door(shared)?;
            let id = viewed(shared, runtime, view, key)?;
            runtime.set_component_enabled(&id, component, on)
        }
        Property::Surface => {
            let surface = match &value {
                In::Nil => None,
                In::Handle(MARK_SURFACE, k) => Some(
                    shared
                        .surface_id(*k)
                        .ok_or("superfície desconhecida da engine")?,
                ),
                In::Text(_) => None,
                _ => {
                    return Err(refused(
                        "uma superfície (de um atributo Superfície ou de outro colisor), o nome \
                         de uma superfície do projeto ou nil",
                    ));
                }
            };
            let runtime = door(shared)?;
            let id = viewed(shared, runtime, view, key)?;
            let e = runtime.entity(&id).expect("checked by viewed");
            if view == View::Collider && e.physics3d.is_none() {
                return Err(flat_collider(e));
            }
            let surface = match value {
                In::Text(named) => Some(super::surface_named(
                    &runtime.project().surfaces,
                    &named,
                    &format!(
                        "{}.{name} = \"{named}\"",
                        view.path().trim_start_matches("obj.")
                    ),
                )?),
                _ => surface,
            };
            runtime.apply_surface(&id, surface)
        }
        Property::Text => {
            let In::Text(text) = value else {
                return Err(refused("um texto"));
            };
            let runtime = door(shared)?;
            let id = viewed(shared, runtime, view, key)?;
            runtime.set_ui_text(&id, &text)
        }
        Property::Color => {
            let In::Vector(color) = value else {
                return Err(refused(
                    "um vetor com o vermelho, o verde e o azul de 0 a 1, como vector.create(1, \
                     0.5, 0)",
                ));
            };
            let runtime = door(shared)?;
            let id = viewed(shared, runtime, view, key)?;
            runtime.set_ui_color(&id, color)
        }
        other => unreachable!("{other:?} is not written"),
    }
}

/// `obj.animation:play(clip, restart?)`.
pub(crate) fn animation_play(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(
        move |lua, (me, clip, restart): (LuaValue, LuaValue, LuaValue)| {
            api(lua, &s, "play", |runtime| {
                let id = receiver(&s, runtime, &me, View::Animation, "play")?;
                let name = text(
                    &clip,
                    "play precisa do nome do clipe (um texto), como no Estúdio",
                )?;
                let restart = match restart {
                    LuaValue::Nil => true,
                    LuaValue::Boolean(restart) => restart,
                    other => {
                        return Err(format!(
                            "play: restart é true (recomeça, o padrão) ou false (um clipe que já \
                             toca segue), não {}",
                            In::of(&other).kind()
                        ));
                    }
                };
                let e = runtime.entity(&id).expect("checked by receiver");
                let wanted = name.trim().to_lowercase();
                let clip = e
                    .clips
                    .iter()
                    .find(|c| c.name == name)
                    .or_else(|| {
                        e.clips
                            .iter()
                            .find(|c| c.name.trim().to_lowercase() == wanted)
                    })
                    .map(|c| c.id.clone())
                    .ok_or_else(|| {
                        let names: Vec<&str> = e.clips.iter().map(|c| c.name.as_str()).collect();
                        let hint = super::closest(&name, names.iter().copied())
                            .map_or_else(|| ".".to_owned(), |near| format!("; quis dizer {near}?"));
                        format!(
                            "{} não tem o clipe \"{name}\"{hint} Clipes: {}",
                            e.name,
                            names.join(", ")
                        )
                    })?;
                runtime.play_animation(&id, &clip, restart)
            })
        },
    )
}
