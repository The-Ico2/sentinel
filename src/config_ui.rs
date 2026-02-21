use std::{collections::{HashMap, HashSet}, path::{Path, PathBuf}};

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
    log_level: Option<String>,
    tick_sleep_ms: Option<i64>,
    watcher_enabled: Option<bool>,
    reapply_on_pause_change: Option<bool>,
    assets: Vec<WallpaperShellAsset>,
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
}

fn parse_shell_ipc_message(body: &str) -> Option<ShellIpcMessage> {
    if let Ok(direct) = serde_json::from_str::<ShellIpcMessage>(body) {
        return Some(direct);
    }

    let value = serde_json::from_str::<serde_json::Value>(body).ok()?;
    if let Some(as_text) = value.as_str() {
        if let Ok(direct_text) = serde_json::from_str::<ShellIpcMessage>(as_text) {
            return Some(direct_text);
        }
    }
    let payload = value
        .get("payload")
        .cloned()
        .unwrap_or_else(|| value.clone());

    if let Some(payload_text) = payload.as_str() {
        if let Ok(from_payload_text) = serde_json::from_str::<ShellIpcMessage>(payload_text) {
            return Some(from_payload_text);
        }
    }

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

        let shell_url = file_path_to_url(&shell_path)?;
        info!("[ui] Launching Sentinel custom-tab shell at {}", shell_url);

        let event_loop = EventLoopBuilder::new().build();
        let window = WindowBuilder::new()
                .with_title("Sentinel")
                .build(&event_loop)
                .map_err(|e| format!("Failed to create Sentinel shell window: {}", e))?;

        let webview = WebViewBuilder::new()
                .with_url(&shell_url)
                .with_ipc_handler(|request| {
                    let payload = request.body().to_string();
                    let result = std::panic::catch_unwind(move || {
                        warn!("[ui] Shell IPC raw payload: {}", payload);

                        let Some(message) = parse_shell_ipc_message(&payload) else {
                            warn!("[ui] Unrecognized shell IPC payload: {}", payload);
                            return;
                        };

                        warn!("[ui] Shell IPC message kind='{}'", message.kind);

                        if !message.kind.eq_ignore_ascii_case("wallpaper_apply_assignment") {
                            return;
                        }

                        let addon_id = message
                            .addon_id
                            .unwrap_or_else(|| "sentinel.addon.wallpaper".to_string());
                        let wallpaper_id = match message.wallpaper_id {
                            Some(v) if !v.trim().is_empty() => v,
                            _ => {
                                warn!("[ui] wallpaper_apply_assignment missing wallpaper_id");
                                return;
                            }
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
                                "[ui] Saved wallpaper assignment: addon='{}' wallpaper='{}' monitor_ids={:?} monitor_indexes={:?}",
                                addon_id,
                                wallpaper_id,
                                monitor_ids,
                                monitor_indexes
                            ),
                            Err(e) => warn!(
                                "[ui] Failed saving wallpaper assignment: addon='{}' wallpaper='{}' monitor_ids={:?} monitor_indexes={:?} error={}",
                                addon_id,
                                wallpaper_id,
                                monitor_ids,
                                monitor_indexes,
                                e
                            ),
                        }
                    });

                    if result.is_err() {
                        warn!("[ui] Recovered from panic while handling shell IPC message");
                    }
                })
                .build(&window)
                .map_err(|e| format!("Failed to create Sentinel shell webview: {}", e))?;

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

fn sentinel_shell_html_path() -> Result<PathBuf, String> {
        let home = std::env::var("USERPROFILE").map_err(|_| "USERPROFILE not set".to_string())?;
        Ok(Path::new(&home)
                .join(".Sentinel")
                .join("cache")
                .join("sentinel_custom_tabs_shell.html"))
}

fn collect_custom_tab_shell_addons(catalog: &[AddonMeta]) -> Vec<CustomTabShellAddon> {
        let mut out = Vec::new();
        for addon in catalog {
                let tabs = discover_custom_tabs(addon);
                if tabs.is_empty() {
                        continue;
                }

        let wallpaper_payload = build_wallpaper_shell_data(addon);

                let shell_tabs: Vec<CustomTabShellPage> = tabs
                        .into_iter()
                        .filter_map(|t| {
                file_path_to_url(&t.path).ok().map(|base_url| {
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
        "hosted": true,
        "wallpaper": wallpaper,
    });
    let payload_str = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string());
    let encoded = urlencoding::encode(&payload_str);
    let sep = if base_url.contains('?') { "&" } else { "?" };
    format!("{}{}sentinelData={}", base_url, sep, encoded)
}

fn build_wallpaper_shell_data(addon: &AddonMeta) -> Option<WallpaperShellData> {
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
                .and_then(|p| file_path_to_url(p).ok());

            WallpaperShellAsset {
                id: asset.id,
                name: asset.name,
                tags: asset.tags,
                short_description: asset.short_description,
                last_updated: asset.last_updated,
                author_name,
                author_url,
                preview_url,
            }
        })
        .collect::<Vec<_>>();

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
        log_level: yaml_string(&config_root, "settings.development.log_level"),
        tick_sleep_ms: yaml_i64(&config_root, "settings.runtime.tick_sleep_ms"),
        watcher_enabled: yaml_bool(&config_root, "settings.performance.watcher.enabled"),
        reapply_on_pause_change: yaml_bool(&config_root, "settings.runtime.reapply_on_pause_change"),
        assets,
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
        target_indexes.push("*".to_string());
    }

    warn!(
        "[ui] Applying wallpaper assignment to config='{}' addon='{}' wallpaper='{}' targets={:?}",
        addon.config_path.display(),
        addon.id,
        wallpaper_id,
        target_indexes
    );

    let content = std::fs::read_to_string(&addon.config_path).unwrap_or_else(|_| "{}".to_string());
    let mut root = serde_yaml::from_str::<Value>(&content).unwrap_or_else(|_| Value::Mapping(Mapping::new()));
    if !matches!(root, Value::Mapping(_)) {
        root = Value::Mapping(Mapping::new());
    }

    let root_map = root
        .as_mapping_mut()
        .ok_or_else(|| "Config root is not a mapping".to_string())?;

    ensure_wallpapers_map(root_map)?;

    for target_idx in &target_indexes {
        let updated_nested = {
            let wallpapers_map = ensure_wallpapers_map(root_map)?;
            update_wallpaper_profile_for_index(wallpapers_map, target_idx, wallpaper_id)
        };
        if updated_nested {
            continue;
        }

        if update_wallpaper_profile_for_index(root_map, target_idx, wallpaper_id) {
            continue;
        }

        let wallpapers_map = ensure_wallpapers_map(root_map)?;
        insert_wallpaper_profile_for_index(wallpapers_map, target_idx, wallpaper_id);
    }

    let serialized = serde_yaml::to_string(&root)
        .map_err(|e| format!("Failed to serialize YAML: {}", e))?;
    std::fs::write(&addon.config_path, serialized)
        .map_err(|e| format!("Failed to write '{}': {}", addon.config_path.display(), e))?;

    Ok(())
}

fn ensure_wallpapers_map(root_map: &mut Mapping) -> Result<&mut Mapping, String> {
    let wallpapers_value = root_map
        .entry(Value::String("wallpapers".to_string()))
        .or_insert_with(|| Value::Mapping(Mapping::new()));
    if !matches!(wallpapers_value, Value::Mapping(_)) {
        *wallpapers_value = Value::Mapping(Mapping::new());
    }

    wallpapers_value
        .as_mapping_mut()
        .ok_or_else(|| "'wallpapers' is not a mapping".to_string())
}

fn update_wallpaper_profile_for_index(
    wallpapers_map: &mut Mapping,
    monitor_index: &str,
    wallpaper_id: &str,
) -> bool {
    for (section_key, section_value) in wallpapers_map.iter_mut() {
        let Some(section_name) = section_key.as_str() else {
            continue;
        };
        if !section_name.starts_with("wallpaper") {
            continue;
        }

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
            return true;
        }
    }

    false
}

fn insert_wallpaper_profile_for_index(
    wallpapers_map: &mut Mapping,
    monitor_index: &str,
    wallpaper_id: &str,
) {

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

fn build_sentinel_custom_tabs_shell_html(
        addons: &[CustomTabShellAddon],
        selected_addon_id: &str,
) -> Result<String, Box<dyn std::error::Error>> {
        let addons_json = serde_json::to_string(addons)?;
        let selected_json = serde_json::to_string(selected_addon_id)?;

        Ok(format!(
                r#"<!doctype html>
<html>
<head>
    <meta charset='utf-8' />
    <meta name='viewport' content='width=device-width, initial-scale=1' />
    <title>Sentinel</title>
    <style>
        html, body {{ margin: 0; padding: 0; height: 100%; background: #0d1117; color: #e6edf3; font-family: Segoe UI, sans-serif; }}
        .shell {{ display: grid; grid-template-columns: 240px 1fr; height: 100%; }}
        .sidebar {{ border-right: 1px solid #30363d; padding: 12px; box-sizing: border-box; background: #0f141b; }}
        .brand {{ font-weight: 700; margin-bottom: 10px; }}
        .addon-btn {{ width: 100%; text-align: left; margin: 4px 0; padding: 8px 10px; border: 1px solid #30363d; background: #111826; color: #e6edf3; border-radius: 6px; cursor: pointer; }}
        .addon-btn.active {{ background: #1f6feb; border-color: #1f6feb; color: white; }}
        .main {{ display: grid; grid-template-rows: auto 1fr; min-width: 0; }}
        .tabs {{ display: flex; gap: 8px; padding: 10px; border-bottom: 1px solid #30363d; background: #0f141b; }}
        .tab-btn {{ padding: 8px 12px; border-radius: 6px; border: 1px solid #30363d; background: #111826; color: #e6edf3; cursor: pointer; }}
        .tab-btn.active {{ background: #238636; border-color: #238636; color: white; }}
        .frame-wrap {{ min-height: 0; }}
        #tabFrame {{ width: 100%; height: 100%; border: 0; background: #0d1117; }}
    </style>
</head>
<body>
    <div class='shell'>
        <aside class='sidebar'>
            <div class='brand'>Sentinel</div>
            <div id='addons'></div>
        </aside>
        <main class='main'>
            <div class='tabs' id='tabs'></div>
            <div class='frame-wrap'><iframe id='tabFrame' title='Addon Tab'></iframe></div>
        </main>
    </div>
    <script>
        const ADDONS = {addons_json};
        let currentAddonId = {selected_json};
        let currentTabId = null;

        window.__sentinelBridgePost = (payload) => {{
            if (!payload) return false;
            if (window.ipc && typeof window.ipc.postMessage === 'function') {{
                window.ipc.postMessage(JSON.stringify(payload));
                return true;
            }}
            if (window.chrome && window.chrome.webview && typeof window.chrome.webview.postMessage === 'function') {{
                window.chrome.webview.postMessage(payload);
                return true;
            }}
            return false;
        }};

        function normalizeBridgePayload(data) {{
            if (!data) return null;

            let value = data;
            if (typeof value === 'string') {{
                try {{ value = JSON.parse(value); }} catch (_) {{ return null; }}
            }}

            if (value && typeof value === 'object') {{
                if (value.sentinelBridge && value.payload) return value.payload;
                if (value.payload && value.payload.type) return value.payload;
                if (value.type) return value;
            }}

            return null;
        }}

        window.addEventListener('message', (event) => {{
            const payload = normalizeBridgePayload(event && event.data);
            if (!payload) return;
            window.__sentinelBridgePost(payload);
        }});

        function getAddon() {{
            return ADDONS.find(a => a.id === currentAddonId) || ADDONS[0];
        }}

        function renderAddons() {{
            const host = document.getElementById('addons');
            host.innerHTML = '';
            ADDONS.forEach(addon => {{
                const btn = document.createElement('button');
                btn.className = 'addon-btn' + (addon.id === currentAddonId ? ' active' : '');
                btn.textContent = addon.name;
                btn.onclick = () => {{
                    currentAddonId = addon.id;
                    currentTabId = null;
                    render();
                }};
                host.appendChild(btn);
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
                btn.className = 'tab-btn' + (tab.id === currentTabId ? ' active' : '');
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

        function render() {{
            renderAddons();
            renderTabs();
        }}

        setTimeout(() => {{
            window.__sentinelBridgePost({{ type: 'shell_ping' }});
        }}, 400);

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
        Self::section_card(ui, "Settings", |ui| {
            ui.label("Reserved for global Sentinel settings and provider preferences.");
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
