//! Sound for the hosts (player and editor): one output device, opened with the first sound and
//! resting while nothing plays, and a mixer of our own that plays what the runtime asks for.
//! oxy_core, headless runs and the MCP never link a device. Without one the mixer runs silent:
//! voices are still tracked, and one diagnostic says why nothing is heard.
mod cache;
mod device;
mod mix;

use cache::Cache;
use mix::{Clip, Kind, Mix};
use oxy_core::{audio::SoundRequest, document::Project, persistence, runtime::Runtime};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::{Duration, Instant},
};

/// The rate the mixer asks the device for, and runs at without one.
const RATE: u32 = 48_000;
/// Memory for decoded sounds: about ten minutes of music at 48 kHz in stereo take 110 MB.
const CACHE_BYTES: usize = 512 * 1024 * 1024;
/// How long the device waits with nothing to play before it rests: a running stream keeps the
/// computer from sleeping.
const REST_AFTER: Duration = Duration::from_secs(1);

/// What freezes the sounds. The host's pause (the engine's in the player, the test's in the
/// editor) freezes every voice where it is; the game's own pause freezes its effects, while
/// sounds asked during it, as a menu's, play.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pause {
    pub host: bool,
    pub game: bool,
}

/// A sound the game's state keeps playing in a loop (see [`Mixer::sync`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Loop {
    /// The audio asset.
    pub asset: String,
    /// Which request of it: another one starts it over.
    pub generation: u64,
    pub volume: f32,
    /// Seconds it fades in as it starts.
    pub fade: f32,
}

/// A loop the game's state stopped, going silent over `fade` seconds (see [`Mixer::sync`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Ending {
    pub asset: String,
    pub generation: u64,
    pub fade: f32,
}

enum Output {
    /// Not opened yet, or closed: the next sound opens it.
    Closed,
    Device(device::Output),
    /// No device: voices go on with the clock, unheard. `failed` when opening one failed, which
    /// the next run tries again.
    Silent {
        clock: Instant,
        failed: bool,
    },
}

/// How the device opens; tests put one that fails in its place.
type Open = fn(Arc<Mutex<Mix>>) -> Result<device::Output, String>;

/// What a host plays: one-shots (at most 64), loops and the Library's preview, mixed at the
/// device's rate in stereo. A sound is decoded once and kept, within about 512 MB.
pub struct Mixer {
    mix: Arc<Mutex<Mix>>,
    output: Output,
    open: Open,
    cache: Cache,
    /// The pause last followed.
    pause: Pause,
    /// The device runs: from a voice starting until a second of quiet (see
    /// [`Mixer::rest_when_quiet`]).
    awake: bool,
    /// Since when nothing was heard.
    quiet: Option<Instant>,
    errors: Vec<String>,
    /// Sounds that failed since the last `stop_all`, which are said once.
    failed: HashSet<PathBuf>,
    /// Where a silent mixer renders.
    block: Vec<f32>,
}

impl Default for Mixer {
    fn default() -> Self {
        Self::new()
    }
}

impl Mixer {
    /// A mixer that opens the default output device with its first sound.
    pub fn new() -> Self {
        Self::with(Output::Closed, device::Output::open)
    }
    /// A mixer that never opens a device, for tests: voices are tracked and go on with the
    /// clock, unheard.
    pub fn silent() -> Self {
        let output = Output::Silent {
            clock: Instant::now(),
            failed: false,
        };
        Self::with(output, device::Output::open)
    }
    fn with(output: Output, open: Open) -> Self {
        Self {
            mix: Arc::new(Mutex::new(Mix::new(RATE))),
            output,
            open,
            cache: Cache::new(CACHE_BYTES),
            pause: Pause::default(),
            awake: false,
            quiet: None,
            errors: Vec::new(),
            failed: HashSet::new(),
            block: Vec::new(),
        }
    }

    /// A frame of a running game: plays the sounds it asked for since the last frame and
    /// follows its pauses (see [`Pause`]).
    pub fn update(&mut self, runtime: &mut Runtime, root: &Path) {
        // The time since the last frame passed under the pauses it had.
        self.advance_silent();
        let pause = Pause {
            host: runtime.paused,
            game: runtime.game_paused(),
        };
        // Before the new sounds: a pause that starts now freezes what was playing, not them.
        self.set_pause(pause);
        for request in std::mem::take(&mut runtime.sounds) {
            self.play_once(root, runtime.project(), &request);
        }
        self.sync(root, runtime.project(), &[], &[], pause);
    }

    /// Plays `request` once, as Reproduzir WAV does; past 64 at once, the oldest stops.
    pub fn play_once(&mut self, root: &Path, project: &Project, request: &SoundRequest) {
        self.advance_silent();
        if let Some(clip) = self.asset_clip(root, project, &request.asset) {
            let volume = request.volume.clamp(0., 1.);
            lock(&self.mix).start(Kind::Once, clip, volume, 0., self.pause.host);
        }
    }

    /// Follows the loops the game's state keeps: one not playing yet starts, fading in over its
    /// `fade`, and one playing takes the volume asked. One no longer asked fades out over the
    /// `fade` of its entry in `ending`, or, in neither list, stops within the anti-click ramp,
    /// as a scene change cuts them. Another `generation` of the same asset starts it over. Also
    /// follows `pause`, and lets the device rest when nothing plays.
    pub fn sync(
        &mut self,
        root: &Path,
        project: &Project,
        loops: &[Loop],
        ending: &[Ending],
        pause: Pause,
    ) {
        self.advance_silent();
        if self.awake && matches!(&self.output, Output::Device(device) if device.lost()) {
            // Unplugged: what plays goes on in the device that takes its place.
            self.wake();
        }
        self.set_pause(pause);
        let mut playing = vec![false; loops.len()];
        {
            let mut mix = lock(&self.mix);
            let rate = mix.rate();
            for voice in mix.voices.iter_mut().filter(|voice| !voice.released) {
                if !matches!(voice.kind, Kind::Loop { .. }) {
                    continue;
                }
                let wanted = loops
                    .iter()
                    .position(|wanted| voice.is_loop(&wanted.asset, wanted.generation));
                if let Some(index) = wanted {
                    voice.volume = loops[index].volume.clamp(0., 1.);
                    playing[index] = true;
                } else {
                    let fade = ending
                        .iter()
                        .find(|end| voice.is_loop(&end.asset, end.generation))
                        .map_or(0., |end| end.fade.max(0.));
                    voice.release(fade, rate);
                }
            }
        }
        for (wanted, _) in loops.iter().zip(playing).filter(|(_, playing)| !playing) {
            let Some(clip) = self.asset_clip(root, project, &wanted.asset) else {
                continue;
            };
            let mut mix = lock(&self.mix);
            let started = mix
                .voices
                .iter()
                .any(|voice| !voice.released && voice.is_loop(&wanted.asset, wanted.generation));
            if !started {
                let kind = Kind::Loop {
                    asset: wanted.asset.clone(),
                    generation: wanted.generation,
                };
                let volume = wanted.volume.clamp(0., 1.);
                mix.start(kind, clip, volume, wanted.fade, self.pause.host);
            }
        }
        self.rest_when_quiet();
    }

    /// Parar, a reload, a new test: every voice stops, within the anti-click ramp, and the
    /// device rests a second later. A device that failed to open is tried again by the next
    /// sound.
    pub fn stop_all(&mut self) {
        let mut mix = lock(&self.mix);
        mix.release(0., |_| true);
        if matches!(self.output, Output::Silent { failed: true, .. }) {
            // Unheard anyway.
            mix.voices.clear();
            drop(mix);
            self.output = Output::Closed;
        }
        self.pause = Pause::default();
        self.failed.clear();
    }

    /// The Library's Ouvir: plays the WAV at `path` once, in place of the last one heard there.
    /// No pause freezes it. A device that failed to open is tried again.
    pub fn preview(&mut self, path: &Path, volume: f32) -> Result<(), String> {
        self.advance_silent();
        if matches!(self.output, Output::Silent { failed: true, .. }) {
            self.output = Output::Closed;
        }
        self.wake();
        let rate = lock(&self.mix).rate();
        let clip = self.cache.clip(path, rate)?;
        let mut mix = lock(&self.mix);
        mix.release(0., |voice| voice.kind == Kind::Preview);
        mix.start(Kind::Preview, clip, volume.clamp(0., 1.), 0., false);
        Ok(())
    }

    /// What went wrong since the last call, for the host's diagnostics or Console.
    pub fn take_errors(&mut self) -> Vec<String> {
        if let Output::Device(device) = &self.output {
            self.errors.extend(device.take_errors());
        }
        std::mem::take(&mut self.errors)
    }

    /// Voices playing or frozen; those going silent after a stop do not count.
    pub fn voice_count(&self) -> usize {
        lock(&self.mix).live()
    }

    /// Lets the device rest once nothing was heard for a second (no voice, or every one
    /// frozen): a running stream keeps the computer from sleeping. `update` and `sync` call it;
    /// a host that does not, as the editor between tests, calls it each frame. True while the
    /// device runs, so a host that draws only on input comes back to check.
    pub fn rest_when_quiet(&mut self) -> bool {
        self.advance_silent();
        if !self.awake {
            return false;
        }
        if !lock(&self.mix).quiet() {
            self.quiet = None;
            return true;
        }
        let since = *self.quiet.get_or_insert_with(Instant::now);
        if since.elapsed() < REST_AFTER {
            return true;
        }
        self.awake = false;
        self.quiet = None;
        if let Output::Device(device) = &self.output
            && !device.rest()
        {
            // A stream that cannot pause closes; the next sound opens it again.
            self.close();
        }
        false
    }

    /// Freezes or frees the voices when a pause starts or ends. The Library's preview is
    /// nobody's; every other voice is an effect for now.
    fn set_pause(&mut self, pause: Pause) {
        if pause == self.pause {
            return;
        }
        let before = std::mem::replace(&mut self.pause, pause);
        let mut mix = lock(&self.mix);
        for voice in mix.voices.iter_mut() {
            if voice.kind == Kind::Preview {
                continue;
            }
            if pause.host != before.host {
                voice.frozen_host = pause.host;
            }
            if pause.game != before.game {
                voice.frozen_game = pause.game;
            }
        }
        // Voices back from a pause need the device, which may have rested.
        let heard = !mix.quiet();
        drop(mix);
        if heard && !self.awake {
            self.wake();
        }
    }

    /// The decoded sound of `asset`; its errors are said once.
    fn asset_clip(&mut self, root: &Path, project: &Project, asset: &str) -> Option<Arc<Clip>> {
        let Some(asset) = project.asset(asset) else {
            self.report(
                PathBuf::from(asset),
                "O jogo pediu um som que não está no projeto.".into(),
            );
            return None;
        };
        let path = match persistence::resolve_asset_path(root, &asset.path) {
            Ok(path) => path,
            Err(error) => {
                self.report(
                    root.join(&asset.path),
                    format!("Som {}: {error}", asset.name),
                );
                return None;
            }
        };
        if self.failed.contains(&path) {
            return None;
        }
        self.wake();
        let rate = lock(&self.mix).rate();
        match self.cache.clip(&path, rate) {
            Ok(clip) => Some(clip),
            Err(error) => {
                self.report(path, format!("Som {}: {error}", asset.name));
                None
            }
        }
    }
    /// An error of the sound `key`, said once until `stop_all`: a game may ask for it every
    /// step.
    fn report(&mut self, key: PathBuf, error: String) {
        if self.failed.insert(key) {
            self.errors.push(error);
        }
    }

    /// Before a voice is heard: the device opens with the first sound, runs again after resting,
    /// and opens again after it stopped (unplugged).
    fn wake(&mut self) {
        self.quiet = None;
        let resting = !std::mem::replace(&mut self.awake, true);
        match &self.output {
            Output::Silent { .. } => return,
            Output::Device(device) if !device.lost() && (!resting || device.play()) => return,
            _ => {}
        }
        self.open_output();
    }
    /// Opens the device, closing one that stopped first. Without one the mixer goes silent and
    /// says why, once a run.
    fn open_output(&mut self) {
        self.close();
        match (self.open)(Arc::clone(&self.mix)) {
            Ok(device) => {
                let mut mix = lock(&self.mix);
                if mix.rate() != device.rate() {
                    // Decoded for another rate: what played stops, and sounds decode again.
                    mix.voices.clear();
                    mix.set_rate(device.rate());
                    self.cache.clear();
                }
                drop(mix);
                self.output = Output::Device(device);
            }
            Err(reason) => {
                self.errors.push(format!(
                    "Sem saída de áudio ({reason}): os sons não serão ouvidos."
                ));
                self.output = Output::Silent {
                    clock: Instant::now(),
                    failed: true,
                };
            }
        }
    }
    /// Closes the device, keeping what it reported for the host.
    fn close(&mut self) {
        if let Output::Device(device) = std::mem::replace(&mut self.output, Output::Closed) {
            self.errors.extend(device.take_errors());
        }
    }

    /// Without a device, the voices go on with the clock, unheard, so a sound ends when it
    /// would have. At most a second at a time.
    fn advance_silent(&mut self) {
        let Output::Silent { clock, .. } = &mut self.output else {
            return;
        };
        let mut mix = lock(&self.mix);
        let rate = mix.rate();
        let frames = (clock.elapsed().as_secs_f64() * f64::from(rate)) as usize;
        let second = rate as usize;
        if frames > second {
            *clock = Instant::now();
        } else {
            *clock += Duration::from_secs_f64(frames as f64 / f64::from(rate));
        }
        let mut left = frames.min(second);
        while left > 0 && !mix.voices.is_empty() {
            let count = left.min(1024);
            self.block.resize(count * 2, 0.);
            mix.render(&mut self.block);
            left -= count;
        }
    }
}

fn lock(mix: &Mutex<Mix>) -> MutexGuard<'_, Mix> {
    mix.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use oxy_core::{
        document::{Asset, AssetKind, Entity, Value, new_id},
        graph::{Edge, Node},
    };

    /// A temporary folder, removed with its files at the end of the test.
    pub(crate) struct Folder(pub(crate) PathBuf);
    impl Folder {
        pub(crate) fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!("oxy-audio-{name}-{}", new_id()));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    /// A 16-bit WAV of `frames` frames at half the full scale.
    pub(crate) fn wav(
        folder: &Path,
        name: &str,
        rate: u32,
        channels: u16,
        frames: usize,
    ) -> PathBuf {
        let path = folder.join(name);
        let spec = hound::WavSpec {
            channels,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&path, spec).unwrap();
        for _ in 0..frames * usize::from(channels) {
            writer.write_sample(16384i16).unwrap();
        }
        writer.finalize().unwrap();
        path
    }
    /// A project with the five-second sound Bip, in `folder`, whose scene plays it as it
    /// starts.
    fn beep_project(folder: &Path) -> (Project, String) {
        std::fs::create_dir_all(folder.join("assets")).unwrap();
        wav(&folder.join("assets"), "bip.wav", 8000, 1, 40_000);
        let mut project = Project::new("Som");
        let asset = new_id();
        project.assets.push(Asset {
            id: asset.clone(),
            name: "Bip".into(),
            path: "assets/bip.wav".into(),
            kind: AssetKind::Audio,
            model: None,
        });
        let mut entity = Entity::new("Alto-falante", None);
        let start = Node::new("event.scene_start", [0.; 2]);
        let mut sound = Node::new("action.sound", [200., 0.]);
        sound
            .params
            .insert("asset".into(), Value::Text(asset.clone()));
        entity.graph.edges.push(Edge {
            from_node: start.id.clone(),
            from_port: "exec".into(),
            to_node: sound.id.clone(),
            to_port: "exec".into(),
        });
        entity.graph.nodes.extend([start, sound]);
        project.scenes[0].entities.push(entity);
        (project, asset)
    }
    fn request(asset: &str) -> SoundRequest {
        SoundRequest {
            asset: asset.into(),
            volume: 1.,
        }
    }

    #[test]
    fn a_silent_mixer_tracks_its_voices() {
        let folder = Folder::new("silencio");
        let (project, asset) = beep_project(&folder.0);
        let mut mixer = Mixer::silent();
        for _ in 0..3 {
            mixer.play_once(&folder.0, &project, &request(&asset));
        }
        assert_eq!(mixer.voice_count(), 3);
        for _ in 0..70 {
            mixer.play_once(&folder.0, &project, &request(&asset));
        }
        assert_eq!(mixer.voice_count(), mix::MAX_ONCE);
        mixer.stop_all();
        assert_eq!(mixer.voice_count(), 0);
        assert!(mixer.take_errors().is_empty());
        // Silent on purpose, it never tries a device.
        assert!(matches!(mixer.output, Output::Silent { failed: false, .. }));
        // With the clock, a short sound ends when it would have.
        let short = wav(&folder.0, "curto.wav", 8000, 1, 80);
        mixer.preview(&short, 1.).unwrap();
        assert_eq!(mixer.voice_count(), 1);
        std::thread::sleep(Duration::from_millis(60));
        mixer.sync(&folder.0, &project, &[], &[], Pause::default());
        assert_eq!(mixer.voice_count(), 0);
    }

    #[test]
    fn without_a_device_the_mixer_says_so_once_a_run_and_goes_on_silent() {
        let folder = Folder::new("sem-dispositivo");
        let (project, asset) = beep_project(&folder.0);
        let missing: Open = |_| Err("nenhum dispositivo de saída".into());
        let said = ["Sem saída de áudio (nenhum dispositivo de saída): os sons não serão ouvidos."];
        let mut mixer = Mixer::with(Output::Closed, missing);
        mixer.play_once(&folder.0, &project, &request(&asset));
        mixer.play_once(&folder.0, &project, &request(&asset));
        assert_eq!(mixer.voice_count(), 2);
        assert_eq!(mixer.take_errors(), said);
        mixer.play_once(&folder.0, &project, &request(&asset));
        assert!(mixer.take_errors().is_empty());
        assert!(matches!(mixer.output, Output::Silent { failed: true, .. }));
        // The next run tries the device again (headphones plugged in, another program let go of
        // it), and says it again.
        mixer.stop_all();
        assert!(matches!(mixer.output, Output::Closed));
        assert_eq!(mixer.voice_count(), 0);
        mixer.play_once(&folder.0, &project, &request(&asset));
        assert_eq!(mixer.take_errors(), said);
        assert_eq!(mixer.voice_count(), 1);
        // So does the Library's Ouvir, while what played goes on.
        mixer
            .preview(&folder.0.join("assets").join("bip.wav"), 1.)
            .unwrap();
        assert_eq!(mixer.take_errors(), said);
        assert_eq!(mixer.voice_count(), 2);
    }

    #[test]
    fn the_device_rests_a_second_after_the_last_sound() {
        let folder = Folder::new("descanso");
        let (project, asset) = beep_project(&folder.0);
        let mut mixer = Mixer::silent();
        let a_second_ago = |mixer: &mut Mixer| {
            mixer.quiet = Some(Instant::now() - REST_AFTER);
        };
        // Nothing played yet: nothing runs.
        assert!(!mixer.rest_when_quiet());
        mixer.play_once(&folder.0, &project, &request(&asset));
        assert!(mixer.rest_when_quiet());
        a_second_ago(&mut mixer);
        assert!(mixer.rest_when_quiet(), "a sound plays");
        // Frozen by a pause, it holds its place and lets the device rest; the resume wakes it.
        let paused = Pause {
            host: true,
            game: false,
        };
        mixer.sync(&folder.0, &project, &[], &[], paused);
        assert!(mixer.rest_when_quiet());
        a_second_ago(&mut mixer);
        assert!(!mixer.rest_when_quiet());
        assert!(!mixer.awake);
        assert_eq!(mixer.voice_count(), 1);
        mixer.sync(&folder.0, &project, &[], &[], Pause::default());
        assert!(mixer.awake);
        // Stopped, the device rests a second after its last ramp.
        mixer.stop_all();
        std::thread::sleep(Duration::from_millis(30));
        assert!(mixer.rest_when_quiet());
        a_second_ago(&mut mixer);
        assert!(!mixer.rest_when_quiet());
        // The Library's Ouvir wakes it too.
        mixer
            .preview(&folder.0.join("assets").join("bip.wav"), 1.)
            .unwrap();
        assert!(mixer.awake);
    }

    #[test]
    fn update_plays_what_the_game_asked_and_follows_its_pauses() {
        let folder = Folder::new("pausas");
        let (project, asset) = beep_project(&folder.0);
        let mut runtime = Runtime::new(&project, &project.start_scene).unwrap();
        assert_eq!(runtime.sounds.len(), 1);
        let mut mixer = Mixer::silent();
        mixer.update(&mut runtime, &folder.0);
        assert!(runtime.sounds.is_empty());
        assert_eq!(mixer.voice_count(), 1);
        let frozen = |mixer: &Mixer| -> Vec<(bool, bool)> {
            let mix = lock(&mixer.mix);
            mix.voices
                .iter()
                .map(|voice| (voice.frozen_host, voice.frozen_game))
                .collect()
        };
        // The game's pause freezes what plays; a sound asked during it (a menu's) plays.
        runtime.toggle_game_pause();
        assert!(runtime.game_paused());
        mixer.update(&mut runtime, &folder.0);
        mixer.play_once(&folder.0, &project, &request(&asset));
        assert_eq!(frozen(&mixer), [(false, true), (false, false)]);
        // The host's pause freezes them all, and the Library's preview never.
        mixer
            .preview(&folder.0.join("assets").join("bip.wav"), 1.)
            .unwrap();
        runtime.set_paused(true);
        mixer.update(&mut runtime, &folder.0);
        assert_eq!(
            frozen(&mixer),
            [(true, true), (true, false), (false, false)]
        );
        runtime.set_paused(false);
        runtime.toggle_game_pause();
        mixer.update(&mut runtime, &folder.0);
        assert_eq!(frozen(&mixer), [(false, false); 3]);
        assert_eq!(mixer.voice_count(), 3);
        mixer.stop_all();
        assert_eq!(mixer.voice_count(), 0);
    }

    #[test]
    fn loops_follow_the_state_asked() {
        let folder = Folder::new("loops");
        let (project, asset) = beep_project(&folder.0);
        let mut mixer = Mixer::silent();
        let wanted = |generation, volume, fade| Loop {
            asset: asset.clone(),
            generation,
            volume,
            fade,
        };
        let ending = |generation, fade| Ending {
            asset: asset.clone(),
            generation,
            fade,
        };
        let state = |mixer: &Mixer| -> Vec<(u64, f32, bool)> {
            let mix = lock(&mixer.mix);
            mix.voices
                .iter()
                .map(|voice| match &voice.kind {
                    Kind::Loop { generation, .. } => (*generation, voice.volume, voice.released),
                    _ => panic!("only loops"),
                })
                .collect()
        };
        let pause = Pause::default();
        mixer.sync(&folder.0, &project, &[wanted(1, 1., 0.)], &[], pause);
        mixer.sync(&folder.0, &project, &[wanted(1, 0.5, 0.)], &[], pause);
        // Asked again, it goes on at the new volume.
        assert_eq!(state(&mixer), [(1, 0.5, false)]);
        // Another request starts it over, while the first fades out over the seconds it ends
        // with.
        let crossfade = [ending(1, 2.)];
        mixer.sync(&folder.0, &project, &[wanted(2, 1., 2.)], &crossfade, pause);
        assert_eq!(state(&mixer), [(1, 0.5, true), (2, 1., false)]);
        assert_eq!(mixer.voice_count(), 1);
        // One in neither list stops within the anti-click ramp, as a scene change cuts it,
        // whatever it faded in with.
        mixer.sync(&folder.0, &project, &[], &crossfade, pause);
        assert_eq!(mixer.voice_count(), 0);
        std::thread::sleep(Duration::from_millis(60));
        mixer.sync(&folder.0, &project, &[], &crossfade, pause);
        assert_eq!(state(&mixer), [(1, 0.5, true)]);
    }

    #[test]
    fn a_broken_sound_is_said_once_a_run() {
        let folder = Folder::new("quebrado");
        let (mut project, asset) = beep_project(&folder.0);
        std::fs::write(folder.0.join("assets").join("bip.wav"), b"nada").unwrap();
        project.assets[0].name = "Porta".into();
        let mut mixer = Mixer::silent();
        mixer.play_once(&folder.0, &project, &request(&asset));
        mixer.play_once(&folder.0, &project, &request(&asset));
        let errors = mixer.take_errors();
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(
            errors[0].starts_with("Som Porta: WAV inválido"),
            "{errors:?}"
        );
        assert_eq!(mixer.voice_count(), 0);
        // Each run says it again.
        mixer.stop_all();
        mixer.play_once(&folder.0, &project, &request(&asset));
        assert_eq!(mixer.take_errors().len(), 1);
        assert!(
            mixer
                .preview(&folder.0.join("assets").join("bip.wav"), 1.)
                .is_err()
        );
    }
}
