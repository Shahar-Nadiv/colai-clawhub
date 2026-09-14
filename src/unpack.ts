// Lay the shipped toolbar out, the first time anything needs it.
//
// The package ships the binary compressed. Not to be clever about download size — the
// registry takes files up to 10 MB each and the binary is 12.4 MB, so an uncompressed one
// cannot be published at all. Gzip brings it to 5 MB, which is also the whole reason this
// file exists rather than the obvious `opt-level = "s"`: shrinking the binary by
// de-optimising it would make every user's toolbar slower forever to satisfy an upload
// form. Unpacking once, on first use, costs a fraction of a second and nothing after that.
//
// There is deliberately no digest check in here. `notWhatWasBuilt` already runs on the
// path this produces, before anything is spawned, and it hashes whatever is actually on
// disk — so a truncated or tampered archive is caught there, by the gate that exists for
// it, rather than by a second half-copy of the same idea.

import { existsSync, readFileSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { gunzipSync } from "node:zlib";

/**
 * Put the toolbar at `binary`, unpacking `archive` if it is not there yet.
 *
 * Returns null when the binary is in place, or a sentence saying why it is not.
 */
export function layOutTheToolbar(archive: string, binary: string): string | null {
  if (existsSync(binary)) {
    return null;
  }
  let bytes: Buffer;
  try {
    bytes = gunzipSync(readFileSync(archive));
  } catch (error) {
    return `could not unpack ${archive}: ${String(error)}`;
  }
  /*
   * Written under a name nothing will run, then moved into place.
   *
   * Two things can ask for the toolbar at once — the CLI and the autostart both call
   * this — and `rename` within a directory is atomic where writing 12 MB is not. Without
   * it the second caller can find a file that exists, is the right name, and is half
   * written, and hand it to `spawn`. The PID keeps the two temporaries apart; whichever
   * rename lands last wins, and both wrote identical bytes.
   */
  const partial = `${binary}.${process.pid}.partial`;
  try {
    writeFileSync(partial, bytes, { mode: 0o755 });
    renameSync(partial, binary);
  } catch (error) {
    try {
      rmSync(partial, { force: true });
    } catch {
      // The rename is what mattered; a leftover temporary is not worth a second failure.
    }
    return (
      `could not write the toolbar to ${binary}: ${String(error)}. ` +
      "The plugin directory has to be writable the first time the toolbar runs."
    );
  }
  return null;
}
