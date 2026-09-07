//! Small WAV output adapter. The runtime only emits requests; neither runtime nor
//! graph code depends on an editor panel or on the availability of an audio device.
use std::path::Path;

#[derive(Clone, Debug)]
pub struct SoundRequest {
    pub asset: String,
    pub volume: f32,
}

#[derive(Debug)]
pub struct PcmAudio {
    pub channels: u16,
    pub sample_rate: u32,
    pub samples: Vec<i16>,
}

pub fn read_wav(path: &Path, volume: f32) -> Result<PcmAudio, String> {
    let mut reader =
        hound::WavReader::open(path).map_err(|error| format!("WAV inválido: {error}"))?;
    let spec = reader.spec();
    if !(1..=2).contains(&spec.channels) || spec.sample_rate == 0 {
        return Err("Áudio v0.1 aceita WAV mono ou estéreo com frequência válida.".into());
    }
    if reader.duration() as u64 * u64::from(spec.channels) > 48_000 * 2 * 600 {
        return Err("WAV excede o limite de 10 minutos para reprodução simples.".into());
    }
    let volume = volume.clamp(0.0, 1.0);
    let samples = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .map(|sample| sample.map(|value| (value.clamp(-1.0, 1.0) * volume * 32767.0) as i16))
            .collect::<Result<Vec<_>, _>>(),
        hound::SampleFormat::Int => {
            if spec.bits_per_sample == 0 || spec.bits_per_sample > 32 {
                return Err("Profundidade de WAV não suportada.".into());
            }
            let scale = (1u64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|sample| {
                    sample.map(|value| {
                        (value as f32 / scale * volume * 32767.0).clamp(-32768.0, 32767.0) as i16
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

pub fn play_wav(path: &Path, volume: f32) -> Result<(), String> {
    let pcm = read_wav(path, volume)?;
    platform::play(pcm)
}

#[cfg(not(target_os = "windows"))]
mod platform {
    use super::PcmAudio;
    pub fn play(_pcm: PcmAudio) -> Result<(), String> {
        Err("Saída WAV nativa disponível no alvo Windows; o arquivo foi validado.".into())
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use super::PcmAudio;
    use std::{ffi::c_void, ptr};

    #[repr(C)]
    struct WaveFormat {
        tag: u16,
        channels: u16,
        sample_rate: u32,
        bytes_per_second: u32,
        block_align: u16,
        bits_per_sample: u16,
        extra_size: u16,
    }
    #[repr(C)]
    struct WaveHeader {
        data: *mut u8,
        length: u32,
        recorded: u32,
        user: usize,
        flags: u32,
        loops: u32,
        next: *mut WaveHeader,
        reserved: usize,
    }
    #[link(name = "winmm")]
    unsafe extern "system" {
        fn waveOutOpen(
            handle: *mut *mut c_void,
            device: u32,
            format: *const WaveFormat,
            callback: usize,
            instance: usize,
            flags: u32,
        ) -> u32;
        fn waveOutPrepareHeader(handle: *mut c_void, header: *mut WaveHeader, size: u32) -> u32;
        fn waveOutWrite(handle: *mut c_void, header: *mut WaveHeader, size: u32) -> u32;
        fn waveOutUnprepareHeader(handle: *mut c_void, header: *mut WaveHeader, size: u32) -> u32;
        fn waveOutReset(handle: *mut c_void) -> u32;
        fn waveOutClose(handle: *mut c_void) -> u32;
    }

    pub fn play(mut pcm: PcmAudio) -> Result<(), String> {
        let format = WaveFormat {
            tag: 1,
            channels: pcm.channels,
            sample_rate: pcm.sample_rate,
            bytes_per_second: pcm.sample_rate * u32::from(pcm.channels) * 2,
            block_align: pcm.channels * 2,
            bits_per_sample: 16,
            extra_size: 0,
        };
        let mut handle = ptr::null_mut();
        // SAFETY: Valid PCM format and out pointer; no callback is requested.
        let status = unsafe { waveOutOpen(&mut handle, u32::MAX, &format, 0, 0, 0) };
        if status != 0 {
            return Err(format!(
                "Dispositivo de áudio indisponível (WinMM {status})."
            ));
        }
        let handle_value = handle as usize;
        let max_seconds =
            pcm.samples.len() as f64 / (f64::from(pcm.sample_rate) * f64::from(pcm.channels)) + 2.0;
        let (started_tx, started_rx) = std::sync::mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("oxy-wav".into())
            .spawn(move || {
                let handle = handle_value as *mut c_void;
                let mut header = Box::new(WaveHeader {
                    data: pcm.samples.as_mut_ptr().cast(),
                    length: (pcm.samples.len() * 2) as u32,
                    recorded: 0,
                    user: 0,
                    flags: 0,
                    loops: 0,
                    next: ptr::null_mut(),
                    reserved: 0,
                });
                let size = std::mem::size_of::<WaveHeader>() as u32;
                // SAFETY: The owned sample buffer/header live until WinMM completes or is reset.
                unsafe {
                    let prepare = waveOutPrepareHeader(handle, &mut *header, size);
                    if prepare == 0 {
                        let write = waveOutWrite(handle, &mut *header, size);
                        if write == 0 {
                            let _ = started_tx.send(Ok(()));
                            let started = std::time::Instant::now();
                            while ptr::read_volatile(&header.flags) & 1 == 0
                                && started.elapsed().as_secs_f64() < max_seconds
                            {
                                std::thread::sleep(std::time::Duration::from_millis(10));
                            }
                            waveOutReset(handle);
                        } else {
                            let _ = started_tx.send(Err(format!(
                                "Dispositivo recusou reprodução WAV (WinMM {write})."
                            )));
                        }
                        waveOutUnprepareHeader(handle, &mut *header, size);
                    } else {
                        let _ = started_tx.send(Err(format!(
                            "Dispositivo recusou buffer WAV (WinMM {prepare})."
                        )));
                    }
                    waveOutClose(handle);
                }
            })
            .map_err(|error| {
                // SAFETY: Thread creation failed so ownership never reached a running worker.
                unsafe {
                    waveOutClose(handle);
                }
                format!("Não foi possível iniciar reprodução: {error}")
            })?;
        started_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .map_err(|error| format!("Dispositivo de áudio não respondeu ao iniciar: {error}"))?
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn invalid_wav_is_a_recoverable_error() {
        assert!(super::read_wav(std::path::Path::new("missing-audio.wav"), 1.0).is_err());
    }
}
