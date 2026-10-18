//! Control over a Unix socket: one command line in, response text out, then the connection closes.
//! Used by the CLI (hotkeys bound in the compositor: `soundpad play 1`) and by the GUI process.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::engine::Core;
use crate::paths;
use crate::vmic;

/// Snapshot of the daemon for the GUI.
#[derive(Serialize, Deserialize, Clone, Default)]
pub struct State {
    pub config: Config,
    pub playing: Vec<(PathBuf, f32)>,
    pub durations: Vec<(PathBuf, f32)>,
    pub status: String,
    pub hotkey_devices: usize,
    pub hotkeys_denied: bool,
    pub replay_buffered: f32,
    pub gui_pid: Option<u32>,
}

pub fn remove_socket() {
    let _ = std::fs::remove_file(paths::socket_path());
}

/// Sends a command to the running daemon. Err if none is running.
pub fn send(cmd: &str) -> std::io::Result<String> {
    let mut stream = UnixStream::connect(paths::socket_path())?;
    stream.set_read_timeout(Some(Duration::from_secs(15)))?;
    stream.write_all(cmd.as_bytes())?;
    stream.write_all(b"\n")?;
    stream.shutdown(std::net::Shutdown::Write)?;
    let mut resp = String::new();
    stream.read_to_string(&mut resp)?;
    Ok(resp)
}

pub fn is_running() -> bool {
    send("ping").is_ok()
}

pub fn fetch_state() -> Option<State> {
    send("state").ok().and_then(|s| serde_json::from_str(&s).ok())
}

/// Starts the daemon in the background (its own process group, output to the log file)
/// and waits until it answers.
pub fn spawn_daemon() -> Result<(), String> {
    use std::os::unix::process::CommandExt;
    let log = paths::log_path();
    if let Some(dir) = log.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let out = std::fs::File::create(&log).map_err(|e| e.to_string())?;
    let err = out.try_clone().map_err(|e| e.to_string())?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut child = std::process::Command::new(exe)
        .arg("daemon")
        .stdin(std::process::Stdio::null())
        .stdout(out)
        .stderr(err)
        .process_group(0)
        .spawn()
        .map_err(|e| e.to_string())?;
    for _ in 0..60 {
        if is_running() {
            return Ok(());
        }
        if let Ok(Some(status)) = child.try_wait() {
            let tail = std::fs::read_to_string(&log).unwrap_or_default();
            return Err(format!("{status}: {}", tail.trim()));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(tr!(format!("no answer, see {}", log.display()), format!("нет ответа, см. {}", log.display())))
}

pub fn serve(core: Arc<Core>) -> std::io::Result<()> {
    remove_socket();
    let listener = UnixListener::bind(paths::socket_path())?;
    std::thread::Builder::new().name("ipc".into()).spawn(move || {
        for stream in listener.incoming().flatten() {
            let core = core.clone();
            std::thread::spawn(move || handle(core, stream));
        }
    })?;
    Ok(())
}

fn handle(core: Arc<Core>, mut stream: UnixStream) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let mut line = String::new();
    if BufReader::new(&stream).read_line(&mut line).is_err() {
        return;
    }
    let resp = execute(&core, line.trim());
    let _ = stream.write_all(resp.as_bytes());
}

fn json<T: Serialize>(v: &T) -> String {
    serde_json::to_string(v).unwrap_or_else(|e| format!("ERR {e}"))
}

fn ok_or_err(r: Result<String, String>) -> String {
    match r {
        Ok(s) => format!("{s}\n"),
        Err(e) => format!("ERR {e}\n"),
    }
}

fn execute(core: &Arc<Core>, line: &str) -> String {
    let (cmd, arg) = line.split_once(' ').map(|(c, a)| (c, a.trim())).unwrap_or((line, ""));
    match cmd {
        "ping" => "pong\n".into(),
        // --- CLI
        "play" if !arg.is_empty() => ok_or_err(core.play_query(arg).map(|n| format!("▶ {n}"))),
        "stop" => {
            core.stop();
            "⏹\n".into()
        }
        "list" => {
            let cfg = core.config.lock().unwrap();
            cfg.sounds
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    let hk = s.hotkey.map(|h| format!("  [{}]", h.label())).unwrap_or_default();
                    format!("{:>3}  {}{hk}\n", i + 1, s.name)
                })
                .collect()
        }
        "voice" => {
            let id = if arg.is_empty() || arg == "off" { "none" } else { arg };
            if !crate::dsp::PRESETS.iter().any(|p| p.0 == id) {
                let ids: Vec<&str> = crate::dsp::PRESETS.iter().map(|p| p.0).collect();
                return format!("ERR {}: {}\n", tr!("presets", "эффекты"), ids.join(", "));
            }
            core.set_voice_effect(id);
            format!("🎤 {}\n", crate::dsp::preset_label(id))
        }
        "replay" => ok_or_err(core.save_replay()),
        "show" => {
            core.show_gui();
            "OK\n".into()
        }
        "quit" => {
            core.request_quit();
            "OK\n".into()
        }
        // --- GUI
        "state" => json(&core.state()),
        "set-config" => match serde_json::from_str::<Config>(arg) {
            Ok(cfg) => {
                core.update_config(cfg);
                "OK\n".into()
            }
            Err(e) => format!("ERR {e}\n"),
        },
        "play-index" => ok_or_err(
            arg.parse::<usize>().map_err(|e| e.to_string()).and_then(|i| core.play_index(i, false)),
        ),
        "stop-path" => {
            core.mixer.stop(Path::new(arg));
            "OK\n".into()
        }
        "sources" => json(&vmic::list_sources()),
        "sinks" => json(&vmic::list_sinks()),
        "capture-hotkey" => json(&core.capture_hotkey()),
        "gui-hello" => {
            if let Ok(pid) = arg.parse() {
                core.set_gui_pid(pid);
            }
            "OK\n".into()
        }
        "import-spl" => match core.import_spl(Path::new(arg)) {
            Ok(r) => json(&r),
            Err(e) => format!("ERR {e}\n"),
        },
        _ => tr!(format!("ERR unknown command: {line}\n"), format!("ERR неизвестная команда: {line}\n")),
    }
}
