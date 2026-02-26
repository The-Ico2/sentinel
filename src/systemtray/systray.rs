// ~/sentinel/sentinel-backend/src/systemtray/systray.rs
// Responsible for Creating and Managing the Systray (starting/stopping/reloading addons, opening editor, backend control)

use std::{
    process::{Child, Command},
    collections::HashMap,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tao::{
    event::Event,
    event_loop::{ControlFlow, EventLoopBuilder},
};
use tray_icon::{
    menu::MenuEvent,
    TrayIconBuilder, TrayIconEvent, MouseButton, MouseButtonState,
};
#[cfg(target_os = "windows")]
use tray_icon::menu::ContextMenu;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};

#[cfg(target_os = "windows")]
use windows::{
    core::{w, BOOL},
    Win32::{
        Foundation::{HWND, LPARAM},
        UI::WindowsAndMessaging::{
            EnumWindows, FindWindowW, GetWindowThreadProcessId, IsIconic, IsWindowVisible,
            SetForegroundWindow, ShowWindow, SW_RESTORE,
        },
    },
};

use crate::{
    Addon,
    systemtray::{
        build::{build_addon_menu, build_backend_menu},
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

/// Attempt to find and bring to foreground a window belonging to the given PID.
#[cfg(target_os = "windows")]
fn focus_process_window(target_pid: u32) -> bool {
    struct CallbackData {
        target_pid: u32,
        found_hwnd: HWND,
    }

    unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let data = &mut *(lparam.0 as *mut CallbackData);
        let mut proc_id: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut proc_id));
        if proc_id == data.target_pid && (IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool()) {
            data.found_hwnd = hwnd;
            return BOOL(0); // stop enumeration
        }
        BOOL(1) // continue
    }

    let mut data = CallbackData {
        target_pid,
        found_hwnd: HWND(std::ptr::null_mut()),
    };
    unsafe {
        let _ = EnumWindows(Some(enum_proc), LPARAM(&mut data as *mut CallbackData as isize));
        if !data.found_hwnd.0.is_null() {
            let _ = ShowWindow(data.found_hwnd, SW_RESTORE);
            let _ = SetForegroundWindow(data.found_hwnd);
            return true;
        }
    }
    false
}

#[cfg(target_os = "windows")]
fn focus_sentinel_window_by_title() -> bool {
    unsafe {
        let hwnd = match FindWindowW(None, w!("Sentinel")) {
            Ok(handle) => handle,
            Err(_) => return false,
        };
        let _ = ShowWindow(hwnd, SW_RESTORE);
        let _ = SetForegroundWindow(hwnd);
        true
    }
}

/// Open the Sentinel UI or bring the existing instance to focus.
fn open_or_focus_ui(ui_child: &mut Option<Child>) {
    // Check if UI is already running
    if let Some(ref mut child) = ui_child {
        match child.try_wait() {
            Ok(None) => {
                // Still running, bring to focus
                #[cfg(target_os = "windows")]
                {
                    let pid = child.id();
                    if focus_process_window(pid) {
                        info!("[ui] Brought existing Sentinel UI to focus");
                        return;
                    }
                    if focus_sentinel_window_by_title() {
                        info!("[ui] Focused existing Sentinel UI window by title fallback");
                        return;
                    }
                    warn!("[ui] UI process running but couldn't find/focus window");
                }
                return;
            }
            Ok(Some(_)) => {
                info!("[ui] Previous UI process has exited");
            }
            Err(e) => {
                error!("[ui] Error checking UI process: {}", e);
            }
        }
    }

    // Fallback: UI may already be running from another backend session.
    #[cfg(target_os = "windows")]
    {
        if focus_sentinel_window_by_title() {
            info!("[ui] Focused existing Sentinel UI window by title");
            return;
        }
    }

    // Open new UI
    match std::env::current_exe() {
        Ok(exe) => {
            match Command::new(exe)
                .arg("--sentinel-ui")
                .spawn()
            {
                Ok(child) => {
                    info!("[ui] Opened Sentinel UI with PID {}", child.id());
                    *ui_child = Some(child);
                }
                Err(e) => error!("[ui] Failed to open Sentinel UI: {}", e),
            }
        }
        Err(e) => error!("[ui] Failed to resolve backend executable: {}", e),
    }
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
    let mut settings = load_tray_settings();
    let mut autostart: HashMap<String, bool> = settings.addon_autostart.clone();

    let detected_backend_startup = is_backend_startup_enabled().unwrap_or(settings.run_backend_at_startup);
    settings.run_backend_at_startup = detected_backend_startup;
    save_tray_settings(&settings);

    let mut backend_run_at_startup = settings.run_backend_at_startup;

    // Build two separate menus: addon menu (right-click) and backend menu (double-right-click)
    let (mut addon_menu, mut addon_id_map) = build_addon_menu(&addons, &autostart);
    let (mut backend_menu, mut backend_id_map) = build_backend_menu(backend_run_at_startup);

    // Tray icon is created WITHOUT a menu so we can handle right/double-right-click ourselves
    let mut tray_icon: Option<tray_icon::TrayIcon> = None;

    // UI single-instance tracking
    let mut ui_child: Option<Child> = None;

    // Debounce state for right-click vs double-right-click
    let debounce_duration = Duration::from_millis(400);
    let mut pending_right_click: Option<Instant> = None;
    let mut double_click_cooldown: Option<Instant> = None;

    event_loop.run(move |event, _, control_flow| {
        match event {
            Event::NewEvents(tao::event::StartCause::Init) => {
                info!("Initializing tray icon");
                let icon = load_icon(std::path::Path::new(icon_path));
                tray_icon = Some(
                    TrayIconBuilder::new()
                        .with_tooltip("Sentinel WDCP")
                        .with_icon(icon)
                        .with_menu_on_left_click(false)
                        .build()
                        .expect("Failed to build tray icon"),
                );
                ensure_user_config_dirs();

                // Attach both menu subclasses to the tray icon's hidden window
                #[cfg(target_os = "windows")]
                if let Some(ref ti) = tray_icon {
                    let hwnd = ti.window_handle() as isize;
                    unsafe {
                        addon_menu.attach_menu_subclass_for_hwnd(hwnd);
                        backend_menu.attach_menu_subclass_for_hwnd(hwnd);
                    }
                }

                // Discover addons and rebuild menus
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

                // Detach old subclasses, rebuild menus, attach new subclasses
                #[cfg(target_os = "windows")]
                if let Some(ref ti) = tray_icon {
                    let hwnd = ti.window_handle() as isize;
                    unsafe {
                        addon_menu.detach_menu_subclass_from_hwnd(hwnd);
                        backend_menu.detach_menu_subclass_from_hwnd(hwnd);
                    }
                }

                let (new_addon, new_addon_ids) = build_addon_menu(&addons, &autostart);
                let (new_backend, new_backend_ids) = build_backend_menu(backend_run_at_startup);

                #[cfg(target_os = "windows")]
                if let Some(ref ti) = tray_icon {
                    let hwnd = ti.window_handle() as isize;
                    unsafe {
                        new_addon.attach_menu_subclass_for_hwnd(hwnd);
                        new_backend.attach_menu_subclass_for_hwnd(hwnd);
                    }
                }

                addon_menu = new_addon;
                addon_id_map = new_addon_ids;
                backend_menu = new_backend;
                backend_id_map = new_backend_ids;
            }

            // Debounce timeout: show the addon (right-click) menu
            Event::NewEvents(tao::event::StartCause::ResumeTimeReached { .. }) => {
                if pending_right_click.take().is_some() {
                    #[cfg(target_os = "windows")]
                    if let Some(ref ti) = tray_icon {
                        let hwnd = ti.window_handle() as isize;
                        unsafe {
                            addon_menu.show_context_menu_for_hwnd(hwnd, None);
                        }
                    }
                }
            }

            Event::UserEvent(UserEvent::TrayIconEvent(tray_evt)) => match tray_evt {
                // Single right-click UP: start debounce timer (may become double-right-click)
                TrayIconEvent::Click {
                    button: MouseButton::Right,
                    button_state: MouseButtonState::Up,
                    ..
                } => {
                    // Suppress the right-up that follows a double-right-click
                    if let Some(cd) = double_click_cooldown {
                        if cd.elapsed() < Duration::from_millis(600) {
                            double_click_cooldown = None;
                            return;
                        }
                    }
                    pending_right_click = Some(Instant::now());
                }

                // Double LEFT click: open / focus the Sentinel UI
                TrayIconEvent::DoubleClick {
                    button: MouseButton::Left,
                    ..
                } => {
                    info!("[tray] Double-left-click → open/focus Sentinel UI");
                    open_or_focus_ui(&mut ui_child);
                }

                // Double RIGHT click: cancel pending single right-click, show backend options menu
                TrayIconEvent::DoubleClick {
                    button: MouseButton::Right,
                    ..
                } => {
                    info!("[tray] Double-right-click → show backend options menu");
                    pending_right_click = None;
                    double_click_cooldown = Some(Instant::now());

                    #[cfg(target_os = "windows")]
                    if let Some(ref ti) = tray_icon {
                        let hwnd = ti.window_handle() as isize;
                        unsafe {
                            backend_menu.show_context_menu_for_hwnd(hwnd, None);
                        }
                    }
                }

                _ => {}
            },

            Event::UserEvent(UserEvent::MenuEvent(event)) => {
                // Look up action in both menu id maps
                let action = addon_id_map
                    .get(&event.id)
                    .or_else(|| backend_id_map.get(&event.id))
                    .cloned();

                if let Some(action) = action {
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
                        MenuAction::ToggleAutostart(name) => {
                            let enabled = {
                                let entry = autostart.entry(name.clone()).or_insert(false);
                                *entry = !*entry;
                                *entry
                            };
                            settings.addon_autostart = autostart.clone();
                            save_tray_settings(&settings);
                            info!("[addons] Toggled autostart for '{}': {}", name, enabled);

                            // Rebuild addon menu
                            #[cfg(target_os = "windows")]
                            if let Some(ref ti) = tray_icon {
                                let hwnd = ti.window_handle() as isize;
                                unsafe { addon_menu.detach_menu_subclass_from_hwnd(hwnd); }
                            }
                            let (new_addon, new_addon_ids) = build_addon_menu(&addons, &autostart);
                            #[cfg(target_os = "windows")]
                            if let Some(ref ti) = tray_icon {
                                let hwnd = ti.window_handle() as isize;
                                unsafe { new_addon.attach_menu_subclass_for_hwnd(hwnd); }
                            }
                            addon_menu = new_addon;
                            addon_id_map = new_addon_ids;
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

                            // Rebuild backend menu
                            #[cfg(target_os = "windows")]
                            if let Some(ref ti) = tray_icon {
                                let hwnd = ti.window_handle() as isize;
                                unsafe { backend_menu.detach_menu_subclass_from_hwnd(hwnd); }
                            }
                            let (new_backend, new_backend_ids) = build_backend_menu(backend_run_at_startup);
                            #[cfg(target_os = "windows")]
                            if let Some(ref ti) = tray_icon {
                                let hwnd = ti.window_handle() as isize;
                                unsafe { new_backend.attach_menu_subclass_for_hwnd(hwnd); }
                            }
                            backend_menu = new_backend;
                            backend_id_map = new_backend_ids;
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

                            // Rebuild addon menu
                            #[cfg(target_os = "windows")]
                            if let Some(ref ti) = tray_icon {
                                let hwnd = ti.window_handle() as isize;
                                unsafe { addon_menu.detach_menu_subclass_from_hwnd(hwnd); }
                            }
                            let (new_addon, new_addon_ids) = build_addon_menu(&addons, &autostart);
                            #[cfg(target_os = "windows")]
                            if let Some(ref ti) = tray_icon {
                                let hwnd = ti.window_handle() as isize;
                                unsafe { new_addon.attach_menu_subclass_for_hwnd(hwnd); }
                            }
                            addon_menu = new_addon;
                            addon_id_map = new_addon_ids;
                            info!("Addon menu updated after rescan");
                        }
                        MenuAction::Exit => {
                            info!("Exiting tray, stopping all addons");
                            crate::ipc::addon::stop_all();

                            // Kill UI if running
                            if let Some(ref mut child) = ui_child {
                                let _ = child.kill();
                                info!("[ui] Stopped Sentinel UI on exit");
                            }

                            tray_icon.take();
                            *control_flow = ControlFlow::Exit;
                        }
                    }
                }
            }

            _ => {}
        }

        // Set control flow: use WaitUntil when debouncing, otherwise Wait
        if *control_flow != ControlFlow::Exit {
            if let Some(click_time) = pending_right_click {
                let deadline = click_time + debounce_duration;
                *control_flow = ControlFlow::WaitUntil(deadline);
            } else {
                *control_flow = ControlFlow::Wait;
            }
        }
    });
}