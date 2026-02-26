// ~/sentinel/sentinel-backend/src/main.rs

#![windows_subsystem = "windows"]

mod logging;
mod cli;
mod paths;
mod ipc;
mod systemtray;
mod utils;
mod config_ui;
mod config;

use crate::{
    cli::{run_cli, bootstrap_user_root},
    systemtray::systray::{spawn_tray, start_configured_autostart_addons},
    ipc::{
        server::start_ipc_server,
        registry::registry_manager,
    },
};

use std::path::PathBuf;
use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::{CloseHandle, GetLastError, HANDLE, ERROR_ALREADY_EXISTS},
        System::Threading::CreateMutexW,
    },
};

#[derive(Clone)]
pub struct Addon { 
    name: String,
    exe_path: PathBuf,
    dir: PathBuf,
    package: String,
}

pub struct SentinelDaemon {
    // Core backend components
    // registry: (),
    // ipc: (),
}

impl SentinelDaemon {
    pub fn new() -> Self {
        info!("Initializing SentinelDaemon backend components");
        Self {
            // registry: (),
            // ipc: (),
        }
    }

    pub fn run(&self) {
        info!("Starting SentinelDaemon");
        info!("Loading backend config");

        let cfg = crate::config::load_config();

        info!("Data pull rates: fast={}ms slow={}ms, paused: {}, refresh_on_request: {}",
            cfg.fast_pull_rate_ms, cfg.slow_pull_rate_ms, cfg.data_pull_paused, cfg.refresh_on_request);

        // 1. Quick registry init — discovers addons/assets only (< 100ms)
        info!("Starting registry manager");
        registry_manager();

        // 2. IPC server up immediately so tray & addons can connect
        info!("Spawning IPC server thread");
        std::thread::spawn(|| {
            info!("IPC server thread running");
            start_ipc_server();
            info!("IPC server thread terminated");
        });

        // 3. Data updater threads populate sysdata in the background
        info!("Starting live data updater");
        crate::ipc::data_updater::start_registry_updater();

        info!("Starting configured addon autostarts (background)");

        std::thread::spawn(|| {
            start_configured_autostart_addons();
        });

        // Start system tray
        info!("Starting system tray");
        spawn_tray();
        info!("System tray initialized");
    }
}

fn acquire_single_instance() -> Option<HANDLE> {
    let mut name: Vec<u16> = "Global\\SentinelBackendSingleton"
        .encode_utf16()
        .collect();
    name.push(0);

    unsafe {
        let mutex = CreateMutexW(None, false, PCWSTR(name.as_ptr())).ok()?;
        let already_exists = GetLastError() == ERROR_ALREADY_EXISTS;
        if already_exists {
            let _ = CloseHandle(mutex);
            return None;
        }
        Some(mutex)
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let is_ui_mode = args
        .iter()
        .any(|a| a == "--addon-config-ui" || a == "--sentinel-ui" || a == "--addon-webview");

    let instance_guard = if is_ui_mode {
        None
    } else {
        match acquire_single_instance() {
            Some(handle) => Some(handle),
            None => {
                return;
            }
        }
    };

    // Enable logging at startup
    logging::init(true);
    info!("Sentinel backend starting");

    bootstrap_user_root();

    if std::env::args().count() > 1 {
        info!("CLI mode detected");
        if let Err(e) = run_cli() {
            error!("CLI bridge error: {e}");
        }
        if let Some(handle) = instance_guard {
            unsafe {
                let _ = CloseHandle(handle);
            }
        }
        return;
    }

    let daemon = SentinelDaemon::new();
    daemon.run();

    info!("Sentinel backend exiting");

    if let Some(handle) = instance_guard {
        unsafe {
            let _ = CloseHandle(handle);
        }
    }
}