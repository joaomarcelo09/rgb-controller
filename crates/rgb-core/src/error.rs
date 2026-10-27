use thiserror::Error;

/// Errors produced by the RGB core library.
#[derive(Debug, Error)]
pub enum RgbError {
    #[error("HID error: {0}")]
    Hid(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("SMBus error: {0}")]
    Smbus(String),

    #[error("device not found: {0}")]
    DeviceNotFound(String),

    #[error("config error: {0}")]
    Config(String),

    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    #[error("device busy: {0}")]
    Busy(String),

    #[error("permission denied: {0}")]
    PermissionDenied(String),

    #[error("unsupported: {0}")]
    Unsupported(String),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

impl From<hidapi::HidError> for RgbError {
    fn from(e: hidapi::HidError) -> Self {
        let s = e.to_string();
        let lower = s.to_lowercase();
        if lower.contains("permission") || lower.contains("denied") || lower.contains("access") {
            RgbError::PermissionDenied(s)
        } else if lower.contains("busy") {
            RgbError::Busy(s)
        } else {
            RgbError::Hid(s)
        }
    }
}

impl From<std::ffi::NulError> for RgbError {
    fn from(e: std::ffi::NulError) -> Self {
        RgbError::InvalidArgument(format!("path contains NUL byte: {e}"))
    }
}

pub type Result<T> = std::result::Result<T, RgbError>;
