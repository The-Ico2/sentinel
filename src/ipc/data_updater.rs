// ~/sentinel/sentinel-backend/src/ipc/data_updater.rs

use std::{thread, time::Duration};
use crate::ipc::registry::{global_registry, write_registry_json};
use crate::paths::sentinel_root_dir;
use crate::ipc::{
    sysdata::display::MonitorManager,
    appdata::active_window::ActiveWindowManager,
};

/// Interval in milliseconds
const DEFAULT_INTERVAL_MS: u64 = 250;

pub fn start_registry_updater(interval_ms: Option<u64>) {
    let interval = Duration::from_millis(interval_ms.unwrap_or(DEFAULT_INTERVAL_MS));

    thread::spawn(move || loop {
        {
            let mut reg = global_registry().write().unwrap();

            // ----- SYSDATA -----
            // Pull Monitors
            reg.sysdata = MonitorManager::enumerate_monitors()
                .into_iter()
                .map(|m| crate::ipc::registry::RegistryEntry {
                    id: format!("display_{}", m.id),
                    category: "display".into(),
                    subtype: "monitor".into(),
                    metadata: serde_json::json!(m),
                    path: std::path::PathBuf::new(),
                    exe_path: "".into(),
                })
                .collect();

            // ----- APPDATA -----
            // Pull Active Window
            reg.appdata = ActiveWindowManager::enumerate_active_windows();
            
            // Write updated registry
            write_registry_json(&reg, &sentinel_root_dir());
        }

        thread::sleep(interval);
    });
}
