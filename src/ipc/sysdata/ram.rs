// ~/sentinel/sentinel-backend/src/ipc/sysdata/ram.rs

use serde_json::{json, Value};
use std::os::windows::process::CommandExt;
use std::process::Command;
use sysinfo::System;

const CREATE_NO_WINDOW: u32 = 0x08000000;

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

	// Query hardware RAM details (speed, slots, form factor, etc.)
	let hw = query_ram_hardware();

	// Query OS memory counters (committed, cached, paged/non-paged pool, hardware reserved)
	let counters = query_memory_counters(total);

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
		"speed_mhz": hw.get("speed_mhz").cloned().unwrap_or(Value::Null),
		"form_factor": hw.get("form_factor").cloned().unwrap_or(Value::Null),
		"slots_used": hw.get("slots_used").cloned().unwrap_or(Value::Null),
		"slots_total": hw.get("slots_total").cloned().unwrap_or(Value::Null),
		"memory_type": hw.get("memory_type").cloned().unwrap_or(Value::Null),
		"sticks": hw.get("sticks").cloned().unwrap_or(json!([])),
		"hardware_reserved_bytes": counters.get("hardware_reserved_bytes").cloned().unwrap_or(Value::Null),
		"committed_bytes": counters.get("committed_bytes").cloned().unwrap_or(Value::Null),
		"commit_limit_bytes": counters.get("commit_limit_bytes").cloned().unwrap_or(Value::Null),
		"cached_bytes": counters.get("cached_bytes").cloned().unwrap_or(Value::Null),
		"paged_pool_bytes": counters.get("paged_pool_bytes").cloned().unwrap_or(Value::Null),
		"non_paged_pool_bytes": counters.get("non_paged_pool_bytes").cloned().unwrap_or(Value::Null),
		"compressed_bytes": counters.get("compressed_bytes").cloned().unwrap_or(Value::Null),
		"top_processes": top_processes,
	})
}

fn query_ram_hardware() -> Value {
	let script = r#"$ErrorActionPreference='SilentlyContinue';
$sticks = Get-CimInstance Win32_PhysicalMemory -ErrorAction SilentlyContinue;
$totalSlots = (Get-CimInstance Win32_PhysicalMemoryArray -ErrorAction SilentlyContinue | Select-Object -First 1).MemoryDevices;
$usedSlots = @($sticks).Count;
"SlotsTotal=$totalSlots";
"SlotsUsed=$usedSlots";
$idx = 0;
foreach ($s in $sticks) {
	"BEGIN_STICK";
	"Capacity=$($s.Capacity)";
	"Speed=$($s.Speed)";
	"ConfiguredClockSpeed=$($s.ConfiguredClockSpeed)";
	"FormFactor=$($s.FormFactor)";
	"MemoryType=$($s.SMBIOSMemoryType)";
	"Manufacturer=$($s.Manufacturer)";
	"PartNumber=$($s.PartNumber)";
	"SerialNumber=$($s.SerialNumber)";
	"BankLabel=$($s.BankLabel)";
	"DeviceLocator=$($s.DeviceLocator)";
	"DataWidth=$($s.DataWidth)";
	"TotalWidth=$($s.TotalWidth)";
	"END_STICK";
	$idx++;
}
"#;

	let output = Command::new("powershell")
		.creation_flags(CREATE_NO_WINDOW)
		.args(["-NoProfile", "-NonInteractive", "-Command", script])
		.output();

	let Ok(output) = output else { return json!({}) };
	if !output.status.success() { return json!({}) }

	let text = String::from_utf8_lossy(&output.stdout);
	let mut slots_total: Option<u32> = None;
	let mut slots_used: Option<u32> = None;
	let mut sticks = Vec::<Value>::new();

	// Stick parsing state
	let mut in_stick = false;
	let mut capacity: Option<u64> = None;
	let mut speed: Option<u32> = None;
	let mut configured_speed: Option<u32> = None;
	let mut form_factor_code: Option<u32> = None;
	let mut mem_type_code: Option<u32> = None;
	let mut manufacturer = String::new();
	let mut part_number = String::new();
	let mut serial = String::new();
	let mut bank = String::new();
	let mut locator = String::new();
	let mut data_width: Option<u32> = None;
	let mut total_width: Option<u32> = None;

	for raw in text.lines() {
		let line = raw.trim();
		if let Some(v) = line.strip_prefix("SlotsTotal=") { slots_total = v.trim().parse().ok(); continue; }
		if let Some(v) = line.strip_prefix("SlotsUsed=") { slots_used = v.trim().parse().ok(); continue; }

		if line == "BEGIN_STICK" {
			in_stick = true;
			capacity = None; speed = None; configured_speed = None;
			form_factor_code = None; mem_type_code = None;
			manufacturer.clear(); part_number.clear(); serial.clear();
			bank.clear(); locator.clear();
			data_width = None; total_width = None;
			continue;
		}
		if line == "END_STICK" {
			if in_stick {
				let ff = form_factor_code.map(|c| match c {
					8 => "DIMM", 12 => "SODIMM", 9 => "RIMM",
					13 => "SRIMM", _ => "Unknown"
				}).unwrap_or("Unknown");

				let mt = mem_type_code.map(|c| match c {
					20 => "DDR", 21 => "DDR2", 24 => "DDR3",
					26 => "DDR4", 34 => "DDR5", _ => "Unknown"
				}).unwrap_or("Unknown");

				sticks.push(json!({
					"capacity_bytes": capacity,
					"speed_mhz": speed,
					"configured_speed_mhz": configured_speed,
					"form_factor": ff,
					"memory_type": mt,
					"manufacturer": if manufacturer.is_empty() { Value::Null } else { json!(manufacturer.trim()) },
					"part_number": if part_number.is_empty() { Value::Null } else { json!(part_number.trim()) },
					"serial_number": if serial.is_empty() { Value::Null } else { json!(serial.trim()) },
					"bank_label": if bank.is_empty() { Value::Null } else { json!(bank.trim()) },
					"device_locator": if locator.is_empty() { Value::Null } else { json!(locator.trim()) },
					"data_width_bits": data_width,
					"total_width_bits": total_width,
				}));
			}
			in_stick = false;
			continue;
		}
		if !in_stick { continue; }
		if let Some(v) = line.strip_prefix("Capacity=") { capacity = v.trim().parse().ok(); }
		else if let Some(v) = line.strip_prefix("Speed=") { speed = v.trim().parse().ok(); }
		else if let Some(v) = line.strip_prefix("ConfiguredClockSpeed=") { configured_speed = v.trim().parse().ok(); }
		else if let Some(v) = line.strip_prefix("FormFactor=") { form_factor_code = v.trim().parse().ok(); }
		else if let Some(v) = line.strip_prefix("MemoryType=") { mem_type_code = v.trim().parse().ok(); }
		else if let Some(v) = line.strip_prefix("Manufacturer=") { manufacturer = v.trim().to_string(); }
		else if let Some(v) = line.strip_prefix("PartNumber=") { part_number = v.trim().to_string(); }
		else if let Some(v) = line.strip_prefix("SerialNumber=") { serial = v.trim().to_string(); }
		else if let Some(v) = line.strip_prefix("BankLabel=") { bank = v.trim().to_string(); }
		else if let Some(v) = line.strip_prefix("DeviceLocator=") { locator = v.trim().to_string(); }
		else if let Some(v) = line.strip_prefix("DataWidth=") { data_width = v.trim().parse().ok(); }
		else if let Some(v) = line.strip_prefix("TotalWidth=") { total_width = v.trim().parse().ok(); }
	}

	// Derive summary from first stick
	let first = sticks.first();
	let speed_mhz = first.and_then(|s| s.get("configured_speed_mhz").or(s.get("speed_mhz"))).cloned().unwrap_or(Value::Null);
	let form_factor = first.and_then(|s| s.get("form_factor")).cloned().unwrap_or(Value::Null);
	let memory_type = first.and_then(|s| s.get("memory_type")).cloned().unwrap_or(Value::Null);

	json!({
		"speed_mhz": speed_mhz,
		"form_factor": form_factor,
		"memory_type": memory_type,
		"slots_used": slots_used,
		"slots_total": slots_total,
		"sticks": sticks,
	})
}

fn query_memory_counters(total_physical: u64) -> Value {
	let script = r#"$ErrorActionPreference='SilentlyContinue';
$os = Get-CimInstance Win32_OperatingSystem -ErrorAction SilentlyContinue | Select-Object -First 1;
if ($os) {
	"TotalVisibleMemorySize=$($os.TotalVisibleMemorySize)";
	"FreePhysicalMemory=$($os.FreePhysicalMemory)";
	"TotalVirtualMemorySize=$($os.TotalVirtualMemorySize)";
	"FreeVirtualMemory=$($os.FreeVirtualMemory)";
}
$perf = Get-CimInstance Win32_PerfFormattedData_PerfOS_Memory -ErrorAction SilentlyContinue | Select-Object -First 1;
if ($perf) {
	"CommittedBytes=$($perf.CommittedBytes)";
	"CommitLimit=$($perf.CommitLimit)";
	"CacheBytes=$($perf.CacheBytes)";
	"PoolPagedBytes=$($perf.PoolPagedBytes)";
	"PoolNonpagedBytes=$($perf.PoolNonpagedBytes)";
}
$cs = Get-CimInstance Win32_ComputerSystem -ErrorAction SilentlyContinue | Select-Object -First 1;
if ($cs) {
	"TotalPhysicalMemory=$($cs.TotalPhysicalMemory)";
}
$compressed = (Get-Counter '\Memory\Compressed Pages' -ErrorAction SilentlyContinue).CounterSamples[0].CookedValue;
if ($compressed) { "CompressedPages=$compressed" }
"#;

	let output = Command::new("powershell")
		.creation_flags(CREATE_NO_WINDOW)
		.args(["-NoProfile", "-NonInteractive", "-Command", script])
		.output();

	let Ok(output) = output else { return json!({}) };
	if !output.status.success() { return json!({}) }

	let text = String::from_utf8_lossy(&output.stdout);
	let mut total_visible_kb: Option<u64> = None;
	let mut committed: Option<u64> = None;
	let mut commit_limit: Option<u64> = None;
	let mut cache_bytes: Option<u64> = None;
	let mut pool_paged: Option<u64> = None;
	let mut pool_nonpaged: Option<u64> = None;
	let mut cs_total_physical: Option<u64> = None;
	let mut compressed_pages: Option<f64> = None;

	for raw in text.lines() {
		let line = raw.trim();
		if let Some(v) = line.strip_prefix("TotalVisibleMemorySize=") { total_visible_kb = v.trim().parse().ok(); }
		else if let Some(v) = line.strip_prefix("CommittedBytes=") { committed = v.trim().parse().ok(); }
		else if let Some(v) = line.strip_prefix("CommitLimit=") { commit_limit = v.trim().parse().ok(); }
		else if let Some(v) = line.strip_prefix("CacheBytes=") { cache_bytes = v.trim().parse().ok(); }
		else if let Some(v) = line.strip_prefix("PoolPagedBytes=") { pool_paged = v.trim().parse().ok(); }
		else if let Some(v) = line.strip_prefix("PoolNonpagedBytes=") { pool_nonpaged = v.trim().parse().ok(); }
		else if let Some(v) = line.strip_prefix("TotalPhysicalMemory=") { cs_total_physical = v.trim().parse().ok(); }
		else if let Some(v) = line.strip_prefix("CompressedPages=") { compressed_pages = v.trim().parse().ok(); }
	}

	// Hardware reserved = physical installed - OS visible
	let hardware_reserved = cs_total_physical.and_then(|installed| {
		let visible = total_visible_kb.map(|kb| kb * 1024).unwrap_or(total_physical);
		if installed > visible {
			Some(installed - visible)
		} else {
			None
		}
	});

	// Compressed memory: pages * 4096
	let compressed_bytes = compressed_pages.map(|p| (p as u64) * 4096);

	json!({
		"hardware_reserved_bytes": hardware_reserved,
		"committed_bytes": committed,
		"commit_limit_bytes": commit_limit,
		"cached_bytes": cache_bytes,
		"paged_pool_bytes": pool_paged,
		"non_paged_pool_bytes": pool_nonpaged,
		"compressed_bytes": compressed_bytes,
	})
}
