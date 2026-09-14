// `openclaw colai` — show the toolbar, put it away, or say where it stands.
//
// `toggle` is the control surface for everything that is not the tray icon: a terminal,
// a launcher, a keyboard shortcut, a script. It matters most where the tray is not there
// to help — a GNOME session with no AppIndicator extension shows no icon at all, and then
// this is the only way back after Escape.
//
// Reaching the running toolbar costs nothing here. It refuses to run twice, and a second
// launch hands its arguments to the copy already on screen — so "tell the toolbar
// something" and "start the toolbar" are the same command, and there is no socket, port
// or protocol between them.
//
// Status lives here rather than in a doctor check because doctor cannot ask:
// `registerBundledHealthChecks` names five bundled plugins and has no seam for an
// installed one, and a plugin's `register` does not run in the doctor process at all.
// A plugin's own command does load the plugin, so this works from an ordinary terminal.

import { spawn } from "node:child_process";
import { existsSync, statSync } from "node:fs";
import type { OpenClawPluginApi } from "openclaw/plugin-sdk/plugin-entry";
import { notWhatWasBuilt } from "./digest.js";
import { toolbarOnScreen, whereabouts } from "./running.js";
import { screenTrouble } from "./screen.js";

type CliProgram = Parameters<Parameters<OpenClawPluginApi["registerCli"]>[0]>[0]["program"];

function megabytes(path: string): string {
  try {
    return `${Math.round(statSync(path).size / 1_000_000)}MB`;
  } catch {
    // Raced an update that unlinked it. The size is a courtesy, not the answer.
    return "size unknown";
  }
}

/** How long to wait for a toolbar to fall over before deciding it is not going to. */
const FIRST_BREATH = 600;

/**
 * Hand one word to the toolbar, starting it if it is not up.
 *
 * Detached, because this returns a terminal to the person who opened it: the toolbar
 * outlives the command that spoke to it, and holding the terminal until a window closes
 * would be the wrong shape for both callers.
 *
 * But not unwatched. A binary that cannot run at all — missing its libraries, built
 * against a newer libc than this machine has, the wrong architecture entirely — fails
 * within milliseconds, and this used to fail into silence: `stdio: "ignore"` threw the
 * loader's explanation away, nothing listened for the failure, and the command still
 * reported success. Somebody whose toolbar never appeared had nothing at all to go on.
 *
 * So the first breath is watched. Beyond it the toolbar is on its own, which is the
 * point of detaching it.
 */
async function tellTheToolbar(
  binary: string,
  word: "show" | "hide" | "toggle" | "quit",
): Promise<void> {
  // The pidfile travels with every word, so a toolbar started from here records itself
  // the same way one started by the plugin does.
  const started = spawn(binary, [word, "--pidfile", whereabouts()], {
    detached: true,
    stdio: ["ignore", "ignore", "pipe"],
  });

  // Kept rather than printed as it arrives, so a toolbar that starts normally and warns
  // about something does not scribble over the terminal it just handed back.
  let complaint = "";
  started.stderr?.on("data", (chunk: Buffer) => {
    complaint += String(chunk);
  });

  const fell = await new Promise<string | null>((answer) => {
    const enough = setTimeout(() => answer(null), FIRST_BREATH);
    // Without this listener a spawn that fails outright — the file vanished between the
    // check and the call, or is not executable — reaches Node as an unhandled error
    // event, which does not warn: it throws, out of a command that was asked to open a
    // window.
    started.once("error", (error: Error) => {
      clearTimeout(enough);
      answer(error.message);
    });
    started.once("exit", (code, signal) => {
      clearTimeout(enough);
      // Exiting cleanly is what a handoff looks like: a second copy hands its word to
      // the one already on screen and stops. That is the success, not a failure.
      answer(signal === null && code === 0 ? null : `it exited with ${signal ?? code}`);
    });
  });

  if (fell === null) {
    // Nothing more to hear. The pipe is a handle, and a handle nobody closes keeps this
    // process alive after the toolbar it started no longer needs it.
    started.stderr?.destroy();
    started.unref();
    return;
  }

  console.error(`The colai toolbar did not start — ${fell}.`);
  if (complaint.trim()) {
    console.error(complaint.trim());
  } else {
    console.error("It wrote nothing to explain why. Run it directly to see what it says:");
    console.error(`  ${binary} ${word}`);
  }
  process.exitCode = 1;
}

export function registerColaiCli(program: CliProgram, toolbarBinary: () => string | null): void {
  const colai = program.command("colai").description("Show the colai toolbar, or put it away");

  /** Every subcommand needs the binary, and none of them can do anything without it. */
  const theToolbar = (): string | null => {
    const binary = toolbarBinary();
    if (binary && existsSync(binary)) {
      /*
       * The same gate the service start uses.
       *
       * It was only on the service path, so `openclaw colai show` spawned the binary
       * without ever asking whether it was the one that was built — which made the check
       * something to step around by using the documented command instead of the automatic
       * one. One gate, both doors.
       */
      const wrong = notWhatWasBuilt(binary);
      if (wrong) {
        console.error(`The colai toolbar ${wrong}`);
        process.exitCode = 1;
        return null;
      }
      return binary;
    }
    console.error("The colai toolbar is not installed.");
    console.error("The package ships it already built, so this means the install did not finish.");
    console.error("Reinstall it: openclaw plugins install @colai/toolbar --force");
    process.exitCode = 1;
    return null;
  };

  for (const [word, description] of [
    ["show", "Put the toolbar on screen"],
    ["hide", "Take the toolbar off screen"],
    ["toggle", "Put the toolbar on screen, or take it off"],
    ["quit", "Close the toolbar entirely"],
  ] as const) {
    colai
      .command(word)
      .description(description)
      .action(async () => {
        // Only the words that would put it on screen. `hide` and `quit` are about a
        // toolbar that is already running, and refusing those would strand it.
        if (word === "show" || word === "toggle") {
          const noScreen = screenTrouble();
          if (noScreen) {
            console.error(`The colai toolbar cannot run here — ${noScreen.why}`);
            if (noScreen.fix) {
              console.error(noScreen.fix);
            }
            process.exitCode = 1;
            return;
          }
        }
        const binary = theToolbar();
        if (binary) {
          await tellTheToolbar(binary, word);
        }
      });
  }

  colai
    .command("status", { isDefault: true })
    .description("Show whether the toolbar is installed and on screen")
    .action(() => {
      const binary = theToolbar();
      if (!binary) {
        return;
      }
      console.log(`Toolbar: ${binary} (${megabytes(binary)})`);

      // The same question the plugin asks before starting it, answered the same way, so
      // status and autostart cannot disagree about whether this machine can run it.
      const noScreen = screenTrouble();
      if (noScreen) {
        console.log(`Screen:  no — ${noScreen.why}`);
        if (noScreen.fix) {
          console.log(`         ${noScreen.fix}`);
        }
        return;
      }
      console.log(`Screen:  ${process.env.DISPLAY || process.platform}`);

      const found = toolbarOnScreen(whereabouts());
      if (found === null) {
        console.log("Running: no.");
        console.log("Put it on screen: openclaw colai show");
        return;
      }
      // Running is all this can honestly say. Whether the overlay is drawn right now is
      // inside that process — asking X would mean a window-manager dependency to answer
      // a question `toggle` already answers correctly by asking the window itself.
      console.log(`Running: yes (pid ${found.pid}).`);
      console.log("On screen or put away: openclaw colai toggle");
    });
}
