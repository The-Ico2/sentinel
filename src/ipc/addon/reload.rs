use serde_json::{Value, json};
use crate::{info, error};
use crate::ipc::registry::global_registry;
use super::stop::stop;
use super::start::start;

pub fn reload(args: Option<Value>) -> Result<Value, String> {
    let addon_name = args
        .as_ref()
        .and_then(|v| v.get("addon_name"))
        .and_then(|v| v.as_str())
        .ok_or("Missing addon_name in args")?
        .to_string();

    // Verify addon exists first
    let reg = global_registry().read().unwrap();
    let _entry = reg.addons.iter().find(|a| {
        a.id == addon_name ||
        a.metadata.get("name")
            .and_then(|n| n.as_str())
            .map(|n| n.eq_ignore_ascii_case(&addon_name))
            .unwrap_or(false)
    })
        .ok_or(format!("Addon not found: {}", addon_name))?
        .clone();
    drop(reg);

    // Stop the addon
    let _ = stop(args.clone());

    // Start it again
    match start(args) {
        Ok(_) => {
            info!("[IPC] Reloaded addon '{}'", addon_name);
            Ok(json!({"status": "reloaded", "addon": addon_name}))
        }
        Err(e) => {
            error!("[IPC] Failed to reload addon '{}': {}", addon_name, e);
            Err(e)
        }
    }
}