# Soundpad Linux — notes for Claude

Native Linux soundboard in Rust: a virtual microphone ("Soundpad Microphone") that carries the real mic
plus sounds, an egui GUI, and a CLI (`soundpad play N`) so hotkeys can be bound in the compositor/DE.
User-facing docs: `README.md` (English) and `README.ru.md` (Russian); keep both in sync.

## Build / run

```sh
cargo build --release              # no -devel packages needed (winit/glutin dlopen system libs)
./target/release/soundpad          # GUI
./target/release/soundpad daemon   # headless
./install.sh                       # installs to ~/.local/bin + .desktop entry
```

Runtime dependencies: `pactl`, `pacat` (PulseAudio client tools, work through pipewire-pulse); `ffmpeg` is optional.

## Architecture (`src/`)

- `main.rs`: CLI dispatch (`gui` default, `daemon`, `play`, `stop`, `list`, `help`); sets up the language, Ctrl+C/SIGTERM cleanup.
- `engine.rs`: `Core`, the shared state for GUI + IPC: `Mutex<Config>`, `Arc<Mixer>`, `VirtualMic`, status line,
  repaint notifier. `shutdown()` is idempotent and unloads the PA modules + removes the socket.
- `vmic.rs`: creates the virtual mic with `pactl load-module`: `module-null-sink` (`soundpad_sink`),
  `module-remap-source` (`soundpad_mic`, from `soundpad_sink.monitor`) and `module-loopback` (real mic → sink).
  `cleanup_stale()` unloads any module whose args mention `soundpad_sink` or `soundpad_mic` (from a crashed run).
  pactl always runs with `LC_ALL=C`; parse the text output (`pactl --format=json` breaks on non-ASCII descriptions).
- `audio.rs`: decoding (Symphonia `all` features → ffmpeg fallback) into interleaved stereo i16 @ 48 kHz,
  plus the mixer. A mixer thread mixes 5 ms blocks and writes f32 into a long-running `pacat --device=soundpad_sink`;
  that blocking write paces the loop in real time. The monitor copy goes through a bounded channel to a second `pacat`
  (default sink) and is dropped when it falls behind, so monitoring can never stall the mic. The pipe to pacat is shrunk
  to 4096 bytes with `F_SETPIPE_SZ` to keep stop latency at about 10 ms. Decoded sounds are cached per path and
  preloaded at startup and when sounds are added.
- `ipc.rs`: Unix socket `$XDG_RUNTIME_DIR/soundpad-linux.sock`, one line in / text out. Replies starting with `ERR` make the CLI exit with 1.
- `gui.rs`: egui/eframe 0.31. Each frame clones the config, edits the clone, writes it back and saves it if it changed;
  side effects (play, mic switch, file dialog) are queued as `Action`s and run after the config lock is released.
  File dialog: `rfd` via the xdg-desktop-portal, on a separate thread.
- `config.rs`: `~/.config/soundpad-linux/config.json` (serde, `#[serde(default)]`, atomic save). `AUDIO_EXTS` includes video containers.
- `i18n.rs`: `tr!("English", "Русский")` macro (`#[macro_use]`, declared first in `main.rs`). Language comes from
  config `language` ("en"/"ru"/null = from `LC_ALL`/`LC_MESSAGES`/`LANG`). **Every user-visible string must go through `tr!`.**

## Testing gotchas

- **Don't start a second instance while the user's Soundpad is running:** at startup `cleanup_stale()` would unload
  the user's live virtual mic, and CLI commands go to the user's instance. Check with `pgrep -a soundpad` first.
- Use a throwaway config: `XDG_CONFIG_HOME=<scratch dir> soundpad daemon`, and set `"monitor_enabled": false` so tests don't beep in the user's headphones.
- Record the virtual mic with `pw-record --target=soundpad_mic …`. `parec` writes nothing to a redirected stdout on this setup.
- Generate test media with ffmpeg lavfi (`sine`, `testsrc`); Fedora's ffmpeg-free has `libopenh264`, not `libx264`.
- To exercise the ffmpeg-less path, run with a `PATH` that only contains `pactl` and `pacat`.
- The GUI may open on another Hyprland workspace, so `grim` screenshots of its geometry can capture something else.
- Expected results: amplitude preserved (e.g. 16000 in → ~16000 out), start ≈ 20 ms after `play`, 0% CPU when idle,
  and no `soundpad` modules left in `pactl list short modules` after SIGINT.

## Conventions

- Code comments in English, terse. Keep dependencies pure-Rust or dlopen-based (no `-sys` crates that need `-devel` packages).
- Wayland: no in-app global hotkeys; the hotkey docs for each DE are in the README.
- Repo: https://github.com/Konzumik/soundpad-linux (MIT).
