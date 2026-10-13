//! The output device, through cpal: the default one, at 48 kHz in stereo when it takes that, in
//! f32 or i16. Its callback pulls each block from the shared mix.
use crate::mix::Mix;
use cpal::{
    SampleFormat,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use std::sync::{Arc, Mutex, PoisonError};

pub(crate) struct Output {
    stream: cpal::Stream,
    rate: u32,
    events: Arc<Mutex<Events>>,
}

/// What the stream reported from its own thread.
#[derive(Default)]
struct Events {
    errors: Vec<String>,
    /// The device stopped (unplugged, or another format): the mixer opens it again.
    lost: bool,
}

impl Output {
    pub(crate) fn open(mix: Arc<Mutex<Mix>>) -> Result<Self, String> {
        let device = cpal::default_host()
            .default_output_device()
            .ok_or("nenhum dispositivo de saída")?;
        let config = choose(&device)?;
        let channels = usize::from(config.channels().max(1));
        let events = Arc::new(Mutex::new(Events::default()));
        let stream = match config.sample_format() {
            SampleFormat::I16 => build::<i16>(&device, config.config(), channels, mix, &events),
            _ => build::<f32>(&device, config.config(), channels, mix, &events),
        }
        .map_err(|error| describe(&error))?;
        stream.play().map_err(|error| describe(&error))?;
        Ok(Self {
            stream,
            rate: config.sample_rate(),
            events,
        })
    }
    pub(crate) fn rate(&self) -> u32 {
        self.rate
    }
    /// Stops pulling blocks, so the device can rest; false when it cannot.
    pub(crate) fn rest(&self) -> bool {
        self.stream.pause().is_ok()
    }
    /// Pulls blocks again after `rest`; false when it cannot.
    pub(crate) fn play(&self) -> bool {
        self.stream.play().is_ok()
    }
    pub(crate) fn lost(&self) -> bool {
        self.events
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .lost
    }
    pub(crate) fn take_errors(&self) -> Vec<String> {
        let mut events = self.events.lock().unwrap_or_else(PoisonError::into_inner);
        std::mem::take(&mut events.errors)
    }
}

/// 48 kHz stereo in f32 or i16 when the device takes it; else its own rate and channels, which
/// the mixer converts to.
fn choose(device: &cpal::Device) -> Result<cpal::SupportedStreamConfig, String> {
    let usable = |format| matches!(format, SampleFormat::F32 | SampleFormat::I16);
    let mut ranges: Vec<_> = device
        .supported_output_configs()
        .map(|ranges| {
            ranges
                .filter(|range| usable(range.sample_format()))
                .collect()
        })
        .unwrap_or_default();
    ranges.sort_by_key(|range| range.sample_format() != SampleFormat::F32);
    if let Some(range) = ranges
        .iter()
        .find(|range| range.channels() == 2 && range.contains_rate(crate::RATE))
    {
        return Ok(range.with_sample_rate(crate::RATE));
    }
    let default = device
        .default_output_config()
        .map_err(|error| describe(&error))?;
    if usable(default.sample_format()) {
        return Ok(default);
    }
    ranges
        .into_iter()
        .find_map(|range| range.try_with_sample_rate(default.sample_rate()))
        .ok_or_else(|| "formato de saída não suportado".into())
}

/// A sample of the device's format from the mix's.
trait Sample: cpal::SizedSample + Send + 'static {
    fn from_mix(value: f32) -> Self;
}
impl Sample for f32 {
    fn from_mix(value: f32) -> Self {
        value.clamp(-1., 1.)
    }
}
impl Sample for i16 {
    fn from_mix(value: f32) -> Self {
        (value.clamp(-1., 1.) * 32767.) as i16
    }
}

fn build<T: Sample>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    channels: usize,
    mix: Arc<Mutex<Mix>>,
    events: &Arc<Mutex<Events>>,
) -> Result<cpal::Stream, cpal::Error> {
    let events = Arc::clone(events);
    let mut block = Vec::new();
    device.build_output_stream(
        config,
        move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
            block.resize(data.len() / channels * 2, 0.);
            mix.lock()
                .unwrap_or_else(PoisonError::into_inner)
                .render(&mut block);
            for (out, frame) in data.chunks_exact_mut(channels).zip(block.chunks_exact(2)) {
                if channels == 1 {
                    out[0] = T::from_mix((frame[0] + frame[1]) * 0.5);
                } else {
                    out[0] = T::from_mix(frame[0]);
                    out[1] = T::from_mix(frame[1]);
                    for rest in &mut out[2..] {
                        *rest = T::from_mix(0.);
                    }
                }
            }
        },
        move |error| report(&events, &error),
        None,
    )
}

fn report(events: &Mutex<Events>, error: &cpal::Error) {
    use cpal::ErrorKind;
    let kind = error.kind();
    // A glitch, a route the stream followed or a priority refused: it goes on playing.
    if matches!(
        kind,
        ErrorKind::Xrun | ErrorKind::DeviceChanged | ErrorKind::RealtimeDenied
    ) {
        return;
    }
    let lost = matches!(
        kind,
        ErrorKind::DeviceNotAvailable | ErrorKind::StreamInvalidated | ErrorKind::HostUnavailable
    );
    let mut events = events.lock().unwrap_or_else(PoisonError::into_inner);
    if events.lost {
        return;
    }
    let text = if lost {
        format!(
            "A saída de áudio parou ({}); ela será aberta de novo.",
            describe(error)
        )
    } else {
        format!("Falha na saída de áudio: {}.", describe(error))
    };
    if events.errors.last() != Some(&text) && events.errors.len() < 16 {
        events.errors.push(text);
    }
    events.lost = lost;
}

fn describe(error: &cpal::Error) -> String {
    use cpal::ErrorKind;
    match error.kind() {
        ErrorKind::DeviceNotAvailable => "dispositivo de áudio indisponível",
        ErrorKind::HostUnavailable => "sistema de áudio indisponível",
        ErrorKind::DeviceBusy => "dispositivo de áudio ocupado",
        ErrorKind::PermissionDenied => "acesso ao áudio negado",
        ErrorKind::UnsupportedConfig => "formato de saída não suportado",
        ErrorKind::StreamInvalidated => "o formato do dispositivo mudou",
        _ => "falha do sistema de áudio",
    }
    .into()
}
