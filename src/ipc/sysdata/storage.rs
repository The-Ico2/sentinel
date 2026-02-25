// ~/sentinel/sentinel-backend/src/ipc/sysdata/storage.rs

use serde_json::{json, Value};
use std::os::windows::process::CommandExt;
use std::process::Command;
use sysinfo::Disks;

const CREATE_NO_WINDOW: u32 = 0x08000000;

pub fn get_storage_json() -> Value {
	let disks = Disks::new_with_refreshed_list();
	let physical_disks = query_physical_disks();

	let mut total_bytes: u64 = 0;
	let mut available_bytes: u64 = 0;

	let list: Vec<Value> = disks
		.list()
		.iter()
		.map(|disk| {
			let disk_total = disk.total_space();
			let disk_available = disk.available_space();
			let disk_used = disk_total.saturating_sub(disk_available);
			total_bytes = total_bytes.saturating_add(disk_total);
			available_bytes = available_bytes.saturating_add(disk_available);

			let usage_percent = if disk_total == 0 {
				0.0
			} else {
				(disk_used as f64 / disk_total as f64) * 100.0
			};

			let mount = disk.mount_point().to_string_lossy().to_string();

			// Try to find matching physical disk info by mount letter
			let letter = mount.trim_end_matches('\\').trim_end_matches('/');
			let phys_match = physical_disks.iter().find(|pd| {
				if let Some(letters) = pd.get("drive_letters").and_then(|v| v.as_array()) {
					letters.iter().any(|l| {
						l.as_str().map(|s| s.eq_ignore_ascii_case(letter)).unwrap_or(false)
					})
				} else {
					false
				}
			});

			let mut entry = json!({
				"name": disk.name().to_string_lossy(),
				"mount": mount,
				"kind": format!("{:?}", disk.kind()),
				"file_system": disk.file_system().to_string_lossy(),
				"removable": disk.is_removable(),
				"total_bytes": disk_total,
				"available_bytes": disk_available,
				"used_bytes": disk_used,
				"usage_percent": usage_percent,
			});

			if let Some(pd) = phys_match {
				let obj = entry.as_object_mut().unwrap();
				if let Some(v) = pd.get("model") { obj.insert("model".into(), v.clone()); }
				if let Some(v) = pd.get("media_type") { obj.insert("media_type".into(), v.clone()); }
				if let Some(v) = pd.get("bus_type") { obj.insert("bus_type".into(), v.clone()); }
				if let Some(v) = pd.get("serial_number") { obj.insert("serial_number".into(), v.clone()); }
				if let Some(v) = pd.get("firmware_version") { obj.insert("firmware_version".into(), v.clone()); }
				if let Some(v) = pd.get("disk_number") { obj.insert("disk_number".into(), v.clone()); }
				if let Some(v) = pd.get("capacity_bytes") { obj.insert("physical_capacity_bytes".into(), v.clone()); }
				if let Some(v) = pd.get("system_disk") { obj.insert("system_disk".into(), v.clone()); }
				if let Some(v) = pd.get("page_file_disk") { obj.insert("page_file_disk".into(), v.clone()); }
				if let Some(v) = pd.get("health_status") { obj.insert("health_status".into(), v.clone()); }
			}

			entry
		})
		.collect();

	let total_used = total_bytes.saturating_sub(available_bytes);
	let overall_percent = if total_bytes == 0 {
		0.0
	} else {
		(total_used as f64 / total_bytes as f64) * 100.0
	};

	json!({
		"total_bytes": total_bytes,
		"available_bytes": available_bytes,
		"used_bytes": total_used,
		"usage_percent": overall_percent,
		"disk_count": list.len(),
		"physical_disks": physical_disks,
		"disks": list,
	})
}

fn query_physical_disks() -> Vec<Value> {
	let script = r#"$ErrorActionPreference='SilentlyContinue';
$physDisks = Get-PhysicalDisk -ErrorAction SilentlyContinue;
$partitions = Get-Partition -ErrorAction SilentlyContinue;
$volumes = Get-Volume -ErrorAction SilentlyContinue;
foreach ($pd in $physDisks) {
	"BEGIN_DISK";
	"Model=$($pd.FriendlyName)";
	"MediaType=$($pd.MediaType)";
	"BusType=$($pd.BusType)";
	"SerialNumber=$($pd.SerialNumber)";
	"FirmwareVersion=$($pd.FirmwareVersion)";
	"Size=$($pd.Size)";
	"HealthStatus=$($pd.HealthStatus)";
	"DeviceId=$($pd.DeviceId)";
	$diskNum = $pd.DeviceId;
	"DiskNumber=$diskNum";
	$parts = $partitions | Where-Object { $_.DiskNumber -eq $diskNum };
	$letters = @();
	$isSysDisk = $false;
	$isPageFile = $false;
	foreach ($p in $parts) {
		if ($p.DriveLetter) {
			$letters += "$($p.DriveLetter):";
			if ($p.IsSystem -or $p.IsBoot) { $isSysDisk = $true }
		}
		if ($p.Type -eq 'IU') { $isPageFile = $true }
	}
	"DriveLetters=$($letters -join ',')";
	"SystemDisk=$isSysDisk";
	$sysLetter = ($env:SystemDrive).TrimEnd(':')
	foreach ($l in $letters) {
		if ($l.TrimEnd(':') -eq $sysLetter) { $isSysDisk = $true }
	}
	"SystemDisk=$isSysDisk";
	$pageLetter = $null;
	$pageFile = Get-CimInstance Win32_PageFileUsage -ErrorAction SilentlyContinue | Select-Object -First 1;
	if ($pageFile) {
		$pageLetter = $pageFile.Name.Substring(0,2);
	}
	$isPF = $false;
	foreach ($l in $letters) {
		if ($pageLetter -and $l -eq $pageLetter) { $isPF = $true }
	}
	"PageFileDisk=$isPF";
	"END_DISK";
}
"#;

	let output = Command::new("powershell")
		.creation_flags(CREATE_NO_WINDOW)
		.args(["-NoProfile", "-NonInteractive", "-Command", script])
		.output();

	let Ok(output) = output else { return Vec::new() };
	if !output.status.success() { return Vec::new() }

	let text = String::from_utf8_lossy(&output.stdout);
	let mut result = Vec::new();
	let mut in_disk = false;
	let mut model = String::new();
	let mut media_type = String::new();
	let mut bus_type = String::new();
	let mut serial = String::new();
	let mut firmware = String::new();
	let mut size: Option<u64> = None;
	let mut health = String::new();
	let mut disk_number: Option<u32> = None;
	let mut drive_letters = Vec::<String>::new();
	let mut system_disk = false;
	let mut page_file_disk = false;

	for raw in text.lines() {
		let line = raw.trim();
		if line == "BEGIN_DISK" {
			in_disk = true;
			model.clear(); media_type.clear(); bus_type.clear(); serial.clear();
			firmware.clear(); health.clear(); drive_letters.clear();
			size = None; disk_number = None;
			system_disk = false; page_file_disk = false;
			continue;
		}
		if line == "END_DISK" {
			if in_disk && !model.is_empty() {
				result.push(json!({
					"model": model,
					"media_type": if media_type.is_empty() { Value::Null } else { json!(media_type) },
					"bus_type": if bus_type.is_empty() { Value::Null } else { json!(bus_type) },
					"serial_number": if serial.is_empty() { Value::Null } else { json!(serial.trim()) },
					"firmware_version": if firmware.is_empty() { Value::Null } else { json!(firmware.trim()) },
					"capacity_bytes": size,
					"health_status": if health.is_empty() { Value::Null } else { json!(health) },
					"disk_number": disk_number,
					"drive_letters": drive_letters,
					"system_disk": system_disk,
					"page_file_disk": page_file_disk,
				}));
			}
			in_disk = false;
			continue;
		}
		if !in_disk { continue; }
		if let Some(v) = line.strip_prefix("Model=") { model = v.trim().to_string(); }
		else if let Some(v) = line.strip_prefix("MediaType=") { media_type = v.trim().to_string(); }
		else if let Some(v) = line.strip_prefix("BusType=") { bus_type = v.trim().to_string(); }
		else if let Some(v) = line.strip_prefix("SerialNumber=") { serial = v.trim().to_string(); }
		else if let Some(v) = line.strip_prefix("FirmwareVersion=") { firmware = v.trim().to_string(); }
		else if let Some(v) = line.strip_prefix("Size=") { size = v.trim().parse().ok(); }
		else if let Some(v) = line.strip_prefix("HealthStatus=") { health = v.trim().to_string(); }
		else if let Some(v) = line.strip_prefix("DiskNumber=") { disk_number = v.trim().parse().ok(); }
		else if let Some(v) = line.strip_prefix("DriveLetters=") {
			drive_letters = v.trim().split(',').filter(|s| !s.is_empty()).map(|s| s.trim().to_string()).collect();
		}
		else if let Some(v) = line.strip_prefix("SystemDisk=") { if v.trim().eq_ignore_ascii_case("true") { system_disk = true; } }
		else if let Some(v) = line.strip_prefix("PageFileDisk=") { page_file_disk = v.trim().eq_ignore_ascii_case("true"); }
	}

	result
}
