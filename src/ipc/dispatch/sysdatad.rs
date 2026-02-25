// ~/sentinel/sentinel-backend/src/ipc/dispatch/sysdatad.rs

use serde_json::Value;
use crate::ipc::sysdata::display::MonitorManager;
use crate::ipc::registry::global_registry;
use crate::ipc::sysdata::time as time_module;

fn metadata_for_category(reg: &crate::ipc::registry::Registry, category: &str) -> Value {
    reg.sysdata
        .iter()
        .find(|entry| entry.category.eq_ignore_ascii_case(category))
        .map(|entry| entry.metadata.clone())
        .unwrap_or(Value::Null)
}

pub fn dispatch_sysdata(cmd: &str) -> Result<Value, String> {
    let reg = global_registry().read().unwrap();

    match cmd {
        "get_displays" => {
            let monitors = MonitorManager::enumerate_monitors();
            let displays: Vec<Value> = monitors.into_iter().map(|m| {
                serde_json::json!({
                    "id": m.id,
                    "primary": m.primary,
                    "x": m.x,
                    "y": m.y,
                    "width": m.width,
                    "height": m.height,
                    "scale": m.scale,
                    "dpi": m.dpi,
                    "refresh_rate_hz": m.refresh_rate_hz,
                    "color_depth_bits": m.color_depth_bits,
                    "bits_per_channel": m.bits_per_channel,
                    "orientation": m.orientation,
                    "aspect_ratio": m.aspect_ratio,
                    "device_name": m.device_name,
                    "monitor_name": m.monitor_name,
                    "connection_type": m.connection_type,
                    "hdr_supported": m.hdr_supported,
                    "physical_width_mm": m.physical_width_mm,
                    "physical_height_mm": m.physical_height_mm,
                    "manufacturer": m.manufacturer,
                    "product_code": m.product_code,
                    "serial_number": m.serial_number,
                    "year_of_manufacture": m.year_of_manufacture,
                })
            }).collect();

            Ok(Value::Array(displays))
        }
        "get_temp" => Ok(serde_json::json!({
            "cpu": metadata_for_category(&reg, "cpu")
                .get("temperature")
                .cloned()
                .unwrap_or(Value::Null),
            "gpu": metadata_for_category(&reg, "gpu")
                .get("temperature")
                .cloned()
                .unwrap_or(Value::Null),
        })),
        "get_cpu" => Ok(metadata_for_category(&reg, "cpu")),
        "get_gpu" => Ok(metadata_for_category(&reg, "gpu")),
        "get_ram" => Ok(metadata_for_category(&reg, "ram")),
        "get_storage" => Ok(metadata_for_category(&reg, "storage")),
        "get_network" => Ok(metadata_for_category(&reg, "network")),
        "get_audio" => Ok(metadata_for_category(&reg, "audio")),
        "get_time"=> Ok(time_module::get_time_json()),
        "get_keyboard" => Ok(metadata_for_category(&reg, "keyboard")),
        "get_mouse" => Ok(metadata_for_category(&reg, "mouse")),
        "get_power" => Ok(metadata_for_category(&reg, "power")),
        "get_bluetooth" => Ok(metadata_for_category(&reg, "bluetooth")),
        "get_wifi" => Ok(metadata_for_category(&reg, "wifi")),
        "get_system" => Ok(metadata_for_category(&reg, "system")),
        "get_processes" => Ok(metadata_for_category(&reg, "processes")),
        "get_idle" => Ok(metadata_for_category(&reg, "idle")),
        "get_notifications" => {
            Ok(crate::ipc::appdata::notifications::get_notifications_json())
        }
        "get_tray_icons" => {
            Ok(crate::ipc::appdata::trayicons::get_tray_icons_json())
        }
        _ => Err(format!("Unknown sysdata command: {}", cmd)),
    }
}