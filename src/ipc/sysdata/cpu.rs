// ~/sentinel/sentinel-backend/src/ipc/sysdata/cpu.rs

use serde_json::{json, Value};
use std::os::windows::process::CommandExt;
use std::process::Command;
use sysinfo::Components;
use sysinfo::System;

const CREATE_NO_WINDOW: u32 = 0x08000000;

pub fn get_cpu_json() -> Value {
	let mut sys = System::new_all();
	sys.refresh_all();

	let cpus = sys.cpus();
	let logical_cores = cpus.len();
	let physical_cores = System::physical_core_count().unwrap_or(0);

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

	let vendor_id = cpus
		.first()
		.map(|c| c.vendor_id().to_string())
		.unwrap_or_else(|| "unknown".to_string());

	let per_core: Vec<Value> = cpus
		.iter()
		.enumerate()
		.map(|(i, c)| {
			json!({
				"core_id": i,
				"usage_percent": c.cpu_usage(),
				"frequency_mhz": c.frequency(),
			})
		})
		.collect();

	let cpu_temp = get_cpu_temperature_json();

	let uptime_seconds = System::uptime();
	let boot_time_unix = System::boot_time();

	let process_count = sys.processes().len();

	let arch = std::env::consts::ARCH;

	json!({
		"brand": brand,
		"vendor_id": vendor_id,
		"arch": arch,
		"logical_cores": logical_cores,
		"physical_cores": physical_cores,
		"usage_percent": avg_usage,
		"frequency_mhz": avg_frequency_mhz,
		"temperature": cpu_temp,
		"per_core": per_core,
		"uptime_seconds": uptime_seconds,
		"boot_time_unix": boot_time_unix,
		"process_count": process_count,
	})
}

fn get_cpu_temperature_json() -> Value {
	let components = Components::new_with_refreshed_list();
	let mut sensors = Vec::<Value>::new();

	for component in components.iter() {
		let label = component.label().to_ascii_lowercase();
		let is_cpu = label.contains("cpu")
			|| label.contains("package")
			|| label.contains("core")
			|| label.contains("tctl")
			|| label.contains("tdie");

		if !is_cpu {
			continue;
		}

		sensors.push(json!({
			"label": component.label(),
			"temperature_c": component.temperature().unwrap_or(0.0),
			"max_c": component.max(),
			"critical_c": component.critical(),
			"source": "sysinfo",
		}));
	}

	if sensors.is_empty() {
		if let Some(temp) = query_wmi_cpu_temp_c() {
			sensors.push(json!({
				"label": "CPU Thermal Zone",
				"temperature_c": temp,
				"source": "wmi",
			}));
		}
	}

	if sensors.is_empty() {
		if let Some(temp) = query_perf_counter_temp_c() {
			sensors.push(json!({
				"label": "Thermal Zone Counter",
				"temperature_c": temp,
				"source": "perf-counter",
			}));
		}
	}

	json!({
		"average_c": average_temp(&sensors),
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

fn query_perf_counter_temp_c() -> Option<f32> {
	let script = r#"$ErrorActionPreference='SilentlyContinue';
$samples = Get-Counter '\Thermal Zone Information(*)\Temperature' |
	Select-Object -ExpandProperty CounterSamples |
	Select-Object -ExpandProperty CookedValue;
if ($samples) {
	$samples | ForEach-Object { $_.ToString([System.Globalization.CultureInfo]::InvariantCulture) }
}"#;

	let output = Command::new("powershell")
		.creation_flags(CREATE_NO_WINDOW)
		.args(["-NoProfile", "-NonInteractive", "-Command", script])
		.output()
		.ok()?;

	if !output.status.success() {
		return None;
	}

	let text = String::from_utf8_lossy(&output.stdout);
	let mut values = Vec::<f32>::new();

	for line in text.lines() {
		let raw = match line.trim().parse::<f32>() {
			Ok(v) => v,
			Err(_) => continue,
		};

		let c = if raw > 1000.0 {
			(raw / 10.0) - 273.15
		} else if raw > 200.0 {
			raw - 273.15
		} else {
			raw
		};

		if c > -50.0 && c < 200.0 {
			values.push(c);
		}
	}

	if values.is_empty() {
		None
	} else {
		Some(values.iter().sum::<f32>() / values.len() as f32)
	}
}
