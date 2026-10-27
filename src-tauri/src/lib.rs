//! Tauri backend: wraps `rgb-core` behind Tauri commands.

use rgb_core::color::Color;
use rgb_core::config::{Config, KeyboardSetting, RamSetting};
use rgb_core::{
    apply_config, apply_global_color, detect_system_devices, validate_config, ApplyReport,
    SystemDevices,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DeviceCounts {
    pub keyboards: usize,
    pub ram_modules: usize,
}

#[tauri::command]
fn list_devices() -> Result<DeviceCounts, String> {
    let devices = detect_system_devices(false);
    Ok(DeviceCounts {
        keyboards: devices.keyboards.len(),
        ram_modules: devices.ram_modules.len(),
    })
}

#[tauri::command]
fn scan_devices() -> Result<SystemDevices, String> {
    Ok(detect_system_devices(true))
}

#[tauri::command]
fn load_config() -> Result<Config, String> {
    Config::load().map_err(|e| e.to_string())
}

#[tauri::command]
fn save_config(config: Config) -> Result<(), String> {
    validate_config(&config).map_err(|e| e.to_string())?;
    config.save().map_err(|e| e.to_string())
}

#[tauri::command]
fn apply(config: Config) -> Result<ApplyReport, String> {
    validate_config(&config).map_err(|e| e.to_string())?;
    Ok(apply_config(&config))
}

#[tauri::command]
fn apply_all_color(hex: String) -> Result<ApplyReport, String> {
    let color = Color::from_hex(&hex).map_err(|e| e.to_string())?;
    Ok(apply_global_color(color))
}

#[tauri::command]
fn set_keyboard_color(hex: String) -> Result<ApplyReport, String> {
    let color = Color::from_hex(&hex).map_err(|e| e.to_string())?;
    let config = Config {
        keyboard: Some(KeyboardSetting { mode: 1, color, brightness: 100 }),
        fans: vec![],
        ram: None,
        ..Config::default()
    };
    Ok(apply_config(&config))
}

#[tauri::command]
fn set_ram_color(hex: String) -> Result<ApplyReport, String> {
    let color = Color::from_hex(&hex).map_err(|e| e.to_string())?;
    let config = Config {
        ram: Some(RamSetting { color, led_count: 10 }),
        keyboard: None,
        fans: vec![],
        ..Config::default()
    };
    Ok(apply_config(&config))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            list_devices,
            scan_devices,
            load_config,
            save_config,
            apply,
            apply_all_color,
            set_keyboard_color,
            set_ram_color
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
