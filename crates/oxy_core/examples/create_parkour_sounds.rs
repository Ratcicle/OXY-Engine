//! Preparation only: synthesizes the original sounds and music of the parkour example
//! (`examples/parkour`) as 16-bit mono 44.1 kHz WAVs in the folder given as the first argument.
//! Every sound is built from tones, envelopes and filtered noise; the noise comes from a
//! fixed-seed generator, so every run writes the same bytes. The music is one period of a
//! periodic signal: notes that ring past the end of the loop wrap around to its start, which is
//! what rendering two passes and keeping the second gives, so the loop point joins with the same
//! value and slope as any other pair of samples.
//!
//! `cargo run --release -p oxy_core --example create_parkour_sounds -- <pasta de saída>`
use std::f64::consts::TAU;
use std::path::Path;

const RATE: u32 = 44_100;
const SEED: u64 = 20_261_014;

fn rate() -> f64 {
    f64::from(RATE)
}

/// Seconds to a whole number of samples.
fn samples(seconds: f64) -> usize {
    (seconds * rate()).round() as usize
}

/// Equal-tempered frequency of a MIDI note (A4 = 69 = 440 Hz).
fn midi_hz(note: u8) -> f64 {
    440.0 * 2f64.powf((f64::from(note) - 69.0) / 12.0)
}

/// White noise in [-1, 1) from SplitMix64 with a fixed seed.
struct Noise(u64);

impl Noise {
    fn new(stream: u64) -> Self {
        Noise(SEED ^ stream.wrapping_mul(0x9E37_79B9_7F4A_7C15))
    }

    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        (z >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
    }
}

/// Residual that smooths a step of the naive wave at phase 0 (PolyBLEP).
fn poly_blep(phase: f64, step: f64) -> f64 {
    if phase < step {
        let x = phase / step;
        x + x - x * x - 1.0
    } else if phase > 1.0 - step {
        let x = (phase - 1.0) / step;
        x * x + x + x + 1.0
    } else {
        0.0
    }
}

/// Band-limited pulse wave without DC: `duty` is the share of the cycle at the top (0.5 is a
/// square), `phase` is in cycles and `step` is the phase advanced per sample.
fn pulse(phase: f64, step: f64, duty: f64) -> f64 {
    let p = phase.rem_euclid(1.0);
    let naive = if p < duty { 1.0 } else { -1.0 };
    naive + poly_blep(p, step)
        - poly_blep((p + 1.0 - duty).rem_euclid(1.0), step)
        - (2.0 * duty - 1.0)
}

/// Triangle wave starting at 0 and rising, `phase` in cycles.
fn triangle(phase: f64) -> f64 {
    1.0 - 4.0 * ((phase + 0.25).rem_euclid(1.0) - 0.5).abs()
}

/// Linear rise from 0 to 1 over `length` seconds.
fn rise(t: f64, length: f64) -> f64 {
    (t / length).min(1.0)
}

/// Linear fall to 0 over the last `fade` seconds of a voice `length` seconds long.
fn tail(t: f64, length: f64, fade: f64) -> f64 {
    ((length - t) / fade).clamp(0.0, 1.0)
}

/// A sound effect of `seconds`, sample by sample from the time since its start.
fn effect(seconds: f64, mut voice: impl FnMut(f64) -> f64) -> Vec<f64> {
    (0..samples(seconds))
        .map(|index| voice(index as f64 / rate()))
        .collect()
}

/// Adds a voice that starts at `onset` and lasts `length` seconds to a looping buffer: what
/// passes the end of the buffer continues at its start.
fn mix(buffer: &mut [f64], onset: f64, length: f64, mut voice: impl FnMut(f64) -> f64) {
    let size = buffer.len();
    let mut index = (onset * rate()).ceil() as usize;
    loop {
        let t = index as f64 / rate() - onset;
        if t >= length {
            break;
        }
        buffer[index % size] += voice(t);
        index += 1;
    }
}

/// 2 ms ramp in and 5 ms ramp out, the edges every effect shares.
fn edges(mut sound: Vec<f64>) -> Vec<f64> {
    let rise = samples(0.002);
    let fall = samples(0.005);
    for (index, value) in sound.iter_mut().take(rise).enumerate() {
        *value *= index as f64 / rise as f64;
    }
    for (index, value) in sound.iter_mut().rev().take(fall).enumerate() {
        *value *= index as f64 / fall as f64;
    }
    sound
}

/// Scales to a peak of −1 dBFS and rounds to 16 bits.
fn to_pcm(sound: &[f64]) -> Vec<i16> {
    let peak = sound
        .iter()
        .fold(0.0f64, |peak, value| peak.max(value.abs()));
    let gain = if peak > 0.0 {
        10f64.powf(-1.0 / 20.0) / peak
    } else {
        0.0
    };
    sound
        .iter()
        .map(|value| (value * gain * 32767.0).round().clamp(-32768.0, 32767.0) as i16)
        .collect()
}

/// 120 BPM, 8 bars of 4/4 (16 s): Am · F · C · G, two bars each. Triangle bass in eighths,
/// 25 % pulse arpeggio in sixteenths, kick on 1 and 3, snare on 2 and 4, hi-hat on the off-beats.
fn music() -> Vec<f64> {
    const BEAT: f64 = 0.5;
    const BARS: usize = 8;
    /// Arpeggio degrees 1-3-5-8-5-3-1-5 as indices into the chord.
    const PATTERN: [usize; 8] = [0, 1, 2, 3, 2, 1, 0, 2];
    /// Bass root and arpeggio notes (MIDI) of Am, F, C and G.
    const CHORDS: [(u8, [u8; 4]); 4] = [
        (45, [69, 72, 76, 81]),
        (41, [65, 69, 72, 77]),
        (48, [72, 76, 79, 84]),
        (43, [67, 71, 74, 79]),
    ];
    let mut buffer = vec![0.0; samples(BARS as f64 * 4.0 * BEAT)];
    let mut noise = Noise::new(1);
    for bar in 0..BARS {
        let (root, chord) = CHORDS[bar / 2];
        let start = bar as f64 * 4.0 * BEAT;
        for eighth in 0..8 {
            // An octave up on the "and" of beat 4.
            let frequency = midi_hz(if eighth == 7 { root + 12 } else { root });
            let gate = 0.225;
            mix(&mut buffer, start + eighth as f64 * BEAT / 2.0, 0.24, |t| {
                // 5 ms attack, down to 70 % in 80 ms, held, 15 ms release.
                let level = if t < 0.005 {
                    t / 0.005
                } else {
                    1.0 - 0.3 * ((t - 0.005) / 0.08).min(1.0)
                };
                let release = if t > gate {
                    1.0 - (t - gate) / 0.015
                } else {
                    1.0
                };
                0.30 * level * release.max(0.0) * triangle(frequency * t)
            });
        }
        for sixteenth in 0..16 {
            let frequency = midi_hz(chord[PATTERN[sixteenth % 8]]);
            let step = frequency / rate();
            let length = 0.3;
            mix(
                &mut buffer,
                start + sixteenth as f64 * BEAT / 4.0,
                length,
                |t| {
                    0.14 * rise(t, 0.002)
                        * (-t / 0.07).exp()
                        * tail(t, length, 0.02)
                        * pulse(frequency * t, step, 0.25)
                },
            );
        }
        for beat in 0..4 {
            let onset = start + beat as f64 * BEAT;
            let length = 0.25;
            if beat % 2 == 0 {
                mix(&mut buffer, onset, length, |t| {
                    let phase = 45.0 * t + 75.0 * 0.03 * (1.0 - (-t / 0.03).exp());
                    0.55 * (-t / 0.09).exp() * tail(t, length, 0.01) * (TAU * phase).sin()
                });
            } else {
                mix(&mut buffer, onset, length, |t| {
                    let rattle = 0.22 * (-t / 0.045).exp() * noise.next();
                    let body = 0.15 * (-t / 0.03).exp() * (TAU * 190.0 * t).sin();
                    rise(t, 0.001) * tail(t, length, 0.01) * (rattle + body)
                });
            }
            let mut previous = noise.next();
            let length = 0.08;
            mix(&mut buffer, onset + BEAT / 2.0, length, |t| {
                // Difference of consecutive noise samples: a first-order high-pass.
                let current = noise.next();
                let high = (current - previous) * 0.5;
                previous = current;
                0.10 * rise(t, 0.0005) * (-t / 0.012).exp() * tail(t, length, 0.005) * high
            });
        }
    }
    let norm = 1.2f64.tanh();
    buffer
        .iter()
        .map(|value| (1.2 * value).tanh() / norm)
        .collect()
}

fn click() -> Vec<f64> {
    let mut noise = Noise::new(2);
    effect(0.06, |t| {
        let tone = (TAU * 1400.0 * t).sin() * (-t / 0.012).exp();
        tone + 0.3 * tail(t, 0.003, 0.0005) * noise.next()
    })
}

fn jump() -> Vec<f64> {
    let length = 0.16;
    let mut phase = 0.0;
    effect(length, |t| {
        let frequency = 320.0 * (760.0f64 / 320.0).powf(t / length);
        let step = frequency / rate();
        let value = 0.25 * pulse(phase, step, 0.5) * tail(t, length, 0.06);
        phase += step;
        value
    })
}

fn land() -> Vec<f64> {
    let mut noise = Noise::new(3);
    let mut low = 0.0;
    effect(0.14, |t| {
        low += 0.15 * (noise.next() - low);
        0.5 * low * (-t / 0.035).exp() + 0.5 * (TAU * 85.0 * t).sin() * (-t / 0.05).exp()
    })
}

fn checkpoint() -> Vec<f64> {
    // C6, E6, G6.
    let notes = [(0.0, midi_hz(84)), (0.08, midi_hz(88)), (0.16, midi_hz(91))];
    let harmonic = 10f64.powf(-12.0 / 20.0);
    effect(0.5, |t| {
        notes
            .iter()
            .filter(|(onset, _)| t >= *onset)
            .map(|&(onset, frequency)| {
                let t = t - onset;
                let level = rise(t, 0.004) * (-t / 0.18).exp();
                level * ((TAU * frequency * t).sin() + harmonic * (TAU * 2.0 * frequency * t).sin())
            })
            .sum()
    })
}

fn fall() -> Vec<f64> {
    let length = 0.6;
    let mut noise = Noise::new(4);
    let mut phase = 0.0;
    effect(length, |t| {
        let frequency = 620.0 * (110.0f64 / 620.0).powf(t / length);
        let value = (triangle(phase) + 0.15 * noise.next()) * (1.0 - t / length);
        phase += frequency / rate();
        value
    })
}

fn finish() -> Vec<f64> {
    let length = 1.4;
    // C5, E5, G5, C6, each held to the end.
    let notes = [
        (0.0, midi_hz(72)),
        (0.10, midi_hz(76)),
        (0.20, midi_hz(79)),
        (0.30, midi_hz(84)),
    ];
    effect(length, |t| {
        let chord: f64 = notes
            .iter()
            .filter(|(onset, _)| t >= *onset)
            .map(|&(onset, frequency)| {
                let t = t - onset;
                let phase = frequency * t;
                rise(t, 0.004)
                    * (0.12 * pulse(phase, frequency / rate(), 0.5) + 0.2 * triangle(phase))
            })
            .sum();
        let tremolo = if t >= 0.4 {
            1.0 - 0.25 * 0.5 * (1.0 - (TAU * 6.0 * (t - 0.4)).cos())
        } else {
            1.0
        };
        chord * tremolo * tail(t, length, 0.5)
    })
}

fn beep() -> Vec<f64> {
    let length = 0.15;
    effect(length, |t| {
        (TAU * 880.0 * t).sin() * rise(t, 0.003) * tail(t, length, 0.04)
    })
}

fn start() -> Vec<f64> {
    effect(0.4, |t| {
        let tone = 0.5 * (TAU * 1760.0 * t).sin() + 0.3 * (TAU * 880.0 * t).sin();
        tone * rise(t, 0.003) * (-t / 0.15).exp()
    })
}

fn write_wav(path: &Path, pcm: &[i16]) -> Result<u64, String> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let fail = |error: hound::Error| format!("{}: {error}", path.display());
    let mut writer = hound::WavWriter::create(path, spec).map_err(fail)?;
    for &sample in pcm {
        writer.write_sample(sample).map_err(fail)?;
    }
    writer.finalize().map_err(fail)?;
    std::fs::metadata(path)
        .map(|meta| meta.len())
        .map_err(|error| format!("{}: {error}", path.display()))
}

fn main() -> Result<(), String> {
    let folder = std::env::args()
        .nth(1)
        .ok_or("uso: create_parkour_sounds <pasta de saída>")?;
    let folder = Path::new(&folder);
    std::fs::create_dir_all(folder).map_err(|error| format!("{}: {error}", folder.display()))?;
    let music = to_pcm(&music());
    let sounds: [(&str, Vec<i16>); 9] = [
        ("musica.wav", music),
        ("clique.wav", to_pcm(&edges(click()))),
        ("pulo.wav", to_pcm(&edges(jump()))),
        ("pouso.wav", to_pcm(&edges(land()))),
        ("checkpoint.wav", to_pcm(&edges(checkpoint()))),
        ("queda.wav", to_pcm(&edges(fall()))),
        ("chegada.wav", to_pcm(&edges(finish()))),
        ("bip.wav", to_pcm(&edges(beep()))),
        ("largada.wav", to_pcm(&edges(start()))),
    ];
    let mut total = 0;
    for (file, pcm) in &sounds {
        let bytes = write_wav(&folder.join(file), pcm)?;
        total += bytes;
        let seconds = pcm.len() as f64 / rate();
        println!("{file:<15} {seconds:>7.3} s {bytes:>9} bytes");
    }
    println!("{:<15} {:>7} {total:>11} bytes", "total", "");
    // The loop point: the step across it next to the steps on each side.
    let loop_pcm = &sounds[0].1;
    let last = loop_pcm.len() - 1;
    let step = |from: usize, to: usize| i32::from(loop_pcm[to]) - i32::from(loop_pcm[from]);
    println!(
        "emenda da música: passos {} | {} | {} (antes | na emenda | depois)",
        step(last - 1, last),
        step(last, 0),
        step(0, 1)
    );
    Ok(())
}
