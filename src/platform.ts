// Which build of the toolbar this machine needs, and where npm put it.
//
// The toolbar is a native program, so there is one of it per operating system and
// architecture. Shipping them all in one package would mean every Linux user downloading a
// macOS binary they can never run, and it would put the package over the registry's size
// limits besides.
//
// So the published `@colai/toolbar` carries no binary at all. It declares every build as an
// *optional* dependency, and each of those declares the `os` and `cpu` it is for. npm then
// installs exactly one of them — the one that matches — and silently skips the rest. This
// is how esbuild, swc and rollup ship, and OpenClaw supports it directly: the names below
// are repeated in `openclaw.install.requiredPlatformPackages`, which makes the host verify
// the matching one actually arrived, retry once with a cold cache, and roll the install
// back if it did not.
//
// Nothing runs at install time to make this happen. OpenClaw installs plugins with
// `--ignore-scripts`, always, with no way to opt out — so a package that downloads its own
// binary in a `postinstall` would simply never run it.

import { createRequire } from "node:module";
import { dirname, join } from "node:path";

/**
 * Every machine there is a build for.
 *
 * Keyed the way Node names things, so the lookup is `process.platform` and `process.arch`
 * with nothing in between to get wrong.
 */
const BUILDS: Record<string, { package: string; says: string }> = {
  "linux-x64": { package: "@colai/toolbar-linux-x64", says: "Linux on x86-64" },
  // `platforms/darwin-arm64/` is scaffolded and deliberately absent from this table.
  //
  // A name here is a promise that npm can fetch the package, and there is no macOS binary
  // yet. Listing it would mean a Mac install that succeeds, skips the optional dependency
  // it cannot find, and then reports the build "is not installed" — which sends somebody
  // looking for a failed download instead of telling them the truth. Left out, the same
  // machine gets `noBuildFor`, which says there is no Mac build and names what there is.
  //
  // Adding it is one line here, one in `optionalDependencies`, and one in
  // `requiredPlatformPackages` — once a binary exists to stand behind them.
};

export function buildFor(
  platform: string = process.platform,
  arch: string = process.arch,
): { package: string; says: string } | null {
  return BUILDS[`${platform}-${arch}`] ?? null;
}

/**
 * What to say to somebody whose machine has no build.
 *
 * Named, and with the list beside it. "Cannot find module" is what this used to look like
 * from the outside, and it sends a person looking for a broken install rather than telling
 * them the truth — that their computer is not one colai has been built for yet.
 */
export function noBuildFor(
  platform: string = process.platform,
  arch: string = process.arch,
): string {
  const built = Object.values(BUILDS)
    .map((build) => build.says)
    .join(", ");
  return (
    `The colai toolbar has no build for ${platform} ${arch}. ` +
    `It runs on ${built}. The toolbar is a native desktop program, so each one has to be ` +
    `built and tested on the machine it is for.`
  );
}

/**
 * The directory npm installed this machine's build into, if it did.
 *
 * `createRequire` rather than a path walked by hand: the platform package is a sibling of
 * this one inside `node_modules`, except when a package manager hoists it somewhere else,
 * and Node's own resolver is the only thing that knows which. Resolving the manifest rather
 * than the package name works whether or not the package declares an entry point — and it
 * declares none, because it holds one binary and no code.
 *
 * Null covers two ordinary cases and one bad one: a checkout with nothing installed, a
 * machine with no build, and an install where the optional dependency did not arrive. The
 * caller tells them apart — it is the difference between "go and build it" and "reinstall".
 */
export function whereTheBuildIs(from: string): string | null {
  const build = buildFor();
  if (!build) {
    return null;
  }
  try {
    const resolve = createRequire(from);
    return dirname(resolve.resolve(`${build.package}/package.json`));
  } catch {
    return null;
  }
}

/** Where the binary sits inside a platform package, and inside a checkout. */
export function binaryUnder(root: string): { binary: string; archive: string } {
  const binary = join(root, "bin", "colai-toolbar");
  return { binary, archive: `${binary}.gz` };
}
