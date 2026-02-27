// ~/sentinel/sentinel-backend/src/ipc/dispatch/backendd.rs

use serde_json::{json, Value};
use crate::config;
use crate::ipc::data_updater::{set_explicit_tracking_demands, touch_ui_heartbeat};

pub fn dispatch_backend(cmd: &str, args: Option<Value>) -> Result<Value, String> {
    match cmd {
        "get_config" => {
            let cfg = config::current_config();
            Ok(json!({
                "fast_pull_rate_ms": cfg.fast_pull_rate_ms,
                "slow_pull_rate_ms": cfg.slow_pull_rate_ms,
                "data_pull_paused": cfg.data_pull_paused,
                "refresh_on_request": cfg.refresh_on_request,
                "ui_data_exception_enabled": cfg.ui_data_exception_enabled,
            }))
        }

        "set_fast_pull_rate" => {
            let ms = args
                .as_ref()
                .and_then(|a| a.get("rate_ms"))
                .and_then(|v| v.as_u64())
                .ok_or("Missing 'rate_ms' in args")?;
            config::set_fast_pull_rate_ms(ms);
            Ok(json!({ "fast_pull_rate_ms": config::fast_pull_rate_ms() }))
        }

        "set_slow_pull_rate" => {
            let ms = args
                .as_ref()
                .and_then(|a| a.get("rate_ms"))
                .and_then(|v| v.as_u64())
                .ok_or("Missing 'rate_ms' in args")?;
            config::set_slow_pull_rate_ms(ms);
            Ok(json!({ "slow_pull_rate_ms": config::slow_pull_rate_ms() }))
        }

        "set_pull_paused" => {
            let paused = args
                .as_ref()
                .and_then(|a| a.get("paused"))
                .and_then(|v| v.as_bool())
                .ok_or("Missing 'paused' in args")?;
            config::set_pull_paused(paused);
            Ok(json!({ "data_pull_paused": config::pull_paused() }))
        }

        "set_refresh_on_request" => {
            let enabled = args
                .as_ref()
                .and_then(|a| a.get("enabled"))
                .and_then(|v| v.as_bool())
                .ok_or("Missing 'enabled' in args")?;
            config::set_refresh_on_request(enabled);
            Ok(json!({ "refresh_on_request": config::refresh_on_request() }))
        }

        "set_ui_data_exception_enabled" => {
            let enabled = args
                .as_ref()
                .and_then(|a| a.get("enabled"))
                .and_then(|v| v.as_bool())
                .ok_or("Missing 'enabled' in args")?;
            config::set_ui_data_exception_enabled(enabled);
            Ok(json!({ "ui_data_exception_enabled": config::ui_data_exception_enabled() }))
        }

        "ui_heartbeat" => {
            touch_ui_heartbeat();
            Ok(json!({ "ok": true }))
        }

        "set_tracking_demands" => {
            let sections = args
                .as_ref()
                .and_then(|a| a.get("sections"))
                .and_then(|v| v.as_array())
                .ok_or("Missing 'sections' in args")?
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect::<Vec<_>>();

            set_explicit_tracking_demands(&sections);
            Ok(json!({ "sections": sections }))
        }

        _ => Err(format!("Unknown backend command: {}", cmd)),
    }
}
