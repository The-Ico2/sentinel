// ~/sentinel/sentinel-backend/src/systemtray/build.rs

use tray_icon::menu::{MenuItem, Menu, Submenu, PredefinedMenuItem, MenuId};
use std::collections::HashMap;

use crate::systemtray::systray::MenuAction;
use crate::Addon;
use crate::{info, warn};

pub fn build_systray(
    addons: &Vec<Addon>,
    autostart: &HashMap<String, bool>
) -> (Menu, HashMap<MenuId, MenuAction>) {
    info!("Building system tray menu...");

    let menu = Menu::new();
    let mut ids: HashMap<MenuId, MenuAction> = HashMap::new();

    if addons.is_empty() {
        warn!("No addons detected when building tray menu");
        let info_item = MenuItem::new("No addons detected", false, None);
        menu.append(&info_item).ok();
    } else {
        for ad in addons {
            info!("Adding submenu for addon '{}'", ad.name);

            // Build submenu for this addon
            let submenu = Submenu::new(&ad.name, true);
            let start = MenuItem::new("Start", true, None);
            let stop = MenuItem::new("Stop", true, None);
            let reload = MenuItem::new("Reload", true, None);
            let editor = MenuItem::new("Editor", true, None);
            let auto_label = if *autostart.get(&ad.name).unwrap_or(&false) {
                "Autostart: On"
            } else {
                "Autostart: Off"
            };
            let auto = MenuItem::new(auto_label, true, None);

            // Capture IDs
            let start_id = start.id().clone();
            let stop_id = stop.id().clone();
            let reload_id = reload.id().clone();
            let editor_id = editor.id().clone();
            let auto_id = auto.id().clone();

            submenu
                .append_items(&[&start, &stop, &reload, &editor, &PredefinedMenuItem::separator(), &auto])
                .ok();

            ids.insert(start_id, MenuAction::Start(ad.name.clone()));
            ids.insert(stop_id, MenuAction::Stop(ad.name.clone()));
            ids.insert(reload_id, MenuAction::Reload(ad.name.clone()));
            ids.insert(editor_id, MenuAction::OpenEditor(ad.name.clone()));
            ids.insert(auto_id, MenuAction::ToggleAutostart(ad.name.clone()));

            menu.append(&submenu).ok();
        }
    }

    // Global actions
    info!("Adding global tray menu actions");
    let rescan = MenuItem::new("Rescan Addons", true, None);
    let exit = MenuItem::new("Exit", true, None);
    let rescan_id = rescan.id().clone();
    let exit_id = exit.id().clone();
    ids.insert(rescan_id, MenuAction::Rescan);
    ids.insert(exit_id, MenuAction::Exit);

    info!("System tray menu build complete with {} addons", addons.len());
    (menu, ids)
}