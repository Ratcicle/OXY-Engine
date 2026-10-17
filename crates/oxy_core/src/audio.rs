//! Sound requests, what the game keeps playing, and WAV decoding. The runtime only queues
//! one-shots and keeps the [`AudioState`] its logic asks for; playing them is the hosts'
//! (oxy_audio), so neither runtime nor graph code depends on an audio device.
use crate::document::Id;
use serde::Serialize;
use std::{collections::BTreeMap, path::Path};

/// Effect loops played at once; one more is refused.
pub const MAX_LOOPS: usize = 32;
/// The longest fade in or out logic asks for, in seconds.
pub const MAX_FADE: f64 = 60.;
/// Loops going silent kept at once; past it the oldest is cut.
const MAX_ENDING: usize = 64;

#[derive(Clone, Debug)]
pub struct SoundRequest {
    pub asset: String,
    pub volume: f32,
}

/// Where a sound plays. A one-shot always plays on the effects.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    Music,
    Effects,
}

/// The run's volumes, 0 to 1. A sound is heard at the master volume times its channel's times
/// its own.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct AudioSettings {
    pub master: f64,
    pub music: f64,
    pub effects: f64,
}
impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            master: 1.,
            music: 1.,
            effects: 1.,
        }
    }
}
impl AudioSettings {
    /// What a sound of `channel` is multiplied by.
    pub fn gain(&self, channel: Channel) -> f64 {
        self.master
            * match channel {
                Channel::Music => self.music,
                Channel::Effects => self.effects,
            }
    }
    /// The volume of `channel`: master, music or effects.
    pub fn get(&self, channel: &str) -> Option<f64> {
        Some(match channel {
            "master" => self.master,
            "music" => self.music,
            "effects" => self.effects,
            _ => return None,
        })
    }
    /// Sets the volume of `channel` (master, music or effects); false when it already was so.
    pub fn set(&mut self, channel: &str, volume: f64) -> Result<bool, String> {
        let value = match channel {
            "master" => &mut self.master,
            "music" => &mut self.music,
            "effects" => &mut self.effects,
            _ => return Err("Canal de volume desconhecido; use master/music/effects".into()),
        };
        let changed = *value != volume;
        *value = volume;
        Ok(changed)
    }
}

/// A sound the game keeps playing in a loop.
#[derive(Clone, Debug, PartialEq)]
pub struct LoopVoice {
    pub asset: Id,
    pub volume: f64,
    /// Seconds it fades in as it starts.
    pub fade: f64,
    /// Which request started it: another one starts it over.
    pub generation: u64,
}

/// A loop the game stopped, going silent over `fade` seconds from `at` (run time).
#[derive(Clone, Debug, PartialEq)]
pub struct Ending {
    pub asset: Id,
    pub channel: Channel,
    pub generation: u64,
    pub fade: f64,
    pub at: f64,
}

/// What the game keeps playing, which the hosts follow every frame, and its volumes. The
/// music, at most one loop, and the volumes go on across scene changes; the effect loops, one
/// per audio, end with their scene. It depends on logic alone, so runs repeat it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AudioState {
    pub music: Option<LoopVoice>,
    /// The effect loops by audio.
    pub loops: BTreeMap<Id, LoopVoice>,
    /// Loops stopped with a fade, until it ends.
    pub ending: Vec<Ending>,
    pub settings: AudioSettings,
    /// The last generation given: a count of its own, apart from the activation serials.
    generation: u64,
}

/// How a request changed what plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoopChange {
    /// It was already playing at that volume.
    Unchanged,
    /// It was playing and took another volume.
    Volume,
    /// It started.
    Started,
}

impl AudioState {
    fn next_generation(&mut self) -> u64 {
        self.generation = self.generation.wrapping_add(1).max(1);
        self.generation
    }
    /// Tocar em loop. On the music, the audio playing goes on and takes `volume`, so a scene
    /// restart does not restart it; another one takes its place, the old one fading out over
    /// `fade` seconds as the new one fades in. On the effects, one loop per audio, at most
    /// [`MAX_LOOPS`]; one playing takes `volume`.
    pub fn play_loop(
        &mut self,
        asset: &str,
        channel: Channel,
        volume: f64,
        fade: f64,
        time: f64,
    ) -> Result<LoopChange, String> {
        let playing = match channel {
            Channel::Music => self.music.as_mut().filter(|music| music.asset == asset),
            Channel::Effects => self.loops.get_mut(asset),
        };
        if let Some(voice) = playing {
            let changed = voice.volume != volume;
            voice.volume = volume;
            return Ok(if changed {
                LoopChange::Volume
            } else {
                LoopChange::Unchanged
            });
        }
        if channel == Channel::Effects && self.loops.len() >= MAX_LOOPS {
            return Err(format!(
                "Já há {MAX_LOOPS} sons em loop nos efeitos, o limite: pare um com Parar som \
                 antes de tocar outro"
            ));
        }
        let voice = LoopVoice {
            asset: asset.into(),
            volume,
            fade,
            generation: self.next_generation(),
        };
        match channel {
            Channel::Music => {
                if let Some(old) = self.music.replace(voice) {
                    self.end(old, Channel::Music, fade, time);
                }
            }
            Channel::Effects => {
                self.loops.insert(asset.into(), voice);
            }
        }
        Ok(LoopChange::Started)
    }
    /// Parar som: the loops of `asset` (every one when None) on `channels`, going silent over
    /// `fade` seconds. False when none was playing.
    pub fn stop(
        &mut self,
        asset: Option<&str>,
        channels: &[Channel],
        fade: f64,
        time: f64,
    ) -> bool {
        let wanted = |voice: &LoopVoice| asset.is_none_or(|asset| voice.asset == asset);
        let mut stopped = Vec::new();
        if channels.contains(&Channel::Music)
            && let Some(music) = self.music.take_if(|music| wanted(music))
        {
            stopped.push((music, Channel::Music));
        }
        if channels.contains(&Channel::Effects) {
            let (gone, kept) = std::mem::take(&mut self.loops)
                .into_iter()
                .partition(|(_, voice)| wanted(voice));
            self.loops = kept;
            stopped.extend(gone.into_values().map(|voice| (voice, Channel::Effects)));
        }
        let any = !stopped.is_empty();
        for (voice, channel) in stopped {
            self.end(voice, channel, fade, time);
        }
        any
    }
    /// A scene change: the effect loops end with their scene, at once, those going silent
    /// included. The music and the volumes go on.
    pub fn change_scene(&mut self) {
        self.loops.clear();
        self.ending.retain(|end| end.channel == Channel::Music);
    }
    /// Forgets the loops done going silent at `time`.
    pub fn prune(&mut self, time: f64) {
        self.ending.retain(|end| end.at + end.fade > time);
    }
    /// The loop of `asset` playing on `channel`.
    pub fn voice(&self, asset: &str, channel: Channel) -> Option<&LoopVoice> {
        match channel {
            Channel::Music => self.music.as_ref().filter(|music| music.asset == asset),
            Channel::Effects => self.loops.get(asset),
        }
    }
    /// Whether the loop that request `generation` started still plays, on either channel.
    pub fn plays(&self, generation: u64) -> bool {
        self.music
            .iter()
            .chain(self.loops.values())
            .any(|voice| voice.generation == generation)
    }
    /// `voice` goes silent over `fade` seconds; without one the host cuts it at once.
    fn end(&mut self, voice: LoopVoice, channel: Channel, fade: f64, at: f64) {
        if fade > 0. {
            if self.ending.len() == MAX_ENDING {
                self.ending.remove(0);
            }
            self.ending.push(Ending {
                asset: voice.asset,
                channel,
                generation: voice.generation,
                fade,
                at,
            });
        }
    }
}

/// What a change of what plays was, as reports list it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    /// A loop started, or took another volume.
    Loop,
    /// Loops stopped.
    Stop,
    /// A channel's volume changed.
    Volume,
}

/// A change logic made to what plays (Tocar em loop, Parar som, Volume do canal), kept for a
/// host that reports a run.
#[derive(Clone, Debug, PartialEq)]
pub struct AudioChange {
    pub kind: ChangeKind,
    /// The audio; none for a stop of every loop of the channel and for a volume.
    pub asset: Option<Id>,
    /// The node's channel: music or effects, all for a stop, master for a volume.
    pub channel: &'static str,
    pub volume: f64,
    pub fade: f64,
    /// A loop that started, not one that only took another volume.
    pub started: bool,
    /// The loop's [`LoopVoice::generation`]; 0 for a stop and a volume.
    pub generation: u64,
}
impl AudioChange {
    /// Whether it outlives the scene that made it: the music's changes and the volumes do; the
    /// effect loops end with their scene.
    pub fn lasts(&self) -> bool {
        self.kind == ChangeKind::Volume || self.channel != "effects"
    }
}

/// Decoded WAV samples, interleaved by channel, at full scale: volume is the mixer's.
#[derive(Debug)]
pub struct PcmAudio {
    pub channels: u16,
    pub sample_rate: u32,
    pub samples: Vec<i16>,
}

/// The longest WAV played, in seconds.
const MAX_SECONDS: u64 = 600;

pub fn read_wav(path: &Path) -> Result<PcmAudio, String> {
    let mut reader =
        hound::WavReader::open(path).map_err(|error| format!("WAV inválido: {error}"))?;
    let spec = reader.spec();
    if !(1..=2).contains(&spec.channels) || spec.sample_rate == 0 {
        return Err("Áudio v0.1 aceita WAV mono ou estéreo com frequência válida.".into());
    }
    if u64::from(reader.duration()) > u64::from(spec.sample_rate) * MAX_SECONDS {
        return Err("WAV excede o limite de 10 minutos para reprodução simples.".into());
    }
    let samples = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .map(|sample| sample.map(|value| (value.clamp(-1.0, 1.0) * 32767.0) as i16))
            .collect::<Result<Vec<_>, _>>(),
        hound::SampleFormat::Int => {
            if spec.bits_per_sample == 0 || spec.bits_per_sample > 32 {
                return Err("Profundidade de WAV não suportada.".into());
            }
            // To 16 bits: 16-bit samples are kept as they are.
            let shift = i32::from(spec.bits_per_sample) - 16;
            reader
                .samples::<i32>()
                .map(|sample| {
                    sample.map(|value| {
                        let value = if shift >= 0 {
                            value >> shift
                        } else {
                            value << -shift
                        };
                        value.clamp(i16::MIN.into(), i16::MAX.into()) as i16
                    })
                })
                .collect::<Result<Vec<_>, _>>()
        }
    }
    .map_err(|error| format!("Falha ao decodificar WAV: {error}"))?;
    Ok(PcmAudio {
        channels: spec.channels,
        sample_rate: spec.sample_rate,
        samples,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_music_goes_on_or_crossfades_and_effect_loops_end_with_the_scene() {
        let mut audio = AudioState::default();
        let started = audio
            .play_loop("trilha", Channel::Music, 0.8, 1., 0.)
            .unwrap();
        assert_eq!(started, LoopChange::Started);
        let first = audio.music.clone().unwrap();
        // Asked again, as a restarted scene does, it goes on: only the volume may change.
        let again = audio
            .play_loop("trilha", Channel::Music, 0.8, 2., 1.)
            .unwrap();
        assert_eq!(again, LoopChange::Unchanged);
        let louder = audio
            .play_loop("trilha", Channel::Music, 1., 0., 1.)
            .unwrap();
        assert_eq!(louder, LoopChange::Volume);
        assert_eq!(audio.music.as_ref().unwrap().generation, first.generation);
        assert!(audio.ending.is_empty());
        // Another audio takes its place, the old one fading out over the new one's seconds.
        audio
            .play_loop("chefe", Channel::Music, 1., 2., 3.)
            .unwrap();
        let music = audio.music.clone().unwrap();
        assert_eq!(music.asset, "chefe");
        assert_ne!(music.generation, first.generation);
        assert!(audio.plays(music.generation) && !audio.plays(first.generation));
        assert_eq!(audio.voice("chefe", Channel::Music), Some(&music));
        assert_eq!(audio.voice("chefe", Channel::Effects), None);
        assert_eq!(
            audio.ending,
            [Ending {
                asset: "trilha".into(),
                channel: Channel::Music,
                generation: first.generation,
                fade: 2.,
                at: 3.,
            }]
        );
        audio.prune(4.9);
        assert_eq!(audio.ending.len(), 1);
        audio.prune(5.);
        assert!(audio.ending.is_empty());
        // Effect loops: one per audio, gone with the scene; the music and volumes stay.
        audio
            .play_loop("vento", Channel::Effects, 0.5, 0., 5.)
            .unwrap();
        audio
            .play_loop("chuva", Channel::Effects, 0.5, 0., 5.)
            .unwrap();
        audio.stop(Some("chuva"), &[Channel::Effects], 1., 5.);
        assert_eq!(audio.loops.keys().collect::<Vec<_>>(), ["vento"]);
        audio.settings.set("music", 0.25).unwrap();
        audio.change_scene();
        assert!(audio.loops.is_empty() && audio.ending.is_empty());
        assert_eq!(audio.music, Some(music));
        assert_eq!(audio.settings.gain(Channel::Music), 0.25);
    }

    #[test]
    fn stops_pick_the_audio_and_the_channels_and_effect_loops_have_a_limit() {
        let mut audio = AudioState::default();
        audio.play_loop("tema", Channel::Music, 1., 0., 0.).unwrap();
        for i in 0..MAX_LOOPS {
            audio
                .play_loop(&format!("efeito {i}"), Channel::Effects, 1., 0., 0.)
                .unwrap();
        }
        let error = audio
            .play_loop("mais um", Channel::Effects, 1., 0., 0.)
            .unwrap_err();
        assert!(error.contains("32 sons em loop"), "{error}");
        // One already playing is not one more.
        audio
            .play_loop("efeito 0", Channel::Effects, 0.5, 0., 0.)
            .unwrap();
        // Without a fade, nothing lingers: the host cuts them.
        assert!(!audio.stop(Some("tema"), &[Channel::Effects], 0., 1.));
        assert!(audio.stop(None, &[Channel::Effects], 0., 1.));
        assert!(audio.loops.is_empty() && audio.ending.is_empty());
        assert!(audio.music.is_some());
        assert!(audio.stop(None, &[Channel::Music, Channel::Effects], 0.5, 1.));
        assert!(audio.music.is_none());
        assert_eq!(audio.ending.len(), 1);
        assert!(!audio.stop(None, &[Channel::Music, Channel::Effects], 0.5, 1.));
        assert!(audio.settings.set("geral", 1.).is_err());
        assert_eq!(audio.settings.set("master", 1.), Ok(false));
        assert_eq!(audio.settings.get("effects"), Some(1.));
    }

    #[test]
    fn invalid_wav_is_a_recoverable_error() {
        assert!(read_wav(Path::new("missing-audio.wav")).is_err());
    }

    #[test]
    fn samples_keep_full_scale_and_the_limit_counts_seconds() {
        let folder = std::env::temp_dir().join(format!("oxy-wav-{}", crate::document::new_id()));
        std::fs::create_dir_all(&folder).unwrap();
        let write = |name: &str, rate: u32, channels: u16, frames: usize| {
            let path = folder.join(name);
            let spec = hound::WavSpec {
                channels,
                sample_rate: rate,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            };
            let mut writer = hound::WavWriter::create(&path, spec).unwrap();
            for _ in 0..frames * usize::from(channels) {
                writer.write_sample(i16::MAX).unwrap();
            }
            writer.finalize().unwrap();
            path
        };
        // No volume is baked in: 16-bit samples come as they are.
        let loud = read_wav(&write("cheio.wav", 8000, 1, 10)).unwrap();
        assert_eq!(loud.samples, [i16::MAX; 10]);
        // Ten minutes of a high rate in stereo pass; one second more does not.
        let rate = 200;
        assert!(read_wav(&write("dez.wav", rate, 2, 600 * rate as usize)).is_ok());
        let error = read_wav(&write("mais.wav", rate, 2, 601 * rate as usize)).unwrap_err();
        assert!(error.contains("10 minutos"), "{error}");
        std::fs::remove_dir_all(folder).unwrap();
    }
}
