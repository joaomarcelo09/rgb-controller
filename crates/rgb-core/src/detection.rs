use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// A HID raw device discovered through sysfs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HidDeviceInfo {
    /// Device node, e.g. `/dev/hidraw3`.
    pub path: PathBuf,
    pub vid: u16,
    pub pid: u16,
    pub name: String,
    pub serial: Option<String>,
}

/// An I2C/SMBus adapter discovered through sysfs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct I2cBusInfo {
    /// Adapter number, device node is `/dev/i2c-{number}`.
    pub number: u32,
    pub name: String,
}

pub const VID_LOGITECH: u16 = 0x046D;
pub const VID_MSI: u16 = 0x1462;

/// List all `/dev/hidraw*` devices by parsing sysfs uevent files.
pub fn list_hidraw_devices() -> Vec<HidDeviceInfo> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir("/sys/class/hidraw") else {
        return out;
    };
    for entry in entries.flatten() {
        let node_name = entry.file_name().to_string_lossy().to_string();
        if !node_name.starts_with("hidraw") {
            continue;
        }
        let Some(info) = parse_hidraw_uevent(&entry.path(), &node_name) else {
            continue;
        };
        out.push(info);
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

fn parse_hidraw_uevent(class_dir: &Path, node_name: &str) -> Option<HidDeviceInfo> {
    let uevent = fs::read_to_string(class_dir.join("device/uevent")).ok()?;
    let mut vid = 0u16;
    let mut pid = 0u16;
    let mut name = String::new();
    let mut serial = None;
    for line in uevent.lines() {
        if let Some(v) = line.strip_prefix("HID_ID=") {
            // Format: HID_ID=0003:00001462:00007C95
            let parts: Vec<&str> = v.split(':').collect();
            if parts.len() >= 3 {
                vid = u16::from_str_radix(parts[1], 16).unwrap_or(0);
                pid = u16::from_str_radix(parts[2], 16).unwrap_or(0);
            }
        } else if let Some(v) = line.strip_prefix("HID_NAME=") {
            name = v.to_string();
        } else if let Some(v) = line.strip_prefix("HID_UNIQ=") {
            serial = Some(v.to_string());
        }
    }
    if vid == 0 && pid == 0 {
        return None;
    }
    Some(HidDeviceInfo {
        path: PathBuf::from("/dev").join(node_name),
        vid,
        pid,
        name,
        serial,
    })
}

/// List SMBus-capable adapters (piix4 / i801 / Nuvoton etc).
pub fn list_i2c_buses() -> Vec<I2cBusInfo> {
    // On some kernels /sys/class/i2c-adapter is absent; the adapters are
    // still visible under /sys/bus/i2c/devices as "i2c-N" entries.
    let mut dirs = ["/sys/class/i2c-adapter", "/sys/bus/i2c/devices"];
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for dir in dirs.iter_mut() {
        let Ok(entries) = fs::read_dir(dir) else { continue };
        for entry in entries.flatten() {
            let dir_name = entry.file_name().to_string_lossy().to_string();
            // Dir names look like "i2c-0".
            let Some(num_str) = dir_name.strip_prefix("i2c-") else {
                continue;
            };
            let Ok(number) = num_str.parse::<u32>() else {
                continue;
            };
            if !seen.insert(number) {
                continue;
            }
            let Ok(name) = fs::read_to_string(entry.path().join("name")) else {
                continue;
            };
            out.push(I2cBusInfo { number, name: name.trim().to_string() });
        }
    }
    out.sort_by_key(|b| b.number);
    out
}

/// True when the adapter is a known SMBus host controller we may probe for
/// RGB RAM (avoids touching GPU aux buses and similar).
pub fn is_smbus_adapter(name: &str) -> bool {
    let n = name.to_lowercase();
    n.contains("smbus") || n.contains("piix4") || n.contains("i801") || n.contains("nuvoton")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_uevent_format() {
        let dir = std::env::temp_dir().join("rgb-core-test-uevent");
        let dev = dir.join("device");
        fs::create_dir_all(&dev).unwrap();
        fs::write(
            dev.join("uevent"),
            "HID_ID=0003:00001462:00007C95\nHID_NAME=MYSTIC LIGHT\nHID_UNIQ=ABC123\n",
        )
        .unwrap();
        let info = parse_hidraw_uevent(&dir, "hidraw5").unwrap();
        assert_eq!(info.vid, 0x1462);
        assert_eq!(info.pid, 0x7C95);
        assert_eq!(info.name, "MYSTIC LIGHT");
        assert_eq!(info.serial.as_deref(), Some("ABC123"));
        assert_eq!(info.path, PathBuf::from("/dev/hidraw5"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rejects_non_hid_uevents() {
        let dir = std::env::temp_dir().join("rgb-core-test-uevent-bad");
        let dev = dir.join("device");
        fs::create_dir_all(&dev).unwrap();
        fs::write(dev.join("uevent"), "DRIVER=usb\n").unwrap();
        assert!(parse_hidraw_uevent(&dir, "hidraw1").is_none());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn smbus_adapter_names() {
        assert!(is_smbus_adapter("SMBus PIIX4 adapter port 0 at 0b00"));
        assert!(is_smbus_adapter("i801_smbus"));
        assert!(!is_smbus_adapter("AMDGPU DM aux hw bus 1"));
    }
}
