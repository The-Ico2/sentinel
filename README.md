<p align="center">
  <h1 align="center">VEIL</h1>
  <p align="center">
    <strong>Modular desktop customization platform for Windows.</strong>
  </p>
  <p align="center">
    Runtime &nbsp;·&nbsp; Registry &nbsp;·&nbsp; IPC &nbsp;·&nbsp; Addon Orchestration
  </p>
  <p align="center">
    <a href="https://github.com/The-Ico2/VEIL"><img src="https://img.shields.io/badge/status-v0.2.2-orange" alt="Version"></a>
    <a href="https://github.com/The-Ico2/VEIL"><img src="https://img.shields.io/badge/platform-Windows%2010%2F11-brightgreen" alt="Platform"></a>
    <a href="https://github.com/The-Ico2/VEIL"><img src="https://img.shields.io/badge/language-Rust-red" alt="Language"></a>
  </p>
</p>

---

## Overview

VEIL is a persistent background service (`odc.exe`) that serves as the **central runtime, registry, and orchestration layer** for desktop enhancements on Windows. Status bars, wallpapers, widgets, window managers, and background services are all treated as first-class **addons** — discovered, started, stopped, and coordinated through a single backend.

Rather than being a monolithic application, VEIL is a **desktop operating layer**: a stable foundation that manages addon lifecycles, exposes real-time system data, coordinates IPC, and provides a live registry of everything running on the desktop.

---

## Design Principles

| Principle | Description |
|:----------|:------------|
| **Everything is an addon** | Status bars, widgets, wallpapers, window managers, and background services are all addons with identical lifecycle management. |
| **Centralized state & discovery** | A live registry tracks what exists, what's running, and what state everything is in — in memory and on disk. |
| **Separation of concerns** | The backend handles system access, lifecycle management, and IPC. Addons focus on behavior and presentation. |
| **Developer-friendly** | Explicit data models, observable state, named-pipe IPC with JSON payloads, and minimal hidden behavior. |

---

## What It Does

| Capability | Details |
|:-----------|:--------|
| **Self-install** | Run the EXE from anywhere — it copies itself to `~/.VEIL/`, scaffolds the directory structure, and relaunches from the installed location. |
| **Single-instance** | A global mutex prevents duplicate backend processes. |
| **Addon lifecycle** | Discovery, autostart, start, stop, reload — all managed via IPC or the system tray. |
| **Live registry** | In-memory + `registry.json` on disk — addons, assets, system data, and active window state. |
| **System data polling** | Dual-tier event-driven model with condvar-based waking. Fast tier (50ms): time, keyboard, mouse, audio, idle, power. Slow tier (1s): CPU, GPU, RAM, storage, network, processes. |
| **IPC server** | Named-pipe server at `\\.\pipe\veil` — JSON request/response protocol with 4 namespaces and 30+ commands. |
| **System tray** | Start/stop addons, toggle autostart, open config UIs, toggle Windows startup, rescan, exit. |
| **Config UI** | Built-in schema-driven settings interface per addon — toggles, dropdowns, sliders, text inputs, asset selectors. |

---

## File Layout

```
~/.VEIL/
├── odc.exe                     # Backend binary
├── config.yaml                 # Backend config (poll rates, pause state)
├── registry.json               # Live registry snapshot (auto-written)
├── tray_settings.json          # Addon autostart & startup preferences
├── logs/
│   └── odc.log
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

## Registry

The registry is VEIL's central data structure — the single source of truth for all runtime state.

**Dual form:**
- In-memory `RwLock<Registry>` — used by the backend and IPC handlers
- Continuously written `registry.json` — for debugging, inspection, and external tools

**Four sections:**

| Section | Contents |
|:--------|:---------|
| **addons** | Installed and discovered addons (from `Addons/` directory manifests) |
| **assets** | Discovered assets grouped by category (from `Assets/` directory manifests) |
| **sysdata** | Runtime system data — CPU, GPU, RAM, storage, displays, network, audio, and more |
| **appdata** | Active window state per monitor, tray icons, toast notifications |

A file watcher monitors `Addons/` and `Assets/` for manifest changes and triggers automatic registry rebuilds.

### Data Polling

The data updater uses a **dual-tier, event-driven** polling model with condvar-based waking:

| Tier | Default Interval | Sections |
|:-----|:-----------------|:---------|
| **Fast** | 50ms | time, keyboard, mouse, audio, idle, power |
| **Slow** | 1000ms | CPU, GPU, RAM, storage, network, processes, system |

Threads respond instantly to demand changes instead of sleeping on fixed timers. A UI heartbeat mechanism (2500ms TTL) forces active updates while the VEIL UI is open.

---

## Addons

Addons are the primary extension mechanism. An addon can be anything that enhances the desktop:

| Type | Examples |
|:-----|:---------|
| Status bar | Custom taskbar replacement |
| Wallpaper engine | Animated/interactive wallpapers via OpenRender |
| Window manager | Tiling, snapping, workspace management |
| Background service | System integrations, automation, monitoring |
| Widget | Floating desktop overlays |

### Addon Lifecycle

Each addon:

1. Declares a manifest (`addon.json`) describing its identity, executable path, and capabilities
2. Is discovered and registered by the backend on startup and when files change
3. Can be started, stopped, or reloaded independently via IPC or the system tray
4. Can be set to autostart when the backend launches
5. Communicates with VEIL through named-pipe IPC
6. Self-installs: copies itself to `~/.VEIL/Addons/<name>/bin/` and scaffolds default config files

---

## IPC Protocol

VEIL exposes a named-pipe IPC server at `\\.\pipe\veil`. All communication uses JSON request/response.

### Request Format

```json
{
  "ns": "sysdata",
  "cmd": "get_cpu",
  "args": null
}
```

### Namespaces

<details open>
<summary><strong><code>sysdata</code> — System Data</strong></summary>

All commands return structured JSON from the live registry.

| Command | Data Returned |
|:--------|:--------------|
| `get_cpu` | Model, vendor, architecture, physical/logical cores, per-core usage & frequency, temperature, total usage, uptime, boot time, process count |
| `get_gpu` | Name, vendor, VRAM, temperature, driver version, utilization |
| `get_ram` | Total/used/free/available memory, swap usage, top 10 processes by memory |
| `get_storage` | Per-disk name, mount, total/used/available, file system, usage percent, disk count |
| `get_displays` | Per-monitor resolution, position, scale, refresh rate, color depth, orientation, primary flag |
| `get_network` | Per-interface name, MAC, IPs, send/receive bytes, packet & error stats |
| `get_power` | AC status, battery percent/charging/health/chemistry, power plan, estimated runtime |
| `get_keyboard` | Layout ID, type/subtype, function key count, toggle states |
| `get_mouse` | Cursor position, button count/swap, wheel, speed, screen dimensions |
| `get_audio` | Default playback/capture endpoints, volume/mute, all endpoints with levels |
| `get_media` | Active session: title, artist, album, playback status, timeline, shuffle, repeat |
| `get_bluetooth` | Adapter info, paired & connected devices |
| `get_wifi` | Connected SSID/BSSID, signal strength, radio type, band, channel, auth/cipher |
| `get_system` | OS info, hostname, locale, Windows theme (dark/light, accent color), BIOS & motherboard |
| `get_time` | Local & UTC timestamps, timezone, day of year, ISO week, quarter, uptime, boot time |
| `get_processes` | Top 15 by CPU, top 15 by memory, total count, status breakdown |
| `get_idle` | Idle time, idle state, screen locked, screensaver active |
| `get_temp` | CPU & GPU temperatures |
| `get_tray_icons` | System tray icons: process name, PID, tooltip, visibility |
| `get_notifications` | Recent toast notifications: app, title, body, timestamp (up to 25) |

</details>

<details open>
<summary><strong><code>registry</code> — Registry Queries</strong></summary>

| Command | Description |
|:--------|:------------|
| `list_addons` | All registered addons |
| `list_assets` | All discovered assets |
| `list_sysdata` | Full system data snapshot |
| `list_appdata` | Active window data per monitor |
| `snapshot` | Combined `sysdata` + `appdata` (accepts optional `sections` arg for demand tracking) |
| `full` | Complete registry dump — addons, assets, sysdata, appdata, metadata |

</details>

<details open>
<summary><strong><code>addon</code> — Addon Lifecycle</strong></summary>

| Command | Args | Description |
|:--------|:-----|:------------|
| `start` | `{ "name": "..." }` | Start an addon by name |
| `stop` | `{ "name": "..." }` | Stop a running addon |
| `reload` | `{ "name": "..." }` | Stop and restart an addon |

</details>

<details open>
<summary><strong><code>backend</code> — Backend Configuration</strong></summary>

| Command | Args | Description |
|:--------|:-----|:------------|
| `get_config` | — | Current config snapshot |
| `set_fast_pull_rate` | `{ "rate_ms": 50 }` | Set fast-tier poll interval |
| `set_slow_pull_rate` | `{ "rate_ms": 1000 }` | Set slow-tier poll interval |
| `set_pull_paused` | `{ "paused": true }` | Pause/resume all data polling |
| `set_refresh_on_request` | `{ "enabled": true }` | Refresh fast-tier data inline on sysdata requests |
| `set_ui_data_exception_enabled` | `{ "enabled": true }` | Allow UI heartbeat to force active updates |
| `ui_heartbeat` | — | Signal that the UI is open (resets 2500ms TTL) |
| `set_tracking_demands` | `{ "sections": [...] }` | Set which data sections to actively poll |

</details>

---

## Application Data

VEIL tracks active application state per monitor:

| Data | Details |
|:-----|:--------|
| **Active windows** | Per-monitor: app name, exe path, icon, window title, PID, focused state, window state (normal/maximized/fullscreen), size & position |
| **Tray icons** | System tray notification area icons: process name, PID, exe path, tooltip, visibility, area (visible/overflow) |
| **Notifications** | Recent Windows toast notifications: app name, title, body, timestamp (up to 25) |

---

## Desktop Integration

Internal modules for direct Windows desktop control (backend infrastructure, not yet exposed via IPC):

| Module | Description |
|:-------|:------------|
| **Taskbar** | Show/hide/toggle the Windows taskbar (`Shell_TrayWnd`) |
| **Wallpaper** | Programmatic wallpaper management via `SystemParametersInfoW` |
| **Theme** | Windows theme detection (placeholder) |
| **Transparency** | Window transparency effects (placeholder) |

---

## Config UI

When launched with `--addon-config-ui`, VEIL generates a settings interface from the addon's `schema.yaml`:

| Control Type | Description |
|:-------------|:------------|
| Toggle | Boolean switch |
| Dropdown | Selection from options |
| Number range | Slider/input within bounds |
| Text input | Free-form text |
| Text list | Multi-value text entries |
| Asset selector | Choose from discovered assets |

Renders using egui (native) with WebView2 for custom addon option pages. Writes changes to the addon's `config.yaml`.

---

## Backend Configuration

```yaml
# ~/.VEIL/config.yaml
fast_pull_rate_ms: 50           # Fast-tier: time, keyboard, mouse, audio, idle, power
slow_pull_rate_ms: 1000         # Slow-tier: cpu, gpu, ram, storage, network, processes
data_pull_paused: false         # Pause all polling
refresh_on_request: false       # Refresh fast-tier inline on IPC requests
ui_data_exception_enabled: true # UI heartbeat forces active updates
```

All values are changeable at runtime via the `backend` IPC namespace and persist to disk.

---

## Tech Stack

| Category | Technology |
|:---------|:-----------|
| Language | Rust |
| Platform | Windows 10/11 (Win32 API, WinRT) |
| IPC | Named pipes (`\\.\pipe\veil`) — JSON request/response |
| Key crates | `windows` 0.62, `sysinfo`, `tao`, `tray-icon`, `wry`, `eframe`, `serde_json`, `serde_yaml`, `chrono`, `notify`, `clap`, `tokio`, `rustfft` |

---

## Building

```bash
cargo build --release
```

The binary self-installs on first run — no manual setup required.

---

## Project Status

Under active development (`v0.2.2`). APIs, internal structures, and behavior may change as the architecture evolves. Linux and macOS modules are scaffolded but not yet functional.

---

## License

See project license file.

---

## Contact

- **Discord:** the_ico2
- **X:** [@The_Ico2](https://x.com/The_Ico2)
