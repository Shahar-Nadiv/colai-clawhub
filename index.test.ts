// The plugin entry, loaded and registered the way the Gateway loads it.
//
// Everything else about colai is checked by reading files. This is the one test that
// runs `register`, because the failure it protects against — a plugin that installs,
// loads, and quietly registers nothing — leaves no other trace.
import { createHash } from "node:crypto";
import { chmodSync, existsSync, mkdtempSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { gzipSync } from "node:zlib";
import { describe, expect, it as test } from "vitest";
import colai from "./index.js";
import { notWhatWasBuilt } from "./src/digest.js";
import { withoutSomebodyElsesLibraries } from "./src/environment.js";
import { toolbarOnScreen } from "./src/running.js";
import { screenTrouble } from "./src/screen.js";
import { Toolbar } from "./src/toolbar-process.js";
import { layOutTheToolbar } from "./src/unpack.js";
import { createTestPluginApi } from "./test/plugin-api.js";

type Service = { id: string; start: (ctx: unknown) => void; stop?: (ctx: unknown) => void };
type CliRegistration = { descriptors?: { name: string }[] };

/** Register the plugin against a host that records what it was handed. */
function registerColai(config: Record<string, unknown> = {}) {
  const services: Service[] = [];
  const commands: string[] = [];
  const said: string[] = [];
  const logger = {
    info: (message: string) => said.push(message),
    warn: (message: string) => said.push(message),
    error: (message: string) => said.push(message),
    debug: () => {},
  };
  colai.register?.(
    createTestPluginApi({
      id: "colai",
      pluginConfig: config,
      logger,
      registerService: (service) => services.push(service as Service),
      registerCli: (_registrar, options) => {
        for (const descriptor of (options as CliRegistration)?.descriptors ?? []) {
          commands.push(descriptor.name);
        }
      },
    }),
  );
  return {
    services,
    commands,
    said,
    ctx: { logger, stateDir: mkdtempSync(join(tmpdir(), "colai-state-")) },
  };
}

describe("the plugin OpenClaw loads", () => {
  test("it registers one service and one command", () => {
    const { services, commands } = registerColai();
    expect(services.map((service) => service.id)).toEqual(["colai-toolbar"]);
    // `openclaw colai` is how anybody outside the Gateway finds out whether the toolbar
    // is there. Without it there is no answer at all: doctor cannot load this plugin.
    expect(commands).toEqual(["colai"]);
  });

  test("the reload declaration names the setting it reloads for", () => {
    /*
     * Every option that the toolbar reads only at startup has to be named here, or it is
     * a setting that appears to do nothing until the next Gateway restart — exactly the
     * shape of bug nobody reports. `autostart` moves the toolbar; `hotkey` is read from
     * the environment when the process is spawned, so changing it needs the same
     * treatment.
     *
     * Checked against the manifest rather than written out twice: a prefix that does not
     * match a declared option is a path to nothing.
     */
    const { services } = registerColai();
    const service = services[0] as Service & { reload?: { configPrefixes?: string[] } };
    const declared = JSON.parse(
      readFileSync(new URL("./openclaw.plugin.json", import.meta.url), "utf8"),
    ) as { id: string; configSchema?: { properties?: Record<string, unknown> } };
    const options = Object.keys(declared.configSchema?.properties ?? {});

    expect(options).toContain("autostart");
    expect(options).toContain("hotkey");
    expect(service.reload?.configPrefixes).toEqual(
      options.map((option) => `plugins.entries.${declared.id}.config.${option}`),
    );
  });

  test("autostart off leaves the toolbar alone, and says so", () => {
    const { services, said, ctx } = registerColai({ autostart: false });
    services[0]?.start(ctx);
    expect(said.join("\n")).toContain("autostart is off");
  });

  test("with no screen it starts nothing rather than failing into one", () => {
    /*
     * The overlay is X11. On a headless host — a server, a container, a CI runner — the
     * honest outcome is a line in the log, not a crash inside a window system that was
     * never there.
     */
    const was = { ...process.env };
    delete process.env.DISPLAY;
    delete process.env.WAYLAND_DISPLAY;
    delete process.env.XDG_SESSION_TYPE;
    try {
      const { services, said, ctx } = registerColai();
      services[0]?.start(ctx);
      expect(said.join("\n")).toContain("DISPLAY is not set");
      expect(said.join("\n"), "and it is not started anyway").toContain("not being started");
    } finally {
      Object.assign(process.env, was);
    }
  });

  test("on Wayland it refuses rather than pointing at the wrong windows", () => {
    /*
     * The failure this replaces was not a crash. XWayland sets DISPLAY, so the toolbar
     * started, drew, and let somebody mark a window it could not see — X11 answers only
     * about XWayland's own clients, so a native Wayland window is absent from the capture
     * and is not what "which window is in front" names. The mark reaching the agent was
     * quietly about something else.
     *
     * Refusing is the honest outcome, and the message has to name the session type or the
     * person has no idea which of their machine's many properties is the problem.
     */
    const was = { ...process.env };
    process.env.DISPLAY = ":0";
    process.env.XDG_SESSION_TYPE = "wayland";
    try {
      const { services, said, ctx } = registerColai();
      services[0]?.start(ctx);
      expect(said.join("\n")).toContain("Wayland");
      expect(said.join("\n"), "with the one thing that changes it").toContain("Xorg");
      expect(said.join("\n")).toContain("not being started");
    } finally {
      Object.assign(process.env, was);
    }
  });

  test("stopping before anything started is not an error", async () => {
    const { services, ctx } = registerColai({ autostart: false });
    await expect(services[0]?.stop?.(ctx)).resolves.toBeUndefined();
  });
});

const sha = (bytes: string) => createHash("sha256").update(bytes).digest("hex");

describe("what gets spawned is what was built", () => {
  /*
   * The package ships a native program that runs as the user, photographs the screen and
   * authenticates to the Gateway, and npm proves nothing about what is inside a tarball.
   * So the build writes a digest beside the binary and the plugin checks it before
   * anything runs.
   *
   * Not a signature — whoever can replace the binary can replace the digest next to it.
   * What it closes is the narrower case, an artifact tampered with in transit or on disk,
   * and it turns a silent substitution into a refusal.
   */
  function staged(bytes: string, digest?: string): string {
    const dir = mkdtempSync(join(tmpdir(), "colai-digest-"));
    const binary = join(dir, "colai-toolbar");
    writeFileSync(binary, bytes);
    if (digest !== undefined) {
      writeFileSync(`${binary}.sha256`, `${digest}\n`);
    }
    return binary;
  }

  test("a binary that matches its digest is allowed through", () => {
    expect(notWhatWasBuilt(staged("toolbar", sha("toolbar")))).toBeNull();
  });

  test("a swapped binary is refused, and says both digests", () => {
    const refusal = notWhatWasBuilt(staged("swapped", sha("toolbar")));
    expect(refusal).toContain("does not match the digest");
    expect(refusal).toContain(sha("swapped"));
    expect(refusal).toContain(sha("toolbar"));
  });

  test("a developer build with no digest beside it still runs", () => {
    // Straight out of `target/`, never staged, nothing to compare against. Refusing it
    // would mean refusing to run the thing somebody just compiled.
    expect(notWhatWasBuilt(staged("freshly compiled"))).toBeNull();
  });
});

describe("the toolbar is not handed somebody else's libraries", () => {
  /*
   * Found by installing the published package into a Gateway started from inside a
   * snap-packaged editor, which is an ordinary thing to do on Ubuntu. The toolbar died
   * before drawing with `symbol lookup error: /snap/core20/.../libpthread.so.0:
   * undefined symbol: __libc_pthread_init, version GLIBC_PRIVATE` and exit 127 — no
   * window, and nothing in that sentence a person could act on.
   *
   * Nothing was missing and nothing was mispackaged: the same binary started normally
   * with those variables gone. A snap rewrites where GTK modules, GIO modules, locales
   * and libraries are found, everything it launches inherits that, and a system GTK
   * program handed a snap's libraries against the system libc does not survive it.
   */
  const snapped = {
    DISPLAY: ":1",
    HOME: "/home/somebody",
    LD_LIBRARY_PATH: "/snap/code/263/usr/lib/x86_64-linux-gnu",
    GTK_PATH: "/snap/code/263/usr/lib/x86_64-linux-gnu/gtk-3.0",
    LOCPATH: "/snap/code/263/usr/lib/locale",
    GIO_MODULE_DIR: "/snap/code/263/usr/lib/x86_64-linux-gnu/gio/modules",
  };

  test("a snap's loader paths are dropped", () => {
    const given = withoutSomebodyElsesLibraries(snapped);
    for (const name of ["LD_LIBRARY_PATH", "GTK_PATH", "LOCPATH", "GIO_MODULE_DIR"]) {
      expect(given[name], `${name} points into a snap`).toBeUndefined();
    }
  });

  test("everything the toolbar actually reads survives", () => {
    // It needs the screen and the home directory to start at all, and the Gateway's own
    // variables to find the Gateway. Emptying the environment would be a worse bug.
    const given = withoutSomebodyElsesLibraries(snapped);
    expect(given.DISPLAY).toBe(":1");
    expect(given.HOME).toBe("/home/somebody");
  });

  test("the same names are left alone when they point somewhere real", () => {
    /*
     * Dropped by value, not by name. Plenty of people set `LD_LIBRARY_PATH` on purpose —
     * this machine has a ROS workspace in it — and a plugin that silently deletes it
     * would be making a different mess for the same reason.
     */
    const deliberate = { LD_LIBRARY_PATH: "/opt/ros/jazzy/lib", GTK_PATH: "/usr/lib/gtk-3.0" };
    expect(withoutSomebodyElsesLibraries(deliberate)).toEqual(deliberate);
  });

  test("a snap on the end of a real list still counts", () => {
    // Snap wrappers prepend rather than replace, so the poison is usually one entry
    // among several and a whole-value comparison would miss it.
    const mixed = { LD_LIBRARY_PATH: "/opt/ros/jazzy/lib:/snap/core20/current/lib" };
    expect(withoutSomebodyElsesLibraries(mixed).LD_LIBRARY_PATH).toBeUndefined();
  });

  test("a path merely mentioning snap is not a snap path", () => {
    // `/home/me/snapshots` is not `/snap/`. Matching the word anywhere would take it.
    const innocent = { LD_LIBRARY_PATH: "/home/me/snapshots/lib:/usr/lib/unsnapped" };
    expect(withoutSomebodyElsesLibraries(innocent)).toEqual(innocent);
  });

  test("both doors use it — the Gateway's and the command's", () => {
    // The plugin spawns the toolbar on autostart and the CLI spawns it on `show`. A fix
    // on one is a toolbar that works until somebody types the documented command.
    for (const file of ["src/toolbar-process.ts", "src/cli.ts"]) {
      const source = readFileSync(new URL(file, import.meta.url), "utf8");
      expect(source, `${file} spawns with the cleaned environment`).toContain(
        "withoutSomebodyElsesLibraries",
      );
    }
  });
});

describe("the toolbar is laid out before it is spawned", () => {
  /*
   * The binary ships compressed and is unpacked on first use. Two reasons this is worth
   * testing rather than trusting: it writes an executable, and it is the one path where
   * two callers can arrive at once — the CLI and the autostart both ask for the toolbar.
   */
  function packed(bytes: string): { archive: string; binary: string } {
    const dir = mkdtempSync(join(tmpdir(), "colai-unpack-"));
    const binary = join(dir, "colai-toolbar");
    const archive = `${binary}.gz`;
    writeFileSync(archive, gzipSync(Buffer.from(bytes)));
    return { archive, binary };
  }

  test("the archive becomes an executable with the bytes that went in", () => {
    const { archive, binary } = packed("a whole toolbar");
    expect(layOutTheToolbar(archive, binary)).toBeNull();
    expect(readFileSync(binary, "utf8")).toBe("a whole toolbar");
    // Without this the plugin installs, unpacks, and then cannot spawn what it unpacked.
    expect(statSync(binary).mode & 0o111).toBeTruthy();
  });

  test("a binary already there is left exactly as it is", () => {
    // Every run after the first, and the case where somebody has put their own build
    // there on purpose. Re-unpacking would silently overwrite it.
    const { archive, binary } = packed("shipped");
    writeFileSync(binary, "the one already here");
    expect(layOutTheToolbar(archive, binary)).toBeNull();
    expect(readFileSync(binary, "utf8")).toBe("the one already here");
  });

  test("a corrupt archive is refused by name, not left half written", () => {
    const { archive, binary } = packed("fine");
    writeFileSync(archive, "not gzip at all");
    expect(layOutTheToolbar(archive, binary)).toContain("could not unpack");
    // The gate after this hashes whatever is on disk, so a partial file here would be
    // caught — but it would be caught as "wrong digest", which reads like tampering.
    expect(existsSync(binary)).toBe(false);
  });

  test("nowhere to write says so, and says what to do about it", () => {
    const { archive, binary } = packed("a whole toolbar");
    const refusal = layOutTheToolbar(archive, join(binary, "beneath-a-file", "colai-toolbar"));
    expect(refusal).toContain("could not write the toolbar");
    expect(refusal).toContain("writable");
  });

  test("what is unpacked is what the digest gate then checks", () => {
    // The two halves meet here: unpack writes the file, `notWhatWasBuilt` hashes it.
    // Nothing verifies the archive itself, on purpose — this is the test that says the
    // bytes still get checked.
    const { archive, binary } = packed("a whole toolbar");
    writeFileSync(`${binary}.sha256`, `${sha("a whole toolbar")}\n`);
    expect(layOutTheToolbar(archive, binary)).toBeNull();
    expect(notWhatWasBuilt(binary)).toBeNull();
  });
});

describe("the toolbar's lifetime, against a real process", () => {
  /*
   * The riskiest code in the plugin, and nothing exercised it before: `start` was only
   * ever tested on the paths that return early, and `stop` only when nothing had started.
   *
   * Both directions go through the toolbar's own front door now — running the binary
   * again hands the word to the copy already on screen. So a stub can play the toolbar by
   * doing the two things the real one does: write its pid where it was told, and remove
   * it when asked to go.
   */
  const quiet = { info: () => {}, warn: () => {} };
  const said: string[] = [];
  const notes = {
    info: (line: string) => said.push(line),
    warn: (line: string) => said.push(line),
  };

  /**
   * A stand-in that behaves like the toolbar in the ways this cares about: it records
   * where it is, it stays up, and `quit` reaches it and takes it away.
   */
  function aFakeToolbar(): { binary: string; pidfile: string } {
    const dir = mkdtempSync(join(tmpdir(), "colai-lifetime-"));
    const binary = join(dir, "colai-toolbar");
    writeFileSync(
      binary,
      [
        "#!/bin/sh",
        'pidfile=""',
        'word="show"',
        "while [ $# -gt 0 ]; do",
        '  case "$1" in',
        '    --pidfile) pidfile="$2"; shift 2 ;;',
        '    show|hide|toggle|quit) word="$1"; shift ;;',
        "    *) shift ;;",
        "  esac",
        "done",
        'up=""',
        'if [ -f "$pidfile" ] && kill -0 "$(cat "$pidfile")" 2>/dev/null; then',
        '  up="$(cat "$pidfile")"',
        "fi",
        // The single-instance handoff, which is the whole reason start and stop are the
        // same mechanism: a second copy hands its word to the first and exits.
        'if [ -n "$up" ]; then',
        '  [ "$word" = "quit" ] && kill "$up" 2>/dev/null',
        "  exit 0",
        "fi",
        // Nothing was up. `quit` has nothing to do; anything else becomes the toolbar.
        '[ "$word" = "quit" ] && exit 0',
        'echo $$ > "$pidfile"',
        `trap 'rm -f "$pidfile"; kill $sleeper 2>/dev/null; exit 0' TERM INT`,
        "sleep 120 &",
        "sleeper=$!",
        "wait $sleeper",
        `rm -f "$pidfile"`,
        "",
      ].join("\n"),
    );
    chmodSync(binary, 0o755);
    return { binary, pidfile: join(dir, "colai-toolbar.pid") };
  }

  const alive = (pid: number): boolean => {
    try {
      process.kill(pid, 0);
      return true;
    } catch {
      return false;
    }
  };

  const settle = (ms = 300) => new Promise((soon) => setTimeout(soon, ms));

  test("stop does not return until the toolbar has actually gone", async () => {
    /*
     * The wait is the point. The toolbar holds a single-instance lock until it exits, so
     * a restart that spawns before then hands its arguments to the dying copy, that copy
     * exits too, and nobody is left on screen — with "toolbar started" in the log.
     */
    const { binary, pidfile } = aFakeToolbar();
    const toolbar = new Toolbar(binary, pidfile);
    toolbar.start(join(mkdtempSync(join(tmpdir(), "colai-log-")), "t.log"), quiet);
    await settle();
    const pid = Number(readFileSync(pidfile, "utf8").trim());
    expect(alive(pid), "it should be up").toBe(true);

    await toolbar.stop();
    expect(alive(pid), "stop returned while it was still running").toBe(false);
  });

  test("a toolbar already up is told to show itself, not spawned over", async () => {
    /*
     * A Gateway killed rather than stopped comes back to find its toolbar still running.
     * Nothing has to recognise it: a second copy hands its word to the first and exits,
     * which is why the adoption logic that used to live here is gone.
     */
    const { binary, pidfile } = aFakeToolbar();
    const first = new Toolbar(binary, pidfile);
    first.start(join(mkdtempSync(join(tmpdir(), "colai-log-")), "t.log"), quiet);
    await settle();
    const pid = Number(readFileSync(pidfile, "utf8").trim());

    said.length = 0;
    const second = new Toolbar(binary, pidfile);
    second.start(join(mkdtempSync(join(tmpdir(), "colai-log-")), "t.log"), notes);
    expect(said.join("\n")).toContain(`already running (pid ${pid})`);

    await second.stop();
    expect(alive(pid), "and the second one can still stop it").toBe(false);
  });

  test("stopping when nothing is up is not an error", async () => {
    const { binary, pidfile } = aFakeToolbar();
    await expect(new Toolbar(binary, pidfile).stop()).resolves.toBeUndefined();
  });

  test("a pidfile left by a crash does not count as a toolbar", () => {
    // The file outlives a process that died without removing it, and the pid may since
    // have been given to somebody else — which matters, because this used to decide what
    // to stop.
    const dir = mkdtempSync(join(tmpdir(), "colai-stale-"));
    const pidfile = join(dir, "colai-toolbar.pid");
    // A pid nothing can be running under.
    writeFileSync(pidfile, "2147483646");
    expect(toolbarOnScreen(pidfile)).toBeNull();
    // And it is cleared, so the next look is a quick one.
    expect(existsSync(pidfile)).toBe(false);
  });
});

describe("whether this machine has a screen the toolbar can work on", () => {
  const linux = "linux" as NodeJS.Platform;

  test("an X11 session is what it is built for", () => {
    expect(screenTrouble({ DISPLAY: ":0", XDG_SESSION_TYPE: "x11" }, linux)).toBeNull();
  });

  test("no display at all says so, and says what to do", () => {
    const trouble = screenTrouble({}, linux);
    expect(trouble?.why).toContain("DISPLAY");
    expect(trouble?.fix, "a refusal with no way forward is half an answer").toBeTruthy();
  });

  /*
   * The one that mattered. Everything the toolbar does to the rest of the screen it does
   * through X11, and under Wayland XWayland answers those questions about its own clients
   * only — so a native Wayland window is not in the capture, is not what "which window is
   * this" names, and the mark that reaches the agent is quietly about something else.
   *
   * It is caught before DISPLAY because a Wayland session sets DISPLAY too. That is
   * precisely how it went unnoticed: the check that existed passed.
   */
  test("a Wayland session is refused, not half-served", () => {
    const said = screenTrouble({ DISPLAY: ":0", XDG_SESSION_TYPE: "wayland" }, linux);
    expect(said?.why, "named, so the person knows which problem this is").toContain("Wayland");
    expect(said?.fix).toContain("Xorg");
  });

  test("and refused however the session announces itself", () => {
    // Some sessions set only one of the two, and compositors differ. Either is enough.
    expect(screenTrouble({ DISPLAY: ":0", WAYLAND_DISPLAY: "wayland-0" }, linux)?.why).toContain(
      "Wayland",
    );
    expect(screenTrouble({ DISPLAY: ":0", XDG_SESSION_TYPE: "Wayland" }, linux)?.why).toContain(
      "Wayland",
    );
  });

  test("the question is not asked where it has no answer", () => {
    // The toolbar does not run on these yet; when it does, they will not be asked about X.
    expect(screenTrouble({}, "win32" as NodeJS.Platform)).toBeNull();
    expect(screenTrouble({}, "darwin" as NodeJS.Platform)).toBeNull();
  });
});

describe("what the published package promises", () => {
  /*
   * Read off the manifest rather than imported from core.
   *
   * These are the things nothing else checks and every one of them has a user-visible
   * failure: a spec that will not parse offers a person "skip for now" and nothing else, a
   * `files` list missing a directory installs a plugin with a blank window, and a scoped
   * package with no `publishConfig` publishes private or not at all. None of them break a
   * test, a build or a lint — they break somebody else's install, once, silently.
   */
  const manifest = JSON.parse(readFileSync(new URL("./package.json", import.meta.url), "utf8")) as {
    name: string;
    license?: string;
    files: string[];
    publishConfig?: { access?: string };
    os?: string[];
    openclaw: { install: { clawhubSpec?: string }; extensions: string[] };
  };

  test("the install spec is one ClawHub will parse", () => {
    /*
     * `parseClawHubPluginSpec` in core refuses anything without this prefix and returns
     * null, and `resolveInstallDefaultChoice` then falls through every branch to "skip".
     * The rule is restated here rather than imported because a plugin test reaching into
     * core `src/**` is the boundary this package is careful about everywhere else.
     */
    const spec = manifest.openclaw.install.clawhubSpec ?? "";
    expect(spec.startsWith("clawhub:")).toBe(true);
    expect(spec.slice("clawhub:".length)).toBe(manifest.name);
  });

  test("the manifest carries what ClawHub refuses a publish without", () => {
    /*
     * `clawhub package publish` validates these two before it will upload anything, and
     * the message it gives is the field path and nothing else. Both were missing, and the
     * first thing that said so was a rejected publish — which is late, and is the kind of
     * thing this file exists to catch instead.
     *
     * `openclawVersion` is what the plugin was built against, and it has to be a version
     * rather than a range: ClawHub records it as `builtWith`, a fact about one build.
     * `pluginApi` is the range of hosts it claims, which is a different statement.
     */
    const build = manifest.openclaw.build ?? {};
    expect(build.openclawVersion).toBe("2026.9.1");
    expect(build.openclawVersion).not.toMatch(/[<>=^~*]/);
    expect(manifest.openclaw.compat?.pluginApi).toBeTruthy();
  });

  test("everything the toolbar needs at runtime is in the tarball", () => {
    // `files` is an allowlist. Dropping one of these produces a plugin that installs
    // cleanly and then does nothing, which no other test would notice.
    for (const needed of [
      "bin/colai-toolbar.gz",
      "bin/colai-toolbar.sha256",
      "dist/",
      "toolbar/ui/",
      "openclaw.plugin.json",
    ]) {
      expect(manifest.files, `${needed} must ship`).toContain(needed);
    }
  });

  test("the binary ships compressed, because uncompressed it cannot be published", () => {
    /*
     * The registry takes files up to 10 MB and the binary is over 12, so `bin/` — which
     * would sweep the uncompressed one in — produces a tarball that packs cleanly and is
     * then rejected on upload with a bare `413`. That failure arrives at the very last
     * step, from a server, with nothing naming the file, so it is worth failing here.
     */
    expect(manifest.files).not.toContain("bin/");
    expect(manifest.files).not.toContain("bin/colai-toolbar");
  });

  test("the entry the manifest names is one the host can resolve", () => {
    expect(manifest.openclaw.extensions).toContain("./index.ts");
    // The host prefers the built runtime and only falls back to the source entry, so the
    // directory holding it has to ship whether or not anybody remembers why.
    expect(manifest.files).toContain("dist/");
  });

  test("a scoped package says it is public, and says what it is licensed as", () => {
    expect(manifest.name.startsWith("@")).toBe(true);
    expect(manifest.publishConfig?.access).toBe("public");
    expect(manifest.license).toBeTruthy();
  });

  test("npm refuses it where it cannot run, rather than installing a useless binary", () => {
    expect(manifest.os).toEqual(["linux"]);
  });
});
