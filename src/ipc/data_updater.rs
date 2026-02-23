// ~/sentinel/sentinel-backend/src/ipc/data_updater.rs

use std::{thread, time::Duration};
use crate::{
    ipc::registry::{global_registry, pull_sysdata, write_registry_json},
    paths::sentinel_root_dir,
    config::{pull_rate_ms, pull_paused},
};
use crate::ipc::{
    appdata::window::ActiveWindowManager,
};

/// Start the registry updater thread.
/// Rate and pause state are read from the global config atomics each iteration.
pub fn start_registry_updater() {
    thread::spawn(move || loop {
        // Check pause state
        if pull_paused() {
            thread::sleep(Duration::from_millis(50));
            continue;
        }

        let rate = pull_rate_ms();

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

        if rate > 0 {
            thread::sleep(Duration::from_millis(rate));
        }
    });
}
