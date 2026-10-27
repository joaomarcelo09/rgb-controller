//! High-level orchestration: apply a `Config` to all detected devices.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::config::{Config, ZoneSetting};
use crate::detection::{is_smbus_adapter, list_hidraw_devices, list_i2c_buses, HidDeviceInfo, I2cBusInfo};
use crate::error::{Result, RgbError};
use crate::logitech::{is_supported_logitech_keyboard, LogitechKeyboard};
use crate::msi::{is_supported_msi_pid, MsiController};
use crate::smbus::{detect_ram_modules, RamController, RamModule, SmbusBus};

/// Result of applying settings to one device category.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceApplyResult {
    pub device: String,
    pub ok: bool,
    pub message: String,
}

impl DeviceApplyResult {
    fn ok(device: impl Into<String>, message: impl Into<String>) -> DeviceApplyResult {
        DeviceApplyResult { device: device.into(), ok: true, message: message.into() }
    }
    fn err(device: impl Into<String>, message: impl Into<String>) -> DeviceApplyResult {
        DeviceApplyResult { device: device.into(), ok: false, message: message.into() }
    }
}

/// Aggregate result of `apply_config`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplyReport {
    pub results: Vec<DeviceApplyResult>,
}

impl ApplyReport {
    pub fn has_errors(&self) -> bool {
        self.results.iter().any(|r| !r.ok)
    }
}

/// All RGB-capable devices found on this system (for the UI).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemDevices {
    pub keyboards: Vec<HidDeviceInfo>,
    pub msi_boards: Vec<HidDeviceInfo>,
    pub ram_modules: Vec<RamModule>,
    pub smbus_buses: Vec<I2cBusInfo>,
    pub hidraw_devices: Vec<HidDeviceInfo>,
}

/// Detect devices relevant to this app. `probe_ram` scans SMBus for RAM
/// modules (safe address ranges only) — set to false for a fast listing.
pub fn detect_system_devices(probe_ram: bool) -> SystemDevices {
    let hid = list_hidraw_devices();
    let buses = list_i2c_buses();
    let mut ram_modules = Vec::new();
    if probe_ram {
        for bus in &buses {
            if !is_smbus_adapter(&bus.name) {
                continue;
            }
            let Ok(handle) = SmbusBus::open(bus.number) else {
                continue;
            };
            // PIIX4 => AMD, i801 => Intel.
            let is_amd = bus.name.to_lowercase().contains("piix4");
            ram_modules.extend(detect_ram_modules(&handle, is_amd));
        }
    }
    SystemDevices {
        keyboards: hid
            .iter()
            .filter(|d| d.vid == crate::detection::VID_LOGITECH && is_supported_logitech_keyboard(d.pid))
            .cloned()
            .collect(),
        msi_boards: hid
            .iter()
            .filter(|d| d.vid == crate::detection::VID_MSI && is_supported_msi_pid(d.pid))
            .cloned()
            .collect(),
        ram_modules,
        smbus_buses: buses,
        hidraw_devices: hid,
    }
}

fn sleep_short() {
    std::thread::sleep(Duration::from_millis(30));
}

fn debug_skip(results: &mut Vec<DeviceApplyResult>, device: &str) {
    log::debug!("{device}: not the RGB interface, skipping");
    results.push(DeviceApplyResult::ok(device, "skipped (not the RGB HID interface)"));
}

/// Apply a full configuration to every detected device.
pub fn apply_config(config: &Config) -> ApplyReport {
    let mut results = Vec::new();

    // ---- Keyboard ----
    if let Some(keyboard) = &config.keyboard {
        let color = config
            .global_color
            .unwrap_or(keyboard.color);
        let keyboards: Vec<HidDeviceInfo> = list_hidraw_devices()
            .into_iter()
            .filter(|d| {
                d.vid == crate::detection::VID_LOGITECH && is_supported_logitech_keyboard(d.pid)
            })
            .collect();
        if keyboards.is_empty() {
            // AC5: a missing device is a graceful skip, not a failure.
            results.push(DeviceApplyResult::ok(
                "keyboard",
                "no supported Logitech keyboard found, skipped",
            ));
        }
        for k in keyboards {
            let path = k.path.display().to_string();
            match LogitechKeyboard::open(&path) {
                Ok(kb) => {
                    let _ = kb.set_software_control(true);
                    sleep_short();
                    // set_static selects OFF mode automatically when the
                    // color is black.
                    match kb.set_static(color, keyboard.brightness) {
                        Ok(()) => results.push(DeviceApplyResult::ok(
                            format!("keyboard ({})", kb.path()),
                            format!("set {}", color),
                        )),
                        Err(e) => results.push(DeviceApplyResult::err(
                            format!("keyboard ({})", kb.path()),
                            e.to_string(),
                        )),
                    }
                }
                // A keyboard may expose several HID interfaces; only one
                // carries the 0x8070 RGB feature. Skip the others.
                Err(RgbError::Unsupported(_)) => {
                    debug_skip(&mut results, &format!("keyboard ({path})"));
                }
                Err(e) => results.push(DeviceApplyResult::err(
                    format!("keyboard ({path})"),
                    e.to_string(),
                )),
            }
        }
    } else {
        results.push(DeviceApplyResult::ok("keyboard", "not configured, skipped"));
    }

    // ---- Fans / motherboard ----
    // Only explicitly configured zones are written; a global color recolors
    // them but never adds zones, so "set all" leaves the board alone.
    let mut zones: Vec<ZoneSetting> = config.fans.clone();
    if let Some(global) = config.global_color {
        for z in &mut zones {
            z.mode = crate::config::msi_mode::STATIC;
            z.color = global;
            z.color2 = global;
        }
    }
    if !zones.is_empty() {
        let boards: Vec<HidDeviceInfo> = list_hidraw_devices()
            .into_iter()
            .filter(|d| d.vid == crate::detection::VID_MSI && is_supported_msi_pid(d.pid))
            .collect();
        if boards.is_empty() {
            // AC5: a missing device is a graceful skip, not a failure.
            results.push(DeviceApplyResult::ok(
                "fans",
                "no supported MSI Mystic Light board found, skipped",
            ));
        }
        for board in boards {
            let path = board.path.display().to_string();
            match MsiController::open(&path) {
                Ok(ctrl) => match ctrl.apply_zones(&zones) {
                    Ok(()) => results.push(DeviceApplyResult::ok(
                        format!("fans ({})", ctrl.path()),
                        format!("applied {} zone(s)", zones.len()),
                    )),
                    Err(e) => results.push(DeviceApplyResult::err(
                        format!("fans ({path})"),
                        e.to_string(),
                    )),
                },
                Err(e) => {
                    results.push(DeviceApplyResult::err(format!("fans ({path})"), e.to_string()))
                }
            }
            sleep_short();
        }
    }

    // ---- RAM ----
    if let Some(ram) = &config.ram {
        let color = config.global_color.unwrap_or(ram.color);
        let mut modules_found = 0;
        for bus in list_i2c_buses() {
            if !is_smbus_adapter(&bus.name) {
                continue;
            }
            let handle = match SmbusBus::open(bus.number) {
                Ok(h) => h,
                Err(_) => continue,
            };
            let is_amd = bus.name.to_lowercase().contains("piix4");
            for mut module in detect_ram_modules(&handle, is_amd) {
                modules_found += 1;
                // The configured LED count (PRD §8) takes precedence over
                // the Corsair detector's default. ENE controllers report
                // their own count, which is authoritative.
                if ram.led_count > 0 && module.controller == RamController::Corsair {
                    module.led_count = ram.led_count;
                }
                match module.set_static(&handle, color) {
                    Ok(()) => {
                        results.push(DeviceApplyResult::ok(
                            format!("ram (bus {}, 0x{:02X})", module.bus, module.address),
                            format!("set {color} ({} leds)", module.led_count),
                        ));
                    }
                    Err(e) => results.push(DeviceApplyResult::err(
                        format!("ram (bus {}, 0x{:02X})", module.bus, module.address),
                        e.to_string(),
                    )),
                }
                sleep_short();
            }
        }
        if modules_found == 0 {
            // AC5: a missing device is a graceful skip, not a failure.
            results.push(DeviceApplyResult::ok(
                "ram",
                "no supported RGB RAM modules found on SMBus, skipped \
                 (check i2c-dev module, i2c group membership, or unsupported modules)",
            ));
        }
    } else {
        results.push(DeviceApplyResult::ok("ram", "not configured, skipped"));
    }

    ApplyReport { results }
}

/// Apply one color to all devices (used by the UI quick action).
pub fn apply_global_color(color: crate::Color) -> ApplyReport {
    let config = Config { global_color: Some(color), ..Config::default() };
    apply_config(&config)
}

pub fn validate_config(config: &Config) -> Result<()> {
    for z in &config.fans {
        if z.mode > 38 {
            return Err(RgbError::Config(format!(
                "zone {}: mode {} out of range (0-38)",
                z.zone.name(),
                z.mode
            )));
        }
        if z.brightness > 10 {
            return Err(RgbError::Config(format!(
                "zone {}: brightness {} out of range (0-10)",
                z.zone.name(),
                z.brightness
            )));
        }
        if z.speed > 2 {
            return Err(RgbError::Config(format!(
                "zone {}: speed {} out of range (0-2)",
                z.zone.name(),
                z.speed
            )));
        }
    }
    if let Some(k) = &config.keyboard {
        if !matches!(k.mode, 0x00 | 0x01 | 0x03 | 0x0A) {
            return Err(RgbError::Config(format!(
                "keyboard: mode 0x{:02X} not supported (0=off, 1=static, 3=spectrum, A=breathing)",
                k.mode
            )));
        }
        if k.brightness > 100 {
            return Err(RgbError::Config(format!(
                "keyboard: brightness {} out of range (0-100)",
                k.brightness
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn validate_accepts_default_config() {
        assert!(validate_config(&Config::default()).is_ok());
    }

    #[test]
    fn validate_rejects_bad_brightness() {
        let mut c = Config::default();
        c.fans.push(ZoneSetting {
            zone: crate::config::FanZone::Jrainbow1,
            mode: crate::config::msi_mode::STATIC,
            color: crate::Color::BLUE,
            color2: crate::Color::BLUE,
            brightness: 10,
            speed: 0,
        });
        c.fans[0].brightness = 11;
        assert!(validate_config(&c).is_err());
        c.fans[0].brightness = 10;
        c.fans[0].speed = 3;
        assert!(validate_config(&c).is_err());
    }

    #[test]
    fn validate_rejects_bad_keyboard_brightness() {
        let mut c = Config::default();
        c.keyboard.as_mut().unwrap().brightness = 101;
        assert!(validate_config(&c).is_err());
    }

    #[test]
    fn report_has_errors_flag() {
        let mut r = ApplyReport { results: vec![] };
        assert!(!r.has_errors());
        r.results.push(DeviceApplyResult::err("x", "boom"));
        assert!(r.has_errors());
    }

    #[test]
    fn global_color_skips_unconfigured_fans() {
        let config = Config {
            global_color: Some(crate::Color::GREEN),
            keyboard: None,
            ram: None,
            ..Config::default()
        };
        // No fan zones configured: the board is never touched.
        let report = apply_config(&config);
        assert!(report.results.iter().all(|r| !r.device.starts_with("fans")));
    }
}
