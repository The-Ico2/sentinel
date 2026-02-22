// ~/sentinel/sentinel-backend/src/ipc/sysdata/idle.rs

use serde_json::{json, Value};
use std::mem;
use std::os::windows::process::CommandExt;

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

	json!({
		"idle_ms": idle_ms,
		"idle_seconds": idle_seconds,
		"idle_minutes": idle_minutes,
		"idle_state": idle_state,
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

fn is_screen_locked() -> bool {
	// Check if the lock screen process is running
	let output = std::process::Command::new("powershell")
		.args([
			"-NoProfile",
			"-Command",
			"(Get-Process -Name LogonUI -ErrorAction SilentlyContinue) -ne $null",
		])
		.creation_flags(0x08000000)
		.output();

	match output {
		Ok(o) => {
			let result = String::from_utf8_lossy(&o.stdout).trim().to_lowercase();
			result == "true"
		}
		Err(_) => false,
	}
}

fn is_screensaver_running() -> bool {
	let output = std::process::Command::new("powershell")
		.args([
			"-NoProfile",
			"-Command",
			"(Get-Process -Name *.scr -ErrorAction SilentlyContinue) -ne $null",
		])
		.creation_flags(0x08000000)
		.output();

	match output {
		Ok(o) => {
			let result = String::from_utf8_lossy(&o.stdout).trim().to_lowercase();
			result == "true"
		}
		Err(_) => false,
	}
}
