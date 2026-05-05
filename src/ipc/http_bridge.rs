// ~/veil/veil-backend/src/ipc/http_bridge.rs
//
// Lightweight HTTP bridge for browser-based wallpaper prototyping.
// Exposes the same IPC dispatch as the named pipe server, but over
// HTTP so that wallpapers loaded in a web browser (via Live Server,
// file://, etc.) can access real-time VEIL data.
//
// Endpoints:
//   GET  /api/{ns}/{cmd}?sections=cpu,gpu,...
//   POST /api/{ns}/{cmd}   (JSON body = args)
//   OPTIONS *               (CORS preflight)
//
// Binds to 127.0.0.1:9851 (localhost only — no remote exposure).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};

/// Start the HTTP bridge server (blocking — call from a dedicated thread).
pub fn start_http_bridge() {
    let listener = match TcpListener::bind("127.0.0.1:9851") {
        Ok(l) => l,
        Err(e) => {
            crate::warn!("[HTTP] Failed to bind 127.0.0.1:9851: {}", e);
            return;
        }
    };
    crate::info!("[HTTP] Bridge listening on http://127.0.0.1:9851");

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                std::thread::spawn(move || {
                    let _ = handle_connection(stream);
                });
            }
            Err(e) => {
                crate::warn!("[HTTP] Accept error: {}", e);
            }
        }
    }
}

// ── Request handling ──────────────────────────────────────────────────

fn handle_connection(stream: TcpStream) -> Result<(), Box<dyn std::error::Error>> {
    stream.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(std::time::Duration::from_secs(5)))?;

    let mut reader = BufReader::new(&stream);

    // Read request line: "GET /api/registry/get_data?sections=cpu HTTP/1.1\r\n"
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;

    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 2 {
        return Ok(());
    }

    let method = parts[0];
    let raw_path = parts[1];

    // Read headers — we need Content-Length for POST bodies
    let mut content_length: usize = 0;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            break;
        }
        if let Some(val) = trimmed.strip_prefix("Content-Length:") {
            content_length = val.trim().parse().unwrap_or(0);
        } else if let Some(val) = trimmed.strip_prefix("content-length:") {
            content_length = val.trim().parse().unwrap_or(0);
        }
    }

    // Handle CORS preflight
    if method == "OPTIONS" {
        return write_response(&stream, 204, "", None);
    }

    // Parse path and query string
    let (path, query) = match raw_path.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (raw_path, None),
    };

    // Route: /api/{ns}/{cmd}
    let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();

    if segments.len() >= 3 && segments[0] == "api" {
        let ns = segments[1];
        let cmd = segments[2];

        // Build args from query string (GET) or body (POST)
        let args = if method == "POST" && content_length > 0 {
            let mut body = vec![0u8; content_length.min(1_048_576)]; // 1MB cap
            reader.read_exact(&mut body)?;
            serde_json::from_slice(&body).ok()
        } else {
            parse_query_to_args(query)
        };

        let body = match crate::ipc::dispatch::dispatch(ns, cmd, args) {
            Ok(data) => {
                serde_json::json!({ "ok": true, "data": data }).to_string()
            }
            Err(e) => {
                serde_json::json!({ "ok": false, "error": e }).to_string()
            }
        };

        write_response(&stream, 200, &body, Some("application/json"))
    } else {
        let body = serde_json::json!({
            "ok": false,
            "error": "Unknown endpoint. Use /api/{ns}/{cmd}"
        }).to_string();
        write_response(&stream, 404, &body, Some("application/json"))
    }
}

// ── Helpers ───────────────────────────────────────────────────────────

/// Write an HTTP response with CORS headers.
fn write_response(
    mut stream: &TcpStream,
    status: u16,
    body: &str,
    content_type: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let status_text = match status {
        200 => "OK",
        204 => "No Content",
        404 => "Not Found",
        _ => "OK",
    };

    let mut headers = format!(
        "HTTP/1.1 {} {}\r\n\
         Access-Control-Allow-Origin: *\r\n\
         Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n\
         Access-Control-Allow-Headers: Content-Type\r\n\
         Access-Control-Max-Age: 86400\r\n\
         Connection: close\r\n",
        status, status_text,
    );

    if let Some(ct) = content_type {
        headers.push_str(&format!("Content-Type: {}\r\n", ct));
        headers.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }

    headers.push_str("\r\n");
    stream.write_all(headers.as_bytes())?;

    if !body.is_empty() {
        stream.write_all(body.as_bytes())?;
    }

    Ok(())
}

/// Convert a query string like `sections=cpu,gpu,ram&foo=bar` into
/// a JSON Value suitable for the dispatch `args` parameter.
///
/// Special handling for `sections` — splits on comma into a JSON array,
/// matching the IPC protocol's expected `{ "sections": ["cpu", "gpu"] }`.
fn parse_query_to_args(query: Option<&str>) -> Option<serde_json::Value> {
    let q = query?;
    if q.is_empty() {
        return None;
    }

    let mut map = serde_json::Map::new();
    for pair in q.split('&') {
        if let Some((key, val)) = pair.split_once('=') {
            let decoded_val = url_decode(val);
            if key == "sections" {
                // Split comma-separated values into a JSON array
                let arr: Vec<serde_json::Value> = decoded_val
                    .split(',')
                    .map(|s| serde_json::Value::String(s.trim().to_string()))
                    .collect();
                map.insert(key.to_string(), serde_json::Value::Array(arr));
            } else {
                map.insert(key.to_string(), serde_json::Value::String(decoded_val));
            }
        }
    }

    if map.is_empty() {
        None
    } else {
        Some(serde_json::Value::Object(map))
    }
}

/// Minimal percent-decoding for query string values.
fn url_decode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '%' {
            let hex: String = chars.by_ref().take(2).collect();
            if hex.len() == 2 {
                if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                    result.push(byte as char);
                    continue;
                }
            }
            result.push('%');
            result.push_str(&hex);
        } else if c == '+' {
            result.push(' ');
        } else {
            result.push(c);
        }
    }
    result
}
