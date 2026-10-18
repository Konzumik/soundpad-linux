//! System tray icon (StatusNotifierItem: KDE, waybar, GNOME with the AppIndicator extension, …).

use std::sync::{Arc, Weak};

use ksni::blocking::{Handle, TrayMethods};
use ksni::menu::{RadioGroup, RadioItem, StandardItem};
use ksni::{MenuItem, ToolTip};

use crate::dsp::{preset_label, PRESETS};
use crate::engine::Core;
use crate::paths;

pub struct SoundpadTray {
    core: Weak<Core>,
}

pub type TrayHandle = Handle<SoundpadTray>;

pub fn start(core: &Arc<Core>) -> Option<TrayHandle> {
    match (SoundpadTray { core: Arc::downgrade(core) }).spawn() {
        Ok(h) => Some(h),
        Err(e) => {
            eprintln!("{}", tr!(format!("soundpad: no system tray ({e})"), format!("soundpad: трей недоступен ({e})")));
            None
        }
    }
}

impl SoundpadTray {
    fn with_core(&self, f: impl FnOnce(&Arc<Core>)) {
        if let Some(c) = self.core.upgrade() {
            f(&c);
        }
    }
}

impl ksni::Tray for SoundpadTray {
    const MENU_ON_ACTIVATE: bool = false;

    fn id(&self) -> String {
        paths::app_id().into()
    }

    fn title(&self) -> String {
        "Soundpad Linux".into()
    }

    fn icon_name(&self) -> String {
        "audio-input-microphone".into()
    }

    fn tool_tip(&self) -> ToolTip {
        let description = self.core.upgrade().map(|c| c.status()).unwrap_or_default();
        ToolTip { title: "Soundpad Linux".into(), description, ..Default::default() }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        self.with_core(|c| c.show_gui());
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let Some(core) = self.core.upgrade() else { return Vec::new() };
        let (effect, replay_on) = {
            let cfg = core.config.lock().unwrap();
            (cfg.voice.effect.clone(), cfg.replay.enabled)
        };
        let selected = PRESETS.iter().position(|p| p.0 == effect).unwrap_or(0);
        let mut items: Vec<MenuItem<Self>> = vec![
            StandardItem {
                label: tr!("Open window", "Открыть окно").into(),
                icon_name: "window-new".into(),
                activate: Box::new(|t: &mut Self| t.with_core(|c| c.show_gui())),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: tr!("Stop sounds", "Остановить звуки").into(),
                icon_name: "media-playback-stop".into(),
                activate: Box::new(|t: &mut Self| t.with_core(|c| c.stop())),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            ksni::menu::SubMenu {
                label: tr!("Voice effect", "Эффект голоса").into(),
                submenu: vec![RadioGroup {
                    selected,
                    select: Box::new(|t: &mut Self, i| {
                        t.with_core(|c| c.set_voice_effect(PRESETS[i].0));
                    }),
                    options: PRESETS.iter().map(|p| RadioItem { label: preset_label(p.0), ..Default::default() }).collect(),
                }
                .into()],
                ..Default::default()
            }
            .into(),
        ];
        if replay_on {
            items.push(
                StandardItem {
                    label: tr!("Save replay", "Сохранить запись").into(),
                    icon_name: "media-record".into(),
                    activate: Box::new(|t: &mut Self| {
                        t.with_core(|c| {
                            let _ = c.save_replay();
                        })
                    }),
                    ..Default::default()
                }
                .into(),
            );
        }
        items.push(MenuItem::Separator);
        items.push(
            StandardItem {
                label: tr!("Quit", "Выход").into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|t: &mut Self| t.with_core(|c| c.request_quit())),
                ..Default::default()
            }
            .into(),
        );
        items
    }
}
