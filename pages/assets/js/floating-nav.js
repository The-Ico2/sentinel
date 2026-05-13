// VEIL Floating Navigation Bar.
//
// Replaces the old fixed sidebar. The bar is absolutely positioned inside
// `.veil-app`, can be dragged anywhere, and supports two orientations
// (horizontal / vertical) — each with an "inverse" variant that reverses
// the button order along the bar's main axis.
//
// Drag is implemented against PRISM's `mousedown` / `mousemove` /
// `mouseup` DOM events, which also honour an implicit pointer capture
// for the duration of the press so the bar follows the cursor even when
// it leaves the handle.
//
// Persisted state (localStorage key "veil:floating-nav"):
//   { left, top, orientation, inverse, pinned, snap }
// where `snap` is one of "tl|t|tr|l|c|r|bl|b|br" or "free".

(function () {
  "use strict";

  var STORAGE_KEY = "veil:floating-nav";

  function $(sel, root) { return (root || document).querySelector(sel); }
  function $$(sel, root) { return Array.prototype.slice.call((root || document).querySelectorAll(sel)); }

  function loadState() {
    try {
      var raw = localStorage.getItem(STORAGE_KEY);
      if (!raw) return null;
      var s = JSON.parse(raw);
      if (typeof s !== "object" || !s) return null;
      return s;
    } catch (e) { return null; }
  }

  function saveState(state) {
    try { localStorage.setItem(STORAGE_KEY, JSON.stringify(state)); } catch (e) {}
  }

  function clamp(n, lo, hi) { return Math.max(lo, Math.min(hi, n)); }

  function viewportSize() {
    var w = (typeof window !== "undefined" && window.innerWidth)  || 1280;
    var h = (typeof window !== "undefined" && window.innerHeight) || 720;
    return { w: w, h: h };
  }

  function applyClasses(bar, state) {
    bar.classList.toggle("is-vertical",  state.orientation === "vertical");
    bar.classList.toggle("is-horizontal", state.orientation !== "vertical");
    bar.classList.toggle("is-inverse", !!state.inverse);
    bar.classList.toggle("is-pinned",  !!state.pinned);
  }

  // Convert a logical snap target into pixel coordinates given the bar's
  // own measured size. Falls back to bottom-centre if measurements fail.
  function snapToCoords(bar, snap) {
    var v = viewportSize();
    var rect = { width: bar.offsetWidth || 240, height: bar.offsetHeight || 48 };
    var pad = 12;
    var x, y;
    switch (snap) {
      case "tl": x = pad;                               y = pad;                                break;
      case "t":  x = (v.w - rect.width) / 2;            y = pad;                                break;
      case "tr": x = v.w - rect.width - pad;            y = pad;                                break;
      case "l":  x = pad;                               y = (v.h - rect.height) / 2;            break;
      case "c":  x = (v.w - rect.width) / 2;            y = (v.h - rect.height) / 2;            break;
      case "r":  x = v.w - rect.width - pad;            y = (v.h - rect.height) / 2;            break;
      case "bl": x = pad;                               y = v.h - rect.height - pad;            break;
      case "b":  x = (v.w - rect.width) / 2;            y = v.h - rect.height - pad;            break;
      case "br": x = v.w - rect.width - pad;            y = v.h - rect.height - pad;            break;
      default:   x = (v.w - rect.width) / 2;            y = v.h - rect.height - pad;            break;
    }
    return { x: clamp(x, 0, v.w - rect.width), y: clamp(y, 0, v.h - rect.height) };
  }

  function applyPosition(bar, state) {
    if (state.snap && state.snap !== "free") {
      var c = snapToCoords(bar, state.snap);
      state.left = c.x;
      state.top  = c.y;
    }
    if (typeof state.left === "number") bar.style.left = state.left + "px";
    if (typeof state.top  === "number") bar.style.top  = state.top  + "px";
    // Free-floating: clear the default centred-bottom transform that
    // applies when no inline position is set.
    bar.style.transform = "none";
    bar.style.right  = "auto";
    bar.style.bottom = "auto";
  }

  function init() {
    var bar = $("#veil-floating-nav");
    if (!bar) return;

    var menu      = bar.querySelector("[data-veil-fnav-menu]");
    var cog       = bar.querySelector("[data-veil-fnav-cog]");
    var handle    = bar.querySelector("[data-veil-fnav-handle]");

    // ── Restore persisted state ─────────────────────────────────────────
    var stored = loadState();
    var state = {
      left:        (stored && typeof stored.left === "number") ? stored.left : null,
      top:         (stored && typeof stored.top  === "number") ? stored.top  : null,
      orientation: (stored && stored.orientation === "vertical") ? "vertical" : "horizontal",
      inverse:     !!(stored && stored.inverse),
      pinned:      !!(stored && stored.pinned),
      snap:        (stored && typeof stored.snap === "string") ? stored.snap : "b"
    };
    applyClasses(bar, state);
    applyPosition(bar, state);

    function persist() { saveState(state); }

    // Re-clamp the bar inside the current viewport. Used during drag and on
    // window resize so the menu can never end up outside the visible area.
    function clampToViewport() {
      var v = viewportSize();
      var maxX = Math.max(0, v.w - bar.offsetWidth);
      var maxY = Math.max(0, v.h - bar.offsetHeight);
      if (typeof state.left === "number") state.left = clamp(state.left, 0, maxX);
      if (typeof state.top  === "number") state.top  = clamp(state.top,  0, maxY);
    }

    // ── Drag (mousedown on handle → document mousemove/mouseup) ─────────
    var drag = null;

    handle.addEventListener("mousedown", function (ev) {
      if (state.pinned) return;
      var rect = bar.getBoundingClientRect ? bar.getBoundingClientRect() : null;
      drag = {
        startX: ev.clientX,
        startY: ev.clientY,
        baseLeft: (rect && rect.left) || parseFloat(bar.style.left) || 0,
        baseTop:  (rect && rect.top)  || parseFloat(bar.style.top)  || 0
      };
      bar.classList.add("is-dragging");
      // Hide the popover while dragging so it doesn't follow weirdly.
      if (menu) menu.setAttribute("hidden", "");
      if (ev.preventDefault) ev.preventDefault();
    });

    document.addEventListener("mousemove", function (ev) {
      if (!drag) return;
      var v = viewportSize();
      var maxX = Math.max(0, v.w - bar.offsetWidth);
      var maxY = Math.max(0, v.h - bar.offsetHeight);
      state.left = clamp(drag.baseLeft + (ev.clientX - drag.startX), 0, maxX);
      state.top  = clamp(drag.baseTop  + (ev.clientY - drag.startY), 0, maxY);
      state.snap = "free";
      applyPosition(bar, state);
    });

    document.addEventListener("mouseup", function () {
      if (!drag) return;
      drag = null;
      bar.classList.remove("is-dragging");
      clampToViewport();
      applyPosition(bar, state);
      persist();
    });

    // Re-clamp on viewport resize so a previously valid position never ends
    // up half off-screen after the window shrinks.
    addEventListener("resize", function () {
      if (state.snap && state.snap !== "free") {
        applyPosition(bar, state);
      } else {
        clampToViewport();
        applyPosition(bar, state);
      }
      persist();
    });

    // ── Cog menu toggle ────────────────────────────────────────────────
    cog.addEventListener("click", function (ev) {
      ev.stopPropagation();
      if (!menu) return;
      if (menu.hasAttribute("hidden")) {
        refreshMenuState();
        menu.removeAttribute("hidden");
      } else {
        menu.setAttribute("hidden", "");
      }
    });

    // Close the menu when clicking anywhere outside it.
    document.addEventListener("click", function (ev) {
      if (!menu || menu.hasAttribute("hidden")) return;
      var t = ev.target;
      while (t) {
        if (t === menu || t === cog) return;
        t = t.parentNode;
      }
      menu.setAttribute("hidden", "");
    });

    function refreshMenuState() {
      var orientBtns = $$("[data-veil-fnav-orient]", menu);
      for (var i = 0; i < orientBtns.length; i++) {
        var on = orientBtns[i].getAttribute("data-veil-fnav-orient") === state.orientation;
        orientBtns[i].classList.toggle("is-active", on);
      }
      var inv = menu.querySelector("[data-veil-fnav-inverse-toggle]");
      if (inv) inv.classList.toggle("is-active", !!state.inverse);
      var pin = menu.querySelector("[data-veil-fnav-pin-toggle]");
      if (pin) pin.classList.toggle("is-active", !!state.pinned);
      var snapBtns = $$("[data-veil-fnav-snap]", menu);
      for (var j = 0; j < snapBtns.length; j++) {
        var s = snapBtns[j].getAttribute("data-veil-fnav-snap");
        snapBtns[j].classList.toggle("is-active", s === state.snap);
      }
    }

    $$("[data-veil-fnav-orient]", menu).forEach(function (btn) {
      btn.addEventListener("click", function () {
        state.orientation = btn.getAttribute("data-veil-fnav-orient");
        applyClasses(bar, state);
        // Re-snap after orientation change so the bar's new dimensions
        // are accounted for.
        if (state.snap && state.snap !== "free") applyPosition(bar, state);
        persist();
        refreshMenuState();
      });
    });

    var invBtn = menu.querySelector("[data-veil-fnav-inverse-toggle]");
    if (invBtn) invBtn.addEventListener("click", function () {
      state.inverse = !state.inverse;
      applyClasses(bar, state);
      persist();
      refreshMenuState();
    });

    var pinBtn = menu.querySelector("[data-veil-fnav-pin-toggle]");
    if (pinBtn) pinBtn.addEventListener("click", function () {
      state.pinned = !state.pinned;
      applyClasses(bar, state);
      persist();
      refreshMenuState();
    });

    $$("[data-veil-fnav-snap]", menu).forEach(function (btn) {
      btn.addEventListener("click", function () {
        state.snap = btn.getAttribute("data-veil-fnav-snap");
        applyPosition(bar, state);
        persist();
        refreshMenuState();
        menu.setAttribute("hidden", "");
      });
    });
  }

  if (typeof addEventListener === "function") {
    addEventListener("DOMContentLoaded", init);
  } else {
    init();
  }
})();
