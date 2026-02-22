// ~/sentinel/sentinel-backend/src/ipc/sysdata/keyboard.rs

use serde_json::{json, Value};
use windows::Win32::UI::Input::KeyboardAndMouse::{
	GetKeyState, GetKeyboardLayoutNameW, GetKeyboardType,
	VK_CAPITAL, VK_NUMLOCK, VK_SCROLL, VK_INSERT,
};

pub fn get_keyboard_json() -> Value {
	unsafe {
		// Toggle key states
		let caps_lock = GetKeyState(VK_CAPITAL.0 as i32) & 1 != 0;
		let num_lock = GetKeyState(VK_NUMLOCK.0 as i32) & 1 != 0;
		let scroll_lock = GetKeyState(VK_SCROLL.0 as i32) & 1 != 0;
		let insert = GetKeyState(VK_INSERT.0 as i32) & 1 != 0;

		// Keyboard type info
		let keyboard_type = GetKeyboardType(0); // 0 = type
		let keyboard_subtype = GetKeyboardType(1); // 1 = subtype
		let num_function_keys = GetKeyboardType(2); // 2 = number of function keys

		// Keyboard layout identifier (KL_NAMELENGTH = 9)
		let mut layout_buf = [0u16; 9];
		let layout_name = if GetKeyboardLayoutNameW(&mut layout_buf).is_ok() {
			String::from_utf16_lossy(&layout_buf)
				.trim_end_matches('\0')
				.to_string()
		} else {
			"unknown".to_string()
		};

		let type_name = match keyboard_type {
			1 => "IBM PC/XT (83-key)",
			2 => "Olivetti ICO (102-key)",
			3 => "IBM PC/AT (84-key)",
			4 => "IBM Enhanced (101/102-key)",
			5 => "Nokia 1050",
			6 => "Nokia 9140",
			7 => "Japanese",
			_ => "Unknown",
		};

		json!({
			"layout_id": layout_name,
			"type_name": type_name,
			"type_id": keyboard_type,
			"subtype": keyboard_subtype,
			"function_key_count": num_function_keys,
			"toggle_states": {
				"caps_lock": caps_lock,
				"num_lock": num_lock,
				"scroll_lock": scroll_lock,
				"insert": insert,
			}
		})
	}
}
