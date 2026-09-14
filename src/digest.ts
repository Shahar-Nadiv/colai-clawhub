// Is the binary about to run the one that was built?
//
// Its own module because it is the one security decision this plugin makes on its own,
// and it should be readable and testable without a running Gateway.

import { createHash } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";

/**
 * Whether the binary about to be spawned is the one that was built.
 *
 * The package ships a native program that runs as the user, photographs the screen and
 * authenticates to the Gateway. npm proves nothing about what is inside a tarball, so the
 * build writes a digest beside the binary and this checks it before anything runs.
 *
 * It is not a signature: whoever can replace the binary can replace the digest next to
 * it. What it closes is the narrower and likelier case — an artifact tampered with in
 * transit or on disk — and it turns a silent substitution into a refusal.
 *
 * The digest is written into this file at publish time rather than only sitting beside the
 * binary, and the built-in one wins. A digest in a file next to the thing it describes can
 * be deleted by anybody who can replace the thing it describes — and deleting it used to
 * make the check disappear without a word, which is the one way a gate must never fail.
 *
 * No digest of either kind means a developer build, straight out of `target/`. Those are
 * not staged and have nothing to compare against; refusing them would mean refusing to run
 * the thing somebody just compiled.
 */
declare const COLAI_BUILT_DIGEST: string;

/** What `scripts/prepublish.mjs` wrote in. Empty in a checkout. */
const BUILT_DIGEST = typeof COLAI_BUILT_DIGEST === "string" ? COLAI_BUILT_DIGEST.trim() : "";

export function notWhatWasBuilt(binary: string): string | null {
  const beside = `${binary}.sha256`;
  const hasBeside = existsSync(beside);
  if (!BUILT_DIGEST && !hasBeside) {
    return null;
  }
  let expected: string;
  let actual: string;
  try {
    // The published digest is the authority. The sibling file is what a local release
    // build has and a published package also carries, and it is only consulted when
    // nothing was written in.
    expected = BUILT_DIGEST || readFileSync(beside, "utf8").trim();
    actual = createHash("sha256").update(readFileSync(binary)).digest("hex");
  } catch (error) {
    return `could not check the toolbar against its digest: ${String(error)}`;
  }
  if (expected !== actual) {
    return (
      `the toolbar does not match the digest shipped beside it, so it was not started. ` +
      `Expected ${expected}, found ${actual}. Reinstall the plugin.`
    );
  }
  return null;
}
