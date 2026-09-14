// Is a toolbar running, and which one?
//
// Asked of a file the toolbar itself writes, not of the process table. It used to scan
// every entry in `/proc` for one whose `exe` matched the binary — with a Linux-kernel
// `" (deleted)"` suffix to survive in-place updates, and a path-tail heuristic to
// recognise a copy from a previous install generation. All of that was Linux, and none of
// it was necessary: the process that knows where it is can simply say so.
//
// The toolbar writes its pid here at startup and removes the file on the way out, so a
// file that is still present names a pid worth asking about. It is asked about, because a
// crash leaves the file behind and the system may since have given that pid to somebody
// else — and the plugin used to stop what it found, which makes being wrong expensive.

import { readFileSync, rmSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

/** A toolbar somebody is looking at. */
export type ToolbarOnScreen = { pid: number };

/**
 * Where the toolbar is told to say it is.
 *
 * Somewhere that does not move. It cannot live beside the binary: OpenClaw puts every
 * install in its own generation directory, so an update would leave the running toolbar
 * writing to a path the new plugin no longer looks at. And it cannot come from the
 * service's `stateDir`, because `openclaw colai` runs in a process that has no service
 * context — the CLI has to reach the same file.
 *
 * So it is the toolbar's own per-user directory, named for the same identifier the app
 * bundle uses, computed the same way on both sides and passed to the toolbar explicitly
 * rather than derived twice.
 */
export function whereabouts(): string {
  return join(perUserDirectory(), "ai.colai.toolbar", "colai-toolbar.pid");
}

/** Where this desktop keeps a program's own files. */
function perUserDirectory(): string {
  if (process.platform === "win32") {
    return process.env.APPDATA || join(homedir(), "AppData", "Roaming");
  }
  if (process.platform === "darwin") {
    return join(homedir(), "Library", "Application Support");
  }
  return process.env.XDG_CONFIG_HOME || join(homedir(), ".config");
}

/**
 * The toolbar that is up, if one is.
 *
 * `process.kill(pid, 0)` sends nothing; it asks whether the pid exists and whether we may
 * signal it. That is the whole check, and it is the same on every platform.
 */
export function toolbarOnScreen(pidfile: string): ToolbarOnScreen | null {
  let pid: number;
  try {
    pid = Number(readFileSync(pidfile, "utf8").trim());
  } catch {
    // No file is the ordinary answer: no toolbar.
    return null;
  }
  if (!Number.isInteger(pid) || pid <= 0) {
    return null;
  }
  try {
    process.kill(pid, 0);
    return { pid };
  } catch {
    // Left behind by a crash. Cleared so the next look is a quick one.
    try {
      rmSync(pidfile, { force: true });
    } catch {
      // Somebody else's to clean up, then.
    }
    return null;
  }
}
