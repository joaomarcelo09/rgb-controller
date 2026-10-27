#!/usr/bin/env bash
# Install the RGB controller startup service and udev rules.
# Usage: sudo ./install-service.sh
set -euo pipefail

if [[ $EUID -ne 0 ]]; then
  echo "Please run as root (sudo)."
  exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(dirname "$SCRIPT_DIR")"
BIN_PATH="${RGB_CTL_BIN:-/usr/local/bin/rgb-ctl}"
CURRENT_USER="${SUDO_USER:-$USER}"

if [[ -z "$CURRENT_USER" || "$CURRENT_USER" == "root" ]]; then
  echo "error: run with sudo from a normal user session (SUDO_USER is empty)."
  exit 1
fi

USER_HOME="$(getent passwd "$CURRENT_USER" | cut -d: -f6)"
CONFIG_PATH="$USER_HOME/.config/rgb-controller/config.json"

# ---- build + install the CLI binary -------------------------------------
# Build as the invoking user, never as root: cargo runs dependency build
# scripts, and a root build would also leave root-owned files in target/.
if [[ ! -x "$BIN_PATH" ]] || [[ "${FORCE_REBUILD:-0}" == "1" ]]; then
  echo ">> Building rgb-ctl (release) as $CURRENT_USER…"
  runuser -u "$CURRENT_USER" -- bash -lc "cd \"$ROOT_DIR\" && cargo build --release -p rgb-cli"
  install -Dm755 "$ROOT_DIR/target/release/rgb-ctl" "$BIN_PATH"
fi

# ---- udev rule: user access to RGB HID devices ---------------------------
UDEV_FILE="/etc/udev/rules.d/99-rgb-controller.rules"
echo ">> Installing udev rules ($UDEV_FILE)"
install -Dm644 "$SCRIPT_DIR/99-rgb-controller.rules" "$UDEV_FILE"

# ---- systemd oneshot service: apply at boot ------------------------------
# Runs as the desktop user, not root: the udev rule (plugdev) and the i2c
# group already grant device access, and the config file is user-writable.
UNIT_FILE="/etc/systemd/system/rgb-ctl.service"
echo ">> Installing systemd service ($UNIT_FILE)"
cat > "$UNIT_FILE" <<EOF
[Unit]
Description=RGB Controller - apply saved colors
After=systemd-udev-settle.service systemd-modules-load.service

[Service]
Type=oneshot
User=$CURRENT_USER
Environment=RGB_CONTROLLER_CONFIG=$CONFIG_PATH
ExecStart=$BIN_PATH apply
TimeoutStartSec=30
NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=read-only
PrivateTmp=yes

[Install]
WantedBy=multi-user.target
EOF

# ---- systemd-sleep hook: re-apply after resume ---------------------------
# systemd-sleep hooks do not inherit the service Environment, so the user's
# config path is baked in here.
SLEEP_HOOK="/usr/lib/systemd/system-sleep/rgb-ctl"
echo ">> Installing resume hook ($SLEEP_HOOK)"
cat > "$SLEEP_HOOK" <<EOF
#!/bin/sh
# Re-apply saved RGB colors after resume; the MSI board and RAM lose their
# lighting state on suspend.
case "\$1/\$2" in
  pre/*) exit 0 ;;
  post/suspend|post/hibernate|post/hybrid-sleep)
    if [ -x $BIN_PATH ]; then
      runuser -u $CURRENT_USER -- env RGB_CONTROLLER_CONFIG=$CONFIG_PATH $BIN_PATH apply
    fi
    ;;
esac
exit 0
EOF
chmod 755 "$SLEEP_HOOK"

# ---- i2c-dev module at boot (SMBus access for RAM) -----------------------
MODULES_FILE="/etc/modules-load.d/rgb-controller.conf"
echo ">> Ensuring i2c-dev loads at boot ($MODULES_FILE)"
echo "i2c-dev" > "$MODULES_FILE"

# ---- groups: i2c (SMBus/RAM) and plugdev (hidraw via udev rule) ----------
# Arch and some other distros don't ship plugdev; create it so the udev rule's
# GROUP= resolves and usermod doesn't abort the script.
for grp in i2c plugdev; do
  if ! getent group "$grp" >/dev/null; then
    echo ">> Creating missing $grp group"
    groupadd -r "$grp"
  fi
done
for grp in i2c plugdev; do
  if ! id -nG "$CURRENT_USER" | grep -qw "$grp"; then
    echo ">> Adding $CURRENT_USER to the $grp group"
    usermod -aG "$grp" "$CURRENT_USER"
  fi
done

udevadm control --reload-rules
udevadm trigger
systemctl daemon-reload
systemctl enable rgb-ctl.service

echo
echo "Done. Notes:"
echo " - Log out/in (or reboot) so the new group memberships take effect."
echo " - Config read from: $CONFIG_PATH"
echo " - Test with: $BIN_PATH list"
