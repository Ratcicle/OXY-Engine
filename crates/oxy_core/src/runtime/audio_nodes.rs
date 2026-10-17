//! Sound operations exposed by the typed graph adapter: one-shots, loops on the music and
//! effects channels and the run's volumes. The runtime queues one-shots and keeps what should
//! play in `run.audio`; the hosts play them.
use super::*;
use crate::audio::{AudioChange, AudioState, ChangeKind, Channel, LoopChange, MAX_FADE};
impl Runtime {
    /// Runs `node` when it is one of these actions, else hands it to the math ones; false when
    /// no one runs it.
    pub(super) fn audio_action(
        &mut self,
        graph: &PreparedGraph,
        node: &Node,
        context: &mut Context,
        budget: &mut usize,
    ) -> Result<bool, String> {
        match node.operation.as_str() {
            "action.sound" => {
                let asset = self.audio_asset(node)?;
                self.sounds.push(SoundRequest {
                    asset,
                    volume: node.number("volume", 1.0).clamp(0.0, 1.0) as f32,
                });
            }
            "action.sound_loop" => {
                let asset = self.audio_asset(node)?;
                let (channel, name) = match node.text("channel") {
                    "music" => (Channel::Music, "music"),
                    "effects" => (Channel::Effects, "effects"),
                    _ => return Err("Canal desconhecido; use music/effects".into()),
                };
                let volume = self.volume(graph, node, context, budget)?;
                let fade = fade(node)?;
                let change = self
                    .run
                    .audio
                    .play_loop(&asset, channel, volume, fade, self.time)?;
                // A request that changes nothing, as a restarted scene asking for its music
                // again, is not reported.
                if change != LoopChange::Unchanged {
                    let started = change == LoopChange::Started;
                    let generation = self
                        .run
                        .audio
                        .voice(&asset, channel)
                        .map_or(0, |voice| voice.generation);
                    self.record_audio(AudioChange {
                        kind: ChangeKind::Loop,
                        asset: Some(asset),
                        channel: name,
                        volume,
                        fade: if started { fade } else { 0. },
                        started,
                        generation,
                    });
                }
            }
            "action.sound_stop" => {
                let asset = match node.text("asset") {
                    "" => None,
                    _ => Some(self.audio_asset(node)?),
                };
                let (channels, name): (&[Channel], _) = match node.text("channel") {
                    "music" => (&[Channel::Music], "music"),
                    "effects" => (&[Channel::Effects], "effects"),
                    "all" => (&[Channel::Music, Channel::Effects], "all"),
                    _ => return Err("Canal desconhecido; use music/effects/all".into()),
                };
                let fade = fade(node)?;
                if self
                    .run
                    .audio
                    .stop(asset.as_deref(), channels, fade, self.time)
                {
                    self.record_audio(AudioChange {
                        kind: ChangeKind::Stop,
                        asset,
                        channel: name,
                        volume: 0.,
                        fade,
                        started: false,
                        generation: 0,
                    });
                }
            }
            "action.volume" => {
                let channel = crate::graph::VOLUME_CHANNELS
                    .iter()
                    .map(|(id, _)| *id)
                    .find(|id| *id == node.text("channel"))
                    .ok_or("Canal de volume desconhecido; use master/music/effects")?;
                let volume = self.volume(graph, node, context, budget)?;
                if self.run.audio.settings.set(channel, volume)? {
                    self.record_audio(AudioChange {
                        kind: ChangeKind::Volume,
                        asset: None,
                        channel,
                        volume,
                        fade: 0.,
                        started: false,
                        generation: 0,
                    });
                }
            }
            _ => return self.math_action(graph, node, context, budget),
        }
        Ok(true)
    }

    pub(super) fn audio_data(
        &self,
        graph: &PreparedGraph,
        node: &Node,
        output: &str,
        context: &Context,
        budget: &mut usize,
        depth: usize,
    ) -> Result<Value, String> {
        match node.operation.as_str() {
            "audio.read" => self
                .run
                .audio
                .settings
                .get(output)
                .map(Value::Number)
                .ok_or_else(|| format!("Ler volumes não tem a saída {output}")),
            _ => self.math_data(graph, node, output, context, budget, depth),
        }
    }

    /// What the game keeps playing and its volumes, for the hosts to play.
    pub fn audio(&self) -> &AudioState {
        &self.run.audio
    }

    /// The WAV asset a sound node names.
    fn audio_asset(&self, node: &Node) -> Result<Id, String> {
        let label = crate::graph::definition(&node.operation).map_or("", |def| def.label);
        match node.text("asset") {
            "" => Err(format!(
                "{label} sem áudio escolhido: escolha o áudio no nó"
            )),
            id => match self.project.asset(id) {
                None => Err("Asset de áudio não existe".into()),
                Some(asset) if asset.kind != crate::document::AssetKind::Audio => {
                    Err("Asset selecionado não é áudio".into())
                }
                Some(asset) => Ok(asset.id.clone()),
            },
        }
    }

    /// A node's Volume: its input when connected, else its parameter, kept within 0 and 1.
    fn volume(
        &self,
        graph: &PreparedGraph,
        node: &Node,
        context: &Context,
        budget: &mut usize,
    ) -> Result<f64, String> {
        self.input_value(graph, node, "volume", context, budget, 0)?
            .number()
            .filter(|volume| volume.is_finite())
            .map(|volume| volume.clamp(0., 1.) + 0.)
            .ok_or_else(|| "Volume precisa ser um número finito".into())
    }

    fn record_audio(&mut self, change: AudioChange) {
        if let Some(recording) = &mut self.recording {
            recording.audio.push(change);
        }
    }

    /// The end of a step: when it changed scene, the loops it started that no longer play, as
    /// an effect loop the change ended or a music the next scene replaced, were never heard,
    /// and their starts leave the recording (see `Recording::audio`).
    pub(super) fn settle_audio_changes(&mut self) {
        let (Some(_), Some(recording)) = (self.audio_before_change, &mut self.recording) else {
            return;
        };
        let start = self.audio_step_start.min(recording.audio.len());
        let step = recording.audio.split_off(start);
        let audio = &self.run.audio;
        recording.audio.extend(
            step.into_iter()
                .filter(|change| !change.started || audio.plays(change.generation)),
        );
    }
}

/// A node's Transição, in seconds.
fn fade(node: &Node) -> Result<f64, String> {
    let fade = node.number("fade", 0.);
    if !fade.is_finite() || !(0.0..=MAX_FADE).contains(&fade) {
        return Err(format!(
            "Transição deve estar entre 0 e {MAX_FADE} segundos"
        ));
    }
    Ok(fade)
}
