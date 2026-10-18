# Soundpad Linux

Быстрый нативный **саундпад и модулятор голоса** для Linux на Rust.
Он создаёт виртуальный микрофон **«Soundpad Microphone»**, в который идут твой настоящий микрофон
(при желании с эффектами и шумоподавлением) и звуки. Этот микрофон можно выбрать в Discord, играх, OBS,
Telegram, Zoom и любой другой программе.

Soundpad из Steam (Windows) на Linux не работает, поэтому этот проект написан как нативная замена.
Заодно он закрывает то, чем чаще всего пользуются в Voicemod и Krisp.

[English version](README.md)

```
настоящий мик ──(loopback или эффекты + RNNoise)──┐
                                                  ├──> «Soundpad Microphone» ──> Discord / игра / OBS
звуки (микшер) ───────────────────────────────────┘
        └──> твои наушники (мониторинг, своя громкость и устройство)
```

## Возможности

**Саундпад**
- Без дополнительных программ играет **mp3, wav, ogg, flac, m4a, aac** и звук из **видео**
  (mp4, mov, mkv, m4v). С `ffmpeg` ещё opus, wma, webm и avi.
- **Глобальные хоткеи прямо в программе:** нажми кнопку хоткея у звука и нажми клавишу или сочетание
  (Ctrl/Alt/Shift/Super + клавиша). Работает на Wayland (Hyprland, Sway, KDE, GNOME…) и X11.
- **Обрезка** начала и конца звука, **нарастание и затухание**, **категории**, поиск.
- **Выравнивание громкости**, чтобы все звуки звучали примерно одинаково. У каждого звука ещё и своя громкость.
- **Импорт из Soundpad для Windows** (списки `.spl`). Файлы ищутся по имени рядом со списком.
- Мониторинг: звуки слышно в наушниках, у этого своя громкость и своё устройство.
- Можно включить одновременное воспроизведение нескольких звуков. Остановка без щелчков,
  мягкий лимитер не даёт перегруза при наложении.

**Голос**
- **Эффекты голоса:** Бурундук, Низкий голос, Демон, Робот, Пришелец, Рация, Эхо, Пещера.
  Каждому эффекту можно назначить хоткей, он включает и выключает эффект.
- **Шумоподавление** (встроенный RNNoise), громкость микрофона, можно слышать себя.
- **Запись последних секунд:** программа хранит последние 5–120 секунд звука с компьютера,
  и одной клавишей их можно сохранить как новый звук.

**Система**
- Работает как **фоновая служба** с **иконкой в трее**. Закрытие окна не выключает микрофон и хоткеи.
- Команды для скриптов и биндов композитора: `soundpad play 3`, `soundpad voice robot`, `soundpad replay`…
- Задержка около 20 мс от клавиши до звука. Звуки заранее загружены, а микрофон без обработки
  идёт через PipeWire loopback без лишней нагрузки.
- В простое ~0% CPU и несколько МБ памяти плюс сами звуки.
- Интерфейс на русском и английском: выбирается автоматически, можно переключить в окне.
- Для сборки не нужны `-devel` пакеты, для работы нужны только `pactl`, `pacat` и `parec`.

## Установка

### Готовая сборка (любой дистрибутив x86_64)

Скачай `soundpad-linux-*-x86_64.tar.gz` из [последнего релиза](https://github.com/Konzumik/soundpad-linux/releases/latest) и выполни:

```sh
tar xzf soundpad-linux-*-x86_64.tar.gz
cd soundpad-linux-*-x86_64
./install.sh
```

Программа установится в `~/.local/bin/soundpad` и появится в меню приложений. Утилиты для работы
всё равно нужны из твоего дистрибутива (пакет `pulseaudio-utils` / `libpulse`, см. ниже).

### Arch Linux (AUR)

`PKGBUILD` лежит в [`packaging/aur`](packaging/aur/PKGBUILD):

```sh
git clone https://github.com/Konzumik/soundpad-linux.git
cd soundpad-linux/packaging/aur && makepkg -si
```

### Сборка из исходников

Нужно:

- **Rust** 1.81+ (`cargo`) и линкер (`gcc`) — для сборки;
- **`pactl`, `pacat`, `parec`** — для работы (утилиты PulseAudio, на PipeWire работают через `pipewire-pulse`);
- **`ffmpeg`** — необязательно, для opus / wma / webm / avi.

Поставь зависимости для своего дистрибутива:

| Дистрибутив | Команда |
|---|---|
| **Fedora** | `sudo dnf install cargo gcc pulseaudio-utils ffmpeg-free` |
| **Arch / Manjaro / EndeavourOS / CachyOS** | `sudo pacman -S --needed rust base-devel libpulse pipewire-pulse ffmpeg` |
| **Debian / Ubuntu / Mint / Pop!_OS** | `sudo apt install build-essential curl pulseaudio-utils pipewire-pulse ffmpeg`, потом Rust через [rustup](https://rustup.rs) (`cargo` из репозиториев часто слишком старый) |
| **openSUSE** | `sudo zypper install rust cargo gcc pulseaudio-utils pipewire-pulseaudio ffmpeg` |

Другие дистрибутивы: поставь Rust, компилятор C и пакет, в котором есть `pactl`, `pacat` и `parec`
(обычно `pulseaudio-utils` или `libpulse`).

На Ubuntu 22.04 и старше звуковым сервером может быть PulseAudio. Это нормально, просто не ставь `pipewire-pulse`.

**NixOS / Nix:**

```sh
nix-shell -p cargo rustc gcc pulseaudio ffmpeg
# окну нужны эти библиотеки во время работы (они подгружаются динамически):
export LD_LIBRARY_PATH=$(nix-build '<nixpkgs>' -A libxkbcommon --no-out-link)/lib:$(nix-build '<nixpkgs>' -A wayland --no-out-link)/lib:$(nix-build '<nixpkgs>' -A libGL --no-out-link)/lib
```

Затем собери и установи:

```sh
git clone https://github.com/Konzumik/soundpad-linux.git
cd soundpad-linux
./install.sh
```

Удаление: `./install.sh --uninstall` (звуки и настройки в `~/.config/soundpad-linux` сохраняются).

### Разрешить глобальные хоткеи

Хоткеи в программе читают клавиатуру напрямую, для этого нужна группа `input`:

```sh
sudo usermod -aG input $USER    # потом перезайди в систему
```

Если не хочешь этого делать, привяжи клавиши в настройках окружения (см. [Горячие клавиши](#горячие-клавиши)).

## Использование

1. Запусти `soundpad` или «Soundpad Linux» из меню. Фоновая служба запустится сама.
2. **Вкладка «Звуки»:** добавь звуки кнопкой **Добавить файлы…**, путём или перетаскиванием в окно.
   Нажми кнопку **⌨** у звука и нажми клавишу, чтобы назначить хоткей (правый клик убирает его).
   **✂** открывает обрезку, затухание и выбор категории.
3. **Вкладка «Голос»:** выбери свой настоящий микрофон, при желании включи шумоподавление или эффект.
4. В Discord / игре / OBS выбери устройство ввода **Soundpad Microphone**.

> **Discord:** отключи шумоподавление (Krisp) и «Автоматическую регулировку усиления» в «Голос и видео»,
> иначе они глушат звуки. Вместо Krisp можно включить RNNoise в самом Soundpad.
> Если устройства нет в списке, перезапусти Discord после запуска Soundpad.

После закрытия окна Soundpad продолжает работать в трее. Выйти можно кнопкой 🚪, через меню трея или `soundpad quit`.
Чтобы закрытие окна завершало программу, выключи *Настройки → Работать в фоне после закрытия окна*.

### Переезд с Soundpad для Windows

В Soundpad для Windows сохрани список звуков в файл `.spl` и перенеси его вместе со звуками на Linux. Затем нажми **📥 Импорт из Soundpad (.spl)…** или перетащи `.spl` в окно.
Файлы ищутся по исходному пути, по пути Wine (`Z:\`) и по имени в папке со `.spl` и её подпапках.

### Команды

| команда | что делает |
|---|---|
| `soundpad` | открыть окно (при необходимости запустит фоновую службу) |
| `soundpad daemon` | запустить фоновую службу в этом терминале (для автозапуска) |
| `soundpad play <N\|имя>` | проиграть звук по номеру или имени (без учёта регистра, по подстроке) |
| `soundpad stop` | остановить все звуки |
| `soundpad list` | список звуков с номерами и хоткеями |
| `soundpad voice <эффект>` | `off`, `chipmunk`, `deep`, `demon`, `robot`, `alien`, `radio`, `echo`, `cave` |
| `soundpad replay` | сохранить запись последних секунд как звук |
| `soundpad show` | открыть окно / переключиться на него |
| `soundpad quit` | остановить фоновую службу |

`Esc` в окне останавливает все звуки.

## Горячие клавиши

**Хоткеи в программе** (рекомендуется) работают везде, если ты в группе `input`. Клавиши не перехватываются:
активная программа их тоже получает, поэтому лучше брать клавиши, которыми не печатаешь
(цифровой блок, F13–F24 или сочетания с модификаторами).

**Или** привяжи `soundpad play …` к клавишам в настройках своего окружения. Soundpad при этом должен быть запущен.
Если окружение не видит `~/.local/bin`, укажи полный путь: `/home/ты/.local/bin/soundpad play 1`.

<details>
<summary><b>Hyprland</b></summary>

`~/.config/hypr/hyprland.conf`:

```ini
bind = , KP_1, exec, soundpad play 1
bind = , KP_2, exec, soundpad play "имя звука"
bind = , KP_5, exec, soundpad voice robot
bind = , KP_0, exec, soundpad stop
exec-once = soundpad daemon      # автозапуск
```
</details>

<details>
<summary><b>Sway</b></summary>

`~/.config/sway/config`:

```
bindsym KP_1 exec soundpad play 1
bindsym KP_0 exec soundpad stop
exec soundpad daemon
```
</details>

<details>
<summary><b>i3</b></summary>

`~/.config/i3/config`:

```
bindsym KP_1 exec --no-startup-id soundpad play 1
bindsym KP_0 exec --no-startup-id soundpad stop
exec --no-startup-id soundpad daemon
```

С выключенным Num Lock клавиши цифрового блока называются `KP_End`, `KP_Down`, `KP_Insert` и т.д.
Точные имена клавиш покажет `wev` (Wayland) или `xev` (X11).
</details>

<details>
<summary><b>GNOME</b></summary>

*Настройки → Клавиатура → Просмотр и настройка комбинаций клавиш → Дополнительные комбинации → +*,
команда `soundpad play 1`. Или из терминала:

```sh
p=/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/soundpad1/
gsettings set org.gnome.settings-daemon.plugins.media-keys custom-keybindings "['$p']"   # перезапишет существующие свои комбинации!
gsettings set org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:$p name 'Soundpad 1'
gsettings set org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:$p command 'soundpad play 1'
gsettings set org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:$p binding 'KP_1'
```

Чтобы в GNOME была видна иконка в трее, нужно расширение [AppIndicator](https://extensions.gnome.org/extension/615/appindicator-support/).
</details>

<details>
<summary><b>KDE Plasma</b></summary>

*Параметры системы → Клавиатура → Комбинации клавиш → Добавить → Команда или сценарий…*,
команда `soundpad play 1`, затем назначь клавишу.
</details>

<details>
<summary><b>Xfce / Cinnamon / MATE</b></summary>

- **Xfce:** *Настройки → Клавиатура → Комбинации клавиш приложений → Добавить*.
- **Cinnamon:** *Параметры системы → Клавиатура → Комбинации клавиш → Добавить свою*.
- **MATE:** *Центр управления → Комбинации клавиш → Добавить*.

Команда: `soundpad play 1` и т.д.
</details>

### Автозапуск

Hyprland / Sway / i3: см. `exec-once` / `exec` выше. В GNOME, KDE, Xfce и других:

```sh
mkdir -p ~/.config/autostart
sed 's/^Exec=soundpad$/Exec=soundpad daemon/' ~/.local/share/applications/soundpad.desktop > ~/.config/autostart/soundpad.desktop
```

## Настройки

Всё, что настраивается в окне, сохраняется в `~/.config/soundpad-linux/config.json` (вручную правь только при остановленном Soundpad).
Записи последних секунд сохраняются в `~/.local/share/soundpad-linux/recordings/`, лог фоновой службы пишется
в `~/.local/state/soundpad-linux/soundpad-linux.log`.

## Если что-то не работает

- **В программе нет «Soundpad Microphone»:** сначала запусти Soundpad, потом перезапусти программу.
  Проверить, что устройство есть: `pactl list short sources | grep soundpad`.
- **Хоткеи не срабатывают:** загляни в *Настройки → Горячие клавиши*. Надпись «Нет доступа к клавиатурам» значит,
  что ты ещё не в группе `input` (после `usermod` нужно перезайти).
- **После падения остались лишние устройства:** просто запусти `soundpad` снова, при старте он их удалит.
  Вручную: `pactl list short modules | grep soundpad`, затем `pactl unload-module <id>`.
- **Звук обрезается или его не слышно у собеседников:** отключи в программе шумоподавление, эхоподавление и автоусиление.
- **С эффектами голос запаздывает:** эффекты, шумоподавление и громкость микрофона добавляют около 50 мс. Когда всё
  это выключено, микрофон идёт напрямую через PipeWire loopback.
- **Хрипит:** уменьши громкость звука или «Громкость в микрофон».
- **Файл не играет:** установи `ffmpeg`. Какого кодека не хватает, видно в строке статуса.
- **Длинные файлы:** декодированный звук держится в памяти (примерно 11 МБ на минуту), так что программа рассчитана на короткие клипы, а не на альбомы.

## Как это работает

`soundpad daemon` — это фоновая служба. При запуске она через `pactl` загружает модули PulseAudio
(на PipeWire их обрабатывает pipewire-pulse): `module-null-sink` (`soundpad_sink`, где всё смешивается)
и `module-remap-source` (`soundpad_mic`, «Soundpad Microphone», сделанный из монитора этого синка).
Настоящий микрофон попадает туда одним из двух способов:
- без эффектов — через `module-loopback`, этим занимается сам PipeWire, без лишней нагрузки;
- с эффектами — захватывается через `parec`, обрабатывается кадрами по 10 мс ([nnnoiseless](https://github.com/jneem/nnnoiseless)
  RNNoise и цепочка DSP: сдвиг высоты на линии задержки, кольцевая модуляция, перегруз, полосовой биквад-фильтр,
  эхо, реверберация в стиле Freeverb) и подмешивается через небольшой джиттер-буфер.

Звуки декодируются через [Symphonia](https://github.com/pdeljanov/Symphonia) (если не вышло, через `ffmpeg`),
переводятся в 48 кГц стерео, смешиваются на Rust и отправляются потоком PCM в постоянно работающие процессы `pacat`
(один для виртуального микрофона, другой для наушников). Буфер канала уменьшен до одной страницы памяти,
поэтому звук останавливается почти мгновенно.

Хоткеи читаются из `/dev/input` через [evdev](https://github.com/emberian/evdev), иконка в трее —
StatusNotifierItem через [ksni](https://github.com/iovxw/ksni). Окно ([egui](https://github.com/emilk/egui)) —
отдельный процесс, который общается со службой через Unix-сокет `$XDG_RUNTIME_DIR/soundpad-linux.sock`.
Через этот же сокет работают команды.

## Лицензия

[MIT](LICENSE)
