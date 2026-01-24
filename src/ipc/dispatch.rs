use crate::custom::windows::WindowsCManager;
use serde_json::Value;
use crate::{info, warn};

mod registryd;
mod sysdatad;

pub fn dispatch(
    _windows: &WindowsCManager, // currently unused, but kept for future commands
    ns: &str,
    cmd: &str,
    _args: Option<Value>,
) -> Result<Value, String> {
    info!("[IPC] Dispatch request -> namespace: '{}', command: '{}'", ns, cmd);

    match ns {
        "registry" => registryd::dispatch_registry(cmd),
        "sysdata" => sysdatad::dispatch_sysdata(cmd),
        _ => {
            warn!("[IPC] Unknown namespace requested: '{}'", ns);
            Err(format!("Unknown namespace: {}", ns))
        }
    }
}
