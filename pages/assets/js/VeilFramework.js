// ═══════════════════════════════════════════════════════════════════════════
//  VeilFramework.js
//  Core UI behaviors: sidebar collapse, hierarchical nav, tooltips,
//  active-nav state, and a thin VEIL IPC bridge for the embedded PRISM
//  runtime. Written in plain ES5 (no transpilation) so PRISM's V8 binding
//  treats it as inert until invoked.
// ═══════════════════════════════════════════════════════════════════════════

(function () {
  "use strict";

  // ── VEIL global ─────────────────────────────────────────────────────────
  // The PRISM runtime exposes a bridge for sending IPC messages back to the
  // VEIL backend. The hooks may not exist (e.g. when previewing pages in a
  // browser); we degrade gracefully.
  var Veil = {
    /** Send an IPC request: { ns, cmd, args } -> Promise<reply> | undefined */
    ipc: function (ns, cmd, args) {
      try {
        if (typeof window !== "undefined" && window.__veil_ipc) {
          return window.__veil_ipc(ns, cmd, args || {});
        }
        if (typeof window !== "undefined" && window.prism && typeof window.prism.ipc === "function") {
          return window.prism.ipc(ns, cmd, args || {});
        }
      } catch (e) { /* swallow */ }
      return undefined;
    },
    /** Navigate to a route id (drives the active nav state and PRISM router). */
    navigate: function (routeId) {
      if (!routeId) return;
      try {
        if (typeof window !== "undefined" && window.__veil_navigate) {
          window.__veil_navigate(routeId);
        }
      } catch (e) { /* ignore */ }
      setActiveNavItem(routeId);
      var pageContent = document.querySelector("page-content");
      if (pageContent) {
        pageContent.setAttribute("data-active-content", routeId);
      }
    },
    /** Show a transient toast. */
    toast: function (msg, kind) {
      var c = document.querySelector(".veil-toast-container, .prism-toast-container, .toast-container");
      if (!c) return;
      var t = document.createElement("div");
      t.className = "veil-toast" + (kind ? " veil-toast--" + kind : "");
      t.textContent = msg;
      c.appendChild(t);
      setTimeout(function () {
        t.style.opacity = "0";
        setTimeout(function () { if (t.parentNode) t.parentNode.removeChild(t); }, 240);
      }, 3000);
    },
    /** Render the addon dropdown in the sidebar from window.__veilAddons. */
    renderAddonList: function () {
      var host = document.querySelector("[data-veil-addon-list]");
      if (!host) return;
      var addons = (typeof window !== "undefined" && window.__veilAddons) || [];
      // Clear current children.
      while (host.firstChild) host.removeChild(host.firstChild);

      if (!addons.length) return;

      for (var i = 0; i < addons.length; i++) {
        var a = addons[i];
        // Per-addon header (clicking the header navigates to the addon's
        // first page if it has one — this gives the dashboard entry point.)
        var firstRoute = a.pages && a.pages.length ? a.pages[0].route : null;
        var addonItem = document.createElement("button");
        addonItem.className = "veil-nav-item veil-nav-item--addon";
        if (firstRoute) addonItem.setAttribute("data-navigate", firstRoute);
        addonItem.setAttribute("data-tooltip", a.name);

        var label = document.createElement("span");
        label.className = "veil-nav-item__label";
        label.textContent = a.name;
        addonItem.appendChild(label);
        host.appendChild(addonItem);

        // Sub-pages for this addon.
        for (var j = 0; j < (a.pages || []).length; j++) {
          var p = a.pages[j];
          var pageItem = document.createElement("button");
          pageItem.className = "veil-nav-item veil-nav-item--addon-page";
          pageItem.setAttribute("data-navigate", p.route);
          pageItem.setAttribute("data-tooltip", p.label);

          var plabel = document.createElement("span");
          plabel.className = "veil-nav-item__label";
          plabel.textContent = p.label;
          pageItem.appendChild(plabel);

          (function (btn, route) {
            btn.addEventListener("click", function () { Veil.navigate(route); });
          })(pageItem, p.route);
          host.appendChild(pageItem);
        }

        (function (btn, route) {
          if (!route) return;
          btn.addEventListener("click", function () { Veil.navigate(route); });
        })(addonItem, firstRoute);
      }
    }
  };

  // ── Tooltip ─────────────────────────────────────────────────────────────
  var tooltipEl = null;
  var tooltipTimer = null;

  function ensureTooltipElement() {
    if (tooltipEl) return tooltipEl;
    tooltipEl = document.createElement("div");
    tooltipEl.className = "veil-tooltip of-tooltip";
    document.body.appendChild(tooltipEl);
    return tooltipEl;
  }

  function showTooltip(anchor) {
    var text = anchor.getAttribute("data-tooltip");
    if (!text) return;
    var tip = ensureTooltipElement();
    tip.textContent = text;
    tip.classList.remove("visible");
    if (anchor.getBoundingClientRect) {
      var r = anchor.getBoundingClientRect();
      tip.style.left = (r.right + 8) + "px";
      tip.style.top = (r.top + (r.height / 2) - 12) + "px";
    }
    tooltipTimer = setTimeout(function () { tip.classList.add("visible"); }, 200);
  }

  function hideTooltip() {
    if (tooltipTimer) { clearTimeout(tooltipTimer); tooltipTimer = null; }
    if (tooltipEl) tooltipEl.classList.remove("visible");
  }

  // ── Active nav state ────────────────────────────────────────────────────
  function setActiveNavItem(targetPage) {
    var all = document.querySelectorAll(
      ".veil-nav-item, .prism-nav-item, .nav-item, .quick-action-btn"
    );
    for (var i = 0; i < all.length; i++) {
      all[i].classList.remove("active");
      all[i].classList.remove("is-active");
    }
    if (!targetPage) return;
    var matches = document.querySelectorAll('[data-navigate="' + targetPage + '"]');
    for (var k = 0; k < matches.length; k++) {
      var el = matches[k];
      if (el.classList.contains("veil-nav-item") || el.classList.contains("prism-nav-item")) {
        el.classList.add("is-active");
      } else {
        el.classList.add("active");
      }
    }

    // Keep ancestor groups open when an active child is set.
    var ancestor = matches[0];
    while (ancestor && ancestor !== document.body) {
      if (ancestor.classList && ancestor.classList.contains("veil-nav-children")) {
        var prev = ancestor.previousElementSibling;
        if (prev && prev.classList && prev.classList.contains("veil-nav-item")) {
          prev.classList.add("is-open");
        }
      }
      ancestor = ancestor.parentNode;
    }
  }

  // ── Sidebar collapse ────────────────────────────────────────────────────
  function toggleSidebar() {
    var sb = document.querySelector(".veil-sidebar, .prism-sidebar");
    if (!sb) return;
    sb.classList.toggle("is-rail");

    // Collapse all expanded groups when going to rail mode.
    if (sb.classList.contains("is-rail")) {
      var open = sb.querySelectorAll(".veil-nav-item.is-open");
      for (var i = 0; i < open.length; i++) open[i].classList.remove("is-open");
    }

    try {
      if (typeof localStorage !== "undefined") {
        localStorage.setItem("veil:sidebar-rail", sb.classList.contains("is-rail") ? "1" : "0");
      }
    } catch (e) { /* ignore */ }
  }

  function restoreSidebarState() {
    try {
      if (typeof localStorage === "undefined") return;
      var v = localStorage.getItem("veil:sidebar-rail");
      if (v === "1") {
        var sb = document.querySelector(".veil-sidebar, .prism-sidebar");
        if (sb) sb.classList.add("is-rail");
      }
    } catch (e) { /* ignore */ }
  }

  // ── Nav-group toggle (hierarchical addon dropdowns) ─────────────────────
  function toggleNavGroup(item) {
    var sb = item.closest ? item.closest(".veil-sidebar, .prism-sidebar") : null;
    // When in rail mode, expanding a group is meaningless — auto-expand sidebar first.
    if (sb && sb.classList.contains("is-rail")) {
      sb.classList.remove("is-rail");
      try { if (typeof localStorage !== "undefined") localStorage.setItem("veil:sidebar-rail", "0"); } catch (e) {}
    }
    item.classList.toggle("is-open");
  }

  // ── Switch / toggle controls ────────────────────────────────────────────
  function bindSwitches() {
    var switches = document.querySelectorAll(".veil-switch");
    for (var i = 0; i < switches.length; i++) {
      (function (sw) {
        sw.addEventListener("click", function () {
          sw.classList.toggle("is-on");
          var event = new CustomEvent("veil:switch", {
            detail: { id: sw.getAttribute("data-id"), on: sw.classList.contains("is-on") }
          });
          sw.dispatchEvent(event);
        });
      })(switches[i]);
    }
  }

  // ── Window controls ─────────────────────────────────────────────────────
  function bindWindowControls() {
    var btns = document.querySelectorAll("[data-window-action]");
    for (var i = 0; i < btns.length; i++) {
      (function (btn) {
        btn.addEventListener("click", function () {
          var act = btn.getAttribute("data-window-action");
          Veil.ipc("window", act, {});
        });
      })(btns[i]);
    }
  }

  // ── Init ────────────────────────────────────────────────────────────────
  function init() {
    restoreSidebarState();

    // Tooltip wiring
    var tts = document.querySelectorAll("[data-tooltip]");
    for (var i = 0; i < tts.length; i++) {
      (function (el) {
        el.addEventListener("mouseenter", function () { showTooltip(el); });
        el.addEventListener("mouseleave", hideTooltip);
      })(tts[i]);
    }

    // Sidebar toggle
    var toggleBtns = document.querySelectorAll("[data-sidebar-toggle]");
    for (var t = 0; t < toggleBtns.length; t++) {
      toggleBtns[t].addEventListener("click", toggleSidebar);
    }

    // Hierarchical nav-group toggles (an item with [data-nav-group])
    var groupBtns = document.querySelectorAll("[data-nav-group]");
    for (var g = 0; g < groupBtns.length; g++) {
      (function (btn) {
        btn.addEventListener("click", function (ev) {
          ev.stopPropagation();
          toggleNavGroup(btn);
        });
      })(groupBtns[g]);
    }

    // Navigation buttons
    var navs = document.querySelectorAll("[data-navigate]");
    for (var n = 0; n < navs.length; n++) {
      (function (btn) {
        btn.addEventListener("click", function () {
          Veil.navigate(btn.getAttribute("data-navigate"));
        });
      })(navs[n]);
    }

    bindSwitches();
    bindWindowControls();

    // Populate the Addons sidebar dropdown if Core has already pushed the list.
    Veil.renderAddonList();

    // Set initial active state
    var pc = document.querySelector("page-content");
    var def = pc ? (pc.getAttribute("data-active-content") || pc.getAttribute("default") || "home") : "home";
    setActiveNavItem(def);
  }

  if (typeof addEventListener === "function") {
    addEventListener("DOMContentLoaded", init);
  } else {
    init();
  }

  // Export
  if (typeof window !== "undefined") {
    window.Veil = Veil;
    window.VeilFramework = {
      setActive: setActiveNavItem,
      toggleSidebar: toggleSidebar,
      toggleNavGroup: toggleNavGroup
    };
  }
})();
