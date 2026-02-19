// ~/sentinel/sentinel-backend/src/ipc/sysdata/cpu.rs

use serde_json::{json, Value};
use sysinfo::System;

pub fn get_cpu_json() -> Value {
	let mut sys = System::new_all();
	sys.refresh_all();

	let cpus = sys.cpus();
	let logical_cores = cpus.len();

	let avg_usage = if cpus.is_empty() {
		0.0
	} else {
		cpus.iter().map(|c| c.cpu_usage()).sum::<f32>() / cpus.len() as f32
	};

	let avg_frequency_mhz = if cpus.is_empty() {
		0u64
	} else {
		cpus.iter().map(|c| c.frequency()).sum::<u64>() / cpus.len() as u64
	};

	let brand = cpus
		.first()
		.map(|c| c.brand().to_string())
		.unwrap_or_else(|| "unknown".to_string());

	json!({
		"brand": brand,
		"logical_cores": logical_cores,
		"usage_percent": avg_usage,
		"frequency_mhz": avg_frequency_mhz,
	})
}
