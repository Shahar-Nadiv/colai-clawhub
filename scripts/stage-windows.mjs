// Stage the Windows toolbar for shipping, the way `build-release.mjs` stages Linux.
//
// Windows cannot be built in the neutral Linux container `build-release.mjs` uses, so the
// `.exe` is produced on a real Windows machine by `npm run build:toolbar` — which already
// remaps the build-host path out of the binary (see the RUSTFLAGS there). This script takes
// that freshly built `.exe` and writes the three tracked files the launcher reads, mirroring
// `platforms/linux-x64/bin/` exactly:
//
//   colai-toolbar.gz          the binary, gzip level 9 — what actually ships
//   colai-toolbar.sha256      the sha256 of the UNPACKED binary, `<hex>\n`
//   colai-toolbar.build.json  provenance, the same fields Linux records
//
// The names carry no `.exe`: `bin/colai-toolbar` (the POSIX launcher) reads
// `platforms/windows-x64/bin/colai-toolbar.gz` and `.sha256` under those exact names, and
// only the copy it unpacks into its cache gets the `.exe` suffix.

import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { gzipSync } from "node:zlib";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const WHICH = "windows-x64";

// The input is the release `.exe` `build:toolbar` leaves in `bin/`. It is not rebuilt here:
// building is minutes of cargo, and this step is only the packaging around it.
const builtExe = join(root, "bin", "colai-toolbar.built.exe");
if (!existsSync(builtExe)) {
  console.error("colai: no Windows toolbar to stage — bin/colai-toolbar.built.exe is missing.");
  console.error("Build one first with: npm run build:toolbar");
  process.exit(1);
}

const binary = readFileSync(builtExe);

// The digest is of the UNPACKED binary — the same thing the launcher hashes after it
// gunzips the archive, and the same thing Linux records in its `.sha256`.
const digest = createHash("sha256").update(binary).digest("hex");

const home = join(root, "platforms", WHICH, "bin");
mkdirSync(home, { recursive: true });

// `colai-toolbar`, no `.exe`: the launcher's archive and digest paths carry no suffix.
const staged = join(home, "colai-toolbar");

writeFileSync(`${staged}.sha256`, `${digest}\n`);
writeFileSync(
  `${staged}.build.json`,
  `${JSON.stringify(
    {
      // No neutral build image and no glibc floor on Windows: the webview is WebView2,
      // shipped by the OS, not linked in. The target triple is the honest analogue of the
      // Linux image line, and glibc is recorded as `null` rather than faked.
      image: "x86_64-pc-windows-msvc",
      glibc: null,
      at: new Date().toISOString().slice(0, 10),
      sha256: digest,
    },
    null,
    2,
  )}\n`,
);

// Level 9: this runs once at release time and every install pays for the difference.
//
// GNU gzip's -9 when the machine has it (Git for Windows ships one), because zlib's level 9
// is not the same search: on this toolbar it came out 139 KB bigger. `-n` leaves the name
// and time out of the header, so staging the same binary twice writes the same archive.
// zlib remains the fallback — the digest is of the unpacked binary, and either archive
// unpacks to the same bytes.
//
// `npm run` from PowerShell does not put Git's `usr/bin` on PATH, so a bare `gzip` is not
// found there; the one beside `git` is looked for as well, the way plugin.test.ts finds `sh`.
function gzipCandidates() {
  const found = ["gzip"];
  const git = spawnSync("where", ["git"], { encoding: "utf8" });
  const gitExe = git.stdout?.split(/\r?\n/).map((l) => l.trim()).find(Boolean);
  if (gitExe) found.push(join(dirname(dirname(gitExe)), "usr", "bin", "gzip.exe")); // <root>\cmd\git.exe
  found.push("C:\\Program Files\\Git\\usr\\bin\\gzip.exe");
  return found;
}
function gzip9(bytes) {
  for (const gzip of gzipCandidates()) {
    const ran = spawnSync(gzip, ["-9", "-n", "-c"], { input: bytes, maxBuffer: 1 << 30 });
    if (!ran.error && ran.status === 0 && ran.stdout.length > 0) return { packed: ran.stdout, by: gzip };
  }
  return { packed: gzipSync(bytes, { level: 9 }), by: "zlib" };
}
const archive = `${staged}.gz`;
const { packed, by } = gzip9(binary);
writeFileSync(archive, packed);

const asMegabytes = (bytes) => `${(bytes / 1024 / 1024).toFixed(1)} MB`;
console.log(
  `colai: staged at platforms/${WHICH}/bin/colai-toolbar.gz — ` +
    `${asMegabytes(binary.length)} unpacked, ${asMegabytes(statSync(archive).size)} packed ` +
    `(by ${by === "zlib" ? "zlib" : "gzip"}).`,
);
console.log(`colai: sha256 ${digest}`);
