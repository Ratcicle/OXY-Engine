//! Script memory and the oracle of a VM made anew, seen from inside the crate: what only a test
//! double shows (state a script keeps in the VM, which the load checks leave no way to do) and
//! what needs the host's limits.
use super::host::{LIMITS, Limits, Shared};
use super::*;
use crate::document::{Entity, Id, Project, Value};
use crate::runtime::{FIXED_DT, InputFrame, Runtime, RuntimeOptions};
use mlua::Lua;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

const PATH: &str = "scripts/teste.luau";

fn sources(files: &[(&str, &str)]) -> Arc<ScriptSources> {
    Arc::new(ScriptSources::from_texts(files.iter().copied()))
}

/// An object with a number attribute `Contagem` that runs `scripts`.
fn object(name: &str, scripts: &[&str]) -> Entity {
    let mut e = Entity::new(name, None);
    e.attributes.insert("Contagem".into(), Value::Number(0.));
    e.scripts = scripts.iter().copied().map(ScriptRef::new).collect();
    e
}

fn project(objects: Vec<Entity>) -> Project {
    let mut project = Project::new("Oráculo");
    project.scenes[0].entities = objects;
    project
}

fn run(project: &Project, files: &[(&str, &str)]) -> Runtime {
    let options = RuntimeOptions {
        scripts: sources(files),
        ..Default::default()
    };
    Runtime::with_options(project, &project.start_scene, options).unwrap()
}

/// A VM whose `test.count()` keeps a counter in the VM itself: the state a game script must
/// never keep there, and a double of any way past the load checks one might still find.
fn leaky_host(sources: Arc<ScriptSources>) -> Result<ScriptHost, String> {
    ScriptHost::with_extra(sources, &|lua: &Lua, base, _: &Rc<Shared>| {
        let test = lua.create_table()?;
        let count = Rc::new(Cell::new(0.));
        test.raw_set(
            "count",
            lua.create_function(move |_, ()| {
                count.set(count.get() + 1.);
                Ok(count.get())
            })?,
        )?;
        base.raw_set("test", test)
    })
}

fn use_leaky_host(rt: &mut Runtime, files: &[(&str, &str)]) {
    let host = leaky_host(sources(files)).unwrap();
    host.begin_scene(rt.scene().entities.iter().map(|e| e.id.as_str()));
    rt.replace_script_host(host);
}

/// Steps two copies of a game together, the second one's VM made anew every `every` steps,
/// keeping its state: the first step after which their states differ.
fn first_divergence(files: &[(&str, &str)], steps: usize, every: usize) -> Option<usize> {
    let project = project(vec![object("A", &[PATH]), object("B", &[PATH])]);
    let start = || {
        let mut rt = run(&project, files);
        use_leaky_host(&mut rt, files);
        rt
    };
    let (mut kept, mut renewed) = (start(), start());
    assert_eq!(kept.state_hash(), renewed.state_hash());
    for step in 1..=steps {
        kept.advance(FIXED_DT, &InputFrame::default());
        renewed.advance(FIXED_DT, &InputFrame::default());
        if step % every == 0 {
            let logs = renewed.logs_written();
            assert_eq!(renewed.recreate_script_host_with(&leaky_host), Ok(()));
            assert_eq!(renewed.logs_written(), logs);
        }
        if kept.state_hash() != renewed.state_hash() {
            return Some(step);
        }
    }
    None
}

#[test]
fn the_oracle_of_a_vm_made_anew_catches_state_hidden_in_the_vm_and_passes_memory() {
    let hidden = "local M = {}\nfunction M.on_step(self)\n    self.attr.Contagem = test.count()\n\
                  end\nreturn M\n";
    // The counter starts over in the new VM: the step after the first reset gives it away.
    assert_eq!(first_divergence(&[(PATH, hidden)], 60, 10), Some(11));
    // The same count kept in memory, with a list, a timer and a message, goes on in any VM.
    let kept = r#"local M = {}
M.state = { n = 0, last = {}, pings = 0 }
function M.on_start(self) self:every(0.25, M.ping) end
function M.on_step(self)
    self.state.n += 1
    table.insert(self.state.last, self.state.n)
    if #self.state.last > 5 then table.remove(self.state.last, 1) end
    self.attr.Contagem = self.state.n * 100 + #self.state.last + self.state.pings
end
function M.ping(self) self:send("ping", { at = self.state.n }) end
function M.on_message(self, name, value) self.state.pings += value.at end
return M
"#;
    assert_eq!(first_divergence(&[(PATH, kept)], 90, 7), None);
}

#[test]
fn a_memory_error_while_building_a_table_of_memory_stops_the_script_and_keeps_the_memory() {
    let source = "local M = {}\nM.state = { items = {} }\nfunction M.fill(self)\n    local t = {}\n\
                  \x20   for i = 1, 1000 do t[i] = string.rep('x', 40) .. i end\n    \
                  self.state.items = t\nend\nfunction M.read(self)\n    self.attr.Contagem = \
                  #self.state.items\nend\nreturn M\n";
    let (a, b) = (object("A", &[PATH]), object("B", &[PATH]));
    let ids: [Id; 2] = [a.id.clone(), b.id.clone()];
    let mut rt = run(&project(vec![a, b]), &[(PATH, source)]);
    assert_eq!(rt.call_script(&ids[0], PATH, "fill", &[]), ScriptCall::Done);
    let kept = rt.script_memory_text(&ids[0], PATH).unwrap();
    // The table of 1000 texts needs ~70 KB of the VM once the garbage of `fill` is gone.
    let host = rt.script_host().unwrap();
    host.lua().gc_collect().unwrap();
    host.lua().gc_collect().unwrap();
    let used = host.lua().used_memory();
    host.set_limits(Limits {
        memory: used + 4 * 1024,
        ..LIMITS
    });
    let ScriptCall::Failed(e) = rt.call_script(&ids[0], PATH, "read", &[]) else {
        panic!("esperava um erro");
    };
    assert_eq!(e.kind, ScriptErrorKind::Limit);
    assert!(
        e.message
            .starts_with("os scripts passaram do limite de memória"),
        "{e}"
    );
    assert_eq!(
        rt.disabled_scripts().collect::<Vec<_>>(),
        [(ids[0].as_str(), PATH)]
    );
    assert_eq!(rt.script_memory_text(&ids[0], PATH).unwrap(), kept);
    // The VM goes on, and the next call's tables start from nothing of this one.
    rt.script_host().unwrap().set_limits(LIMITS);
    assert_eq!(rt.call_script(&ids[1], PATH, "read", &[]), ScriptCall::Done);
    assert_eq!(
        rt.scene().entity(&ids[1]).unwrap().attributes["Contagem"],
        Value::Number(0.)
    );
}
