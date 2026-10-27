#!/usr/bin/env bash
# Build and install the RGB Controller desktop app + rgb-ctl boot service.
# Usage: ./scripts/install.sh        (run as your normal user; asks for sudo)
set -euo pipefail

if [[ $EUID -eq 0 ]]; then
  echo "Run as your normal user (not sudo); the script calls sudo when needed."
  exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(dirname "$SCRIPT_DIR")"
PREFIX="${PREFIX:-/usr/local}"

cd "$ROOT_DIR"

# ---- build (as the user, so target/ and node_modules/ stay user-owned) ----
if [[ ! -d node_modules ]]; then
  echo ">> Installing frontend dependencies…"
  npm ci
fi
echo ">> Building desktop app (release)…"
npx tauri build --no-bundle
echo ">> Building rgb-ctl (release)…"
cargo build --release -p rgb-cli

# ---- install binaries, desktop entry, icons -------------------------------
echo ">> Installing to $PREFIX (sudo)…"
sudo install -Dm755 target/release/rgb-controller "$PREFIX/bin/rgb-controller"
sudo install -Dm755 target/release/rgb-ctl "$PREFIX/bin/rgb-ctl"
sudo install -Dm644 src-tauri/icons/32x32.png \
  "$PREFIX/share/icons/hicolor/32x32/apps/rgb-controller.png"
sudo install -Dm644 src-tauri/icons/128x128.png \
  "$PREFIX/share/icons/hicolor/128x128/apps/rgb-controller.png"
sudo install -Dm644 src-tauri/icons/128x128@2x.png \
  "$PREFIX/share/icons/hicolor/256x256/apps/rgb-controller.png"

sudo install -d "$PREFIX/share/applications"
sudo tee "$PREFIX/share/applications/rgb-controller.desktop" >/dev/null <<EOF
[Desktop Entry]
Type=Application
Name=RGB Controller
Comment=Control keyboard, fan and RAM RGB lighting
Exec=$PREFIX/bin/rgb-controller
Icon=rgb-controller
Terminal=false
Categories=Utility;Settings;HardwareSettings;
EOF

sudo gtk-update-icon-cache -q "$PREFIX/share/icons/hicolor" 2>/dev/null || true
sudo update-desktop-database -q "$PREFIX/share/applications" 2>/dev/null || true

# ---- udev rules, groups, boot + resume service ----------------------------
sudo RGB_CTL_BIN="$PREFIX/bin/rgb-ctl" "$SCRIPT_DIR/install-service.sh"

echo
echo "Installed. Launch \"RGB Controller\" from your app menu or run: rgb-controller"
echo "Log out and back in first so the i2c/plugdev groups apply (needed for RAM)."
