// ~/sentinel/sentinel-backend/src/ipc/appdata/active_window.rs

use serde::Serialize;
use std::{collections::HashMap, path::PathBuf};
use sha2::{Digest, Sha256};
use windows::{
    Win32::{
        Foundation::{HWND, RECT},
        Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFOEXW, MONITOR_DEFAULTTONEAREST},
        UI::WindowsAndMessaging::{
            GetForegroundWindow, GetWindowLongW, GetWindowRect, GetWindowThreadProcessId,
            IsZoomed, GWL_STYLE, WS_CAPTION, WS_THICKFRAME,
        },
    },
};
use as_bool::AsBool;

use crate::{
    ipc::registry::RegistryEntry,
    info, warn, error,
};

#[derive(Serialize, Debug, Clone)]
pub struct ActiveWindowInfo {
    pub monitor_id: String,
    pub app_icon: String,
    pub app_name: String,
    pub exe_path: String,
    pub window_state: String,
}

pub struct ActiveWindowManager;

impl ActiveWindowManager {
    /// Enumerate active windows per monitor (focused window only)
    /// Automatically tracks previous windows internally to log changes without flooding logs
    pub fn enumerate_active_windows() -> Vec<RegistryEntry> {
        use std::sync::Mutex;

        // static store for previous active window names per monitor
        static PREV_WINDOWS: once_cell::sync::Lazy<Mutex<HashMap<String, String>>> =
            once_cell::sync::Lazy::new(|| Mutex::new(HashMap::new()));

        let mut prev_windows = PREV_WINDOWS.lock().unwrap();
        let mut results_map: HashMap<String, RegistryEntry> = HashMap::new();

        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.0 == std::ptr::null_mut() {
                if !prev_windows.is_empty() {
                    warn!("No foreground window detected");
                    prev_windows.clear();
                }
                return vec![];
            }

            if let Some(entry) = Self::window_to_monitor_info(hwnd) {
                let monitor_id = entry.metadata["monitor_id"]
                    .as_str()
                    .unwrap_or("unknown")
                    .to_string();
                let app_name = entry.metadata["app_name"].as_str().unwrap_or("unknown");

                // Only log if the active window changed
                if prev_windows.get(&monitor_id).map(|n| n.as_str()) != Some(app_name) {
                    info!("Active window on monitor {} changed to {}", monitor_id, app_name);
                    prev_windows.insert(monitor_id.clone(), app_name.to_string());
                }

                results_map.insert(monitor_id, entry);
            }
        }

        results_map.into_values().collect()
    }

    unsafe fn window_to_monitor_info(hwnd: HWND) -> Option<RegistryEntry> {
        let mut rect: RECT = std::mem::zeroed();
        let rect_ok = GetWindowRect(hwnd, &mut rect).as_bool();

        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        if monitor.0 == std::ptr::null_mut() {
            error!("MonitorFromWindow returned null for HWND={:?}", hwnd.0);
            return None;
        }

        let mut mi_ex: MONITORINFOEXW = std::mem::zeroed();
        mi_ex.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        if !GetMonitorInfoW(monitor, &mut mi_ex.monitorInfo).as_bool() {
            error!("GetMonitorInfoW failed for monitor HWND={:?}", hwnd.0);
            return None;
        }

        let monitor_id = {
            let mut hasher = Sha256::new();
            let device_name = String::from_utf16_lossy(
                &mi_ex.szDevice.iter().take_while(|c| **c != 0).cloned().collect::<Vec<_>>(),
            );
            hasher.update(device_name.as_bytes());
            hasher.update(mi_ex.monitorInfo.rcMonitor.left.to_le_bytes());
            hasher.update(mi_ex.monitorInfo.rcMonitor.top.to_le_bytes());
            hasher.update(mi_ex.monitorInfo.rcMonitor.right.to_le_bytes());
            hasher.update(mi_ex.monitorInfo.rcMonitor.bottom.to_le_bytes());
            format!("{:x}", hasher.finalize())
        };

        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));

        let exe_path = crate::utils::get_process_exe(pid).unwrap_or_else(|_| "".into());
        let friendly_name = crate::utils::get_process_name(pid).unwrap_or_else(|_| "".into());
        let app_name = if !friendly_name.is_empty() && friendly_name != "unknown" {
            friendly_name
        } else if !exe_path.is_empty() {
            exe_path.split(&['\\', '/'][..])
                    .last()
                    .unwrap_or("unknown")
                    .to_string()
        } else {
            "unknown".into()
        };

        let app_icon = if !exe_path.is_empty() {
            format!("{}\\icon.ico", exe_path)
        } else {
            "".into()
        };

        let maximized = IsZoomed(hwnd).as_bool();

        let (covers_monitor, covers_work) = if rect_ok {
            let monitor_rc = mi_ex.monitorInfo.rcMonitor;
            let work_rc = mi_ex.monitorInfo.rcWork;
            let epsilon = 1i32;

            let monitor_match = (rect.left - monitor_rc.left).abs() <= epsilon
                && (rect.top - monitor_rc.top).abs() <= epsilon
                && (rect.right - monitor_rc.right).abs() <= epsilon
                && (rect.bottom - monitor_rc.bottom).abs() <= epsilon;

            let work_match = (rect.left - work_rc.left).abs() <= epsilon
                && (rect.top - work_rc.top).abs() <= epsilon
                && (rect.right - work_rc.right).abs() <= epsilon
                && (rect.bottom - work_rc.bottom).abs() <= epsilon;

            (monitor_match, work_match)
        } else {
            (false, false)
        };

        let style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
        let has_frame = (style & (WS_CAPTION.0 | WS_THICKFRAME.0)) != 0;

        let fullscreen = covers_monitor && (!covers_work || !has_frame);

        let window_state = if fullscreen {
            "fullscreen"
        } else if maximized || covers_work {
            "maximized"
        } else {
            "normal"
        }
        .to_string();

        Some(RegistryEntry {
            id: format!("active_window_{}", monitor_id),
            category: "active_window".into(),
            subtype: "monitor".into(),
            metadata: serde_json::json!(ActiveWindowInfo {
                monitor_id,
                app_icon,
                app_name,
                exe_path: exe_path.clone(),
                window_state,
            }),
            path: PathBuf::new(),
            exe_path,
        })
    }

}