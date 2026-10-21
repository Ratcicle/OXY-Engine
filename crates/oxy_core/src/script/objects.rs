//! Objects and the scene for scripts (design §5.4): the methods of objects written with mlua
//! (the hierarchy, components, copies, removal, `first_hit` and the transform's methods) and
//! the global table `scene` (finding objects by name or attribute, models, the scene running
//! and changing it). They reach the world through the `Runtime`'s operations, as the nodes do,
//! and name objects, models and scenes by the names the person gave them.
//!
//! Lists of objects come in scene order. A copy (`obj:clone`, `scene.spawn_model`) takes its IDs
//! from the object running, the script and the copies it made before (stage 2's counters), so
//! a run that makes the same copies gives them the same IDs; its scripts hear `on_start` once
//! the call and the tasks already queued ran. `scene.change` opens the scene when the call
//! ends: the rest of the function still runs in the scene it is in.
use super::api::{option_name, text};
use super::errors::closest;
use super::handles::{self, In, MARK_OBJECT};
use super::host::{Shared, api, object};
use crate::document::{Id, Project};
use crate::runtime::{Runtime, TransformPart, TransformSpace};
use glam::Vec3;
use mlua::{Function, Lua, Table, Value as LuaValue};
use std::collections::HashSet;
use std::rc::Rc;

/// Names a message about a list writes before it counts the rest.
const LISTED: usize = 10;

/// The components `has_component` knows, in order.
const COMPONENTS: [&str; 8] = [
    "animation",
    "area",
    "camera",
    "character",
    "collider",
    "controller",
    "platform",
    "ui",
];

/// The object `value` is, in the scene: its ID; why not. `what` names the method or the
/// parameter in the error.
fn object_arg(s: &Shared, runtime: &Runtime, value: &LuaValue, what: &str) -> Result<Id, String> {
    let LuaValue::LightUserData(handle) = value else {
        return Err(format!(
            "{what} precisa de um objeto, não {}",
            In::of(value).kind()
        ));
    };
    match handles::decode(handle.0) {
        (MARK_OBJECT, key) => s
            .id(key)
            .filter(|id| runtime.entity(id).is_some())
            .ok_or_else(|| s.gone(key)),
        (mark, _) => Err(format!(
            "{what} precisa de um objeto, não {}",
            handles::mark_kind(mark)
        )),
    }
}

/// The object before `:` in `obj:method(...)`.
fn receiver(s: &Shared, runtime: &Runtime, me: &LuaValue, method: &str) -> Result<Id, String> {
    match me {
        LuaValue::LightUserData(handle) if handles::decode(handle.0).0 == MARK_OBJECT => {
            object_arg(s, runtime, me, method)
        }
        _ => Err(format!(
            "{method} é um método de objetos: obj:{method}(...)"
        )),
    }
}

/// A vector argument, or `refused`.
fn vector(value: &LuaValue, refused: &str) -> Result<Vec3, String> {
    match value {
        LuaValue::Vector(v) if [v.x(), v.y(), v.z()].iter().all(|c| c.is_finite()) => {
            Ok(Vec3::new(v.x(), v.y(), v.z()))
        }
        LuaValue::Vector(_) => Err(format!("{refused}; este tem infinito ou NaN")),
        other => Err(format!("{refused}, não {}", In::of(other).kind())),
    }
}

/// The handle keys of `ids`, registering them.
fn keys(s: &Shared, runtime: &Runtime, ids: Vec<Id>) -> Result<Vec<u64>, String> {
    ids.into_iter()
        .map(|id| {
            s.register(&id)
                .map_err(|first| handles::collision(runtime, &first, &id))
        })
        .collect()
}

/// A Luau list of the objects of `keys`.
fn list(lua: &Lua, keys: Vec<u64>) -> mlua::Result<Table> {
    lua.create_sequence_from(keys.into_iter().map(object))
}

/// `names`, the first [`LISTED`] of them and then how many more.
fn listed(names: &[&str]) -> String {
    let mut shown: Vec<String> = names.iter().take(LISTED).map(|n| (*n).to_owned()).collect();
    if names.len() > LISTED {
        shown.push(format!("e mais {}", names.len() - LISTED));
    }
    shown.join(", ")
}

/// `obj:children()`.
pub(crate) fn children(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(move |lua, me: LuaValue| {
        let found = api(lua, &s, "children", |runtime| {
            let id = receiver(&s, runtime, &me, "children")?;
            let ids = runtime
                .scene()
                .entities
                .iter()
                .filter(|e| e.parent.as_deref() == Some(&id))
                .map(|e| e.id.clone())
                .collect();
            keys(&s, runtime, ids)
        })?;
        list(lua, found)
    })
}

/// The children of `id` (with `deep`, everything under it) named `name`, in scene order.
fn named_under(runtime: &Runtime, id: &str, name: &str, deep: bool) -> Vec<Id> {
    let scene = runtime.scene();
    let under: HashSet<Id> = if deep {
        scene
            .descendants(id)
            .into_iter()
            .filter(|found| found != id)
            .collect()
    } else {
        HashSet::new()
    };
    scene
        .entities
        .iter()
        .filter(|e| {
            e.name == name
                && if deep {
                    under.contains(&e.id)
                } else {
                    e.parent.as_deref() == Some(id)
                }
        })
        .map(|e| e.id.clone())
        .collect()
}

/// The arguments of `find_child` and `get_child`: the name and whether to look deep.
fn child_args(name: &LuaValue, deep: &LuaValue, method: &str) -> Result<(String, bool), String> {
    let name = text(
        name,
        &format!("{method} precisa do nome do filho (um texto)"),
    )?;
    let deep = match deep {
        LuaValue::Nil => false,
        LuaValue::Boolean(deep) => *deep,
        other => {
            return Err(format!(
                "{method}: deep é true (procurar em tudo o que está abaixo) ou false, não {}",
                In::of(other).kind()
            ));
        }
    };
    Ok((name, deep))
}

/// `obj:find_child(name, deep?)`.
pub(crate) fn find_child(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(
        move |lua, (me, name, deep): (LuaValue, LuaValue, LuaValue)| {
            let found = api(lua, &s, "find_child", |runtime| {
                let id = receiver(&s, runtime, &me, "find_child")?;
                let (name, deep) = child_args(&name, &deep, "find_child")?;
                let first = named_under(runtime, &id, &name, deep).into_iter().next();
                keys(&s, runtime, first.into_iter().collect())
            })?;
            Ok(found.into_iter().next().map_or(LuaValue::Nil, object))
        },
    )
}

/// `obj:get_child(name, deep?)`.
pub(crate) fn get_child(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(
        move |lua, (me, name, deep): (LuaValue, LuaValue, LuaValue)| {
            let found = api(lua, &s, "get_child", |runtime| {
                let id = receiver(&s, runtime, &me, "get_child")?;
                let (name, deep) = child_args(&name, &deep, "get_child")?;
                match named_under(runtime, &id, &name, deep).into_iter().next() {
                    Some(child) => keys(&s, runtime, vec![child]),
                    None => Err(no_child(runtime, &id, &name, deep)),
                }
            });
            found.map(|keys| object(keys[0]))
        },
    )
}

/// Why `get_child` found no `name` under `id`: with the names there are.
fn no_child(runtime: &Runtime, id: &str, name: &str, deep: bool) -> String {
    let scene = runtime.scene();
    let owner = runtime.entity(id).map_or("o objeto", |e| e.name.as_str());
    let under: HashSet<Id> = scene.descendants(id).into_iter().collect();
    let names: Vec<&str> = scene
        .entities
        .iter()
        .filter(|e| {
            e.id != id
                && if deep {
                    under.contains(&e.id)
                } else {
                    e.parent.as_deref() == Some(id)
                }
        })
        .map(|e| e.name.as_str())
        .collect();
    let missing = if deep {
        format!("{owner} não tem nada chamado \"{name}\" abaixo dele")
    } else {
        format!("{owner} não tem um filho chamado \"{name}\"")
    };
    if names.is_empty() {
        let none = if deep {
            "não há nada abaixo dele"
        } else {
            "ele não tem filhos"
        };
        return format!("{missing}: {none}");
    }
    let hint = closest(name, names.iter().copied())
        .map_or_else(String::new, |near| format!(" (quis dizer {near}?)"));
    let are = if deep { "abaixo dele" } else { "filhos" };
    format!(
        "{missing}{hint}; {are}: {}; para quando a ausência é esperada, use find_child, que dá \
         nil",
        listed(&names)
    )
}

/// `obj:has_component(kind)`.
pub(crate) fn has_component(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(move |lua, (me, kind): (LuaValue, LuaValue)| {
        api(lua, &s, "has_component", |runtime| {
            let id = receiver(&s, runtime, &me, "has_component")?;
            let kind = text(
                &kind,
                "has_component precisa do nome do componente (um texto), como \"collider\"",
            )?;
            let e = runtime.entity(&id).expect("checked by receiver");
            Ok(match kind.as_str() {
                "animation" => !e.clips.is_empty(),
                "area" => {
                    e.collider.as_ref().is_some_and(|c| c.is_trigger)
                        || e.physics3d.as_ref().is_some_and(|c| c.sensor)
                }
                "camera" => e.camera.is_some() || e.camera_rig.is_some(),
                "character" => e.character3d.is_some(),
                "collider" => e.collider.is_some() || e.physics3d.is_some(),
                "controller" => e.controller.is_some(),
                "platform" => e.platform.is_some(),
                "ui" => e.ui.is_some(),
                other => {
                    let hint = closest(other, COMPONENTS)
                        .map_or_else(String::new, |near| format!("; quis dizer {near}?"));
                    return Err(format!(
                        "has_component não conhece o componente \"{other}\"{hint} Componentes: {}",
                        COMPONENTS.join(", ")
                    ));
                }
            })
        })
    })
}

/// `obj:destroy()`.
pub(crate) fn destroy(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(move |lua, me: LuaValue| {
        api(lua, &s, "destroy", |runtime| {
            let id = receiver(&s, runtime, &me, "destroy")?;
            let gone: HashSet<Id> = runtime.scene().descendants(&id).into_iter().collect();
            runtime.remove_object(&id);
            s.forget(&gone);
            Ok(())
        })
    })
}

/// What `obj:clone` and `scene.spawn_model` take in their options, read before the call
/// reaches the world.
#[derive(Default)]
struct Placement {
    offset: Option<Vec3>,
    position: Option<Vec3>,
    rotation: Option<Vec3>,
    /// The parent's handle key.
    parent: Option<u64>,
    attributes: Vec<(String, In)>,
}

/// The options of `function`, which takes the fields `known`.
fn placement(options: &LuaValue, function: &str, known: &[&str]) -> Result<Placement, String> {
    let table = match options {
        LuaValue::Nil => return Ok(Placement::default()),
        LuaValue::Table(table) => table,
        other => {
            return Err(format!(
                "as opções de {function} são uma tabela, como {{ position = vector.create(0, 1, \
                 0) }}, não {}",
                In::of(other).kind()
            ));
        }
    };
    let mut found = Placement::default();
    for pair in table.pairs::<LuaValue, LuaValue>() {
        let (key, value) = pair.map_err(|e| e.to_string())?;
        let key = option_name(&key, function)?;
        let refused = |what: &str| format!("{function}: {key} recebe {what}");
        match key.as_str() {
            "offset" | "position" | "rotation" if known.contains(&key.as_str()) => {
                let v = vector(&value, &refused("um vetor, como vector.create(0, 1, 0)"))?;
                match key.as_str() {
                    "offset" => found.offset = Some(v),
                    "position" => found.position = Some(v),
                    _ => found.rotation = Some(v),
                }
            }
            "parent" => match In::of(&value) {
                In::Handle(MARK_OBJECT, key) => found.parent = Some(key),
                In::Nil => {}
                other => return Err(refused(&format!("um objeto, não {}", other.kind()))),
            },
            "attributes" => {
                let LuaValue::Table(attributes) = &value else {
                    return Err(refused("uma tabela de atributos, como { Vida = 3 }"));
                };
                for pair in attributes.pairs::<LuaValue, LuaValue>() {
                    let (name, value) = pair.map_err(|e| e.to_string())?;
                    let LuaValue::String(name) = name else {
                        return Err(refused("uma tabela com os nomes dos atributos em texto"));
                    };
                    let name = name.to_str().map_err(|e| e.to_string())?.to_owned();
                    found.attributes.push((name, In::of(&value)));
                }
                found.attributes.sort_by(|a, b| a.0.cmp(&b.0));
            }
            _ => {
                return Err(format!(
                    "{function} não conhece a opção {key}; use {}",
                    known.join(", ")
                ));
            }
        }
    }
    if found.offset.is_some() && found.position.is_some() {
        return Err(format!("{function}: use offset ou position, não os dois"));
    }
    Ok(found)
}

/// Places the copy `root` just made as `placement` says and gives it its attributes; a refusal
/// takes the copy away again, so nothing is made.
fn settle(
    s: &Shared,
    runtime: &mut Runtime,
    root: &Id,
    placement: Placement,
    function: &str,
) -> Result<u64, String> {
    let settled = (|| {
        let parent = match placement.parent {
            Some(key) => Some(
                s.id(key)
                    .filter(|id| runtime.entity(id).is_some())
                    .ok_or_else(|| format!("{function}: o objeto de parent não existe mais"))?,
            ),
            None => None,
        };
        runtime.place_spawned(
            root,
            parent.as_deref(),
            placement.position,
            placement.rotation,
        )?;
        for (name, value) in placement.attributes {
            let old = runtime
                .attribute(root, &name)?
                .ok_or_else(|| handles::missing_attribute(runtime, root, &name, false))?;
            let new = handles::attribute_value(s, &name, old, value)?;
            runtime.set_attribute(root, &name, new)?;
        }
        s.register(root)
            .map_err(|first| handles::collision(runtime, &first, root))
    })();
    if settled.is_err() {
        runtime.remove_object(root);
    }
    settled.map_err(|e| format!("{function}: {e}"))
}

/// The object running and its script, which name a copy's IDs.
fn copier(s: &Shared) -> Result<(Id, String), String> {
    let key = s
        .caller_key()
        .ok_or("cópia fora de uma chamada de script")?;
    let owner = s.id(key).ok_or_else(|| s.gone(key))?;
    let script = s.module().ok_or("cópia fora de uma chamada de script")?;
    Ok((owner, script.path.to_string()))
}

/// `obj:clone(options?)`.
pub(crate) fn clone(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(move |lua, (me, options): (LuaValue, LuaValue)| {
        let placement = placement(
            &options,
            "clone",
            &["offset", "position", "rotation", "parent", "attributes"],
        );
        let key = api(lua, &s, "clone", |runtime| {
            let id = receiver(&s, runtime, &me, "clone")?;
            let placement = placement?;
            let (owner, script) = copier(&s)?;
            let offset = placement.offset.unwrap_or(Vec3::ZERO);
            let root = runtime.script_spawn_copy(&id, offset, &owner, &script)?;
            settle(&s, runtime, &root, placement, "clone")
        })?;
        Ok(object(key))
    })
}

/// `obj:first_hit(other)`.
pub(crate) fn first_hit(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(move |lua, (me, other): (LuaValue, LuaValue)| {
        api(lua, &s, "first_hit", |runtime| {
            let area = receiver(&s, runtime, &me, "first_hit")?;
            let target = object_arg(&s, runtime, &other, "first_hit")?;
            runtime.first_hit(&area, &target)
        })
    })
}

/// `obj:jump_to(position)`.
pub(crate) fn jump_to(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(move |lua, (me, position): (LuaValue, LuaValue)| {
        api(lua, &s, "jump_to", |runtime| {
            let id = receiver(&s, runtime, &me, "jump_to")?;
            let position = vector(
                &position,
                "jump_to recebe a posição no mundo, um vetor como vector.create(0, 1, 0)",
            )?;
            runtime.write_transform(
                &id,
                TransformPart::Position,
                TransformSpace::World,
                position,
                false,
                true,
            )
        })
    })
}

/// `obj:to_world_direction(direction)` and, `inverse`, `obj:to_local_direction(direction)`.
fn direction(
    lua: &Lua,
    shared: &Rc<Shared>,
    method: &'static str,
    inverse: bool,
) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(move |lua, (me, direction): (LuaValue, LuaValue)| {
        let turned = api(lua, &s, method, |runtime| {
            let id = receiver(&s, runtime, &me, method)?;
            let v = vector(
                &direction,
                &format!("{method} recebe uma direção, um vetor como vector.create(0, 0, -1)"),
            )?;
            let rotation = runtime.world_rotation(&id)?;
            Ok(if inverse {
                rotation.inverse() * v
            } else {
                rotation * v
            })
        })?;
        Ok(mlua::Vector::new(turned.x, turned.y, turned.z))
    })
}

pub(crate) fn to_world_direction(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    direction(lua, shared, "to_world_direction", false)
}

pub(crate) fn to_local_direction(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    direction(lua, shared, "to_local_direction", true)
}

/// The objects of the scene running that `matches`, in scene order.
fn scene_objects(
    s: &Shared,
    runtime: &Runtime,
    matches: impl Fn(&crate::document::Entity) -> bool,
) -> Result<Vec<u64>, String> {
    let ids = runtime
        .scene()
        .entities
        .iter()
        .filter(|e| matches(e))
        .map(|e| e.id.clone())
        .collect();
    keys(s, runtime, ids)
}

/// Why the scene running has no object named `name`: with the nearest name it has.
fn no_object(runtime: &Runtime, name: &str) -> String {
    let scene = runtime.scene();
    let hint = closest(name, scene.entities.iter().map(|e| e.name.as_str()))
        .map_or_else(String::new, |near| format!(" (quis dizer {near}?)"));
    format!(
        "a cena {} não tem um objeto chamado \"{name}\"{hint}; para quando a ausência é \
         esperada, use scene.find, que dá nil",
        scene.name
    )
}

/// `scene.find(name)` and, `required`, `scene.get(name)`.
fn find(
    lua: &Lua,
    shared: &Rc<Shared>,
    function: &'static str,
    required: bool,
) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(move |lua, name: LuaValue| {
        let found = api(lua, &s, function, |runtime| {
            let name = text(
                &name,
                &format!("{function} precisa do nome do objeto (um texto)"),
            )?;
            let mut found = scene_objects(&s, runtime, |e| e.name == name)?;
            found.truncate(1);
            if required && found.is_empty() {
                return Err(no_object(runtime, &name));
            }
            Ok(found)
        })?;
        Ok(found.first().copied().map_or(LuaValue::Nil, object))
    })
}

pub(crate) fn scene_find(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    find(lua, shared, "scene.find", false)
}

pub(crate) fn scene_get(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    find(lua, shared, "scene.get", true)
}

/// `scene.find_all(name)`.
pub(crate) fn scene_find_all(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(move |lua, name: LuaValue| {
        let found = api(lua, &s, "scene.find_all", |runtime| {
            let name = text(
                &name,
                "scene.find_all precisa do nome dos objetos (um texto)",
            )?;
            scene_objects(&s, runtime, |e| e.name == name)
        })?;
        list(lua, found)
    })
}

/// `scene.with_attribute(name)`.
pub(crate) fn scene_with_attribute(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(move |lua, name: LuaValue| {
        let found = api(lua, &s, "scene.with_attribute", |runtime| {
            let name = text(
                &name,
                "scene.with_attribute precisa do nome do atributo (um texto)",
            )?;
            scene_objects(&s, runtime, |e| e.attributes.contains_key(&name))
        })?;
        list(lua, found)
    })
}

/// `scene.spawn_model(model, options?)`.
pub(crate) fn scene_spawn_model(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(move |lua, (model, options): (LuaValue, LuaValue)| {
        let model = text(
            &model,
            "scene.spawn_model precisa do nome do modelo (um texto), como na Biblioteca",
        );
        let placement = placement(
            &options,
            "scene.spawn_model",
            &["position", "rotation", "parent", "attributes"],
        );
        let key = api(lua, &s, "scene.spawn_model", |runtime| {
            let model = model?;
            let placement = placement?;
            let (owner, script) = copier(&s)?;
            let root = runtime.script_spawn_model(&model, &owner, &script)?;
            settle(&s, runtime, &root, placement, "scene.spawn_model")
        })?;
        Ok(object(key))
    })
}

/// `scene.current()`.
pub(crate) fn scene_current(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(move |lua, ()| {
        api(lua, &s, "scene.current", |runtime| {
            Ok(runtime.scene().name.clone())
        })
    })
}

/// The scene of `project` that `name` names: by its name in any case and without the spaces at
/// its ends, when no other scene has it (or by its ID).
fn scene_named(project: &Project, name: &str) -> Result<Id, String> {
    if let Some(scene) = project.scene(name) {
        return Ok(scene.id.clone());
    }
    let wanted = name.trim().to_lowercase();
    let named: Vec<&str> = project
        .scenes
        .iter()
        .filter(|s| s.name.trim().to_lowercase() == wanted)
        .map(|s| s.id.as_str())
        .collect();
    match named.as_slice() {
        [one] => Ok((*one).to_owned()),
        [] => {
            let names: Vec<&str> = project.scenes.iter().map(|s| s.name.as_str()).collect();
            let hint = closest(name, names.iter().copied())
                .map_or_else(|| ".".to_owned(), |near| format!("; quis dizer {near}?"));
            Err(format!(
                "a cena \"{name}\" não existe no projeto{hint} Cenas: {}",
                listed(&names)
            ))
        }
        many => Err(format!(
            "{} cenas se chamam \"{name}\"; renomeie uma delas para abri-la pelo nome",
            many.len()
        )),
    }
}

/// `scene.change(name)`.
pub(crate) fn scene_change(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(move |lua, name: LuaValue| {
        api(lua, &s, "scene.change", |runtime| {
            let name = text(
                &name,
                "scene.change precisa do nome da cena (um texto), como no projeto",
            )?;
            s.change_scene(scene_named(runtime.project(), &name)?);
            Ok(())
        })
    })
}

/// `scene.restart()`.
pub(crate) fn scene_restart(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(move |lua, ()| {
        api(lua, &s, "scene.restart", |runtime| {
            s.change_scene(runtime.scene_id().to_owned());
            Ok(())
        })
    })
}
