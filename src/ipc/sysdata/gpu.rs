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

fn decode_command_output(bytes: &[u8]) -> String {
	if bytes.is_empty() {
		return String::new();
	}

	let looks_utf16le = bytes.starts_with(&[0xFF, 0xFE])
		|| bytes
			.iter()
			.skip(1)
			.step_by(2)
			.take(64)
			.filter(|&&b| b == 0)
			.count()
			> 16;

	if looks_utf16le {
		let mut u16s = Vec::<u16>::with_capacity(bytes.len() / 2);
		let mut start = 0usize;
		if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE {
			start = 2;
		}
		for chunk in bytes[start..].chunks_exact(2) {
			u16s.push(u16::from_le_bytes([chunk[0], chunk[1]]));
		}
		String::from_utf16_lossy(&u16s)
	} else {
		String::from_utf8_lossy(bytes).to_string()
	}
}

pub fn get_gpu_json() -> Value {
	let components = Components::new_with_refreshed_list();

	let mut adapters = query_wmi_video_controllers();
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

	if adapters.is_empty() {
		let mut names = std::collections::BTreeSet::<String>::new();
		for sensor in &sensors {
			if let Some(label) = sensor.get("label").and_then(|v| v.as_str()) {
				if !label.trim().is_empty() {
					names.insert(label.trim().to_string());
				}
			}
		}

		for name in names {
			adapters.push(json!({
				"name": name,
				"adapter_ram_bytes": Value::Null,
				"source": "sensor-fallback",
			}));
		}
	}

	let average_c = average_temp(&sensors);

	json!({
		"detected": !adapters.is_empty() || !sensors.is_empty(),
		"adapters": adapters,
		"temperature": {
			"average_c": average_c,
			"sensors": sensors.clone(),
		},
		"sensors": sensors,
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

	let text = decode_command_output(&output.stdout);
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

	if adapters.is_empty() {
		query_cim_video_controllers()
	} else {
		adapters
	}
}

fn query_cim_video_controllers() -> Vec<Value> {
	let script = r#"$ErrorActionPreference='SilentlyContinue';
$rows = Get-CimInstance Win32_VideoController;
foreach ($r in $rows) {
	"Name=$($r.Name)";
	"AdapterRAM=$($r.AdapterRAM)";
	"PNPDeviceID=$($r.PNPDeviceID)";
	"DriverVersion=$($r.DriverVersion)";
	"";
}"#;

	let output = Command::new("powershell")
		.creation_flags(CREATE_NO_WINDOW)
		.args(["-NoProfile", "-NonInteractive", "-Command", script])
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
	let mut current_pnp: Option<String> = None;
	let mut current_driver: Option<String> = None;

	for raw in text.lines() {
		let line = raw.trim();
		if line.is_empty() {
			if !current_name.is_empty() {
				adapters.push(json!({
					"name": current_name,
					"adapter_ram_bytes": current_ram,
					"pnp_device_id": current_pnp,
					"driver_version": current_driver,
					"source": "cim",
				}));
				current_name = String::new();
				current_ram = None;
				current_pnp = None;
				current_driver = None;
			}
			continue;
		}

		if let Some(value) = line.strip_prefix("Name=") {
			current_name = value.trim().to_string();
		} else if let Some(value) = line.strip_prefix("AdapterRAM=") {
			current_ram = value.trim().parse::<u64>().ok();
		} else if let Some(value) = line.strip_prefix("PNPDeviceID=") {
			current_pnp = Some(value.trim().to_string());
		} else if let Some(value) = line.strip_prefix("DriverVersion=") {
			current_driver = Some(value.trim().to_string());
		}
	}

	if !current_name.is_empty() {
		adapters.push(json!({
			"name": current_name,
			"adapter_ram_bytes": current_ram,
			"pnp_device_id": current_pnp,
			"driver_version": current_driver,
			"source": "cim",
		}));
	}

	adapters
}

fn query_nvidia_smi_temps() -> Vec<Value> {
	let output = run_nvidia_smi()
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

fn run_nvidia_smi() -> Command {
	if let Some(path) = resolve_nvidia_smi_path() {
		return Command::new(path);
	}
	Command::new("nvidia-smi")
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
