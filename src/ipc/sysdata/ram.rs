// ~/sentinel/sentinel-backend/src/ipc/sysdata/ram.rs

use serde_json::{json, Value};
use sysinfo::System;

pub fn get_ram_json() -> Value {
	let mut sys = System::new_all();
	sys.refresh_all();

	let total = sys.total_memory();
	let used = sys.used_memory();
	let available = sys.available_memory();
	let free = sys.free_memory();
	let total_swap = sys.total_swap();
	let used_swap = sys.used_swap();
	let free_swap = sys.free_swap();

	let usage_percent = if total == 0 {
		0.0
	} else {
		(used as f64 / total as f64) * 100.0
	};

	let swap_usage_percent = if total_swap == 0 {
		0.0
	} else {
		(used_swap as f64 / total_swap as f64) * 100.0
	};

	// Top memory-consuming processes (top 10)
	let mut processes: Vec<(&sysinfo::Pid, &sysinfo::Process)> = sys.processes().iter().collect();
	processes.sort_by(|a, b| b.1.memory().cmp(&a.1.memory()));

	let top_processes: Vec<Value> = processes
		.iter()
		.take(10)
		.map(|(pid, proc_info)| {
			json!({
				"pid": pid.as_u32(),
				"name": proc_info.name().to_string_lossy(),
				"memory_bytes": proc_info.memory(),
				"virtual_memory_bytes": proc_info.virtual_memory(),
			})
		})
		.collect();

	json!({
		"total_bytes": total,
		"used_bytes": used,
		"available_bytes": available,
		"free_bytes": free,
		"usage_percent": usage_percent,
		"swap_total_bytes": total_swap,
		"swap_used_bytes": used_swap,
		"swap_free_bytes": free_swap,
		"swap_usage_percent": swap_usage_percent,
		"top_processes": top_processes,
	})
}
