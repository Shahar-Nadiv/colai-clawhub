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
    expect(files).toEqual(["show.md"]);
  });

  test("/colai:show names the binary plainly and leaves detaching to it", () => {
    const show = read("commands", "show.md");
    expect(show, "the command must reach the launcher on PATH").toContain("colai-toolbar show");

    /*
     * `setsid` is refused by Claude Code's sandbox — "cannot be statically analyzed" — and
     * `&` or `nohup` would be a second answer to a question the binary already answers for
     * itself, in `step_out_of_the_way`. A command that backgrounds a process that also
     * backgrounds itself is not twice as detached; it is a lost exit status.
     */
    const asked = show.split("```")[1] ?? "";
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
