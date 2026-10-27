//! `rgb-ctl` — headless CLI for the RGB controller.
//!
//! Commands:
//! - `rgb-ctl list`            list detected RGB devices
//! - `rgb-ctl apply`           apply the saved config (used at boot / resume)
//! - `rgb-ctl set <target> <color>` quick color: targets `all`, `keyboard`,
//!   `fans`, `ram`, or a fan zone name (e.g. `jrainbow1`)
//! - `rgb-ctl off`             turn everything off
//! - `rgb-ctl save`            write the default config to disk
//! - `rgb-ctl status`          show current config

use std::process::ExitCode;

use rgb_core::config::{msi_mode, Config, FanZone, KeyboardSetting, RamSetting, ZoneSetting};
use rgb_core::{
    apply_config, color::Color, detect_system_devices, validate_config,
};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        usage();
        return ExitCode::FAILURE;
    }
    match run(&args) {
        Ok(msg) => {
            println!("{msg}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<String, String> {
    match args[0].as_str() {
        "list" => cmd_list(),
        "apply" => cmd_apply(),
        "save" => cmd_save(),
        "status" => cmd_status(),
        "off" => cmd_quick(None),
        "set" => {
            if args.len() != 3 {
                return Err("usage: rgb-ctl set <target> <color>".into());
            }
            let color = Color::from_hex(&args[2]).map_err(|e| e.to_string())?;
            cmd_quick(Some((args[1].as_str(), color)))
        }
        other => Err(format!("unknown command: {other}")),
    }
}

fn usage() {
    eprintln!(
        "rgb-ctl - RGB controller CLI\n\n\
         Usage:\n\
         \x20 rgb-ctl list\n\
         \x20 rgb-ctl apply\n\
         \x20 rgb-ctl set <target> <RRGGBB>\n\
         \x20 rgb-ctl off\n\
         \x20 rgb-ctl save\n\
         \x20 rgb-ctl status\n\n\
         Targets: all | keyboard | fans | ram | jrgb1 | jrgb2 | jrainbow1 |\n\
         \x20        jrainbow2 | onboard"
    );
}

fn cmd_list() -> Result<String, String> {
    let devices = detect_system_devices(true);
    let mut out = String::new();
    out.push_str(&format!("Keyboards ({}):\n", devices.keyboards.len()));
    for k in &devices.keyboards {
        out.push_str(&format!("  {} {:04X}:{:04X} {}\n", k.path.display(), k.vid, k.pid, k.name));
    }
    out.push_str(&format!("MSI Mystic Light boards ({}):\n", devices.msi_boards.len()));
    for b in &devices.msi_boards {
        out.push_str(&format!("  {} {:04X}:{:04X} {}\n", b.path.display(), b.vid, b.pid, b.name));
    }
    out.push_str(&format!("RAM modules ({}):\n", devices.ram_modules.len()));
    for r in &devices.ram_modules {
        out.push_str(&format!(
            "  bus {} addr 0x{:02X} {:?} leds {}\n",
            r.bus, r.address, r.controller, r.led_count
        ));
    }
    out.push_str(&format!("SMBus adapters ({}):\n", devices.smbus_buses.len()));
    for b in &devices.smbus_buses {
        out.push_str(&format!("  /dev/i2c-{} {}\n", b.number, b.name));
    }
    Ok(out.trim_end().to_string())
}

fn cmd_apply() -> Result<String, String> {
    let config = Config::load().map_err(|e| e.to_string())?;
    validate_config(&config).map_err(|e| e.to_string())?;
    let report = apply_config(&config);
    let mut out = String::new();
    for r in &report.results {
        out.push_str(&format!(
            "[{}] {}: {}\n",
            if r.ok { "OK" } else { "FAIL" },
            r.device,
            r.message
        ));
    }
    if report.has_errors() {
        return Err(format!("{}some devices failed", out));
    }
    Ok(out.trim_end().to_string())
}

fn cmd_status() -> Result<String, String> {
    let config = Config::load().map_err(|e| e.to_string())?;
    serde_json::to_string_pretty(&config).map_err(|e| e.to_string())
}

fn cmd_save() -> Result<String, String> {
    let config = Config::load().map_err(|e| e.to_string())?;
    validate_config(&config).map_err(|e| e.to_string())?;
    config.save().map_err(|e| e.to_string())?;
    Ok(format!("saved config to {}", Config::config_path().display()))
}

fn cmd_quick(setting: Option<(&str, Color)>) -> Result<String, String> {
    let mut config = Config::load().map_err(|e| e.to_string())?;

    match setting {
        None => {
            // off: everything off — configured fan zones get the real OFF
            // effect mode, keyboard/RAM get black (their off representation).
            config.global_color = None;
            for z in &mut config.fans {
                z.mode = msi_mode::OFF;
                z.color = Color::OFF;
                z.color2 = Color::OFF;
            }
            config.keyboard = config.keyboard.map(|k| KeyboardSetting {
                color: Color::OFF,
                ..k
            });
            config.ram = config.ram.map(|r| RamSetting { color: Color::OFF, ..r });
        }
        Some(("all", c)) => {
            config.global_color = Some(c);
        }
        Some((target, c)) => {
            config.global_color = None;
            match target {
                "keyboard" => {
                    config.keyboard = Some(KeyboardSetting {
                        mode: 1,
                        color: c,
                        brightness: 100,
                    });
                }
                "ram" => {
                    config.ram = Some(RamSetting { color: c, led_count: 10 });
                }
                "fans" => {
                    config.fans = FanZone::ALL
                        .iter()
                        .map(|z| ZoneSetting {
                            zone: *z,
                            mode: msi_mode::STATIC,
                            color: c,
                            color2: c,
                            brightness: 10,
                            speed: 0,
                        })
                        .collect();
                }
                zone => {
                    let zone = FanZone::from_name(zone)
                        .ok_or_else(|| format!("unknown target: {zone}"))?;
                    if let Some(existing) = config.fans.iter_mut().find(|z| z.zone == zone) {
                        existing.color = c;
                        existing.color2 = c;
                    } else {
                        config.fans.push(ZoneSetting {
                            zone,
                            mode: msi_mode::STATIC,
                            color: c,
                            color2: c,
                            brightness: 10,
                            speed: 0,
                        });
                    }
                }
            }
        }
    }

    validate_config(&config).map_err(|e| e.to_string())?;
    config.save().map_err(|e| e.to_string())?;
    let report = apply_config(&config);
    let mut out = String::new();
    for r in &report.results {
        out.push_str(&format!(
            "[{}] {}: {}\n",
            if r.ok { "OK" } else { "FAIL" },
            r.device,
            r.message
        ));
    }
    if report.has_errors() {
        return Err(format!("{}some devices failed", out));
    }
    Ok(out.trim_end().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_unknown_target_fails() {
        assert!(run(&["set".into(), "bogus".into(), "FF0000".into()]).is_err());
    }

    #[test]
    fn set_bad_color_fails() {
        assert!(run(&["set".into(), "all".into(), "ZZZZZZ".into()]).is_err());
    }

    #[test]
    fn unknown_command_fails() {
        assert!(run(&["frobnicate".into()]).is_err());
    }

    #[test]
    fn set_missing_args_fails() {
        assert!(run(&["set".into(), "all".into()]).is_err());
    }
}
