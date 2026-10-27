use std::fmt;

use serde::{Deserialize, Serialize};

/// An 8-bit RGB color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    pub const OFF: Color = Color { r: 0, g: 0, b: 0 };
    pub const WHITE: Color = Color { r: 255, g: 255, b: 255 };
    pub const RED: Color = Color { r: 255, g: 0, b: 0 };
    pub const GREEN: Color = Color { r: 0, g: 255, b: 0 };
    pub const BLUE: Color = Color { r: 0, g: 0, b: 255 };

    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Color { r, g, b }
    }

    /// Parse a color from `#RRGGBB`, `RRGGBB` or `rrggbb` (case-insensitive hex).
    pub fn from_hex(s: &str) -> Result<Color, HexParseError> {
        let s = s.trim().trim_start_matches('#');
        if s.len() != 6 {
            return Err(HexParseError::InvalidLength(s.len()));
        }
        let mut out = [0u8; 3];
        for (i, byte) in out.iter_mut().enumerate() {
            let pair = s.get(i * 2..i * 2 + 2).ok_or(HexParseError::InvalidChar)?;
            *byte = u8::from_str_radix(pair, 16).map_err(|_| HexParseError::InvalidChar)?;
        }
        Ok(Color { r: out[0], g: out[1], b: out[2] })
    }

    pub fn to_hex(&self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }

    pub fn is_off(&self) -> bool {
        self.r == 0 && self.g == 0 && self.b == 0
    }
}

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HexParseError {
    InvalidLength(usize),
    InvalidChar,
}

impl fmt::Display for HexParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HexParseError::InvalidLength(l) => write!(f, "expected 6 hex digits, got {l}"),
            HexParseError::InvalidChar => write!(f, "invalid hex character"),
        }
    }
}

impl std::error::Error for HexParseError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_with_and_without_prefix() {
        assert_eq!(Color::from_hex("#FF0000").unwrap(), Color::RED);
        assert_eq!(Color::from_hex("ff0000").unwrap(), Color::RED);
        assert_eq!(Color::from_hex("00ff00").unwrap(), Color::GREEN);
        assert_eq!(Color::from_hex("#0000FF").unwrap(), Color::BLUE);
    }

    #[test]
    fn parses_lowercase_and_mixed_case() {
        assert_eq!(Color::from_hex("#aB12cD").unwrap(), Color { r: 0xAB, g: 0x12, b: 0xCD });
    }

    #[test]
    fn rejects_bad_input() {
        assert!(Color::from_hex("#FFF").is_err());
        assert!(Color::from_hex("#GGGGGG").is_err());
        assert!(Color::from_hex("#FF000").is_err());
    }

    #[test]
    fn roundtrips_hex() {
        let c = Color { r: 0x1A, g: 0xB2, b: 0xCC };
        assert_eq!(Color::from_hex(&c.to_hex()).unwrap(), c);
    }
}
