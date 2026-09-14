// Whether there is a screen here that this toolbar can actually work on.
//
// Kept apart from the code that starts things, and pure, because it is a decision rather
// than an action: it is asked before the toolbar is spawned, and again by `openclaw colai
// status`, and both have to give the same answer.

/** Why the toolbar will not work here, and what the person can do about it. */
export type NoScreen = {
  /** What is wrong, in a sentence somebody who did not build this can act on. */
  why: string;
  /** The one thing that changes it, when there is one. */
  fix?: string;
};

/**
 * What stops the toolbar working, if anything does.
 *
 * Two answers, and the second is the one that matters.
 *
 * **No display at all** is the ordinary case on a server or in a container, and it fails
 * honestly on its own — this only says so earlier and more clearly.
 *
 * **A Wayland session** does not. Everything this toolbar does to the rest of the screen
 * it does through X11: it shapes its own window, photographs regions, and asks which
 * window is in front. Under Wayland, XWayland answers all three — and answers them only
 * about XWayland's own clients. A native Wayland window is not in the answer. So the
 * toolbar starts, draws, and lets somebody mark a window it cannot see: the capture comes
 * back without it, "which window is this" names something else, and the mark that reaches
 * the agent is quietly about the wrong thing.
 *
 * That is worse than not starting. A tool that refuses is a tool somebody can work
 * around; a tool that is confidently wrong about what it photographed costs them the
 * conversation they were trying to have. Compositors differ in how much leaks through,
 * which is an argument for refusing rather than against — "sometimes right" is not a
 * property worth shipping.
 *
 * Checked before `DISPLAY`, because a Wayland session sets `DISPLAY` too. That is exactly
 * how this went unnoticed: the check that existed passed.
 */
export function screenTrouble(
  env: NodeJS.ProcessEnv = process.env,
  platform: NodeJS.Platform = process.platform,
): NoScreen | null {
  // Only Linux has this question. Elsewhere a desktop is not optional, and the toolbar
  // does not run on those platforms yet anyway.
  if (platform !== "linux") {
    return null;
  }

  const session = (env.XDG_SESSION_TYPE ?? "").toLowerCase();
  if (session === "wayland" || (env.WAYLAND_DISPLAY ?? "") !== "") {
    return {
      why: "this is a Wayland session, and the toolbar can only see X11 windows on one. It would photograph and point at the wrong things rather than say so.",
      fix: "Log out, and at the login screen choose the X11 session — on Ubuntu it is the gear beside the Sign In button, marked “Ubuntu on Xorg”.",
    };
  }

  if (!env.DISPLAY) {
    return {
      why: "there is no screen to draw on — DISPLAY is not set.",
      fix: "Run OpenClaw where the desktop is, or set DISPLAY to the one it should use.",
    };
  }

  return null;
}
