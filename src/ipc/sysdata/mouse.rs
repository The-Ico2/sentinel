// ~/sentinel/sentinel-backend/src/ipc/sysdata/mouse.rs

use serde_json::{json, Value};
use std::sync::{OnceLock, RwLock};
use windows::Win32::{
	Foundation::POINT,
	UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_MBUTTON, VK_RBUTTON},
	UI::WindowsAndMessaging::{
		GetCursorPos, GetSystemMetrics, SystemParametersInfoW,
		SM_CMOUSEBUTTONS, SM_MOUSEPRESENT, SM_MOUSEWHEELPRESENT, SM_SWAPBUTTON,
		SM_CXSCREEN, SM_CYSCREEN, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN,
		SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SM_CMONITORS,
		SPI_GETMOUSESPEED, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
	},
};

#[derive(Default, Clone)]
struct MouseEventState {
	left_down: bool,
	right_down: bool,
	middle_down: bool,
	left_clicks: u64,
	right_clicks: u64,
	middle_clicks: u64,
}

static MOUSE_STATE: OnceLock<RwLock<MouseEventState>> = OnceLock::new();

fn mouse_state() -> &'static RwLock<MouseEventState> {
	MOUSE_STATE.get_or_init(|| RwLock::new(MouseEventState::default()))
}

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

		let left_down = (GetAsyncKeyState(VK_LBUTTON.0.into()) as u16 & 0x8000) != 0;
		let right_down = (GetAsyncKeyState(VK_RBUTTON.0.into()) as u16 & 0x8000) != 0;
		let middle_down = (GetAsyncKeyState(VK_MBUTTON.0.into()) as u16 & 0x8000) != 0;

		let mut clicked = Vec::<&str>::new();
		let (left_clicks, right_clicks, middle_clicks) = {
			let mut state = mouse_state().write().unwrap();

			if left_down && !state.left_down {
				state.left_clicks = state.left_clicks.saturating_add(1);
				clicked.push("left");
			}
			if right_down && !state.right_down {
				state.right_clicks = state.right_clicks.saturating_add(1);
				clicked.push("right");
			}
			if middle_down && !state.middle_down {
				state.middle_clicks = state.middle_clicks.saturating_add(1);
				clicked.push("middle");
			}

			state.left_down = left_down;
			state.right_down = right_down;
			state.middle_down = middle_down;

			(state.left_clicks, state.right_clicks, state.middle_clicks)
		};

		json!({
			"present": mouse_present,
			"cursor": {
				"x": if cursor_ok { pos.x } else { 0 },
				"y": if cursor_ok { pos.y } else { 0 },
			},
			"buttons": {
				"count": num_buttons,
				"swapped": buttons_swapped,
				"left_down": left_down,
				"right_down": right_down,
				"middle_down": middle_down,
				"left_clicks": left_clicks,
				"right_clicks": right_clicks,
				"middle_clicks": middle_clicks,
			},
			"events": {
				"clicked": clicked,
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
