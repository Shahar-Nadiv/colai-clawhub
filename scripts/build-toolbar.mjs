// Build the toolbar and stage it for packing, and answer for it afterwards.
//
// The toolbar is a Tauri program, so somebody has to compile it. Not the installing
// machine: OpenClaw installs plugins with `--ignore-scripts`, always, with no flag and
// no config to opt in — `createSafeNpmInstallArgs` in `src/infra/safe-package-install.ts`
// puts it in every managed npm install. A `postinstall` here would never run, and asking
// somebody who wants a toolbar for a Rust toolchain and GTK headers was never a good
// trade anyway. So the toolbar travels already built, in `bin/`, and this is what puts
// it there before the package is packed.

import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { copyFileSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const crate = join(root, "toolbar", "src-tauri");
const logFile = join(root, "toolbar", "src-tauri", "target", "build.log");
const compiled = join(crate, "target", "release", "colai-toolbar");
// `.built`, not `colai-toolbar`, because that name is taken. `bin/colai-toolbar` is the
// launcher the Claude Code plugin puts on PATH — a tracked shell script — and a local
// build dropping a 13 MB binary on top of it would work here and be committed by mistake
// there. The launcher prefers this file when it exists, so a local build is still the
// thing that runs.
const staged = join(root, "bin", "colai-toolbar.built");

/** What building the toolbar needs, and the one command that provides it. */
const NEEDS = [
  {
    what: "the Rust toolchain",
    probe: ["cargo", ["--version"]],
    fix: "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh",
  },
  {
    what: "GTK and WebKit development headers",
    probe: ["pkg-config", ["--exists", "webkit2gtk-4.1", "gtk+-3.0", "libsoup-3.0"]],
    fix: "sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev",
  },
];

function whatIsMissing() {
  return NEEDS.filter((need) => {
    const [command, args] = need.probe;
    try {
      // A missing command sets `error` rather than a status, so both count as a miss.
      return spawnSync(command, args, { encoding: "utf8" }).status !== 0;
    } catch {
      return true;
    }
  });
}

const missing = whatIsMissing();

if (missing.length > 0) {
  console.error("\ncolai: the toolbar cannot be built here, because this machine is missing:\n");
  for (const need of missing) {
    console.error(`  · ${need.what}`);
    console.error(`    ${need.fix}\n`);
  }
  process.exit(1);
}

// Minutes, not seconds, and on a cold cache considerably more. Said before it starts,
// because a silent build that takes eleven minutes reads as a hang.
console.error("\ncolai: building the toolbar. This takes a few minutes the first time.\n");

const began = Date.now();
// `--locked`: a publish build must be the lockfile's build. Without it a dependency
// resolving forward between the lock and the publish is a different binary than the one
// the sources here describe, and nobody would know.
const built = spawnSync("cargo", ["build", "--release", "--locked"], {
  cwd: crate,
  encoding: "utf8",
  stdio: ["ignore", "pipe", "pipe"],
  env: {
    ...process.env,
    // Panic locations from dependencies are compiled in as absolute paths, so without
    // this the published binary tells everyone who downloads it the home directory of
    // whoever built it. Remapped rather than stripped, because the file and line are
    // still worth having in a crash report.
    RUSTFLAGS: [
      process.env.RUSTFLAGS ?? "",
      `--remap-path-prefix=${join(process.env.HOME ?? "", ".cargo")}=/cargo`,
      `--remap-path-prefix=${crate}=/colai`,
    ]
      .filter(Boolean)
      .join(" "),
  },
});
const took = Math.round((Date.now() - began) / 1000);

try {
  writeFileSync(logFile, `${built.stdout ?? ""}\n${built.stderr ?? ""}`);
} catch {
  // A log nobody can write is not a reason to fail a build that worked.
}

if (built.status !== 0) {
  console.error(`\ncolai: the toolbar failed to build after ${took}s.`);
  console.error(`The compiler's output is in ${logFile}\n`);
  process.exit(1);
}

// Staged rather than shipped from `target/`, so what the package carries is one named
// file and not a corner of a build directory.
try {
  mkdirSync(dirname(staged), { recursive: true });
  copyFileSync(compiled, staged);
} catch (error) {
  // Reached when `CARGO_TARGET_DIR` is set in the publisher's environment, among other
  // things. The dependency probe above went to some trouble to be kind; a raw ENOENT
  // stack here would undo it.
  console.error(`\ncolai: the toolbar built, but could not be staged: ${String(error)}`);
  console.error(`Expected it at ${compiled}. Is CARGO_TARGET_DIR set?\n`);
  process.exit(1);
}

/*
 * And what it is, written beside it.
 *
 * The package ships a native binary that runs as the user, photographs the screen and
 * holds an identity that authenticates to the Gateway. Nothing in npm proves the binary
 * in the tarball is the one that was built here — so the plugin checks this digest before
 * it spawns anything, and a swapped binary is a refusal rather than a silent success.
 *
 * This is not a signature and does not pretend to be: somebody who can replace the binary
 * can replace the digest beside it. It closes the case where only the artifact is
 * tampered with, and it makes what shipped auditable against a rebuild.
 */
// A build from here is for this machine, so any note saying the staged binary came out
// of the release image is now false. Removed rather than left to be believed: that note
// is what `check-shippable.mjs` reads to decide whether a tarball may be published, and
// a stale one would wave through a binary that runs on no Linux but this one.
rmSync(`${staged}.build.json`, { force: true });

const digest = createHash("sha256").update(readFileSync(staged)).digest("hex");
writeFileSync(`${staged}.sha256`, `${digest}\n`);
console.error(`colai: toolbar built in ${took}s and staged at bin/colai-toolbar.built.`);
console.error(`colai: sha256 ${digest}\n`);
