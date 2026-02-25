// ~/sentinel/sentinel-backend/src/ipc/dispatch/registryd.rs

use serde_json::Value;
use crate::config::refresh_on_request;
use crate::ipc::data_updater::refresh_fast_tier_now;
use crate::ipc::registry::{global_registry, registry_to_output_json};

pub fn dispatch_registry(cmd: &str) -> Result<Value, String> {
    // When refresh_on_request is enabled, refresh fast-tier data inline
    // so clients always get the freshest lightweight readings.
    let is_sysdata_query = matches!(cmd, "list_sysdata" | "list_appdata" | "snapshot");
    if is_sysdata_query && refresh_on_request() {
        refresh_fast_tier_now();
    }

    let reg = global_registry().read().unwrap();
    let output = registry_to_output_json(&reg);

    match cmd {
        "list_addons" => Ok(output.get("addons").cloned().unwrap_or(Value::Null)),
        "list_assets" => Ok(output.get("assets").cloned().unwrap_or(Value::Null)),
        "list_sysdata" => Ok(output.get("sysdata").cloned().unwrap_or(Value::Null)),
        "list_appdata" => Ok(output.get("appdata").cloned().unwrap_or(Value::Null)),
        // Combined snapshot — returns sysdata + appdata in a single response
        // so callers only need one IPC round-trip instead of two.
        "snapshot" => {
            let sysdata = output.get("sysdata").cloned().unwrap_or(Value::Null);
            let appdata = output.get("appdata").cloned().unwrap_or(Value::Null);
            Ok(serde_json::json!({ "sysdata": sysdata, "appdata": appdata }))
        }
        _ => Err(format!("Unknown registry command: {}", cmd)),
    }
}