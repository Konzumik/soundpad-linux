#[macro_use]
mod i18n;
mod audio;
mod config;
mod engine;
mod gui;
mod ipc;
mod vmic;

use std::process::exit;

const USAGE_EN: &str = "\
Soundpad Linux — soundboard with a virtual microphone for PipeWire/PulseAudio

Usage:
  soundpad                 start with the window
  soundpad daemon          start without a window (control via commands only)
  soundpad play <N|name>   play a sound by number or name
  soundpad stop            stop all sounds
  soundpad list            list sounds

Select the \"Soundpad Microphone\" input in Discord / your game / OBS.
Config: ~/.config/soundpad-linux/config.json";

const USAGE_RU: &str = "\
Soundpad Linux — саундпад с виртуальным микрофоном для PipeWire/PulseAudio

Использование:
  soundpad                 запустить с окном
  soundpad daemon          запустить без окна (управление только командами)
  soundpad play <N|имя>    проиграть звук по номеру или имени
  soundpad stop            остановить все звуки
  soundpad list            список звуков

В Discord/игре/OBS выбери микрофон «Soundpad Microphone».
Конфиг: ~/.config/soundpad-linux/config.json";

fn main() {
    i18n::set_language(None);
    let (cfg, _) = config::Config::load();
    i18n::set_language(cfg.language.as_deref());
    let usage = tr!(USAGE_EN, USAGE_RU);

    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None | Some("gui") => run(true),
        Some("daemon") => run(false),
        Some("play") if args.len() > 1 => client(&format!("play {}", args[1..].join(" "))),
        Some(cmd @ ("stop" | "list")) => client(cmd),
        Some("-h" | "--help" | "help") => println!("{usage}"),
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
            eprintln!(
                "{}",
                tr!(
                    "Soundpad is not running. Start `soundpad` or `soundpad daemon`.",
                    "Soundpad не запущен. Запусти `soundpad` или `soundpad daemon`."
                )
            );
            exit(1);
        }
    }
}

fn run(with_gui: bool) {
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
    if let Err(e) = ctrlc::set_handler(move || {
        c.shutdown();
        exit(0);
    }) {
        eprintln!("{}", tr!(format!("soundpad: failed to set signal handler: {e}"), format!("soundpad: не удалось установить обработчик сигналов: {e}")));
    }

    if with_gui {
        if let Err(e) = gui::run(core.clone()) {
            eprintln!("{}", tr!(format!("Window error: {e}"), format!("Ошибка окна: {e}")));
        }
    } else {
        println!("{}", tr!("Soundpad is running without a window. Ctrl+C to quit.", "Soundpad запущен без окна. Ctrl+C — выход."));
        loop {
            std::thread::park();
        }
    }
    core.shutdown();
}
