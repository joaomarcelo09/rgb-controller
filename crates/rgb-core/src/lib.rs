pub mod apply;
pub mod color;
pub mod config;
pub mod detection;
pub mod error;
pub mod logitech;
pub mod msi;
pub mod smbus;

pub use apply::{
    apply_config, apply_global_color, detect_system_devices, validate_config, ApplyReport,
    DeviceApplyResult, SystemDevices,
};
pub use color::Color;
pub use config::{Config, FanZone, KeyboardSetting, RamSetting, ZoneSetting};
pub use detection::{
    list_hidraw_devices, list_i2c_buses, HidDeviceInfo, I2cBusInfo, VID_LOGITECH, VID_MSI,
};
pub use error::RgbError;
