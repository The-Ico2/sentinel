// ~/sentinel/sentinel-backend/src/systemtray/systray.rs
// Responsible for Creating and Managing the Systray (starting/stopping/reloading addons, opening editor, backend control)

use sysinfo::System;
use std::{
    process::{Child, Command},
    collections::HashMap,
    path::{Path},
};
use tao::{
    event::Event,
    event_loop::{ControlFlow, EventLoopBuilder},
};
use tray_icon::{
    menu::MenuEvent,
    TrayIcon, TrayIconBuilder, TrayIconEvent,
};
use serde_json::json;

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
    OpenEditor(String),
    ToggleAutostart(String),
    Rescan,
    Exit
}

pub enum UserEvent {
    TrayIconEvent(TrayIconEvent),
    MenuEvent(MenuEvent)
}

fn ensure_user_config_dirs() {
    if let Ok(home) = std::env::var("USERPROFILE") {
        let root = Path::new(&home).join(".Sentinel");
        for p in [
            root.join("Assets/StatusBar"),
            root.join("Assets/Widgets"),
            root.join("Assets/Wallpapers"),
            root.join("Assets/Themes"),
        ] {
            if let Err(e) = std::fs::create_dir_all(&p) {
                warn!("Failed to create config dir {}: {}", p.display(), e);
            } else {
                info!("Ensured config dir exists: {}", p.display());
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

    let mut addons = discover_addons();
    info!("Discovered {} addons for tray", addons.len());
    let mut children: HashMap<String, Child> = HashMap::new();
    let mut autostart: HashMap<String, bool> =
        addons.iter().map(|a| (a.name.clone(), false)).collect();

    let (mut menu, mut id_map) = build_systray(&addons, &autostart);
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
                        MenuAction::OpenEditor(name) => {
                            if let Some(ad) = addons.iter().find(|a| a.name == name) {
                                let exe = &ad.exe_path;
                                if exe.is_file() {
                                    match Command::new(exe).arg("editor").spawn() {
                                        Ok(_) => info!("[addons] Launched editor for '{}'", name),
                                        Err(e) => error!("[addons] Failed to launch editor '{}': {}", name, e),
                                    }
                                } else {
                                    warn!("[addons] Editor exe not found for '{}': {}", name, exe.display());
                                }
                            }
                        }
                        MenuAction::ToggleAutostart(name) => {
                            let entry = autostart.entry(name.clone()).or_insert(false);
                            *entry = !*entry;
                            info!("[addons] Toggled autostart for '{}': {}", name, *entry);
                            let (new_menu, new_map) = build_systray(&addons, &autostart);
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
                            let (new_menu, new_map) = build_systray(&addons, &autostart);
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