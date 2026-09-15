// colai: point at anything on screen and hand it to an agent.
//
// The toolbar is a desktop program — an X11 overlay that shapes itself, photographs what
// is behind it and reads which window is in front. None of that can live inside a Node
// plugin or a Control UI page, so this plugin does not contain the toolbar: it builds it
// on install and runs it while OpenClaw is running.
//
// Nothing here asks anybody to configure a Gateway. The toolbar finds one by asking the
// `openclaw` CLI — the same CLI that installed this plugin — and that call installs and
// starts the service if it is not up yet. Somebody who runs `openclaw plugins install`
// should get a toolbar, not a form.

import { existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { buildPluginConfigSchema, definePluginEntry } from "openclaw/plugin-sdk/plugin-entry";
import { z } from "zod";
import { notWhatWasBuilt } from "./src/digest.js";
import { binaryUnder, buildFor, noBuildFor, whereTheBuildIs } from "./src/platform.js";
import { whereabouts } from "./src/running.js";
import { screenTrouble } from "./src/screen.js";
import { Toolbar } from "./src/toolbar-process.js";
import { layOutTheToolbar } from "./src/unpack.js";

/**
 * The package root, whichever file is running.
 *
 * In a checkout this module IS `index.ts` at the root; installed from npm it is
 * `dist/index.js`, one level down. Everything below is addressed from the package —
 * the toolbar's sources, the build script — so a `here` that means two different
 * places depending on how the plugin was obtained is a plugin that works in
 * development and points at nothing on anybody else's machine.
 */
function packageRoot(): string {
  let at = dirname(fileURLToPath(import.meta.url));
  while (!existsSync(join(at, "package.json"))) {
    const up = dirname(at);
    if (up === at) {
      throw new Error("colai: could not find the plugin package root");
    }
    at = up;
  }
  return at;
}

const here = packageRoot();

const ColaiConfigSchema = z.strictObject({
  /**
   * Whether the toolbar appears when OpenClaw does.
   *
   * On, because that is what it is for: a toolbar nobody can see is a toolbar nobody
   * uses, and it is how this behaved when it lived inside the desktop app. Off leaves it
   * installed and built, waiting to be started by hand.
   */
  autostart: z.boolean().optional(),

  /**
   * The keys that bring the toolbar up from anywhere.
   *
   * Every other shortcut the rail advertises is a single letter the page handles, and the
   * page only hears a key once the overlay holds the keyboard — which it takes when a
   * panel opens and at no other time. So the letters were unreachable from the desktop,
   * which is exactly where somebody is standing when they want to mark something.
   *
   * Configurable because a chord is the one setting that can collide with a desktop
   * nobody here can see. Left unset it is Ctrl+Alt+Space, and a binding another program
   * already holds is reported rather than silently doing nothing.
   */
  hotkey: z.string().optional(),
});

const configSchema = buildPluginConfigSchema(ColaiConfigSchema);

/**
 * Where the toolbar is.
 *
 * `bin/` is the one that matters: OpenClaw installs plugins with `--ignore-scripts`,
 * always and with no way to opt in, so nothing a package says can make it compile on
 * the installing machine. The toolbar therefore travels already built, staged into
 * `bin/` before the package is packed.
 *
 * The two `target/` paths are for working on the toolbar itself, where `cargo build`
 * has just put one there and staging it first would only add a copy.
 */
/**
 * Where the toolbar writes what it has to say.
 *
 * Beside OpenClaw's own state rather than in the package, because a plugin directory is
 * replaced wholesale on update and a log that disappears when you upgrade is a log that
 * is never there when it is wanted.
 */
function logFile(stateDir: string): string {
  return join(stateDir, "logs", "colai-toolbar.log");
}

/**
 * The toolbar this machine should run, laid out and ready to spawn.
 *
 * Three places, in order, and the order is the point:
 *
 *  1. The platform package npm installed — `@colai/toolbar-linux-x64` and friends. This is
 *     what every real install uses. The binary arrives gzipped and is unpacked on first
 *     use; `unpack.ts` says why.
 *  2. `platforms/<name>/bin/` in a checkout, where `build-release.mjs` stages it.
 *  3. `target/release` or `target/debug`, for somebody who has just run `cargo build`.
 *
 * The three ways this returns null are three different situations and get three different
 * sentences, because "nothing happened" sends a person looking in the wrong place. A
 * machine with no build at all is not a broken install, and saying so is the difference
 * between somebody filing a bug and somebody understanding.
 */
function toolbarBinary(): string | null {
  const build = buildFor();
  if (!build) {
    console.error(noBuildFor());
    return null;
  }

  const roots = [whereTheBuildIs(import.meta.url), join(here, "platforms", archOf(build))];
  for (const root of roots) {
    if (!root) {
      continue;
    }
    const { binary, archive } = binaryUnder(root);
    if (existsSync(binary)) {
      return binary;
    }
    if (existsSync(archive)) {
      const trouble = layOutTheToolbar(archive, binary);
      if (trouble) {
        console.error(`The colai toolbar ${trouble}`);
        return null;
      }
      return binary;
    }
  }

  // Straight out of `cargo build`, which stages nothing and needs no unpacking.
  const built = [
    join(here, "toolbar/src-tauri/target/release/colai-toolbar"),
    join(here, "toolbar/src-tauri/target/debug/colai-toolbar"),
  ].find((path) => existsSync(path));
  if (built) {
    return built;
  }

  console.error(`The colai toolbar is installed but ${build.package} is not.`);
  console.error("npm skips an optional dependency it cannot fetch, and this is that.");
  console.error("Reinstall it: openclaw plugins install @colai/toolbar --force");
  return null;
}

/** `@colai/toolbar-linux-x64` → `linux-x64`, which is what the checkout directory is called. */
function archOf(build: { package: string }): string {
  return build.package.slice("@colai/toolbar-".length);
}

export default definePluginEntry({
  id: "colai",
  name: "colai",
  description: "Point at anything on screen and hand it to an agent.",
  configSchema,
  register(api) {
    const parsed = ColaiConfigSchema.safeParse(api.pluginConfig ?? {});
    if (!parsed.success) {
      throw new Error(
        `Invalid colai plugin config: ${parsed.error.issues[0]?.message ?? "invalid config"}`,
      );
    }
    const autostart = parsed.data.autostart ?? true;
    const hotkey = parsed.data.hotkey;

    // `openclaw colai show|hide|toggle|status` — the whole control surface outside the
    // toolbar's own window, and the only one a plugin can offer without changing
    // OpenClaw. Anything can run a command: a terminal, a launcher, a shortcut.
    api.registerCli(
      async ({ program }) => {
        const { registerColaiCli } = await import("./src/cli.js");
        registerColaiCli(program, toolbarBinary);
      },
      {
        descriptors: [
          {
            name: "colai",
            description: "Show the colai toolbar, or put it away",
            hasSubcommands: true,
          },
        ],
      },
    );

    /** The toolbar this service is responsible for, once it knows where its state lives. */
    let toolbar: Toolbar | null = null;

    api.registerService({
      id: "colai-toolbar",
      // Turning autostart off should put the toolbar away, not wait for a restart.
      // The hotkey too: it is read when the toolbar starts, so changing it has to
      // restart the toolbar or the setting would appear to do nothing until a reboot.
      reload: {
        configPrefixes: [
          "plugins.entries.colai.config.autostart",
          "plugins.entries.colai.config.hotkey",
        ],
      },
      start(ctx) {
        if (!autostart) {
          ctx.logger.info("colai: autostart is off, so the toolbar is not being started.");
          return;
        }
        /*
         * Somewhere to draw, and somewhere this toolbar can see.
         *
         * Two different refusals, and only one of them is obvious. No display at all is
         * the ordinary case on a server or in a container. A Wayland session is not: it
         * sets DISPLAY, passes every check this used to make, and then shows the toolbar
         * a desktop with most of the windows missing from it. `screenTrouble` holds both,
         * and why.
         */
        const noScreen = screenTrouble();
        if (noScreen) {
          ctx.logger.warn(`colai: ${noScreen.why} The toolbar is not being started.`);
          if (noScreen.fix) {
            ctx.logger.warn(`colai: ${noScreen.fix}`);
          }
          return;
        }
        const binary = toolbarBinary();
        if (!binary) {
          ctx.logger.warn(
            "colai: the toolbar was not installed. Reinstall the plugin: `openclaw plugins install @colai/toolbar --force`.",
          );
          return;
        }
        const swapped = notWhatWasBuilt(binary, buildFor()?.package ?? null);
        if (swapped) {
          ctx.logger.error(`colai: ${swapped}`);
          return;
        }
        toolbar = new Toolbar(binary, whereabouts());
        toolbar.start(logFile(ctx.stateDir), ctx.logger, hotkey);
      },
      stop() {
        return toolbar?.stop() ?? Promise.resolve();
      },
    });
  },
});
