// extensions.rs — Discovery and loading of VEIL addon UI extensions.
//
// Each addon may ship an `ext.prism.json` manifest describing the pages it
// contributes to the main VEIL UI. At startup, Core scans:
//   1. <install_root>/Addons/*/ext.prism.json   (production)
//   2. <workspace>/Addons/*/ext.prism.json      (dev fallback)
//
// Each declared page becomes a `prism_runtime` `Route` registered on the
// `AppHost`, with a synthetic id of the form `<addon_id>.<page_id>`.
//
// Sources may be either:
//   - `*.html`  — compiled at runtime by PRISM (PageSource::HtmlFile)
//   - `*.prg`   — pre-compiled PrdDocument (PageSource::Document)

use std::path::{Path, PathBuf};
use std::sync::Arc;

use prism_runtime::scene::app_host::{PageSource, Route};
use prism_runtime::PrdDocument;
use serde::Deserialize;

use crate::installer;

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct ExtManifest {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub pages: Vec<ExtPage>,
}

#[derive(Debug, Deserialize)]
pub struct ExtPage {
    pub id: String,
    pub label: String,
    /// Path to the page source, relative to the addon's directory.
    pub source: String,
    /// If true, surface this page in the sidebar's Addons dropdown.
    #[serde(default = "default_true")]
    pub sidebar: bool,
}

fn default_true() -> bool { true }

/// A page that has been resolved into a registerable Route.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct DiscoveredPage {
    pub addon_id: String,
    pub addon_name: String,
    pub page_id: String,
    pub page_label: String,
    pub route_id: String,
    pub sidebar: bool,
}

/// Result of discovering a single addon.
#[allow(dead_code)]
pub struct DiscoveredAddon {
    pub manifest: ExtManifest,
    pub addon_dir: PathBuf,
    pub routes: Vec<Route>,
    pub pages: Vec<DiscoveredPage>,
}

/// Scan all known addon roots and return discovered addons + their routes.
pub fn discover_all() -> Vec<DiscoveredAddon> {
    let mut roots: Vec<PathBuf> = Vec::new();

    if let Some(home) = installer::user_home_dir() {
        roots.push(home.join("ProjectOpen").join("VEIL").join("Addons"));
    }

    // Dev fallback: workspace-local Addons folder next to the running exe's
    // crate root. CARGO_MANIFEST_DIR points at Core/, so go up one.
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if let Some(workspace_root) = manifest_dir.parent() {
        let dev_addons = workspace_root.join("Addons");
        if dev_addons.is_dir() {
            roots.push(dev_addons);
        }
    }

    let mut seen_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut out: Vec<DiscoveredAddon> = Vec::new();

    for root in &roots {
        if !root.is_dir() { continue; }
        let entries = match std::fs::read_dir(root) {
            Ok(e) => e,
            Err(e) => {
                log::warn!("[ext] cannot read addon root {}: {e}", root.display());
                continue;
            }
        };
        for entry in entries.flatten() {
            let addon_dir = entry.path();
            if !addon_dir.is_dir() { continue; }
            let manifest_path = addon_dir.join("ext.prism.json");
            if !manifest_path.is_file() { continue; }

            match load_addon(&addon_dir, &manifest_path) {
                Ok(found) => {
                    if seen_ids.contains(&found.manifest.id) {
                        log::debug!(
                            "[ext] skipping duplicate addon id '{}' from {}",
                            found.manifest.id, addon_dir.display(),
                        );
                        continue;
                    }
                    seen_ids.insert(found.manifest.id.clone());
                    log::info!(
                        "[ext] discovered addon '{}' ({}) with {} page(s) from {}",
                        found.manifest.name,
                        found.manifest.id,
                        found.routes.len(),
                        addon_dir.display(),
                    );
                    out.push(found);
                }
                Err(e) => {
                    log::warn!(
                        "[ext] failed to load {}: {e}",
                        manifest_path.display(),
                    );
                }
            }
        }
    }

    out
}

fn load_addon(addon_dir: &Path, manifest_path: &Path) -> Result<DiscoveredAddon, String> {
    let raw = std::fs::read_to_string(manifest_path)
        .map_err(|e| format!("read manifest: {e}"))?;
    let manifest: ExtManifest = serde_json::from_str(&raw)
        .map_err(|e| format!("parse manifest: {e}"))?;

    if manifest.id.is_empty() {
        return Err("manifest id is empty".into());
    }

    let mut routes = Vec::with_capacity(manifest.pages.len());
    let mut pages = Vec::with_capacity(manifest.pages.len());

    for page in &manifest.pages {
        if page.id.is_empty() {
            log::warn!("[ext] {}: page with empty id, skipping", manifest.id);
            continue;
        }
        let source_path = addon_dir.join(&page.source);
        if !source_path.is_file() {
            log::warn!(
                "[ext] {}.{}: source file not found: {}",
                manifest.id, page.id, source_path.display(),
            );
            continue;
        }

        let route_id = format!("{}.{}", manifest.id, page.id);
        let source = match source_path
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_ascii_lowercase())
            .as_deref()
        {
            Some("prg") => match load_prg(&source_path) {
                Ok(doc) => PageSource::Document(Arc::new(doc)),
                Err(e) => {
                    log::warn!(
                        "[ext] {}: failed to load .prg {}: {e}",
                        route_id, source_path.display(),
                    );
                    continue;
                }
            },
            _ => PageSource::HtmlFile(source_path.clone()),
        };

        routes.push(Route {
            id: route_id.clone(),
            label: page.label.clone(),
            icon: None,
            source,
            separator: false,
        });

        pages.push(DiscoveredPage {
            addon_id: manifest.id.clone(),
            addon_name: manifest.name.clone(),
            page_id: page.id.clone(),
            page_label: page.label.clone(),
            route_id,
            sidebar: page.sidebar,
        });
    }

    Ok(DiscoveredAddon {
        manifest,
        addon_dir: addon_dir.to_path_buf(),
        routes,
        pages,
    })
}

fn load_prg(path: &Path) -> Result<PrdDocument, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read: {e}"))?;
    PrdDocument::from_binary(&bytes)
}

/// Build the JS payload that registers the discovered addons with the
/// front-end so the sidebar can render its hierarchical Addons dropdown.
///
/// The payload sets `window.__veilAddons` (and calls a renderer hook if the
/// framework JS is already loaded). It is safe to call before or after the
/// framework JS has initialised — the framework re-checks the global on
/// `DOMContentLoaded` as well.
pub fn build_sidebar_payload(addons: &[DiscoveredAddon]) -> String {
    #[derive(serde::Serialize)]
    struct AddonOut<'a> {
        id: &'a str,
        name: &'a str,
        pages: Vec<PageOut<'a>>,
    }
    #[derive(serde::Serialize)]
    struct PageOut<'a> {
        id: &'a str,
        label: &'a str,
        route: &'a str,
    }

    let serialised: Vec<AddonOut> = addons
        .iter()
        .map(|a| AddonOut {
            id: &a.manifest.id,
            name: &a.manifest.name,
            pages: a
                .pages
                .iter()
                .filter(|p| p.sidebar)
                .map(|p| PageOut {
                    id: &p.page_id,
                    label: &p.page_label,
                    route: &p.route_id,
                })
                .collect(),
        })
        .collect();

    let json = serde_json::to_string(&serialised).unwrap_or_else(|_| "[]".into());
    format!(
        "window.__veilAddons = {json};\
         if (window.Veil && typeof Veil.renderAddonList === 'function') {{ Veil.renderAddonList(); }}",
    )
}
