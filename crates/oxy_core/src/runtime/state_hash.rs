//! A fingerprint of the running game, to tell whether two runs stayed the same: a hash of
//! everything that shapes the steps to come, and a digest that names the part where two runs
//! differ. Left out are the host's queues (logs, traces, sounds, save requests, scene starts,
//! the recording), which the host takes whenever it likes, and caches rebuilt from the scene
//! (lookups, prepared graphs, physics worlds). Maps and sets are read sorted, never in their
//! own order, and floats exactly: JSON and Debug text both write the shortest form that reads
//! back the same number. The hash is the portable one of `random`: a build gives the same one
//! in every process.
use super::*;
use crate::random::Hasher;
use serde::Serialize;
use std::fmt::{self, Write as _};

/// The state in parts, each with its hash. The run's own come first (scene, time, input,
/// tasks, areas, characters, random, variables.<name> for each game variable, save, audio,
/// camera, script_errors), then each object's in scene order: transform, visible,
/// attributes.<name>, document (its whole document form), character, platform, body (2D
/// movement), animation, behavior (stopped by an error or by logic), scripts (those a limit
/// stopped) and script:<path> (the memory of each script, by path). Parts a project without
/// scripts never has are left out, not written empty, and so is what scripts add to a part
/// (their tasks, timer numbers, messages, draws and copies).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StateDigest {
    pub parts: Vec<StatePart>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatePart {
    /// The object the part belongs to; none for the run's own.
    pub object: Option<Id>,
    pub name: String,
    pub hash: u64,
}
/// Where two digests first differ.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateDifference {
    pub object: Option<Id>,
    /// The part's name; "exists" for an object only one of them has, "order" for objects both
    /// have in another scene order.
    pub part: String,
}

impl StateDigest {
    /// Every part in one number: equal digests give equal hashes.
    pub fn hash(&self) -> u64 {
        let mut hasher = Hasher::default();
        for part in &self.parts {
            let object = part.object.as_deref().unwrap_or_default();
            let hash = part.hash.to_le_bytes();
            let whole = crate::random::hash(&[object.as_bytes(), part.name.as_bytes(), &hash]);
            hasher.write(&whole.to_le_bytes());
        }
        hasher.finish()
    }

    /// The first part, in digest order, where `self` and `other` differ.
    pub fn first_difference(&self, other: &Self) -> Option<StateDifference> {
        // Each object's parts are together, the run's own first.
        fn groups(digest: &StateDigest) -> Vec<&[StatePart]> {
            digest.parts.chunk_by(|a, b| a.object == b.object).collect()
        }
        let (ours, theirs) = (groups(self), groups(other));
        let found = |object: &Option<Id>, part: &str| {
            Some(StateDifference {
                object: object.clone(),
                part: part.into(),
            })
        };
        for i in 0..ours.len().max(theirs.len()) {
            let (a, b) = (ours.get(i), theirs.get(i));
            if let (Some(a), Some(b)) = (a, b)
                && a[0].object == b[0].object
            {
                if let Some(part) = part_difference(a, b) {
                    return found(&a[0].object, part);
                }
                continue;
            }
            // An object only one of them has, or objects both have in another order.
            let lacks = |groups: &[&[StatePart]], object: &Option<Id>| {
                !groups.iter().any(|group| group[0].object == *object)
            };
            let only = a
                .filter(|a| lacks(&theirs, &a[0].object))
                .or_else(|| b.filter(|b| lacks(&ours, &b[0].object)));
            return match (only, a.or(b)) {
                (Some(group), _) => found(&group[0].object, "exists"),
                (None, Some(group)) => found(&group[0].object, "order"),
                (None, None) => None,
            };
        }
        None
    }

    fn add(&mut self, object: Option<&Id>, name: impl Into<String>, fill: impl FnOnce(&mut Sink)) {
        self.add_some(object, name, |sink| {
            fill(sink);
            Some(Ok(()))
        });
    }

    /// `add`, for a part `fill` may find the object lacks (it answers None then).
    fn add_some(
        &mut self,
        object: Option<&Id>,
        name: impl Into<String>,
        fill: impl FnOnce(&mut Sink) -> Option<fmt::Result>,
    ) {
        let mut sink = Sink::default();
        if fill(&mut sink).is_some() {
            self.parts.push(StatePart {
                object: object.cloned(),
                name: name.into(),
                hash: sink.0.finish(),
            });
        }
    }
}

/// The name of the first part where two parts of one object differ: the one with other
/// contents, or the one only one of them has.
fn part_difference<'a>(ours: &'a [StatePart], theirs: &'a [StatePart]) -> Option<&'a str> {
    let i = (0..ours.len().max(theirs.len())).find(|&i| ours.get(i) != theirs.get(i))?;
    match (ours.get(i), theirs.get(i)) {
        (Some(a), Some(b)) if a.name != b.name && theirs.iter().any(|p| p.name == a.name) => {
            Some(b.name.as_str())
        }
        (Some(a), _) => Some(a.name.as_str()),
        (None, b) => b.map(|b| b.name.as_str()),
    }
}

/// `items` in their sorted order, for reading a map or a set the same way in every run.
pub(super) fn sorted<T: Ord>(items: impl IntoIterator<Item = T>) -> Vec<T> {
    let mut items: Vec<_> = items.into_iter().collect();
    items.sort();
    items
}

/// Feeds the portable hash: JSON through `io::Write`, Debug text through `fmt::Write`. Each
/// value ends in ';', so neighbours never run together.
#[derive(Default)]
struct Sink(Hasher);
impl std::io::Write for Sink {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.write(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl fmt::Write for Sink {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        self.0.write(text.as_bytes());
        Ok(())
    }
}
impl Sink {
    /// The document form, as the project file holds it.
    fn json(&mut self, value: &impl Serialize) -> &mut Self {
        // Writing to the hash never fails, and the document has no map JSON cannot key.
        let _ = serde_json::to_writer(&mut *self, value);
        self.0.write(b";");
        self
    }
    fn debug(&mut self, value: &impl fmt::Debug) -> &mut Self {
        let _ = write!(self, "{value:?};");
        self
    }
}

impl Runtime {
    /// A hash of the game state: runs that reached the same state have the same one, in this
    /// process or in another. The [`state_digest`](Self::state_digest) in one number.
    pub fn state_hash(&self) -> u64 {
        self.state_digest().hash()
    }

    /// The game state in parts (see [`StateDigest`]), to find where two runs differ.
    pub fn state_digest(&self) -> StateDigest {
        // Every field, so that a new one is read here or left out on purpose.
        let Self {
            // The running scene is read below; the other scenes and the source stay as they
            // are until a scene change copies them again (the source names the variables).
            project: _,
            source: _,
            scene_id,
            // Rebuilt from the scene when needed.
            index: _,
            graphs: _,
            input_modes: _,
            event_operations: _,
            physics_dirty: _,
            // The host's, taken whenever it likes.
            logs: _,
            traces: _,
            sounds: _,
            save_requests: _,
            scene_starts: _,
            recording: _,
            // Only within a step.
            sounds_before_change: _,
            audio_before_change: _,
            audio_step_start: _,
            query_work: _,
            paused,
            game_paused,
            cursor_shown,
            quit_requested,
            stopped,
            logs_written,
            traces_written,
            time,
            accumulator,
            pending_pressed,
            pending_released,
            pending_look,
            pending_wheel,
            input_timeline,
            step_input,
            characters,
            cameras,
            ready,
            waiting,
            bodies,
            animations,
            disabled_behaviors,
            overlap_pairs,
            area_activations,
            damage_hits,
            area_hits,
            serial,
            run,
            // The project's scripts, as the project itself, and the VM with what each module
            // hears, a cache of modules.
            script_sources: _,
            scripts: _,
            script_callbacks: _,
            // Counted from the timer tasks, which are read below.
            timer_room: _,
            disabled_scripts,
            script_memory,
            messages,
            // Only within a step.
            messages_sent: _,
            messages_by_script: _,
            // Measured, never state.
            script_costs: _,
        } = self;
        let RunState {
            seed,
            rolls,
            spawns,
            script_rolls,
            script_spawns,
            timers,
            variables,
            save,
            paused_for,
            audio,
            failures_logged,
            script_errors,
        } = run;
        let mut digest = StateDigest::default();
        digest.add(None, "scene", |s| {
            s.debug(scene_id)
                .debug(&(paused, stopped))
                .debug(&(game_paused, quit_requested))
                .debug(cursor_shown);
        });
        digest.add(None, "time", |s| {
            s.debug(time).debug(accumulator).debug(paused_for);
        });
        digest.add(None, "input", |s| {
            s.debug(pending_pressed)
                .debug(pending_released)
                .debug(pending_look)
                .debug(pending_wheel)
                .debug(input_timeline)
                .debug(step_input);
        });
        digest.add(None, "tasks", |s| {
            // Lines and nodes run so far, which the host's taking leaves as they are, and the
            // failures already told, which no later line repeats.
            s.debug(&(logs_written, traces_written, ready.len()))
                .debug(failures_logged);
            for task in ready.iter().chain(waiting) {
                let Task {
                    due,
                    owner,
                    target,
                    context,
                } = task;
                let Context {
                    owner: runs_as,
                    other,
                    activation,
                    outputs,
                    hits,
                    trajectory,
                } = context;
                let hits = hits
                    .as_ref()
                    .map(|hits| sorted(hits.lock().unwrap().iter().cloned()));
                // A node's task reads as it did before tasks had targets; a script's adds its
                // own: the script, the function, what it passes and its timer.
                match target {
                    TaskTarget::Node(node) => s.debug(&(
                        due, owner, node, runs_as, other, activation, outputs, hits, trajectory,
                    )),
                    TaskTarget::Script(script) => s.debug(&(
                        due, owner, script, runs_as, other, activation, outputs, hits, trajectory,
                    )),
                };
            }
            // The last number a script's timer got, which names the next one.
            if *timers > 0 {
                s.debug(timers);
            }
            // The messages on their way, which become tasks in the next logic phase.
            if !messages.is_empty() {
                s.debug(messages);
            }
        });
        digest.add(None, "areas", |s| {
            let hit = |hits: &HitLease| sorted(hits.lock().unwrap().iter().cloned());
            s.debug(serial)
                .debug(&sorted(overlap_pairs))
                .debug(&sorted(area_activations))
                .debug(&sorted(damage_hits.iter().map(|(key, hits)| {
                    (key, hits.upgrade().map(|hits| hit(&hits)))
                })))
                .debug(&sorted(area_hits.iter().map(|(id, hits)| (id, hit(hits)))));
        });
        digest.add(None, "characters", |s| {
            let _ = characters.write_shared(s);
        });
        digest.add(None, "random", |s| {
            s.debug(seed).debug(rolls).debug(spawns);
            if !script_rolls.is_empty() || !script_spawns.is_empty() {
                s.debug(script_rolls).debug(script_spawns);
            }
        });
        // One part per variable, in the order the project declares them, with every key's value.
        for declared in &self.source.variables {
            let id = &declared.id;
            digest.add(None, format!("variables.{}", declared.name), |s| {
                for entry in variables
                    .range((id.clone(), String::new())..)
                    .take_while(|((owner, _), _)| owner == id)
                {
                    s.debug(&entry);
                }
            });
        }
        // What a new run would start from, and whether Ler progresso salvo finds any.
        digest.add(None, "save", |s| {
            s.debug(save);
        });
        // The music, the effect loops and the volumes the hosts follow.
        digest.add(None, "audio", |s| {
            s.debug(audio);
        });
        digest.add(None, "camera", |s| {
            let _ = cameras.write_digest(self.scene(), s);
        });
        // The script errors told so far, which no later line repeats; absent without any.
        digest.add_some(None, "script_errors", |s| {
            (!script_errors.is_empty()).then(|| write!(s, "{script_errors:?}"))
        });
        for entity in &self.scene().entities {
            let id = Some(&entity.id);
            // The parts the logic changes most come before the whole, which then names
            // whatever else changed.
            digest.add(id, "transform", |s| {
                s.json(&entity.transform);
            });
            digest.add(id, "visible", |s| {
                s.json(&entity.visible);
            });
            for (name, value) in &entity.attributes {
                digest.add(id, format!("attributes.{name}"), |s| {
                    s.json(value);
                });
            }
            digest.add(id, "document", |s| {
                s.json(entity);
            });
            digest.add_some(id, "character", |s| {
                characters.write_character(&entity.id, s)
            });
            digest.add_some(id, "platform", |s| characters.write_platform(&entity.id, s));
            digest.add_some(id, "body", |s| {
                bodies.get(&entity.id).map(|body| write!(s, "{body:?}"))
            });
            digest.add_some(id, "animation", |s| {
                animations
                    .get(&entity.id)
                    .map(|player| write!(s, "{player:?}"))
            });
            digest.add_some(id, "behavior", |_| {
                disabled_behaviors.contains(&entity.id).then_some(Ok(()))
            });
            digest.add_some(id, "scripts", |s| {
                if disabled_scripts.is_empty() {
                    return None;
                }
                let stopped: Vec<&String> = disabled_scripts
                    .range((entity.id.clone(), String::new())..)
                    .take_while(|(owner, _)| *owner == entity.id)
                    .map(|(_, path)| path)
                    .collect();
                (!stopped.is_empty()).then(|| write!(s, "{stopped:?}"))
            });
            for (path, memory) in script_memory.get(&entity.id).into_iter().flatten() {
                digest.add(id, format!("script:{path}"), |s| {
                    s.debug(&memory.fields());
                });
            }
        }
        digest
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part(object: Option<&str>, name: &str, hash: u64) -> StatePart {
        StatePart {
            object: object.map(Into::into),
            name: name.into(),
            hash,
        }
    }
    fn digest(parts: &[StatePart]) -> StateDigest {
        StateDigest {
            parts: parts.to_vec(),
        }
    }
    fn difference(a: &StateDigest, b: &StateDigest) -> Option<(Option<String>, String)> {
        a.first_difference(b).map(|d| (d.object, d.part))
    }

    #[test]
    fn differences_name_the_first_part_and_objects_one_run_lacks() {
        let run = part(None, "tasks", 1);
        let a = [
            part(Some("a"), "transform", 2),
            part(Some("a"), "document", 3),
        ];
        let b = [
            part(Some("b"), "transform", 4),
            part(Some("b"), "document", 5),
        ];
        let base = digest(&[
            run.clone(),
            a[0].clone(),
            a[1].clone(),
            b[0].clone(),
            b[1].clone(),
        ]);
        assert_eq!(base.first_difference(&base.clone()), None);
        assert_eq!(base.hash(), base.clone().hash());
        let object = |id: &str| Some(id.to_owned());

        let mut moved = base.clone();
        moved.parts[3].hash = 9;
        moved.parts[4].hash = 9;
        assert_eq!(
            difference(&base, &moved),
            Some((object("b"), "transform".into()))
        );
        assert_ne!(base.hash(), moved.hash());
        let mut busy = base.clone();
        busy.parts[0].hash = 9;
        busy.parts[1].hash = 9;
        assert_eq!(difference(&base, &busy), Some((None, "tasks".into())));

        // A part only one of them has, either way round.
        let mut stopped = base.clone();
        stopped.parts.insert(3, part(Some("a"), "behavior", 0));
        assert_eq!(
            difference(&base, &stopped),
            Some((object("a"), "behavior".into()))
        );
        assert_eq!(
            difference(&stopped, &base),
            Some((object("a"), "behavior".into()))
        );

        // An object only one of them has, wherever it is, and objects in another order.
        let without_a = digest(&[run.clone(), b[0].clone(), b[1].clone()]);
        assert_eq!(
            difference(&base, &without_a),
            Some((object("a"), "exists".into()))
        );
        assert_eq!(
            difference(&without_a, &base),
            Some((object("a"), "exists".into()))
        );
        let without_b = digest(&[run.clone(), a[0].clone(), a[1].clone()]);
        assert_eq!(
            difference(&without_b, &base),
            Some((object("b"), "exists".into()))
        );
        let swapped = digest(&[run, b[0].clone(), b[1].clone(), a[0].clone(), a[1].clone()]);
        assert_eq!(
            difference(&base, &swapped),
            Some((object("a"), "order".into()))
        );
    }
}
