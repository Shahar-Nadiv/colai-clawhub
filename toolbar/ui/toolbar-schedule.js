// Scheduled tasks, on the shared overlay shell.
//
// The clock key (left of Work) opens this. It is the same idea as Work — a prompt, and the picture
// of whatever was pointed at — but run later and on a cadence rather than now. colai owns the
// scheduling itself (the tasks live on disk, a thread fires the ones that come due; see
// `colai_schedule.rs`), because Claude Code's own scheduled tasks are cloud routines a plugin can't
// read. So this panel composes and lists colai's local tasks, and links out to claude.ai for the
// cloud ones.
//
// The cadence's first firing time is worked out here, in the page, because this side knows the
// person's own timezone — "every day at 9" is 9 o'clock where they are. It travels to Rust as an
// epoch, which only ever advances it by whole intervals after that.
//
// A classic script sharing one global scope with the rest; reads `el`/`state` from toolbar.js, the
// shell helpers from toolbar-shell.js, and persists its dragged position through `remember`.

/** The cadences on offer, in the order they appear. */
const CADENCES = [
  ["once", "Once"],
  ["hourly", "Every hour"],
  ["daily", "Daily"],
  ["weekly", "Weekly"],
];

/** The days of the week, Sunday first, matching JavaScript's `Date.getDay()`. */
const WEEKDAYS = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

/** A fresh, empty composer. */
function blankDraft() {
  return { prompt: "", kind: "daily", time: "09:00", weekday: 1, once: "" };
}

function openSchedule() {
  if (!state.scheduleDraft) state.scheduleDraft = blankDraft();
  // Like Work: hung off the rail near its own key, not centred on the shell. Opening it closes the
  // rail's flyouts (the receiver picker among them) and the Work panel, so one thing is up at a time.
  state.panel = "schedule";
  state.open = null;
  if (state.work) state.work.open = false;
  el.schedule.hidden = false;
  if (typeof trapFocus === "function") trapFocus(el.schedule);
  loadSchedules();
  render();
}

/** Read the scheduled tasks from disk. Cheap; on demand and after anything changes them. */
function loadSchedules() {
  if (state.scheduleLoading) return;
  state.scheduleLoading = true;
  void invoke("colai_schedule_list")
    .then((got) => {
      state.schedules = Array.isArray(got) ? got : [];
      state.scheduleLoading = false;
      render();
    })
    .catch(() => {
      state.schedules = [];
      state.scheduleLoading = false;
      render();
    });
}

/* ── turning a cadence into its first firing time, in local time ─────────── */

/** Split an "HH:MM" string into [hours, minutes], defaulting to 9:00 if it is malformed. */
function hoursAndMinutes(time) {
  const match = /^(\d{1,2}):(\d{2})$/.exec((time || "").trim());
  if (!match) return [9, 0];
  const h = Math.min(23, Math.max(0, Number(match[1])));
  const m = Math.min(59, Math.max(0, Number(match[2])));
  return [h, m];
}

/** The next time today's clock reaches HH:MM — or tomorrow's, if it is already past. */
function nextDaily(time) {
  const [h, m] = hoursAndMinutes(time);
  const when = new Date();
  when.setHours(h, m, 0, 0);
  if (when.getTime() <= Date.now()) when.setDate(when.getDate() + 1);
  return when.getTime();
}

/** The next time the given weekday reaches HH:MM. */
function nextWeekly(weekday, time) {
  const [h, m] = hoursAndMinutes(time);
  const when = new Date();
  when.setHours(h, m, 0, 0);
  let add = (Number(weekday) - when.getDay() + 7) % 7;
  if (add === 0 && when.getTime() <= Date.now()) add = 7;
  when.setDate(when.getDate() + add);
  return when.getTime();
}

/** The first firing time and a human label for the current draft. */
function cadenceOf(draft) {
  switch (draft.kind) {
    case "once": {
      // A datetime-local string is local time already; an empty one means "an hour from now".
      const at = draft.once ? new Date(draft.once).getTime() : Date.now() + 3_600_000;
      const when = Number.isFinite(at) ? at : Date.now() + 3_600_000;
      return { nextRun: when, label: `Once, on ${whenWords(when)}` };
    }
    case "hourly":
      return { nextRun: Date.now() + 3_600_000, label: "Every hour" };
    case "weekly":
      return {
        nextRun: nextWeekly(draft.weekday, draft.time),
        label: `Weekly on ${WEEKDAYS[Number(draft.weekday)] || "Monday"} at ${clockWords(draft.time)}`,
      };
    case "daily":
    default:
      return { nextRun: nextDaily(draft.time), label: `Daily at ${clockWords(draft.time)}` };
  }
}

/** "9:00 AM" for an "HH:MM" string, in the person's own reading of the clock. */
function clockWords(time) {
  const [h, m] = hoursAndMinutes(time);
  const when = new Date();
  when.setHours(h, m, 0, 0);
  return when.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
}

/** A short, local rendering of an epoch — "Mon, Jan 6, 9:00 AM". */
function whenWords(ms) {
  if (!Number.isFinite(ms)) return "—";
  return new Date(ms).toLocaleString([], {
    weekday: "short",
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
  });
}

/* ── drawing the panel ───────────────────────────────────────────────────── */

function drawSchedule() {
  if (!el.schedule) return;
  el.schedule.hidden = state.panel !== "schedule";
  if (state.panel !== "schedule") return;
  if (!state.scheduleDraft) state.scheduleDraft = blankDraft();
  const head = scheduleHead();
  const body = document.createElement("div");
  body.className = "sched-body";
  body.append(scheduleComposer(), scheduleList());
  el.schedule.replaceChildren(head, body);
  // Positioned by placeFlyout in render(), pinned to the clock key the way Work is pinned to send —
  // so it stays attached to the rail wherever the rail is, rather than floating free.
}

/** Title, the link out to the cloud routines, and the close button. The whole row is the handle. */
function scheduleHead() {
  const head = document.createElement("div");
  head.className = "sched-head";
  const title = document.createElement("span");
  title.className = "sched-title";
  title.textContent = "Scheduled tasks";

  const web = document.createElement("button");
  web.type = "button";
  web.className = "sched-web";
  web.textContent = "Open on claude.ai ↗";
  web.title = "Open Claude Code's scheduled tasks in your browser";
  web.addEventListener("click", () => {
    void invoke("colai_schedule_open_web").catch((error) => {
      state.trouble = `Couldn't open the page — ${error && error.message ? error.message : String(error)}`;
      render();
    });
  });

  const shut = document.createElement("button");
  shut.type = "button";
  shut.className = "sched-shut";
  shut.title = "Close";
  shut.setAttribute("aria-label", "Close scheduled tasks");
  shut.textContent = "×";
  shut.addEventListener("click", () => {
    closeSurface();
    render();
  });

  head.append(title, web, shut);
  return head;
}

/** The composer: what to do, how often, and what it is pointing at. */
function scheduleComposer() {
  const draft = state.scheduleDraft;
  const wrap = document.createElement("div");
  wrap.className = "sched-compose";

  const prompt = document.createElement("textarea");
  prompt.className = "sched-prompt";
  prompt.rows = 2;
  prompt.placeholder = "What should Claude Code do, on a schedule?";
  prompt.value = draft.prompt;
  // Kept on every keystroke so a re-render restores it; no render here, to leave the caret alone.
  prompt.addEventListener("input", () => {
    draft.prompt = prompt.value;
  });
  /*
   * `/` and `@` here too, because this box is a prompt like any other.
   *
   * A scheduled task's words reach Claude Code exactly as typed when it fires, so a `/`
   * command runs and an `@` path is read — the same two keystrokes doing the same two
   * things they do in a terminal. The menu is positioned against this `.ask-box`, and the
   * draft (which survives a re-render) holds the menu state and the words — see `replying`.
   */
  const promptBox = document.createElement("div");
  promptBox.className = "ask-box";
  const promptMenu = document.createElement("div");
  promptMenu.className = "ask-menu";
  promptMenu.hidden = true;
  promptBox.append(prompt, promptMenu);
  completes(prompt, promptMenu, replying(askingOn(draft), (said) => (draft.prompt = said)));

  // How often. A segmented row; changing it re-renders, because which fields show depends on it.
  const cadence = document.createElement("div");
  cadence.className = "sched-seg";
  for (const [value, shown] of CADENCES) {
    const pick = document.createElement("button");
    pick.type = "button";
    pick.className = "sched-seg-tab";
    pick.dataset.on = String(draft.kind === value);
    pick.dataset.value = value;
    pick.textContent = shown;
    pick.addEventListener("click", () => {
      draft.kind = value;
      render();
    });
    cadence.append(pick);
  }

  // When, shaped by the cadence: a time for daily/weekly, a weekday too for weekly, a full
  // date-and-time for a one-off, and nothing for hourly.
  const when = document.createElement("div");
  when.className = "sched-when";
  if (draft.kind === "weekly") {
    const day = document.createElement("select");
    day.className = "sched-day";
    WEEKDAYS.forEach((name, index) => {
      const option = document.createElement("option");
      option.value = String(index);
      option.textContent = name;
      if (Number(draft.weekday) === index) option.selected = true;
      day.append(option);
    });
    day.addEventListener("change", () => {
      draft.weekday = Number(day.value);
    });
    when.append(day);
  }
  if (draft.kind === "daily" || draft.kind === "weekly") {
    const time = document.createElement("input");
    time.type = "time";
    time.className = "sched-time";
    time.value = draft.time;
    time.addEventListener("input", () => {
      draft.time = time.value || "09:00";
    });
    when.append(time);
  }
  if (draft.kind === "once") {
    const at = document.createElement("input");
    at.type = "datetime-local";
    at.className = "sched-at";
    at.value = draft.once;
    at.addEventListener("input", () => {
      draft.once = at.value;
    });
    when.append(at);
  }
  if (draft.kind === "hourly") {
    const note = document.createElement("span");
    note.className = "sched-when-note";
    note.textContent = "Runs once an hour, starting an hour from now.";
    when.append(note);
  }

  // What it is pointing at, if anything — the "visual prompt" it will carry every time it fires.
  const marks = state.marks || [];
  if (marks.length) {
    const pointing = document.createElement("p");
    pointing.className = "sched-marks";
    pointing.textContent = `Pointing at ${marks.length} thing${marks.length === 1 ? "" : "s"} — a picture of each goes with this task.`;
    wrap.append(promptBox, cadence, when, pointing);
  } else {
    wrap.append(promptBox, cadence, when);
  }

  const add = document.createElement("button");
  add.type = "button";
  add.className = "sched-add";
  add.textContent = "Schedule it";
  add.addEventListener("click", scheduleIt);
  wrap.append(add);
  return wrap;
}

/** Schedule the composed task, with whatever is pointed at attached to it. */
function scheduleIt() {
  const draft = state.scheduleDraft || blankDraft();
  const prompt = (draft.prompt || "").trim();
  if (!prompt) {
    state.trouble = "A scheduled task needs something to do.";
    render();
    return;
  }
  const { nextRun, label } = cadenceOf(draft);
  const markIds = (state.marks || []).map((mark) => mark.id);
  // Each key written as `key: value`, not shorthand — the source-shape test that checks every
  // required argument is handed over reads these with a regex that skips a shorthand key sitting
  // right after another, and would miss `nextRun`.
  void invoke("colai_schedule_add", {
    prompt: prompt,
    nextRun: nextRun,
    cadence: { kind: draft.kind, label: label },
    cwd: state.cwd || null,
    markIds: markIds,
  })
    .then(() => {
      // A clean slate for the next one, and the pointed-at marks are now the task's — clear them
      // the way sending does, so they are not silently attached to the next thing too.
      state.scheduleDraft = blankDraft();
      if (markIds.length) {
        state.marks = [];
        state.undone = [];
      }
      say(`Scheduled — ${label.toLowerCase()}.`, "receipt");
      loadSchedules();
      render();
    })
    .catch((error) => {
      state.trouble = `Couldn't schedule that — ${error && error.message ? error.message : String(error)}`;
      render();
    });
}

/** The list of scheduled tasks, each with when it next runs and the ways to change it. */
function scheduleList() {
  const list = document.createElement("div");
  list.className = "sched-list";
  const tasks = state.schedules || [];
  if (state.scheduleLoading && !tasks.length) {
    const note = document.createElement("p");
    note.className = "sched-empty";
    note.textContent = "Reading your scheduled tasks…";
    list.append(note);
    return list;
  }
  if (!tasks.length) {
    const note = document.createElement("p");
    note.className = "sched-empty";
    note.textContent = "Nothing scheduled yet.";
    list.append(note);
    return list;
  }
  for (const task of tasks) list.append(taskRow(task));
  return list;
}

/** One scheduled task. */
function taskRow(task) {
  const row = document.createElement("div");
  row.className = "sched-task";
  row.dataset.on = String(task.enabled !== false);

  const main = document.createElement("div");
  main.className = "sched-task-main";
  const what = document.createElement("div");
  what.className = "sched-task-what";
  what.textContent = task.prompt || "";
  const meta = document.createElement("div");
  meta.className = "sched-task-meta";
  const cadence = (task.cadence && task.cadence.label) || (task.cadence && task.cadence.kind) || "once";
  const next = task.enabled === false ? "paused" : `next ${whenWords(task.nextRun)}`;
  const ran = task.lastRun ? ` · last ran ${whenWords(task.lastRun)}` : "";
  const shot = task.images && task.images.length ? " · 📎" : "";
  meta.textContent = `${cadence} · ${next}${ran}${shot}`;
  main.append(what, meta);

  const buttons = document.createElement("div");
  buttons.className = "sched-task-buttons";

  const runNow = document.createElement("button");
  runNow.type = "button";
  runNow.className = "sched-task-run";
  runNow.textContent = "Run now";
  runNow.title = "Run this task once, right now";
  runNow.addEventListener("click", () => {
    void invoke("colai_schedule_run_now", { id: task.id })
      .then(() => {
        say(`Running “${schedulePreview(task.prompt)}” now.`, "receipt");
        loadSchedules();
        render();
      })
      .catch((error) => {
        state.trouble = `Couldn't run that — ${error && error.message ? error.message : String(error)}`;
        render();
      });
  });

  const pause = document.createElement("button");
  pause.type = "button";
  pause.className = "sched-task-pause";
  const on = task.enabled !== false;
  pause.textContent = on ? "Pause" : "Resume";
  pause.title = on ? "Stop this task firing" : "Let this task fire again";
  pause.addEventListener("click", () => {
    void invoke("colai_schedule_toggle", { id: task.id, enabled: !on })
      .then(() => {
        loadSchedules();
        render();
      })
      .catch((error) => {
        state.trouble = `Couldn't change that — ${error && error.message ? error.message : String(error)}`;
        render();
      });
  });

  const remove = document.createElement("button");
  remove.type = "button";
  remove.className = "sched-task-remove";
  remove.textContent = "Remove";
  remove.title = "Forget this task";
  remove.addEventListener("click", () => {
    void invoke("colai_schedule_remove", { id: task.id })
      .then(() => {
        loadSchedules();
        render();
      })
      .catch((error) => {
        state.trouble = `Couldn't remove that — ${error && error.message ? error.message : String(error)}`;
        render();
      });
  });

  buttons.append(runNow, pause, remove);
  row.append(main, buttons);
  return row;
}

/** A prompt shortened to fit a one-line receipt or notice. */
function schedulePreview(text) {
  const said = String(text || "").trim();
  return said.length > 40 ? `${said.slice(0, 39)}…` : said;
}
