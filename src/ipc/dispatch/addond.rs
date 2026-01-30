// ~/sentinel/sentinel-backend/src/ipc/dispatch/addond.rs

use serde_json::Value;
use crate::ipc::addon::{start, stop, reload};

pub fn dispatch_addon(cmd: &str, args: Option<Value>) -> Result<Value, String> {
    match cmd {
        "start" => start(args),
        "stop" => stop(args),
        "reload" => reload(args),
        _ => Err(format!("Unknown addon command: {}", cmd)),
    }
}
