// ~/veil/veil-backend/src/ipc/dispatch/trackingd.rs

use serde_json::Value;
use crate::ipc::data_updater::set_explicit_tracking_demands;

fn sections_from_args(args: Option<&Value>) -> Vec<String> {
    args
        .and_then(|a| a.get("sections"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

pub fn dispatch_tracking(cmd: &str, args: Option<Value>) -> Result<Value, String> {
    match cmd {
        "set_demands" => {
            let sections = sections_from_args(args.as_ref());
            set_explicit_tracking_demands(&sections);
            Ok(serde_json::json!({ "ok": true }))
        }
        _ => Err(format!("Unknown tracking command: {}", cmd)),
    }
}
