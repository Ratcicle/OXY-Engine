//! Shared keyframe sampling for editor previews and the standalone player.
use crate::document::{Id, Scene, Transform, new_id};
use glam::{EulerRot, Vec3};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Interpolation {
    Linear,
    Hold,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Keyframe {
    pub time: f32,
    pub transform: Transform,
    pub interpolation: Interpolation,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub target: Id,
    pub keyframes: Vec<Keyframe>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AnimationEvent {
    pub time: f32,
    pub name: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    pub id: Id,
    pub name: String,
    pub duration: f32,
    pub looping: bool,
    pub tracks: Vec<Track>,
    pub events: Vec<AnimationEvent>,
}
impl Clip {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: new_id(),
            name: name.into(),
            duration: 1.,
            looping: false,
            tracks: Vec::new(),
            events: Vec::new(),
        }
    }
    pub fn validate(&self, ids: &HashSet<&str>) -> Result<(), String> {
        if self.id.is_empty()
            || self.name.is_empty()
            || !self.duration.is_finite()
            || self.duration <= 0.
        {
            return Err("Clip deve ter ID, nome e duração positiva".into());
        }
        let mut targets = HashSet::new();
        for track in &self.tracks {
            if !ids.contains(track.target.as_str()) {
                return Err(format!(
                    "Peça ausente no clip {}: {}",
                    self.name, track.target
                ));
            }
            if !targets.insert(&track.target) {
                return Err(format!("Trilhas repetidas no clip {}", self.name));
            }
            let mut previous = -1.;
            for key in &track.keyframes {
                if !key.time.is_finite()
                    || key.time < 0.
                    || key.time > self.duration
                    || key.time <= previous
                    || !key.transform.finite()
                {
                    return Err(format!(
                        "Quadros-chave inválidos ou fora de ordem na animação {}",
                        self.name
                    ));
                }
                previous = key.time;
            }
        }
        for event in &self.events {
            if !event.time.is_finite()
                || event.time < 0.
                || event.time > self.duration
                || event.name.is_empty()
            {
                return Err(format!("Marcador inválido no clip {}", self.name));
            }
        }
        Ok(())
    }
    pub fn insert_key(&mut self, target: &str, key: Keyframe) {
        let index = if let Some(i) = self.tracks.iter().position(|t| t.target == target) {
            i
        } else {
            self.tracks.push(Track {
                target: target.into(),
                keyframes: Vec::new(),
            });
            self.tracks.len() - 1
        };
        let track = &mut self.tracks[index];
        if let Some(existing) = track
            .keyframes
            .iter_mut()
            .find(|k| (k.time - key.time).abs() < 0.00001)
        {
            *existing = key
        } else {
            track.keyframes.push(key)
        }
        track.keyframes.sort_by(|a, b| a.time.total_cmp(&b.time));
    }
}
pub fn interpolate(a: &Transform, b: &Transform, t: f32) -> Transform {
    let t = t.clamp(0., 1.);
    let rotation = a.quaternion().slerp(b.quaternion(), t);
    let (x, y, z) = rotation.to_euler(EulerRot::XYZ);
    Transform {
        position: Vec3::from(a.position)
            .lerp(Vec3::from(b.position), t)
            .to_array(),
        rotation: [x, y, z],
        scale: Vec3::from(a.scale).lerp(Vec3::from(b.scale), t).to_array(),
        pivot: a.pivot,
    }
}
pub fn sample_track(track: &Track, time: f32) -> Option<Transform> {
    let first = track.keyframes.first()?;
    if time <= first.time {
        return Some(first.transform.clone());
    }
    for keys in track.keyframes.windows(2) {
        let a = &keys[0];
        let b = &keys[1];
        if time < b.time {
            return Some(if a.interpolation == Interpolation::Hold {
                a.transform.clone()
            } else {
                interpolate(
                    &a.transform,
                    &b.transform,
                    (time - a.time) / (b.time - a.time),
                )
            });
        }
    }
    track.keyframes.last().map(|k| k.transform.clone())
}
/// Samples into a runtime/preview scene. Callers retain the immutable document pose.
pub fn sample_clip(scene: &mut Scene, clip: &Clip, time: f32) {
    let time = if clip.looping && clip.duration > 0. {
        time.rem_euclid(clip.duration)
    } else {
        time.clamp(0., clip.duration.max(0.))
    };
    let Ok(index) = crate::scene_view::SceneIndex::new(scene) else {
        return;
    };
    for track in &clip.tracks {
        if let (Some(entity), Some(mut pose)) = (
            index
                .position(&track.target)
                .and_then(|i| scene.entities.get_mut(i)),
            sample_track(track, time),
        ) {
            pose.pivot = entity.transform.pivot;
            entity.transform = pose;
        }
    }
}
/// Crosses the half-open interval (from,to]. Times are absolute, not wrapped.
/// Start with `from = -f64::EPSILON` to include markers at time zero.
pub fn advance_events(clip: &Clip, from: f64, to: f64) -> Vec<String> {
    if !from.is_finite() || !to.is_finite() || to <= from || clip.duration <= 0. {
        return Vec::new();
    }
    let duration = f64::from(clip.duration);
    let mut events = Vec::new();
    for event in &clip.events {
        let marker = f64::from(event.time);
        if clip.looping {
            let first = (((from - marker) / duration).floor() as i64 + 1).max(0);
            let last = ((to - marker) / duration).floor() as i64;
            for cycle in first..=last.min(first.saturating_add(4095)) {
                events.push((marker + cycle as f64 * duration, event.name.clone()));
            }
        } else if marker > from && marker <= to {
            events.push((marker, event.name.clone()));
        }
    }
    events.sort_by(|a, b| a.0.total_cmp(&b.0));
    events
        .into_iter()
        .take(4096)
        .map(|(_, name)| name)
        .collect()
}
#[derive(Clone, Debug)]
pub struct AnimationPlayer {
    pub clip_id: Id,
    pub time: f64,
    pub playing: bool,
    started: bool,
}
impl AnimationPlayer {
    pub fn new(clip_id: Id) -> Self {
        Self {
            clip_id,
            time: 0.,
            playing: true,
            started: false,
        }
    }
    pub fn restart(&mut self) {
        self.time = 0.;
        self.playing = true;
        self.started = false;
    }
    pub fn advance(&mut self, clip: &Clip, dt: f32) -> Vec<String> {
        if !self.playing || dt <= 0. || !dt.is_finite() {
            return Vec::new();
        }
        let from = if self.started {
            self.time
        } else {
            -f64::EPSILON
        };
        self.started = true;
        self.time += f64::from(dt);
        if !clip.looping {
            self.time = self.time.min(f64::from(clip.duration));
            if self.time >= f64::from(clip.duration) {
                self.playing = false
            }
        }
        advance_events(clip, from, self.time)
    }
    pub fn sample(&self, scene: &mut Scene, clip: &Clip) {
        sample_clip(scene, clip, self.time as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{Entity, SceneKind};
    #[test]
    fn rotation_uses_shortest_quaternion_arc() {
        let mut a = Transform::default();
        a.rotation[2] = 170_f32.to_radians();
        let mut b = a.clone();
        b.rotation[2] = -170_f32.to_radians();
        let mid = interpolate(&a, &b, 0.5);
        let up = mid.quaternion() * Vec3::X;
        assert!(up.x < -0.99);
    }
    #[test]
    fn hold_and_linear_sampling() {
        let a = Transform::default();
        let mut b = a.clone();
        b.position[0] = 10.;
        let mut t = Track {
            target: "x".into(),
            keyframes: vec![
                Keyframe {
                    time: 0.,
                    transform: a,
                    interpolation: Interpolation::Linear,
                },
                Keyframe {
                    time: 1.,
                    transform: b,
                    interpolation: Interpolation::Linear,
                },
            ],
        };
        assert_eq!(sample_track(&t, 0.5).unwrap().position[0], 5.);
        t.keyframes[0].interpolation = Interpolation::Hold;
        assert_eq!(sample_track(&t, 0.5).unwrap().position[0], 0.);
        assert_eq!(sample_track(&t, 1.).unwrap().position[0], 10.);
    }
    #[test]
    fn markers_cross_once_including_loops() {
        let mut c = Clip::new("hit");
        c.looping = true;
        c.events.push(AnimationEvent {
            time: 0.25,
            name: "Impacto".into(),
        });
        assert_eq!(advance_events(&c, 0., 0.3), vec!["Impacto"]);
        assert!(advance_events(&c, 0.3, 0.31).is_empty());
        assert_eq!(advance_events(&c, 0.3, 2.3).len(), 2);
        assert_eq!(advance_events(&c, 0.25, 1.25).len(), 1);
    }
    #[test]
    fn zero_marker_and_pause() {
        let mut c = Clip::new("zero");
        c.looping = true;
        c.events.push(AnimationEvent {
            time: 0.,
            name: "Passo".into(),
        });
        let mut p = AnimationPlayer::new(c.id.clone());
        assert_eq!(p.advance(&c, 0.1), vec!["Passo"]);
        p.playing = false;
        assert!(p.advance(&c, 1.).is_empty());
        p.playing = true;
        assert_eq!(p.advance(&c, 0.95), vec!["Passo"]);
    }
    #[test]
    fn preview_clone_does_not_write_base() {
        let mut s = Scene::new("s", SceneKind::ThreeD);
        let e = Entity::new("piece", None);
        let mut c = Clip::new("c");
        let mut pose = e.transform.clone();
        pose.position[0] = 5.;
        c.insert_key(
            &e.id,
            Keyframe {
                time: 0.,
                transform: pose,
                interpolation: Interpolation::Linear,
            },
        );
        s.entities.push(e);
        let mut preview = s.clone();
        sample_clip(&mut preview, &c, 0.);
        assert_eq!(s.entities[0].transform.position[0], 0.);
        assert_eq!(preview.entities[0].transform.position[0], 5.);
    }
}
