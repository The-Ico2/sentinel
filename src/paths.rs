// ~/sentinel/sentinel-backend/src/paths.rs

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

/// The canonical Sentinel root is always `~/.Sentinel/`.
/// All config, addons, and assets live here.
/// Result is cached after the first successful resolution.
pub fn sentinel_root_dir() -> PathBuf {
    CACHED_ROOT.get_or_init(|| {
        let root = if let Some(home) = user_home_dir() {
            home.join(".Sentinel")
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
        info!("Sentinel root resolved: {}", root.display());
        root
    }).clone()
}

/// Returns true if the currently running exe is inside the sentinel root (`~/.Sentinel/`).
pub fn is_running_from_sentinel_root() -> bool {
    let root = sentinel_root_dir();
    match std::env::current_exe() {
        Ok(exe) => exe.starts_with(&root),
        Err(_) => false,
    }
}