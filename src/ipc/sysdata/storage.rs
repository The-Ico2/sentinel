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
			total_bytes = total_bytes.saturating_add(disk_total);
			available_bytes = available_bytes.saturating_add(disk_available);

			json!({
				"name": disk.name().to_string_lossy(),
				"mount": disk.mount_point().to_string_lossy(),
				"kind": format!("{:?}", disk.kind()),
				"removable": disk.is_removable(),
				"total_bytes": disk_total,
				"available_bytes": disk_available,
				"used_bytes": disk_total.saturating_sub(disk_available),
			})
		})
		.collect();

	json!({
		"total_bytes": total_bytes,
		"available_bytes": available_bytes,
		"used_bytes": total_bytes.saturating_sub(available_bytes),
		"disks": list,
	})
}
