use windows::{
    Win32::{
        Foundation::HWND,
        UI::WindowsAndMessaging::*,
    },
};
use windows_strings::w;
use crate::{info, warn};

pub struct Taskbar {
    pub _hwnd: Option<HWND>,
}

impl Taskbar {
    pub fn new() -> Self {
        let hwnd = unsafe { FindWindowW(w!("Shell_TrayWnd"), None).ok() };
        if hwnd.is_some() {
            info!("[Taskbar] Found Shell_TrayWnd window handle");
        } else {
            warn!("[Taskbar] Could not find Shell_TrayWnd window handle");
        }
        Self { _hwnd: hwnd }
    }
}