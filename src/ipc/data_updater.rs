// ~/sentinel/sentinel-backend/src/ipc/data_updater.rs

use std::{thread, time::Duration};
use crate::{
    ipc::registry::{global_registry, pull_sysdata, write_registry_json},
    paths::sentinel_root_dir,
};
use crate::ipc::{
    appdata::window::ActiveWindowManager,
};

/// Interval in milliseconds
const DEFAULT_INTERVAL_MS: u64 = 100;

pub fn start_registry_updater(interval_ms: Option<u64>) {
    let interval = Duration::from_millis(interval_ms.unwrap_or(DEFAULT_INTERVAL_MS));

    thread::spawn(move || loop {
        // Collect outside lock (can be slow)
        let sysdata = pull_sysdata();
        let appdata = ActiveWindowManager::enumerate_active_windows();
        let mut sysdata_changed = false;
        let mut appdata_changed = false;

        {
            let mut reg = global_registry().write().unwrap();

            // ----- SYSDATA -----
            if reg.sysdata != sysdata {
                reg.sysdata = sysdata;
                sysdata_changed = true;
            }

            // ----- APPDATA -----
            // Pull all windows per monitor
            if reg.appdata != appdata {
                reg.appdata = appdata;
                appdata_changed = true;
            }
        }

        if appdata_changed || sysdata_changed {
            let root = sentinel_root_dir();
            let snapshot = global_registry().read().unwrap().clone();
            write_registry_json(&snapshot, &root);
        }

        thread::sleep(interval);
    });
}
