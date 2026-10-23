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

  onReady(function () {
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
