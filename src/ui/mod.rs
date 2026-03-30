mod pages;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use openrender_runtime::gpu::context::GpuContext;
use openrender_runtime::gpu::renderer::Renderer;
use openrender_runtime::scene::app_host::{AppEvent, AppHost, PageSource, Route};
use openrender_runtime::scene::input_handler::{
    KeyCode, Modifiers, MouseButton as CxMouseButton, RawInputEvent,
};
use openrender_runtime::capabilities::{CapabilitySet, NetworkAccess, TrayAccess, SingleInstance};
use openrender_runtime::instance::{self, InstanceLockResult};
use openrender_runtime::tray::{TrayConfig, TrayMenuEntry, TrayMenuItem, TrayMenuAction, TraySubmenu};

use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

use crate::Addon;
use crate::autostart;
use crate::ipc::sysdata::display::MonitorManager;

/// Launch the OpenRender UI for OpenDesktop.
pub fn launch() -> Result<(), Box<dyn std::error::Error>> {
    // Enforce single-instance for the UI process.
    let instance_guard = match instance::acquire_single_instance("OpenDesktop-UI") {
        InstanceLockResult::Acquired(guard) => Some(guard),
        InstanceLockResult::AlreadyRunning => {
            log::info!("Another OpenDesktop UI instance is already running — focusing it.");
            return Ok(());
        }
    };

    let mut host = build_app_host();

    if let Some(guard) = instance_guard {
        host.set_instance_guard(guard);
    }

    let event_loop = EventLoop::new().expect("Failed to create event loop");
    let mut app = OpenDesktopApp::new(host);

    if let Err(e) = event_loop.run_app(&mut app) {
        log::error!("Event loop error: {e}");
    }

    Ok(())
}

/// Load an icon declared via `<include type="icon">` from the compiled document.
fn load_declared_icon(host: &AppHost, target: &str) -> Option<(Vec<u8>, u32, u32)> {
    for decl in host.icon_declarations() {
        if decl.target.is_empty() || decl.target == target {
            let path = std::path::Path::new(&decl.path);
            if path.exists() {
                if let Ok(img) = image::open(path) {
                    let rgba = img.into_rgba8();
                    let w = rgba.width();
                    let h = rgba.height();
                    return Some((rgba.into_raw(), w, h));
                }
            }
        }
    }
    None
}

fn build_app_host() -> AppHost {
    let mut host = AppHost::new("OpenDesktop");
    host.sidebar_width = 0.0; // Sidebar is part of the page HTML

    host.set_capabilities(
        CapabilitySet::new()
            .declare(TrayAccess)
            .declare(NetworkAccess)
            .declare(SingleInstance),
    );

    // Single route: base.html is the template with sidebar + page-content.
    // Individual pages (home, addons, data, settings, store) are loaded as
    // content fragments inside <page-content> via data-navigate sidebar clicks.
    host.add_route(Route {
        id: "home".into(),
        label: "Home".into(),
        icon: None,
        source: PageSource::HtmlFile(pages::base_page()),
        separator: false,
    });

    // Load custom title bar if present.
    host.load_title_bar(&pages::base_page().parent().unwrap_or(std::path::Path::new(".")));

    host.navigate_to("home");
    host
}

// ---------------------------------------------------------------------------
// Application state
// ---------------------------------------------------------------------------

struct OpenDesktopApp {
    host: AppHost,
    window: Option<Arc<Window>>,
    gpu_ctx: Option<GpuContext>,
    renderer: Option<Renderer>,
    last_frame: Instant,
    frame_count: u64,
    fps_timer: Instant,
    current_modifiers: winit::keyboard::ModifiersState,
    exit_requested: bool,
    cursor_pos: (f32, f32),
    // Periodic task timers
    last_heartbeat: Instant,
    last_registry_push: Instant,
    last_config_push: Instant,
    last_monitor_poll: Instant,
    // Cached data for change-detection on pushes
    cached_registry_json: String,
    cached_config_json: String,
    cached_monitor_json: String,
    // Current data filter
    data_filter: String,
    // Addon tray state
    tray_addons: Vec<Addon>,
    tray_settings: autostart::TraySettings,
}

impl OpenDesktopApp {
    fn new(host: AppHost) -> Self {
        let mut tray_settings = autostart::load_tray_settings();

        // Sync the run-at-startup flag with the actual registry state.
        let detected = autostart::is_backend_startup_enabled()
            .unwrap_or(tray_settings.run_backend_at_startup);
        if detected != tray_settings.run_backend_at_startup {
            tray_settings.run_backend_at_startup = detected;
            autostart::save_tray_settings(&tray_settings);
        }

        Self {
            host,
            window: None,
            gpu_ctx: None,
            renderer: None,
            last_frame: Instant::now(),
            frame_count: 0,
            fps_timer: Instant::now(),
            current_modifiers: winit::keyboard::ModifiersState::empty(),
            exit_requested: false,
            cursor_pos: (0.0, 0.0),
            last_heartbeat: Instant::now(),
            last_registry_push: Instant::now(),
            last_config_push: Instant::now(),
            last_monitor_poll: Instant::now(),
            cached_registry_json: String::new(),
            cached_config_json: String::new(),
            cached_monitor_json: String::new(),
            data_filter: "all".to_string(),
            tray_addons: Vec::new(),
            tray_settings,
        }
    }

    fn dispatch_input(&mut self, raw: RawInputEvent) {
        let (vw, vh) = self.viewport_size();
        self.host.handle_input(raw, vw, vh);
    }

    fn viewport_size(&self) -> (f32, f32) {
        let ctx = match self.gpu_ctx.as_ref() {
            Some(c) => c,
            None => return (1280.0, 800.0),
        };
        let scale = self.window.as_ref().map(|w| w.scale_factor() as f32).unwrap_or(1.0);
        (ctx.size.0 as f32 / scale, ctx.size.1 as f32 / scale)
    }

    // -----------------------------------------------------------------------
    // IPC dispatch
    // -----------------------------------------------------------------------

    fn handle_ipc(&mut self, ns: &str, cmd: &str, args: Option<serde_json::Value>) {
        match (ns, cmd) {
            // ── Addons ────────────────────────────────────────────────
            ("addons", "refresh") => self.ipc_addons_refresh(),
            ("addons", "open-folder") => self.ipc_open_od_folder("Addons"),

            // ── Data ──────────────────────────────────────────────────
            ("data", "filter") => {
                if let Some(f) = args.as_ref().and_then(|a| a.get("filter")).and_then(|v| v.as_str()) {
                    self.data_filter = f.to_string();
                    self.push_registry_data();
                }
            }

            // ── Settings ──────────────────────────────────────────────
            ("settings", "toggle-pause") => self.ipc_backend_setting("pull_paused", None),
            ("settings", "toggle-refresh") => self.ipc_backend_setting("refresh_on_request", None),

            // ── Store ─────────────────────────────────────────────────
            ("store", "refresh") => {
                self.host.execute_js(
                    "if(typeof showToast==='function')showToast('Store refresh not yet implemented','info');",
                );
            }

            // ── App ───────────────────────────────────────────────────
            ("app", "open-folder") => self.ipc_open_od_folder(""),
            ("app", "exit") => {
                log::info!("App exit: stopping all addons");
                crate::ipc::addon::stop_all();
                self.exit_requested = true;
            }

            _ => {
                log::debug!("Unhandled IPC: {ns}/{cmd}");
            }
        }
    }

    // -----------------------------------------------------------------------
    // Addons
    // -----------------------------------------------------------------------

    fn ipc_addons_refresh(&mut self) {
        // Discover installed addons from the OD home directory.
        let od_home = match od_home_dir() {
            Some(d) => d,
            None => return,
        };
        let addons_dir = od_home.join("Addons");
        if !addons_dir.exists() {
            self.host.execute_js(
                "if(typeof showToast==='function')showToast('Addons folder not found','warning');",
            );
            return;
        }

        let mut cards = String::new();
        let mut count = 0u32;
        if let Ok(entries) = std::fs::read_dir(&addons_dir) {
            for entry in entries.flatten() {
                if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().to_string();
                let config_path = entry.path().join("config.yaml");
                let has_config = config_path.exists();
                let version_hint = if has_config { "configured" } else { "no config" };

                let icon_char = name.chars().next().unwrap_or('A');
                cards.push_str(&format!(
                    concat!(
                        "<div class=\"addon-card\">",
                          "<div class=\"addon-meta\">",
                            "<div class=\"addon-icon\">{icon}</div>",
                            "<div class=\"addon-info\">",
                              "<span class=\"addon-name\">{name}</span>",
                              "<span class=\"addon-version\">{ver}</span>",
                            "</div>",
                          "</div>",
                        "</div>",
                    ),
                    icon = Self::escape_html(&icon_char.to_string()),
                    name = Self::escape_html(&name),
                    ver = Self::escape_html(version_hint),
                ));
                count += 1;
            }
        }

        let js = format!(
            concat!(
                "var ag=document.getElementById('addon-grid');",
                "if(ag)ag.innerHTML='{cards}';",
                "var ac=document.getElementById('addon-count');if(ac)ac.textContent='{n}';",
                "if(typeof showToast==='function')showToast('Found {n} addon(s)','success');",
            ),
            cards = Self::escape_js(&cards),
            n = count,
        );
        self.host.execute_js(&js);
    }

    // -----------------------------------------------------------------------
    // Backend settings
    // -----------------------------------------------------------------------

    fn ipc_backend_setting(&mut self, key: &str, value: Option<serde_json::Value>) {
        // For toggle commands, we don't know the current state, so we
        // just send the command and let the daemon handle it.
        let (cmd, args) = match key {
            "pull_paused" => {
                let val = value.and_then(|v| v.as_bool()).unwrap_or(true);
                ("set_pull_paused", serde_json::json!({"paused": val}))
            }
            "refresh_on_request" => {
                let val = value.and_then(|v| v.as_bool()).unwrap_or(true);
                ("set_refresh_on_request", serde_json::json!({"enabled": val}))
            }
            _ => {
                log::warn!("Unknown backend setting: {key}");
                return;
            }
        };

        let req = crate::ipc::request::IpcRequest {
            ns: "backend".to_string(),
            cmd: cmd.to_string(),
            args: Some(args),
        };
        match crate::ipc::request::send_ipc_request(req) {
            Ok(resp) if resp.ok => {
                log::info!("Backend setting '{key}' updated");
                self.host.execute_js(
                    &format!("if(typeof showToast==='function')showToast('Setting updated','success');"),
                );
            }
            Ok(resp) => {
                log::warn!("Backend rejected setting '{}': {:?}", key, resp.error);
            }
            Err(e) => {
                log::warn!("Failed to send setting to daemon: {e}");
                self.host.execute_js(
                    "if(typeof showToast==='function')showToast('Backend not running','warning');",
                );
            }
        }
    }

    // -----------------------------------------------------------------------
    // Open folder
    // -----------------------------------------------------------------------

    fn ipc_open_od_folder(&mut self, subfolder: &str) {
        let od_home = match od_home_dir() {
            Some(d) => d,
            None => return,
        };
        let dir = if subfolder.is_empty() {
            od_home
        } else {
            od_home.join(subfolder)
        };
        if dir.exists() {
            #[cfg(target_os = "windows")]
            {
                let _ = std::process::Command::new("explorer").arg(&dir).spawn();
            }
        }
    }

    // -----------------------------------------------------------------------
    // Addon tray management
    // -----------------------------------------------------------------------

    /// Discover addons via IPC and rebuild tray menu with per-addon submenus.
    fn refresh_tray_addons(&mut self) {
        let request = crate::ipc::request::IpcRequest {
            ns: "registry".into(),
            cmd: "list_addons".into(),
            args: None,
        };

        let addons: Vec<Addon> = match crate::ipc::request::send_ipc_request(request) {
            Ok(resp) if resp.ok => {
                resp.data
                    .and_then(|v| v.as_array().cloned())
                    .unwrap_or_default()
                    .iter()
                    .map(|entry| {
                        let name = entry["metadata"]["name"].as_str().unwrap_or("unknown").to_string();
                        let exe_path = entry["exe_path"]
                            .as_str()
                            .or_else(|| entry["entry_path"].as_str())
                            .unwrap_or_default()
                            .into();
                        let dir = entry["path"].as_str().unwrap_or_default().into();
                        let package = entry["id"].as_str().unwrap_or_default().to_string();
                        Addon { name, exe_path, dir, package }
                    })
                    .collect()
            }
            _ => Vec::new(),
        };

        // Merge autostart state: keep existing flags, default new addons to false.
        let new_autostart: HashMap<String, bool> = addons
            .iter()
            .map(|a| {
                let enabled = *self.tray_settings.addon_autostart.get(&a.name).unwrap_or(&false);
                (a.name.clone(), enabled)
            })
            .collect();
        self.tray_settings.addon_autostart = new_autostart;
        autostart::save_tray_settings(&self.tray_settings);

        self.tray_addons = addons;
        self.rebuild_tray_menu();
    }

    /// Build tray menu entries from the current addon list and settings.
    fn build_tray_menu_entries(&self) -> Vec<TrayMenuEntry> {
        let mut items: Vec<TrayMenuEntry> = Vec::new();

        if self.tray_addons.is_empty() {
            items.push(TrayMenuEntry::Item(
                TrayMenuItem::new("no-addons", "No addons detected").disabled(),
            ));
        } else {
            for addon in &self.tray_addons {
                let name = &addon.name;
                let pkg = &addon.package;
                let auto_label = if *self.tray_settings.addon_autostart.get(name).unwrap_or(&false) {
                    "Autostart: On"
                } else {
                    "Autostart: Off"
                };

                let sub_items = vec![
                    TrayMenuEntry::Item(
                        TrayMenuItem::new(format!("addon-start:{name}"), "Start")
                            .with_action(TrayMenuAction::Custom(format!("addon-start:{name}"))),
                    ),
                    TrayMenuEntry::Item(
                        TrayMenuItem::new(format!("addon-stop:{name}"), "Stop")
                            .with_action(TrayMenuAction::Custom(format!("addon-stop:{name}"))),
                    ),
                    TrayMenuEntry::Item(
                        TrayMenuItem::new(format!("addon-reload:{name}"), "Reload")
                            .with_action(TrayMenuAction::Custom(format!("addon-reload:{name}"))),
                    ),
                    TrayMenuEntry::Item(
                        TrayMenuItem::new(format!("addon-configure:{pkg}"), "Configure")
                            .with_action(TrayMenuAction::Custom(format!("addon-configure:{pkg}"))),
                    ),
                    TrayMenuEntry::Separator,
                    TrayMenuEntry::Item(
                        TrayMenuItem::new(format!("addon-autostart:{name}"), auto_label)
                            .with_action(TrayMenuAction::Custom(format!("addon-autostart:{name}"))),
                    ),
                ];

                items.push(TrayMenuEntry::Submenu(TraySubmenu::new(name.clone(), sub_items)));
            }
        }

        // Backend options
        items.push(TrayMenuEntry::Separator);

        let startup_label = if self.tray_settings.run_backend_at_startup {
            "Run at startup: On"
        } else {
            "Run at startup: Off"
        };
        items.push(TrayMenuEntry::Item(
            TrayMenuItem::new("backend-startup", startup_label)
                .with_action(TrayMenuAction::Custom("backend-startup".into())),
        ));
        items.push(TrayMenuEntry::Item(
            TrayMenuItem::new("rescan", "Rescan Addons")
                .with_action(TrayMenuAction::Custom("rescan".into())),
        ));

        items
    }

    /// Push the current addon menu to the system tray.
    fn rebuild_tray_menu(&mut self) {
        let items = self.build_tray_menu_entries();
        self.host.update_tray_menu(&items);
    }

    /// Handle a custom tray action (addon start/stop/reload/configure/autostart, backend options).
    fn handle_tray_action(&mut self, action: &str) {
        if let Some(addon_name) = action.strip_prefix("addon-start:") {
            self.tray_addon_ipc("addon", "start", addon_name);
        } else if let Some(addon_name) = action.strip_prefix("addon-stop:") {
            self.tray_addon_ipc("addon", "stop", addon_name);
        } else if let Some(addon_name) = action.strip_prefix("addon-reload:") {
            self.tray_addon_ipc("addon", "reload", addon_name);
        } else if let Some(addon_id) = action.strip_prefix("addon-configure:") {
            self.tray_open_config_ui(addon_id);
        } else if let Some(addon_name) = action.strip_prefix("addon-autostart:") {
            self.tray_toggle_autostart(addon_name);
        } else if action == "backend-startup" {
            self.tray_toggle_backend_startup();
        } else if action == "rescan" {
            self.refresh_tray_addons();
        } else {
            log::debug!("Unhandled tray action: {action}");
        }
    }

    fn tray_addon_ipc(&self, ns: &str, cmd: &str, addon_name: &str) {
        let req = crate::ipc::request::IpcRequest {
            ns: ns.to_string(),
            cmd: cmd.to_string(),
            args: Some(serde_json::json!({"addon_name": addon_name})),
        };
        match crate::ipc::request::send_ipc_request(req) {
            Ok(resp) if resp.ok => log::info!("[tray] {cmd} '{}' via IPC", addon_name),
            Ok(resp) => log::error!("[tray] Failed to {cmd} '{}': {}", addon_name, resp.error.unwrap_or_default()),
            Err(e) => log::error!("[tray] IPC error {cmd} '{}': {}", addon_name, e),
        }
    }

    fn tray_open_config_ui(&self, addon_id: &str) {
        match std::env::current_exe() {
            Ok(exe) => {
                match std::process::Command::new(exe)
                    .arg("--addon-config-ui")
                    .arg(addon_id)
                    .spawn()
                {
                    Ok(_) => log::info!("[tray] Opened config UI for '{}'", addon_id),
                    Err(e) => log::error!("[tray] Failed to open config UI for '{}': {}", addon_id, e),
                }
            }
            Err(e) => log::error!("[tray] Failed to resolve executable for config UI: {}", e),
        }
    }

    fn tray_toggle_autostart(&mut self, addon_name: &str) {
        let entry = self.tray_settings.addon_autostart.entry(addon_name.to_string()).or_insert(false);
        *entry = !*entry;
        let enabled = *entry;
        autostart::save_tray_settings(&self.tray_settings);
        log::info!("[tray] Toggled autostart for '{}': {}", addon_name, enabled);
        self.rebuild_tray_menu();
    }

    fn tray_toggle_backend_startup(&mut self) {
        let next = !self.tray_settings.run_backend_at_startup;
        match autostart::set_backend_startup_enabled(next) {
            Ok(_) => {
                self.tray_settings.run_backend_at_startup = next;
                autostart::save_tray_settings(&self.tray_settings);
                log::info!("[tray] Run at startup toggled: {}", next);
                self.rebuild_tray_menu();
            }
            Err(e) => log::error!("[tray] Failed to toggle run at startup: {}", e),
        }
    }

    // -----------------------------------------------------------------------
    // Periodic data pushes
    // -----------------------------------------------------------------------

    fn send_ui_heartbeat(&mut self) {
        let req = crate::ipc::request::IpcRequest {
            ns: "backend".to_string(),
            cmd: "ui_heartbeat".to_string(),
            args: None,
        };
        let _ = crate::ipc::request::send_ipc_request(req);
    }

    fn push_registry_data(&mut self) {
        // Fetch full registry snapshot from the daemon via IPC.
        let req = crate::ipc::request::IpcRequest {
            ns: "registry".to_string(),
            cmd: "full".to_string(),
            args: None,
        };
        let json = match crate::ipc::request::send_ipc_request(req) {
            Ok(resp) if resp.ok => {
                serde_json::to_string(&resp.data).unwrap_or_default()
            }
            _ => return,
        };
        if json == self.cached_registry_json || json.is_empty() {
            return;
        }
        self.cached_registry_json = json.clone();

        let js = format!(
            "if(typeof __odPushRegistry==='function')__odPushRegistry({});",
            json,
        );
        self.host.execute_js(&js);
    }

    fn push_config_data(&mut self) {
        let od_home = match od_home_dir() {
            Some(d) => d,
            None => return,
        };
        let config_path = od_home.join("config.yaml");
        let yaml_text = match std::fs::read_to_string(&config_path) {
            Ok(t) => t,
            Err(_) => return,
        };
        let cfg: crate::config::BackendConfig = match serde_yaml::from_str(&yaml_text) {
            Ok(c) => c,
            Err(_) => return,
        };
        let cfg_json = match serde_json::to_string(&cfg) {
            Ok(j) => j,
            Err(_) => return,
        };
        if cfg_json == self.cached_config_json {
            return;
        }
        self.cached_config_json = cfg_json.clone();
        let js = format!(
            "window.__odConfig={};if(typeof __odOnConfigPush==='function')__odOnConfigPush(window.__odConfig);",
            cfg_json,
        );
        self.host.execute_js(&js);
    }

    fn push_monitor_data(&mut self) {
        let monitors: Vec<serde_json::Value> = MonitorManager::enumerate_monitors()
            .into_iter()
            .map(|m| {
                serde_json::json!({
                    "id": m.id,
                    "x": m.x,
                    "y": m.y,
                    "width": m.width,
                    "height": m.height,
                    "scale": m.scale,
                    "primary": m.primary,
                })
            })
            .collect();
        let json = match serde_json::to_string(&monitors) {
            Ok(j) => j,
            Err(_) => return,
        };
        if json == self.cached_monitor_json {
            return;
        }
        self.cached_monitor_json = json.clone();

        // Update the monitor count on the home page.
        let count = monitors.len();
        let js = format!(
            concat!(
                "var mc=document.getElementById('monitor-count');if(mc)mc.textContent='{}';",
                "if(typeof __odPushMonitors==='function')__odPushMonitors({});",
            ),
            count, json,
        );
        self.host.execute_js(&js);
    }

    fn run_periodic_tasks(&mut self) {
        // UI heartbeat every 500ms
        if self.last_heartbeat.elapsed().as_millis() >= 500 {
            self.last_heartbeat = Instant::now();
            self.send_ui_heartbeat();
        }

        // Monitor poll every 2s
        if self.last_monitor_poll.elapsed().as_secs() >= 2 {
            self.last_monitor_poll = Instant::now();
            self.push_monitor_data();
        }

        // Config push every 2s
        if self.last_config_push.elapsed().as_secs() >= 2 {
            self.last_config_push = Instant::now();
            self.push_config_data();
        }

        // Registry push every 200ms
        if self.last_registry_push.elapsed().as_millis() >= 200 {
            self.last_registry_push = Instant::now();
            self.push_registry_data();
        }
    }

    // -----------------------------------------------------------------------
    // Render
    // -----------------------------------------------------------------------

    fn render_frame(&mut self) {
        let now = Instant::now();
        let dt = now.duration_since(self.last_frame).as_secs_f32();
        self.last_frame = now;

        // FPS counter (every 128 frames).
        self.frame_count += 1;
        if self.frame_count & 0x7F == 0 {
            let elapsed = self.fps_timer.elapsed().as_secs_f64();
            if elapsed >= 2.0 {
                let fps = self.frame_count as f64 / elapsed;
                log::debug!("FPS: {fps:.1}");
                self.frame_count = 0;
                self.fps_timer = Instant::now();
            }
        }

        // Periodic tasks
        self.run_periodic_tasks();

        // Tick the AppHost
        let (vw, vh, scale, ctx_w, ctx_h, events) = {
            let (ctx, renderer) = match (self.gpu_ctx.as_ref(), self.renderer.as_mut()) {
                (Some(c), Some(r)) => (c, r),
                _ => return,
            };

            let scale = self
                .window
                .as_ref()
                .map(|w| w.scale_factor() as f32)
                .unwrap_or(1.0);

            let vw = ctx.size.0 as f32 / scale;
            let vh = ctx.size.1 as f32 / scale;

            let events = self.host.tick(vw, vh, dt, &mut renderer.font_system);
            (vw, vh, scale, ctx.size.0, ctx.size.1, events)
        };

        for event in events {
            match event {
                AppEvent::NavigateTo(page_id) => {
                    log::info!("Navigated to: {page_id}");
                    if self.host.active_page() != Some(&page_id) {
                        self.host.navigate_to(&page_id);
                        self.host.init_js_for_active_page(ctx_w, ctx_h);
                    }
                    match page_id.as_str() {
                        "home" => {
                            self.push_monitor_data();
                            self.ipc_addons_refresh();
                        }
                        "addons" => self.ipc_addons_refresh(),
                        "data" => self.push_registry_data(),
                        "settings" => self.push_config_data(),
                        _ => {}
                    }
                }
                AppEvent::ContentSwapped { content_id } => {
                    log::info!("Content swapped to: {content_id}");
                    match content_id.as_str() {
                        "home" => {
                            self.push_monitor_data();
                            self.ipc_addons_refresh();
                        }
                        "addons" => self.ipc_addons_refresh(),
                        "data" => self.push_registry_data(),
                        "settings" => self.push_config_data(),
                        _ => {}
                    }
                }
                AppEvent::PageReloaded(page_id) => {
                    log::info!("Page reloaded: {page_id}");
                    self.host.init_js_for_active_page(ctx_w, ctx_h);
                }
                AppEvent::OpenExternal(url) => {
                    log::info!("Open external: {url}");
                    #[cfg(target_os = "windows")]
                    {
                        let _ =
                            std::process::Command::new("cmd").args(["/C", "start", &url]).spawn();
                    }
                }
                AppEvent::TrayShowWindow => {
                    if let Some(ref win) = self.window {
                        win.set_visible(true);
                        win.focus_window();
                    }
                }
                AppEvent::TrayToggleWindow => {
                    if let Some(ref win) = self.window {
                        if win.is_visible().unwrap_or(true) {
                            win.set_visible(false);
                        } else {
                            win.set_visible(true);
                            win.focus_window();
                        }
                    }
                }
                AppEvent::TrayAction(action) => {
                    self.handle_tray_action(&action);
                }
                AppEvent::CloseRequested => {
                    self.exit_requested = true;
                }
                AppEvent::SetTitle(title) => {
                    if let Some(ref win) = self.window {
                        win.set_title(&title);
                    }
                }
                AppEvent::MinimizeRequested => {
                    if let Some(ref win) = self.window {
                        win.set_minimized(true);
                    }
                }
                AppEvent::MaximizeToggleRequested => {
                    if let Some(ref win) = self.window {
                        win.set_maximized(!win.is_maximized());
                    }
                }
                AppEvent::WindowDragRequested => {
                    if let Some(ref win) = self.window {
                        let _ = win.drag_window();
                    }
                }
                AppEvent::IpcCommand { ns, cmd, args } => {
                    self.handle_ipc(&ns, &cmd, args);
                }
                _ => {}
            }
        }

        // Re-borrow for rendering.
        let (ctx, renderer) = match (self.gpu_ctx.as_ref(), self.renderer.as_mut()) {
            (Some(c), Some(r)) => (c, r),
            _ => return,
        };

        // Upload dirty canvas textures.
        let dirty = self.host.dirty_canvases();
        for (canvas_id, _node, width, height, rgba) in dirty {
            let slot = self.host.canvas_slot(canvas_id);
            renderer.upload_canvas_texture(&ctx.device, &ctx.queue, slot, width, height, &rgba);
        }
        self.host.commit_canvas_uploads();

        // Get scene instances and DevTools instances.
        let (scene_instances, devtools_instances, clear_color) =
            self.host.split_instances(vw, vh);

        // Prepare scene text areas.
        let mut text_areas = if let Some(scene) = self.host.active_scene() {
            scene.text_areas()
        } else {
            Vec::new()
        };
        if let Some(tb_scene) = self.host.title_bar_scene() {
            text_areas.extend(tb_scene.text_areas());
        }

        // DevTools text.
        let devtools_entries = self.host.devtools_text_entries(vw, vh);
        let mut devtools_buffers: Vec<glyphon::Buffer> = Vec::new();
        for entry in &devtools_entries {
            let font_size = entry.font_size;
            let line_height = font_size * 1.3;
            let metrics = glyphon::Metrics::new(font_size, line_height);
            let mut buffer = glyphon::Buffer::new(&mut renderer.font_system, metrics);
            let weight = if entry.bold {
                glyphon::Weight(700)
            } else {
                glyphon::Weight(400)
            };
            let attrs = glyphon::Attrs::new()
                .family(glyphon::Family::SansSerif)
                .weight(weight);
            buffer.set_size(&mut renderer.font_system, Some(entry.width), None);
            buffer.set_text(
                &mut renderer.font_system,
                &entry.text,
                &attrs,
                glyphon::Shaping::Advanced,
                None,
            );
            buffer.shape_until_scroll(&mut renderer.font_system, false);
            devtools_buffers.push(buffer);
        }
        let mut devtools_text_areas: Vec<glyphon::TextArea<'_>> = Vec::new();
        for (i, entry) in devtools_entries.iter().enumerate() {
            if let Some(buf) = devtools_buffers.get(i) {
                let c = entry.color;
                devtools_text_areas.push(glyphon::TextArea {
                    buffer: buf,
                    left: entry.x,
                    top: entry.y,
                    scale: 1.0,
                    bounds: glyphon::TextBounds {
                        left: entry.x as i32,
                        top: entry.y as i32,
                        right: (entry.x + entry.width) as i32,
                        bottom: (entry.y + entry.font_size * 2.0) as i32,
                    },
                    default_color: glyphon::Color::rgba(
                        (c.r * 255.0) as u8,
                        (c.g * 255.0) as u8,
                        (c.b * 255.0) as u8,
                        (c.a * 255.0) as u8,
                    ),
                    custom_glyphs: &[],
                });
            }
        }

        // Context menu overlay.
        let ctx_menu_instances = self.host.context_menu_instances();
        let ctx_menu_entries = self.host.context_menu_text_entries();
        let mut ctx_menu_buffers: Vec<glyphon::Buffer> = Vec::new();
        for entry in &ctx_menu_entries {
            let font_size = entry.font_size;
            let line_height = font_size * 1.3;
            let metrics = glyphon::Metrics::new(font_size, line_height);
            let mut buffer = glyphon::Buffer::new(&mut renderer.font_system, metrics);
            let weight = if entry.bold { glyphon::Weight(700) } else { glyphon::Weight(400) };
            let attrs = glyphon::Attrs::new()
                .family(glyphon::Family::SansSerif)
                .weight(weight);
            buffer.set_size(&mut renderer.font_system, Some(entry.width), None);
            buffer.set_text(
                &mut renderer.font_system,
                &entry.text,
                &attrs,
                glyphon::Shaping::Advanced,
                None,
            );
            buffer.shape_until_scroll(&mut renderer.font_system, false);
            ctx_menu_buffers.push(buffer);
        }
        let mut ctx_menu_text_areas: Vec<glyphon::TextArea<'_>> = Vec::new();
        for (i, entry) in ctx_menu_entries.iter().enumerate() {
            if let Some(buf) = ctx_menu_buffers.get(i) {
                let c = entry.color;
                ctx_menu_text_areas.push(glyphon::TextArea {
                    buffer: buf,
                    left: entry.x,
                    top: entry.y,
                    scale: 1.0,
                    bounds: glyphon::TextBounds {
                        left: entry.x as i32,
                        top: entry.y as i32,
                        right: (entry.x + entry.width) as i32,
                        bottom: (entry.y + entry.font_size * 2.0) as i32,
                    },
                    default_color: glyphon::Color::rgba(
                        (c.r * 255.0) as u8,
                        (c.g * 255.0) as u8,
                        (c.b * 255.0) as u8,
                        (c.a * 255.0) as u8,
                    ),
                    custom_glyphs: &[],
                });
            }
        }

        // Render triple-layered: scene → devtools → context menu
        renderer.begin_frame(ctx, dt, scale);
        match renderer.render_triple_layered(
            ctx,
            &scene_instances,
            text_areas,
            &devtools_instances,
            devtools_text_areas,
            &ctx_menu_instances,
            ctx_menu_text_areas,
            clear_color,
        ) {
            Ok(()) => {}
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                if let Some(ref mut gpu) = self.gpu_ctx {
                    let (w, h) = gpu.size;
                    gpu.resize(w, h);
                }
            }
            Err(wgpu::SurfaceError::OutOfMemory) => {
                log::error!("GPU out of memory — exiting");
                std::process::exit(1);
            }
            Err(e) => {
                log::warn!("Surface error: {e:?}");
            }
        }
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn escape_html(s: &str) -> String {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    }

    fn escape_js(s: &str) -> String {
        s.replace('\\', "\\\\")
            .replace('\'', "\\'")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
    }
}

// ---------------------------------------------------------------------------
// OD home directory helper
// ---------------------------------------------------------------------------

fn od_home_dir() -> Option<std::path::PathBuf> {
    dirs_next::home_dir().map(|h| h.join("ProjectOpen").join("OpenDesktop"))
}

// ---------------------------------------------------------------------------
// winit ApplicationHandler
// ---------------------------------------------------------------------------

impl ApplicationHandler for OpenDesktopApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        log::info!("Initialising OpenDesktop window...");

        let (icon_rgba, icon_w, icon_h) = load_declared_icon(&self.host, "window")
            .unwrap_or_else(|| generate_default_icon());
        let window_icon = winit::window::Icon::from_rgba(icon_rgba, icon_w, icon_h).ok();

        let attrs = WindowAttributes::default()
            .with_title("OpenDesktop")
            .with_inner_size(PhysicalSize::new(1280u32, 800u32))
            .with_decorations(!self.host.has_custom_title_bar)
            .with_window_icon(window_icon);

        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                log::error!("Failed to create window: {e}");
                event_loop.exit();
                return;
            }
        };

        let gpu_ctx = match pollster::block_on(GpuContext::new(window.clone())) {
            Ok(ctx) => ctx,
            Err(e) => {
                log::error!("GPU init failed: {e}");
                event_loop.exit();
                return;
            }
        };

        let renderer = match Renderer::new(&gpu_ctx) {
            Ok(r) => r,
            Err(e) => {
                log::error!("Renderer init failed: {e}");
                event_loop.exit();
                return;
            }
        };

        self.window = Some(window.clone());
        self.gpu_ctx = Some(gpu_ctx);
        self.renderer = Some(renderer);
        self.last_frame = Instant::now();
        self.fps_timer = Instant::now();

        // Initialize system tray.
        let (tray_rgba, tray_w, tray_h) = load_declared_icon(&self.host, "system")
            .unwrap_or_else(|| generate_default_icon());
        self.host.init_tray_with_config(TrayConfig {
            enabled: true,
            tooltip: "OpenDesktop".to_string(),
            icon_rgba: Some((tray_rgba, tray_w, tray_h)),
            ..TrayConfig::default()
        });

        // Discover addons and populate tray menu with addon controls.
        self.refresh_tray_addons();

        // Initialize JS runtime for the active page.
        let (w, h) = self.gpu_ctx.as_ref().map(|c| c.size).unwrap_or((1280, 800));
        self.host.init_js_for_active_page(w, h);

        // Populate DevTools GPU info.
        if let Some(ref ctx) = self.gpu_ctx {
            let info = ctx.adapter.get_info();
            self.host.set_gpu_info(format!(
                "{} ({:?})",
                info.name,
                info.backend,
            ));
        }

        window.request_redraw();
        event_loop.set_control_flow(ControlFlow::Poll);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                if self.host.has_active_tray() {
                    log::info!("Minimizing to system tray.");
                    if let Some(ref win) = self.window {
                        win.set_visible(false);
                    }
                } else {
                    log::info!("Close requested — shutting down.");
                    event_loop.exit();
                }
            }

            WindowEvent::Resized(new_size) => {
                if let Some(ref mut ctx) = self.gpu_ctx {
                    ctx.resize(new_size.width, new_size.height);
                    if let Some(scene) = self.host.active_scene_mut() {
                        scene.invalidate_layout();
                    }
                }
            }

            WindowEvent::RedrawRequested => {
                self.render_frame();
                if self.exit_requested {
                    event_loop.exit();
                    return;
                }
                if let Some(ref w) = self.window {
                    w.request_redraw();
                }
            }

            WindowEvent::CursorMoved { position, .. } => {
                let scale = self.window.as_ref().map(|w| w.scale_factor() as f32).unwrap_or(1.0);
                let lx = position.x as f32 / scale;
                let ly = position.y as f32 / scale;
                self.cursor_pos = (lx, ly);
                self.dispatch_input(RawInputEvent::MouseMove { x: lx, y: ly });
            }

            WindowEvent::MouseInput { state, button, .. } => {
                let btn = match button {
                    winit::event::MouseButton::Left => CxMouseButton::Left,
                    winit::event::MouseButton::Right => CxMouseButton::Right,
                    winit::event::MouseButton::Middle => CxMouseButton::Middle,
                    _ => return,
                };
                let raw = match state {
                    winit::event::ElementState::Pressed => {
                        RawInputEvent::MouseDown { x: 0.0, y: 0.0, button: btn }
                    }
                    winit::event::ElementState::Released => {
                        RawInputEvent::MouseUp { x: 0.0, y: 0.0, button: btn }
                    }
                };
                self.dispatch_input(raw);
            }

            WindowEvent::MouseWheel { delta, .. } => {
                let (dx, dy) = match delta {
                    winit::event::MouseScrollDelta::LineDelta(x, y) => (x * 40.0, y * 40.0),
                    winit::event::MouseScrollDelta::PixelDelta(pos) => {
                        (pos.x as f32, pos.y as f32)
                    }
                };
                self.dispatch_input(RawInputEvent::MouseWheel {
                    x: self.cursor_pos.0,
                    y: self.cursor_pos.1,
                    delta_x: dx,
                    delta_y: dy,
                });
            }

            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == winit::event::ElementState::Pressed {
                    let mods = Modifiers {
                        ctrl: self.current_modifiers.control_key(),
                        shift: self.current_modifiers.shift_key(),
                        alt: self.current_modifiers.alt_key(),
                    };
                    let key = winit_key_to_cx(&event.logical_key);
                    self.dispatch_input(RawInputEvent::KeyDown {
                        key,
                        modifiers: mods,
                    });

                    if let Some(text) = &event.text {
                        let s = text.to_string();
                        if !s.is_empty() && !mods.ctrl && !mods.alt {
                            let ch = s.chars().next().unwrap_or('\0');
                            if !ch.is_control() {
                                self.dispatch_input(RawInputEvent::TextInput { text: s });
                            }
                        }
                    }
                }
            }

            WindowEvent::ModifiersChanged(modifiers) => {
                self.current_modifiers = modifiers.state();
            }

            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        for event in self.host.poll_tray() {
            match event {
                AppEvent::TrayShowWindow => {
                    if let Some(ref win) = self.window {
                        win.set_visible(true);
                        win.focus_window();
                    }
                }
                AppEvent::TrayToggleWindow => {
                    if let Some(ref win) = self.window {
                        if win.is_visible().unwrap_or(true) {
                            win.set_visible(false);
                        } else {
                            win.set_visible(true);
                            win.focus_window();
                        }
                    }
                }
                AppEvent::TrayAction(action) => {
                    self.handle_tray_action(&action);
                }
                AppEvent::CloseRequested => {
                    log::info!("Tray exit: stopping all addons");
                    crate::ipc::addon::stop_all();
                    self.exit_requested = true;
                }
                _ => {}
            }
        }

        if let Some(ref w) = self.window {
            w.request_redraw();
        }
        if self.exit_requested {
            event_loop.exit();
        }
    }
}

// ---------------------------------------------------------------------------
// Default icon generator (solid-colour 32x32 RGBA)
// ---------------------------------------------------------------------------

fn generate_default_icon() -> (Vec<u8>, u32, u32) {
    let (w, h) = (32u32, 32u32);
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for _ in 0..(w * h) {
        rgba.extend_from_slice(&[59, 130, 246, 255]); // accent blue
    }
    (rgba, w, h)
}

// ---------------------------------------------------------------------------
// Key mapping
// ---------------------------------------------------------------------------

fn winit_key_to_cx(key: &winit::keyboard::Key) -> KeyCode {
    use winit::keyboard::{Key as WKey, NamedKey};
    match key {
        WKey::Named(NamedKey::Enter) => KeyCode::Enter,
        WKey::Named(NamedKey::Tab) => KeyCode::Tab,
        WKey::Named(NamedKey::Escape) => KeyCode::Escape,
        WKey::Named(NamedKey::Backspace) => KeyCode::Backspace,
        WKey::Named(NamedKey::Delete) => KeyCode::Delete,
        WKey::Named(NamedKey::ArrowLeft) => KeyCode::Left,
        WKey::Named(NamedKey::ArrowRight) => KeyCode::Right,
        WKey::Named(NamedKey::ArrowUp) => KeyCode::Up,
        WKey::Named(NamedKey::ArrowDown) => KeyCode::Down,
        WKey::Named(NamedKey::Home) => KeyCode::Home,
        WKey::Named(NamedKey::End) => KeyCode::End,
        WKey::Named(NamedKey::PageUp) => KeyCode::PageUp,
        WKey::Named(NamedKey::PageDown) => KeyCode::PageDown,
        WKey::Named(NamedKey::Space) => KeyCode::Space,
        WKey::Character(c) => match c.as_str() {
            "a" | "A" => KeyCode::A,
            "c" | "C" => KeyCode::C,
            "v" | "V" => KeyCode::V,
            "x" | "X" => KeyCode::X,
            "z" | "Z" => KeyCode::Z,
            _ => KeyCode::Other(c.chars().next().unwrap_or('\0') as u32),
        },
        _ => KeyCode::Other(0),
    }
}
