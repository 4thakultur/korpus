// Korpus progressive enhancement. Every feature here is optional: all pages
// work with plain links and forms when this script does not run.
(function () {
  "use strict";
  document.documentElement.classList.add("js");

  function onReady(fn) {
    if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", fn);
    else fn();
  }

  // Interface text in the page's language, set on <body> by the server.
  function t(key, fallback) {
    var v = document.body && document.body.getAttribute("data-t-" + key);
    return v || fallback;
  }

  function parseNumber(s) {
    var v = parseFloat(String(s).trim().replace(",", "."));
    return isFinite(v) ? v : null;
  }

  // Only the latest external search may update the page.
  var remoteRequest = null;

  // Search results are one list. The server renders saved foods; results
  // from external food databases are fetched separately (data-src) and
  // appended, so a slow database never delays the user's own foods.
  function finishResults(section) {
    var loading = section.querySelector("#results-loading");
    if (loading) loading.remove();
    var list = section.querySelector("#result-list");
    var empty = section.querySelector("#no-results");
    if (empty && list && !list.querySelector("li")) empty.hidden = false;
    section.setAttribute("aria-busy", "false");
  }

  // When a database briefly refuses because of its rate limit, try again.
  var RETRY_DELAY = 5000;
  var MAX_RETRIES = 6;

  function loadRemote(section, src, attempt) {
    src = src || section.getAttribute("data-src");
    attempt = attempt || 0;
    if (!src) return;
    section.removeAttribute("data-src");
    if (remoteRequest) remoteRequest.abort();
    var controller = window.AbortController ? new AbortController() : null;
    remoteRequest = controller;
    section.setAttribute("aria-busy", "true");
    var errors = section.querySelector("#results-errors");
    fetch(src, {
      headers: { "X-Requested-With": "fetch" },
      credentials: "same-origin",
      signal: controller ? controller.signal : undefined
    })
      .then(function (r) {
        if (!r.ok) throw new Error("HTTP " + r.status);
        return r.text();
      })
      .then(function (html) {
        if (!section.isConnected) return; // replaced by a newer search
        var doc = new DOMParser().parseFromString(html, "text/html");
        var list = section.querySelector("#result-list");
        var fresh = doc.querySelector("ul");
        if (list && fresh) {
          // A retry returns the whole page of results again.
          list.querySelectorAll("li[data-remote]").forEach(function (li) { li.remove(); });
          Array.prototype.slice.call(fresh.children).forEach(function (li) {
            li.setAttribute("data-remote", "");
            list.appendChild(li);
          });
          var next = section.querySelector("#next-if-more");
          if (next && fresh.getAttribute("data-has-more") === "true") next.hidden = false;
        }
        if (errors) {
          errors.innerHTML = "";
          doc.querySelectorAll("p").forEach(function (p) { errors.appendChild(p); });
        }
        if (fresh && fresh.getAttribute("data-retry") === "true" && attempt < MAX_RETRIES) {
          setTimeout(function () {
            if (section.isConnected) loadRemote(section, src, attempt + 1);
          }, RETRY_DELAY);
          return;
        }
        finishResults(section);
      })
      .catch(function (e) {
        if ((e && e.name === "AbortError") || !section.isConnected) return;
        if (errors) {
          var p = document.createElement("p");
          p.className = "warning";
          p.textContent = t("remote-error", "External results could not be loaded. Your own foods are still available above.");
          errors.appendChild(p);
        }
        finishResults(section);
      });
  }

  // Results refresh 300 ms after the last keystroke: saved foods first, then
  // every food database (the server waits briefly when a database's rate
  // limit is reached and caches its results).
  var SEARCH_DELAY = 300;
  var REMOTE_MIN_CHARS = 2;

  function setupLiveSearch(form) {
    var input = form.querySelector('input[type="search"]');
    if (!input) return;
    var timer = null;
    var seq = 0;

    input.addEventListener("input", function () {
      clearTimeout(timer);
      if (remoteRequest) remoteRequest.abort();
      timer = setTimeout(function () {
        var params = new URLSearchParams(new FormData(form));
        var url = "/search?" + params.toString();
        var mine = ++seq;
        fetch(url, { credentials: "same-origin" })
          .then(function (r) { return r.ok ? r.text() : Promise.reject(r.status); })
          .then(function (html) {
            if (mine !== seq) return; // a newer request is in flight
            var doc = new DOMParser().parseFromString(html, "text/html");
            var h1 = document.querySelector("main h1");
            var freshH1 = doc.querySelector("main h1");
            if (h1 && freshH1) h1.textContent = freshH1.textContent;
            if (doc.title) document.title = doc.title;
            var fresh = doc.getElementById("results");
            var current = document.getElementById("results");
            if (fresh && current) current.replaceWith(fresh);
            history.replaceState(null, "", url);
            if (!fresh || !fresh.hasAttribute("data-src")) return;
            if (input.value.trim().length < REMOTE_MIN_CHARS) {
              fresh.removeAttribute("data-src");
              finishResults(fresh);
            } else {
              loadRemote(fresh);
            }
          })
          .catch(function () { /* keep current results; submitting the form still works */ });
      }, SEARCH_DELAY);
    });
  }

  function setupNutrientPreview(form) {
    var amount = form.querySelector('[name="amount"]');
    var out = form.querySelector("[data-preview]");
    if (!amount || !out) return;
    var per = {
      kcal: parseNumber(form.getAttribute("data-kcal")),
      p: parseNumber(form.getAttribute("data-protein")),
      c: parseNumber(form.getAttribute("data-carbs")),
      f: parseNumber(form.getAttribute("data-fat"))
    };
    var decimal = (document.body && document.body.getAttribute("data-decimal")) || ".";
    function fmt(v, digits) { return v === null ? "–" : v.toFixed(digits).replace(".", decimal); }
    function scale(v, a) { return v === null ? null : (v * a) / 100; }
    function update() {
      var a = parseNumber(amount.value);
      if (a === null || a <= 0) { out.hidden = true; return; }
      out.hidden = false;
      out.textContent = t("this-amount", "This amount:") + " " + fmt(scale(per.kcal, a), 0) +
        " kcal · " + t("p", "P") + " " + fmt(scale(per.p, a), 1) + " g · " + t("c", "C") + " " +
        fmt(scale(per.c, a), 1) + " g · " + t("f", "F") + " " + fmt(scale(per.f, a), 1) + " g";
    }
    amount.addEventListener("input", update);
    form.querySelectorAll("[data-amount]").forEach(function (b) {
      b.addEventListener("click", function () {
        amount.value = b.getAttribute("data-amount");
        update();
        amount.focus();
      });
    });
    update();
  }

  // Calendar popover following the WAI-ARIA date picker dialog pattern.
  // Dates are handled in UTC so local DST changes never shift a calendar
  // day. `onChoose` receives the picked ISO date.
  var calendarCount = 0;

  function setupCalendar(input, button, anchor, todayIso, onChoose) {
    var lang = document.documentElement.lang || "en";
    var popId = "calendar-" + ++calendarCount;

    function parseIso(s) {
      var m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(s || "");
      return m ? new Date(Date.UTC(+m[1], +m[2] - 1, +m[3])) : null;
    }
    function iso(d) { return d.toISOString().slice(0, 10); }
    function addDays(d, n) { return new Date(d.getTime() + n * 86400000); }
    function addMonths(d, n) {
      var y = d.getUTCFullYear(), m = d.getUTCMonth() + n;
      var last = new Date(Date.UTC(y, m + 1, 0)).getUTCDate();
      return new Date(Date.UTC(y, m, Math.min(d.getUTCDate(), last)));
    }
    function fmt(opts) {
      opts.timeZone = "UTC";
      try { return new Intl.DateTimeFormat(lang, opts); } catch (e) { return new Intl.DateTimeFormat("en", opts); }
    }
    // 0 = Sunday. Intl.Locale weekInfo is not everywhere yet.
    var firstDay = lang.slice(0, 2) === "en" ? 0 : 1;
    try {
      var loc = new Intl.Locale(lang);
      var info = loc.getWeekInfo ? loc.getWeekInfo() : loc.weekInfo;
      if (info && info.firstDay) firstDay = info.firstDay % 7;
    } catch (e) { /* keep the default */ }

    var monthFmt = fmt({ month: "long", year: "numeric" });
    var dayFmt = fmt({ weekday: "long", day: "numeric", month: "long", year: "numeric" });
    var weekdayFmt = fmt({ weekday: "short" });
    var weekdayLongFmt = fmt({ weekday: "long" });

    var selected = parseIso(input.value) || new Date();
    var today = parseIso(todayIso) || selected;
    var focused = selected;
    var chevron = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" ' +
      'stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false"><path d="D"/></svg>';

    var pop = document.createElement("div");
    pop.className = "calendar";
    pop.id = popId;
    pop.hidden = true;
    pop.setAttribute("role", "dialog");
    pop.setAttribute("aria-modal", "false");
    pop.setAttribute("aria-labelledby", popId + "-title");
    pop.innerHTML =
      '<div class="calendar-head"><h2 class="calendar-title" id="' + popId + '-title" aria-live="polite"></h2>' +
      '<div class="calendar-arrows">' +
      '<button type="button" data-step="-1">' + chevron.replace("D", "m15 5-7 7 7 7") + '</button>' +
      '<button type="button" data-step="1">' + chevron.replace("D", "m9 5 7 7-7 7") + '</button>' +
      '</div></div><table role="grid" aria-labelledby="' + popId + '-title"><thead><tr></tr></thead><tbody></tbody></table>' +
      '<div class="calendar-foot"><button type="button" data-today-button></button></div>';
    var title = pop.querySelector(".calendar-title");
    var arrows = pop.querySelectorAll("[data-step]");
    arrows[0].setAttribute("aria-label", t("prev-month", "Previous month"));
    arrows[1].setAttribute("aria-label", t("next-month", "Next month"));
    var todayButton = pop.querySelector("[data-today-button]");
    todayButton.textContent = t("today", "Today");
    var tbody = pop.querySelector("tbody");

    // 2023-01-01 was a Sunday.
    var headRow = pop.querySelector("thead tr");
    for (var i = 0; i < 7; i++) {
      var wd = new Date(Date.UTC(2023, 0, 1 + ((firstDay + i) % 7)));
      var th = document.createElement("th");
      th.scope = "col";
      th.setAttribute("abbr", weekdayLongFmt.format(wd));
      th.textContent = weekdayFmt.format(wd).replace(/\.$/, "");
      headRow.appendChild(th);
    }
    anchor.appendChild(pop);
    button.setAttribute("aria-haspopup", "dialog");
    button.setAttribute("aria-controls", popId);
    button.setAttribute("aria-expanded", "false");

    function render() {
      title.textContent = monthFmt.format(focused);
      var y = focused.getUTCFullYear(), m = focused.getUTCMonth();
      var first = new Date(Date.UTC(y, m, 1));
      var days = new Date(Date.UTC(y, m + 1, 0)).getUTCDate();
      var lead = (first.getUTCDay() - firstDay + 7) % 7;
      var html = "";
      for (var cell = 0; cell < Math.ceil((lead + days) / 7) * 7; cell++) {
        if (cell % 7 === 0) html += "<tr>";
        var n = cell - lead + 1;
        if (n < 1 || n > days) {
          html += "<td></td>";
        } else {
          var d = new Date(Date.UTC(y, m, n)), k = iso(d);
          html += '<td><button type="button" data-date="' + k + '" tabindex="' +
            (k === iso(focused) ? "0" : "-1") + '" aria-label="' + dayFmt.format(d) + '"' +
            ' aria-pressed="' + (k === iso(selected)) + '"' +
            (k === iso(today) ? ' aria-current="date"' : "") + ">" + n + "</button></td>";
        }
        if (cell % 7 === 6) html += "</tr>";
      }
      tbody.innerHTML = html;
    }

    function focusDay() {
      var b = tbody.querySelector('[data-date="' + iso(focused) + '"]');
      if (b) b.focus();
    }
    function moveTo(d) {
      var redraw = d.getUTCMonth() !== focused.getUTCMonth() || d.getUTCFullYear() !== focused.getUTCFullYear();
      focused = d;
      if (redraw) render();
      else tbody.querySelectorAll("button").forEach(function (b) {
        b.tabIndex = b.getAttribute("data-date") === iso(d) ? 0 : -1;
      });
      focusDay();
    }
    function open() {
      focused = selected;
      render();
      pop.hidden = false;
      button.setAttribute("aria-expanded", "true");
      focusDay();
    }
    function close(restoreFocus) {
      if (pop.hidden) return;
      pop.hidden = true;
      button.setAttribute("aria-expanded", "false");
      if (restoreFocus) button.focus();
    }
    function choose(d) {
      if (iso(d) === iso(selected)) { close(true); return; }
      selected = d;
      close(true);
      onChoose(iso(d));
    }

    button.addEventListener("click", function () {
      if (pop.hidden) open(); else close(true);
    });
    arrows.forEach(function (a) {
      a.addEventListener("click", function () {
        focused = addMonths(focused, +a.getAttribute("data-step"));
        render();
      });
    });
    todayButton.addEventListener("click", function () { choose(today); });
    tbody.addEventListener("click", function (e) {
      var b = e.target.closest("[data-date]");
      if (b) choose(parseIso(b.getAttribute("data-date")));
    });
    tbody.addEventListener("keydown", function (e) {
      if (!e.target.closest("[data-date]")) return;
      var weekPos = (focused.getUTCDay() - firstDay + 7) % 7;
      var next = {
        ArrowLeft: addDays(focused, -1),
        ArrowRight: addDays(focused, 1),
        ArrowUp: addDays(focused, -7),
        ArrowDown: addDays(focused, 7),
        Home: addDays(focused, -weekPos),
        End: addDays(focused, 6 - weekPos),
        PageUp: addMonths(focused, e.shiftKey ? -12 : -1),
        PageDown: addMonths(focused, e.shiftKey ? 12 : 1)
      }[e.key];
      if (!next) return;
      e.preventDefault();
      moveTo(next);
    });
    pop.addEventListener("keydown", function (e) {
      if (e.key === "Escape") {
        e.preventDefault();
        close(true);
      } else if (e.key === "Tab") {
        // Keep focus inside the open dialog.
        var items = [arrows[0], arrows[1], tbody.querySelector('[tabindex="0"]'), todayButton];
        var at = items.indexOf(document.activeElement);
        var to = (at + (e.shiftKey ? -1 : 1) + items.length) % items.length;
        e.preventDefault();
        items[to].focus();
      }
    });
    document.addEventListener("click", function (e) {
      if (!pop.hidden && !pop.contains(e.target) && !button.contains(e.target)) close(false);
    });
  }

  function setupDateField(input) {
    var lang = document.documentElement.lang || "en";
    var label = input.labels && input.labels[0];
    var name = label ? label.textContent.trim() : "";
    var shortFmt, longFmt;
    try {
      shortFmt = new Intl.DateTimeFormat(lang, { day: "numeric", month: "short", year: "numeric", timeZone: "UTC" });
      longFmt = new Intl.DateTimeFormat(lang, { day: "numeric", month: "long", year: "numeric", timeZone: "UTC" });
    } catch (e) { return; }
    var button = document.createElement("button");
    button.type = "button";
    button.className = "date-button";
    button.id = input.id + "-button";
    var describedBy = input.getAttribute("aria-describedby");
    if (describedBy) button.setAttribute("aria-describedby", describedBy);
    function show() {
      var m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(input.value);
      var d = m ? new Date(Date.UTC(+m[1], +m[2] - 1, +m[3])) : null;
      button.textContent = d ? shortFmt.format(d) : t("choose-date", "Choose a date");
      button.setAttribute("aria-label", (name ? name + ": " : "") + (d ? longFmt.format(d) : t("choose-date", "Choose a date")));
    }
    show();
    input.after(button);
    // Clicking the row's label opens the calendar, as it would focus the field.
    if (label) label.htmlFor = button.id;
    var now = new Date();
    var todayIso = now.getFullYear() + "-" + String(now.getMonth() + 1).padStart(2, "0") + "-" +
      String(now.getDate()).padStart(2, "0");
    setupCalendar(input, button, input.closest(".form-row") || input.parentNode, todayIso, function (d) {
      input.value = d;
      show();
      input.dispatchEvent(new Event("change", { bubbles: true }));
    });
  }

  // Tab switching: the selected tab always draws its own highlight. Only
  // while switching does a pill take its place, sliding from the previous
  // tab to the chosen one; it is removed once the slide ends. The previous
  // tab is remembered across the page load so the new page can finish it.
  var PILL_KEY = "korpus-tab-from";
  var PILL_MS = 380;

  function setupTabPill(bar) {
    var list = bar.querySelector("ul");
    var current = bar.querySelector('a[aria-current="page"]');
    if (!list || !current) return;
    var links = Array.prototype.slice.call(list.querySelectorAll("a"));
    var timer = null;

    function place(a) {
      var r = a.getBoundingClientRect();
      list.style.setProperty("--pill-x", (r.left - list.getBoundingClientRect().left - list.clientLeft) + "px");
      list.style.setProperty("--pill-w", r.width + "px");
    }
    function finish() {
      bar.classList.remove("has-pill", "pill-ready", "switching");
      links.forEach(function (l) { l.classList.remove("is-target"); });
    }
    function slide(from, to) {
      clearTimeout(timer);
      bar.classList.remove("pill-ready");
      place(from);
      bar.classList.add("has-pill");
      void list.offsetWidth; // commit the start position before the transition
      bar.classList.add("pill-ready");
      place(to);
      timer = setTimeout(function () { if (to === current) finish(); }, PILL_MS + 50);
    }

    var fromHref = null;
    try {
      fromHref = sessionStorage.getItem(PILL_KEY);
      sessionStorage.removeItem(PILL_KEY);
    } catch (e) { /* storage unavailable: no slide in */ }
    var from = links.filter(function (l) { return l.getAttribute("href") === fromHref; })[0];
    if (from && from !== current) slide(from, current);

    list.addEventListener("click", function (e) {
      var a = e.target.closest("a");
      if (!a || a === current || e.defaultPrevented || e.button !== 0 ||
          e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
      links.forEach(function (l) { l.classList.toggle("is-target", l === a); });
      bar.classList.add("switching");
      slide(current, a);
      try { sessionStorage.setItem(PILL_KEY, current.getAttribute("href")); } catch (err) { /* ignore */ }
    });

    // Back/forward cache restores this page as it was left: mid-switch.
    window.addEventListener("pageshow", function (e) { if (e.persisted) finish(); });
  }

  onReady(function () {
    var tabBar = document.querySelector(".tab-bar");
    if (tabBar) setupTabPill(tabBar);

    document.querySelectorAll("form[data-confirm]").forEach(function (f) {
      f.addEventListener("submit", function (e) {
        if (!window.confirm(f.getAttribute("data-confirm"))) e.preventDefault();
      });
    });

    // Preview the appearance as soon as it is picked; saving stores it.
    document.querySelectorAll("[data-theme-picker]").forEach(function (group) {
      group.addEventListener("change", function (e) {
        var theme = e.target && e.target.value;
        if (!theme) return;
        document.documentElement.setAttribute("data-theme", theme);
        var meta = document.querySelector('meta[name="color-scheme"]');
        if (meta) meta.setAttribute("content", theme === "auto" ? "light dark" : theme);
      });
    });

    document.querySelectorAll("input[data-autosubmit]").forEach(function (i) {
      i.addEventListener("change", function () { if (i.value) i.form.submit(); });
    });

    // The diary's date button jumps straight to the chosen day.
    document.querySelectorAll("[data-date-picker]").forEach(function (button) {
      var input = button.form && button.form.querySelector("input[type=date]");
      if (!input) return;
      setupCalendar(input, button, button.closest(".day-nav"), button.getAttribute("data-today"), function (d) {
        input.value = d;
        button.form.submit();
      });
    });

    // Date fields in forms become a capsule showing the date, as in iOS.
    document.querySelectorAll("input[data-calendar]").forEach(setupDateField);

    if (location.pathname === "/setup") {
      var tz = document.querySelector("input[data-detect-timezone]");
      try {
        var detected = Intl.DateTimeFormat().resolvedOptions().timeZone;
        if (tz && detected && tz.value === "UTC") tz.value = detected;
      } catch (e) { /* keep server default */ }
    }

    var results = document.querySelector("#results[data-src]");
    if (results) loadRemote(results);

    document.querySelectorAll("form[data-live-search]").forEach(setupLiveSearch);
    document.querySelectorAll("form[data-nutrients]").forEach(setupNutrientPreview);

    var summary = document.querySelector("[data-focus]");
    if (summary) summary.focus();
  });
})();
