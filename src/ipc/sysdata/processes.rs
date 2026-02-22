// ~/sentinel/sentinel-backend/src/ipc/sysdata/processes.rs

use serde_json::{json, Value};
use sysinfo::System;

pub fn get_processes_json() -> Value {
	let mut sys = System::new_all();
	sys.refresh_all();
	// Second refresh to get accurate CPU usage (sysinfo needs two samples)
	std::thread::sleep(std::time::Duration::from_millis(200));
	sys.refresh_all();

	let processes = sys.processes();
	let total_processes = processes.len();

	// Collect into a sortable vec
	let mut proc_list: Vec<(&sysinfo::Pid, &sysinfo::Process)> = processes.iter().collect();

	// Top 15 by CPU usage
	proc_list.sort_by(|a, b| {
		b.1.cpu_usage()
			.partial_cmp(&a.1.cpu_usage())
			.unwrap_or(std::cmp::Ordering::Equal)
	});
	let top_cpu: Vec<Value> = proc_list
		.iter()
		.take(15)
		.map(|(pid, p)| {
			json!({
				"pid": pid.as_u32(),
				"name": p.name().to_string_lossy(),
				"cpu_percent": p.cpu_usage(),
				"memory_bytes": p.memory(),
				"status": format!("{:?}", p.status()),
			})
		})
		.collect();

	// Top 15 by memory
	proc_list.sort_by(|a, b| b.1.memory().cmp(&a.1.memory()));
	let top_memory: Vec<Value> = proc_list
		.iter()
		.take(15)
		.map(|(pid, p)| {
			json!({
				"pid": pid.as_u32(),
				"name": p.name().to_string_lossy(),
				"cpu_percent": p.cpu_usage(),
				"memory_bytes": p.memory(),
				"virtual_memory_bytes": p.virtual_memory(),
				"status": format!("{:?}", p.status()),
			})
		})
		.collect();

	// Aggregate stats
	let total_cpu: f32 = processes.values().map(|p| p.cpu_usage()).sum();
	let total_memory: u64 = processes.values().map(|p| p.memory()).sum();

	// Count by status
	let mut running = 0u32;
	let mut sleeping = 0u32;
	let mut stopped = 0u32;
	let mut zombie = 0u32;
	let mut other = 0u32;

	for p in processes.values() {
		match p.status() {
			sysinfo::ProcessStatus::Run => running += 1,
			sysinfo::ProcessStatus::Sleep => sleeping += 1,
			sysinfo::ProcessStatus::Stop => stopped += 1,
			sysinfo::ProcessStatus::Zombie => zombie += 1,
			_ => other += 1,
		}
	}

	json!({
		"total_count": total_processes,
		"total_cpu_usage": total_cpu,
		"total_memory_bytes": total_memory,
		"status_counts": {
			"running": running,
			"sleeping": sleeping,
			"stopped": stopped,
			"zombie": zombie,
			"other": other,
		},
		"top_cpu": top_cpu,
		"top_memory": top_memory,
	})
}
