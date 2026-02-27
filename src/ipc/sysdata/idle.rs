// ~/sentinel/sentinel-backend/src/ipc/sysdata/idle.rs

use serde_json::{json, Value};
use std::mem;

#[cfg(target_os = "windows")]
use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};

pub fn get_idle_json() -> Value {
	let idle_ms = get_idle_time_ms();
	let screen_locked = is_screen_locked();
	let screensaver_active = is_screensaver_running();

	let idle_seconds = idle_ms / 1000;
	let idle_minutes = idle_seconds / 60;

	let idle_state = if screen_locked {
		"locked"
	} else if screensaver_active {
		"screensaver"
	} else if idle_minutes >= 15 {
		"away"
	} else if idle_minutes >= 5 {
		"idle"
	} else {
		"active"
	};

	let is_idle = idle_state != "active";

	json!({
		"idle_ms": idle_ms,
		"idle_time_ms": idle_ms,
		"idle_seconds": idle_seconds,
		"idle_minutes": idle_minutes,
		"idle_state": idle_state,
		"is_idle": is_idle,
		"screen_locked": screen_locked,
		"screensaver_active": screensaver_active,
	})
}

#[cfg(target_os = "windows")]
fn get_idle_time_ms() -> u64 {
	unsafe {
		let mut lii = LASTINPUTINFO {
			cbSize: mem::size_of::<LASTINPUTINFO>() as u32,
			dwTime: 0,
		};
		if GetLastInputInfo(&mut lii).as_bool() {
			let tick = windows::Win32::System::SystemInformation::GetTickCount();
			let elapsed = tick.wrapping_sub(lii.dwTime);
			elapsed as u64
		} else {
			0
		}
	}
}

#[cfg(not(target_os = "windows"))]
fn get_idle_time_ms() -> u64 {
	0
}

/// Check if the screen is locked by looking for the LogonUI process.
/// Uses Win32 process enumeration instead of spawning PowerShell.
fn is_screen_locked() -> bool {
	use windows::Win32::System::ProcessStatus::EnumProcesses;
	use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
	use windows::Win32::Foundation::CloseHandle;

	unsafe {
		let mut pids = vec![0u32; 4096];
		let mut bytes_returned = 0u32;
		if EnumProcesses(
			pids.as_mut_ptr(),
			(pids.len() * std::mem::size_of::<u32>()) as u32,
			&mut bytes_returned,
		).is_err() {
			return false;
		}

		let count = bytes_returned as usize / std::mem::size_of::<u32>();
		for &pid in &pids[..count] {
			if pid == 0 { continue; }
			let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
				continue;
			};
			let mut buf = [0u16; 260];
			let mut size = buf.len() as u32;
			let ok = windows::Win32::System::Threading::QueryFullProcessImageNameW(
				handle,
				windows::Win32::System::Threading::PROCESS_NAME_FORMAT(0),
				windows::core::PWSTR(buf.as_mut_ptr()),
				&mut size,
			);
			let _ = CloseHandle(handle);
			if ok.is_ok() && size > 0 {
				let name = String::from_utf16_lossy(&buf[..size as usize]);
				if let Some(filename) = name.rsplit('\\').next() {
					if filename.eq_ignore_ascii_case("LogonUI.exe") {
						return true;
					}
				}
			}
		}
		false
	}
}

/// Check if a screensaver is running by querying SystemParametersInfo.
fn is_screensaver_running() -> bool {
	use windows::Win32::UI::WindowsAndMessaging::{
		SystemParametersInfoW, SPI_GETSCREENSAVERRUNNING, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
	};
	unsafe {
		let mut running: i32 = 0;
		let ok = SystemParametersInfoW(
			SPI_GETSCREENSAVERRUNNING,
			0,
			Some(&mut running as *mut i32 as *mut _),
			SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
		);
		ok.is_ok() && running != 0
	}
}