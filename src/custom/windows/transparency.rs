use windows::core::Result;
use crate::custom::windows::registry::set_reg_dword;
use crate::{info, warn};

pub struct Transparency;

impl Transparency {
    pub fn new() -> Self {
        info!("[Transparency] Transparency manager initialized");
        Self
    }

    pub fn set_enabled(&self, enabled: bool) -> Result<()> {
        info!("[Transparency] Setting transparency to {}", enabled);

        match set_reg_dword(
            r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
            "EnableTransparency",
            enabled as u32,
        ) {
            Ok(_) => {
                info!("[Transparency] Transparency successfully set to {}", enabled);
                Ok(())
            }
            Err(e) => {
                warn!("[Transparency] Failed to set transparency: {:?}", e);
                Err(e)
            }
        }
    }
}