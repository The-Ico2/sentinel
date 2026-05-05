// ~/veil/veil-backend/src/ipc/dispatch/controld.rs
//
// "control" IPC namespace — host-side utilities exposed to JS wallpapers.
//
// Commands:
//   write_log  { name: "<filename>", content: "<text>" }
//              Writes/overwrites a file inside ~/ProjectOpen/VEIL/logs/.
//              Only simple filenames are accepted (no path separators, no "..").

use serde_json::{json, Value};
use std::fs;
use crate::paths::veil_root_dir;

pub fn dispatch_control(cmd: &str, args: Option<Value>) -> Result<Value, String> {
    match cmd {
        "write_log" => {
            let args = args.ok_or_else(|| "write_log requires args { name, content }".to_string())?;

            let name = args["name"]
                .as_str()
                .ok_or_else(|| "Missing string field 'name'".to_string())?;

            let content = args["content"]
                .as_str()
                .ok_or_else(|| "Missing string field 'content'".to_string())?;

            // Security: reject any name containing path-traversal characters.
            if name.contains('/') || name.contains('\\') || name.contains("..") || name.is_empty() {
                return Err(format!("Invalid log filename: {:?}", name));
            }

            let log_dir = veil_root_dir().join("logs");
            fs::create_dir_all(&log_dir)
                .map_err(|e| format!("Could not create logs dir: {}", e))?;

            let path = log_dir.join(name);
            fs::write(&path, content)
                .map_err(|e| format!("Could not write log file: {}", e))?;

            crate::info!("[control] Wrote log file: {}", path.display());
            Ok(json!({ "path": path.to_string_lossy() }))
        }

        _ => Err(format!("Unknown control command: {}", cmd)),
    }
}
