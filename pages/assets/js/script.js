// VEIL page bootstrap. Handles home stats + System Data rendering.

(function () {
  "use strict";

  function $(sel, root) { return (root || document).querySelector(sel); }
  function $$(sel, root) { return Array.prototype.slice.call((root || document).querySelectorAll(sel)); }

  function setText(sel, value) {
    var el = $(sel);
    if (el) el.textContent = String(value);
  }

  function safeJsonString(v) {
    try { return JSON.stringify(v); } catch (e) { return "[unserializable]"; }
  }

  function formatAgo(ms) {
    if (!ms) return "never";
    var delta = Math.max(0, Date.now() - ms);
    if (delta < 1000) return "just now";
    if (delta < 60000) return Math.floor(delta / 1000) + "s ago";
    if (delta < 3600000) return Math.floor(delta / 60000) + "m ago";
    return Math.floor(delta / 3600000) + "h ago";
  }

  function formatNumber(n) {
    return (typeof n === "number" && isFinite(n)) ? n.toLocaleString() : String(n);
  }

  function summarizeChannel(channel, value) {
    if (value == null) return "n/a";

    if (channel === "cpu") {
      var cpuUsage = value.usage_percent || value.total_usage || value.usage;
      return cpuUsage != null ? (Number(cpuUsage).toFixed(1) + "% usage") : safeJsonString(value).slice(0, 80);
    }

    if (channel === "ram") {
      if (value.usage_percent != null) {
        return Number(value.usage_percent).toFixed(1) + "% used";
      }
    }

    if (channel === "displays" && Array.isArray(value)) {
      return value.length + " monitor" + (value.length === 1 ? "" : "s");
    }

    if (channel === "network") {
      if (value.throughput_human) return "throughput " + value.throughput_human;
      if (value.transmitted_bytes_per_second != null || value.received_bytes_per_second != null) {
        return "up " + formatNumber(value.transmitted_bytes_per_second || 0) + " B/s, down " + formatNumber(value.received_bytes_per_second || 0) + " B/s";
      }
    }

    if (channel === "time") {
      if (value.local_iso) return value.local_iso;
      if (value.iso) return value.iso;
    }

    if (typeof value === "string") return value;
    return safeJsonString(value).slice(0, 140);
  }

  function refreshHomeStats() {
    console.log("[VEIL] refreshHomeStats() called, window.Veil:", !!window.Veil, "window.__veil_ipc:", !!window.__veil_ipc);
    if (!window.Veil) return;
    var p = window.Veil.ipc("registry", "full", {});
    console.log("[VEIL] refreshHomeStats ipc p:", p ? typeof p : "null");
    if (!p || typeof p.then !== "function") {
      console.log("[VEIL] refreshHomeStats: no thenable returned");
      return;
    }

    p.then(function (full) {
      console.log("[VEIL] refreshHomeStats got full:", full ? "ok" : "null", "addons isArray:", Array.isArray(full && full.addons));
      if (!full) return;
      var addons = Array.isArray(full.addons) ? full.addons.length : Object.keys(full.addons || {}).length;
      var assets = 0;
      if (full.assets && typeof full.assets === "object") {
        var keys = Object.keys(full.assets);
        for (var i = 0; i < keys.length; i++) {
          var arr = full.assets[keys[i]];
          if (Array.isArray(arr)) assets += arr.length;
        }
      }
      var monitors = 0;
      if (full.sysdata && Array.isArray(full.sysdata.displays)) {
        monitors = full.sysdata.displays.length;
      }

      setText("[data-stat='addons']", addons);
      setText("[data-stat='assets']", assets);
      setText("[data-stat='monitors']", monitors);
    });
  }

  function bindAddonRefresh() {
    var btn = $("[data-action='addons.refresh']");
    if (!btn) return;
    btn.addEventListener("click", function () {
      if (window.Veil) window.Veil.ipc("addons", "refresh", {});
      if (window.Veil) window.Veil.toast("Addons refreshed", "violet");
    });
  }

  function bindOpenFolder() {
    var btns = $$("[data-action='open-folder']");
    btns.forEach(function (b) {
      b.addEventListener("click", function () {
        var which = b.getAttribute("data-folder") || "Addons";
        if (window.Veil) window.Veil.ipc("system", "open-folder", { folder: which });
      });
    });
  }

  function initDataPage() {
    console.log("[VEIL] initDataPage() called");
    var tableBody = $("[data-veil-channels]");
    console.log("[VEIL] tableBody:", tableBody ? tableBody._nid : "null", "window.Veil:", !!window.Veil, "window.__veil_ipc:", !!window.__veil_ipc);
    if (!tableBody || !window.Veil) return;

    var filterInput = $("#veil-data-filter");
    var reloadBtn = $("[data-action='data.reload']");
    var heartbeatSwitch = $("#veil-ui-heartbeat-toggle");
    var heartbeatState = $("#veil-ui-heartbeat-state");
    var heartbeatSeen = $("#veil-ui-heartbeat-last-seen");

    var state = {
      heartbeatEnabled: true,
      heartbeatTimer: null,
      heartbeatLastSent: 0,
      pullTimer: null,
      fastRateMs: 50,
      slowRateMs: 1000,
      channels: {}
    };

    var FAST_CHANNELS = {
      time: true,
      keyboard: true,
      mouse: true,
      audio: true,
      media: true,
      idle: true
    };

    var DEMAND_SECTIONS = [
      "time", "cpu", "gpu", "ram", "storage", "displays", "network", "wifi",
      "bluetooth", "audio", "media", "keyboard", "mouse", "power", "idle", "system", "processes"
    ];

    function renderHeartbeatUi() {
      if (heartbeatSwitch) {
        heartbeatSwitch.classList.toggle("is-on", !!state.heartbeatEnabled);
      }
      if (heartbeatState) {
        heartbeatState.textContent = state.heartbeatEnabled ? "ON" : "OFF";
      }
      if (heartbeatSeen) {
        heartbeatSeen.textContent = state.heartbeatLastSent ? formatAgo(state.heartbeatLastSent) : "never";
      }
    }

    function beatOnce() {
      if (!state.heartbeatEnabled || !window.Veil) return;
      var p = window.Veil.ipc("backend", "ui_heartbeat", {});
      state.heartbeatLastSent = Date.now();
      renderHeartbeatUi();
      if (p && typeof p.catch === "function") {
        p.catch(function () {});
      }
    }

    function syncHeartbeatTimer() {
      if (state.heartbeatTimer) {
        clearInterval(state.heartbeatTimer);
        state.heartbeatTimer = null;
      }
      if (state.heartbeatEnabled) {
        beatOnce();
        state.heartbeatTimer = setInterval(beatOnce, 1000);
      }
      renderHeartbeatUi();
    }

    function pushDemands() {
      var p = window.Veil.ipc("tracking", "set_demands", { sections: DEMAND_SECTIONS });
      if (p && typeof p.catch === "function") p.catch(function () {});
    }

    function rateLabel(channel) {
      return FAST_CHANNELS[channel] ? (state.fastRateMs + " ms") : (state.slowRateMs + " ms");
    }

    function renderRows(sysdata) {
      var filterText = filterInput ? String(filterInput.value || "").toLowerCase().trim() : "";
      var rows = [];

      for (var i = 0; i < DEMAND_SECTIONS.length; i++) {
        var key = DEMAND_SECTIONS[i];
        var value = sysdata ? sysdata[key] : null;
        if (typeof value === "undefined") continue;

        var fp = safeJsonString(value);
        if (!state.channels[key]) {
          state.channels[key] = {
            fingerprint: fp,
            changedAt: Date.now(),
            seenAt: Date.now()
          };
        } else {
          if (state.channels[key].fingerprint !== fp) {
            state.channels[key].fingerprint = fp;
            state.channels[key].changedAt = Date.now();
          }
          state.channels[key].seenAt = Date.now();
        }

        var latest = summarizeChannel(key, value);
        if (filterText && key.toLowerCase().indexOf(filterText) === -1 && latest.toLowerCase().indexOf(filterText) === -1) {
          continue;
        }

        rows.push(
          "<tr>" +
            "<td>" + key + "</td>" +
            "<td title='" + latest.replace(/'/g, "&#39;") + "'>" + latest + "</td>" +
            "<td>" + rateLabel(key) + "</td>" +
            "<td>" + formatAgo(state.channels[key].changedAt) + "</td>" +
          "</tr>"
        );
      }

      if (!rows.length) {
        tableBody.innerHTML = "<tr><td colspan='4' style='color: var(--veil-text-tertiary); padding: var(--veil-space-5); text-align: center;'>No channels matched the current filter.</td></tr>";
      } else {
        tableBody.innerHTML = rows.join("");
      }
    }

    function pullDataSnapshot() {
      console.log("[VEIL] pullDataSnapshot() called");
      var p = window.Veil.ipc("registry", "snapshot", { sections: DEMAND_SECTIONS });
      console.log("[VEIL] ipc() returned:", p ? typeof p : "null/undefined");
      if (!p || typeof p.then !== "function") {
        console.log("[VEIL] pullDataSnapshot: no thenable, p=", p);
        return;
      }

      p.then(function (snap) {
        console.log("[VEIL] snap received:", snap ? "ok" : "null");
        var sysdata = snap && snap.sysdata ? snap.sysdata : {};
        var keys = Object.keys(sysdata);
        console.log("[VEIL] sysdata keys:", keys.length, "first key:", keys[0] || "none");
        renderRows(sysdata);
      });
    }

    function pullBackendConfig() {
      var p = window.Veil.ipc("backend", "get_config", {});
      if (!p || typeof p.then !== "function") return;

      p.then(function (cfg) {
        if (!cfg) return;
        if (typeof cfg.fast_pull_rate_ms === "number") state.fastRateMs = cfg.fast_pull_rate_ms;
        if (typeof cfg.slow_pull_rate_ms === "number") state.slowRateMs = cfg.slow_pull_rate_ms;
        if (typeof cfg.ui_data_exception_enabled === "boolean") state.heartbeatEnabled = cfg.ui_data_exception_enabled;
        syncHeartbeatTimer();
      });
    }

    function setHeartbeatEnabled(enabled) {
      state.heartbeatEnabled = !!enabled;
      syncHeartbeatTimer();
      var p = window.Veil.ipc("backend", "set_ui_data_exception_enabled", { enabled: state.heartbeatEnabled });
      if (p && typeof p.catch === "function") p.catch(function () {});
    }

    if (heartbeatSwitch) {
      heartbeatSwitch.addEventListener("click", function () {
        setHeartbeatEnabled(!state.heartbeatEnabled);
      });
    }

    if (filterInput) {
      filterInput.addEventListener("input", function () {
        pullDataSnapshot();
      });
    }

    if (reloadBtn) {
      reloadBtn.addEventListener("click", function () {
        pushDemands();
        beatOnce();
        pullDataSnapshot();
      });
    }

    pushDemands();
    pullBackendConfig();
    pullDataSnapshot();

    // Clear any previously running timers (in case initDataPage is called
    // again after a content swap brings the data page back into view).
    if (window.__veil_data_pull_timer) {
      clearInterval(window.__veil_data_pull_timer);
    }
    state.pullTimer = setInterval(function () {
      pullDataSnapshot();
      renderHeartbeatUi();
    }, 1200);
    window.__veil_data_pull_timer = state.pullTimer;
  }

  function fetchAndRenderAddons() {
    if (!window.Veil) return;
    var p = window.Veil.ipc("registry", "list_addons", {});
    if (!p || typeof p.then !== "function") return;
    p.then(function (addons) {
      console.log("[VEIL] fetchAndRenderAddons: got", Array.isArray(addons) ? addons.length : "non-array", "addons");
      // Transform registry entries into the format VeilFramework expects:
      // { name, pages: [{label, route}] }
      var mapped = [];
      if (Array.isArray(addons)) {
        for (var i = 0; i < addons.length; i++) {
          var a = addons[i];
          var meta = a.metadata || {};
          mapped.push({
            name: meta.name || a.id || "Addon",
            id: a.id,
            pages: meta.pages || []
          });
        }
      }
      window.__veilAddons = mapped;
      if (window.Veil) window.Veil.renderAddonList();
    });
  }

  function init() {
    bindAddonRefresh();
    bindOpenFolder();
    refreshHomeStats();
    fetchAndRenderAddons();
    setInterval(refreshHomeStats, 5000);
    setInterval(fetchAndRenderAddons, 15000);
    initDataPage();

    // Called by PRISM after a page-content swap so page-specific JS can
    // initialize against the freshly inserted DOM.
    window.__veil_on_content_swap = function (contentId) {
      if (contentId === "data") {
        initDataPage();
      }
    };
  }

  if (typeof addEventListener === "function") {
    addEventListener("DOMContentLoaded", init);
  }
})();
