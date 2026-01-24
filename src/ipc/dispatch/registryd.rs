// ~/sentinel/sentinel-backend/src/ipc/dispatch/registryd.rs

use serde_json::Value;
use crate::ipc::registry::global_registry;

pub fn dispatch_registry(cmd: &str) -> Result<Value, String> {
    let reg = global_registry().read().unwrap();
    match cmd {
        "list_addons" => Ok(serde_json::to_value(&reg.addons).unwrap()),
        "list_assets" => Ok(serde_json::to_value(&reg.assets).unwrap()),
        "list_sysdata" => Ok(serde_json::to_value(&reg.sysdata).unwrap()),
        "list_appdata" => Ok(serde_json::to_value(&reg.appdata).unwrap()),
        _ => Err(format!("Unknown registry command: {}", cmd)),
    }
}