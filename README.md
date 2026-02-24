# Sentinel Desktop Customization Platform

> **Note:** This is my first Rust project, and I’m actively learning as I build. Expect rough edges and architectural evolution. If you spot bugs, design issues, or potential improvements, feel free to open a PR or reach out to me on **Discord** or **X (formerly Twitter)**.

Sentinel is a modular, extensible desktop customization platform designed primarily for Windows. Its purpose is to serve as a **central runtime, registry, and orchestration layer** for desktop enhancements—such as status bars, widgets, system integrations, and background services—without locking users or developers into a single UI framework or workflow.

Rather than being a single monolithic application, Sentinel is best understood as a **desktop operating layer**: a persistent backend service responsible for managing addons, exposing system data, coordinating IPC, and providing a stable foundation on which highly customizable desktop components can be built.

---

## High-Level Philosophy

Sentinel is built around a few core principles:

* **Everything is an addon**
  Status bars, widgets, background services, and integrations are all treated as first-class addons.

* **Centralized state & discovery**
  A live registry tracks what exists, what’s running, and what state everything is in.

* **Separation of concerns**
  The backend handles system access, lifecycle management, and IPC. Addons focus on behavior and presentation.

* **Developer-friendly by design**
  Explicit data models, observable state, and minimal hidden behavior.

The project intentionally avoids becoming “just another bar” or “just another widget engine.” Instead, Sentinel aims to be the **platform those tools are built on**.

---

## What Sentinel Does

At its core, Sentinel runs as a long-lived background process that:

* Manages addon lifecycles (discovery, loading, starting, stopping, reloading)
* Maintains a **live registry** of addons, widgets, assets, and system data
* Exposes system-level information (monitors, windows, processes, wallpapers, etc.)
* Provides IPC channels for communication between addons and the backend
* Coordinates desktop-level behavior such as window anchoring, overlays, and persistence
* Integrates with the system tray as a runtime control surface

Sentinel is designed to be authoritative: addons may come and go, but Sentinel remains the source of truth for system state and runtime coordination.

---

## Registry-Driven Architecture

A core concept in Sentinel is the **registry**.

The registry exists as both:

* An in-memory data structure used by the backend
* A continuously updated `registry.json` file written to disk for inspection and debugging

The registry tracks:

* Installed and discovered addons
* Loaded widgets and UI components
* Assets and resources
* Runtime system data (monitors, displays, state)
* Addon metadata (IDs, versions, status)

This design prioritizes transparency and debuggability—you can always inspect exactly what Sentinel believes exists and what state it’s in at any moment.

---

## Addons

Addons are the primary extension mechanism in Sentinel.

An addon can be:

* A status bar implementation
* A widget or widget provider
* A background system integration
* A utility or automation service

Each addon:

* Declares a manifest describing its identity and capabilities
* Is discovered and registered by the backend
* Can be started, stopped, or reloaded independently
* Communicates with Sentinel through defined IPC mechanisms

This keeps addons loosely coupled while still being centrally coordinated.

---

## Widgets & UI Components

Widgets are logical UI units exposed by addons. Sentinel does **not** enforce how widgets are rendered. Instead, it:

* Tracks widget metadata
* Manages lifecycle and placement information
* Supplies system and runtime data via IPC

This allows multiple rendering technologies or UI toolkits to coexist under the same backend without friction.

---

## System Data

Sentinel continuously polls and exposes a wide range of system-level data through the registry and IPC, giving addons real-time access to hardware, OS, and environment state. All data is available as structured JSON via IPC dispatch commands.

### Hardware & Performance

| Category | IPC Command | Data Provided |
| ---------- | ------------- | --------------- |
| **CPU** | `get_cpu` | Model, vendor, architecture, physical/logical cores, per-core usage & frequency, temperature, total usage, uptime, boot time, process count |
| **GPU** | `get_gpu` | Name, vendor, VRAM, temperature, driver version, utilization |
| **RAM** | `get_ram` | Total/used/free/available memory, swap usage, top 10 processes by memory |
| **Storage** | `get_storage` | Per-disk name, mount, total/used/available, file system, usage percent, overall usage, disk count |
| **Display** | `get_displays` | Per-monitor resolution, position, scale, refresh rate, color depth, orientation, device name, primary flag |
| **Network** | `get_network` | Per-interface name, MAC address, IP addresses, send/receive bytes, packet & error stats, interface count |
| **Power** | `get_power` | AC status, battery percent/charging/health/chemistry, power plan, estimated runtime, battery saver state |

### Peripherals & Input

| Category | IPC Command | Data Provided |
| ---------- | ------------- | --------------- |
| **Keyboard** | `get_keyboard` | Layout ID, type/subtype, function key count, toggle states (Caps/Num/Scroll Lock, Insert) |
| **Mouse** | `get_mouse` | Cursor position, button count/swap, wheel present, speed, screen dimensions, monitor count |

### Media & Audio

| Category | IPC Command | Data Provided |
| ---------- | ------------- | --------------- |
| **Audio** | `get_audio` | Default playback/capture endpoints, volume/mute, all endpoints with levels; active media session (title, artist, album, playback status, timeline position/duration, shuffle, repeat, source app) |

### Connectivity

| Category | IPC Command | Data Provided |
| ---------- | ------------- | --------------- |
| **Bluetooth** | `get_bluetooth` | Adapter presence/name/status, paired & connected devices with name, status, class, instance ID |
| **WiFi** | `get_wifi` | Connected SSID/BSSID, signal strength, radio type, band, channel, auth/cipher, transmit/receive rate, interface list |

### System & Environment

| Category | IPC Command | Data Provided |
| ---------- | ------------- | --------------- |
| **System** | `get_system` | OS name/version/kernel/arch, hostname/username/domain, locale (language, region, currency), Windows theme (dark/light, accent color, transparency), BIOS & motherboard info |
| **Time** | `get_time` | Local & UTC timestamps, timezone, UTC offset, day of year, ISO week, quarter, leap year, uptime, boot time, 12h time with AM/PM |
| **Processes** | `get_processes` | Top 15 by CPU, top 15 by memory, total process count, status breakdown (running/sleeping/stopped/zombie), aggregate CPU & memory usage |
| **Idle** | `get_idle` | Idle time (ms/sec/min), idle state (active/idle/away/locked/screensaver), screen locked, screensaver active |
| **Temperature** | `get_temp` | CPU & GPU temperatures |

---

## Application Data

Sentinel also tracks active application state, giving addons real-time awareness of what the user is doing on the desktop.

| Category | IPC Command | Data Provided |
| ---------- | ------------- | --------------- |
| **Active Windows** | via registry | Per-monitor active window: app name, exe path, window title, PID, focused state, window state (normal/maximized/fullscreen), size & position |
| **Tray Icons** | `get_tray_icons` | System tray notification area icons: process name, PID, exe path, tooltip, visibility, area (visible/overflow) |
| **Notifications** | `get_notifications` | Recent Windows toast notifications: app name, title, body, timestamp (up to 25) |

The **tray icons** module enables addons to build custom system tray interfaces—enumerating which apps have tray icons, their tooltips, and visibility state, providing the foundation for fully custom notification area implementations.

---

## Desktop Customization

Sentinel provides direct integration with Windows desktop customization APIs:

* **Wallpaper** — programmatic wallpaper management
* **Taskbar** — taskbar visibility and behavior control
* **Transparency** — window transparency effects
* **Theme** — Windows theme detection (dark/light mode, accent color)

---

## System Tray & Runtime Control

Sentinel exposes a system tray interface that acts as the primary control surface for:

* Starting and stopping the backend
* Managing addons at runtime (start, stop, reload, autostart toggle)
* Reloading configuration without restarting the system
* Accessing logs and debugging tools
* Rescanning for new addons

This keeps Sentinel practical as a daily-use system while remaining developer-oriented.

---

## IPC & Data Flow

Sentinel favors **explicit configuration and observable state** over implicit or hidden behavior:

* Configuration files define desired behavior
* Runtime state is reflected directly into the registry
* Addons react to state changes rather than polling blindly
* System data is polled at 100ms intervals and written to the registry on change
* Addons communicate through named pipe IPC with structured JSON request/response

All IPC commands follow a `category.command` pattern (e.g., `sysdata.get_cpu`, `sysdata.get_bluetooth`, `registry.list_addons`).

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
* **Platform:** Windows (Win32 API, WinRT, WMI/CIM via PowerShell)
* **Key crates:** `windows` 0.62, `sysinfo`, `tao`, `tray-icon`, `wry`, `eframe`, `serde_json`, `chrono`

---

## Project Status

Sentinel is under active development. APIs, internal structures, and behavior may change as the architecture evolves. Flexibility and clarity are currently prioritized over long-term stability or backward compatibility.

---

## Contact

* **Discord:** the_ico2
* **X (Twitter):** The_Ico2
