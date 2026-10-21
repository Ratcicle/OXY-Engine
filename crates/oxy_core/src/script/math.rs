//! The math the API adds to Luau's (design §5.4): `vmath`, what the vector nodes did beyond
//! Luau's own `vector` library (a normalization that gives zero for a zero vector, the distance,
//! the vector on the ground) and the horizontal turn that points somewhere; and `quat`,
//! quaternions in the axes and degrees of the editor.
//!
//! A quaternion is a userdata with a tag of its own and the engine's metatable, which never
//! changes: no function writes into one, every operation makes another, so a quaternion made at
//! a module's top is a constant, which the freeze lets the module keep (design §3.9). Memory,
//! timers and messages do not keep one: its angles do (`quat.to_euler`). Its fields `x`, `y`,
//! `z` and `w` read; `a * b` and `q * v` multiply as `quat.multiply` and `quat.rotate` do.
//!
//! The functions are C functions that read their arguments off the stack and raise the error,
//! in Portuguese and at the script line, after dropping what Rust holds (the exception rule of
//! [`crate::script`]). Their math is glam's and cannot panic.
use super::handles::{self, raise_here};
use super::host::Shared;
use glam::{DVec3, EulerRot, Mat3, Quat, Vec3};
use mlua::{Function, Lua, ffi};
use std::ffi::CStr;
use std::os::raw::c_int;
use std::rc::Rc;

/// The userdata tag of quaternions (mlua keeps to tags 0 and 1).
pub(crate) const QUAT_TAG: c_int = 64;

/// Installs the metatable of quaternions, frozen.
pub(crate) fn install(lua: &Lua) -> mlua::Result<()> {
    let metamethods: [(&CStr, ffi::lua_CFunction); 5] = [
        (c"__index", quat_index),
        (c"__newindex", quat_newindex),
        (c"__tostring", quat_tostring),
        (c"__eq", quat_eq),
        (c"__mul", quat_mul),
    ];
    // SAFETY: makes one table, gives it to the tag and leaves the stack empty; `exec_raw` runs
    // it protected. The tag's metatable is set once, by the VM's creation.
    unsafe {
        lua.exec_raw::<()>((), |l| {
            ffi::lua_createtable(l, 0, 8);
            for (name, f) in metamethods {
                ffi::lua_pushcclosurek(l, f, name.as_ptr(), 0, None);
                ffi::lua_rawsetfield(l, -2, name.as_ptr());
            }
            ffi::lua_pushstring(l, c"Quat".as_ptr());
            ffi::lua_rawsetfield(l, -2, c"__type".as_ptr());
            ffi::lua_pushstring(l, c"travado".as_ptr());
            ffi::lua_rawsetfield(l, -2, c"__metatable".as_ptr());
            ffi::lua_setreadonly(l, -1, 1);
            ffi::lua_setuserdatametatable(l, QUAT_TAG);
        })
    }
}

/// Pushes a quaternion with `q`'s components (x, y, z, w).
///
/// # Safety
/// `l` runs protected or a C function Luau called: a memory error unwinds from here.
pub(crate) unsafe fn push_quat(l: *mut ffi::lua_State, q: [f32; 4]) {
    unsafe {
        let p = ffi::lua_newuserdatataggedwithmetatable(l, size_of::<[f32; 4]>(), QUAT_TAG);
        (p as *mut [f32; 4]).write_unaligned(q);
    }
}

/// The components of the quaternion at `idx`, if it is one.
///
/// # Safety
/// `idx` is a valid index of `l`.
pub(crate) unsafe fn quat_at(l: *mut ffi::lua_State, idx: c_int) -> Option<[f32; 4]> {
    unsafe {
        let p = ffi::lua_touserdatatagged(l, idx, QUAT_TAG);
        (!p.is_null()).then(|| (p as *const [f32; 4]).read_unaligned())
    }
}

/// What the value at `idx` is, in words.
///
/// # Safety
/// `idx` is a valid index of `l`.
unsafe fn kind_at(l: *mut ffi::lua_State, idx: c_int) -> &'static str {
    unsafe {
        match ffi::lua_type(l, idx) {
            ffi::LUA_TNIL | ffi::LUA_TNONE => "nil",
            ffi::LUA_TBOOLEAN => "booleano",
            ffi::LUA_TNUMBER => "número",
            ffi::LUA_TSTRING => "texto",
            ffi::LUA_TVECTOR => "vetor",
            ffi::LUA_TTABLE => "tabela",
            ffi::LUA_TFUNCTION => "função",
            ffi::LUA_TLIGHTUSERDATA => {
                handles::mark_kind(handles::decode(ffi::lua_tolightuserdata(l, idx)).0)
            }
            _ if quat_at(l, idx).is_some() => "quatérnio",
            _ => "outro valor",
        }
    }
}

/// The number argument `n`, named `name` in `function`'s error.
///
/// # Safety
/// Called from a C function Luau runs.
unsafe fn number(l: *mut ffi::lua_State, n: c_int, function: &str, name: &str) -> f32 {
    unsafe {
        if ffi::lua_type(l, n) != ffi::LUA_TNUMBER {
            raise_here(
                l,
                format!(
                    "{function}: {name} precisa ser um número, não {}",
                    kind_at(l, n)
                ),
            )
        }
        ffi::lua_tonumberx(l, n, std::ptr::null_mut()) as f32
    }
}

/// The vector argument `n`.
///
/// # Safety
/// Called from a C function Luau runs.
unsafe fn vector(l: *mut ffi::lua_State, n: c_int, function: &str, name: &str) -> Vec3 {
    unsafe {
        if ffi::lua_type(l, n) != ffi::LUA_TVECTOR {
            raise_here(
                l,
                format!(
                    "{function}: {name} precisa ser um vetor, como vector.create(0, 1, 0), não {}",
                    kind_at(l, n)
                ),
            )
        }
        let v = ffi::lua_tovector(l, n);
        Vec3::new(*v, *v.add(1), *v.add(2))
    }
}

/// The quaternion argument `n`.
///
/// # Safety
/// Called from a C function Luau runs.
unsafe fn quaternion(l: *mut ffi::lua_State, n: c_int, function: &str, name: &str) -> Quat {
    unsafe {
        match quat_at(l, n) {
            Some(q) => Quat::from_array(q),
            None => raise_here(
                l,
                format!(
                    "{function}: {name} precisa ser um quatérnio, como quat.identity(), não {}",
                    kind_at(l, n)
                ),
            ),
        }
    }
}

/// # Safety
/// As for [`push_quat`].
unsafe fn push_vector(l: *mut ffi::lua_State, v: Vec3) -> c_int {
    unsafe {
        ffi::lua_pushvector(l, v.x, v.y, v.z);
        1
    }
}

/// A result that left the numbers a vector or a quaternion keeps.
///
/// # Safety
/// Called from a C function Luau runs.
unsafe fn finite(l: *mut ffi::lua_State, function: &str, ok: bool) {
    if !ok {
        unsafe {
            raise_here(
                l,
                format!("{function}: o resultado sai dos números que um vetor guarda"),
            )
        }
    }
}

/// The horizontal turn, in degrees, of `direction` on the ground, as `obj.yaw` reads an object's
/// forward (-Z): 0 points to -Z, 90 to -X. A vertical or zero direction turns 0.
pub(crate) fn yaw_of(direction: Vec3) -> f32 {
    if direction.x == 0. && direction.z == 0. {
        return 0.;
    }
    crate::runtime::yaw_of(direction)
}

unsafe extern "C-unwind" fn lerp(l: *mut ffi::lua_State) -> c_int {
    unsafe {
        let a = vector(l, 1, "vmath.lerp", "a");
        let b = vector(l, 2, "vmath.lerp", "b");
        let t = number(l, 3, "vmath.lerp", "t");
        let v = a + (b - a) * t;
        finite(l, "vmath.lerp", v.is_finite());
        push_vector(l, v)
    }
}

unsafe extern "C-unwind" fn distance(l: *mut ffi::lua_State) -> c_int {
    unsafe {
        let a = vector(l, 1, "vmath.distance", "a");
        let b = vector(l, 2, "vmath.distance", "b");
        ffi::lua_pushnumber(l, a.as_dvec3().distance(b.as_dvec3()));
        1
    }
}

unsafe extern "C-unwind" fn normalize_or_zero(l: *mut ffi::lua_State) -> c_int {
    unsafe {
        let v = vector(l, 1, "vmath.normalize_or_zero", "v");
        // As Operar Vetor3 normalizes: in f64, zero for a vector without a direction.
        push_vector(
            l,
            v.as_dvec3()
                .try_normalize()
                .unwrap_or(DVec3::ZERO)
                .as_vec3(),
        )
    }
}

unsafe extern "C-unwind" fn flat(l: *mut ffi::lua_State) -> c_int {
    unsafe {
        let v = vector(l, 1, "vmath.flat", "v");
        push_vector(l, Vec3::new(v.x, 0., v.z))
    }
}

unsafe extern "C-unwind" fn yaw_of_direction(l: *mut ffi::lua_State) -> c_int {
    unsafe {
        let v = vector(l, 1, "vmath.yaw_of", "direction");
        ffi::lua_pushnumber(l, f64::from(yaw_of(v)));
        1
    }
}

unsafe extern "C-unwind" fn identity(l: *mut ffi::lua_State) -> c_int {
    unsafe {
        push_quat(l, Quat::IDENTITY.to_array());
        1
    }
}

unsafe extern "C-unwind" fn from_euler(l: *mut ffi::lua_State) -> c_int {
    unsafe {
        let x = number(l, 1, "quat.from_euler", "x").to_radians();
        let y = number(l, 2, "quat.from_euler", "y").to_radians();
        let z = number(l, 3, "quat.from_euler", "z").to_radians();
        let q = Quat::from_euler(EulerRot::XYZ, x, y, z);
        finite(l, "quat.from_euler", q.is_finite());
        push_quat(l, q.to_array());
        1
    }
}

unsafe extern "C-unwind" fn from_axis_angle(l: *mut ffi::lua_State) -> c_int {
    unsafe {
        let axis = vector(l, 1, "quat.from_axis_angle", "axis");
        let degrees = number(l, 2, "quat.from_axis_angle", "degrees");
        let Some(axis) = axis.try_normalize() else {
            raise_here(
                l,
                "quat.from_axis_angle: axis precisa de uma direção (um vetor que não é zero)"
                    .into(),
            )
        };
        let q = Quat::from_axis_angle(axis, degrees.to_radians());
        finite(l, "quat.from_axis_angle", q.is_finite());
        push_quat(l, q.to_array());
        1
    }
}

unsafe extern "C-unwind" fn look_at(l: *mut ffi::lua_State) -> c_int {
    unsafe {
        let forward = vector(l, 1, "quat.look_at", "forward");
        let up = if ffi::lua_type(l, 2) <= ffi::LUA_TNIL {
            Vec3::Y
        } else {
            vector(l, 2, "quat.look_at", "up")
        };
        let Some(forward) = forward.try_normalize() else {
            raise_here(
                l,
                "quat.look_at: forward precisa de uma direção (um vetor que não é zero)".into(),
            )
        };
        // An object looks along its -Z: its Z axis is the way back.
        let back = -forward;
        let Some(right) = up.cross(back).try_normalize() else {
            raise_here(
                l,
                "quat.look_at: forward e up não podem ter a mesma direção (nem up ser zero); \
                 passe outro up"
                    .into(),
            )
        };
        let q = Quat::from_mat3(&Mat3::from_cols(right, back.cross(right), back)).normalize();
        push_quat(l, q.to_array());
        1
    }
}

unsafe extern "C-unwind" fn slerp(l: *mut ffi::lua_State) -> c_int {
    unsafe {
        let a = quaternion(l, 1, "quat.slerp", "a");
        let b = quaternion(l, 2, "quat.slerp", "b");
        let t = number(l, 3, "quat.slerp", "t");
        let q = a.slerp(b, t);
        finite(l, "quat.slerp", q.is_finite());
        push_quat(l, q.to_array());
        1
    }
}

unsafe extern "C-unwind" fn multiply(l: *mut ffi::lua_State) -> c_int {
    unsafe {
        let a = quaternion(l, 1, "quat.multiply", "a");
        let b = quaternion(l, 2, "quat.multiply", "b");
        push_quat(l, (a * b).to_array());
        1
    }
}

unsafe extern "C-unwind" fn rotate(l: *mut ffi::lua_State) -> c_int {
    unsafe {
        let q = quaternion(l, 1, "quat.rotate", "q");
        let v = vector(l, 2, "quat.rotate", "v");
        let rotated = q * v;
        finite(l, "quat.rotate", rotated.is_finite());
        push_vector(l, rotated)
    }
}

unsafe extern "C-unwind" fn inverse(l: *mut ffi::lua_State) -> c_int {
    unsafe {
        let q = quaternion(l, 1, "quat.inverse", "q");
        push_quat(l, q.inverse().to_array());
        1
    }
}

unsafe extern "C-unwind" fn to_euler(l: *mut ffi::lua_State) -> c_int {
    unsafe {
        let q = quaternion(l, 1, "quat.to_euler", "q");
        // As obj.rotation reads the angles.
        let (x, y, z) = q.to_euler(EulerRot::XYZ);
        push_vector(l, Vec3::new(x.to_degrees(), y.to_degrees(), z.to_degrees()))
    }
}

/// `q.x`, `q.y`, `q.z` and `q.w`.
unsafe extern "C-unwind" fn quat_index(l: *mut ffi::lua_State) -> c_int {
    unsafe {
        let q = quat_at(l, 1).unwrap_or_default();
        let mut len = 0usize;
        let key = if ffi::lua_type(l, 2) == ffi::LUA_TSTRING {
            let p = ffi::lua_tolstring(l, 2, &mut len);
            std::slice::from_raw_parts(p as *const u8, len)
        } else {
            &[]
        };
        let component = match key {
            b"x" => q[0],
            b"y" => q[1],
            b"z" => q[2],
            b"w" => q[3],
            _ => {
                let key = String::from_utf8_lossy(key).into_owned();
                raise_here(
                    l,
                    format!(
                        "Quat não tem '{key}'; um quatérnio tem x, y, z e w, e o que se faz com \
                         ele está em quat (quat.rotate, quat.to_euler, quat.inverse…)"
                    ),
                )
            }
        };
        ffi::lua_pushnumber(l, f64::from(component));
        1
    }
}

unsafe extern "C-unwind" fn quat_newindex(l: *mut ffi::lua_State) -> c_int {
    unsafe {
        raise_here(
            l,
            "um quatérnio não muda; faça outro com as funções de quat (quat.from_euler, \
             quat.multiply…)"
                .into(),
        )
    }
}

unsafe extern "C-unwind" fn quat_tostring(l: *mut ffi::lua_State) -> c_int {
    unsafe {
        let [x, y, z, w] = quat_at(l, 1).unwrap_or_default();
        let text = format!("quat({x}, {y}, {z}, {w})");
        ffi::lua_pushlstring(l, text.as_ptr().cast(), text.len());
        1
    }
}

unsafe extern "C-unwind" fn quat_eq(l: *mut ffi::lua_State) -> c_int {
    unsafe {
        let same = quat_at(l, 1).is_some() && quat_at(l, 1) == quat_at(l, 2);
        ffi::lua_pushboolean(l, c_int::from(same));
        1
    }
}

/// `a * b`, a quaternion by another, and `q * v`, a vector turned.
unsafe extern "C-unwind" fn quat_mul(l: *mut ffi::lua_State) -> c_int {
    unsafe {
        let Some(a) = quat_at(l, 1) else {
            raise_here(
                l,
                "um quatérnio multiplica outro quatérnio ou um vetor, à direita: q * v".into(),
            )
        };
        let a = Quat::from_array(a);
        if let Some(b) = quat_at(l, 2) {
            push_quat(l, (a * Quat::from_array(b)).to_array());
            return 1;
        }
        if ffi::lua_type(l, 2) == ffi::LUA_TVECTOR {
            let rotated = a * vector(l, 2, "q * v", "v");
            finite(l, "q * v", rotated.is_finite());
            return push_vector(l, rotated);
        }
        raise_here(
            l,
            format!(
                "um quatérnio multiplica outro quatérnio ou um vetor, não {}",
                kind_at(l, 2)
            ),
        )
    }
}

/// A function of the API written in C, named `name` in Luau's own messages.
fn c_function(lua: &Lua, f: ffi::lua_CFunction, name: &'static CStr) -> mlua::Result<Function> {
    // SAFETY: pushes one C function, which `exec_raw` returns.
    unsafe {
        lua.exec_raw((), |l| {
            ffi::lua_pushcclosurek(l, f, name.as_ptr(), 0, None);
        })
    }
}

macro_rules! functions {
    ($($make:ident => $f:ident, $name:literal;)*) => {
        $(
            pub(crate) fn $make(lua: &Lua, _: &Rc<Shared>) -> mlua::Result<Function> {
                c_function(lua, $f, $name)
            }
        )*
    };
}

functions! {
    vmath_lerp => lerp, c"vmath.lerp";
    vmath_distance => distance, c"vmath.distance";
    vmath_normalize_or_zero => normalize_or_zero, c"vmath.normalize_or_zero";
    vmath_flat => flat, c"vmath.flat";
    vmath_yaw_of => yaw_of_direction, c"vmath.yaw_of";
    quat_identity => identity, c"quat.identity";
    quat_from_euler => from_euler, c"quat.from_euler";
    quat_from_axis_angle => from_axis_angle, c"quat.from_axis_angle";
    quat_look_at => look_at, c"quat.look_at";
    quat_slerp => slerp, c"quat.slerp";
    quat_multiply => multiply, c"quat.multiply";
    quat_rotate => rotate, c"quat.rotate";
    quat_inverse => inverse, c"quat.inverse";
    quat_to_euler => to_euler, c"quat.to_euler";
}
