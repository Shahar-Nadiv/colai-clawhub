// The agent's response, beside the toolbar.
//
// When a turn is sent, a card opens next to the rail (not out at the mark, which scattered answers
// across the desktop) and shows the answer there — the "Agent Responses" surface. It is written
// for somebody who does not want to read an agent
// thinking out loud: while the turn runs it shows a plain-language checklist of what the agent is
// doing ("Writing snake.html"), not the prose; when it finishes it leads with what actually
// changed ("Created snake.html") and keeps the full reply folded behind a reveal. The Work panel
// keeps the same response as a row, so nothing is lost when this card is dismissed.
//
// A classic script sharing one global scope with the rest; reads `el`/`state` from toolbar.js, the
// typed turn from `state.turns` (filled by the `colai:response` listener, with `steps` filled by
// the `colai:doing`/`colai:did` listeners), and reuses `verdict` (toolbar-answers.js) to send a
// follow-up in the same turn.

/** The kind badge text. */
const KIND_SAID = {
  answer: "Answer",
  question: "Question",
  change: "Change",
};

/** A reply longer than this is folded behind "Show more" — the card stays short by default. */
const ANSWER_FOLD_AT = 220;

/** Which kind of step a tool is, for the checklist and the outcome line. */
function stepKind(name) {
  const key = String(name || "")
    .toLowerCase()
    .replace(/_/g, "");
  if (key === "write") return "create";
  if (["edit", "multiedit", "applypatch", "notebookedit"].includes(key)) return "edit";
  if (["bash", "shell", "exec", "terminal", "process"].includes(key)) return "run";
  if (["read", "viewimage", "readpage", "webfetch"].includes(key)) return "read";
  if (["grep", "search", "glob", "find", "ls", "websearch"].includes(key)) return "look";
  return "other";
}

/** The file a tool call was about, if any — the last path segment, which is what a person reads. */
function stepFile(args) {
  const path = args && (args.file_path || args.path || args.notebook_path);
  if (!path) return null;
  const parts = String(path).split(/[\\/]/);
  return parts[parts.length - 1] || String(path);
}

/** Record a step onto the turn it belongs to, so the card can show it. Deduped by call id. */
function noteStep(said) {
  const key = said && said.sessionKey;
  if (!key) return;
  const turn = (state.turns[key] = state.turns[key] || { kind: "answer", state: "working" });
  turn.steps = turn.steps || [];
  if (said.id && turn.steps.some((step) => step.id === said.id)) return;
  const args = said.args || {};
  const verb = stepKind(said.name);
  turn.steps.push({
    id: said.id || null,
    said: doingTool({ name: said.name, args }) || "Working…",
    file: stepFile(args),
    verb,
    done: false,
    wrong: false,
    // What it would take to put this edit back, kept from the agent's own tool call. An Edit
    // records the exact before/after text, so Undo can reverse it on disk without the terminal
    // that made it (see `undoChanges` + the `colai_revert_edits` command). A full-file Write keeps
    // no prior copy, so it is marked unrevertible rather than guessed at.
    change: changeFrom(verb, said.name, args),
  });
}

/** The reversible record of a create/edit step, or null when it is not a file change. */
function changeFrom(verb, name, args) {
  if (verb !== "edit" && verb !== "create") return null;
  const path = (args && (args.file_path || args.path || args.notebook_path)) || null;
  if (!path) return null;
  const key = String(name || "").toLowerCase().replace(/_/g, "");
  // Only a plain Edit carries a single before/after this can reverse on disk. Write overwrites a
  // whole file, and MultiEdit/applypatch/notebook edits are multi-part — recorded, but marked so
  // Undo says it cannot put them back by itself rather than doing half a job.
  if (key === "edit") {
    return { path, kind: "edit", old: String(args.old_string || ""), new: String(args.new_string || "") };
  }
  // A MultiEdit is named as one, so a rewind that cannot put it back says which it was rather
  // than calling it a Write.
  const kind = key === "multiedit" ? "multiedit" : "write";
  return { path, kind, old: "", new: String(args.content || args.new_string || "") };
}

/** The reversible changes a settled turn made, newest last (the order they were applied). */
function changesOf(turn) {
  return (turn.steps || []).map((step) => step.change).filter(Boolean);
}

/** Tick (or cross) the step a result belongs to, wherever it started. */
function finishStep(said) {
  if (!said || !said.id) return;
  const key = said.sessionKey;
  const turns = key && state.turns[key] ? [state.turns[key]] : Object.values(state.turns || {});
  for (const turn of turns) {
    const step = (turn.steps || []).find((one) => one.id === said.id);
    if (step) {
      step.done = true;
      step.wrong = Boolean(said.wrong || said.never);
      return;
    }
  }
}

/**
 * What the turn actually changed, in one plain line — the headline a no-code person reads instead
 * of the prose. Built from the steps, not from what the agent said about them, so it is true even
 * when the agent's account is long or absent. `null` when nothing was written (a pure answer).
 */
function outcomeOf(turn) {
  const touched = (turn.steps || []).filter((step) => step.verb === "create" || step.verb === "edit");
  const created = [...new Set(touched.filter((s) => s.verb === "create").map((s) => s.file).filter(Boolean))];
  const edited = [...new Set(touched.filter((s) => s.verb === "edit").map((s) => s.file).filter(Boolean))];
  const files = [...new Set([...created, ...edited])];
  if (!files.length) return null;
  if (files.length === 1) {
    const only = files[0];
    const madeNew = created.includes(only) && !edited.includes(only);
    return { said: `${madeNew ? "Created" : "Updated"} ${only}`, files };
  }
  return { said: `Changed ${files.length} files`, files };
}

/**
 * Put the card beside the toolbar — not out at the mark.
 *
 * It used to open where the marks were, out on the desktop, which scattered answers across the
 * screen and far from the rail that sent them. It hangs off the toolbar now: under a rail docked
 * along the top, above one docked along the bottom, and to the open side of one docked left or
 * right — aligned to the rail's near edge, and always nudged fully on-screen. One fixed place to
 * look for what the agent said, wherever the rail has been dragged.
 */
function placeResponse() {
  const card = el.response;
  if (!el.rail) return;
  const rail = el.rail.getBoundingClientRect();
  const width = card.offsetWidth || 340;
  const height = card.offsetHeight || 160;
  const gap = 12;
  const edge = 12;
  const dock = state.dock || "top";
  const vertical =
    typeof isVertical === "function" ? isVertical(dock) : dock === "left" || dock === "right";

  let left;
  let top;
  if (vertical) {
    // A side-docked rail is a vertical pill; the card sits beside it, tops aligned. If the chosen
    // side has no room, it takes the other.
    top = rail.top;
    left = dock === "right" ? rail.left - width - gap : rail.right + gap;
    if (left < edge) left = rail.right + gap;
    if (left + width > window.innerWidth - edge) left = rail.left - width - gap;
  } else {
    // A top/bottom-docked rail is horizontal; the card hangs under it (or over it), left edges
    // aligned so it reads as belonging to the rail.
    left = rail.left;
    top = dock === "bottom" ? rail.top - height - gap : rail.bottom + gap;
    if (top < edge) top = rail.bottom + gap;
    if (top + height > window.innerHeight - edge) top = rail.top - height - gap;
  }
  left = Math.max(edge, Math.min(left, window.innerWidth - width - edge));
  top = Math.max(edge, Math.min(top, window.innerHeight - height - edge));
  card.style.position = "fixed";
  card.style.left = `${Math.round(left)}px`;
  card.style.top = `${Math.round(top)}px`;
}

/** Take the card down. The turn stays in `state.turns` and in the Work log. */
function dismissResponse() {
  state.responding = null;
  render();
}

/** The header: a state dot, the kind badge, who, and a close. */
function respHead(kind, turn, active) {
  const head = document.createElement("div");
  head.className = "resp-head";
  const dot = document.createElement("span");
  dot.className = "resp-dot";
  const badge = document.createElement("span");
  badge.className = "resp-badge";
  const settled = turn.state === "done" || turn.state === "failed";
  // Say the state in a word, not just a colour — "Working" / "Done" / "Couldn't".
  badge.textContent = turn.state === "failed"
    ? "Couldn't"
    : settled
      ? "Done"
      : KIND_SAID[kind] === "Answer"
        ? "Working"
        : KIND_SAID[kind] || "Working";
  const who = document.createElement("span");
  who.className = "resp-who";
  who.textContent = active.who || "";
  const shut = document.createElement("button");
  shut.type = "button";
  shut.className = "resp-shut";
  shut.title = "Dismiss";
  shut.setAttribute("aria-label", "Dismiss");
  shut.textContent = "×";
  shut.addEventListener("click", dismissResponse);
  head.append(dot, badge, who, shut);
  return head;
}

/** The checklist shown while the turn runs: what has been done, and the one thing happening now. */
function respSteps(turn) {
  const steps = turn.steps || [];
  const wrap = document.createElement("div");
  wrap.className = "resp-steps";
  // The last few, so a long turn does not grow the card without bound; the current one always
  // shows because it is last.
  for (const step of steps.slice(-4)) {
    const row = document.createElement("div");
    row.className = "resp-step";
    row.dataset.state = step.wrong ? "wrong" : step.done ? "done" : "now";
    const mark = document.createElement("span");
    mark.className = "resp-tick";
    mark.textContent = step.wrong ? "×" : step.done ? "✓" : "◍";
    const said = document.createElement("span");
    said.className = "resp-step-said";
    said.textContent = step.said;
    row.append(mark, said);
    wrap.append(row);
  }
  // Nothing picked up yet — the agent is still reading the ask.
  if (!steps.length) {
    const row = document.createElement("div");
    row.className = "resp-step";
    row.dataset.state = "now";
    const mark = document.createElement("span");
    mark.className = "resp-tick";
    mark.textContent = "◍";
    const said = document.createElement("span");
    said.className = "resp-step-said";
    said.textContent = doingSaid(state.doing, state.atWork, Date.now()) || "Working…";
    row.append(mark, said);
    wrap.append(row);
  }
  return wrap;
}

/** The outcome line + file chips shown once the turn has finished. */
function respOutcome(turn) {
  const outcome = outcomeOf(turn);
  if (!outcome) return null;
  const wrap = document.createElement("div");
  wrap.className = "resp-outcome";
  const line = document.createElement("div");
  line.className = "resp-outcome-said";
  const tick = document.createElement("span");
  tick.className = "resp-tick";
  tick.textContent = "✓";
  const said = document.createElement("span");
  said.textContent = outcome.said;
  line.append(tick, said);
  wrap.append(line);
  if (outcome.files.length > 1) {
    const chips = document.createElement("div");
    chips.className = "resp-files";
    for (const file of outcome.files.slice(0, 6)) {
      const chip = document.createElement("span");
      chip.className = "resp-file";
      chip.textContent = file;
      chips.append(chip);
    }
    wrap.append(chips);
  }
  return wrap;
}

/** The agent's own words, folded to a few lines with a reveal when they run long. */
function respAnswer(turn, active) {
  const text = (turn.body || "").trim();
  if (!text) return null;
  const wrap = document.createElement("div");
  wrap.className = "resp-answer";
  // The agent's words, as markdown rather than a run-on line (see toolbar-markdown.js). Folded to a
  // few lines by default; "Show more" lifts the fold. Code blocks scroll within the card.
  const said = document.createElement("div");
  said.className = "md";
  said.append(renderMarkdown(text));
  const long = text.length > ANSWER_FOLD_AT;
  if (long && !active.showAll) said.classList.add("md-fold");
  wrap.append(said);
  if (long) {
    const more = document.createElement("button");
    more.type = "button";
    more.className = "resp-more";
    more.textContent = active.showAll ? "Show less" : "Show more";
    more.addEventListener("click", () => {
      active.showAll = !active.showAll;
      render();
    });
    wrap.append(more);
  }
  return wrap;
}

function drawResponse() {
  if (!el.response) return;
  const active = state.responding;
  if (!active) {
    el.response.hidden = true;
    return;
  }
  const turn = state.turns[String(active.turnId)] || { kind: "answer", state: "working" };
  const kind = turn.kind || "answer";
  const settled = turn.state === "done" || turn.state === "failed";
  const failed = turn.state === "failed";

  el.response.hidden = false;
  el.response.dataset.kind = kind;
  el.response.dataset.state = turn.state || "working";

  const bits = [respHead(kind, turn, active)];

  // The prompt, collapsed to a one-line quote so the question stays above its answer.
  const quote = document.createElement("p");
  quote.className = "resp-quote";
  quote.textContent = `› ${active.said || (active.marks || []).join(", ") || "…"}`;
  bits.push(quote);

  if (!settled) {
    // While it runs: the plain-language checklist, never the streaming prose.
    bits.push(respSteps(turn));
  } else {
    // Finished: lead with what changed, then the answer folded short. A failure shows its reason
    // as the answer and skips the outcome line (nothing was changed to report).
    if (!failed) {
      const outcome = respOutcome(turn);
      if (outcome) bits.push(outcome);
      // The edits it made, to keep or put back — shown here rather than applied silently. A mark
      // handed to a terminal applies its edits directly, so this is a post-hoc review: Keep leaves
      // them, Undo reverses them on disk.
      const review = respReview(turn, active);
      if (review) bits.push(review);
    }
    const answer = respAnswer(turn, active);
    if (answer) bits.push(answer);

    // Follow-up chips: up to three suggested next steps that send as the next prompt.
    if (Array.isArray(turn.followUps) && turn.followUps.length) {
      const chips = document.createElement("div");
      chips.className = "resp-follows";
      for (const step of turn.followUps.slice(0, 3)) {
        const chip = document.createElement("button");
        chip.type = "button";
        chip.className = "resp-follow";
        chip.textContent = step;
        chip.addEventListener("click", () => replyInTurn(active.turnId, step));
        chips.append(chip);
      }
      bits.push(chips);
    }
  }

  // The reply field — steer or continue the turn in plain words. Enter sends.
  const reply = document.createElement("textarea");
  reply.className = "resp-reply popup-note";
  reply.rows = 1;
  reply.placeholder = settled ? "Ask for a change…" : "Reply…";
  reply.dataset.field = "resp-reply";
  reply.value = active.draft || "";
  reply.addEventListener("input", () => {
    active.draft = reply.value;
  });
  /*
   * `/` and `@` here too, because this box is a prompt like any other.
   *
   * What goes in it reaches the conversation exactly as typed, so a `/` command runs and an
   * `@` path is read — the same two keystrokes doing the same two things they do in a
   * terminal. The menu is positioned against this `.ask-box`, and `active` (which survives a
   * re-render — it is what `active.draft` is kept on) holds the menu state — see `replying`.
   */
  const replyBox = document.createElement("div");
  replyBox.className = "ask-box";
  const replyMenu = document.createElement("div");
  replyMenu.className = "ask-menu";
  replyMenu.hidden = true;
  // Attached before `completes`, so on Enter this listener runs first and can see the menu
  // still open — unless it is, it sends. The other way round, the menu would take the
  // highlighted row and close, and this would then read a hidden menu and send.
  reply.addEventListener("keydown", (event) => {
    if (replyMenu.hidden && event.key === "Enter" && (event.ctrlKey || event.metaKey || !event.shiftKey)) {
      event.preventDefault();
      const said = reply.value.trim();
      if (said) {
        replyInTurn(active.turnId, said);
        active.draft = "";
      }
    }
  });
  replyBox.append(reply, replyMenu);
  completes(reply, replyMenu, replying(askingOn(active), (said) => (active.draft = said)));
  bits.push(replyBox);

  el.response.replaceChildren(...bits);
  placeResponse();
}

/** A review screenshot, click-to-enlarge — the card's thumbnail is small, the detail is in the shot. */
function shotImage(src, alt) {
  const shot = document.createElement("div");
  shot.className = "resp-after-shot";
  const img = document.createElement("img");
  img.src = src;
  img.alt = `${alt} — click to enlarge`;
  img.title = "Click to enlarge";
  img.addEventListener("click", () => openZoom(src));
  shot.append(img);
  return shot;
}

/**
 * The edits a turn made, to keep or put back.
 *
 * Shown once the turn has settled: a screenshot of the result (click to enlarge), Keep and Undo,
 * and the actual edit behind a "See the code" reveal for anyone who wants it. Keep just dismisses;
 * Undo reverses the edits on disk via `colai_revert_edits` — which is how an edit a terminal made,
 * that colai only watched, can still be taken back. Null when the turn changed no files, or once it
 * has already been kept or put back.
 */
function respReview(turn, active) {
  const changes = changesOf(turn);
  if (!changes.length || active.reviewed) return null;

  const wrap = document.createElement("div");
  wrap.className = "resp-review";

  // Show the result as a *picture*, never as code. For an edited web page, render the edited file
  // and screenshot that — a true after without waiting for a reload. Otherwise re-photograph the
  // marked spot on screen. The raw change stays available only behind "See the code".
  const web = changes.find((change) => /\.html?$/i.test(change.path || ""));
  const entry = (state.history || []).find(
    (one) => one.sessionKey === String(active.turnId) && one.regions && one.regions.length,
  );
  if (web) {
    if (!active.rendered) {
      active.rendered = true;
      renderAfter(active, web.path);
    }
    if (active.renderShot) {
      wrap.append(shotImage(active.renderShot, "the edited page, rendered"));
    }
    const note = document.createElement("p");
    note.className = "resp-shot-note";
    note.textContent = active.rendering
      ? "Rendering the result…"
      : active.renderTrouble
        ? `Couldn't render it — ${active.renderTrouble}`
        : "Rendered from the edited page. A part that only shows after you interact (a modal, a game-over card) can't appear here.";
    wrap.append(note);
    const again = document.createElement("button");
    again.type = "button";
    again.className = "resp-more";
    again.textContent = active.renderShot ? "Re-render" : "Show the result";
    again.disabled = Boolean(active.rendering);
    again.addEventListener("click", () => renderAfter(active, web.path));
    wrap.append(again);
  } else if (entry && typeof photograph === "function") {
    // Not a web page to render — re-photograph the marked spot on screen (the after, once the app
    // has refreshed). Never the before.
    if (!active.shotOnce) {
      active.shotOnce = true;
      shootAfter(active, entry);
    }
    if (active.afterShot) {
      wrap.append(shotImage(active.afterShot, "the spot on screen, after the change"));
    }
    const note = document.createElement("p");
    note.className = "resp-shot-note";
    note.textContent = active.shooting
      ? "Photographing the spot…"
      : active.afterTrouble
        ? `Couldn't photograph it — ${active.afterTrouble}`
        : "If this still shows the old version, reload the app and re-shoot.";
    wrap.append(note);
    const shoot = document.createElement("button");
    shoot.type = "button";
    shoot.className = "resp-more";
    shoot.textContent = active.afterShot ? "Re-shoot" : "Show it on screen";
    shoot.disabled = Boolean(active.shooting);
    shoot.addEventListener("click", () => shootAfter(active, entry));
    wrap.append(shoot);
  }

  if (active.undoTrouble) {
    const said = document.createElement("p");
    said.className = "resp-undo-trouble";
    said.textContent = active.undoTrouble;
    wrap.append(said);
  }

  const row = document.createElement("div");
  row.className = "resp-review-row";
  const keep = document.createElement("button");
  keep.type = "button";
  keep.className = "resp-keep";
  keep.textContent = "Keep";
  keep.addEventListener("click", () => {
    active.reviewed = "kept";
    dismissResponse();
  });
  const undo = document.createElement("button");
  undo.type = "button";
  undo.className = "resp-undo";
  undo.textContent = active.undoing ? "Putting back…" : "Undo";
  undo.disabled = Boolean(active.undoing);
  undo.addEventListener("click", () => undoChanges(active, changes));

  // The code itself, folded away — visual-first, for anyone who wants to see exactly what changed.
  const see = document.createElement("button");
  see.type = "button";
  see.className = "resp-more";
  see.textContent = active.showCode ? "Hide the code" : "See the code";
  see.addEventListener("click", () => {
    active.showCode = !active.showCode;
    render();
  });

  row.append(keep, undo, see);
  wrap.append(row);

  if (active.showCode) {
    const code = document.createElement("div");
    code.className = "resp-diff";
    for (const change of changes) {
      const head = document.createElement("div");
      head.className = "resp-diff-file";
      head.textContent = change.path;
      code.append(head);
      if (change.kind === "edit") {
        const minus = document.createElement("pre");
        minus.className = "resp-diff-line resp-diff-del";
        minus.textContent = change.old || "(nothing)";
        const plus = document.createElement("pre");
        plus.className = "resp-diff-line resp-diff-add";
        plus.textContent = change.new;
        code.append(minus, plus);
      } else {
        const note = document.createElement("p");
        note.className = "resp-diff-note";
        note.textContent =
          change.kind === "multiedit"
            ? "Several edits at once — Undo can't put this one back on its own."
            : "A new file / full rewrite — Undo can't put this one back on its own.";
        code.append(note);
      }
    }
    wrap.append(code);
  }

  return wrap;
}

/**
 * Photograph the marked spot again — the after, re-shot. Never a "before": it is always a fresh
 * picture of the region as it is now. Reuses `photograph`, the one capture path.
 */
function shootAfter(active, entry) {
  const spot = entry.regions && entry.regions[0];
  if (!spot || typeof photograph !== "function") return;
  active.shooting = true;
  active.afterTrouble = null;
  render();
  const mark = { tool: "box", region: spot.region || null, points: spot.points || [], on: spot.on || null };
  Promise.resolve(photograph(mark))
    .then(() => {
      active.shooting = false;
      active.afterShot = mark.thumb || active.afterShot || null;
      active.afterTrouble = mark.thumb ? null : mark.trouble || "no picture came back";
      render();
    })
    .catch((error) => {
      active.shooting = false;
      active.afterTrouble = error && error.message ? error.message : String(error);
      render();
    });
}

/**
 * Render the edited web page and show the screenshot as the after — a picture of the result, not
 * the code. colai renders the file headlessly, so this is true the moment the edit lands, without
 * the live app having reloaded.
 */
function renderAfter(active, path) {
  active.rendering = true;
  active.renderTrouble = null;
  render();
  void invoke("colai_render_shot", { file: path })
    .then((url) => {
      active.rendering = false;
      active.renderShot = url || active.renderShot || null;
      active.renderTrouble = url ? null : "no picture came back";
      render();
    })
    .catch((error) => {
      active.rendering = false;
      active.renderTrouble = error && error.message ? error.message : String(error);
      render();
    });
}

/** Put the turn's edits back on disk, and say how it went. */
function undoChanges(active, changes) {
  active.undoing = true;
  active.undoTrouble = null;
  render();
  void invoke("colai_revert_edits", { edits: changes, dryRun: false })
    .then((result) => {
      active.undoing = false;
      // `{ restored, skipped }`, one entry per edit; only what was left alone needs saying.
      const left = (result && Array.isArray(result.skipped) ? result.skipped : []).map(
        (one) => `${one.file}: ${one.why}`,
      );
      if (left.length === 0) {
        active.reviewed = "undone";
        state.trouble = `Put ${changes.length === 1 ? "the change" : `${changes.length} changes`} back.`;
        dismissResponse();
      } else {
        // Some could not be reversed (a full-file write, or text edited since). Keep the card up and
        // say which, rather than claiming a clean undo.
        active.undoTrouble = `Couldn't put ${left.length} back — ${left.join("; ")}`;
        render();
      }
    })
    .catch((error) => {
      active.undoing = false;
      active.undoTrouble = `Couldn't undo — ${error && error.message ? error.message : String(error)}`;
      render();
    });
}

/** Send a follow-up into the same turn, reusing the answer object the send registered. */
function replyInTurn(turnId, said) {
  const answer = (state.answers || []).find((one) => one.sessionKey === String(turnId));
  if (answer && typeof verdict === "function") {
    void verdict(answer, said);
  }
  render();
}
