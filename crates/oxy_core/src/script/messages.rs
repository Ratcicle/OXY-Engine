//! Messages between objects (design §4.6): `obj:send(name, value?)` queues, as engine data,
//! `on_message(self, name, value, from)` for the scripts of `obj` that hear it, with the value
//! copied as memory copies it. The next logic phase delivers them in the order they were sent,
//! after the events and timers due, so two objects answering each other trade one message per
//! phase. In a step a script sends at most [`MAX_MESSAGES`] from one object, past which it stops
//! there, as past any limit, and the scene's scripts send at most [`MAX_SCENE_MESSAGES`], past
//! which a send fails without stopping anyone: a script that floods stops, the others do not. The
//! graph sends and hears none (it is frozen).
use super::handles::{self, MARK_OBJECT};
use super::host::{Limit, Shared, api, locate};
use super::memory::{self, ScriptValue};
use crate::document::Id;
use mlua::{Function, Lua, Value as LuaValue};
use std::rc::Rc;

/// A message on its way: the next logic phase delivers it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Message {
    pub from: Id,
    pub to: Id,
    pub name: String,
    pub value: Option<ScriptValue>,
}

/// Messages a script may send from one object in one step.
pub(crate) const MAX_MESSAGES: usize = 1024;
/// Messages the scripts of a scene may send in one step.
pub(crate) const MAX_SCENE_MESSAGES: usize = 4 * MAX_MESSAGES;
/// The longest name of a message, in bytes.
const MAX_NAME: usize = 256;
/// What the value of a message may hold, as memory counts it.
const MAX_VALUE: usize = 4096;

/// Why a message was not sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Unsent {
    /// The script sent its messages of the step from this object: it stops there.
    Script,
    /// The scene's scripts sent theirs: only this call fails.
    Scene,
}

/// `obj:send(name, value?)`, a method of objects.
pub(crate) fn send(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<Function> {
    let s = shared.clone();
    lua.create_function(
        move |lua, (to, name, value): (LuaValue, LuaValue, LuaValue)| {
            // The value first, outside the panic barrier: the C API reads its tables.
            let value = memory::from_lua(lua, &s, &value, MAX_VALUE)
                .map_err(|refusal| refusal.at("o valor da mensagem"));
            let at = locate(lua);
            api(lua, &s, "send", |runtime| {
                let to = match &to {
                    LuaValue::LightUserData(handle) => match handles::decode(handle.0) {
                        (MARK_OBJECT, key) => Some(key),
                        _ => None,
                    },
                    _ => None,
                }
                .ok_or("send é um método de objetos: alvo:send(\"nome\", valor)")?;
                let to = s.id(to).ok_or_else(|| s.gone(to))?;
                let LuaValue::String(name) = &name else {
                    return Err("send precisa do nome da mensagem (um texto)".into());
                };
                let name = name
                    .to_str()
                    .map_err(|_| "o nome da mensagem não é UTF-8")?
                    .to_owned();
                if name.is_empty() || name.len() > MAX_NAME {
                    return Err(format!("o nome da mensagem tem de 1 a {MAX_NAME} bytes"));
                }
                let value = value?;
                let from = s.caller_key().ok_or("send fora de uma chamada de script")?;
                let from = s.id(from).ok_or_else(|| s.gone(from))?;
                let script = s
                    .module()
                    .ok_or("send fora de uma chamada de script")?
                    .path
                    .clone();
                if runtime.entity(&to).is_none() {
                    return Err("o objeto de destino não existe mais".into());
                }
                let message = Message {
                    from,
                    to,
                    name,
                    value,
                };
                match runtime.send_message(message, &script, at.clone()) {
                    Ok(()) => Ok(()),
                    Err(Unsent::Script) => {
                        s.overran(Limit::Messages, at);
                        Err(Limit::Messages.message(&s.limits.get()))
                    }
                    Err(Unsent::Scene) => Err(format!(
                        "os scripts da cena já mandaram {MAX_SCENE_MESSAGES} mensagens neste \
                             passo; esta não foi enviada"
                    )),
                }
            })
        },
    )
}
