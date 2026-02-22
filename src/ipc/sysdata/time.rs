use chrono::{Local, Datelike, Timelike, Utc};
use serde_json::json;
use sysinfo::System;

pub fn get_time_json() -> serde_json::Value {
    let now = Local::now();
    let utc_now = Utc::now();

    let timestamp = now.timestamp();
    let millis = now.timestamp_millis();

    let iso = now.to_rfc3339();
    let utc_iso = utc_now.to_rfc3339();

    let date = now.date_naive();

    let offset = now.offset();
    let utc_offset_seconds = offset.local_minus_utc();
    let utc_offset_hours = utc_offset_seconds as f64 / 3600.0;

    let uptime_seconds = System::uptime();
    let boot_time_unix = System::boot_time();

    // Day of year (1-366)
    let day_of_year = date.ordinal();

    // ISO week number
    let iso_week = date.iso_week().week();

    // Is leap year
    let is_leap_year = date.leap_year();

    // Quarter
    let quarter = ((date.month() - 1) / 3) + 1;

    let am_pm = if now.hour() < 12 { "AM" } else { "PM" };
    let hour_12 = {
        let h = now.hour() % 12;
        if h == 0 { 12 } else { h }
    };

    json!({
        "ok": true,
        "iso": iso,
        "utc_iso": utc_iso,
        "unix": timestamp,
        "unix_ms": millis,
        "year": date.year(),
        "month": date.month(),
        "day": date.day(),
        "weekday": format!("{:?}", now.weekday()),
        "day_of_year": day_of_year,
        "iso_week": iso_week,
        "quarter": quarter,
        "is_leap_year": is_leap_year,
        "hour": now.hour(),
        "minute": now.minute(),
        "second": now.second(),
        "millisecond": (millis % 1000) as i64,
        "timezone": format!("{}", offset),
        "utc_offset_seconds": utc_offset_seconds,
        "utc_offset_hours": utc_offset_hours,
        "uptime_seconds": uptime_seconds,
        "boot_time_unix": boot_time_unix,
        "human": now.format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
        "human_date": now.format("%Y-%m-%d").to_string(),
        "human_time": now.format("%H:%M:%S").to_string(),
        "am_pm": am_pm,
        "hour_12": hour_12,
    })
}
