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
#[derive(Debug, Default)]
pub(crate) struct InputTimeline {
    queue: VecDeque<TimedInput>,
    held: BTreeSet<String>,
    movement: [f32; 2],
}
impl InputTimeline {
    pub fn push(&mut self, event: TimedInput, now: f64) -> Result<(), String> {
        self.push_all(vec![event], now)
    }
    /// Queues `events` in order: all of them or, when one is refused, none.
    pub fn push_all(&mut self, events: Vec<TimedInput>, now: f64) -> Result<(), String> {
        let mut last = self.queue.back().map(|e| e.at);
        for event in &events {
            let finite = match event.change {
                InputChange::Movement(v) | InputChange::Look(v) => v.iter().all(|v| v.is_finite()),
                InputChange::Wheel(v) => v.is_finite(),
                _ => true,
            };
            if !finite
                || !event.at.is_finite()
                || event.at < now
                || last.is_some_and(|last| last > event.at)
            {
                return Err("Entrada temporal deve ser finita, ordenada e não pode reescrever passos já simulados.".into());
            }
            last = Some(event.at);
        }
        if self.queue.len() + events.len() > 16384 {
            return Err("Limite de 16384 eventos de entrada pendentes excedido.".into());
        }
        self.queue.extend(events);
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
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty() && self.held.is_empty() && self.movement == [0.; 2]
    }
    /// When the last input still waiting happens.
    pub fn last_at(&self) -> Option<f64> {
        self.queue.back().map(|e| e.at)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_batch_is_queued_whole_or_not_at_all() {
        let input = |at: f64, change: InputChange| TimedInput { at, change };
        let mut timeline = InputTimeline::default();
        timeline
            .push(input(1., InputChange::Press("pular".into())), 0.)
            .unwrap();
        for refused in [
            // Out of order, or with a value no step could use: the earlier ones stay out too.
            vec![
                input(2., InputChange::Release("pular".into())),
                input(1.5, InputChange::Movement([0., 1.])),
            ],
            vec![
                input(2., InputChange::Release("pular".into())),
                input(3., InputChange::Look([f32::INFINITY, 0.])),
            ],
            vec![input(0.5, InputChange::Release("pular".into()))],
        ] {
            assert!(timeline.push_all(refused, 0.).is_err());
            assert_eq!(timeline.last_at(), Some(1.));
        }
        let many: Vec<_> = (0..16384)
            .map(|i| input(2. + f64::from(i), InputChange::Wheel(1.)))
            .collect();
        let error = timeline.push_all(many, 0.).unwrap_err();
        assert!(error.contains("16384"), "{error}");
        assert_eq!(timeline.last_at(), Some(1.));
        timeline
            .push_all(
                vec![
                    input(2., InputChange::Release("pular".into())),
                    input(2., InputChange::Movement([0., 1.])),
                ],
                0.,
            )
            .unwrap();
        assert_eq!(timeline.last_at(), Some(2.));
    }
}
