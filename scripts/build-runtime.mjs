// Build the plugin's Node runtime and stage it for packing.
//
// The package declares `./index.ts` as its extension entry, and OpenClaw refuses to
// install a TypeScript entry without compiled output beside it — so `dist/index.js` is
// not an optimisation, it is the difference between a plugin that loads and one that
// does not.
//
// This exists because that output used to come from a script in the OpenClaw repository,
// which is not part of this package and is not on a publisher's disk. `dist/` is also
// ignored by git. Between the two, a publish from a clean checkout shipped every file
// except the one the host actually runs, and the failure arrived as "plugin not found"
// on somebody else's machine. Nothing here reaches outside this directory.
//
// Bundled rather than transpiled file-by-file: the entry pulls in `src/`, and a bundle
// is one file whose imports are exactly the things that must stay external — the host's
// own plugin SDK, which the host provides, and `zod`, which is a real dependency and
// must not be duplicated into the package.

import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, rmSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const entry = join(root, "index.ts");
const outDir = join(root, "dist");
const outFile = join(outDir, "index.js");

/**
 * Left to the host, not bundled.
 *
 * `openclaw/*` is the host itself — bundling it would embed a second copy of the plugin
 * SDK that could not see the running one. `zod` is a declared dependency, so npm installs
 * it; carrying it inside the bundle as well would ship it twice and make the two disagree
 * on identity, which is exactly the failure `instanceof` checks produce.
 */
const THEIRS = ["openclaw", "openclaw/*", "@openclaw/*", "zod"];

function esbuild() {
  // Resolved rather than assumed on PATH: this runs as `prepack`, where the only thing
  // guaranteed present is what this package itself declares.
  const local = join(root, "node_modules", ".bin", "esbuild");
  if (existsSync(local)) return local;
  // A workspace hoists its dependencies to the root it shares. Walking up is how npm
  // itself resolves a binary, and doing less than that would refuse to build inside the
  // checkout this package is developed in.
  let at = root;
  for (let up = 0; up < 6; up += 1) {
    at = dirname(at);
    const hoisted = join(at, "node_modules", ".bin", "esbuild");
    if (existsSync(hoisted)) return hoisted;
  }
  return null;
}

const build = esbuild();
if (!build) {
  console.error("colai: esbuild is missing, so the plugin runtime cannot be built.");
  console.error("It is a devDependency of this package. Install it: npm install");
  process.exit(1);
}

// Cleared first, so a stale file from an older layout cannot survive into the tarball and
// be loaded in preference to what was just built.
rmSync(outDir, { recursive: true, force: true });
mkdirSync(outDir, { recursive: true });

/**
 * The digest of the binary this runtime will be published beside.
 *
 * `digest.ts` prefers this over the `.sha256` sitting next to the binary, because a file
 * next to the thing it describes can be deleted by whoever replaces the thing it
 * describes — and an absent digest used to mean "developer build, nothing to check", so
 * deleting it made the check disappear without a word.
 *
 * Empty in a checkout, and empty is correct there: `npm run build` builds this *before*
 * `build-toolbar.mjs` produces the binary, so any digest taken here would be of the
 * previous one. It is only filled in by `scripts/prepublish.mjs`, which runs after a
 * release binary is staged and is the only path where a fixed digest means anything.
 */
function builtDigest() {
  if (process.env.COLAI_STAMP_DIGEST !== "1") return "";
  const binary = join(root, "bin", "colai-toolbar");
  if (!existsSync(binary)) {
    console.error("colai: asked to stamp the digest, but there is no binary to stamp.");
    process.exit(1);
  }
  return createHash("sha256").update(readFileSync(binary)).digest("hex");
}

const stamped = builtDigest();
if (stamped) console.log(`colai: stamping the runtime with digest ${stamped.slice(0, 12)}…`);

const ran = spawnSync(
  build,
  [
    entry,
    "--bundle",
    `--outfile=${outFile}`,
    "--platform=node",
    "--format=esm",
    `--define:COLAI_BUILT_DIGEST=${JSON.stringify(stamped)}`,
    // The floor the package already claims: `openclaw.install.minHostVersion` is
    // >=2026.9.1, whose Node is 22. Targeting lower would be a promise nothing keeps.
    "--target=node22",
    "--log-level=warning",
    ...THEIRS.map((one) => `--external:${one}`),
  ],
  { cwd: root, encoding: "utf8", stdio: "inherit" },
);

if (ran.status !== 0) {
  console.error("colai: the plugin runtime failed to build.");
  process.exit(ran.status ?? 1);
}

if (!existsSync(outFile)) {
  console.error(`colai: the build reported success but ${outFile} is not there.`);
  process.exit(1);
}

console.log("colai: plugin runtime built at dist/index.js.");
