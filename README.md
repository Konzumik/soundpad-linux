# Soundpad Linux

A fast, native **soundboard and voice changer** for Linux, written in Rust.
It creates a virtual microphone, **"Soundpad Microphone"**, that carries your real
microphone (optionally with voice effects and noise suppression) plus any sounds you play.
You can use it in Discord, games, OBS, Telegram, Zoom or anything else that records from a mic.

The Windows Soundpad from Steam doesn't run on Linux, so this was written as a native replacement.
It also covers the parts of Voicemod and Krisp that people use most.

[Русская версия / Russian version](README.ru.md)

```
real mic ──(loopback, or effects + RNNoise)──┐
                                             ├──> "Soundpad Microphone" ──> Discord / game / OBS
sounds (built-in mixer) ─────────────────────┘
        └──> your headphones (monitoring, separate volume and device)
```

## Features

**Soundboard**
- Plays **mp3, wav, ogg, flac, m4a, aac** natively, and also the audio track of **video files**
  (mp4, mov, mkv, m4v). With `ffmpeg` installed it also plays opus, wma, webm and avi.
- **Global hotkeys inside the app:** click the hotkey button next to a sound, then press a key or combo
  (Ctrl/Alt/Shift/Super + key). Works on Wayland (Hyprland, Sway, KDE, GNOME…) and X11.
- **Trim** the start and end of a sound, with **fade in/out**. Sounds can be sorted into **categories**, and you can search the list.
- **Volume normalization** brings every sound to a similar loudness. Each sound also has its own volume.
- **Import from Windows Soundpad** (`.spl` sound lists). Files are found by name next to the list.
- Monitoring: hear the sounds in your headphones, with their own volume and output device.
- Several sounds can play at once (optional). Stopping fades out without clicks, and a soft limiter keeps overlapping sounds from clipping.

**Voice**
- **Voice changer** presets: Chipmunk, Deep voice, Demon, Robot, Alien, Radio, Echo, Cave.
  Each preset can get a hotkey that toggles it.
- **Noise suppression** (RNNoise, built in), mic volume, and an option to hear yourself.
- **Instant replay:** keep the last 5–120 seconds of desktop audio and save them as a new sound with one key.

**System**
- Runs as a **background service** with a **tray icon**. Closing the window keeps the mic and hotkeys working.
- CLI for scripts and compositor binds: `soundpad play 3`, `soundpad voice robot`, `soundpad replay`…
- Low latency: about 20 ms from key to sound. Sounds are preloaded, and an untouched mic goes through a
  zero-overhead PipeWire loopback.
- Small footprint: ~0% CPU when idle, a few MB of RAM plus the decoded sounds.
- English and Russian interface (auto-detected; switchable in the window).
- No `-devel` packages needed to build. At runtime it needs only `pactl`, `pacat` and `parec`.

## Installation

### Prebuilt binary (any x86_64 distro)

Download `soundpad-linux-*-x86_64.tar.gz` from the [latest release](https://github.com/Konzumik/soundpad-linux/releases/latest), then:

```sh
tar xzf soundpad-linux-*-x86_64.tar.gz
cd soundpad-linux-*-x86_64
./install.sh
```

The binary goes to `~/.local/bin/soundpad` and an entry is added to your app menu. You still need the
runtime tools from your distro (the `pulseaudio-utils` / `libpulse` package, see below).

### Arch Linux (AUR)

A `PKGBUILD` is in [`packaging/aur`](packaging/aur/PKGBUILD):

```sh
git clone https://github.com/Konzumik/soundpad-linux.git
cd soundpad-linux/packaging/aur && makepkg -si
```

### Build from source

You need:

- **Rust** 1.81+ (`cargo`) and a C linker (`gcc`) to build;
- **`pactl`, `pacat`, `parec`** at runtime (PulseAudio client tools; they talk to PipeWire through `pipewire-pulse`);
- optionally **`ffmpeg`** for opus / wma / webm / avi.

Install the dependencies for your distro:

| Distro | Command |
|---|---|
| **Fedora** | `sudo dnf install cargo gcc pulseaudio-utils ffmpeg-free` |
| **Arch / Manjaro / EndeavourOS / CachyOS** | `sudo pacman -S --needed rust base-devel libpulse pipewire-pulse ffmpeg` |
| **Debian / Ubuntu / Mint / Pop!_OS** | `sudo apt install build-essential curl pulseaudio-utils pipewire-pulse ffmpeg`, then Rust via [rustup](https://rustup.rs) (the distro's `cargo` is often too old) |
| **openSUSE** | `sudo zypper install rust cargo gcc pulseaudio-utils pipewire-pulseaudio ffmpeg` |

Other distros: install a Rust toolchain, a C compiler and the package that provides `pactl`, `pacat` and `parec`
(usually called `pulseaudio-utils` or `libpulse`).

On Ubuntu 22.04 and older, PulseAudio may still be your sound server. That's fine: leave out `pipewire-pulse`.

**NixOS / Nix:**

```sh
nix-shell -p cargo rustc gcc pulseaudio ffmpeg
# the window needs these libraries at runtime (they are loaded dynamically):
export LD_LIBRARY_PATH=$(nix-build '<nixpkgs>' -A libxkbcommon --no-out-link)/lib:$(nix-build '<nixpkgs>' -A wayland --no-out-link)/lib:$(nix-build '<nixpkgs>' -A libGL --no-out-link)/lib
```

Then build and install:

```sh
git clone https://github.com/Konzumik/soundpad-linux.git
cd soundpad-linux
./install.sh
```

Uninstall: `./install.sh --uninstall` (your sounds and settings in `~/.config/soundpad-linux` are kept).

### Allow global hotkeys

In-app hotkeys read the keyboard directly, which requires the `input` group:

```sh
sudo usermod -aG input $USER    # then log out and back in
```

If you'd rather not do that, bind keys in your desktop instead (see [Hotkeys](#hotkeys)).

## Usage

1. Run `soundpad` (or open "Soundpad Linux" from your app menu). The background service starts automatically.
2. **Sounds tab:** add sounds with **Add files…**, a path, or by dragging files into the window.
   Click the **⌨** button next to a sound and press a key to assign a hotkey (right-click clears it).
   **✂** opens trimming, fades and the category.
3. **Voice tab:** pick your real microphone, and optionally turn on noise suppression or a voice effect.
4. In Discord / your game / OBS, select **Soundpad Microphone** as the input device.

> **Discord:** turn off noise suppression (Krisp) and "Automatic gain control" in *Voice & Video*, otherwise
> sound effects get filtered out (you can use Soundpad's own RNNoise instead).
> If the device does not show up, restart Discord after starting Soundpad.

Closing the window keeps Soundpad running in the tray. Quit it with the 🚪 button, the tray menu or `soundpad quit`.
To quit when the window is closed, turn off *Settings → Keep running in the background*.

### Moving from Windows Soundpad

In Windows Soundpad save your sound list as a `.spl` file, and copy it together with your sound files to Linux. Then use **📥 Import from Soundpad (.spl)…** or drop the `.spl` into the window.
Files are looked up by their original path, by Wine's `Z:\` path, and by file name in the
`.spl`'s folder and its subfolders.

### Command line

| Command | What it does |
|---|---|
| `soundpad` | open the window (starts the background service if needed) |
| `soundpad daemon` | run the background service in the foreground (for autostart) |
| `soundpad play <N\|name>` | play sound number `N`, or by name (case-insensitive, substring match) |
| `soundpad stop` | stop all sounds |
| `soundpad list` | list sounds with their numbers and hotkeys |
| `soundpad voice <effect>` | `off`, `chipmunk`, `deep`, `demon`, `robot`, `alien`, `radio`, `echo`, `cave` |
| `soundpad replay` | save the replay buffer as a sound |
| `soundpad show` | open / focus the window |
| `soundpad quit` | stop the background service |

`Esc` in the window stops all sounds.

## Hotkeys

**In-app hotkeys** (recommended) work everywhere once you're in the `input` group. Keys are not grabbed:
the focused app still receives them, so prefer keys you don't type with (numpad, F13–F24, or a combo with modifiers).

**Alternatively**, bind keys to `soundpad play …` in your desktop's own shortcut settings. Soundpad has to be running.
If your desktop doesn't see `~/.local/bin`, use the full path, e.g. `/home/you/.local/bin/soundpad play 1`.

<details>
<summary><b>Hyprland</b></summary>

`~/.config/hypr/hyprland.conf`:

```ini
bind = , KP_1, exec, soundpad play 1
bind = , KP_2, exec, soundpad play "airhorn"
bind = , KP_5, exec, soundpad voice robot
bind = , KP_0, exec, soundpad stop
exec-once = soundpad daemon      # autostart
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

With Num Lock off, numpad keys are named `KP_End`, `KP_Down`, `KP_Insert`…
Run `wev` (Wayland) or `xev` (X11) to see the exact key names.
</details>

<details>
<summary><b>GNOME</b></summary>

*Settings → Keyboard → View and Customize Shortcuts → Custom Shortcuts → +*,
command `soundpad play 1`. Or from a terminal:

```sh
p=/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/soundpad1/
gsettings set org.gnome.settings-daemon.plugins.media-keys custom-keybindings "['$p']"   # overwrites existing custom shortcuts!
gsettings set org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:$p name 'Soundpad 1'
gsettings set org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:$p command 'soundpad play 1'
gsettings set org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:$p binding 'KP_1'
```

GNOME needs the [AppIndicator extension](https://extensions.gnome.org/extension/615/appindicator-support/) to show the tray icon.
</details>

<details>
<summary><b>KDE Plasma</b></summary>

*System Settings → Keyboard → Shortcuts → Add New → Command or Script…*,
command `soundpad play 1`, then assign a key.
</details>

<details>
<summary><b>Xfce / Cinnamon / MATE</b></summary>

- **Xfce:** *Settings → Keyboard → Application Shortcuts → Add*.
- **Cinnamon:** *System Settings → Keyboard → Shortcuts → Add custom shortcut*.
- **MATE:** *Control Center → Keyboard Shortcuts → Add*.

Use the command `soundpad play 1` (and so on).
</details>

### Autostart

Hyprland / Sway / i3: see `exec-once` / `exec` above. On GNOME, KDE, Xfce and others:

```sh
mkdir -p ~/.config/autostart
sed 's/^Exec=soundpad$/Exec=soundpad daemon/' ~/.local/share/applications/soundpad.desktop > ~/.config/autostart/soundpad.desktop
```

## Configuration

Everything set in the window is saved to `~/.config/soundpad-linux/config.json` (edit it only while Soundpad is stopped).
Replay recordings are saved to `~/.local/share/soundpad-linux/recordings/`, and the background service's log
goes to `~/.local/state/soundpad-linux/soundpad-linux.log`.

## Troubleshooting

- **"Soundpad Microphone" isn't listed in an app:** start Soundpad first, then restart the app.
  Check that the device exists with `pactl list short sources | grep soundpad`.
- **Hotkeys don't react:** check *Settings → Hotkeys*. "No access to keyboards" means you're not in the `input` group yet (log out and back in after `usermod`).
- **Leftover devices after a crash:** just start `soundpad` again, it removes stale devices on startup.
  To remove them by hand: `pactl list short modules | grep soundpad`, then `pactl unload-module <id>`.
- **Others hear the sound but it's cut or muted:** disable noise suppression / echo cancellation / automatic gain in the app.
- **My voice is delayed with effects on:** effects, noise suppression and mic volume add about 50 ms. With all of them off,
  the mic goes through a direct PipeWire loopback.
- **Distorted sound:** lower the per-sound volume or *Volume into mic*.
- **A file won't play:** install `ffmpeg`. The status bar names the missing codec.
- **Long files:** decoded audio stays in memory (about 11 MB per minute), so this tool is meant for clips rather than albums.

## How it works

`soundpad daemon` is the background service. At startup it loads PulseAudio modules through `pactl`
(pipewire-pulse handles them on PipeWire systems): `module-null-sink` (`soundpad_sink`, where everything is mixed)
and `module-remap-source` (`soundpad_mic`, "Soundpad Microphone", built from that sink's monitor).
The real microphone gets in in one of two ways:
- with no effects, through `module-loopback` (handled by PipeWire itself, zero overhead);
- with effects, captured with `parec`, processed in 10 ms frames ([nnnoiseless](https://github.com/jneem/nnnoiseless)
  RNNoise plus a DSP chain: delay-line pitch shifter, ring modulator, waveshaper, biquad band-pass, echo, Freeverb-style reverb)
  and mixed in through a small jitter buffer.

Sounds are decoded with [Symphonia](https://github.com/pdeljanov/Symphonia) (with `ffmpeg` as a fallback),
resampled to 48 kHz stereo, mixed in Rust and streamed as raw PCM into long-running `pacat` processes (one for the
virtual mic, one for monitoring). The pipe buffer is shrunk to one page so stopping a sound is near-instant.

Hotkeys come from `/dev/input` via [evdev](https://github.com/emberian/evdev), and the tray icon is a StatusNotifierItem via
[ksni](https://github.com/iovxw/ksni). The window ([egui](https://github.com/emilk/egui)) is a separate
process that talks to the service over a Unix socket at `$XDG_RUNTIME_DIR/soundpad-linux.sock`. The CLI commands use the same socket.

## License

[MIT](LICENSE)
