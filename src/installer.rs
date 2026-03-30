// installer.rs — Reusable self-installer for ProjectOpen applications.
//
// Drop this file into any ProjectOpen Rust project and configure it via
// `InstallerConfig`. It handles:
//
//   1. Resolving the canonical app root: ~/ProjectOpen/<app-name>/
//   2. Creating the directory structure (configurable subdirs)
//   3. Self-install: copying the exe into the app root and relaunching
//   4. Addon bootstrap: copying addon exes into <root>/Addons/<name>/bin/
//
// Usage (core app):
//
//   ```rust
//   mod installer;
//
//   fn main() {
//       let config = installer::InstallerConfig::core("OpenDesktop")
//           .exe_name("od-core.exe")
//           .subdirs(&["Addons", "Assets", "logs"]);
//       installer::bootstrap(&config, log_fn);
//       // ... rest of your app
//   }
//   ```
//
// Usage (addon):
//
//   ```rust
//   mod installer;
//
//   fn main() {
//       let config = installer::InstallerConfig::addon("OpenDesktop", "wallpaper")
//           .exe_name("od-wallpaper.exe")
//           .addon_subdirs(&["options"]);
//       installer::bootstrap(&config, log_fn);
//       // ... rest of your addon
//   }
//   ```

use std::fs;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// What kind of component is being installed.
#[derive(Clone, Debug)]
pub enum ComponentKind {
    /// A top-level application binary that lives directly in the app root.
    Core,
    /// An addon that lives under `<root>/Addons/<addon_name>/bin/`.
    Addon { name: String },
}

/// Configuration for the self-installer.
///
/// All fields have sensible defaults — only `app_name` is required.
#[derive(Clone, Debug)]
pub struct InstallerConfig {
    /// The ProjectOpen application name (e.g. "OpenDesktop", "OpenPeripheral").
    /// Determines the root directory: `~/ProjectOpen/<app_name>/`.
    pub app_name: String,

    /// What kind of component this binary is.
    pub kind: ComponentKind,

    /// The filename to install the exe as (e.g. "od-core.exe").
    /// Defaults to the current exe's filename.
    pub exe_name: Option<String>,

    /// Subdirectories to create under the app root (core) or addon dir (addon).
    /// For core: e.g. `["Addons", "Assets", "logs"]`
    /// For addons: e.g. `["options"]` (created under `<root>/Addons/<name>/`)
    pub subdirs: Vec<String>,

    /// Whether to self-install (copy exe + relaunch). Default: true.
    pub self_install: bool,

    /// Whether to exit the current process after a successful relaunch.
    /// Default: true. Set to false if you want to handle the relaunch yourself.
    pub exit_after_relaunch: bool,
}

impl InstallerConfig {
    /// Create a config for a core application binary.
    pub fn core(app_name: &str) -> Self {
        Self {
            app_name: app_name.to_string(),
            kind: ComponentKind::Core,
            exe_name: None,
            subdirs: Vec::new(),
            self_install: true,
            exit_after_relaunch: true,
        }
    }

    /// Create a config for an addon binary.
    pub fn addon(app_name: &str, addon_name: &str) -> Self {
        Self {
            app_name: app_name.to_string(),
            kind: ComponentKind::Addon { name: addon_name.to_string() },
            exe_name: None,
            subdirs: Vec::new(),
            self_install: true,
            exit_after_relaunch: true,
        }
    }

    /// Set the installed exe filename.
    pub fn exe_name(mut self, name: &str) -> Self {
        self.exe_name = Some(name.to_string());
        self
    }

    /// Set subdirectories to create under the install root.
    pub fn subdirs(mut self, dirs: &[&str]) -> Self {
        self.subdirs = dirs.iter().map(|s| s.to_string()).collect();
        self
    }

    /// Alias for `subdirs` — reads better for addon configs.
    pub fn addon_subdirs(self, dirs: &[&str]) -> Self {
        self.subdirs(dirs)
    }

    /// Disable self-install (directory creation still happens).
    pub fn no_self_install(mut self) -> Self {
        self.self_install = false;
        self
    }

    /// Don't exit after relaunch (caller handles it).
    pub fn no_exit_after_relaunch(mut self) -> Self {
        self.exit_after_relaunch = false;
        self
    }
}

// ---------------------------------------------------------------------------
// Logging callback
// ---------------------------------------------------------------------------

/// Log level for installer messages.
#[derive(Clone, Copy, Debug)]
pub enum LogLevel {
    Info,
    Warn,
    Error,
}

/// Logging callback type. The installer doesn't depend on any logging crate —
/// you provide a closure that routes messages to your own logger.
///
/// Example:
/// ```rust
/// |level, msg| match level {
///     installer::LogLevel::Info  => info!("{}", msg),
///     installer::LogLevel::Warn  => warn!("{}", msg),
///     installer::LogLevel::Error => error!("{}", msg),
/// }
/// ```
pub type LogFn = fn(LogLevel, &str);

// ---------------------------------------------------------------------------
// Path resolution
// ---------------------------------------------------------------------------

/// Resolve the user's home directory (Windows-first).
pub fn user_home_dir() -> Option<PathBuf> {
    if let Ok(profile) = std::env::var("USERPROFILE") {
        return Some(PathBuf::from(profile));
    }
    let drive = std::env::var("HOMEDRIVE").ok();
    let path = std::env::var("HOMEPATH").ok();
    match (drive, path) {
        (Some(d), Some(p)) => Some(PathBuf::from(format!("{d}{p}"))),
        _ => None,
    }
}

/// Resolve the canonical app root: `~/ProjectOpen/<app_name>/`.
pub fn app_root(app_name: &str) -> Option<PathBuf> {
    user_home_dir().map(|home| home.join("ProjectOpen").join(app_name))
}

/// Resolve the install directory for a given config.
/// - Core: `~/ProjectOpen/<app_name>/`
/// - Addon: `~/ProjectOpen/<app_name>/Addons/<addon_name>/`
pub fn install_dir(config: &InstallerConfig) -> Option<PathBuf> {
    let root = app_root(&config.app_name)?;
    match &config.kind {
        ComponentKind::Core => Some(root),
        ComponentKind::Addon { name } => Some(root.join("Addons").join(name)),
    }
}

/// The directory where the exe is placed.
/// - Core: the app root itself.
/// - Addon: `<addon_dir>/bin/`.
pub fn exe_dir(config: &InstallerConfig) -> Option<PathBuf> {
    let dir = install_dir(config)?;
    match &config.kind {
        ComponentKind::Core => Some(dir),
        ComponentKind::Addon { .. } => Some(dir.join("bin")),
    }
}

/// Returns true if the currently running exe is inside its install directory.
pub fn is_installed(config: &InstallerConfig) -> bool {
    let target_dir = match exe_dir(config) {
        Some(d) => d,
        None => return false,
    };
    match std::env::current_exe() {
        Ok(exe) => exe.starts_with(&target_dir),
        Err(_) => false,
    }
}

/// Resolve the logs directory: `~/ProjectOpen/.Logs/<app_name>/`.
pub fn logs_dir(app_name: &str) -> Option<PathBuf> {
    user_home_dir().map(|home| home.join("ProjectOpen").join(".Logs").join(app_name))
}

// ---------------------------------------------------------------------------
// Bootstrap
// ---------------------------------------------------------------------------

/// Result of a bootstrap operation.
#[derive(Debug)]
pub enum BootstrapResult {
    /// Already running from the install location. Continue normally.
    AlreadyInstalled,
    /// Self-install was skipped (config.self_install = false). Continue normally.
    Skipped,
    /// Exe was copied and a relaunch was spawned. The caller should exit
    /// (this is done automatically unless `exit_after_relaunch` is false).
    Relaunched,
    /// Self-install failed at some step. The app should still try to run.
    Failed(String),
}

/// Run the full bootstrap sequence:
/// 1. Create directory structure.
/// 2. If not already installed, copy the exe and relaunch.
///
/// Pass your logging function so installer messages go to your own log.
pub fn bootstrap(config: &InstallerConfig, log: LogFn) -> BootstrapResult {
    log(LogLevel::Info, &format!(
        "[installer] Bootstrap starting for {} ({:?})",
        config.app_name, config.kind
    ));

    // --- Create directory structure ---
    let base_dir = match install_dir(config) {
        Some(d) => d,
        None => {
            let msg = "Cannot resolve install directory (home dir not found)";
            log(LogLevel::Error, &format!("[installer] {msg}"));
            return BootstrapResult::Failed(msg.to_string());
        }
    };

    // Always create the base directory.
    if let Err(e) = fs::create_dir_all(&base_dir) {
        log(LogLevel::Warn, &format!(
            "[installer] Failed to create {}: {e}", base_dir.display()
        ));
    }

    // For addons, also ensure the bin/ directory exists.
    if matches!(config.kind, ComponentKind::Addon { .. }) {
        let bin = base_dir.join("bin");
        if let Err(e) = fs::create_dir_all(&bin) {
            log(LogLevel::Warn, &format!(
                "[installer] Failed to create {}: {e}", bin.display()
            ));
        }
    }

    // Create configured subdirectories.
    for sub in &config.subdirs {
        let dir = base_dir.join(sub);
        if let Err(e) = fs::create_dir_all(&dir) {
            log(LogLevel::Warn, &format!(
                "[installer] Failed to create {}: {e}", dir.display()
            ));
        }
    }

    log(LogLevel::Info, &format!(
        "[installer] Directory structure ready at {}", base_dir.display()
    ));

    // --- Self-install ---
    if !config.self_install {
        log(LogLevel::Info, "[installer] Self-install disabled, skipping.");
        return BootstrapResult::Skipped;
    }

    if is_installed(config) {
        log(LogLevel::Info, "[installer] Already running from install directory.");
        return BootstrapResult::AlreadyInstalled;
    }

    let current_exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            let msg = format!("Cannot determine current exe path: {e}");
            log(LogLevel::Error, &format!("[installer] {msg}"));
            return BootstrapResult::Failed(msg);
        }
    };

    let target_dir = match exe_dir(config) {
        Some(d) => d,
        None => return BootstrapResult::Failed("Cannot resolve exe directory".into()),
    };
    let _ = fs::create_dir_all(&target_dir);

    let exe_filename = config.exe_name.as_deref().unwrap_or_else(|| {
        current_exe.file_name().and_then(|n| n.to_str()).unwrap_or("app.exe")
    });
    let dst = target_dir.join(exe_filename);

    log(LogLevel::Info, &format!(
        "[installer] Source: {}", current_exe.display()
    ));
    log(LogLevel::Info, &format!(
        "[installer] Target: {}", dst.display()
    ));

    let should_copy = needs_copy(&current_exe, &dst);

    if should_copy {
        log(LogLevel::Info, "[installer] Copying exe to install directory...");
        match fs::copy(&current_exe, &dst) {
            Ok(bytes) => log(LogLevel::Info, &format!(
                "[installer] Installed {exe_filename} ({bytes} bytes) -> {}",
                dst.display()
            )),
            Err(e) => {
                let msg = format!("Failed to copy exe: {e}");
                log(LogLevel::Error, &format!("[installer] {msg}"));
                return BootstrapResult::Failed(msg);
            }
        }
    } else {
        log(LogLevel::Info, "[installer] Installed exe is already up to date.");
    }

    // Relaunch from the installed location.
    let args: Vec<String> = std::env::args().skip(1).collect();
    log(LogLevel::Info, &format!(
        "[installer] Relaunching from {} with args: {:?}", dst.display(), args
    ));

    match std::process::Command::new(&dst).args(&args).spawn() {
        Ok(_) => {
            log(LogLevel::Info, "[installer] Relaunch successful.");
            if config.exit_after_relaunch {
                std::process::exit(0);
            }
            BootstrapResult::Relaunched
        }
        Err(e) => {
            let msg = format!("Failed to relaunch: {e}");
            log(LogLevel::Warn, &format!("[installer] {msg}"));
            BootstrapResult::Failed(msg)
        }
    }
}

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------

/// Check if the source exe needs to be copied to the destination.
/// Returns true if the destination doesn't exist, or differs in size or
/// modification time.
fn needs_copy(src: &Path, dst: &Path) -> bool {
    let src_meta = match fs::metadata(src) {
        Ok(m) => m,
        Err(_) => return false,
    };
    let dst_meta = match fs::metadata(dst) {
        Ok(m) => m,
        Err(_) => return true, // destination doesn't exist
    };

    if src_meta.len() != dst_meta.len() {
        return true;
    }

    // If sizes match, check modification time.
    src_meta.modified().ok().zip(dst_meta.modified().ok())
        .map(|(s, d)| s > d)
        .unwrap_or(false)
}
