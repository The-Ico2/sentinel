// ~/sentinel/sentinel-backend/src/systemtray/build.rs

use tray_icon::menu::{MenuItem, Menu, Submenu, PredefinedMenuItem, MenuId};
use std::collections::HashMap;

use crate::systemtray::systray::MenuAction;
use crate::Addon;
use crate::{info, warn};

/// Builds the main addon menu (shown on single right-click).
/// Contains addon submenus (start/stop/reload/configure/autostart) and Exit.
pub fn build_addon_menu(
    addons: &Vec<Addon>,
    autostart: &HashMap<String, bool>,
) -> (Menu, HashMap<MenuId, MenuAction>) {
    info!("Building addon tray menu...");

    let menu = Menu::new();
    let mut ids: HashMap<MenuId, MenuAction> = HashMap::new();

    if addons.is_empty() {
        warn!("No addons detected when building addon menu");
        let info_item = MenuItem::new("No addons detected", false, None);
        menu.append(&info_item).ok();
    } else {
        for ad in addons {
            info!("Adding submenu for addon '{}'", ad.name);

            let submenu = Submenu::new(&ad.name, true);
            let start = MenuItem::new("Start", true, None);
            let stop = MenuItem::new("Stop", true, None);
            let reload = MenuItem::new("Reload", true, None);
            let configure = MenuItem::new("Configure", true, None);
            let auto_label = if *autostart.get(&ad.name).unwrap_or(&false) {
                "Autostart: On"
            } else {
                "Autostart: Off"
            };
            let auto = MenuItem::new(auto_label, true, None);

            let start_id = start.id().clone();
            let stop_id = stop.id().clone();
            let reload_id = reload.id().clone();
            let configure_id = configure.id().clone();
            let auto_id = auto.id().clone();

            submenu
                .append_items(&[&start, &stop, &reload, &configure, &PredefinedMenuItem::separator(), &auto])
                .ok();

            ids.insert(start_id, MenuAction::Start(ad.name.clone()));
            ids.insert(stop_id, MenuAction::Stop(ad.name.clone()));
            ids.insert(reload_id, MenuAction::Reload(ad.name.clone()));
            ids.insert(configure_id, MenuAction::OpenConfigUi(ad.package.clone()));
            ids.insert(auto_id, MenuAction::ToggleAutostart(ad.name.clone()));

            menu.append(&submenu).ok();
        }
    }

    // Exit is always present in the addon menu
    menu.append(&PredefinedMenuItem::separator()).ok();
    let exit = MenuItem::new("Exit", true, None);
    let exit_id = exit.id().clone();
    ids.insert(exit_id, MenuAction::Exit);
    menu.append(&exit).ok();

    info!("Addon menu build complete with {} addons", addons.len());
    (menu, ids)
}

/// Builds the backend options menu (shown on double right-click).
/// Contains Rescan Addons and Run Sentinel at startup toggle.
pub fn build_backend_menu(
    backend_run_at_startup: bool,
) -> (Menu, HashMap<MenuId, MenuAction>) {
    info!("Building backend options menu...");

    let menu = Menu::new();
    let mut ids: HashMap<MenuId, MenuAction> = HashMap::new();

    let backend_startup_label = if backend_run_at_startup {
        "Run Sentinel at startup: On"
    } else {
        "Run Sentinel at startup: Off"
    };
    let backend_startup = MenuItem::new(backend_startup_label, true, None);
    let rescan = MenuItem::new("Rescan Addons", true, None);

    let backend_startup_id = backend_startup.id().clone();
    let rescan_id = rescan.id().clone();

    ids.insert(backend_startup_id, MenuAction::ToggleBackendStartup);
    ids.insert(rescan_id, MenuAction::Rescan);

    menu.append(&backend_startup).ok();
    menu.append(&rescan).ok();

    info!("Backend options menu build complete");
    (menu, ids)
}