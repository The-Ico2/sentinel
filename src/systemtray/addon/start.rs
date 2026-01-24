use std::process::{Stdio, Command, Child};

use crate::Addon;
use crate::{info, error};

pub fn start_addon(addon: &Addon) -> std::io::Result<Child> {
    info!("Starting addon '{}'", addon.name);

    // Ensure binary exists
    if !addon.exe_path.exists() {
        error!("Addon executable not found: {}", addon.exe_path.display());
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Addon executable not found: {}", addon.exe_path.display()),
        ));
    }

    match Command::new(&addon.exe_path)
        .current_dir(&addon.dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => {
            info!("Addon '{}' started successfully with PID {}", addon.name, child.id());
            Ok(child)
        }
        Err(e) => {
            error!("Failed to start addon '{}': {}", addon.name, e);
            Err(e)
        }
    }
}