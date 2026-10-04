// Saying that something came back, to somebody who was looking elsewhere.
//
// The whole point of marking on the screen is that you carry on working while an agent
// does. Which means the moment a reply lands is the moment nobody is watching for it:
// the pin lights up on a window that is behind three others, and it is found later by
// accident.
//
// So an answer says so. Once, briefly, with a way into it — and then it gets out of the
// way, because a notification that has to be dismissed is a second thing to do.

/** How long anything the toolbar says about itself stays up. */
const TOAST_FOR = 5000;

/** What is on screen saying something arrived, and the timers that will take them off. */
const fading = new Map();

/**
 * Raise one for an answer that has just spoken for the first time.
 *
 * The first turn only. An agent says what it is doing before it says what it found, and
 * one send that talks four times is one thing that happened — four toasts for it would
 * be the toolbar shouting about its own progress.
 *
 * And for a reply that came after the turn had ended (`heardLaterReply`), which says
 * "replied" instead: that one is a separate thing that happened, often much later.
 */
function raiseToast(answer, said, verb = "answered") {
  const id = `toast-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`;
  // One per conversation. A chat that replies again while its last toast is still up has
  // the newer thing to say, and a column of toasts about one conversation is the toolbar
  // shouting — so the older one goes, timer and all.
  for (const was of state.toasts.filter((toast) => toast.sessionKey === answer.sessionKey)) {
    const timer = fading.get(was.id);
    if (timer !== undefined) clearTimeout(timer);
    fading.delete(was.id);
  }
  state.toasts = [
    ...state.toasts.filter((toast) => toast.sessionKey !== answer.sessionKey),
    { id, who: answer.who, said, verb, sessionKey: answer.sessionKey },
  ];
  fading.set(
    id,
    setTimeout(() => dropToast(id), TOAST_FOR),
  );
  render();
}

function dropToast(id) {
  const timer = fading.get(id);
  if (timer !== undefined) clearTimeout(timer);
  fading.delete(id);
  state.toasts = state.toasts.filter((toast) => toast.id !== id);
  render();
}

function drawToasts() {
  el.toasts.hidden = state.toasts.length === 0;
  if (state.toasts.length === 0) {
    el.toasts.replaceChildren();
    return;
  }
  el.toasts.replaceChildren(
    ...state.toasts.map((toast) => {
      const one = document.createElement("button");
      one.type = "button";
      one.className = "toast";
      one.title = "Open it";

      const who = document.createElement("span");
      who.className = "toast-who";
      who.textContent = `${toast.who} ${toast.verb || "answered"}`;
      const said = document.createElement("span");
      said.className = "toast-said";
      said.textContent = toast.said;
      one.append(who, said);

      /*
       * Pressing it opens the conversation it is about, and takes the toast away — it has
       * done its job the moment somebody has acted on it.
       *
       * It used to set `answer.open`, a field nothing has read since replies stopped being
       * pins on the desktop, and only call `openWork()` in the branch that never runs — a
       * toast is raised only for a session already in `state.answers`. So pressing it
       * dismissed the toast and opened nothing at all.
       */
      //
      // `openWorkAt` unfolds the row, widens the panel if it would hide it, and counts its
      // replies as read — pressing the toast is looking at the reply.
      one.addEventListener("click", () => {
        openWorkAt(toast.sessionKey);
        dropToast(toast.id);
      });
      return one;
    }),
  );
  placeToasts();
}

/**
 * Low on the screen the rail is on, out of the way of what is being worked on.
 *
 * The same room the windows use, so a toast never lands across a bezel or under the
 * desktop's own dock — which is exactly where the eye is least likely to find it.
 */
function placeToasts() {
  const room = usable(screenAt(state.screens, state.at || { x: 0, y: 0 }));
  const box = el.toasts.getBoundingClientRect();
  el.toasts.style.left = `${Math.round(room.right - box.width - EDGE)}px`;
  el.toasts.style.top = `${Math.round(room.bottom - box.height - EDGE)}px`;
}
