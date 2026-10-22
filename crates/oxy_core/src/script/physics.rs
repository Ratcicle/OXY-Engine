//! The queries of the 3D physics for scripts (design §5.4): `physics.raycast`, `sphere_cast`,
//! `capsule_cast` and `capsule_fits`, the four nodes of Consultas físicas 3D through the same
//! `Runtime::query_scene`, which moves nothing. A query that touches nothing gives nil; one the
//! engine refuses (a 2D scene, past 256 queries in the step, a zero direction) gives nil and
//! the reason, as the nodes' Consulta válida and Motivo de recusa tell it; a wrong argument is
//! an error. The collision groups go by their names in the project or as a number.
use super::api::{finite, flag, option_name, text};
use super::handles::{self, In, MARK_OBJECT};
use super::host::{Shared, api, object, surface};
use super::objects::vector;
use crate::document::{Id, Project};
use crate::physics3d::QueryHit;
use crate::runtime::{SceneQuery, SceneQueryShape};
use glam::Vec3;
use mlua::{Function, Lua, Table, Value as LuaValue};
use std::rc::Rc;

/// What the options of a query say, read before the call reaches the world.
struct Filter {
    areas: bool,
    /// The handle key of the object left out.
    exclude: Option<u64>,
    category: Option<Groups>,
    mask: Option<Groups>,
}

/// Collision groups as a script gives them: a mask, or names the project resolves.
enum Groups {
    Bits(u32),
    Names(Vec<String>),
}

impl Groups {
    fn read(value: &LuaValue, function: &str, key: &str) -> Result<Self, String> {
        let refused = || {
            format!(
                "{function}: {key} recebe os nomes dos grupos de colisão (\"Personagens\" ou {{ \
                 \"Cenário padrão\", \"Personagens\" }}) ou a máscara em número, não {}",
                In::of(value).kind()
            )
        };
        match value {
            LuaValue::String(_) => Ok(Self::Names(vec![text(value, &refused())?])),
            LuaValue::Table(list) => list
                .sequence_values::<LuaValue>()
                .map(|name| text(&name.map_err(|e| e.to_string())?, &refused()))
                .collect::<Result<_, _>>()
                .map(Self::Names),
            value => {
                let n = finite(value, &refused())?;
                if n.fract() != 0. || !(0. ..=f64::from(u32::MAX)).contains(&n) {
                    return Err(format!(
                        "{function}: {key} em número é uma máscara inteira de 0 a 4294967295"
                    ));
                }
                Ok(Self::Bits(n as u32))
            }
        }
    }

    fn bits(&self, project: &Project, function: &str) -> Result<u32, String> {
        match self {
            Self::Bits(bits) => Ok(*bits),
            Self::Names(names) => names.iter().try_fold(0, |bits, name| {
                project
                    .collision_group_bit(name)
                    .map(|bit| bits | 1 << bit)
                    .map_err(|e| format!("{function}: {e}"))
            }),
        }
    }
}

fn filter(options: &LuaValue, function: &str) -> Result<Filter, String> {
    let mut found = Filter {
        areas: false,
        exclude: None,
        category: None,
        mask: None,
    };
    let table: &Table = match options {
        LuaValue::Nil => return Ok(found),
        LuaValue::Table(table) => table,
        other => {
            return Err(format!(
                "as opções de {function} são uma tabela, como {{ exclude = self }}, não {}",
                In::of(other).kind()
            ));
        }
    };
    for pair in table.pairs::<LuaValue, LuaValue>() {
        let (key, value) = pair.map_err(|e| e.to_string())?;
        match option_name(&key, function)?.as_str() {
            "areas" => {
                found.areas = flag(&value, None, &format!("{function}: areas é true ou false"))?
            }
            "exclude" => {
                found.exclude = match In::of(&value) {
                    In::Nil => None,
                    In::Handle(MARK_OBJECT, key) => Some(key),
                    other => {
                        return Err(format!(
                            "{function}: exclude recebe um objeto, não {}",
                            other.kind()
                        ));
                    }
                }
            }
            "category" => found.category = Some(Groups::read(&value, function, "category")?),
            "mask" => found.mask = Some(Groups::read(&value, function, "mask")?),
            other => {
                return Err(format!(
                    "{function} não conhece a opção {other}; use areas, exclude, category ou mask"
                ));
            }
        }
    }
    Ok(found)
}

/// The numbers of a query's shape, by their names.
fn sizes(values: &[(&LuaValue, &str)], function: &str) -> Result<Vec<f32>, String> {
    values
        .iter()
        .map(|(value, name)| {
            let n = finite(value, &format!("{function}: {name} é um número em metros"))? as f32;
            if !n.is_finite() || n < 0. {
                return Err(format!(
                    "{function}: {name} é um número de metros, 0 ou mais; recebeu {n}"
                ));
            }
            Ok(n)
        })
        .collect()
}

/// A hit as scripts read it.
fn hit_table(lua: &Lua, s: &Shared, hit: QueryHit, key: u64) -> mlua::Result<Table> {
    let table = lua.create_table_with_capacity(0, 6)?;
    table.raw_set("object", object(key))?;
    let v = |v: Vec3| mlua::Vector::new(v.x, v.y, v.z);
    table.raw_set("point", v(hit.point))?;
    table.raw_set("normal", v(hit.normal))?;
    table.raw_set("distance", f64::from(hit.distance))?;
    table.raw_set("fraction", f64::from(hit.fraction))?;
    if let Some(id) = &hit.surface {
        table.raw_set("surface", surface(s.register_surface(id)))?;
    }
    Ok(table)
}

/// What a query found: the hit and the handle key of its object, or why the engine refused it.
type Found = Result<Option<(QueryHit, u64)>, String>;

/// Runs a query of `shape` from `origin` toward `direction` up to `distance`, with `options`;
/// a wrong argument is the outer error, a refused query the inner one.
fn query(
    lua: &Lua,
    s: &Shared,
    function: &'static str,
    shape: Result<SceneQueryShape, String>,
    origin: Result<Vec3, String>,
    direction: Result<(Vec3, f32), String>,
    options: &LuaValue,
) -> mlua::Result<Found> {
    let options = filter(options, function);
    api(lua, s, function, |runtime| {
        let (shape, origin, (direction, distance), options) =
            (shape?, origin?, direction?, options?);
        let exclude: Option<Id> = match options.exclude {
            Some(key) => Some(
                s.id(key)
                    .filter(|id| runtime.entity(id).is_some())
                    .ok_or_else(|| format!("{function}: o objeto de exclude não existe mais"))?,
            ),
            None => None,
        };
        let project = runtime.project();
        let category = match &options.category {
            Some(groups) => groups.bits(project, function)?,
            None => 1,
        };
        let mask = match &options.mask {
            Some(groups) => groups.bits(project, function)?,
            None => u32::MAX,
        };
        let found = runtime.query_scene(&SceneQuery {
            shape,
            origin,
            direction,
            distance,
            include_sensors: options.areas,
            exclude,
            category,
            mask,
        });
        Ok(match found {
            Ok(Some(hit)) => {
                let key = s
                    .register(&hit.object)
                    .map_err(|first| handles::collision(runtime, &first, &hit.object))?;
                Ok(Some((hit, key)))
            }
            Ok(None) => Ok(None),
            Err(reason) => Err(reason),
        })
    })
}

/// A query's result as scripts get it: the hit or nil, and the reason of a refusal.
fn cast_result(lua: &Lua, s: &Shared, found: Found) -> mlua::Result<(LuaValue, LuaValue)> {
    Ok(match found {
        Ok(Some((hit, key))) => (LuaValue::Table(hit_table(lua, s, hit, key)?), LuaValue::Nil),
        Ok(None) => (LuaValue::Nil, LuaValue::Nil),
        Err(reason) => (LuaValue::Nil, LuaValue::String(lua.create_string(reason)?)),
    })
}

/// The direction and the distance of a cast.
fn path(direction: &LuaValue, distance: &LuaValue, function: &str) -> Result<(Vec3, f32), String> {
    let direction = vector(
        direction,
        &format!(
            "{function}: direction é a direção no mundo, um vetor como vector.create(0, 0, -1)"
        ),
    )?;
    let distance = sizes(&[(distance, "distance")], function)?[0];
    Ok((direction, distance))
}

fn origin(value: &LuaValue, function: &str) -> Result<Vec3, String> {
    vector(
        value,
        &format!("{function}: origin é um ponto do mundo, um vetor como vector.create(0, 1, 0)"),
    )
}

/// `physics.raycast(origin, direction, distance, options?)`.
pub(crate) fn raycast(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(
        move |lua, (from, direction, distance, options): (LuaValue, LuaValue, LuaValue, LuaValue)| {
            let f = "physics.raycast";
            let found = query(
                lua,
                &s,
                f,
                Ok(SceneQueryShape::Ray),
                origin(&from, f),
                path(&direction, &distance, f),
                &options,
            )?;
            cast_result(lua, &s, found)
        },
    )
}

/// `physics.sphere_cast(origin, radius, direction, distance, options?)`.
pub(crate) fn sphere_cast(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(
        move |lua,
              (from, radius, direction, distance, options): (
            LuaValue,
            LuaValue,
            LuaValue,
            LuaValue,
            LuaValue,
        )| {
            let f = "physics.sphere_cast";
            let shape =
                sizes(&[(&radius, "radius")], f).map(|n| SceneQueryShape::Sphere { radius: n[0] });
            let found = query(
                lua,
                &s,
                f,
                shape,
                origin(&from, f),
                path(&direction, &distance, f),
                &options,
            )?;
            cast_result(lua, &s, found)
        },
    )
}

/// `physics.capsule_cast(origin, radius, height, direction, distance, options?)`.
pub(crate) fn capsule_cast(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(
        move |lua,
              (from, radius, height, direction, distance, options): (
            LuaValue,
            LuaValue,
            LuaValue,
            LuaValue,
            LuaValue,
            LuaValue,
        )| {
            let f = "physics.capsule_cast";
            let shape = sizes(&[(&radius, "radius"), (&height, "height")], f).map(|n| {
                SceneQueryShape::Capsule {
                    radius: n[0],
                    height: n[1],
                }
            });
            let found = query(
                lua,
                &s,
                f,
                shape,
                origin(&from, f),
                path(&direction, &distance, f),
                &options,
            )?;
            cast_result(lua, &s, found)
        },
    )
}

/// `physics.capsule_fits(position, radius, height, options?)`: whether it fits, the contact
/// when it does not, and why the engine refused the query.
pub(crate) fn capsule_fits(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(
        move |lua, (at, radius, height, options): (LuaValue, LuaValue, LuaValue, LuaValue)| {
            let f = "physics.capsule_fits";
            let shape = sizes(&[(&radius, "radius"), (&height, "height")], f).map(|n| {
                SceneQueryShape::Space {
                    radius: n[0],
                    height: n[1],
                }
            });
            let position = vector(
                &at,
                &format!("{f}: position é o centro da cápsula no mundo, um vetor"),
            );
            let found = query(lua, &s, f, shape, position, Ok((Vec3::NEG_Z, 0.)), &options)?;
            Ok(match found {
                Ok(None) => (true, LuaValue::Nil, LuaValue::Nil),
                Ok(Some((hit, key))) => (
                    false,
                    LuaValue::Table(hit_table(lua, &s, hit, key)?),
                    LuaValue::Nil,
                ),
                Err(reason) => (
                    false,
                    LuaValue::Nil,
                    LuaValue::String(lua.create_string(reason)?),
                ),
            })
        },
    )
}
