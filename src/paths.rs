//! Per-instance names and locations. `SOUNDPAD_INSTANCE=<name>` runs a fully isolated instance
//! (own PA devices, socket and tray id), so a test build can run next to a live one.

use std::path::PathBuf;
use std::sync::OnceLock;

fn suffix() -> &'static str {
    static S: OnceLock<String> = OnceLock::new();
    S.get_or_init(|| {
        std::env::var("SOUNDPAD_INSTANCE")
            .ok()
            .map(|s| s.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>())
            .filter(|s| !s.is_empty())
            .map(|s| format!("_{s}"))
            .unwrap_or_default()
    })
}

fn cached(cell: &'static OnceLock<String>, make: impl FnOnce() -> String) -> &'static str {
    cell.get_or_init(make)
}

pub fn sink_name() -> &'static str {
    static S: OnceLock<String> = OnceLock::new();
    cached(&S, || format!("soundpad{}_sink", suffix()))
}

pub fn source_name() -> &'static str {
    static S: OnceLock<String> = OnceLock::new();
    cached(&S, || format!("soundpad{}_mic", suffix()))
}

pub fn app_id() -> &'static str {
    static S: OnceLock<String> = OnceLock::new();
    cached(&S, || format!("soundpad-linux{}", suffix().replace('_', "-")))
}

pub fn socket_path() -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    dir.join(format!("{}.sock", app_id()))
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| "/tmp".into())
}

fn xdg(var: &str, fallback: &str) -> PathBuf {
    std::env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home().join(fallback))
        .join("soundpad-linux")
}

pub fn config_dir() -> PathBuf {
    xdg("XDG_CONFIG_HOME", ".config")
}

pub fn recordings_dir() -> PathBuf {
    xdg("XDG_DATA_HOME", ".local/share").join("recordings")
}

pub fn log_path() -> PathBuf {
    xdg("XDG_STATE_HOME", ".local/state").join(format!("{}.log", app_id()))
}
