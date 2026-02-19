// ~/sentinel/sentinel-backend/src/ipc/sysdata/ram.rs

use serde_json::{json, Value};
use sysinfo::System;

pub fn get_ram_json() -> Value {
	let mut sys = System::new_all();
	sys.refresh_all();

	let total = sys.total_memory();
	let used = sys.used_memory();
	let available = sys.available_memory();
	let total_swap = sys.total_swap();
	let used_swap = sys.used_swap();

	let usage_percent = if total == 0 {
		0.0
	} else {
		(used as f64 / total as f64) * 100.0
	};

	json!({
		"total_bytes": total,
		"used_bytes": used,
		"available_bytes": available,
		"usage_percent": usage_percent,
		"swap_total_bytes": total_swap,
		"swap_used_bytes": used_swap,
	})
}
