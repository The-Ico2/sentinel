// ~/sentinel/sentinel-backend/src/main.rs

#![windows_subsystem = "windows"]

mod logging;
mod custom;
mod cli;
mod paths;
mod ipc;
mod systemtray;
mod utils;

use crate::{
    cli::{run_cli, bootstrap_user_root},
    systemtray::systray::spawn_tray,
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

        // Start registry manager
        info!("Starting registry manager");
        registry_manager();

        // Start live sysdata/appdata updater (every 500ms)
        info!("Starting live data updater");
        crate::ipc::data_updater::start_registry_updater(Some(500));

        // Start IPC server in a separate thread
        info!("Spawning IPC server thread");
        std::thread::spawn(|| {
            info!("IPC server thread running");
            start_ipc_server();
            info!("IPC server thread terminated");
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
    let _instance_guard = match acquire_single_instance() {
        Some(handle) => handle,
        None => {
            return;
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
        unsafe {
            let _ = CloseHandle(_instance_guard);
        }
        return;
    }

    let daemon = SentinelDaemon::new();
    daemon.run();

    info!("Sentinel backend exiting");

    unsafe {
        let _ = CloseHandle(_instance_guard);
    }
}