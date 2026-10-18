//! The window. A client of the daemon: it polls `state` and sends the edited config back,
//! so closing it never interrupts the virtual mic or hotkeys.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui::{self, Align, Color32, Layout, RichText};

use crate::config::{Config, Hotkey, AUDIO_EXTS};
use crate::dsp::{preset_label, PRESETS};
use crate::ipc::{self, State};
use crate::vmic::DeviceInfo;
use crate::{i18n, paths};

pub fn run() -> Result<(), String> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Soundpad Linux")
            .with_app_id(paths::app_id())
            .with_inner_size([860.0, 620.0])
            .with_min_inner_size([560.0, 400.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native("Soundpad Linux", options, Box::new(|cc| Ok(Box::new(GuiApp::new(cc)))))
        .map_err(|e| e.to_string())
}

#[derive(Clone, PartialEq, Debug)]
enum Target {
    Sound(PathBuf),
    Stop,
    Voice(String),
    Replay,
}

enum Picked {
    Files(Vec<PathBuf>),
    Spl(PathBuf),
}

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Sounds,
    Voice,
    Settings,
}

enum Action {
    Play(usize),
    StopPath(PathBuf),
    StopAll,
    Capture(Target),
    PickFiles,
    PickSpl,
    SaveReplay,
    RefreshDevices,
    StartDaemon,
    Quit,
}

#[derive(Default)]
struct Shared {
    state: Option<State>,
}

struct GuiApp {
    shared: Arc<Mutex<Shared>>,
    cfg: Config,
    /// Ignore polled config for a moment after a local edit, so sliders don't jump back.
    suppress_until: Instant,
    tab: Tab,
    category: Option<String>,
    extra_categories: Vec<String>,
    new_category: String,
    filter: String,
    path_input: String,
    sources: Vec<DeviceInfo>,
    sinks: Vec<DeviceInfo>,
    capturing: Option<Target>,
    capture_tx: Sender<(Target, Option<Hotkey>)>,
    capture_rx: Receiver<(Target, Option<Hotkey>)>,
    picked_tx: Sender<Picked>,
    picked_rx: Receiver<Picked>,
    editing: Option<PathBuf>,
    error: Option<String>,
}

fn send(cmd: &str) -> Option<String> {
    ipc::send(cmd).ok()
}

fn fetch_devices(cmd: &str) -> Vec<DeviceInfo> {
    send(cmd).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

impl GuiApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let shared = Arc::new(Mutex::new(Shared::default()));
        let (sh, ctx) = (shared.clone(), cc.egui_ctx.clone());
        std::thread::spawn(move || {
            let mut said_hello = false;
            loop {
                let state = ipc::fetch_state();
                match &state {
                    Some(_) if !said_hello => said_hello = send(&format!("gui-hello {}", std::process::id())).is_some(),
                    None => said_hello = false,
                    _ => {}
                }
                let busy = state.as_ref().is_some_and(|s| !s.playing.is_empty());
                sh.lock().unwrap().state = state;
                ctx.request_repaint();
                std::thread::sleep(Duration::from_millis(if busy { 40 } else { 200 }));
            }
        });
        let (capture_tx, capture_rx) = channel();
        let (picked_tx, picked_rx) = channel();
        let initial = ipc::fetch_state();
        GuiApp {
            shared,
            cfg: initial.map(|s| s.config).unwrap_or_default(),
            suppress_until: Instant::now(),
            // Dev aid for screenshots: SOUNDPAD_GUI_TAB=voice|settings opens that tab.
            tab: match std::env::var("SOUNDPAD_GUI_TAB").as_deref() {
                Ok("voice") => Tab::Voice,
                Ok("settings") => Tab::Settings,
                _ => Tab::Sounds,
            },
            category: None,
            extra_categories: Vec::new(),
            new_category: String::new(),
            filter: String::new(),
            path_input: String::new(),
            sources: fetch_devices("sources"),
            sinks: fetch_devices("sinks"),
            capturing: None,
            capture_tx,
            capture_rx,
            picked_tx,
            picked_rx,
            editing: None,
            error: None,
        }
    }

    // ------------------------------------------------------------ widgets

    /// Button showing a hotkey; click = assign (press a key), right click = clear.
    fn hotkey_button(&self, ui: &mut egui::Ui, current: Option<Hotkey>, target: Target, actions: &mut Vec<Action>) -> bool {
        let capturing = self.capturing.as_ref() == Some(&target);
        let label = if capturing {
            RichText::new(tr!("press a key…", "нажми клавишу…")).color(Color32::from_rgb(240, 190, 80))
        } else {
            match current {
                Some(h) => RichText::new(h.label()).monospace(),
                None => RichText::new("⌨ —").weak(),
            }
        };
        let resp = ui
            .add(egui::Button::new(label).min_size(egui::vec2(110.0, 0.0)))
            .on_hover_text(tr!("Click to assign a key (Esc cancels), right-click to clear", "Нажми, чтобы назначить клавишу (Esc — отмена), правый клик — убрать"));
        if resp.clicked() && self.capturing.is_none() {
            actions.push(Action::Capture(target));
        }
        resp.secondary_clicked()
    }

    fn device_combo(ui: &mut egui::Ui, id: &str, value: &mut Option<String>, devices: &[DeviceInfo], none_label: &str) {
        let label_of = |name: &Option<String>| match name {
            None => none_label.to_string(),
            Some(n) => devices
                .iter()
                .find(|d| &d.name == n)
                .map(|d| d.description.clone())
                .unwrap_or_else(|| tr!(format!("{n} (not found)"), format!("{n} (не найден)"))),
        };
        egui::ComboBox::from_id_salt(id).width(330.0).selected_text(label_of(value)).show_ui(ui, |ui| {
            ui.selectable_value(value, None, none_label);
            for d in devices {
                ui.selectable_value(value, Some(d.name.clone()), &d.description);
            }
        });
    }

    // ------------------------------------------------------------ tabs

    fn sounds_tab(&mut self, ui: &mut egui::Ui, cfg: &mut Config, state: &State, actions: &mut Vec<Action>) {
        ui.horizontal(|ui| {
            if ui.button(tr!("➕ Add files…", "➕ Добавить файлы…")).clicked() {
                actions.push(Action::PickFiles);
            }
            if ui
                .button(tr!("📥 Import from Soundpad (.spl)…", "📥 Импорт из Soundpad (.spl)…"))
                .on_hover_text(tr!("Sound list exported from Windows Soundpad", "Список звуков из Soundpad для Windows"))
                .clicked()
            {
                actions.push(Action::PickSpl);
            }
            let resp = ui.add(
                egui::TextEdit::singleline(&mut self.path_input)
                    .hint_text(tr!("…or a path to a file/folder, or drop files here", "…или путь к файлу/папке, или перетащи файлы сюда"))
                    .desired_width(ui.available_width() - 90.0),
            );
            let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (ui.button(tr!("Add", "Добавить")).clicked() || enter) && !self.path_input.trim().is_empty() {
                let raw = self.path_input.trim().trim_matches(|c| c == '"' || c == '\'');
                let n = cfg.add_paths([PathBuf::from(expand_home(raw))], self.category.as_deref().unwrap_or(""));
                self.error = (n == 0).then(|| tr!("Nothing added: file not found or already in the list", "Ничего не добавлено: файл не найден или уже в списке").to_string());
                self.path_input.clear();
            }
        });

        // Category tabs
        let mut cats = cfg.categories();
        for c in &self.extra_categories {
            if !cats.contains(c) {
                cats.push(c.clone());
            }
        }
        ui.horizontal_wrapped(|ui| {
            if ui.selectable_label(self.category.is_none(), tr!("All", "Все")).clicked() {
                self.category = None;
            }
            for c in &cats {
                if ui.selectable_label(self.category.as_ref() == Some(c), c).clicked() {
                    self.category = Some(c.clone());
                }
            }
            ui.add(egui::TextEdit::singleline(&mut self.new_category).hint_text(tr!("new category", "новая категория")).desired_width(120.0));
            if ui.button("+").clicked() && !self.new_category.trim().is_empty() {
                let c = self.new_category.trim().to_string();
                if !self.extra_categories.contains(&c) {
                    self.extra_categories.push(c.clone());
                }
                self.category = Some(c);
                self.new_category.clear();
            }
            ui.separator();
            ui.label("🔍");
            ui.add(egui::TextEdit::singleline(&mut self.filter).hint_text(tr!("Search", "Поиск")).desired_width(160.0));
        });
        ui.separator();

        let filter = self.filter.to_lowercase();
        let mut remove = None;
        let mut swap = None;
        let mut clear_hotkey = None;
        let count = cfg.sounds.len();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            egui::Grid::new("sounds").striped(true).num_columns(7).spacing([8.0, 6.0]).show(ui, |ui| {
                for i in 0..cfg.sounds.len() {
                    let sound = &cfg.sounds[i];
                    if self.category.as_ref().is_some_and(|c| &sound.category != c)
                        || (!filter.is_empty() && !sound.name.to_lowercase().contains(&filter))
                    {
                        continue;
                    }
                    let path = sound.path.clone();
                    let progress = state.playing.iter().find(|(p, _)| *p == path).map(|(_, f)| *f);

                    ui.label(RichText::new(format!("{:>3}", i + 1)).monospace().weak());
                    let (icon, hint) = if progress.is_some() { ("⏹", tr!("Stop", "Остановить")) } else { ("▶", tr!("Play", "Играть")) };
                    if ui.button(RichText::new(icon).size(16.0)).on_hover_text(hint).clicked() {
                        actions.push(if progress.is_some() { Action::StopPath(path.clone()) } else { Action::Play(i) });
                    }
                    ui.vertical(|ui| {
                        ui.set_min_width(250.0);
                        let color = if progress.is_some() { Color32::from_rgb(110, 200, 120) } else { ui.visuals().text_color() };
                        ui.add(egui::TextEdit::singleline(&mut cfg.sounds[i].name).text_color(color).desired_width(250.0))
                            .on_hover_text(path.display().to_string());
                        if let Some(f) = progress {
                            ui.add(egui::ProgressBar::new(f).desired_width(250.0).desired_height(4.0));
                        }
                    });
                    if self.hotkey_button(ui, cfg.sounds[i].hotkey, Target::Sound(path.clone()), actions) {
                        clear_hotkey = Some(i);
                    }
                    ui.add(egui::Slider::new(&mut cfg.sounds[i].volume, 0.0..=2.0).custom_formatter(percent))
                        .on_hover_text(tr!("Volume of this sound", "Громкость этого звука"));
                    if ui.button("✂").on_hover_text(tr!("Trim, fades, category", "Обрезка, затухание, категория")).clicked() {
                        self.editing = Some(path.clone());
                    }
                    ui.horizontal(|ui| {
                        if ui.add_enabled(i > 0, egui::Button::new("⏶")).clicked() {
                            swap = Some((i, i - 1));
                        }
                        if ui.add_enabled(i + 1 < count, egui::Button::new("⏷")).clicked() {
                            swap = Some((i, i + 1));
                        }
                        if ui.button("✖").on_hover_text(tr!("Remove from list", "Удалить из списка")).clicked() {
                            remove = Some(i);
                        }
                    });
                    ui.end_row();
                }
            });
            if cfg.sounds.is_empty() {
                ui.add_space(40.0);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new(tr!("The list is empty", "Список пуст")).size(18.0).weak());
                    let exts = AUDIO_EXTS.join(", ");
                    ui.label(RichText::new(tr!(format!("Drop audio or video files here ({exts})"), format!("Перетащи сюда аудио- или видеофайлы ({exts})"))).weak());
                });
            }
        });
        if let Some(i) = clear_hotkey {
            cfg.sounds[i].hotkey = None;
        }
        if let Some((a, b)) = swap {
            cfg.sounds.swap(a, b);
        }
        if let Some(i) = remove {
            cfg.sounds.remove(i);
        }
    }

    fn edit_window(&mut self, ctx: &egui::Context, cfg: &mut Config, state: &State, actions: &mut Vec<Action>) {
        let Some(path) = self.editing.clone() else { return };
        let Some(idx) = cfg.sounds.iter().position(|s| s.path == path) else {
            self.editing = None;
            return;
        };
        let duration = state.durations.iter().find(|(p, _)| *p == path).map(|(_, d)| *d);
        let cats = cfg.categories();
        let mut open = true;
        egui::Window::new(tr!("Edit sound", "Настройка звука")).open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
            let s = &mut cfg.sounds[idx];
            egui::Grid::new("edit").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                ui.label(tr!("Name:", "Название:"));
                ui.text_edit_singleline(&mut s.name);
                ui.end_row();

                ui.label(tr!("Category:", "Категория:"));
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut s.category).desired_width(140.0));
                    egui::ComboBox::from_id_salt("cat").selected_text("⏷").width(30.0).show_ui(ui, |ui| {
                        ui.selectable_value(&mut s.category, String::new(), tr!("(none)", "(без категории)"));
                        for c in &cats {
                            ui.selectable_value(&mut s.category, c.clone(), c);
                        }
                    });
                });
                ui.end_row();

                let max_ms = duration.map(|d| (d * 1000.0) as u32).unwrap_or(600_000);
                ui.label(tr!("Start:", "Начало:"));
                ui.add(egui::DragValue::new(&mut s.start_ms).range(0..=max_ms).speed(10).suffix(" ms"));
                ui.end_row();

                ui.label(tr!("End:", "Конец:"));
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut s.end_ms).range(0..=max_ms).speed(10).suffix(" ms"));
                    ui.label(RichText::new(tr!("0 = to the end", "0 = до конца")).weak());
                });
                ui.end_row();

                ui.label(tr!("Fade in:", "Нарастание:"));
                ui.add(egui::DragValue::new(&mut s.fade_in_ms).range(0..=10_000).speed(5).suffix(" ms"));
                ui.end_row();

                ui.label(tr!("Fade out:", "Затухание:"));
                ui.add(egui::DragValue::new(&mut s.fade_out_ms).range(0..=10_000).speed(5).suffix(" ms"));
                ui.end_row();

                ui.label(tr!("Length:", "Длина:"));
                ui.label(match duration {
                    Some(d) => format!("{d:.2} s"),
                    None => tr!("loading…", "загружается…").to_string(),
                });
                ui.end_row();
            });
            ui.label(RichText::new(s.path.display().to_string()).weak().small());
            ui.horizontal(|ui| {
                if ui.button(tr!("▶ Preview", "▶ Прослушать")).clicked() {
                    actions.push(Action::Play(idx));
                }
                if ui.button(tr!("Reset trim", "Сбросить обрезку")).clicked() {
                    s.start_ms = 0;
                    s.end_ms = 0;
                    s.fade_in_ms = 0;
                    s.fade_out_ms = 0;
                }
            });
        });
        if !open {
            self.editing = None;
        }
    }

    fn voice_tab(&mut self, ui: &mut egui::Ui, cfg: &mut Config, state: &State, actions: &mut Vec<Action>) {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.heading(tr!("Microphone", "Микрофон"));
            egui::Grid::new("mic").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                ui.label(tr!("Device:", "Устройство:"));
                ui.horizontal(|ui| {
                    Self::device_combo(ui, "mic", &mut cfg.mic_source, &self.sources, tr!("— off (sounds only) —", "— выключен (только звуки) —"));
                    if ui.button("⟳").on_hover_text(tr!("Refresh device list", "Обновить список устройств")).clicked() {
                        actions.push(Action::RefreshDevices);
                    }
                });
                ui.end_row();
                ui.label(tr!("Mic volume:", "Громкость микрофона:"));
                ui.add(egui::Slider::new(&mut cfg.voice.mic_gain, 0.0..=3.0).custom_formatter(percent));
                ui.end_row();
                ui.label("");
                ui.checkbox(&mut cfg.voice.denoise, tr!("Noise suppression (RNNoise)", "Шумоподавление (RNNoise)"));
                ui.end_row();
                ui.label("");
                ui.checkbox(&mut cfg.voice.hear_myself, tr!("Hear myself in headphones", "Слышать себя в наушниках"));
                ui.end_row();
            });

            ui.add_space(10.0);
            ui.heading(tr!("Voice changer", "Изменение голоса"));
            ui.label(RichText::new(tr!(
                "Effects, noise suppression and mic volume route your voice through Soundpad (≈50 ms extra latency).",
                "Эффекты, шумоподавление и громкость пропускают голос через Soundpad (≈50 мс доп. задержки)."
            )).weak());
            let mut clear = None;
            egui::Grid::new("effects").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                for (id, ..) in PRESETS {
                    let selected = cfg.voice.effect == *id;
                    if ui.add(egui::Button::new(preset_label(id)).selected(selected).min_size(egui::vec2(170.0, 0.0))).clicked() {
                        cfg.voice.effect = id.to_string();
                    }
                    if self.hotkey_button(ui, cfg.voice.hotkeys.get(*id).copied(), Target::Voice(id.to_string()), actions) {
                        clear = Some(id.to_string());
                    }
                    ui.end_row();
                }
            });
            if let Some(id) = clear {
                cfg.voice.hotkeys.remove(&id);
            }
            ui.label(RichText::new(tr!("An effect's hotkey toggles it on and off.", "Хоткей эффекта включает и выключает его.")).weak());

            ui.add_space(10.0);
            ui.heading(tr!("Instant replay", "Запись последних секунд"));
            ui.checkbox(&mut cfg.replay.enabled, tr!("Keep the last seconds of desktop audio", "Хранить последние секунды звука с компьютера"));
            ui.add_enabled_ui(cfg.replay.enabled, |ui| {
                ui.horizontal(|ui| {
                    ui.label(tr!("Length:", "Длина:"));
                    ui.add(egui::Slider::new(&mut cfg.replay.seconds, 5..=120).suffix(tr!(" s", " с")));
                });
                ui.horizontal(|ui| {
                    if ui.button(tr!(format!("💾 Save last {} s as a sound", cfg.replay.seconds), format!("💾 Сохранить последние {} с как звук", cfg.replay.seconds))).clicked() {
                        actions.push(Action::SaveReplay);
                    }
                    if self.hotkey_button(ui, cfg.replay.hotkey, Target::Replay, actions) {
                        cfg.replay.hotkey = None;
                    }
                    if cfg.replay.enabled {
                        ui.label(RichText::new(tr!(format!("buffered: {:.0} s", state.replay_buffered), format!("в буфере: {:.0} с", state.replay_buffered))).weak());
                    }
                });
            });
        });
    }

    fn settings_tab(&mut self, ui: &mut egui::Ui, cfg: &mut Config, state: &State, actions: &mut Vec<Action>) {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.heading(tr!("Sound", "Звук"));
            egui::Grid::new("settings").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                ui.label(tr!("Volume into mic:", "Громкость в микрофон:"));
                ui.add(egui::Slider::new(&mut cfg.master_volume, 0.0..=2.0).custom_formatter(percent));
                ui.end_row();

                ui.checkbox(&mut cfg.monitor_enabled, tr!("Hear in headphones:", "Слышать в наушниках:"));
                ui.add_enabled(cfg.monitor_enabled, egui::Slider::new(&mut cfg.monitor_volume, 0.0..=2.0).custom_formatter(percent));
                ui.end_row();

                ui.label(tr!("Headphones:", "Наушники:"));
                ui.horizontal(|ui| {
                    Self::device_combo(ui, "sink", &mut cfg.monitor_device, &self.sinks, tr!("System default", "Как в системе"));
                    if ui.button("⟳").clicked() {
                        actions.push(Action::RefreshDevices);
                    }
                });
                ui.end_row();

                ui.label("");
                ui.checkbox(&mut cfg.allow_overlap, tr!("Allow several sounds at once", "Разрешить несколько звуков одновременно"));
                ui.end_row();
                ui.label("");
                ui.checkbox(&mut cfg.normalize, tr!("Normalize loudness of all sounds", "Выравнивать громкость всех звуков"));
                ui.end_row();
            });

            ui.add_space(10.0);
            ui.heading(tr!("Hotkeys", "Горячие клавиши"));
            ui.checkbox(&mut cfg.hotkeys_enabled, tr!("Enable global hotkeys", "Включить глобальные хоткеи"));
            if state.hotkeys_denied {
                ui.colored_label(
                    Color32::from_rgb(240, 120, 90),
                    tr!("No access to keyboards. Run this and log in again:", "Нет доступа к клавиатурам. Выполни и перезайди в систему:"),
                );
                ui.code("sudo usermod -aG input $USER");
            } else {
                ui.label(RichText::new(tr!(format!("Keyboards found: {}", state.hotkey_devices), format!("Найдено клавиатур: {}", state.hotkey_devices))).weak());
            }
            ui.horizontal(|ui| {
                ui.label(tr!("Stop all sounds:", "Остановить все звуки:"));
                if self.hotkey_button(ui, cfg.stop_hotkey, Target::Stop, actions) {
                    cfg.stop_hotkey = None;
                }
            });
            ui.collapsing(tr!("Bind in the compositor instead (soundpad play …)", "Или через композитор (soundpad play …)"), |ui| {
                ui.code("bind = , KP_1, exec, soundpad play 1\nbind = , KP_0, exec, soundpad stop\nbind = , KP_5, exec, soundpad voice robot");
                ui.hyperlink_to(
                    tr!("Guides for GNOME, KDE, Sway, i3…", "Инструкции для GNOME, KDE, Sway, i3…"),
                    "https://github.com/Konzumik/soundpad-linux#hotkeys",
                );
            });

            ui.add_space(10.0);
            ui.heading(tr!("Window", "Окно"));
            ui.checkbox(
                &mut cfg.close_to_tray,
                tr!("Keep running in the background when the window is closed", "Работать в фоне после закрытия окна"),
            );
            ui.horizontal(|ui| {
                ui.label(tr!("Language:", "Язык:"));
                language_combo(ui, cfg);
            });
        });
    }

    fn not_running_ui(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(120.0);
                ui.heading(tr!("Soundpad is not running", "Soundpad не запущен"));
                ui.add_space(10.0);
                if ui.button(RichText::new(tr!("▶ Start", "▶ Запустить")).size(18.0)).clicked() {
                    actions.push(Action::StartDaemon);
                }
                if let Some(e) = &self.error {
                    ui.add_space(10.0);
                    ui.colored_label(Color32::from_rgb(240, 120, 90), e);
                }
            });
        });
    }

    fn apply_capture(cfg: &mut Config, target: &Target, hk: Hotkey) {
        // A key does one thing: drop it from anything else first.
        for s in cfg.sounds.iter_mut().filter(|s| s.hotkey == Some(hk)) {
            s.hotkey = None;
        }
        cfg.voice.hotkeys.retain(|_, h| *h != hk);
        if cfg.stop_hotkey == Some(hk) {
            cfg.stop_hotkey = None;
        }
        if cfg.replay.hotkey == Some(hk) {
            cfg.replay.hotkey = None;
        }
        match target {
            Target::Sound(p) => {
                if let Some(s) = cfg.sounds.iter_mut().find(|s| &s.path == p) {
                    s.hotkey = Some(hk);
                }
            }
            Target::Stop => cfg.stop_hotkey = Some(hk),
            Target::Voice(id) => {
                cfg.voice.hotkeys.insert(id.clone(), hk);
            }
            Target::Replay => cfg.replay.hotkey = Some(hk),
        }
    }

    fn run_action(&mut self, ctx: &egui::Context, action: Action) {
        match action {
            Action::Play(i) => {
                if let Some(r) = send(&format!("play-index {i}")) {
                    self.error = r.strip_prefix("ERR ").map(|e| e.trim().to_string());
                }
            }
            Action::StopPath(p) => {
                send(&format!("stop-path {}", p.display()));
            }
            Action::StopAll => {
                send("stop");
            }
            Action::Capture(target) => {
                self.capturing = Some(target.clone());
                let (tx, ctx) = (self.capture_tx.clone(), ctx.clone());
                std::thread::spawn(move || {
                    let hk = send("capture-hotkey").and_then(|s| serde_json::from_str::<Option<Hotkey>>(&s).ok()).flatten();
                    let _ = tx.send((target, hk));
                    ctx.request_repaint();
                });
            }
            Action::PickFiles => {
                let (tx, ctx) = (self.picked_tx.clone(), ctx.clone());
                std::thread::spawn(move || {
                    if let Some(files) = rfd::FileDialog::new()
                        .set_title(tr!("Choose sounds", "Выбери звуки"))
                        .add_filter(tr!("Audio / video", "Аудио / видео"), AUDIO_EXTS)
                        .pick_files()
                    {
                        let _ = tx.send(Picked::Files(files));
                        ctx.request_repaint();
                    }
                });
            }
            Action::PickSpl => {
                let (tx, ctx) = (self.picked_tx.clone(), ctx.clone());
                std::thread::spawn(move || {
                    if let Some(file) = rfd::FileDialog::new()
                        .set_title(tr!("Soundpad sound list", "Список звуков Soundpad"))
                        .add_filter("Soundpad (.spl)", &["spl", "xml"])
                        .pick_file()
                    {
                        let _ = tx.send(Picked::Spl(file));
                        ctx.request_repaint();
                    }
                });
            }
            Action::SaveReplay => {
                send("replay");
            }
            Action::RefreshDevices => {
                self.sources = fetch_devices("sources");
                self.sinks = fetch_devices("sinks");
            }
            Action::StartDaemon => {
                self.error = ipc::spawn_daemon().err();
                if self.error.is_none() {
                    if let Some(s) = ipc::fetch_state() {
                        self.cfg = s.config;
                    }
                    self.sources = fetch_devices("sources");
                    self.sinks = fetch_devices("sinks");
                }
            }
            Action::Quit => {
                send("quit");
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
}

impl eframe::App for GuiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let state = self.shared.lock().unwrap().state.clone();
        let mut actions = Vec::new();
        let Some(state) = state else {
            i18n::set_language(self.cfg.language.as_deref());
            self.not_running_ui(ctx, &mut actions);
            for a in actions {
                self.run_action(ctx, a);
            }
            return;
        };
        if Instant::now() > self.suppress_until {
            self.cfg = state.config.clone();
        }
        i18n::set_language(self.cfg.language.as_deref());
        let mut cfg = self.cfg.clone();

        while let Ok((target, hk)) = self.capture_rx.try_recv() {
            if self.capturing.as_ref() == Some(&target) {
                self.capturing = None;
            }
            if let Some(hk) = hk {
                Self::apply_capture(&mut cfg, &target, hk);
            }
        }
        let mut new_paths: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        while let Ok(p) = self.picked_rx.try_recv() {
            match p {
                Picked::Files(f) => new_paths.extend(f),
                Picked::Spl(f) => import_spl(&f, &mut self.error),
            }
        }
        if !new_paths.is_empty() {
            let spl: Vec<PathBuf> = new_paths.iter().filter(|p| p.extension().is_some_and(|e| e == "spl")).cloned().collect();
            for f in &spl {
                import_spl(f, &mut self.error);
            }
            new_paths.retain(|p| !spl.contains(p));
            cfg.add_paths(new_paths, self.category.as_deref().unwrap_or(""));
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) && self.capturing.is_none() {
            actions.push(Action::StopAll);
        }

        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.heading("🎵 Soundpad Linux");
                ui.add_space(12.0);
                for (tab, label) in [
                    (Tab::Sounds, tr!("Sounds", "Звуки")),
                    (Tab::Voice, tr!("Voice", "Голос")),
                    (Tab::Settings, tr!("Settings", "Настройки")),
                ] {
                    if ui.selectable_label(self.tab == tab, RichText::new(label).size(15.0)).clicked() {
                        self.tab = tab;
                    }
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("🚪").on_hover_text(tr!("Quit Soundpad completely", "Полностью выйти из Soundpad")).clicked() {
                        actions.push(Action::Quit);
                    }
                    if ui.button(RichText::new(tr!("⏹ Stop all", "⏹ Стоп всё")).strong()).on_hover_text("Esc").clicked() {
                        actions.push(Action::StopAll);
                    }
                    language_combo(ui, &mut cfg);
                    if cfg.voice.effect != "none" {
                        ui.label(RichText::new(format!("🎤 {}", preset_label(&cfg.voice.effect))).color(Color32::from_rgb(180, 140, 250)));
                    }
                });
            });
            ui.add_space(4.0);
        });

        egui::TopBottomPanel::bottom("bottom").show(ctx, |ui| {
            ui.add_space(3.0);
            match &self.error {
                Some(e) => ui.colored_label(Color32::from_rgb(240, 120, 90), e),
                None => ui.label(&state.status),
            };
            ui.add_space(3.0);
        });

        egui::CentralPanel::default().show(ctx, |ui| match self.tab {
            Tab::Sounds => self.sounds_tab(ui, &mut cfg, &state, &mut actions),
            Tab::Voice => self.voice_tab(ui, &mut cfg, &state, &mut actions),
            Tab::Settings => self.settings_tab(ui, &mut cfg, &state, &mut actions),
        });
        self.edit_window(ctx, &mut cfg, &state, &mut actions);

        if cfg != self.cfg {
            self.cfg = cfg;
            if let Ok(js) = serde_json::to_string(&self.cfg) {
                send(&format!("set-config {js}"));
            }
            self.suppress_until = Instant::now() + Duration::from_millis(500);
            self.error = None;
        }
        for a in actions {
            self.run_action(ctx, a);
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if !self.cfg.close_to_tray {
            send("quit");
        }
    }
}

fn import_spl(path: &Path, error: &mut Option<String>) {
    match send(&format!("import-spl {}", path.display())) {
        Some(r) if r.starts_with("ERR") => *error = Some(r.trim_start_matches("ERR ").trim().to_string()),
        Some(r) => {
            if let Ok((_, missing)) = serde_json::from_str::<(usize, Vec<String>)>(&r) {
                if !missing.is_empty() {
                    *error = Some(tr!(
                        format!("Not found ({}): put the files next to the .spl and import again. First: {}", missing.len(), missing[0]),
                        format!("Не найдено ({}): положи файлы рядом со .spl и импортируй снова. Первый: {}", missing.len(), missing[0])
                    ));
                }
            }
        }
        None => {}
    }
}

fn language_combo(ui: &mut egui::Ui, cfg: &mut Config) {
    let label = |l: &Option<String>| match l.as_deref() {
        Some("en") => "English",
        Some("ru") => "Русский",
        _ => tr!("Auto", "Авто"),
    };
    let mut lang = cfg.language.clone();
    egui::ComboBox::from_id_salt(ui.next_auto_id()).width(90.0).selected_text(format!("🌐 {}", label(&lang))).show_ui(ui, |ui| {
        ui.selectable_value(&mut lang, None, label(&None));
        ui.selectable_value(&mut lang, Some("en".into()), "English");
        ui.selectable_value(&mut lang, Some("ru".into()), "Русский");
    });
    if lang != cfg.language {
        i18n::set_language(lang.as_deref());
        cfg.language = lang;
    }
}

fn percent(v: f64, _: std::ops::RangeInclusive<usize>) -> String {
    format!("{:.0}%", v * 100.0)
}

fn expand_home(p: &str) -> String {
    match (p.strip_prefix("~/"), std::env::var("HOME")) {
        (Some(rest), Ok(home)) => format!("{home}/{rest}"),
        _ => p.to_string(),
    }
}
