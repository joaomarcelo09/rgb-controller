//! Logitech HID++ 2.0 keyboard RGB control via feature `0x8070`
//! (COLOR LED EFFECTS).
//!
//! Reference: OpenRGB `LogitechProtocolCommon.{h,cpp}` (GPL-2.0-or-later):
//! 20-byte long FAP report: report_id `0x11`, device index `0xFF`,
//! feature index, command, 16 data bytes.
//!
//! The command byte is `(function << 4) | software_id`. Software ID 0 is
//! reserved for device notifications, so requests use `HIDPP_SW_ID`.
//!
//! Setting a static color: function 3 (setZoneEffect) with
//! `data[0] = zone`, `data[1] = mode` (1 = static), `data[2..5] = RGB`,
//! `data[12] = 0x00`. Zones are addressed one at a time (0 = keys,
//! 1 = logo on the G PRO); there is no "all zones" index.

use hidapi::{HidApi, HidDevice};
use log::debug;

use crate::color::Color;
use crate::error::{Result, RgbError};

pub const LOGITECH_LONG_REPORT_ID: u8 = 0x11;
pub const LOGITECH_LONG_REPORT_LEN: usize = 20;
pub const LOGITECH_DEVICE_INDEX: u8 = 0xFF;

pub const HIDPP_SW_ID: u8 = 0x01;
/// Feature index of HID++ 2.0 error responses.
pub const HIDPP_ERROR_FEATURE_INDEX: u8 = 0xFF;

pub const HIDPP_PAGE_ROOT: u16 = 0x0000;
pub const HIDPP_ROOT_GET_FEATURE: u8 = 0x00 | HIDPP_SW_ID;
pub const HIDPP_PAGE_RGB_EFFECTS1: u16 = 0x8070;
pub const FP8070_GET_INFO: u8 = 0x00 | HIDPP_SW_ID;
pub const FP8070_SET_EFFECT: u8 = 0x30 | HIDPP_SW_ID;
pub const FP8070_SET_SW_CTL: u8 = 0x80 | HIDPP_SW_ID;

/// How long to wait for a HID++ response.
const RESPONSE_TIMEOUT_MS: i32 = 300;

/// HID++ 2.0 long report (20 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HidppLongReport {
    pub report_id: u8,
    pub device_index: u8,
    pub feature_index: u8,
    pub command: u8,
    pub data: [u8; 16],
}

impl HidppLongReport {
    pub fn new(feature_index: u8, command: u8) -> HidppLongReport {
        HidppLongReport {
            report_id: LOGITECH_LONG_REPORT_ID,
            device_index: LOGITECH_DEVICE_INDEX,
            feature_index,
            command,
            data: [0u8; 16],
        }
    }

    pub fn to_bytes(&self) -> [u8; LOGITECH_LONG_REPORT_LEN] {
        let mut out = [0u8; LOGITECH_LONG_REPORT_LEN];
        out[0] = self.report_id;
        out[1] = self.device_index;
        out[2] = self.feature_index;
        out[3] = self.command;
        out[4..20].copy_from_slice(&self.data);
        out
    }

    pub fn from_bytes(b: &[u8; LOGITECH_LONG_REPORT_LEN]) -> HidppLongReport {
        let mut data = [0u8; 16];
        data.copy_from_slice(&b[4..20]);
        HidppLongReport {
            report_id: b[0],
            device_index: b[1],
            feature_index: b[2],
            command: b[3],
            data,
        }
    }

    /// Query the feature index for a HID++ feature page via the ROOT feature.
    pub fn get_feature_request(feature_page: u16) -> HidppLongReport {
        let mut r = HidppLongReport::new(0x00, HIDPP_ROOT_GET_FEATURE);
        // Big-endian feature page in data[0..2] (HID++ 2.0 spec).
        r.data[0] = (feature_page >> 8) as u8;
        r.data[1] = (feature_page & 0xFF) as u8;
        r
    }

    /// Set an effect (static color, breathing, spectrum or off) on one zone.
    ///
    /// Byte usage follows OpenRGB `LogitechProtocolCommon::setMode` for the
    /// 0x8070 feature:
    /// - `data[0]` zone, `data[1]` mode
    /// - `data[2..5]` RGB
    /// - `data[5..7]` speed (uint16 BE) for breathing; unused for static
    /// - `data[7..9]` speed for spectrum
    /// - `data[8]` brightness for breathing, `data[9]` for spectrum
    /// - `data[12]` persistence/power-mode bits (0x00 for 0x8070)
    ///
    /// `brightness` (0-100) is only meaningful for breathing/spectrum modes
    /// and is ignored for static, matching the vendor firmware behavior.
    pub fn set_effect(
        feature_index: u8,
        zone: u8,
        mode: u8,
        color: Color,
        brightness: u8,
    ) -> HidppLongReport {
        let mut r = HidppLongReport::new(feature_index, FP8070_SET_EFFECT);
        r.data[0] = zone;
        r.data[1] = mode;
        r.data[2] = color.r;
        r.data[3] = color.g;
        r.data[4] = color.b;
        match mode {
            crate::config::logitech_mode::BREATHING => {
                r.data[8] = brightness;
            }
            crate::config::logitech_mode::SPECTRUM => {
                r.data[9] = brightness;
            }
            _ => {}
        }
        // Power mode / persistence bits; 0x00 for 0x8070 (OpenRGB behavior).
        r.data[12] = 0x00;
        r
    }

    /// Enable/disable software (direct) control of the RGB feature.
    pub fn set_software_control(feature_index: u8, enable: bool) -> HidppLongReport {
        let mut r = HidppLongReport::new(feature_index, FP8070_SET_SW_CTL);
        let v = if enable { 1 } else { 0 };
        r.data[0] = v;
        r.data[1] = v;
        r
    }
}

/// Logitech keyboard PIDs with the 0x8070 RGB effects feature.
pub fn is_supported_logitech_keyboard(pid: u16) -> bool {
    matches!(
        pid,
        0xC330 | // G PRO Keyboard (first gen)
        0xC335 | // G512
        0xC336 | // G512 SE
        0xC337 | // G815
        0xC338 | // G512 Carbon
        0xC339 | // G PRO Gaming Keyboard
        0xC33A | // G413
        0xC33B | // G513 Carbon
        0xC33C | // G513 Silver
        0xC33D | // G915 TKL
        0xC33F // G915
    )
}

/// Controller for a Logitech HID++ keyboard.
pub struct LogitechKeyboard {
    dev: HidDevice,
    path: String,
    /// Feature index of 0x8070, queried at open time.
    feature_index: u8,
    /// Number of lighting zones reported by 0x8070 getInfo.
    zone_count: u8,
}

impl LogitechKeyboard {
    pub fn open(path: &str) -> Result<LogitechKeyboard> {
        let api = HidApi::new()?;
        let cpath = std::ffi::CString::new(path)?;
        let dev = api
            .open_path(&cpath)
            .map_err(|e| RgbError::Hid(format!("cannot open {path}: {e}")))?;
        let feature_index = Self::query_feature_index(&dev)?;
        let info = transact(&dev, &HidppLongReport::new(feature_index, FP8070_GET_INFO))?;
        // Fall back to a single zone if the device reports none.
        let zone_count = info.data[0].max(1);
        debug!("Logitech {path}: RGB feature 0x8070 at index {feature_index}, {zone_count} zone(s)");
        Ok(LogitechKeyboard { dev, path: path.to_string(), feature_index, zone_count })
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn feature_index(&self) -> u8 {
        self.feature_index
    }

    fn query_feature_index(dev: &HidDevice) -> Result<u8> {
        let req = HidppLongReport::get_feature_request(HIDPP_PAGE_RGB_EFFECTS1);
        let resp = transact(dev, &req).map_err(|e| match e {
            // Non-HID++ interfaces never answer; skip them quietly.
            RgbError::Unsupported(_) => e,
            other => RgbError::Unsupported(format!("feature query failed: {other}")),
        })?;
        // Index 0 means the device doesn't implement the feature.
        if resp.data[0] == 0 {
            return Err(RgbError::Unsupported("device has no 0x8070 RGB feature".into()));
        }
        Ok(resp.data[0])
    }

    /// Apply a static color to every zone (keys, logo, ...).
    pub fn set_static(&self, color: Color, brightness: u8) -> Result<()> {
        let mode = if color.is_off() {
            crate::config::logitech_mode::OFF
        } else {
            crate::config::logitech_mode::STATIC
        };
        for zone in 0..self.zone_count {
            let report =
                HidppLongReport::set_effect(self.feature_index, zone, mode, color, brightness);
            transact(&self.dev, &report)?;
        }
        Ok(())
    }

    /// Take software control of the RGB feature (recommended before writing
    /// effects so the firmware doesn't override them).
    pub fn set_software_control(&self, enable: bool) -> Result<()> {
        let report = HidppLongReport::set_software_control(self.feature_index, enable);
        transact(&self.dev, &report).map(|_| ())
    }
}

/// Send a request and wait for its matching response. HID++ 2.0 answers every
/// request, either echoing feature index + command or with an error report
/// (`0xFF`, original feature index, original command, error code).
fn transact(dev: &HidDevice, req: &HidppLongReport) -> Result<HidppLongReport> {
    let bytes = req.to_bytes();
    let n = dev.write(&bytes)?;
    if n != LOGITECH_LONG_REPORT_LEN {
        return Err(RgbError::Hid(format!(
            "short write: expected {LOGITECH_LONG_REPORT_LEN} bytes, wrote {n}"
        )));
    }
    debug!("Logitech: sent report {:02X?}", bytes);

    let mut buf = [0u8; LOGITECH_LONG_REPORT_LEN];
    loop {
        let n = dev.read_timeout(&mut buf, RESPONSE_TIMEOUT_MS)?;
        if n == 0 {
            return Err(RgbError::Unsupported(
                "no HID++ response (not the RGB interface?)".into(),
            ));
        }
        if n != LOGITECH_LONG_REPORT_LEN || buf[0] != LOGITECH_LONG_REPORT_ID {
            continue; // unrelated report (short HID++ or input), keep waiting
        }
        let resp = HidppLongReport::from_bytes(&buf);
        if resp.feature_index == HIDPP_ERROR_FEATURE_INDEX
            && resp.command == req.feature_index
            && resp.data[0] == req.command
        {
            return Err(RgbError::Hid(format!(
                "HID++ error 0x{:02X} for feature {:02X} command {:02X}",
                resp.data[1], req.feature_index, req.command
            )));
        }
        if resp.feature_index == req.feature_index && resp.command == req.command {
            return Ok(resp);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_report_layout_is_20_bytes() {
        let r = HidppLongReport::new(0x0E, FP8070_SET_EFFECT);
        let b = r.to_bytes();
        assert_eq!(b.len(), 20);
        assert_eq!(b[0], 0x11);
        assert_eq!(b[1], 0xFF);
        assert_eq!(b[2], 0x0E);
        assert_eq!(b[3], FP8070_SET_EFFECT);
        assert!(b[4..].iter().all(|&x| x == 0));
    }

    #[test]
    fn set_effect_static_payload() {
        let r = HidppLongReport::set_effect(0x0E, 0, 1, Color::RED, 100);
        let b = r.to_bytes();
        assert_eq!(b[0], 0x11);
        assert_eq!(b[1], 0xFF);
        assert_eq!(b[2], 0x0E);
        assert_eq!(b[3], FP8070_SET_EFFECT);
        assert_eq!(b[4], 0); // zone
        assert_eq!(b[5], 1); // mode static
        assert_eq!(b[6], 255); // R
        assert_eq!(b[7], 0); // G
        assert_eq!(b[8], 0); // B
        // Brightness is ignored for static mode (OpenRGB behavior).
        assert_eq!(b[12], 0);
        assert_eq!(b[16], 0x00); // persistence bits
    }

    #[test]
    fn brightness_used_only_for_effect_modes() {
        let breathing = HidppLongReport::set_effect(0x0E, 0, 0x0A, Color::WHITE, 77);
        assert_eq!(breathing.data[8], 77);
        let spectrum = HidppLongReport::set_effect(0x0E, 0, 0x03, Color::WHITE, 77);
        assert_eq!(spectrum.data[9], 77);
        let static_ = HidppLongReport::set_effect(0x0E, 0, 0x01, Color::WHITE, 77);
        assert_eq!(static_.data[8], 0);
        assert_eq!(static_.data[9], 0);
    }

    #[test]
    fn feature_query_request_format() {
        let r = HidppLongReport::get_feature_request(HIDPP_PAGE_RGB_EFFECTS1);
        let b = r.to_bytes();
        assert_eq!(b[0], 0x11);
        assert_eq!(b[1], 0xFF);
        assert_eq!(b[2], 0x00); // ROOT feature
        assert_eq!(b[3], HIDPP_ROOT_GET_FEATURE);
        assert_eq!(b[4], 0x80); // high byte first (big-endian)
        assert_eq!(b[5], 0x70);
    }

    #[test]
    fn set_effect_targets_zone() {
        let logo = HidppLongReport::set_effect(0x0D, 1, 1, Color::RED, 100);
        assert_eq!(logo.data[0], 1);
    }

    #[test]
    fn commands_carry_nonzero_software_id() {
        for cmd in [HIDPP_ROOT_GET_FEATURE, FP8070_GET_INFO, FP8070_SET_EFFECT, FP8070_SET_SW_CTL] {
            assert_ne!(cmd & 0x0F, 0, "command {cmd:02X} has software id 0");
        }
    }

    #[test]
    fn software_control_payload() {
        let r = HidppLongReport::set_software_control(0x0E, true);
        let b = r.to_bytes();
        assert_eq!(b[3], FP8070_SET_SW_CTL);
        assert_eq!(b[4], 1);
        assert_eq!(b[5], 1);
    }

    #[test]
    fn roundtrip_report() {
        let r = HidppLongReport::set_effect(0x0B, 1, 0x0A, Color { r: 1, g: 2, b: 3 }, 50);
        let b = r.to_bytes();
        assert_eq!(HidppLongReport::from_bytes(&b), r);
    }

    #[test]
    fn pid_list_contains_user_keyboard() {
        assert!(is_supported_logitech_keyboard(0xC339));
        assert!(is_supported_logitech_keyboard(0xC33F));
        assert!(!is_supported_logitech_keyboard(0x1234));
    }
}
