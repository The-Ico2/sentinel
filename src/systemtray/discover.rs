// ~/sentinel/sentinel-backend/src/systemtray/discover.rs

// use serde::{Deserialize, Serialize};
use crate::ipc::request::{IpcRequest, send_ipc_request};
use crate::Addon;
use crate::{info, warn, error};

// #[derive(Debug, Clone, Serialize, Deserialize)]
// pub struct TrayAddon {
//     pub id: String,
//     pub name: Option<String>,
//     pub version: Option<String>,
//     pub path: String,
// }

pub fn discover_addons() -> Vec<Addon> {
    info!("Discovering addons via IPC registry request");

    let request = IpcRequest {
        ns: "registry".into(),
        cmd: "list_addons".into(),
        args: None,
    };

    let response = match send_ipc_request(request) {
        Ok(r) => {
            info!("IPC request successful");
            r
        }
        Err(e) => {
            error!("Failed to send IPC request for addons: {:?}", e);
            return Vec::new();
        }
    };

    let entries = match response.data {
        Some(v) => v,
        None => {
            warn!("IPC response contained no data for addons");
            return Vec::new();
        }
    };

    let array = match entries.as_array() {
        Some(a) => a,
        None => {
            warn!("IPC response data is not an array; cannot discover addons");
            return Vec::new();
        }
    };

    let addons: Vec<Addon> = array
        .iter()
        .map(|entry| {
            let name = entry["metadata"]["name"].as_str().unwrap_or("unknown").to_string();
            let exe_path = entry["exe_path"].as_str().unwrap_or_default().into();
            let dir = entry["path"].as_str().unwrap_or_default().into();
            let package = entry["id"].as_str().unwrap_or_default().to_string();

            info!("Discovered addon: '{}' [{}]", name, package);

            Addon {
                name,
                exe_path,
                dir,
                package,
            }
        })
        .collect();

    info!("Total addons discovered: {}", addons.len());
    addons
}