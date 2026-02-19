// ~/sentinel/sentinel-backend/src/ipc/sysdata/network.rs

use serde_json::{json, Value};
use sysinfo::Networks;

pub fn get_network_json() -> Value {
	let mut networks = Networks::new_with_refreshed_list();
	networks.refresh(false);

	let mut total_rx: u64 = 0;
	let mut total_tx: u64 = 0;

	let list: Vec<Value> = (&networks)
		.into_iter()
		.map(|(name, data)| {
			let rx = data.received();
			let tx = data.transmitted();
			total_rx = total_rx.saturating_add(rx);
			total_tx = total_tx.saturating_add(tx);

			json!({
				"interface": name,
				"received_bytes": rx,
				"transmitted_bytes": tx,
				"total_received_bytes": data.total_received(),
				"total_transmitted_bytes": data.total_transmitted(),
			})
		})
		.collect();

	json!({
		"received_bytes": total_rx,
		"transmitted_bytes": total_tx,
		"interfaces": list,
	})
}
