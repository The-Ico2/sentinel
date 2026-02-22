// ~/sentinel/sentinel-backend/src/ipc/sysdata/network.rs

use serde_json::{json, Value};
use std::{collections::HashMap, sync::{Mutex, OnceLock}, time::Instant};
use sysinfo::Networks;

#[derive(Default)]
struct NetworkSnapshot {
	totals_by_name: HashMap<String, (u64, u64)>,
	last_tick: Option<Instant>,
}

pub fn get_network_json() -> Value {
	let mut networks = Networks::new_with_refreshed_list();
	networks.refresh(false);

	static PREV: OnceLock<Mutex<NetworkSnapshot>> = OnceLock::new();
	let prev_state = PREV.get_or_init(|| Mutex::new(NetworkSnapshot::default()));
	let mut prev = prev_state.lock().unwrap();
	let now = Instant::now();
	let elapsed_s = prev
		.last_tick
		.map(|t| now.saturating_duration_since(t).as_secs_f64())
		.unwrap_or(0.0)
		.max(0.001);

	let mut tick_rx: u64 = 0;
	let mut tick_tx: u64 = 0;
	let mut aggregate_total_rx: u64 = 0;
	let mut aggregate_total_tx: u64 = 0;
	let mut aggregate_packets_rx: u64 = 0;
	let mut aggregate_packets_tx: u64 = 0;
	let mut aggregate_errors_rx: u64 = 0;
	let mut aggregate_errors_tx: u64 = 0;
	let mut next_totals = HashMap::<String, (u64, u64)>::new();

	let list: Vec<Value> = (&networks)
		.into_iter()
		.map(|(name, data)| {
			let rx = data.received();
			let tx = data.transmitted();
			let total_rx = data.total_received();
			let total_tx = data.total_transmitted();

			let packets_rx = data.packets_received();
			let packets_tx = data.packets_transmitted();
			let total_packets_rx = data.total_packets_received();
			let total_packets_tx = data.total_packets_transmitted();

			let errors_rx = data.errors_on_received();
			let errors_tx = data.errors_on_transmitted();
			let total_errors_rx = data.total_errors_on_received();
			let total_errors_tx = data.total_errors_on_transmitted();

			let mac = data.mac_address();
			let ip_networks: Vec<Value> = data.ip_networks()
				.iter()
				.map(|ip| json!({
					"addr": ip.addr.to_string(),
					"prefix": ip.prefix,
				}))
				.collect();

			tick_rx = tick_rx.saturating_add(rx);
			tick_tx = tick_tx.saturating_add(tx);
			aggregate_total_rx = aggregate_total_rx.saturating_add(total_rx);
			aggregate_total_tx = aggregate_total_tx.saturating_add(total_tx);
			aggregate_packets_rx = aggregate_packets_rx.saturating_add(total_packets_rx);
			aggregate_packets_tx = aggregate_packets_tx.saturating_add(total_packets_tx);
			aggregate_errors_rx = aggregate_errors_rx.saturating_add(total_errors_rx);
			aggregate_errors_tx = aggregate_errors_tx.saturating_add(total_errors_tx);

			let prev_totals = prev
				.totals_by_name
				.get(name)
				.copied()
				.unwrap_or((total_rx, total_tx));

			let rx_per_second = ((total_rx.saturating_sub(prev_totals.0)) as f64 / elapsed_s).max(0.0);
			let tx_per_second = ((total_tx.saturating_sub(prev_totals.1)) as f64 / elapsed_s).max(0.0);

			next_totals.insert(name.to_string(), (total_rx, total_tx));

			json!({
				"interface": name,
				"mac_address": mac.to_string(),
				"ip_addresses": ip_networks,
				"received_bytes": rx,
				"transmitted_bytes": tx,
				"total_received_bytes": total_rx,
				"total_transmitted_bytes": total_tx,
				"received_bytes_per_second": rx_per_second,
				"transmitted_bytes_per_second": tx_per_second,
				"packets": {
					"received": packets_rx,
					"transmitted": packets_tx,
					"total_received": total_packets_rx,
					"total_transmitted": total_packets_tx,
				},
				"errors": {
					"received": errors_rx,
					"transmitted": errors_tx,
					"total_received": total_errors_rx,
					"total_transmitted": total_errors_tx,
				},
			})
		})
		.collect();

	prev.totals_by_name = next_totals;
	prev.last_tick = Some(now);

	json!({
		"received_bytes": tick_rx,
		"transmitted_bytes": tick_tx,
		"total_received_bytes": aggregate_total_rx,
		"total_transmitted_bytes": aggregate_total_tx,
		"received_bytes_per_second": if elapsed_s > 0.0 { tick_rx as f64 / elapsed_s } else { 0.0 },
		"transmitted_bytes_per_second": if elapsed_s > 0.0 { tick_tx as f64 / elapsed_s } else { 0.0 },
		"total_packets_received": aggregate_packets_rx,
		"total_packets_transmitted": aggregate_packets_tx,
		"total_errors_received": aggregate_errors_rx,
		"total_errors_transmitted": aggregate_errors_tx,
		"interface_count": list.len(),
		"interfaces": list,
	})
}
