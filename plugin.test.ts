// The front door: what a person reaches when they install this from a marketplace.
//
// Everything else in this package is about what the toolbar does once it is running. This
// is about the twenty seconds before that — `claude plugin install`, `/colai:show`, and the
// launcher in between — where the failures are quiet ones. A command file that registers
// under a name nobody types, a marketplace pointing at a version that does not exist, an
// archive that is in `.gitignore` and so is simply absent from every install: each of those
// works perfectly on the machine it was built on.
//
// The launcher tests run the real script against a toolbar made up on the spot. A three
// line shell script stands in for the 11 MB binary because nothing here is about the
// binary — it is about whether the thing that shipped is the thing that runs.
import { execFileSync, spawnSync } from "node:child_process";
import type { ExecFileSyncOptions, SpawnSyncOptions, SpawnSyncReturns } from "node:child_process";
import {
  chmodSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  writeFileSync,
} from "node:fs";
import { createHash } from "node:crypto";
import { tmpdir } from "node:os";
import { delimiter, dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { gunzipSync, gzipSync } from "node:zlib";
import { describe, expect, it as test } from "vitest";

const root = dirname(fileURLToPath(import.meta.url));
const read = (...where: string[]) => readFileSync(join(root, ...where), "utf8");
const readJson = (...where: string[]) => JSON.parse(read(...where));

const LAUNCHER = join(root, "bin", "colai-toolbar");
const ARCHIVE = join("platforms", "linux-x64", "bin", "colai-toolbar.gz");
const PROMISED = join("platforms", "linux-x64", "bin", "colai-toolbar.sha256");

// Running the launcher, which is a `#!/bin/sh` script, on Windows.
//
// Node's execFileSync/spawnSync cannot run a shebang script directly there — they hand the
// path to CreateProcess, which knows nothing of `#!` and fails with ENOENT. So on win32 the
// launcher is invoked through `sh` (the bash that ships with Git for Windows, and with the
// GitHub windows-latest runners) with its path as sh's first argument; on POSIX it is run
// directly, exactly as an install does. Three seams need care so the Windows run exercises
// the very same launcher logic the suite asserts on, rather than a different one:
//
//   * `sh` is often not on the PATH of a Node launched from PowerShell, so it is resolved to
//     Git's `usr/bin/sh.exe` — the one that preserves the PATH order it is handed (the `bin/`
//     wrapper reorders PATH and would hide the shim below). The coreutils the launcher shells
//     out to live beside it, so that directory is put on PATH too.
//   * `uname` here answers `MINGW64_NT…`/`x86_64`, so the launcher would resolve the
//     windows-x64 target and look for an archive the fixtures never build. A tiny `uname`
//     that answers `Linux`/`x86_64`, placed first on PATH, keeps it on the linux-x64 path
//     the fixtures set up and the assertions name.
//   * A cache or home path with backslashes makes the `sha256sum` the launcher shells out to
//     escape it and emit a leading `\` on the digest line, so the launcher's own integrity
//     check sees a mismatch and refuses to run. The MSYS tools all accept forward slashes, so
//     the paths handed to sh are converted first.
//
// Nothing real is ever launched: the toolbar that unpacks is the fake the fixture built. If
// `sh` cannot be found, the launcher cases are skipped with a stated reason (below) rather
// than left to fail silently.
const onWindows = process.platform === "win32";
const NO_SH = "skipped: sh (Git for Windows) is needed to run the #!/bin/sh launcher on Windows";

/** Git's `usr/bin/sh.exe` and the coreutils dir beside it, or bare `sh` if it is on PATH. */
function resolveSh(): { sh: string; toolsDir: string | null } | null {
  const roots: string[] = [];
  const git = spawnSync("where", ["git"], { encoding: "utf8" });
  const found = git.stdout?.split(/\r?\n/).map((l) => l.trim()).find(Boolean);
  if (found) roots.push(dirname(dirname(found))); // <root>\cmd\git.exe -> <root>
  roots.push("C:\\Program Files\\Git", "C:\\Program Files (x86)\\Git");
  for (const r of roots) {
    const sh = join(r, "usr", "bin", "sh.exe");
    if (existsSync(sh) && spawnSync(sh, ["-c", "exit 0"]).error == null) {
      return { sh, toolsDir: join(r, "usr", "bin") };
    }
  }
  // Invoked from Git Bash, say: already on PATH with the coreutils alongside it.
  if (spawnSync("sh", ["-c", "exit 0"]).error == null) return { sh: "sh", toolsDir: null };
  return null;
}

const SH = onWindows ? resolveSh() : null;
const haveSh = !onWindows || SH != null;
if (!haveSh) console.warn(NO_SH);

const toPosix = (p: string) => p.split("\\").join("/");

/**
 * A directory holding a `uname` that names the machine we say it is, and a `sysctl` that
 * answers Rosetta's `sysctl.proc_translated` with `translated`. First on PATH, it decides
 * which platform directory the launcher reads, whatever machine the suite is really on.
 */
function machineShim(system: string, machine: string, translated = "0"): string {
  const dir = mkdtempSync(join(tmpdir(), "colai-uname-"));
  writeFileSync(
    join(dir, "uname"),
    // `-sm` is the one the launcher asks, in one fork; the other two are kept for anything
    // that still asks the halves separately.
    `#!/bin/sh\ncase "$1" in\n  -sm) echo ${system} ${machine} ;;\n  -m) echo ${machine} ;;\n  *) echo ${system} ;;\nesac\n`,
  );
  writeFileSync(join(dir, "sysctl"), `#!/bin/sh\necho ${translated}\n`);
  chmodSync(join(dir, "uname"), 0o755);
  chmodSync(join(dir, "sysctl"), 0o755);
  return dir;
}

// Linux by default, everywhere but Linux. A Windows `uname` answers `MINGW64_NT…` (above),
// and a Mac's answers `Darwin`/`arm64` — which the launcher now maps to darwin-arm64, a
// directory the fixtures below never build. So a macOS CI runner needs the same lie a
// Windows one does, to stay on the linux-x64 path the assertions name.
const unameShim = process.platform !== "linux" ? machineShim("Linux", "x86_64") : "";

/** The env to hand sh: paths the launcher reads de-Windowsed, the chosen `uname` first on PATH. */
function shEnv(env: NodeJS.ProcessEnv = {}, shim = unameShim): NodeJS.ProcessEnv {
  const fixed: NodeJS.ProcessEnv = { ...env };
  for (const key of ["HOME", "XDG_CACHE_HOME", "XDG_CONFIG_HOME", "COLAI_TOOLBAR_BIN"]) {
    const value = fixed[key];
    if (typeof value === "string" && value) fixed[key] = toPosix(value);
  }
  const prepend = [toPosix(shim)];
  if (SH?.toolsDir) prepend.push(toPosix(SH.toolsDir));
  fixed.PATH = [...prepend, fixed.PATH ?? ""].join(delimiter);
  return fixed;
}

/** On POSIX the launcher runs directly, so a shim only has to go first on the PATH it gets. */
function withShim<T extends { env?: NodeJS.ProcessEnv }>(options: T, shim: string): T {
  if (!shim) return options;
  const env = options.env ?? process.env;
  return { ...options, env: { ...env, PATH: [shim, env.PATH].filter(Boolean).join(delimiter) } };
}

/** Run the launcher as an install would: directly on POSIX, through `sh` on win32. */
function runLauncher(
  launcher: string,
  args: string[],
  options: ExecFileSyncOptions = {},
  shim = unameShim,
): string {
  const out = onWindows
    ? execFileSync(SH!.sh, [toPosix(launcher), ...args], { ...options, env: shEnv(options.env, shim) })
    : execFileSync(launcher, args, withShim(options, shim));
  return out as unknown as string;
}

function spawnLauncher(
  launcher: string,
  args: string[],
  options: SpawnSyncOptions = {},
  shim = unameShim,
): SpawnSyncReturns<string> {
  return (onWindows
    ? spawnSync(SH!.sh, [toPosix(launcher), ...args], { ...options, env: shEnv(options.env, shim) })
    : spawnSync(launcher, args, withShim(options, shim))) as SpawnSyncReturns<string>;
}

describe("what a marketplace hands out", () => {
  test("the marketplace offers the plugin beside it, at the version that plugin claims", () => {
    // Two manifests carry the version and a person installs whichever the marketplace says.
    // Disagreeing, the install asks the registry for a version that was never published and
    // fails with a resolution error about a number nobody typed.
    const market = readJson(".claude-plugin", "marketplace.json");
    const plugin = readJson(".claude-plugin", "plugin.json");

    const offered = market.plugins.filter((one: { name: string }) => one.name === plugin.name);
    expect(offered, `the marketplace never offers ${plugin.name}`).toHaveLength(1);
    expect(offered[0].source, "it must point at the plugin in this repository").toBe("./");
    expect(offered[0].version).toBe(plugin.version);
  });

  test("both manifests survive Claude Code's own strict validator", () => {
    // The validator is what caught `commands/README.md` registering as a command, and it
    // knows about fields this file has never heard of. Running it here is the difference
    // between a check that ages and one that does not.
    for (const manifest of ["marketplace.json", "plugin.json"]) {
      const ran = spawnSync("claude", ["plugin", "validate", join(".claude-plugin", manifest), "--strict"], {
        cwd: root,
        encoding: "utf8",
      });
      if (ran.error) {
        // Claude Code is not installed on every machine this suite runs on, and a missing
        // tool is not a failing plugin. Skipped loudly rather than passed quietly.
        console.warn(`skipped: claude plugin validate is not available (${ran.error.message})`);
        return;
      }
      expect(`${manifest}: ${ran.stdout}${ran.stderr}`).toContain("Validation passed");
    }
  });

  test("the README's install command names a marketplace that exists", () => {
    // `marketplace add` clones the default branch, so that branch has to BE the plugin — with
    // the marketplace at its root — and the README must not send people to another one. (It
    // once named a side branch because `main` was still an older product with no marketplace.)
    const readme = read("README.md");
    expect(readme).toMatch(/claude plugin marketplace add Shahar-Nadiv\/colai-clawhub\s*\n/);
    expect(readme, "no branch pinned in the install line").not.toMatch(/marketplace add Shahar-Nadiv\/colai-clawhub@/);
    expect(existsSync(join(root, ".claude-plugin", "marketplace.json")), "the marketplace lives at the root").toBe(true);
    expect(readme, "and the plugin id the marketplace actually offers").toContain(
      "claude plugin install colai@colai",
    );
  });

  test("there is no commands/ dir — colai starts from the SessionStart hook, not a chat command", () => {
    /*
     * colai is a desktop overlay, and summoning a GUI by typing `/colai:show` into a chat was
     * the wrong shape for it. A plugin can start something only from a slash command or a hook;
     * colai now starts itself from the SessionStart hook (hooks/say-colai-is-here.sh), so there
     * is nothing to type and no commands/ directory at all. This asserts the directory is gone,
     * so a stray `.md` can never re-register a command by accident.
     */
    expect(existsSync(join(root, "commands")), "commands/ must not exist").toBe(false);
  });

  test("the SessionStart hook starts the toolbar rather than only talking about it", () => {
    /*
     * The old hook merely printed "run /colai:show". Now it launches the toolbar itself when
     * one is not already running — detached, so the hook returns inside its budget and the
     * toolbar outlives the shell — guarded by the same pidfile it reads to decide, and skippable
     * with COLAI_AUTOSTART=0. It must not phone the model to do it; the launch is a plain `!`-free
     * shell line in the hook.
     */
    const hook = readFileSync(new URL("./hooks/say-colai-is-here.sh", import.meta.url), "utf8");
    expect(hook, "it runs the launcher's show").toMatch(/colai-toolbar["'\s]+show|"\$LAUNCHER"\s+show/);
    expect(hook, "detached so the hook returns at once").toContain("nohup");
    expect(hook, "backgrounded").toMatch(/&\s*$/m);
    expect(hook, "honours an opt-out").toContain("COLAI_AUTOSTART");
    expect(hook, "only launches when nothing is already running").toContain('running" = yes');
  });

  test("the hook leaves the binary to survive detaching, and only backgrounds for its timeout", () => {
    /*
     * The binary detaches itself (step_out_of_the_way: a new process group on Unix,
     * DETACHED_PROCESS on Windows) and exits 0 — but falls back to the foreground if that spawn
     * fails. A SessionStart hook has a hard few-second budget the old slash command never had,
     * so the hook backgrounds the launch (nohup + &) as belt-and-suspenders: harmless in the
     * normal self-detach case, and the only thing that keeps a foreground fallback from being
     * killed when the hook's shell exits. setsid stays out — Claude Code's sandbox refuses it.
     */
    const hook = readFileSync(new URL("./hooks/say-colai-is-here.sh", import.meta.url), "utf8");
    expect(hook, "backgrounds the launch for the hook's timeout").toMatch(/nohup .*colai-toolbar.*show|nohup sh "\$LAUNCHER" show/);
    expect(hook, "setsid is refused by the sandbox").not.toContain("setsid");
  });

  /*
   * On Windows the toolbar records its NATIVE pid, and Git Bash's `kill -0` only knows MSYS
   * pids — so a hook that asked `kill` was told "gone" about a toolbar on the screen, and
   * launched (and showed) it again at every session start. The hook asks `ps -W` there and
   * reads the WINPID column. Each case below hands it a made-up `ps -W` table and a Windows
   * `uname`, with no launcher to reach, so nothing can start whatever the answer.
   */
  const PS_W =
    "      PID    PPID    PGID     WINPID   TTY         UID    STIME COMMAND\n" +
    "  4210724       0       0      16420  ?              0 19:49:09 C:\\colai\\colai-toolbar.exe\n" +
    "S    2001    2000    2001       7777  pty0      197609 19:50:01 /usr/bin/sleep\n" +
    "     3003       1    3003       9999  pty0      197609 19:50:02 /usr/bin/bash\n";

  function hookSays(pid: string): string {
    const shim = machineShim("MINGW64_NT-10.0-26200", "x86_64");
    writeFileSync(join(shim, "ps"), `#!/bin/sh\n[ "$1" = -W ] || exit 1\ncat <<'EOF'\n${PS_W}EOF\n`);
    chmodSync(join(shim, "ps"), 0o755);
    const config = mkdtempSync(join(tmpdir(), "colai-hook-"));
    mkdirSync(join(config, "ai.colai.toolbar"), { recursive: true });
    writeFileSync(join(config, "ai.colai.toolbar", "colai-toolbar.pid"), `${pid}\n`);
    const nowhere = toPosix(mkdtempSync(join(tmpdir(), "colai-no-launcher-")));
    const hook = join(root, "hooks", "say-colai-is-here.sh");
    const env = { ...process.env, XDG_CONFIG_HOME: config, CLAUDE_PLUGIN_ROOT: nowhere, COLAI_AUTOSTART: "1" };
    const ran = onWindows
      ? spawnSync(SH!.sh, [toPosix(hook)], { env: shEnv(env, shim), encoding: "utf8" })
      : spawnSync("sh", [hook], withShim({ env, encoding: "utf8" as const }, shim));
    return ran.stdout;
  }

  test.skipIf(!haveSh)("on Windows the hook finds a running toolbar by its native pid", () => {
    expect(hookSays("16420")).toContain("colai is up");
    // A row MSYS prefixed with a state letter shifts WINPID one column right.
    expect(hookSays("7777"), "a state-prefixed row").toContain("colai is up");
  });

  test.skipIf(!haveSh)("and does not mistake an MSYS pid, or a dead one, for the toolbar", () => {
    expect(hookSays("3003"), "3003 is only an MSYS pid here").not.toContain("colai is up");
    expect(hookSays("424242"), "nobody has this pid").not.toContain("colai is up");
  });
});

describe("the launcher that stands in for the binary", () => {
  /**
   * A plugin directory as an install sees it: the real launcher, and a toolbar we invent,
   * staged for `which` (linux-x64 unless a test says otherwise).
   */
  function installedWith(toolbar: string, digest?: string, which = "linux-x64") {
    const where = mkdtempSync(join(tmpdir(), "colai-front-door-"));
    mkdirSync(join(where, "bin"), { recursive: true });
    mkdirSync(join(where, "platforms", which, "bin"), { recursive: true });
    writeFileSync(join(where, "bin", "colai-toolbar"), readFileSync(LAUNCHER));
    chmodSync(join(where, "bin", "colai-toolbar"), 0o755);

    const bytes = Buffer.from(toolbar);
    writeFileSync(join(where, "platforms", which, "bin", "colai-toolbar.gz"), gzipSync(bytes));
    writeFileSync(
      join(where, "platforms", which, "bin", "colai-toolbar.sha256"),
      `${digest ?? createHash("sha256").update(bytes).digest("hex")}\n`,
    );
    return { where, cache: join(where, "cache") };
  }

  const A_TOOLBAR = "#!/bin/sh\necho 'the toolbar ran'\necho \"argv: $*\"\n";

  test.skipIf(!haveSh)("unpacks on first use, and hands the arguments on", () => {
    const { where, cache } = installedWith(A_TOOLBAR);
    const said = runLauncher(join(where, "bin", "colai-toolbar"), ["show", "--pidfile", "/tmp/x"], {
      env: { ...process.env, XDG_CACHE_HOME: cache, COLAI_TOOLBAR_BIN: "" },
      encoding: "utf8",
    });
    expect(said).toContain("the toolbar ran");
    expect(said, "every argument reaches the toolbar untouched").toContain(
      "argv: show --pidfile /tmp/x",
    );
  });

  test.skipIf(!haveSh)("unpacks into a directory named for the digest, never a file named for it", () => {
    /*
     * X11 reads a window's `WM_CLASS` from the executable's basename, and a desktop keys
     * its icon, its taskbar grouping and any window rule the user wrote off that string.
     * Unpacked as `colai-toolbar-3e18b97d6676`, the overlay introduces itself under a new
     * name after every release and loses all three.
     *
     * The digest still has to be in the path — that is what stops an update trusting an old
     * file with the right name — so it goes in the directory.
     */
    const { where, cache } = installedWith(A_TOOLBAR);
    runLauncher(join(where, "bin", "colai-toolbar"), [], {
      env: { ...process.env, XDG_CACHE_HOME: cache, COLAI_TOOLBAR_BIN: "" },
    });

    const digest = createHash("sha256").update(Buffer.from(A_TOOLBAR)).digest("hex");
    const kept = readdirSync(join(cache, "colai"));
    expect(kept).toEqual([digest.slice(0, 12)]);
    expect(readdirSync(join(cache, "colai", digest.slice(0, 12)))).toEqual(["colai-toolbar"]);
  });

  test.skipIf(!haveSh)("refuses an archive that is not what was built, and runs nothing", () => {
    // The digest beside the archive is not a signature and does not pretend to be. What it
    // closes is the case where the two disagree — a truncated clone, a half-finished
    // download, a swapped file — and the only safe answer there is to run nothing at all.
    const { where, cache } = installedWith(A_TOOLBAR, "0".repeat(64));
    const ran = spawnLauncher(join(where, "bin", "colai-toolbar"), [], {
      env: { ...process.env, XDG_CACHE_HOME: cache, COLAI_TOOLBAR_BIN: "" },
      encoding: "utf8",
    });

    expect(ran.status, "a mismatch must not be a successful run").not.toBe(0);
    expect(ran.stdout, "nothing may be executed").not.toContain("the toolbar ran");
    expect(ran.stderr).toContain("not the one that was built");
  });

  test("says which machine this is when there is no build for it", () => {
    // An x86-64 binary handed to an arm64 Mac says `Exec format error`, which sends a
    // person looking for a corrupt download. This is the one place that can tell them the
    // truth, because it is the only part of colai that runs on a machine with no colai.
    const launcher = readFileSync(LAUNCHER, "utf8");
    expect(launcher).toContain("Linux-x86_64");
    expect(launcher).toContain("has no build for");
  });

  /*
   * A Mac is named by `uname` and, under Rosetta, lied about by it. Each case below stages
   * the made-up toolbar only where that machine should look, so a wrong mapping is a refusal
   * rather than a quiet run of some other platform's build.
   */
  const MACS = [
    { said: "Apple Silicon", system: "Darwin", machine: "arm64", translated: "0", which: "darwin-arm64" },
    { said: "Apple Silicon, by its other name", system: "Darwin", machine: "aarch64", translated: "0", which: "darwin-arm64" },
    { said: "an Intel Mac", system: "Darwin", machine: "x86_64", translated: "0", which: "darwin-x64" },
    // A shell running under Rosetta reports x86_64 on an arm64 machine. The native build is
    // the right one to run there, and the x64 one may not exist at all.
    { said: "Apple Silicon behind Rosetta", system: "Darwin", machine: "x86_64", translated: "1", which: "darwin-arm64" },
  ];

  for (const mac of MACS) {
    test.skipIf(!haveSh)(`${mac.said} runs the ${mac.which} build`, () => {
      const { where, cache } = installedWith(A_TOOLBAR, undefined, mac.which);
      const said = runLauncher(
        join(where, "bin", "colai-toolbar"),
        ["show"],
        { env: { ...process.env, XDG_CACHE_HOME: cache, COLAI_TOOLBAR_BIN: "" }, encoding: "utf8" },
        machineShim(mac.system, mac.machine, mac.translated),
      );
      expect(said).toContain("the toolbar ran");
    });
  }

  test.skipIf(!haveSh)("a Mac with nothing staged is told how to build one, not handed Linux's", () => {
    // Only Linux is built in the release container; a Mac builds and stages its own. The
    // refusal names the platform it looked for and the two commands that would fill it.
    const { where, cache } = installedWith(A_TOOLBAR); // linux-x64 only
    const ran = spawnLauncher(
      join(where, "bin", "colai-toolbar"),
      ["show"],
      { env: { ...process.env, XDG_CACHE_HOME: cache, COLAI_TOOLBAR_BIN: "" }, encoding: "utf8" },
      machineShim("Darwin", "arm64"),
    );
    expect(ran.status).not.toBe(0);
    expect(ran.stdout, "nothing may be executed").not.toContain("the toolbar ran");
    expect(ran.stderr).toContain("carries no toolbar for darwin-arm64");
    expect(ran.stderr).toContain("npm run stage:macos");
  });

  test.skipIf(!haveSh)("a machine nobody has built for is told so in a sentence", () => {
    const { where, cache } = installedWith(A_TOOLBAR);
    const ran = spawnLauncher(
      join(where, "bin", "colai-toolbar"),
      ["show"],
      { env: { ...process.env, XDG_CACHE_HOME: cache, COLAI_TOOLBAR_BIN: "" }, encoding: "utf8" },
      machineShim("FreeBSD", "riscv64"),
    );
    expect(ran.status).not.toBe(0);
    expect(ran.stderr).toContain("has no build for FreeBSD riscv64");
  });

  /** Run the launcher with a home of our own and read back what the toolbar was handed. */
  function argvFrom(where: string, cache: string, args: string[], home: string) {
    const said = runLauncher(join(where, "bin", "colai-toolbar"), args, {
      env: {
        ...process.env,
        HOME: home,
        XDG_CACHE_HOME: cache,
        XDG_CONFIG_HOME: "",
        COLAI_TOOLBAR_BIN: "",
      },
      encoding: "utf8",
    });
    return said.match(/argv: (.*)/)?.[1] ?? "";
  }

  test.skipIf(!haveSh)("it names the pidfile itself, because the command cannot", () => {
    /*
     * `/colai:show` is a `!` line, and a `!` line is permission-checked before it runs and
     * refused if it contains a `$`. So the command cannot say `--pidfile "$HOME/..."`, and
     * the launcher is the last place that knows both the home directory and the convention.
     *
     * It matters beyond tidiness: the pidfile is how `hooks/say-colai-is-here.sh` decides
     * whether to tell somebody the toolbar is already running. Without it the notice says
     * "colai is installed" to a person looking straight at a running toolbar.
     */
    const { where, cache } = installedWith(A_TOOLBAR);
    const argv = argvFrom(where, cache, ["show"], "/home/somebody");
    expect(argv).toBe("show --pidfile /home/somebody/.config/ai.colai.toolbar/colai-toolbar.pid");
  });

  test("and the hook looks in that exact place", () => {
    // Two files deriving one path, which is the thing `whereabouts.rs` warned about when it
    // said the path is given rather than derived. They are allowed to be two only while
    // they cannot drift.
    const notice = readFileSync(new URL("./hooks/say-colai-is-here.sh", import.meta.url), "utf8");
    const launcher = readFileSync(LAUNCHER, "utf8");
    const WHERE = '"${XDG_CONFIG_HOME:-$HOME/.config}/ai.colai.toolbar/colai-toolbar.pid"';
    expect(notice, "the notice must derive it this way").toContain(WHERE);
    expect(launcher, "and the launcher the same way").toContain(WHERE);
  });

  test.skipIf(!haveSh)("an update takes the version it replaces with it", () => {
    /*
     * The digest lives in the directory name rather than the filename on purpose — X11 takes
     * a window's `WM_CLASS` from the executable's basename, so a digest in the filename makes
     * the overlay announce itself as a different application after every release.
     *
     * The cost of that went unnoticed until somebody measured an install: every release left
     * its predecessor behind, nine and a half megabytes a version, accumulating for the life
     * of the machine and none of it ever runnable again. Ten releases is a hundred megabytes
     * of binaries nobody will ever execute.
     */
    const { where, cache } = installedWith(A_TOOLBAR);
    const stale = join(cache, "colai", "aaaaaaaaaaaa");
    mkdirSync(stale, { recursive: true });
    writeFileSync(join(stale, "colai-toolbar"), "an earlier release");

    runLauncher(join(where, "bin", "colai-toolbar"), ["show"], {
      env: { ...process.env, XDG_CACHE_HOME: cache, COLAI_TOOLBAR_BIN: "" },
      encoding: "utf8",
    });

    expect(existsSync(stale), "the version it replaced should be gone").toBe(false);
    // And exactly one is left: the one it just unpacked.
    expect(readdirSync(join(cache, "colai")).length).toBe(1);
  });

  test.skipIf(!haveSh)("a refused update leaves the working copy alone", () => {
    /*
     * The sweep lives inside the successful-unpack branch, which is the whole of why it is
     * safe. A tampered archive is refused before anything is removed, so somebody whose
     * download was corrupted still has the toolbar they had yesterday.
     */
    const { where, cache } = installedWith(A_TOOLBAR, "0".repeat(64));
    const good = join(cache, "colai", "keepthisone");
    mkdirSync(good, { recursive: true });
    writeFileSync(join(good, "colai-toolbar"), "yesterday's toolbar");

    let refused = false;
    try {
      runLauncher(join(where, "bin", "colai-toolbar"), ["show"], {
        env: { ...process.env, XDG_CACHE_HOME: cache, COLAI_TOOLBAR_BIN: "" },
        encoding: "utf8",
        stdio: "pipe",
      });
    } catch {
      refused = true;
    }
    expect(refused, "a digest that does not match must refuse to run").toBe(true);
    expect(existsSync(good), "and must not have swept anything").toBe(true);
  });

  test.skipIf(!haveSh)("a caller who named one is not overruled", () => {
    // The plugin's own TypeScript passes `--pidfile` explicitly and picks the path itself.
    const { where, cache } = installedWith(A_TOOLBAR);
    const argv = argvFrom(where, cache, ["show", "--pidfile", "/tmp/chosen"], "/home/somebody");
    expect(argv).toBe("show --pidfile /tmp/chosen");
  });

  test.skipIf(!haveSh)("every way out of the launcher carries it", () => {
    /*
     * There are three `exec`s here — an explicit `COLAI_TOOLBAR_BIN`, a build already lying
     * around from `npm run build:toolbar` or `build:release`, and the shipped archive — and
     * the default was first written above only the last of them. Which is the path a
     * shipped install takes and not the one a working checkout does, so it was added in the
     * one place it could not be observed, and the pidfile went on not being written.
     */
    const { where, cache } = installedWith(A_TOOLBAR);

    // The explicit one.
    const built = join(where, "colai-toolbar.fake");
    writeFileSync(built, A_TOOLBAR);
    chmodSync(built, 0o755);
    const explicit = runLauncher(join(where, "bin", "colai-toolbar"), ["show"], {
      env: { ...process.env, HOME: "/home/somebody", XDG_CONFIG_HOME: "", COLAI_TOOLBAR_BIN: built },
      encoding: "utf8",
    });
    expect(explicit, "COLAI_TOOLBAR_BIN").toContain("--pidfile /home/somebody/.config/");

    // One already lying around beside the launcher.
    const beside = join(where, "bin", "colai-toolbar.built");
    writeFileSync(beside, A_TOOLBAR);
    chmodSync(beside, 0o755);
    expect(argvFrom(where, cache, ["show"], "/home/somebody"), "a local build").toContain(
      "--pidfile /home/somebody/.config/",
    );
  });
});

describe("what actually travels in a clone", () => {
  test("the archive is tracked and the binary it unpacks to is not", () => {
    /*
     * Backwards-looking and deliberate. Claude Code runs nothing at install time — there is
     * no `postinstall` — so if the toolbar is not in the clone it is nowhere, and there is
     * no later step that fetches it. The 4 MB archive therefore travels; the 11 MB binary
     * does not.
     */
    const ignored = (path: string) =>
      spawnSync("git", ["check-ignore", "-q", path], { cwd: root }).status === 0;

    expect(ignored(ARCHIVE), "the shipped archive must reach an install").toBe(false);
    expect(ignored(PROMISED), "and so must the digest that vouches for it").toBe(false);
    expect(ignored(join("bin", "colai-toolbar")), "the launcher is source, not output").toBe(false);
    expect(ignored(join("platforms", "linux-x64", "bin", "colai-toolbar"))).toBe(true);
    expect(ignored(join("bin", "colai-toolbar.built")), "a local build is nobody else's").toBe(true);
  });

  test("no lockfile, because a lockfile is an npm install in somebody else's plugin dir", () => {
    /*
     * Claude Code installs a plugin by copying its checkout, and a `package-lock.json`
     * sitting in it makes the installer run npm over the manifest beside it. That manifest
     * here is the OpenClaw npm wrapper, so the install fetched 328 packages and 595 MB —
     * OpenClaw itself among them — into the plugin directory of somebody who asked for a
     * toolbar and needs none of it: the launcher is a shell script and the toolbar is a
     * binary.
     *
     * A `package.json` with dependencies is harmless alone. The lockfile is the trigger,
     * which is why this is the thing being asserted, and why it was measured rather than
     * reasoned about.
     */
    const ignored = (path: string) =>
      spawnSync("git", ["check-ignore", "-q", path], { cwd: root }).status === 0;
    const tracked =
      spawnSync("git", ["ls-files", "--error-unmatch", "package-lock.json"], { cwd: root })
        .status === 0;

    expect(tracked, "a lockfile must never reach an install").toBe(false);
    expect(ignored("package-lock.json"), "and must not be able to sneak back").toBe(true);
  });

  test("the archive in this checkout is the binary its digest claims", () => {
    // Not a property of the code — a property of the four files that are about to be
    // committed. A digest written beside an archive by hand, or left behind by an earlier
    // build, is the kind of thing nothing else would ever notice.
    const promised = read(PROMISED).trim().split(/\s+/)[0];
    const bytes = gunzipSync(readFileSync(join(root, ARCHIVE)), {
      maxOutputLength: 32 * 1024 * 1024,
    });
    expect(createHash("sha256").update(bytes).digest("hex")).toBe(promised);
  });
});

describe("a mark left for a chat somebody is sitting in", () => {
  const hook = readFileSync(new URL("./hooks/take-what-colai-staged.sh", import.meta.url), "utf8");
  const wiring = JSON.parse(
    readFileSync(new URL("./hooks/hooks.json", import.meta.url), "utf8"),
  ) as { hooks: Record<string, { hooks: { command: string; timeout?: number }[] }[]> };

  test("it runs on the person's next message, not at session start", () => {
    /*
     * `SessionStart` would be wrong and tempting. A mark is usually made long after the
     * session began — the toolbar is open beside a chat that has been running an hour —
     * and a hook that only fires at the start would hold it until the next restart.
     */
    const on = wiring.hooks.UserPromptSubmit?.[0]?.hooks?.[0];
    expect(on, "UserPromptSubmit must be wired").toBeTruthy();
    expect(on?.command).toContain("take-what-colai-staged.sh");
    expect(on?.timeout, "and must not hang somebody's prompt").toBeGreaterThan(0);
  });

  test("it hands over on stdout, which is the channel the model reads", () => {
    /*
     * The two channels are not interchangeable and this file wants the opposite one from
     * `say-colai-is-here.sh`: stdout reaches the MODEL, `systemMessage` reaches the person.
     * A mark is context arriving, so it goes to the model — printing it as a systemMessage
     * would show the person their own question back and tell Claude nothing.
     */
    expect(hook).toContain('cat "$at/say.txt"');
    // Without its comments, because the paragraph explaining which channel this is NOT has
    // to be allowed to name the other one.
    const code = hook.replace(/^\s*#.*$/gm, "");
    expect(code, "a mark is not a notice").not.toContain("systemMessage");
  });

  test("it takes only marks addressed to this conversation", () => {
    expect(hook).toContain("CLAUDE_CODE_SESSION_ID");
    // No id, no marks. Guessing which conversation a mark belongs to would deliver
    // somebody's screenshot into the wrong chat.
    expect(hook).toMatch(/\[ -n "\$\{CLAUDE_CODE_SESSION_ID:-\}" \] \|\| exit 0/);
  });

  test("a half-written mark is skipped, and a handed-over one does not come again", () => {
    // The hook can run at any instant, including between the picture being written and the
    // question being written.
    expect(hook).toContain("*.part) continue");
    // And context the model acts on must not be re-delivered, or it acts on it twice.
    expect(hook).toContain('rm -rf "$at"');
  });

  test("nothing waiting says nothing at all", () => {
    // This runs in front of every message the person sends. A hook that announced its own
    // emptiness would put a line of noise before every turn of every conversation.
    expect(hook).toMatch(/\[ -d "\$WAITING" \] \|\| exit 0/);
  });

  test("the outbox is where the launcher and the notice already look", () => {
    // Four files now derive paths under this directory. They are allowed to be four only
    // while they cannot drift.
    const WHERE = '"${XDG_CONFIG_HOME:-$HOME/.config}/ai.colai.toolbar';
    expect(hook).toContain(WHERE);
  });
});
