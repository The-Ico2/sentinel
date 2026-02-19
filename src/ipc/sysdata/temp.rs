// ~/sentinel/sentinel-backend/src/ipc/sysdata/temp.rs

use serde_json::{json, Value};
use sysinfo::Components;

pub fn get_temp_json() -> Value {
	let components = Components::new_with_refreshed_list();

	let mut sum = 0.0f32;
	let mut count = 0usize;

	let sensors: Vec<Value> = components
		.iter()
		.map(|component| {
			let temp = component.temperature().unwrap_or(0.0);
			if temp > 0.0 {
				sum += temp;
				count += 1;
			}

			json!({
				"label": component.label(),
				"temperature_c": temp,
				"max_c": component.max(),
				"critical_c": component.critical(),
			})
		})
		.collect();

	let avg = if count == 0 { 0.0 } else { sum / count as f32 };

	json!({
		"average_c": avg,
		"sensors": sensors,
	})
}
