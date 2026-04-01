// ═══════════════════════════════════════════════════════════════════════════
//  OpenFramework.js — Interactive behaviors for OpenFramework layout system
//  Tooltips, navigation active state, entrance animations
// ═══════════════════════════════════════════════════════════════════════════

(function () {
  "use strict";

  // ── Tooltip ─────────────────────────────────────────────────────────────
  var tooltipEl = null;
  var tooltipTimer = null;

  function ensureTooltipElement() {
    if (tooltipEl) return tooltipEl;
    tooltipEl = document.createElement("div");
    tooltipEl.setAttribute("class", "of-tooltip");
    document.body.appendChild(tooltipEl);
    return tooltipEl;
  }

  function showTooltip(anchor) {
    var text = anchor.getAttribute("data-tooltip");
    if (!text) return;
    var tip = ensureTooltipElement();
    tip.textContent = text;
    tip.classList.remove("visible");

    // Position: to the right of the anchor with 8px gap
    // For horizontal bars, position below instead
    var rect = anchor.getBoundingClientRect
      ? anchor.getBoundingClientRect()
      : null;

    // Default: right of element (for vertical sidebar)
    tip.style.left = "56px";
    tip.style.top = "auto";

    // Delay show for Fluent feel (200ms)
    tooltipTimer = setTimeout(function () {
      tip.classList.add("visible");
    }, 200);
  }

  function hideTooltip() {
    if (tooltipTimer) {
      clearTimeout(tooltipTimer);
      tooltipTimer = null;
    }
    if (tooltipEl) {
      tooltipEl.classList.remove("visible");
    }
  }

  // ── Navigation Active State ─────────────────────────────────────────────
  function setActiveNavItem(targetPage) {
    // Remove active from all quick-action-btn and nav-item elements
    var allBtns = document.querySelectorAll(".quick-action-btn");
    for (var i = 0; i < allBtns.length; i++) {
      allBtns[i].classList.remove("active");
    }
    var allNavItems = document.querySelectorAll(".nav-item");
    for (var j = 0; j < allNavItems.length; j++) {
      allNavItems[j].classList.remove("active");
    }

    // Set active on matching element
    if (targetPage) {
      var btns = document.querySelectorAll(".quick-action-btn");
      for (var k = 0; k < btns.length; k++) {
        if (btns[k].getAttribute("data-navigate") === targetPage) {
          btns[k].classList.add("active");
        }
      }
      var navItems = document.querySelectorAll(".nav-item");
      for (var l = 0; l < navItems.length; l++) {
        if (navItems[l].getAttribute("data-navigate") === targetPage) {
          navItems[l].classList.add("active");
        }
      }
    }
  }

  // ── Init ────────────────────────────────────────────────────────────────
  function init() {
    // Attach tooltip hover events to [data-tooltip] elements
    var tooltipTargets = document.querySelectorAll("[data-tooltip]");
    for (var i = 0; i < tooltipTargets.length; i++) {
      (function (el) {
        el.addEventListener("mouseenter", function () {
          showTooltip(el);
        });
        el.addEventListener("mouseleave", function () {
          hideTooltip();
        });
      })(tooltipTargets[i]);
    }

    // Attach click handlers to navigate buttons for active state
    var navBtns = document.querySelectorAll("[data-navigate]");
    for (var j = 0; j < navBtns.length; j++) {
      (function (btn) {
        btn.addEventListener("click", function () {
          setActiveNavItem(btn.getAttribute("data-navigate"));
        });
      })(navBtns[j]);
    }

    // Set initial active state based on default page
    var pageContent = document.querySelector("page-content");
    if (pageContent) {
      var defaultPage = pageContent.getAttribute("data-active-content") ||
                        pageContent.getAttribute("data-default") ||
                        "home";
      setActiveNavItem(defaultPage);
    } else {
      setActiveNavItem("home");
    }
  }

  // Run init on DOMContentLoaded
  if (typeof addEventListener === "function") {
    addEventListener("DOMContentLoaded", init);
  }
})();