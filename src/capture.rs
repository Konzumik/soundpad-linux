//! Audio capture via `parec`: the real microphone (for voice effects) and the desktop
//! output (for the instant-replay buffer).

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use crate::audio::RATE;

/// A running `parec`; killed on drop (its reader thread then sees EOF and exits).
pub struct Capture {
    child: Child,
}

impl Drop for Capture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Records `device` as f32 at 48 kHz, calling `on_chunk` with `frames * channels` samples at a time.
pub fn start(
    device: &str,
    channels: usize,
    frames: usize,
    stream_name: &str,
    mut on_chunk: impl FnMut(&[f32]) + Send + 'static,
) -> std::io::Result<Capture> {
    let mut child = Command::new("parec")
        .args(["--record", "--raw", "--format=float32le", "--latency-msec=10", "--client-name=Soundpad"])
        .arg(format!("--rate={RATE}"))
        .arg(format!("--channels={channels}"))
        .arg(format!("--device={device}"))
        .arg(format!("--stream-name={stream_name}"))
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut stdout = child.stdout.take().expect("piped stdout");
    std::thread::Builder::new().name("capture".into()).spawn(move || {
        let mut bytes = vec![0u8; frames * channels * 4];
        let mut samples = vec![0f32; frames * channels];
        while stdout.read_exact(&mut bytes).is_ok() {
            for (s, b) in samples.iter_mut().zip(bytes.as_chunks::<4>().0) {
                *s = f32::from_le_bytes(*b);
            }
            on_chunk(&samples);
        }
    })?;
    Ok(Capture { child })
}

/// Ring buffer of the last N seconds of desktop audio (interleaved stereo i16).
pub struct Replay {
    buf: Arc<Mutex<VecDeque<i16>>>,
    _capture: Capture,
}

impl Replay {
    pub fn start(seconds: u32) -> std::io::Result<Replay> {
        let cap = seconds.max(1) as usize * RATE as usize * 2;
        let buf = Arc::new(Mutex::new(VecDeque::with_capacity(cap)));
        let b = buf.clone();
        let capture = start("@DEFAULT_MONITOR@", 2, 960, "Soundpad replay buffer", move |chunk| {
            let mut q = b.lock().unwrap();
            q.extend(chunk.iter().map(|x| (x.clamp(-1.0, 1.0) * 32767.0) as i16));
            if q.len() > cap {
                let excess = q.len() - cap;
                q.drain(..excess);
            }
        })?;
        Ok(Replay { buf, _capture: capture })
    }

    pub fn buffered_seconds(&self) -> f32 {
        self.buf.lock().unwrap().len() as f32 / (RATE as f32 * 2.0)
    }

    /// Writes the buffer to `dir` as a WAV file and returns its path.
    pub fn save(&self, dir: &Path) -> std::io::Result<PathBuf> {
        let data: Vec<i16> = self.buf.lock().unwrap().iter().copied().collect();
        if data.is_empty() {
            return Err(std::io::Error::other(tr!("nothing recorded yet", "ещё ничего не записано")));
        }
        std::fs::create_dir_all(dir)?;
        let path = dir.join(format!("replay-{}.wav", local_timestamp()));
        write_wav(&path, &data, 2)?;
        Ok(path)
    }
}

pub fn write_wav(path: &Path, samples: &[i16], channels: u16) -> std::io::Result<()> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = std::io::BufWriter::new(std::fs::File::create(path)?);
    out.write_all(b"RIFF")?;
    out.write_all(&(36 + data_len).to_le_bytes())?;
    out.write_all(b"WAVEfmt ")?;
    out.write_all(&16u32.to_le_bytes())?;
    out.write_all(&1u16.to_le_bytes())?; // PCM
    out.write_all(&channels.to_le_bytes())?;
    out.write_all(&RATE.to_le_bytes())?;
    out.write_all(&(RATE * channels as u32 * 2).to_le_bytes())?;
    out.write_all(&(channels * 2).to_le_bytes())?;
    out.write_all(&16u16.to_le_bytes())?;
    out.write_all(b"data")?;
    out.write_all(&data_len.to_le_bytes())?;
    for s in samples {
        out.write_all(&s.to_le_bytes())?;
    }
    out.flush()
}

/// "YYYYMMDD-HHMMSS" in local time.
pub fn local_timestamp() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as libc::time_t)
        .unwrap_or(0);
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&now, &mut tm) };
    format!(
        "{:04}{:02}{:02}-{:02}{:02}{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec
    )
}
