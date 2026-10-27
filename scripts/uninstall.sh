#!/usr/bin/env bash
# Remove everything install.sh / install-service.sh put on the system.
# Usage: ./scripts/uninstall.sh      (asks for sudo; keeps your config)
set -euo pipefail

PREFIX="${PREFIX:-/usr/local}"

sudo systemctl disable --now rgb-ctl.service 2>/dev/null || true
sudo rm -f \
  /etc/systemd/system/rgb-ctl.service \
  /usr/lib/systemd/system-sleep/rgb-ctl \
  /etc/udev/rules.d/99-rgb-controller.rules \
  /etc/modules-load.d/rgb-controller.conf \
  "$PREFIX/bin/rgb-controller" \
  "$PREFIX/bin/rgb-ctl" \
  "$PREFIX/share/applications/rgb-controller.desktop" \
  "$PREFIX"/share/icons/hicolor/{32x32,128x128,256x256}/apps/rgb-controller.png
sudo systemctl daemon-reload
sudo udevadm control --reload-rules

echo "Uninstalled. Config kept at ~/.config/rgb-controller/ (delete it manually if wanted)."
echo "Group memberships (i2c, plugdev) were left as-is."
