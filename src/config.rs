// ~/sentinel/sentinel-backend/src/config.rs

use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        OnceLock, RwLock,
    },
};
use crate::{info, warn, error};
use crate::paths::sentinel_root_dir;

/// Backend configuration persisted in config.yaml next to the executable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendConfig {
    /// Interval in milliseconds between registry data pulls (0–5000).
    #[serde(default = "default_pull_rate")]
    pub data_pull_rate_ms: u64,

    /// Whether data pulling is currently paused.
    #[serde(default)]
    pub data_pull_paused: bool,
}

fn default_pull_rate() -> u64 {
    100
}

impl Default for BackendConfig {
    fn default() -> Self {
        Self {
            data_pull_rate_ms: default_pull_rate(),
            data_pull_paused: false,
        }
    }
}

// ── Runtime atomics so the updater thread can read without locking ──

static PULL_RATE_MS: AtomicU64 = AtomicU64::new(100);
static PULL_PAUSED: AtomicBool = AtomicBool::new(false);

/// Read the current pull rate (milliseconds).
pub fn pull_rate_ms() -> u64 {
    PULL_RATE_MS.load(Ordering::Relaxed)
}

/// Read whether pulling is paused.
pub fn pull_paused() -> bool {
    PULL_PAUSED.load(Ordering::Relaxed)
}

/// Set the pull rate at runtime and persist to disk.
pub fn set_pull_rate_ms(ms: u64) {
    let clamped = ms.min(5000);
    PULL_RATE_MS.store(clamped, Ordering::Relaxed);
    update_and_save(|cfg| cfg.data_pull_rate_ms = clamped);
    info!("Data pull rate set to {}ms", clamped);
}

/// Set the paused state at runtime and persist to disk.
pub fn set_pull_paused(paused: bool) {
    PULL_PAUSED.store(paused, Ordering::Relaxed);
    update_and_save(|cfg| cfg.data_pull_paused = paused);
    info!("Data pull paused: {}", paused);
}

// ── Persistent on-disk config ──

static CONFIG: OnceLock<RwLock<BackendConfig>> = OnceLock::new();

fn global_config() -> &'static RwLock<BackendConfig> {
    CONFIG.get_or_init(|| RwLock::new(BackendConfig::default()))
}

fn config_path() -> PathBuf {
    sentinel_root_dir().join("config.yaml")
}

/// Load config.yaml from disk (or create defaults). Call once at startup.
pub fn load_config() -> BackendConfig {
    let path = config_path();

    let cfg = if path.exists() {
        match std::fs::read_to_string(&path) {
            Ok(text) => match serde_yaml::from_str::<BackendConfig>(&text) {
                Ok(c) => {
                    info!("Loaded backend config from {}", path.display());
                    c
                }
                Err(e) => {
                    warn!("Failed to parse config.yaml, using defaults: {e}");
                    BackendConfig::default()
                }
            },
            Err(e) => {
                warn!("Failed to read config.yaml, using defaults: {e}");
                BackendConfig::default()
            }
        }
    } else {
        info!("No config.yaml found, creating defaults at {}", path.display());
        let defaults = BackendConfig::default();
        save_config_to_disk(&defaults);
        defaults
    };

    // Sync atomics
    PULL_RATE_MS.store(cfg.data_pull_rate_ms.min(5000), Ordering::Relaxed);
    PULL_PAUSED.store(cfg.data_pull_paused, Ordering::Relaxed);

    // Store in global
    *global_config().write().unwrap() = cfg.clone();

    cfg
}

/// Return a snapshot of the current in-memory config.
pub fn current_config() -> BackendConfig {
    global_config().read().unwrap().clone()
}

fn update_and_save(f: impl FnOnce(&mut BackendConfig)) {
    let mut cfg = global_config().write().unwrap();
    f(&mut cfg);
    save_config_to_disk(&cfg);
}

fn save_config_to_disk(cfg: &BackendConfig) {
    let path = config_path();
    match serde_yaml::to_string(cfg) {
        Ok(text) => {
            if let Err(e) = std::fs::write(&path, text) {
                error!("Failed to write config.yaml: {e}");
            }
        }
        Err(e) => error!("Failed to serialize config: {e}"),
    }
}
