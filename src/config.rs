use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::paths;

/// Audio files and video containers (only their audio track is played).
pub const AUDIO_EXTS: &[&str] = &[
    "mp3", "wav", "ogg", "oga", "flac", "m4a", "aac", "opus", "wma", "aiff", "aif", "mp4", "m4v",
    "mov", "mkv", "webm", "avi",
];

/// A key with modifiers; `key` is a Linux input event code (see input-event-codes.h).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(default)]
pub struct Hotkey {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
    pub key: u16,
}

impl Hotkey {
    pub fn label(&self) -> String {
        let mut s = String::new();
        for (on, name) in [(self.ctrl, "Ctrl+"), (self.alt, "Alt+"), (self.shift, "Shift+"), (self.meta, "Super+")] {
            if on {
                s.push_str(name);
            }
        }
        s + &key_name(self.key)
    }
}

pub fn key_name(code: u16) -> String {
    let raw = format!("{:?}", evdev::KeyCode::new(code));
    match raw.strip_prefix("KEY_") {
        Some(k) => match k.strip_prefix("KP") {
            Some(rest) if !rest.is_empty() => format!("Num{}", title_case(rest)),
            _ => title_case(k),
        },
        None if raw.starts_with("BTN_") => raw,
        None => format!("#{code}"),
    }
}

fn title_case(s: &str) -> String {
    if s.len() <= 3 {
        return s.to_string();
    }
    let mut c = s.chars();
    c.next().map(|f| f.to_string() + &c.as_str().to_lowercase()).unwrap_or_default()
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Sound {
    pub name: String,
    pub path: PathBuf,
    pub volume: f32,
    pub hotkey: Option<Hotkey>,
    pub category: String,
    /// Trim: playback starts at `start_ms` and ends at `end_ms` (0 = end of file).
    pub start_ms: u32,
    pub end_ms: u32,
    pub fade_in_ms: u32,
    pub fade_out_ms: u32,
}

impl Default for Sound {
    fn default() -> Self {
        Sound {
            name: String::new(),
            path: PathBuf::new(),
            volume: 1.0,
            hotkey: None,
            category: String::new(),
            start_ms: 0,
            end_ms: 0,
            fade_in_ms: 0,
            fade_out_ms: 0,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct VoiceConfig {
    /// Effect preset id from `dsp::PRESETS` ("none" = off).
    pub effect: String,
    /// RNNoise noise suppression on the real microphone.
    pub denoise: bool,
    /// Hear your own (processed) voice in the headphones.
    pub hear_myself: bool,
    pub mic_gain: f32,
    /// Hotkeys that toggle an effect preset, keyed by preset id.
    pub hotkeys: BTreeMap<String, Hotkey>,
}

impl Default for VoiceConfig {
    fn default() -> Self {
        VoiceConfig {
            effect: "none".into(),
            denoise: false,
            hear_myself: false,
            mic_gain: 1.0,
            hotkeys: BTreeMap::new(),
        }
    }
}

impl VoiceConfig {
    /// Whether the mic has to go through our own DSP instead of a plain PipeWire loopback.
    pub fn needs_processing(&self) -> bool {
        self.effect != "none" || self.denoise || self.hear_myself || (self.mic_gain - 1.0).abs() > 0.001
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct ReplayConfig {
    /// Continuously keep the last `seconds` of desktop audio so it can be saved as a sound.
    pub enabled: bool,
    pub seconds: u32,
    pub hotkey: Option<Hotkey>,
}

impl Default for ReplayConfig {
    fn default() -> Self {
        ReplayConfig { enabled: false, seconds: 30, hotkey: None }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Config {
    pub sounds: Vec<Sound>,
    /// Real microphone mixed into the virtual one. None = sounds only.
    pub mic_source: Option<String>,
    /// Volume of sounds sent into the virtual microphone.
    pub master_volume: f32,
    /// Also play sounds to an output device (headphones).
    pub monitor_enabled: bool,
    pub monitor_volume: f32,
    /// Output for monitoring; None = system default.
    pub monitor_device: Option<String>,
    /// Allow several sounds at once; otherwise a new sound stops the previous one.
    pub allow_overlap: bool,
    /// Bring all sounds to a similar loudness.
    pub normalize: bool,
    pub hotkeys_enabled: bool,
    pub stop_hotkey: Option<Hotkey>,
    /// Closing the window keeps Soundpad running in the background (tray / hotkeys).
    pub close_to_tray: bool,
    pub voice: VoiceConfig,
    pub replay: ReplayConfig,
    /// UI language: "en", "ru", or None to follow the system locale.
    pub language: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            sounds: Vec::new(),
            mic_source: None,
            master_volume: 1.0,
            monitor_enabled: true,
            monitor_volume: 0.6,
            monitor_device: None,
            allow_overlap: false,
            normalize: false,
            hotkeys_enabled: true,
            stop_hotkey: None,
            close_to_tray: true,
            voice: VoiceConfig::default(),
            replay: ReplayConfig::default(),
            language: None,
        }
    }
}

fn config_path() -> PathBuf {
    paths::config_dir().join("config.json")
}

impl Config {
    /// Returns the config and whether it was freshly created (no file yet).
    pub fn load() -> (Config, bool) {
        match std::fs::read_to_string(config_path()) {
            Ok(text) => match serde_json::from_str(&text) {
                Ok(cfg) => (cfg, false),
                Err(e) => {
                    eprintln!("{}", tr!(format!("soundpad: broken config ({e}), using defaults"), format!("soundpad: битый конфиг ({e}), использую настройки по умолчанию")));
                    (Config::default(), false)
                }
            },
            Err(_) => (Config::default(), true),
        }
    }

    pub fn save(&self) {
        let path = config_path();
        let res = (|| -> std::io::Result<()> {
            std::fs::create_dir_all(paths::config_dir())?;
            let tmp = path.with_extension("json.tmp");
            std::fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
            std::fs::rename(tmp, &path)
        })();
        if let Err(e) = res {
            eprintln!("{}", tr!(format!("soundpad: failed to save config: {e}"), format!("soundpad: не удалось сохранить конфиг: {e}")));
        }
    }

    /// Adds files (or every audio file inside given directories) to `category`. Returns how many were added.
    pub fn add_paths(&mut self, paths: impl IntoIterator<Item = PathBuf>, category: &str) -> usize {
        let mut added = 0;
        for path in paths {
            if path.is_dir() {
                let mut files: Vec<PathBuf> = std::fs::read_dir(&path)
                    .into_iter()
                    .flatten()
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| p.is_file() && is_audio(p))
                    .collect();
                files.sort();
                added += self.add_paths(files, category);
            } else if path.is_file() {
                if self.sounds.iter().any(|s| s.path == path) {
                    continue;
                }
                let name = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "sound".into());
                self.sounds.push(Sound { name, path, category: category.to_string(), ..Default::default() });
                added += 1;
            }
        }
        added
    }

    /// Finds a sound by 1-based number or by (case-insensitive) name: exact match first, then substring.
    pub fn find(&self, query: &str) -> Option<usize> {
        let query = query.trim();
        if let Ok(n) = query.parse::<usize>() {
            return (n >= 1 && n <= self.sounds.len()).then(|| n - 1);
        }
        let q = query.to_lowercase();
        self.sounds
            .iter()
            .position(|s| s.name.to_lowercase() == q)
            .or_else(|| self.sounds.iter().position(|s| s.name.to_lowercase().contains(&q)))
    }

    /// Non-empty categories in order of first appearance.
    pub fn categories(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for s in &self.sounds {
            if !s.category.is_empty() && !out.contains(&s.category) {
                out.push(s.category.clone());
            }
        }
        out
    }
}

pub fn is_audio(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .map(|e| AUDIO_EXTS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}
