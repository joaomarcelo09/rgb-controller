# PRD — RGB Controller (Tauri + React + Rust)

**Version:** 1.0
**Status:** Draft for review
**Date:** 2026-10-26

> **Historical document.** This is the original v1 spec. Since then: fans were
> removed from the GUI (they run from an external hub), XPG/ENE RAM support
> was added, and the HID++ details in FR-2.1 and the appendix were corrected
> (big-endian feature IDs, non-zero software ID, per-zone writes). See
> [`README.md`](README.md) and [`AGENTS.md`](AGENTS.md) for current behavior.

## 1. Problem statement

The user has three categories of RGB hardware (keyboard, case fans, RAM) on a
Linux desktop. No single application currently controls all three reliably:
OpenRGB exists but fails to change the fan colors on the MSI motherboard
because of a known protocol bug (see appendix). This project delivers a
Tauri desktop app (React frontend, Rust backend) that sets colors on all three
device categories from one UI, persists the chosen colors, and re-applies them
automatically at system startup.

## 2. Goals / Non-goals

### Goals
- Detect and control: Logitech keyboard, MSI Mystic Light motherboard RGB
  headers (fans), RGB RAM via SMBus.
- Set a single color (or per-zone colors for fans) across all devices from one UI.
- Persist the configuration and re-apply it on boot (systemd service) and after resume.
- Unit-testable protocol layer that never requires physical hardware to verify
  packet correctness.

### Non-goals (v1)
- Per-LED / animated effects engines (static colors and basic off mode only).
- Motherboards other than MSI Mystic Light 185-byte USB HID controllers.
- Keyboards other than Logitech HID++ 2.0 RGB (0x8070 feature) devices.
- RAM vendors other than Corsair Vengeance-style SMBus controllers (v1; the
  detector is designed to be extended).
- Fan speed control, DPI control, macro control.

## 3. Hardware scope (verified against user's system)

| Device | Bus | IDs | Protocol |
|---|---|---|---|
| MSI B550M PRO-VDH WIFI (MS-7C95) — fan/ARGB headers | USB HID `/dev/hidraw*` | VID `0x1462` PID `0x7C95` | Mystic Light 185-byte feature report `0x52` |
| Logitech PRO Gaming Keyboard | USB HID `/dev/hidraw*` | VID `0x046D` PID `0xC339` | HID++ 2.0, feature `0x8070` (LED effects) |
| RGB DDR4 RAM modules | SMBus `/dev/i2c-*` (PIIX4 on AMD) | probe `0x58`–`0x5F` | Corsair DRAM register protocol |

## 4. Functional requirements

### FR-1 Device detection
- FR-1.1 Enumerate HID devices by parsing `/sys/class/hidraw/*/device/uevent`
  (VID/PID/name/serial); match MSI Mystic Light PIDs and Logitech keyboard PIDs.
- FR-1.2 Enumerate SMBus adapters from `/sys/class/i2c-adapter` (piix4/i801),
  probe Corsair DRAM signature (reg `0x43` ∈ {0x1A,0x1B,0x1C} and
  reg `0x44` ∈ {0x01,0x03,0x04}) at `0x58`–`0x5F` (AMD) / `0x18`–`0x1F` (Intel).
- FR-1.3 The UI shows detected devices grouped by category with per-device status.

### FR-2 Color control
- FR-2.1 Keyboard: set static color via HID++ long report to feature `0x8070`
  (zone 0, mode `0x01`, RGB payload), mode `0x00` for off.
- FR-2.2 Fans/motherboard: build 185-byte feature report (report id `0x52`),
  set zone effect=static, color1/color2, brightness/speed flags **with the
  enable bit 7 set (`0x80`)** — this is the fix OpenRGB lacks — and write twice
  (`save=0` then `save=1`, ≥50 ms apart) to the zone offsets JRGB1/JRGB2/
  JRAINBOW1/JRAINBOW2/ONBOARD.
- FR-2.3 RAM: build Corsair direct-color packet (`LED count` + RGB data + CRC-8),
  write via SMBus block writes to regs `0x31`/`0x32`.
- FR-2.4 Global "apply one color to everything" action.
- FR-2.5 All writes go through read-modify-write where the device supports
  reading state (MSI) to avoid clobbering untouched zones.

### FR-3 Persistence
- FR-3.1 Config stored as JSON at `~/.config/rgb-controller/config.json`
  (override via `RGB_CONTROLLER_CONFIG` env var).
- FR-3.2 Config contains keyboard setting, per-zone fan settings, RAM setting,
  and a global quick-color.

### FR-4 Startup auto-apply
- FR-4.1 A headless CLI binary (`rgb-ctl apply`) applies the saved config and exits.
- FR-4.2 A systemd oneshot service runs it after boot (`WantedBy=multi-user.target`)
  and a systemd-sleep hook re-applies after resume.
- FR-4.3 A udev rule grants the desktop user access to the specific HID devices
  (046d:*, 1462:*) without root.

### FR-5 GUI (Tauri + React)
- FR-5.1 Sections for Keyboard / Fans (per zone) / RAM with color pickers.
- FR-5.2 Buttons: "Apply", "Save", "Set all to color".
- FR-5.3 Device list with detection status; errors surfaced to the UI.

## 5. Non-functional requirements

- NFR-1 **Safety**: never probe SPD EEPROM addresses (0x50–0x53); restrict SMBus
  probing to the documented RGB controller ranges; log every write.
- NFR-2 Protocol layer is a pure library (`rgb-core`) with zero GUI dependencies;
  100% of packet building covered by unit tests.
- NFR-3 Works without root for HID devices when udev rule installed; SMBus may
  require `i2c-dev` group membership (documented).
- NFR-4 All hardware writes are fallible and return typed errors surfaced in
  UI/CLI exit codes.

## 6. Acceptance criteria

1. `cargo test` passes in `rgb-core` and `rgb-cli` without any hardware.
2. The MSI packet builder produces bytes matching the reference capture
   (blue static, brightness 10, enable bit set → flags `0xA8`).
3. Logitech long-report builder matches the documented 20-byte layout.
4. Corsair DRAM packet includes a valid CRC-8; detector rejects non-Corsair
   registers.
5. `rgb-ctl apply` reads config and applies to all three device categories
   (verified on hardware by the user; graceful skip if a device is absent).
6. systemd service + sleep hook installed via provided script.
7. GUI builds (`npm run build` + `cargo check -p rgb-controller`).

## 7. Appendix — protocol summary (reverse-engineering reference)

- OpenRGB `Controllers/MSIMysticLightController/*` (GPL-2) — zone layout, mode IDs.
- Community fix (stephenlzc/msi-mystic-light-web, MIT): enable bit 7 in
  speed/brightness flags; double-send preview+commit.
- OpenRGB `Controllers/LogitechController/LogitechProtocolCommon.{h,cpp}` —
  HID++ long FAP 20-byte layout, feature 0x8070 command 0x30 (set effect).
- OpenRGB `Controllers/CorsairDRAMController/*` — register map (0x0B,0x20,0x21,
  0x23,0x30,0x31,0x32,0x41,0x82), direct packet with CRC-8, AMD probe range.

## 8. Risks

- MSI LED firmware bricking reports exist for 7C95 (usually recoverable via MSI
  LED firmware update / CMOS clear). Mitigation: preview+commit double-write,
  no per-LED streaming in v1.
- SMBus is a shared bus; wrong writes can disturb other devices. Mitigation:
  signature-check before write (NFR-1).
- Unknown RAM LED count for Corsair modules → default 10, configurable per
  device in config.json.
