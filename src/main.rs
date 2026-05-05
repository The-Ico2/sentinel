// ~/veil/veil-backend/src/main.rs

#![windows_subsystem = "windows"]

mod logging;
mod cli;
mod paths;
mod ipc;
mod autostart;
mod utils;
mod config_ui;
mod config;
mod ui;
pub mod installer;

use crate::{
    cli::{run_cli, bootstrap_user_root},
    autostart::{start_configured_autostart_addons, ensure_user_config_dirs},
    ipc::{
        server::start_ipc_server,
        registry::registry_manager,
    },
};

use std::path::PathBuf;
use std::time::Duration;
use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::{CloseHandle, GetLastError, HANDLE, ERROR_ALREADY_EXISTS},
        System::Threading::CreateMutexW,
        UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2},
    },
};

#[derive(Clone)]
pub struct Addon { 
    name: String,
    exe_path: PathBuf,
    dir: PathBuf,
    package: String,
}

pub struct ODDaemon {
    // Core backend components
    // registry: (),
    // ipc: (),
}

impl ODDaemon {
    pub fn new() -> Self {
        info!("Initializing ODDaemon backend components");
        Self {
            // registry: (),
            // ipc: (),
        }
    }

    pub fn run(&self) {
        info!("Starting ODDaemon");
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

        // 2b. HTTP bridge for browser-based wallpaper prototyping
        info!("Spawning HTTP bridge thread");
        std::thread::spawn(|| {
            crate::ipc::http_bridge::start_http_bridge();
        });

        // 3. Data updater threads populate sysdata in the background
        info!("Starting live data updater");
        crate::ipc::data_updater::start_registry_updater();

        info!("Starting configured addon autostarts (background)");

        std::thread::spawn(|| {
            start_configured_autostart_addons();
        });

        // Ensure user config directories exist
        ensure_user_config_dirs();

        // Auto-launch the OpenRender UI process (owns the system tray).
        // The UI starts hidden — the tray icon appears immediately and the
        // user can double-click it to show the window.
        info!("Launching VEIL UI process (tray host)");
        match std::env::current_exe() {
            Ok(exe) => {
                match std::process::Command::new(&exe)
                    .arg("--veil-ui")
                    .spawn()
                {
                    Ok(child) => info!("UI process started (PID {})", child.id()),
                    Err(e) => error!("Failed to start UI process: {}", e),
                }
            }
            Err(e) => error!("Failed to resolve executable for UI launch: {}", e),
        }

        // Block main thread — the daemon stays alive until the process is killed.
        // The system tray is now managed by the OpenRender UI process.
        info!("Daemon running (tray managed by UI process)");
        loop {
            std::thread::sleep(Duration::from_secs(3600));
        }
    }
}

fn acquire_single_instance() -> Option<HANDLE> {
    let mut name: Vec<u16> = "Global\\VEILBackendSingleton"
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
    // Enable per-monitor DPI awareness so GetCursorPos, GetSystemMetrics, and
    // all display coordinates use physical pixels — matching the coordinate
    // space of DPI-aware addons (e.g. wallpaper).  Without this, cursor
    // positions are virtualised by Windows on non-primary monitors with
    // different DPI, causing wallpaper cursor trails to drift.
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }

    // Run self-install/bootstrap before singleton acquisition so a relaunch
    // from ~/ProjectOpen/VEIL/VEIL.exe is not blocked by this process mutex.
    bootstrap_user_root();

    let args: Vec<String> = std::env::args().collect();
    let is_ui_mode = args
        .iter()
        .any(|a| a == "--addon-config-ui" || a == "--veil-ui" || a == "--addon-webview");

    // `--no-backend` (alias `--ui-only`): launch JUST the PRISM-managed UI
    // (window, scene graph, system tray) without spinning up the IPC server,
    // HTTP bridge, data updaters, addon autostart, etc. Useful for quickly
    // iterating on UI changes or running VEIL on a system where backend
    // services would conflict with another running instance.
    let no_backend = args.iter().any(|a| a == "--no-backend" || a == "--ui-only");

    // Enable logging before the singleton check so a silent exit is observable.
    logging::init("VEIL", "Core", true);
    info!("VEIL backend starting (args={:?})", &args[1..]);

    let instance_guard = if is_ui_mode {
        None
    } else {
        match acquire_single_instance() {
            Some(handle) => Some(handle),
            None => {
                info!("Another VEIL backend instance already holds the singleton mutex — exiting.");
                return;
            }
        }
    };

    if no_backend && !is_ui_mode {
        info!("--no-backend flag detected: launching UI directly without backend services");
        if let Err(e) = crate::ui::launch() {
            error!("UI launch failed: {e}");
        }
        if let Some(handle) = instance_guard {
            unsafe { let _ = CloseHandle(handle); }
        }
        return;
    }

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

    let daemon = ODDaemon::new();
    daemon.run();

    info!("VEIL backend exiting");

    if let Some(handle) = instance_guard {
        unsafe {
            let _ = CloseHandle(handle);
        }
    }
}