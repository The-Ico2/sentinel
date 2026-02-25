// ~/sentinel/sentinel-backend/src/ipc/sysdata/gpu.rs

use serde_json::{json, Value};
use std::{
	env,
	path::PathBuf,
	process::Command,
	sync::OnceLock,
};
use std::os::windows::process::CommandExt;
use sysinfo::{Components, System};

const CREATE_NO_WINDOW: u32 = 0x08000000;

pub fn get_gpu_json() -> Value {
	let components = Components::new_with_refreshed_list();

	// Try nvidia-smi first for comprehensive data (usage, VRAM, power, clocks, etc.)
	let nvidia_detailed = query_nvidia_smi_detailed();

	// WMI adapters as baseline / non-NVIDIA fallback
	let wmi_adapters = query_wmi_video_controllers();

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

	// Calculate shared GPU memory (Windows allocates ~half of system RAM for GPU sharing)
	let shared_gpu_memory_bytes: u64 = {
		let mut s = System::new();
		s.refresh_memory();
		s.total_memory() / 2
	};

	// Build the final adapters list — prefer nvidia-smi detailed, merge WMI info
	// Also includes WMI-only adapters (e.g. Intel iGPU not covered by nvidia-smi)
	let adapters: Vec<Value> = if !nvidia_detailed.is_empty() {
		let mut merged: Vec<Value> = nvidia_detailed.iter().map(|nv| {
			let nv_name = nv.get("name").and_then(|v| v.as_str()).unwrap_or("");
			let wmi_match = wmi_adapters.iter().find(|w| {
				let wn = w.get("name").and_then(|v| v.as_str()).unwrap_or("");
				!wn.is_empty() && !nv_name.is_empty()
					&& (wn.to_lowercase().contains(&nv_name.to_lowercase())
						|| nv_name.to_lowercase().contains(&wn.to_lowercase()))
			});
			let mut obj = nv.clone();
			if let Some(wmi) = wmi_match {
				let o = obj.as_object_mut().unwrap();
				for key in &["adapter_ram_bytes", "driver_version", "driver_date", "manufacturer",
					"physical_location", "pnp_device_id", "video_processor"] {
					if let Some(v) = wmi.get(*key).filter(|v| !v.is_null()) {
						o.entry(*key).or_insert_with(|| v.clone());
					}
				}
				if let Some(res) = wmi.get("current_resolution").filter(|v| !v.is_null()) {
					o.insert("current_resolution".into(), res.clone());
				}
			}
			obj.as_object_mut().unwrap().insert("shared_gpu_memory_bytes".into(), json!(shared_gpu_memory_bytes));
			obj
		}).collect();
		// Add WMI-only adapters not matched by nvidia-smi (e.g. Intel iGPU)
		for wmi in &wmi_adapters {
			let wn = wmi.get("name").and_then(|v| v.as_str()).unwrap_or("");
			let already = merged.iter().any(|m| {
				let mn = m.get("name").and_then(|v| v.as_str()).unwrap_or("");
				!mn.is_empty() && !wn.is_empty()
					&& (mn.to_lowercase().contains(&wn.to_lowercase())
						|| wn.to_lowercase().contains(&mn.to_lowercase()))
			});
			if !already && !wn.is_empty() {
				let mut w = wmi.clone();
				w.as_object_mut().unwrap().insert("shared_gpu_memory_bytes".into(), json!(shared_gpu_memory_bytes));
				merged.push(w);
			}
		}
		merged
	} else if !wmi_adapters.is_empty() {
		wmi_adapters.into_iter().map(|mut w| {
			w.as_object_mut().unwrap().insert("shared_gpu_memory_bytes".into(), json!(shared_gpu_memory_bytes));
			w
		}).collect()
	} else {
		let mut names = std::collections::BTreeSet::<String>::new();
		for sensor in &gpu_sensors {
			if let Some(label) = sensor.get("label").and_then(|v| v.as_str()) {
				if !label.trim().is_empty() {
					names.insert(label.trim().to_string());
				}
			}
		}
		names.into_iter().map(|name| json!({
			"name": name,
			"adapter_ram_bytes": Value::Null,
			"shared_gpu_memory_bytes": shared_gpu_memory_bytes,
			"source": "sensor-fallback",
		})).collect()
	};

	let mut all_sensors = gpu_sensors;
	for adapter in &adapters {
		if let Some(temp) = adapter.get("temperature_c").and_then(|v| v.as_f64()) {
			if temp > 0.0 {
				let name = adapter.get("name").and_then(|v| v.as_str()).unwrap_or("GPU");
				if !all_sensors.iter().any(|s| {
					s.get("label").and_then(|v| v.as_str()).unwrap_or("") == name
						&& s.get("source").and_then(|v| v.as_str()).unwrap_or("") == "nvidia-smi"
				}) {
					all_sensors.push(json!({
						"label": name,
						"temperature_c": temp,
						"source": "nvidia-smi",
					}));
				}
			}
		}
	}

	let average_c = average_temp(&all_sensors);

	// Top-level summary from primary adapter (first one)
	let primary = adapters.first();
	let name = primary.and_then(|a| a.get("name")).cloned().unwrap_or(Value::Null);
	let usage_percent = primary.and_then(|a| a.get("usage_percent")).cloned().unwrap_or(Value::Null);
	let vram_total_mb = primary.and_then(|a| a.get("vram_total_mb")).cloned().unwrap_or(Value::Null);
	let vram_used_mb = primary.and_then(|a| a.get("vram_used_mb")).cloned().unwrap_or(Value::Null);
	let vram_free_mb = primary.and_then(|a| a.get("vram_free_mb")).cloned().unwrap_or(Value::Null);
	let driver_version = primary.and_then(|a| a.get("driver_version")).cloned().unwrap_or(Value::Null);
	let driver_date = primary.and_then(|a| a.get("driver_date")).cloned().unwrap_or(Value::Null);
	let manufacturer = primary.and_then(|a| a.get("manufacturer")).cloned().unwrap_or(Value::Null);
	let physical_location = primary.and_then(|a| a.get("physical_location")).cloned().unwrap_or(Value::Null);
	let power_draw_w = primary.and_then(|a| a.get("power_draw_w")).cloned().unwrap_or(Value::Null);
	let fan_speed_percent = primary.and_then(|a| a.get("fan_speed_percent")).cloned().unwrap_or(Value::Null);
	let memory_usage_percent = primary.and_then(|a| a.get("memory_usage_percent")).cloned().unwrap_or(Value::Null);
	let encoder_usage = primary.and_then(|a| a.get("encoder_usage_percent")).cloned().unwrap_or(Value::Null);
	let decoder_usage = primary.and_then(|a| a.get("decoder_usage_percent")).cloned().unwrap_or(Value::Null);
	let clock_graphics = primary.and_then(|a| a.get("clock_graphics_mhz")).cloned().unwrap_or(Value::Null);
	let clock_memory = primary.and_then(|a| a.get("clock_memory_mhz")).cloned().unwrap_or(Value::Null);

	json!({
		"detected": !adapters.is_empty() || !all_sensors.is_empty(),
		"name": name,
		"usage_percent": usage_percent,
		"vram_total_mb": vram_total_mb,
		"vram_used_mb": vram_used_mb,
		"vram_free_mb": vram_free_mb,
		"memory_usage_percent": memory_usage_percent,
		"shared_gpu_memory_bytes": shared_gpu_memory_bytes,
		"driver_version": driver_version,
		"driver_date": driver_date,
		"manufacturer": manufacturer,
		"physical_location": physical_location,
		"temperature_c": average_c,
		"power_draw_w": power_draw_w,
		"fan_speed_percent": fan_speed_percent,
		"encoder_usage_percent": encoder_usage,
		"decoder_usage_percent": decoder_usage,
		"clock_graphics_mhz": clock_graphics,
		"clock_memory_mhz": clock_memory,
		"adapters": adapters,
		"temperature": {
			"average_c": average_c,
			"sensors": all_sensors.clone(),
		},
		"sensors": all_sensors,
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
	if count == 0 { 0.0 } else { sum / count as f32 }
}

/// Query nvidia-smi for comprehensive GPU stats.
fn query_nvidia_smi_detailed() -> Vec<Value> {
	let output = run_nvidia_smi()
		.creation_flags(CREATE_NO_WINDOW)
		.args([
			"--query-gpu=name,driver_version,utilization.gpu,utilization.memory,utilization.encoder,utilization.decoder,memory.total,memory.used,memory.free,temperature.gpu,power.draw,fan.speed,clocks.current.graphics,clocks.current.memory",
			"--format=csv,noheader,nounits",
		])
		.output();

	let Ok(output) = output else { return Vec::new() };
	if !output.status.success() { return Vec::new() }

	let text = String::from_utf8_lossy(&output.stdout);
	text.lines()
		.filter_map(|line| {
			let parts: Vec<&str> = line.split(',').map(|v| v.trim()).collect();
			if parts.len() < 14 { return None }

			let parse_f = |s: &str| -> Value {
				let s = s.trim();
				if s == "[Not Supported]" || s == "N/A" || s == "[N/A]" { return Value::Null }
				s.parse::<f64>().ok().map(|v| json!(v)).unwrap_or(Value::Null)
			};
			let parse_u = |s: &str| -> Value {
				let s = s.trim();
				if s == "[Not Supported]" || s == "N/A" || s == "[N/A]" { return Value::Null }
				s.parse::<u64>().ok().map(|v| json!(v)).unwrap_or(Value::Null)
			};

			Some(json!({
				"name": parts[0],
				"driver_version": if parts[1].contains("Not Supported") || parts[1] == "N/A" { Value::Null } else { json!(parts[1]) },
				"usage_percent": parse_f(parts[2]),
				"memory_usage_percent": parse_f(parts[3]),
				"encoder_usage_percent": parse_f(parts[4]),
				"decoder_usage_percent": parse_f(parts[5]),
				"vram_total_mb": parse_u(parts[6]),
				"vram_used_mb": parse_u(parts[7]),
				"vram_free_mb": parse_u(parts[8]),
				"temperature_c": parse_f(parts[9]),
				"power_draw_w": parse_f(parts[10]),
				"fan_speed_percent": parse_f(parts[11]),
				"clock_graphics_mhz": parse_u(parts[12]),
				"clock_memory_mhz": parse_u(parts[13]),
				"source": "nvidia-smi",
			}))
		})
		.collect()
}

fn query_wmi_video_controllers() -> Vec<Value> {
	let script = r#"$ErrorActionPreference='SilentlyContinue';
$gpus = Get-CimInstance Win32_VideoController;
foreach ($g in $gpus) {
	"Name=$($g.Name)";
	"AdapterRAM=$($g.AdapterRAM)";
	"DriverVersion=$($g.DriverVersion)";
	if ($g.DriverDate) { "DriverDate=$($g.DriverDate.ToString('yyyy-MM-dd'))" } else { "DriverDate=" };
	"PNPDeviceID=$($g.PNPDeviceID)";
	"AdapterCompatibility=$($g.AdapterCompatibility)";
	"VideoProcessor=$($g.VideoProcessor)";
	"CurrentHorizontalResolution=$($g.CurrentHorizontalResolution)";
	"CurrentVerticalResolution=$($g.CurrentVerticalResolution)";
	"CurrentRefreshRate=$($g.CurrentRefreshRate)";
	"Status=$($g.Status)";
	if ($g.PNPDeviceID) {
		$regPath = "HKLM:\SYSTEM\CurrentControlSet\Enum\$($g.PNPDeviceID)";
		$loc = (Get-ItemProperty -Path $regPath -Name 'LocationInformation' -EA SilentlyContinue).LocationInformation;
		if ($loc) { "LocationInformation=$loc" };
	}
	"";
}
"#;

	let output = Command::new("powershell")
		.creation_flags(CREATE_NO_WINDOW)
		.args(["-NoProfile", "-NonInteractive", "-Command", script])
		.output();

	let Ok(output) = output else { return Vec::new() };
	if !output.status.success() { return Vec::new() }

	let text = String::from_utf8_lossy(&output.stdout);
	parse_gpu_adapters(&text)
}

fn flush_gpu_adapter(adapters: &mut Vec<Value>, fields: &mut std::collections::HashMap<String, String>) {
	let name = fields.get("Name").map(|s| s.trim().to_string()).unwrap_or_default();
	if name.is_empty() {
		fields.clear();
		return;
	}

	let ram: Option<u64> = fields.get("AdapterRAM").and_then(|s| s.trim().parse().ok());
	let driver = fields.get("DriverVersion").map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
	let driver_date = fields.get("DriverDate").map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
	let manufacturer = fields.get("AdapterCompatibility").map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
	let processor = fields.get("VideoProcessor").map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
	let pnp = fields.get("PNPDeviceID").map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
	let h_res: Option<u32> = fields.get("CurrentHorizontalResolution").and_then(|s| s.trim().parse().ok());
	let v_res: Option<u32> = fields.get("CurrentVerticalResolution").and_then(|s| s.trim().parse().ok());
	let refresh: Option<u32> = fields.get("CurrentRefreshRate").and_then(|s| s.trim().parse().ok());
	let status = fields.get("Status").map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
	let location_raw = fields.get("LocationInformation").map(|s| s.trim().to_string()).filter(|s| !s.is_empty());

	// Parse physical location from "PCI bus X, device Y, function Z"
	let physical_location = location_raw.as_deref().map(|loc| {
		let parts: Vec<&str> = loc.split(',').collect();
		let mut bus = Value::Null;
		let mut device = Value::Null;
		let mut function = Value::Null;
		for part in &parts {
			let p = part.trim().to_lowercase();
			if p.contains("bus") {
				if let Some(n) = p.split_whitespace().filter_map(|w| w.parse::<u32>().ok()).next() {
					bus = json!(n);
				}
			} else if p.contains("device") {
				if let Some(n) = p.split_whitespace().filter_map(|w| w.parse::<u32>().ok()).next() {
					device = json!(n);
				}
			} else if p.contains("function") {
				if let Some(n) = p.split_whitespace().filter_map(|w| w.parse::<u32>().ok()).next() {
					function = json!(n);
				}
			}
		}
		json!({
			"bus": bus,
			"device": device,
			"function": function,
			"description": loc,
		})
	}).unwrap_or(Value::Null);

	let mut obj = json!({
		"name": name,
		"adapter_ram_bytes": ram,
		"driver_version": driver.as_deref(),
		"driver_date": driver_date.as_deref(),
		"manufacturer": manufacturer.as_deref(),
		"video_processor": processor.as_deref(),
		"current_resolution": if h_res.is_some() && v_res.is_some() {
			json!(format!("{}x{}", h_res.unwrap(), v_res.unwrap()))
		} else { Value::Null },
		"current_refresh_rate_hz": refresh,
		"status": status.as_deref(),
		"physical_location": physical_location,
		"source": "cim",
	});
	if let Some(p) = pnp.as_ref() {
		obj.as_object_mut().unwrap().insert("pnp_device_id".into(), json!(p));
	}
	adapters.push(obj);
	fields.clear();
}

fn parse_gpu_adapters(text: &str) -> Vec<Value> {
	let mut adapters = Vec::<Value>::new();
	let mut fields = std::collections::HashMap::<String, String>::new();

	for raw in text.lines() {
		let line = raw.trim();
		if line.is_empty() {
			flush_gpu_adapter(&mut adapters, &mut fields);
			continue;
		}
		if let Some((key, val)) = line.split_once('=') {
			fields.insert(key.trim().to_string(), val.to_string());
		}
	}
	flush_gpu_adapter(&mut adapters, &mut fields);
	adapters
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
			if let Ok(pf) = env::var("ProgramFiles") {
				candidates.push(PathBuf::from(&pf).join("NVIDIA Corporation").join("NVSMI").join("nvidia-smi.exe"));
			}
			if let Ok(pf86) = env::var("ProgramFiles(x86)") {
				candidates.push(PathBuf::from(&pf86).join("NVIDIA Corporation").join("NVSMI").join("nvidia-smi.exe"));
			}
			if let Ok(windir) = env::var("SystemRoot") {
				candidates.push(PathBuf::from(windir).join("System32").join("nvidia-smi.exe"));
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
