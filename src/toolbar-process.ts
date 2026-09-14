// Putting the toolbar on the screen, and taking it off again.
//
// Its own module because it is the riskiest part of the plugin and the only part with a
// real process in it: everything else here decides something, this one starts and stops a
// window somebody is looking at.
//
// Both directions go through the toolbar's own front door. Running the binary again hands
// the arguments to the copy already on screen — that is how `openclaw colai toggle`
// works — so starting and stopping are the same mechanism, and it is the same mechanism
// on every platform. Signalling a pid was neither: finding the pid meant reading `/proc`,
// and on Windows Node maps SIGTERM to `TerminateProcess`, which gives a window somebody
// is looking at no chance to put anything down.

import { spawn } from "node:child_process";
import { mkdirSync, openSync } from "node:fs";
import { dirname } from "node:path";
import { toolbarOnScreen } from "./running.js";

/** How long a toolbar gets to go before it is given up on. */
const STOP_PATIENCE = 3_000;

/** How often to look, while waiting for it to go. */
const LOOK_AGAIN = 50;

/** Enough of a logger to say what happened; the Gateway's own, in practice. */
type Says = {
  info: (message: string) => void;
  warn: (message: string) => void;
};

/** One toolbar, from the moment it is asked for to the moment it is gone. */
export class Toolbar {
  constructor(
    private readonly binary: string,
    private readonly pidfile: string,
  ) {}

  /**
   * Put it on the screen.
   *
   * Spawned whether or not one is already up: a second copy hands its argument to the
   * first and exits, so this is also how an orphan left by a Gateway that was killed
   * rather than stopped gets told to show itself. Nothing has to recognise it first,
   * which is the whole reason the adoption logic that used to live here is gone.
   */
  start(log: string, says: Says, hotkey?: string): void {
    const already = toolbarOnScreen(this.pidfile);

    let sink: number | "ignore" = "ignore";
    try {
      mkdirSync(dirname(log), { recursive: true });
      sink = openSync(log, "a");
    } catch (error) {
      // A log nobody can write is not a reason to withhold the toolbar.
      says.warn(`colai: could not open ${log}: ${String(error)}`);
    }

    // Detached: this is a window somebody looks at for hours, and closing the Gateway
    // should not take it off the screen mid-sentence.
    const started = spawn(this.binary, ["show", "--pidfile", this.pidfile], {
      detached: true,
      stdio: ["ignore", sink, sink],
      // The binding travels in the environment rather than on the command line: it is a
      // setting, and the arguments here are words the toolbar acts on. Absent means the
      // toolbar's own default, which is the whole of the behaviour when nobody has
      // chosen — so an unset option and an empty string mean the same thing.
      env: hotkey ? { ...process.env, COLAI_HOTKEY: hotkey } : process.env,
    });
    started.unref();
    // Without this a spawn that fails outright — the binary vanished under us, or is not
    // executable — reaches Node as an unhandled error event, which does not warn: it
    // throws, inside the Gateway, out of a plugin that was only asked to open a window.
    started.once("error", (error: Error) => {
      says.warn(`colai: the toolbar could not be started — ${error.message}`);
    });
    started.once("exit", (code, signal) => {
      // Exiting straight away is what a handoff looks like, and a handoff is a success —
      // the toolbar somebody can see is the one that was already there.
      const handedOff = signal === null && code === 0;
      if (!handedOff) {
        says.warn(`colai: the toolbar exited (${signal ?? code}). Its log is ${log}`);
      }
    });
    says.info(
      already
        ? `colai: a toolbar was already running (pid ${already.pid}); it was asked to show itself.`
        : `colai: toolbar started. Its log is ${log}`,
    );
  }

  /**
   * Take it off the screen, and do not come back until it has gone.
   *
   * The wait is the point. The toolbar holds a single-instance lock until it actually
   * exits, so a restart that spawns before then hands its arguments to the dying copy,
   * that copy exits too, and nobody is left on screen — with "toolbar started" in the log.
   */
  async stop(): Promise<void> {
    const going = toolbarOnScreen(this.pidfile);
    if (!going) {
      return;
    }
    // Asked, not signalled. It puts itself down, the same way on every platform.
    const asked = spawn(this.binary, ["quit", "--pidfile", this.pidfile], {
      detached: true,
      stdio: "ignore",
    });
    // Same reason as in `start`: an unheard error event is thrown, not logged. Nothing to
    // say here beyond not throwing — the wait below is what decides whether it worked,
    // and it ends in a SIGKILL either way.
    asked.once("error", () => {});
    asked.unref();

    const until = Date.now() + STOP_PATIENCE;
    while (Date.now() < until) {
      if (!toolbarOnScreen(this.pidfile)) {
        return;
      }
      await new Promise((soon) => setTimeout(soon, LOOK_AGAIN));
    }
    // It had its chance. A toolbar that will not go is worse than one that is made to.
    try {
      process.kill(going.pid, "SIGKILL");
    } catch {
      // Raced us to it.
    }
  }
}
