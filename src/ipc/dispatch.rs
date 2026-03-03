use serde_json::Value;
use crate::warn;

mod registryd;
mod sysdatad;
mod addond;
mod backendd;
mod trackingd;

pub fn dispatch(
    ns: &str,
    cmd: &str,
    args: Option<Value>,
) -> Result<Value, String> {
    match ns {
        "registry" => registryd::dispatch_registry(cmd, args),
        "sysdata" => sysdatad::dispatch_sysdata(cmd),
        "addon" => addond::dispatch_addon(cmd, args),
        "backend" => backendd::dispatch_backend(cmd, args),
        "tracking" => trackingd::dispatch_tracking(cmd, args),
        _ => {
            warn!("[IPC] Unknown namespace requested: '{}'", ns);
            Err(format!("Unknown namespace: {}", ns))
        }
    }
}
