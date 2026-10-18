use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

use eframe::egui::{self, Align, Color32, Layout, RichText};

use crate::config::{Config, AUDIO_EXTS};
use crate::engine::Core;
use crate::i18n;
use crate::vmic::{self, SourceInfo};

pub fn run(core: Arc<Core>) -> Result<(), String> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Soundpad Linux")
            .with_app_id("soundpad-linux")
            .with_inner_size([780.0, 580.0])
            .with_min_inner_size([520.0, 360.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        "Soundpad Linux",
        options,
        Box::new(move |cc| Ok(Box::new(GuiApp::new(cc, core)))),
    )
    .map_err(|e| e.to_string())
}

enum Action {
    Play(usize),
    StopAll,
    SetMic(Option<String>),
    PickFiles,
}

struct GuiApp {
    core: Arc<Core>,
    sources: Vec<SourceInfo>,
    path_input: String,
    filter: String,
    picked_tx: Sender<Vec<PathBuf>>,
    picked_rx: Receiver<Vec<PathBuf>>,
}

fn added_status(n: usize) -> String {
    if n == 0 {
        tr!("Nothing added: file not found or already in the list", "Ничего не добавлено: файл не найден или уже в списке").into()
    } else {
        tr!(format!("Sounds added: {n}"), format!("Добавлено звуков: {n}"))
    }
}

impl GuiApp {
    fn new(cc: &eframe::CreationContext<'_>, core: Arc<Core>) -> Self {
        let ctx = cc.egui_ctx.clone();
        core.set_notifier(Box::new(move || ctx.request_repaint()));
        let (picked_tx, picked_rx) = channel();
        GuiApp {
            core,
            sources: vmic::list_sources(),
            path_input: String::new(),
            filter: String::new(),
            picked_tx,
            picked_rx,
        }
    }

    fn settings_ui(&mut self, ui: &mut egui::Ui, cfg: &mut Config, actions: &mut Vec<Action>) {
        egui::Grid::new("settings").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label(tr!("Microphone:", "Микрофон:"));
            ui.horizontal(|ui| {
                let label_of = |name: &Option<String>| match name {
                    None => tr!("— off (sounds only) —", "— выключен (только звуки) —").to_string(),
                    Some(n) => self
                        .sources
                        .iter()
                        .find(|s| &s.name == n)
                        .map(|s| s.description.clone())
                        .unwrap_or_else(|| tr!(format!("{n} (not found)"), format!("{n} (не найден)"))),
                };
                let mut selected = cfg.mic_source.clone();
                egui::ComboBox::from_id_salt("mic")
                    .width(320.0)
                    .selected_text(label_of(&selected))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut selected, None, label_of(&None));
                        for s in &self.sources {
                            ui.selectable_value(&mut selected, Some(s.name.clone()), &s.description);
                        }
                    });
                if selected != cfg.mic_source {
                    actions.push(Action::SetMic(selected));
                }
                if ui.button("⟳").on_hover_text(tr!("Refresh device list", "Обновить список устройств")).clicked() {
                    self.sources = vmic::list_sources();
                }
            });
            ui.end_row();

            ui.label(tr!("Volume into mic:", "Громкость в микрофон:"));
            ui.add(egui::Slider::new(&mut cfg.master_volume, 0.0..=2.0).custom_formatter(percent));
            ui.end_row();

            ui.checkbox(&mut cfg.monitor_enabled, tr!("Hear in headphones:", "Слышать в наушниках:"));
            ui.add_enabled(
                cfg.monitor_enabled,
                egui::Slider::new(&mut cfg.monitor_volume, 0.0..=2.0).custom_formatter(percent),
            );
            ui.end_row();

            ui.label("");
            ui.checkbox(&mut cfg.allow_overlap, tr!("Allow several sounds at once", "Разрешить несколько звуков одновременно"));
            ui.end_row();
        });
    }

    fn add_ui(&mut self, ui: &mut egui::Ui, cfg: &mut Config, actions: &mut Vec<Action>) {
        ui.horizontal(|ui| {
            if ui.button(tr!("➕ Add files…", "➕ Добавить файлы…")).clicked() {
                actions.push(Action::PickFiles);
            }
            let add_label = tr!("Add", "Добавить");
            let resp = ui.add(
                egui::TextEdit::singleline(&mut self.path_input)
                    .hint_text(tr!(
                        "…or a path to a file/folder (or drop files into the window)",
                        "…или путь к файлу/папке (или перетащи файлы в окно)"
                    ))
                    .desired_width(ui.available_width() - 90.0),
            );
            let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (ui.button(add_label).clicked() || enter) && !self.path_input.trim().is_empty() {
                let raw = self.path_input.trim().trim_matches(|c| c == '"' || c == '\'');
                let n = cfg.add_paths([PathBuf::from(shellexpand_home(raw))]);
                self.core.set_status(added_status(n));
                self.path_input.clear();
            }
        });
    }

    fn sounds_ui(&mut self, ui: &mut egui::Ui, cfg: &mut Config, actions: &mut Vec<Action>) {
        let playing = self.core.mixer.playing();
        let filter = self.filter.to_lowercase();
        let mut remove = None;
        let mut swap = None;
        let count = cfg.sounds.len();

        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            egui::Grid::new("sounds").striped(true).num_columns(5).spacing([8.0, 6.0]).show(ui, |ui| {
                for (i, sound) in cfg.sounds.iter_mut().enumerate() {
                    if !filter.is_empty() && !sound.name.to_lowercase().contains(&filter) {
                        continue;
                    }
                    let progress = playing.iter().find(|(p, _)| *p == sound.path).map(|(_, f)| *f);

                    ui.label(RichText::new(format!("{:>3}", i + 1)).monospace().weak());

                    let (icon, hint) = if progress.is_some() {
                        ("⏹", tr!("Stop", "Остановить"))
                    } else {
                        ("▶", tr!("Play", "Играть"))
                    };
                    if ui.button(RichText::new(icon).size(16.0)).on_hover_text(hint).clicked() {
                        if progress.is_some() {
                            self.core.mixer.stop(&sound.path);
                        } else {
                            actions.push(Action::Play(i));
                        }
                    }

                    ui.vertical(|ui| {
                        let color = if progress.is_some() { Color32::from_rgb(110, 200, 120) } else { ui.visuals().text_color() };
                        ui.add(
                            egui::TextEdit::singleline(&mut sound.name)
                                .text_color(color)
                                .desired_width(280.0),
                        )
                        .on_hover_text(sound.path.display().to_string());
                        if let Some(f) = progress {
                            ui.add(egui::ProgressBar::new(f).desired_width(280.0).desired_height(4.0));
                        }
                    });

                    ui.add(
                        egui::Slider::new(&mut sound.volume, 0.0..=2.0)
                            .custom_formatter(percent)
                            .show_value(true),
                    )
                    .on_hover_text(tr!("Volume of this sound", "Громкость этого звука"));

                    ui.horizontal(|ui| {
                        if ui.add_enabled(i > 0, egui::Button::new("↑")).clicked() {
                            swap = Some((i, i - 1));
                        }
                        if ui.add_enabled(i + 1 < count, egui::Button::new("↓")).clicked() {
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
                    ui.label(
                        RichText::new(tr!(
                            format!("Drop audio or video files here ({exts})"),
                            format!("Перетащи сюда аудио- или видеофайлы ({exts})")
                        ))
                        .weak(),
                    );
                });
            }
        });

        if let Some((a, b)) = swap {
            cfg.sounds.swap(a, b);
        }
        if let Some(i) = remove {
            let s = cfg.sounds.remove(i);
            self.core.mixer.stop(&s.path);
            self.core.mixer.forget(&s.path);
        }
    }
}

impl eframe::App for GuiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let before = self.core.config.lock().unwrap().clone();
        let mut cfg = before.clone();
        let mut actions = Vec::new();

        let mut new_paths: Vec<PathBuf> =
            ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        while let Ok(p) = self.picked_rx.try_recv() {
            new_paths.extend(p);
        }
        if !new_paths.is_empty() {
            let n = cfg.add_paths(new_paths);
            self.core.set_status(added_status(n));
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            actions.push(Action::StopAll);
        }

        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.heading("🎵 Soundpad Linux");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button(RichText::new(tr!("⏹ Stop all", "⏹ Стоп всё")).strong()).on_hover_text("Esc").clicked() {
                        actions.push(Action::StopAll);
                    }
                    let lang_label = |l: &Option<String>| match l.as_deref() {
                        Some("en") => "English",
                        Some("ru") => "Русский",
                        _ => tr!("Auto", "Авто"),
                    };
                    let mut lang = cfg.language.clone();
                    egui::ComboBox::from_id_salt("lang")
                        .width(90.0)
                        .selected_text(format!("🌐 {}", lang_label(&lang)))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut lang, None, lang_label(&None));
                            ui.selectable_value(&mut lang, Some("en".into()), "English");
                            ui.selectable_value(&mut lang, Some("ru".into()), "Русский");
                        });
                    if lang != cfg.language {
                        i18n::set_language(lang.as_deref());
                        cfg.language = lang;
                    }
                });
            });
            ui.separator();
            self.settings_ui(ui, &mut cfg, &mut actions);
            ui.add_space(6.0);
        });

        egui::TopBottomPanel::bottom("bottom").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.label(self.core.status());
            ui.collapsing(tr!("⌨ Hotkeys", "⌨ Горячие клавиши"), |ui| {
                ui.label(tr!(
                    "Wayland doesn't let apps grab global hotkeys, so bind them in your compositor / DE. Hyprland example:",
                    "Wayland не даёт приложениям глобальные хоткеи, поэтому они настраиваются в композиторе. Пример для Hyprland:"
                ));
                ui.code(tr!(
                    "bind = , KP_1, exec, soundpad play 1\nbind = , KP_2, exec, soundpad play \"sound name\"\nbind = , KP_0, exec, soundpad stop",
                    "bind = , KP_1, exec, soundpad play 1\nbind = , KP_2, exec, soundpad play \"имя звука\"\nbind = , KP_0, exec, soundpad stop"
                ));
                ui.hyperlink_to(
                    tr!("Guides for other desktops (GNOME, KDE, Sway, i3…)", "Инструкции для других окружений (GNOME, KDE, Sway, i3…)"),
                    "https://github.com/Konzumik/soundpad-linux#hotkeys",
                );
            });
            ui.add_space(4.0);
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            self.add_ui(ui, &mut cfg, &mut actions);
            ui.horizontal(|ui| {
                ui.label("🔍");
                ui.add(egui::TextEdit::singleline(&mut self.filter).hint_text(tr!("Search", "Поиск")).desired_width(200.0));
                ui.label(RichText::new(tr!(format!("sounds: {}", cfg.sounds.len()), format!("звуков: {}", cfg.sounds.len()))).weak());
            });
            ui.separator();
            self.sounds_ui(ui, &mut cfg, &mut actions);
        });

        if cfg != before {
            let added = cfg.sounds.iter().any(|s| !before.sounds.iter().any(|b| b.path == s.path));
            *self.core.config.lock().unwrap() = cfg.clone();
            cfg.save();
            self.core.apply_levels();
            if added {
                self.core.preload_all();
            }
        }

        for action in actions {
            match action {
                Action::Play(i) => {
                    let _ = self.core.play_index(i, false);
                }
                Action::StopAll => self.core.stop(),
                Action::SetMic(m) => self.core.set_mic(m),
                Action::PickFiles => {
                    let tx = self.picked_tx.clone();
                    let ctx = ctx.clone();
                    std::thread::spawn(move || {
                        if let Some(files) = rfd::FileDialog::new()
                            .set_title(tr!("Choose sounds", "Выбери звуки"))
                            .add_filter(tr!("Audio / video", "Аудио / видео"), AUDIO_EXTS)
                            .pick_files()
                        {
                            let _ = tx.send(files);
                            ctx.request_repaint();
                        }
                    });
                }
            }
        }

        if !self.core.mixer.playing().is_empty() {
            ctx.request_repaint_after(Duration::from_millis(33));
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.core.shutdown();
    }
}

fn percent(v: f64, _: std::ops::RangeInclusive<usize>) -> String {
    format!("{:.0}%", v * 100.0)
}

fn shellexpand_home(p: &str) -> String {
    match (p.strip_prefix("~/"), std::env::var("HOME")) {
        (Some(rest), Ok(home)) => format!("{home}/{rest}"),
        _ => p.to_string(),
    }
}
