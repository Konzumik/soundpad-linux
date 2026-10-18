#[macro_use]
mod i18n;
mod audio;
mod capture;
mod config;
mod dsp;
mod engine;
mod gui;
mod hotkeys;
mod ipc;
mod paths;
mod spl;
mod tray;
mod vmic;

use std::process::exit;

const USAGE_EN: &str = "\
Soundpad Linux — soundboard and voice changer with a virtual microphone (PipeWire/PulseAudio)

Usage:
  soundpad                 open the window (starts the background service if needed)
  soundpad daemon          run the background service in the foreground (for autostart)
  soundpad play <N|name>   play a sound by number or name
  soundpad stop            stop all sounds
  soundpad list            list sounds
  soundpad voice <effect>  set a voice effect (off, robot, demon, chipmunk, …)
  soundpad replay          save the last seconds of desktop audio as a sound
  soundpad show            open / focus the window
  soundpad quit            stop the background service

Select the \"Soundpad Microphone\" input in Discord / your game / OBS.
Config: ~/.config/soundpad-linux/config.json";

const USAGE_RU: &str = "\
Soundpad Linux — саундпад и изменение голоса с виртуальным микрофоном (PipeWire/PulseAudio)

Использование:
  soundpad                 открыть окно (при необходимости запустит фоновую службу)
  soundpad daemon          запустить фоновую службу в этом терминале (для автозапуска)
  soundpad play <N|имя>    проиграть звук по номеру или имени
  soundpad stop            остановить все звуки
  soundpad list            список звуков
  soundpad voice <эффект>  эффект голоса (off, robot, demon, chipmunk, …)
  soundpad replay          сохранить последние секунды звука с компьютера как звук
  soundpad show            открыть окно / переключиться на него
  soundpad quit            остановить фоновую службу

В Discord/игре/OBS выбери микрофон «Soundpad Microphone».
Конфиг: ~/.config/soundpad-linux/config.json";

fn main() {
    i18n::set_language(None);
    let (cfg, _) = config::Config::load();
    i18n::set_language(cfg.language.as_deref());
    let usage = tr!(USAGE_EN, USAGE_RU);

    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None | Some("gui") => run_gui(),
        Some("daemon") => run_daemon(),
        Some("play") if args.len() > 1 => client(&format!("play {}", args[1..].join(" "))),
        Some("voice") => client(&format!("voice {}", args[1..].join(" "))),
        Some(cmd @ ("stop" | "list" | "replay" | "show" | "quit")) => client(cmd),
        Some("-h" | "--help" | "help") => println!("{usage}"),
        Some("-V" | "--version") => println!("soundpad {}", env!("CARGO_PKG_VERSION")),
        _ => {
            eprintln!("{usage}");
            exit(2);
        }
    }
}

fn client(cmd: &str) {
    match ipc::send(cmd) {
        Ok(resp) => {
            print!("{resp}");
            if resp.starts_with("ERR") {
                exit(1);
            }
        }
        Err(_) => {
            eprintln!("{}", tr!("Soundpad is not running. Start `soundpad`.", "Soundpad не запущен. Запусти `soundpad`."));
            exit(1);
        }
    }
}

fn run_gui() {
    if !ipc::is_running() {
        if let Err(e) = ipc::spawn_daemon() {
            eprintln!("{}", tr!(format!("Failed to start Soundpad: {e}"), format!("Не удалось запустить Soundpad: {e}")));
            exit(1);
        }
    } else if ipc::fetch_state().is_some_and(|s| s.gui_pid.is_some()) {
        engine::focus_window();
        eprintln!("{}", tr!("The Soundpad window is already open.", "Окно Soundpad уже открыто."));
        return;
    }
    if let Err(e) = gui::run() {
        eprintln!("{}", tr!(format!("Window error: {e}"), format!("Ошибка окна: {e}")));
        exit(1);
    }
}

fn run_daemon() {
    if ipc::is_running() {
        eprintln!("{}", tr!("Soundpad is already running.", "Soundpad уже запущен."));
        exit(1);
    }
    let core = engine::Core::start().unwrap_or_else(|e| {
        eprintln!("{}", tr!(format!("Failed to create the virtual microphone: {e}"), format!("Не удалось создать виртуальный микрофон: {e}")));
        exit(1);
    });
    if let Err(e) = ipc::serve(core.clone()) {
        eprintln!("{}", tr!(format!("soundpad: command control unavailable: {e}"), format!("soundpad: управление командами недоступно: {e}")));
    }
    let c = core.clone();
    if let Err(e) = ctrlc::set_handler(move || c.request_quit()) {
        eprintln!("{}", tr!(format!("soundpad: failed to set signal handler: {e}"), format!("soundpad: не удалось установить обработчик сигналов: {e}")));
    }
    println!("{}", tr!("Soundpad is running. Ctrl+C or `soundpad quit` to stop.", "Soundpad запущен. Ctrl+C или `soundpad quit` — выход."));
    core.wait_quit();
    core.shutdown();
    exit(0);
}
