use windows::core::{PCWSTR, Result};
use windows::Win32::System::Registry::*;
use windows::Win32::Foundation::ERROR_SUCCESS;
use crate::{info, warn};

pub fn set_reg_dword(path: &str, name: &str, value: u32) -> Result<()> {
    unsafe {
        let mut key = HKEY::default();

        // null‑terminated wide key path
        let mut path_utf16: Vec<u16> = path.encode_utf16().collect();
        path_utf16.push(0);

        // create/open registry key
        let status = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(path_utf16.as_ptr()),
            None,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut key,
            None,
        );

        if status != ERROR_SUCCESS {
            warn!("[Registry] Failed to create/open key '{}': {:#X}", path, status.0);
            return Err(status.into());
        }
        info!("[Registry] Opened registry key '{}'", path);

        // null‑terminated wide value name
        let mut name_utf16: Vec<u16> = name.encode_utf16().collect();
        name_utf16.push(0);

        // set DWORD value
        let status = RegSetValueExW(
            key,
            PCWSTR(name_utf16.as_ptr()),
            None,
            REG_DWORD,
            Some(&value.to_le_bytes()),
        );

        // close key
        let _ =RegCloseKey(key);

        if status != ERROR_SUCCESS {
            warn!("[Registry] Failed to set DWORD '{}={}' in key '{}': {:#X}", name, value, path, status.0);
            return Err(status.into());
        }

        info!("[Registry] Set DWORD '{}={}' in key '{}'", name, value, path);
        Ok(())
    }
}