# RGB Controller

A small Linux desktop app (Tauri + React + Rust) that sets the RGB colors of a
**Logitech keyboard** and **RGB RAM** from one window, plus a headless CLI
(`rgb-ctl`) that re-applies your colors at boot and after suspend.

It talks to the hardware directly (USB HID and SMBus). There's no vendor
software, no daemon running in the background, and no network access.

> ⚠️ This software writes to hardware controllers. It only touches the
> address ranges and registers documented below and has been verified on the
> hardware listed, but you use it at your own risk (see [License](#license)).

## Features

- One color picker per device, plus a single "Keyboard + RAM" color
- Colors persist in a JSON config and are re-applied at boot and on resume
- `rgb-ctl` CLI for scripting (`rgb-ctl set all ff0000`)
- Runs without root: access goes through udev rules and the `i2c` group
- Protocol layer (`rgb-core`) is a plain Rust library with unit tests that need no hardware

## Supported hardware

| Device | Interface | Status |
|---|---|---|
| Logitech G PRO keyboard (`046D:C339`) | USB HID, HID++ 2.0 feature `0x8070` | ✅ Verified |
| Other Logitech 0x8070 keyboards (G512, G513, G815, G915, G413…) | same | 🟡 Should work, untested |
| XPG Spectrix DDR4 (ENE controller `AUDA0-E6K5-0101`) | SMBus | ✅ Verified |
| Corsair Vengeance-style DDR4 | SMBus | 🟡 Implemented, untested |
| MSI Mystic Light 185-byte boards (e.g. B550M PRO-VDH WIFI) | USB HID | 🔧 In the library and CLI, not in the GUI |

Fans connected to a separate RGB hub/remote can't be controlled from software
unless the hub's ARGB input is wired to a motherboard `JRAINBOW` header and the
hub is set to motherboard sync mode. That's why the GUI leaves fans out.

## Requirements

- Linux with systemd and udev (developed on Arch Linux)
- Rust (stable) and Node.js 18+ with npm
- [Tauri v2 system dependencies](https://v2.tauri.app/start/prerequisites/#linux)

Arch Linux:

```sh
sudo pacman -S --needed base-devel rust nodejs npm webkit2gtk-4.1 \
  openssl librsvg libappindicator-gtk3 i2c-tools
```

Debian / Ubuntu:

```sh
sudo apt install build-essential curl libwebkit2gtk-4.1-dev libssl-dev \
  librsvg2-dev libayatana-appindicator3-dev libudev-dev i2c-tools
# plus Rust from https://rustup.rs and Node.js 18+
```

## Install

Clone this repository, then run the installer from its folder as your
**normal user**. It builds everything as you and only uses `sudo` for the
system-wide steps. `sudo` needs a real terminal to prompt for your password.

```sh
./scripts/install.sh
```

Then **log out and back in** so the new group memberships apply. Open
**RGB Controller** from your application menu, or run `rgb-controller`.

### What the installer changes on your system

| Path | Purpose |
|---|---|
| `/usr/local/bin/rgb-controller`, `/usr/local/bin/rgb-ctl` | App and CLI |
| `/usr/local/share/applications/rgb-controller.desktop` + icons | Menu entry |
| `/etc/udev/rules.d/99-rgb-controller.rules` | Lets `plugdev` members use the Logitech/MSI HID devices |
| `/etc/modules-load.d/rgb-controller.conf` | Loads `i2c-dev` at boot (needed for RAM) |
| `/etc/systemd/system/rgb-ctl.service` | Runs `rgb-ctl apply` at boot, as your user |
| `/usr/lib/systemd/system-sleep/rgb-ctl` | Re-applies colors after resume, as your user |
| Groups `plugdev`, `i2c` | Created if missing; your user is added to both |

Use `PREFIX=/opt/rgb ./scripts/install.sh` to install somewhere other than `/usr/local`.

### Uninstall

```sh
./scripts/uninstall.sh
```

This removes everything listed above except your config
(`~/.config/rgb-controller/`) and your group memberships.

## Usage

### Desktop app

Pick a color for the keyboard or RAM and press **Apply**. **Save config** stores
the colors that the boot service re-applies.

### CLI

```sh
rgb-ctl list                  # detected devices
rgb-ctl set all ff0000        # keyboard + RAM red
rgb-ctl set keyboard 00ff00
rgb-ctl set ram 0000ff
rgb-ctl off                   # keyboard + RAM off
rgb-ctl apply                 # apply the saved config (what the service runs)
rgb-ctl status                # print the saved config
```

`set` and `off` also save the new colors to the config.

### Configuration

The config lives at `~/.config/rgb-controller/config.json`. Set
`RGB_CONTROLLER_CONFIG=/path/to/config.json` to use a different file.

```json
{
  "global_color": null,
  "keyboard": { "mode": 1, "color": { "r": 0, "g": 255, "b": 0 }, "brightness": 100 },
  "fans": [],
  "ram": { "color": { "r": 0, "g": 255, "b": 0 }, "led_count": 10 }
}
```

`fans` drives MSI motherboard headers and is empty by default. Add zones
(`jrgb1`, `jrainbow1`, …) only if your fans are wired to the board.

## How it works

```
crates/rgb-core   Protocol library: Logitech HID++, ENE/Corsair SMBus, MSI HID, config
crates/rgb-cli    rgb-ctl, the headless CLI used by the boot/resume service
src-tauri         Tauri shell: Rust commands wrapping rgb-core
src/              React frontend
scripts/          install / uninstall, systemd + udev setup, icon generator
```

- **Keyboard (HID++ 2.0):** the app looks up the index of feature `0x8070`
  through the root feature, using big-endian feature IDs. It then asks for the
  zone count and sends `setZoneEffect` (static) to every zone, keys and logo.
  Every request waits for the device's reply, so errors show up instead of
  failing silently.
- **XPG RAM (ENE over SMBus):** controllers are found with a read-only
  signature check at `0x70–0x7F`. Colors go to the effect table at `0x8160`
  (R, B, G byte order), followed by static mode and apply (`0x80A0 = 0x01`).
  Changes are never saved to the module's flash, which is why the service
  re-applies them. Several sticks may share one address and change together.
- **Corsair RAM:** signature check at `0x58–0x5F` (AMD) / `0x18–0x1F` (Intel),
  then direct-color packets with a CRC-8.
- **MSI boards:** read-modify-write of the 185-byte feature report `0x52`,
  with the brightness "enable" bit 7 set and a preview + commit double send.

## Security and safety

- **No root at runtime.** The GUI, CLI, boot service and resume hook all run
  as your user. The service is also sandboxed (`NoNewPrivileges`,
  `ProtectSystem=strict`, `ProtectHome=read-only`, `PrivateTmp`).
- **Builds never run as root.** `install.sh` refuses to run under sudo, and
  `install-service.sh` compiles as the invoking user.
- **udev scope:** the rule gives `plugdev` members (and the active session,
  through `uaccess`) raw access to Logitech `C3xx` and MSI Mystic Light HID
  devices. That includes the keyboard's input interface, so only add trusted
  users to `plugdev`.
- **SMBus safety:** probing only touches the documented RGB controller ranges.
  SPD EEPROMs (`0x50–0x57`) and DDR4 page-select addresses (`0x30–0x37`) are
  never touched. Detection is read-only, and the app refuses to write if a
  controller reports an implausible LED count.
- **No network, no telemetry.** The app makes no network requests.

Found a security problem? Please open a private
[security advisory](../../security/advisories/new) instead of a public issue.

## Troubleshooting

| Symptom | Check |
|---|---|
| `RAM modules (0)` | `lsmod \| grep i2c_dev`; are you in the `i2c` group (`id`)? Did you log out and back in? |
| Keyboard listed but `skipped` on one path | Normal: only one of the keyboard's HID interfaces carries the RGB feature |
| `permission denied` on `/dev/hidraw*` | The udev rule isn't active: `sudo udevadm trigger`, or re-plug the device |
| Colors lost after suspend | `systemctl status rgb-ctl.service`, `journalctl -u rgb-ctl -b` |
| `sudo: a terminal is required` | Run `install.sh` in a real terminal, not through a non-interactive runner |

## Development

```sh
npm install
cargo test                       # protocol tests, no hardware needed
npx tauri dev                    # run the app with hot reload
cargo run -p rgb-cli -- list     # run the CLI from source
npx tauri build                  # release build + .deb/.rpm/AppImage bundles
```

The `.deb`, `.rpm` and AppImage bundles in `target/release/bundle/` don't set
up the udev rules, groups or boot service. Run `install.sh` at least once.

### Adding a device

1. Put the protocol in a new module under `crates/rgb-core/src/`, with pure
   packet builders and unit tests that check exact bytes.
2. Add detection that only reads and fails safe; never probe outside the
   device's documented address range.
3. Wire it into `apply.rs` and, if needed, the CLI and the Tauri commands.
4. Verify it on real hardware and update the table above.

See [`AGENTS.md`](AGENTS.md) for coding conventions and hardware safety rules.

## Contributing

Issues and pull requests are welcome. Please include your device's VID:PID
(`rgb-ctl list`) or ENE device name, and run `cargo test` and `npx tsc --noEmit`
before opening a pull request.

## Acknowledgements

Protocol details were cross-checked against [OpenRGB](https://gitlab.com/CalcProgrammer1/OpenRGB)
(Logitech, ENE and Corsair DRAM, MSI Mystic Light) and community reverse
engineering of MSI Mystic Light. Many thanks to the OpenRGB contributors for
documenting these protocols.

## License

[MIT](LICENSE). This software is provided "as is", without warranty of any kind.
