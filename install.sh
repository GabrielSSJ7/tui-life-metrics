#!/usr/bin/env bash
set -euo pipefail

# tui-life-metrics installer.
# Builds the release binary, installs it to ~/.local/bin, ensures the data
# directory exists, and prints the two Omarchy/Hyprland keybinds to paste in.
#
# Overridable env vars:
#   BIN_DIR               install target (default: ~/.local/bin)
#   TUI_LIFE_METRICS_DIR  data root      (default: ~/.local/tui-life-metrics)
#   TERM_CMD              terminal used by the keybinds (default: kitty)

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN_DIR="${BIN_DIR:-$HOME/.local/bin}"
DATA_DIR="${TUI_LIFE_METRICS_DIR:-$HOME/.local/tui-life-metrics}"
TERM_CMD="${TERM_CMD:-kitty}"
BIN_NAME="tui-life-metrics"
BIN_PATH="$BIN_DIR/$BIN_NAME"

echo "==> building release binary"
cargo build --release --manifest-path "$REPO_DIR/Cargo.toml"

echo "==> installing to $BIN_PATH"
mkdir -p "$BIN_DIR"
install -m 0755 "$REPO_DIR/target/release/$BIN_NAME" "$BIN_PATH"

echo "==> ensuring data dir $DATA_DIR"
mkdir -p "$DATA_DIR"

case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *) echo "WARN: $BIN_DIR is not in \$PATH — add it to your shell profile" ;;
esac

cat <<EOF

Done. Binary installed: $BIN_PATH

Omarchy/Hyprland keybinds — add to ~/.config/hypr/hyprland.conf:

  # Capture an action
  bind = SUPER ALT, L, exec, $TERM_CMD --class tui-life-metrics -e $BIN_PATH add

  # Open the dashboard
  bind = SUPER CTRL ALT, L, exec, $TERM_CMD --class tui-life-metrics-dash -e $BIN_PATH dashboard

Then reload Hyprland:

  hyprctl reload

SUPER+ALT+L captures, SUPER+CTRL+ALT+L opens the dashboard.
The capture window needs the \`claude\` CLI on PATH to parse sentences.
EOF
