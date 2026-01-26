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

## System Integration

Sentinel integrates directly with the operating system to provide reliable system-level functionality, including:

* Monitor and display enumeration
* Window management and positioning
* Wallpaper and desktop interactions
* Process and system information

All platform-specific and privileged logic lives in the backend, allowing addons to remain portable and focused.

---

## System Tray & Runtime Control

Sentinel exposes a system tray interface that acts as the primary control surface for:

* Starting and stopping the backend
* Managing addons at runtime
* Reloading configuration without restarting the system
* Accessing logs and debugging tools

This keeps Sentinel practical as a daily-use system while remaining developer-oriented.

---

## Configuration & Data Flow

Sentinel favors **explicit configuration and observable state** over implicit or hidden behavior:

* Configuration files define desired behavior
* Runtime state is reflected directly into the registry
* Addons react to state changes rather than polling blindly

This approach makes the system easier to reason about, debug, and extend over time.

---

## Intended Audience

Sentinel is designed for:

* Power users who want deep desktop customization
* Developers building custom desktop UI components
* Experimentation with alternative desktop workflows
* Long-running desktop setups that require stability and introspection

It is **not** intended to be a one-click theming tool—it is a platform.

---

## Project Status

Sentinel is under active development. APIs, internal structures, and behavior may change as the architecture evolves. Flexibility and clarity are currently prioritized over long-term stability or backward compatibility.

---

## Contact

* **Discord:** the_ico2
* **X (Twitter):** The_Ico2
