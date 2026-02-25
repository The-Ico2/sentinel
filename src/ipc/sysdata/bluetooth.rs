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
	// Use WinRT Bluetooth APIs for accurate connection status.
	// PnP Status "OK" only means the driver is loaded, not that the device
	// is actually connected — paired devices stay "OK" while disconnected.
	let script = r#"$ErrorActionPreference='SilentlyContinue';
Add-Type -AssemblyName System.Runtime.WindowsRuntime 2>$null;
[void][Windows.Devices.Bluetooth.BluetoothDevice,Windows.Devices.Bluetooth,ContentType=WindowsRuntime];
[void][Windows.Devices.Bluetooth.BluetoothLEDevice,Windows.Devices.Bluetooth,ContentType=WindowsRuntime];
[void][Windows.Devices.Enumeration.DeviceInformation,Windows.Devices.Enumeration,ContentType=WindowsRuntime];
$asTask = ([System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object {
    $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and
    $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1'
})[0];
function WA($op, $type) {
    $t = $asTask.MakeGenericMethod($type).Invoke($null, @($op));
    $t.Wait(5000) | Out-Null;
    return $t.Result;
}
$seen = @{};
# Classic Bluetooth
try {
    $sel = [Windows.Devices.Bluetooth.BluetoothDevice]::GetDeviceSelectorFromPairingState($true);
    $devs = WA ([Windows.Devices.Enumeration.DeviceInformation]::FindAllAsync($sel)) ([Windows.Devices.Enumeration.DeviceInformationCollection]);
    foreach ($info in $devs) {
        $bt = WA ([Windows.Devices.Bluetooth.BluetoothDevice]::FromIdAsync($info.Id)) ([Windows.Devices.Bluetooth.BluetoothDevice]);
        if ($bt -and $bt.Name) {
            $isConn = $bt.ConnectionStatus -eq [Windows.Devices.Bluetooth.BluetoothConnectionStatus]::Connected;
            $addr = '{0:X12}' -f $bt.BluetoothAddress;
            if (-not $seen.ContainsKey($addr)) {
                $seen[$addr] = $true;
                "BEGIN_DEVICE";
                "Name=$($bt.Name)";
                "Connected=$isConn";
                "Address=$addr";
                "Class=$($bt.ClassOfDevice.MajorClass)";
                "Type=Classic";
                "END_DEVICE";
            }
        }
    }
} catch {}
# Bluetooth LE
try {
    $selLE = [Windows.Devices.Bluetooth.BluetoothLEDevice]::GetDeviceSelectorFromPairingState($true);
    $devsLE = WA ([Windows.Devices.Enumeration.DeviceInformation]::FindAllAsync($selLE)) ([Windows.Devices.Enumeration.DeviceInformationCollection]);
    foreach ($info in $devsLE) {
        $ble = WA ([Windows.Devices.Bluetooth.BluetoothLEDevice]::FromIdAsync($info.Id)) ([Windows.Devices.Bluetooth.BluetoothLEDevice]);
        if ($ble -and $ble.Name) {
            $isConn = $ble.ConnectionStatus -eq [Windows.Devices.Bluetooth.BluetoothConnectionStatus]::Connected;
            $addr = '{0:X12}' -f $ble.BluetoothAddress;
            if (-not $seen.ContainsKey($addr)) {
                $seen[$addr] = $true;
                "BEGIN_DEVICE";
                "Name=$($ble.Name)";
                "Connected=$isConn";
                "Address=$addr";
                "Class=BLE";
                "Type=LE";
                "END_DEVICE";
            }
        }
    }
} catch {}
"#;

	let output = Command::new("powershell")
		.creation_flags(CREATE_NO_WINDOW)
		.args(["-NoProfile", "-NonInteractive", "-Command", script])
		.output();

	let Ok(output) = output else {
		return fallback_bluetooth_pnp();
	};
	if !output.status.success() {
		return fallback_bluetooth_pnp();
	}

	let text = String::from_utf8_lossy(&output.stdout);
	let devices = parse_bluetooth_output(&text);

	// If WinRT returned nothing, try PnP fallback
	if devices.is_empty() {
		return fallback_bluetooth_pnp();
	}
	devices
}

fn parse_bluetooth_output(text: &str) -> Vec<Value> {
	let mut devices = Vec::new();
	let mut name = String::new();
	let mut connected = false;
	let mut address = String::new();
	let mut class = String::new();
	let mut dev_type = String::new();
	let mut in_device = false;

	for raw in text.lines() {
		let line = raw.trim();
		if line == "BEGIN_DEVICE" {
			in_device = true;
			name.clear();
			connected = false;
			address.clear();
			class.clear();
			dev_type.clear();
			continue;
		}
		if line == "END_DEVICE" {
			if in_device && !name.is_empty() {
				// Filter out virtual/transport devices
				if name.to_lowercase().contains("avrcp transport") {
					in_device = false;
					continue;
				}
				devices.push(json!({
					"name": name,
					"connected": connected,
					"address": if address.is_empty() { Value::Null } else { json!(address) },
					"class": if class.is_empty() { Value::Null } else { json!(class) },
					"type": if dev_type.is_empty() { Value::Null } else { json!(dev_type) },
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
		} else if let Some(v) = line.strip_prefix("Connected=") {
			connected = v.trim().eq_ignore_ascii_case("True");
		} else if let Some(v) = line.strip_prefix("Address=") {
			address = v.trim().to_string();
		} else if let Some(v) = line.strip_prefix("Class=") {
			class = v.trim().to_string();
		} else if let Some(v) = line.strip_prefix("Type=") {
			dev_type = v.trim().to_string();
		}
	}

	devices
}

/// Fallback: PnP enumeration when WinRT is unavailable (connection status less accurate)
fn fallback_bluetooth_pnp() -> Vec<Value> {
	let script = r#"$ErrorActionPreference='SilentlyContinue';
$devices = Get-PnpDevice -Class Bluetooth -ErrorAction SilentlyContinue | Where-Object { $_.FriendlyName -notmatch 'Bluetooth|Radio|Enumerator|Microsoft|Avrcp Transport' };
foreach ($d in $devices) {
	"BEGIN_DEVICE";
	"Name=$($d.FriendlyName)";
	"Connected=False";
	"Class=$($d.Class)";
	"Type=Unknown";
	"END_DEVICE";
}"#;

	let output = Command::new("powershell")
		.creation_flags(CREATE_NO_WINDOW)
		.args(["-NoProfile", "-NonInteractive", "-Command", script])
		.output();

	let Ok(output) = output else { return Vec::new() };
	if !output.status.success() { return Vec::new() }

	let text = String::from_utf8_lossy(&output.stdout);
	parse_bluetooth_output(&text)
}
