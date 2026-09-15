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
//     npx clawhub@0.23.3 package pack . --pack-destination .pack
//     npx clawhub@0.23.3 package publish .pack/<tarball> --source-repo <owner/repo> \
//       --source-commit <sha>
//
// Two details that each cost a rejected publish.
//
// The argument is a *source*, not a package name: a folder, `owner/repo` on GitHub, or a
// packed `.tgz`. `colai/toolbar` looked like a package name, was read as a GitHub
// repository, and answered `GitHub repo not found`.
//
// And it has to be the packed tarball, not the folder. Publishing a folder uploads every
// file as its own multipart part — for this package 92 of them and 13.6 MB of raw bytes,
// which the registry refuses with a bare `413 Request Entity Too Large`. The tarball is
// one 5.3 MB part because it is compressed, and it is the only path with a fallback: over
// 18 MB the CLI stages it to storage first rather than failing.
//
// Publishing a tarball cannot read the git checkout it came from, so the provenance has to
// be passed. It must name a commit that is *pushed* — ClawHub records it as the source a
// reviewer reads, and a local-only SHA points at nothing.
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
const needed = ["dist/index.js", "README.md", "LICENSE"];
const absent = needed.filter((file) => !shipped.has(file));
if (absent.length > 0) {
  console.error(`\ncolai: the tarball is missing ${absent.join(", ")}. Nothing was published.`);
  process.exit(1);
}

const manifest = JSON.parse(readFileSync(join(root, "package.json"), "utf8"));

// ── 4. every build the wrapper promises, and a digest for each ──────────────
//
// The wrapper declares its platform packages twice — once as `optionalDependencies`, which
// is how npm picks one, and once as `requiredPlatformPackages`, which is how OpenClaw
// checks npm picked one. Publishing a wrapper that names a build nobody staged produces an
// install that succeeds and then cannot find its toolbar, on that platform only, which is
// the worst shape of bug: invisible to whoever published it.
const runtime = readFileSync(join(root, "dist", "index.js"), "utf8");
const promised = Object.keys(manifest.optionalDependencies ?? {});
const declared = manifest.openclaw?.install?.requiredPlatformPackages ?? [];

const disagree = promised.filter((name) => !declared.includes(name));
if (disagree.length > 0) {
  console.error(`\ncolai: ${disagree.join(", ")} is an optional dependency but is not in`);
  console.error("openclaw.install.requiredPlatformPackages, so the host will not verify it.");
  process.exit(1);
}

for (const name of promised) {
  const which = name.slice("@colai/toolbar-".length);
  const beside = join(root, "platforms", which, "bin", "colai-toolbar.sha256");
  if (!existsSync(beside)) {
    console.error(`\ncolai: ${name} is promised but nothing is staged in platforms/${which}.`);
    console.error("Build it first, or take it out of optionalDependencies. Nothing was published.");
    process.exit(1);
  }
  // The stamp is what makes the gate survive somebody deleting the digest beside the
  // binary, and there is one per build — so a wrapper missing one silently stops checking
  // that platform while still checking the others.
  const digest = readFileSync(beside, "utf8").trim();
  if (!runtime.includes(digest)) {
    console.error(`\ncolai: the runtime carries no digest for ${name}.`);
    console.error("Without it, deleting the .sha256 beside that binary disables the check.");
    process.exit(1);
  }
  console.log(`  ${name} — staged, digest ${digest.slice(0, 12)}…`);
}

// The commit a reviewer would be sent to. Reported rather than assumed: a publish that
// names an unpushed SHA is provenance pointing at nothing.
const head = spawnSync("git", ["rev-parse", "HEAD"], { cwd: root, encoding: "utf8" });
const sha = head.status === 0 ? head.stdout.trim() : "<sha>";
const pushed =
  spawnSync("git", ["branch", "-r", "--contains", sha], {
    cwd: root,
    encoding: "utf8",
  }).stdout?.trim() ?? "";
if (sha !== "<sha>" && !pushed) {
  console.error(`\ncolai: HEAD (${sha.slice(0, 12)}) is not on any remote branch.`);
  console.error("Push it first — ClawHub records it as the source people read.");
  process.exit(1);
}

console.log("\ncolai: ready to publish.");
console.log("  npx clawhub@0.23.3 package pack . --pack-destination .pack");
// The name npm gives the tarball: scope folded into the filename, `@` dropped.
const tarball = `${report.name.replace("@", "").replace("/", "-")}-${report.version}.tgz`;
// `owner/repo` out of the remote, whichever spelling it is written in.
const remote = spawnSync("git", ["remote", "get-url", "origin"], { cwd: root, encoding: "utf8" });
const repo =
  remote.stdout
    ?.trim()
    .replace(/\.git$/, "")
    .match(/[:/]([^/]+\/[^/]+)$/)?.[1] ?? "<owner/repo>";
console.log(`  npx clawhub@0.23.3 package publish .pack/${tarball} \\`);
console.log(`    --source-repo ${repo} --source-commit ${sha}`);
