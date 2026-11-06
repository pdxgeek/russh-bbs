/* russh-bbs project page: a few small touches, no dependencies, no network.
   Everything works without this file; it only adds the theme toggle, the style tabs,
   the copy button and the typed demo. With prefers-reduced-motion the demo is a still. */
(function () {
  "use strict";
  var doc = document;
  var root = doc.documentElement;
  var reduce = window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)");

  function store(key, value) {
    try {
      if (value === undefined) return window.localStorage.getItem(key);
      if (value === null) window.localStorage.removeItem(key); else window.localStorage.setItem(key, value);
    } catch (e) { /* private window or blocked storage: fine */ }
    return null;
  }

  /* ---- theme: auto -> light -> dark ---- */
  var themeBtn = doc.getElementById("theme");
  function applyTheme(t) {
    if (t === "light" || t === "dark") root.setAttribute("data-theme", t); else root.removeAttribute("data-theme");
    if (themeBtn) themeBtn.textContent = "theme: " + (t === "light" || t === "dark" ? t : "auto");
  }
  var theme = store("theme");
  applyTheme(theme);
  if (themeBtn) {
    themeBtn.hidden = false;
    themeBtn.addEventListener("click", function () {
      var cur = root.getAttribute("data-theme");
      var next = cur === null ? "light" : cur === "light" ? "dark" : null;
      store("theme", next);
      applyTheme(next);
    });
  }

  /* ---- copy buttons ---- */
  var copies = doc.querySelectorAll("[data-copy]");
  Array.prototype.forEach.call(copies, function (btn) {
    btn.hidden = false;
    btn.addEventListener("click", function () {
      var text = btn.getAttribute("data-copy");
      var done = function () { var old = btn.textContent; btn.textContent = "copied"; setTimeout(function () { btn.textContent = old; }, 1400); };
      if (navigator.clipboard && navigator.clipboard.writeText) navigator.clipboard.writeText(text).then(done, function () {});
    });
  });

  /* ---- style switcher ---- */
  var tabs = doc.querySelectorAll("#style-tabs [role=tab]");
  var figs = doc.querySelectorAll(".style-fig");
  function pick(i, focus) {
    Array.prototype.forEach.call(tabs, function (t, j) {
      var on = i === j;
      t.setAttribute("aria-selected", on ? "true" : "false");
      t.tabIndex = on ? 0 : -1;
      if (on && focus) t.focus();
    });
    Array.prototype.forEach.call(figs, function (f, j) { f.classList.toggle("on", i === j); });
  }
  Array.prototype.forEach.call(tabs, function (t, i) {
    t.addEventListener("click", function () { pick(i); });
    t.addEventListener("keydown", function (e) {
      var n = tabs.length, k = e.key;
      if (k === "ArrowRight" || k === "ArrowDown") { e.preventDefault(); pick((i + 1) % n, true); }
      else if (k === "ArrowLeft" || k === "ArrowUp") { e.preventDefault(); pick((i + n - 1) % n, true); }
      else if (k === "Home") { e.preventDefault(); pick(0, true); }
      else if (k === "End") { e.preventDefault(); pick(n - 1, true); }
    });
  });
  if (tabs.length) pick(0);

  /* ---- the typed demo: real captured screens, looped ---- */
  var demo = doc.getElementById("demo");
  if (!demo) return;
  var cmdEl = doc.getElementById("demo-cmd");
  var capEl = doc.getElementById("demo-cap");
  var pauseBtn = doc.getElementById("demo-pause");
  var imgs = {};
  Array.prototype.forEach.call(demo.querySelectorAll("img[data-step]"), function (im) { imgs[im.getAttribute("data-step")] = im; });

  var steps = [
    { type: "$ ssh -p 2222 new@board.example", show: "waiting", hold: 2600, cap: "A key the board does not know gets the waiting room and nothing else." },
    { type: "$ ssh -p 2222 you@board.example", show: "login", hold: 1700, cap: "A member signs in with a key or a password. This is the login screen." },
    { type: "(any key)", show: "menu", hold: 3200, cap: "The main menu, in the Phosphor style on the dashboard layout." },
    { type: "(press d)", show: "doors", hold: 2400, cap: "Doors: the games that ship with the board." },
    { type: "(press 5)", show: "longhaul", hold: 3400, cap: "Long Haul, a space-trade door, drawn in the same terminal." }
  ];

  function show(name) {
    Object.keys(imgs).forEach(function (k) { imgs[k].classList.toggle("on", k === name); });
  }
  function still() {
    show("menu");
    cmdEl.innerHTML = '<span class="p">$</span> ssh -p 2222 you@board.example';
    capEl.textContent = "The main menu, in the Phosphor style on the dashboard layout. Real capture from a real ssh session.";
  }

  var running = true, token = 0;
  function sleep(ms) { return new Promise(function (r) { setTimeout(r, ms); }); }
  function esc(s) { return s.replace(/&/g, "&amp;").replace(/</g, "&lt;"); }

  async function play(my) {
    while (my === token) {
      for (var i = 0; i < steps.length; i++) {
        var s = steps[i];
        if (my !== token) return;
        var typed = "";
        for (var c = 0; c < s.type.length; c++) {
          if (my !== token) return;
          typed += s.type[c];
          cmdEl.innerHTML = esc(typed) + '<span class="caret"></span>';
          await sleep(28 + Math.random() * 40);
        }
        await sleep(350);
        if (my !== token) return;
        show(s.show);
        capEl.textContent = s.cap;
        await sleep(s.hold);
      }
    }
  }
  function start() { token++; running = true; if (pauseBtn) pauseBtn.textContent = "pause demo"; play(token); }
  function stop() { token++; running = false; if (pauseBtn) pauseBtn.textContent = "play demo"; still(); }

  function motionOK() { return !(reduce && reduce.matches); }
  if (pauseBtn) {
    if (motionOK()) pauseBtn.hidden = false;
    pauseBtn.addEventListener("click", function () { if (running) stop(); else start(); });
  }
  if (motionOK()) start(); else still();
  if (reduce && reduce.addEventListener) {
    reduce.addEventListener("change", function () { if (motionOK()) { if (pauseBtn) pauseBtn.hidden = false; start(); } else { if (pauseBtn) pauseBtn.hidden = true; stop(); } });
  }
  doc.addEventListener("visibilitychange", function () { if (doc.hidden && running) { token++; } else if (!doc.hidden && running && motionOK()) { token++; play(token); } });
})();
