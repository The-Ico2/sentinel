use serde_json::{Value, json};
use std::process::{Command, Stdio};
use sysinfo::{System, ProcessesToUpdate};
use crate::{info, error};
use crate::ipc::registry::global_registry;
use super::utils::registry_entry_to_addon;

/// Check if an addon is already running by matching exe path or process name.
fn is_addon_running(addon: &crate::Addon) -> bool {
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::All, true);
    for (_pid, proc_) in sys.processes() {
        if let Some(exe) = proc_.exe() {
            if exe == addon.exe_path.as_path() {
                return true;
            }
        }
        if proc_.name().eq_ignore_ascii_case(&format!("{}.exe", addon.package)) {
            return true;
        }
    }
    false
}

pub fn start(args: Option<Value>) -> Result<Value, String> {
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

    // Check if addon is already running
    if is_addon_running(&addon) {
        info!("[IPC] Addon '{}' is already running, skipping start", addon.name);
        return Ok(json!({"status": "already_running", "addon": addon_name}));
    }

    info!("Starting addon '{}'", addon.name);

    // Ensure binary exists
    if !addon.exe_path.exists() {
        error!("Addon executable not found: {}", addon.exe_path.display());
        return Err(format!("Addon executable not found: {}", addon.exe_path.display()));
    }

    match Command::new(&addon.exe_path)
        .current_dir(&addon.dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => {
            info!("[IPC] Started addon '{}' with PID {}", addon.name, child.id());
            Ok(json!({"status": "started", "addon": addon_name}))
        }
        Err(e) => {
            error!("[IPC] Failed to start addon '{}': {}", addon.name, e);
            Err(format!("Failed to start addon: {}", e))
        }
    }
}

