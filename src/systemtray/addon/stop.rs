// ~/sentinel/sentinel-backend/src/systemtray/addon/stop.rs

use std::collections::HashMap;
use std::process::Child;
use std::path::Path;

use sysinfo::{System};

use crate::Addon;
use crate::{info, warn, error};

pub fn stop_addon(addon: &Addon, children: &mut HashMap<String, Child>) -> bool {
    info!("Stopping addon '{}'", addon.name);

    // First try OS-level process kill by exe path/name
    let sys = System::new_all(); // load all processes
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

    // Also stop tracked child if present
    if let Some(mut child) = children.remove(&addon.name) {
        match child.kill() {
            Ok(_) => info!("Stopped tracked child process for '{}'", addon.name),
            Err(e) => error!("Failed to stop tracked child for '{}': {}", addon.name, e),
        }
        stopped = true;
    }

    if stopped {
        info!("Addon '{}' stopped successfully", addon.name);
    } else {
        warn!("No running process found for addon '{}'", addon.name);
    }

    stopped
}