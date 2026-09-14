// Everything that must be true before this package is handed to ClawHub.
//
// This exists because the gate it replaces does not run. `check-shippable.mjs` is wired as
// a `prepack` hook, which covers `npm pack` and `npm publish` — and the actual publishing
// path is `clawhub package pack`, which shells out to:
//
//     npm pack <dir> --json --ignore-scripts --pack-destination <dir>
//
// `--ignore-scripts`, explicitly. So on the one path that matters, `prepack` never fires:
// the shippability check never runs, and neither does `build-runtime.mjs`, which means
// ClawHub would happily pack whatever stale `dist/index.js` happened to be lying around.
// A lifecycle hook is the wrong shape for a gate whose publisher disables lifecycle hooks.
//
// So this is a thing you run, on purpose, and then publish:
//
//     npm run ship
//     npx clawhub@0.23.3 package publish . --dry-run
//     npx clawhub@0.23.3 package publish .
//
// The argument is a *source*, not a package name: a folder, or `owner/repo` on GitHub.
// `colai/toolbar` looked like a package name and was read as a GitHub repository, which
// answered `GitHub repo not found`. It has to be the folder, because the binary and the
// built runtime are not in the repository — they are gitignored, and a GitHub-sourced
// publish would ship a plugin with no toolbar in it.
//
// It is deliberately not a hook of any kind, and it is not called `prepublish` either:
// npm still treats that name as a lifecycle in some versions and runs it on `npm
// install`, which would mean a release gate firing every time somebody installed
// dependencies. Being a hook is what made the last one skippable; being a *misnamed*
// hook would make this one fire when nobody asked.

import { spawnSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");

function step(title, command, args, env = {}) {
  console.log(`\ncolai: ${title}`);
  const ran = spawnSync(command, args, {
    cwd: root,
    stdio: "inherit",
    encoding: "utf8",
    env: { ...process.env, ...env },
  });
  if (ran.status !== 0) {
    console.error(`\ncolai: ${title} — failed. Nothing was published.`);
    process.exit(ran.status ?? 1);
  }
}

// ── 1. is the binary one a stranger can run ──────────────────────────────────
//
// Glibc floor read out of the binary, no home directory inside it, digest matching, and
// `files` holding everything the plugin needs to start.
step("checking the toolbar is shippable", process.execPath, [join(here, "check-shippable.mjs")]);

// ── 2. build the runtime, and stamp it with the binary's digest ──────────────
//
// After the check, so the digest stamped in is the digest of a binary that passed it.
step("building the plugin runtime", process.execPath, [join(here, "build-runtime.mjs")], {
  COLAI_STAMP_DIGEST: "1",
});

// ── 3. say what would actually ship ──────────────────────────────────────────
//
// `--ignore-scripts` so this dry run behaves the way ClawHub's real one will, rather than
// re-entering the two steps above.
console.log("\ncolai: what would be published");
const packed = spawnSync(
  "npm",
  ["pack", "--dry-run", "--json", "--ignore-scripts", "--pack-destination", "/tmp"],
  { cwd: root, encoding: "utf8" },
);
if (packed.status !== 0) {
  console.error(packed.stderr || packed.stdout);
  console.error("\ncolai: npm could not pack this. Nothing was published.");
  process.exit(1);
}

const [report] = JSON.parse(packed.stdout);
const megabytes = (bytes) => `${(bytes / 1024 / 1024).toFixed(1)} MiB`;
console.log(`  ${report.name}@${report.version}`);
console.log(`  ${report.entryCount} files, ${megabytes(report.size)} packed`);

// The runtime the host will actually load, and the binary it will actually spawn. Named
// individually because a tarball that is the right size and is missing one of these
// installs cleanly and then does nothing.
const shipped = new Set(report.files.map((file) => file.path));
const needed = [
  "dist/index.js",
  "bin/colai-toolbar",
  "bin/colai-toolbar.sha256",
  "README.md",
  "LICENSE",
];
const absent = needed.filter((file) => !shipped.has(file));
if (absent.length > 0) {
  console.error(`\ncolai: the tarball is missing ${absent.join(", ")}. Nothing was published.`);
  process.exit(1);
}

// ── 4. and that the stamp took ───────────────────────────────────────────────
const digest = readFileSync(join(root, "bin", "colai-toolbar.sha256"), "utf8").trim();
const runtime = readFileSync(join(root, "dist", "index.js"), "utf8");
if (!runtime.includes(digest)) {
  console.error("\ncolai: the runtime was not stamped with the binary's digest.");
  console.error("Without it, deleting the .sha256 beside the binary disables the check.");
  process.exit(1);
}

console.log("\ncolai: ready to publish.");
console.log("  npx clawhub@0.23.3 package publish . --dry-run");
console.log("  npx clawhub@0.23.3 package publish .");
