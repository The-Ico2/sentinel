// ~/sentinel/sentinel-backend/src/ipc/dispatch/registryd.rs

use serde_json::Value;
use crate::ipc::data_updater::set_explicit_tracking_demands;
use crate::ipc::registry::{global_registry, registry_to_output_json};

fn sections_from_args(args: Option<&Value>) -> Option<Vec<String>> {
    args
        .and_then(|a| a.get("sections"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect::<Vec<_>>()
        })
}

pub fn dispatch_registry(cmd: &str, args: Option<Value>) -> Result<Value, String> {
    let sections_arg = sections_from_args(args.as_ref());
    let _sections = sections_arg.clone().unwrap_or_default();

    if cmd == "snapshot" {
        if let Some(explicit_sections) = sections_arg {
            set_explicit_tracking_demands(&explicit_sections);
        }
    }

    // No inline refresh — updater threads maintain the registry in real time.
    // Reading directly from the in-memory registry avoids lock contention and
    // keeps IPC latency minimal.

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
        // Full registry output including addons, assets, __meta — used by
        // the Sentinel UI Data page so it can display everything.
        "full" => Ok(output),
        _ => Err(format!("Unknown registry command: {}", cmd)),
    }
}