// ~/sentinel/sentinel-backend/src/ipc/registry.rs

use serde::{Deserialize, Serialize};
use serde_json::Value;
use notify::{RecommendedWatcher, RecursiveMode, Watcher, EventKind, Config};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{OnceLock, RwLock, mpsc::channel},
    time::{Duration, Instant},
};

use crate::{
    info, warn, error,
    paths::sentinel_root_dir,
};

static LAST_REGISTRY_WRITE: OnceLock<RwLock<Instant>> = OnceLock::new();

/// Single registry entry (addon, widget, etc)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RegistryEntry {
    pub id: String,
    pub category: String,
    pub subtype: String,
    pub metadata: Value,
    pub path: PathBuf,
    pub exe_path: String,
}

/// Entire registry state
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct Registry {
    pub addons: Vec<RegistryEntry>,
    pub assets: Vec<RegistryEntry>,
    pub sysdata: Vec<RegistryEntry>,
    pub appdata: Vec<RegistryEntry>,
}

static REGISTRY: OnceLock<RwLock<Registry>> = OnceLock::new();

pub fn global_registry() -> &'static RwLock<Registry> {
    REGISTRY.get_or_init(|| RwLock::new(Registry::default()))
}

//
// ---------- DISCOVERY ----------
//

pub fn discover_addons(addons_root: &Path) -> Vec<RegistryEntry> {
    info!("Discovering addons in '{}'", addons_root.display());
    let mut entries = Vec::new();

    if let Ok(read_dir) = std::fs::read_dir(addons_root) {
        for entry in read_dir.flatten() {
            let addon_dir = entry.path();
            let manifest_path = addon_dir.join("addon.json");

            if let Ok(data) = std::fs::read_to_string(&manifest_path) {
                match serde_json::from_str::<Value>(&data) {
                    Ok(mut meta) => {
                        info!("Discovered addon: {}", meta["name"].as_str().unwrap_or("unknown"));

                        // Convert exe_path to absolute path
                        if let Some(exe_rel) = meta["exe_path"].as_str() {
                            let exe_abs = addon_dir.join(exe_rel);
                            
                            // Warn if the executable file doesn't exist
                            if !exe_abs.exists() {
                                warn!(
                                    "Addon '{}' exe path does not exist: {}",
                                    meta["name"].as_str().unwrap_or("unknown"),
                                    exe_abs.display()
                                );
                            }

                            meta["exe_path"] = Value::String(exe_abs.to_string_lossy().to_string());
                        } else {
                            warn!("Addon '{}' has no 'exe_path' in manifest", addon_dir.display());
                        }


                        entries.push(RegistryEntry {
                            id: meta["id"].as_str().unwrap_or("").to_string(),
                            category: "addon".into(),
                            subtype: addon_dir.file_name().unwrap().to_string_lossy().to_string(),
                            metadata: meta.clone(),
                            path: addon_dir,
                            exe_path: meta["exe_path"].as_str().unwrap_or("").to_string(),
                        });
                    }
                    Err(e) => warn!("Failed to parse manifest '{}': {e}", manifest_path.display()),
                }
            } else {
                warn!("Failed to read manifest: '{}'", manifest_path.display());
            }
        }
    } else {
        warn!("Addons root '{}' not found or unreadable", addons_root.display());
    }

    entries
}

pub fn discover_assets(assets_root: &Path) -> Vec<RegistryEntry> {
    info!("Discovering assets in '{}'", assets_root.display());
    let mut entries = Vec::new();

    if let Ok(read_dir) = std::fs::read_dir(assets_root) {
        for category in read_dir.flatten() {
            let category_path = category.path();
            let category_name = category_path.file_name().unwrap().to_string_lossy().to_string();

            for asset in walkdir::WalkDir::new(&category_path)
                .min_depth(1)
                .max_depth(2)
                .into_iter()
                .filter_map(Result::ok)
            {
                if asset.file_name() == "manifest.json" {
                    if let Ok(data) = std::fs::read_to_string(asset.path()) {
                        match serde_json::from_str::<Value>(&data) {
                            Ok(mut meta) => {
                                info!(
                                    "Discovered asset: {} ({})",
                                    meta["id"].as_str().unwrap_or("unknown"),
                                    category_name
                                );

                                // Handle exe_path if present
                                let exe_path = if let Some(exe_rel) = meta["exe_path"].as_str() {
                                    let exe_abs = asset.path().parent().unwrap().join(exe_rel);
                                    if !exe_abs.exists() {
                                        warn!(
                                            "Asset '{}' exe path does not exist: {}",
                                            meta["id"].as_str().unwrap_or("unknown"),
                                            exe_abs.display()
                                        );
                                    }
                                    meta["exe_path"] = Value::String(exe_abs.to_string_lossy().to_string());
                                    exe_abs.to_string_lossy().to_string()
                                } else {
                                    "NULL".into()
                                };

                                entries.push(RegistryEntry {
                                    id: meta["id"].as_str().unwrap_or("").to_string(),
                                    category: category_name.clone(),
                                    subtype: asset
                                        .path()
                                        .parent()
                                        .and_then(|p| p.file_name())
                                        .unwrap()
                                        .to_string_lossy()
                                        .to_string(),
                                    metadata: meta,
                                    path: asset.path().parent().unwrap().to_path_buf(),
                                    exe_path,
                                });
                            }
                            Err(e) => warn!("Failed to parse asset manifest '{}': {e}", asset.path().display()),
                        }
                    } else {
                        warn!("Failed to read asset manifest '{}'", asset.path().display());
                    }
                }
            }
        }
    } else {
        warn!("Assets root '{}' not found or unreadable", assets_root.display());
    }

    entries
}

/// Categories that belong to the **fast** (lightweight) tier.
pub const FAST_CATEGORIES: &[&str] = &[
    "time", "keyboard", "mouse", "audio", "idle", "power", "display",
];

/// Pull only fast-tier sysdata (cheap calls: time, keyboard, mouse, audio, idle, power, display).
pub fn pull_sysdata_fast() -> Vec<RegistryEntry> {
    use crate::ipc::sysdata::{
        display::MonitorManager,
        audio::get_audio_json,
        time::get_time_json,
        keyboard::get_keyboard_json,
        mouse::get_mouse_json,
        power::get_power_json,
        idle::get_idle_json,
    };
    use serde_json::json;

    let mut entries = Vec::new();

    // Monitors (fast — just EnumDisplayMonitors + GetMonitorInfo)
    let monitors = MonitorManager::enumerate_monitors();
    for m in monitors {
        entries.push(RegistryEntry {
            id: format!("display_{}", m.id),
            category: "display".into(),
            subtype: "monitor".into(),
            metadata: json!({
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
            }),
            path: std::path::PathBuf::new(),
            exe_path: "".into(),
        });
    }

    entries.push(RegistryEntry { id: "audio".into(),    category: "audio".into(),    subtype: "system".into(), metadata: get_audio_json(),    path: std::path::PathBuf::new(), exe_path: "".into() });
    entries.push(RegistryEntry { id: "time".into(),     category: "time".into(),     subtype: "system".into(), metadata: get_time_json(),     path: std::path::PathBuf::new(), exe_path: "".into() });
    entries.push(RegistryEntry { id: "keyboard".into(), category: "keyboard".into(), subtype: "system".into(), metadata: get_keyboard_json(), path: std::path::PathBuf::new(), exe_path: "".into() });
    entries.push(RegistryEntry { id: "mouse".into(),    category: "mouse".into(),    subtype: "system".into(), metadata: get_mouse_json(),    path: std::path::PathBuf::new(), exe_path: "".into() });
    entries.push(RegistryEntry { id: "power".into(),    category: "power".into(),    subtype: "system".into(), metadata: get_power_json(),    path: std::path::PathBuf::new(), exe_path: "".into() });
    entries.push(RegistryEntry { id: "idle".into(),     category: "idle".into(),     subtype: "system".into(), metadata: get_idle_json(),     path: std::path::PathBuf::new(), exe_path: "".into() });

    entries
}

/// Pull only slow-tier sysdata (expensive calls: cpu, gpu, ram, storage, network, bluetooth, wifi, system, processes).
pub fn pull_sysdata_cpu() -> RegistryEntry {
    use crate::ipc::sysdata::cpu::get_cpu_json;

    RegistryEntry {
        id: "cpu".into(),
        category: "cpu".into(),
        subtype: "system".into(),
        metadata: get_cpu_json(),
        path: std::path::PathBuf::new(),
        exe_path: "".into(),
    }
}

/// Pull slow-tier sysdata that is heavier/less latency-sensitive than CPU.
/// CPU is collected in its own updater loop so it can honor configured
/// collection rates even when other slow probes block.
pub fn pull_sysdata_slow() -> Vec<RegistryEntry> {
    use std::thread;
    use crate::ipc::sysdata::{
        gpu::get_gpu_json,
        ram::get_ram_json,
        storage::get_storage_json,
        network::get_network_json,
        bluetooth::get_bluetooth_json,
        wifi::get_wifi_json,
        system::get_system_json,
        processes::get_processes_json,
    };

    // Run all slow-tier modules in parallel — each one spawns PowerShell /
    // WMI queries that block for 1-5s.  By running concurrently the total
    // wall-clock time is that of the slowest single module instead of the
    // sum of all of them.
    thread::scope(|s| {
        let h_ram       = s.spawn(|| ("ram",       get_ram_json()));
        let h_gpu       = s.spawn(|| ("gpu",       get_gpu_json()));
        let h_storage   = s.spawn(|| ("storage",   get_storage_json()));
        let h_network   = s.spawn(|| ("network",   get_network_json()));
        let h_bluetooth = s.spawn(|| ("bluetooth", get_bluetooth_json()));
        let h_wifi      = s.spawn(|| ("wifi",      get_wifi_json()));
        let h_system    = s.spawn(|| ("system",    get_system_json()));
        let h_processes = s.spawn(|| ("processes", get_processes_json()));

        let results = [
            h_ram.join(),
            h_gpu.join(),
            h_storage.join(),
            h_network.join(),
            h_bluetooth.join(),
            h_wifi.join(),
            h_system.join(),
            h_processes.join(),
        ];

        results
            .into_iter()
            .filter_map(|r| r.ok())
            .map(|(cat, metadata)| RegistryEntry {
                id: cat.into(),
                category: cat.into(),
                subtype: "system".into(),
                metadata,
                path: std::path::PathBuf::new(),
                exe_path: "".into(),
            })
            .collect()
    })
}

/// Merge a partial tier update into the existing sysdata vec.
/// Entries whose category belongs to `tier_categories` are replaced; the rest are kept.
pub fn merge_sysdata_tier(existing: &[RegistryEntry], fresh: Vec<RegistryEntry>, tier_categories: &[&str]) -> Vec<RegistryEntry> {
    let mut merged: Vec<RegistryEntry> = existing
        .iter()
        .filter(|e| !tier_categories.iter().any(|c| e.category.eq_ignore_ascii_case(c)))
        .cloned()
        .collect();
    merged.extend(fresh);
    merged
}

//
// ---------- REGISTRY MANAGER ----------
//

pub fn registry_manager() {
    let root = sentinel_root_dir();
    info!("Initializing registry at '{}'", root.display());

    // Quick initial build — discover addons & assets only.
    // Sysdata and appdata are populated by the data-updater threads that
    // start immediately after, so the IPC server & tray come up fast.
    {
        let mut reg = global_registry().write().unwrap();
        let addons = discover_addons(&root.join("Addons"));
        let assets = discover_assets(&root.join("Assets"));
        *reg = Registry { addons, assets, sysdata: Vec::new(), appdata: Vec::new() };
        write_registry_json(&reg, &root);
        info!(
            "Registry initialized: {} addons, {} assets",
            reg.addons.len(),
            reg.assets.len()
        );
    }


    // Watch for live changes
    std::thread::spawn(move || {
        if let Err(e) = registry_watcher() {
            error!("Registry watcher failed: {e}");
        }
    });
}

//
// ---------- WATCHER ----------
//

pub fn registry_watcher() -> Result<(), Box<dyn std::error::Error>> {
    info!("Starting registry watcher");
    let (tx, rx) = channel();
    let root = sentinel_root_dir();
    let addons_root = root.join("Addons");
    let assets_root = root.join("Assets");

    let mut watcher: RecommendedWatcher =
        Watcher::new(tx, Config::default().with_poll_interval(Duration::from_millis(250)))?;

    if root.exists() {
        watcher.watch(&root, RecursiveMode::Recursive)?;
        info!("Watching registry root '{}'", root.display());
    } else {
        warn!("Registry root '{}' does not exist", root.display());
    }

    loop {
        match rx.recv() {
            Ok(Ok(event)) => {
                if matches!(event.kind, EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)) {
                    let touches_registry_json = event.paths.iter().any(|p| {
                        p.file_name()
                            .and_then(|n| n.to_str())
                            .map(|n| n.eq_ignore_ascii_case("registry.json"))
                            .unwrap_or(false)
                    });

                    if touches_registry_json {
                        continue;
                    }

                    let touches_content_tree = event.paths.iter().any(|p| {
                        if !(p.starts_with(&addons_root) || p.starts_with(&assets_root)) {
                            return false;
                        }

                        p.file_name()
                            .and_then(|n| n.to_str())
                            .map(|n| {
                                n.eq_ignore_ascii_case("addon.json")
                                    || n.eq_ignore_ascii_case("manifest.json")
                            })
                            .unwrap_or(false)
                    });

                    if touches_content_tree {
                        info!("Detected Addons/Assets change, reloading registry");
                        reload_registry(&root);
                    }

                }
            }
            Ok(Err(e)) => error!("[RegistryWatcher] notify error: {e}"),
            Err(e) => error!("[RegistryWatcher] channel error: {e}"),
        }
    }
}

fn reload_registry(root: &Path) {
    info!("Reloading registry...");
    let addons = discover_addons(&root.join("Addons"));
    let assets = discover_assets(&root.join("Assets"));

    {
        let mut reg = global_registry().write().unwrap();
        // Re-discover addons & assets; keep current sysdata & appdata
        // (managed by the data-updater threads).
        reg.addons = addons;
        reg.assets = assets;
        write_registry_json(&reg, root);
    }

    info!("Registry reload complete");
}

pub fn write_registry_json(reg: &Registry, root: &Path) {
    let path = root.join("registry.json");

    let output = registry_to_output_json(reg);

    let json = match serde_json::to_string_pretty(&output) {
        Ok(j) => j,
        Err(e) => {
            error!("Failed to serialize registry: {e}");
            return;
        }
    };

    if let Err(e) = std::fs::write(&path, json) {
        error!("Failed to write registry.json: {e}");
    } else {
        *LAST_REGISTRY_WRITE
            .get_or_init(|| RwLock::new(Instant::now()))
            .write()
            .unwrap() = Instant::now();
    }
}

pub fn registry_to_output_json(reg: &Registry) -> Value {
    serde_json::json!({
        "addons": output_addons(&reg.addons),
        "assets": output_assets(&reg.assets),
        "sysdata": output_sysdata(&reg.sysdata),
        "appdata": output_appdata(&reg.appdata, &reg.sysdata),
    })
}

fn output_addons(addons: &[RegistryEntry]) -> Vec<Value> {
    addons
        .iter()
        .map(|entry| {
            let mut metadata = entry.metadata.clone();
            if let Some(obj) = metadata.as_object_mut() {
                obj.remove("exe_path");
            }

            serde_json::json!({
                "id": entry.id,
                "metadata": metadata,
                "path": entry.path,
                "entry_path": entry.exe_path,
            })
        })
        .collect()
}

fn output_assets(assets: &[RegistryEntry]) -> Value {
    let mut grouped = BTreeMap::<String, Vec<Value>>::new();

    for entry in assets {
        let mut metadata = entry.metadata.clone();
        if let Some(obj) = metadata.as_object_mut() {
            obj.remove("exe_path");
        }

        let entry_path = if let Some(v) = entry
            .metadata
            .get("files")
            .and_then(|f| f.get("entry"))
            .and_then(|v| v.as_str())
        {
            v.to_string()
        } else if let Some(v) = entry.metadata.get("entry").and_then(|v| v.as_str()) {
            v.to_string()
        } else if entry.exe_path != "NULL" {
            entry.exe_path.clone()
        } else {
            String::new()
        };

        grouped
            .entry(entry.category.clone())
            .or_default()
            .push(serde_json::json!({
                "id": entry.id,
                "category": entry.category,
                "subtype": entry.subtype,
                "metadata": metadata,
                "path": entry.path,
                "entry_path": entry_path,
            }));
    }

    serde_json::to_value(grouped).unwrap_or(Value::Null)
}

fn output_sysdata(sysdata: &[RegistryEntry]) -> Value {
    let displays: Vec<Value> = sysdata
        .iter()
        .filter(|entry| entry.category.eq_ignore_ascii_case("display"))
        .map(|entry| {
            let id = entry
                .metadata
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or(&entry.id)
                .to_string();

            serde_json::json!({
                "id": id,
                "category": entry.category,
                "subtype": entry.subtype,
                "metadata": entry.metadata,
            })
        })
        .collect();

    let category_meta = |name: &str| {
        sysdata
            .iter()
            .find(|entry| entry.category.eq_ignore_ascii_case(name))
            .map(|entry| entry.metadata.clone())
            .unwrap_or(Value::Null)
    };

    serde_json::json!({
        "displays": displays,
        "cpu": category_meta("cpu"),
        "ram": category_meta("ram"),
        "gpu": category_meta("gpu"),
        "storage": category_meta("storage"),
        "network": category_meta("network"),
        "audio": category_meta("audio"),
        "time": category_meta("time"),
        "keyboard": category_meta("keyboard"),
        "mouse": category_meta("mouse"),
        "power": category_meta("power"),
        "bluetooth": category_meta("bluetooth"),
        "wifi": category_meta("wifi"),
        "system": category_meta("system"),
        "processes": category_meta("processes"),
        "idle": category_meta("idle"),
    })
}

fn output_appdata(appdata: &[RegistryEntry], sysdata: &[RegistryEntry]) -> Value {
    let mut by_monitor = serde_json::Map::<String, Value>::new();

    for display in sysdata
        .iter()
        .filter(|entry| entry.category.eq_ignore_ascii_case("display"))
    {
        if let Some(monitor_id) = display.metadata.get("id").and_then(|v| v.as_str()) {
            by_monitor
                .entry(monitor_id.to_string())
                .or_insert_with(|| serde_json::json!({ "windows": [] }));
        }
    }

    for entry in appdata {
        if !entry.category.eq_ignore_ascii_case("active_window") {
            continue;
        }

        let Some(monitor_id) = entry
            .metadata
            .get("monitor_id")
            .and_then(|v| v.as_str())
            .map(|v| v.to_string())
        else {
            continue;
        };

        let window = serde_json::json!({
            "focused": entry.metadata.get("focused").and_then(|v| v.as_bool()).unwrap_or(false),
            "app_name": entry.metadata.get("app_name").and_then(|v| v.as_str()).unwrap_or("unknown"),
            "app_icon": entry.metadata.get("app_icon").and_then(|v| v.as_str()).unwrap_or(""),
            "exe_path": entry.metadata.get("exe_path").and_then(|v| v.as_str()).unwrap_or(""),
            "window_title": entry.metadata.get("window_title").and_then(|v| v.as_str()).unwrap_or(""),
            "pid": entry.metadata.get("pid").and_then(|v| v.as_u64()).unwrap_or(0),
            "window_state": entry.metadata.get("window_state").and_then(|v| v.as_str()).unwrap_or("normal"),
            "size": entry.metadata.get("size").cloned().unwrap_or_else(|| serde_json::json!({"width": 0, "height": 0})),
            "position": entry.metadata.get("position").cloned().unwrap_or_else(|| serde_json::json!({"x": 0, "y": 0})),
        });

        by_monitor
            .entry(monitor_id.clone())
            .or_insert_with(|| serde_json::json!({ "windows": [] }));

        if let Some(windows) = by_monitor
            .get_mut(&monitor_id)
            .and_then(|v| v.get_mut("windows"))
            .and_then(|v| v.as_array_mut())
        {
            windows.push(window);
        }
    }

    Value::Object(by_monitor)
}