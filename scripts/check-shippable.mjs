// Refuse to pack a toolbar that was built on the wrong machine.
//
// Two properties of the binary are decided by the machine that compiled it and by
// nothing in this repository: the oldest Linux it will run on, and whose home directory
// is embedded in it. A developer build on a current desktop gets both wrong — it once
// shipped with a GLIBC_2.39 floor, which is Ubuntu 24.04 and almost nothing else, and
// with the author's home directory inside it — and it gets them wrong silently. The
// tarball is the right size, the digest matches, the install succeeds, and the failure
// arrives on a stranger's machine as a window that never opens.
//
// So the build records where it happened, and this is the gate that reads it. Building
// locally stays exactly as easy as it was; only *publishing* one is refused.
//
// It used to read only that record. A note saying "built for release" is a claim about
// the binary, not the binary — and the three things the note is a claim *about* are all
// cheap to measure directly, so now they are measured. What is left of the note is the
// part that cannot be measured: where and when.
//
// One more thing worth knowing, and the reason `ship.mjs` exists: this runs as a
// `prepack` hook, and `clawhub package pack` shells out to `npm pack --ignore-scripts`.
// On the actual publishing path, nothing here runs at all.

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
/*
 * Which build this run is about.
 *
 * There is one binary per machine now, each in a platform package of its own. This script
 * measures one of them — the Linux build by default, because that is the one this machine
 * can produce; CI passes the name of whichever it just built.
 *
 * The glibc and home-directory checks below are Linux questions and are skipped for any
 * other build. That is not a hole: a macOS binary has its own things to check and no
 * `readelf` to check them with, and pretending otherwise would be a check that passes by
 * not looking.
 */
const which = process.argv[2] ?? "linux-x64";
const home = join(root, "platforms", which);
const binary = join(home, "bin", "colai-toolbar");
const provenance = `${binary}.build.json`;
const digestFile = `${binary}.sha256`;

/** The oldest glibc a published toolbar may demand. Jammy, which the release image is. */
const OLDEST_SUPPORTED = "2.35";

/** What the tarball is useless without, whatever else is in it. */
// What the *wrapper* must ship. The binary is not on this list any more and must not be:
// it lives in the platform packages, and a wrapper carrying one would send every Linux
// user a macOS binary they can never run. `MUST_NOT_SHIP` below is the other half.
const MUST_SHIP = ["dist/", "toolbar/ui/", "openclaw.plugin.json", "README.md", "LICENSE"];

function older(left, right) {
  const [leftMajor, leftMinor] = left.split(".").map(Number);
  const [rightMajor, rightMinor] = right.split(".").map(Number);
  return leftMajor !== rightMajor ? leftMajor - rightMajor : leftMinor - rightMinor;
}

const complaints = [];

/**
 * The glibc floor, read out of the binary rather than out of a note beside it.
 *
 * `readelf -V` lists every versioned symbol the loader will demand; the highest `GLIBC_`
 * among them is the oldest release that can satisfy them all. Where `readelf` is missing
 * this says so and falls back to the note, because a publish machine without binutils is
 * unusual enough to be worth reporting rather than guessing about.
 */
function glibcFloor() {
  let listed;
  try {
    listed = execFileSync("readelf", ["-V", binary], { encoding: "utf8", maxBuffer: 32 << 20 });
  } catch {
    return null;
  }
  const versions = [...listed.matchAll(/GLIBC_(\d+)\.(\d+)/g)].map(([, major, minor]) => [
    Number(major),
    Number(minor),
  ]);
  if (versions.length === 0) return null;
  versions.sort((left, right) => (left[0] !== right[0] ? left[0] - right[0] : left[1] - right[1]));
  const [major, minor] = versions[versions.length - 1];
  return `${major}.${minor}`;
}

// ── the escape hatch, unchanged ───────────────────────────────────────────────
if (process.env.COLAI_ALLOW_DEV_BUILD === "1") {
  console.warn("colai: packing a developer build because COLAI_ALLOW_DEV_BUILD=1.");
  console.warn("colai: do not publish this tarball — it runs only on this machine's Linux.");
  process.exit(0);
}

// ── was it built for release at all ───────────────────────────────────────────
if (!existsSync(provenance)) {
  console.error("colai: this toolbar was not built for release, so it will not be packed.");
  console.error("");
  console.error("A binary built here inherits this machine's glibc as the oldest Linux it");
  console.error("can run on, and carries this checkout's path inside it. Build it in the");
  console.error("release image instead:");
  console.error("");
  console.error("  npm run build:release");
  console.error("");
  console.error("To pack one for local testing anyway: COLAI_ALLOW_DEV_BUILD=1 npm pack");
  process.exit(1);
}

const built = JSON.parse(readFileSync(provenance, "utf8"));

// ── the glibc floor, measured ────────────────────────────────────────────────
// The glibc floor is a question about an ELF; a Mach-O has nothing to answer it with.
const onLinux = which.startsWith("linux-");
const measured = onLinux ? glibcFloor() : null;
if (onLinux && measured === null) {
  console.warn("colai: readelf is not available, so the glibc floor is taken on trust.");
}
const floor = measured ?? built.glibc ?? "99.99";
if (older(floor, OLDEST_SUPPORTED) > 0) {
  complaints.push(
    `it needs glibc ${floor}, and may not need more than ${OLDEST_SUPPORTED}. It would ` +
      "refuse to start on every Linux older than the one it was built on.",
  );
}
if (measured !== null && built.glibc && measured !== built.glibc) {
  // The note and the binary disagree, which means one of them is about a different file.
  complaints.push(
    `its provenance note says glibc ${built.glibc} but the binary demands ${measured}. ` +
      "The note is describing a different build.",
  );
}

// ── nobody's home directory inside it ────────────────────────────────────────
//
// The property `release/Dockerfile` exists to guarantee, tested rather than assumed.
// Tauri embeds its build context path and `--remap-path-prefix` cannot reach it, so this
// is the one check that actually proves the release image did its job.
const bytes = readFileSync(binary);
const homes = onLinux ? [...bytes.toString("latin1").matchAll(/\/home\/[A-Za-z0-9._-]+/g)] : [];
if (homes.length > 0) {
  const seen = [...new Set(homes.map(([found]) => found))].slice(0, 3);
  complaints.push(
    `it carries somebody's home directory inside it — ${seen.join(", ")}. That ships to ` +
      "every person who installs it. Build it in the release image: npm run build:release",
  );
}

// ── the digest describes this binary ─────────────────────────────────────────
if (!existsSync(digestFile)) {
  complaints.push("the digest that should sit beside it is missing.");
} else {
  const expected = readFileSync(digestFile, "utf8").trim();
  const actual = createHash("sha256").update(bytes).digest("hex");
  if (expected !== actual) {
    complaints.push(
      `its digest describes a different binary — the file beside it says ${expected}, ` +
        `the binary hashes to ${actual}.`,
    );
  }
}

// ── everything it needs at runtime is allowed into the tarball ───────────────
const manifest = JSON.parse(readFileSync(join(root, "package.json"), "utf8"));
// The binary travels in `@colai/toolbar-<platform>`, never in the wrapper. `files` is an
// allowlist, so this can only go wrong by somebody adding it back on purpose — which is
// exactly the change that would quietly put the tarball back over the registry's limit.
const shipped = manifest.files.filter((file) => file.startsWith("bin/"));
if (shipped.length > 0) {
  complaints.push(
    `\`files\` ships ${shipped.join(", ")}. The binary belongs to the platform packages ` +
      "under `platforms/`, not to the wrapper.",
  );
}

const missing = MUST_SHIP.filter((needed) => !manifest.files.includes(needed));
if (missing.length > 0) {
  complaints.push(
    `\`files\` leaves out ${missing.join(", ")}, so the tarball would install a plugin ` +
      "that cannot run.",
  );
}

if (complaints.length > 0) {
  console.error("colai: this toolbar will not be packed.");
  for (const complaint of complaints) {
    console.error("");
    console.error(`  · ${complaint}`);
  }
  console.error("");
  process.exit(1);
}

console.log(
  `colai: shippable — built ${built.at} in ${built.image}, runs on glibc ${floor} and newer, ` +
    "no home directory inside it, digest matches.",
);
