// ~/sentinel/sentinel-backend/src/ipc/sysdata/display.rs

use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{collections::HashMap, mem::size_of, os::windows::process::CommandExt, process::Command};
use windows::{
    core::{BOOL, PCWSTR},
    Win32::{
        Foundation::LPARAM,
        Graphics::Gdi::{
            EnumDisplayDevicesW, EnumDisplayMonitors, EnumDisplaySettingsW, GetMonitorInfoW,
            DEVMODEW, DISPLAY_DEVICEW, HDC, HMONITOR, MONITORINFOEXW,
            ENUM_CURRENT_SETTINGS,
        },
        UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI},
    },
};

const CREATE_NO_WINDOW: u32 = 0x08000000;

#[derive(Serialize, Debug, Clone)]
pub struct MonitorInfo {
    pub id: String,
    pub primary: bool,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub scale: f32,
    pub dpi: u32,
    pub refresh_rate_hz: u32,
    pub color_depth_bits: u32,
    pub bits_per_channel: u32,
    pub orientation: String,
    pub aspect_ratio: String,
    pub device_name: String,
    pub monitor_name: String,
    pub connection_type: String,
    pub hdr_supported: bool,
    pub physical_width_mm: u32,
    pub physical_height_mm: u32,
    pub manufacturer: String,
    pub product_code: String,
    pub serial_number: String,
    pub year_of_manufacture: u32,
}

/// Parse EDID data from registry to extract monitor details
fn query_edid_monitors() -> Vec<(String, EdidInfo)> {
    let script = r#"$ErrorActionPreference='SilentlyContinue';
$monitors = Get-CimInstance -Namespace root\wmi -ClassName WmiMonitorID;
$conn = Get-CimInstance -Namespace root\wmi -ClassName WmiMonitorConnectionParams;
$bp = Get-CimInstance -Namespace root\wmi -ClassName WmiMonitorBasicDisplayParams;
foreach ($m in $monitors) {
    $inst = $m.InstanceName;
    $name = if ($m.UserFriendlyName) { ($m.UserFriendlyName | Where-Object {$_ -ne 0} | ForEach-Object {[char]$_}) -join '' } else { '' };
    $mfr = if ($m.ManufacturerName) { ($m.ManufacturerName | Where-Object {$_ -ne 0} | ForEach-Object {[char]$_}) -join '' } else { '' };
    $prod = $m.ProductCodeID;
    $serial = if ($m.SerialNumberID) { ($m.SerialNumberID | Where-Object {$_ -ne 0} | ForEach-Object {[char]$_}) -join '' } else { '' };
    $year = $m.YearOfManufacture;
    $c = $conn | Where-Object { $_.InstanceName -eq $inst };
    $connType = if ($c) { $c.VideoOutputTechnology } else { '' };
    $b = $bp | Where-Object { $_.InstanceName -eq $inst };
    $hSize = if ($b) { $b.MaxHorizontalImageSize } else { 0 };
    $vSize = if ($b) { $b.MaxVerticalImageSize } else { 0 };
    "InstanceName=$inst";
    "MonitorName=$name";
    "Manufacturer=$mfr";
    "ProductCode=$prod";
    "SerialNumber=$serial";
    "YearOfManufacture=$year";
    "VideoOutputTechnology=$connType";
    "HorizontalSizeCm=$hSize";
    "VerticalSizeCm=$vSize";
    "";
}
# HDR support from WmiMonitorBrightness or AdvancedColor
$adv = Get-CimInstance -Namespace root\wmi -ClassName WmiMonitorColorCharacteristics -EA SilentlyContinue;
if ($adv) {
    foreach ($a in $adv) {
        "HDR_Instance=$($a.InstanceName)";
    }
}
"#;

    let output = Command::new("powershell")
        .creation_flags(CREATE_NO_WINDOW)
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output();

    let Ok(output) = output else { return Vec::new() };
    if !output.status.success() { return Vec::new() }

    let text = String::from_utf8_lossy(&output.stdout);
    let mut result = Vec::<(String, EdidInfo)>::new();
    let mut fields = HashMap::<String, String>::new();

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            if let Some(inst) = fields.get("InstanceName").map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
                let info = EdidInfo {
                    monitor_name: fields.get("MonitorName").map(|s| s.trim().to_string()).unwrap_or_default(),
                    manufacturer: fields.get("Manufacturer").map(|s| s.trim().to_string()).unwrap_or_default(),
                    product_code: fields.get("ProductCode").map(|s| s.trim().to_string()).unwrap_or_default(),
                    serial_number: fields.get("SerialNumber").map(|s| s.trim().to_string()).unwrap_or_default(),
                    year_of_manufacture: fields.get("YearOfManufacture").and_then(|s| s.trim().parse().ok()).unwrap_or(0),
                    connection_type: fields.get("VideoOutputTechnology").and_then(|s| s.trim().parse::<i32>().ok())
                        .map(|v| match v {
                            0 => "VGA".to_string(),
                            1 => "S-Video".to_string(),
                            2 => "Composite".to_string(),
                            3 => "Component".to_string(),
                            4 => "DVI".to_string(),
                            5 => "HDMI".to_string(),
                            6 => "LVDS".to_string(),
                            8 => "D-Jpn".to_string(),
                            9 => "SDI".to_string(),
                            10 => "DisplayPort (ext)".to_string(),
                            11 => "DisplayPort (int)".to_string(),
                            12 => "UDI (ext)".to_string(),
                            13 => "UDI (int)".to_string(),
                            14 => "SDTV Dongle".to_string(),
                            15 => "Miracast".to_string(),
                            -2147483648 => "Internal".to_string(),
                            _ => format!("Other ({})", v),
                        }).unwrap_or_default(),
                    physical_width_mm: fields.get("HorizontalSizeCm").and_then(|s| s.trim().parse::<u32>().ok())
                        .map(|v| v * 10).unwrap_or(0),
                    physical_height_mm: fields.get("VerticalSizeCm").and_then(|s| s.trim().parse::<u32>().ok())
                        .map(|v| v * 10).unwrap_or(0),
                };
                // Match by device path: InstanceName contains the display device ID
                result.push((inst, info));
            }
            fields.clear();
            continue;
        }
        if let Some((key, val)) = line.split_once('=') {
            fields.insert(key.trim().to_string(), val.to_string());
        }
    }
    result
}

/// Check HDR capability via PowerShell AdvancedColorInfo
fn query_hdr_support() -> HashMap<String, bool> {
    let script = r#"$ErrorActionPreference='SilentlyContinue';
Add-Type -AssemblyName System.Runtime.WindowsRuntime 2>$null;
try {
    $displays = [Windows.Devices.Display.DisplayMonitor,Windows.Devices.Display,ContentType=WindowsRuntime]::GetType();
} catch {}
# Fallback: check registry for AdvancedColor
$acKey = 'HKLM:\SYSTEM\CurrentControlSet\Control\GraphicsDrivers\MonitorDataStore';
if (Test-Path $acKey) {
    Get-ChildItem $acKey -EA SilentlyContinue | ForEach-Object {
        $name = $_.PSChildName;
        $adv = (Get-ItemProperty $_.PSPath -Name 'AdvancedColorSupported' -EA SilentlyContinue).AdvancedColorSupported;
        if ($adv -eq 1) { "HDR=$name" };
    }
}
"#;
    let output = Command::new("powershell")
        .creation_flags(CREATE_NO_WINDOW)
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output();

    let Ok(output) = output else { return HashMap::new() };
    let text = String::from_utf8_lossy(&output.stdout);
    let mut result = HashMap::<String, bool>::new();
    for line in text.lines() {
        if let Some(name) = line.trim().strip_prefix("HDR=") {
            result.insert(name.to_string(), true);
        }
    }
    result
}

#[derive(Debug, Clone, Default)]
struct EdidInfo {
    monitor_name: String,
    manufacturer: String,
    product_code: String,
    serial_number: String,
    year_of_manufacture: u32,
    connection_type: String,
    physical_width_mm: u32,
    physical_height_mm: u32,
}

fn compute_aspect_ratio(w: i32, h: i32) -> String {
    if w <= 0 || h <= 0 { return String::new() }
    let gcd = gcd(w as u32, h as u32);
    let rw = w as u32 / gcd;
    let rh = h as u32 / gcd;
    format!("{}:{}", rw, rh)
}

fn gcd(mut a: u32, mut b: u32) -> u32 {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a
}

/// Try to get the monitor device name from EnumDisplayDevices for matching with EDID
fn get_monitor_device_ids() -> HashMap<String, String> {
    let mut result = HashMap::<String, String>::new();
    unsafe {
        let mut adapter_idx = 0u32;
        loop {
            let mut adapter_dev: DISPLAY_DEVICEW = std::mem::zeroed();
            adapter_dev.cb = size_of::<DISPLAY_DEVICEW>() as u32;
            if !EnumDisplayDevicesW(PCWSTR(std::ptr::null()), adapter_idx, &mut adapter_dev, 0).as_bool() {
                break;
            }
            let adapter_name = String::from_utf16_lossy(
                &adapter_dev.DeviceName.iter().take_while(|c| **c != 0).cloned().collect::<Vec<_>>()
            );

            let mut mon_idx = 0u32;
            loop {
                let mut mon_dev: DISPLAY_DEVICEW = std::mem::zeroed();
                mon_dev.cb = size_of::<DISPLAY_DEVICEW>() as u32;
                let mut adapter_name_utf16: Vec<u16> = adapter_name.encode_utf16().collect();
                adapter_name_utf16.push(0);
                if !EnumDisplayDevicesW(PCWSTR(adapter_name_utf16.as_ptr()), mon_idx, &mut mon_dev, 0).as_bool() {
                    break;
                }
                let mon_id = String::from_utf16_lossy(
                    &mon_dev.DeviceID.iter().take_while(|c| **c != 0).cloned().collect::<Vec<_>>()
                );
                result.insert(adapter_name.clone(), mon_id);
                mon_idx += 1;
            }
            adapter_idx += 1;
        }
    }
    result
}

/// Extract the hardware ID portion from a monitor path.
/// E.g. "MONITOR\\GSM5BBF\\{guid}" → "GSM5BBF"
///      "DISPLAY\\GSM5BBF\\5&1234..." → "GSM5BBF"
fn extract_hw_id(path: &str) -> String {
    let parts: Vec<&str> = path.split('\\').collect();
    if parts.len() >= 2 {
        parts[1].to_string()
    } else {
        String::new()
    }
}

pub struct MonitorManager;

impl MonitorManager {
    pub fn enumerate_monitors() -> Vec<MonitorInfo> {
        // Query EDID info and monitor device IDs
        let edid_data = query_edid_monitors();
        let monitor_device_ids = get_monitor_device_ids();
        let _hdr_map = query_hdr_support();

        unsafe extern "system" fn callback(
            hmonitor: HMONITOR,
            _hdc: HDC,
            _rect: *mut windows::Win32::Foundation::RECT,
            lparam: LPARAM,
        ) -> BOOL {
            let ctx = &mut *(lparam.0 as *mut MonitorEnumContext);

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

                let refresh_rate = devmode.dmDisplayFrequency;
                let color_depth = devmode.dmBitsPerPel;
                let bits_per_channel = color_depth / 3; // Approximate (e.g., 32bpp -> 10, 24bpp -> 8)
                let orientation = match devmode.Anonymous1.Anonymous2.dmDisplayOrientation.0 {
                    0 => "landscape",
                    1 => "portrait",
                    2 => "landscape_flipped",
                    3 => "portrait_flipped",
                    _ => "unknown",
                };

                let derived_scale = (physical_width as f32 / logical_width as f32)
                    .max(physical_height as f32 / logical_height as f32);
                if derived_scale.is_finite() && derived_scale > 0.0 {
                    scale = derived_scale;
                }

                let physical_x = (rc.left as f32 * scale).round() as i32;
                let physical_y = (rc.top as f32 * scale).round() as i32;

                let aspect_ratio = compute_aspect_ratio(physical_width, physical_height);

                // Match EDID data by looking up the monitor's device ID.
                // EnumDisplayDevices gives us a DeviceID like:
                //   MONITOR\\GSM5BBF\\{guid}
                // WMI InstanceName looks like:
                //   DISPLAY\\GSM5BBF\\5&1234...\\0_0_...
                // We extract the hardware portion (e.g. \"GSM5BBF\") for matching.
                let mon_device_id = ctx.monitor_device_ids.get(&device_name).cloned().unwrap_or_default();
                let hw_id_part = extract_hw_id(&mon_device_id);
                let mut matched_edid: Option<&EdidInfo> = None;
                let mut matched_idx: Option<usize> = None;

                if !hw_id_part.is_empty() {
                    for (i, (inst, info)) in ctx.edid_data.iter().enumerate() {
                        if ctx.used_edid_indices.contains(&i) { continue; }
                        let inst_hw = extract_hw_id(inst);
                        if !inst_hw.is_empty() && inst_hw.eq_ignore_ascii_case(&hw_id_part) {
                            matched_edid = Some(info);
                            matched_idx = Some(i);
                            break;
                        }
                    }
                }

                // Fallback: match unused EDID entries by order
                if matched_edid.is_none() {
                    for (i, (_inst, info)) in ctx.edid_data.iter().enumerate() {
                        if !ctx.used_edid_indices.contains(&i) {
                            matched_edid = Some(info);
                            matched_idx = Some(i);
                            break;
                        }
                    }
                }

                if let Some(idx) = matched_idx {
                    ctx.used_edid_indices.push(idx);
                }

                let edid = matched_edid.cloned().unwrap_or_default();

                let mut hasher = Sha256::new();
                hasher.update(device_name.as_bytes());
                hasher.update(rc.left.to_le_bytes());
                hasher.update(rc.top.to_le_bytes());
                hasher.update(rc.right.to_le_bytes());
                hasher.update(rc.bottom.to_le_bytes());
                let id = format!("{:x}", hasher.finalize());

                ctx.monitors.push(MonitorInfo {
                    id,
                    primary,
                    x: physical_x,
                    y: physical_y,
                    width: physical_width,
                    height: physical_height,
                    scale,
                    dpi: dpi_x,
                    refresh_rate_hz: refresh_rate,
                    color_depth_bits: color_depth,
                    bits_per_channel,
                    orientation: orientation.to_string(),
                    aspect_ratio,
                    device_name: device_name.clone(),
                    monitor_name: edid.monitor_name,
                    connection_type: edid.connection_type,
                    hdr_supported: false,
                    physical_width_mm: edid.physical_width_mm,
                    physical_height_mm: edid.physical_height_mm,
                    manufacturer: edid.manufacturer,
                    product_code: edid.product_code,
                    serial_number: edid.serial_number,
                    year_of_manufacture: edid.year_of_manufacture,
                });
            }
            BOOL(1)
        }

        struct MonitorEnumContext {
            monitors: Vec<MonitorInfo>,
            edid_data: Vec<(String, EdidInfo)>,
            used_edid_indices: Vec<usize>,
            monitor_device_ids: HashMap<String, String>,
        }

        let mut ctx = MonitorEnumContext {
            monitors: Vec::new(),
            edid_data: edid_data,
            used_edid_indices: Vec::new(),
            monitor_device_ids: monitor_device_ids,
        };

        unsafe {
            let _ = EnumDisplayMonitors(None, None, Some(callback), LPARAM(&mut ctx as *mut _ as isize));
        }
        ctx.monitors
    }
}