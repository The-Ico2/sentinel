use chrono::{Local, Datelike, Timelike};
use serde_json::json;

pub fn get_time_json() -> serde_json::Value {
    let now = Local::now();

    let timestamp = now.timestamp();
    let millis = now.timestamp_millis();

    let iso = now.to_rfc3339();

    let date = now.date_naive();

    json!({
        "ok": true,
        "iso": iso,
        "unix": timestamp,
        "unix_ms": millis,
        "year": date.year(),
        "month": date.month(),
        "day": date.day(),
        "weekday": format!("{:?}", now.weekday()),
        "hour": now.hour(),
        "minute": now.minute(),
        "second": now.second(),
        "millisecond": (millis % 1000) as i64,
        "human": now.format("%Y-%m-%d %H:%M:%S%.3f").to_string()
    })
}
