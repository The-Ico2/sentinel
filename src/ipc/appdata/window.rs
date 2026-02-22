// ~/sentinel/sentinel-backend/src/ipc/appdata/window.rs

use serde::Serialize;
use std::{
    cell::RefCell,
    collections::HashMap,
    path::PathBuf,
    sync::Mutex,
};
use sha2::{Digest, Sha256};
use windows::{
    core::BOOL,
    Win32::{
        Foundation::{HWND, LPARAM, RECT},
        Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFOEXW, MONITOR_DEFAULTTONEAREST},
        UI::WindowsAndMessaging::{
            EnumWindows, GetForegroundWindow, GetWindow, GetWindowLongW, GetWindowRect,
            GetWindowTextLengthW, GetWindowTextW,
            GetWindowThreadProcessId, IsIconic, IsWindowVisible, IsZoomed,
            GWL_EXSTYLE, GWL_STYLE, GW_OWNER, WS_CAPTION, WS_EX_TOOLWINDOW, WS_THICKFRAME,
        },
    },
};

use crate::{
    ipc::registry::RegistryEntry,
    info, warn, error,
};

#[derive(Serialize, Debug, Clone)]
pub struct ActiveWindowInfo {
    pub monitor_id: String,
    pub focused: bool,
    pub app_icon: String,
    pub app_name: String,
    pub exe_path: String,
    pub window_title: String,
    pub pid: u32,
    pub window_state: String,
    pub size: WindowSize,
    pub position: WindowPosition,
}

#[derive(Serialize, Debug, Clone)]
pub struct WindowSize {
    pub width: i32,
    pub height: i32,
}

#[derive(Serialize, Debug, Clone)]
pub struct WindowPosition {
    pub x: i32,
    pub y: i32,
}

pub struct ActiveWindowManager;

impl ActiveWindowManager {
    /// Enumerate all visible, non-minimized windows and map each to its nearest monitor.
    /// Focused window is tagged through metadata.focused.
    pub fn enumerate_active_windows() -> Vec<RegistryEntry> {
        static PREV_FOCUSED_WINDOW: once_cell::sync::Lazy<Mutex<HashMap<String, String>>> =
            once_cell::sync::Lazy::new(|| Mutex::new(HashMap::new()));

        let mut prev_focused = PREV_FOCUSED_WINDOW.lock().unwrap();
        let mut results = Vec::new();

        unsafe {
            let focused_hwnd = GetForegroundWindow();

            if focused_hwnd.0 == std::ptr::null_mut() {
                if !prev_focused.is_empty() {
                    warn!("No foreground window detected");
                    prev_focused.clear();
                }
            }

            let hwnds = Self::enumerate_candidate_windows();
            if hwnds.is_empty() && focused_hwnd.0 != std::ptr::null_mut() {
                if let Some(entry) = Self::window_to_monitor_info(focused_hwnd, focused_hwnd) {
                    let monitor_id = entry.metadata["monitor_id"]
                        .as_str()
                        .unwrap_or("unknown")
                        .to_string();
                    let app_name = entry.metadata["app_name"].as_str().unwrap_or("unknown");

                    if prev_focused.get(&monitor_id).map(|n| n.as_str()) != Some(app_name) {
                        info!("Focused window on monitor {} changed to {}", monitor_id, app_name);
                        prev_focused.insert(monitor_id.clone(), app_name.to_string());
                    }

                    results.push(entry);
                }
                return results;
            }

            for hwnd in hwnds {
                if let Some(entry) = Self::window_to_monitor_info(hwnd, focused_hwnd) {
                    if entry
                        .metadata
                        .get("focused")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false)
                    {
                        let monitor_id = entry.metadata["monitor_id"]
                            .as_str()
                            .unwrap_or("unknown")
                            .to_string();
                        let app_name = entry.metadata["app_name"].as_str().unwrap_or("unknown");

                        if prev_focused.get(&monitor_id).map(|n| n.as_str()) != Some(app_name) {
                            info!("Focused window on monitor {} changed to {}", monitor_id, app_name);
                            prev_focused.insert(monitor_id.clone(), app_name.to_string());
                        }
                    }

                    results.push(entry);
                }
            }
        }

        results
    }

    unsafe fn enumerate_candidate_windows() -> Vec<HWND> {
        thread_local! {
            static ENUM_HANDLES: RefCell<Vec<HWND>> = const { RefCell::new(Vec::new()) };
        }

        unsafe extern "system" fn enum_proc(hwnd: HWND, _lparam: LPARAM) -> BOOL {
            if hwnd.0 == std::ptr::null_mut() {
                return BOOL::from(true);
            }

            if IsWindowVisible(hwnd).0 == 0 || IsIconic(hwnd).0 != 0 {
                return BOOL::from(true);
            }

            let owner = GetWindow(hwnd, GW_OWNER).ok();
            if owner.map(|h| h.0 != std::ptr::null_mut()).unwrap_or(false) {
                return BOOL::from(true);
            }

            let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
            if (ex_style & WS_EX_TOOLWINDOW.0) != 0 {
                return BOOL::from(true);
            }

            if GetWindowTextLengthW(hwnd) <= 0 {
                return BOOL::from(true);
            }

            let mut rect: RECT = std::mem::zeroed();
            if GetWindowRect(hwnd, &mut rect).is_err() {
                return BOOL::from(true);
            }

            let width = rect.right - rect.left;
            let height = rect.bottom - rect.top;
            if width <= 0 || height <= 0 {
                return BOOL::from(true);
            }

            ENUM_HANDLES.with(|handles| handles.borrow_mut().push(hwnd));
            BOOL::from(true)
        }

        ENUM_HANDLES.with(|handles| handles.borrow_mut().clear());
        let _ = EnumWindows(Some(enum_proc), LPARAM(0));
        ENUM_HANDLES.with(|handles| handles.borrow().clone())
    }

    unsafe fn window_to_monitor_info(hwnd: HWND, focused_hwnd: HWND) -> Option<RegistryEntry> {
        let mut rect: RECT = std::mem::zeroed();
        let rect_ok = GetWindowRect(hwnd, &mut rect).is_ok();

        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        if monitor.0 == std::ptr::null_mut() {
            error!("MonitorFromWindow returned null for HWND={:?}", hwnd.0);
            return None;
        }

        let mut mi_ex: MONITORINFOEXW = std::mem::zeroed();
        mi_ex.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        if GetMonitorInfoW(monitor, &mut mi_ex.monitorInfo).0 == 0 {
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

        // Get window title
        let title_len = GetWindowTextLengthW(hwnd);
        let window_title = if title_len > 0 {
            let mut buf = vec![0u16; (title_len + 1) as usize];
            let len = GetWindowTextW(hwnd, &mut buf);
            String::from_utf16_lossy(&buf[..len as usize])
        } else {
            String::new()
        };

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

        let app_name_lower = app_name.to_ascii_lowercase();
        if matches!(
            app_name_lower.as_str(),
            "textinputhost.exe"
                | "searchhost.exe"
                | "shellexperiencehost.exe"
                | "startmenuexperiencehost.exe"
                | "applicationframehost.exe"
                | "systemsettings.exe"
        ) {
            return None;
        }

        let app_icon = if !exe_path.is_empty() {
            format!("{}\\icon.ico", exe_path)
        } else {
            "".into()
        };

        let maximized = IsZoomed(hwnd).0 != 0;

        let covers_monitor = if rect_ok {
            let monitor_rc = mi_ex.monitorInfo.rcMonitor;
            let epsilon = 1i32;

            let monitor_match = (rect.left - monitor_rc.left).abs() <= epsilon
                && (rect.top - monitor_rc.top).abs() <= epsilon
                && (rect.right - monitor_rc.right).abs() <= epsilon
                && (rect.bottom - monitor_rc.bottom).abs() <= epsilon;

            monitor_match
        } else {
            false
        };

        let style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
        let has_frame = (style & (WS_CAPTION.0 | WS_THICKFRAME.0)) != 0;

        let fullscreen = covers_monitor && !maximized && !has_frame;

        let window_state = if fullscreen {
            "fullscreen"
        } else if maximized {
            "maximized"
        } else {
            "normal"
        }
        .to_string();

        Some(RegistryEntry {
            id: format!("active_window_{}_{}", monitor_id, hwnd.0 as usize),
            category: "active_window".into(),
            subtype: "monitor".into(),
            metadata: serde_json::json!(ActiveWindowInfo {
                monitor_id,
                focused: hwnd.0 == focused_hwnd.0,
                app_icon,
                app_name,
                exe_path: exe_path.clone(),
                window_title,
                pid,
                window_state,
                size: WindowSize {
                    width: (rect.right - rect.left).max(0),
                    height: (rect.bottom - rect.top).max(0),
                },
                position: WindowPosition {
                    x: rect.left,
                    y: rect.top,
                },
            }),
            path: PathBuf::new(),
            exe_path,
        })
    }
}