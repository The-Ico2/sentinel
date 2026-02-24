# Sentinel Desktop Customization Platform

> **Note:** This is my first Rust project, and I'm actively learning as I build. Expect rough edges and architectural evolution. If you spot bugs, design issues, or potential improvements, feel free to open a PR or reach out to me on **Discord** or **X (formerly Twitter)**.

Sentinel is a modular, extensible desktop customization platform designed primarily for Windows. Its purpose is to serve as a **central runtime, registry, and orchestration layer** for desktop enhancements—such as status bars, widgets, system integrations, and background services—without locking users or developers into a single UI framework or workflow.

Rather than being a single monolithic application, Sentinel is best understood as a **desktop operating layer**: a persistent backend service responsible for managing addons, exposing system data, coordinating IPC, and providing a stable foundation on which highly customizable desktop components can be built.

---

## High-Level Philosophy

Sentinel is built around a few core principles:

* **Everything is an addon**
  Status bars, widgets, background services, and integrations are all treated as first-class addons.

* **Centralized state & discovery**
  A live registry tracks what exists, what's running, and what state everything is in.

* **Separation of concerns**
  The backend handles system access, lifecycle management, and IPC. Addons focus on behavior and presentation.

* **Developer-friendly by design**
  Explicit data models, observable state, and minimal hidden behavior.

The project intentionally avoids becoming "just another bar" or "just another widget engine." Instead, Sentinel aims to be the **platform those tools are built on**.

---

## What Sentinel Does

At its core, Sentinel runs as a long-lived background process (`sentinelc.exe`) that:

* **Self-installs** — Run the EXE from anywhere and it copies itself to `~/.Sentinel/`, creates the directory structure (`Addons/`, `Assets/`, `logs/`), and relaunches from the installed location.
* **Single-instance enforcement** — A global mutex prevents duplicate backend processes.
* Manages addon lifecycles (discovery, autostart, starting, stopping, reloading)
* Maintains a **live registry** of addons, assets, and system data
* Continuously polls and exposes system-level information (monitors, windows, processes, audio, etc.)
* Provides a named-pipe IPC server for communication between addons and the backend
* Integrates with the system tray as a runtime control surface
* Provides per-addon configuration UI via WebView2

Sentinel is designed to be authoritative: addons may come and go, but Sentinel remains the source of truth for system state and runtime coordination.

---

## Bootstrap & Self-Install

When `sentinelc.exe` is launched, it:

1. Creates `~/.Sentinel/` if it doesn't exist, along with `Addons/` and `Assets/` subdirectories
2. If not already running from `~/.Sentinel/`, copies itself there and relaunches
3. Initializes logging to `~/.Sentinel/logs/sentinelc.log`
4. Starts the registry manager, IPC server, data updater, and system tray

Addons follow the same self-install pattern: each copies itself to `~/.Sentinel/Addons/<name>/bin/`, scaffolds default config files (`addon.json`, `config.yaml`, `schema.yaml`), and relaunches from the installed location.

---

## File Layout

```ps1
~/.Sentinel/
├── sentinelc.exe
├── config.yaml                 # Backend config (pull rate, pause state)
├── registry.json               # Live registry snapshot (auto-written)
├── tray_settings.json          # Addon autostart & startup preferences
├── logs/
│   └── sentinelc.log
├── Addons/
│   └── <addon-name>/
│       ├── addon.json          # Addon manifest (id, name, exe_path, etc.)
│       ├── config.yaml         # Addon-specific configuration
│       ├── schema.yaml         # UI schema for config editor
│       ├── bin/
│       │   └── <addon-exe>
│       └── options/
│           └── *.html          # Custom addon settings pages
└── Assets/
    └── <category>/
        └── <asset>/
            └── manifest.json
```

---

## Registry-Driven Architecture

A core concept in Sentinel is the **registry**.

The registry exists as both:

* An in-memory data structure used by the backend (`RwLock<Registry>`)
* A continuously updated `registry.json` file written to disk for inspection and debugging

The registry tracks four top-level categories:

* **addons** — Installed and discovered addons (from `Addons/` directory manifests)
* **assets** — Discovered assets grouped by category (from `Assets/` directory manifests)
* **sysdata** — Runtime system data (CPU, GPU, RAM, displays, network, etc.)
* **appdata** — Active window state per monitor

A file watcher monitors `Addons/` and `Assets/` for changes to `addon.json` and `manifest.json` files. When changes are detected, the registry automatically rebuilds and rewrites `registry.json`.

The data updater thread continuously polls system data at a configurable interval (default 100ms) and writes to the registry only when values change.

---

## Addons

Addons are the primary extension mechanism in Sentinel.

An addon can be:

* A status bar implementation
* A wallpaper engine
* A window manager
* A background system integration
* A utility or automation service

Each addon:

* Declares a manifest (`addon.json`) describing its identity, executable path, and capabilities
* Is discovered and registered by the backend on startup and when files change
* Can be started, stopped, or reloaded independently via IPC or the system tray
* Can be set to autostart when the backend launches
* Communicates with Sentinel through named-pipe IPC

---

## IPC

Sentinel exposes a named-pipe IPC server at `\\.\pipe\sentinel`. Addons send JSON requests and receive JSON responses.

### Request Format

```json
{
  "ns": "sysdata",
  "cmd": "get_cpu",
  "args": null
}
```

### Namespaces & Commands

#### `sysdata` — System Data

All commands return structured JSON from the live registry.

| Command | Data Provided |
| --------- | --------------- |
| `get_cpu` | Model, vendor, architecture, physical/logical cores, per-core usage & frequency, temperature, total usage, uptime, boot time, process count |
| `get_gpu` | Name, vendor, VRAM, temperature, driver version, utilization |
| `get_ram` | Total/used/free/available memory, swap usage, top 10 processes by memory |
| `get_storage` | Per-disk name, mount, total/used/available, file system, usage percent, overall usage, disk count |
| `get_displays` | Per-monitor resolution, position, scale, refresh rate, color depth, orientation, device name, primary flag |
| `get_network` | Per-interface name, MAC address, IP addresses, send/receive bytes, packet & error stats, interface count |
| `get_power` | AC status, battery percent/charging/health/chemistry, power plan, estimated runtime, battery saver state |
| `get_keyboard` | Layout ID, type/subtype, function key count, toggle states (Caps/Num/Scroll Lock, Insert) |
| `get_mouse` | Cursor position, button count/swap, wheel present, speed, screen dimensions, monitor count |
| `get_audio` | Default playback/capture endpoints, volume/mute, all endpoints with levels; active media session (title, artist, album, playback status, timeline, shuffle, repeat, source app) |
| `get_bluetooth` | Adapter presence/name/status, paired & connected devices with name, status, class, instance ID |
| `get_wifi` | Connected SSID/BSSID, signal strength, radio type, band, channel, auth/cipher, transmit/receive rate, interface list |
| `get_system` | OS name/version/kernel/arch, hostname/username/domain, locale, Windows theme (dark/light, accent color, transparency), BIOS & motherboard info |
| `get_time` | Local & UTC timestamps, timezone, UTC offset, day of year, ISO week, quarter, leap year, uptime, boot time, 12h time with AM/PM |
| `get_processes` | Top 15 by CPU, top 15 by memory, total process count, status breakdown, aggregate CPU & memory usage |
| `get_idle` | Idle time (ms/sec/min), idle state (active/idle/away/locked/screensaver), screen locked, screensaver active |
| `get_temp` | CPU & GPU temperatures |
| `get_tray_icons` | System tray notification area icons: process name, PID, exe path, tooltip, visibility, area |
| `get_notifications` | Recent Windows toast notifications: app name, title, body, timestamp (up to 25) |

#### `registry` — Registry Queries

| Command | Description |
| --------- | ------------- |
| `list_addons` | List all registered addons |
| `list_assets` | List all discovered assets |
| `list_sysdata` | Full system data snapshot |
| `list_appdata` | Active window data per monitor |

#### `addon` — Addon Lifecycle

| Command | Args | Description |
| --------- | ------ | ------------- |
| `start` | `{ "name": "..." }` | Start an addon by name |
| `stop` | `{ "name": "..." }` | Stop a running addon |
| `reload` | `{ "name": "..." }` | Stop and restart an addon |

#### `backend` — Backend Configuration

| Command | Args | Description |
| --------- | ------ | ------------- |
| `get_config` | — | Get current pull rate and pause state |
| `set_pull_rate` | `{ "rate_ms": 200 }` | Set data poll interval (0–5000ms) |
| `set_pull_paused` | `{ "paused": true }` | Pause/resume system data polling |

---

## Application Data

Sentinel tracks active application state per monitor, giving addons real-time awareness of what the user is doing on the desktop:

* **Active Windows** — Per-monitor active window: app name, exe path, icon, window title, PID, focused state, window state (normal/maximized/fullscreen), size & position
* **Tray Icons** — System tray notification area icons: process name, PID, exe path, tooltip, visibility, area (visible/overflow)
* **Notifications** — Recent Windows toast notifications: app name, title, body, timestamp (up to 25)

---

## Desktop Customization (Internal)

Sentinel includes internal modules for direct Windows desktop integration. These are currently used as backend infrastructure and are **not yet exposed as IPC commands**:

* **Taskbar** — Show/hide/toggle the Windows taskbar (`Shell_TrayWnd`)
* **Wallpaper** — Programmatic wallpaper management via `SystemParametersInfoW`
* **Theme** — Placeholder for Windows theme detection
* **Transparency** — Placeholder for window transparency effects

---

## System Tray & Runtime Control

Sentinel exposes a system tray interface that acts as the primary control surface for:

* Starting and stopping individual addons
* Toggling addon autostart (persisted in `tray_settings.json`)
* Opening per-addon configuration UI
* Toggling backend startup with Windows (via registry `HKCU\...\Run`)
* Rescanning for new addons
* Exiting the backend

The system tray dynamically discovers addons and rebuilds its menu when addons are added or removed.

---

## Config UI

Sentinel includes a built-in configuration UI system for addons. When launched with `--addon-config-ui`, it:

* Reads the addon's `schema.yaml` to generate a settings interface
* Supports controls: toggles, dropdowns, number ranges, text inputs, text lists, asset selectors
* Renders using egui (native) with WebView2 for custom addon option pages
* Writes changes back to the addon's `config.yaml`

---

## Backend Configuration

The backend's own config lives at `~/.Sentinel/config.yaml`:

```yaml
data_pull_rate_ms: 100    # System data poll interval (0–5000ms)
data_pull_paused: false   # Pause system data polling
```

Both values can be changed at runtime via the `backend` IPC namespace and are persisted to disk.

---

## Intended Audience

Sentinel is designed for:

* Power users who want deep desktop customization
* Developers building custom desktop UI components
* Experimentation with alternative desktop workflows
* Long-running desktop setups that require stability and introspection

It is **not** intended to be a one-click theming tool—it is a platform.

---

## Tech Stack

* **Language:** Rust
* **Platform:** Windows 10/11 (Win32 API, WinRT)
* **IPC:** Named pipes (`\\.\pipe\sentinel`) with JSON request/response
* **Key crates:** `windows` 0.62, `sysinfo`, `tao`, `tray-icon`, `wry`, `eframe`, `serde_json`, `serde_yaml`, `chrono`, `notify`, `clap`

---

## Project Status

Sentinel is under active development (`v0.1.0-alpha`). APIs, internal structures, and behavior may change as the architecture evolves. Linux and macOS modules are scaffolded but not functional.

---

## Contact

* **Discord:** the_ico2
* **X (Twitter):** The_Ico2
