use windows::core::Result;
use crate::custom::windows::registry::set_reg_dword;
use crate::{info, warn};

pub struct Theme;

impl Theme {
    pub fn new() -> Self {
        info!("[Theme] Theme manager initialized");
        Self
    }

    pub fn set_dark_mode(&self, enabled: bool) -> Result<()> {
        info!("[Theme] Setting dark mode to {}", enabled);
        let value = if enabled { 0 } else { 1 };

        // Set AppsUseLightTheme
        if let Err(e) = set_reg_dword(
            r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
            "AppsUseLightTheme",
            value,
        ) {
            warn!("[Theme] Failed to set AppsUseLightTheme: {:?}", e);
            return Err(e);
        }

        // Set SystemUsesLightTheme
        match set_reg_dword(
            r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
            "SystemUsesLightTheme",
            value,
        ) {
            Ok(_) => {
                info!("[Theme] Dark mode successfully set to {}", enabled);
                Ok(())
            }
            Err(e) => {
                warn!("[Theme] Failed to set SystemUsesLightTheme: {:?}", e);
                Err(e)
            }
        }
    }
}