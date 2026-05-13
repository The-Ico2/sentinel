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
    /**
     * Render the addon list in the sidebar from window.__veilAddons.
     * Each addon is a full-width row with icon + name. Clicking an
     * addon toggles a list of its child pages (or, if it only has one
     * page, navigates directly).
     */
    renderAddonList: function () {
      var host = document.querySelector("[data-veil-addon-list]");
      if (!host) return;
      var addons = (typeof window !== "undefined" && window.__veilAddons) || [];

      // Clear current children.
      while (host.firstChild) host.removeChild(host.firstChild);

      if (!addons.length) {
        var empty = document.createElement("div");
        empty.className = "veil-addon-list__empty";
        empty.textContent = "No addons installed";
        host.appendChild(empty);
        return;
      }

      function makeIconSvg(letter) {
        // Fallback: a rounded square with the addon's initial.
        // Inline SVG so PRISM rasterises it at currentColor.
        var ns = "http://www.w3.org/2000/svg";
        var svg = document.createElementNS(ns, "svg");
        svg.setAttribute("viewBox", "0 0 24 24");
        svg.setAttribute("fill", "none");
        svg.setAttribute("stroke", "currentColor");
        svg.setAttribute("stroke-width", "1.6");
        var rect = document.createElementNS(ns, "rect");
        rect.setAttribute("x", "3"); rect.setAttribute("y", "3");
        rect.setAttribute("width", "18"); rect.setAttribute("height", "18");
        rect.setAttribute("rx", "4");
        svg.appendChild(rect);
        var text = document.createElementNS(ns, "text");
        text.setAttribute("x", "12"); text.setAttribute("y", "16");
        text.setAttribute("text-anchor", "middle");
        text.setAttribute("font-size", "11");
        text.setAttribute("fill", "currentColor");
        text.setAttribute("stroke", "none");
        text.textContent = (letter || "?").toUpperCase();
        svg.appendChild(text);
        return svg;
      }

      for (var i = 0; i < addons.length; i++) {
        var a = addons[i];
        var pages = a.pages || [];
        var hasMultiple = pages.length > 1;

        // ── Addon row ──────────────────────────────────────────
        var row = document.createElement("button");
        row.className = "veil-addon-item";
        row.setAttribute("data-tooltip", a.name);

        var icon = document.createElement("span");
        icon.className = "veil-addon-item__icon";
        icon.appendChild(makeIconSvg(a.name.charAt(0)));
        row.appendChild(icon);

        var nameEl = document.createElement("span");
        nameEl.className = "veil-addon-item__name";
        nameEl.textContent = a.name;
        row.appendChild(nameEl);

        if (hasMultiple) {
          var caret = document.createElement("span");
          caret.className = "veil-addon-item__caret";
          var ns = "http://www.w3.org/2000/svg";
          var csvg = document.createElementNS(ns, "svg");
          csvg.setAttribute("viewBox", "0 0 24 24");
          csvg.setAttribute("fill", "none");
          csvg.setAttribute("stroke", "currentColor");
          csvg.setAttribute("stroke-width", "1.8");
          csvg.setAttribute("stroke-linecap", "round");
          csvg.setAttribute("stroke-linejoin", "round");
          var poly = document.createElementNS(ns, "polyline");
          poly.setAttribute("points", "9 6 15 12 9 18");
          csvg.appendChild(poly);
          caret.appendChild(csvg);
          row.appendChild(caret);
        }
        host.appendChild(row);

        // ── Children (pages) — hidden by default unless single ─
        var children = document.createElement("div");
        children.className = "veil-addon-children";
        if (!hasMultiple) children.style.display = "none";
        for (var j = 0; j < pages.length; j++) {
          var p = pages[j];
          var pageItem = document.createElement("button");
          pageItem.className = "veil-addon-page";
          pageItem.setAttribute("data-navigate", p.route);
          pageItem.setAttribute("data-tooltip", p.label);
          pageItem.textContent = p.label;
          (function (btn, route) {
            btn.addEventListener("click", function () { Veil.navigate(route); });
          })(pageItem, p.route);
          children.appendChild(pageItem);
        }
        host.appendChild(children);

        // Row click: toggle children, or navigate if single page.
        (function (btn, kids, multiple, firstRoute) {
          btn.addEventListener("click", function () {
            if (!multiple) {
              if (firstRoute) Veil.navigate(firstRoute);
              return;
            }
            var open = btn.classList.contains("is-open");
            if (open) {
              btn.classList.remove("is-open");
              kids.style.display = "none";
            } else {
              btn.classList.add("is-open");
              kids.style.display = "flex";
            }
          });
        })(row, children, hasMultiple, pages.length ? pages[0].route : null);
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
    // PRISM's selector engine does not support comma-separated selector
    // lists in querySelectorAll — split and union the matches ourselves
    // so every nav variant gets cleared on every navigation.
    var navSelectors = [
      ".veil-nav-item",
      ".prism-nav-item",
      ".nav-item",
      ".quick-action-btn",
      ".veil-foot-btn",
      ".veil-fnav-btn"
    ];
    for (var s = 0; s < navSelectors.length; s++) {
      var nodes = document.querySelectorAll(navSelectors[s]);
      for (var i = 0; i < nodes.length; i++) {
        nodes[i].classList.remove("active");
        nodes[i].classList.remove("is-active");
      }
    }
    if (!targetPage) return;
    var matches = document.querySelectorAll('[data-navigate="' + targetPage + '"]');
    for (var k = 0; k < matches.length; k++) {
      var el = matches[k];
      if (
        el.classList.contains("veil-nav-item") ||
        el.classList.contains("prism-nav-item") ||
        el.classList.contains("veil-fnav-btn")
      ) {
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

    initInputExtensions();
  }

  // ── Input extensions: onRightClick / onRightClickF + LMB hold ─────
  // PRISM dispatches a `contextmenu` event on right-click. We walk the
  // ancestor chain looking for two attributes:
  //
  //   onRightClick="expr"   — full override. Runs `expr` and prevents
  //                            the built-in PRISM menu from showing.
  //                            (The host suppresses its menu when this
  //                            handler calls `preventDefault()`.)
  //   onRightClickF="expr"  — additive. Runs `expr` (may be empty) and
  //                            prevents the built-in menu on the first
  //                            right-click; on a second right-click on
  //                            the same element within 600ms we ask the
  //                            host to open the built-in menu via
  //                            `__or_requestPrismMenu(x, y)`.
  //
  // We also implement LMB hold timing globally:
  //   - 1000ms hold → dispatch a synthetic `longclick` event so addon
  //                    JS can react (the "developer function").
  //   - 2000ms hold → ask the host to open the built-in PRISM menu.
  //
  // Scrollbars / active scroll suppression is intentionally skipped
  // since PRISM does not currently expose native scrollbar regions.
  function initInputExtensions() {
    var lastRClick = { node: null, t: 0 };
    var R_DOUBLE_MS = 600;

    function findAttrAncestor(node, name) {
      while (node && typeof node.getAttribute === "function") {
        var v = node.getAttribute(name);
        if (v !== null && v !== undefined) return { node: node, value: v };
        node = node.parentNode;
      }
      return null;
    }

    function runExpr(expr, node, evt) {
      var src = String(expr || "").trim();
      if (!src) return;
      try {
        // Evaluate as a function body so handlers can use `event` and
        // `this` (= the element that carries the attribute).
        var fn = new Function("event", src);
        fn.call(node, evt);
      } catch (e) {
        if (window.console) console.error("[VEIL] inline handler error:", e, "in", src);
      }
    }

    document.addEventListener("contextmenu", function (evt) {
      var target = evt.target;
      var override   = findAttrAncestor(target, "onRightClick");
      var additive   = findAttrAncestor(target, "onRightClickF");

      if (override) {
        evt.preventDefault();
        runExpr(override.value, override.node, evt);
        lastRClick = { node: null, t: 0 };
        return;
      }

      if (additive) {
        evt.preventDefault();
        runExpr(additive.value, additive.node, evt);
        var now = Date.now();
        if (lastRClick.node === additive.node && (now - lastRClick.t) <= R_DOUBLE_MS) {
          // Second click within window → open built-in PRISM menu.
          if (typeof __or_requestPrismMenu === "function") {
            __or_requestPrismMenu(evt.clientX || 0, evt.clientY || 0);
          }
          lastRClick = { node: null, t: 0 };
        } else {
          lastRClick = { node: additive.node, t: now };
        }
        return;
      }

      // No attribute on the chain — let the host show its built-in menu.
      lastRClick = { node: null, t: 0 };
    });

    // ── LMB hold timing ───────────────────────────────────────────
    var holdTimer1 = null;   // 1s — dispatch `longclick`
    var holdTimer2 = null;   // 2s — request built-in PRISM menu
    var holdTarget = null;
    var holdPos = { x: 0, y: 0 };

    function clearHold() {
      if (holdTimer1) { clearTimeout(holdTimer1); holdTimer1 = null; }
      if (holdTimer2) { clearTimeout(holdTimer2); holdTimer2 = null; }
      holdTarget = null;
    }

    document.addEventListener("mousedown", function (evt) {
      if (evt.button !== 0) return;
      holdTarget = evt.target;
      holdPos.x = evt.clientX || 0;
      holdPos.y = evt.clientY || 0;

      holdTimer1 = setTimeout(function () {
        if (!holdTarget) return;
        var ev2 = {
          type: "longclick",
          target: holdTarget,
          currentTarget: holdTarget,
          clientX: holdPos.x,
          clientY: holdPos.y,
          button: 0,
          buttons: 1,
          defaultPrevented: false,
          stopPropagation: function () {},
          preventDefault: function () { this.defaultPrevented = true; }
        };
        // Best-effort dispatch via PRISM's listener cache. Manually walk
        // ancestors so handlers registered with addEventListener fire.
        var n = holdTarget;
        while (n) {
          if (n._eventListeners && n._eventListeners.longclick) {
            var fns = n._eventListeners.longclick.slice();
            ev2.currentTarget = n;
            for (var i = 0; i < fns.length; i++) {
              try { fns[i].call(n, ev2); } catch (e) { console.error("longclick:", e); }
            }
          }
          n = n.parentNode;
        }
      }, 1000);

      holdTimer2 = setTimeout(function () {
        if (typeof __or_requestPrismMenu === "function") {
          __or_requestPrismMenu(holdPos.x, holdPos.y);
        }
      }, 2000);
    });

    document.addEventListener("mouseup",   clearHold);
    document.addEventListener("mousemove", function (evt) {
      // Cancel hold timers if the cursor strays too far from the press
      // origin — treat that as a drag, not a hold.
      if (!holdTarget) return;
      var dx = (evt.clientX || 0) - holdPos.x;
      var dy = (evt.clientY || 0) - holdPos.y;
      if ((dx * dx + dy * dy) > 64) clearHold();
    });
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
