// ~/sentinel/sentinel-backend/src/ipc/sysdata/system.rs

use serde_json::{json, Value};
use std::os::windows::process::CommandExt;
use std::process::Command;
use sysinfo::System;

const CREATE_NO_WINDOW: u32 = 0x08000000;

pub fn get_system_json() -> Value {
	let os_name = System::name().unwrap_or_else(|| "unknown".into());
	let os_long = System::long_os_version().unwrap_or_else(|| "unknown".into());
	let os_version = System::os_version().unwrap_or_else(|| "unknown".into());
	let kernel_version = System::kernel_version().unwrap_or_else(|| "unknown".into());
	let hostname = System::host_name().unwrap_or_else(|| "unknown".into());
	let arch = std::env::consts::ARCH;
	let cpu_arch = System::cpu_arch();

	let username = std::env::var("USERNAME").unwrap_or_else(|_| "unknown".into());
	let computer_name = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "unknown".into());
	let user_domain = std::env::var("USERDOMAIN").unwrap_or_else(|_| "unknown".into());
	let user_profile = std::env::var("USERPROFILE").unwrap_or_else(|_| "unknown".into());

	let locale = get_system_locale();
	let theme = get_windows_theme();
	let bios_info = get_bios_info();
	let motherboard_info = get_motherboard_info();

	json!({
		"os": {
			"name": os_name,
			"long_name": os_long,
			"version": os_version,
			"kernel_version": kernel_version,
			"arch": arch,
			"cpu_arch": cpu_arch,
		},
		"hostname": hostname,
		"computer_name": computer_name,
		"username": username,
		"user_domain": user_domain,
		"user_profile": user_profile,
		"locale": locale,
		"theme": theme,
		"bios": bios_info,
		"motherboard": motherboard_info,
	})
}

fn get_system_locale() -> Value {
	let script = r#"$ErrorActionPreference='SilentlyContinue';
$culture = [System.Globalization.CultureInfo]::CurrentCulture;
"Name=$($culture.Name)";
"DisplayName=$($culture.DisplayName)";
"TwoLetterISO=$($culture.TwoLetterISOLanguageName)";
"ThreeLetterISO=$($culture.ThreeLetterISOLanguageName)";
$tz = [System.TimeZoneInfo]::Local;
"TimeZoneId=$($tz.Id)";
"TimeZoneName=$($tz.DisplayName)";
"TimeZoneUtcOffset=$($tz.BaseUtcOffset.TotalHours)";
"DaylightSaving=$($tz.SupportsDaylightSavingTime)";
$region = [System.Globalization.RegionInfo]::CurrentRegion;
"Country=$($region.DisplayName)";
"CountryCode=$($region.TwoLetterISORegionName)";
"CurrencySymbol=$($region.CurrencySymbol)";
"CurrencyName=$($region.CurrencyEnglishName)";"#;

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
	let mut name = String::new();
	let mut display_name = String::new();
	let mut two_letter = String::new();
	let mut three_letter = String::new();
	let mut tz_id = String::new();
	let mut tz_name = String::new();
	let mut tz_offset = String::new();
	let mut dst = String::new();
	let mut country = String::new();
	let mut country_code = String::new();
	let mut currency_symbol = String::new();
	let mut currency_name = String::new();

	for raw in text.lines() {
		let line = raw.trim();
		if let Some(v) = line.strip_prefix("Name=") {
			name = v.trim().to_string();
		} else if let Some(v) = line.strip_prefix("DisplayName=") {
			display_name = v.trim().to_string();
		} else if let Some(v) = line.strip_prefix("TwoLetterISO=") {
			two_letter = v.trim().to_string();
		} else if let Some(v) = line.strip_prefix("ThreeLetterISO=") {
			three_letter = v.trim().to_string();
		} else if let Some(v) = line.strip_prefix("TimeZoneId=") {
			tz_id = v.trim().to_string();
		} else if let Some(v) = line.strip_prefix("TimeZoneName=") {
			tz_name = v.trim().to_string();
		} else if let Some(v) = line.strip_prefix("TimeZoneUtcOffset=") {
			tz_offset = v.trim().to_string();
		} else if let Some(v) = line.strip_prefix("DaylightSaving=") {
			dst = v.trim().to_string();
		} else if let Some(v) = line.strip_prefix("Country=") {
			country = v.trim().to_string();
		} else if let Some(v) = line.strip_prefix("CountryCode=") {
			country_code = v.trim().to_string();
		} else if let Some(v) = line.strip_prefix("CurrencySymbol=") {
			currency_symbol = v.trim().to_string();
		} else if let Some(v) = line.strip_prefix("CurrencyName=") {
			currency_name = v.trim().to_string();
		}
	}

	json!({
		"language": {
			"name": name,
			"display_name": display_name,
			"iso_two_letter": two_letter,
			"iso_three_letter": three_letter,
		},
		"timezone": {
			"id": tz_id,
			"display_name": tz_name,
			"utc_offset_hours": tz_offset.parse::<f64>().ok(),
			"daylight_saving": dst.to_ascii_lowercase() == "true",
		},
		"region": {
			"country": country,
			"country_code": country_code,
			"currency_symbol": currency_symbol,
			"currency_name": currency_name,
		},
	})
}

fn get_windows_theme() -> Value {
	// Read Windows registry for theme settings
	let script = r#"$ErrorActionPreference='SilentlyContinue';
$path = 'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Themes\Personalize';
$light = (Get-ItemProperty -Path $path -Name AppsUseLightTheme -ErrorAction SilentlyContinue).AppsUseLightTheme;
$sysLight = (Get-ItemProperty -Path $path -Name SystemUsesLightTheme -ErrorAction SilentlyContinue).SystemUsesLightTheme;
$transparency = (Get-ItemProperty -Path $path -Name EnableTransparency -ErrorAction SilentlyContinue).EnableTransparency;
$colorPath = 'HKCU:\SOFTWARE\Microsoft\Windows\DWM';
$accent = (Get-ItemProperty -Path $colorPath -Name AccentColor -ErrorAction SilentlyContinue).AccentColor;
$colorizePrev = (Get-ItemProperty -Path $colorPath -Name ColorPrevalence -ErrorAction SilentlyContinue).ColorPrevalence;
"AppsUseLightTheme=$light";
"SystemUsesLightTheme=$sysLight";
"EnableTransparency=$transparency";
"AccentColor=$accent";
"ColorPrevalence=$colorizePrev";"#;

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
	let mut apps_light: Option<bool> = None;
	let mut sys_light: Option<bool> = None;
	let mut transparency: Option<bool> = None;
	let mut accent_color: Option<u32> = None;
	let mut color_prevalence: Option<bool> = None;

	for raw in text.lines() {
		let line = raw.trim();
		if let Some(v) = line.strip_prefix("AppsUseLightTheme=") {
			apps_light = Some(v.trim() == "1");
		} else if let Some(v) = line.strip_prefix("SystemUsesLightTheme=") {
			sys_light = Some(v.trim() == "1");
		} else if let Some(v) = line.strip_prefix("EnableTransparency=") {
			transparency = Some(v.trim() == "1");
		} else if let Some(v) = line.strip_prefix("AccentColor=") {
			accent_color = v.trim().parse::<u32>().ok();
		} else if let Some(v) = line.strip_prefix("ColorPrevalence=") {
			color_prevalence = Some(v.trim() == "1");
		}
	}

	// Convert ABGR accent color to hex RGB string
	let accent_hex = accent_color.map(|c| {
		let r = c & 0xFF;
		let g = (c >> 8) & 0xFF;
		let b = (c >> 16) & 0xFF;
		format!("#{:02x}{:02x}{:02x}", r, g, b)
	});

	let app_theme = apps_light.map(|l| if l { "light" } else { "dark" });
	let sys_theme = sys_light.map(|l| if l { "light" } else { "dark" });

	json!({
		"app_theme": app_theme,
		"system_theme": sys_theme,
		"transparency_enabled": transparency,
		"accent_color_hex": accent_hex,
		"accent_color_raw": accent_color,
		"color_on_title_bars": color_prevalence,
	})
}

fn get_bios_info() -> Value {
	let script = r#"$ErrorActionPreference='SilentlyContinue';
$bios = Get-CimInstance -ClassName Win32_BIOS -ErrorAction SilentlyContinue | Select-Object -First 1;
if ($bios) {
	"Manufacturer=$($bios.Manufacturer)";
	"Name=$($bios.Name)";
	"Version=$($bios.SMBIOSBIOSVersion)";
	"ReleaseDate=$($bios.ReleaseDate)";
	"SerialNumber=$($bios.SerialNumber)";
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
	let mut manufacturer = String::new();
	let mut name = String::new();
	let mut version = String::new();
	let mut release_date = String::new();
	let mut serial = String::new();

	for raw in text.lines() {
		let line = raw.trim();
		if let Some(v) = line.strip_prefix("Manufacturer=") {
			manufacturer = v.trim().to_string();
		} else if let Some(v) = line.strip_prefix("Name=") {
			name = v.trim().to_string();
		} else if let Some(v) = line.strip_prefix("Version=") {
			version = v.trim().to_string();
		} else if let Some(v) = line.strip_prefix("ReleaseDate=") {
			release_date = v.trim().to_string();
		} else if let Some(v) = line.strip_prefix("SerialNumber=") {
			serial = v.trim().to_string();
		}
	}

	json!({
		"manufacturer": if manufacturer.is_empty() { Value::Null } else { json!(manufacturer) },
		"name": if name.is_empty() { Value::Null } else { json!(name) },
		"version": if version.is_empty() { Value::Null } else { json!(version) },
		"release_date": if release_date.is_empty() { Value::Null } else { json!(release_date) },
		"serial_number": if serial.is_empty() { Value::Null } else { json!(serial) },
	})
}

fn get_motherboard_info() -> Value {
	let script = r#"$ErrorActionPreference='SilentlyContinue';
$board = Get-CimInstance -ClassName Win32_BaseBoard -ErrorAction SilentlyContinue | Select-Object -First 1;
if ($board) {
	"Manufacturer=$($board.Manufacturer)";
	"Product=$($board.Product)";
	"Version=$($board.Version)";
	"SerialNumber=$($board.SerialNumber)";
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
	let mut manufacturer = String::new();
	let mut product = String::new();
	let mut version = String::new();
	let mut serial = String::new();

	for raw in text.lines() {
		let line = raw.trim();
		if let Some(v) = line.strip_prefix("Manufacturer=") {
			manufacturer = v.trim().to_string();
		} else if let Some(v) = line.strip_prefix("Product=") {
			product = v.trim().to_string();
		} else if let Some(v) = line.strip_prefix("Version=") {
			version = v.trim().to_string();
		} else if let Some(v) = line.strip_prefix("SerialNumber=") {
			serial = v.trim().to_string();
		}
	}

	json!({
		"manufacturer": if manufacturer.is_empty() { Value::Null } else { json!(manufacturer) },
		"product": if product.is_empty() { Value::Null } else { json!(product) },
		"version": if version.is_empty() { Value::Null } else { json!(version) },
		"serial_number": if serial.is_empty() { Value::Null } else { json!(serial) },
	})
}
