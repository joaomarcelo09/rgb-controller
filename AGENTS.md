# AGENTS.md

Guidance for AI coding agents (and humans) working on this repository.
User-facing docs are in [`README.md`](README.md).

## Project overview

A Linux RGB controller for a Logitech keyboard and RGB RAM, written in Rust
(protocol library + CLI), with a Tauri v2 + React + TypeScript GUI.

```
crates/rgb-core/src/
  logitech.rs   HID++ 2.0 feature 0x8070 (keyboard)
  smbus.rs      i2c-dev access; ENE (XPG) and Corsair DRAM controllers
  msi.rs        MSI Mystic Light 185-byte HID report (library/CLI only)
  detection.rs  hidraw / i2c adapter enumeration via sysfs
  apply.rs      Config -> devices orchestration, validation
  config.rs     JSON config (~/.config/rgb-controller/config.json)
crates/rgb-cli/  rgb-ctl binary (boot/resume service entry point)
src-tauri/       Tauri commands wrapping rgb-core
src/App.tsx      React UI (single component)
scripts/         install.sh, install-service.sh, uninstall.sh, udev rule
```

## Commands

```sh
cargo test                          # all Rust tests, no hardware needed
cargo build --release -p rgb-cli    # CLI only
npx tsc --noEmit                    # frontend type check
npx tauri dev                       # run the GUI
npx tauri build --no-bundle         # release GUI binary
bash -n scripts/*.sh                # script syntax check
```

Before finishing a change, run `cargo test` and `npx tsc --noEmit`. Both must pass.

## Hardware safety rules (must follow)

This code writes to real hardware on a shared SMBus. A mistake can corrupt
memory SPD data or brick an LED controller.

- **Never** read or write SMBus addresses `0x50–0x57` (SPD EEPROM) or
  `0x30–0x37` (DDR4 SPD page select / write protect). A test in `smbus.rs`
  enforces this for the ENE range; keep it passing.
- Detection must be **read-only** and signature-based (for example, ENE
  registers `0xA0–0xAF` read back `0..=15`). Never write to an address before
  its signature has been confirmed.
- Never write the ENE apply register with `0xAA`, which saves to flash.
  Use `0x01`, which applies to volatile memory only.
- Don't add ENE address remapping (slot index / I2C address registers)
  without the owner's explicit approval.
- Don't add per-LED streaming for MSI boards; it's linked to firmware brick reports.
- Never run `cargo`, `npm` or the GUI as root. The scripts build as the
  invoking user on purpose.
- Don't run hardware-writing commands (`rgb-ctl set/off/apply`, raw SMBus/HID
  scripts) unless the user asks. They change the user's visible lighting.

## Protocol facts verified on hardware

Don't "fix" these back to what older docs or other projects say:

- HID++ `getFeature` takes the feature ID **big-endian** (`0x80, 0x70`).
  A reply of index `0` means "not supported" and must be treated as an error.
- HID++ command byte = `(function << 4) | sw_id`, with `sw_id != 0`
  (`HIDPP_SW_ID = 0x01`). Every request has a reply; read it and treat
  feature index `0xFF` as an error.
- 0x8070 has no "all zones" index. Query the zone count with `getInfo` and
  set each zone. The G PRO has 2 zones (keys, logo).
- XPG Spectrix D41 (`AUDA0-E6K5-0101`) ignores the V1 color table `0x8010`
  even though its name suggests V1. Colors must go to `0x8160`, in R, B, G
  order. Three sticks answer on a single address, `0x70`.

## Conventions

- Keep protocol logic in `rgb-core` as pure packet/byte builders with unit
  tests that assert exact bytes. I/O wrappers stay thin.
- A missing device is a graceful skip (`DeviceApplyResult::ok(..., "skipped")`),
  not a failure. A device that's present but rejects a write is an error.
- Errors use `RgbError` (`thiserror`). Don't `unwrap()` on I/O paths.
- Match the existing comment style: module-level `//!` docs citing the
  protocol reference, and short `//` comments explaining *why*.
- Frontend types in `src/App.tsx` mirror the Rust serde structs. Update both together.
- The default config has no fan zones (`fans: []`). The owner's fans are on an
  external hub, so "set all" must never touch the motherboard unless zones are
  configured explicitly.

## Privacy / publishing

- Never commit user configs (`config.json` is gitignored), logs, `.env`
  files, absolute home paths or machine-specific identifiers.
- Install scripts must not hardcode a username or home path. Derive them
  from `SUDO_USER` / `getent` at install time.
