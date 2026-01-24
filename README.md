# Sentinel Desktop Customization Platform

Sentinel is a modular, extensible desktop customization platform designed primarily for Windows. Its goal is to act as a **central runtime, registry, and orchestration layer** for desktop enhancements such as status bars, widgets, system integrations, and background services — without locking users or developers into a single UI or workflow.

Rather than being a single monolithic application, Sentinel is best understood as a **desktop operating layer**: a backend service that manages addons, exposes system data, coordinates IPC, and provides a stable foundation on top of which highly customizable desktop components can be built.

---

## High‑Level Philosophy

Sentinel is built around a few core ideas:

* **Everything is an addon** – status bars, widgets, background services, and integrations are all treated as first‑class addons.
* **Centralized state & discovery** – a live registry keeps track of installed, running, and discovered components.
* **Separation of concerns** – the backend handles system access, lifecycle management, and IPC; addons focus on presentation and behavior.
* **Developer‑friendly** – predictable data models, explicit registries, and minimal magic.

The project intentionally avoids becoming "just another bar" or "just another widget engine." Instead, it aims to be the **platform those tools are built on**.

---

## What Sentinel Does

At its core, Sentinel runs as a background backend process that:

* Manages addon lifecycle (discover, load, start, stop, reload)
* Maintains a **live registry** of addons, widgets, assets, and system data
* Exposes system‑level information (monitors, windows, processes, wallpapers, etc.)
* Provides IPC channels for addons to communicate with the backend
* Coordinates desktop‑level behavior such as window anchoring, overlays, and persistence
* Integrates with the system tray as a control surface for runtime management

The backend is designed to be long‑lived and authoritative: addons can come and go, but Sentinel remains the source of truth.

---

## Registry‑Driven Architecture

A central concept in Sentinel is the **registry**.

The registry is both:

* An in‑memory data structure used by the backend
* A continuously updated `registry.json` file on disk for inspection and debugging

The registry tracks:

* Installed and discovered addons
* Loaded widgets and UI components
* Assets and resources
* Runtime system data (monitors, displays, state)
* Addon metadata (IDs, versions, status)

This design provides transparency and debuggability: you can see exactly what Sentinel believes exists and what state it is in at any moment.

---

## Addons

Addons are the primary extension mechanism in Sentinel.

An addon can be:

* A status bar implementation
* A widget or widget provider
* A background system integration
* A utility or automation service

Each addon:

* Has a manifest describing identity and capabilities
* Is discovered and registered by the backend
* Can be started, stopped, or reloaded independently
* Communicates with Sentinel through defined IPC mechanisms

This makes addons loosely coupled but centrally coordinated.

---

## Widgets & UI Components

Widgets are logical UI units that addons may expose. Sentinel itself does not enforce how widgets are rendered; instead, it:

* Tracks widget metadata
* Handles lifecycle and placement information
* Supplies system data to widgets via IPC

This allows different rendering technologies or UI toolkits to coexist under the same backend.

---

## System Integration

Sentinel integrates directly with the operating system to provide reliable system‑level capabilities, including:

* Monitor and display enumeration
* Window management and positioning
* Wallpaper and desktop interactions
* Process and system information

These integrations are handled in the backend so that addons do not need elevated permissions or platform‑specific code.

---

## System Tray & Runtime Control

Sentinel exposes a system tray interface that acts as the primary control surface for:

* Starting and stopping the backend
* Managing addons at runtime
* Reloading configuration without restarting the system
* Quick access to debugging and logs

This keeps Sentinel usable as a daily driver while remaining developer‑oriented.

---

## Configuration & Data Flow

Sentinel favors **explicit configuration and observable state** over hidden behavior.

* Configuration files define desired behavior
* Runtime state is reflected into the registry
* Addons react to state changes instead of polling blindly

This makes the system easier to reason about and extend over time.

---

## Intended Audience

Sentinel is designed for:

* Power users who want deep desktop customization
* Developers building custom desktop UI components
* Experimentation with alternative desktop workflows
* Long‑running desktop setups that need stability and introspection

It is not designed to be a minimal, one‑click theming tool — it is a platform.

---

## Project Status

Sentinel is under active development. APIs, internal structures, and behavior may evolve as the architecture is refined. Stability is improving, but flexibility and clarity are currently prioritized over backward compatibility.

---

## Summary

Sentinel is a **desktop customization backend and platform**, not a single UI. It provides:

* A live registry‑driven architecture
* Modular addons and widgets
* Centralized system integration
* Runtime control and observability

If you think of your desktop as something programmable rather than static, Sentinel is meant to be the foundation that makes that practical.