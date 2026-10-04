// Stage the macOS toolbar for shipping, the way `stage-windows.mjs` stages Windows.
//
// A Mac binary cannot be built in the neutral Linux container `build-release.mjs` uses, nor
// cross-linked from Windows, so it is produced on a real Mac by `npm run build:toolbar` —
// which already remaps the build-host path out of the binary (see the RUSTFLAGS there) and
// pins the oldest macOS it starts on. This script takes that freshly built binary and writes
// the three tracked files the launcher reads, mirroring `platforms/linux-x64/bin/` exactly:
//
//   colai-toolbar.gz          the binary, gzip level 9 — what actually ships
//   colai-toolbar.sha256      the sha256 of the UNPACKED binary, `<hex>\n`
//   colai-toolbar.build.json  provenance, the same fields Linux records
//
// Which platform directory it lands in is read from the binary itself, not from the machine
// running this: an Intel build copied to an Apple Silicon Mac is still an Intel build, and
// staging it under darwin-arm64 would hand every M-series Mac a toolbar for the wrong CPU.

import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { gzipSync } from "node:zlib";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

// The input is the release binary `build:toolbar` leaves in `bin/` on a Mac — no suffix.
const built = join(root, "bin", "colai-toolbar.built");
if (!existsSync(built)) {
  console.error("colai: no macOS toolbar to stage — bin/colai-toolbar.built is missing.");
  console.error("Build one first, on a Mac, with: npm run build:toolbar");
  process.exit(1);
}

let binary = readFileSync(built);

// A thin 64-bit Mach-O starts `cf fa ed fe` (MH_MAGIC_64, little-endian) and names its CPU
// in the next four bytes. Anything else — a universal binary, an ELF from a Linux build, a
// PE from Windows — is not something the launcher knows how to place, so it is refused here
// rather than shipped to the wrong machine.
const CPUS = {
  0x0100000c: { which: "darwin-arm64", triple: "aarch64-apple-darwin" },
  0x01000007: { which: "darwin-x64", triple: "x86_64-apple-darwin" },
};
const isMachO64 =
  binary.length >= 8 && binary[0] === 0xcf && binary[1] === 0xfa && binary[2] === 0xed && binary[3] === 0xfe;
const cpu = isMachO64 ? CPUS[binary.readUInt32LE(4)] : undefined;
if (!cpu) {
  console.error(
    "colai: bin/colai-toolbar.built is not a single-architecture macOS binary, so there is no " +
      "platform directory to stage it in. Build it on a Mac with: npm run build:toolbar",
  );
  process.exit(1);
}
const { which: WHICH, triple } = cpu;

// Ad-hoc signed before it is hashed. Apple Silicon refuses to run arm64 code with no
// signature at all — the kernel kills it on exec — and the linker's automatic ad-hoc
// signature is only as good as nothing having touched the file since. Re-signing here is
// cheap, and the digest below is then of the exact bytes a Mac will be asked to run.
//
// Ad-hoc is not a Developer ID: it satisfies the kernel, not Gatekeeper. That is enough for
// a binary that arrives in a git clone and is unpacked by a shell script, because neither
// sets the quarantine flag Gatekeeper acts on. A notarized build is a later step.
if (process.platform === "darwin") {
  const scratch = mkdtempSync(join(tmpdir(), "colai-stage-"));
  const signing = join(scratch, "colai-toolbar");
  try {
    copyFileSync(built, signing);
    for (const args of [
      ["--force", "--sign", "-", signing],
      ["--verify", signing],
    ]) {
      const ran = spawnSync("codesign", args, { encoding: "utf8" });
      if (ran.error || ran.status !== 0) {
        console.error(`colai: codesign ${args[0]} failed: ${ran.stderr || String(ran.error)}`);
        process.exit(1);
      }
    }
    binary = readFileSync(signing);
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
} else {
  console.warn(
    "colai: not on a Mac, so the binary is not re-signed here; the linker's ad-hoc signature " +
      "is relied on. Stage on a Mac if anything has touched the file since it was linked.",
  );
}

// The digest is of the UNPACKED, signed binary — the same thing the launcher hashes after it
// gunzips the archive, and the same thing Linux and Windows record in their `.sha256`.
const digest = createHash("sha256").update(binary).digest("hex");

const home = join(root, "platforms", WHICH, "bin");
mkdirSync(home, { recursive: true });
const staged = join(home, "colai-toolbar");

writeFileSync(`${staged}.sha256`, `${digest}\n`);
writeFileSync(
  `${staged}.build.json`,
  `${JSON.stringify(
    {
      // No neutral build image and no glibc floor on a Mac: the webview is WKWebView, shipped
      // by the OS. The target triple stands in for the Linux image line, glibc is `null`
      // rather than faked, and `macos` is the oldest release the binary agrees to start on.
      image: triple,
      glibc: null,
      macos: process.env.MACOSX_DEPLOYMENT_TARGET ?? "11.0",
      at: new Date().toISOString().slice(0, 10),
      sha256: digest,
    },
    null,
    2,
  )}\n`,
);

// Level 9: this runs once at release time and every install pays for the difference.
//
// The system gzip's -9 when there is one (every Mac has it), because zlib's level 9 is not
// the same search and came out larger on the Windows toolbar by 139 KB. `-n` leaves the name
// and time out of the header, so staging the same binary twice writes the same archive.
// zlib remains the fallback — the digest is of the unpacked binary, and either archive
// unpacks to the same bytes.
function gzip9(bytes) {
  const ran = spawnSync("gzip", ["-9", "-n", "-c"], { input: bytes, maxBuffer: 1 << 30 });
  if (!ran.error && ran.status === 0 && ran.stdout.length > 0) return ran.stdout;
  return gzipSync(bytes, { level: 9 });
}
const archive = `${staged}.gz`;
writeFileSync(archive, gzip9(binary));

const asMegabytes = (bytes) => `${(bytes / 1024 / 1024).toFixed(1)} MB`;
console.log(
  `colai: staged at platforms/${WHICH}/bin/colai-toolbar.gz — ` +
    `${asMegabytes(binary.length)} unpacked, ${asMegabytes(statSync(archive).size)} packed.`,
);
console.log(`colai: sha256 ${digest}`);
