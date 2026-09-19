#!/usr/bin/env bash
# Build mchose and its window, put both on PATH, install the udev rule.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

case "${1:-}" in
  "") cargo build --release --features gui --manifest-path "$here/Cargo.toml" ;;
  --desktop-only) test -x "$here/target/release/mchose-gui" ;;
  *) echo "Usage: $0 [--desktop-only]" >&2; exit 2 ;;
esac
mkdir -p "$HOME/.local/bin"
ln -sf "$here/target/release/mchose" "$HOME/.local/bin/mchose"
ln -sf "$here/target/release/mchose-gui" "$HOME/.local/bin/mchose-gui"
echo "installed mchose and mchose-gui in $HOME/.local/bin"

# Desktop sessions need not inherit the shell's ~/.local/bin PATH.
data_home="${XDG_DATA_HOME:-$HOME/.local/share}"
mkdir -p "$data_home/applications" "$data_home/icons/hicolor/scalable/apps" "$data_home/icons/hicolor/256x256/apps"
install -m 644 "$here/assets/brand/icon.svg" "$data_home/icons/hicolor/scalable/apps/mchose.svg"
install -m 644 "$here/assets/brand/icon.png" "$data_home/icons/hicolor/256x256/apps/mchose.png"
python3 - "$here/mchose.desktop" "$data_home/applications/mchose.desktop" "$HOME/.local/bin/mchose-gui" <<'PYTHON'
import pathlib, sys
source, target, executable = sys.argv[1:]
# Exec is desktop-entry syntax, not shell syntax. Escape both parsing layers.
quoted = executable.replace('\\', '\\\\\\\\').replace('"', '\\\\"').replace('`', '\\\\`').replace('$', '\\\\$').replace('%', '%%')
text = pathlib.Path(source).read_text()
text = text.replace('Exec=mchose-gui', 'Exec="' + quoted + '"')
pathlib.Path(target).write_text(text)
PYTHON
update-desktop-database "$data_home/applications" 2>/dev/null || true
gtk-update-icon-cache -f -t "$data_home/icons/hicolor" 2>/dev/null || true

if [[ "${1:-}" == "--desktop-only" ]]; then
  echo "updated MCHOSE Mouse launcher and icons"
  exit 0
fi

if ! cmp -s "$here/70-mchose.rules" /etc/udev/rules.d/70-mchose.rules; then
  sudo cp "$here/70-mchose.rules" /etc/udev/rules.d/70-mchose.rules
  sudo udevadm control --reload
  sudo udevadm trigger
  echo "installed /etc/udev/rules.d/70-mchose.rules"
fi

"$HOME/.local/bin/mchose" devices
