// ~/sentinel/sentinel-backend/src/ipc/dispatch/registryd.rs

use serde_json::Value;
use crate::ipc::registry::{global_registry, registry_to_output_json};

pub fn dispatch_registry(cmd: &str) -> Result<Value, String> {
    let reg = global_registry().read().unwrap();
    let output = registry_to_output_json(&reg);

    match cmd {
        "list_addons" => Ok(output.get("addons").cloned().unwrap_or(Value::Null)),
        "list_assets" => Ok(output.get("assets").cloned().unwrap_or(Value::Null)),
        "list_sysdata" => Ok(output.get("sysdata").cloned().unwrap_or(Value::Null)),
        "list_appdata" => Ok(output.get("appdata").cloned().unwrap_or(Value::Null)),
        _ => Err(format!("Unknown registry command: {}", cmd)),
    }
}