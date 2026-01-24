use std::ptr::null_mut;
use windows::{
    core::{Result, PCWSTR},
    Win32::{
        Foundation::HWND,
        UI::WindowsAndMessaging::*,
        System::Registry::*,
    },
};

use crate::{info, warn};

//
// ---------------- TASKBAR ----------------
//

#[derive(Clone)]
pub struct TaskbarController {
    hwnd: Option<HWND>,
}

impl TaskbarController {
    pub fn new() -> Self {
        let hwnd = unsafe {
            FindWindowW(windows::core::w!("Shell_TrayWnd"), None).ok()
        };
        if hwnd.is_some() {
            info!("[WindowsC][Taskbar] Found Shell_TrayWnd window");
        } else {
            warn!("[WindowsC][Taskbar] Failed to find Shell_TrayWnd window");
        }
        Self { hwnd }
    }

    pub fn hide(&self) {
        if let Some(hwnd) = self.hwnd {
            unsafe { ShowWindow(hwnd, SW_HIDE); }
            info!("[WindowsC][Taskbar] Taskbar hidden");
        }
    }

    pub fn show(&self) {
        if let Some(hwnd) = self.hwnd {
            unsafe { ShowWindow(hwnd, SW_SHOW); }
            info!("[WindowsC][Taskbar] Taskbar shown");
        }
    }

    pub fn toggle(&self) {
        if let Some(hwnd) = self.hwnd {
            unsafe {
                let visible = IsWindowVisible(hwnd).as_bool();
                ShowWindow(hwnd, if visible { SW_HIDE } else { SW_SHOW });
                info!("[WindowsC][Taskbar] Taskbar toggled: now {}", if visible { "hidden" } else { "visible" });
            }
        }
    }
}

//
// ---------------- REGISTRY HELPERS ----------------
//

fn set_reg_dword(path: &str, name: &str, value: u32) -> Result<()> {
    unsafe {
        let (_p_buf, path_w) = to_pcwstr(path);
        let (_n_buf, name_w) = to_pcwstr(name);

        let mut key = HKEY::default();
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            path_w,
            Some(0),
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut key,
            Some(null_mut()),
        )?;
        RegSetValueExW(
            key,
            name_w,
            Some(0),
            REG_DWORD,
            Some(&value.to_le_bytes()),
        )?;
        info!("[WindowsC][Registry] Set DWORD {}={} in {}", name, value, path);
        Ok(())
    }
}

fn to_pcwstr(s: &str) -> (Vec<u16>, PCWSTR) {
    let wide: Vec<u16> = s.encode_utf16().chain(Some(0)).collect();
    let pcwstr = PCWSTR(wide.as_ptr());
    (wide, pcwstr)
}

//
// ---------------- WINDOWS CUSTOMIZER ----------------
//

pub struct WindowsCManager {
    pub taskbar: TaskbarController,
}

impl WindowsCManager {
    pub fn new() -> Self {
        Self {
            taskbar: TaskbarController::new(),
        }
    }

    //
    // ---- Transparency ----
    //

    pub fn set_transparency(&self, enabled: bool) -> Result<()> {
        info!("[WindowsC] Setting transparency: {}", enabled);
        set_reg_dword(
            r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
            "EnableTransparency",
            enabled as u32,
        )
    }

    //
    // ---- Light / Dark Mode ----
    //

    pub fn set_dark_mode(&self, enabled: bool) -> Result<()> {
        info!("[WindowsC] Setting dark mode: {}", enabled);
        let value = if enabled { 0 } else { 1 };

        set_reg_dword(
            r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
            "AppsUseLightTheme",
            value,
        )?;

        set_reg_dword(
            r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
            "SystemUsesLightTheme",
            value,
        )
    }

    //
    // ---- Accent Color ----
    //

    pub fn set_accent_color(&self, argb: u32) -> Result<()> {
        info!("[WindowsC] Setting accent color: 0x{:08X}", argb);
        set_reg_dword(
            r"Software\Microsoft\Windows\DWM",
            "AccentColor",
            argb,
        )
    }

    //
    // ---- Titlebar & Borders ----
    //

    pub fn enable_colored_titlebars(&self, enabled: bool) -> Result<()> {
        info!("[WindowsC] Setting colored titlebars: {}", enabled);
        set_reg_dword(
            r"Software\Microsoft\Windows\DWM",
            "ColorPrevalence",
            enabled as u32,
        )
    }

    //
    // ---- Wallpaper ----
    //

    pub fn set_wallpaper(&self, path: &str) -> Result<()> {
        info!("[WindowsC] Setting wallpaper: {}", path);
        let (_buf, wide_path) = to_pcwstr(path);

        unsafe {
            SystemParametersInfoW(
                SPI_SETDESKWALLPAPER,
                0,
                wide_path.0 as _,
                SPIF_UPDATEINIFILE | SPIF_SENDCHANGE,
            )
            .ok()
        }
    }
}
