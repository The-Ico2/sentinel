// ~/sentinel/sentinel-backend/src/ipc/dispatch/sysdatad.rs

use serde_json::Value;
use crate::ipc::sysdata::display::MonitorManager;
use crate::ipc::registry::global_registry;

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
                    "scale": m.scale
                })
            }).collect();

            Ok(Value::Array(displays))
        }
        "get_temp" => Ok(serde_json::to_value(&reg.sysdata).unwrap()),
        "get_cpu" => Ok(serde_json::to_value(&reg.sysdata).unwrap()),
        "get_gpu" => Ok(serde_json::to_value(&reg.sysdata).unwrap()),
        "get_ram" => Ok(serde_json::to_value(&reg.sysdata).unwrap()),
        "get_storage" => Ok(serde_json::to_value(&reg.sysdata).unwrap()),
        "get_network" => Ok(serde_json::to_value(&reg.sysdata).unwrap()),

        _ => Err(format!("Unknown sysdata command: {}", cmd)),
    }
}