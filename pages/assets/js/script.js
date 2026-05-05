// VEIL — page-specific bootstrap. Pulls live data from the backend over the
// PRISM IPC bridge and refreshes the UI. Safe in browser previews (degrades).

(function () {
  "use strict";

  function $(sel, root) { return (root || document).querySelector(sel); }
  function $$(sel, root) { return Array.prototype.slice.call((root || document).querySelectorAll(sel)); }

  function setText(sel, value) {
    var el = $(sel);
    if (el != null) el.textContent = String(value);
  }

  // ── Data refresh ────────────────────────────────────────────────────────
  function refreshHomeStats(data) {
    if (!data) return;
    if (data.addons != null)   setText("[data-stat='addons']",   data.addons);
    if (data.assets != null)   setText("[data-stat='assets']",   data.assets);
    if (data.monitors != null) setText("[data-stat='monitors']", data.monitors);
    if (data.uptime != null)   setText("[data-stat='uptime']",   data.uptime);
  }

  function pullRegistry() {
    if (!window.Veil) return;
    var p = window.Veil.ipc("registry", "snapshot", {});
    if (p && typeof p.then === "function") {
      p.then(function (snap) {
        if (!snap) return;
        refreshHomeStats({
          addons: snap.addonCount || 0,
          assets: snap.assetCount || 0,
          monitors: snap.monitorCount || 0
        });
      });
    }
  }

  function bindAddonRefresh() {
    var btn = $("[data-action='addons.refresh']");
    if (!btn) return;
    btn.addEventListener("click", function () {
      if (window.Veil) window.Veil.ipc("addons", "refresh", {});
      window.Veil && window.Veil.toast("Addons refreshed", "violet");
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

  function init() {
    bindAddonRefresh();
    bindOpenFolder();
    pullRegistry();
    setInterval(pullRegistry, 5000);
  }

  if (typeof addEventListener === "function") {
    addEventListener("DOMContentLoaded", init);
  }
})();
