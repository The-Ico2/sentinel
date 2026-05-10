// ~/veil/veil-backend/src/ipc/dispatch/registryd.rs

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

fn normalize_section(section: &str) -> Option<&'static str> {
    match section.to_ascii_lowercase().as_str() {
        "display" | "displays" => Some("displays"),
        "time" => Some("time"),
        "cpu" => Some("cpu"),
        "gpu" => Some("gpu"),
        "ram" => Some("ram"),
        "storage" => Some("storage"),
        "network" => Some("network"),
        "wifi" => Some("wifi"),
        "bluetooth" => Some("bluetooth"),
        "audio" => Some("audio"),
        "media" => Some("media"),
        "keyboard" => Some("keyboard"),
        "mouse" => Some("mouse"),
        "power" => Some("power"),
        "idle" => Some("idle"),
        "system" => Some("system"),
        "processes" => Some("processes"),
        "appdata" => Some("appdata"),
        _ => None,
    }
}

fn filter_snapshot_by_sections(output: &Value, sections: &[String]) -> Value {
    if sections.is_empty() {
        return serde_json::json!({
            "sysdata": output.get("sysdata").cloned().unwrap_or(Value::Null),
            "appdata": output.get("appdata").cloned().unwrap_or(Value::Null),
        });
    }

    let mut sys_obj = serde_json::Map::new();
    let sysdata_obj = output.get("sysdata").and_then(|v| v.as_object());

    let mut include_appdata = false;
    for requested in sections {
        let Some(normalized) = normalize_section(requested) else {
            continue;
        };

        if normalized == "appdata" {
            include_appdata = true;
            continue;
        }

        if let Some(src) = sysdata_obj.and_then(|obj| obj.get(normalized)) {
            sys_obj.insert(normalized.to_string(), src.clone());
        }
    }

    serde_json::json!({
        "sysdata": Value::Object(sys_obj),
        "appdata": if include_appdata {
            output.get("appdata").cloned().unwrap_or(Value::Null)
        } else {
            Value::Null
        },
    })
}

pub fn dispatch_registry(cmd: &str, args: Option<Value>) -> Result<Value, String> {
    let sections_arg = sections_from_args(args.as_ref());
    let sections = sections_arg.clone().unwrap_or_default();

    if cmd == "snapshot" || cmd == "get_data" {
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
        "snapshot" | "get_data" => Ok(filter_snapshot_by_sections(&output, &sections)),
        // Full registry output including addons, assets, __meta — used by
        // the VEIL UI Data page so it can display everything.
        "full" => Ok(output),
        _ => Err(format!("Unknown registry command: {}", cmd)),
    }
}