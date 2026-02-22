// ~/sentinel/sentinel-backend/src/ipc/sysdata/mouse.rs

use serde_json::{json, Value};
use windows::Win32::{
	Foundation::POINT,
	UI::WindowsAndMessaging::{
		GetCursorPos, GetSystemMetrics, SystemParametersInfoW,
		SM_CMOUSEBUTTONS, SM_MOUSEPRESENT, SM_MOUSEWHEELPRESENT, SM_SWAPBUTTON,
		SM_CXSCREEN, SM_CYSCREEN, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN,
		SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SM_CMONITORS,
		SPI_GETMOUSESPEED, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
	},
};

pub fn get_mouse_json() -> Value {
	unsafe {
		// Cursor position
		let mut pos = POINT::default();
		let cursor_ok = GetCursorPos(&mut pos).is_ok();

		// System metrics
		let mouse_present = GetSystemMetrics(SM_MOUSEPRESENT) != 0;
		let num_buttons = GetSystemMetrics(SM_CMOUSEBUTTONS);
		let wheel_present = GetSystemMetrics(SM_MOUSEWHEELPRESENT) != 0;
		let buttons_swapped = GetSystemMetrics(SM_SWAPBUTTON) != 0;

		// Screen dimensions
		let primary_width = GetSystemMetrics(SM_CXSCREEN);
		let primary_height = GetSystemMetrics(SM_CYSCREEN);
		let virtual_width = GetSystemMetrics(SM_CXVIRTUALSCREEN);
		let virtual_height = GetSystemMetrics(SM_CYVIRTUALSCREEN);
		let virtual_x = GetSystemMetrics(SM_XVIRTUALSCREEN);
		let virtual_y = GetSystemMetrics(SM_YVIRTUALSCREEN);
		let monitor_count = GetSystemMetrics(SM_CMONITORS);

		// Mouse speed (1-20 range)
		let mut mouse_speed: i32 = 10;
		let _ = SystemParametersInfoW(
			SPI_GETMOUSESPEED,
			0,
			Some(&mut mouse_speed as *mut i32 as *mut _),
			SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
		);

		json!({
			"present": mouse_present,
			"cursor": {
				"x": if cursor_ok { pos.x } else { 0 },
				"y": if cursor_ok { pos.y } else { 0 },
			},
			"buttons": {
				"count": num_buttons,
				"swapped": buttons_swapped,
			},
			"wheel_present": wheel_present,
			"speed": mouse_speed,
			"screen": {
				"primary_width": primary_width,
				"primary_height": primary_height,
				"virtual_width": virtual_width,
				"virtual_height": virtual_height,
				"virtual_x": virtual_x,
				"virtual_y": virtual_y,
				"monitor_count": monitor_count,
			}
		})
	}
}
