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
    /// Interval (ms) for lightweight data: time, keyboard, mouse, audio, idle, power.
    #[serde(default = "default_fast_rate")]
    pub fast_pull_rate_ms: u64,

    /// Interval (ms) for heavyweight data: cpu, gpu, ram, storage, network, processes, etc.
    #[serde(default = "default_slow_rate")]
    pub slow_pull_rate_ms: u64,

    /// Whether data pulling is currently paused.
    #[serde(default)]
    pub data_pull_paused: bool,

    /// Whether to refresh fast-tier data inline on every IPC sysdata request.
    #[serde(default = "default_true")]
    pub refresh_on_request: bool,

    // -- back-compat: silently absorb the old single-rate field if present --
    #[serde(default, skip_serializing)]
    #[allow(dead_code)]
    data_pull_rate_ms: Option<u64>,
}

fn default_fast_rate() -> u64 { 50 }
fn default_slow_rate() -> u64 { 500 }
fn default_true()      -> bool { true }

impl Default for BackendConfig {
    fn default() -> Self {
        Self {
            fast_pull_rate_ms: default_fast_rate(),
            slow_pull_rate_ms: default_slow_rate(),
            data_pull_paused: false,
            refresh_on_request: default_true(),
            data_pull_rate_ms: None,
        }
    }
}

// ── Runtime atomics so the updater threads can read without locking ──

static FAST_PULL_RATE_MS: AtomicU64  = AtomicU64::new(50);
static SLOW_PULL_RATE_MS: AtomicU64  = AtomicU64::new(500);
static PULL_PAUSED:       AtomicBool = AtomicBool::new(false);
static REFRESH_ON_REQ:    AtomicBool = AtomicBool::new(true);

pub fn fast_pull_rate_ms() -> u64    { FAST_PULL_RATE_MS.load(Ordering::Relaxed) }
pub fn slow_pull_rate_ms() -> u64    { SLOW_PULL_RATE_MS.load(Ordering::Relaxed) }
pub fn pull_paused()       -> bool   { PULL_PAUSED.load(Ordering::Relaxed) }
pub fn refresh_on_request() -> bool  { REFRESH_ON_REQ.load(Ordering::Relaxed) }

/// Set the fast-tier pull rate at runtime and persist to disk.
pub fn set_fast_pull_rate_ms(ms: u64) {
    let clamped = ms.min(5000);
    FAST_PULL_RATE_MS.store(clamped, Ordering::Relaxed);
    update_and_save(|cfg| cfg.fast_pull_rate_ms = clamped);
    info!("Fast pull rate set to {}ms", clamped);
}

/// Set the slow-tier pull rate at runtime and persist to disk.
pub fn set_slow_pull_rate_ms(ms: u64) {
    let clamped = ms.min(10000);
    SLOW_PULL_RATE_MS.store(clamped, Ordering::Relaxed);
    update_and_save(|cfg| cfg.slow_pull_rate_ms = clamped);
    info!("Slow pull rate set to {}ms", clamped);
}

/// Set the paused state at runtime and persist to disk.
pub fn set_pull_paused(paused: bool) {
    PULL_PAUSED.store(paused, Ordering::Relaxed);
    update_and_save(|cfg| cfg.data_pull_paused = paused);
    info!("Data pull paused: {}", paused);
}

/// Set refresh-on-request at runtime and persist to disk.
pub fn set_refresh_on_request(enabled: bool) {
    REFRESH_ON_REQ.store(enabled, Ordering::Relaxed);
    update_and_save(|cfg| cfg.refresh_on_request = enabled);
    info!("Refresh on request: {}", enabled);
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
    FAST_PULL_RATE_MS.store(cfg.fast_pull_rate_ms.min(5000), Ordering::Relaxed);
    SLOW_PULL_RATE_MS.store(cfg.slow_pull_rate_ms.min(10000), Ordering::Relaxed);
    PULL_PAUSED.store(cfg.data_pull_paused, Ordering::Relaxed);
    REFRESH_ON_REQ.store(cfg.refresh_on_request, Ordering::Relaxed);

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
