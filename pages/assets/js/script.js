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
    if (!window.Veil) return;
    var pc = $("page-content");
    var active = pc && pc.getAttribute("data-active-content");
    if (active && active !== "home") return;
    if (!$("[data-stat='addons']")) return;
    var p = window.Veil.ipc("registry", "full", {});
    if (!p || typeof p.then !== "function") return;

    p.then(function (full) {
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

  // Tear down any data-page timers/listeners from a prior page-swap. We
  // must clear EVERYTHING here because cached element refs from a previous
  // mount hold node IDs that get reused for unrelated elements after the
  // page-content swap, causing stray "ON" / "just now" text to land on
  // random nodes (sidebar items, dashboard cards, etc.).
  function teardownDataPage() {
    if (window.__veil_data_pull_timer) {
      clearInterval(window.__veil_data_pull_timer);
      window.__veil_data_pull_timer = null;
    }
    if (window.__veil_data_heartbeat_timer) {
      clearInterval(window.__veil_data_heartbeat_timer);
      window.__veil_data_heartbeat_timer = null;
    }
    window.__veil_data_state = null;
  }

  function initDataPage() {
    teardownDataPage();
    if (!window.Veil) return;
    // Use lazy lookups for everything: a cached ref captured here would
    // outlive the data fragment after a content swap and start writing
    // into whatever node now occupies its old ID.
    if (!$("[data-veil-channels]")) return;

    var state = {
      heartbeatEnabled: true,
      heartbeatLastSent: 0,
      fastRateMs: 50,
      slowRateMs: 1000,
      channels: {}
    };
    window.__veil_data_state = state;

    function isActive() {
      var pc = $("page-content");
      var active = pc && pc.getAttribute("data-active-content");
      return active === "data" || active === null;
    }

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
      if (!isActive()) return;
      var pill = $("#veil-ui-heartbeat-indicator");
      var st   = $("#veil-ui-heartbeat-state");
      var seen = $("#veil-ui-heartbeat-last-seen");
      if (pill) pill.classList.toggle("is-on", !!state.heartbeatEnabled);
      if (st)   st.textContent   = state.heartbeatEnabled ? "ON" : "OFF";
      if (seen) seen.textContent = state.heartbeatLastSent ? formatAgo(state.heartbeatLastSent) : "never";
    }

    function beatOnce() {
      if (!state.heartbeatEnabled || !window.Veil) return;
      var p = window.Veil.ipc("backend", "ui_heartbeat", {});
      state.heartbeatLastSent = Date.now();
      renderHeartbeatUi();
      if (p && typeof p.catch === "function") p.catch(function () {});
    }

    function syncHeartbeatTimer() {
      if (window.__veil_data_heartbeat_timer) {
        clearInterval(window.__veil_data_heartbeat_timer);
        window.__veil_data_heartbeat_timer = null;
      }
      if (state.heartbeatEnabled) {
        beatOnce();
        window.__veil_data_heartbeat_timer = setInterval(beatOnce, 1000);
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

    // ── Channel grouping for the cards layout ────────────────────
    // The data page now groups raw sections into a small set of
    // category cards. Each card shows a title and three status dots:
    //   1. Coverage  — fraction of the card's sections currently
    //      reporting data (none → red, some → yellow, all → green).
    //   2. Activity  — most-recent fingerprint change across the
    //      card's sections (<2s green, <30s yellow, else red).
    //   3. Latency   — most-recent value-arrival timestamp across
    //      the card's sections (<2s green, <10s yellow, else red).
    var CARD_GROUPS = [
      { id: "cpu",     title: "CPU",            sections: ["cpu"] },
      { id: "gpu",     title: "GPU",            sections: ["gpu"] },
      { id: "ram",     title: "Memory",         sections: ["ram"] },
      { id: "storage", title: "Storage",        sections: ["storage"] },
      { id: "displays",title: "Displays",       sections: ["displays"] },
      { id: "network", title: "Network",        sections: ["network", "wifi", "bluetooth"] },
      { id: "audio",   title: "Audio & Media",  sections: ["audio", "media"] },
      { id: "input",   title: "Input",          sections: ["keyboard", "mouse", "idle"] },
      { id: "system",  title: "System",         sections: ["time", "system", "processes", "power"] }
    ];

    function ensureCard(host, group) {
      var card = host.querySelector('[data-card="' + group.id + '"]');
      if (card) return card;
      card = document.createElement("div");
      card.className = "veil-data-card";
      card.setAttribute("data-card", group.id);

      var head = document.createElement("div");
      head.className = "veil-data-card__head";
      var title = document.createElement("span");
      title.className = "veil-data-card__title";
      title.textContent = group.title;
      var dots = document.createElement("span");
      dots.className = "veil-data-card__dots";
      ["coverage", "activity", "latency"].forEach(function (k) {
        var d = document.createElement("span");
        d.className = "veil-data-dot";
        d.setAttribute("data-dot", k);
        dots.appendChild(d);
      });
      head.appendChild(title);
      head.appendChild(dots);
      card.appendChild(head);

      var sections = document.createElement("div");
      sections.className = "veil-data-card__sections";
      group.sections.forEach(function (sec) {
        var row = document.createElement("div");
        row.className = "veil-data-card__section";
        row.setAttribute("data-section", sec);
        var name = document.createElement("span");
        name.className = "veil-data-card__section-name";
        name.textContent = sec;
        var val = document.createElement("span");
        val.className = "veil-data-card__section-value";
        val.textContent = "—";
        row.appendChild(name);
        row.appendChild(val);
        sections.appendChild(row);
      });
      card.appendChild(sections);

      host.appendChild(card);
      return card;
    }

    function setDot(card, key, level) {
      var dot = card.querySelector('[data-dot="' + key + '"]');
      if (!dot) return;
      var want = "veil-data-dot is-" + level;
      if (dot.className !== want) dot.className = want;
    }

    function setSection(card, sec, value, stale) {
      var row = card.querySelector('[data-section="' + sec + '"]');
      if (!row) return;
      var val = row.querySelector('[data-col="value"]') || row.querySelector(".veil-data-card__section-value");
      if (val && val.textContent !== value) val.textContent = value;
      var staleNow = !!stale;
      var hasStale = row.classList.contains("is-stale");
      if (staleNow !== hasStale) row.classList.toggle("is-stale", staleNow);
    }

    function renderCards(sysdata) {
      if (!isActive()) return;
      var host = $("[data-veil-channels]");
      if (!host) return;
      var filterInput = $("#veil-data-filter");
      var filterText = filterInput ? String(filterInput.value || "").toLowerCase().trim() : "";

      var empty = host.querySelector('[data-veil-empty]');
      if (empty && empty.parentNode) empty.parentNode.removeChild(empty);

      var now = Date.now();
      var anyVisible = 0;

      for (var gi = 0; gi < CARD_GROUPS.length; gi++) {
        var group = CARD_GROUPS[gi];
        var card = ensureCard(host, group);

        var present = 0, total = group.sections.length;
        var newestChange = 0, newestSeen = 0;
        var matchesFilter = !filterText || group.title.toLowerCase().indexOf(filterText) !== -1;

        for (var si = 0; si < group.sections.length; si++) {
          var sec = group.sections[si];
          var value = sysdata ? sysdata[sec] : undefined;

          if (typeof value === "undefined") {
            setSection(card, sec, "—", true);
            continue;
          }

          var fp = safeJsonString(value);
          if (!state.channels[sec]) {
            state.channels[sec] = { fingerprint: fp, changedAt: now, seenAt: now };
          } else {
            if (state.channels[sec].fingerprint !== fp) {
              state.channels[sec].fingerprint = fp;
              state.channels[sec].changedAt = now;
            }
            state.channels[sec].seenAt = now;
          }
          present++;
          if (state.channels[sec].changedAt > newestChange) newestChange = state.channels[sec].changedAt;
          if (state.channels[sec].seenAt > newestSeen) newestSeen = state.channels[sec].seenAt;

          var summary = summarizeChannel(sec, value);
          setSection(card, sec, summary, false);
          if (!matchesFilter && (sec.toLowerCase().indexOf(filterText) !== -1 ||
                                  summary.toLowerCase().indexOf(filterText) !== -1)) {
            matchesFilter = true;
          }
        }

        // Coverage dot
        var coverage = (present === 0) ? "bad" : (present < total ? "warn" : "good");
        // Activity dot
        var ageChange = newestChange ? (now - newestChange) : Infinity;
        var activity = (ageChange < 2000) ? "good" : (ageChange < 30000 ? "warn" : "bad");
        // Latency dot
        var ageSeen = newestSeen ? (now - newestSeen) : Infinity;
        var latency = (ageSeen < 2000) ? "good" : (ageSeen < 10000 ? "warn" : "bad");

        setDot(card, "coverage", coverage);
        setDot(card, "activity", activity);
        setDot(card, "latency",  latency);

        var hide = !matchesFilter;
        if (card.style.display !== (hide ? "none" : "")) card.style.display = hide ? "none" : "";
        if (!hide) anyVisible++;
      }

      var fallback = host.querySelector('[data-veil-fallback]');
      if (anyVisible === 0) {
        if (!fallback) {
          fallback = document.createElement("div");
          fallback.setAttribute("data-veil-fallback", "");
          fallback.style.gridColumn = "1 / -1";
          fallback.style.color = "var(--veil-text-tertiary)";
          fallback.style.padding = "var(--veil-space-5)";
          fallback.style.textAlign = "center";
          host.appendChild(fallback);
        }
        fallback.textContent = filterText ? "No channels matched the current filter." : "Awaiting data…";
      } else if (fallback && fallback.parentNode) {
        fallback.parentNode.removeChild(fallback);
      }
    }

    function pullDataSnapshot() {
      if (!isActive()) return;
      var p = window.Veil.ipc("registry", "snapshot", { sections: DEMAND_SECTIONS });
      if (!p || typeof p.then !== "function") return;
      p.then(function (snap) {
        var sysdata = snap && snap.sysdata ? snap.sysdata : {};
        renderCards(sysdata);
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

    var heartbeatPill = $("#veil-ui-heartbeat-indicator");
    if (heartbeatPill) {
      heartbeatPill.addEventListener("click", function () {
        setHeartbeatEnabled(!state.heartbeatEnabled);
      });
    }

    var filterInput = $("#veil-data-filter");
    if (filterInput) {
      filterInput.addEventListener("input", function () {
        pullDataSnapshot();
      });
    }

    var reloadBtn = $("[data-action='data.reload']");
    if (reloadBtn) {
      reloadBtn.addEventListener("click", function () {
        pushDemands();
        beatOnce();
        pullDataSnapshot();
      });
    }

    pushDemands();
    renderHeartbeatUi();          // paint the indicator immediately
    syncHeartbeatTimer();         // start beating now (don't wait for config)
    pullBackendConfig();
    pullDataSnapshot();

    window.__veil_data_pull_timer = setInterval(function () {
      if (!isActive()) {
        teardownDataPage();
        return;
      }
      pullDataSnapshot();
      renderHeartbeatUi();
    }, 1200);
  }

  function fetchAndRenderAddons() {
    if (!window.Veil) return;
    var p = window.Veil.ipc("registry", "list_addons", {});
    if (!p || typeof p.then !== "function") return;
    p.then(function (addons) {
      // Transform registry entries into the format consumers expect:
      // { name, id, pages: [{label, route}], description }
      var mapped = [];
      if (Array.isArray(addons)) {
        for (var i = 0; i < addons.length; i++) {
          var a = addons[i];
          var meta = a.metadata || {};
          mapped.push({
            name: meta.name || a.id || "Addon",
            id: a.id,
            description: meta.description || meta.summary || "",
            pages: meta.pages || []
          });
        }
      }
      window.__veilAddons = mapped;
      if (window.Veil && typeof window.Veil.renderAddonList === "function") {
        window.Veil.renderAddonList();
      }
      renderAddonCards();
    });
  }

  // ── Dashboard: addon cards ────────────────────────────────────────
  // Each addon is rendered as a card. Hovering reveals two controls:
  //   - start/stop toggle (sends `addon.start` / `addon.stop` IPC)
  //   - settings (navigates to the addon's settings page if declared,
  //     otherwise the addon's first non-home page).
  // Clicking the card body navigates to the addon's home page.
  //
  // We have no live "is running" query, so the running state is tracked
  // optimistically in localStorage and toggled on each user action.
  var ADDON_RUN_KEY = "veil:addon-running";
  function loadRunningSet() {
    try {
      var raw = localStorage.getItem(ADDON_RUN_KEY);
      var obj = raw ? JSON.parse(raw) : {};
      return (obj && typeof obj === "object") ? obj : {};
    } catch (e) { return {}; }
  }
  function saveRunningSet(set) {
    try { localStorage.setItem(ADDON_RUN_KEY, JSON.stringify(set)); } catch (e) {}
  }

  function pickAddonHomeRoute(addon) {
    if (!addon || !Array.isArray(addon.pages)) return null;
    for (var i = 0; i < addon.pages.length; i++) {
      var route = addon.pages[i].route || "";
      if (/\.home$/i.test(route) || /^home$/i.test(addon.pages[i].label || "")) {
        return route;
      }
    }
    return addon.pages.length ? addon.pages[0].route : null;
  }
  function pickAddonSettingsRoute(addon) {
    if (!addon || !Array.isArray(addon.pages)) return null;
    for (var i = 0; i < addon.pages.length; i++) {
      var route = addon.pages[i].route || "";
      var label = addon.pages[i].label || "";
      if (/\.settings$/i.test(route) || /^settings$/i.test(label)) return route;
    }
    return null;
  }

  function makeIcon(letter) {
    var ns = "http://www.w3.org/2000/svg";
    var svg = document.createElementNS(ns, "svg");
    svg.setAttribute("viewBox", "0 0 32 32");
    svg.setAttribute("width", "26");
    svg.setAttribute("height", "26");
    var t = document.createElementNS(ns, "text");
    t.setAttribute("x", "16");
    t.setAttribute("y", "21");
    t.setAttribute("text-anchor", "middle");
    t.setAttribute("font-size", "16");
    t.setAttribute("font-weight", "700");
    t.setAttribute("fill", "currentColor");
    t.textContent = (letter || "?").toUpperCase();
    svg.appendChild(t);
    return svg;
  }

  function renderAddonCards() {
    var host = $("[data-veil-addon-cards]");
    if (!host) return;
    var pc = $("page-content");
    var active = pc && pc.getAttribute("data-active-content");
    if (active && active !== "home") return;

    var addons = Array.isArray(window.__veilAddons) ? window.__veilAddons : [];
    // Wipe and rebuild — addon list is a low-frequency update and the
    // per-card hover/click handlers need fresh closures over the addon data.
    while (host.firstChild) host.removeChild(host.firstChild);

    if (!addons.length) {
      var empty = document.createElement("div");
      empty.className = "veil-empty";
      empty.setAttribute("data-veil-addon-cards-empty", "");
      empty.innerHTML =
        '<span class="veil-empty__mark">' +
          '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">' +
            '<rect x="3" y="3" width="18" height="18" rx="2"/>' +
            '<line x1="9" y1="3" x2="9" y2="21"/>' +
            '<line x1="15" y1="3" x2="15" y2="21"/>' +
          '</svg>' +
        '</span>' +
        '<p>No addons installed yet.</p>' +
        '<p class="veil-empty__hint">Browse the store to install your first addon.</p>';
      host.appendChild(empty);
      return;
    }

    var running = loadRunningSet();

    addons.forEach(function (addon) {
      var card = document.createElement("div");
      card.className = "veil-addon-card";
      card.setAttribute("data-addon-id", addon.id);
      if (running[addon.id]) card.classList.add("is-running");

      var iconWrap = document.createElement("div");
      iconWrap.className = "veil-addon-card__icon";
      iconWrap.appendChild(makeIcon((addon.name || "?").charAt(0)));
      card.appendChild(iconWrap);

      var name = document.createElement("div");
      name.className = "veil-addon-card__name";
      name.textContent = addon.name;
      card.appendChild(name);

      if (addon.description) {
        var desc = document.createElement("div");
        desc.className = "veil-addon-card__desc";
        desc.textContent = addon.description;
        card.appendChild(desc);
      }

      var status = document.createElement("div");
      status.className = "veil-addon-card__status";
      var dot = document.createElement("span");
      dot.className = "veil-addon-card__status-dot";
      var label = document.createElement("span");
      label.textContent = running[addon.id] ? "Running" : "Stopped";
      status.appendChild(dot);
      status.appendChild(label);
      card.appendChild(status);

      // ── Hover-revealed controls ───────────────────────────────
      var controls = document.createElement("div");
      controls.className = "veil-addon-card__controls";

      var runBtn = document.createElement("button");
      runBtn.className = "veil-addon-card__btn";
      runBtn.setAttribute("data-tooltip", running[addon.id] ? "Stop addon" : "Start addon");
      runBtn.setAttribute("aria-label", running[addon.id] ? "Stop" : "Start");
      runBtn.innerHTML = running[addon.id]
        ? '<svg viewBox="0 0 24 24" fill="currentColor"><rect x="6" y="6" width="12" height="12" rx="1"/></svg>'
        : '<svg viewBox="0 0 24 24" fill="currentColor"><polygon points="6,4 20,12 6,20"/></svg>';
      if (running[addon.id]) runBtn.classList.add("veil-addon-card__btn--stop");
      runBtn.addEventListener("click", function (ev) {
        ev.stopPropagation();
        var nowRunning = !running[addon.id];
        running[addon.id] = nowRunning;
        saveRunningSet(running);
        var cmd = nowRunning ? "start" : "stop";
        if (window.Veil) {
          var p = window.Veil.ipc("addon", cmd, { addon_name: addon.id });
          if (p && typeof p.catch === "function") p.catch(function () {});
          window.Veil.toast(addon.name + " " + (nowRunning ? "started" : "stopped"), nowRunning ? "violet" : "blood");
        }
        renderAddonCards();
      });
      controls.appendChild(runBtn);

      var settingsRoute = pickAddonSettingsRoute(addon);
      if (settingsRoute) {
        var setBtn = document.createElement("button");
        setBtn.className = "veil-addon-card__btn";
        setBtn.setAttribute("data-tooltip", "Settings");
        setBtn.setAttribute("aria-label", "Settings");
        setBtn.innerHTML = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.6 1.6 0 0 0 .3 1.7l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.6 1.6 0 0 0-1.7-.3 1.6 1.6 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.6 1.6 0 0 0-1-1.5 1.6 1.6 0 0 0-1.7.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.6 1.6 0 0 0 .3-1.7 1.6 1.6 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.6 1.6 0 0 0 1.5-1 1.6 1.6 0 0 0-.3-1.7l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.6 1.6 0 0 0 1.7.3h.1a1.6 1.6 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.6 1.6 0 0 0 1 1.5 1.6 1.6 0 0 0 1.7-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.6 1.6 0 0 0-.3 1.7V9a1.6 1.6 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.6 1.6 0 0 0-1.5 1z"/></svg>';
        setBtn.addEventListener("click", function (ev) {
          ev.stopPropagation();
          if (window.Veil) window.Veil.navigate(settingsRoute);
        });
        controls.appendChild(setBtn);
      }
      card.appendChild(controls);

      // Card body click → navigate to the addon's home page.
      var homeRoute = pickAddonHomeRoute(addon);
      if (homeRoute) {
        card.addEventListener("click", function () {
          if (window.Veil) window.Veil.navigate(homeRoute);
        });
      } else {
        card.style.cursor = "default";
      }

      host.appendChild(card);
    });
  }

  function init() {
    bindAddonRefresh();
    bindOpenFolder();
    // Seed displays tracking so monitors count is populated on the home page
    // before the Data page's pushDemands() ever runs.
    if (window.Veil) {
      window.Veil.ipc("tracking", "set_demands", { sections: ["displays"] });
    }
    refreshHomeStats();
    fetchAndRenderAddons();
    setInterval(refreshHomeStats, 5000);
    setInterval(fetchAndRenderAddons, 15000);
    initDataPage();

    // Called by PRISM after a page-content swap so page-specific JS can
    // initialize against the freshly inserted DOM. Crucially we MUST
    // tear down per-page state when leaving a page so timers/closures
    // don't keep stomping element IDs that have been reused for new
    // unrelated nodes.
    window.__veil_on_content_swap = function (contentId) {
      teardownDataPage();
      if (contentId === "data") {
        initDataPage();
      } else if (contentId === "home") {
        refreshHomeStats();
        renderAddonCards();
      }
    };
  }

  if (typeof addEventListener === "function") {
    addEventListener("DOMContentLoaded", init);
  }
})();
