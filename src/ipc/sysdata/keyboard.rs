// ~/sentinel/sentinel-backend/src/ipc/sysdata/keyboard.rs

use serde_json::{json, Value};
use std::{collections::HashSet, sync::{OnceLock, RwLock}};
use windows::Win32::UI::Input::KeyboardAndMouse::{
	GetAsyncKeyState,
	GetKeyState, GetKeyboardLayoutNameW, GetKeyboardType,
	VK_CAPITAL, VK_NUMLOCK, VK_SCROLL, VK_INSERT,
};

static KEYBOARD_PRESSED: OnceLock<RwLock<HashSet<i32>>> = OnceLock::new();

fn keyboard_pressed() -> &'static RwLock<HashSet<i32>> {
	KEYBOARD_PRESSED.get_or_init(|| RwLock::new(HashSet::new()))
}

const TRACKED_KEYS: &[(i32, &str)] = &[
	(0x08, "Backspace"), (0x09, "Tab"), (0x0D, "Enter"), (0x10, "Shift"),
	(0x11, "Control"), (0x12, "Alt"), (0x14, "CapsLock"), (0x1B, "Escape"),
	(0x20, "Space"),
	(0x25, "ArrowLeft"), (0x26, "ArrowUp"), (0x27, "ArrowRight"), (0x28, "ArrowDown"),
	(0x30, "0"), (0x31, "1"), (0x32, "2"), (0x33, "3"), (0x34, "4"),
	(0x35, "5"), (0x36, "6"), (0x37, "7"), (0x38, "8"), (0x39, "9"),
	(0x41, "A"), (0x42, "B"), (0x43, "C"), (0x44, "D"), (0x45, "E"),
	(0x46, "F"), (0x47, "G"), (0x48, "H"), (0x49, "I"), (0x4A, "J"),
	(0x4B, "K"), (0x4C, "L"), (0x4D, "M"), (0x4E, "N"), (0x4F, "O"),
	(0x50, "P"), (0x51, "Q"), (0x52, "R"), (0x53, "S"), (0x54, "T"),
	(0x55, "U"), (0x56, "V"), (0x57, "W"), (0x58, "X"), (0x59, "Y"),
	(0x5A, "Z"),
	(0x70, "F1"), (0x71, "F2"), (0x72, "F3"), (0x73, "F4"), (0x74, "F5"),
	(0x75, "F6"), (0x76, "F7"), (0x77, "F8"), (0x78, "F9"), (0x79, "F10"),
	(0x7A, "F11"), (0x7B, "F12"),
];

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

		let mut down_events = Vec::<String>::new();
		let mut up_events = Vec::<String>::new();
		let mut pressed_keys = Vec::<String>::new();

		let mut currently_pressed = HashSet::<i32>::new();
		for (vk, label) in TRACKED_KEYS {
			let down = (GetAsyncKeyState(*vk) as u16 & 0x8000) != 0;
			if down {
				currently_pressed.insert(*vk);
				pressed_keys.push((*label).to_string());
			}
		}

		{
			let mut previous = keyboard_pressed().write().unwrap();

			for (vk, label) in TRACKED_KEYS {
				let now_down = currently_pressed.contains(vk);
				let was_down = previous.contains(vk);

				if now_down && !was_down {
					down_events.push((*label).to_string());
				} else if !now_down && was_down {
					up_events.push((*label).to_string());
				}
			}

			*previous = currently_pressed;
		}

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
			},
			"pressed_keys": pressed_keys,
			"pressed_count": pressed_keys.len(),
			"events": {
				"down": down_events,
				"up": up_events,
			}
		})
	}
}
