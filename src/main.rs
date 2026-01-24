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
    cli::run_cli,
    systemtray::systray::spawn_tray,
    ipc::{
        server::start_ipc_server,
        registry::registry_manager,
    },
};

use std::path::PathBuf;

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

        // Start live sysdata/appdata updater (every 250ms)
        info!("Starting live data updater");
        crate::ipc::data_updater::start_registry_updater(Some(250));

        // Start IPC server in a separate thread
        info!("Spawning IPC server thread");
        std::thread::spawn(|| {
            info!("IPC server thread running");
            start_ipc_server();
            info!("IPC server thread terminated");
        });

        // Start CLI bridge
        info!("Starting CLI bridge");
        match run_cli() {
            Ok(_) => info!("CLI bridge exited normally"),
            Err(e) => error!("CLI bridge error: {e}"),
        }

        // Start system tray
        info!("Starting system tray");
        spawn_tray();
        info!("System tray initialized");
    }
}

fn main() {
    // Enable logging at startup
    logging::init(true);
    info!("Sentinel backend starting");

    let daemon = SentinelDaemon::new();
    daemon.run();

    info!("Sentinel backend exiting");
}