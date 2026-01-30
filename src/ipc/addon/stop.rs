

use serde_json::{Value, json};
use std::path::Path;
use sysinfo::System;
use crate::{info, error, warn};
use crate::ipc::registry::global_registry;
use super::utils::registry_entry_to_addon;

pub fn stop(args: Option<Value>) -> Result<Value, String> {
    let addon_name = args
        .as_ref()
        .and_then(|v| v.get("addon_name"))
        .and_then(|v| v.as_str())
        .ok_or("Missing addon_name in args")?
        .to_string();

    let reg = global_registry().read().unwrap();
    let entry = reg.addons.iter().find(|a| {
        a.id == addon_name ||
        a.metadata.get("name")
            .and_then(|n| n.as_str())
            .map(|n| n.eq_ignore_ascii_case(&addon_name))
            .unwrap_or(false)
    })
        .ok_or(format!("Addon not found: {}", addon_name))?
        .clone();
    drop(reg);

    let addon = registry_entry_to_addon(&entry)?;

    info!("Stopping addon '{}'", addon.name);

    // Try OS-level process kill by exe path/name
    let sys = System::new_all();
    let mut stopped = false;

    for (_pid, proc_) in sys.processes() {
        let mut matches = false;

        if proc_.exe() == Some(Path::new(&addon.exe_path)) {
            matches = true;
            info!("Process matched by exe path: {}", addon.exe_path.display());
        }

        if !matches && proc_.name().eq_ignore_ascii_case(&format!("{}.exe", addon.package)) {
            matches = true;
            info!("Process matched by name: {}.exe", addon.package);
        }

        if matches {
            match proc_.kill() {
                true => info!("Successfully killed OS process for '{}'", addon.name),
                false => warn!("Failed to kill OS process for '{}'", addon.name),
            }
            stopped = true;
        }
    }

    if stopped {
        info!("[IPC] Stopped addon '{}'", addon_name);
        Ok(json!({"status": "stopped", "addon": addon_name}))
    } else {
        error!("[IPC] Failed to stop addon '{}'", addon_name);
        Err(format!("Failed to stop addon: {}", addon_name))
    }
}