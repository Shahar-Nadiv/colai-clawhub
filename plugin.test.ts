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
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { gunzipSync, gzipSync } from "node:zlib";
import { describe, expect, it as test } from "vitest";

const root = dirname(fileURLToPath(import.meta.url));
const read = (...where: string[]) => readFileSync(join(root, ...where), "utf8");
const readJson = (...where: string[]) => JSON.parse(read(...where));

const LAUNCHER = join(root, "bin", "colai-toolbar");
const ARCHIVE = join("platforms", "linux-x64", "bin", "colai-toolbar.gz");
const PROMISED = join("platforms", "linux-x64", "bin", "colai-toolbar.sha256");

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
    // `marketplace add` clones the default branch, and the default branch here is the
    // OpenClaw plugin, which carries no marketplace at all. A reader who copies the line
    // without the branch gets "Marketplace file not found" and no reason for it.
    const readme = read("README.md");
    expect(readme).toContain("claude plugin marketplace add Shahar-Nadiv/colai-clawhub@");
    expect(readme, "and the plugin id the marketplace actually offers").toContain(
      "claude plugin install colai@colai",
    );
  });

  test("commands/ holds commands and nothing else", () => {
    /*
     * Every `.md` here becomes a listable, typeable command namespaced under the plugin.
     * A note to other developers called `README.md` is therefore `/colai:README` in the
     * command list of everybody who installs this — which is exactly what shipped, until
     * the validator said so.
     *
     * The name matters too: `colai.md` would be `/colai:colai`, and `/colai` alone answers
     * "Did you mean /color?".
     */
    const files = readdirSync(join(root, "commands"));
    expect(files).toEqual(["quit.md", "show.md"]);
  });

  test("the commands run the toolbar rather than asking Claude to", () => {
    /*
     * This is the difference between a command and a prompt, and for a while these were
     * prompts. The body said "run exactly this" over a fenced block, which makes the model
     * read the instruction, decide to obey it, and call Bash — a model turn deciding
     * something that was never in question, and free to decide otherwise.
     *
     * A `!` line is not that. It runs BEFORE anything reaches the model, always, and what
     * the model receives is the output. `/colai:show` starts the toolbar whether or not
     * Claude is paying attention.
     */
    for (const name of ["show.md", "quit.md"]) {
      const body = read("commands", name);
      const bang = body.match(/^!`([^`]+)`$/m);
      expect(bang, `${name} must run the toolbar with a ! line, not ask for it`).not.toBe(null);
      expect(bang?.[1], `${name} must reach the launcher on PATH`).toMatch(/^colai-toolbar /);
      expect(body, `${name} must not tell the model to run anything`).not.toMatch(/[Rr]un exactly this/);
    }
  });

  test("a ! line holds no variable, because the permission check refuses one", () => {
    /*
     * Found by running it. `/colai:show` did nothing at all and said nothing, and the
     * transcript carried the reason:
     *
     *   Shell command permission check failed for pattern
     *   "!`colai-toolbar show --pidfile "$HOME/..." --in "$CLAUDE_CODE_SESSION_ID"`":
     *   Contains simple_expansion
     *
     * The check is run before the line is, and it will not approve what it cannot read —
     * a variable's value is not knowable in advance, so any `$` is refused outright. The
     * command is therefore bare, and everything that needs an environment reads it further
     * down, in the launcher and in the binary, where there is no such check.
     *
     * This is silent when it goes wrong: no error surfaces to the person, the toolbar
     * simply never starts. Which is why it is a test.
     */
    for (const name of ["show.md", "quit.md"]) {
      const bang = read("commands", name).match(/^!`([^`]+)`$/m)?.[1] ?? "";
      expect(bang, `${name}: a $ in a ! line is refused before it runs`).not.toContain("$");
      expect(bang, "and backticks cannot nest").not.toContain("`");
    }
  });

  test("the command leaves detaching to the binary", () => {
    /*
     * `setsid` is refused by Claude Code's sandbox — "cannot be statically analyzed" — and
     * `&` or `nohup` would be a second answer to a question the binary already answers for
     * itself, in `step_out_of_the_way`. A command that backgrounds a process that also
     * backgrounds itself is not twice as detached; it is a lost exit status.
     */
    const asked = read("commands", "show.md").match(/^!`([^`]+)`$/m)?.[1] ?? "";
    expect(asked, "the ! line must be found, or this checks nothing").toContain("colai-toolbar");
    for (const trick of ["setsid", "nohup", "&"]) {
      expect(asked, `${trick} does not belong in the command`).not.toContain(trick);
    }
  });
});

describe("the launcher that stands in for the binary", () => {
  /** A plugin directory as an install sees it: the real launcher, and a toolbar we invent. */
  function installedWith(toolbar: string, digest?: string) {
    const where = mkdtempSync(join(tmpdir(), "colai-front-door-"));
    mkdirSync(join(where, "bin"), { recursive: true });
    mkdirSync(join(where, "platforms", "linux-x64", "bin"), { recursive: true });
    writeFileSync(join(where, "bin", "colai-toolbar"), readFileSync(LAUNCHER));
    chmodSync(join(where, "bin", "colai-toolbar"), 0o755);

    const bytes = Buffer.from(toolbar);
    writeFileSync(join(where, ARCHIVE), gzipSync(bytes));
    writeFileSync(
      join(where, PROMISED),
      `${digest ?? createHash("sha256").update(bytes).digest("hex")}\n`,
    );
    return { where, cache: join(where, "cache") };
  }

  const A_TOOLBAR = "#!/bin/sh\necho 'the toolbar ran'\necho \"argv: $*\"\n";

  test("unpacks on first use, and hands the arguments on", () => {
    const { where, cache } = installedWith(A_TOOLBAR);
    const said = execFileSync(join(where, "bin", "colai-toolbar"), ["show", "--pidfile", "/tmp/x"], {
      env: { ...process.env, XDG_CACHE_HOME: cache, COLAI_TOOLBAR_BIN: "" },
      encoding: "utf8",
    });
    expect(said).toContain("the toolbar ran");
    expect(said, "every argument reaches the toolbar untouched").toContain(
      "argv: show --pidfile /tmp/x",
    );
  });

  test("unpacks into a directory named for the digest, never a file named for it", () => {
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
    execFileSync(join(where, "bin", "colai-toolbar"), [], {
      env: { ...process.env, XDG_CACHE_HOME: cache, COLAI_TOOLBAR_BIN: "" },
    });

    const digest = createHash("sha256").update(Buffer.from(A_TOOLBAR)).digest("hex");
    const kept = readdirSync(join(cache, "colai"));
    expect(kept).toEqual([digest.slice(0, 12)]);
    expect(readdirSync(join(cache, "colai", digest.slice(0, 12)))).toEqual(["colai-toolbar"]);
  });

  test("refuses an archive that is not what was built, and runs nothing", () => {
    // The digest beside the archive is not a signature and does not pretend to be. What it
    // closes is the case where the two disagree — a truncated clone, a half-finished
    // download, a swapped file — and the only safe answer there is to run nothing at all.
    const { where, cache } = installedWith(A_TOOLBAR, "0".repeat(64));
    const ran = spawnSync(join(where, "bin", "colai-toolbar"), [], {
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

  /** Run the launcher with a home of our own and read back what the toolbar was handed. */
  function argvFrom(where: string, cache: string, args: string[], home: string) {
    const said = execFileSync(join(where, "bin", "colai-toolbar"), args, {
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

  test("it names the pidfile itself, because the command cannot", () => {
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

  test("a caller who named one is not overruled", () => {
    // The plugin's own TypeScript passes `--pidfile` explicitly and picks the path itself.
    const { where, cache } = installedWith(A_TOOLBAR);
    const argv = argvFrom(where, cache, ["show", "--pidfile", "/tmp/chosen"], "/home/somebody");
    expect(argv).toBe("show --pidfile /tmp/chosen");
  });

  test("every way out of the launcher carries it", () => {
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
    const explicit = execFileSync(join(where, "bin", "colai-toolbar"), ["show"], {
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
