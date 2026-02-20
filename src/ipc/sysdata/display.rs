// ~/sentinel/sentinel-backend/src/ipc/sysdata/display.rs

use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{mem::size_of};
use windows::{
    core::{BOOL, PCWSTR},
    Win32::{
        Foundation::LPARAM,
        Graphics::Gdi::{
            EnumDisplayMonitors, EnumDisplaySettingsW, GetMonitorInfoW, DEVMODEW, HDC, HMONITOR,
            MONITORINFOEXW, ENUM_CURRENT_SETTINGS,
        },
        UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI},
    },
};

#[derive(Serialize, Debug, Clone)]
pub struct MonitorInfo {
    pub id: String,
    pub primary: bool,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub scale: f32,
}

pub struct MonitorManager;

impl MonitorManager {
    pub fn enumerate_monitors() -> Vec<MonitorInfo> {
        let mut monitors: Vec<MonitorInfo> = Vec::new();

        unsafe extern "system" fn callback(
            hmonitor: HMONITOR,
            _hdc: HDC,
            _rect: *mut windows::Win32::Foundation::RECT,
            lparam: LPARAM,
        ) -> BOOL {
            let list = &mut *(lparam.0 as *mut Vec<MonitorInfo>);

            let mut mi_ex: MONITORINFOEXW = std::mem::zeroed();
            mi_ex.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;

            if GetMonitorInfoW(hmonitor, &mut mi_ex.monitorInfo).as_bool() {
                let rc = mi_ex.monitorInfo.rcMonitor;
                let primary = mi_ex.monitorInfo.dwFlags & 1 != 0;
                let logical_width = (rc.right - rc.left).max(1);
                let logical_height = (rc.bottom - rc.top).max(1);

                let mut dpi_x = 96u32;
                let mut dpi_y = 96u32;
                let mut scale = if GetDpiForMonitor(hmonitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y).is_ok() {
                    dpi_x as f32 / 96.0
                } else {
                    1.0
                };

                let device_name = String::from_utf16_lossy(
                    &mi_ex.szDevice
                        .iter()
                        .take_while(|c| **c != 0)
                        .cloned()
                        .collect::<Vec<_>>()
                );

                let mut physical_width = logical_width;
                let mut physical_height = logical_height;

                let mut devmode: DEVMODEW = std::mem::zeroed();
                devmode.dmSize = size_of::<DEVMODEW>() as u16;

                let mut device_utf16: Vec<u16> = device_name.encode_utf16().collect();
                device_utf16.push(0);

                if EnumDisplaySettingsW(
                    PCWSTR(device_utf16.as_ptr()),
                    ENUM_CURRENT_SETTINGS,
                    &mut devmode,
                )
                .as_bool()
                {
                    if devmode.dmPelsWidth > 0 {
                        physical_width = devmode.dmPelsWidth as i32;
                    }
                    if devmode.dmPelsHeight > 0 {
                        physical_height = devmode.dmPelsHeight as i32;
                    }
                }

                let derived_scale = (physical_width as f32 / logical_width as f32)
                    .max(physical_height as f32 / logical_height as f32);
                if derived_scale.is_finite() && derived_scale > 0.0 {
                    scale = derived_scale;
                }

                let physical_x = (rc.left as f32 * scale).round() as i32;
                let physical_y = (rc.top as f32 * scale).round() as i32;

                let mut hasher = Sha256::new();
                hasher.update(device_name.as_bytes()); // \\.\DISPLAY1
                hasher.update(rc.left.to_le_bytes());
                hasher.update(rc.top.to_le_bytes());
                hasher.update(rc.right.to_le_bytes());
                hasher.update(rc.bottom.to_le_bytes());
                let id = format!("{:x}", hasher.finalize());

                list.push(MonitorInfo {
                    id,
                    primary,
                    x: physical_x,
                    y: physical_y,
                    width: physical_width,
                    height: physical_height,
                    scale,
                });
            }
            BOOL(1)
        }

        unsafe {
            let _ = EnumDisplayMonitors(None, None, Some(callback), LPARAM(&mut monitors as *mut _ as isize));
        }
        monitors
    }
}