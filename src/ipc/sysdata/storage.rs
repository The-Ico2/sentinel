// ~/sentinel/sentinel-backend/src/ipc/sysdata/storage.rs

use serde_json::{json, Value};
use sysinfo::Disks;

pub fn get_storage_json() -> Value {
	let disks = Disks::new_with_refreshed_list();

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

			json!({
				"name": disk.name().to_string_lossy(),
				"mount": disk.mount_point().to_string_lossy(),
				"kind": format!("{:?}", disk.kind()),
				"file_system": disk.file_system().to_string_lossy(),
				"removable": disk.is_removable(),
				"total_bytes": disk_total,
				"available_bytes": disk_available,
				"used_bytes": disk_used,
				"usage_percent": usage_percent,
			})
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
		"disks": list,
	})
}
