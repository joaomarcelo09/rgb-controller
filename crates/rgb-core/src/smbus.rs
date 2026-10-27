//! RGB RAM control over the SMBus (i2c-dev interface).
//!
//! Reference: OpenRGB `CorsairDRAMController` (GPL-2.0-or-later).
//!
//! Corsair Vengeance-style modules expose registers on the SMBus:
//! detection via `0x43` (device id ∈ {0x1A,0x1B,0x1C}) and `0x44`
//! (led count ∈ {1,3,4}); direct color via block writes to `0x31`
//! (first 32 bytes) and `0x32` (remainder) with an 8-bit CRC footer.
//!
//! AMD platforms: probe `0x58..=0x5F`. Intel platforms: probe `0x18..=0x1F`.
//!
//! ENE controllers (XPG Spectrix, ASUS Aura DRAM; OpenRGB
//! `ENESMBusController`) sit at `0x70..=0x7F`. They use 16-bit registers:
//! a byte-swapped word write to command `0x00` sets the register pointer,
//! then command `0x01` writes and `0x81` reads a byte. Detection is a
//! read-only signature check (registers `0xA0..=0xAF` read back `0..=15`).
//! Several modules may share one address and are written together.
//!
//! SPD EEPROM addresses (`0x50..=0x57`) and DDR4 page-select addresses
//! (`0x30..=0x37`) are never touched.

use std::fs::{File, OpenOptions};
use std::os::fd::AsRawFd;

use log::{debug, warn};
use serde::{Deserialize, Serialize};

use crate::color::Color;
use crate::error::{Result, RgbError};

// Linux i2c-dev ioctls (stable ABI, /usr/include/linux/i2c-dev.h).
const I2C_SLAVE: libc::c_ulong = 0x0703;
const I2C_SMBUS: libc::c_ulong = 0x0720;
const I2C_SMBUS_WRITE: u8 = 0;
const I2C_SMBUS_READ: u8 = 1;
const I2C_SMBUS_BYTE_DATA: u32 = 2;
const I2C_SMBUS_WORD_DATA: u32 = 3;
const I2C_SMBUS_BLOCK_DATA: u32 = 5;

// Corsair DRAM registers.
const REG_RESET_BUFFER: u8 = 0x0B;
const REG_SET_BINARY_DATA: u8 = 0x20;
const REG_BINARY_START: u8 = 0x21;
const REG_COLOR_BUFFER_1: u8 = 0x31;
const REG_COLOR_BUFFER_2: u8 = 0x32;
const REG_BUSY_STATUS: u8 = 0x41;
const REG_DEVICE_INFO_A: u8 = 0x43;
const REG_DEVICE_INFO_B: u8 = 0x44;

// ENE DRAM registers.
const ENE_CMD_SET_POINTER: u8 = 0x00;
const ENE_CMD_WRITE_BYTE: u8 = 0x01;
const ENE_CMD_READ_BYTE: u8 = 0x81;
const ENE_REG_CONFIG_TABLE: u16 = 0x1C00;
const ENE_CONFIG_LED_COUNT: u16 = 0x02;
/// Effect color table (R, B, G per LED). The older `0x8010` table from the
/// V1 register map does not drive the LEDs on XPG Spectrix D41 modules
/// (`AUDA0-E6K5-0101`, verified on hardware); `0x8160` does.
const ENE_REG_COLORS_EFFECT: u16 = 0x8160;
const ENE_REG_DIRECT: u16 = 0x8020;
const ENE_REG_MODE: u16 = 0x8021;
/// Applies pending changes to RAM only; `0xAA` would save to flash.
const ENE_REG_APPLY: u16 = 0x80A0;
const ENE_APPLY_VAL: u8 = 0x01;
const ENE_MODE_OFF: u8 = 0x00;
const ENE_MODE_STATIC: u8 = 0x01;
/// Upper bound on LEDs written, so a bad config read can't run the color
/// table into unrelated registers.
const ENE_MAX_LEDS: u8 = 16;

/// Byte order the ENE controller expects in the word-write register pointer.
pub fn ene_pointer_word(reg: u16) -> u16 {
    reg.swap_bytes()
}

/// ENE effect color bytes for `led_count` LEDs: `[R, B, G]` per LED.
pub fn ene_color_bytes(color: Color, led_count: u8) -> Vec<u8> {
    (0..led_count).flat_map(|_| [color.r, color.b, color.g]).collect()
}
/// CRC-8 (poly 0x07, init 0x00, no reflection, no xorout) as used by
/// OpenRGB (`CRCPP::CRC::CRC_8`) for Corsair DRAM packets.
pub fn crc8(data: &[u8]) -> u8 {
    let mut crc: u8 = 0;
    for &b in data {
        crc ^= b;
        for _ in 0..8 {
            if crc & 0x80 != 0 {
                crc = (crc << 1) ^ 0x07;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

/// Build the Corsair DRAM direct-color packet:
/// `[led_count][rgb * led_count][crc8]`.
pub fn build_direct_packet(color: Color, led_count: u8) -> Vec<u8> {
    let mut p = Vec::with_capacity(led_count as usize * 3 + 2);
    p.push(led_count);
    for _ in 0..led_count {
        p.push(color.r);
        p.push(color.g);
        p.push(color.b);
    }
    let crc = crc8(&p);
    p.push(crc);
    p
}

#[repr(C)]
#[derive(Clone, Copy)]
union SmbusData {
    byte: u8,
    word: u16,
    block: [u8; 34],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct SmbusIoctlData {
    read_write: u8,
    command: u8,
    size: u32,
    data: *mut SmbusData,
}

unsafe fn smbus_ioctl(fd: i32, rw: u8, command: u8, size: u32, data: *mut SmbusData) -> Result<()> {
    let args = SmbusIoctlData { read_write: rw, command, size, data };
    let ret = unsafe { libc::ioctl(fd, I2C_SMBUS, &args as *const _) };
    if ret < 0 {
        Err(RgbError::Smbus(format!(
            "ioctl failed on cmd 0x{command:02X}: {}",
            std::io::Error::last_os_error()
        )))
    } else {
        Ok(())
    }
}

fn ioctl_i2c_slave(fd: i32, addr: u16) -> Result<()> {
    let ret = unsafe { libc::ioctl(fd, I2C_SLAVE, addr as libc::c_ulong) };
    if ret < 0 {
        Err(RgbError::Smbus(format!(
            "I2C_SLAVE ioctl failed for 0x{addr:02X}: {}",
            std::io::Error::last_os_error()
        )))
    } else {
        Ok(())
    }
}

fn smbus_read_byte_data(fd: i32, addr: u16, command: u8) -> Result<u8> {
    let mut data = SmbusData { byte: 0 };
    unsafe {
        ioctl_i2c_slave(fd, addr)?;
        smbus_ioctl(fd, I2C_SMBUS_READ, command, I2C_SMBUS_BYTE_DATA, &mut data)?;
        Ok(data.byte)
    }
}

fn smbus_write_byte_data(fd: i32, addr: u16, command: u8, value: u8) -> Result<()> {
    let mut data = SmbusData { byte: value };
    unsafe {
        ioctl_i2c_slave(fd, addr)?;
        smbus_ioctl(fd, I2C_SMBUS_WRITE, command, I2C_SMBUS_BYTE_DATA, &mut data)
    }
}

fn smbus_write_word_data(fd: i32, addr: u16, command: u8, value: u16) -> Result<()> {
    let mut data = SmbusData { word: value };
    unsafe {
        ioctl_i2c_slave(fd, addr)?;
        smbus_ioctl(fd, I2C_SMBUS_WRITE, command, I2C_SMBUS_WORD_DATA, &mut data)
    }
}

fn smbus_write_block_data(fd: i32, addr: u16, command: u8, values: &[u8]) -> Result<()> {
    if values.is_empty() || values.len() > 32 {
        return Err(RgbError::InvalidArgument(format!(
            "SMBus block write must be 1-32 bytes, got {}",
            values.len()
        )));
    }
    let mut data = SmbusData { block: [0u8; 34] };
    unsafe {
        data.block[0] = values.len() as u8;
        data.block[1..1 + values.len()].copy_from_slice(values);
        ioctl_i2c_slave(fd, addr)?;
        smbus_ioctl(fd, I2C_SMBUS_WRITE, command, I2C_SMBUS_BLOCK_DATA, &mut data)
    }
}

/// An open SMBus bus (`/dev/i2c-N`).
pub struct SmbusBus {
    file: File,
    /// Bus device number (for logging).
    pub number: u32,
}

impl SmbusBus {
    pub fn open(number: u32) -> Result<SmbusBus> {
        let path = format!("/dev/i2c-{number}");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|e| RgbError::Io(std::io::Error::new(e.kind(), format!("{path}: {e}"))))?;
        Ok(SmbusBus { file, number })
    }

    fn fd(&self) -> i32 {
        self.file.as_raw_fd()
    }

    /// Probe for a Corsair DRAM controller at `addr`.
    pub fn probe_corsair(&self, addr: u16) -> bool {
        let Ok(id) = smbus_read_byte_data(self.fd(), addr, REG_DEVICE_INFO_A) else {
            return false;
        };
        let Ok(leds) = smbus_read_byte_data(self.fd(), addr, REG_DEVICE_INFO_B) else {
            return false;
        };
        let ok = matches!(id, 0x1A..=0x1C) && matches!(leds, 0x01 | 0x03 | 0x04);
        if ok {
            debug!("SMBus {}: Corsair DRAM found at 0x{addr:02X}", self.number);
        }
        ok
    }

    /// Write the direct color packet to a Corsair DRAM module.
    pub fn corsair_set_direct(&self, addr: u16, packet: &[u8]) -> Result<()> {
        debug!("SMBus {}: direct packet for 0x{addr:02X}: {packet:02X?}", self.number);
        // Busy-wait until the module is ready.
        for _ in 0..50 {
            match smbus_read_byte_data(self.fd(), addr, REG_BUSY_STATUS) {
                Ok(0) | Err(_) => break,
                Ok(_) => std::thread::sleep(std::time::Duration::from_millis(10)),
            }
        }
        let (first, second) = if packet.len() > 32 {
            (&packet[..32], Some(&packet[32..]))
        } else {
            (packet, None)
        };
        match smbus_write_block_data(self.fd(), addr, REG_COLOR_BUFFER_1, first) {
            Ok(()) => {}
            Err(e) => {
                warn!("block write failed, falling back to binary protocol: {e}");
                return self.corsair_set_direct_fallback(addr, packet);
            }
        }
        if let Some(rest) = second {
            smbus_write_block_data(self.fd(), addr, REG_COLOR_BUFFER_2, rest)?;
        }
        Ok(())
    }

    /// Fallback used when SMBus block transfers are unsupported: stream the
    /// packet through the binary-data registers (OpenRGB `WriteRGBBuff`).
    fn corsair_set_direct_fallback(&self, addr: u16, packet: &[u8]) -> Result<()> {
        smbus_write_byte_data(self.fd(), addr, REG_RESET_BUFFER, 0x00)?;
        smbus_write_byte_data(self.fd(), addr, REG_BINARY_START, 0x00)?;
        for &b in packet {
            smbus_write_byte_data(self.fd(), addr, REG_SET_BINARY_DATA, b)?;
        }
        // Apply color data buffer (ID 2).
        smbus_write_byte_data(self.fd(), addr, 0x82, 2)?;
        Ok(())
    }
}

impl SmbusBus {
    /// Probe for an ENE DRAM controller at `addr` (read-only).
    pub fn probe_ene(&self, addr: u16) -> bool {
        let ok = (0xA0..=0xAFu8)
            .all(|reg| matches!(smbus_read_byte_data(self.fd(), addr, reg), Ok(v) if v == reg - 0xA0));
        if ok {
            debug!("SMBus {}: ENE DRAM controller found at 0x{addr:02X}", self.number);
        }
        ok
    }

    fn ene_read(&self, addr: u16, reg: u16) -> Result<u8> {
        smbus_write_word_data(self.fd(), addr, ENE_CMD_SET_POINTER, ene_pointer_word(reg))?;
        smbus_read_byte_data(self.fd(), addr, ENE_CMD_READ_BYTE)
    }

    fn ene_write(&self, addr: u16, reg: u16, value: u8) -> Result<()> {
        smbus_write_word_data(self.fd(), addr, ENE_CMD_SET_POINTER, ene_pointer_word(reg))?;
        smbus_write_byte_data(self.fd(), addr, ENE_CMD_WRITE_BYTE, value)
    }

    /// LED count from the controller's config table.
    pub fn ene_led_count(&self, addr: u16) -> Result<u8> {
        self.ene_read(addr, ENE_REG_CONFIG_TABLE + ENE_CONFIG_LED_COUNT)
    }

    /// Set every LED to a static color (black turns the LEDs off). Changes
    /// are applied but not saved, so the boot service re-applies them.
    pub fn ene_set_static(&self, addr: u16, color: Color, led_count: u8) -> Result<()> {
        if led_count == 0 || led_count > ENE_MAX_LEDS {
            return Err(RgbError::Smbus(format!(
                "ENE 0x{addr:02X}: implausible LED count {led_count}, refusing to write"
            )));
        }
        for (i, b) in ene_color_bytes(color, led_count).into_iter().enumerate() {
            self.ene_write(addr, ENE_REG_COLORS_EFFECT + i as u16, b)?;
        }
        let mode = if color.is_off() { ENE_MODE_OFF } else { ENE_MODE_STATIC };
        self.ene_write(addr, ENE_REG_DIRECT, 0)?;
        self.ene_write(addr, ENE_REG_MODE, mode)?;
        self.ene_write(addr, ENE_REG_APPLY, ENE_APPLY_VAL)
    }
}

/// Addresses probed for Corsair DRAM on AMD (PIIX4) chipsets.
pub const CORSAIR_AMD_ADDRESSES: std::ops::RangeInclusive<u16> = 0x58..=0x5F;
/// Addresses probed for Corsair DRAM on Intel (i801) chipsets.
pub const CORSAIR_INTEL_ADDRESSES: std::ops::RangeInclusive<u16> = 0x18..=0x1F;

/// Addresses probed for ENE DRAM controllers (any chipset).
pub const ENE_ADDRESSES: std::ops::RangeInclusive<u16> = 0x70..=0x7F;

/// RGB controller family on a RAM module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RamController {
    Corsair,
    /// ENE (XPG Spectrix, ASUS Aura DRAM).
    Ene,
}

/// A detected RAM module.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RamModule {
    pub bus: u32,
    pub address: u8,
    pub led_count: u8,
    pub controller: RamController,
}

impl RamModule {
    /// Apply a static color to this module.
    pub fn set_static(&self, bus: &SmbusBus, color: Color) -> Result<()> {
        match self.controller {
            RamController::Corsair => bus.corsair_set_direct(
                self.address as u16,
                &build_direct_packet(color, self.led_count),
            ),
            RamController::Ene => bus.ene_set_static(self.address as u16, color, self.led_count),
        }
    }
}

/// Detect Corsair DRAM modules on a bus; returns their addresses.
pub fn detect_corsair_modules(bus: &SmbusBus, is_amd: bool) -> Vec<RamModule> {
    let range = if is_amd { CORSAIR_AMD_ADDRESSES } else { CORSAIR_INTEL_ADDRESSES };
    let mut out = Vec::new();
    for addr in range {
        if bus.probe_corsair(addr) {
            out.push(RamModule {
                bus: bus.number,
                address: addr as u8,
                led_count: 10,
                controller: RamController::Corsair,
            });
        }
    }
    out
}

/// Detect ENE DRAM controllers on a bus.
pub fn detect_ene_modules(bus: &SmbusBus) -> Vec<RamModule> {
    let mut out = Vec::new();
    for addr in ENE_ADDRESSES {
        if !bus.probe_ene(addr) {
            continue;
        }
        match bus.ene_led_count(addr) {
            Ok(led_count) => out.push(RamModule {
                bus: bus.number,
                address: addr as u8,
                led_count,
                controller: RamController::Ene,
            }),
            Err(e) => warn!("SMBus {}: ENE 0x{addr:02X} LED count read failed: {e}", bus.number),
        }
    }
    out
}

/// Detect every supported RGB RAM controller on a bus.
pub fn detect_ram_modules(bus: &SmbusBus, is_amd: bool) -> Vec<RamModule> {
    let mut out = detect_corsair_modules(bus, is_amd);
    out.extend(detect_ene_modules(bus));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc8_known_vectors() {
        // CRC-8/SMBUS-style with init 0: check-vector for "123456789" with
        // poly 0x07/init 0/no reflection is 0xF4.
        assert_eq!(crc8(b"123456789"), 0xF4);
        assert_eq!(crc8(&[]), 0x00);
    }

    #[test]
    fn direct_packet_structure() {
        let p = build_direct_packet(Color::RED, 5);
        assert_eq!(p.len(), 5 * 3 + 2);
        assert_eq!(p[0], 5); // led count
        assert_eq!(&p[1..4], &[255, 0, 0]); // first led
        let crc = crc8(&p[..p.len() - 1]);
        assert_eq!(p[p.len() - 1], crc);
    }

    #[test]
    fn direct_packet_led_counts() {
        for n in [1u8, 5, 10, 32] {
            let p = build_direct_packet(Color::BLUE, n);
            assert_eq!(p.len() as u8, n * 3 + 2);
        }
    }

    #[test]
    fn block_write_rejects_oversized() {
        let p = build_direct_packet(Color::RED, 12); // 38 bytes
        let fd = -1; // ioctl on invalid fd fails anyway, but length check fires first
        assert!(smbus_write_block_data(fd, 0x58, REG_COLOR_BUFFER_1, &p).is_err());
    }

    #[test]
    fn ene_pointer_is_byte_swapped() {
        assert_eq!(ene_pointer_word(0x8160), 0x6081);
        assert_eq!(ene_pointer_word(0x1C02), 0x021C);
    }

    #[test]
    fn ene_colors_are_rbg_ordered() {
        let c = Color { r: 1, g: 2, b: 3 };
        assert_eq!(ene_color_bytes(c, 2), vec![1, 3, 2, 1, 3, 2]);
    }

    #[test]
    fn ene_addresses_avoid_spd_and_page_select() {
        assert!(ENE_ADDRESSES.clone().all(|a| !(0x30..=0x37).contains(&a) && !(0x50..=0x57).contains(&a)));
    }

    #[test]
    fn packet_offsets_stay_within_block_limits() {
        // 10 LEDs -> 32 bytes, fits in a single block write.
        let p = build_direct_packet(Color::GREEN, 10);
        assert_eq!(p.len(), 32);
        assert!(p.len() <= 32);
    }
}
