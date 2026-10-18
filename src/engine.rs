//! The daemon: owns the virtual mic, mixer, mic processing, replay buffer, hotkeys and tray.
//! The GUI is a separate process that talks to it over IPC.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::Duration;

use crate::audio::{Mixer, PlayRequest};
use crate::capture::{self, Capture, Replay};
use crate::config::{Config, Hotkey, Sound};
use crate::dsp::{self, VoiceProcessor};
use crate::hotkeys::Hotkeys;
use crate::ipc::State;
use crate::vmic::{self, VirtualMic};
use crate::{i18n, paths, spl, tray};

#[derive(Default)]
struct Routing {
    applied: Option<(Option<String>, bool)>, // (mic, processing)
    capture: Option<Capture>,
}

pub struct Core {
    pub config: Mutex<Config>,
    pub mixer: Arc<Mixer>,
    vmic: Mutex<Option<VirtualMic>>,
    routing: Mutex<Routing>,
    voice: Arc<Mutex<VoiceProcessor>>,
    replay: Mutex<Option<(u32, Replay)>>,
    hotkeys: OnceLock<Arc<Hotkeys>>,
    tray_refresh: Mutex<Option<Sender<()>>>,
    gui_pid: Mutex<Option<u32>>,
    status: Mutex<String>,
    quit: (Mutex<bool>, Condvar),
    shut_down: AtomicBool,
}

enum HotkeyAction {
    Play(usize),
    Stop,
    Voice(String),
    Replay,
}

impl Core {
    pub fn start() -> Result<Arc<Core>, String> {
        let (mut cfg, fresh) = Config::load();
        if fresh {
            cfg.mic_source = vmic::default_source();
            cfg.save();
        }
        i18n::set_language(cfg.language.as_deref());
        vmic::cleanup_stale();
        let vm = VirtualMic::setup()?;
        let mixer = Mixer::start(paths::sink_name());

        let core = Arc::new(Core {
            config: Mutex::new(cfg),
            mixer,
            vmic: Mutex::new(Some(vm)),
            routing: Mutex::new(Routing::default()),
            voice: Arc::new(Mutex::new(VoiceProcessor::new())),
            replay: Mutex::new(None),
            hotkeys: OnceLock::new(),
            tray_refresh: Mutex::new(None),
            gui_pid: Mutex::new(None),
            status: Mutex::new(
                tr!(
                    "Ready. Select the \"Soundpad Microphone\" input in Discord / your game.",
                    "Готово. В Discord/игре выбери микрофон «Soundpad Microphone».",
                )
                .into(),
            ),
            quit: (Mutex::new(false), Condvar::new()),
            shut_down: AtomicBool::new(false),
        });
        core.apply_runtime(None);

        let weak = Arc::downgrade(&core);
        let hk = Hotkeys::start(Arc::new(move |hk| {
            if let Some(c) = weak.upgrade() {
                c.on_hotkey(hk);
            }
        }));
        let _ = core.hotkeys.set(hk);

        if let Some(handle) = tray::start(&core) {
            // Refreshing from a separate thread: ksni deadlocks if `update` runs inside a tray callback.
            let (tx, rx) = channel::<()>();
            std::thread::spawn(move || {
                while rx.recv().is_ok() {
                    while rx.try_recv().is_ok() {}
                    handle.update(|_| {});
                }
            });
            *core.tray_refresh.lock().unwrap() = Some(tx);
        }
        core.preload_all();
        Ok(core)
    }

    fn refresh_tray(&self) {
        if let Some(tx) = &*self.tray_refresh.lock().unwrap() {
            let _ = tx.send(());
        }
    }

    /// Decode every sound in the background so hotkeys start instantly.
    pub fn preload_all(self: &Arc<Self>) {
        let paths: Vec<_> = self.config.lock().unwrap().sounds.iter().map(|s| s.path.clone()).collect();
        let core = self.clone();
        std::thread::spawn(move || {
            for p in paths {
                if let Err(e) = core.mixer.load(&p) {
                    let p = p.display();
                    core.set_status(tr!(format!("Failed to load {p}: {e}"), format!("Не удалось загрузить {p}: {e}")));
                }
            }
        });
    }

    pub fn status(&self) -> String {
        self.status.lock().unwrap().clone()
    }

    pub fn set_status(&self, s: impl Into<String>) {
        *self.status.lock().unwrap() = s.into();
        self.refresh_tray();
    }

    // ------------------------------------------------------------ configuration

    /// Replaces the whole config (from the GUI) and applies what changed.
    pub fn update_config(self: &Arc<Self>, new: Config) {
        let prev = {
            let mut cfg = self.config.lock().unwrap();
            let prev = std::mem::replace(&mut *cfg, new);
            cfg.save();
            prev
        };
        self.apply_runtime(Some(&prev));
    }

    fn modify_config(self: &Arc<Self>, f: impl FnOnce(&mut Config)) {
        let mut cfg = self.config.lock().unwrap().clone();
        f(&mut cfg);
        self.update_config(cfg);
    }

    /// Pushes the current config into the audio engine. Only changed parts are rebuilt.
    fn apply_runtime(self: &Arc<Self>, prev: Option<&Config>) {
        let cfg = self.config.lock().unwrap().clone();
        i18n::set_language(cfg.language.as_deref());
        self.mixer.set_levels(cfg.master_volume, cfg.monitor_enabled, cfg.monitor_volume, cfg.voice.mic_gain, cfg.voice.hear_myself);
        self.mixer.set_monitor_device(cfg.monitor_device.clone());
        self.voice.lock().unwrap().configure(&cfg.voice.effect, cfg.voice.denoise);
        self.route_mic(&cfg);
        self.update_replay(&cfg);
        if let Some(prev) = prev {
            for old in &prev.sounds {
                if !cfg.sounds.iter().any(|s| s.path == old.path) {
                    self.mixer.stop(&old.path);
                    self.mixer.forget(&old.path);
                }
            }
            if cfg.sounds.iter().any(|s| !prev.sounds.iter().any(|p| p.path == s.path)) {
                self.preload_all();
            }
        }
        self.refresh_tray();
    }

    /// Plain PipeWire loopback when the mic is untouched; our capture + DSP when effects are on.
    fn route_mic(&self, cfg: &Config) {
        let processing = cfg.voice.needs_processing();
        let wanted = (cfg.mic_source.clone(), processing);
        let mut r = self.routing.lock().unwrap();
        if r.applied.as_ref() == Some(&wanted) {
            return;
        }
        r.capture = None;
        self.mixer.mic.set_active(false);
        let mut vm_guard = self.vmic.lock().unwrap();
        let Some(vm) = vm_guard.as_mut() else { return };
        let mic = cfg.mic_source.as_deref();
        let res = if processing {
            vm.set_loopback(None).and_then(|_| match mic {
                Some(mic) => self.start_mic_capture(mic).map(|c| {
                    r.capture = Some(c);
                    self.mixer.mic.set_active(true);
                }),
                None => Ok(()),
            })
        } else {
            vm.set_loopback(mic)
        };
        drop(vm_guard);
        r.applied = Some(wanted);
        drop(r);
        if let Err(e) = res {
            self.set_status(tr!(format!("Failed to connect microphone: {e}"), format!("Не удалось подключить микрофон: {e}")));
        }
    }

    fn start_mic_capture(&self, mic: &str) -> Result<Capture, String> {
        let voice = self.voice.clone();
        let bus = self.mixer.mic.clone();
        let mut frame = vec![0f32; dsp::FRAME];
        capture::start(mic, 1, dsp::FRAME, "Soundpad mic", move |chunk| {
            frame.copy_from_slice(chunk);
            voice.lock().unwrap().process(&mut frame);
            bus.push(&frame);
        })
        .map_err(|e| e.to_string())
    }

    fn update_replay(&self, cfg: &Config) {
        let want = cfg.replay.enabled.then(|| cfg.replay.seconds.clamp(5, 300));
        let mut rp = self.replay.lock().unwrap();
        match (rp.as_ref().map(|r| r.0), want) {
            (Some(have), Some(w)) if have == w => {}
            (_, None) => *rp = None,
            (_, Some(w)) => {
                *rp = None;
                match Replay::start(w) {
                    Ok(r) => *rp = Some((w, r)),
                    Err(e) => {
                        drop(rp);
                        self.set_status(tr!(format!("Replay buffer failed: {e}"), format!("Не удалось включить запись: {e}")));
                    }
                }
            }
        }
    }

    // ------------------------------------------------------------ actions

    /// Plays sound by 0-based index. With `wait`, blocks until decoding finishes so errors can be reported.
    pub fn play_index(self: &Arc<Self>, idx: usize, wait: bool) -> Result<String, String> {
        let (sound, overlap, normalize) = {
            let cfg = self.config.lock().unwrap();
            let s = cfg
                .sounds
                .get(idx)
                .cloned()
                .ok_or_else(|| tr!(format!("no sound #{}", idx + 1), format!("нет звука №{}", idx + 1)))?;
            (s, cfg.allow_overlap, cfg.normalize)
        };
        let (tx, rx) = channel();
        let core = self.clone();
        let name = sound.name.clone();
        let req = PlayRequest {
            path: sound.path,
            gain: sound.volume,
            start_ms: sound.start_ms,
            end_ms: sound.end_ms,
            fade_in_ms: sound.fade_in_ms,
            fade_out_ms: sound.fade_out_ms,
            exclusive: !overlap,
            normalize,
        };
        self.mixer.play(req, move |res| {
            match &res {
                Ok(()) => core.set_status(format!("▶ {name}")),
                Err(e) => core.set_status(tr!(format!("Error in \"{name}\": {e}"), format!("Ошибка «{name}»: {e}"))),
            }
            let _ = tx.send(res);
        });
        if wait {
            match rx.recv_timeout(Duration::from_secs(10)) {
                Ok(Err(e)) => return Err(e),
                Err(_) => return Err(tr!("decoding took too long", "декодирование слишком долгое").into()),
                Ok(Ok(())) => {}
            }
        }
        Ok(sound.name)
    }

    pub fn play_query(self: &Arc<Self>, query: &str) -> Result<String, String> {
        let idx = self
            .config
            .lock()
            .unwrap()
            .find(query)
            .ok_or_else(|| tr!(format!("sound \"{query}\" not found"), format!("звук «{query}» не найден")))?;
        self.play_index(idx, true)
    }

    pub fn stop(&self) {
        self.mixer.stop_all();
        self.set_status(tr!("⏹ Stopped", "⏹ Остановлено"));
    }

    pub fn set_voice_effect(self: &Arc<Self>, id: &str) {
        let id = if dsp::PRESETS.iter().any(|p| p.0 == id) { id } else { "none" };
        self.modify_config(|c| c.voice.effect = id.to_string());
        self.set_status(format!("🎤 {}", dsp::preset_label(id)));
    }

    /// Hotkey behaviour: pressing the active effect's key turns it off.
    pub fn toggle_voice_effect(self: &Arc<Self>, id: &str) {
        let current = self.config.lock().unwrap().voice.effect.clone();
        self.set_voice_effect(if current == id { "none" } else { id });
    }

    fn on_hotkey(self: &Arc<Self>, hk: Hotkey) {
        let action = {
            let cfg = self.config.lock().unwrap();
            if !cfg.hotkeys_enabled {
                return;
            }
            if cfg.stop_hotkey == Some(hk) {
                HotkeyAction::Stop
            } else if let Some(i) = cfg.sounds.iter().position(|s| s.hotkey == Some(hk)) {
                HotkeyAction::Play(i)
            } else if let Some((id, _)) = cfg.voice.hotkeys.iter().find(|(_, h)| **h == hk) {
                HotkeyAction::Voice(id.clone())
            } else if cfg.replay.hotkey == Some(hk) {
                HotkeyAction::Replay
            } else {
                return;
            }
        };
        match action {
            HotkeyAction::Play(i) => {
                let _ = self.play_index(i, false);
            }
            HotkeyAction::Stop => self.stop(),
            HotkeyAction::Voice(id) => self.toggle_voice_effect(&id),
            HotkeyAction::Replay => {
                let _ = self.save_replay();
            }
        }
    }

    pub fn capture_hotkey(&self) -> Option<Hotkey> {
        self.hotkeys.get()?.capture(Duration::from_secs(10))
    }

    /// Saves the replay buffer as a WAV file and adds it to the sound list.
    pub fn save_replay(self: &Arc<Self>) -> Result<String, String> {
        let path = match &*self.replay.lock().unwrap() {
            Some((_, r)) => r.save(&paths::recordings_dir()).map_err(|e| e.to_string()),
            None => Err(tr!("the replay buffer is off", "запись выключена").to_string()),
        };
        let path = match path {
            Ok(p) => p,
            Err(e) => {
                self.set_status(tr!(format!("Replay: {e}"), format!("Запись: {e}")));
                return Err(e);
            }
        };
        let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let category = tr!("Replays", "Записи").to_string();
        let p = path.clone();
        self.modify_config(move |c| c.sounds.push(Sound { name: name.clone(), path: p, category, ..Default::default() }));
        self.set_status(tr!(format!("💾 Saved {}", path.display()), format!("💾 Сохранено: {}", path.display())));
        Ok(path.display().to_string())
    }

    /// Imports a Windows Soundpad list. Returns (added, not found).
    pub fn import_spl(self: &Arc<Self>, spl_path: &Path) -> Result<(usize, Vec<String>), String> {
        let items = spl::parse(spl_path)?;
        let category = tr!("Imported", "Импорт").to_string();
        let mut missing = Vec::new();
        let mut found: Vec<(String, PathBuf)> = Vec::new();
        for it in items {
            match it.path {
                Some(p) => found.push((it.title, p)),
                None => missing.push(it.original),
            }
        }
        let mut added = 0;
        self.modify_config(|c| {
            for (title, path) in found {
                if !c.sounds.iter().any(|s| s.path == path) {
                    c.sounds.push(Sound { name: title, path, category: category.clone(), ..Default::default() });
                    added += 1;
                }
            }
        });
        self.set_status(tr!(
            format!("Imported {added} sounds, not found: {}", missing.len()),
            format!("Импортировано звуков: {added}, не найдено: {}", missing.len())
        ));
        Ok((added, missing))
    }

    // ------------------------------------------------------------ GUI process

    pub fn set_gui_pid(&self, pid: u32) {
        *self.gui_pid.lock().unwrap() = Some(pid);
    }

    fn gui_alive(&self) -> Option<u32> {
        let mut pid = self.gui_pid.lock().unwrap();
        let alive = pid.filter(|p| {
            std::fs::read(format!("/proc/{p}/cmdline")).is_ok_and(|c| String::from_utf8_lossy(&c).contains("soundpad"))
        });
        *pid = alive;
        alive
    }

    /// Opens the window (or focuses it if it is already open).
    pub fn show_gui(&self) {
        if self.gui_alive().is_some() {
            focus_window();
            return;
        }
        let Ok(exe) = std::env::current_exe() else { return };
        use std::os::unix::process::CommandExt;
        let _ = std::process::Command::new(exe)
            .arg("gui")
            .stdin(std::process::Stdio::null())
            .process_group(0)
            .spawn();
    }

    pub fn state(&self) -> State {
        let cfg = self.config.lock().unwrap().clone();
        let hk = self.hotkeys.get();
        State {
            config: cfg,
            playing: self.mixer.playing(),
            durations: self.mixer.durations(),
            status: self.status(),
            hotkey_devices: hk.map_or(0, |h| h.device_count()),
            hotkeys_denied: hk.is_some_and(|h| h.permission_denied()),
            replay_buffered: self.replay.lock().unwrap().as_ref().map_or(0.0, |r| r.1.buffered_seconds()),
            gui_pid: self.gui_alive(),
        }
    }

    // ------------------------------------------------------------ lifecycle

    pub fn request_quit(&self) {
        *self.quit.0.lock().unwrap() = true;
        self.quit.1.notify_all();
    }

    pub fn wait_quit(&self) {
        let mut q = self.quit.0.lock().unwrap();
        while !*q {
            q = self.quit.1.wait(q).unwrap();
        }
    }

    /// Unloads the virtual devices and stops capture. Safe to call more than once.
    pub fn shutdown(&self) {
        if self.shut_down.swap(true, Ordering::SeqCst) {
            return;
        }
        self.mixer.stop_all();
        self.routing.lock().unwrap().capture = None;
        *self.replay.lock().unwrap() = None;
        if let Some(mut vm) = self.vmic.lock().unwrap().take() {
            vm.teardown();
        }
        crate::ipc::remove_socket();
    }
}

/// Best-effort focus of an already open window (only Hyprland exposes this to clients).
pub fn focus_window() {
    if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some() {
        let _ = std::process::Command::new("hyprctl")
            .args(["dispatch", "focuswindow", &format!("class:^({})$", paths::app_id())])
            .stdout(std::process::Stdio::null())
            .status();
    }
}
