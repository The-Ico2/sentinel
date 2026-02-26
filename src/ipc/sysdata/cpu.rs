// ~/sentinel/sentinel-backend/src/ipc/sysdata/cpu.rs

use serde_json::{json, Value};
use std::cell::RefCell;
use std::os::windows::process::CommandExt;
use std::process::Command;
use sysinfo::Components;
use sysinfo::ProcessesToUpdate;
use sysinfo::System;
use windows::Win32::Foundation::FILETIME;
use windows::Win32::System::Threading::GetSystemTimes;

const CREATE_NO_WINDOW: u32 = 0x08000000;

thread_local! {
    static CPU_SYS: RefCell<System> = RefCell::new({
        let mut sys = System::new();
        sys.refresh_cpu_all();
        sys.refresh_processes(ProcessesToUpdate::All, true);
        sys
    });
	static CPU_TIMES: RefCell<Option<(u64, u64, u64)>> = const { RefCell::new(None) };
}

pub fn get_cpu_json() -> Value {
	let (logical_cores, avg_usage, avg_frequency_mhz, brand, vendor_id, per_core, process_count) =
		CPU_SYS.with(|cell| {
			let mut sys = cell.borrow_mut();

			// CPU usage in sysinfo is delta-based. Reusing the same System instance
			// across pulls yields stable task-manager-like readings.
			sys.refresh_cpu_all();
			sys.refresh_processes(ProcessesToUpdate::All, true);

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

			let process_count = sys.processes().len();

			(
				logical_cores,
				avg_usage,
				avg_frequency_mhz,
				brand,
				vendor_id,
				per_core,
				process_count,
			)
		});

	let physical_cores = System::physical_core_count().unwrap_or(0);

	let usage_percent = query_system_cpu_usage_percent()
		.or_else(query_perf_cpu_usage_percent)
		.unwrap_or(avg_usage);

	let cpu_temp = get_cpu_temperature_json();

	let uptime_seconds = System::uptime();
	let boot_time_unix = System::boot_time();

	let arch = std::env::consts::ARCH;

	// Query additional CPU details from WMI (base speed, sockets, virtualization, caches, handles, threads)
	let cpu_details = query_cpu_details();

	json!({
		"brand": brand,
		"vendor_id": vendor_id,
		"arch": arch,
		"logical_cores": logical_cores,
		"physical_cores": physical_cores,
		"usage_percent": usage_percent,
		"frequency_mhz": avg_frequency_mhz,
		"base_frequency_mhz": cpu_details.get("base_frequency_mhz").cloned().unwrap_or(Value::Null),
		"max_frequency_mhz": cpu_details.get("max_frequency_mhz").cloned().unwrap_or(Value::Null),
		"sockets": cpu_details.get("sockets").cloned().unwrap_or(json!(1)),
		"virtualization": cpu_details.get("virtualization").cloned().unwrap_or(Value::Null),
		"l1_cache_kb": cpu_details.get("l1_cache_kb").cloned().unwrap_or(Value::Null),
		"l2_cache_kb": cpu_details.get("l2_cache_kb").cloned().unwrap_or(Value::Null),
		"l3_cache_kb": cpu_details.get("l3_cache_kb").cloned().unwrap_or(Value::Null),
		"thread_count": cpu_details.get("thread_count").cloned().unwrap_or(Value::Null),
		"handle_count": cpu_details.get("handle_count").cloned().unwrap_or(Value::Null),
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

fn query_system_cpu_usage_percent() -> Option<f32> {
	fn ft_to_u64(ft: FILETIME) -> u64 {
		((ft.dwHighDateTime as u64) << 32) | (ft.dwLowDateTime as u64)
	}

	unsafe {
		let mut idle = FILETIME::default();
		let mut kernel = FILETIME::default();
		let mut user = FILETIME::default();

		if GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)).is_err() {
			return None;
		}

		let idle_now = ft_to_u64(idle);
		let kernel_now = ft_to_u64(kernel);
		let user_now = ft_to_u64(user);

		CPU_TIMES.with(|cell| {
			let mut prev = cell.borrow_mut();
			if let Some((idle_prev, kernel_prev, user_prev)) = *prev {
				let idle_delta = idle_now.saturating_sub(idle_prev);
				let kernel_delta = kernel_now.saturating_sub(kernel_prev);
				let user_delta = user_now.saturating_sub(user_prev);
				let total_delta = kernel_delta.saturating_add(user_delta);

				*prev = Some((idle_now, kernel_now, user_now));

				if total_delta == 0 {
					return None;
				}

				let busy = total_delta.saturating_sub(idle_delta);
				let pct = (busy as f64 * 100.0 / total_delta as f64) as f32;
				Some(pct.clamp(0.0, 100.0))
			} else {
				*prev = Some((idle_now, kernel_now, user_now));
				None
			}
		})
	}
}

fn query_perf_cpu_usage_percent() -> Option<f32> {
	let script = r#"$ErrorActionPreference='SilentlyContinue';
$sample = Get-Counter '\Processor(_Total)\% Processor Time' -ErrorAction SilentlyContinue |
	Select-Object -ExpandProperty CounterSamples |
	Select-Object -First 1 -ExpandProperty CookedValue;
if ($sample -ne $null) {
	$sample.ToString([System.Globalization.CultureInfo]::InvariantCulture)
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
	for line in text.lines() {
		if let Ok(v) = line.trim().parse::<f32>() {
			if v.is_finite() {
				return Some(v.clamp(0.0, 100.0));
			}
		}
	}

	None
}

/// Query CPU details not available from sysinfo: base speed, caches, sockets, virtualization, handles.
fn query_cpu_details() -> Value {
	let script = r#"$ErrorActionPreference='SilentlyContinue';
$cpu = Get-CimInstance Win32_Processor -ErrorAction SilentlyContinue | Select-Object -First 1;
if ($cpu) {
	"MaxClockSpeed=$($cpu.MaxClockSpeed)";
	"NumberOfCores=$($cpu.NumberOfCores)";
	"NumberOfLogicalProcessors=$($cpu.NumberOfLogicalProcessors)";
	"SocketDesignation=$($cpu.SocketDesignation)";
	"L2CacheSize=$($cpu.L2CacheSize)";
	"L3CacheSize=$($cpu.L3CacheSize)";
	"VirtualizationFirmwareEnabled=$($cpu.VirtualizationFirmwareEnabled)";
	"VMMonitorModeExtensions=$($cpu.VMMonitorModeExtensions)";
	"Manufacturer=$($cpu.Manufacturer)";
	"Stepping=$($cpu.Stepping)";
}
$socketCount = @(Get-CimInstance Win32_Processor -ErrorAction SilentlyContinue).Count;
if (-not $socketCount) { $socketCount = 1 }
"Sockets=$socketCount";
$caches = Get-CimInstance Win32_CacheMemory -ErrorAction SilentlyContinue;
$totalL1 = 0;
foreach ($c in $caches) { if ($c.Purpose -match 'L1' -or $c.Level -eq 3) { $totalL1 += $c.MaxCacheSize } }
if ($totalL1 -gt 0) { "L1Total=$totalL1" }
$handles = (Get-Process -ErrorAction SilentlyContinue | Measure-Object -Property HandleCount -Sum).Sum;
"TotalHandles=$handles";
$threads = (Get-CimInstance Win32_OperatingSystem -ErrorAction SilentlyContinue).NumberOfProcesses;
try { $threads = (Get-Process -ErrorAction SilentlyContinue | Measure-Object -Property Threads -Sum -ErrorAction SilentlyContinue).Sum } catch {};
if (-not $threads) { $threads = (Get-CimInstance Win32_PerfFormattedData_PerfOS_System -ErrorAction SilentlyContinue).Threads };
"TotalThreads=$threads";
"#;

	let output = Command::new("powershell")
		.creation_flags(CREATE_NO_WINDOW)
		.args(["-NoProfile", "-NonInteractive", "-Command", script])
		.output();

	let Ok(output) = output else { return json!({}) };
	if !output.status.success() { return json!({}) }

	let text = String::from_utf8_lossy(&output.stdout);
	let mut max_clock_mhz: Option<u64> = None;
	let mut sockets: Option<u64> = None;
	let mut socket_designation: Option<String> = None;
	let mut l1_total_kb: Option<u64> = None;
	let mut l2_cache_kb: Option<u64> = None;
	let mut l3_cache_kb: Option<u64> = None;
	let mut virt_fw: Option<bool> = None;
	let mut vm_ext: Option<bool> = None;
	let mut total_handles: Option<u64> = None;
	let mut total_threads: Option<u64> = None;
	let mut manufacturer: Option<String> = None;
	let mut stepping: Option<String> = None;

	for raw in text.lines() {
		let line = raw.trim();
		if let Some(v) = line.strip_prefix("MaxClockSpeed=") { max_clock_mhz = v.trim().parse().ok(); }
		else if let Some(v) = line.strip_prefix("Sockets=") { sockets = v.trim().parse().ok(); }
		else if let Some(v) = line.strip_prefix("SocketDesignation=") { socket_designation = Some(v.trim().to_string()); }
		else if let Some(v) = line.strip_prefix("L1Total=") { l1_total_kb = v.trim().parse().ok(); }
		else if let Some(v) = line.strip_prefix("L2CacheSize=") { l2_cache_kb = v.trim().parse().ok(); }
		else if let Some(v) = line.strip_prefix("L3CacheSize=") { l3_cache_kb = v.trim().parse().ok(); }
		else if let Some(v) = line.strip_prefix("VirtualizationFirmwareEnabled=") { virt_fw = Some(v.trim().eq_ignore_ascii_case("true")); }
		else if let Some(v) = line.strip_prefix("VMMonitorModeExtensions=") { vm_ext = Some(v.trim().eq_ignore_ascii_case("true")); }
		else if let Some(v) = line.strip_prefix("TotalHandles=") { total_handles = v.trim().parse().ok(); }
		else if let Some(v) = line.strip_prefix("TotalThreads=") { total_threads = v.trim().parse().ok(); }
		else if let Some(v) = line.strip_prefix("Manufacturer=") { manufacturer = Some(v.trim().to_string()); }
		else if let Some(v) = line.strip_prefix("Stepping=") { stepping = Some(v.trim().to_string()); }
	}

	let virtualization = match (virt_fw, vm_ext) {
		(Some(true), _) | (_, Some(true)) => json!("Enabled"),
		(Some(false), _) => json!("Disabled"),
		_ => Value::Null,
	};

	json!({
		"base_frequency_mhz": max_clock_mhz,
		"max_frequency_mhz": max_clock_mhz,
		"sockets": sockets.unwrap_or(1),
		"socket_designation": socket_designation,
		"l1_cache_kb": l1_total_kb,
		"l2_cache_kb": l2_cache_kb,
		"l3_cache_kb": l3_cache_kb,
		"virtualization": virtualization,
		"handle_count": total_handles,
		"thread_count": total_threads,
		"manufacturer": manufacturer,
		"stepping": stepping,
	})
}
