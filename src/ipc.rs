//! Control over a Unix socket, so hotkeys can be bound in the compositor
//! (e.g. Hyprland: `bind = , KP_1, exec, soundpad play 1`).
//! Protocol: one command line in, response text out, then the connection closes.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::engine::Core;

pub fn socket_path() -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir());
    dir.join("soundpad-linux.sock")
}

pub fn remove_socket() {
    let _ = std::fs::remove_file(socket_path());
}

/// Sends a command to the running instance. Err if none is running.
pub fn send(cmd: &str) -> std::io::Result<String> {
    let mut stream = UnixStream::connect(socket_path())?;
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

pub fn serve(core: Arc<Core>) -> std::io::Result<()> {
    remove_socket();
    let listener = UnixListener::bind(socket_path())?;
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

fn execute(core: &Arc<Core>, line: &str) -> String {
    let (cmd, arg) = line.split_once(' ').map(|(c, a)| (c, a.trim())).unwrap_or((line, ""));
    match cmd {
        "ping" => "pong\n".into(),
        "play" if !arg.is_empty() => match core.play_query(arg) {
            Ok(name) => format!("▶ {name}\n"),
            Err(e) => format!("ERR {e}\n"),
        },
        "stop" => {
            core.stop();
            "⏹\n".into()
        }
        "list" => {
            let cfg = core.config.lock().unwrap();
            cfg.sounds.iter().enumerate().map(|(i, s)| format!("{:>3}  {}\n", i + 1, s.name)).collect()
        }
        _ => tr!(format!("ERR unknown command: {line}\n"), format!("ERR неизвестная команда: {line}\n")),
    }
}
