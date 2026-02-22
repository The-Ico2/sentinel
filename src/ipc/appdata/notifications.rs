// ~/sentinel/sentinel-backend/src/ipc/appdata/notifications.rs
// Retrieves recent Windows notification history via PowerShell / WinRT toast notification APIs

use serde_json::{json, Value};
use std::os::windows::process::CommandExt;

/// Retrieves recent toast/notification data from the Windows Action Center.
/// Uses PowerShell to query the notification store via WinRT APIs.
pub fn get_notifications_json() -> Value {
	let script = r#"
try {
    [Windows.UI.Notifications.Management.UserNotificationListener, Windows.UI.Notifications, ContentType = WindowsRuntime] | Out-Null
    [Windows.UI.Notifications.Management.UserNotificationListenerAccessStatus, Windows.UI.Notifications, ContentType = WindowsRuntime] | Out-Null
    [Windows.Foundation.IAsyncOperation`1, Windows.Foundation, ContentType = WindowsRuntime] | Out-Null

    $listener = [Windows.UI.Notifications.Management.UserNotificationListener]::Current

    $accessOp = $listener.RequestAccessAsync()
    $null = [Windows.Foundation.IAsyncOperation[Windows.UI.Notifications.Management.UserNotificationListenerAccessStatus]]
    # Wait for access
    Start-Sleep -Milliseconds 200
    $access = $accessOp.GetResults()

    if ($access -ne 'Allowed') {
        Write-Output "ACCESS_DENIED=$access"
        exit
    }

    $notifOp = $listener.GetNotificationsAsync(0)
    Start-Sleep -Milliseconds 500
    $notifications = $notifOp.GetResults()

    Write-Output "COUNT=$($notifications.Count)"

    $i = 0
    foreach ($notif in $notifications) {
        if ($i -ge 25) { break }
        $toast = $notif.Notification
        $binding = $toast.Visual.GetBinding([Windows.UI.Notifications.KnownNotificationBindings]::ToastGeneric)

        $appId = $notif.AppInfo.DisplayInfo.DisplayName
        $texts = @()
        if ($binding) {
            foreach ($elem in $binding.GetTextElements()) {
                $texts += $elem.Text
            }
        }
        $title = if ($texts.Count -gt 0) { $texts[0] } else { "" }
        $body = if ($texts.Count -gt 1) { ($texts[1..($texts.Count-1)] -join "`n") } else { "" }
        $created = $notif.CreationTime.ToString("o")
        $id = $notif.Id

        Write-Output "NOTIF_START=$i"
        Write-Output "NOTIF_ID=$id"
        Write-Output "NOTIF_APP=$appId"
        Write-Output "NOTIF_TITLE=$title"
        Write-Output "NOTIF_BODY=$body"
        Write-Output "NOTIF_TIME=$created"
        Write-Output "NOTIF_END=$i"
        $i++
    }
} catch {
    Write-Output "ERROR=$($_.Exception.Message)"
}
"#;

	let output = std::process::Command::new("powershell")
		.args(["-NoProfile", "-Command", script])
		.creation_flags(0x08000000) // CREATE_NO_WINDOW
		.output();

	match output {
		Ok(o) => {
			let stdout = String::from_utf8_lossy(&o.stdout);
			parse_notifications_output(&stdout)
		}
		Err(e) => {
			json!({
				"error": format!("Failed to query notifications: {}", e),
				"count": 0,
				"notifications": [],
			})
		}
	}
}

fn parse_notifications_output(output: &str) -> Value {
	let lines: Vec<&str> = output.lines().collect();

	// Check for error/access denied
	for line in &lines {
		if let Some(err) = line.strip_prefix("ERROR=") {
			return json!({
				"error": err.trim(),
				"count": 0,
				"notifications": [],
			});
		}
		if let Some(status) = line.strip_prefix("ACCESS_DENIED=") {
			return json!({
				"error": format!("Notification access denied: {}", status.trim()),
				"access": "denied",
				"count": 0,
				"notifications": [],
			});
		}
	}

	let total_count: u32 = lines
		.iter()
		.find_map(|l| l.strip_prefix("COUNT="))
		.and_then(|v| v.trim().parse().ok())
		.unwrap_or(0);

	let mut notifications = Vec::new();
	let mut current: Option<NotifBuilder> = None;

	for line in &lines {
		if line.starts_with("NOTIF_START=") {
			current = Some(NotifBuilder::default());
		} else if line.starts_with("NOTIF_END=") {
			if let Some(n) = current.take() {
				notifications.push(json!({
					"id": n.id,
					"app_name": n.app,
					"title": n.title,
					"body": n.body,
					"created_at": n.time,
				}));
			}
		} else if let Some(ref mut n) = current {
			if let Some(v) = line.strip_prefix("NOTIF_ID=") {
				n.id = v.trim().to_string();
			} else if let Some(v) = line.strip_prefix("NOTIF_APP=") {
				n.app = v.trim().to_string();
			} else if let Some(v) = line.strip_prefix("NOTIF_TITLE=") {
				n.title = v.trim().to_string();
			} else if let Some(v) = line.strip_prefix("NOTIF_BODY=") {
				n.body = v.trim().to_string();
			} else if let Some(v) = line.strip_prefix("NOTIF_TIME=") {
				n.time = v.trim().to_string();
			}
		}
	}

	json!({
		"count": total_count,
		"returned": notifications.len(),
		"notifications": notifications,
	})
}

#[derive(Default)]
struct NotifBuilder {
	id: String,
	app: String,
	title: String,
	body: String,
	time: String,
}
