#!/bin/sh
# Installs Soundpad Linux for the current user (no root needed).
# From a release archive it installs the bundled binary; from a source checkout it builds first.
# Usage: ./install.sh            install into ~/.local
#        ./install.sh --uninstall
set -eu

BIN="$HOME/.local/bin/soundpad"
DESKTOP="$HOME/.local/share/applications/soundpad.desktop"
cd "$(dirname "$0")"

if [ "${1:-}" = "--uninstall" ]; then
    "$BIN" quit >/dev/null 2>&1 || true
    rm -f "$BIN" "$DESKTOP" "$HOME/.config/autostart/soundpad.desktop"
    echo "Removed Soundpad (your sounds and settings in ~/.config/soundpad-linux are kept)."
    exit 0
fi

missing=""
for tool in pactl pacat parec; do
    command -v "$tool" >/dev/null 2>&1 || missing="$missing $tool"
done
if [ -f ./soundpad ]; then
    src=./soundpad
else
    for tool in cargo cc; do
        command -v "$tool" >/dev/null 2>&1 || missing="$missing $tool"
    done
    src=target/release/soundpad
fi
if [ -n "$missing" ]; then
    echo "Missing:$missing — see the Installation section of README.md for your distro." >&2
    exit 1
fi
command -v ffmpeg >/dev/null 2>&1 || echo "Note: ffmpeg not found — opus/wma/webm/avi files won't play (everything else will)."

[ "$src" = ./soundpad ] || cargo build --release
"$BIN" quit >/dev/null 2>&1 || true   # restart a running old version
install -Dm755 "$src" "$BIN"
install -Dm644 soundpad.desktop "$DESKTOP"
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$HOME/.local/share/applications" 2>/dev/null || true

echo "Installed: $BIN"
case ":$PATH:" in
    *":$HOME/.local/bin:"*) ;;
    *) echo "Warning: ~/.local/bin is not in your PATH. Add it, e.g.: echo 'export PATH=\"\$HOME/.local/bin:\$PATH\"' >> ~/.profile" ;;
esac
if ! id -nG | tr ' ' '\n' | grep -qx input; then
    echo "For in-app global hotkeys add yourself to the 'input' group, then log out and back in:"
    echo "    sudo usermod -aG input \$USER"
fi
echo "Run: soundpad"
