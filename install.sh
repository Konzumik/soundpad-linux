#!/bin/sh
# Builds Soundpad Linux and installs it for the current user (no root needed).
# Usage: ./install.sh            build + install into ~/.local
#        ./install.sh --uninstall
set -eu

BIN="$HOME/.local/bin/soundpad"
DESKTOP="$HOME/.local/share/applications/soundpad.desktop"

if [ "${1:-}" = "--uninstall" ]; then
    rm -f "$BIN" "$DESKTOP"
    echo "Removed $BIN and $DESKTOP (config in ~/.config/soundpad-linux is kept)."
    exit 0
fi

missing=""
for tool in cargo cc pactl pacat; do
    command -v "$tool" >/dev/null 2>&1 || missing="$missing $tool"
done
if [ -n "$missing" ]; then
    echo "Missing:$missing — see the Installation section of README.md for your distro." >&2
    exit 1
fi
command -v ffmpeg >/dev/null 2>&1 || echo "Note: ffmpeg not found — opus/wma/webm/avi files won't play (everything else will)."

cd "$(dirname "$0")"
cargo build --release
install -Dm755 target/release/soundpad "$BIN"
install -Dm644 soundpad.desktop "$DESKTOP"
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$HOME/.local/share/applications" 2>/dev/null || true

echo "Installed: $BIN"
case ":$PATH:" in
    *":$HOME/.local/bin:"*) ;;
    *) echo "Warning: ~/.local/bin is not in your PATH. Add it, e.g.: echo 'export PATH=\"\$HOME/.local/bin:\$PATH\"' >> ~/.profile" ;;
esac
echo "Run: soundpad"
