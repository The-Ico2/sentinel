// ~/sentinel/sentinel-backend/src/ipc/sysdata/temp.rs

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

pub fn get_temp_json() -> Value {
	let components = Components::new_with_refreshed_list();

	let mut cpu_sensors = Vec::<Value>::new();
	let mut gpu_sensors = Vec::<Value>::new();

	for component in components.iter() {
		let label = component.label().to_ascii_lowercase();
		let temp = component.temperature().unwrap_or(0.0);
		let entry = json!({
			"label": component.label(),
			"temperature_c": temp,
			"max_c": component.max(),
			"critical_c": component.critical(),
		});

		if label.contains("gpu") || label.contains("graphics") || label.contains("nvidia") || label.contains("radeon") {
			gpu_sensors.push(entry);
		} else {
			cpu_sensors.push(entry);
		}
	}

	if cpu_sensors.is_empty() {
		if let Some(temp) = query_wmi_cpu_temp_c() {
			cpu_sensors.push(json!({
				"label": "CPU Thermal Zone",
				"temperature_c": temp,
				"source": "wmi",
			}));
		}
	}

	if gpu_sensors.is_empty() {
		gpu_sensors.extend(query_nvidia_smi_gpu_temp());
	}

	json!({
		"cpu": {
			"average_c": average_temp(&cpu_sensors),
			"sensors": cpu_sensors,
		},
		"gpu": {
			"average_c": average_temp(&gpu_sensors),
			"sensors": gpu_sensors,
		}
	})
}

fn average_temp(sensors: &[Value]) -> f32 {
	let mut sum = 0.0f32;
	let mut count = 0usize;

	for sensor in sensors {
		if let Some(t) = sensor.get("temperature_c").and_then(|v| v.as_f64()) {
			let tf = t as f32;
			if tf > 0.0 {
				sum += tf;
				count += 1;
			}
		}
	}

	if count == 0 {
		0.0
	} else {
		sum / count as f32
	}
}

fn query_wmi_cpu_temp_c() -> Option<f32> {
	let output = Command::new("wmic")
		.creation_flags(CREATE_NO_WINDOW)
		.args([
			"/namespace:\\\\root\\wmi",
			"PATH",
			"MSAcpi_ThermalZoneTemperature",
			"get",
			"CurrentTemperature",
			"/value",
		])
		.output()
		.ok()?;

	if !output.status.success() {
		return None;
	}

	let text = String::from_utf8_lossy(&output.stdout);
	let mut values = Vec::<f32>::new();
	for line in text.lines() {
		if let Some(value) = line.trim().strip_prefix("CurrentTemperature=") {
			if let Ok(raw) = value.trim().parse::<f32>() {
				let c = (raw / 10.0) - 273.15;
				if c > -50.0 && c < 200.0 {
					values.push(c);
				}
			}
		}
	}

	if values.is_empty() {
		None
	} else {
		Some(values.iter().sum::<f32>() / values.len() as f32)
	}
}

fn query_nvidia_smi_gpu_temp() -> Vec<Value> {
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
