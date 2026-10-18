//! Global hotkeys read straight from `/dev/input/event*` (evdev). Works on any Wayland
//! compositor or X11; needs read access to input devices (the `input` group).
//! Keys are not grabbed: they still reach the focused application.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use evdev::{EventSummary, KeyCode};

use crate::config::Hotkey;

pub struct Hotkeys {
    capture: Mutex<Option<Sender<Option<Hotkey>>>>,
    devices: Mutex<HashSet<PathBuf>>,
    /// Some `/dev/input/event*` exists but could not be opened (permissions).
    denied: AtomicBool,
}

type Handler = Arc<dyn Fn(Hotkey) + Send + Sync>;

const MODIFIERS: [(KeyCode, u8); 8] = [
    (KeyCode::KEY_LEFTCTRL, 1),
    (KeyCode::KEY_RIGHTCTRL, 1),
    (KeyCode::KEY_LEFTALT, 2),
    (KeyCode::KEY_RIGHTALT, 2),
    (KeyCode::KEY_LEFTSHIFT, 4),
    (KeyCode::KEY_RIGHTSHIFT, 4),
    (KeyCode::KEY_LEFTMETA, 8),
    (KeyCode::KEY_RIGHTMETA, 8),
];

impl Hotkeys {
    pub fn start(handler: Handler) -> Arc<Hotkeys> {
        let hk = Arc::new(Hotkeys {
            capture: Mutex::new(None),
            devices: Mutex::new(HashSet::new()),
            denied: AtomicBool::new(false),
        });
        let h = hk.clone();
        std::thread::Builder::new()
            .name("hotkey-scan".into())
            .spawn(move || loop {
                h.scan(&handler);
                std::thread::sleep(Duration::from_secs(3));
            })
            .expect("spawn hotkey scanner");
        hk
    }

    pub fn device_count(&self) -> usize {
        self.devices.lock().unwrap().len()
    }

    pub fn permission_denied(&self) -> bool {
        self.device_count() == 0 && self.denied.load(Ordering::Relaxed)
    }

    /// Waits for the next key combo (Esc cancels). Hotkey actions are suppressed meanwhile.
    pub fn capture(&self, timeout: Duration) -> Option<Hotkey> {
        let (tx, rx) = channel();
        *self.capture.lock().unwrap() = Some(tx);
        let res = rx.recv_timeout(timeout).ok().flatten();
        *self.capture.lock().unwrap() = None;
        res
    }

    fn scan(self: &Arc<Self>, handler: &Handler) {
        let opened: Vec<(PathBuf, evdev::Device)> = evdev::enumerate().collect();
        let total = std::fs::read_dir("/dev/input")
            .map(|d| d.flatten().filter(|e| e.file_name().to_string_lossy().starts_with("event")).count())
            .unwrap_or(0);
        self.denied.store(opened.len() < total, Ordering::Relaxed);

        for (path, dev) in opened {
            if self.devices.lock().unwrap().contains(&path) || !is_keyboard(&dev) {
                continue;
            }
            self.devices.lock().unwrap().insert(path.clone());
            let (me, handler) = (self.clone(), handler.clone());
            let _ = std::thread::Builder::new().name("hotkey-dev".into()).spawn(move || {
                me.read_device(dev, &handler);
                me.devices.lock().unwrap().remove(&path);
            });
        }
    }

    fn read_device(&self, mut dev: evdev::Device, handler: &Handler) {
        let mut mods = [false; 8];
        loop {
            let Ok(events) = dev.fetch_events() else { return };
            for ev in events {
                let EventSummary::Key(_, code, value) = ev.destructure() else { continue };
                if let Some(i) = MODIFIERS.iter().position(|(k, _)| *k == code) {
                    mods[i] = value != 0;
                    continue;
                }
                if value != 1 {
                    continue; // release or auto-repeat
                }
                let bits = MODIFIERS.iter().zip(mods).filter(|(_, on)| *on).fold(0, |acc, ((_, b), _)| acc | b);
                let hk = Hotkey { ctrl: bits & 1 != 0, alt: bits & 2 != 0, shift: bits & 4 != 0, meta: bits & 8 != 0, key: code.0 };
                if let Some(tx) = self.capture.lock().unwrap().take() {
                    let _ = tx.send((code != KeyCode::KEY_ESC).then_some(hk));
                    continue;
                }
                handler(hk);
            }
        }
    }
}

fn is_keyboard(dev: &evdev::Device) -> bool {
    dev.supported_keys().is_some_and(|keys| {
        [KeyCode::KEY_A, KeyCode::KEY_1, KeyCode::KEY_F1, KeyCode::KEY_KP1, KeyCode::KEY_F13]
            .iter()
            .any(|k| keys.contains(*k))
    })
}
