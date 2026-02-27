// ~/sentinel/sentinel-backend/src/ipc/data_updater.rs
//
// Event-driven registry updater.  Each tier sleeps on a Condvar so it
// wakes *instantly* when tracking demands change, instead of waiting
// for a timer to expire.  The configured fast/slow rates serve as
// *maximum* intervals between collections — not polling sleeps.

use std::{
    collections::HashSet,
    sync::{
        atomic::{AtomicU64, Ordering},
        Condvar, Mutex, OnceLock, RwLock,
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use crate::{
    ipc::registry::{
        global_registry, pull_sysdata_cpu,
        merge_sysdata_tier, RegistryEntry,
    },
    config::{fast_pull_rate_ms, slow_pull_rate_ms, pull_paused, ui_data_exception_enabled},
};
use crate::ipc::{
    appdata::window::ActiveWindowManager,
};
use serde_json::json;

const UI_HEARTBEAT_TTL_MS: u64 = 2500;
const IDLE_SLEEP_MS: u64 = 250;

static LAST_UI_HEARTBEAT_MS: AtomicU64 = AtomicU64::new(0);
static EXPLICIT_TRACKED_SECTIONS: OnceLock<RwLock<HashSet<String>>> = OnceLock::new();

/// Global wake signal.  Any code that changes tracking demands or config
/// should call `wake_updaters()` so sleeping threads re-evaluate immediately.
static WAKE_SIGNAL: OnceLock<(Mutex<bool>, Condvar)> = OnceLock::new();

fn wake_pair() -> &'static (Mutex<bool>, Condvar) {
    WAKE_SIGNAL.get_or_init(|| (Mutex::new(false), Condvar::new()))
}

/// Wake all updater threads immediately (e.g. after demand changes).
pub fn wake_updaters() {
    let (lock, cvar) = wake_pair();
    if let Ok(mut signaled) = lock.lock() {
        *signaled = true;
        cvar.notify_all();
    }
}

/// Sleep for at most `dur`, but return early if `wake_updaters()` is called.
fn interruptible_sleep(dur: Duration) {
    let (lock, cvar) = wake_pair();
    if let Ok(mut signaled) = lock.lock() {
        // Clear any stale signal, then wait with timeout
        *signaled = false;
        let _ = cvar.wait_timeout(signaled, dur);
    }
}

const TRACKABLE_SECTIONS: &[&str] = &[
    "time", "cpu", "gpu", "ram", "storage", "displays", "network", "wifi",
    "bluetooth", "audio", "keyboard", "mouse", "power", "idle", "system",
    "processes", "appdata",
];

fn tracked_sections() -> &'static RwLock<HashSet<String>> {
    EXPLICIT_TRACKED_SECTIONS.get_or_init(|| RwLock::new(HashSet::new()))
}

fn normalize_section(section: &str) -> Option<&'static str> {
    match section.to_ascii_lowercase().as_str() {
        "display" | "displays" => Some("displays"),
        "time" => Some("time"),
        "cpu" => Some("cpu"),
        "gpu" => Some("gpu"),
        "ram" => Some("ram"),
        "storage" => Some("storage"),
        "network" => Some("network"),
        "wifi" => Some("wifi"),
        "bluetooth" => Some("bluetooth"),
        "audio" => Some("audio"),
        "keyboard" => Some("keyboard"),
        "mouse" => Some("mouse"),
        "power" => Some("power"),
        "idle" => Some("idle"),
        "system" => Some("system"),
        "processes" => Some("processes"),
        "appdata" => Some("appdata"),
        _ => None,
    }
}

fn section_to_internal_category(section: &str) -> Option<&'static str> {
    match normalize_section(section)? {
        "displays" => Some("display"),
        other => Some(other),
    }
}

pub fn set_explicit_tracking_demands(sections: &[String]) {
    let mut next = HashSet::<String>::new();
    for section in sections {
        if let Some(normalized) = normalize_section(section) {
            next.insert(normalized.to_string());
        }
    }

    let changed = {
        let current = tracked_sections().read().unwrap();
        *current != next
    };

    if changed {
        let mut current = tracked_sections().write().unwrap();
        *current = next;
        drop(current);
        // Immediately wake all updater threads so they begin
        // collecting newly-demanded sections without waiting.
        wake_updaters();
    }
}

pub fn section_tracking_enabled(section: &str) -> bool {
    let Some(normalized) = normalize_section(section) else {
        return false;
    };

    if tracked_sections().read().unwrap().contains(normalized) {
        return true;
    }

    if !ui_data_exception_enabled() {
        return false;
    }

    let now = now_ms();
    let last_ui = LAST_UI_HEARTBEAT_MS.load(Ordering::Relaxed);
    now.saturating_sub(last_ui) <= UI_HEARTBEAT_TTL_MS
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn touch_ui_heartbeat() {
    LAST_UI_HEARTBEAT_MS.store(now_ms(), Ordering::Relaxed);
    wake_updaters();
}

pub fn demand_tracking_active() -> bool {
    !pull_paused() && TRACKABLE_SECTIONS.iter().any(|section| section_tracking_enabled(section))
}

fn single_sys_entry(category: &str) -> Option<RegistryEntry> {
    match category {
        "cpu" => Some(pull_sysdata_cpu()),
        "gpu" => Some(RegistryEntry { id: "gpu".into(), category: "gpu".into(), subtype: "system".into(), metadata: crate::ipc::sysdata::gpu::get_gpu_json(), path: std::path::PathBuf::new(), exe_path: "".into() }),
        "ram" => Some(RegistryEntry { id: "ram".into(), category: "ram".into(), subtype: "system".into(), metadata: crate::ipc::sysdata::ram::get_ram_json(), path: std::path::PathBuf::new(), exe_path: "".into() }),
        "storage" => Some(RegistryEntry { id: "storage".into(), category: "storage".into(), subtype: "system".into(), metadata: crate::ipc::sysdata::storage::get_storage_json(), path: std::path::PathBuf::new(), exe_path: "".into() }),
        "network" => Some(RegistryEntry { id: "network".into(), category: "network".into(), subtype: "system".into(), metadata: crate::ipc::sysdata::network::get_network_json(), path: std::path::PathBuf::new(), exe_path: "".into() }),
        "bluetooth" => Some(RegistryEntry { id: "bluetooth".into(), category: "bluetooth".into(), subtype: "system".into(), metadata: crate::ipc::sysdata::bluetooth::get_bluetooth_json(), path: std::path::PathBuf::new(), exe_path: "".into() }),
        "wifi" => Some(RegistryEntry { id: "wifi".into(), category: "wifi".into(), subtype: "system".into(), metadata: crate::ipc::sysdata::wifi::get_wifi_json(), path: std::path::PathBuf::new(), exe_path: "".into() }),
        "system" => Some(RegistryEntry { id: "system".into(), category: "system".into(), subtype: "system".into(), metadata: crate::ipc::sysdata::system::get_system_json(), path: std::path::PathBuf::new(), exe_path: "".into() }),
        "processes" => Some(RegistryEntry { id: "processes".into(), category: "processes".into(), subtype: "system".into(), metadata: crate::ipc::sysdata::processes::get_processes_json(), path: std::path::PathBuf::new(), exe_path: "".into() }),
        "audio" => Some(RegistryEntry { id: "audio".into(), category: "audio".into(), subtype: "system".into(), metadata: crate::ipc::sysdata::audio::get_audio_json(), path: std::path::PathBuf::new(), exe_path: "".into() }),
        "time" => Some(RegistryEntry { id: "time".into(), category: "time".into(), subtype: "system".into(), metadata: crate::ipc::sysdata::time::get_time_json(), path: std::path::PathBuf::new(), exe_path: "".into() }),
        "keyboard" => Some(RegistryEntry { id: "keyboard".into(), category: "keyboard".into(), subtype: "system".into(), metadata: crate::ipc::sysdata::keyboard::get_keyboard_json(), path: std::path::PathBuf::new(), exe_path: "".into() }),
        "mouse" => Some(RegistryEntry { id: "mouse".into(), category: "mouse".into(), subtype: "system".into(), metadata: crate::ipc::sysdata::mouse::get_mouse_json(), path: std::path::PathBuf::new(), exe_path: "".into() }),
        "power" => Some(RegistryEntry { id: "power".into(), category: "power".into(), subtype: "system".into(), metadata: crate::ipc::sysdata::power::get_power_json(), path: std::path::PathBuf::new(), exe_path: "".into() }),
        "idle" => Some(RegistryEntry { id: "idle".into(), category: "idle".into(), subtype: "system".into(), metadata: crate::ipc::sysdata::idle::get_idle_json(), path: std::path::PathBuf::new(), exe_path: "".into() }),
        "display" => Some(RegistryEntry {
            id: "display_group".into(),
            category: "display".into(),
            subtype: "system".into(),
            metadata: json!({
                "monitors": crate::ipc::sysdata::display::MonitorManager::enumerate_monitors().into_iter().map(|m| json!({
                    "id": m.id,
                    "primary": m.primary,
                    "x": m.x,
                    "y": m.y,
                    "width": m.width,
                    "height": m.height,
                    "scale": m.scale,
                    "dpi": m.dpi,
                    "refresh_rate_hz": m.refresh_rate_hz,
                    "color_depth_bits": m.color_depth_bits,
                    "bits_per_channel": m.bits_per_channel,
                    "orientation": m.orientation,
                    "aspect_ratio": m.aspect_ratio,
                    "device_name": m.device_name,
                    "monitor_name": m.monitor_name,
                    "connection_type": m.connection_type,
                    "hdr_supported": m.hdr_supported,
                    "physical_width_mm": m.physical_width_mm,
                    "physical_height_mm": m.physical_height_mm,
                    "manufacturer": m.manufacturer,
                    "product_code": m.product_code,
                    "serial_number": m.serial_number,
                    "year_of_manufacture": m.year_of_manufacture,
                })).collect::<Vec<_>>()
            }),
            path: std::path::PathBuf::new(),
            exe_path: "".into(),
        }),
        _ => None,
    }
}

// ── Updater threads ─────────────────────────────────────────────────
//
// Each tier thread:
//   1. Collects data from its sensors **outside** the registry lock.
//   2. Briefly acquires a write lock to merge results.
//   3. Sleeps via `interruptible_sleep()` (Condvar) so it wakes
//      instantly when demands or config change.

/// Start registry updater threads — fast, appdata, cpu, and slow tiers.
pub fn start_registry_updater() {

    // ── Fast-tier (time, audio, keyboard, mouse, idle, power, display) ──
    thread::spawn(move || {
        loop {
            if pull_paused() {
                interruptible_sleep(Duration::from_millis(50));
                continue;
            }

            let mut fast_requested = Vec::<&str>::new();
            for section in ["time", "keyboard", "mouse", "audio", "idle"] {
                if section_tracking_enabled(section) {
                    if let Some(cat) = section_to_internal_category(section) {
                        fast_requested.push(cat);
                    }
                }
            }

            if fast_requested.is_empty() {
                interruptible_sleep(Duration::from_millis(IDLE_SLEEP_MS));
                continue;
            }

            let rate = fast_pull_rate_ms().max(1);

            // Collect outside the lock
            let fast_data: Vec<RegistryEntry> = fast_requested
                .iter()
                .filter_map(|cat| single_sys_entry(cat))
                .collect();

            // Merge under write lock (brief)
            {
                let mut reg = global_registry().write().unwrap();
                let merged = merge_sysdata_tier(&reg.sysdata, fast_data, &fast_requested);
                if reg.sysdata != merged {
                    reg.sysdata = merged;
                }
            }

            interruptible_sleep(Duration::from_millis(rate));
        }
    });

    // ── Appdata (active windows) ──
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(5));
        loop {
            if pull_paused() {
                interruptible_sleep(Duration::from_millis(100));
                continue;
            }

            if !section_tracking_enabled("appdata") {
                interruptible_sleep(Duration::from_millis(IDLE_SLEEP_MS));
                continue;
            }

            let appdata_rate = fast_pull_rate_ms().max(25);
            let appdata = ActiveWindowManager::enumerate_active_windows();

            {
                let mut reg = global_registry().write().unwrap();
                if reg.appdata != appdata {
                    reg.appdata = appdata;
                }
            }

            interruptible_sleep(Duration::from_millis(appdata_rate));
        }
    });

    // ── CPU (slow, isolated) ──
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(10));
        loop {
            if pull_paused() {
                interruptible_sleep(Duration::from_millis(100));
                continue;
            }

            if !section_tracking_enabled("cpu") {
                interruptible_sleep(Duration::from_millis(IDLE_SLEEP_MS));
                continue;
            }

            let rate = slow_pull_rate_ms().max(50);
            let cpu_entry = pull_sysdata_cpu();

            {
                let mut reg = global_registry().write().unwrap();
                let merged = merge_sysdata_tier(&reg.sysdata, vec![cpu_entry], &["cpu"]);
                if reg.sysdata != merged {
                    reg.sysdata = merged;
                }
            }

            interruptible_sleep(Duration::from_millis(rate));
        }
    });

    // ── Slow-tier (gpu, ram, storage, network, bluetooth, wifi, system, processes) ──
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(15));
        loop {
            if pull_paused() {
                interruptible_sleep(Duration::from_millis(100));
                continue;
            }

            let slow_sections: &[&str] = &[
                "gpu", "ram", "storage", "network",
                "bluetooth", "wifi", "system", "processes",
                "power", "displays",
            ];

            let mut requested_slow = Vec::<&str>::new();
            for section in slow_sections {
                if section_tracking_enabled(section) {
                    if let Some(cat) = section_to_internal_category(section) {
                        requested_slow.push(cat);
                    }
                }
            }

            if requested_slow.is_empty() {
                interruptible_sleep(Duration::from_millis(IDLE_SLEEP_MS));
                continue;
            }

            let rate = slow_pull_rate_ms().max(50);

            let slow_data: Vec<RegistryEntry> = requested_slow
                .iter()
                .filter_map(|cat| single_sys_entry(cat))
                .collect();

            {
                let mut reg = global_registry().write().unwrap();
                let merged = merge_sysdata_tier(&reg.sysdata, slow_data, &requested_slow);
                if reg.sysdata != merged {
                    reg.sysdata = merged;
                }
            }

            interruptible_sleep(Duration::from_millis(rate));
        }
    });
}
