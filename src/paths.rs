// ~/sentinel/sentinel-backend/src/paths.rs

use std::path::PathBuf;
use crate::{info, warn};

pub fn user_home_dir() -> Option<PathBuf> {
    // Primary (most reliable on Windows)
    if let Ok(profile) = std::env::var("USERPROFILE") {
        info!("USERPROFILE environment variable found: {}", profile);
        return Some(PathBuf::from(profile));
    }

    // Fallback (older / edge cases)
    let drive = std::env::var("HOMEDRIVE").ok();
    let path = std::env::var("HOMEPATH").ok();

    match (drive, path) {
        (Some(d), Some(p)) => {
            let full = PathBuf::from(format!("{}{}", d, p));
            info!("Resolved home directory from HOMEDRIVE/HOMEPATH: {}", full.display());
            Some(full)
        }
        _ => {
            warn!("Could not resolve home directory using USERPROFILE or HOMEDRIVE/HOMEPATH");
            None
        }
    }
}

pub fn sentinel_root_dir() -> PathBuf {
    match std::env::current_exe() {
        Ok(path) => {
            if let Some(parent) = path.parent() {
                parent.to_path_buf()
            } else {
                warn!("Current executable has no parent, using current directory as sentinel root");
                std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
            }
        }
        Err(e) => {
            warn!("Failed to get current executable path: {e}, using current directory as sentinel root");
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
        }
    }
}