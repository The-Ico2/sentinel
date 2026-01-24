// ~/sentinel/sentinel-backend/src/utils.rs

use windows::{
    core::Result,
    Win32::{
        Foundation::{CloseHandle, HANDLE},
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
            },
            ProcessStatus::K32GetModuleFileNameExW,
            Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
        },
    },
};
use as_bool::AsBool;

pub fn get_process_name(pid: u32) -> Result<String> {
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)?;
        if snapshot.is_invalid() {
            return Ok("unknown".to_string());
        }

        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

        if Process32FirstW(snapshot, &mut entry).as_bool() {
            loop {
                if entry.th32ProcessID == pid {
                    let name = String::from_utf16_lossy(
                        &entry.szExeFile
                            .iter()
                            .take_while(|c| **c != 0)
                            .cloned()
                            .collect::<Vec<_>>(),
                    );
                    let _ = CloseHandle(snapshot); // close the snapshot handle
                    return Ok(name);
                }

                if !Process32NextW(snapshot, &mut entry).as_bool() {
                    break;
                }
            }
        }

        let _ = CloseHandle(snapshot);
        Ok("unknown".to_string())
    }
}

pub fn get_process_exe(pid: u32) -> Result<String> {
    unsafe {
        // Open process with limited query rights
        let handle: HANDLE = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)?;
        if handle.is_invalid() {
            return Ok("unknown".into());
        }

        let mut buffer = vec![0u16; 260];
        // Pass None for the main module (hModule)
        let len = K32GetModuleFileNameExW(Some(handle), None, &mut buffer);
        let _ = CloseHandle(handle);

        if len == 0 {
            return Ok("unknown".into());
        }

        Ok(String::from_utf16_lossy(&buffer[..len as usize]))
    }
}
