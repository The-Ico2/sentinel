// ~/sentinel/sentinel-backend/src/ipc/sysdata/wifi.rs

use serde_json::{json, Value};
use std::os::windows::process::CommandExt;
use std::process::Command;

const CREATE_NO_WINDOW: u32 = 0x08000000;

pub fn get_wifi_json() -> Value {
	let connected = get_connected_wifi();
	let interfaces = get_wifi_interfaces();

	json!({
		"connected": connected,
		"interfaces": interfaces,
	})
}

fn get_connected_wifi() -> Value {
	// `netsh wlan show interfaces` gives the most reliable info
	let output = Command::new("netsh")
		.creation_flags(CREATE_NO_WINDOW)
		.args(["wlan", "show", "interfaces"])
		.output();

	let Ok(output) = output else {
		return json!({ "is_connected": false });
	};
	if !output.status.success() {
		return json!({ "is_connected": false });
	}

	let text = String::from_utf8_lossy(&output.stdout);

	let mut ssid = String::new();
	let mut bssid = String::new();
	let mut state = String::new();
	let mut signal_percent: Option<u32> = None;
	let mut radio_type = String::new();
	let mut auth = String::new();
	let mut cipher = String::new();
	let mut band = String::new();
	let mut channel: Option<u32> = None;
	let mut receive_rate: Option<f64> = None;
	let mut transmit_rate: Option<f64> = None;
	let mut profile = String::new();
	let mut interface_name = String::new();
	let mut interface_type = String::new();

	for raw in text.lines() {
		let line = raw.trim();
		// Fields use ": " separator except some that use " : "
		if let Some((key, val)) = line.split_once(':') {
			let key = key.trim();
			let val = val.trim();

			match key {
				"Name" | "name" => interface_name = val.to_string(),
				"State" | "state" => state = val.to_string(),
				"SSID" | "ssid" if ssid.is_empty() => ssid = val.to_string(),
				"BSSID" | "bssid" => bssid = val.to_string(),
				"Signal" | "signal" => {
					signal_percent = val.trim_end_matches('%').trim().parse().ok();
				}
				"Radio type" | "radio type" | "Radio Type" => radio_type = val.to_string(),
				"Authentication" | "authentication" => auth = val.to_string(),
				"Cipher" | "cipher" => cipher = val.to_string(),
				"Band" | "band" => band = val.to_string(),
				"Channel" | "channel" => channel = val.parse().ok(),
				"Receive rate (Mbps)" | "receive rate (Mbps)" | "Receive rate" => {
					receive_rate = val.trim_end_matches(" Mbps").trim().parse().ok();
				}
				"Transmit rate (Mbps)" | "transmit rate (Mbps)" | "Transmit rate" => {
					transmit_rate = val.trim_end_matches(" Mbps").trim().parse().ok();
				}
				"Profile" | "profile" => profile = val.to_string(),
				"Type" | "type" => interface_type = val.to_string(),
				_ => {}
			}
		}
	}

	let is_connected = state.to_ascii_lowercase().contains("connected")
		&& !state.to_ascii_lowercase().contains("disconnected");

	// Try to infer band from radio type if not explicitly present
	if band.is_empty() && !radio_type.is_empty() {
		let rt_lower = radio_type.to_ascii_lowercase();
		band = if rt_lower.contains("ac") || rt_lower.contains("ax") || rt_lower.contains("5") {
			"5 GHz".to_string()
		} else if rt_lower.contains("n") {
			"2.4 GHz or 5 GHz".to_string()
		} else if rt_lower.contains("g") || rt_lower.contains("b") {
			"2.4 GHz".to_string()
		} else {
			String::new()
		};
	}

	let signal_quality = signal_percent.map(|s| {
		if s >= 80 { "excellent" }
		else if s >= 60 { "good" }
		else if s >= 40 { "fair" }
		else if s >= 20 { "weak" }
		else { "very_weak" }
	});

	json!({
		"is_connected": is_connected,
		"ssid": if ssid.is_empty() { Value::Null } else { json!(ssid) },
		"bssid": if bssid.is_empty() { Value::Null } else { json!(bssid) },
		"signal_percent": signal_percent,
		"signal_quality": signal_quality,
		"radio_type": if radio_type.is_empty() { Value::Null } else { json!(radio_type) },
		"band": if band.is_empty() { Value::Null } else { json!(band) },
		"channel": channel,
		"authentication": if auth.is_empty() { Value::Null } else { json!(auth) },
		"cipher": if cipher.is_empty() { Value::Null } else { json!(cipher) },
		"receive_rate_mbps": receive_rate,
		"transmit_rate_mbps": transmit_rate,
		"profile": if profile.is_empty() { Value::Null } else { json!(profile) },
		"interface_name": if interface_name.is_empty() { Value::Null } else { json!(interface_name) },
		"interface_type": if interface_type.is_empty() { Value::Null } else { json!(interface_type) },
		"state": if state.is_empty() { Value::Null } else { json!(state) },
	})
}

fn get_wifi_interfaces() -> Vec<Value> {
	// Get available WLAN interfaces
	let script = r#"$ErrorActionPreference='SilentlyContinue';
$adapters = Get-NetAdapter -ErrorAction SilentlyContinue | Where-Object { $_.InterfaceDescription -match 'Wi-Fi|WiFi|Wireless|WLAN|802\.11' -or $_.Name -match 'Wi-Fi|WiFi|Wireless|WLAN' };
foreach ($a in $adapters) {
	"BEGIN_IFACE";
	"Name=$($a.Name)";
	"Description=$($a.InterfaceDescription)";
	"Status=$($a.Status)";
	"MacAddress=$($a.MacAddress)";
	"LinkSpeed=$($a.LinkSpeed)";
	"MediaType=$($a.MediaType)";
	"END_IFACE";
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
	let mut interfaces = Vec::new();
	let mut name = String::new();
	let mut desc = String::new();
	let mut status = String::new();
	let mut mac = String::new();
	let mut link_speed = String::new();
	let mut media_type = String::new();
	let mut in_iface = false;

	for raw in text.lines() {
		let line = raw.trim();
		if line == "BEGIN_IFACE" {
			in_iface = true;
			name.clear();
			desc.clear();
			status.clear();
			mac.clear();
			link_speed.clear();
			media_type.clear();
			continue;
		}
		if line == "END_IFACE" {
			if in_iface && !name.is_empty() {
				interfaces.push(json!({
					"name": name,
					"description": desc,
					"status": status,
					"mac_address": mac,
					"link_speed": link_speed,
					"media_type": media_type,
				}));
			}
			in_iface = false;
			continue;
		}
		if !in_iface {
			continue;
		}
		if let Some(v) = line.strip_prefix("Name=") {
			name = v.trim().to_string();
		} else if let Some(v) = line.strip_prefix("Description=") {
			desc = v.trim().to_string();
		} else if let Some(v) = line.strip_prefix("Status=") {
			status = v.trim().to_string();
		} else if let Some(v) = line.strip_prefix("MacAddress=") {
			mac = v.trim().to_string();
		} else if let Some(v) = line.strip_prefix("LinkSpeed=") {
			link_speed = v.trim().to_string();
		} else if let Some(v) = line.strip_prefix("MediaType=") {
			media_type = v.trim().to_string();
		}
	}

	interfaces
}
