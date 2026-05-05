// ~/veil/veil-backend/src/paths.rs

use std::path::PathBuf;
use std::sync::OnceLock;
use crate::{info, warn};

static CACHED_ROOT: OnceLock<PathBuf> = OnceLock::new();

pub fn user_home_dir() -> Option<PathBuf> {
    // Primary (most reliable on Windows)
    if let Ok(profile) = std::env::var("USERPROFILE") {
        return Some(PathBuf::from(profile));
    }

    // Fallback (older / edge cases)
    let drive = std::env::var("HOMEDRIVE").ok();
    let path = std::env::var("HOMEPATH").ok();

    match (drive, path) {
        (Some(d), Some(p)) => {
            let full = PathBuf::from(format!("{}{}", d, p));
            Some(full)
        }
        _ => {
            warn!("Could not resolve home directory using USERPROFILE or HOMEDRIVE/HOMEPATH");
            None
        }
    }
}

/// The canonical VEIL root is always `~/ProjectOpen/VEIL/`.
/// All config, addons, and assets live here.
/// Result is cached after the first successful resolution.
pub fn veil_root_dir() -> PathBuf {
    CACHED_ROOT.get_or_init(|| {
        let root = if let Some(home) = user_home_dir() {
            home.join("ProjectOpen").join("VEIL")
        } else {
            warn!("Could not resolve home directory, falling back to exe parent");
            match std::env::current_exe() {
                Ok(path) => path.parent().map(|p| p.to_path_buf())
                    .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))),
                Err(e) => {
                    warn!("Failed to get current executable path: {e}");
                    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
                }
            }
        };
        info!("VEIL root resolved: {}", root.display());
        root
    }).clone()
}