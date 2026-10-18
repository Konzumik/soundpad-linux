# Soundpad Linux

Быстрый и лёгкий саундпад для Linux на Rust.
Он создаёт виртуальный микрофон **«Soundpad Microphone»**, в который подмешиваются твой настоящий
микрофон и звуки. Этот микрофон можно выбрать в Discord, играх, OBS, Telegram, Zoom и любой другой программе.

Soundpad из Steam (Windows) на Linux не работает, поэтому этот проект написан как нативная замена.

[English version](README.md)

```
настоящий мик ──loopback──┐
                          ├──> soundpad_sink ──> «Soundpad Microphone» ──> Discord / игра / OBS
звуки (микшер) ───────────┘
        └──> твои наушники (мониторинг, отдельная громкость)
```

## Возможности

- Виртуальный микрофон через PipeWire (или PulseAudio), в который подмешивается настоящий мик.
- Без дополнительных программ играет **mp3, wav, ogg, flac, m4a, aac** и звук из **видео**
  (mp4, mov, mkv, m4v). С `ffmpeg` ещё opus, wma, webm и avi.
- Можно слышать звуки в своих наушниках (мониторинг), у этого своя громкость.
- Громкость каждого звука и общая громкость в микрофон, опционально несколько звуков одновременно.
- Глобальные хоткеи через настройки твоего окружения (работает и на Wayland): `soundpad play 3`.
- Задержка около 20 мс от команды до звука, звуки заранее загружаются в память.
- В простое ~0% CPU и несколько МБ памяти плюс сами звуки.
- Интерфейс на русском и английском: выбирается по системному языку, можно переключить в окне.
- Для сборки не нужны `-devel` пакеты, для работы нужны только `pactl` и `pacat`.

## Установка

Нужно:

- **Rust** 1.81+ (`cargo`) и линкер (`gcc`) — для сборки;
- **`pactl` + `pacat`** — для работы (утилиты PulseAudio, на PipeWire работают через `pipewire-pulse`);
- **`ffmpeg`** — необязательно, для opus / wma / webm / avi.

Поставь зависимости для своего дистрибутива, затем [собери и установи](#сборка-и-установка).

### Fedora

```sh
sudo dnf install cargo gcc pulseaudio-utils ffmpeg-free
```

### Arch Linux / Manjaro / EndeavourOS / CachyOS

```sh
sudo pacman -S --needed rust base-devel libpulse pipewire-pulse ffmpeg
```

(В Arch `pactl` и `pacat` входят в пакет `libpulse`.)

### Debian / Ubuntu / Linux Mint / Pop!_OS

`cargo` из репозиториев Debian/Ubuntu часто слишком старый, поэтому Rust лучше ставить через [rustup](https://rustup.rs):

```sh
sudo apt install build-essential curl pulseaudio-utils pipewire-pulse ffmpeg
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
```

На Ubuntu 22.04 и старше звуковым сервером может быть PulseAudio. Это нормально, просто не ставь `pipewire-pulse`.

### openSUSE (Tumbleweed / Leap)

```sh
sudo zypper install rust cargo gcc pulseaudio-utils pipewire-pulseaudio ffmpeg
```

### NixOS / Nix

```sh
nix-shell -p cargo rustc gcc pulseaudio ffmpeg
# окну нужны эти библиотеки во время работы (они подгружаются динамически):
export LD_LIBRARY_PATH=$(nix-build '<nixpkgs>' -A libxkbcommon --no-out-link)/lib:$(nix-build '<nixpkgs>' -A wayland --no-out-link)/lib:$(nix-build '<nixpkgs>' -A libGL --no-out-link)/lib
```

### Другие дистрибутивы

Поставь Rust (из пакетного менеджера или через [rustup](https://rustup.rs)), компилятор C
и пакет, в котором есть `pactl` и `pacat` (обычно `pulseaudio-utils` или `libpulse`).

### Сборка и установка

```sh
git clone https://github.com/Konzumik/soundpad-linux.git
cd soundpad-linux
./install.sh
```

Скрипт соберёт программу, установит её в `~/.local/bin/soundpad` и добавит ярлык в меню приложений.
Папка `~/.local/bin` должна быть в `PATH`.

Удаление: `./install.sh --uninstall` (список звуков в `~/.config/soundpad-linux` сохраняется).

Вручную:

```sh
cargo build --release
install -Dm755 target/release/soundpad ~/.local/bin/soundpad
install -Dm644 soundpad.desktop ~/.local/share/applications/soundpad.desktop
```

## Использование

1. Запусти `soundpad` или «Soundpad Linux» из меню приложений.
2. Добавь звуки: кнопкой **Добавить файлы…**, путём к файлу или папке, или перетаскиванием в окно.
3. Выбери свой настоящий микрофон в списке **Микрофон** (или «выключен», если нужны только звуки).
4. В Discord / игре / OBS выбери устройство ввода **Soundpad Microphone**.

> **Discord:** отключи шумоподавление (Krisp) в «Голос и видео», иначе оно глушит звуки.
> Если устройства нет в списке, перезапусти Discord после запуска Soundpad.

### Команды

| команда | что делает |
|---|---|
| `soundpad` | запуск с окном |
| `soundpad daemon` | запуск без окна (только команды и хоткеи) |
| `soundpad play <N\|имя>` | проиграть звук по номеру или имени (без учёта регистра, по подстроке) |
| `soundpad stop` | остановить все звуки |
| `soundpad list` | список звуков с номерами |

`Esc` в окне останавливает все звуки.

## Горячие клавиши

На Wayland приложения не могут перехватывать клавиши глобально, поэтому команды `soundpad play …`
привязываются к клавишам в настройках твоего окружения. Soundpad при этом должен быть запущен (окно или `daemon`).

Если окружение не видит `~/.local/bin`, укажи полный путь: `/home/ты/.local/bin/soundpad play 1`.

### Hyprland

`~/.config/hypr/hyprland.conf`:

```ini
bind = , KP_1, exec, soundpad play 1
bind = , KP_2, exec, soundpad play "имя звука"
bind = , KP_0, exec, soundpad stop
exec-once = soundpad          # автозапуск (или soundpad daemon)
```

### Sway

`~/.config/sway/config`:

```
bindsym KP_1 exec soundpad play 1
bindsym KP_0 exec soundpad stop
exec soundpad daemon
```

### i3

`~/.config/i3/config`:

```
bindsym KP_1 exec --no-startup-id soundpad play 1
bindsym KP_0 exec --no-startup-id soundpad stop
exec --no-startup-id soundpad daemon
```

С выключенным Num Lock клавиши цифрового блока называются `KP_End`, `KP_Down`, `KP_Insert` и т.д.
Точные имена клавиш покажет `wev` (Wayland) или `xev` (X11).

### GNOME

*Настройки → Клавиатура → Просмотр и настройка комбинаций клавиш → Дополнительные комбинации → +*,
команда `soundpad play 1`. Или из терминала:

```sh
p=/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/soundpad1/
gsettings set org.gnome.settings-daemon.plugins.media-keys custom-keybindings "['$p']"   # перезапишет существующие свои комбинации!
gsettings set org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:$p name 'Soundpad 1'
gsettings set org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:$p command 'soundpad play 1'
gsettings set org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:$p binding 'KP_1'
```

### KDE Plasma

*Параметры системы → Клавиатура → Комбинации клавиш → Добавить → Команда или сценарий…*,
команда `soundpad play 1`, затем назначь клавишу.

### Xfce / Cinnamon / MATE

- **Xfce:** *Настройки → Клавиатура → Комбинации клавиш приложений → Добавить*.
- **Cinnamon:** *Параметры системы → Клавиатура → Комбинации клавиш → Добавить свою*.
- **MATE:** *Центр управления → Комбинации клавиш → Добавить*.

Команда: `soundpad play 1` и т.д.

### Автозапуск (GNOME, KDE, Xfce, …)

```sh
mkdir -p ~/.config/autostart
cp ~/.local/share/applications/soundpad.desktop ~/.config/autostart/
```

## Настройки

Настройки и список звуков хранятся в `~/.config/soundpad-linux/config.json`:

```json
{
  "sounds": [{ "name": "airhorn", "path": "/home/you/Sounds/airhorn.mp3", "volume": 1.0 }],
  "mic_source": "alsa_input.usb-...",
  "master_volume": 1.0,
  "monitor_enabled": true,
  "monitor_volume": 0.6,
  "allow_overlap": false,
  "language": null
}
```

`language`: `"en"`, `"ru"` или `null` (как в системе).
Одновременно может работать только один экземпляр, поэтому перед ручной правкой файла закрой Soundpad.

## Если что-то не работает

- **В программе нет «Soundpad Microphone»:** сначала запусти Soundpad, потом перезапусти программу.
  Проверить, что устройство есть: `pactl list short sources | grep soundpad`.
- **После падения остались лишние устройства:** просто запусти `soundpad` снова, при старте он их удалит.
  Вручную: `pactl list short modules | grep soundpad`, затем `pactl unload-module <id>`.
- **Звук обрезается или его не слышно у собеседников:** отключи в программе шумоподавление, эхоподавление и автоусиление.
- **Хрипит:** уменьши громкость звука или «Громкость в микрофон». Больше 100% может давать перегруз.
- **Файл не играет:** установи `ffmpeg`. Какого кодека не хватает, видно в строке статуса.
- **Длинные файлы:** декодированный звук держится в памяти (примерно 11 МБ на минуту), так что программа рассчитана на короткие клипы, а не на альбомы.

## Как это работает

При запуске Soundpad через `pactl` загружает три модуля PulseAudio (на PipeWire их обрабатывает pipewire-pulse):

1. `module-null-sink`: `soundpad_sink`, где всё смешивается;
2. `module-remap-source`: `soundpad_mic` («Soundpad Microphone»), сделанный из монитора этого синка;
3. `module-loopback`: твой настоящий микрофон → `soundpad_sink`.

Звуки декодируются через [Symphonia](https://github.com/pdeljanov/Symphonia) (если не вышло, через `ffmpeg`),
переводятся в 48 кГц стерео, смешиваются на Rust и отправляются потоком PCM в два постоянно работающих
процесса `pacat`: один пишет в виртуальный синк, другой в наушники. Буфер канала уменьшен до одной
страницы памяти, поэтому звук останавливается почти мгновенно. При выходе модули выгружаются.

Интерфейс сделан на [egui](https://github.com/emilk/egui). Команды `play` / `stop` / `list` передаются
запущенному экземпляру через Unix-сокет `$XDG_RUNTIME_DIR/soundpad-linux.sock`.

## Лицензия

[MIT](LICENSE)
