//! The mixing itself, with no device: voices of decoded clips added into stereo blocks, each one
//! with its volume, fade and anti-click ramp. Pure, so tests run it frame by frame.
use oxy_core::audio::PcmAudio;
use std::sync::Arc;

/// Seconds of the anti-click ramp: a cut, a pause, a resume and a volume change take this long.
pub(crate) const RAMP_SECONDS: f32 = 0.010;
/// One-shots heard at once; one more stops the oldest.
pub(crate) const MAX_ONCE: usize = 64;
/// Voices kept at once, those still going silent included. Past it those are dropped at once,
/// which only a flood of sounds within a few milliseconds reaches.
const MAX_VOICES: usize = 256;

/// A decoded sound in the mixing format: stereo frames at the output rate.
#[derive(Debug)]
pub(crate) struct Clip {
    samples: Vec<i16>,
}

impl Clip {
    /// `pcm` at `rate`, by linear interpolation; a mono sound plays on both sides.
    pub(crate) fn new(pcm: &PcmAudio, rate: u32) -> Self {
        let channels = usize::from(pcm.channels.max(1));
        let input = pcm.samples.len() / channels;
        let frames = if pcm.sample_rate == rate || input == 0 {
            input
        } else {
            let (input, rate, source) = (input as u64, u64::from(rate), u64::from(pcm.sample_rate));
            ((input * rate + source / 2) / source) as usize
        };
        let sample =
            |frame: usize, side: usize| f64::from(pcm.samples[frame * channels + side % channels]);
        let step = f64::from(pcm.sample_rate) / f64::from(rate);
        let mut samples = Vec::with_capacity(frames * 2);
        for frame in 0..frames {
            let at = frame as f64 * step;
            let first = (at as usize).min(input - 1);
            let next = (first + 1).min(input - 1);
            let weight = (at - first as f64).min(1.);
            for side in 0..2 {
                let value = sample(first, side) * (1. - weight) + sample(next, side) * weight;
                samples.push(value.round() as i16);
            }
        }
        Self { samples }
    }
    pub(crate) fn frames(&self) -> usize {
        self.samples.len() / 2
    }
    /// The memory it holds, for the decode cache's budget.
    pub(crate) fn bytes(&self) -> usize {
        std::mem::size_of_val(self.samples.as_slice())
    }
}

/// Who a voice plays for.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Kind {
    /// Once, for the game (Reproduzir WAV).
    Once,
    /// In a loop while the game's state keeps it: the request `generation` of `asset`.
    Loop { asset: String, generation: u64 },
    /// The Library's Ouvir, which no pause freezes.
    Preview,
}

pub(crate) struct Voice {
    pub(crate) kind: Kind,
    clip: Arc<Clip>,
    position: usize,
    /// The volume asked; `gain` follows it within the anti-click ramp.
    pub(crate) volume: f32,
    gain: f32,
    /// The fade in or out: `fade` goes toward `fade_target` by `fade_step` a frame.
    fade: f32,
    fade_target: f32,
    fade_step: f32,
    /// The anti-click ramp of a pause: down to silence while frozen, back up after.
    ramp: f32,
    pub(crate) frozen_host: bool,
    pub(crate) frozen_game: bool,
    /// Stopped: it goes silent and leaves.
    pub(crate) released: bool,
}

impl Voice {
    pub(crate) fn is_loop(&self, asset: &str, generation: u64) -> bool {
        matches!(&self.kind, Kind::Loop { asset: a, generation: g } if a == asset && *g == generation)
    }
    /// Stops it: silent in `seconds`, or in the anti-click ramp when that is shorter. Stopping it
    /// again only makes it faster.
    pub(crate) fn release(&mut self, seconds: f32, rate: u32) {
        let step = self.fade / (seconds.max(RAMP_SECONDS) * rate as f32);
        self.fade_step = if self.released {
            self.fade_step.max(step)
        } else {
            step
        };
        self.fade_target = 0.;
        self.released = true;
    }
    /// Adds this voice to `out`, stereo frames; false once it is over.
    fn render(&mut self, out: &mut [f32], step: f32) -> bool {
        let samples = &self.clip.samples;
        let frames = samples.len() / 2;
        let looping = matches!(self.kind, Kind::Loop { .. });
        let frozen = self.frozen_host || self.frozen_game;
        for frame in out.chunks_exact_mut(2) {
            self.ramp = approach(self.ramp, if frozen { 0. } else { 1. }, step);
            if self.ramp == 0. {
                // Silent: a frozen voice holds its place; a stopped one is over.
                return !self.released;
            }
            self.gain = approach(self.gain, self.volume, step);
            self.fade = approach(self.fade, self.fade_target, self.fade_step);
            let gain = self.gain * self.fade * self.ramp / 32768.;
            frame[0] += f32::from(samples[self.position * 2]) * gain;
            frame[1] += f32::from(samples[self.position * 2 + 1]) * gain;
            self.position += 1;
            if self.position == frames {
                if !looping {
                    return false;
                }
                // A loop goes on from its first frame, with no gap.
                self.position = 0;
            }
            if self.released && self.fade == 0. {
                return false;
            }
        }
        true
    }
}

/// The voices playing, shared with the device's callback.
pub(crate) struct Mix {
    rate: u32,
    /// How much the anti-click ramp moves a frame.
    step: f32,
    pub(crate) voices: Vec<Voice>,
}

impl Mix {
    pub(crate) fn new(rate: u32) -> Self {
        let mut mix = Self {
            rate,
            step: 1.,
            voices: Vec::new(),
        };
        mix.set_rate(rate);
        mix
    }
    pub(crate) fn rate(&self) -> u32 {
        self.rate
    }
    pub(crate) fn set_rate(&mut self, rate: u32) {
        self.rate = rate.max(1);
        self.step = 1. / (RAMP_SECONDS * self.rate as f32);
    }
    /// Starts `clip` at `volume`, fading in over `fade` seconds (at once with 0), frozen when
    /// the host is paused. Past [`MAX_ONCE`] one-shots, the oldest stops.
    pub(crate) fn start(
        &mut self,
        kind: Kind,
        clip: Arc<Clip>,
        volume: f32,
        fade: f32,
        frozen_host: bool,
    ) {
        if clip.frames() == 0 {
            return;
        }
        let once = |voice: &&mut Voice| voice.kind == Kind::Once && !voice.released;
        if kind == Kind::Once && self.voices.iter_mut().filter(once).count() >= MAX_ONCE {
            let rate = self.rate;
            if let Some(oldest) = self.voices.iter_mut().find(once) {
                oldest.release(0., rate);
            }
        }
        if self.voices.len() >= MAX_VOICES {
            let mut excess = self.voices.len() + 1 - MAX_VOICES;
            self.voices.retain(|voice| {
                let drop = excess > 0 && voice.released;
                excess -= usize::from(drop);
                !drop
            });
        }
        let fading = fade > 0.;
        self.voices.push(Voice {
            kind,
            clip,
            position: 0,
            volume,
            gain: volume,
            fade: if fading { 0. } else { 1. },
            fade_target: 1.,
            fade_step: if fading {
                1. / (fade * self.rate as f32)
            } else {
                1.
            },
            ramp: if frozen_host { 0. } else { 1. },
            frozen_host,
            frozen_game: false,
            released: false,
        });
    }
    /// Stops the voices `which` picks, silent in `seconds` (see [`Voice::release`]).
    pub(crate) fn release(&mut self, seconds: f32, which: impl Fn(&Voice) -> bool) {
        let rate = self.rate;
        for voice in self.voices.iter_mut().filter(|voice| which(voice)) {
            voice.release(seconds, rate);
        }
    }
    /// The next `out.len() / 2` stereo frames of every voice, added up. Voices that end leave.
    pub(crate) fn render(&mut self, out: &mut [f32]) {
        out.fill(0.);
        let step = self.step;
        self.voices.retain_mut(|voice| voice.render(out, step));
    }
    /// Voices playing or frozen; the ones going silent after a stop do not count.
    pub(crate) fn live(&self) -> usize {
        self.voices.iter().filter(|voice| !voice.released).count()
    }
    /// Nothing to hear: no voice, or every one frozen.
    pub(crate) fn quiet(&self) -> bool {
        self.voices
            .iter()
            .all(|voice| voice.frozen_host || voice.frozen_game)
    }
}

/// `value` a `step` closer to `target`, landing on it from within a step: rounding leaves the
/// steps a hair short of their sum.
fn approach(value: f32, target: f32, step: f32) -> f32 {
    let distance = target - value;
    if distance.abs() <= step * 1.001 {
        target
    } else {
        value + step.copysign(distance)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// At this rate the anti-click ramp takes 10 frames, 0.1 a frame.
    const RATE: u32 = 1000;

    fn clip(values: &[i16]) -> Arc<Clip> {
        Arc::new(Clip {
            samples: values.iter().flat_map(|&value| [value, value]).collect(),
        })
    }
    /// Half the full scale, for `frames` frames.
    fn half(frames: usize) -> Arc<Clip> {
        clip(&vec![16384; frames])
    }
    /// The left side of the next `frames` frames; the right side must be the same.
    fn left(mix: &mut Mix, frames: usize) -> Vec<f32> {
        let mut out = vec![0.; frames * 2];
        mix.render(&mut out);
        out.chunks(2)
            .map(|frame| {
                assert_eq!(frame[0], frame[1]);
                frame[0]
            })
            .collect()
    }
    fn near(values: &[f32], expected: &[f32]) {
        assert_eq!(values.len(), expected.len(), "{values:?}");
        for (value, expected) in values.iter().zip(expected) {
            assert!(
                (value - expected).abs() < 1e-4,
                "{values:?} != {expected:?}"
            );
        }
    }

    #[test]
    fn voices_add_up_each_at_its_volume() {
        let mut mix = Mix::new(RATE);
        mix.start(Kind::Once, half(100), 0.5, 0., false);
        mix.start(Kind::Once, half(100), 1., 0., false);
        // 0.5 × 0.5 + 0.5 × 1.
        near(&left(&mut mix, 3), &[0.75; 3]);
        // A new volume arrives within the ramp, not at once.
        mix.voices[0].volume = 1.;
        near(&left(&mut mix, 6), &[0.8, 0.85, 0.9, 0.95, 1.0, 1.0]);
    }

    #[test]
    fn a_loop_wraps_with_no_gap_and_a_one_shot_ends() {
        let mut mix = Mix::new(RATE);
        let values = [1000, 2000, 3000];
        let looped = Kind::Loop {
            asset: "a".into(),
            generation: 1,
        };
        mix.start(looped, clip(&values), 1., 0., false);
        let out = left(&mut mix, 10);
        let expected: Vec<f32> = (0..10).map(|i| values[i % 3] as f32 / 32768.).collect();
        near(&out, &expected);
        assert_eq!(mix.live(), 1);
        let mut mix = Mix::new(RATE);
        mix.start(Kind::Once, clip(&values), 1., 0., false);
        let out = left(&mut mix, 5);
        near(&out[3..], &[0., 0.]);
        assert!(mix.voices.is_empty());
    }

    #[test]
    fn fades_in_and_out_over_their_seconds() {
        let mut mix = Mix::new(RATE);
        let looped = Kind::Loop {
            asset: "a".into(),
            generation: 1,
        };
        // In over 10 ms: 10 frames.
        mix.start(looped, half(1000), 1., 0.01, false);
        let ramp_in: Vec<f32> = (1..=10).map(|i| 0.05 * i as f32).collect();
        near(&left(&mut mix, 10), &ramp_in);
        near(&left(&mut mix, 2), &[0.5; 2]);
        // Out over 20 ms, then gone.
        mix.release(0.02, |_| true);
        assert_eq!(mix.live(), 0);
        let ramp_out: Vec<f32> = (1..=20).rev().map(|i| 0.025 * (i - 1) as f32).collect();
        near(&left(&mut mix, 20), &ramp_out);
        assert!(mix.voices.is_empty());
    }

    #[test]
    fn a_cut_and_a_pause_take_the_anti_click_ramp() {
        let mut mix = Mix::new(RATE);
        mix.start(Kind::Once, half(1000), 1., 0., false);
        left(&mut mix, 1);
        // A cut is never instant: 10 ms down to silence.
        mix.release(0., |_| true);
        let down: Vec<f32> = (0..10).map(|i| 0.45 - 0.05 * i as f32).collect();
        near(&left(&mut mix, 10), &down);
        assert!(mix.voices.is_empty());

        // A pause ramps down too, then holds the place until it resumes.
        let rising: Vec<i16> = (0..100).map(|i| i * 100).collect();
        mix.start(Kind::Once, clip(&rising), 1., 0., false);
        left(&mut mix, 2);
        mix.voices[0].frozen_game = true;
        let frozen = left(&mut mix, 12);
        let expected: Vec<f32> = (2..12)
            .map(|i| (i * 100) as f32 / 32768. * (0.9 - 0.1 * (i - 2) as f32))
            .chain([0., 0.])
            .collect();
        near(&frozen, &expected);
        near(&left(&mut mix, 5), &[0.; 5]);
        mix.voices[0].frozen_game = false;
        // Back from frame 11, where it stopped, ramping up again.
        let resumed = left(&mut mix, 3);
        let expected: Vec<f32> = (0..3)
            .map(|i| ((11 + i) * 100) as f32 / 32768. * (0.1 * (i + 1) as f32))
            .collect();
        near(&resumed, &expected);
        // A voice started while the host is paused waits silent.
        let mut mix = Mix::new(RATE);
        mix.start(Kind::Once, half(10), 1., 0., true);
        near(&left(&mut mix, 20), &[0.; 20]);
        assert_eq!(mix.live(), 1);
        // Stopped while frozen, it leaves at once.
        mix.release(0., |_| true);
        left(&mut mix, 1);
        assert!(mix.voices.is_empty());
    }

    #[test]
    fn past_64_one_shots_the_oldest_stops() {
        let mut mix = Mix::new(RATE);
        mix.start(Kind::Preview, half(1000), 1., 0., false);
        for _ in 0..MAX_ONCE + 6 {
            mix.start(Kind::Once, half(1000), 1., 0., false);
        }
        assert_eq!(mix.live(), MAX_ONCE + 1);
        let stopped: Vec<bool> = mix.voices.iter().map(|voice| voice.released).collect();
        assert!(!stopped[0], "the preview is not a one-shot");
        assert!(stopped[1..7].iter().all(|&stopped| stopped));
        assert!(stopped[7..].iter().all(|&stopped| !stopped));
        // They go silent within the ramp.
        left(&mut mix, 10);
        assert_eq!(mix.voices.len(), MAX_ONCE + 1);
    }

    #[test]
    fn clips_are_resampled_to_the_output_rate_in_stereo() {
        let pcm = |channels, sample_rate, samples| PcmAudio {
            channels,
            sample_rate,
            samples,
        };
        // A second at 22050 Hz is a second at 48 kHz.
        let second = Clip::new(&pcm(1, 22050, vec![7; 22050]), 48_000);
        assert_eq!(second.frames(), 48_000);
        assert!(second.samples.iter().all(|&sample| sample == 7));
        assert_eq!(second.bytes(), 48_000 * 2 * 2);
        // Mono plays on both sides; stereo keeps its sides; the same rate is a copy.
        let mono = Clip::new(&pcm(1, 100, vec![1, -2, 3]), 100);
        assert_eq!(mono.samples, [1, 1, -2, -2, 3, 3]);
        let stereo = Clip::new(&pcm(2, 100, vec![1, -1, 2, -2]), 100);
        assert_eq!(stereo.samples, [1, -1, 2, -2]);
        // Twice the rate fills in the middle, and holds the last frame.
        let doubled = Clip::new(&pcm(1, 1, vec![0, 1000]), 2);
        assert_eq!(doubled.samples, [0, 0, 500, 500, 1000, 1000, 1000, 1000]);
        assert_eq!(Clip::new(&pcm(2, 44100, Vec::new()), 48_000).frames(), 0);
    }
}
