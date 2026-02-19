// ~/sentinel/sentinel-backend/src/ipc/registry.rs

use serde::{Deserialize, Serialize};
use serde_json::Value;
use notify::{RecommendedWatcher, RecursiveMode, Watcher, EventKind, Config};
use std::{
    path::{Path, PathBuf},
    sync::{OnceLock, RwLock, mpsc::channel},
    time::{Duration, Instant},
};

use crate::{
    info, warn, error,
    paths::sentinel_root_dir,
    ipc::appdata::active_window::ActiveWindowManager,
};

static LAST_REGISTRY_WRITE: OnceLock<RwLock<Instant>> = OnceLock::new();

/// Single registry entry (addon, widget, etc)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryEntry {
    pub id: String,
    pub category: String,
    pub subtype: String,
    pub metadata: Value,
    pub path: PathBuf,
    pub exe_path: String,
}

/// Entire registry state
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
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

pub fn pull_sysdata() -> Vec<RegistryEntry> {
    use crate::ipc::sysdata::{
        display::MonitorManager,
        audio::get_audio_json,
        cpu::get_cpu_json,
        gpu::get_gpu_json,
        network::get_network_json,
        ram::get_ram_json,
        storage::get_storage_json,
        temp::get_temp_json,
        time::get_time_json,
    };
    use serde_json::json;

    let mut entries = Vec::new();

    // Monitors
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
                "scale": m.scale
            }),
            path: std::path::PathBuf::new(),
            exe_path: "".into(),
        });
    }

    // CPU
    entries.push(RegistryEntry {
        id: "cpu".into(),
        category: "cpu".into(),
        subtype: "system".into(),
        metadata: get_cpu_json(),
        path: std::path::PathBuf::new(),
        exe_path: "".into(),
    });

    // RAM
    entries.push(RegistryEntry {
        id: "ram".into(),
        category: "ram".into(),
        subtype: "system".into(),
        metadata: get_ram_json(),
        path: std::path::PathBuf::new(),
        exe_path: "".into(),
    });

    // GPU
    entries.push(RegistryEntry {
        id: "gpu".into(),
        category: "gpu".into(),
        subtype: "system".into(),
        metadata: get_gpu_json(),
        path: std::path::PathBuf::new(),
        exe_path: "".into(),
    });

    // Storage
    entries.push(RegistryEntry {
        id: "storage".into(),
        category: "storage".into(),
        subtype: "system".into(),
        metadata: get_storage_json(),
        path: std::path::PathBuf::new(),
        exe_path: "".into(),
    });

    // Network
    entries.push(RegistryEntry {
        id: "network".into(),
        category: "network".into(),
        subtype: "system".into(),
        metadata: get_network_json(),
        path: std::path::PathBuf::new(),
        exe_path: "".into(),
    });

    // Temp
    entries.push(RegistryEntry {
        id: "temp".into(),
        category: "temp".into(),
        subtype: "system".into(),
        metadata: get_temp_json(),
        path: std::path::PathBuf::new(),
        exe_path: "".into(),
    });

    // Audio
    entries.push(RegistryEntry {
        id: "audio".into(),
        category: "audio".into(),
        subtype: "system".into(),
        metadata: get_audio_json(),
        path: std::path::PathBuf::new(),
        exe_path: "".into(),
    });

    // Time
    entries.push(RegistryEntry {
        id: "time".into(),
        category: "time".into(),
        subtype: "system".into(),
        metadata: get_time_json(),
        path: std::path::PathBuf::new(),
        exe_path: "".into(),
    });

    entries
}

pub fn pull_appdata() -> Vec<RegistryEntry> {
    ActiveWindowManager::enumerate_active_windows()
}
//
// ---------- REGISTRY MANAGER ----------
//

pub fn registry_manager() {
    let root = sentinel_root_dir();
    info!("Initializing registry at '{}'", root.display());

    // Initial build
    {
        let mut reg = global_registry().write().unwrap();
        *reg = registry_build(&root);
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
                    if event.paths.iter().any(|p| {
                        p.file_name()
                            .and_then(|n| n.to_str())
                            .map(|n| n.eq_ignore_ascii_case("registry.json"))
                            .unwrap_or(false)
                    }) {
                        let last = LAST_REGISTRY_WRITE
                            .get_or_init(|| RwLock::new(Instant::now()))
                            .read()
                            .unwrap();

                        if last.elapsed() < Duration::from_millis(500) {
                            continue; // ignore self-write
                        }

                        info!("Detected external change to registry.json, reloading registry");
                        reload_registry(&root);
                    }

                }
            }
            Ok(Err(e)) => error!("[RegistryWatcher] notify error: {e}"),
            Err(e) => error!("[RegistryWatcher] channel error: {e}"),
        }
    }
}

//
// ---------- BUILD / RELOAD ----------
//

pub fn registry_build(root: &Path) -> Registry {
    info!("Building registry from disk at '{}'", root.display());
    let addons = discover_addons(&root.join("Addons"));
    let assets = discover_assets(&root.join("Assets"));
    let sysdata = pull_sysdata();
    let appdata = pull_appdata();

    info!("Built registry: {} addons, {} assets", addons.len(), assets.len());

    Registry { addons, assets, sysdata, appdata }
}

fn reload_registry(root: &Path) {
    info!("Reloading registry...");
    let new_registry = registry_build(root);

    {
        let mut reg = global_registry().write().unwrap();
        *reg = new_registry;
        write_registry_json(&reg, root);
    }

    info!("Registry reload complete");
}

pub fn write_registry_json(reg: &Registry, root: &Path) {
    let path = root.join("registry.json");

    let json = match serde_json::to_string_pretty(reg) {
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

        info!("registry.json updated");
    }
}