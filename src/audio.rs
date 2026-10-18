//! Decoding and real-time mixing. Mixed PCM is streamed into `pacat` processes:
//! one targets the virtual mic sink, the other the default output (monitoring).

use std::collections::HashMap;
use std::io::Write;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
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

/// Interleaved stereo i16 at 48 kHz (half the memory of f32, inaudible difference).
pub type Pcm = Arc<Vec<i16>>;

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

// ---------------------------------------------------------------- mixer

struct Voice {
    key: PathBuf,
    data: Pcm,
    pos: usize,
    gain: f32,
}

struct MixState {
    voices: Vec<Voice>,
    master: f32,
    monitor_on: bool,
    monitor_gain: f32,
}

pub struct Mixer {
    state: Arc<Mutex<MixState>>,
    cache: Mutex<HashMap<PathBuf, Pcm>>,
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
        cmd.args([
            "--playback",
            "--raw",
            "--format=float32le",
            "--channels=2",
            "--latency-msec=20",
            "--client-name=Soundpad",
        ])
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
            if let Some((mut child, _)) = self.child.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    }
}

impl Drop for PacatOut {
    fn drop(&mut self) {
        if let Some((mut child, stdin)) = self.child.take() {
            drop(stdin);
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn f32_bytes(samples: &[f32], out: &mut Vec<u8>) {
    out.clear();
    for s in samples {
        out.extend_from_slice(&s.clamp(-1.0, 1.0).to_le_bytes());
    }
}

impl Mixer {
    pub fn start(mic_sink: &str) -> Arc<Mixer> {
        let state = Arc::new(Mutex::new(MixState {
            voices: Vec::new(),
            master: 1.0,
            monitor_on: true,
            monitor_gain: 0.6,
        }));
        let (mon_tx, mon_rx) = sync_channel::<Vec<u8>>(16);

        let mic_out = PacatOut::new(Some(mic_sink), "Soundpad → Microphone");
        let st = state.clone();
        std::thread::Builder::new()
            .name("mixer".into())
            .spawn(move || mix_loop(st, mic_out, mon_tx))
            .expect("spawn mixer");
        std::thread::Builder::new()
            .name("monitor".into())
            .spawn(move || monitor_loop(mon_rx))
            .expect("spawn monitor");

        Arc::new(Mixer { state, cache: Mutex::new(HashMap::new()) })
    }

    pub fn set_levels(&self, master: f32, monitor_on: bool, monitor_gain: f32) {
        let mut st = self.state.lock().unwrap();
        st.master = master;
        st.monitor_on = monitor_on;
        st.monitor_gain = monitor_gain;
    }

    fn cached(&self, path: &Path) -> Option<Pcm> {
        self.cache.lock().unwrap().get(path).cloned()
    }

    /// Decodes (or takes from cache) without holding the cache lock while decoding.
    pub fn load(&self, path: &Path) -> Result<Pcm, String> {
        if let Some(pcm) = self.cached(path) {
            return Ok(pcm);
        }
        let pcm: Pcm = Arc::new(decode_file(path)?);
        self.cache.lock().unwrap().insert(path.to_path_buf(), pcm.clone());
        Ok(pcm)
    }

    pub fn forget(&self, path: &Path) {
        self.cache.lock().unwrap().remove(path);
    }

    /// Starts playback. Cached sounds start immediately; others are decoded on a worker thread.
    pub fn play(
        self: &Arc<Self>,
        path: PathBuf,
        gain: f32,
        exclusive: bool,
        on_done: impl FnOnce(Result<(), String>) + Send + 'static,
    ) {
        if let Some(pcm) = self.cached(&path) {
            self.start_voice(path, pcm, gain, exclusive);
            on_done(Ok(()));
            return;
        }
        let mixer = self.clone();
        std::thread::spawn(move || match mixer.load(&path) {
            Ok(pcm) => {
                mixer.start_voice(path, pcm, gain, exclusive);
                on_done(Ok(()));
            }
            Err(e) => on_done(Err(e)),
        });
    }

    fn start_voice(&self, key: PathBuf, data: Pcm, gain: f32, exclusive: bool) {
        let mut st = self.state.lock().unwrap();
        if exclusive {
            st.voices.clear();
        }
        st.voices.push(Voice { key, data, pos: 0, gain });
    }

    pub fn stop_all(&self) {
        self.state.lock().unwrap().voices.clear();
    }

    pub fn stop(&self, path: &Path) {
        self.state.lock().unwrap().voices.retain(|v| v.key != path);
    }

    /// Paths currently playing with their progress (0..1).
    pub fn playing(&self) -> Vec<(PathBuf, f32)> {
        let st = self.state.lock().unwrap();
        st.voices
            .iter()
            .map(|v| (v.key.clone(), v.pos as f32 / v.data.len().max(1) as f32))
            .collect()
    }
}

fn mix_loop(state: Arc<Mutex<MixState>>, mut mic_out: PacatOut, mon_tx: SyncSender<Vec<u8>>) {
    let n = BLOCK_FRAMES * CHANNELS;
    let mut mix = vec![0f32; n];
    let mut out = vec![0f32; n];
    let mut bytes = Vec::with_capacity(n * 4);
    loop {
        mix.fill(0.0);
        let (master, monitor_on, monitor_gain) = {
            let mut st = state.lock().unwrap();
            for v in st.voices.iter_mut() {
                let take = (v.data.len() - v.pos).min(n);
                let src = &v.data[v.pos..v.pos + take];
                for (m, &s) in mix.iter_mut().zip(src) {
                    *m += s as f32 * (v.gain / 32768.0);
                }
                v.pos += take;
            }
            st.voices.retain(|v| v.pos < v.data.len());
            (st.master, st.monitor_on, st.monitor_gain)
        };

        for (o, m) in out.iter_mut().zip(&mix) {
            *o = m * master;
        }
        f32_bytes(&out, &mut bytes);
        mic_out.write(&bytes);

        // Monitoring must never stall the mic stream: drop blocks if it falls behind.
        let gain = if monitor_on { monitor_gain } else { 0.0 };
        for (o, m) in out.iter_mut().zip(&mix) {
            *o = m * gain;
        }
        let mut mon_bytes = Vec::with_capacity(n * 4);
        f32_bytes(&out, &mut mon_bytes);
        if let Err(TrySendError::Disconnected(_)) = mon_tx.try_send(mon_bytes) {
            return;
        }
    }
}

fn monitor_loop(rx: Receiver<Vec<u8>>) {
    let mut out = PacatOut::new(None, "Soundpad → Monitor");
    for block in rx {
        out.write(&block);
    }
}
