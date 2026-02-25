// ~/sentinel/sentinel-backend/src/ipc/data_updater.rs

use std::{thread, time::Duration};
use crate::{
    ipc::registry::{
        global_registry, pull_sysdata_fast, pull_sysdata_slow,
        merge_sysdata_tier, write_registry_json, FAST_CATEGORIES,
    },
    paths::sentinel_root_dir,
    config::{fast_pull_rate_ms, slow_pull_rate_ms, pull_paused},
};
use crate::ipc::{
    appdata::window::ActiveWindowManager,
};

/// Start two registry updater threads — fast tier and slow tier.
pub fn start_registry_updater() {
    // ── Fast-tier thread (time, audio, keyboard, mouse, idle, power, display + appdata) ──
    thread::spawn(move || loop {
        if pull_paused() {
            thread::sleep(Duration::from_millis(50));
            continue;
        }

        let rate = fast_pull_rate_ms();

        let fast_data = pull_sysdata_fast();
        let appdata = ActiveWindowManager::enumerate_active_windows();
        let mut changed = false;

        {
            let mut reg = global_registry().write().unwrap();

            let merged = merge_sysdata_tier(&reg.sysdata, fast_data, FAST_CATEGORIES);
            if reg.sysdata != merged {
                reg.sysdata = merged;
                changed = true;
            }

            if reg.appdata != appdata {
                reg.appdata = appdata;
                changed = true;
            }
        }

        if changed {
            let root = sentinel_root_dir();
            let snapshot = global_registry().read().unwrap().clone();
            write_registry_json(&snapshot, &root);
        }

        if rate > 0 {
            thread::sleep(Duration::from_millis(rate));
        }
    });

    // ── Slow-tier thread (cpu, gpu, ram, storage, network, bluetooth, wifi, system, processes) ──
    thread::spawn(move || {
        // Small offset so both threads don't write at exact same instant
        thread::sleep(Duration::from_millis(25));

        loop {
            if pull_paused() {
                thread::sleep(Duration::from_millis(100));
                continue;
            }

            let rate = slow_pull_rate_ms();
            let slow_categories: Vec<&str> = vec![
                "cpu", "gpu", "ram", "storage", "network",
                "bluetooth", "wifi", "system", "processes",
            ];

            let slow_data = pull_sysdata_slow();
            let mut changed = false;

            {
                let mut reg = global_registry().write().unwrap();
                let merged = merge_sysdata_tier(&reg.sysdata, slow_data, &slow_categories);
                if reg.sysdata != merged {
                    reg.sysdata = merged;
                    changed = true;
                }
            }

            if changed {
                let root = sentinel_root_dir();
                let snapshot = global_registry().read().unwrap().clone();
                write_registry_json(&snapshot, &root);
            }

            if rate > 0 {
                thread::sleep(Duration::from_millis(rate));
            }
        }
    });
}

/// Refresh fast-tier sysdata inline (called from IPC dispatch when `refresh_on_request` is enabled).
/// Returns `true` if the registry was updated.
pub fn refresh_fast_tier_now() -> bool {
    let fast_data = pull_sysdata_fast();
    let mut changed = false;

    {
        let mut reg = global_registry().write().unwrap();
        let merged = merge_sysdata_tier(&reg.sysdata, fast_data, FAST_CATEGORIES);
        if reg.sysdata != merged {
            reg.sysdata = merged;
            changed = true;
        }
    }

    if changed {
        let root = sentinel_root_dir();
        let snapshot = global_registry().read().unwrap().clone();
        write_registry_json(&snapshot, &root);
    }

    changed
}
