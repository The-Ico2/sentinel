// ~/sentinel/sentinel-backend/src/ipc/sysdata/power.rs

use serde_json::{json, Value};
use std::os::windows::process::CommandExt;
use std::process::Command;
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};

const CREATE_NO_WINDOW: u32 = 0x08000000;

pub fn get_power_json() -> Value {
	unsafe {
		let mut status = SYSTEM_POWER_STATUS::default();
		if GetSystemPowerStatus(&mut status).is_ok() {
			let ac_status = match status.ACLineStatus {
				0 => "offline",
				1 => "online",
				_ => "unknown",
			};

			let battery_flag = status.BatteryFlag;
			let charging = battery_flag & 8 != 0;
			let no_battery = battery_flag & 128 != 0;
			let battery_critical = battery_flag & 4 != 0;
			let battery_low = battery_flag & 2 != 0;
			let battery_high = battery_flag & 1 != 0;

			let battery_percent = if status.BatteryLifePercent == 255 {
				Value::Null
			} else {
				json!(status.BatteryLifePercent)
			};

			let battery_lifetime_seconds = if status.BatteryLifeTime == 0xFFFFFFFF {
				Value::Null
			} else {
				json!(status.BatteryLifeTime)
			};

			let battery_fulllife_seconds = if status.BatteryFullLifeTime == 0xFFFFFFFF {
				Value::Null
			} else {
				json!(status.BatteryFullLifeTime)
			};

			let battery_saver = status.SystemStatusFlag != 0;
			let power_plan = get_active_power_plan();
			let battery_details = get_battery_details();

			json!({
				"ac_status": ac_status,
				"battery": {
					"present": !no_battery,
					"percent": battery_percent,
					"charging": charging,
					"critical": battery_critical,
					"low": battery_low,
					"high": battery_high,
					"lifetime_seconds": battery_lifetime_seconds,
					"fulllife_seconds": battery_fulllife_seconds,
					"saver_active": battery_saver,
					"details": battery_details,
				},
				"power_plan": power_plan,
			})
		} else {
			json!({
				"ac_status": "unknown",
				"battery": {
					"present": false,
					"percent": Value::Null,
					"charging": false,
				},
				"power_plan": Value::Null,
			})
		}
	}
}

fn get_active_power_plan() -> Value {
	let output = Command::new("powercfg")
		.creation_flags(CREATE_NO_WINDOW)
		.args(["/getactivescheme"])
		.output();

	if let Ok(output) = output {
		if output.status.success() {
			let text = String::from_utf8_lossy(&output.stdout);
			// Output format: "Power Scheme GUID: <guid>  (Name)"
			if let Some(name_start) = text.find('(') {
				if let Some(name_end) = text.rfind(')') {
					if name_start < name_end {
						return json!(text[name_start + 1..name_end].trim());
					}
				}
			}
		}
	}

	Value::Null
}

fn get_battery_details() -> Value {
	let script = r#"$ErrorActionPreference='SilentlyContinue';
$b = Get-CimInstance -ClassName Win32_Battery -ErrorAction SilentlyContinue | Select-Object -First 1;
if ($b) {
	"Name=$($b.Name)";
	"DeviceID=$($b.DeviceID)";
	"DesignCapacity=$($b.DesignCapacity)";
	"FullChargeCapacity=$($b.FullChargeCapacity)";
	"DesignVoltage=$($b.DesignVoltage)";
	"Status=$($b.Status)";
	"Chemistry=$($b.Chemistry)";
	"EstimatedChargeRemaining=$($b.EstimatedChargeRemaining)";
	"EstimatedRunTime=$($b.EstimatedRunTime)";
	"BatteryStatus=$($b.BatteryStatus)";
} else {
	"NoBattery=true";
}"#;

	let output = Command::new("powershell")
		.creation_flags(CREATE_NO_WINDOW)
		.args(["-NoProfile", "-NonInteractive", "-Command", script])
		.output();

	let Ok(output) = output else {
		return Value::Null;
	};
	if !output.status.success() {
		return Value::Null;
	}

	let text = String::from_utf8_lossy(&output.stdout);

	if text.contains("NoBattery=true") {
		return Value::Null;
	}

	let mut name = String::new();
	let mut device_id = String::new();
	let mut design_capacity: Option<u64> = None;
	let mut full_charge_capacity: Option<u64> = None;
	let mut design_voltage: Option<u64> = None;
	let mut status_str = String::new();
	let mut chemistry: Option<u16> = None;
	let mut estimated_charge: Option<u16> = None;
	let mut estimated_runtime: Option<u32> = None;
	let mut battery_status: Option<u16> = None;

	for raw in text.lines() {
		let line = raw.trim();
		if let Some(v) = line.strip_prefix("Name=") {
			name = v.trim().to_string();
		} else if let Some(v) = line.strip_prefix("DeviceID=") {
			device_id = v.trim().to_string();
		} else if let Some(v) = line.strip_prefix("DesignCapacity=") {
			design_capacity = v.trim().parse().ok();
		} else if let Some(v) = line.strip_prefix("FullChargeCapacity=") {
			full_charge_capacity = v.trim().parse().ok();
		} else if let Some(v) = line.strip_prefix("DesignVoltage=") {
			design_voltage = v.trim().parse().ok();
		} else if let Some(v) = line.strip_prefix("Status=") {
			status_str = v.trim().to_string();
		} else if let Some(v) = line.strip_prefix("Chemistry=") {
			chemistry = v.trim().parse().ok();
		} else if let Some(v) = line.strip_prefix("EstimatedChargeRemaining=") {
			estimated_charge = v.trim().parse().ok();
		} else if let Some(v) = line.strip_prefix("EstimatedRunTime=") {
			estimated_runtime = v.trim().parse().ok();
		} else if let Some(v) = line.strip_prefix("BatteryStatus=") {
			battery_status = v.trim().parse().ok();
		}
	}

	let chemistry_name = chemistry.map(|c| match c {
		1 => "Other",
		2 => "Unknown",
		3 => "Lead Acid",
		4 => "Nickel Cadmium",
		5 => "Nickel Metal Hydride",
		6 => "Lithium-ion",
		7 => "Zinc Air",
		8 => "Lithium Polymer",
		_ => "Unknown",
	});

	let health_percent = design_capacity
		.zip(full_charge_capacity)
		.map(|(design, full)| {
			if design > 0 {
				((full as f64 / design as f64) * 100.0).min(100.0)
			} else {
				0.0
			}
		});

	json!({
		"name": if name.is_empty() { Value::Null } else { json!(name) },
		"device_id": if device_id.is_empty() { Value::Null } else { json!(device_id) },
		"design_capacity_mwh": design_capacity,
		"full_charge_capacity_mwh": full_charge_capacity,
		"health_percent": health_percent,
		"design_voltage_mv": design_voltage,
		"chemistry": chemistry_name,
		"status": if status_str.is_empty() { Value::Null } else { json!(status_str) },
		"estimated_charge_percent": estimated_charge,
		"estimated_runtime_minutes": estimated_runtime,
		"battery_status_code": battery_status,
	})
}
