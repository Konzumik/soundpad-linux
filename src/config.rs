use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Audio files and video containers (only their audio track is played).
pub const AUDIO_EXTS: &[&str] = &[
    "mp3", "wav", "ogg", "oga", "flac", "m4a", "aac", "opus", "wma", "aiff", "aif", "mp4", "m4v",
    "mov", "mkv", "webm", "avi",
];

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Sound {
    pub name: String,
    pub path: PathBuf,
    #[serde(default = "one")]
    pub volume: f32,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Config {
    pub sounds: Vec<Sound>,
    /// Real microphone mixed into the virtual one. None = sounds only.
    pub mic_source: Option<String>,
    /// Volume of sounds sent into the virtual microphone.
    pub master_volume: f32,
    /// Also play sounds to the default output (headphones).
    pub monitor_enabled: bool,
    pub monitor_volume: f32,
    /// Allow several sounds at once; otherwise a new sound stops the previous one.
    pub allow_overlap: bool,
    /// UI language: "en", "ru", or None to follow the system locale.
    pub language: Option<String>,
}

fn one() -> f32 {
    1.0
}

impl Default for Config {
    fn default() -> Self {
        Config {
            sounds: Vec::new(),
            mic_source: None,
            master_volume: 1.0,
            monitor_enabled: true,
            monitor_volume: 0.6,
            allow_overlap: false,
            language: None,
        }
    }
}

pub fn config_dir() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home().join(".config"));
    base.join("soundpad-linux")
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| "/tmp".into())
}

fn config_path() -> PathBuf {
    config_dir().join("config.json")
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
            std::fs::create_dir_all(config_dir())?;
            let tmp = path.with_extension("json.tmp");
            std::fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
            std::fs::rename(tmp, &path)
        })();
        if let Err(e) = res {
            eprintln!("{}", tr!(format!("soundpad: failed to save config: {e}"), format!("soundpad: не удалось сохранить конфиг: {e}")));
        }
    }

    /// Adds files (or every audio file inside given directories). Returns how many were added.
    pub fn add_paths(&mut self, paths: impl IntoIterator<Item = PathBuf>) -> usize {
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
                added += self.add_paths(files);
            } else if path.is_file() {
                if self.sounds.iter().any(|s| s.path == path) {
                    continue;
                }
                let name = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "sound".into());
                self.sounds.push(Sound { name, path, volume: 1.0 });
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
}

fn is_audio(p: &std::path::Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .map(|e| AUDIO_EXTS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}
