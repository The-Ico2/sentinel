// ~/sentinel/sentinel-backend/src/ipc/sysdata/gpu.rs

use serde_json::{json, Value};
use sysinfo::Components;

pub fn get_gpu_json() -> Value {
	let components = Components::new_with_refreshed_list();

	let gpu_sensors: Vec<Value> = components
		.iter()
		.filter_map(|component| {
			let label = component.label().to_ascii_lowercase();
			let is_gpu = label.contains("gpu")
				|| label.contains("graphics")
				|| label.contains("nvidia")
				|| label.contains("radeon")
				|| label.contains("amd")
				|| label.contains("intel");

			if !is_gpu {
				return None;
			}

			Some(json!({
				"label": component.label(),
				"temperature_c": component.temperature(),
				"max_c": component.max(),
				"critical_c": component.critical(),
			}))
		})
		.collect();

	json!({
		"detected": !gpu_sensors.is_empty(),
		"sensors": gpu_sensors,
	})
}
