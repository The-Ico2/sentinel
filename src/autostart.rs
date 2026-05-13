// ~/veil/veil-backend/src/autostart.rs
//
// Tray settings persistence (autostart flags, run-at-startup) and
// user-config directory bootstrapping — extracted from the old systemtray module
// so it can be shared by the daemon and the OpenRender UI tray.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};

use crate::{info, warn};
use crate::paths::veil_root_dir;

// ---------------------------------------------------------------------------
// Tray settings
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TraySettings {
    #[serde(default)]
    pub run_backend_at_startup: bool,
    #[serde(default)]
    pub addon_autostart: HashMap<String, bool>,
}

pub fn tray_settings_path() -> Option<PathBuf> {
    Some(veil_root_dir().join("tray_settings.json"))
}

pub fn load_tray_settings() -> TraySettings {
    let Some(path) = tray_settings_path() else {
        warn!("USERPROFILE not set; using default tray settings");
        return TraySettings::default();
    };

    if !path.exists() {
        return TraySettings::default();
    }

    match std::fs::read_to_string(&path) {
        Ok(content) => match serde_json::from_str::<TraySettings>(&content) {
            Ok(settings) => settings,
            Err(e) => {
                warn!("Failed to parse tray settings '{}': {}", path.display(), e);
                TraySettings::default()
            }
        },
        Err(e) => {
            warn!("Failed to read tray settings '{}': {}", path.display(), e);
            TraySettings::default()
        }
    }
}

// ---------------------------------------------------------------------------
// Run at startup (Windows registry)
// ---------------------------------------------------------------------------

#[cfg(not(target_os = "windows"))]
pub fn is_backend_startup_enabled() -> Result<bool, String> {
    Ok(false)
}
#[cfg(not(target_os = "windows"))]
pub fn set_backend_startup_enabled(_enabled: bool) -> Result<(), String> {
    Err("Run at startup toggle is only supported on Windows".to_string())
}

// ---------------------------------------------------------------------------
// Addon autostart
// ---------------------------------------------------------------------------

pub fn start_configured_autostart_addons() {
    let settings = load_tray_settings();

    let addons_to_start: Vec<String> = settings
        .addon_autostart
        .iter()
        .filter(|(_, enabled)| **enabled)
        .map(|(name, _)| name.clone())
        .collect();

    if addons_to_start.is_empty() {
        info!("[addons] No addons configured for autostart");
        return;
    }

    for addon_name in addons_to_start {
        match crate::ipc::addon::start(Some(json!({"addon_name": addon_name.clone()}))) {
            Ok(_) => info!("[addons] Autostarted '{}' on backend startup", addon_name),
            Err(e) => warn!("[addons] Failed to autostart '{}' on backend startup: {}", addon_name, e),
        }
    }
}

// ---------------------------------------------------------------------------
// User config directory bootstrapping
// ---------------------------------------------------------------------------

pub fn ensure_user_config_dirs() {
    if std::env::var("USERPROFILE").is_ok() {
        let root = veil_root_dir();
        for p in [
            root.join("Assets"),
            root.join("Assets/Addons"),
        ] {
            if let Err(e) = std::fs::create_dir_all(&p) {
                warn!("Failed to create config dir {}: {}", p.display(), e);
            } else {
                info!("Ensured config dir exists: {}", p.display());
            }
        }

        let addons_root = root.join("Addons");
        if let Ok(addon_entries) = std::fs::read_dir(&addons_root) {
            for addon_entry in addon_entries.flatten() {
                let addon_dir = addon_entry.path();
                if !addon_dir.is_dir() {
                    continue;
                }

                let addon_json = addon_dir.join("addon.json");
                let parsed = std::fs::read_to_string(&addon_json)
                    .ok()
                    .and_then(|text| serde_json::from_str::<JsonValue>(&text).ok())
                    .unwrap_or(JsonValue::Null);

                let accepts_assets = parsed
                    .get("accepts_assets")
                    .and_then(|v| v.as_bool())
                    .or_else(|| parsed.get("assets").and_then(|a| a.get("accepts")).and_then(|v| v.as_bool()))
                    .unwrap_or(false);

                if !accepts_assets {
                    continue;
                }

                let addon_id = parsed
                    .get("id")
                    .and_then(|v| v.as_str())
                    .or_else(|| addon_dir.file_name().and_then(|s| s.to_str()))
                    .unwrap_or("unknown-addon");

                let addon_assets_dir = root.join("Assets").join("Addons").join(addon_id);
                if let Err(e) = std::fs::create_dir_all(&addon_assets_dir) {
                    warn!("Failed to create addon asset dir {}: {}", addon_assets_dir.display(), e);
                } else {
                    info!("Ensured addon asset dir exists: {}", addon_assets_dir.display());
                }

                let categories = parsed
                    .get("asset_categories")
                    .and_then(|v| v.as_array())
                    .or_else(|| parsed.get("assets").and_then(|a| a.get("categories")).and_then(|v| v.as_array()))
                    .cloned()
                    .unwrap_or_default();

                for category in categories {
                    if let Some(category_name) = category.as_str() {
                        let category_dir = root.join("Assets").join(category_name);
                        if let Err(e) = std::fs::create_dir_all(&category_dir) {
                            warn!("Failed to create asset category dir {}: {}", category_dir.display(), e);
                        } else {
                            info!("Ensured asset category dir exists: {}", category_dir.display());
                        }
                    }
                }
            }
        }
    } else {
        warn!("USERPROFILE not set; cannot create user config directories");
    }
}
