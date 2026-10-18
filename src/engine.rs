use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::audio::Mixer;
use crate::config::Config;
use crate::vmic::{self, VirtualMic};

pub struct Core {
    pub config: Mutex<Config>,
    pub mixer: Arc<Mixer>,
    vmic: Mutex<Option<VirtualMic>>,
    status: Mutex<String>,
    notifier: Mutex<Option<Box<dyn Fn() + Send>>>,
    shut_down: AtomicBool,
}

impl Core {
    pub fn start() -> Result<Arc<Core>, String> {
        let (mut cfg, fresh) = Config::load();
        if fresh {
            cfg.mic_source = vmic::default_source();
            cfg.save();
        }
        vmic::cleanup_stale();
        let vm = match VirtualMic::setup(cfg.mic_source.as_deref()) {
            Ok(vm) => vm,
            // The saved mic may be unplugged: still bring up the virtual device without it.
            Err(e) if cfg.mic_source.is_some() => {
                let mic = cfg.mic_source.as_deref().unwrap_or_default();
                eprintln!("{}", tr!(format!("soundpad: microphone {mic} is unavailable: {e}"), format!("soundpad: микрофон {mic} недоступен: {e}")));
                vmic::cleanup_stale();
                VirtualMic::setup(None)?
            }
            Err(e) => return Err(e),
        };
        let mixer = Mixer::start(vmic::SINK);
        mixer.set_levels(cfg.master_volume, cfg.monitor_enabled, cfg.monitor_volume);

        let core = Arc::new(Core {
            config: Mutex::new(cfg),
            mixer,
            vmic: Mutex::new(Some(vm)),
            status: Mutex::new(
                tr!(
                    "Ready. Select the \"Soundpad Microphone\" input in Discord / your game.",
                    "Готово. В Discord/игре выбери микрофон «Soundpad Microphone».",
                )
                .into(),
            ),
            notifier: Mutex::new(None),
            shut_down: AtomicBool::new(false),
        });
        core.preload_all();
        Ok(core)
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

    pub fn set_notifier(&self, f: Box<dyn Fn() + Send>) {
        *self.notifier.lock().unwrap() = Some(f);
    }

    fn notify(&self) {
        if let Some(f) = &*self.notifier.lock().unwrap() {
            f();
        }
    }

    pub fn status(&self) -> String {
        self.status.lock().unwrap().clone()
    }

    pub fn set_status(&self, s: impl Into<String>) {
        *self.status.lock().unwrap() = s.into();
        self.notify();
    }

    /// Plays sound by 0-based index. With `wait`, blocks until decoding finishes so errors can be reported.
    pub fn play_index(self: &Arc<Self>, idx: usize, wait: bool) -> Result<String, String> {
        let (sound, overlap) = {
            let cfg = self.config.lock().unwrap();
            let s = cfg.sounds.get(idx).cloned().ok_or_else(|| tr!(format!("no sound #{}", idx + 1), format!("нет звука №{}", idx + 1)))?;
            (s, cfg.allow_overlap)
        };
        let (tx, rx) = channel();
        let core = self.clone();
        let name = sound.name.clone();
        self.mixer.play(sound.path, sound.volume, !overlap, move |res| {
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
        let idx = self.config.lock().unwrap().find(query).ok_or_else(|| tr!(format!("sound \"{query}\" not found"), format!("звук «{query}» не найден")))?;
        self.play_index(idx, true)
    }

    pub fn stop(&self) {
        self.mixer.stop_all();
        self.set_status(tr!("⏹ Stopped", "⏹ Остановлено"));
    }

    pub fn apply_levels(&self) {
        let cfg = self.config.lock().unwrap();
        self.mixer.set_levels(cfg.master_volume, cfg.monitor_enabled, cfg.monitor_volume);
    }

    pub fn set_mic(&self, mic: Option<String>) {
        let res = match &mut *self.vmic.lock().unwrap() {
            Some(vm) => vm.set_mic(mic.as_deref()),
            None => Ok(()),
        };
        match res {
            Ok(()) => {
                let mut cfg = self.config.lock().unwrap();
                cfg.mic_source = mic;
                cfg.save();
                drop(cfg);
                self.set_status(tr!("Microphone switched", "Микрофон переключён"));
            }
            Err(e) => self.set_status(tr!(format!("Failed to connect microphone: {e}"), format!("Не удалось подключить микрофон: {e}"))),
        }
    }

    /// Unloads the virtual devices. Safe to call more than once.
    pub fn shutdown(&self) {
        if self.shut_down.swap(true, Ordering::SeqCst) {
            return;
        }
        self.mixer.stop_all();
        if let Some(mut vm) = self.vmic.lock().unwrap().take() {
            vm.teardown();
        }
        crate::ipc::remove_socket();
    }
}
