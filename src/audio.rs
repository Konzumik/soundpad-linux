//! Decoding and real-time mixing. Mixed PCM is streamed into `pacat` processes:
//! one targets the virtual mic sink, the other the monitoring output (headphones).

use std::collections::{HashMap, VecDeque};
use std::io::Write;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

pub const RATE: u32 = 48_000;
const CHANNELS: usize = 2;
/// 5 ms per block: small enough for snappy start/stop.
const BLOCK_FRAMES: usize = 240;
/// Fade applied when a sound is stopped, to avoid clicks.
const RELEASE_FRAMES: u32 = 480;

/// A decoded sound: interleaved stereo i16 at 48 kHz (half the memory of f32, inaudible difference).
pub struct Sample {
    pub pcm: Vec<i16>,
    /// Gain that brings the sound to a common loudness (used when normalization is on).
    pub norm_gain: f32,
}

impl Sample {
    fn new(pcm: Vec<i16>) -> Sample {
        let norm_gain = loudness_gain(&pcm);
        Sample { pcm, norm_gain }
    }

    pub fn frames(&self) -> usize {
        self.pcm.len() / CHANNELS
    }

    pub fn seconds(&self) -> f32 {
        self.frames() as f32 / RATE as f32
    }
}

/// Gated RMS loudness → gain towards about −20 dBFS RMS, without boosting into clipping.
fn loudness_gain(pcm: &[i16]) -> f32 {
    const GATE: f32 = 0.003; // ≈ −50 dBFS: ignore silence
    let (mut sum, mut n, mut peak) = (0f64, 0usize, 0f32);
    for &s in pcm {
        let x = (s as f32 / 32768.0).abs();
        peak = peak.max(x);
        if x > GATE {
            sum += (x * x) as f64;
            n += 1;
        }
    }
    if n == 0 {
        return 1.0;
    }
    let rms = (sum / n as f64).sqrt() as f32;
    let mut g = (0.1 / rms).clamp(0.25, 4.0);
    if g > 1.0 {
        g = g.min(1.0 / peak.max(1e-6)).max(1.0);
    }
    g
}

// ---------------------------------------------------------------- decoding

pub fn decode_file(path: &Path) -> Result<Vec<i16>, String> {
    match decode_symphonia(path) {
        Ok(pcm) if !pcm.is_empty() => Ok(pcm),
        Ok(_) => decode_ffmpeg(path).map_err(|_| tr!("file is empty", "файл пустой").to_string()),
        // Symphonia lacks e.g. Opus/WMA: fall back to ffmpeg if it is installed.
        Err(e) => decode_ffmpeg(path).map_err(|fe| format!("{e}; ffmpeg: {fe}")),
    }
}

fn decode_symphonia(path: &Path) -> Result<Vec<i16>, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| tr!(format!("unrecognized format: {e}"), format!("формат не распознан: {e}")))?;
    let mut format = probed.format;
    // Video files (mp4, mkv, mov) usually list the video track first: take the first
    // track that has a sample rate (i.e. is audio) and a decoder we support.
    let mut last_err = tr!("no audio track", "нет аудиодорожки").to_string();
    let mut chosen = None;
    for track in format.tracks() {
        let params = &track.codec_params;
        if params.codec == CODEC_TYPE_NULL || params.sample_rate.is_none() {
            continue;
        }
        match symphonia::default::get_codecs().make(params, &DecoderOptions::default()) {
            Ok(dec) => {
                chosen = Some((track.id, params.sample_rate.unwrap(), dec));
                break;
            }
            Err(e) => last_err = tr!(format!("unsupported audio codec: {e}"), format!("аудиокодек не поддерживается: {e}")),
        }
    }
    let (track_id, mut rate, mut decoder) = chosen.ok_or(last_err)?;

    let mut stereo: Vec<f32> = Vec::new();
    let mut sample_buf: Option<SampleBuffer<f32>> = None;
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(SymError::ResetRequired) => break,
            Err(e) => return Err(e.to_string()),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(SymError::DecodeError(_)) => continue,
            Err(e) => return Err(e.to_string()),
        };
        let spec = *decoded.spec();
        rate = spec.rate;
        let chans = spec.channels.count().max(1);
        let buf = match &mut sample_buf {
            Some(b) if b.capacity() >= decoded.capacity() * chans => b,
            _ => sample_buf.insert(SampleBuffer::new(decoded.capacity() as u64, spec)),
        };
        buf.copy_interleaved_ref(decoded);
        for frame in buf.samples().chunks_exact(chans) {
            let (l, r) = if chans == 1 { (frame[0], frame[0]) } else { (frame[0], frame[1]) };
            stereo.push(l);
            stereo.push(r);
        }
    }
    let stereo = if rate != RATE { resample(&stereo, rate) } else { stereo };
    Ok(to_i16(&stereo))
}

fn decode_ffmpeg(path: &Path) -> Result<Vec<i16>, String> {
    let out = Command::new("ffmpeg")
        .args(["-v", "error", "-nostdin", "-i"])
        .arg(path)
        .args(["-f", "s16le", "-ac", "2", "-ar", &RATE.to_string(), "-"])
        .stderr(Stdio::piped())
        .output()
        .map_err(|_| tr!("not installed", "не установлен").to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(out.stdout.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect())
}

/// Linear-interpolation resampler for interleaved stereo.
fn resample(input: &[f32], from: u32) -> Vec<f32> {
    let frames_in = input.len() / CHANNELS;
    if frames_in < 2 {
        return input.to_vec();
    }
    let step = from as f64 / RATE as f64;
    let frames_out = ((frames_in - 1) as f64 / step) as usize;
    let mut out = Vec::with_capacity(frames_out * CHANNELS);
    for i in 0..frames_out {
        let src = i as f64 * step;
        let i0 = src as usize;
        let frac = (src - i0 as f64) as f32;
        let i1 = (i0 + 1).min(frames_in - 1);
        for c in 0..CHANNELS {
            let a = input[i0 * CHANNELS + c];
            let b = input[i1 * CHANNELS + c];
            out.push(a + (b - a) * frac);
        }
    }
    out
}

fn to_i16(samples: &[f32]) -> Vec<i16> {
    samples.iter().map(|s| (s.clamp(-1.0, 1.0) * 32767.0) as i16).collect()
}

// ---------------------------------------------------------------- mic bus

/// Processed microphone audio (mono) handed from the capture thread to the mixer.
pub struct MicBus {
    pub active: AtomicBool,
    ring: Mutex<(VecDeque<f32>, bool)>, // (samples, primed)
}

/// Jitter buffer bounds: start after 20 ms, never let latency grow past 100 ms.
const MIC_PRIME: usize = 960;
const MIC_MAX: usize = 4800;

impl MicBus {
    fn new() -> Self {
        MicBus { active: AtomicBool::new(false), ring: Mutex::new((VecDeque::new(), false)) }
    }

    pub fn push(&self, samples: &[f32]) {
        let mut r = self.ring.lock().unwrap();
        r.0.extend(samples);
        if r.0.len() > MIC_MAX {
            let excess = r.0.len() - MIC_PRIME;
            r.0.drain(..excess);
        }
    }

    pub fn set_active(&self, on: bool) {
        self.active.store(on, Ordering::Relaxed);
        if !on {
            let mut r = self.ring.lock().unwrap();
            r.0.clear();
            r.1 = false;
        }
    }

    /// Fills `out` (zeros on underrun) and returns whether anything was available.
    fn pull(&self, out: &mut [f32]) -> bool {
        out.fill(0.0);
        let mut r = self.ring.lock().unwrap();
        let (buf, primed) = &mut *r;
        if !*primed {
            if buf.len() < MIC_PRIME {
                return false;
            }
            *primed = true;
        }
        let n = out.len().min(buf.len());
        for (o, s) in out.iter_mut().zip(buf.drain(..n)) {
            *o = s;
        }
        if n < out.len() {
            *primed = false;
        }
        n > 0
    }
}

// ---------------------------------------------------------------- mixer

/// What to play and how (trim/fade come from the sound's settings).
pub struct PlayRequest {
    pub path: PathBuf,
    pub gain: f32,
    pub start_ms: u32,
    pub end_ms: u32,
    pub fade_in_ms: u32,
    pub fade_out_ms: u32,
    pub exclusive: bool,
    pub normalize: bool,
}

struct Voice {
    key: PathBuf,
    data: Arc<Sample>,
    frame: usize,
    start: usize,
    end: usize,
    fade_in: usize,
    fade_out: usize,
    gain: f32,
    release: Option<u32>,
}

impl Voice {
    fn finished(&self) -> bool {
        self.frame >= self.end || self.release == Some(0)
    }
}

struct MixState {
    voices: Vec<Voice>,
    master: f32,
    monitor_on: bool,
    monitor_gain: f32,
    mic_gain: f32,
    hear_myself: bool,
}

pub struct Mixer {
    state: Arc<Mutex<MixState>>,
    cache: Mutex<HashMap<PathBuf, Arc<Sample>>>,
    pub mic: Arc<MicBus>,
    monitor_device: Arc<Mutex<(u64, Option<String>)>>,
}

/// A long-running `pacat` fed with raw f32 stereo through a tiny pipe (low latency).
struct PacatOut {
    device: Option<String>,
    stream_name: &'static str,
    child: Option<(Child, ChildStdin)>,
}

impl PacatOut {
    fn new(device: Option<&str>, stream_name: &'static str) -> Self {
        PacatOut { device: device.map(String::from), stream_name, child: None }
    }

    fn spawn(&self) -> std::io::Result<(Child, ChildStdin)> {
        let mut cmd = Command::new("pacat");
        cmd.args(["--playback", "--raw", "--format=float32le", "--channels=2", "--latency-msec=20", "--client-name=Soundpad"])
            .arg(format!("--rate={RATE}"))
            .arg(format!("--stream-name={}", self.stream_name))
            .stdin(Stdio::piped())
            .stdout(Stdio::null());
        if let Some(dev) = &self.device {
            cmd.arg(format!("--device={dev}"));
        }
        let mut child = cmd.spawn()?;
        let stdin = child.stdin.take().expect("piped stdin");
        // Default pipe buffer is 64 KiB (~170 ms of audio); shrink it to one page (~10 ms).
        unsafe {
            libc::fcntl(stdin.as_raw_fd(), libc::F_SETPIPE_SZ, 4096);
        }
        Ok((child, stdin))
    }

    /// Blocks until pacat accepts the data, which paces the mixer in real time.
    fn write(&mut self, bytes: &[u8]) {
        if self.child.is_none() {
            match self.spawn() {
                Ok(c) => self.child = Some(c),
                Err(e) => {
                    eprintln!("{}", tr!(format!("soundpad: failed to start pacat: {e}"), format!("soundpad: не удалось запустить pacat: {e}")));
                    std::thread::sleep(Duration::from_secs(1));
                    return;
                }
            }
        }
        let (_, stdin) = self.child.as_mut().unwrap();
        if stdin.write_all(bytes).is_err() {
            let name = self.stream_name;
            eprintln!("{}", tr!(format!("soundpad: pacat ({name}) exited, restarting"), format!("soundpad: pacat ({name}) завершился, перезапускаю")));
            self.kill();
            std::thread::sleep(Duration::from_millis(500));
        }
    }

    fn kill(&mut self) {
        if let Some((mut child, stdin)) = self.child.take() {
            drop(stdin);
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for PacatOut {
    fn drop(&mut self) {
        self.kill();
    }
}

/// Transparent below 0.8, smoothly saturates above: no harsh clipping when sounds overlap.
fn soft_clip(x: f32) -> f32 {
    let a = x.abs();
    if a <= 0.8 {
        x
    } else {
        x.signum() * (0.8 + 0.2 * ((a - 0.8) / 0.2).tanh())
    }
}

fn f32_bytes(samples: &[f32], out: &mut Vec<u8>) {
    out.clear();
    for s in samples {
        out.extend_from_slice(&soft_clip(*s).to_le_bytes());
    }
}

fn ms_to_frames(ms: u32) -> usize {
    ms as usize * RATE as usize / 1000
}

impl Mixer {
    pub fn start(mic_sink: &str) -> Arc<Mixer> {
        let state = Arc::new(Mutex::new(MixState {
            voices: Vec::new(),
            master: 1.0,
            monitor_on: true,
            monitor_gain: 0.6,
            mic_gain: 1.0,
            hear_myself: false,
        }));
        let mic = Arc::new(MicBus::new());
        let monitor_device = Arc::new(Mutex::new((0, None)));
        let (mon_tx, mon_rx) = sync_channel::<Vec<u8>>(16);

        let mic_out = PacatOut::new(Some(mic_sink), "Soundpad → Microphone");
        let (st, bus) = (state.clone(), mic.clone());
        std::thread::Builder::new()
            .name("mixer".into())
            .spawn(move || mix_loop(st, bus, mic_out, mon_tx))
            .expect("spawn mixer");
        let dev = monitor_device.clone();
        std::thread::Builder::new()
            .name("monitor".into())
            .spawn(move || monitor_loop(mon_rx, dev))
            .expect("spawn monitor");

        Arc::new(Mixer { state, cache: Mutex::new(HashMap::new()), mic, monitor_device })
    }

    pub fn set_levels(&self, master: f32, monitor_on: bool, monitor_gain: f32, mic_gain: f32, hear_myself: bool) {
        let mut st = self.state.lock().unwrap();
        st.master = master;
        st.monitor_on = monitor_on;
        st.monitor_gain = monitor_gain;
        st.mic_gain = mic_gain;
        st.hear_myself = hear_myself;
    }

    pub fn set_monitor_device(&self, device: Option<String>) {
        let mut d = self.monitor_device.lock().unwrap();
        if d.1 != device {
            d.0 += 1;
            d.1 = device;
        }
    }

    fn cached(&self, path: &Path) -> Option<Arc<Sample>> {
        self.cache.lock().unwrap().get(path).cloned()
    }

    /// Decodes (or takes from cache) without holding the cache lock while decoding.
    pub fn load(&self, path: &Path) -> Result<Arc<Sample>, String> {
        if let Some(s) = self.cached(path) {
            return Ok(s);
        }
        let sample = Arc::new(Sample::new(decode_file(path)?));
        self.cache.lock().unwrap().insert(path.to_path_buf(), sample.clone());
        Ok(sample)
    }

    pub fn forget(&self, path: &Path) {
        self.cache.lock().unwrap().remove(path);
    }

    /// Durations (seconds) of every decoded sound.
    pub fn durations(&self) -> Vec<(PathBuf, f32)> {
        self.cache.lock().unwrap().iter().map(|(p, s)| (p.clone(), s.seconds())).collect()
    }

    /// Starts playback. Cached sounds start immediately; others are decoded on a worker thread.
    pub fn play(self: &Arc<Self>, req: PlayRequest, on_done: impl FnOnce(Result<(), String>) + Send + 'static) {
        if let Some(sample) = self.cached(&req.path) {
            self.start_voice(req, sample);
            on_done(Ok(()));
            return;
        }
        let mixer = self.clone();
        std::thread::spawn(move || match mixer.load(&req.path) {
            Ok(sample) => {
                mixer.start_voice(req, sample);
                on_done(Ok(()));
            }
            Err(e) => on_done(Err(e)),
        });
    }

    fn start_voice(&self, req: PlayRequest, data: Arc<Sample>) {
        let total = data.frames();
        let start = ms_to_frames(req.start_ms).min(total);
        let end = if req.end_ms == 0 { total } else { ms_to_frames(req.end_ms).clamp(start, total) };
        let gain = req.gain * if req.normalize { data.norm_gain } else { 1.0 };
        let voice = Voice {
            key: req.path,
            data,
            frame: start,
            start,
            end,
            fade_in: ms_to_frames(req.fade_in_ms),
            fade_out: ms_to_frames(req.fade_out_ms),
            gain,
            release: None,
        };
        let mut st = self.state.lock().unwrap();
        if req.exclusive {
            for v in st.voices.iter_mut() {
                v.release.get_or_insert(RELEASE_FRAMES);
            }
        }
        st.voices.push(voice);
    }

    pub fn stop_all(&self) {
        for v in self.state.lock().unwrap().voices.iter_mut() {
            v.release.get_or_insert(RELEASE_FRAMES);
        }
    }

    pub fn stop(&self, path: &Path) {
        for v in self.state.lock().unwrap().voices.iter_mut().filter(|v| v.key == path) {
            v.release.get_or_insert(RELEASE_FRAMES);
        }
    }

    /// Paths currently playing with their progress (0..1) within the trimmed range.
    pub fn playing(&self) -> Vec<(PathBuf, f32)> {
        let st = self.state.lock().unwrap();
        st.voices
            .iter()
            .filter(|v| v.release.is_none())
            .map(|v| (v.key.clone(), (v.frame - v.start) as f32 / (v.end - v.start).max(1) as f32))
            .collect()
    }
}

fn mix_loop(state: Arc<Mutex<MixState>>, mic: Arc<MicBus>, mut mic_out: PacatOut, mon_tx: SyncSender<Vec<u8>>) {
    let n = BLOCK_FRAMES * CHANNELS;
    let mut mix = vec![0f32; n];
    let mut out = vec![0f32; n];
    let mut voice_in = vec![0f32; BLOCK_FRAMES];
    let mut bytes = Vec::with_capacity(n * 4);
    loop {
        mix.fill(0.0);
        let (master, monitor_on, monitor_gain, mic_gain, hear_myself) = {
            let mut st = state.lock().unwrap();
            for v in st.voices.iter_mut() {
                for f in 0..BLOCK_FRAMES {
                    if v.finished() {
                        break;
                    }
                    let mut g = v.gain / 32768.0;
                    let pos = v.frame - v.start;
                    if pos < v.fade_in {
                        g *= pos as f32 / v.fade_in as f32;
                    }
                    let left = v.end - v.frame;
                    if left < v.fade_out {
                        g *= left as f32 / v.fade_out as f32;
                    }
                    if let Some(r) = &mut v.release {
                        g *= *r as f32 / RELEASE_FRAMES as f32;
                        *r -= 1;
                    }
                    let i = v.frame * CHANNELS;
                    mix[f * 2] += v.data.pcm[i] as f32 * g;
                    mix[f * 2 + 1] += v.data.pcm[i + 1] as f32 * g;
                    v.frame += 1;
                }
            }
            st.voices.retain(|v| !v.finished());
            (st.master, st.monitor_on, st.monitor_gain, st.mic_gain, st.hear_myself)
        };
        let has_voice = mic.active.load(Ordering::Relaxed) && mic.pull(&mut voice_in);

        for (f, o) in out.chunks_exact_mut(2).enumerate() {
            let v = if has_voice { voice_in[f] * mic_gain } else { 0.0 };
            o[0] = mix[f * 2] * master + v;
            o[1] = mix[f * 2 + 1] * master + v;
        }
        f32_bytes(&out, &mut bytes);
        mic_out.write(&bytes);

        // Monitoring must never stall the mic stream: drop blocks if it falls behind.
        let gain = if monitor_on { monitor_gain } else { 0.0 };
        for (f, o) in out.chunks_exact_mut(2).enumerate() {
            let v = if has_voice && hear_myself { voice_in[f] * mic_gain } else { 0.0 };
            o[0] = mix[f * 2] * gain + v;
            o[1] = mix[f * 2 + 1] * gain + v;
        }
        let mut mon_bytes = Vec::with_capacity(n * 4);
        f32_bytes(&out, &mut mon_bytes);
        if let Err(TrySendError::Disconnected(_)) = mon_tx.try_send(mon_bytes) {
            return;
        }
    }
}

fn monitor_loop(rx: Receiver<Vec<u8>>, device: Arc<Mutex<(u64, Option<String>)>>) {
    let mut generation = 0;
    let mut out = PacatOut::new(None, "Soundpad → Monitor");
    for block in rx {
        let (gen, dev) = device.lock().unwrap().clone();
        if gen != generation {
            generation = gen;
            out = PacatOut::new(dev.as_deref(), "Soundpad → Monitor");
        }
        out.write(&block);
    }
}
