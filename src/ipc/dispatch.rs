use crate::custom::windows::WindowsCManager;
use serde_json::Value;
use crate::warn;

mod registryd;
mod sysdatad;
mod addond;

pub fn dispatch(
    _windows: &WindowsCManager, // currently unused, but kept for future commands
    ns: &str,
    cmd: &str,
    args: Option<Value>,
) -> Result<Value, String> {
    match ns {
        "registry" => registryd::dispatch_registry(cmd),
        "sysdata" => sysdatad::dispatch_sysdata(cmd),
        "addon" => addond::dispatch_addon(cmd, args),
        _ => {
            warn!("[IPC] Unknown namespace requested: '{}'", ns);
            Err(format!("Unknown namespace: {}", ns))
        }
    }
}
