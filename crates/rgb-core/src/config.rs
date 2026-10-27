use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::color::Color;
use crate::error::{Result, RgbError};

/// A zone on the MSI motherboard (fan headers / onboard LEDs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FanZone {
    Jrgb1,
    Jrgb2,
    Jrainbow1,
    Jrainbow2,
    Onboard,
    Pipe1,
    Pipe2,
    Corsair,
    CorsairOuter,
}

impl FanZone {
    pub const ALL: [FanZone; 9] = [
        FanZone::Jrgb1,
        FanZone::Jrgb2,
        FanZone::Jrainbow1,
        FanZone::Jrainbow2,
        FanZone::Onboard,
        FanZone::Pipe1,
        FanZone::Pipe2,
        FanZone::Corsair,
        FanZone::CorsairOuter,
    ];

    pub fn name(&self) -> &'static str {
        match self {
            FanZone::Jrgb1 => "JRGB1",
            FanZone::Jrgb2 => "JRGB2",
            FanZone::Jrainbow1 => "JRAINBOW1",
            FanZone::Jrainbow2 => "JRAINBOW2",
            FanZone::Onboard => "ONBOARD",
            FanZone::Pipe1 => "PIPE1",
            FanZone::Pipe2 => "PIPE2",
            FanZone::Corsair => "JCORSAIR",
            FanZone::CorsairOuter => "JCORSAIR_OUTER",
        }
    }

    pub fn from_name(s: &str) -> Option<FanZone> {
        let s = s.to_ascii_lowercase().replace([' ', '-', '_'], "");
        match s.as_str() {
            "jrgb1" => Some(FanZone::Jrgb1),
            "jrgb2" => Some(FanZone::Jrgb2),
            "jrainbow1" => Some(FanZone::Jrainbow1),
            "jrainbow2" => Some(FanZone::Jrainbow2),
            "onboard" => Some(FanZone::Onboard),
            "pipe1" => Some(FanZone::Pipe1),
            "pipe2" => Some(FanZone::Pipe2),
            "jcorsair" => Some(FanZone::Corsair),
            "jcorsairouter" | "jcorsair_outer" => Some(FanZone::CorsairOuter),
            _ => None,
        }
    }
}

/// Effect mode IDs (MSI Mystic Light 185-byte protocol).
pub mod msi_mode {
    pub const OFF: u8 = 0;
    pub const STATIC: u8 = 1;
    pub const BREATHING: u8 = 2;
    pub const FLASHING: u8 = 3;
    pub const LIGHTNING: u8 = 5;
    pub const RAINBOW: u8 = 25;
}

/// Logitech HID++ 0x8070 effect mode IDs.
pub mod logitech_mode {
    pub const OFF: u8 = 0x00;
    pub const STATIC: u8 = 0x01;
    pub const SPECTRUM: u8 = 0x03;
    pub const BREATHING: u8 = 0x0A;
}

/// Per-zone setting for the motherboard/fan headers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ZoneSetting {
    pub zone: FanZone,
    /// Effect mode; `msi_mode::STATIC` (1) for a solid color.
    #[serde(default = "default_mode")]
    pub mode: u8,
    pub color: Color,
    /// Secondary color used by dual-color effects.
    #[serde(default)]
    pub color2: Color,
    /// Brightness 0-10 (0 = off).
    #[serde(default = "default_brightness")]
    pub brightness: u8,
    /// Effect speed 0-2.
    #[serde(default)]
    pub speed: u8,
}

fn default_mode() -> u8 {
    msi_mode::STATIC
}
fn default_brightness() -> u8 {
    10
}

/// Keyboard setting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyboardSetting {
    /// HID++ effect mode; `logitech_mode::STATIC` for solid color.
    #[serde(default = "default_mode")]
    pub mode: u8,
    pub color: Color,
    /// Brightness 0-100, mapped into the effect payload.
    #[serde(default = "default_keyboard_brightness")]
    pub brightness: u8,
}

fn default_keyboard_brightness() -> u8 {
    100
}

/// RAM setting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RamSetting {
    pub color: Color,
    /// LEDs per module (Corsair modules: 5 or 10 typical). Default 10.
    #[serde(default = "default_ram_led_count")]
    pub led_count: u8,
}

fn default_ram_led_count() -> u8 {
    10
}

/// Persistent configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Quick single color applied to every device ("set all").
    pub global_color: Option<Color>,
    pub keyboard: Option<KeyboardSetting>,
    pub fans: Vec<ZoneSetting>,
    pub ram: Option<RamSetting>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            global_color: None,
            keyboard: Some(KeyboardSetting {
                mode: logitech_mode::STATIC,
                color: Color::BLUE,
                brightness: 100,
            }),
            // The case fans hang off their own hub, not the board, so no
            // motherboard zones are driven unless configured explicitly.
            fans: vec![],
            ram: Some(RamSetting { color: Color::BLUE, led_count: 10 }),
        }
    }
}

impl Config {
    pub fn config_path() -> PathBuf {
        if let Ok(p) = std::env::var("RGB_CONTROLLER_CONFIG") {
            return PathBuf::from(p);
        }
        let base = std::env::var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/tmp".into()))
                    .join(".config")
            });
        base.join("rgb-controller").join("config.json")
    }

    pub fn load() -> Result<Config> {
        let path = Self::config_path();
        if !path.exists() {
            return Ok(Config::default());
        }
        let raw = fs::read_to_string(&path)
            .map_err(|e| RgbError::Config(format!("cannot read {}: {e}", path.display())))?;
        serde_json::from_str(&raw)
            .map_err(|e| RgbError::Config(format!("cannot parse {}: {e}", path.display())))
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| RgbError::Config(format!("cannot create {}: {e}", parent.display())))?;
        }
        let raw = serde_json::to_string_pretty(self)?;
        fs::write(&path, raw)
            .map_err(|e| RgbError::Config(format!("cannot write {}: {e}", path.display())))
    }

    /// Effective color for a device category, taking `global_color` into account.
    pub fn keyboard_color(&self) -> Option<Color> {
        self.global_color
            .or_else(|| self.keyboard.as_ref().map(|k| k.color))
    }

    pub fn ram_color(&self) -> Option<Color> {
        self.global_color.or_else(|| self.ram.as_ref().map(|r| r.color))
    }

    pub fn fan_color(&self, zone: FanZone) -> Option<Color> {
        self.global_color.or_else(|| {
            self.fans
                .iter()
                .find(|z| z.zone == zone)
                .map(|z| z.color)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zone(zone: FanZone) -> ZoneSetting {
        ZoneSetting {
            zone,
            mode: msi_mode::STATIC,
            color: Color::BLUE,
            color2: Color::BLUE,
            brightness: 10,
            speed: 0,
        }
    }

    #[test]
    fn default_config_has_expected_values() {
        let c = Config::default();
        assert!(c.keyboard.is_some());
        assert!(c.ram.is_some());
        assert!(c.fans.is_empty());
        assert_eq!(c.keyboard.as_ref().unwrap().mode, logitech_mode::STATIC);
    }

    #[test]
    fn config_roundtrips_json() {
        let c = Config::default();
        let s = serde_json::to_string(&c).unwrap();
        let c2: Config = serde_json::from_str(&s).unwrap();
        assert_eq!(c, c2);
    }

    #[test]
    fn serde_uses_expected_zone_names() {
        let c = Config { fans: vec![zone(FanZone::Jrgb1), zone(FanZone::Jrainbow1)], ..Config::default() };
        let s = serde_json::to_string(&c).unwrap();
        assert!(s.contains("\"jrgb1\""));
        assert!(s.contains("\"jrainbow1\""));
    }

    #[test]
    fn global_color_overrides() {
        let c = Config { global_color: Some(Color::RED), ..Config::default() };
        assert_eq!(c.keyboard_color(), Some(Color::RED));
        assert_eq!(c.ram_color(), Some(Color::RED));
        assert_eq!(c.fan_color(FanZone::Jrgb1), Some(Color::RED));
        assert_eq!(c.fan_color(FanZone::Onboard), Some(Color::RED));
    }

    #[test]
    fn without_global_uses_specific() {
        let c = Config { fans: vec![zone(FanZone::Jrainbow1)], ..Config::default() };
        assert_eq!(c.keyboard_color(), Some(Color::BLUE));
        assert_eq!(c.fan_color(FanZone::Jrainbow1), Some(Color::BLUE));
        assert_eq!(c.fan_color(FanZone::Onboard), None);
    }

    #[test]
    fn zone_name_roundtrip() {
        for z in FanZone::ALL {
            assert_eq!(FanZone::from_name(z.name()), Some(z));
            let lower = z.name().to_lowercase();
            assert_eq!(FanZone::from_name(&lower), Some(z));
        }
        assert_eq!(FanZone::from_name("bogus"), None);
    }
}
