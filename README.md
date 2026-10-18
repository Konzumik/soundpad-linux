# Soundpad Linux

A fast, lightweight soundboard for Linux, written in Rust.
It creates a virtual microphone, **"Soundpad Microphone"**, that carries your real
microphone plus any sounds you play, so you can use it in Discord, games, OBS,
Telegram, Zoom or anything else that records from a mic.

The Windows Soundpad from Steam doesn't run on Linux, so this was written as a native replacement.

[Русская версия / Russian version](README.ru.md)

```
real microphone ──loopback──┐
                            ├──> soundpad_sink ──> "Soundpad Microphone" ──> Discord / game / OBS
sounds (built-in mixer) ────┘
        └──> your headphones (monitoring, separate volume)
```

## Features

- Virtual microphone over PipeWire (or PulseAudio), with your real mic mixed in.
- Plays **mp3, wav, ogg, flac, m4a, aac** natively, and also the audio track of **video files**
  (mp4, mov, mkv, m4v). With `ffmpeg` installed it also plays opus, wma, webm and avi.
- You can hear sounds in your own headphones (monitoring) with a separate volume.
- Volume per sound and for the whole mic, plus an option to let several sounds play at once.
- Global hotkeys through your desktop's own shortcut settings (works on Wayland), e.g. `soundpad play 3`.
- Low latency: about 20 ms from command to sound. Sounds are preloaded into memory.
- Tiny footprint: ~0% CPU when idle, a few MB of RAM plus the decoded sounds.
- English and Russian interface (auto-detected from your locale; switchable in the window).
- No `-devel` packages needed to build. At runtime it only needs `pactl` and `pacat`.

## Installation

You need:

- **Rust** 1.81+ (`cargo`) and a C linker (`gcc`) to build;
- **`pactl` + `pacat`** at runtime (PulseAudio client tools — they talk to PipeWire through `pipewire-pulse`);
- optionally **`ffmpeg`** for opus / wma / webm / avi.

Pick your distro, install the dependencies, then [build and install](#build-and-install).

### Fedora

```sh
sudo dnf install cargo gcc pulseaudio-utils ffmpeg-free
```

### Arch Linux / Manjaro / EndeavourOS / CachyOS

```sh
sudo pacman -S --needed rust base-devel libpulse pipewire-pulse ffmpeg
```

(`pactl` and `pacat` are part of `libpulse` on Arch.)

### Debian / Ubuntu / Linux Mint / Pop!_OS

The `cargo` in Debian/Ubuntu repositories is often too old, so install Rust with [rustup](https://rustup.rs):

```sh
sudo apt install build-essential curl pulseaudio-utils pipewire-pulse ffmpeg
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
```

On Ubuntu 22.04 and older, PulseAudio may still be your sound server. That's fine: leave out `pipewire-pulse`.

### openSUSE (Tumbleweed / Leap)

```sh
sudo zypper install rust cargo gcc pulseaudio-utils pipewire-pulseaudio ffmpeg
```

### NixOS / Nix

```sh
nix-shell -p cargo rustc gcc pulseaudio ffmpeg
# the window needs these libraries at runtime (they are loaded dynamically):
export LD_LIBRARY_PATH=$(nix-build '<nixpkgs>' -A libxkbcommon --no-out-link)/lib:$(nix-build '<nixpkgs>' -A wayland --no-out-link)/lib:$(nix-build '<nixpkgs>' -A libGL --no-out-link)/lib
```

### Other distributions

Install a Rust toolchain (your package manager or [rustup](https://rustup.rs)), a C compiler,
and the package that provides `pactl` and `pacat` (usually `pulseaudio-utils` or `libpulse`).

### Build and install

```sh
git clone https://github.com/Konzumik/soundpad-linux.git
cd soundpad-linux
./install.sh
```

This builds a release binary and installs it into `~/.local/bin/soundpad`, plus an
application-menu entry. Make sure `~/.local/bin` is in your `PATH`.

Uninstall: `./install.sh --uninstall` (your sound list in `~/.config/soundpad-linux` is kept).

To do it by hand instead:

```sh
cargo build --release
install -Dm755 target/release/soundpad ~/.local/bin/soundpad
install -Dm644 soundpad.desktop ~/.local/share/applications/soundpad.desktop
```

## Usage

1. Run `soundpad` (or open "Soundpad Linux" from your app menu).
2. Add sounds: use **Add files…**, type a path to a file or folder, or drag & drop files into the window.
3. Pick your real microphone in the **Microphone** drop-down (or "off" for sounds only).
4. In Discord / your game / OBS, select **Soundpad Microphone** as the input device.

> **Discord:** turn off noise suppression (Krisp) in *Voice & Video*, otherwise it filters the sounds out.
> If the device does not show up, restart Discord after starting Soundpad.

### Command line

| Command | What it does |
|---|---|
| `soundpad` | start with the window |
| `soundpad daemon` | start without a window (control via commands / hotkeys only) |
| `soundpad play <N\|name>` | play sound number `N`, or by name (case-insensitive, substring match) |
| `soundpad stop` | stop all sounds |
| `soundpad list` | list sounds with their numbers |

`Esc` in the window stops all sounds.

## Hotkeys

On Wayland applications can't grab global hotkeys, so you bind keys to `soundpad play …`
in your desktop's own shortcut settings. Soundpad has to be running (window or `daemon`).

If your desktop doesn't see `~/.local/bin`, use the full path, e.g. `/home/you/.local/bin/soundpad play 1`.

### Hyprland

`~/.config/hypr/hyprland.conf`:

```ini
bind = , KP_1, exec, soundpad play 1
bind = , KP_2, exec, soundpad play "airhorn"
bind = , KP_0, exec, soundpad stop
exec-once = soundpad          # autostart (or: soundpad daemon)
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

With Num Lock off, numpad keys are named `KP_End`, `KP_Down`, `KP_Insert`…
Run `wev` (Wayland) or `xev` (X11) to see the exact key names.

### GNOME

*Settings → Keyboard → View and Customize Shortcuts → Custom Shortcuts → +*,
command `soundpad play 1`. Or from a terminal:

```sh
p=/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/soundpad1/
gsettings set org.gnome.settings-daemon.plugins.media-keys custom-keybindings "['$p']"   # overwrites existing custom shortcuts!
gsettings set org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:$p name 'Soundpad 1'
gsettings set org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:$p command 'soundpad play 1'
gsettings set org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:$p binding 'KP_1'
```

### KDE Plasma

*System Settings → Keyboard → Shortcuts → Add New → Command or Script…*,
command `soundpad play 1`, then assign a key.

### Xfce / Cinnamon / MATE

- **Xfce:** *Settings → Keyboard → Application Shortcuts → Add*.
- **Cinnamon:** *System Settings → Keyboard → Shortcuts → Add custom shortcut*.
- **MATE:** *Control Center → Keyboard Shortcuts → Add*.

Use the command `soundpad play 1` (and so on).

### Autostart (GNOME, KDE, Xfce, …)

```sh
mkdir -p ~/.config/autostart
cp ~/.local/share/applications/soundpad.desktop ~/.config/autostart/
```

## Configuration

Settings and the sound list live in `~/.config/soundpad-linux/config.json`:

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

`language` can be `"en"`, `"ru"` or `null` (follow the system locale).
Only one instance can run at a time, so stop Soundpad before editing the file by hand.

## Troubleshooting

- **"Soundpad Microphone" isn't listed in an app:** start Soundpad first, then restart the app.
  Check that the device exists with `pactl list short sources | grep soundpad`.
- **Leftover devices after a crash:** just start `soundpad` again, it removes stale devices on startup.
  To remove them by hand: `pactl list short modules | grep soundpad`, then `pactl unload-module <id>`.
- **Others hear the sound but it's cut or muted:** disable noise suppression / echo cancellation / automatic gain in the app.
- **Distorted sound:** lower the per-sound volume or *Volume into mic*. Values above 100% can clip.
- **A file won't play:** install `ffmpeg`; the error message in the status bar names the missing codec.
- **Long files:** decoded audio stays in memory (about 11 MB per minute), so this tool is meant for clips rather than albums.

## How it works

At startup Soundpad loads three PulseAudio modules through `pactl` (pipewire-pulse handles them on PipeWire systems):

1. `module-null-sink`: `soundpad_sink`, where everything is mixed;
2. `module-remap-source`: `soundpad_mic` ("Soundpad Microphone"), built from that sink's monitor;
3. `module-loopback`: your real microphone → `soundpad_sink`.

Sounds are decoded with [Symphonia](https://github.com/pdeljanov/Symphonia) (with `ffmpeg` as a fallback),
resampled to 48 kHz stereo, mixed in Rust and streamed as raw PCM into two long-running
`pacat` processes: one feeds the virtual sink, the other your headphones. The pipe buffer is
shrunk to one page so that stopping a sound is near-instant. The modules are unloaded on exit.

The GUI uses [egui](https://github.com/emilk/egui). The `play` / `stop` / `list` commands go to the
running instance over a Unix socket at `$XDG_RUNTIME_DIR/soundpad-linux.sock`.

## License

[MIT](LICENSE)
