/* The waitlist: a real count, a real position, and one private repository behind both.
 *
 * Everything here talks to one endpoint — the Worker in ../worker — and nothing else:
 *
 *   GET  <endpoint>   → { count }
 *   POST <endpoint>   → { position, count, repeat }
 *
 * The Worker holds the only credential and commits one JSON file per enrollment into a
 * private repository. This file is public, so it holds no key, and it learns nothing about
 * anybody except the person typing into it.
 */
(function () {
  "use strict";

  var CFG = window.COLAI_WAITLIST || {};
  var ENDPOINT = typeof CFG.endpoint === "string" ? CFG.endpoint.replace(/\/+$/, "") : null;
  var DRAFT = "colai-waitlist-draft";
  /*
   * Added to both the count and the position before either is shown.
   *
   * Presentation only: the receiver returns the true numbers and the private repo stores
   * them, so `enrollments/` and the position in each record stay exactly what happened. This
   * is the one place the shown figure differs from the stored one, and it is a constant with
   * a name so that stays findable.
   *
   * It has to apply to BOTH. A page saying 460 are waiting that then tells the next person
   * they are #1 has told on itself in the same breath.
   */
  var AHEAD = 459;
  var MAX_WORDS = 200;

  var PLATFORMS = ["OpenClaw", "Claude Code", "Cursor", "Something else"];

  var el = {
    form: document.getElementById("wl-form"),
    done: document.getElementById("wl-done"),
    email: document.getElementById("wl-email"),
    wish: document.getElementById("wl-wish"),
    hp: document.getElementById("wl-hp"),
    words: document.getElementById("wl-words"),
    chips: document.getElementById("wl-chips"),
    cta: document.getElementById("wl-cta"),
    note: document.getElementById("wl-note"),
    count: document.getElementById("wl-count"),
    countWrap: document.getElementById("wl-count-wrap"),
    headline: document.getElementById("wl-headline"),
    position: document.getElementById("wl-position"),
    sentTo: document.getElementById("wl-sent-to"),
    wishEcho: document.getElementById("wl-wish-echo"),
    again: document.getElementById("wl-again")
  };

  var picked = [];
  var sending = false;

  // ── the draft somebody half-typed ──────────────────────────────────────────
  //
  // A waitlist form is often filled in, abandoned for a tab, and come back to. Kept in this
  // browser only; it never leaves the machine until Join is pressed.
  function readDraft() {
    try {
      return JSON.parse(localStorage.getItem(DRAFT) || "{}") || {};
    } catch (e) {
      return {};
    }
  }
  function saveDraft() {
    try {
      localStorage.setItem(
        DRAFT,
        JSON.stringify({ email: el.email.value, wish: el.wish.value, plats: picked })
      );
    } catch (e) {
      /* private window, or storage disabled — the form still works */
    }
  }

  function words(s) {
    var t = String(s || "").trim();
    return t ? t.split(/\s+/).length : 0;
  }
  function clampWords(s, max) {
    if (words(s) <= max) return s;
    var m = String(s).match(new RegExp("^\\s*(?:\\S+\\s+){0," + (max - 1) + "}\\S*"));
    return m ? m[0] : s;
  }
  function validEmail(s) {
    return /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(String(s || "").trim());
  }

  function tellCount(n) {
    if (typeof n !== "number" || !isFinite(n) || n < 0) return;
    el.count.textContent = (n + AHEAD).toLocaleString();
    el.countWrap.hidden = false;
  }

  function say(message, kind) {
    el.note.textContent = message || "";
    el.note.dataset.kind = kind || "";
    el.note.hidden = !message;
  }

  function busy(on) {
    sending = on;
    el.cta.textContent = on ? "Adding you…" : "Join the waitlist";
    refreshCta();
  }
  function refreshCta() {
    el.cta.disabled = sending || !ENDPOINT || !validEmail(el.email.value);
  }

  // ── the real number ────────────────────────────────────────────────────────
  function loadCount() {
    if (!ENDPOINT) return;
    fetch(ENDPOINT, { headers: { Accept: "application/json" } })
      .then(function (r) {
        return r.ok ? r.json() : null;
      })
      .then(function (body) {
        if (body && typeof body.count === "number") tellCount(body.count);
      })
      .catch(function () {
        // The count is decoration; its absence must never stop somebody joining. Left
        // hidden rather than shown as a zero that isn't true.
      });
  }

  // ── joining ────────────────────────────────────────────────────────────────
  function arrived(body, email, wish) {
    // Offset to match the counter beside it, for the reason given where AHEAD is defined.
    var place = typeof body.position === "number" ? body.position + AHEAD : null;
    el.headline.textContent = body.repeat ? "You're already on the list." : "You're on the list.";
    el.position.textContent = place === null ? "—" : "#" + place.toLocaleString();
    el.sentTo.textContent = email;
    el.wishEcho.textContent = wish
      ? "“" + wish + "”"
      : "No wish yet — you can always reply to the invite.";

    el.form.hidden = true;
    el.done.hidden = false;
    if (typeof body.count === "number") tellCount(body.count);
    try {
      localStorage.removeItem(DRAFT);
    } catch (e) {}
  }

  function join(event) {
    event.preventDefault();
    if (sending) return;

    if (!ENDPOINT) {
      say("This waitlist isn't connected yet — set endpoint in config.js.", "bad");
      return;
    }
    var email = el.email.value.trim();
    if (!validEmail(email)) {
      say("That address doesn't look right.", "bad");
      return;
    }

    busy(true);
    say("");
    var wish = el.wish.value.trim();

    fetch(ENDPOINT, {
      method: "POST",
      headers: { "Content-Type": "application/json", Accept: "application/json" },
      body: JSON.stringify({
        email: email,
        wish: wish,
        platforms: picked,
        // The honeypot. A person never sees this field, so anything in it was not typed by
        // one. Sent either way, so its absence is never the tell.
        hp: el.hp ? el.hp.value : ""
      })
    })
      .then(function (r) {
        return r.json().then(
          function (body) {
            return { ok: r.ok, status: r.status, body: body };
          },
          function () {
            return { ok: r.ok, status: r.status, body: null };
          }
        );
      })
      .then(function (res) {
        if (!res.ok) throw res;
        arrived(res.body || {}, email, wish);
      })
      .catch(function (res) {
        busy(false);
        var body = (res && res.body) || {};
        say(
          typeof body.error === "string" && body.error
            ? body.error
            : "That didn't go through. Check your connection and try again.",
          "bad"
        );
      });
  }

  // ── chips ──────────────────────────────────────────────────────────────────
  function drawChips() {
    el.chips.replaceChildren();
    PLATFORMS.forEach(function (label) {
      var on = picked.indexOf(label) !== -1;
      var b = document.createElement("button");
      b.type = "button";
      b.className = "chip";
      b.textContent = label;
      b.setAttribute("aria-pressed", String(on));
      b.addEventListener("click", function () {
        if (on) picked = picked.filter(function (p) { return p !== label; });
        else picked = picked.concat([label]);
        drawChips();
        saveDraft();
      });
      el.chips.appendChild(b);
    });
  }

  function countWords() {
    var n = words(el.wish.value);
    el.words.textContent = n + " / " + MAX_WORDS + " words";
    el.words.dataset.near = n >= MAX_WORDS ? "full" : n >= MAX_WORDS - 30 ? "close" : "";
  }

  // ── wire up ────────────────────────────────────────────────────────────────
  function start() {
    var draft = readDraft();
    el.email.value = draft.email || "";
    el.wish.value = draft.wish || "";
    picked = Array.isArray(draft.plats) ? draft.plats : [];

    drawChips();
    countWords();
    refreshCta();

    el.email.addEventListener("input", function () {
      refreshCta();
      saveDraft();
      if (el.note.dataset.kind === "bad") say("");
    });
    el.wish.addEventListener("input", function () {
      var kept = clampWords(el.wish.value, MAX_WORDS);
      if (kept !== el.wish.value) el.wish.value = kept;
      countWords();
      saveDraft();
    });
    el.form.addEventListener("submit", join);
    el.again.addEventListener("click", function () {
      el.done.hidden = true;
      el.form.hidden = false;
      el.email.value = "";
      el.wish.value = "";
      picked = [];
      drawChips();
      countWords();
      refreshCta();
      say("");
      el.email.focus();
    });

    if (!ENDPOINT) {
      say("This waitlist isn't connected yet — set endpoint in config.js.", "bad");
    }
    loadCount();
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", start);
  } else {
    start();
  }
})();
