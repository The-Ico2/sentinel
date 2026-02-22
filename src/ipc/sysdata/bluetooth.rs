// ~/sentinel/sentinel-backend/src/ipc/sysdata/bluetooth.rs

use serde_json::{json, Value};
use std::os::windows::process::CommandExt;
use std::process::Command;

const CREATE_NO_WINDOW: u32 = 0x08000000;

pub fn get_bluetooth_json() -> Value {
	let adapter = get_bluetooth_adapter();
	let devices = get_bluetooth_devices();

	json!({
		"adapter": adapter,
		"devices": devices,
	})
}

fn get_bluetooth_adapter() -> Value {
	let script = r#"$ErrorActionPreference='SilentlyContinue';
$radio = Get-PnpDevice -Class Bluetooth -ErrorAction SilentlyContinue | Where-Object { $_.FriendlyName -match 'Bluetooth|Radio' -and $_.Status -eq 'OK' } | Select-Object -First 1;
if ($radio) {
	"Name=$($radio.FriendlyName)";
	"Status=$($radio.Status)";
	"InstanceId=$($radio.InstanceId)";
	"Present=true";
} else {
	$any = Get-PnpDevice -Class Bluetooth -ErrorAction SilentlyContinue | Select-Object -First 1;
	if ($any) {
		"Name=$($any.FriendlyName)";
		"Status=$($any.Status)";
		"InstanceId=$($any.InstanceId)";
		"Present=true";
	} else {
		"Present=false";
	}
}"#;

	let output = Command::new("powershell")
		.creation_flags(CREATE_NO_WINDOW)
		.args(["-NoProfile", "-NonInteractive", "-Command", script])
		.output();

	let Ok(output) = output else {
		return json!({ "present": false });
	};
	if !output.status.success() {
		return json!({ "present": false });
	}

	let text = String::from_utf8_lossy(&output.stdout);
	let mut name = String::new();
	let mut status = String::new();
	let mut instance_id = String::new();
	let mut present = false;

	for raw in text.lines() {
		let line = raw.trim();
		if let Some(v) = line.strip_prefix("Name=") {
			name = v.trim().to_string();
		} else if let Some(v) = line.strip_prefix("Status=") {
			status = v.trim().to_string();
		} else if let Some(v) = line.strip_prefix("InstanceId=") {
			instance_id = v.trim().to_string();
		} else if let Some(v) = line.strip_prefix("Present=") {
			present = v.trim() == "true";
		}
	}

	json!({
		"present": present,
		"name": if name.is_empty() { Value::Null } else { json!(name) },
		"status": if status.is_empty() { Value::Null } else { json!(status) },
		"instance_id": if instance_id.is_empty() { Value::Null } else { json!(instance_id) },
	})
}

fn get_bluetooth_devices() -> Vec<Value> {
	let script = r#"$ErrorActionPreference='SilentlyContinue';
$devices = Get-PnpDevice -Class Bluetooth -ErrorAction SilentlyContinue | Where-Object { $_.FriendlyName -notmatch 'Bluetooth|Radio|Enumerator|Microsoft' };
foreach ($d in $devices) {
	"BEGIN_DEVICE";
	"Name=$($d.FriendlyName)";
	"Status=$($d.Status)";
	"Class=$($d.Class)";
	"InstanceId=$($d.InstanceId)";
	"END_DEVICE";
}
$btDevices = Get-CimInstance -Namespace root\cimv2 -ClassName Win32_PnPEntity -Filter "PNPClass='Bluetooth'" -ErrorAction SilentlyContinue | Where-Object { $_.Name -notmatch 'Bluetooth|Radio|Enumerator|Microsoft' };
foreach ($d in $btDevices) {
	$already = $false;
	if ($devices) { foreach ($existing in $devices) { if ($existing.InstanceId -eq $d.PNPDeviceID) { $already = $true; break } } }
	if (-not $already) {
		"BEGIN_DEVICE";
		"Name=$($d.Name)";
		"Status=$($d.Status)";
		"Class=Bluetooth";
		"InstanceId=$($d.PNPDeviceID)";
		"END_DEVICE";
	}
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
	let mut devices = Vec::new();
	let mut name = String::new();
	let mut status = String::new();
	let mut class = String::new();
	let mut instance_id = String::new();
	let mut in_device = false;

	for raw in text.lines() {
		let line = raw.trim();
		if line == "BEGIN_DEVICE" {
			in_device = true;
			name.clear();
			status.clear();
			class.clear();
			instance_id.clear();
			continue;
		}
		if line == "END_DEVICE" {
			if in_device && !name.is_empty() {
				let connected = status.eq_ignore_ascii_case("OK");
				devices.push(json!({
					"name": name,
					"connected": connected,
					"status": status,
					"class": class,
					"instance_id": instance_id,
				}));
			}
			in_device = false;
			continue;
		}
		if !in_device {
			continue;
		}
		if let Some(v) = line.strip_prefix("Name=") {
			name = v.trim().to_string();
		} else if let Some(v) = line.strip_prefix("Status=") {
			status = v.trim().to_string();
		} else if let Some(v) = line.strip_prefix("Class=") {
			class = v.trim().to_string();
		} else if let Some(v) = line.strip_prefix("InstanceId=") {
			instance_id = v.trim().to_string();
		}
	}

	devices
}
