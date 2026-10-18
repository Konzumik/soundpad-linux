//! Voice processing for the real microphone: RNNoise noise suppression and effect presets.
//! Everything runs on mono f32 at 48 kHz in 10 ms (480-sample) frames.

use std::f32::consts::PI;

use nnnoiseless::DenoiseState;

const FS: f32 = 48_000.0;
pub const FRAME: usize = DenoiseState::FRAME_SIZE; // 480

/// (id, English label, Russian label)
pub const PRESETS: &[(&str, &str, &str)] = &[
    ("none", "No effect", "Без эффекта"),
    ("chipmunk", "Chipmunk", "Бурундук"),
    ("deep", "Deep voice", "Низкий голос"),
    ("demon", "Demon", "Демон"),
    ("robot", "Robot", "Робот"),
    ("alien", "Alien", "Пришелец"),
    ("radio", "Radio", "Рация"),
    ("echo", "Echo", "Эхо"),
    ("cave", "Cave", "Пещера"),
];

pub fn preset_label(id: &str) -> String {
    PRESETS
        .iter()
        .find(|p| p.0 == id)
        .map(|p| tr!(p.1, p.2).to_string())
        .unwrap_or_else(|| id.to_string())
}

#[derive(Default)]
struct Params {
    pitch_semitones: f32,
    ring_hz: f32,
    ring_mix: f32,
    drive: f32,
    band: Option<(f32, f32)>,
    echo_ms: f32,
    echo_feedback: f32,
    echo_mix: f32,
    reverb_mix: f32,
    gain: f32,
}

fn params(id: &str) -> Params {
    let p = Params { gain: 1.0, ..Default::default() };
    match id {
        "chipmunk" => Params { pitch_semitones: 8.0, ..p },
        "deep" => Params { pitch_semitones: -4.0, gain: 1.1, ..p },
        "demon" => Params { pitch_semitones: -7.0, drive: 2.5, reverb_mix: 0.25, gain: 1.2, ..p },
        "robot" => Params { ring_hz: 70.0, ring_mix: 1.0, echo_ms: 12.0, echo_feedback: 0.55, echo_mix: 0.5, gain: 1.3, ..p },
        "alien" => Params { pitch_semitones: 4.0, ring_hz: 420.0, ring_mix: 0.5, ..p },
        "radio" => Params { band: Some((500.0, 2800.0)), drive: 3.0, gain: 1.4, ..p },
        "echo" => Params { echo_ms: 320.0, echo_feedback: 0.45, echo_mix: 0.5, ..p },
        "cave" => Params { reverb_mix: 0.5, ..p },
        _ => p,
    }
}

// ---------------------------------------------------------------- building blocks

/// Delay-line pitch shifter: two read taps sweep through a 30 ms window with
/// complementary sin² crossfades, so the output pitch is scaled by `ratio`.
struct PitchShift {
    buf: Vec<f32>,
    w: usize,
    phase: f32,
    step: f32,
    window: f32,
}

const PS_LEN: usize = 4096;

impl PitchShift {
    fn new(semitones: f32) -> Self {
        let ratio = 2f32.powf(semitones / 12.0);
        let window = 0.030 * FS;
        PitchShift { buf: vec![0.0; PS_LEN], w: 0, phase: 0.0, step: (1.0 - ratio) / window, window }
    }

    fn tap(&self, phase: f32) -> f32 {
        let pos = (self.w as f32 - 2.0 - phase * self.window).rem_euclid(PS_LEN as f32);
        let i = pos as usize;
        let frac = pos - i as f32;
        let a = self.buf[i % PS_LEN];
        let b = self.buf[(i + 1) % PS_LEN];
        a + (b - a) * frac
    }

    fn process(&mut self, x: f32) -> f32 {
        self.buf[self.w] = x;
        self.phase = (self.phase + self.step).rem_euclid(1.0);
        let p2 = (self.phase + 0.5) % 1.0;
        let g1 = (PI * self.phase).sin().powi(2);
        let g2 = (PI * p2).sin().powi(2);
        let y = self.tap(self.phase) * g1 + self.tap(p2) * g2;
        self.w = (self.w + 1) % PS_LEN;
        y
    }
}

/// RBJ biquad.
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl Biquad {
    fn new(highpass: bool, freq: f32, q: f32) -> Self {
        let w0 = 2.0 * PI * freq / FS;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * q);
        let a0 = 1.0 + alpha;
        let (b0, b1, b2) = if highpass {
            ((1.0 + cos) / 2.0, -(1.0 + cos), (1.0 + cos) / 2.0)
        } else {
            ((1.0 - cos) / 2.0, 1.0 - cos, (1.0 - cos) / 2.0)
        };
        Biquad {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: -2.0 * cos / a0,
            a2: (1.0 - alpha) / a0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }
}

struct Echo {
    buf: Vec<f32>,
    i: usize,
    feedback: f32,
    mix: f32,
}

impl Echo {
    fn new(ms: f32, feedback: f32, mix: f32) -> Self {
        let len = ((ms / 1000.0 * FS) as usize).max(1);
        Echo { buf: vec![0.0; len], i: 0, feedback, mix }
    }

    fn process(&mut self, x: f32) -> f32 {
        let delayed = self.buf[self.i];
        self.buf[self.i] = x + delayed * self.feedback;
        self.i = (self.i + 1) % self.buf.len();
        x + delayed * self.mix
    }
}

/// Freeverb-style reverb: parallel damped combs followed by series allpasses.
struct Reverb {
    combs: Vec<(Vec<f32>, usize, f32)>,
    allpasses: Vec<(Vec<f32>, usize)>,
    mix: f32,
}

impl Reverb {
    fn new(mix: f32) -> Self {
        let scale = FS / 44_100.0;
        let len = |n: usize| (n as f32 * scale) as usize;
        Reverb {
            combs: [1557, 1617, 1491, 1422, 1277, 1356].iter().map(|&n| (vec![0.0; len(n)], 0, 0.0)).collect(),
            allpasses: [556, 441, 341, 225].iter().map(|&n| (vec![0.0; len(n)], 0)).collect(),
            mix,
        }
    }

    fn process(&mut self, x: f32) -> f32 {
        const FEEDBACK: f32 = 0.82;
        const DAMP: f32 = 0.3;
        let input = x * 0.3;
        let mut wet = 0.0;
        for (buf, i, filt) in &mut self.combs {
            let out = buf[*i];
            *filt = out * (1.0 - DAMP) + *filt * DAMP;
            buf[*i] = input + *filt * FEEDBACK;
            *i = (*i + 1) % buf.len();
            wet += out;
        }
        for (buf, i) in &mut self.allpasses {
            let b = buf[*i];
            buf[*i] = wet + b * 0.5;
            wet = b - wet;
            *i = (*i + 1) % buf.len();
        }
        x * (1.0 - self.mix) + wet * self.mix
    }
}

// ---------------------------------------------------------------- chain

struct Chain {
    pitch: Option<PitchShift>,
    ring: Option<(f32, f32, f32)>, // (phase increment, mix, phase)
    drive: f32,
    band: Option<(Biquad, Biquad)>,
    echo: Option<Echo>,
    reverb: Option<Reverb>,
    gain: f32,
}

impl Chain {
    fn new(id: &str) -> Self {
        let p = params(id);
        Chain {
            pitch: (p.pitch_semitones != 0.0).then(|| PitchShift::new(p.pitch_semitones)),
            ring: (p.ring_mix > 0.0).then_some((p.ring_hz / FS, p.ring_mix, 0.0)),
            drive: p.drive,
            band: p.band.map(|(lo, hi)| (Biquad::new(true, lo, 0.707), Biquad::new(false, hi, 0.707))),
            echo: (p.echo_mix > 0.0).then(|| Echo::new(p.echo_ms, p.echo_feedback, p.echo_mix)),
            reverb: (p.reverb_mix > 0.0).then(|| Reverb::new(p.reverb_mix)),
            gain: p.gain,
        }
    }

    fn process(&mut self, mut x: f32) -> f32 {
        if let Some(ps) = &mut self.pitch {
            x = ps.process(x);
        }
        if let Some((inc, mix, phase)) = &mut self.ring {
            *phase = (*phase + *inc) % 1.0;
            x = x * (1.0 - *mix) + x * (2.0 * PI * *phase).sin() * *mix;
        }
        if self.drive > 0.0 {
            x = (x * self.drive).tanh() / self.drive.tanh();
        }
        if let Some((hp, lp)) = &mut self.band {
            x = lp.process(hp.process(x));
        }
        if let Some(e) = &mut self.echo {
            x = e.process(x);
        }
        if let Some(r) = &mut self.reverb {
            x = r.process(x);
        }
        x * self.gain
    }
}

pub struct VoiceProcessor {
    effect: String,
    chain: Chain,
    denoise: Option<Box<DenoiseState<'static>>>,
    scratch: Vec<f32>,
}

impl VoiceProcessor {
    pub fn new() -> Self {
        VoiceProcessor { effect: "none".into(), chain: Chain::new("none"), denoise: None, scratch: vec![0.0; FRAME] }
    }

    pub fn configure(&mut self, effect: &str, denoise: bool) {
        if effect != self.effect {
            self.effect = effect.to_string();
            self.chain = Chain::new(effect);
        }
        if denoise != self.denoise.is_some() {
            self.denoise = denoise.then(DenoiseState::new);
        }
    }

    /// Processes one frame of exactly `FRAME` samples in place.
    pub fn process(&mut self, frame: &mut [f32]) {
        if let Some(dn) = &mut self.denoise {
            // RNNoise works on 16-bit-scaled floats.
            for (s, x) in self.scratch.iter_mut().zip(frame.iter()) {
                *s = x * 32768.0;
            }
            dn.process_frame(frame, &self.scratch);
            for x in frame.iter_mut() {
                *x /= 32768.0;
            }
        }
        if self.effect != "none" {
            for x in frame.iter_mut() {
                *x = self.chain.process(*x);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dominant frequency via zero crossings of a steady sine.
    fn freq_of(sig: &[f32]) -> f32 {
        let crossings = sig.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
        crossings as f32 / (sig.len() as f32 / FS)
    }

    #[test]
    fn pitch_shift_moves_frequency() {
        for (semi, expect) in [(12.0, 880.0), (-12.0, 220.0), (7.0, 659.0)] {
            let mut ps = PitchShift::new(semi);
            let out: Vec<f32> = (0..48_000).map(|i| ps.process((2.0 * PI * 440.0 * i as f32 / FS).sin())).collect();
            let f = freq_of(&out[4800..]);
            assert!((f - expect).abs() / expect < 0.08, "{semi} st: got {f} Hz, want {expect}");
        }
    }

    #[test]
    fn every_preset_is_stable() {
        for (id, ..) in PRESETS {
            let mut vp = VoiceProcessor::new();
            vp.configure(id, true);
            let mut peak = 0f32;
            for n in 0..200 {
                let mut frame: Vec<f32> = (0..FRAME).map(|i| 0.5 * (2.0 * PI * 300.0 * (n * FRAME + i) as f32 / FS).sin()).collect();
                vp.process(&mut frame);
                peak = frame.iter().fold(peak, |m, x| m.max(x.abs()));
                assert!(frame.iter().all(|x| x.is_finite()), "{id} produced NaN");
            }
            assert!(peak < 4.0, "{id} blew up: peak {peak}");
        }
    }
}
