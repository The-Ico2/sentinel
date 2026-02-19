// ~/sentinel/sentinel-backend/src/ipc/sysdata/gpu.rs

use serde_json::{json, Value};
use std::{
	env,
	path::PathBuf,
	process::Command,
	sync::OnceLock,
};
use std::os::windows::process::CommandExt;
use sysinfo::Components;

const CREATE_NO_WINDOW: u32 = 0x08000000;

pub fn get_gpu_json() -> Value {
	let components = Components::new_with_refreshed_list();

	let adapters = query_wmi_video_controllers();
	let nvidia_temps = query_nvidia_smi_temps();

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

	let mut sensors = gpu_sensors;
	for sensor in nvidia_temps {
		sensors.push(sensor);
	}

	json!({
		"detected": !adapters.is_empty() || !sensors.is_empty(),
		"adapters": adapters,
		"sensors": sensors,
	})
}

fn query_wmi_video_controllers() -> Vec<Value> {
	let output = Command::new("wmic")
		.creation_flags(CREATE_NO_WINDOW)
		.args(["path", "win32_VideoController", "get", "Name,AdapterRAM", "/value"])
		.output();

	let Ok(output) = output else {
		return Vec::new();
	};
	if !output.status.success() {
		return Vec::new();
	}

	let text = String::from_utf8_lossy(&output.stdout);
	let mut adapters = Vec::<Value>::new();
	let mut current_name = String::new();
	let mut current_ram: Option<u64> = None;

	for raw in text.lines() {
		let line = raw.trim();
		if line.is_empty() {
			if !current_name.is_empty() {
				adapters.push(json!({
					"name": current_name,
					"adapter_ram_bytes": current_ram,
				}));
				current_name = String::new();
				current_ram = None;
			}
			continue;
		}

		if let Some(value) = line.strip_prefix("Name=") {
			current_name = value.trim().to_string();
		} else if let Some(value) = line.strip_prefix("AdapterRAM=") {
			current_ram = value.trim().parse::<u64>().ok();
		}
	}

	if !current_name.is_empty() {
		adapters.push(json!({
			"name": current_name,
			"adapter_ram_bytes": current_ram,
		}));
	}

	adapters
}

fn query_nvidia_smi_temps() -> Vec<Value> {
	let Some(nvidia_smi) = resolve_nvidia_smi_path() else {
		return Vec::new();
	};

	let output = Command::new(nvidia_smi)
		.creation_flags(CREATE_NO_WINDOW)
		.args(["--query-gpu=name,temperature.gpu", "--format=csv,noheader,nounits"])
		.output();

	let Ok(output) = output else {
		return Vec::new();
	};
	if !output.status.success() {
		return Vec::new();
	}

	let text = String::from_utf8_lossy(&output.stdout);
	text.lines()
		.filter_map(|line| {
			let parts: Vec<&str> = line.split(',').map(|v| v.trim()).collect();
			if parts.len() < 2 {
				return None;
			}

			Some(json!({
				"label": parts[0],
				"temperature_c": parts[1].parse::<f32>().ok(),
				"source": "nvidia-smi",
			}))
		})
		.collect()
}

fn resolve_nvidia_smi_path() -> Option<PathBuf> {
	static NVIDIA_SMI_PATH: OnceLock<Option<PathBuf>> = OnceLock::new();

	NVIDIA_SMI_PATH
		.get_or_init(|| {
			let mut candidates = Vec::<PathBuf>::new();
			if let Ok(program_files) = env::var("ProgramFiles") {
				candidates.push(
					PathBuf::from(program_files)
						.join("NVIDIA Corporation")
						.join("NVSMI")
						.join("nvidia-smi.exe"),
				);
			}
			if let Ok(program_files_x86) = env::var("ProgramFiles(x86)") {
				candidates.push(
					PathBuf::from(program_files_x86)
						.join("NVIDIA Corporation")
						.join("NVSMI")
						.join("nvidia-smi.exe"),
				);
			}

			for candidate in candidates {
				if candidate.is_file() {
					return Some(candidate);
				}
			}

			None
		})
		.clone()
}
