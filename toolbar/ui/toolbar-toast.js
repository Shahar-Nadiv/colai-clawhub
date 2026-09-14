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
 */
function raiseToast(answer, said) {
  const id = `toast-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`;
  state.toasts = [...state.toasts, { id, who: answer.who, said, sessionKey: answer.sessionKey }];
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
      who.textContent = `${toast.who} answered`;
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
      one.addEventListener("click", () => {
        const entry = state.history.find((row) => row.sessionKey === toast.sessionKey);
        if (entry) {
          entry.view.open = true;
          void loadTurns(entry);
        }
        openWork();
        showLatestWork(toast.sessionKey);
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
