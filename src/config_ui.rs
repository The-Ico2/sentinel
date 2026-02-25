use std::{borrow::Cow, collections::{HashMap, HashSet}, path::{Path, PathBuf}};

use eframe::{App, NativeOptions, egui};
use egui::{Color32, RichText, Stroke, TextureHandle, TextureOptions};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use serde_yaml::{Mapping, Value};
use tao::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder},
    window::WindowBuilder,
};
use wry::WebViewBuilder;

use crate::{error, info, warn};
use crate::ipc::sysdata::display::{MonitorInfo, MonitorManager};

#[derive(Clone)]
struct AddonMeta {
    id: String,
    name: String,
    package: String,
    addon_root: PathBuf,
    config_path: PathBuf,
    schema_path: PathBuf,
    accepts_assets: bool,
    asset_categories: Vec<String>,
}

struct AddonConfigState {
    meta: AddonMeta,
    root: Value,
    schema: Option<AddonSchema>,
    status: String,
    assets: Vec<AssetOption>,
    asset_selector_paths: Vec<Vec<String>>,
    custom_tabs: Vec<CustomTabPage>,
}

#[derive(Clone)]
struct CustomTabPage {
    id: String,
    title: String,
    path: PathBuf,
}

#[derive(Clone, Serialize)]
struct CustomTabShellPage {
    id: String,
    title: String,
    url: String,
}

#[derive(Clone, Serialize)]
struct CustomTabShellAddon {
    id: String,
    name: String,
    tabs: Vec<CustomTabShellPage>,
}

#[derive(Clone, Serialize)]
struct WallpaperShellAsset {
    id: String,
    name: String,
    tags: Vec<String>,
    short_description: Option<String>,
    last_updated: Option<String>,
    author_name: Option<String>,
    author_url: Option<String>,
    preview_url: Option<String>,
    html_url: Option<String>,
    editable: serde_json::Value,
    manifest_path: String,
}

#[derive(Clone, Serialize)]
struct WallpaperShellData {
    enabled: Option<bool>,
    wallpaper_id: Option<String>,
    mode: Option<String>,
    z_index: Option<String>,
    monitor_index: Vec<String>,
    assignments: HashMap<String, String>,
    monitors: Vec<WallpaperShellMonitor>,
    assets: Vec<WallpaperShellAsset>,
    // settings.development
    log_level: Option<String>,
    update_check: Option<bool>,
    debug: Option<bool>,
    // settings.runtime
    tick_sleep_ms: Option<i64>,
    reapply_on_pause_change: Option<bool>,
    // settings.performance.pausing
    pause_focus: Option<String>,
    pause_maximized: Option<String>,
    pause_fullscreen: Option<String>,
    pause_battery: Option<String>,
    pause_check_interval_ms: Option<i64>,
    // settings.performance.watcher
    watcher_enabled: Option<bool>,
    watcher_interval_ms: Option<i64>,
    // settings.performance.interactions
    interactions_send_move: Option<bool>,
    interactions_send_click: Option<bool>,
    interactions_poll_interval_ms: Option<i64>,
    interactions_move_threshold_px: Option<f64>,
    // settings.performance.audio
    audio_enabled: Option<bool>,
    audio_sample_interval_ms: Option<i64>,
    audio_endpoint_refresh_ms: Option<i64>,
    audio_retry_interval_ms: Option<i64>,
    audio_change_threshold: Option<f64>,
    audio_quantize_decimals: Option<i64>,
    // settings.diagnostics
    log_pause_state_changes: Option<bool>,
    log_watcher_reloads: Option<bool>,
    // per-monitor profiles
    profiles: Vec<WallpaperShellProfile>,
    // metadata
    addon_version: Option<String>,
    backend_version: Option<String>,
    cache_size_bytes: Option<u64>,
    addon_root_path: Option<String>,
}

#[derive(Clone, Serialize)]
struct WallpaperShellProfile {
    monitor_index: String,
    enabled: bool,
    wallpaper_id: String,
    mode: Option<String>,
    z_index: Option<String>,
}

#[derive(Clone, Serialize)]
struct WallpaperShellMonitor {
    id: String,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    scale: f32,
    primary: bool,
}

#[derive(Clone)]
struct WallpaperProfileEntry {
    section: String,
    enabled: bool,
    monitor_index: Vec<String>,
    wallpaper_id: String,
    mode: Option<String>,
    z_index: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ShellIpcMessage {
    #[serde(rename = "type")]
    kind: String,
    addon_id: Option<String>,
    wallpaper_id: Option<String>,
    monitor_ids: Option<Vec<String>>,
    monitor_indexes: Option<Vec<String>>,
    // For config_update
    path: Option<String>,
    value: Option<serde_json::Value>,
    // For wallpaper_update_property
    property: Option<String>,
    // For backend_setting
    key: Option<String>,
    // For wallpaper_save_editable / wallpaper_capture_preview
    manifest_path: Option<String>,
}

fn parse_shell_ipc_message(body: &str) -> Option<ShellIpcMessage> {
    if let Ok(direct) = serde_json::from_str::<ShellIpcMessage>(body) {
        return Some(direct);
    }

    let value = serde_json::from_str::<serde_json::Value>(body).ok()?;
    let payload = value
        .get("payload")
        .cloned()
        .unwrap_or_else(|| value.clone());
    serde_json::from_value::<ShellIpcMessage>(payload).ok()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum UiSection {
    Home,
    Addons,
    Integrations,
    Settings,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AddonHubTab {
    Library,
    Editor,
    Discover,
    Settings,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct AddonSchema {
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    ui: SchemaUi,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct SchemaUi {
    #[serde(default)]
    sections: Vec<SchemaSection>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct SchemaSection {
    title: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    render_mode: Option<String>,
    #[serde(default)]
    fields: Vec<SchemaField>,
    #[serde(default)]
    sections: Vec<SchemaSection>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct SchemaField {
    path: String,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    control: String,
    #[serde(default)]
    options: Vec<String>,
    #[serde(default)]
    min: Option<f64>,
    #[serde(default)]
    max: Option<f64>,
    #[serde(default)]
    step: Option<f64>,
    #[serde(default)]
    asset_category: Option<String>,
    #[serde(default)]
    show_preview: Option<bool>,
}

#[derive(Clone)]
struct AssetOption {
    id: String,
    name: String,
    version: Option<String>,
    tags: Vec<String>,
    short_description: Option<String>,
    long_description: Option<String>,
    last_updated: Option<String>,
    authors: Vec<(String, String)>,
    preview_paths: Vec<PathBuf>,
    manifest_path: PathBuf,
    editable: JsonValue,
}

struct UiCaches {
    preview_textures: HashMap<String, TextureHandle>,
    preview_index: HashMap<String, usize>,
}

impl UiCaches {
    fn new() -> Self {
        Self {
            preview_textures: HashMap::new(),
            preview_index: HashMap::new(),
        }
    }
}

pub fn run_addon_config_ui(addon_ref: &str) -> Result<(), Box<dyn std::error::Error>> {
    run_sentinel_ui(Some(addon_ref))
}

pub fn run_sentinel_ui(addon_focus: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    let addon_catalog = discover_addon_configs();
    if addon_catalog.is_empty() {
        warn!("No addon config.yaml files were discovered for Sentinel UI");
    }

    for addon in &addon_catalog {
        ensure_config_file_exists(&addon.config_path)?;
    }

    let custom_tab_addons = collect_custom_tab_shell_addons(&addon_catalog);
    if !custom_tab_addons.is_empty() {
        info!("Launching Sentinel WebView shell for custom addon tabs");
        return run_sentinel_custom_tabs_shell(custom_tab_addons, addon_focus);
    }

    let mut selected = 0usize;
    if let Some(focus) = addon_focus {
        if let Some(idx) = addon_catalog
            .iter()
            .position(|a| a.id.eq_ignore_ascii_case(focus) || a.name.eq_ignore_ascii_case(focus))
        {
            selected = idx;
        }
    }

    let addon_state = if addon_catalog.is_empty() {
        None
    } else {
        Some(load_addon_state(addon_catalog[selected].clone())?)
    };

    let app = SentinelApp {
        section: if addon_focus.is_some() {
            UiSection::Addons
        } else {
            UiSection::Home
        },
        addon_catalog,
        selected_addon_idx: selected,
        addon_state,
        global_status: "Ready".to_string(),
        caches: UiCaches::new(),
        addon_hub_tab: AddonHubTab::Settings,
        editor_selected_asset: None,
        library_selected_monitor: None,
        selected_custom_tab: None,
        last_opened_custom_tab: None,
        settings_fast_rate: 50,
        settings_slow_rate: 500,
        settings_pull_paused: false,
        settings_refresh_on_request: true,
        settings_loaded: false,
    };

    let options = NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1200.0, 760.0]),
        ..Default::default()
    };

    eframe::run_native("Sentinel", options, Box::new(move |_cc| Ok(Box::new(app))))
        .map_err(|e| format!("Failed to open Sentinel UI: {}", e))?;

    Ok(())
}

fn run_sentinel_custom_tabs_shell(
        addons: Vec<CustomTabShellAddon>,
        addon_focus: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
        if addons.is_empty() {
                return Ok(());
        }

        let selected_addon_id = addon_focus
                .and_then(|focus| {
                        addons
                                .iter()
                                .find(|a| a.id.eq_ignore_ascii_case(focus) || a.name.eq_ignore_ascii_case(focus))
                                .map(|a| a.id.clone())
                })
                .unwrap_or_else(|| addons[0].id.clone());

        let html = build_sentinel_custom_tabs_shell_html(&addons, &selected_addon_id)?;
        let shell_path = sentinel_shell_html_path()?;
        if let Some(parent) = shell_path.parent() {
                std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&shell_path, html)?;

        // Use sentinel:// custom protocol so shell + iframes are same-origin.
        // This is critical: WebView2's WebMessageReceived only fires for
        // top-level frame messages, and file:// iframes silently drop
        // window.parent.postMessage due to opaque-origin restrictions.
        // With sentinel:// → http://sentinel.localhost, both frames share
        // the same origin, so the iframe can directly call
        // window.parent.__sentinelBridgePost() to relay to Rust.
        let sentinel_home = sentinel_home_dir()?;
        let shell_url = file_path_to_sentinel_url(&shell_path, &sentinel_home)?;
        info!("[ui] Launching Sentinel custom-tab shell at {}", shell_url);

        let event_loop = EventLoopBuilder::new().build();
        let window = WindowBuilder::new()
                .with_title("Sentinel")
                .build(&event_loop)
                .map_err(|e| format!("Failed to create Sentinel shell window: {}", e))?;

        let protocol_root = sentinel_home.clone();
        let webview = WebViewBuilder::new()
                .with_custom_protocol("sentinel".to_string(), move |_webview_id, request| {
                    let uri = request.uri().to_string();
                    // Extract path from sentinel://localhost/path or http://sentinel.localhost/path
                    let raw_path = uri
                        .strip_prefix("sentinel://localhost")
                        .or_else(|| uri.strip_prefix("sentinel://"))
                        .or_else(|| {
                            // WebView2 workaround: http://sentinel.localhost/path
                            uri.strip_prefix("http://sentinel.localhost")
                                .or_else(|| uri.strip_prefix("https://sentinel.localhost"))
                        })
                        .unwrap_or(&uri);
                    let path_part = raw_path.split('?').next().unwrap_or("");
                    let clean = path_part.trim_start_matches('/');
                    let decoded = urlencoding::decode(clean).unwrap_or_else(|_| clean.into());
                    let file_path = protocol_root.join(decoded.replace('/', "\\"));

                    match std::fs::read(&file_path) {
                        Ok(data) => {
                            let mime = guess_mime_type(&file_path);
                            wry::http::Response::builder()
                                .header("Content-Type", mime)
                                .header("Access-Control-Allow-Origin", "*")
                                .header("Cache-Control", "no-store, no-cache, must-revalidate")
                                .body(Cow::Owned(data))
                                .unwrap()
                        }
                        Err(_) => {
                            wry::http::Response::builder()
                                .status(404)
                                .header("Content-Type", "text/plain")
                                .body(Cow::Borrowed(b"Not Found" as &[u8]))
                                .unwrap()
                        }
                    }
                })
                .with_url(&shell_url)
                .with_initialization_script(
                    // This runs in ALL frames (main + iframes) on WebView2.
                    // Because we serve everything through sentinel:// custom
                    // protocol, the shell and iframes are same-origin.
                    // Iframes can directly call window.parent.__sentinelBridgePost()
                    // to relay messages to Rust via the top-level frame's
                    // window.ipc.postMessage → WebMessageReceived handler.
                    r#"
                    (function() {
                        var isTopFrame;
                        try { isTopFrame = (window === window.top); } catch(e) { isTopFrame = false; }

                        if (isTopFrame) {
                            window.__sentinelIPC = function(payload) {
                                try {
                                    var msg = (typeof payload === 'string') ? payload : JSON.stringify(payload);
                                    if (window.chrome && window.chrome.webview && typeof window.chrome.webview.postMessage === 'function') {
                                        window.chrome.webview.postMessage(msg);
                                        return true;
                                    }
                                    if (window.ipc && typeof window.ipc.postMessage === 'function') {
                                        window.ipc.postMessage(msg);
                                        return true;
                                    }
                                } catch(e) {}
                                return false;
                            };
                        } else {
                            // Same-origin: directly call parent's bridge function
                            window.__sentinelIPC = function(payload) {
                                try {
                                    var msg = (typeof payload === 'string') ? payload : JSON.stringify(payload);
                                    if (window.parent && typeof window.parent.__sentinelBridgePost === 'function') {
                                        return !!window.parent.__sentinelBridgePost(msg);
                                    }
                                    // Fallback: direct top-level ipc if accessible
                                    if (window.parent && window.parent.ipc && typeof window.parent.ipc.postMessage === 'function') {
                                        window.parent.ipc.postMessage(msg);
                                        return true;
                                    }
                                } catch(e) {}
                                return false;
                            };
                        }
                    })();
                    "#.to_string()
                )
                .with_ipc_handler(|request| {
                    let payload = request.body().to_string();
                    warn!("[ui] IPC handler invoked, payload length={}", payload.len());
                    let result = std::panic::catch_unwind(move || {
                        let Some(message) = parse_shell_ipc_message(&payload) else {
                            warn!("[ui] Unrecognized shell IPC payload: {}", payload);
                            return;
                        };

                        warn!("[ui] Shell IPC message kind='{}'", message.kind);
                        let addon_id = message
                            .addon_id
                            .clone()
                            .unwrap_or_else(|| "sentinel.addon.wallpaper".to_string());

                        match message.kind.to_lowercase().as_str() {
                            "wallpaper_apply_assignment" => {
                                let wallpaper_id = match message.wallpaper_id {
                                    Some(v) if !v.trim().is_empty() => v,
                                    _ => return,
                                };
                                let monitor_ids = message.monitor_ids.unwrap_or_default();
                                let monitor_indexes = message.monitor_indexes.unwrap_or_default();

                                match apply_wallpaper_assignment_from_shell(
                                    &addon_id,
                                    &wallpaper_id,
                                    &monitor_ids,
                                    &monitor_indexes,
                                ) {
                                    Ok(_) => warn!(
                                        "[ui] Saved wallpaper assignment: addon='{}' wallpaper='{}' indexes={:?}",
                                        addon_id, wallpaper_id, monitor_indexes
                                    ),
                                    Err(e) => warn!(
                                        "[ui] Failed saving wallpaper assignment: error={}", e
                                    ),
                                }
                            }
                            "config_update" => {
                                let path = message.path.unwrap_or_default();
                                let value = message.value.unwrap_or(serde_json::Value::Null);
                                match apply_config_update(&addon_id, &path, &value) {
                                    Ok(_) => warn!("[ui] Config update: {}={}", path, value),
                                    Err(e) => warn!("[ui] Config update failed: {}", e),
                                }
                            }
                            "wallpaper_update_property" => {
                                let monitor_indexes = message.monitor_indexes.unwrap_or_default();
                                let property = message.property.unwrap_or_default();
                                let value = message.value.unwrap_or(serde_json::Value::Null);
                                match apply_wallpaper_property_update(&addon_id, &monitor_indexes, &property, &value) {
                                    Ok(_) => warn!("[ui] Wallpaper property {}={} for {:?}", property, value, monitor_indexes),
                                    Err(e) => warn!("[ui] Wallpaper property update failed: {}", e),
                                }
                            }
                            "clear_cache" => {
                                match clear_addon_cache(&addon_id) {
                                    Ok(_) => warn!("[ui] Cache cleared for '{}'", addon_id),
                                    Err(e) => warn!("[ui] Cache clear failed: {}", e),
                                }
                            }
                            "backend_setting" => {
                                let key = message.key.unwrap_or_default();
                                let value = message.value.unwrap_or(serde_json::Value::Null);
                                warn!("[ui] Backend setting update: {}={}", key, value);
                                match key.as_str() {
                                    "fast_pull_rate" => {
                                        if let Some(ms) = value.as_u64() {
                                            crate::config::set_fast_pull_rate_ms(ms);
                                        }
                                    }
                                    "slow_pull_rate" => {
                                        if let Some(ms) = value.as_u64() {
                                            crate::config::set_slow_pull_rate_ms(ms);
                                        }
                                    }
                                    "pull_paused" => {
                                        if let Some(paused) = value.as_bool() {
                                            crate::config::set_pull_paused(paused);
                                        }
                                    }
                                    "refresh_on_request" => {
                                        if let Some(enabled) = value.as_bool() {
                                            crate::config::set_refresh_on_request(enabled);
                                        }
                                    }
                                    _ => {
                                        warn!("[ui] Unknown backend setting key: {}", key);
                                    }
                                }
                            }
                            "wallpaper_save_editable" => {
                                let wallpaper_id = message.wallpaper_id.unwrap_or_default();
                                let key = message.key.unwrap_or_default();
                                let value = message.value.unwrap_or(serde_json::Value::Null);
                                let manifest_path_str = message.manifest_path.unwrap_or_default();
                                match save_editable_to_manifest(&manifest_path_str, &key, &value) {
                                    Ok(_) => warn!("[ui] Editable saved: wp='{}' key='{}' val={}", wallpaper_id, key, value),
                                    Err(e) => warn!("[ui] Editable save failed: {}", e),
                                }
                            }
                            "wallpaper_capture_preview" => {
                                let wallpaper_id = message.wallpaper_id.unwrap_or_default();
                                let manifest_path_str = message.manifest_path.unwrap_or_default();
                                match capture_wallpaper_preview(&manifest_path_str) {
                                    Ok(_) => warn!("[ui] Preview captured for '{}'", wallpaper_id),
                                    Err(e) => warn!("[ui] Preview capture failed: {}", e),
                                }
                            }
                            other => {
                                warn!("[ui] Unhandled IPC message kind: '{}'", other);
                            }
                        }
                    });

                    if result.is_err() {
                        warn!("[ui] Recovered from panic while handling shell IPC message");
                    }
                })
                .build(&window)
                .map_err(|e| format!("Failed to create Sentinel shell webview: {}", e))?;

        let mut last_monitor_poll = std::time::Instant::now();
        let mut cached_monitor_json = String::new();
        let mut cached_registry_json = String::new();
        let mut last_registry_push = std::time::Instant::now();
        let snapshot_home = sentinel_home.clone();

        event_loop.run(move |event, _, control_flow| {
                *control_flow = ControlFlow::WaitUntil(
                    std::time::Instant::now() + std::time::Duration::from_millis(500)
                );

                // Periodic monitor polling for live UI updates (every 2s)
                if last_monitor_poll.elapsed() >= std::time::Duration::from_millis(2000) {
                    last_monitor_poll = std::time::Instant::now();
                    let fresh_monitors: Vec<WallpaperShellMonitor> = MonitorManager::enumerate_monitors()
                        .into_iter()
                        .map(|m| WallpaperShellMonitor {
                            id: m.id,
                            x: m.x,
                            y: m.y,
                            width: m.width,
                            height: m.height,
                            scale: m.scale,
                            primary: m.primary,
                        })
                        .collect();
                    if let Ok(json) = serde_json::to_string(&fresh_monitors) {
                        if json != cached_monitor_json {
                            cached_monitor_json = json.clone();
                            let _ = webview.evaluate_script(&format!(
                                "if(typeof __sentinelPushMonitors==='function')__sentinelPushMonitors({});",
                                json
                            ));
                        }
                    }
                }

                // Push live registry data to the Data page (every 500ms).
                // The UI runs in a separate process from the backend daemon,
                // so global_registry() here is empty. Read the registry.json
                // snapshot that the daemon writes to disk instead.
                if last_registry_push.elapsed() >= std::time::Duration::from_millis(500) {
                    last_registry_push = std::time::Instant::now();
                    let registry_path = snapshot_home.join("registry.json");
                    if let Ok(json_str) = std::fs::read_to_string(&registry_path) {
                        // Only push if data actually changed
                        if json_str != cached_registry_json {
                            cached_registry_json = json_str.clone();
                            let _ = webview.evaluate_script(&format!(
                                "if(typeof __sentinelPushRegistry==='function')__sentinelPushRegistry({});",
                                json_str
                            ));
                        }
                    }
                }

                match &event {
                    Event::WindowEvent { event: win_event, .. } => {
                        match win_event {
                            WindowEvent::CloseRequested => {
                                warn!("[ui] Shell window CloseRequested — exiting event loop");
                                *control_flow = ControlFlow::Exit;
                            }
                            WindowEvent::Destroyed => {
                                warn!("[ui] Shell window Destroyed event received");
                            }
                            _ => {}
                        }
                    }
                    Event::LoopDestroyed => {
                        warn!("[ui] Shell event loop destroyed");
                    }
                    _ => {}
                }
        });
}

fn sentinel_home_dir() -> Result<PathBuf, String> {
        let home = std::env::var("USERPROFILE").map_err(|_| "USERPROFILE not set".to_string())?;
        Ok(Path::new(&home).join(".Sentinel"))
}

fn sentinel_shell_html_path() -> Result<PathBuf, String> {
        Ok(sentinel_home_dir()?
                .join("cache")
                .join("sentinel_custom_tabs_shell.html"))
}

/// Convert a filesystem path under .Sentinel to a sentinel:// custom protocol URL.
/// E.g. `C:\Users\Xande\.Sentinel\Addons\wallpaper\options\library.html`
///    → `sentinel://localhost/Addons/wallpaper/options/library.html`
fn file_path_to_sentinel_url(path: &Path, sentinel_home: &Path) -> Result<String, String> {
        let canonical = std::fs::canonicalize(path)
                .map_err(|e| format!("Failed to resolve path '{}': {}", path.display(), e))?;
        let home_canonical = std::fs::canonicalize(sentinel_home)
                .map_err(|e| format!("Failed to resolve home '{}': {}", sentinel_home.display(), e))?;

        let mut canon_str = canonical.to_string_lossy().to_string();
        let mut home_str = home_canonical.to_string_lossy().to_string();
        // Strip UNC prefix if present
        if let Some(stripped) = canon_str.strip_prefix(r"\\?\") {
                canon_str = stripped.to_string();
        }
        if let Some(stripped) = home_str.strip_prefix(r"\\?\") {
                home_str = stripped.to_string();
        }

        let relative = canon_str
                .strip_prefix(&home_str)
                .ok_or_else(|| format!("Path '{}' is not under sentinel home '{}'", canon_str, home_str))?
                .trim_start_matches('\\');

        let url_path = relative.replace('\\', "/").replace(' ', "%20");
        // WebView2 rewrites sentinel://localhost/ to http://sentinel.localhost/
        // internally. URLs embedded in page content (iframe src, img src, etc.)
        // must use the rewritten http:// form to be navigable within the page.
        Ok(format!("http://sentinel.localhost/{}", url_path))
}

fn guess_mime_type(path: &Path) -> &'static str {
        match path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase().as_str() {
                "html" | "htm" => "text/html",
                "css" => "text/css",
                "js" | "mjs" => "application/javascript",
                "json" => "application/json",
                "png" => "image/png",
                "jpg" | "jpeg" => "image/jpeg",
                "gif" => "image/gif",
                "svg" => "image/svg+xml",
                "ico" => "image/x-icon",
                "webp" => "image/webp",
                "woff" => "font/woff",
                "woff2" => "font/woff2",
                "ttf" => "font/ttf",
                "otf" => "font/otf",
                "mp4" => "video/mp4",
                "webm" => "video/webm",
                "mp3" => "audio/mpeg",
                "ogg" => "audio/ogg",
                "wav" => "audio/wav",
                "xml" => "application/xml",
                "txt" => "text/plain",
                "yaml" | "yml" => "text/yaml",
                _ => "application/octet-stream",
        }
}

fn collect_custom_tab_shell_addons(catalog: &[AddonMeta]) -> Vec<CustomTabShellAddon> {
        let sentinel_home = match sentinel_home_dir() {
                Ok(h) => h,
                Err(_) => return Vec::new(),
        };
        let mut out = Vec::new();
        for addon in catalog {
                let tabs = discover_custom_tabs(addon);
                if tabs.is_empty() {
                        continue;
                }

        let wallpaper_payload = build_wallpaper_shell_data(addon, &sentinel_home);

                let shell_tabs: Vec<CustomTabShellPage> = tabs
                        .into_iter()
                        .filter_map(|t| {
                file_path_to_sentinel_url(&t.path, &sentinel_home).ok().map(|base_url| {
                    let url = append_sentinel_data_query(&base_url, &addon.id, wallpaper_payload.as_ref());
                    CustomTabShellPage {
                                        id: t.id,
                                        title: t.title,
                                        url,
                    }
                })
                        })
                        .collect();

                if shell_tabs.is_empty() {
                        continue;
                }

                out.push(CustomTabShellAddon {
                        id: addon.id.clone(),
                        name: addon.name.clone(),
                        tabs: shell_tabs,
                });
        }
        out
}

fn append_sentinel_data_query(
    base_url: &str,
    addon_id: &str,
    wallpaper: Option<&WallpaperShellData>,
) -> String {
    let payload = serde_json::json!({
        "addonId": addon_id,
        "wallpaper": wallpaper,
    });
    let payload_str = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string());
    let encoded = urlencoding::encode(&payload_str);
    let sep = if base_url.contains('?') { "&" } else { "?" };
    format!("{}{}sentinelData={}", base_url, sep, encoded)
}

fn build_wallpaper_shell_data(addon: &AddonMeta, sentinel_home: &Path) -> Option<WallpaperShellData> {
    let is_wallpaper = addon.package.eq_ignore_ascii_case("wallpaper")
        || addon.id.to_lowercase().contains("wallpaper")
        || addon.name.to_lowercase().contains("wallpaper");
    if !is_wallpaper {
        return None;
    }

    let config_content = std::fs::read_to_string(&addon.config_path).ok()?;
    let config_root = serde_yaml::from_str::<Value>(&config_content).ok()?;

    let schema = load_schema(&addon.schema_path);
    let mut monitors = MonitorManager::enumerate_monitors()
        .into_iter()
        .map(|m| WallpaperShellMonitor {
            id: m.id,
            x: m.x,
            y: m.y,
            width: m.width,
            height: m.height,
            scale: m.scale,
            primary: m.primary,
        })
        .collect::<Vec<_>>();
    sort_monitors_for_wallpaper_indexes(&mut monitors);

    let profiles = parse_wallpaper_profiles(&config_root);
    let enabled_profiles: Vec<&WallpaperProfileEntry> = profiles.iter().filter(|p| p.enabled).collect();

    let mut assignments = build_monitor_assignments(&monitors, &enabled_profiles);
    if assignments.is_empty() {
        assignments = yaml_string_map(&config_root, "wallpaper.assignments");
    }

    let primary_profile = enabled_profiles
        .first()
        .copied()
        .or_else(|| profiles.first());

    let assets = discover_assets_for_meta(addon, schema.as_ref())
        .into_iter()
        .map(|asset| {
            let (author_name, author_url) = asset
                .authors
                .first()
                .cloned()
                .map(|(n, u)| (Some(n), Some(u)))
                .unwrap_or((None, None));

            let preview_url = asset
                .preview_paths
                .first()
                .and_then(|p| file_path_to_sentinel_url(p, sentinel_home).ok());

            // Resolve the wallpaper's index.html URL
            let manifest_dir = asset.manifest_path.parent().unwrap_or(Path::new(""));
            let index_path = manifest_dir.join("index.html");
            let html_url = if index_path.exists() {
                file_path_to_sentinel_url(&index_path, sentinel_home).ok()
            } else {
                None
            };

            WallpaperShellAsset {
                id: asset.id,
                name: asset.name,
                tags: asset.tags,
                short_description: asset.short_description,
                last_updated: asset.last_updated,
                author_name,
                author_url,
                preview_url,
                html_url,
                editable: asset.editable.clone(),
                manifest_path: asset.manifest_path.to_string_lossy().to_string(),
            }
        })
        .collect::<Vec<_>>();

    let shell_profiles: Vec<WallpaperShellProfile> = profiles.iter().map(|p| {
        WallpaperShellProfile {
            monitor_index: p.monitor_index.first().cloned().unwrap_or_else(|| "*".to_string()),
            enabled: p.enabled,
            wallpaper_id: p.wallpaper_id.clone(),
            mode: p.mode.clone(),
            z_index: p.z_index.clone(),
        }
    }).collect();

    // Addon version from addon.json
    let addon_json_path = addon.addon_root.join("addon.json");
    let addon_version = std::fs::read_to_string(&addon_json_path).ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("version").and_then(|v| v.as_str().map(|s| s.to_string())));

    let backend_version = Some(env!("CARGO_PKG_VERSION").to_string());

    let cache_dir = addon.addon_root.join("cache");
    let cache_size_bytes = Some(dir_size(&cache_dir));
    let addon_root_path = Some(addon.addon_root.to_string_lossy().to_string());

    Some(WallpaperShellData {
        enabled: Some(!enabled_profiles.is_empty())
            .or_else(|| yaml_bool(&config_root, "wallpaper.enabled")),
        wallpaper_id: primary_profile
            .map(|p| p.wallpaper_id.clone())
            .or_else(|| yaml_string(&config_root, "wallpaper.wallpaper_id")),
        mode: primary_profile
            .and_then(|p| p.mode.clone())
            .or_else(|| yaml_string(&config_root, "wallpaper.mode")),
        z_index: primary_profile
            .and_then(|p| p.z_index.clone())
            .or_else(|| yaml_string(&config_root, "wallpaper.z_index")),
        monitor_index: primary_profile
            .map(|p| p.monitor_index.clone())
            .unwrap_or_else(|| yaml_string_list(&config_root, "wallpaper.monitor_index")),
        assignments,
        monitors,
        assets,
        // settings.development
        log_level: yaml_string(&config_root, "settings.development.log_level"),
        update_check: yaml_bool(&config_root, "settings.development.update_check"),
        debug: yaml_bool(&config_root, "settings.development.debug"),
        // settings.runtime
        tick_sleep_ms: yaml_i64(&config_root, "settings.runtime.tick_sleep_ms"),
        reapply_on_pause_change: yaml_bool(&config_root, "settings.runtime.reapply_on_pause_change"),
        // settings.performance.pausing
        pause_focus: yaml_string(&config_root, "settings.performance.pausing.focus"),
        pause_maximized: yaml_string(&config_root, "settings.performance.pausing.maximized"),
        pause_fullscreen: yaml_string(&config_root, "settings.performance.pausing.fullscreen"),
        pause_battery: yaml_string(&config_root, "settings.performance.pausing.battery"),
        pause_check_interval_ms: yaml_i64(&config_root, "settings.performance.pausing.check_interval_ms"),
        // settings.performance.watcher
        watcher_enabled: yaml_bool(&config_root, "settings.performance.watcher.enabled"),
        watcher_interval_ms: yaml_i64(&config_root, "settings.performance.watcher.interval_ms"),
        // settings.performance.interactions
        interactions_send_move: yaml_bool(&config_root, "settings.performance.interactions.send_move"),
        interactions_send_click: yaml_bool(&config_root, "settings.performance.interactions.send_click"),
        interactions_poll_interval_ms: yaml_i64(&config_root, "settings.performance.interactions.poll_interval_ms"),
        interactions_move_threshold_px: yaml_f64(&config_root, "settings.performance.interactions.move_threshold_px"),
        // settings.performance.audio
        audio_enabled: yaml_bool(&config_root, "settings.performance.audio.enabled"),
        audio_sample_interval_ms: yaml_i64(&config_root, "settings.performance.audio.sample_interval_ms"),
        audio_endpoint_refresh_ms: yaml_i64(&config_root, "settings.performance.audio.endpoint_refresh_ms"),
        audio_retry_interval_ms: yaml_i64(&config_root, "settings.performance.audio.retry_interval_ms"),
        audio_change_threshold: yaml_f64(&config_root, "settings.performance.audio.change_threshold"),
        audio_quantize_decimals: yaml_i64(&config_root, "settings.performance.audio.quantize_decimals"),
        // settings.diagnostics
        log_pause_state_changes: yaml_bool(&config_root, "settings.diagnostics.log_pause_state_changes"),
        log_watcher_reloads: yaml_bool(&config_root, "settings.diagnostics.log_watcher_reloads"),
        // per-monitor profiles
        profiles: shell_profiles,
        // metadata
        addon_version,
        backend_version,
        cache_size_bytes,
        addon_root_path,
    })
}

fn parse_wallpaper_profiles(root: &Value) -> Vec<WallpaperProfileEntry> {
    let Some(root_map) = root.as_mapping() else {
        return Vec::new();
    };

    let mut out = Vec::<WallpaperProfileEntry>::new();

    for (key, value) in root_map {
        let Some(section) = key.as_str() else {
            continue;
        };
        if !section.starts_with("wallpaper") {
            continue;
        }
        if let Some(section_map) = value.as_mapping() {
            if let Some(entry) = parse_wallpaper_profile_section(section, section_map) {
                out.push(entry);
            }
        }
    }

    if let Some(Value::Mapping(wallpapers_map)) = root_map.get(Value::String("wallpapers".to_string())) {
        for (key, value) in wallpapers_map {
            let Some(section) = key.as_str() else {
                continue;
            };
            if !section.starts_with("wallpaper") {
                continue;
            }
            if let Some(section_map) = value.as_mapping() {
                if let Some(entry) = parse_wallpaper_profile_section(section, section_map) {
                    out.push(entry);
                }
            }
        }
    }

    out.sort_by(|a, b| wallpaper_section_order_key(&a.section).cmp(&wallpaper_section_order_key(&b.section)));
    out
}

fn parse_wallpaper_profile_section(section: &str, map: &Mapping) -> Option<WallpaperProfileEntry> {
    let wallpaper_id = map
        .get(Value::String("wallpaper_id".to_string()))
        .and_then(|v| v.as_str())?
        .trim()
        .to_string();
    if wallpaper_id.is_empty() {
        return None;
    }

    let enabled = map
        .get(Value::String("enabled".to_string()))
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let monitor_index = match map.get(Value::String("monitor_index".to_string())) {
        Some(Value::Sequence(seq)) => {
            let parsed: Vec<String> = seq
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();
            if parsed.is_empty() {
                vec!["*".to_string()]
            } else {
                parsed
            }
        }
        Some(Value::String(v)) => vec![v.clone()],
        _ => vec!["*".to_string()],
    };

    let mode = map
        .get(Value::String("mode".to_string()))
        .and_then(|v| v.as_str())
        .map(|s| s.to_lowercase());
    let z_index = map
        .get(Value::String("z_index".to_string()))
        .and_then(|v| v.as_str())
        .map(|s| s.to_lowercase());

    Some(WallpaperProfileEntry {
        section: section.to_string(),
        enabled,
        monitor_index,
        wallpaper_id,
        mode,
        z_index,
    })
}

fn wallpaper_section_order_key(section: &str) -> (u8, u32, String) {
    if section == "wallpaper" {
        return (0, 0, section.to_string());
    }

    if let Some(suffix) = section.strip_prefix("wallpaper") {
        if let Ok(number) = suffix.parse::<u32>() {
            return (1, number, section.to_string());
        }
    }

    (2, u32::MAX, section.to_string())
}

fn sort_monitors_for_wallpaper_indexes(monitors: &mut [WallpaperShellMonitor]) {
    if monitors.len() <= 1 {
        return;
    }

    let min_height = monitors
        .iter()
        .map(|m| m.height.max(1))
        .min()
        .unwrap_or(1);
    let row_tolerance = (min_height / 4).max(80);

    monitors.sort_by(|a, b| b.y.cmp(&a.y));

    let mut rows: Vec<(i32, Vec<WallpaperShellMonitor>)> = Vec::new();
    for monitor in monitors.iter().cloned() {
        if let Some((_, row)) = rows
            .iter_mut()
            .find(|(anchor_y, _)| (monitor.y - *anchor_y).abs() <= row_tolerance)
        {
            row.push(monitor);
        } else {
            rows.push((monitor.y, vec![monitor]));
        }
    }

    rows.sort_by(|(ay, _), (by, _)| by.cmp(ay));

    let mut flattened = Vec::<WallpaperShellMonitor>::with_capacity(monitors.len());
    for (_, mut row) in rows {
        row.sort_by(|a, b| a.x.cmp(&b.x));
        flattened.extend(row);
    }

    for (idx, monitor) in flattened.into_iter().enumerate() {
        monitors[idx] = monitor;
    }
}

fn build_monitor_assignments(
    monitors: &[WallpaperShellMonitor],
    profiles: &[&WallpaperProfileEntry],
) -> HashMap<String, String> {
    let mut assignments = HashMap::<String, String>::new();
    let mut assigned = HashSet::<usize>::new();

    for priority in [0u8, 1u8, 2u8] {
        for profile in profiles {
            if profile_priority(profile) != priority {
                continue;
            }

            let targets = resolve_profile_monitor_indexes(monitors, &profile.monitor_index, &assigned);
            if targets.is_empty() {
                continue;
            }

            for &index in &targets {
                assigned.insert(index);
                if let Some(monitor) = monitors.get(index) {
                    assignments.insert(monitor.id.clone(), profile.wallpaper_id.clone());
                }
            }

            if profile.monitor_index.iter().any(|k| k == "*") {
                assignments
                    .entry("*".to_string())
                    .or_insert_with(|| profile.wallpaper_id.clone());
            }
        }
    }

    assignments
}

fn profile_priority(profile: &WallpaperProfileEntry) -> u8 {
    if profile
        .monitor_index
        .iter()
        .any(|k| k.eq_ignore_ascii_case("p"))
    {
        return 0;
    }
    if profile.monitor_index.iter().any(|k| k == "*") {
        return 2;
    }
    1
}

fn resolve_profile_monitor_indexes(
    monitors: &[WallpaperShellMonitor],
    keys: &[String],
    assigned: &HashSet<usize>,
) -> Vec<usize> {
    let mut out = Vec::<usize>::new();

    if keys.iter().any(|k| k.eq_ignore_ascii_case("p")) {
        if let Some((idx, _)) = monitors.iter().enumerate().find(|(_, m)| m.primary) {
            out.push(idx);
        }
    }

    for key in keys {
        if key == "*" || key.eq_ignore_ascii_case("p") {
            continue;
        }

        if let Ok(idx) = key.parse::<usize>() {
            if idx < monitors.len() && !assigned.contains(&idx) && !out.contains(&idx) {
                out.push(idx);
            }
        }
    }

    if keys.iter().any(|k| k == "*") {
        for idx in 0..monitors.len() {
            if assigned.contains(&idx) || out.contains(&idx) {
                continue;
            }
            out.push(idx);
        }
    }

    out
}

fn apply_wallpaper_assignment_from_shell(
    addon_id: &str,
    wallpaper_id: &str,
    monitor_ids: &[String],
    monitor_indexes: &[String],
) -> Result<(), String> {
    if monitor_ids.is_empty() && monitor_indexes.is_empty() {
        return Err("No monitor IDs supplied".to_string());
    }

    let addon = discover_addon_configs()
        .into_iter()
        .find(|a| a.id.eq_ignore_ascii_case(addon_id))
        .ok_or_else(|| format!("Addon '{}' not found", addon_id))?;

    let mut target_indexes = monitor_indexes
        .iter()
        .filter(|v| !v.trim().is_empty())
        .cloned()
        .collect::<Vec<_>>();

    if target_indexes.is_empty() {
        let mut monitors = MonitorManager::enumerate_monitors()
            .into_iter()
            .map(|m| WallpaperShellMonitor {
                id: m.id,
                x: m.x,
                y: m.y,
                width: m.width,
                height: m.height,
                scale: m.scale,
                primary: m.primary,
            })
            .collect::<Vec<_>>();
        sort_monitors_for_wallpaper_indexes(&mut monitors);

        for monitor_id in monitor_ids {
            if let Some(idx) = monitors.iter().position(|m| m.id == *monitor_id) {
                target_indexes.push(idx.to_string());
            }
        }
    }
    target_indexes.sort();
    target_indexes.dedup();

    if target_indexes.is_empty() {
        return Err("No monitor indexes resolved from monitor IDs".to_string());
    }

    let content = std::fs::read_to_string(&addon.config_path).unwrap_or_else(|_| "{}".to_string());
    let mut root = serde_yaml::from_str::<Value>(&content).unwrap_or_else(|_| Value::Mapping(Mapping::new()));
    if !matches!(root, Value::Mapping(_)) {
        root = Value::Mapping(Mapping::new());
    }

    let root_map = root
        .as_mapping_mut()
        .ok_or_else(|| "Config root is not a mapping".to_string())?;

    let wallpapers_value = root_map
        .entry(Value::String("wallpapers".to_string()))
        .or_insert_with(|| Value::Mapping(Mapping::new()));
    if !matches!(wallpapers_value, Value::Mapping(_)) {
        *wallpapers_value = Value::Mapping(Mapping::new());
    }

    let wallpapers_map = wallpapers_value
        .as_mapping_mut()
        .ok_or_else(|| "'wallpapers' is not a mapping".to_string())?;

    for target_idx in &target_indexes {
        upsert_wallpaper_profile_for_index(wallpapers_map, target_idx, wallpaper_id);
    }

    let serialized = serde_yaml::to_string(&root)
        .map_err(|e| format!("Failed to serialize YAML: {}", e))?;
    std::fs::write(&addon.config_path, serialized)
        .map_err(|e| format!("Failed to write '{}': {}", addon.config_path.display(), e))?;

    Ok(())
}

fn upsert_wallpaper_profile_for_index(
    wallpapers_map: &mut Mapping,
    monitor_index: &str,
    wallpaper_id: &str,
) {
    for (_section_key, section_value) in wallpapers_map.iter_mut() {
        let Some(section_map) = section_value.as_mapping_mut() else {
            continue;
        };

        let current_indexes = section_map
            .get(Value::String("monitor_index".to_string()))
            .and_then(|v| match v {
                Value::Sequence(seq) => Some(
                    seq.iter()
                        .filter_map(|item| item.as_str().map(|s| s.to_string()))
                        .collect::<Vec<_>>(),
                ),
                Value::String(s) => Some(vec![s.clone()]),
                _ => None,
            })
            .unwrap_or_default();

        if current_indexes.len() == 1 && current_indexes[0] == monitor_index {
            section_map.insert(
                Value::String("wallpaper_id".to_string()),
                Value::String(wallpaper_id.to_string()),
            );
            section_map.insert(Value::String("enabled".to_string()), Value::Bool(true));
            if !section_map.contains_key(Value::String("mode".to_string())) {
                section_map.insert(
                    Value::String("mode".to_string()),
                    Value::String("fill".to_string()),
                );
            }
            if !section_map.contains_key(Value::String("z_index".to_string())) {
                section_map.insert(
                    Value::String("z_index".to_string()),
                    Value::String("desktop".to_string()),
                );
            }
            return;
        }
    }

    let mut max_suffix = 0u32;
    for section_key in wallpapers_map.keys() {
        let Some(section_name) = section_key.as_str() else {
            continue;
        };
        if let Some(suffix) = section_name.strip_prefix("wallpaper") {
            if let Ok(num) = suffix.parse::<u32>() {
                max_suffix = max_suffix.max(num + 1);
            }
        }
    }

    let new_key = format!("wallpaper{}", max_suffix);
    let mut new_section = Mapping::new();
    new_section.insert(Value::String("enabled".to_string()), Value::Bool(true));
    new_section.insert(
        Value::String("monitor_index".to_string()),
        Value::Sequence(vec![Value::String(monitor_index.to_string())]),
    );
    new_section.insert(
        Value::String("wallpaper_id".to_string()),
        Value::String(wallpaper_id.to_string()),
    );
    new_section.insert(
        Value::String("mode".to_string()),
        Value::String("fill".to_string()),
    );
    new_section.insert(
        Value::String("z_index".to_string()),
        Value::String("desktop".to_string()),
    );

    wallpapers_map.insert(Value::String(new_key), Value::Mapping(new_section));
}

fn yaml_string(root: &Value, dotted_path: &str) -> Option<String> {
    get_node(root, &split_path(dotted_path))
        .and_then(|v| v.as_str().map(|s| s.to_string()))
}

fn yaml_bool(root: &Value, dotted_path: &str) -> Option<bool> {
    get_node(root, &split_path(dotted_path)).and_then(|v| v.as_bool())
}

fn yaml_i64(root: &Value, dotted_path: &str) -> Option<i64> {
    get_node(root, &split_path(dotted_path)).and_then(|v| v.as_i64())
}

fn yaml_f64(root: &Value, dotted_path: &str) -> Option<f64> {
    get_node(root, &split_path(dotted_path)).and_then(|v| v.as_f64())
}

fn yaml_string_list(root: &Value, dotted_path: &str) -> Vec<String> {
    match get_node(root, &split_path(dotted_path)) {
        Some(Value::Sequence(seq)) => seq
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect(),
        Some(Value::String(s)) => vec![s.clone()],
        _ => Vec::new(),
    }
}

fn yaml_string_map(root: &Value, dotted_path: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let Some(Value::Mapping(map)) = get_node(root, &split_path(dotted_path)) else {
        return out;
    };

    for (k, v) in map {
        if let (Some(key), Some(value)) = (k.as_str(), v.as_str()) {
            out.insert(key.to_string(), value.to_string());
        }
    }

    out
}

// ── Config update helpers ──

fn dir_size(path: &Path) -> u64 {
    if !path.exists() { return 0; }
    walkdir::WalkDir::new(path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter_map(|e| e.metadata().ok())
        .filter(|m| m.is_file())
        .map(|m| m.len())
        .sum()
}

fn json_to_yaml(value: &serde_json::Value) -> Value {
    serde_yaml::to_value(value).unwrap_or(Value::Null)
}

fn set_yaml_value(root: &mut Value, path: &str, value: Value) {
    let parts: Vec<&str> = path.split('.').collect();
    let mut current = root;

    for (i, part) in parts.iter().enumerate() {
        if i == parts.len() - 1 {
            if let Value::Mapping(map) = current {
                map.insert(Value::String(part.to_string()), value);
                return;
            }
        } else {
            if !matches!(current, Value::Mapping(_)) {
                *current = Value::Mapping(Mapping::new());
            }
            let map = current.as_mapping_mut().unwrap();
            let key = Value::String(part.to_string());
            if !map.contains_key(&key) {
                map.insert(key.clone(), Value::Mapping(Mapping::new()));
            }
            current = map.get_mut(&key).unwrap();
        }
    }
}

fn apply_config_update(addon_id: &str, path: &str, value: &serde_json::Value) -> Result<(), String> {
    if path.is_empty() {
        return Err("Empty config path".to_string());
    }

    let addon = discover_addon_configs()
        .into_iter()
        .find(|a| a.id.eq_ignore_ascii_case(addon_id))
        .ok_or_else(|| format!("Addon '{}' not found", addon_id))?;

    let content = std::fs::read_to_string(&addon.config_path).unwrap_or_else(|_| "{}".to_string());
    let mut root = serde_yaml::from_str::<Value>(&content).unwrap_or_else(|_| Value::Mapping(Mapping::new()));

    set_yaml_value(&mut root, path, json_to_yaml(value));

    let serialized = serde_yaml::to_string(&root)
        .map_err(|e| format!("Failed to serialize YAML: {}", e))?;
    std::fs::write(&addon.config_path, serialized)
        .map_err(|e| format!("Failed to write config: {}", e))?;

    Ok(())
}

fn apply_wallpaper_property_update(
    addon_id: &str,
    monitor_indexes: &[String],
    property: &str,
    value: &serde_json::Value,
) -> Result<(), String> {
    if property.is_empty() || monitor_indexes.is_empty() {
        return Err("Missing property or monitor indexes".to_string());
    }

    let addon = discover_addon_configs()
        .into_iter()
        .find(|a| a.id.eq_ignore_ascii_case(addon_id))
        .ok_or_else(|| format!("Addon '{}' not found", addon_id))?;

    let content = std::fs::read_to_string(&addon.config_path).unwrap_or_else(|_| "{}".to_string());
    let mut root = serde_yaml::from_str::<Value>(&content).unwrap_or_else(|_| Value::Mapping(Mapping::new()));
    if !matches!(root, Value::Mapping(_)) {
        root = Value::Mapping(Mapping::new());
    }

    let root_map = root.as_mapping_mut().ok_or("Root is not a mapping")?;
    let wallpapers_value = root_map
        .entry(Value::String("wallpapers".to_string()))
        .or_insert_with(|| Value::Mapping(Mapping::new()));
    if !matches!(wallpapers_value, Value::Mapping(_)) {
        *wallpapers_value = Value::Mapping(Mapping::new());
    }
    let wallpapers_map = wallpapers_value.as_mapping_mut().ok_or("wallpapers not a mapping")?;

    let yaml_value = json_to_yaml(value);

    for (_section_key, section_value) in wallpapers_map.iter_mut() {
        let Some(section_map) = section_value.as_mapping_mut() else { continue };

        let current_indexes = section_map
            .get(Value::String("monitor_index".to_string()))
            .and_then(|v| match v {
                Value::Sequence(seq) => Some(
                    seq.iter()
                        .filter_map(|item| item.as_str().map(|s| s.to_string()))
                        .collect::<Vec<_>>(),
                ),
                Value::String(s) => Some(vec![s.clone()]),
                _ => None,
            })
            .unwrap_or_default();

        let matches = current_indexes.iter().any(|idx| monitor_indexes.contains(idx));
        if !matches { continue; }

        section_map.insert(Value::String(property.to_string()), yaml_value.clone());
    }

    let serialized = serde_yaml::to_string(&root)
        .map_err(|e| format!("Failed to serialize YAML: {}", e))?;
    std::fs::write(&addon.config_path, serialized)
        .map_err(|e| format!("Failed to write config: {}", e))?;

    Ok(())
}

fn clear_addon_cache(addon_id: &str) -> Result<(), String> {
    let addon = discover_addon_configs()
        .into_iter()
        .find(|a| a.id.eq_ignore_ascii_case(addon_id))
        .ok_or_else(|| format!("Addon '{}' not found", addon_id))?;

    let cache_dir = addon.addon_root.join("cache");
    if cache_dir.exists() {
        std::fs::remove_dir_all(&cache_dir)
            .map_err(|e| format!("Failed to remove cache: {}", e))?;
    }
    Ok(())
}

/// Save a single editable property back to the wallpaper's manifest.json.
/// Navigates the "editable" object, finding the key (even inside groups), and updates its "value".
fn save_editable_to_manifest(manifest_path_str: &str, key: &str, value: &serde_json::Value) -> Result<(), String> {
    if manifest_path_str.is_empty() || key.is_empty() {
        return Err("Missing manifest path or key".to_string());
    }
    let manifest_path = PathBuf::from(manifest_path_str);
    if !manifest_path.exists() {
        return Err(format!("Manifest not found: {}", manifest_path.display()));
    }

    let text = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("Read manifest: {}", e))?;
    let mut manifest: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| format!("Parse manifest: {}", e))?;

    let editable = manifest
        .get_mut("editable")
        .ok_or("No editable section in manifest")?;

    // Try top-level key first
    if let Some(entry) = editable.get_mut(key) {
        if entry.is_object() && entry.get("selector").is_some() {
            entry["value"] = value.clone();
            let serialized = serde_json::to_string_pretty(&manifest)
                .map_err(|e| format!("Serialize manifest: {}", e))?;
            std::fs::write(&manifest_path, serialized)
                .map_err(|e| format!("Write manifest: {}", e))?;
            return Ok(());
        }
    }

    // Search inside groups
    let editable_obj = editable.as_object_mut()
        .ok_or("editable is not an object")?;
    for (_group_key, group_val) in editable_obj.iter_mut() {
        if let Some(obj) = group_val.as_object_mut() {
            if let Some(entry) = obj.get_mut(key) {
                if entry.is_object() && entry.get("selector").is_some() {
                    entry["value"] = value.clone();
                    let serialized = serde_json::to_string_pretty(&manifest)
                        .map_err(|e| format!("Serialize manifest: {}", e))?;
                    std::fs::write(&manifest_path, serialized)
                        .map_err(|e| format!("Write manifest: {}", e))?;
                    return Ok(());
                }
            }
        }
    }

    Err(format!("Key '{}' not found in editable section", key))
}

/// Capture a screenshot of the wallpaper for the preview image.
/// Uses the wallpaper's index.html and captures via a headless approach.
/// Since WebView screenshot APIs are limited, we copy the existing first preview
/// or create a placeholder. In a full implementation, this would use a headless
/// browser or wry's screenshot capability.
fn capture_wallpaper_preview(manifest_path_str: &str) -> Result<(), String> {
    if manifest_path_str.is_empty() {
        return Err("Missing manifest path".to_string());
    }
    let manifest_path = PathBuf::from(manifest_path_str);
    let manifest_dir = manifest_path.parent()
        .ok_or("Cannot determine manifest directory")?;

    let preview_dir = manifest_dir.join("preview");
    if !preview_dir.exists() {
        std::fs::create_dir_all(&preview_dir)
            .map_err(|e| format!("Failed to create preview dir: {}", e))?;
    }

    // Use wry's print_to_pdf-like approach or an offscreen capture.
    // For now we mark that a new preview is needed by writing a sentinel timestamp
    // so the wallpaper addon's watcher knows to regenerate.
    let marker_path = preview_dir.join(".preview_capture_pending");
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    std::fs::write(&marker_path, format!("{}", timestamp))
        .map_err(|e| format!("Failed to write capture marker: {}", e))?;

    warn!("[ui] Preview capture requested for {} — marker written", manifest_path_str);
    Ok(())
}

fn build_sentinel_custom_tabs_shell_html(
        addons: &[CustomTabShellAddon],
        selected_addon_id: &str,
) -> Result<String, Box<dyn std::error::Error>> {
        let addons_json = serde_json::to_string(addons)?;
        let selected_json = serde_json::to_string(selected_addon_id)?;

        Ok(format!(
                r#"<!doctype html>
<html lang="en">
<head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>Sentinel</title>
    <style>
        :root {{
            --bg-base: #0a0a0f;
            --bg-surface: #111118;
            --bg-elevated: #1a1a24;
            --bg-hover: #22222e;
            --bg-active: #2a2a38;
            --border-subtle: rgba(255,255,255,0.06);
            --border-default: rgba(255,255,255,0.1);
            --border-strong: rgba(255,255,255,0.15);
            --text-primary: #e8e8ed;
            --text-secondary: #8b8b9e;
            --text-tertiary: #5c5c72;
            --accent: #dc2626;
            --accent-hover: #ef4444;
            --accent-subtle: rgba(220,38,38,0.15);
            --accent-border: rgba(220,38,38,0.3);
            --radius-sm: 6px;
            --radius-md: 10px;
            --radius-lg: 14px;
            --shadow-md: 0 4px 12px rgba(0,0,0,0.4);
            --transition-fast: 150ms cubic-bezier(0.4,0,0.2,1);
            --sidebar-width: 180px;
        }}
        *, *::before, *::after {{ margin: 0; padding: 0; box-sizing: border-box; }}
        html, body {{ height: 100%; overflow: hidden; }}
        body {{
            font-family: "Segoe UI", Inter, -apple-system, sans-serif;
            background: var(--bg-base);
            color: var(--text-primary);
            display: flex;
            line-height: 1.5;
            -webkit-font-smoothing: antialiased;
        }}
        ::-webkit-scrollbar {{ width: 6px; }}
        ::-webkit-scrollbar-track {{ background: transparent; }}
        ::-webkit-scrollbar-thumb {{ background: var(--border-default); border-radius: 3px; }}
        ::-webkit-scrollbar-thumb:hover {{ background: var(--border-strong); }}

        @keyframes fadeInUp {{
            from {{ opacity: 0; transform: translateY(8px); }}
            to {{ opacity: 1; transform: translateY(0); }}
        }}

        /* ===== Left Sidebar ===== */
        #left-vertical-bar {{
            width: var(--sidebar-width);
            min-width: var(--sidebar-width);
            height: 100vh;
            background: var(--bg-surface);
            border-right: 1px solid var(--border-subtle);
            display: flex;
            flex-direction: column;
            padding: 16px 0;
            gap: 8px;
            z-index: 10;
        }}
        #logo {{
            width: 100%;
            height: 40px;
            display: flex;
            align-items: center;
            justify-content: center;
            color: var(--accent);
            margin-bottom: 4px;
            position: relative;
        }}
        #logo::after {{
            content: "";
            position: absolute;
            bottom: -4px;
            left: 16px;
            right: 16px;
            height: 1px;
            background: var(--border-subtle);
        }}
        #nav-label {{
            font-size: 10px;
            font-weight: 600;
            text-transform: uppercase;
            letter-spacing: 0.08em;
            color: var(--text-tertiary);
            padding: 8px 16px 4px;
        }}
        #nav-menu {{
            display: flex;
            flex-direction: column;
            padding: 0 8px;
            gap: 2px;
        }}
        #nav-spacer {{ flex: 1; }}
        .nav-item {{
            width: 100%;
            height: 36px;
            display: flex;
            align-items: center;
            gap: 10px;
            padding: 0 12px;
            border: none;
            border-radius: var(--radius-md);
            background: none;
            color: var(--text-tertiary);
            cursor: pointer;
            transition: all var(--transition-fast);
            position: relative;
            font-family: inherit;
            font-size: 13px;
            font-weight: 500;
            text-align: left;
        }}
        .nav-item:hover {{
            background: var(--bg-hover);
            color: var(--text-secondary);
        }}
        .nav-item.active {{
            background: var(--accent-subtle);
            color: var(--accent);
        }}
        .nav-item.active::before {{
            content: "";
            position: absolute;
            left: -8px;
            width: 3px;
            height: 20px;
            background: var(--accent);
            border-radius: 0 3px 3px 0;
            box-shadow: 0 0 12px rgba(220,38,38,0.4), 0 0 4px rgba(220,38,38,0.6);
        }}
        .nav-item svg {{ flex-shrink: 0; }}
        .nav-item-label {{
            white-space: nowrap;
            overflow: hidden;
            text-overflow: ellipsis;
        }}
        #nav-bottom {{
            padding: 0 8px 4px;
        }}
        .nav-quick-actions {{
            display: flex;
            gap: 6px;
            padding: 10px 0;
            border-top: 1px solid var(--border-subtle);
        }}
        .quick-action-btn {{
            flex: 1;
            height: 36px;
            display: flex;
            align-items: center;
            justify-content: center;
            border: 1px solid var(--border-subtle);
            border-radius: var(--radius-md);
            background: var(--bg-elevated);
            color: var(--text-tertiary);
            cursor: pointer;
            transition: all var(--transition-fast);
            position: relative;
        }}
        .quick-action-btn:hover {{
            background: var(--bg-hover);
            border-color: var(--border-default);
            color: var(--text-primary);
        }}
        .quick-action-btn:active {{ background: var(--bg-active); transform: scale(0.96); }}
        .quick-action-btn.active {{ background: var(--accent); color: #fff; border-color: var(--accent); }}
        .quick-action-btn[data-tooltip]:hover::after {{
            content: attr(data-tooltip);
            position: absolute;
            bottom: calc(100% + 8px);
            left: 50%;
            transform: translateX(-50%);
            background: var(--bg-elevated);
            color: var(--text-primary);
            padding: 3px 8px;
            border-radius: var(--radius-sm);
            font-size: 11px;
            font-weight: 500;
            white-space: nowrap;
            box-shadow: var(--shadow-md);
            border: 1px solid var(--border-default);
            z-index: 100;
            pointer-events: none;
        }}

        /* ===== Right Panel ===== */
        #right-addon-panel {{
            flex: 1;
            height: 100vh;
            display: flex;
            flex-direction: column;
            overflow: hidden;
            background: var(--bg-base);
        }}
        #right-page-panel {{
            flex: 1;
            height: 100vh;
            display: none;
            flex-direction: column;
            overflow: hidden;
            background: var(--bg-base);
        }}
        .page-header {{
            padding: 20px 28px 14px;
            border-bottom: 1px solid var(--border-subtle);
            background: var(--bg-surface);
            flex-shrink: 0;
        }}
        .page-header h2 {{
            font-size: 18px;
            font-weight: 600;
        }}
        .page-content {{
            flex: 1;
            min-height: 0;
            overflow-y: auto;
            padding: 24px 28px;
        }}
        .addon-cards-grid {{
            display: grid;
            grid-template-columns: repeat(auto-fill, minmax(260px, 1fr));
            gap: 16px;
        }}
        .addon-card {{
            background: var(--bg-surface);
            border: 1px solid var(--border-subtle);
            border-radius: var(--radius-lg);
            padding: 20px;
            cursor: pointer;
            transition: all var(--transition-fast);
            display: flex;
            align-items: center;
            gap: 14px;
        }}
        .addon-card:hover {{
            border-color: var(--accent-border);
            transform: translateY(-2px);
            box-shadow: var(--shadow-md);
        }}
        .addon-card-icon {{
            width: 44px;
            height: 44px;
            border-radius: var(--radius-md);
            background: var(--accent-subtle);
            display: flex;
            align-items: center;
            justify-content: center;
            color: var(--accent);
            flex-shrink: 0;
        }}
        .addon-card-info h3 {{
            font-size: 14px;
            font-weight: 600;
            margin-bottom: 2px;
        }}
        .addon-card-info span {{
            font-size: 12px;
            color: var(--text-tertiary);
        }}
        .page-settings-group {{
            background: var(--bg-surface);
            border: 1px solid var(--border-subtle);
            border-radius: var(--radius-lg);
            padding: 20px;
            margin-bottom: 16px;
        }}
        .page-settings-group h3 {{
            font-size: 14px;
            font-weight: 600;
            margin-bottom: 12px;
            color: var(--text-secondary);
        }}
        .setting-row {{
            display: flex;
            align-items: center;
            justify-content: space-between;
            padding: 10px 0;
            border-bottom: 1px solid var(--border-subtle);
        }}
        .setting-row:last-child {{ border-bottom: none; }}
        .s-label {{
            font-size: 13px;
            color: var(--text-secondary);
            font-weight: 500;
        }}
        .s-input {{
            background: var(--bg-elevated);
            border: 1px solid var(--border-subtle);
            border-radius: var(--radius-sm);
            padding: 6px 10px;
            color: var(--text-primary);
            font-size: 13px;
            font-family: inherit;
            min-width: 100px;
        }}
        .s-input:focus {{ outline: none; border-color: var(--accent); }}
        .s-toggle {{
            position: relative;
            width: 36px;
            height: 20px;
            display: inline-block;
        }}
        .s-toggle input {{ opacity: 0; width: 0; height: 0; }}
        .s-slider {{
            position: absolute;
            inset: 0;
            background: var(--bg-hover);
            border-radius: 10px;
            cursor: pointer;
            transition: background var(--transition-fast);
        }}
        .s-slider::before {{
            content: '';
            position: absolute;
            top: 2px;
            left: 2px;
            width: 16px;
            height: 16px;
            background: var(--text-secondary);
            border-radius: 50%;
            transition: transform var(--transition-fast), background var(--transition-fast);
        }}
        .s-toggle input:checked + .s-slider {{ background: var(--accent); }}
        .s-toggle input:checked + .s-slider::before {{ transform: translateX(16px); background: #fff; }}
        .data-json-wrap {{
            background: var(--bg-surface);
            border: 1px solid var(--border-subtle);
            border-radius: var(--radius-lg);
            padding: 16px;
            overflow: auto;
            max-height: calc(100vh - 130px);
        }}
        .data-json-wrap pre {{
            font-family: "JetBrains Mono", "Cascadia Code", "Consolas", monospace;
            font-size: 12px;
            line-height: 1.6;
            color: var(--text-secondary);
            white-space: pre-wrap;
            word-break: break-all;
        }}
        .data-filter {{
            display: flex;
            gap: 8px;
            margin-bottom: 16px;
            flex-wrap: wrap;
        }}
        .data-filter-chip {{
            padding: 6px 14px;
            border-radius: var(--radius-sm);
            border: 1px solid var(--border-subtle);
            background: var(--bg-elevated);
            color: var(--text-secondary);
            font-size: 12px;
            font-weight: 500;
            cursor: pointer;
            transition: all var(--transition-fast);
            font-family: inherit;
        }}
        .data-filter-chip:hover {{ background: var(--bg-hover); }}
        .data-filter-chip.active {{
            background: var(--accent-subtle);
            color: var(--accent);
            border-color: var(--accent-border);
        }}
        /* ── Data panel cards ─────────────────────── */
        .data-panels-grid {{
            columns: 3 300px;
            column-gap: 16px;
        }}
        .data-panel {{
            background: var(--bg-surface);
            border: 1px solid var(--border-subtle);
            border-radius: var(--radius-lg);
            overflow: hidden;
            break-inside: avoid;
            margin-bottom: 16px;
            transition: border-color var(--transition-fast);
        }}
        .data-panel:hover {{
            border-color: var(--accent-border);
        }}
        .data-panel-header {{
            display: flex;
            align-items: center;
            gap: 10px;
            padding: 14px 16px 10px;
            border-bottom: 1px solid var(--border-subtle);
        }}
        .data-panel-icon {{
            width: 28px;
            height: 28px;
            display: flex;
            align-items: center;
            justify-content: center;
            border-radius: var(--radius-sm);
            background: var(--accent-subtle);
            color: var(--accent);
            flex-shrink: 0;
        }}
        .data-panel-icon svg {{ width: 16px; height: 16px; }}
        .data-panel-title {{
            font-size: 13px;
            font-weight: 600;
            color: var(--text-primary);
        }}
        .data-panel-subtitle {{
            font-size: 11px;
            color: var(--text-dim);
            font-weight: 400;
        }}
        .data-panel-body {{
            padding: 12px 16px 14px;
            display: flex;
            flex-direction: column;
            gap: 8px;
        }}
        .data-row {{
            display: flex;
            justify-content: space-between;
            align-items: center;
            font-size: 12px;
        }}
        .data-row-label {{
            color: var(--text-secondary);
        }}
        .data-row-value {{
            color: var(--text-primary);
            font-family: "JetBrains Mono", "Cascadia Code", "Consolas", monospace;
            font-size: 11px;
            font-weight: 500;
            text-align: right;
            max-width: 60%;
            overflow: hidden;
            text-overflow: ellipsis;
            white-space: nowrap;
        }}
        .data-bar-wrap {{
            width: 100%;
            height: 6px;
            background: var(--bg-hover);
            border-radius: 3px;
            overflow: hidden;
            margin-top: 2px;
        }}
        .data-bar-fill {{
            height: 100%;
            border-radius: 3px;
            background: var(--accent);
            transition: width 0.3s ease;
        }}
        .data-bar-fill.warn {{ background: #f59e0b; }}
        .data-bar-fill.danger {{ background: #ef4444; }}
        .data-big-value {{
            font-size: 24px;
            font-weight: 700;
            color: var(--text-primary);
            font-family: "JetBrains Mono", "Cascadia Code", "Consolas", monospace;
            line-height: 1.2;
        }}
        .data-big-unit {{
            font-size: 12px;
            font-weight: 400;
            color: var(--text-dim);
            margin-left: 4px;
        }}
        .data-tag {{
            display: inline-block;
            padding: 2px 8px;
            border-radius: 10px;
            font-size: 10px;
            font-weight: 600;
            text-transform: uppercase;
            letter-spacing: 0.5px;
        }}
        .data-tag.online {{ background: rgba(34,197,94,0.15); color: #22c55e; }}
        .data-tag.offline {{ background: rgba(239,68,68,0.15); color: #ef4444; }}
        .data-tag.charging {{ background: rgba(59,130,246,0.15); color: #3b82f6; }}
        .data-panel-footer {{
            padding: 0 16px 12px;
            font-size: 11px;
            color: var(--text-dim);
        }}
        .data-connection-dot {{
            width: 8px;
            height: 8px;
            border-radius: 50%;
            display: inline-block;
            margin-right: 6px;
        }}
        .data-connection-dot.live {{ background: #22c55e; box-shadow: 0 0 6px rgba(34,197,94,0.4); }}
        .data-connection-dot.stale {{ background: #f59e0b; }}
        .data-connection-dot.dead {{ background: #ef4444; }}
        .data-stat-row {{
            display: flex;
            gap: 16px;
        }}
        .data-stat-item {{
            flex: 1;
        }}
        .data-stat-label {{
            font-size: 10px;
            text-transform: uppercase;
            letter-spacing: 0.5px;
            color: var(--text-dim);
            margin-bottom: 2px;
        }}
        .data-stat-value {{
            font-size: 14px;
            font-weight: 600;
            color: var(--text-primary);
            font-family: "JetBrains Mono", "Cascadia Code", "Consolas", monospace;
        }}
        .data-drives-list {{
            display: flex;
            flex-direction: column;
            gap: 10px;
        }}
        .data-drive-label {{
            display: flex;
            justify-content: space-between;
            font-size: 11px;
            margin-bottom: 2px;
        }}
        .data-drive-label span:first-child {{ color: var(--text-secondary); }}
        .data-drive-label span:last-child {{ color: var(--text-dim); font-family: "JetBrains Mono", "Cascadia Code", "Consolas", monospace; }}
        .data-appdata-section {{
            margin-top: 8px;
        }}
        .data-appdata-monitor {{
            margin-bottom: 12px;
        }}
        .data-appdata-monitor-title {{
            font-size: 12px;
            font-weight: 600;
            color: var(--text-secondary);
            margin-bottom: 6px;
        }}
        .data-window-item {{
            display: flex;
            align-items: center;
            gap: 8px;
            padding: 6px 0;
            border-bottom: 1px solid var(--border-subtle);
            font-size: 12px;
        }}
        .data-window-item:last-child {{ border-bottom: none; }}
        .data-window-app {{
            font-weight: 500;
            color: var(--text-primary);
            white-space: nowrap;
            overflow: hidden;
            text-overflow: ellipsis;
            max-width: 120px;
        }}
        .data-window-title {{
            flex: 1;
            color: var(--text-dim);
            overflow: hidden;
            text-overflow: ellipsis;
            white-space: nowrap;
        }}
        .data-window-badge {{
            padding: 1px 6px;
            border-radius: 8px;
            font-size: 10px;
            font-weight: 600;
            background: var(--accent-subtle);
            color: var(--accent);
            white-space: nowrap;
        }}
        .tab-bar {{
            display: flex;
            gap: 0;
            padding: 0 24px;
            background: var(--bg-surface);
            border-bottom: 1px solid var(--border-subtle);
            flex-shrink: 0;
        }}
        .tab {{
            padding: 14px 20px;
            font-size: 13px;
            font-weight: 500;
            color: var(--text-tertiary);
            cursor: pointer;
            transition: all var(--transition-fast);
            border: none;
            border-bottom: 2px solid transparent;
            background: none;
            font-family: inherit;
        }}
        .tab:hover {{
            color: var(--text-secondary);
            background: var(--bg-hover);
        }}
        .tab.active {{
            color: var(--accent);
            border-bottom-color: var(--accent);
        }}
        .frame-wrap {{
            flex: 1;
            min-height: 0;
        }}
        #tabFrame {{
            width: 100%;
            height: 100%;
            border: 0;
            background: var(--bg-base);
        }}
    </style>
</head>
<body>
    <div id="left-vertical-bar">
        <div id="logo">
            <svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/>
            </svg>
        </div>
        <div id="nav-label">Addons</div>
        <div id="nav-menu"></div>
        <div id="nav-spacer"></div>
        <div id="nav-bottom">
            <div class="nav-quick-actions">
                <button class="quick-action-btn" data-tooltip="Home">
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                        <path d="M3 9l9-7 9 7v11a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>
                        <polyline points="9 22 9 12 15 12 15 22"/>
                    </svg>
                </button>
                <button class="quick-action-btn" data-tooltip="Settings">
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                        <circle cx="12" cy="12" r="3"/>
                        <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/>
                    </svg>
                </button>
                <button class="quick-action-btn" data-tooltip="Data">
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                        <ellipse cx="12" cy="5" rx="9" ry="3"/>
                        <path d="M21 12c0 1.66-4 3-9 3s-9-1.34-9-3"/>
                        <path d="M3 5v14c0 1.66 4 3 9 3s9-1.34 9-3V5"/>
                    </svg>
                </button>
            </div>
        </div>
    </div>

    <div id="right-addon-panel">
        <div class="tab-bar" id="tabs"></div>
        <div class="frame-wrap"><iframe id="tabFrame" title="Addon Tab"></iframe></div>
    </div>

    <div id="right-page-panel">
        <div class="page-header" id="page-header"></div>
        <div class="page-content" id="page-content"></div>
    </div>

    <script>
        const ADDONS = {addons_json};
        let currentAddonId = {selected_json};
        let currentTabId = null;

        const ADDON_ICONS = {{
            'wallpaper': '<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="18" height="18" rx="2" ry="2"/><circle cx="8.5" cy="8.5" r="1.5"/><polyline points="21 15 16 10 5 21"/></svg>',
            'statusbar': '<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="18" height="18" rx="2" ry="2"/><line x1="3" y1="9" x2="21" y2="9"/></svg>',
            'window': '<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="3" width="20" height="14" rx="2" ry="2"/><line x1="8" y1="21" x2="16" y2="21"/><line x1="12" y1="17" x2="12" y2="21"/></svg>',
        }};

        const DEFAULT_ICON = '<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 2L2 7l10 5 10-5-10-5z"/><path d="M2 17l10 5 10-5"/><path d="M2 12l10 5 10-5"/></svg>';

        function getAddonIcon(addonId) {{
            const lower = addonId.toLowerCase();
            for (const [key, svg] of Object.entries(ADDON_ICONS)) {{
                if (lower.includes(key)) return svg;
            }}
            return DEFAULT_ICON;
        }}

        window.__sentinelBridgePost = (payload) => {{
            if (!payload) return false;
            var msg = (typeof payload === 'string') ? payload : JSON.stringify(payload);
            try {{
                if (window.ipc && typeof window.ipc.postMessage === 'function') {{
                    window.ipc.postMessage(msg);
                    return true;
                }}
            }} catch (_) {{}}
            try {{
                if (window.chrome && window.chrome.webview && typeof window.chrome.webview.postMessage === 'function') {{
                    window.chrome.webview.postMessage(msg);
                    return true;
                }}
            }} catch (_) {{}}
            return false;
        }};

        window.addEventListener('message', (event) => {{
            let data = event && event.data;
            if (!data) return;

            if (typeof data === 'string') {{
                try {{ data = JSON.parse(data); }} catch (_) {{ return; }}
            }}

            if (data && data.sentinelBridge && data.payload) {{
                window.__sentinelBridgePost(data.payload);
                return;
            }}

            if (data && data.type) {{
                window.__sentinelBridgePost(data);
            }}
        }});

        function getAddon() {{
            return ADDONS.find(a => a.id === currentAddonId) || ADDONS[0];
        }}

        let viewMode = 'addon';

        function renderAddons() {{
            const host = document.getElementById('nav-menu');
            host.innerHTML = '';
            ADDONS.forEach(addon => {{
                const btn = document.createElement('button');
                btn.className = 'nav-item' + (viewMode === 'addon' && addon.id === currentAddonId ? ' active' : '');
                btn.innerHTML = getAddonIcon(addon.id) + '<span class="nav-item-label">' + addon.name + '</span>';
                btn.onclick = () => {{
                    viewMode = 'addon';
                    currentAddonId = addon.id;
                    currentTabId = null;
                    render();
                }};
                host.appendChild(btn);
            }});
            document.querySelectorAll('.quick-action-btn').forEach(btn => {{
                const tip = (btn.getAttribute('data-tooltip') || '').toLowerCase();
                btn.classList.toggle('active', viewMode === tip);
            }});
        }}

        function renderTabs() {{
            const addon = getAddon();
            const tabsHost = document.getElementById('tabs');
            const frame = document.getElementById('tabFrame');
            tabsHost.innerHTML = '';

            if (!addon || !addon.tabs || addon.tabs.length === 0) {{
                frame.src = 'about:blank';
                return;
            }}

            if (!currentTabId || !addon.tabs.some(t => t.id === currentTabId)) {{
                currentTabId = addon.tabs[0].id;
            }}

            addon.tabs.forEach(tab => {{
                const btn = document.createElement('button');
                btn.className = 'tab' + (tab.id === currentTabId ? ' active' : '');
                btn.textContent = tab.title;
                btn.onclick = () => {{
                    currentTabId = tab.id;
                    renderTabs();
                }};
                tabsHost.appendChild(btn);
            }});

            const current = addon.tabs.find(t => t.id === currentTabId) || addon.tabs[0];
            frame.src = current.url;
        }}

        function renderHomePage() {{
            const header = document.getElementById('page-header');
            const content = document.getElementById('page-content');
            header.innerHTML = '<h2>Dashboard</h2><p style="color:var(--text-dim);margin:4px 0 0;">Installed addons overview</p>';
            content.innerHTML = '<div class="addon-cards-grid">' + ADDONS.map(addon =>
                '<div class="addon-card" data-aid="' + addon.id + '">' +
                    '<div class="addon-card-icon">' + getAddonIcon(addon.id) + '</div>' +
                    '<div class="addon-card-info">' +
                        '<h3>' + addon.name + '</h3>' +
                        '<span>' + addon.id + '</span>' +
                        '<span>' + addon.tabs.length + ' tab' + (addon.tabs.length !== 1 ? 's' : '') + '</span>' +
                    '</div>' +
                '</div>'
            ).join('') + '</div>';
            content.querySelectorAll('.addon-card').forEach(card => {{
                card.onclick = () => {{
                    viewMode = 'addon';
                    currentAddonId = card.dataset.aid;
                    currentTabId = null;
                    render();
                }};
            }});
        }}

        function renderSettingsPage() {{
            const header = document.getElementById('page-header');
            const content = document.getElementById('page-content');
            header.innerHTML = '<h2>Settings</h2><p style="color:var(--text-dim);margin:4px 0 0;">Backend configuration</p>';
            content.innerHTML =
                '<div class="page-settings-group">' +
                    '<h3>Data Collection — Fast Tier</h3>' +
                    '<p style="color:var(--text-dim);font-size:12px;margin:2px 0 8px;">Lightweight data: audio, time, keyboard, mouse, idle, power, display</p>' +
                    '<div class="setting-row"><span class="s-label">Fast Pull Rate (ms)</span>' +
                        '<input type="number" id="cfg-fast-rate" class="s-input" value="50" min="10" max="5000" step="10">' +
                    '</div>' +
                '</div>' +
                '<div class="page-settings-group">' +
                    '<h3>Data Collection — Slow Tier</h3>' +
                    '<p style="color:var(--text-dim);font-size:12px;margin:2px 0 8px;">Heavyweight data: CPU, GPU, RAM, storage, network, bluetooth, wifi, system, processes</p>' +
                    '<div class="setting-row"><span class="s-label">Slow Pull Rate (ms)</span>' +
                        '<input type="number" id="cfg-slow-rate" class="s-input" value="500" min="100" max="10000" step="100">' +
                    '</div>' +
                '</div>' +
                '<div class="page-settings-group">' +
                    '<h3>Streaming</h3>' +
                    '<div class="setting-row"><span class="s-label">Refresh on Request</span>' +
                        '<label class="s-toggle"><input type="checkbox" id="cfg-refresh-on-req" checked><span class="s-slider"></span></label>' +
                    '</div>' +
                    '<p style="color:var(--text-dim);font-size:12px;margin:2px 0 8px;">When enabled, fast-tier data is refreshed inline on every IPC request for lowest latency</p>' +
                '</div>' +
                '<div class="page-settings-group">' +
                    '<h3>Pause</h3>' +
                    '<div class="setting-row"><span class="s-label">Pause All Collection</span>' +
                        '<label class="s-toggle"><input type="checkbox" id="cfg-pull-paused"><span class="s-slider"></span></label>' +
                    '</div>' +
                '</div>' +
                '<div class="page-settings-group">' +
                    '<h3>Interface</h3>' +
                    '<div class="setting-row"><span class="s-label">Theme</span>' +
                        '<select id="cfg-theme" class="s-input"><option value="dark" selected>Dark</option><option value="light">Light</option></select>' +
                    '</div>' +
                '</div>';
            var fastEl = document.getElementById('cfg-fast-rate');
            var slowEl = document.getElementById('cfg-slow-rate');
            var rorEl = document.getElementById('cfg-refresh-on-req');
            var pauseEl = document.getElementById('cfg-pull-paused');
            var fastTimer = null;
            var slowTimer = null;
            if (fastEl) fastEl.addEventListener('input', function() {{
                clearTimeout(fastTimer);
                var v = Number(fastEl.value);
                fastTimer = setTimeout(function() {{
                    window.__sentinelBridgePost({{ type: 'backend_setting', key: 'fast_pull_rate', value: v }});
                }}, 400);
            }});
            if (slowEl) slowEl.addEventListener('input', function() {{
                clearTimeout(slowTimer);
                var v = Number(slowEl.value);
                slowTimer = setTimeout(function() {{
                    window.__sentinelBridgePost({{ type: 'backend_setting', key: 'slow_pull_rate', value: v }});
                }}, 400);
            }});
            if (rorEl) rorEl.addEventListener('change', function() {{
                window.__sentinelBridgePost({{ type: 'backend_setting', key: 'refresh_on_request', value: rorEl.checked }});
            }});
            if (pauseEl) pauseEl.addEventListener('change', function() {{
                window.__sentinelBridgePost({{ type: 'backend_setting', key: 'pull_paused', value: pauseEl.checked }});
            }});
        }}

        function renderDataPage() {{
            const header = document.getElementById('page-header');
            const content = document.getElementById('page-content');
            header.innerHTML = '<h2>Data</h2><p style="color:var(--text-dim);margin:4px 0 0;"><span class="data-connection-dot live"></span>Live registry — updates every 500ms</p>';
            var chips = ['All','Hardware','Network','Input','System','App','JSON'];
            window.__dataActiveChip = window.__dataActiveChip || 'All';
            content.innerHTML =
                '<div class="data-filter">' +
                    chips.map(function(c) {{ return '<button class="data-filter-chip' + (c === window.__dataActiveChip ? ' active' : '') + '">' + c + '</button>'; }}).join('') +
                '</div>' +
                '<div id="data-panels-container" class="data-panels-grid"></div>' +
                '<div id="data-json-fallback" class="data-json-wrap" style="display:none;"><pre id="data-json-pre">Loading\u2026</pre></div>';
            content.querySelectorAll('.data-filter-chip').forEach(function(chip) {{
                chip.onclick = function() {{
                    window.__dataActiveChip = chip.textContent;
                    content.querySelectorAll('.data-filter-chip').forEach(function(c) {{ c.classList.toggle('active', c.textContent === window.__dataActiveChip); }});
                    renderDataPanels(window.__lastRegistryData);
                }};
            }});
            // Render immediately if we already have data
            if (window.__lastRegistryData) {{
                renderDataPanels(window.__lastRegistryData);
            }} else {{
                document.getElementById('data-panels-container').innerHTML = '<div style="color:var(--text-dim);padding:20px;">Waiting for registry data\u2026</div>';
            }}
        }}

        // ── Panel icon SVGs ──
        var PANEL_ICONS = {{
            cpu: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="4" y="4" width="16" height="16" rx="2"/><line x1="9" y1="1" x2="9" y2="4"/><line x1="15" y1="1" x2="15" y2="4"/><line x1="9" y1="20" x2="9" y2="23"/><line x1="15" y1="20" x2="15" y2="23"/><line x1="20" y1="9" x2="23" y2="9"/><line x1="20" y1="15" x2="23" y2="15"/><line x1="1" y1="9" x2="4" y2="9"/><line x1="1" y1="15" x2="4" y2="15"/></svg>',
            gpu: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="2" y="6" width="20" height="12" rx="2"/><circle cx="12" cy="12" r="3"/><line x1="6" y1="6" x2="6" y2="2"/><line x1="18" y1="6" x2="18" y2="2"/></svg>',
            ram: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="2" y="7" width="20" height="10" rx="1"/><line x1="6" y1="17" x2="6" y2="21"/><line x1="10" y1="17" x2="10" y2="21"/><line x1="14" y1="17" x2="14" y2="21"/><line x1="18" y1="17" x2="18" y2="21"/><rect x="5" y="9" width="2" height="4" rx="0.5"/><rect x="9" y="9" width="2" height="4" rx="0.5"/><rect x="13" y="9" width="2" height="4" rx="0.5"/><rect x="17" y="9" width="2" height="4" rx="0.5"/></svg>',
            storage: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><ellipse cx="12" cy="5" rx="9" ry="3"/><path d="M21 12c0 1.66-4 3-9 3s-9-1.34-9-3"/><path d="M3 5v14c0 1.66 4 3 9 3s9-1.34 9-3V5"/></svg>',
            network: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="10"/><line x1="2" y1="12" x2="22" y2="12"/><path d="M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z"/></svg>',
            audio: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polygon points="11 5 6 9 2 9 2 15 6 15 11 19 11 5"/><path d="M15.54 8.46a5 5 0 0 1 0 7.07"/><path d="M19.07 4.93a10 10 0 0 1 0 14.14"/></svg>',
            time: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="10"/><polyline points="12 6 12 12 16 14"/></svg>',
            keyboard: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="2" y="6" width="20" height="12" rx="2"/><line x1="6" y1="10" x2="6" y2="10"/><line x1="10" y1="10" x2="10" y2="10"/><line x1="14" y1="10" x2="14" y2="10"/><line x1="18" y1="10" x2="18" y2="10"/><line x1="8" y1="14" x2="16" y2="14"/></svg>',
            mouse: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="6" y="3" width="12" height="18" rx="6"/><line x1="12" y1="7" x2="12" y2="11"/></svg>',
            power: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="1" y="6" width="18" height="12" rx="2"/><line x1="23" y1="10" x2="23" y2="14"/><line x1="19" y1="10" x2="19" y2="14"/></svg>',
            bluetooth: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="6.5 6.5 17.5 17.5 12 23 12 1 17.5 6.5 6.5 17.5"/></svg>',
            wifi: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M5 12.55a11 11 0 0 1 14.08 0"/><path d="M1.42 9a16 16 0 0 1 21.16 0"/><path d="M8.53 16.11a6 6 0 0 1 6.95 0"/><line x1="12" y1="20" x2="12.01" y2="20"/></svg>',
            system: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="2" y="3" width="20" height="14" rx="2"/><line x1="8" y1="21" x2="16" y2="21"/><line x1="12" y1="17" x2="12" y2="21"/></svg>',
            displays: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="2" y="3" width="20" height="14" rx="2"/><line x1="8" y1="21" x2="16" y2="21"/><line x1="12" y1="17" x2="12" y2="21"/></svg>',
            processes: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="22 12 18 12 15 21 9 3 6 12 2 12"/></svg>',
            idle: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z"/></svg>',
            appdata: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="3" y="3" width="18" height="18" rx="2"/><line x1="3" y1="9" x2="21" y2="9"/><line x1="9" y1="21" x2="9" y2="9"/></svg>'
        }};

        var FILTER_MAP = {{
            'All': null,
            'Hardware': ['cpu','gpu','ram','storage','displays'],
            'Network': ['network','wifi','bluetooth'],
            'Input': ['keyboard','mouse','audio'],
            'System': ['time','power','idle','system','processes'],
            'App': ['appdata'],
            'JSON': ['__json__']
        }};

        function fmtBytes(b) {{
            if (!b && b !== 0) return '—';
            if (b >= 1073741824) return (b / 1073741824).toFixed(1) + ' GB';
            if (b >= 1048576) return (b / 1048576).toFixed(1) + ' MB';
            if (b >= 1024) return (b / 1024).toFixed(1) + ' KB';
            return b + ' B';
        }}

        function pctBar(pct, label) {{
            var cls = pct > 90 ? 'danger' : pct > 70 ? 'warn' : '';
            return '<div class="data-row"><span class="data-row-label">' + (label||'') + '</span><span class="data-row-value">' + pct.toFixed(1) + '%</span></div>' +
                   '<div class="data-bar-wrap"><div class="data-bar-fill ' + cls + '" style="width:' + Math.min(pct,100) + '%"></div></div>';
        }}

        function dataRow(label, value) {{
            return '<div class="data-row"><span class="data-row-label">' + label + '</span><span class="data-row-value">' + (value != null ? value : '\u2014') + '</span></div>';
        }}

        function panelCard(key, title, subtitle, bodyHtml) {{
            var icon = PANEL_ICONS[key] || PANEL_ICONS.system;
            return '<div class="data-panel" data-panel-key="' + key + '">' +
                '<div class="data-panel-header">' +
                    '<div class="data-panel-icon">' + icon + '</div>' +
                    '<div><div class="data-panel-title">' + title + '</div>' +
                        (subtitle ? '<div class="data-panel-subtitle">' + subtitle + '</div>' : '') +
                    '</div>' +
                '</div>' +
                '<div class="data-panel-body">' + bodyHtml + '</div>' +
            '</div>';
        }}

        function buildCpuPanel(d) {{
            if (!d || d === null) return '';
            var body = '';
            if (d.usage_percent != null) body += pctBar(d.usage_percent, 'Usage');
            body += dataRow('Name', d.brand || '\u2014');
            if (d.base_frequency_mhz != null) body += dataRow('Base Speed', (d.base_frequency_mhz/1000).toFixed(2) + ' GHz');
            if (d.frequency_mhz != null) body += dataRow('Speed', (d.frequency_mhz/1000).toFixed(2) + ' GHz');
            if (d.sockets != null) body += dataRow('Sockets', d.sockets);
            if (d.physical_cores != null) body += dataRow('Cores', d.physical_cores);
            if (d.logical_cores != null) body += dataRow('Logical Processors', d.logical_cores);
            if (d.virtualization != null) body += dataRow('Virtualization', d.virtualization ? '<span class="data-tag online">Enabled</span>' : '<span class="data-tag offline">Disabled</span>');
            if (d.l1_cache_kb != null) body += dataRow('L1 Cache', d.l1_cache_kb >= 1024 ? (d.l1_cache_kb/1024).toFixed(1) + ' MB' : d.l1_cache_kb + ' KB');
            if (d.l2_cache_kb != null) body += dataRow('L2 Cache', d.l2_cache_kb >= 1024 ? (d.l2_cache_kb/1024).toFixed(1) + ' MB' : d.l2_cache_kb + ' KB');
            if (d.l3_cache_kb != null) body += dataRow('L3 Cache', d.l3_cache_kb >= 1024 ? (d.l3_cache_kb/1024).toFixed(1) + ' MB' : d.l3_cache_kb + ' KB');
            if (d.process_count != null) body += dataRow('Processes', d.process_count);
            if (d.thread_count != null) body += dataRow('Threads', d.thread_count);
            if (d.handle_count != null) body += dataRow('Handles', d.handle_count);
            if (d.temperature && d.temperature.average_c) body += dataRow('Temperature', d.temperature.average_c.toFixed(1) + ' \u00b0C');
            if (d.uptime_seconds != null) {{
                var s = d.uptime_seconds; var dd = Math.floor(s/86400); var hh = Math.floor((s%86400)/3600); var mm = Math.floor((s%3600)/60); var ss = s%60;
                body += dataRow('Up Time', (dd > 0 ? dd + ':' : '') + (hh<10?'0':'') + hh + ':' + (mm<10?'0':'') + mm + ':' + (ss<10?'0':'') + ss);
            }}
            return panelCard('cpu', 'CPU', d.brand || null, body);
        }}

        function buildGpuPanel(d) {{
            if (!d || d === null) return '';
            var body = '';
            var adapters = d.adapters || [];
            if (adapters.length > 1) {{
                // Multi-GPU: show each adapter as a section
                adapters.forEach(function(a, i) {{
                    body += '<div style="margin-bottom:8px;padding-bottom:8px;' + (i < adapters.length-1 ? 'border-bottom:1px solid var(--border-color,#333);' : '') + '">';
                    body += dataRow('GPU ' + i, a.name || '\u2014');
                    if (a.usage_percent != null) body += pctBar(a.usage_percent, 'Utilization');
                    if (a.vram_total_mb != null && a.vram_used_mb != null) {{
                        body += dataRow('Dedicated Memory', (a.vram_used_mb/1024).toFixed(1) + ' / ' + (a.vram_total_mb/1024).toFixed(1) + ' GB');
                    }}
                    if (a.shared_gpu_memory_bytes != null) body += dataRow('Shared Memory', fmtBytes(a.shared_gpu_memory_bytes));
                    if (a.driver_version) body += dataRow('Driver', a.driver_version);
                    if (a.driver_date) body += dataRow('Driver Date', a.driver_date);
                    if (a.manufacturer) body += dataRow('Manufacturer', a.manufacturer);
                    if (a.physical_location && typeof a.physical_location === 'object') {{
                        body += dataRow('Physical Location', 'PCI bus ' + (a.physical_location.bus!=null?a.physical_location.bus:'?') + ', device ' + (a.physical_location.device!=null?a.physical_location.device:'?') + ', function ' + (a.physical_location.function!=null?a.physical_location.function:'?'));
                    }}
                    if (a.temperature_c != null) body += dataRow('Temperature', a.temperature_c.toFixed(1) + ' \u00b0C');
                    if (a.power_draw_w != null) body += dataRow('Power Draw', a.power_draw_w.toFixed(1) + ' W');
                    if (a.encoder_usage_percent != null) body += dataRow('Video Encode', a.encoder_usage_percent.toFixed(0) + '%');
                    if (a.decoder_usage_percent != null) body += dataRow('Video Decode', a.decoder_usage_percent.toFixed(0) + '%');
                    body += '</div>';
                }});
            }} else {{
                // Single GPU — flat layout
                if (d.usage_percent != null) body += pctBar(d.usage_percent, 'GPU Load');
                body += dataRow('Name', d.name || '\u2014');
                if (d.vram_total_mb != null && d.vram_used_mb != null) {{
                    body += dataRow('Dedicated Memory', (d.vram_used_mb/1024).toFixed(1) + ' / ' + (d.vram_total_mb/1024).toFixed(1) + ' GB');
                }}
                if (d.shared_gpu_memory_bytes != null) body += dataRow('Shared Memory', fmtBytes(d.shared_gpu_memory_bytes));
                if (d.driver_version) body += dataRow('Driver', d.driver_version);
                if (d.driver_date) body += dataRow('Driver Date', d.driver_date);
                if (d.manufacturer) body += dataRow('Manufacturer', d.manufacturer);
                if (d.physical_location && typeof d.physical_location === 'object') {{
                    body += dataRow('Physical Location', 'PCI bus ' + (d.physical_location.bus!=null?d.physical_location.bus:'?') + ', device ' + (d.physical_location.device!=null?d.physical_location.device:'?') + ', function ' + (d.physical_location.function!=null?d.physical_location.function:'?'));
                }}
                if (d.temperature_c != null) body += dataRow('Temperature', d.temperature_c.toFixed(1) + ' \u00b0C');
                if (d.power_draw_w != null) body += dataRow('Power Draw', d.power_draw_w.toFixed(1) + ' W');
                if (d.fan_speed_percent != null) body += dataRow('Fan Speed', d.fan_speed_percent + '%');
                if (d.clock_graphics_mhz != null) body += dataRow('GPU Clock', d.clock_graphics_mhz + ' MHz');
                if (d.clock_memory_mhz != null) body += dataRow('Mem Clock', d.clock_memory_mhz + ' MHz');
                if (d.encoder_usage_percent != null) body += dataRow('Video Encode', d.encoder_usage_percent.toFixed(0) + '%');
                if (d.decoder_usage_percent != null) body += dataRow('Video Decode', d.decoder_usage_percent.toFixed(0) + '%');
            }}
            return panelCard('gpu', 'GPU', d.name || null, body);
        }}

        function buildRamPanel(d) {{
            if (!d || d === null) return '';
            var body = '';
            if (d.usage_percent != null) body += pctBar(d.usage_percent, 'Usage');
            if (d.total_bytes != null) {{
                body += '<div class="data-stat-row">' +
                    '<div class="data-stat-item"><div class="data-stat-label">In Use</div><div class="data-stat-value">' + fmtBytes(d.used_bytes) + '</div></div>' +
                    '<div class="data-stat-item"><div class="data-stat-label">Available</div><div class="data-stat-value">' + fmtBytes(d.available_bytes) + '</div></div>' +
                    '<div class="data-stat-item"><div class="data-stat-label">Total</div><div class="data-stat-value">' + fmtBytes(d.total_bytes) + '</div></div>' +
                '</div>';
            }}
            if (d.speed_mhz) body += dataRow('Speed', d.speed_mhz + ' MT/s');
            if (d.slots_used != null && d.slots_total != null) body += dataRow('Slots Used', d.slots_used + ' of ' + d.slots_total);
            if (d.form_factor) body += dataRow('Form Factor', d.form_factor);
            if (d.memory_type) body += dataRow('Type', d.memory_type);
            if (d.hardware_reserved_bytes != null) body += dataRow('Hardware Reserved', fmtBytes(d.hardware_reserved_bytes));
            if (d.committed_bytes != null && d.commit_limit_bytes != null) body += dataRow('Committed', fmtBytes(d.committed_bytes) + ' / ' + fmtBytes(d.commit_limit_bytes));
            if (d.cached_bytes != null) body += dataRow('Cached', fmtBytes(d.cached_bytes));
            if (d.paged_pool_bytes != null) body += dataRow('Paged Pool', fmtBytes(d.paged_pool_bytes));
            if (d.non_paged_pool_bytes != null) body += dataRow('Non-paged Pool', fmtBytes(d.non_paged_pool_bytes));
            if (d.compressed_bytes != null && d.compressed_bytes > 0) body += dataRow('Compressed', fmtBytes(d.compressed_bytes));
            return panelCard('ram', 'Memory', d.memory_type ? d.total_bytes ? fmtBytes(d.total_bytes) + ' ' + d.memory_type : d.memory_type : null, body);
        }}

        function buildStoragePanel(d) {{
            if (!d || d === null) return '';
            var body = '';
            var drives = d.disks || d.drives || (Array.isArray(d) ? d : []);
            var physDisks = d.physical_disks || [];
            if (physDisks.length > 0) {{
                physDisks.forEach(function(pd, i) {{
                    body += '<div style="margin-bottom:8px;padding-bottom:8px;' + (i < physDisks.length-1 ? 'border-bottom:1px solid var(--border-color,#333);' : '') + '">';
                    var label = 'Disk ' + (pd.disk_number != null ? pd.disk_number : i);
                    var model = pd.model || '';
                    body += dataRow(label, model || '\u2014');
                    if (pd.media_type) body += dataRow('Type', pd.media_type + (pd.bus_type ? ' (' + pd.bus_type + ')' : ''));
                    if (pd.physical_capacity_bytes != null) body += dataRow('Capacity', fmtBytes(pd.physical_capacity_bytes));
                    if (pd.system_disk) body += dataRow('System Disk', '<span class="data-tag online">Yes</span>');
                    if (pd.page_file_disk) body += dataRow('Page File', '<span class="data-tag online">Yes</span>');
                    if (pd.health_status) body += dataRow('Health', pd.health_status);
                    if (pd.firmware_version) body += dataRow('Firmware', pd.firmware_version);
                    // Show logical volumes for this physical disk
                    var pdVolumes = pd.volumes || [];
                    pdVolumes.forEach(function(v) {{
                        if (v.drive_letter) body += dataRow('  ' + v.drive_letter + ':', v.label ? v.label + ' (' + fmtBytes(v.size_bytes) + ')' : fmtBytes(v.size_bytes));
                    }});
                    body += '</div>';
                }});
            }}
            if (drives.length > 0) {{
                body += '<div class="data-drives-list">';
                drives.forEach(function(drv) {{
                    var name = drv.mount || drv.name || drv.letter || '?';
                    var total = drv.total_bytes || 0;
                    var avail = drv.available_bytes || 0;
                    var used = drv.used_bytes || (total - avail);
                    var pct = total > 0 ? (used / total * 100) : 0;
                    var cls = pct > 90 ? 'danger' : pct > 70 ? 'warn' : '';
                    body += '<div><div class="data-drive-label"><span>' + name + '</span><span>' + fmtBytes(used) + ' / ' + fmtBytes(total) + '</span></div>' +
                            '<div class="data-bar-wrap"><div class="data-bar-fill ' + cls + '" style="width:' + Math.min(pct,100) + '%"></div></div></div>';
                }});
                body += '</div>';
            }} else if (physDisks.length === 0) {{
                body += dataRow('Status', 'No drives detected');
            }}
            return panelCard('storage', 'Storage', (physDisks.length || drives.length) + ' disk(s)', body);
        }}

        function buildNetworkPanel(d) {{
            if (!d || d === null) return '';
            var body = '';
            var ifaces = d.interfaces || [];
            if (Array.isArray(ifaces) && ifaces.length > 0) {{
                ifaces.slice(0, 6).forEach(function(iface, idx) {{
                    var name = iface.interface || iface.name || 'Interface';
                    var desc = iface.description || '';
                    var hasTraffic = (iface.received_bytes_per_second > 0 || iface.transmitted_bytes_per_second > 0 || iface.total_received_bytes > 0);
                    var statusTag = iface.adapter_status === 'Up' ? 'online' : (hasTraffic ? 'online' : 'offline');
                    var statusText = iface.adapter_status || (hasTraffic ? 'Active' : 'Idle');
                    body += '<div style="' + (idx > 0 ? 'margin-top:8px;padding-top:8px;border-top:1px solid var(--border-color,#333);' : '') + '">';
                    body += dataRow(name, '<span class="data-tag ' + statusTag + '">' + statusText + '</span>');
                    if (desc) body += dataRow('Adapter', desc);
                    if (iface.link_speed) body += dataRow('Link Speed', iface.link_speed);
                    if (iface.media_type) body += dataRow('Type', iface.media_type);
                    var ipv4 = null; var ipv6 = null;
                    if (iface.ip_addresses && Array.isArray(iface.ip_addresses)) {{
                        for (var i = 0; i < iface.ip_addresses.length; i++) {{
                            var addr = iface.ip_addresses[i].addr || '';
                            if (!ipv4 && addr && addr.indexOf('.') !== -1 && addr.indexOf(':') === -1) ipv4 = addr;
                            if (!ipv6 && addr && addr.indexOf(':') !== -1) ipv6 = addr;
                        }}
                    }}
                    if (ipv4) body += dataRow('IPv4', ipv4);
                    if (ipv6) body += dataRow('IPv6', '<span style="font-size:11px">' + ipv6 + '</span>');
                    if (iface.received_bytes_per_second != null) body += dataRow('Down', fmtBytes(Math.round(iface.received_bytes_per_second)) + '/s');
                    if (iface.transmitted_bytes_per_second != null) body += dataRow('Up', fmtBytes(Math.round(iface.transmitted_bytes_per_second)) + '/s');
                    if (iface.driver_version) body += dataRow('Driver', iface.driver_version);
                    body += '</div>';
                }});
            }} else {{
                body += dataRow('Status', 'No interfaces detected');
            }}
            return panelCard('network', 'Network', (ifaces.length || 0) + ' interface(s)', body);
        }}

        function buildAudioPanel(d) {{
            if (!d || d === null) return '';
            var body = '';
            var od = d.output_device || {{}};
            var id = d.input_device || {{}};
            if (od.volume_percent != null) {{
                body += pctBar(od.volume_percent, 'Volume');
            }}
            body += dataRow('Muted', od.muted != null ? (od.muted ? 'Yes' : 'No') : '\u2014');
            if (od.name) body += dataRow('Output', od.name);
            if (id.name) body += dataRow('Input', id.name);
            var ms = d.media_session;
            if (ms && ms.playing) {{
                body += dataRow('Playing', (ms.title || '?') + (ms.artist ? ' \u2014 ' + ms.artist : ''));
            }}
            return panelCard('audio', 'Audio', null, body);
        }}

        function buildTimePanel(d) {{
            if (!d || d === null) return '';
            var body = '';
            if (d.iso || d.datetime) {{
                var t = d.iso || d.datetime;
                body += '<div class="data-big-value">' + t.substring(11,19) + '</div>';
                body += dataRow('Date', t.substring(0,10));
            }}
            if (d.timezone) body += dataRow('Timezone', d.timezone);
            if (d.uptime_seconds != null) {{
                var s = d.uptime_seconds;
                var h = Math.floor(s/3600); var m = Math.floor((s%3600)/60);
                body += dataRow('Uptime', h + 'h ' + m + 'm');
            }}
            return panelCard('time', 'Time', null, body);
        }}

        function buildKeyboardPanel(d) {{
            if (!d || d === null) return '';
            var body = '';
            var ts = d.toggle_states || {{}};
            body += dataRow('Caps Lock', ts.caps_lock ? '<span class="data-tag online">ON</span>' : '<span class="data-tag offline">OFF</span>');
            body += dataRow('Num Lock', ts.num_lock ? '<span class="data-tag online">ON</span>' : '<span class="data-tag offline">OFF</span>');
            body += dataRow('Scroll Lock', ts.scroll_lock ? '<span class="data-tag online">ON</span>' : '<span class="data-tag offline">OFF</span>');
            if (d.layout_id) body += dataRow('Layout', d.layout_id);
            if (d.type_name) body += dataRow('Type', d.type_name);
            return panelCard('keyboard', 'Keyboard', null, body);
        }}

        function buildMousePanel(d) {{
            if (!d || d === null) return '';
            var body = '';
            var c = d.cursor || {{}};
            var b = d.buttons || {{}};
            if (c.x != null && c.y != null) body += dataRow('Position', c.x + ', ' + c.y);
            if (b.count != null) body += dataRow('Buttons', b.count);
            if (b.swapped != null) body += dataRow('Swap Buttons', b.swapped ? 'Yes' : 'No');
            if (d.speed != null) body += dataRow('Speed', d.speed);
            if (d.wheel_present != null) body += dataRow('Wheel', d.wheel_present ? 'Present' : 'None');
            return panelCard('mouse', 'Mouse', null, body);
        }}

        function buildPowerPanel(d) {{
            if (!d || d === null) return '';
            var body = '';
            var bat = d.battery || {{}};
            if (bat.percent != null) {{
                body += pctBar(bat.percent, 'Battery');
            }}
            var acOn = d.ac_status === 'online';
            var status = acOn ? 'AC Power' : (bat.charging ? 'Charging' : 'Battery');
            var tagClass = acOn || bat.charging ? 'charging' : (bat.percent != null && bat.percent < 20 ? 'offline' : 'online');
            body += dataRow('Status', '<span class="data-tag ' + tagClass + '">' + status + '</span>');
            if (bat.saver_active != null) body += dataRow('Battery Saver', bat.saver_active ? 'Active' : 'Off');
            if (d.power_plan) body += dataRow('Power Plan', d.power_plan);
            return panelCard('power', 'Power', null, body);
        }}

        function buildWifiPanel(d) {{
            if (!d || d === null) return '';
            var body = '';
            var c = d.connected || {{}};
            if (c.ssid) body += dataRow('SSID', c.ssid);
            if (c.signal_percent != null) body += pctBar(c.signal_percent, 'Signal');
            if (c.bssid) body += dataRow('BSSID', c.bssid);
            if (c.channel) body += dataRow('Channel', c.channel);
            if (c.band) body += dataRow('Band', c.band);
            if (c.radio_type) body += dataRow('Protocol', c.radio_type);
            body += dataRow('Connected', c.is_connected ? '<span class="data-tag online">Yes</span>' : '<span class="data-tag offline">No</span>');
            return panelCard('wifi', 'WiFi', c.ssid || null, body);
        }}

        function buildBluetoothPanel(d) {{
            if (!d || d === null) return '';
            var body = '';
            var ad = d.adapter || {{}};
            body += dataRow('Available', ad.present ? '<span class="data-tag online">Yes</span>' : '<span class="data-tag offline">No</span>');
            if (ad.status) body += dataRow('Status', ad.status);
            if (ad.name) body += dataRow('Adapter', ad.name);
            var devices = d.devices || [];
            if (devices.length > 0) {{
                body += dataRow('Devices', devices.length);
                devices.slice(0, 5).forEach(function(dev) {{
                    var devName = typeof dev === 'string' ? dev : (dev.name || dev.address || '?');
                    var conn = dev.connected ? ' <span class="data-tag online">Connected</span>' : '';
                    body += dataRow('', devName + conn);
                }});
            }}
            return panelCard('bluetooth', 'Bluetooth', null, body);
        }}

        function buildSystemPanel(d) {{
            if (!d || d === null) return '';
            var body = '';
            var os = d.os || {{}};
            if (os.long_name || os.name) body += dataRow('OS', os.long_name || os.name);
            if (os.version) body += dataRow('Version', os.version);
            if (d.hostname || d.computer_name) body += dataRow('Hostname', d.hostname || d.computer_name);
            if (d.username) body += dataRow('User', d.username);
            if (os.cpu_arch || os.arch) body += dataRow('Arch', os.cpu_arch || os.arch);
            if (d.motherboard && d.motherboard.manufacturer && d.motherboard.product) body += dataRow('Board', d.motherboard.manufacturer + ' ' + d.motherboard.product);
            return panelCard('system', 'System', d.hostname || d.computer_name || null, body);
        }}

        function buildDisplaysPanel(displays) {{
            if (!displays || !Array.isArray(displays) || displays.length === 0) return '';
            var body = '';
            displays.forEach(function(m, i) {{
                var meta = m.metadata || m;
                var w = meta.width || '?';
                var h = meta.height || '?';
                var primary = meta.primary ? ' <span class="data-tag online">PRIMARY</span>' : '';
                var monName = meta.monitor_name || '';
                body += '<div style="' + (i > 0 ? 'margin-top:8px;padding-top:8px;border-top:1px solid var(--border-color,#333);' : '') + '">';
                body += dataRow('Monitor ' + (i+1) + primary, monName || (w + '\u00d7' + h));
                body += dataRow('Resolution', w + '\u00d7' + h);
                if (meta.aspect_ratio) body += dataRow('Aspect Ratio', meta.aspect_ratio);
                if (meta.refresh_rate_hz) body += dataRow('Refresh Rate', meta.refresh_rate_hz + ' Hz');
                if (meta.dpi) body += dataRow('DPI', meta.dpi);
                if (meta.scale && meta.scale !== 1.0) body += dataRow('Scale', (meta.scale * 100).toFixed(0) + '%');
                if (meta.color_depth_bits) body += dataRow('Color Depth', meta.color_depth_bits + ' bit' + (meta.bits_per_channel ? ' (' + meta.bits_per_channel + ' bpc)' : ''));
                if (meta.orientation && meta.orientation !== 'landscape') body += dataRow('Orientation', meta.orientation);
                if (meta.connection_type) body += dataRow('Connection', meta.connection_type);
                if (meta.hdr_supported) body += dataRow('HDR', '<span class="data-tag online">Supported</span>');
                if (meta.manufacturer) body += dataRow('Manufacturer', meta.manufacturer);
                if (meta.physical_width_mm && meta.physical_height_mm) {{
                    var diag = Math.sqrt(meta.physical_width_mm*meta.physical_width_mm + meta.physical_height_mm*meta.physical_height_mm) / 25.4;
                    body += dataRow('Size', meta.physical_width_mm/10 + ' \u00d7 ' + meta.physical_height_mm/10 + ' cm (' + diag.toFixed(1) + '")');
                }}
                if (meta.year_of_manufacture && meta.year_of_manufacture > 0) body += dataRow('Year', meta.year_of_manufacture);
                body += '</div>';
            }});
            return panelCard('displays', 'Displays', displays.length + ' monitor(s)', body);
        }}

        function buildIdlePanel(d) {{
            if (!d || d === null) return '';
            var body = '';
            if (d.idle_seconds != null || d.idle_ms != null) {{
                var sec = d.idle_seconds != null ? d.idle_seconds : Math.floor(d.idle_ms / 1000);
                var m = Math.floor(sec / 60);
                var s = sec % 60;
                body += '<div class="data-big-value">' + m + '<span class="data-big-unit">m</span> ' + s + '<span class="data-big-unit">s</span></div>';
            }}
            if (d.screensaver_active != null) body += dataRow('Screensaver', d.screensaver_active ? 'Active' : 'Inactive');
            if (d.screen_locked != null) body += dataRow('Screen Locked', d.screen_locked ? 'Yes' : 'No');
            if (d.idle_state) body += dataRow('State', d.idle_state);
            return panelCard('idle', 'Idle', null, body);
        }}

        function buildProcessesPanel(d) {{
            if (!d || d === null) return '';
            var body = '';
            var procs = d.top_cpu || d.top_memory || [];
            if (d.total_count != null) body += dataRow('Total', d.total_count);
            if (procs.length > 0) {{
                procs.slice(0, 8).forEach(function(p) {{
                    var name = p.name || '?';
                    var cpu = p.cpu_percent != null ? p.cpu_percent.toFixed(1) + '%' : '';
                    var mem = p.memory_bytes ? fmtBytes(p.memory_bytes) : '';
                    body += dataRow(name, cpu + (cpu && mem ? ' / ' : '') + mem);
                }});
            }} else {{
                body += dataRow('Status', 'No process data');
            }}
            return panelCard('processes', 'Processes', d.total_count ? d.total_count + ' running' : null, body);
        }}

        function buildAppdataPanel(appdata) {{
            if (!appdata || typeof appdata !== 'object') return '';
            var monitors = Object.keys(appdata);
            if (monitors.length === 0) return '';
            var body = '<div class="data-appdata-section">';
            monitors.forEach(function(monId, idx) {{
                var entry = appdata[monId];
                var windows = (entry && entry.windows) || [];
                body += '<div class="data-appdata-monitor">';
                body += '<div class="data-appdata-monitor-title">Monitor ' + (idx + 1) + ' <span style="color:var(--text-dim);font-weight:400;font-size:11px;">' + monId.substring(0,12) + '\u2026</span></div>';
                if (windows.length === 0) {{
                    body += '<div style="font-size:12px;color:var(--text-dim);">No active windows</div>';
                }} else {{
                    windows.forEach(function(w) {{
                        body += '<div class="data-window-item">' +
                            '<span class="data-window-app">' + (w.app_name || '?') + '</span>' +
                            '<span class="data-window-title">' + (w.window_title || '') + '</span>' +
                            (w.focused ? '<span class="data-window-badge">focused</span>' : '') +
                            (w.window_state && w.window_state !== 'normal' ? '<span class="data-window-badge">' + w.window_state + '</span>' : '') +
                        '</div>';
                    }});
                }}
                body += '</div>';
            }});
            body += '</div>';
            return panelCard('appdata', 'Active Windows', monitors.length + ' monitor(s)', body);
        }}

        function renderDataPanels(data) {{
            var container = document.getElementById('data-panels-container');
            var jsonFallback = document.getElementById('data-json-fallback');
            if (!container) return;
            if (!data) {{ container.innerHTML = '<div style="color:var(--text-dim);padding:20px;">No data available</div>'; return; }}

            var filter = window.__dataActiveChip || 'All';

            // JSON raw view
            if (filter === 'JSON') {{
                container.style.display = 'none';
                if (jsonFallback) {{
                    jsonFallback.style.display = 'block';
                    var pre = document.getElementById('data-json-pre');
                    if (pre) pre.textContent = JSON.stringify(data, null, 2);
                }}
                return;
            }}

            if (jsonFallback) jsonFallback.style.display = 'none';
            container.style.display = '';

            var allowed = FILTER_MAP[filter];
            var sys = data.sysdata || {{}};
            var html = '';

            function shouldShow(key) {{ return !allowed || allowed.indexOf(key) !== -1; }}

            if (shouldShow('time'))       html += buildTimePanel(sys.time);
            if (shouldShow('cpu'))        html += buildCpuPanel(sys.cpu);
            if (shouldShow('gpu'))        html += buildGpuPanel(sys.gpu);
            if (shouldShow('ram'))        html += buildRamPanel(sys.ram);
            if (shouldShow('storage'))    html += buildStoragePanel(sys.storage);
            if (shouldShow('displays'))   html += buildDisplaysPanel(sys.displays);
            if (shouldShow('network'))    html += buildNetworkPanel(sys.network);
            if (shouldShow('wifi'))       html += buildWifiPanel(sys.wifi);
            if (shouldShow('bluetooth'))  html += buildBluetoothPanel(sys.bluetooth);
            if (shouldShow('audio'))      html += buildAudioPanel(sys.audio);
            if (shouldShow('keyboard'))   html += buildKeyboardPanel(sys.keyboard);
            if (shouldShow('mouse'))      html += buildMousePanel(sys.mouse);
            if (shouldShow('power'))      html += buildPowerPanel(sys.power);
            if (shouldShow('idle'))       html += buildIdlePanel(sys.idle);
            if (shouldShow('system'))     html += buildSystemPanel(sys.system);
            if (shouldShow('processes'))  html += buildProcessesPanel(sys.processes);
            if (shouldShow('appdata'))    html += buildAppdataPanel(data.appdata);

            container.innerHTML = html || '<div style="color:var(--text-dim);padding:20px;">No data for this filter</div>';
        }}

        window.__sentinelPushMonitors = function(monitors) {{
            var frame = document.getElementById('tabFrame');
            if (frame && frame.contentWindow) {{
                frame.contentWindow.postMessage({{ type: '__sentinel_monitors', monitors: monitors }}, '*');
            }}
        }};

        // Live registry data push from Rust event loop
        window.__sentinelPushRegistry = function(data) {{
            window.__lastRegistryData = data;
            // Only update if the Data page is currently active
            if (viewMode === 'data') {{
                renderDataPanels(data);
            }}
        }};

        document.querySelectorAll('.quick-action-btn').forEach(function(btn) {{
            btn.addEventListener('click', function() {{
                var tip = (btn.getAttribute('data-tooltip') || '').toLowerCase();
                if (tip === 'home' || tip === 'settings' || tip === 'data') {{
                    viewMode = tip;
                    render();
                }}
            }});
        }});

        function render() {{
            renderAddons();
            var addonPanel = document.getElementById('right-addon-panel');
            var pagePanel = document.getElementById('right-page-panel');
            if (viewMode === 'addon') {{
                addonPanel.style.display = 'flex';
                pagePanel.style.display = 'none';
                renderTabs();
            }} else {{
                addonPanel.style.display = 'none';
                pagePanel.style.display = 'flex';
                if (viewMode === 'home') renderHomePage();
                else if (viewMode === 'settings') renderSettingsPage();
                else if (viewMode === 'data') renderDataPage();
            }}
        }}

        render();
    </script>
</body>
</html>
"#
        ))
}

struct SentinelApp {
    section: UiSection,
    addon_catalog: Vec<AddonMeta>,
    selected_addon_idx: usize,
    addon_state: Option<AddonConfigState>,
    global_status: String,
    caches: UiCaches,
    addon_hub_tab: AddonHubTab,
    editor_selected_asset: Option<String>,
    library_selected_monitor: Option<String>,
    selected_custom_tab: Option<String>,
    last_opened_custom_tab: Option<String>,
    // Backend settings state
    settings_fast_rate: u64,
    settings_slow_rate: u64,
    settings_pull_paused: bool,
    settings_refresh_on_request: bool,
    settings_loaded: bool,
}

impl SentinelApp {
    fn load_selected_addon(&mut self) {
        if self.addon_catalog.is_empty() {
            self.addon_state = None;
            self.global_status = "No addons available".to_string();
            return;
        }

        self.selected_addon_idx = self.selected_addon_idx.min(self.addon_catalog.len() - 1);
        let selected = self.addon_catalog[self.selected_addon_idx].clone();
        match load_addon_state(selected) {
            Ok(state) => {
                self.addon_hub_tab = if state.meta.accepts_assets {
                    AddonHubTab::Library
                } else {
                    AddonHubTab::Settings
                };
                self.editor_selected_asset = state.assets.first().map(|a| a.id.clone());
                self.library_selected_monitor = None;
                self.selected_custom_tab = state.custom_tabs.first().map(|t| t.id.clone());
                self.last_opened_custom_tab = None;
                self.addon_state = Some(state);
                self.global_status = "Loaded addon config".to_string();
            }
            Err(e) => {
                self.global_status = format!("Failed to load addon config: {}", e);
                self.addon_state = None;
            }
        }
    }

    fn sidebar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("sidebar")
            .resizable(false)
            .default_width(220.0)
            .show(ctx, |ui| {
                ui.heading("Sentinel");
                ui.label(RichText::new("Native control center").color(Color32::GRAY));
                ui.add_space(8.0);
                ui.separator();

                ui.selectable_value(&mut self.section, UiSection::Home, "Home");
                ui.selectable_value(&mut self.section, UiSection::Addons, "Addons");
                ui.selectable_value(&mut self.section, UiSection::Integrations, "Integrations");
                ui.selectable_value(&mut self.section, UiSection::Settings, "Settings");

                ui.separator();
                ui.label(RichText::new("Schema + asset hub").italics());
                ui.label(RichText::new("Scope: local native UI (non-web)").italics());
            });
    }

    fn section_card(ui: &mut egui::Ui, title: &str, add_contents: impl FnOnce(&mut egui::Ui)) {
        egui::Frame::default()
            .fill(Color32::from_rgb(21, 24, 30))
            .stroke(Stroke::new(1.0, Color32::from_rgb(55, 66, 82)))
            .corner_radius(6.0)
            .inner_margin(egui::Margin::same(10))
            .show(ui, |ui| {
                ui.label(RichText::new(title).strong().size(16.0));
                ui.add_space(6.0);
                add_contents(ui)
            });
    }

    fn show_home(&mut self, ui: &mut egui::Ui) {
        Self::section_card(ui, "Overview", |ui| {
            ui.label("Addon config pages are schema-driven.");
            ui.label("Addons that accept assets get Library / Editor / Discover / Settings tabs.");
            ui.label("Addons without assets expose Settings only.");
        });
    }

    fn show_integrations(&mut self, ui: &mut egui::Ui) {
        Self::section_card(ui, "Integrations", |ui| {
            ui.group(|ui| {
                ui.strong("Steam Workshop");
                ui.label("Planned provider for browsing/installing/updating addon assets.");
                ui.label(RichText::new("Status: scaffolded").color(Color32::LIGHT_BLUE));
            });
        });
    }

    fn show_settings(&mut self, ui: &mut egui::Ui) {
        // Load current values from the backend config on first visit
        if !self.settings_loaded {
            let cfg = crate::config::current_config();
            self.settings_fast_rate = cfg.fast_pull_rate_ms;
            self.settings_slow_rate = cfg.slow_pull_rate_ms;
            self.settings_pull_paused = cfg.data_pull_paused;
            self.settings_refresh_on_request = cfg.refresh_on_request;
            self.settings_loaded = true;
        }

        Self::section_card(ui, "Backend Settings", |ui| {
            ui.label("Control the Sentinel backend data engine.");
            ui.add_space(10.0);

            // ── Fast-tier pull rate slider ──
            ui.label(RichText::new("Fast Pull Rate").strong());
            ui.label(
                RichText::new("How often lightweight data is collected (audio, time, keyboard, mouse, idle, power, display). 0–5000 ms.")
                    .small()
                    .color(Color32::GRAY),
            );
            ui.add_space(4.0);

            let fast_before = self.settings_fast_rate;
            ui.horizontal(|ui| {
                ui.add(
                    egui::Slider::new(&mut self.settings_fast_rate, 0..=5000)
                        .suffix(" ms")
                        .clamping(egui::SliderClamping::Always),
                );
                ui.label(format!("{}ms", self.settings_fast_rate));
            });

            if self.settings_fast_rate != fast_before {
                crate::config::set_fast_pull_rate_ms(self.settings_fast_rate);
                self.global_status = format!("Fast pull rate → {}ms", self.settings_fast_rate);
            }

            ui.add_space(12.0);
            ui.separator();
            ui.add_space(8.0);

            // ── Slow-tier pull rate slider ──
            ui.label(RichText::new("Slow Pull Rate").strong());
            ui.label(
                RichText::new("How often heavyweight data is collected (CPU, GPU, RAM, storage, network, processes, etc.). 0–10000 ms.")
                    .small()
                    .color(Color32::GRAY),
            );
            ui.add_space(4.0);

            let slow_before = self.settings_slow_rate;
            ui.horizontal(|ui| {
                ui.add(
                    egui::Slider::new(&mut self.settings_slow_rate, 0..=10000)
                        .suffix(" ms")
                        .clamping(egui::SliderClamping::Always),
                );
                ui.label(format!("{}ms", self.settings_slow_rate));
            });

            if self.settings_slow_rate != slow_before {
                crate::config::set_slow_pull_rate_ms(self.settings_slow_rate);
                self.global_status = format!("Slow pull rate → {}ms", self.settings_slow_rate);
            }

            ui.add_space(12.0);
            ui.separator();
            ui.add_space(8.0);

            // ── Refresh on request toggle ──
            ui.label(RichText::new("Refresh on Request").strong());
            ui.label(
                RichText::new("When enabled, fast-tier data is refreshed inline on every IPC sysdata request for lower latency.")
                    .small()
                    .color(Color32::GRAY),
            );
            ui.add_space(4.0);

            let ror_before = self.settings_refresh_on_request;
            ui.checkbox(&mut self.settings_refresh_on_request, "Enabled");

            if self.settings_refresh_on_request != ror_before {
                crate::config::set_refresh_on_request(self.settings_refresh_on_request);
                self.global_status = if self.settings_refresh_on_request {
                    "Refresh on request enabled".to_string()
                } else {
                    "Refresh on request disabled".to_string()
                };
            }

            ui.add_space(12.0);
            ui.separator();
            ui.add_space(8.0);

            // ── Pause toggle ──
            ui.label(RichText::new("Pause Data Pulling").strong());
            ui.label(
                RichText::new("While paused the registry will not update. Useful for reducing resource usage.")
                    .small()
                    .color(Color32::GRAY),
            );
            ui.add_space(4.0);

            let paused_before = self.settings_pull_paused;
            ui.checkbox(&mut self.settings_pull_paused, "Paused");

            if self.settings_pull_paused != paused_before {
                crate::config::set_pull_paused(self.settings_pull_paused);
                self.global_status = if self.settings_pull_paused {
                    "Data pulling paused".to_string()
                } else {
                    "Data pulling resumed".to_string()
                };
            }

            ui.add_space(12.0);
            ui.separator();
            ui.add_space(8.0);

            // ── Reload from disk button ──
            if ui.button("Reload config from disk").clicked() {
                let cfg = crate::config::load_config();
                self.settings_fast_rate = cfg.fast_pull_rate_ms;
                self.settings_slow_rate = cfg.slow_pull_rate_ms;
                self.settings_pull_paused = cfg.data_pull_paused;
                self.settings_refresh_on_request = cfg.refresh_on_request;
                self.global_status = "Reloaded config.yaml".to_string();
            }
        });
    }

    fn render_addon_tabs(&mut self, ui: &mut egui::Ui) -> bool {
        if self.addon_catalog.is_empty() {
            return false;
        }

        let mut changed = false;
        ui.horizontal_wrapped(|ui| {
            for (idx, addon) in self.addon_catalog.iter().enumerate() {
                let selected = idx == self.selected_addon_idx;
                let text = RichText::new(&addon.name)
                    .strong()
                    .color(if selected { Color32::WHITE } else { Color32::from_rgb(210, 215, 225) });

                if ui.selectable_label(selected, text).clicked() {
                    self.selected_addon_idx = idx;
                    changed = true;
                }
            }
        });

        changed
    }

    fn render_hub_tabs(ui: &mut egui::Ui, tab: &mut AddonHubTab, accepts_assets: bool) {
        ui.horizontal(|ui| {
            if accepts_assets {
                ui.selectable_value(tab, AddonHubTab::Library, "Library");
                ui.selectable_value(tab, AddonHubTab::Editor, "Editor");
                ui.selectable_value(tab, AddonHubTab::Discover, "Discover");
            }
            ui.selectable_value(tab, AddonHubTab::Settings, "Settings");
        });
    }

    fn show_addons(&mut self, ui: &mut egui::Ui) {
        Self::section_card(ui, "Addon Hub", |ui| {
            if self.addon_catalog.is_empty() {
                ui.label("No addons found in ~/.Sentinel/Addons.");
                return;
            }

            if self.render_addon_tabs(ui) || self.addon_state.is_none() {
                self.load_selected_addon();
            }

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(8.0);

            if let Some(mut state) = self.addon_state.take() {
                if !state.meta.accepts_assets {
                    self.addon_hub_tab = AddonHubTab::Settings;
                }

                ui.horizontal(|ui| {
                    ui.label(RichText::new(state.meta.config_path.display().to_string()).small().color(Color32::GRAY));
                    if let Some(schema) = &state.schema {
                        if let Some(ver) = &schema.version {
                            ui.label(RichText::new(format!("schema {}", ver)).small().color(Color32::LIGHT_BLUE));
                        }
                    }
                });
                ui.add_space(6.0);

                if !state.custom_tabs.is_empty() {
                    render_custom_hub_tabs(ui, &state.custom_tabs, &mut self.selected_custom_tab);
                } else {
                    Self::render_hub_tabs(ui, &mut self.addon_hub_tab, state.meta.accepts_assets);
                }
                ui.add_space(8.0);
                ui.separator();
                ui.add_space(8.0);

                let before_render = serde_yaml::to_string(&state.root).ok();

                egui::ScrollArea::vertical().show(ui, |ui| {
                    if !state.custom_tabs.is_empty() {
                        render_selected_custom_tab(
                            ui,
                            &state.meta,
                            &state.custom_tabs,
                            &mut self.selected_custom_tab,
                            &mut self.last_opened_custom_tab,
                            &mut self.global_status,
                        );
                    } else {
                        match self.addon_hub_tab {
                            AddonHubTab::Library => self.render_library_tab(ui, &mut state),
                            AddonHubTab::Editor => self.render_editor_tab(ui, &mut state),
                            AddonHubTab::Discover => self.render_discover_tab(ui, &mut state),
                            AddonHubTab::Settings => self.render_settings_tab(ui, &mut state),
                        }
                    }
                });

                let after_render = serde_yaml::to_string(&state.root).ok();
                if before_render != after_render {
                    match save_addon_state(&mut state) {
                        Ok(_) => {
                            state.status = "Live saved config.yaml".to_string();
                            self.global_status = "Live saved addon config".to_string();
                        }
                        Err(e) => {
                            state.status = format!("Live save failed: {}", e);
                            self.global_status = "Live save failed".to_string();
                            error!("Config UI live save failed: {}", e);
                        }
                    }
                }

                ui.add_space(10.0);
                if ui.button("Reload").clicked() {
                    match load_addon_state(state.meta.clone()) {
                        Ok(new_state) => {
                            state = new_state;
                            self.global_status = "Reloaded addon config".to_string();
                        }
                        Err(e) => {
                            state.status = format!("Reload failed: {}", e);
                            self.global_status = "Reload failed".to_string();
                        }
                    }
                }
                ui.label(&state.status);

                self.addon_state = Some(state);
            }
        });
    }

    fn render_library_tab(&mut self, ui: &mut egui::Ui, state: &mut AddonConfigState) {
        if render_addon_custom_tab_page(ui, &state.meta, "library") {
            return;
        }

        ui.label(RichText::new("Enabled assets and assignments").strong());
        ui.add_space(4.0);

        let selector_values = read_asset_selector_values(&state.root, &state.asset_selector_paths);
        if selector_values.is_empty() {
            ui.label("No asset selector fields found in config/schema.");
        } else {
            for (path, value) in selector_values {
                ui.label(format!("{} -> {}", path, value));
            }
        }

        let monitors = MonitorManager::enumerate_monitors();
        let selected_monitor = self.library_selected_monitor.clone().unwrap_or_else(|| {
            monitors
                .iter()
                .find(|m| m.primary)
                .map(|m| m.id.clone())
                .or_else(|| monitors.first().map(|m| m.id.clone()))
                .unwrap_or_else(|| "*".to_string())
        });
        self.library_selected_monitor = Some(selected_monitor.clone());

        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Assign target:").strong());
            if ui
                .selectable_label(selected_monitor == "*", "All Monitors")
                .clicked()
            {
                self.library_selected_monitor = Some("*".to_string());
            }

            for monitor in &monitors {
                let label = if monitor.primary {
                    format!("Primary {}x{}", monitor.width, monitor.height)
                } else {
                    format!("{}x{}", monitor.width, monitor.height)
                };
                if ui
                    .selectable_label(selected_monitor == monitor.id, label)
                    .clicked()
                {
                    self.library_selected_monitor = Some(monitor.id.clone());
                }
            }
        });

        ui.add_space(6.0);
        render_monitor_layout_preview(ui, &monitors, &state.root, &state.assets, self.library_selected_monitor.as_deref());

        ui.add_space(8.0);
        if let Some(chosen_id) = render_asset_cards(ui, &state.assets, &mut self.caches, &self.editor_selected_asset, true) {
            self.editor_selected_asset = Some(chosen_id.clone());
            let monitor_key = self
                .library_selected_monitor
                .clone()
                .unwrap_or_else(|| "*".to_string());
            apply_asset_assignment_to_monitor(&mut state.root, &monitor_key, &chosen_id);
        }
    }

    fn render_editor_tab(&mut self, ui: &mut egui::Ui, state: &mut AddonConfigState) {
        if render_addon_custom_tab_page(ui, &state.meta, "editor") {
            return;
        }

        ui.label(RichText::new("Editor").strong());
        ui.add_space(6.0);

        if state.assets.is_empty() {
            ui.label("No assets discovered for this addon.");
            return;
        }

        if self.editor_selected_asset.is_none() {
            self.editor_selected_asset = Some(state.assets[0].id.clone());
        }

        ui.horizontal_wrapped(|ui| {
            for asset in &state.assets {
                let selected = self
                    .editor_selected_asset
                    .as_ref()
                    .map(|v| v == &asset.id)
                    .unwrap_or(false);
                if ui.selectable_label(selected, &asset.name).clicked() {
                    self.editor_selected_asset = Some(asset.id.clone());
                }
            }
        });

        ui.add_space(8.0);

        let selected_id = match &self.editor_selected_asset {
            Some(v) => v.clone(),
            None => return,
        };

        if let Some(asset) = state.assets.iter().find(|a| a.id == selected_id) {
            render_asset_detail(ui, asset, &mut self.caches);

            ui.add_space(10.0);
            ui.label(RichText::new("Apply asset").strong());
            if ui.button("Set as active").clicked() {
                for selector_path in &state.asset_selector_paths {
                    if let Some(v) = get_node_mut(&mut state.root, selector_path) {
                        *v = Value::String(asset.id.clone());
                    }
                }
            }

            if state.meta.id.to_lowercase().contains("wallpaper") {
                ui.add_space(10.0);
                ui.label(RichText::new("Wallpaper editable properties").strong());
                render_editable_values(ui, &asset.id, &asset.editable, &mut state.root);
            }
        }
    }

    fn render_discover_tab(&mut self, ui: &mut egui::Ui, state: &mut AddonConfigState) {
        if render_addon_custom_tab_page(ui, &state.meta, "discover") {
            return;
        }

        ui.label(RichText::new("Discover").strong());
        ui.add_space(6.0);

        if state.assets.is_empty() {
            ui.label("No assets discovered for this addon.");
            return;
        }

        if let Some(chosen_id) = render_asset_cards(ui, &state.assets, &mut self.caches, &self.editor_selected_asset, true) {
            self.editor_selected_asset = Some(chosen_id);
            self.addon_hub_tab = AddonHubTab::Editor;
        }
    }

    fn render_settings_tab(&mut self, ui: &mut egui::Ui, state: &mut AddonConfigState) {
        if render_addon_custom_tab_page(ui, &state.meta, "settings") {
            return;
        }

        let mut open_library_requested = false;
        if let Some(schema) = &state.schema {
            if !schema.ui.sections.is_empty() {
                for section in &schema.ui.sections {
                    render_schema_section(
                        ui,
                        &mut state.root,
                        section,
                        &state.meta,
                        &state.assets,
                        &mut self.caches,
                        0,
                        &mut open_library_requested,
                    );
                    ui.add_space(8.0);
                }
            } else {
                render_raw_fallback(ui, &mut state.root);
            }
        } else {
            render_raw_fallback(ui, &mut state.root);
        }

        if open_library_requested && state.meta.accepts_assets {
            self.addon_hub_tab = AddonHubTab::Library;
        }
    }
}

impl App for SentinelApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.sidebar(ctx);
        egui::CentralPanel::default().show(ctx, |ui| match self.section {
            UiSection::Home => self.show_home(ui),
            UiSection::Addons => self.show_addons(ui),
            UiSection::Integrations => self.show_integrations(ui),
            UiSection::Settings => self.show_settings(ui),
        });
    }
}

fn render_schema_section(
    ui: &mut egui::Ui,
    root: &mut Value,
    section: &SchemaSection,
    meta: &AddonMeta,
    assets: &[AssetOption],
    caches: &mut UiCaches,
    depth: usize,
    open_library_requested: &mut bool,
) {
    let path_segments = split_path(section.path.as_deref().unwrap_or_default());
    let stroke_color = match depth % 3 {
        0 => Color32::from_rgb(70, 122, 194),
        1 => Color32::from_rgb(84, 160, 120),
        _ => Color32::from_rgb(170, 122, 84),
    };

    egui::Frame::default()
        .stroke(Stroke::new(1.0, stroke_color))
        .fill(Color32::from_rgb(18, 20, 26))
        .corner_radius(5.0)
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            egui::CollapsingHeader::new(RichText::new(&section.title).strong())
                .default_open(depth < 2)
                .show(ui, |ui| {
                    if let Some(desc) = &section.description {
                        ui.label(RichText::new(desc).small().color(Color32::GRAY));
                        ui.add_space(4.0);
                    }

                    if section
                        .render_mode
                        .as_deref()
                        .map(|m| m.eq_ignore_ascii_case("map_cards"))
                        .unwrap_or(false)
                    {
                        render_map_cards(ui, root, &path_segments, section, meta, assets, caches, depth + 1, open_library_requested);
                    } else {
                        render_normal_section(ui, root, &path_segments, section, meta, assets, caches, depth + 1, open_library_requested);
                    }
                });
        });
}

fn render_normal_section(
    ui: &mut egui::Ui,
    root: &mut Value,
    section_path: &[String],
    section: &SchemaSection,
    meta: &AddonMeta,
    assets: &[AssetOption],
    caches: &mut UiCaches,
    depth: usize,
    open_library_requested: &mut bool,
) {
    let Some(target) = get_node_mut(root, section_path) else {
        ui.label(RichText::new("Section path not found in config").color(Color32::RED));
        return;
    };

    for field in &section.fields {
        render_schema_field(ui, target, field, meta, assets, caches, open_library_requested);
    }

    for nested in &section.sections {
        render_nested_section(ui, target, nested, meta, assets, caches, depth, open_library_requested);
        ui.add_space(6.0);
    }
}

fn render_nested_section(
    ui: &mut egui::Ui,
    current_node: &mut Value,
    section: &SchemaSection,
    meta: &AddonMeta,
    assets: &[AssetOption],
    caches: &mut UiCaches,
    depth: usize,
    open_library_requested: &mut bool,
) {
    let nested_path = split_path(section.path.as_deref().unwrap_or_default());
    let stroke_color = match depth % 3 {
        0 => Color32::from_rgb(70, 122, 194),
        1 => Color32::from_rgb(84, 160, 120),
        _ => Color32::from_rgb(170, 122, 84),
    };

    egui::Frame::default()
        .stroke(Stroke::new(1.0, stroke_color))
        .fill(Color32::from_rgb(18, 20, 26))
        .corner_radius(5.0)
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            egui::CollapsingHeader::new(RichText::new(&section.title).strong())
                .default_open(depth < 2)
                .show(ui, |ui| {
                    if let Some(desc) = &section.description {
                        ui.label(RichText::new(desc).small().color(Color32::GRAY));
                        ui.add_space(4.0);
                    }

                    if section
                        .render_mode
                        .as_deref()
                        .map(|m| m.eq_ignore_ascii_case("map_cards"))
                        .unwrap_or(false)
                    {
                        render_map_cards_on_node(ui, current_node, &nested_path, section, meta, assets, caches, depth + 1, open_library_requested);
                    } else {
                        let Some(target) = get_node_mut(current_node, &nested_path) else {
                            ui.label(RichText::new("Section path not found in config").color(Color32::RED));
                            return;
                        };

                        for field in &section.fields {
                            render_schema_field(ui, target, field, meta, assets, caches, open_library_requested);
                        }

                        for nested in &section.sections {
                            render_nested_section(ui, target, nested, meta, assets, caches, depth + 1, open_library_requested);
                            ui.add_space(6.0);
                        }
                    }
                });
        });
}

fn render_map_cards(
    ui: &mut egui::Ui,
    root: &mut Value,
    map_path: &[String],
    section: &SchemaSection,
    meta: &AddonMeta,
    assets: &[AssetOption],
    caches: &mut UiCaches,
    depth: usize,
    open_library_requested: &mut bool,
) {
    let Some(target) = get_node_mut(root, map_path) else {
        ui.label(RichText::new("Map section path not found").color(Color32::RED));
        return;
    };

    render_map_cards_target(ui, target, section, meta, assets, caches, depth, open_library_requested);
}

fn render_map_cards_on_node(
    ui: &mut egui::Ui,
    current_node: &mut Value,
    map_path: &[String],
    section: &SchemaSection,
    meta: &AddonMeta,
    assets: &[AssetOption],
    caches: &mut UiCaches,
    depth: usize,
    open_library_requested: &mut bool,
) {
    let Some(target) = get_node_mut(current_node, map_path) else {
        ui.label(RichText::new("Map section path not found").color(Color32::RED));
        return;
    };

    render_map_cards_target(ui, target, section, meta, assets, caches, depth, open_library_requested);
}

fn render_map_cards_target(
    ui: &mut egui::Ui,
    target: &mut Value,
    section: &SchemaSection,
    meta: &AddonMeta,
    assets: &[AssetOption],
    caches: &mut UiCaches,
    depth: usize,
    open_library_requested: &mut bool,
) {
    let Value::Mapping(map) = target else {
        ui.label(RichText::new("Map section is not a mapping").color(Color32::RED));
        return;
    };

    for (item_key, item_value) in map.iter_mut() {
        let item_name = item_key
            .as_str()
            .map(pretty_label)
            .unwrap_or_else(|| "Item".to_string());

        let stroke_color = match depth % 3 {
            0 => Color32::from_rgb(70, 122, 194),
            1 => Color32::from_rgb(84, 160, 120),
            _ => Color32::from_rgb(170, 122, 84),
        };

        egui::Frame::default()
            .stroke(Stroke::new(1.0, stroke_color))
            .fill(Color32::from_rgb(16, 18, 24))
            .corner_radius(5.0)
            .inner_margin(egui::Margin::same(8))
            .show(ui, |ui| {
                ui.label(RichText::new(item_name).strong());
                ui.add_space(4.0);
                for field in &section.fields {
                    render_schema_field(ui, item_value, field, meta, assets, caches, open_library_requested);
                }
            });
        ui.add_space(6.0);
    }
}

fn render_schema_field(
    ui: &mut egui::Ui,
    target_node: &mut Value,
    field: &SchemaField,
    meta: &AddonMeta,
    assets: &[AssetOption],
    caches: &mut UiCaches,
    open_library_requested: &mut bool,
) {
    let path_segments = split_path(&field.path);
    if path_segments.is_empty() {
        return;
    }

    let field_label = field
        .label
        .clone()
        .unwrap_or_else(|| pretty_label(path_segments.last().map(|s| s.as_str()).unwrap_or_default()));

    let Some(value) = get_node_mut(target_node, &path_segments) else {
        ui.horizontal(|ui| {
            ui.label(RichText::new(field_label).strong());
            ui.label(RichText::new("(missing path)").color(Color32::RED));
        });
        return;
    };

    ui.horizontal(|ui| {
        ui.set_min_width(320.0);
        ui.label(RichText::new(&field_label).strong());

        match field.control.as_str() {
            "toggle" => {
                if let Value::Bool(v) = value {
                    ui.toggle_value(v, if *v { "True" } else { "False" });
                } else {
                    ui.label(RichText::new("Expected bool").color(Color32::RED));
                }
            }
            "number_range" => render_number_range(ui, value, field.min, field.max, field.step),
            "dropdown" => render_dropdown(ui, value, &field.options),
            "text_list" => render_text_list(ui, value),
            "asset_selector" => render_asset_selector(ui, value, field, meta, assets, caches, open_library_requested),
            _ => render_text_value(ui, value),
        }
    });

    if let Some(desc) = &field.description {
        ui.label(RichText::new(desc).small().color(Color32::GRAY));
    }
    ui.add_space(4.0);
}

fn render_asset_selector(
    ui: &mut egui::Ui,
    value: &mut Value,
    field: &SchemaField,
    meta: &AddonMeta,
    assets: &[AssetOption],
    caches: &mut UiCaches,
    open_library_requested: &mut bool,
) {
    if !meta.accepts_assets {
        ui.label(RichText::new("Addon does not accept assets").color(Color32::GRAY));
        return;
    }

    let selected_id = match value {
        Value::String(v) => v.clone(),
        _ => {
            ui.label(RichText::new("Expected string asset id").color(Color32::RED));
            return;
        }
    };

    let selected_asset = assets.iter().find(|a| a.id == selected_id);
    let selected_label = selected_asset
        .map(|a| a.name.clone())
        .unwrap_or_else(|| selected_id.clone());

    ui.vertical(|ui| {
        ui.label(selected_label);
        if ui.button("Open Asset Library").clicked() {
            *open_library_requested = true;
        }

        if field.show_preview.unwrap_or(false) {
            if let Some(asset) = selected_asset {
                if let Some(path) = pick_preview_path(asset, caches) {
                    if let Some(texture) = load_preview_texture(ui.ctx(), &path, caches) {
                        ui.image((texture.id(), egui::vec2(220.0, 124.0)));
                    }
                }
            }
        }
    });
}

fn render_number_range(
    ui: &mut egui::Ui,
    value: &mut Value,
    min: Option<f64>,
    max: Option<f64>,
    step: Option<f64>,
) {
    match value {
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                let mut val = i as f64;
                let mut slider = egui::Slider::new(&mut val, min.unwrap_or(-100_000.0)..=max.unwrap_or(100_000.0));
                slider = slider.step_by(step.unwrap_or(1.0));
                if ui.add(slider).changed() {
                    *value = Value::Number((val.round() as i64).into());
                }
            } else if let Some(f) = n.as_f64() {
                let mut val = f;
                let mut slider = egui::Slider::new(&mut val, min.unwrap_or(-100_000.0)..=max.unwrap_or(100_000.0));
                slider = slider.step_by(step.unwrap_or(0.1));
                if ui.add(slider).changed() {
                    *value = serde_yaml::to_value(val).unwrap_or(Value::Null);
                }
            }
        }
        _ => {
            ui.label(RichText::new("Expected number").color(Color32::RED));
        }
    }
}

fn render_dropdown(ui: &mut egui::Ui, value: &mut Value, options: &[String]) {
    match value {
        Value::String(s) => {
            let mut opts = options.to_vec();
            if !opts.iter().any(|o| o == s) {
                opts.insert(0, s.clone());
            }

            egui::ComboBox::from_id_salt(format!("dropdown:{}", s))
                .selected_text(s.as_str())
                .width(220.0)
                .show_ui(ui, |ui| {
                    for option in opts {
                        ui.selectable_value(s, option.clone(), option);
                    }
                });
        }
        _ => {
            ui.label(RichText::new("Expected string").color(Color32::RED));
        }
    }
}

fn render_text_list(ui: &mut egui::Ui, value: &mut Value) {
    match value {
        Value::Sequence(seq) => {
            let mut joined = seq
                .iter()
                .filter_map(|v| match v {
                    Value::String(s) => Some(s.clone()),
                    Value::Number(n) => Some(n.to_string()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(", ");

            if ui.text_edit_singleline(&mut joined).changed() {
                *value = Value::Sequence(
                    joined
                        .split(',')
                        .map(|s| s.trim())
                        .filter(|s| !s.is_empty())
                        .map(|s| Value::String(s.to_string()))
                        .collect::<Vec<_>>(),
                );
            }
        }
        Value::String(s) => {
            ui.text_edit_singleline(s);
        }
        _ => {
            ui.label(RichText::new("Expected list or string").color(Color32::RED));
        }
    }
}

fn render_text_value(ui: &mut egui::Ui, value: &mut Value) {
    match value {
        Value::String(s) => {
            ui.text_edit_singleline(s);
        }
        Value::Bool(v) => {
            ui.toggle_value(v, if *v { "True" } else { "False" });
        }
        Value::Number(n) => {
            ui.label(n.to_string());
        }
        Value::Null => {
            let mut as_text = "null".to_string();
            if ui.text_edit_singleline(&mut as_text).changed() && !as_text.eq_ignore_ascii_case("null") {
                *value = Value::String(as_text);
            }
        }
        _ => {
            ui.label(RichText::new("Unsupported field type").color(Color32::RED));
        }
    }
}

fn render_asset_cards(
    ui: &mut egui::Ui,
    assets: &[AssetOption],
    caches: &mut UiCaches,
    selected_asset: &Option<String>,
    allow_click_select: bool,
) -> Option<String> {
    if assets.is_empty() {
        ui.label("No assets discovered.");
        return None;
    }

    let mut clicked: Option<String> = None;

    for asset in assets {
        let selected = selected_asset
            .as_ref()
            .map(|id| id == &asset.id)
            .unwrap_or(false);

        let frame = egui::Frame::default()
            .stroke(Stroke::new(
                if selected { 2.0 } else { 1.0 },
                if selected {
                    Color32::from_rgb(72, 170, 255)
                } else {
                    Color32::from_rgb(68, 85, 110)
                },
            ))
            .fill(if selected {
                Color32::from_rgb(20, 34, 50)
            } else {
                Color32::from_rgb(18, 22, 30)
            })
            .corner_radius(6.0)
            .inner_margin(egui::Margin::same(12));

        let response = frame.show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new(&asset.name).strong());
                        ui.label(RichText::new(&asset.id).small().color(Color32::GRAY));
                        if let Some(v) = &asset.version {
                            ui.label(RichText::new(format!("v{}", v)).small());
                        }
                        if !asset.tags.is_empty() {
                            ui.label(RichText::new(format!("Tags: {}", asset.tags.join(", "))).small());
                        }
                        if let Some(sd) = &asset.short_description {
                            ui.label(sd);
                        }
                    });

                    ui.add_space(10.0);
                    if let Some(path) = pick_preview_path(asset, caches) {
                        if let Some(texture) = load_preview_texture(ui.ctx(), &path, caches) {
                            ui.image((texture.id(), egui::vec2(250.0, 140.0)));
                        }
                    }
                });

                if let Some(ld) = &asset.long_description {
                    ui.label(ld);
                }
                if let Some(date) = &asset.last_updated {
                    ui.label(RichText::new(format!("Last updated: {}", date)).small().color(Color32::GRAY));
                }

                if !asset.authors.is_empty() {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new("Authors:").strong());
                        for (name, url) in &asset.authors {
                            ui.hyperlink_to(name, url);
                        }
                    });
                }
            });

        if allow_click_select && response.response.clicked() {
            clicked = Some(asset.id.clone());
        }
        ui.add_space(8.0);
    }

    clicked
}

fn render_asset_detail(ui: &mut egui::Ui, asset: &AssetOption, caches: &mut UiCaches) {
    ui.label(RichText::new(&asset.name).strong().size(18.0));
    ui.label(RichText::new(&asset.id).small().color(Color32::GRAY));

    ui.horizontal(|ui| {
        if ui.button("Prev").clicked() {
            cycle_preview(asset, caches, false);
        }
        if ui.button("Next").clicked() {
            cycle_preview(asset, caches, true);
        }
    });

    if let Some(path) = pick_preview_path(asset, caches) {
        if let Some(texture) = load_preview_texture(ui.ctx(), &path, caches) {
            ui.image((texture.id(), egui::vec2(760.0, 420.0)));
        }
    }

    if let Some(sd) = &asset.short_description {
        ui.label(sd);
    }
    if let Some(ld) = &asset.long_description {
        ui.label(ld);
    }

    ui.label(RichText::new(format!("Manifest: {}", asset.manifest_path.display())).small().color(Color32::GRAY));
}

fn render_editable_values(ui: &mut egui::Ui, asset_id: &str, editable: &JsonValue, root: &mut Value) {
    let Some(obj) = editable.as_object() else {
        ui.label(RichText::new("No editable fields defined in manifest").small().color(Color32::GRAY));
        return;
    };

    for (key, val) in obj {
        ui.horizontal(|ui| {
            ui.label(RichText::new(pretty_label(key)).strong());
            let store_path = split_path(&format!("wallpaper.asset_props.{}.{}", asset_id, key));
            ensure_node_path(root, &store_path, json_to_yaml_scalar(val));
            if let Some(current) = get_node_mut(root, &store_path) {
                render_text_value(ui, current);
            }
        });
    }
}

fn json_to_yaml_scalar(v: &JsonValue) -> Value {
    match v {
        JsonValue::Bool(b) => Value::Bool(*b),
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Number(i.into())
            } else if let Some(f) = n.as_f64() {
                serde_yaml::to_value(f).unwrap_or(Value::Null)
            } else {
                Value::Null
            }
        }
        JsonValue::String(s) => Value::String(s.clone()),
        _ => Value::Null,
    }
}

fn ensure_node_path(root: &mut Value, path: &[String], default_value: Value) {
    if path.is_empty() {
        return;
    }

    let mut current = root;
    for (idx, segment) in path.iter().enumerate() {
        let is_last = idx == path.len() - 1;
        let key = Value::String(segment.clone());

        let needs_mapping = !matches!(current, Value::Mapping(_));
        if needs_mapping {
            *current = Value::Mapping(Mapping::new());
        }

        let map = match current {
            Value::Mapping(m) => m,
            _ => unreachable!(),
        };

        if is_last {
            map.entry(key).or_insert(default_value.clone());
            return;
        } else {
            current = map.entry(key).or_insert_with(|| Value::Mapping(Mapping::new()));
        }
    }
}

fn cycle_preview(asset: &AssetOption, caches: &mut UiCaches, forward: bool) {
    if asset.preview_paths.is_empty() {
        return;
    }

    let idx = caches.preview_index.entry(asset.id.clone()).or_insert(0);
    if forward {
        *idx = (*idx + 1) % asset.preview_paths.len();
    } else if *idx == 0 {
        *idx = asset.preview_paths.len() - 1;
    } else {
        *idx -= 1;
    }
}

fn pick_preview_path(asset: &AssetOption, caches: &UiCaches) -> Option<PathBuf> {
    if asset.preview_paths.is_empty() {
        return None;
    }
    let idx = caches.preview_index.get(&asset.id).copied().unwrap_or(0).min(asset.preview_paths.len() - 1);
    Some(asset.preview_paths[idx].clone())
}

fn load_preview_texture(ctx: &egui::Context, path: &Path, caches: &mut UiCaches) -> Option<TextureHandle> {
    let key = path.to_string_lossy().to_string();
    if !caches.preview_textures.contains_key(&key) {
        let image = image::open(path).ok()?.into_rgba8();
        let size = [image.width() as usize, image.height() as usize];
        let color_image = egui::ColorImage::from_rgba_unmultiplied(size, &image.into_raw());
        let texture = ctx.load_texture(key.clone(), color_image, TextureOptions::LINEAR);
        caches.preview_textures.insert(key.clone(), texture);
    }
    caches.preview_textures.get(&key).cloned()
}

fn read_asset_selector_values(root: &Value, selector_paths: &[Vec<String>]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for path in selector_paths {
        if let Some(v) = get_node(root, path) {
            if let Value::String(s) = v {
                out.push((path.join("."), s.clone()));
            }
        }
    }
    out
}

fn apply_asset_assignment_to_monitor(root: &mut Value, monitor_key: &str, asset_id: &str) {
    let assignments_path = split_path("wallpaper.assignments");
    ensure_node_path(root, &assignments_path, Value::Mapping(Mapping::new()));

    let assignment_entry_path = split_path(&format!("wallpaper.assignments.{}", monitor_key));
    ensure_node_path(root, &assignment_entry_path, Value::String(asset_id.to_string()));
    if let Some(v) = get_node_mut(root, &assignment_entry_path) {
        *v = Value::String(asset_id.to_string());
    }

    if let Some(v) = get_node_mut(root, &split_path("wallpaper.wallpaper_id")) {
        *v = Value::String(asset_id.to_string());
    }

    let monitor_index_path = split_path("wallpaper.monitor_index");
    ensure_node_path(root, &monitor_index_path, Value::Sequence(vec![]));
    if let Some(Value::Sequence(seq)) = get_node_mut(root, &monitor_index_path) {
        let exists = seq.iter().any(|v| v.as_str().map(|s| s == monitor_key).unwrap_or(false));
        if !exists {
            seq.push(Value::String(monitor_key.to_string()));
        }
    }
}

fn get_assigned_asset_for_monitor(root: &Value, monitor_key: &str) -> Option<String> {
    let assignment_entry_path = split_path(&format!("wallpaper.assignments.{}", monitor_key));
    if let Some(Value::String(v)) = get_node(root, &assignment_entry_path) {
        return Some(v.clone());
    }

    if let Some(Value::String(v)) = get_node(root, &split_path("wallpaper.assignments.*")) {
        return Some(v.clone());
    }

    if let Some(Value::String(v)) = get_node(root, &split_path("wallpaper.wallpaper_id")) {
        return Some(v.clone());
    }

    None
}

fn render_monitor_layout_preview(
    ui: &mut egui::Ui,
    monitors: &[MonitorInfo],
    root: &Value,
    assets: &[AssetOption],
    selected_monitor: Option<&str>,
) {
    if monitors.is_empty() {
        ui.label("No monitor data available");
        return;
    }

    ui.label(RichText::new("Monitor Layout Preview").strong());
    let desired_size = egui::vec2(ui.available_width().min(820.0), 240.0);
    let (rect, _) = ui.allocate_exact_size(desired_size, egui::Sense::hover());
    let painter = ui.painter_at(rect);

    painter.rect_filled(rect, 6.0, Color32::from_rgb(10, 14, 20));

    let min_x = monitors.iter().map(|m| m.x).min().unwrap_or(0) as f32;
    let min_y = monitors.iter().map(|m| m.y).min().unwrap_or(0) as f32;
    let max_x = monitors.iter().map(|m| m.x + m.width).max().unwrap_or(1) as f32;
    let max_y = monitors.iter().map(|m| m.y + m.height).max().unwrap_or(1) as f32;

    let total_w = (max_x - min_x).max(1.0);
    let total_h = (max_y - min_y).max(1.0);

    let pad = 10.0;
    let scale = ((rect.width() - pad * 2.0) / total_w).min((rect.height() - pad * 2.0) / total_h);

    for monitor in monitors {
        let left = rect.left() + pad + ((monitor.x as f32 - min_x) * scale);
        let top = rect.top() + pad + ((monitor.y as f32 - min_y) * scale);
        let w = (monitor.width as f32 * scale).max(40.0);
        let h = (monitor.height as f32 * scale).max(30.0);
        let mrect = egui::Rect::from_min_size(egui::pos2(left, top), egui::vec2(w, h));

        let selected = selected_monitor.map(|id| id == monitor.id).unwrap_or(false);
        painter.rect_filled(
            mrect,
            4.0,
            if selected {
                Color32::from_rgb(36, 68, 100)
            } else {
                Color32::from_rgb(24, 28, 38)
            },
        );
        painter.rect_stroke(
            mrect,
            4.0,
            Stroke::new(
                if selected { 2.0 } else { 1.0 },
                if selected {
                    Color32::from_rgb(72, 170, 255)
                } else {
                    Color32::from_rgb(95, 105, 125)
                },
            ),
            egui::StrokeKind::Outside,
        );

        let monitor_label = if monitor.primary {
            "Primary"
        } else {
            "Monitor"
        };
        painter.text(
            mrect.left_top() + egui::vec2(6.0, 6.0),
            egui::Align2::LEFT_TOP,
            format!("{} {}x{}", monitor_label, monitor.width, monitor.height),
            egui::FontId::proportional(11.0),
            Color32::WHITE,
        );

        let assigned_id = get_assigned_asset_for_monitor(root, &monitor.id)
            .or_else(|| get_assigned_asset_for_monitor(root, "*"))
            .unwrap_or_else(|| "none".to_string());
        let assigned_name = assets
            .iter()
            .find(|a| a.id == assigned_id)
            .map(|a| a.name.clone())
            .unwrap_or(assigned_id);

        painter.text(
            mrect.left_bottom() - egui::vec2(-6.0, 6.0),
            egui::Align2::LEFT_BOTTOM,
            assigned_name,
            egui::FontId::proportional(11.0),
            Color32::from_rgb(160, 220, 255),
        );
    }
}

fn get_node<'a>(root: &'a Value, path: &[String]) -> Option<&'a Value> {
    if path.is_empty() {
        return Some(root);
    }

    let mut current = root;
    for segment in path {
        let Value::Mapping(map) = current else {
            return None;
        };
        current = map.get(Value::String(segment.clone()))?;
    }
    Some(current)
}

fn render_raw_fallback(ui: &mut egui::Ui, root: &mut Value) {
    ui.label(RichText::new("No schema.yaml found. Showing fallback editor.").small().color(Color32::GRAY));
    ui.add_space(6.0);
    let mut path = Vec::new();
    render_yaml_node_fallback(ui, root, &mut path, 0);
}

fn render_yaml_node_fallback(ui: &mut egui::Ui, node: &mut Value, path: &mut Vec<String>, depth: usize) {
    match node {
        Value::Mapping(map) => {
            for (key, value) in map.iter_mut() {
                let key_name = match key.as_str() {
                    Some(k) => k.to_string(),
                    None => continue,
                };

                path.push(key_name.clone());
                let display_key = pretty_label(&key_name);

                match value {
                    Value::Mapping(_) | Value::Sequence(_) => {
                        let stroke_color = match depth % 3 {
                            0 => Color32::from_rgb(70, 122, 194),
                            1 => Color32::from_rgb(84, 160, 120),
                            _ => Color32::from_rgb(170, 122, 84),
                        };

                        egui::Frame::default()
                            .stroke(Stroke::new(1.0, stroke_color))
                            .fill(Color32::from_rgb(18, 20, 26))
                            .corner_radius(5.0)
                            .inner_margin(egui::Margin::same(8))
                            .show(ui, |ui| {
                                egui::CollapsingHeader::new(RichText::new(display_key).strong())
                                    .default_open(depth < 2)
                                    .show(ui, |ui| {
                                        ui.add_space(4.0);
                                        render_yaml_node_fallback(ui, value, path, depth + 1);
                                    });
                            });
                        ui.add_space(6.0);
                    }
                    _ => {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(display_key).strong());
                            render_text_value(ui, value);
                        });
                        ui.add_space(4.0);
                    }
                }

                path.pop();
            }
        }
        Value::Sequence(seq) => {
            for (idx, item) in seq.iter_mut().enumerate() {
                let item_key = format!("Item {}", idx + 1);
                path.push(idx.to_string());
                egui::Frame::default()
                    .stroke(Stroke::new(1.0, Color32::from_rgb(80, 80, 95)))
                    .fill(Color32::from_rgb(18, 20, 26))
                    .corner_radius(5.0)
                    .inner_margin(egui::Margin::same(8))
                    .show(ui, |ui| {
                        egui::CollapsingHeader::new(RichText::new(item_key).strong())
                            .default_open(depth < 2)
                            .show(ui, |ui| render_yaml_node_fallback(ui, item, path, depth + 1));
                    });
                path.pop();
                ui.add_space(6.0);
            }
        }
        _ => {
            ui.horizontal(|ui| {
                ui.label("Value");
                render_text_value(ui, node);
            });
        }
    }
}

fn split_path(path: &str) -> Vec<String> {
    path.split('.')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

fn get_node_mut<'a>(root: &'a mut Value, path: &[String]) -> Option<&'a mut Value> {
    if path.is_empty() {
        return Some(root);
    }

    let mut current = root;
    for segment in path {
        let Value::Mapping(map) = current else {
            return None;
        };
        current = map.get_mut(Value::String(segment.clone()))?;
    }
    Some(current)
}

fn pretty_label(raw: &str) -> String {
    raw.replace(['-', '_'], " ")
        .split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn addon_custom_tab_path(meta: &AddonMeta, tab_name: &str) -> PathBuf {
    meta.addon_root
        .join("options")
        .join(format!("{}.html", tab_name.to_lowercase()))
}

fn open_local_html(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Err(format!("Tab page not found: {}", path.display()));
    }

    if cfg!(target_os = "windows") {
        std::process::Command::new("explorer")
            .arg(path)
            .spawn()
            .map_err(|e| format!("Failed to open tab page '{}': {}", path.display(), e))?;
        Ok(())
    } else {
        Err("Custom HTML page opening is currently implemented for Windows only".to_string())
    }
}

fn file_path_to_url(path: &Path) -> Result<String, String> {
    let canonical = std::fs::canonicalize(path)
        .map_err(|e| format!("Failed to resolve path '{}': {}", path.display(), e))?;

    let mut raw = canonical.to_string_lossy().to_string();
    if let Some(stripped) = raw.strip_prefix(r"\\?\") {
        raw = stripped.to_string();
    }

    if raw.starts_with(r"\\") {
        let unc = raw.trim_start_matches('\\').replace('\\', "/").replace(' ', "%20");
        return Ok(format!("file://{}", unc));
    }

    let normalized = raw.replace('\\', "/").replace(' ', "%20");
    Ok(format!("file:///{}", normalized))
}

pub fn run_standalone_webview(path: &str, title: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    let page_path = PathBuf::from(path);
    if !page_path.exists() {
        return Err(format!("Tab page not found: {}", page_path.display()).into());
    }

    let url = file_path_to_url(&page_path)?;
    let window_title = title.unwrap_or("Sentinel").to_string();
    info!(
        "[ui] Launching standalone addon webview: title='{}', page='{}'",
        window_title,
        page_path.display()
    );

    let event_loop = EventLoopBuilder::new().build();
    let window = WindowBuilder::new()
        .with_title(window_title)
        .build(&event_loop)
        .map_err(|e| format!("Failed to create Sentinel addon webview window: {}", e))?;

    let webview = WebViewBuilder::new()
        .with_url(&url)
        .build(&window)
        .map_err(|e| format!("Failed to create Sentinel addon webview: {}", e))?;

    event_loop.run(move |event, _, control_flow| {
        let _keep_alive = &webview;
        *control_flow = ControlFlow::Wait;
        if let Event::WindowEvent {
            event: WindowEvent::CloseRequested,
            ..
        } = event
        {
            *control_flow = ControlFlow::Exit;
        }
    });
}

fn open_in_sentinel_webview(path: &Path, title: String) -> Result<(), String> {
    if !path.exists() {
        return Err(format!("Tab page not found: {}", path.display()));
    }

    let exe = std::env::current_exe()
        .map_err(|e| format!("Failed to resolve Sentinel executable: {}", e))?;

    info!(
        "[ui] Spawning addon webview process: title='{}', page='{}'",
        title,
        path.display()
    );

    std::process::Command::new(exe)
        .arg("--addon-webview")
        .arg(path.display().to_string())
        .arg("--addon-webview-title")
        .arg(title)
        .spawn()
        .map_err(|e| format!("Failed to spawn Sentinel webview process: {}", e))?;

    Ok(())
}

fn render_addon_custom_tab_page(ui: &mut egui::Ui, meta: &AddonMeta, tab_name: &str) -> bool {
    let page_path = addon_custom_tab_path(meta, tab_name);
    if !page_path.exists() {
        return false;
    }

    let title = format!("Addon-designed {} page", pretty_label(tab_name));
    ui.label(RichText::new(title).strong());
    ui.label(RichText::new(page_path.display().to_string()).small().color(Color32::GRAY));
    ui.add_space(6.0);

    if ui.button("Open addon page").clicked() {
        if let Err(e) = open_local_html(&page_path) {
            ui.label(RichText::new(e).color(Color32::RED));
        }
    }

    ui.add_space(4.0);
    ui.label(RichText::new("This tab is fully owned by the addon via options HTML.").small().color(Color32::LIGHT_BLUE));
    true
}

fn discover_custom_tabs(meta: &AddonMeta) -> Vec<CustomTabPage> {
    let mut tabs = Vec::new();
    let options_dir = meta.addon_root.join("options");
    let entries = match std::fs::read_dir(&options_dir) {
        Ok(v) => v,
        Err(_) => return tabs,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let is_html = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("html"))
            .unwrap_or(false);
        if !is_html {
            continue;
        }

        let stem = match path.file_stem().and_then(|s| s.to_str()) {
            Some(v) if !v.is_empty() => v.to_lowercase(),
            _ => continue,
        };

        tabs.push(CustomTabPage {
            id: stem.clone(),
            title: pretty_label(&stem),
            path,
        });
    }

    fn rank(id: &str) -> i32 {
        match id {
            "library" => 0,
            "editor" => 1,
            "discover" => 2,
            "settings" => 3,
            _ => 100,
        }
    }

    tabs.sort_by(|a, b| {
        let ra = rank(&a.id);
        let rb = rank(&b.id);
        ra.cmp(&rb).then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
    });

    tabs
}

fn render_custom_hub_tabs(ui: &mut egui::Ui, tabs: &[CustomTabPage], selected: &mut Option<String>) {
    if tabs.is_empty() {
        return;
    }

    if selected.is_none() {
        *selected = Some(tabs[0].id.clone());
    }

    ui.horizontal_wrapped(|ui| {
        for tab in tabs {
            let is_selected = selected.as_ref().map(|v| v == &tab.id).unwrap_or(false);
            if ui.selectable_label(is_selected, &tab.title).clicked() {
                *selected = Some(tab.id.clone());
            }
        }
    });
}

fn render_selected_custom_tab(
    ui: &mut egui::Ui,
    meta: &AddonMeta,
    tabs: &[CustomTabPage],
    selected: &mut Option<String>,
    last_opened: &mut Option<String>,
    global_status: &mut String,
) {
    if tabs.is_empty() {
        return;
    }

    if selected.is_none() {
        *selected = Some(tabs[0].id.clone());
    }

    let selected_id = match selected {
        Some(v) => v.clone(),
        None => return,
    };

    let tab = tabs.iter().find(|t| t.id == selected_id).unwrap_or(&tabs[0]);
    let tab_key = format!("{}:{}", meta.id, tab.id);
    if last_opened.as_ref() != Some(&tab_key) {
        let title = format!("Sentinel • {} • {}", meta.name, tab.title);
        match open_in_sentinel_webview(&tab.path, title) {
            Ok(_) => {
                *last_opened = Some(tab_key);
                *global_status = format!("Opened {} in Sentinel WebView", tab.title);
            }
            Err(e) => {
                *global_status = e;
            }
        }
    }

    ui.label(RichText::new(format!("Addon-designed {} page", tab.title)).strong());
    ui.label(RichText::new(tab.path.display().to_string()).small().color(Color32::GRAY));
    ui.add_space(6.0);
    ui.label(RichText::new("This tab is rendered by the addon HTML in a Sentinel WebView window.").small().color(Color32::LIGHT_BLUE));
    if ui.button("Reopen tab page").clicked() {
        *last_opened = None;
    }
}

fn save_addon_state(state: &mut AddonConfigState) -> Result<(), String> {
    let serialized = serde_yaml::to_string(&state.root)
        .map_err(|e| format!("Failed to serialize YAML: {}", e))?;
    std::fs::write(&state.meta.config_path, serialized)
        .map_err(|e| format!("Failed to write config file: {}", e))?;
    Ok(())
}

fn load_addon_state(meta: AddonMeta) -> Result<AddonConfigState, Box<dyn std::error::Error>> {
    ensure_config_file_exists(&meta.config_path)?;

    let content = std::fs::read_to_string(&meta.config_path).unwrap_or_else(|_| "{}".to_string());
    let root = serde_yaml::from_str::<Value>(&content).unwrap_or_else(|_| Value::Mapping(Mapping::new()));

    let schema = load_schema(&meta.schema_path);
    let asset_selector_paths = collect_asset_selector_paths(schema.as_ref());
    let assets = discover_assets_for_meta(&meta, schema.as_ref());
    let custom_tabs = discover_custom_tabs(&meta);

    Ok(AddonConfigState {
        meta,
        root,
        schema,
        status: "Live save enabled".to_string(),
        assets,
        asset_selector_paths,
        custom_tabs,
    })
}

fn collect_asset_selector_paths(schema: Option<&AddonSchema>) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    let Some(schema) = schema else { return out; };

    fn walk_section(section: &SchemaSection, base_path: &[String], out: &mut Vec<Vec<String>>) {
        let section_path = section.path.as_deref().map(split_path).unwrap_or_default();
        let mut full_base = base_path.to_vec();
        full_base.extend(section_path);

        for field in &section.fields {
            if field.control.eq_ignore_ascii_case("asset_selector") {
                let mut full = full_base.clone();
                full.extend(split_path(&field.path));
                out.push(full);
            }
        }

        for nested in &section.sections {
            walk_section(nested, &full_base, out);
        }
    }

    for section in &schema.ui.sections {
        walk_section(section, &[], &mut out);
    }
    out
}

fn load_schema(path: &Path) -> Option<AddonSchema> {
    if !path.exists() {
        return None;
    }

    let content = std::fs::read_to_string(path).ok()?;
    if content.trim().is_empty() {
        return None;
    }

    match serde_yaml::from_str::<AddonSchema>(&content) {
        Ok(schema) => Some(schema),
        Err(e) => {
            warn!("Failed to parse schema '{}': {}", path.display(), e);
            None
        }
    }
}

fn ensure_config_file_exists(path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    if !path.exists() {
        std::fs::write(path, "{}\n")?;
        info!("Created missing config file at {}", path.display());
    }
    Ok(())
}

fn discover_addon_configs() -> Vec<AddonMeta> {
    let mut result = Vec::new();

    let home = match std::env::var("USERPROFILE") {
        Ok(v) => v,
        Err(_) => return result,
    };

    let addons_root = PathBuf::from(home).join(".Sentinel").join("Addons");
    let entries = match std::fs::read_dir(&addons_root) {
        Ok(v) => v,
        Err(_) => return result,
    };

    for entry in entries.flatten() {
        let addon_dir = entry.path();
        if !addon_dir.is_dir() {
            continue;
        }

        let addon_json = addon_dir.join("addon.json");
        let parsed = std::fs::read_to_string(&addon_json)
            .ok()
            .and_then(|text| serde_json::from_str::<JsonValue>(&text).ok())
            .unwrap_or(JsonValue::Null);

        let id = parsed
            .get("id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| addon_dir.file_name().and_then(|s| s.to_str()).map(|s| s.to_string()))
            .unwrap_or_else(|| "unknown".to_string());

        let name = parsed
            .get("name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| id.clone());

        let package = parsed
            .get("package")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| name.to_lowercase());

        let accepts_assets = parsed
            .get("accepts_assets")
            .and_then(|v| v.as_bool())
            .or_else(|| parsed.get("assets").and_then(|a| a.get("accepts")).and_then(|v| v.as_bool()))
            .unwrap_or(false);

        let asset_categories = parsed
            .get("asset_categories")
            .and_then(|v| v.as_array())
            .or_else(|| parsed.get("assets").and_then(|a| a.get("categories")).and_then(|v| v.as_array()))
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).map(|s| s.to_string()).collect::<Vec<_>>())
            .unwrap_or_default();

        result.push(AddonMeta {
            id,
            name,
            package,
            addon_root: addon_dir.clone(),
            config_path: addon_dir.join("config.yaml"),
            schema_path: addon_dir.join("schema.yaml"),
            accepts_assets,
            asset_categories,
        });
    }

    result.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    result
}

fn discover_assets_for_meta(meta: &AddonMeta, schema: Option<&AddonSchema>) -> Vec<AssetOption> {
    if !meta.accepts_assets {
        return Vec::new();
    }

    let mut categories: HashSet<String> = meta.asset_categories.iter().cloned().collect();
    if categories.is_empty() {
        categories.insert(meta.package.clone());
        categories.insert("wallpaper".to_string());
        categories.insert("wallpapers".to_string());
    }

    if let Some(schema) = schema {
        fn scan_section(section: &SchemaSection, out: &mut HashSet<String>) {
            for field in &section.fields {
                if field.control.eq_ignore_ascii_case("asset_selector") {
                    if let Some(c) = &field.asset_category {
                        out.insert(c.clone());
                    }
                }
            }
            for nested in &section.sections {
                scan_section(nested, out);
            }
        }

        for sec in &schema.ui.sections {
            scan_section(sec, &mut categories);
        }
    }

    let mut merged = Vec::new();
    let mut seen = HashSet::new();
    for category in categories {
        for asset in discover_assets_for_category(&category) {
            if seen.insert(asset.id.clone()) {
                merged.push(asset);
            }
        }
    }

    merged.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    merged
}

fn discover_assets_for_category(category: &str) -> Vec<AssetOption> {
    let mut result = Vec::new();

    let home = match std::env::var("USERPROFILE") {
        Ok(v) => v,
        Err(_) => return result,
    };

    let assets_root = PathBuf::from(home).join(".Sentinel").join("Assets");
    let category_root = match find_category_dir_case_insensitive(&assets_root, category) {
        Some(p) => p,
        None => return result,
    };

    for entry in walkdir::WalkDir::new(&category_root)
        .min_depth(1)
        .max_depth(4)
        .into_iter()
        .filter_map(Result::ok)
    {
        if entry.file_name().to_string_lossy().to_lowercase() != "manifest.json" {
            continue;
        }

        let manifest_path = entry.path().to_path_buf();
        let manifest_dir = match manifest_path.parent() {
            Some(v) => v,
            None => continue,
        };

        let manifest_text = match std::fs::read_to_string(&manifest_path) {
            Ok(t) => t,
            Err(_) => continue,
        };

        let manifest = parse_json_relaxed(&manifest_text).unwrap_or(JsonValue::Null);

        let id = manifest
            .get("id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| manifest_dir.file_name().and_then(|s| s.to_str()).map(|s| s.to_string()))
            .unwrap_or_else(|| "unknown".to_string());

        let name = manifest
            .get("name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| id.clone());

        let metadata = manifest.get("metadata").cloned().unwrap_or(JsonValue::Null);
        let tags = metadata
            .get("tags")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).map(|s| s.to_string()).collect())
            .unwrap_or_else(Vec::new);

        let short_description = metadata
            .get("short_description")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let long_description = metadata
            .get("long_description")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let last_updated = metadata
            .get("last_updated")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let version = manifest.get("version").and_then(|v| v.as_str()).map(|s| s.to_string());

        let authors = metadata
            .get("author")
            .and_then(|v| v.as_object())
            .map(|obj| {
                obj.iter()
                    .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_string()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(Vec::new);

        let preview_paths = collect_preview_paths(&metadata, manifest_dir);
        let editable = manifest.get("editable").cloned().unwrap_or(JsonValue::Null);

        result.push(AssetOption {
            id,
            name,
            version,
            tags,
            short_description,
            long_description,
            last_updated,
            authors,
            preview_paths,
            manifest_path,
            editable,
        });
    }

    result
}

fn parse_json_relaxed(text: &str) -> Option<JsonValue> {
    serde_json::from_str::<JsonValue>(text)
        .ok()
        .or_else(|| json5::from_str::<JsonValue>(text).ok())
}

fn collect_preview_paths(metadata: &JsonValue, manifest_dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();

    if let Some(preview_val) = metadata.get("preview") {
        match preview_val {
            JsonValue::String(s) => {
                if s.ends_with("/*") {
                    let dir = manifest_dir.join(s.trim_end_matches("/*"));
                    if let Ok(entries) = std::fs::read_dir(dir) {
                        for entry in entries.flatten() {
                            let p = entry.path();
                            if p.is_file() && is_preview_media(&p) {
                                out.push(p);
                            }
                        }
                    }
                } else {
                    let p = manifest_dir.join(s);
                    if p.exists() && is_preview_media(&p) {
                        out.push(p);
                    }
                }
            }
            JsonValue::Array(arr) => {
                for item in arr {
                    if let Some(rel) = item.as_str() {
                        let p = manifest_dir.join(rel);
                        if p.exists() && is_preview_media(&p) {
                            out.push(p);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    if out.is_empty() {
        let preview_dir = manifest_dir.join("preview");
        if let Ok(entries) = std::fs::read_dir(preview_dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_file() && is_preview_media(&p) {
                    out.push(p);
                }
            }
        }
    }

    out.sort();
    out
}

fn is_preview_media(path: &Path) -> bool {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_lowercase();
    matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "bmp" | "gif" | "webp")
}

fn find_category_dir_case_insensitive(assets_root: &Path, wanted: &str) -> Option<PathBuf> {
    let wanted_lc = wanted.to_lowercase();
    let direct = assets_root.join(wanted);
    if direct.exists() {
        return Some(direct);
    }

    let entries = std::fs::read_dir(assets_root).ok()?;
    for entry in entries.flatten() {
        let p = entry.path();
        if !p.is_dir() {
            continue;
        }
        let name = p.file_name().and_then(|s| s.to_str()).unwrap_or_default().to_lowercase();
        if name == wanted_lc || name == format!("{}s", wanted_lc) || format!("{}s", name) == wanted_lc {
            return Some(p);
        }
    }

    None
}