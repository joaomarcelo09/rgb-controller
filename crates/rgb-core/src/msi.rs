//! MSI Mystic Light 185-byte motherboard controller (USB HID feature reports).
//!
//! Protocol reference:
//! - OpenRGB `MSIMysticLight185Controller` (GPL-2.0-or-later), zone layout & modes
//! - Community reverse-engineering (stephenlzc/msi-mystic-light-web, MIT):
//!   the speed/brightness flags byte MUST have bit 7 set (hardware enable),
//!   and the packet must be sent twice (preview `save=0`, then commit `save=1`).

use std::thread;
use std::time::Duration;

use hidapi::{HidApi, HidDevice};
use log::debug;

use crate::config::{msi_mode, FanZone, ZoneSetting};
use crate::color::Color;
use crate::error::{Result, RgbError};

pub const MSI_REPORT_ID: u8 = 0x52;
pub const MSI_PACKET_LEN: usize = 185;
/// Report ID used for the (not yet supported) per-LED direct mode.
pub const MSI_PERLED_REPORT_ID: u8 = 0x53;

/// Bit 7 of the speed/brightness flags byte: hardware enable flag.
/// OpenRGB never sets this, which is why zones stay dark.
pub const FLAGS_ENABLE_BIT: u8 = 0x80;
/// Zone colors enabled / non-rainbow flag.
pub const COLOR_FLAGS_NON_RAINBOW: u8 = 0x80;
/// Sync flag used for the onboard zone (OpenRGB: 0x80 | SYNC_SETTING_ONBOARD).
pub const SYNC_SETTING_ONBOARD: u8 = 0x01;
/// Default cycle/LED count byte for JRAINBOW zones (OpenRGB default).
pub const JRAINBOW_CYCLE_DEFAULT: u8 = 100;
/// Fan flags default for the JCORSAIR zone (OpenRGB default).
pub const CORSAIR_FAN_FLAGS_DEFAULT: u8 = 40;

/// Brightness range: 0 (off) .. 10 (max).
pub const MAX_BRIGHTNESS: u8 = 10;
/// Speed range: 0 .. 2.
pub const MAX_SPEED: u8 = 2;

/// Zone sub-packet (10 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZoneData {
    pub effect: u8,
    pub color: Color,
    pub flags: u8,
    pub color2: Color,
    pub color_flags: u8,
    pub padding: u8,
}

impl ZoneData {
    pub fn new_static(color: Color, brightness: u8, speed: u8) -> ZoneData {
        ZoneData {
            effect: msi_mode::STATIC,
            color,
            flags: build_flags(brightness, speed),
            color2: color,
            color_flags: COLOR_FLAGS_NON_RAINBOW,
            padding: 0,
        }
    }

    pub fn encode(&self) -> [u8; 10] {
        [
            self.effect,
            self.color.r,
            self.color.g,
            self.color.b,
            self.flags,
            self.color2.r,
            self.color2.g,
            self.color2.b,
            self.color_flags,
            self.padding,
        ]
    }

    pub fn decode(b: &[u8]) -> ZoneData {
        ZoneData {
            effect: b[0],
            color: Color { r: b[1], g: b[2], b: b[3] },
            flags: b[4],
            color2: Color { r: b[5], g: b[6], b: b[7] },
            color_flags: b[8],
            padding: b[9],
        }
    }
}

/// Rainbow zone sub-packet: zone data + cycle/LED count byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RainbowZoneData {
    pub zone: ZoneData,
    pub cycle_or_led: u8,
}

impl RainbowZoneData {
    pub fn new_static(color: Color, brightness: u8, speed: u8) -> RainbowZoneData {
        RainbowZoneData {
            zone: ZoneData::new_static(color, brightness, speed),
            cycle_or_led: JRAINBOW_CYCLE_DEFAULT,
        }
    }

    pub fn encode(&self) -> [u8; 11] {
        let mut out = [0u8; 11];
        out[..10].copy_from_slice(&self.zone.encode());
        out[10] = self.cycle_or_led;
        out
    }

    pub fn decode(b: &[u8]) -> RainbowZoneData {
        RainbowZoneData { zone: ZoneData::decode(&b[..10]), cycle_or_led: b[10] }
    }
}

/// JCORSAIR zone sub-packet (11 bytes, different layout).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CorsairZoneData {
    pub effect: u8,
    pub color: Color,
    pub fan_flags: u8,
    pub corsair_quantity: u8,
    pub padding: [u8; 4],
    pub is_individual: u8,
}

impl CorsairZoneData {
    pub fn encode(&self) -> [u8; 11] {
        [
            self.effect,
            self.color.r,
            self.color.g,
            self.color.b,
            self.fan_flags,
            self.corsair_quantity,
            self.padding[0],
            self.padding[1],
            self.padding[2],
            self.padding[3],
            self.is_individual,
        ]
    }

    pub fn decode(b: &[u8]) -> CorsairZoneData {
        CorsairZoneData {
            effect: b[0],
            color: Color { r: b[1], g: b[2], b: b[3] },
            fan_flags: b[4],
            corsair_quantity: b[5],
            padding: [b[6], b[7], b[8], b[9]],
            is_individual: b[10],
        }
    }
}

/// Compute the speed/brightness flags byte including the enable bit.
///
/// `(brightness << 2) | (speed & 0x03) | 0x80`
pub fn build_flags(brightness: u8, speed: u8) -> u8 {
    let br = brightness.min(MAX_BRIGHTNESS);
    let sp = speed.min(MAX_SPEED);
    (br << 2) | (sp & 0x03) | FLAGS_ENABLE_BIT
}

/// Full 185-byte feature packet (report ID 0x52).
///
/// Offsets from OpenRGB `FeaturePacket_185`:
/// 1: JRGB1, 11: PIPE1, 21: PIPE2, 31: JRAINBOW1(11B), 42: JRAINBOW2(11B),
/// 53: JCORSAIR(11B), 64: JCORSAIR_OUTER, 74: ONBOARD, 84..174: ONBOARD1-9,
/// 174: JRGB2, 184: save_data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MsiPacket185 {
    pub j_rgb_1: ZoneData,
    pub j_pipe_1: ZoneData,
    pub j_pipe_2: ZoneData,
    pub j_rainbow_1: RainbowZoneData,
    pub j_rainbow_2: RainbowZoneData,
    pub j_corsair: CorsairZoneData,
    pub j_corsair_outer: ZoneData,
    pub on_board: ZoneData,
    pub on_board_1: ZoneData,
    pub on_board_2: ZoneData,
    pub on_board_3: ZoneData,
    pub on_board_4: ZoneData,
    pub on_board_5: ZoneData,
    pub on_board_6: ZoneData,
    pub on_board_7: ZoneData,
    pub on_board_8: ZoneData,
    pub on_board_9: ZoneData,
    pub j_rgb_2: ZoneData,
    pub save_data: u8,
}

impl Default for MsiPacket185 {
    fn default() -> Self {
        let blank = ZoneData {
            effect: msi_mode::OFF,
            color: Color::OFF,
            flags: build_flags(MAX_BRIGHTNESS, 0),
            color2: Color::OFF,
            color_flags: COLOR_FLAGS_NON_RAINBOW,
            padding: 0,
        };
        MsiPacket185 {
            j_rgb_1: blank,
            j_pipe_1: blank,
            j_pipe_2: blank,
            j_rainbow_1: RainbowZoneData { zone: blank, cycle_or_led: JRAINBOW_CYCLE_DEFAULT },
            j_rainbow_2: RainbowZoneData { zone: blank, cycle_or_led: JRAINBOW_CYCLE_DEFAULT },
            j_corsair: CorsairZoneData {
                effect: msi_mode::OFF,
                color: Color::OFF,
                fan_flags: CORSAIR_FAN_FLAGS_DEFAULT,
                corsair_quantity: 0,
                padding: [0; 4],
                is_individual: 0,
            },
            j_corsair_outer: blank,
            on_board: blank,
            on_board_1: blank,
            on_board_2: blank,
            on_board_3: blank,
            on_board_4: blank,
            on_board_5: blank,
            on_board_6: blank,
            on_board_7: blank,
            on_board_8: blank,
            on_board_9: blank,
            j_rgb_2: blank,
            save_data: 0,
        }
    }
}

impl MsiPacket185 {
    pub fn to_bytes(&self) -> [u8; MSI_PACKET_LEN] {
        let mut out = [0u8; MSI_PACKET_LEN];
        out[0] = MSI_REPORT_ID;
        out[1..11].copy_from_slice(&self.j_rgb_1.encode());
        out[11..21].copy_from_slice(&self.j_pipe_1.encode());
        out[21..31].copy_from_slice(&self.j_pipe_2.encode());
        out[31..42].copy_from_slice(&self.j_rainbow_1.encode());
        out[42..53].copy_from_slice(&self.j_rainbow_2.encode());
        out[53..64].copy_from_slice(&self.j_corsair.encode());
        out[64..74].copy_from_slice(&self.j_corsair_outer.encode());
        out[74..84].copy_from_slice(&self.on_board.encode());
        out[84..94].copy_from_slice(&self.on_board_1.encode());
        out[94..104].copy_from_slice(&self.on_board_2.encode());
        out[104..114].copy_from_slice(&self.on_board_3.encode());
        out[114..124].copy_from_slice(&self.on_board_4.encode());
        out[124..134].copy_from_slice(&self.on_board_5.encode());
        out[134..144].copy_from_slice(&self.on_board_6.encode());
        out[144..154].copy_from_slice(&self.on_board_7.encode());
        out[154..164].copy_from_slice(&self.on_board_8.encode());
        out[164..174].copy_from_slice(&self.on_board_9.encode());
        out[174..184].copy_from_slice(&self.j_rgb_2.encode());
        out[184] = self.save_data;
        out
    }

    pub fn from_bytes(b: &[u8; MSI_PACKET_LEN]) -> MsiPacket185 {
        debug_assert_eq!(b[0], MSI_REPORT_ID, "unexpected report id");
        MsiPacket185 {
            j_rgb_1: ZoneData::decode(&b[1..11]),
            j_pipe_1: ZoneData::decode(&b[11..21]),
            j_pipe_2: ZoneData::decode(&b[21..31]),
            j_rainbow_1: RainbowZoneData::decode(&b[31..42]),
            j_rainbow_2: RainbowZoneData::decode(&b[42..53]),
            j_corsair: CorsairZoneData::decode(&b[53..64]),
            j_corsair_outer: ZoneData::decode(&b[64..74]),
            on_board: ZoneData::decode(&b[74..84]),
            on_board_1: ZoneData::decode(&b[84..94]),
            on_board_2: ZoneData::decode(&b[94..104]),
            on_board_3: ZoneData::decode(&b[104..114]),
            on_board_4: ZoneData::decode(&b[114..124]),
            on_board_5: ZoneData::decode(&b[124..134]),
            on_board_6: ZoneData::decode(&b[134..144]),
            on_board_7: ZoneData::decode(&b[144..154]),
            on_board_8: ZoneData::decode(&b[154..164]),
            on_board_9: ZoneData::decode(&b[164..174]),
            j_rgb_2: ZoneData::decode(&b[174..184]),
            save_data: b[184],
        }
    }

    /// Read the zone sub-packet. Returns `None` for `JCORSAIR`, whose layout
    /// differs from the standard zone data (use `set_zone` to modify it).
    pub fn zone_data(&self, zone: FanZone) -> Option<&ZoneData> {
        match zone {
            FanZone::Jrgb1 => Some(&self.j_rgb_1),
            FanZone::Jrgb2 => Some(&self.j_rgb_2),
            FanZone::Jrainbow1 => Some(&self.j_rainbow_1.zone),
            FanZone::Jrainbow2 => Some(&self.j_rainbow_2.zone),
            FanZone::Onboard => Some(&self.on_board),
            FanZone::Pipe1 => Some(&self.j_pipe_1),
            FanZone::Pipe2 => Some(&self.j_pipe_2),
            FanZone::Corsair => None,
            FanZone::CorsairOuter => Some(&self.j_corsair_outer),
        }
    }

    pub fn set_zone_static(&mut self, zone: FanZone, color: Color, brightness: u8, speed: u8) {
        let data = ZoneData::new_static(color, brightness, speed);
        self.set_zone(zone, data);
    }

    pub fn set_zone(&mut self, zone: FanZone, data: ZoneData) {
        match zone {
            FanZone::Jrgb1 => self.j_rgb_1 = data,
            FanZone::Jrgb2 => self.j_rgb_2 = data,
            FanZone::Jrainbow1 => self.j_rainbow_1.zone = data,
            FanZone::Jrainbow2 => self.j_rainbow_2.zone = data,
            FanZone::Onboard => {
                // Onboard zone keeps the sync flag set (OpenRGB behavior).
                let mut d = data;
                d.color_flags |= SYNC_SETTING_ONBOARD;
                self.on_board = d;
            }
            FanZone::Pipe1 => self.j_pipe_1 = data,
            FanZone::Pipe2 => self.j_pipe_2 = data,
            FanZone::Corsair => {
                let c = self.j_corsair;
                self.j_corsair = CorsairZoneData {
                    effect: data.effect,
                    color: data.color,
                    fan_flags: c.fan_flags,
                    corsair_quantity: c.corsair_quantity,
                    padding: c.padding,
                    is_individual: c.is_individual,
                };
            }
            FanZone::CorsairOuter => self.j_corsair_outer = data,
        }
    }
}

impl CorsairZoneData {
    /// View of the color portion of the JCORSAIR zone.
    pub fn zone_data_view(&self) -> ZoneData {
        ZoneData {
            effect: self.effect,
            color: self.color,
            flags: self.fan_flags,
            color2: self.color,
            color_flags: self.is_individual,
            padding: 0,
        }
    }
}

/// PIDs known to use the Mystic Light HID protocol.
pub fn is_supported_msi_pid(pid: u16) -> bool {
    matches!(
        pid,
        0x0076 | // common PID for a set of 185-byte boards
        0x7B12 | 0x7B16 | 0x7B17 | 0x7B18 | 0x7B50 | 0x7B85 | 0x7B92 | 0x7B93 |
        0x7C02 | 0x7C34 | 0x7C35 | 0x7C36 | 0x7C37 | 0x7C56 | 0x7C59 | 0x7C60 |
        0x7C67 | 0x7C71 | 0x7C73 | 0x7C75 | 0x7C76 | 0x7C77 | 0x7C79 | 0x7C80 |
        0x7C81 | 0x7C82 | 0x7C83 | 0x7C84 | 0x7C86 | 0x7C87 | 0x7C90 | 0x7C91 |
        0x7C92 | 0x7C94 | 0x7C95 | 0x7C98 | 0x7D03 | 0x7D04 | 0x7D06 | 0x7D07 |
        0x7D08 | 0x7D09 | 0x7D13 | 0x7D14 | 0x7D15 | 0x7D17 | 0x7D18 | 0x7D19 |
        0x7D20 | 0x7D25 | 0x7D27 | 0x7D28 | 0x7D29 | 0x7D30 | 0x7D31 | 0x7D32 |
        0x7D33 | 0x7D36 | 0x7D37 | 0x7D38 | 0x7D40 | 0x7D41 | 0x7D42 | 0x7D43 |
        0x7D46 | 0x7D50 | 0x7D51 | 0x7D52 | 0x7D53 | 0x7D54 | 0x7D59 | 0x7D67 |
        0x7D69 | 0x7D70 | 0x7D73 | 0x7D74 | 0x7D75 | 0x7D76 | 0x7D77 | 0x7D78 |
        0x7D86 | 0x7D88 | 0x7D89 | 0x7D90 | 0x7D91 | 0x7D93 | 0x7D96 | 0x7D97 |
        0x7D98 | 0x7D99 | 0x7E01 | 0x7E03 | 0x7E06 | 0x7E07 | 0x7E09 | 0x7E10 |
        0x7E20 | 0x7E32 | 0x7E34 | 0x7E59 | 0x7E70 | 0x7E80 | 0x7E81 | 0xB926
    )
}

/// Controller handle for one MSI Mystic Light HID device.
pub struct MsiController {
    dev: HidDevice,
    path: String,
}

impl MsiController {
    pub fn open(path: &str) -> Result<MsiController> {
        let api = HidApi::new()?;
        let cpath = std::ffi::CString::new(path)?;
        let dev = api
            .open_path(&cpath)
            .map_err(|e| RgbError::Hid(format!("cannot open {path}: {e}")))?;
        Ok(MsiController { dev, path: path.to_string() })
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    /// Prepare a read buffer for `hid_get_feature_report`: the first byte
    /// must carry the report ID of the report to be read (hidapi requirement).
    fn read_buffer() -> [u8; MSI_PACKET_LEN] {
        let mut buf = [0u8; MSI_PACKET_LEN];
        buf[0] = MSI_REPORT_ID;
        buf
    }

    /// Read the current 185-byte packet from the device.
    pub fn read_packet(&self) -> Result<MsiPacket185> {
        let mut buf = Self::read_buffer();
        let n = self.dev.get_feature_report(&mut buf)?;
        if n != MSI_PACKET_LEN {
            return Err(RgbError::Hid(format!(
                "short feature report: expected {MSI_PACKET_LEN} bytes, got {n}"
            )));
        }
        if buf[0] != MSI_REPORT_ID {
            return Err(RgbError::Hid(format!(
                "unexpected report id 0x{:02X}, expected 0x{MSI_REPORT_ID:02X}",
                buf[0]
            )));
        }
        Ok(MsiPacket185::from_bytes(&buf))
    }

    /// Send the packet as preview (`save=0`) or commit (`save=1`).
    pub fn write_packet(&self, packet: &MsiPacket185, save: bool) -> Result<()> {
        let mut p = *packet;
        p.save_data = if save { 1 } else { 0 };
        let bytes = p.to_bytes();
        self.dev.send_feature_report(&bytes)?;
        debug!(
            "MSI {}: sent 185-byte packet (save={})",
            self.path,
            if save { "commit" } else { "preview" }
        );
        Ok(())
    }

    /// Apply the given zone settings using read-modify-write and the
    /// preview+commit double-send required by the firmware.
    pub fn apply_zones(&self, zones: &[ZoneSetting]) -> Result<()> {
        // Read current board state so untouched zones are preserved. If the
        // read fails we refuse to write: sending a blank packet would turn
        // every other zone off.
        let mut packet = self
            .read_packet()
            .map_err(|e| RgbError::Hid(format!("{}: read failed, refusing to write: {e}", self.path)))?;

        for z in zones {
            if z.mode == msi_mode::OFF {
                if let Some(data) = packet.zone_data(z.zone) {
                    let mut data = *data;
                    data.effect = msi_mode::OFF;
                    packet.set_zone(z.zone, data);
                } else {
                    // JCORSAIR: set effect off directly.
                    packet.j_corsair.effect = msi_mode::OFF;
                }
                log::debug!("MSI {}: zone {} -> off", self.path, z.zone.name());
                continue;
            }
            packet.set_zone_static(z.zone, z.color, z.brightness, z.speed);
            log::debug!(
                "MSI {}: zone {} -> {} brightness={} speed={}",
                self.path,
                z.zone.name(),
                z.color,
                z.brightness,
                z.speed
            );
        }

        self.write_packet(&packet, false)?;
        thread::sleep(Duration::from_millis(50));
        self.write_packet(&packet, true)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reference_blue() -> [u8; MSI_PACKET_LEN] {
        // From the MSI wiki capture (JRGB1 static blue, brightness 10,
        // color flags 0x80) with the enable bit added (0x28 -> 0xA8).
        let mut p = [0u8; MSI_PACKET_LEN];
        p[0] = 0x52;
        p[1..11].copy_from_slice(&[0x01, 0x00, 0x00, 0xFF, 0xA8, 0x00, 0x00, 0xFF, 0x80, 0x00]);
        p
    }

    #[test]
    fn packet_offsets_match_wiki_layout() {
        let p = MsiPacket185::default();
        let b = p.to_bytes();
        assert_eq!(b.len(), 185);
        assert_eq!(b[0], 0x52);
        // JRGB1 ends at byte 10, PIPE1 starts at 11, JRGB2 at 174, save at 184.
        assert_eq!(b[184], 0);
    }

    #[test]
    fn static_blue_jrgb1_matches_reference() {
        let mut p = MsiPacket185::default();
        p.set_zone_static(FanZone::Jrgb1, Color::BLUE, 10, 0);
        let b = p.to_bytes();
        assert_eq!(&b[1..11], &reference_blue()[1..11]);
        // Enable bit must be set (the OpenRGB bug fix).
        assert_eq!(b[5] & 0x80, 0x80);
        assert_eq!(b[5], 0xA8);
    }

    #[test]
    fn enable_bit_always_set_in_flags() {
        assert_eq!(build_flags(10, 0), 0xA8);
        assert_eq!(build_flags(5, 1), (5 << 2) | 1 | 0x80);
        assert_eq!(build_flags(10, 2), (10 << 2) | 2 | 0x80);
        // Clamping.
        assert_eq!(build_flags(255, 255), 0x80 | 0x28 | 0x02);
    }

    #[test]
    fn rainbow_zones_have_cycle_byte() {
        let mut p = MsiPacket185::default();
        p.set_zone_static(FanZone::Jrainbow1, Color::RED, 10, 0);
        let b = p.to_bytes();
        // JRAINBOW1 at offset 31, 11 bytes, cycle byte at 41.
        assert_eq!(b[31], msi_mode::STATIC);
        assert_eq!(b[41], JRAINBOW_CYCLE_DEFAULT);
    }

    #[test]
    fn off_mode_keeps_other_zones() {
        let mut p = MsiPacket185::default();
        p.set_zone_static(FanZone::Jrgb1, Color::GREEN, 8, 0);
        p.set_zone_static(FanZone::Jrainbow2, Color::RED, 10, 2);
        let bytes = p.to_bytes();

        let mut p2 = MsiPacket185::from_bytes(&bytes);
        // Turn JRGB1 off, JRAINBOW2 must be untouched.
        let jrgb1 = *p2.zone_data(FanZone::Jrgb1).unwrap();
        p2.set_zone(FanZone::Jrgb1, ZoneData { effect: msi_mode::OFF, ..jrgb1 });
        let b2 = p2.to_bytes();
        assert_eq!(b2[1], msi_mode::OFF);
        assert_eq!(b2[42..53], bytes[42..53]);
    }

    #[test]
    fn roundtrip_preserves_all_fields() {
        let mut p = MsiPacket185::default();
        p.set_zone_static(FanZone::Jrgb1, Color { r: 1, g: 2, b: 3 }, 4, 1);
        p.set_zone_static(FanZone::Jrainbow1, Color { r: 9, g: 8, b: 7 }, 6, 2);
        p.set_zone_static(FanZone::Onboard, Color { r: 5, g: 5, b: 5 }, 10, 0);
        p.save_data = 1;
        let b = p.to_bytes();
        assert_eq!(MsiPacket185::from_bytes(&b), p);
    }

    #[test]
    fn onboard_keeps_sync_flag() {
        let mut p = MsiPacket185::default();
        p.set_zone_static(FanZone::Onboard, Color::WHITE, 10, 0);
        assert_eq!(p.on_board.color_flags & SYNC_SETTING_ONBOARD, SYNC_SETTING_ONBOARD);
        assert_eq!(p.on_board.color_flags, 0x80 | 0x01);
    }

    #[test]
    fn jrgb2_and_offsets() {
        let mut p = MsiPacket185::default();
        p.set_zone_static(FanZone::Jrgb2, Color::GREEN, 10, 0);
        let b = p.to_bytes();
        assert_eq!(b[174], msi_mode::STATIC);
        assert_eq!(b[175..178], [0x00, 0xFF, 0x00]);
        assert_eq!(b[178] & 0x80, 0x80);
    }

    #[test]
    fn read_buffer_starts_with_report_id() {
        // hidapi requires the first byte of the read buffer to carry the
        // report ID before get_feature_report is called.
        let buf = MsiController::read_buffer();
        assert_eq!(buf[0], MSI_REPORT_ID);
        assert_eq!(buf.len(), MSI_PACKET_LEN);
    }

    #[test]
    fn supported_pid_list_contains_user_board() {
        assert!(is_supported_msi_pid(0x7C95));
        assert!(is_supported_msi_pid(0x0076));
        assert!(!is_supported_msi_pid(0x1234));
    }

    #[test]
    fn corsair_zone_encoding_layout() {
        let c = CorsairZoneData {
            effect: msi_mode::STATIC,
            color: Color::BLUE,
            fan_flags: CORSAIR_FAN_FLAGS_DEFAULT,
            corsair_quantity: 3,
            padding: [0; 4],
            is_individual: 0,
        };
        let b = c.encode();
        assert_eq!(b.len(), 11);
        assert_eq!(b[0], msi_mode::STATIC);
        assert_eq!(b[1..4], [0, 0, 255]);
        assert_eq!(b[4], CORSAIR_FAN_FLAGS_DEFAULT);
        assert_eq!(b[5], 3);
        assert_eq!(CorsairZoneData::decode(&b), c);
    }
}
