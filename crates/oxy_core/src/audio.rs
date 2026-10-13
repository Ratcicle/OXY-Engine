//! Sound requests and WAV decoding. The runtime only emits requests; playing them is the hosts'
//! (oxy_audio), so neither runtime nor graph code depends on an audio device.
use std::path::Path;

#[derive(Clone, Debug)]
pub struct SoundRequest {
    pub asset: String,
    pub volume: f32,
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
