//! Timestamped input for deterministic replays/tests. Times are simulation seconds,
//! not wall-clock or presentation timestamps. Equal timestamps retain enqueue order.
use crate::runtime::InputFrame;
use std::collections::{BTreeSet, VecDeque};
#[derive(Clone, Debug)]
pub enum InputChange {
    Press(String),
    Release(String),
    Movement([f32; 2]),
    Look([f32; 2]),
    Wheel(f32),
}
#[derive(Clone, Debug)]
pub struct TimedInput {
    pub at: f64,
    pub change: InputChange,
}
#[derive(Default)]
pub(crate) struct InputTimeline {
    queue: VecDeque<TimedInput>,
    held: BTreeSet<String>,
    movement: [f32; 2],
}
impl InputTimeline {
    pub fn push(&mut self, event: TimedInput, now: f64) -> Result<(), String> {
        let finite = match event.change {
            InputChange::Movement(v) | InputChange::Look(v) => v.iter().all(|v| v.is_finite()),
            InputChange::Wheel(v) => v.is_finite(),
            _ => true,
        };
        if !finite
            || !event.at.is_finite()
            || event.at < now
            || self.queue.back().is_some_and(|last| last.at > event.at)
        {
            return Err("Entrada temporal deve ser finita, ordenada e não pode reescrever passos já simulados.".into());
        }
        if self.queue.len() >= 16384 {
            return Err("Limite de 16384 eventos de entrada pendentes excedido.".into());
        }
        self.queue.push_back(event);
        Ok(())
    }
    pub fn sample(&mut self, time: f64, frame: &mut InputFrame) {
        while self.queue.front().is_some_and(|e| e.at <= time) {
            match self.queue.pop_front().unwrap().change {
                InputChange::Press(action) => {
                    if self.held.insert(action.clone()) {
                        frame.pressed.insert(action);
                    }
                }
                InputChange::Release(action) => {
                    if self.held.remove(&action) {
                        frame.released.insert(action);
                    }
                }
                InputChange::Movement(v) => self.movement = v,
                InputChange::Look(v) => {
                    frame.look[0] += v[0];
                    frame.look[1] += v[1];
                }
                InputChange::Wheel(v) => frame.wheel += v,
            }
        }
        frame.held.extend(self.held.iter().cloned());
        frame.movement[0] += self.movement[0];
        frame.movement[1] += self.movement[1];
    }
    pub fn unconsumed_look(&self, time: f64) -> glam::Vec2 {
        self.queue
            .iter()
            .take_while(|e| e.at <= time)
            .filter_map(|e| match e.change {
                InputChange::Look(v) => Some(glam::Vec2::from(v)),
                _ => None,
            })
            .sum()
    }
}
