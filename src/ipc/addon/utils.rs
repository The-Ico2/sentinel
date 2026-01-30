// ~/sentinel/sentinel-backend/src/ipc/addon/utils.rs

use std::path::PathBuf;
use crate::Addon;

pub fn registry_entry_to_addon(entry: &crate::ipc::registry::RegistryEntry) -> Result<Addon, String> {
    let name = entry.id.clone();
    let exe_path = PathBuf::from(&entry.exe_path);
    let dir = exe_path.parent().ok_or("Invalid exe path")?.to_path_buf();
    let package = entry.metadata.get("package")
        .and_then(|v| v.as_str())
        .ok_or("Missing package in metadata")?
        .to_string();
    
    Ok(Addon {
        name,
        exe_path,
        dir,
        package,
    })
}
