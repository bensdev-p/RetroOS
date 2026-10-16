//! retro-synth — every RetroOS sound is generated from code at build time
//! (docs/STYLE.md §6). 22.05 kHz, 16-bit mono, gently lo-fi.
//!
//!   retro-synth <out-dir>     writes startup.wav, shutdown.wav, beep.wav, alert.wav,
//!                             error.wav, notify.wav, crumple.wav

use std::f32::consts::TAU;
use std::io::Write;

const RATE: u32 = 22_050;

fn secs(s: f32) -> usize {
    (s * RATE as f32) as usize
}

/// Attack/exponential-decay envelope.
fn env(t: f32, attack: f32, tau: f32) -> f32 {
    if t < 0.0 {
        0.0
    } else if t < attack {
        t / attack
    } else {
        (-(t - attack) / tau).exp()
    }
}

/// Two-operator FM voice: carrier at `f`, modulator at `f * ratio`, index decays.
fn fm(buf: &mut [f32], start: f32, f: f32, ratio: f32, index: f32, tau: f32, amp: f32) {
    let s0 = secs(start);
    for (i, s) in buf.iter_mut().enumerate().skip(s0) {
        let t = (i - s0) as f32 / RATE as f32;
        let e = env(t, 0.008, tau);
        if e < 1e-4 && t > 0.1 {
            break;
        }
        let idx = index * (-(t) / (tau * 0.45)).exp() + 0.15;
        let m = (TAU * f * ratio * t).sin() * idx;
        *s += amp * e * (TAU * f * t + m).sin();
    }
}

fn square(buf: &mut [f32], start: f32, dur: f32, f: f32, amp: f32) {
    let (s0, n) = (secs(start), secs(dur));
    let fade = secs(0.005).max(1);
    for i in 0..n.min(buf.len().saturating_sub(s0)) {
        let t = i as f32 / RATE as f32;
        let ph = (f * t).fract();
        let edge = (i.min(n - i) as f32 / fade as f32).min(1.0);
        buf[s0 + i] += amp * edge * if ph < 0.5 { 1.0 } else { -1.0 };
    }
}

fn sine_blip(buf: &mut [f32], start: f32, dur: f32, f: f32, amp: f32) {
    let (s0, n) = (secs(start), secs(dur));
    for i in 0..n.min(buf.len().saturating_sub(s0)) {
        let t = i as f32 / RATE as f32;
        let e = (std::f32::consts::PI * i as f32 / n as f32).sin();
        buf[s0 + i] += amp * e * (TAU * f * t).sin();
    }
}

/// Deterministic noise (xorshift), so builds are reproducible.
struct Noise(u32);
impl Noise {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

/// Lo-fi finishing: one-pole low-pass, normalise to -3 dBFS, 12-bit quantise.
fn finish(mut buf: Vec<f32>, cutoff: f32) -> Vec<i16> {
    let a = 1.0 - (-TAU * cutoff / RATE as f32).exp();
    let mut y = 0.0;
    for s in buf.iter_mut() {
        y += a * (*s - y);
        *s = y;
    }
    let peak = buf.iter().fold(0.0f32, |m, s| m.max(s.abs())).max(1e-6);
    let gain = 0.707 / peak;
    buf.iter()
        .map(|s| {
            let v = (s * gain * 32767.0) as i32;
            ((v >> 4) << 4).clamp(-32768, 32767) as i16
        })
        .collect()
}

fn wav(path: &std::path::Path, samples: &[i16]) -> std::io::Result<()> {
    let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
    let data_len = (samples.len() * 2) as u32;
    f.write_all(b"RIFF")?;
    f.write_all(&(36 + data_len).to_le_bytes())?;
    f.write_all(b"WAVEfmt ")?;
    f.write_all(&16u32.to_le_bytes())?;
    f.write_all(&1u16.to_le_bytes())?; // PCM
    f.write_all(&1u16.to_le_bytes())?; // mono
    f.write_all(&RATE.to_le_bytes())?;
    f.write_all(&(RATE * 2).to_le_bytes())?;
    f.write_all(&2u16.to_le_bytes())?;
    f.write_all(&16u16.to_le_bytes())?;
    f.write_all(b"data")?;
    f.write_all(&data_len.to_le_bytes())?;
    for s in samples {
        f.write_all(&s.to_le_bytes())?;
    }
    f.flush()
}

/// The RetroOS startup chime: a warm FM-bell C-major add9 chord, lightly strummed.
fn startup() -> Vec<i16> {
    let mut b = vec![0.0; secs(2.2)];
    for (i, f) in [261.63f32, 329.63, 392.00, 587.33].iter().enumerate() {
        let start = i as f32 * 0.028;
        fm(&mut b, start, *f, 1.0, 1.8, 0.75, 1.0);
        fm(&mut b, start, *f * 2.0, 3.5, 0.6, 0.35, 0.18); // bell shimmer
    }
    fm(&mut b, 0.0, 130.81, 1.0, 1.0, 0.9, 0.5); // C3 body
    finish(b, 5200.0)
}

fn shutdown() -> Vec<i16> {
    let mut b = vec![0.0; secs(1.3)];
    for (start, f) in [(0.0f32, 392.00f32), (0.32, 261.63)] {
        fm(&mut b, start, f, 1.0, 1.4, 0.45, 1.0);
        fm(&mut b, start, f / 2.0, 1.0, 0.6, 0.5, 0.4);
    }
    finish(b, 4500.0)
}

fn beep() -> Vec<i16> {
    let mut b = vec![0.0; secs(0.13)];
    square(&mut b, 0.0, 0.12, 880.0, 0.6);
    finish(b, 9000.0)
}

fn alert() -> Vec<i16> {
    let mut b = vec![0.0; secs(0.2)];
    sine_blip(&mut b, 0.0, 0.07, 659.25, 1.0);
    sine_blip(&mut b, 0.07, 0.11, 880.0, 1.0);
    finish(b, 7000.0)
}

fn error() -> Vec<i16> {
    let mut b = vec![0.0; secs(0.27)];
    square(&mut b, 0.0, 0.25, 110.0, 0.5);
    square(&mut b, 0.0, 0.25, 111.6, 0.4);
    for (i, s) in b.iter_mut().enumerate() {
        *s *= env(i as f32 / RATE as f32, 0.004, 0.18);
    }
    finish(b, 2400.0)
}

fn notify() -> Vec<i16> {
    let mut b = vec![0.0; secs(0.22)];
    for (i, f) in [1046.5f32, 1318.5, 1568.0].iter().enumerate() {
        sine_blip(&mut b, i as f32 * 0.065, 0.04, *f, 0.9);
    }
    finish(b, 8000.0)
}

fn crumple() -> Vec<i16> {
    let mut b = vec![0.0; secs(0.42)];
    let mut n = Noise(0x5EED_1993);
    let mut burst = 0.0f32;
    for (i, s) in b.iter_mut().enumerate() {
        if i % secs(0.018) == 0 {
            burst = n.next().abs();
        }
        let t = i as f32 / RATE as f32;
        *s = n.next() * burst * env(t, 0.01, 0.16);
    }
    finish(b, 3800.0)
}

fn main() {
    let Some(out) = std::env::args().nth(1) else {
        eprintln!("usage: retro-synth <out-dir>");
        std::process::exit(2);
    };
    let out = std::path::PathBuf::from(out);
    std::fs::create_dir_all(&out).expect("create output dir");
    type Patch = fn() -> Vec<i16>;
    let sounds: [(&str, Patch); 7] = [
        ("startup", startup),
        ("shutdown", shutdown),
        ("beep", beep),
        ("alert", alert),
        ("error", error),
        ("notify", notify),
        ("crumple", crumple),
    ];
    for (name, f) in sounds {
        let s = f();
        let p = out.join(format!("{name}.wav"));
        wav(&p, &s).expect("write wav");
        eprintln!(
            "{:<9} {:>5} ms -> {}",
            name,
            s.len() as u32 * 1000 / RATE,
            p.display()
        );
    }
}
