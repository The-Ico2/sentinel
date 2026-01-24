use windows::{
    Win32::{
        Foundation::HWND,
        UI::WindowsAndMessaging::*,
    },
};
use windows_strings::w;
use crate::{info, warn};

pub struct Taskbar {
    hwnd: Option<HWND>,
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

impl Taskbar {
    pub fn new() -> Self {
        let hwnd = unsafe { FindWindowW(w!("Shell_TrayWnd"), None).ok() };
        if hwnd.is_some() {
            info!("[Taskbar] Found Shell_TrayWnd window handle");
        } else {
            warn!("[Taskbar] Could not find Shell_TrayWnd window handle");
        }
        Self { hwnd }
    }

    pub fn set(&self, visible: bool) {
        if let Some(hwnd) = self.hwnd {
            unsafe {
                let _ =ShowWindow(hwnd, if visible { SW_SHOW } else { SW_HIDE });
            }
            info!("[Taskbar] Taskbar visibility set to {}", visible);
        } else {
            warn!("[Taskbar] Cannot set visibility, hwnd is None");
        }
    }

    pub fn toggle(&self) {
        if let Some(hwnd) = self.hwnd {
            unsafe {
                let visible = IsWindowVisible(hwnd).as_bool();
                let _ =ShowWindow(hwnd, if visible { SW_HIDE } else { SW_SHOW });
                info!("[Taskbar] Taskbar toggled, new visibility: {}", !visible);
            }
        } else {
            warn!("[Taskbar] Cannot toggle, hwnd is None");
        }
    }
}