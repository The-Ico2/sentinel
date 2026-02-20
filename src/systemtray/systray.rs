// ~/sentinel/sentinel-backend/src/systemtray/systray.rs
// Responsible for Creating and Managing the Systray (starting/stopping/reloading addons, opening editor, backend control)

use sysinfo::System;
use std::{
    process::{Child, Command},
    collections::HashMap,
    path::{Path, PathBuf},
};
use tao::{
    event::Event,
    event_loop::{ControlFlow, EventLoopBuilder},
};
use tray_icon::{
    menu::MenuEvent,
    TrayIcon, TrayIconBuilder, TrayIconEvent,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};

use crate::{
    Addon,
    systemtray::{
        build::build_systray,
        discover::discover_addons,
    },
    ipc::request::send_ipc_request,
};
use crate::{info, warn, error};

#[derive(Clone)]
pub enum MenuAction { 
    Start(String),
    Stop(String),
    Reload(String),
    OpenConfigUi(String),
    OpenSentinelUi,
    ToggleAutostart(String),
    ToggleBackendStartup,
    Rescan,
    Exit
}

pub enum UserEvent {
    TrayIconEvent(TrayIconEvent),
    MenuEvent(MenuEvent)
}

const STARTUP_REGISTRY_NAME: &str = "SentinelBackend";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct TraySettings {
    #[serde(default)]
    run_backend_at_startup: bool,
    #[serde(default)]
    addon_autostart: HashMap<String, bool>,
}

fn tray_settings_path() -> Option<PathBuf> {
    std::env::var("USERPROFILE")
        .ok()
        .map(|home| Path::new(&home).join(".Sentinel").join("tray_settings.json"))
}

fn load_tray_settings() -> TraySettings {
    let Some(path) = tray_settings_path() else {
        warn!("USERPROFILE not set; using default tray settings");
        return TraySettings::default();
    };

    if !path.exists() {
        return TraySettings::default();
    }

    match std::fs::read_to_string(&path) {
        Ok(content) => match serde_json::from_str::<TraySettings>(&content) {
            Ok(settings) => settings,
            Err(e) => {
                warn!("Failed to parse tray settings '{}': {}", path.display(), e);
                TraySettings::default()
            }
        },
        Err(e) => {
            warn!("Failed to read tray settings '{}': {}", path.display(), e);
            TraySettings::default()
        }
    }
}

fn save_tray_settings(settings: &TraySettings) {
    let Some(path) = tray_settings_path() else {
        warn!("USERPROFILE not set; cannot save tray settings");
        return;
    };

    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            error!("Failed to create tray settings directory '{}': {}", parent.display(), e);
            return;
        }
    }

    let content = match serde_json::to_string_pretty(settings) {
        Ok(value) => value,
        Err(e) => {
            error!("Failed to serialize tray settings: {}", e);
            return;
        }
    };

    if let Err(e) = std::fs::write(&path, content) {
        error!("Failed to write tray settings '{}': {}", path.display(), e);
    }
}

#[cfg(target_os = "windows")]
fn is_backend_startup_enabled() -> Result<bool, String> {
    let output = Command::new("reg")
        .args([
            "query",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
            "/v",
            STARTUP_REGISTRY_NAME,
        ])
        .output()
        .map_err(|e| format!("Failed to query startup registry: {}", e))?;

    Ok(output.status.success())
}

#[cfg(not(target_os = "windows"))]
fn is_backend_startup_enabled() -> Result<bool, String> {
    Ok(false)
}

#[cfg(target_os = "windows")]
fn set_backend_startup_enabled(enabled: bool) -> Result<(), String> {
    if enabled {
        let exe = std::env::current_exe()
            .map_err(|e| format!("Failed to resolve backend executable: {}", e))?;
        let exe_value = format!("\"{}\"", exe.display());

        let output = Command::new("reg")
            .args([
                "add",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                "/v",
                STARTUP_REGISTRY_NAME,
                "/t",
                "REG_SZ",
                "/d",
                &exe_value,
                "/f",
            ])
            .output()
            .map_err(|e| format!("Failed to enable startup registry entry: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("Failed to enable startup registry entry: {}", stderr.trim()));
        }

        Ok(())
    } else {
        let output = Command::new("reg")
            .args([
                "delete",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                "/v",
                STARTUP_REGISTRY_NAME,
                "/f",
            ])
            .output()
            .map_err(|e| format!("Failed to disable startup registry entry: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let message = stderr.trim();
            if !message.contains("unable to find") && !message.contains("Unable to find") {
                return Err(format!("Failed to disable startup registry entry: {}", message));
            }
        }

        Ok(())
    }
}

#[cfg(not(target_os = "windows"))]
fn set_backend_startup_enabled(_enabled: bool) -> Result<(), String> {
    Err("Run at startup toggle is only supported on Windows".to_string())
}

pub fn start_configured_autostart_addons() {
    let settings = load_tray_settings();

    let addons_to_start: Vec<String> = settings
        .addon_autostart
        .iter()
        .filter(|(_, enabled)| **enabled)
        .map(|(name, _)| name.clone())
        .collect();

    if addons_to_start.is_empty() {
        info!("[addons] No addons configured for autostart");
        return;
    }

    for addon_name in addons_to_start {
        match crate::ipc::addon::start(Some(json!({"addon_name": addon_name.clone()}))) {
            Ok(_) => info!("[addons] Autostarted '{}' on backend startup", addon_name),
            Err(e) => warn!("[addons] Failed to autostart '{}' on backend startup: {}", addon_name, e),
        }
    }
}

fn ensure_user_config_dirs() {
    if let Ok(home) = std::env::var("USERPROFILE") {
        let root = Path::new(&home).join(".Sentinel");
        for p in [
            root.join("Assets"),
            root.join("Assets/Addons"),
        ] {
            if let Err(e) = std::fs::create_dir_all(&p) {
                warn!("Failed to create config dir {}: {}", p.display(), e);
            } else {
                info!("Ensured config dir exists: {}", p.display());
            }
        }

        let addons_root = root.join("Addons");
        if let Ok(addon_entries) = std::fs::read_dir(&addons_root) {
            for addon_entry in addon_entries.flatten() {
                let addon_dir = addon_entry.path();
                if !addon_dir.is_dir() {
                    continue;
                }

                let addon_json = addon_dir.join("addon.json");
                let parsed = std::fs::read_to_string(&addon_json)
                    .ok()
                    .and_then(|text| serde_json::from_str::<JsonValue>(&text).ok())
                    .unwrap_or(JsonValue::Null);

                let accepts_assets = parsed
                    .get("accepts_assets")
                    .and_then(|v| v.as_bool())
                    .or_else(|| parsed.get("assets").and_then(|a| a.get("accepts")).and_then(|v| v.as_bool()))
                    .unwrap_or(false);

                if !accepts_assets {
                    continue;
                }

                let addon_id = parsed
                    .get("id")
                    .and_then(|v| v.as_str())
                    .or_else(|| addon_dir.file_name().and_then(|s| s.to_str()))
                    .unwrap_or("unknown-addon");

                let addon_assets_dir = root.join("Assets").join("Addons").join(addon_id);
                if let Err(e) = std::fs::create_dir_all(&addon_assets_dir) {
                    warn!("Failed to create addon asset dir {}: {}", addon_assets_dir.display(), e);
                } else {
                    info!("Ensured addon asset dir exists: {}", addon_assets_dir.display());
                }

                let categories = parsed
                    .get("asset_categories")
                    .and_then(|v| v.as_array())
                    .or_else(|| parsed.get("assets").and_then(|a| a.get("categories")).and_then(|v| v.as_array()))
                    .cloned()
                    .unwrap_or_default();

                for category in categories {
                    if let Some(category_name) = category.as_str() {
                        let category_dir = root.join("Assets").join(category_name);
                        if let Err(e) = std::fs::create_dir_all(&category_dir) {
                            warn!("Failed to create asset category dir {}: {}", category_dir.display(), e);
                        } else {
                            info!("Ensured asset category dir exists: {}", category_dir.display());
                        }
                    }
                }
            }
        }
    } else {
        warn!("USERPROFILE not set; cannot create user config directories");
    }
}

// TODO: Actually Use when neccessary
fn _is_addon_running(addon: &Addon) -> bool {
    let mut sys = System::new();
    sys.refresh_all();
    for (_pid, proc_) in sys.processes() {
        if let Some(exe) = proc_.exe() {
            if exe == addon.exe_path {
                info!("Addon '{}' is running (exe path match)", addon.name);
                return true;
            }
        }
        if proc_.name().eq_ignore_ascii_case(&format!("{}.exe", addon.package)) {
            info!("Addon '{}' is running (process name match)", addon.name);
            return true;
        }
    }
    false
}

fn load_icon(path: &std::path::Path) -> tray_icon::Icon {
    info!("Loading tray icon from {}", path.display());
    let (icon_rgba, icon_width, icon_height) = {
        let image = image::open(path)
            .expect("Failed to open icon path")
            .into_rgba8();
        let (width, height) = image.dimensions();
        let rgba = image.into_raw();
        (rgba, width, height)
    };
    tray_icon::Icon::from_rgba(icon_rgba, icon_width, icon_height).expect("Failed to create tray icon")
}

pub fn spawn_tray() {
    info!("Starting system tray");
    let icon_path = concat!(env!("CARGO_MANIFEST_DIR"), "/icon.png");
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    TrayIconEvent::set_event_handler(Some({
        let proxy = proxy.clone();
        move |event| {
            let _ = proxy.send_event(UserEvent::TrayIconEvent(event));
        }
    }));

    let proxy = event_loop.create_proxy();
    MenuEvent::set_event_handler(Some({
        let proxy = proxy.clone();
        move |event| {
            let _ = proxy.send_event(UserEvent::MenuEvent(event));
        }
    }));

    let mut addons: Vec<Addon> = Vec::new();
    let mut children: HashMap<String, Child> = HashMap::new();
    let mut settings = load_tray_settings();
    let mut autostart: HashMap<String, bool> = settings.addon_autostart.clone();

    let detected_backend_startup = is_backend_startup_enabled().unwrap_or(settings.run_backend_at_startup);
    settings.run_backend_at_startup = detected_backend_startup;
    save_tray_settings(&settings);

    let mut backend_run_at_startup = settings.run_backend_at_startup;

    let (mut menu, mut id_map) = build_systray(&addons, &autostart, backend_run_at_startup);
    let mut tray_icon: Option<TrayIcon> = None;

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::NewEvents(tao::event::StartCause::Init) => {
                info!("Initializing tray icon");
                let icon = load_icon(std::path::Path::new(icon_path));
                tray_icon = Some(
                    TrayIconBuilder::new()
                        .with_menu(Box::new(menu.clone()))
                        .with_tooltip("Sentinel WDCP")
                        .with_icon(icon)
                        .build()
                        .expect("Failed to build tray icon"),
                );
                ensure_user_config_dirs();

                addons = discover_addons();
                info!("Discovered {} addons for tray", addons.len());
                autostart = addons
                    .iter()
                    .map(|a| {
                        let enabled = *autostart.get(&a.name).unwrap_or(&false);
                        (a.name.clone(), enabled)
                    })
                    .collect();
                settings.addon_autostart = autostart.clone();
                save_tray_settings(&settings);
                let (new_menu, new_map) = build_systray(&addons, &autostart, backend_run_at_startup);
                let icon = load_icon(std::path::Path::new(icon_path));
                tray_icon = Some(
                    TrayIconBuilder::new()
                        .with_menu(Box::new(new_menu.clone()))
                        .with_tooltip("Sentinel WDCP")
                        .with_icon(icon)
                        .build()
                        .expect("Failed to rebuild tray icon after addon discovery"),
                );
                menu = new_menu;
                id_map = new_map;
            }

            Event::UserEvent(UserEvent::TrayIconEvent(event)) => match event {
                TrayIconEvent::Click { button, .. } => info!("[tray] Click: {:?}", button),
                TrayIconEvent::DoubleClick { button, .. } => info!("[tray] DoubleClick: {:?}", button),
                _ => println!("[tray] Other tray event"),
            },

            Event::UserEvent(UserEvent::MenuEvent(event)) => {
                if let Some(action) = id_map.get(&event.id).cloned() {
                    match action {
                        MenuAction::Start(name) => {
                            let req = crate::ipc::request::IpcRequest {
                                ns: "addon".to_string(),
                                cmd: "start".to_string(),
                                args: Some(json!({"addon_name": name.clone()})),
                            };
                            match send_ipc_request(req) {
                                Ok(resp) if resp.ok => info!("[addons] Started '{}' via IPC", name),
                                Ok(resp) => error!("[addons] Failed to start '{}': {}", name, resp.error.unwrap_or_default()),
                                Err(e) => error!("[addons] IPC error starting '{}': {}", name, e),
                            }
                        }
                        MenuAction::Stop(name) => {
                            let req = crate::ipc::request::IpcRequest {
                                ns: "addon".to_string(),
                                cmd: "stop".to_string(),
                                args: Some(json!({"addon_name": name.clone()})),
                            };
                            match send_ipc_request(req) {
                                Ok(resp) if resp.ok => info!("[addons] Stopped '{}' via IPC", name),
                                Ok(resp) => error!("[addons] Failed to stop '{}': {}", name, resp.error.unwrap_or_default()),
                                Err(e) => error!("[addons] IPC error stopping '{}': {}", name, e),
                            }
                        }
                        MenuAction::Reload(name) => {
                            let req = crate::ipc::request::IpcRequest {
                                ns: "addon".to_string(),
                                cmd: "reload".to_string(),
                                args: Some(json!({"addon_name": name.clone()})),
                            };
                            match send_ipc_request(req) {
                                Ok(resp) if resp.ok => info!("[addons] Reloaded '{}' via IPC", name),
                                Ok(resp) => error!("[addons] Failed to reload '{}': {}", name, resp.error.unwrap_or_default()),
                                Err(e) => error!("[addons] IPC error reloading '{}': {}", name, e),
                            }
                        }
                        MenuAction::OpenConfigUi(addon_id) => {
                            match std::env::current_exe() {
                                Ok(exe) => {
                                    match Command::new(exe)
                                        .arg("--addon-config-ui")
                                        .arg(&addon_id)
                                        .spawn()
                                    {
                                        Ok(_) => info!("[addons] Opened config UI for '{}'", addon_id),
                                        Err(e) => error!("[addons] Failed to open config UI for '{}': {}", addon_id, e),
                                    }
                                }
                                Err(e) => {
                                    error!("[addons] Failed to resolve backend executable for config UI '{}': {}", addon_id, e)
                                }
                            }
                        }
                        MenuAction::OpenSentinelUi => {
                            match std::env::current_exe() {
                                Ok(exe) => {
                                    match Command::new(exe)
                                        .arg("--sentinel-ui")
                                        .spawn()
                                    {
                                        Ok(_) => info!("[ui] Opened Sentinel UI"),
                                        Err(e) => error!("[ui] Failed to open Sentinel UI: {}", e),
                                    }
                                }
                                Err(e) => error!("[ui] Failed to resolve backend executable for Sentinel UI: {}", e),
                            }
                        }
                        MenuAction::ToggleAutostart(name) => {
                            let enabled = {
                                let entry = autostart.entry(name.clone()).or_insert(false);
                                *entry = !*entry;
                                *entry
                            };
                            settings.addon_autostart = autostart.clone();
                            save_tray_settings(&settings);
                            info!("[addons] Toggled autostart for '{}': {}", name, enabled);
                            let (new_menu, new_map) = build_systray(&addons, &autostart, backend_run_at_startup);
                            let icon = load_icon(std::path::Path::new(icon_path));
                            tray_icon = Some(
                                TrayIconBuilder::new()
                                    .with_menu(Box::new(new_menu.clone()))
                                    .with_tooltip("Sentinel WDCP")
                                    .with_icon(icon)
                                    .build()
                                    .expect("Failed to rebuild tray icon"),
                            );
                            menu = new_menu;
                            id_map = new_map;
                        }
                        MenuAction::ToggleBackendStartup => {
                            let next = !backend_run_at_startup;
                            match set_backend_startup_enabled(next) {
                                Ok(_) => {
                                    backend_run_at_startup = next;
                                    settings.run_backend_at_startup = backend_run_at_startup;
                                    save_tray_settings(&settings);
                                    info!("[backend] Run at startup toggled: {}", backend_run_at_startup);
                                }
                                Err(e) => {
                                    error!("[backend] Failed to toggle run at startup: {}", e);
                                }
                            }

                            let (new_menu, new_map) = build_systray(&addons, &autostart, backend_run_at_startup);
                            let icon = load_icon(std::path::Path::new(icon_path));
                            tray_icon = Some(
                                TrayIconBuilder::new()
                                    .with_menu(Box::new(new_menu.clone()))
                                    .with_tooltip("Sentinel WDCP")
                                    .with_icon(icon)
                                    .build()
                                    .expect("Failed to rebuild tray icon"),
                            );
                            menu = new_menu;
                            id_map = new_map;
                        }
                        MenuAction::Rescan => {
                            info!("Rescanning addons");
                            addons = discover_addons();
                            autostart = addons.iter().map(|a| {
                                let v = *autostart.get(&a.name).unwrap_or(&false);
                                (a.name.clone(), v)
                            }).collect();
                            settings.addon_autostart = autostart.clone();
                            save_tray_settings(&settings);
                            let (new_menu, new_map) = build_systray(&addons, &autostart, backend_run_at_startup);
                            let icon = load_icon(std::path::Path::new(icon_path));
                            tray_icon = Some(
                                TrayIconBuilder::new()
                                    .with_menu(Box::new(new_menu.clone()))
                                    .with_tooltip("Sentinel")
                                    .with_icon(icon)
                                    .build()
                                    .expect("Failed to rebuild tray icon"),
                            );
                            menu = new_menu;
                            id_map = new_map;
                            info!("Addon menu updated after rescan");
                        }
                        MenuAction::Exit => {
                            info!("Exiting tray, stopping all addons");
                            for (name, mut child) in children.drain() {
                                let _ = child.kill();
                                info!("[addons] Stopped '{}' on exit", name);
                            }
                            tray_icon.take();
                            *control_flow = ControlFlow::Exit;
                        }
                    }
                }
            }

            _ => {}
        }
    });
}