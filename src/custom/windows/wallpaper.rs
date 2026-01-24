use std::ffi::c_void;
use windows::{
    core::Result,
    Win32::UI::WindowsAndMessaging::*,
};
use crate::{info, warn};

pub struct Wallpaper;

impl Wallpaper {
    pub fn new() -> Self {
        info!("[Wallpaper] Wallpaper manager initialized");
        Self
    }

    pub fn set(&self, path: &str) -> Result<()> {
        info!("[Wallpaper] Attempting to set wallpaper: {}", path);
        unsafe {
            let mut wide = path.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();

            let res = SystemParametersInfoW(
                SPI_SETDESKWALLPAPER,
                0,
                Some(wide.as_mut_ptr() as *mut c_void),
                SPIF_UPDATEINIFILE | SPIF_SENDCHANGE,
            );

            if let Err(e) = res {
                warn!("[Wallpaper] Failed to set wallpaper: {:?} (path: {})", e, path);
                return Err(e);
            }

            info!("[Wallpaper] Wallpaper set successfully");
            Ok(())
        }
    }
}