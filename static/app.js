// Korpus progressive enhancement. Every feature here is optional: all pages
// work with plain links and forms when this script does not run.
(function () {
  "use strict";
  document.documentElement.classList.add("js");
  // The sport icon sprite, with this script's version so they refresh together.
  var SPORTS_SVG = "/static/sports.svg" + (document.currentScript ? new URL(document.currentScript.src).search : "");

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

  // When the session has ended (logged out on another device, expired),
  // fetches end on the login page. Go there instead of treating the login
  // page as a result, or a change as saved.
  var LOGIN_PATHS = ["/login", "/login/code", "/account/setup"];
  function checkLogin(r) {
    var path = r.redirected ? new URL(r.url).pathname : "";
    if (r.status === 401 || LOGIN_PATHS.indexOf(path) !== -1) {
      location.assign("/login?next=" + encodeURIComponent(location.pathname + location.search));
      throw new Error("login required");
    }
    return r;
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
      .then(checkLogin)
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
          .then(checkLogin)
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
    // An optional date (data-clearable) gets a button that empties it.
    var clear = null;
    if (input.hasAttribute("data-clearable")) {
      clear = document.createElement("button");
      clear.type = "button";
      clear.className = "date-clear";
      clear.setAttribute("aria-label", t("clear", "Clear") + (name ? ": " + name : ""));
      clear.innerHTML = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" ' +
        'stroke-linecap="round" aria-hidden="true" focusable="false"><path d="m7 7 10 10M17 7 7 17"/></svg>';
      clear.addEventListener("click", function () {
        input.value = "";
        show();
        input.dispatchEvent(new Event("change", { bubbles: true }));
        button.focus();
      });
    }
    function show() {
      var m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(input.value);
      var d = m ? new Date(Date.UTC(+m[1], +m[2] - 1, +m[3])) : null;
      button.textContent = d ? shortFmt.format(d) : t("choose-date", "Choose a date");
      button.setAttribute("aria-label", (name ? name + ": " : "") + (d ? longFmt.format(d) : t("choose-date", "Choose a date")));
      if (clear) clear.hidden = !d;
    }
    show();
    input.after(button);
    if (clear) button.after(clear);
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

  // Pop-up menus: each <select> becomes a button showing its value with up
  // and down chevrons that opens a floating menu, as in iOS. The select
  // stays in the form, hidden, and carries the value. The menu follows the
  // WAI-ARIA listbox pattern: focus stays on the list and the highlighted
  // option is announced through aria-activedescendant.
  var menuCount = 0;
  var MENU_MARGIN = 8;
  var MENU_MAX_HEIGHT = 420;

  function foldText(s) {
    return s.normalize("NFD").replace(/[̀-ͯ]/g, "").toLowerCase();
  }

  function setupMenu(select) {
    if (select.hasAttribute("data-menu") || select.multiple) return;
    select.setAttribute("data-menu", "");
    var id = select.id || "menu-select-" + (menuCount + 1);
    var menuId = "menu-" + ++menuCount;
    var label = select.labels && select.labels[0];
    var check = '<svg class="check" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" ' +
      'stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false"><path d="m4.5 12.5 5 5 10-11"/></svg>';

    var button = document.createElement("button");
    button.type = "button";
    button.className = "menu-button";
    button.id = id + "-button";
    button.setAttribute("aria-haspopup", "listbox");
    button.setAttribute("aria-expanded", "false");
    button.setAttribute("aria-controls", menuId);
    button.innerHTML = '<span class="menu-value"></span><svg class="menu-chevrons" viewBox="0 0 10 16" fill="none" ' +
      'stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" ' +
      'focusable="false"><path d="m1.5 5.5 3.5-3.5 3.5 3.5M1.5 10.5l3.5 3.5 3.5-3.5"/></svg>';
    var value = button.querySelector(".menu-value");
    ["aria-describedby", "aria-invalid"].forEach(function (a) {
      if (select.hasAttribute(a)) button.setAttribute(a, select.getAttribute(a));
    });
    if (label) {
      if (!label.id) label.id = id + "-label";
      // The name is the label followed by the current value.
      button.setAttribute("aria-labelledby", label.id + " " + button.id);
      label.htmlFor = button.id;
    }

    var list = document.createElement("ul");
    list.className = "menu";
    list.id = menuId;
    list.hidden = true;
    list.tabIndex = -1;
    list.setAttribute("role", "listbox");
    if (label) list.setAttribute("aria-labelledby", label.id);
    // An option's data-icon names a symbol of the sport icon sprite.
    function icon(o) {
      var name = o && o.getAttribute("data-icon");
      return name ? '<svg class="sport-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" ' +
        'stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false">' +
        '<use href="' + SPORTS_SVG + '#' + encodeURIComponent(name) + '"/></svg>' : "";
    }
    var options = Array.prototype.slice.call(select.options).filter(function (o) { return !o.disabled; });
    var items = options.map(function (o, i) {
      var li = document.createElement("li");
      li.id = menuId + "-" + i;
      li.setAttribute("role", "option");
      if (o.lang) li.lang = o.lang;
      li.innerHTML = check + icon(o) + '<span class="menu-text"></span>';
      li.querySelector(".menu-text").textContent = o.textContent;
      list.appendChild(li);
      return li;
    });
    var active = -1;

    function selectedIndex() {
      for (var i = 0; i < options.length; i++) if (options[i].selected) return i;
      return 0;
    }
    function show() {
      var o = options[selectedIndex()];
      value.innerHTML = icon(o) + "<span></span>";
      value.lastChild.textContent = o ? o.textContent : "";
      if (o && o.lang) value.lang = o.lang; else value.removeAttribute("lang");
    }
    function setActive(i, scroll) {
      if (!items.length) return;
      active = Math.max(0, Math.min(items.length - 1, i));
      items.forEach(function (li, k) { li.classList.toggle("active", k === active); });
      list.setAttribute("aria-activedescendant", items[active].id);
      if (scroll) items[active].scrollIntoView({ block: "nearest" });
    }
    // Below the button when it fits, otherwise wherever there is more room;
    // right edges aligned, as values sit on the right of their rows.
    function place() {
      var r = button.getBoundingClientRect();
      var vw = document.documentElement.clientWidth, vh = window.innerHeight;
      // Stay clear of the floating tab bar.
      var bar = document.querySelector(".tab-bar");
      if (bar && getComputedStyle(bar).position === "fixed") vh = Math.min(vh, bar.getBoundingClientRect().top);
      var below = vh - r.bottom - MENU_MARGIN * 2, above = r.top - MENU_MARGIN * 2;
      var natural = Math.min(list.scrollHeight, MENU_MAX_HEIGHT);
      var down = natural <= below || below >= above;
      list.style.maxHeight = Math.min(MENU_MAX_HEIGHT, down ? below : above) + "px";
      var h = list.offsetHeight, w = list.offsetWidth;
      var left = Math.max(MENU_MARGIN, Math.min(r.right - w, vw - w - MENU_MARGIN));
      list.style.left = left + "px";
      list.style.top = (down ? r.bottom + MENU_MARGIN : r.top - MENU_MARGIN - h) + "px";
      list.classList.toggle("up", !down);
    }
    function follow(e) { if (e.target !== list) place(); }
    function open() {
      if (!list.hidden) return;
      items.forEach(function (li, k) { li.setAttribute("aria-selected", String(options[k].selected)); });
      list.hidden = false;
      button.setAttribute("aria-expanded", "true");
      place();
      setActive(selectedIndex(), true);
      list.focus({ preventScroll: true });
      window.addEventListener("resize", close);
      window.addEventListener("scroll", follow, true);
    }
    function close(restoreFocus) {
      if (list.hidden) return;
      list.hidden = true;
      list.classList.remove("keyboard");
      button.setAttribute("aria-expanded", "false");
      window.removeEventListener("resize", close);
      window.removeEventListener("scroll", follow, true);
      if (restoreFocus === true) button.focus();
    }
    function choose(i) {
      var o = options[i];
      close(true);
      if (!o || o.selected) return;
      select.value = o.value;
      show();
      select.dispatchEvent(new Event("change", { bubbles: true }));
    }

    var typed = "", typedAt = 0;
    function typeahead(ch) {
      var now = Date.now();
      typed = now - typedAt > 600 ? ch : typed + ch;
      typedAt = now;
      var q = foldText(typed);
      // Repeating one letter cycles through the options starting with it.
      var cycle = q.length > 1 && q.split("").every(function (c) { return c === q[0]; });
      if (cycle) q = q[0];
      var start = cycle || q.length === 1 ? active + 1 : active;
      for (var n = 0; n < items.length; n++) {
        var k = (start + n) % items.length;
        if (foldText(options[k].textContent).indexOf(q) === 0) { setActive(k, true); return; }
      }
    }

    show();
    select.hidden = true;
    select.after(button);
    document.body.appendChild(list);

    button.addEventListener("click", function () {
      if (list.hidden) open(); else close(true);
    });
    button.addEventListener("keydown", function (e) {
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        open();
        list.classList.add("keyboard");
      }
    });
    list.addEventListener("keydown", function (e) {
      var page = Math.max(1, Math.floor(list.clientHeight / 44) - 1);
      var to = {
        ArrowDown: active + 1, ArrowUp: active - 1, Home: 0, End: items.length - 1,
        PageDown: active + page, PageUp: active - page
      }[e.key];
      if (to !== undefined) {
        e.preventDefault();
        list.classList.add("keyboard");
        setActive(to, true);
      } else if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        if (e.key === " " && typed && Date.now() - typedAt < 600) { typeahead(" "); return; }
        choose(active);
      } else if (e.key === "Escape" || e.key === "Tab") {
        e.preventDefault();
        close(true);
      } else if (e.key.length === 1 && !e.ctrlKey && !e.metaKey && !e.altKey) {
        list.classList.add("keyboard");
        typeahead(e.key);
      }
    });
    list.addEventListener("pointermove", function (e) {
      var li = e.target.closest("[role=option]");
      if (!li) return;
      list.classList.remove("keyboard");
      var k = items.indexOf(li);
      if (k !== active) setActive(k, false);
    });
    list.addEventListener("click", function (e) {
      var li = e.target.closest("[role=option]");
      if (li) choose(items.indexOf(li));
    });
    list.addEventListener("focusout", function (e) {
      if (!list.contains(e.relatedTarget) && e.relatedTarget !== button) close(false);
    });
    document.addEventListener("pointerdown", function (e) {
      if (!list.hidden && !list.contains(e.target) && !button.contains(e.target)) close(false);
    });
    // A section re-rendered by autosave takes the select (and its button)
    // out of the page; its menu goes with it.
    new MutationObserver(function (_, observer) {
      if (!select.isConnected) { close(false); list.remove(); observer.disconnect(); }
    }).observe(document.body, { childList: true, subtree: true });
  }

  // File fields: the browser's own button is unstyled and speaks the
  // browser's language, so a row shows the chosen file's name with a tinted
  // button instead. The input stays in the form and receives the files.
  function setupFilePicker(input) {
    var label = input.labels && input.labels[0];
    var row = document.createElement("div");
    row.className = "file-pick";
    var name = document.createElement("span");
    name.className = "file-name";
    name.id = input.id + "-name";
    var button = document.createElement("button");
    button.type = "button";
    button.className = "pill";
    button.id = input.id + "-button";
    button.textContent = t("choose-file", "Choose file");
    ["aria-describedby", "aria-invalid"].forEach(function (a) {
      if (input.hasAttribute(a)) button.setAttribute(a, input.getAttribute(a));
    });
    if (label) {
      if (!label.id) label.id = input.id + "-label";
      button.setAttribute("aria-labelledby", label.id + " " + button.id + " " + name.id);
      label.htmlFor = button.id;
    }
    function show() {
      var files = input.files || [];
      name.textContent = files.length === 1 ? files[0].name
        : files.length > 1 ? t("files", "{n} files").replace("{n}", files.length)
        : t("no-file", "No file chosen");
      row.classList.toggle("has-file", files.length > 0);
      if (files.length) row.classList.remove("invalid");
    }
    input.classList.add("visually-hidden");
    input.tabIndex = -1;
    input.after(row);
    row.appendChild(name);
    row.appendChild(button);
    button.addEventListener("click", function () { input.click(); });
    input.addEventListener("change", show);
    // A required field left empty is reported on the visible button.
    input.addEventListener("invalid", function () {
      row.classList.add("invalid");
      button.focus();
    });
    show();
  }

  // Tab switching: the selected tab always draws its own highlight. Only
  // while switching does a pill take its place, sliding from the previous
  // tab to the chosen one; it is removed once the slide ends. The previous
  // tab is remembered across the page load so the new page can finish it.
  var PILL_KEY = "korpus-tab-from";
  var PILL_MS = 380;

  // The exercise list filters as you type, like the server does: every
  // typed word must start a word of one of the exercise's names (in any
  // language). The muscle chips keep what was typed.
  function setupExerciseFilter(form) {
    var input = form.querySelector("input[name=q]");
    var rows = document.querySelectorAll("#exercise-list > li");
    var none = document.getElementById("exercise-none");
    var chips = document.querySelectorAll(".chips a");
    if (!input) return;
    function fold(s) {
      return foldText(s).replace(/œ/g, "oe").replace(/æ/g, "ae").replace(/ß/g, "ss")
        .replace(/[^\p{L}\p{N}]+/gu, " ").trim();
    }
    function apply() {
      var words = fold(input.value).split(" ").filter(Boolean);
      var shown = 0;
      rows.forEach(function (li) {
        var text = (li.getAttribute("data-search") || "").split(" ");
        var ok = words.every(function (w) {
          return text.some(function (t) { return t.indexOf(w) === 0 || (w.length > 3 && t.indexOf(w) >= 0); });
        });
        li.hidden = !ok;
        if (ok) shown++;
      });
      if (none) none.hidden = shown > 0;
      chips.forEach(function (a) {
        var url = new URL(a.href, location.href);
        if (input.value.trim()) url.searchParams.set("q", input.value.trim());
        else url.searchParams.delete("q");
        a.href = url.pathname + url.search;
      });
    }
    input.addEventListener("input", apply);
    form.addEventListener("submit", function (e) { e.preventDefault(); apply(); });
  }

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

  // Settings apply as soon as they change. The form is posted in the
  // background; the server redirects to the refreshed page on success or
  // answers 422 with the errors, and the parts of the page that depend on
  // the saved values are swapped in. A language change reloads the page so
  // every label is in the new language.
  var AUTOSAVE_DELAY = 400;

  function sectionOf(el) { return el.closest("section[aria-labelledby]"); }

  function freshSection(doc, section) {
    return doc.querySelector('section[aria-labelledby="' + section.getAttribute("aria-labelledby") + '"]');
  }

  // `savedLanguage` carries the stored language over when a form is
  // re-rendered with errors, as it may show an unsaved choice.
  function setupAutosave(form, savedLanguage) {
    var timer = null;
    var seq = 0;
    var language = form.querySelector('[name="language"]');
    if (savedLanguage === undefined) savedLanguage = language ? language.value : null;

    function showStatus(f, text, isError) {
      var status = f.querySelector("[data-autosave-status]");
      if (!status || !text) return;
      var p = document.createElement("p");
      p.className = isError ? "warning" : "flash";
      p.textContent = text;
      status.replaceChildren(p);
    }

    // Replaces the form's section with its refreshed version and returns
    // the new form.
    function swap(doc, focusErrors) {
      var section = sectionOf(form);
      var fresh = section && freshSection(doc, section);
      if (!fresh) return form;
      var activeId = document.activeElement && document.activeElement.id;
      section.replaceWith(fresh);
      fresh.querySelectorAll("select").forEach(setupMenu);
      var f = fresh.querySelector("form[data-autosave]");
      if (f) setupAutosave(f, savedLanguage);
      var summary = focusErrors && fresh.querySelector(".error-summary");
      var target = summary || (activeId && document.getElementById(activeId));
      if (target) target.focus();
      return f || form;
    }

    function save() {
      var mine = ++seq;
      fetch(form.action, {
        method: "POST",
        body: new URLSearchParams(new FormData(form)),
        credentials: "same-origin"
      })
        .then(checkLogin)
        .then(function (r) {
          if (!r.ok && r.status !== 422) throw new Error("HTTP " + r.status);
          return r.text().then(function (html) { return { ok: r.ok, html: html }; });
        })
        .then(function (res) {
          if (mine !== seq || !form.isConnected) return; // a newer change is being saved
          var doc = new DOMParser().parseFromString(res.html, "text/html");
          if (!res.ok) {
            swap(doc, true);
            return;
          }
          if (language && language.value !== savedLanguage) {
            location.replace(form.action + "?msg=settings_saved");
            return;
          }
          var f = form;
          // Clear errors left from an earlier attempt.
          if (sectionOf(form) && sectionOf(form).querySelector(".error-summary, .field-error")) f = swap(doc, false);
          // The country decides the order of the food composition tables.
          var tables = document.querySelector('section[aria-labelledby="datasets-heading"]');
          if (tables && form.querySelector('[name="country"]')) {
            var freshTables = freshSection(doc, tables);
            if (freshTables) tables.replaceWith(freshTables);
          }
          showStatus(f, f.getAttribute("data-saved"), false);
        })
        .catch(function () {
          if (mine === seq && form.isConnected) {
            showStatus(form, t("save-error", "Could not save. Check your connection and try again."), true);
          }
        });
    }

    function schedule(delay) {
      clearTimeout(timer);
      timer = setTimeout(save, delay);
    }

    // Text fields save when left or on Enter; choices save once picked
    // (debounced, as arrow keys on a closed list change it step by step).
    form.addEventListener("change", function (e) {
      schedule(e.target.type === "radio" ? 0 : AUTOSAVE_DELAY);
    });
    form.addEventListener("submit", function (e) {
      e.preventDefault();
      schedule(0);
    });
  }

  // A workout in progress: values save as they are entered, so nothing is
  // lost when the page is left, and ticking a set starts the rest timer of
  // its exercise. The form's buttons still post it normally.
  function setupWorkout(form) {
    var timer = null;
    var seq = 0;
    var status = form.querySelector("[data-autosave-status]");

    function showError(text) {
      if (!status) return;
      var p = document.createElement("p");
      p.className = "warning";
      p.textContent = text;
      status.replaceChildren(p);
    }

    function save(beacon) {
      clearTimeout(timer);
      timer = null;
      var body = new URLSearchParams(new FormData(form));
      body.set("action", "save");
      var mine = ++seq;
      // The buttons are named "action", which hides form.action.
      fetch(form.getAttribute("action"), {
        method: "POST",
        body: body,
        credentials: "same-origin",
        keepalive: !!beacon,
        headers: { "X-Korpus-Autosave": "1" }
      })
        .then(checkLogin)
        .then(function (r) {
          if (mine !== seq) return;
          if (r.status === 422) showError(form.getAttribute("data-invalid"));
          else if (!r.ok) throw new Error("HTTP " + r.status);
          else if (status) status.replaceChildren();
        })
        .catch(function () {
          if (mine === seq) showError(t("save-error", "Could not save. Check your connection and try again."));
        });
    }

    form.addEventListener("input", function () {
      clearTimeout(timer);
      timer = setTimeout(save, 800);
    });
    form.addEventListener("change", function (e) {
      if (!e.target.classList.contains("set-done")) return;
      var row = e.target.closest("tr");
      if (row) row.classList.toggle("is-done", e.target.checked);
      save();
      var section = e.target.closest("[data-rest]");
      var rest = section ? parseInt(section.getAttribute("data-rest"), 10) : 0;
      if (e.target.checked && rest > 0) startRest(rest);
    });
    // A button posts the whole form, which saves it too.
    form.addEventListener("submit", function () { clearTimeout(timer); timer = null; seq++; });
    // Leaving with a change not yet saved (switching tab, locking the phone).
    document.addEventListener("visibilitychange", function () {
      if (document.visibilityState === "hidden" && timer) save(true);
    });

    var bar = document.querySelector("[data-rest-timer]");
    var left = bar && bar.querySelector("[data-rest-left]");
    var announce = document.querySelector("[data-rest-announce]");
    var end = 0;
    var tick = null;
    function fmt(s) { return Math.floor(s / 60) + ":" + String(s % 60).padStart(2, "0"); }
    function stopRest() {
      clearInterval(tick);
      tick = null;
      if (bar) bar.hidden = true;
    }
    function update() {
      var s = Math.max(0, Math.ceil((end - Date.now()) / 1000));
      left.textContent = fmt(s);
      if (s > 0) return;
      clearInterval(tick);
      tick = null;
      left.textContent = bar.getAttribute("data-done");
      if (announce) announce.textContent = bar.getAttribute("data-done");
      if (navigator.vibrate) navigator.vibrate(200);
      setTimeout(function () { if (!tick) bar.hidden = true; }, 4000);
    }
    function startRest(seconds) {
      if (!bar || !left) return;
      end = Date.now() + seconds * 1000;
      if (announce) announce.textContent = "";
      bar.hidden = false;
      clearInterval(tick);
      tick = setInterval(update, 250);
      update();
    }
    if (bar) {
      bar.querySelector("[data-rest-skip]").addEventListener("click", stopRest);
      bar.querySelector("[data-rest-add]").addEventListener("click", function () {
        end = Math.max(end, Date.now()) + 15000;
        if (!tick) tick = setInterval(update, 250);
        update();
      });
    }
  }

  // Destructive form posts are confirmed in an alert styled like iOS:
  // data-confirm is the title, data-confirm-detail an optional message and
  // data-confirm-action the destructive button's label.
  var confirmAlert = null;

  function buildConfirm() {
    var d = document.createElement("dialog");
    d.className = "alert";
    d.setAttribute("role", "alertdialog");
    d.setAttribute("aria-labelledby", "alert-title");
    d.setAttribute("aria-describedby", "alert-detail");
    d.innerHTML =
      '<div class="alert-text"><h2 id="alert-title"></h2><p id="alert-detail"></p></div>' +
      '<div class="alert-actions"><button type="button" class="alert-cancel"></button>' +
      '<button type="button" class="alert-confirm"></button></div>';
    d.querySelector(".alert-cancel").textContent = t("cancel", "Cancel");
    d.querySelector(".alert-cancel").addEventListener("click", function () { d.close(); });
    d.querySelector(".alert-confirm").addEventListener("click", function () {
      var form = d.form;
      d.close();
      // submit() skips the submit event, so the alert does not open again.
      if (form && form.isConnected) form.submit();
    });
    d.addEventListener("close", function () {
      if (d.opener && d.opener.isConnected) d.opener.focus();
      d.form = null;
      d.opener = null;
    });
    document.body.appendChild(d);
    return d;
  }

  function showConfirm(form) {
    var d = confirmAlert || (confirmAlert = buildConfirm());
    var detail = form.getAttribute("data-confirm-detail") || "";
    d.querySelector("#alert-title").textContent = form.getAttribute("data-confirm");
    var p = d.querySelector("#alert-detail");
    p.textContent = detail;
    p.hidden = !detail;
    d.querySelector(".alert-confirm").textContent =
      form.getAttribute("data-confirm-action") || t("delete", "Delete");
    d.form = form;
    d.opener = document.activeElement;
    d.showModal();
    // The safe choice has focus, as on iOS.
    d.querySelector(".alert-cancel").focus();
  }

  // Adding a food opened from Foods asks for the meal in an action sheet
  // styled like iOS, built from the meal menu's options. Without the script
  // (or <dialog>) the menu stays on the page, preset from the time of day.
  function setupMealSheet(form) {
    var select = form.querySelector('select[name="meal"]');
    if (!select || !window.HTMLDialogElement) return;
    var row = select.closest(".form-row");
    if (row) row.hidden = true;

    var d = document.createElement("dialog");
    d.className = "sheet";
    d.setAttribute("aria-labelledby", "meal-sheet-title");
    var group = document.createElement("div");
    group.className = "sheet-group";
    var title = document.createElement("h2");
    title.id = "meal-sheet-title";
    title.className = "sheet-title";
    title.textContent = form.getAttribute("data-ask-meal");
    group.appendChild(title);
    Array.prototype.forEach.call(select.options, function (o) {
      var b = document.createElement("button");
      b.type = "button";
      b.value = o.value;
      b.textContent = o.textContent;
      b.addEventListener("click", function () {
        select.value = o.value;
        d.close();
        // submit() skips the submit event, so the sheet does not open again.
        form.submit();
      });
      group.appendChild(b);
    });
    var cancel = document.createElement("button");
    cancel.type = "button";
    cancel.className = "sheet-cancel";
    cancel.textContent = t("cancel", "Cancel");
    cancel.addEventListener("click", function () { d.close(); });
    d.appendChild(group);
    d.appendChild(cancel);
    // A tap outside the sheet dismisses it, as on iOS.
    d.addEventListener("click", function (e) {
      var r = d.getBoundingClientRect();
      if (e.clientX < r.left || e.clientX > r.right || e.clientY < r.top || e.clientY > r.bottom) d.close();
    });
    d.addEventListener("close", function () {
      if (d.opener && d.opener.isConnected) d.opener.focus();
      d.opener = null;
    });
    document.body.appendChild(d);

    // Runs after the browser has checked the amount.
    form.addEventListener("submit", function (e) {
      e.preventDefault();
      d.opener = document.activeElement;
      d.showModal();
      // The meal suggested by the time of day has focus, so Enter picks it.
      var suggested = group.querySelector('button[value="' + select.value + '"]');
      (suggested || group.querySelector("button")).focus();
    });
  }

  onReady(function () {
    var tabBar = document.querySelector(".tab-bar");
    if (tabBar) setupTabPill(tabBar);

    // Delegated so forms inserted later (live search results) are covered.
    document.addEventListener("submit", function (e) {
      var f = e.target;
      if (!(f instanceof HTMLFormElement) || !f.hasAttribute("data-confirm")) return;
      if (!window.HTMLDialogElement) return; // submits unconfirmed, as without the script
      e.preventDefault();
      showConfirm(f);
    });

    document.querySelectorAll("form select").forEach(setupMenu);
    document.querySelectorAll("form[data-ask-meal]").forEach(setupMealSheet);
    document.querySelectorAll('input[type="file"][id]').forEach(setupFilePicker);
    // Error summary links point at the field; a select's menu button
    // stands in for it.
    document.addEventListener("click", function (e) {
      var a = e.target.closest && e.target.closest('a[href^="#"]');
      var field = a && document.getElementById(a.getAttribute("href").slice(1));
      var button = field && field.hasAttribute("data-menu") && document.getElementById(field.id + "-button");
      if (!button) return;
      e.preventDefault();
      button.focus();
    });
    document.querySelectorAll("form[data-autosave]").forEach(function (f) { setupAutosave(f); });
    document.querySelectorAll("form[data-workout]").forEach(setupWorkout);

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

    // Fields that only apply to a repeating activity show while
    // "Repeat every week" is on.
    document.querySelectorAll("input[data-repeat-toggle]").forEach(function (toggle) {
      var rows = toggle.form ? toggle.form.querySelectorAll("[data-repeat-only]") : [];
      function sync() { rows.forEach(function (r) { r.hidden = !toggle.checked; }); }
      toggle.addEventListener("change", sync);
      sync();
    });

    // The name and intensity fields only apply to a custom sport.
    document.querySelectorAll("select[data-sport-menu]").forEach(function (menu) {
      var rows = menu.form ? menu.form.querySelectorAll("[data-custom-only]") : [];
      function sync() { rows.forEach(function (r) { r.hidden = menu.value !== "custom"; }); }
      menu.addEventListener("change", sync);
      sync();
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
    document.querySelectorAll("form[data-exercise-filter]").forEach(setupExerciseFilter);

    var summary = document.querySelector("[data-focus]");
    if (summary) summary.focus();
  });
})();
