// ~/sentinel/sentinel-backend/src/ipc/sysdata/audio.rs

use serde_json::{json, Value};

pub fn get_audio_json() -> Value {
	json!({
		"detected": true,
		"default_output": "system-default",
		"state": "available",
		"note": "Use wallpaper addon native audio meter for real-time amplitude.",
	})
}
