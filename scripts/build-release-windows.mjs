// Build the Windows toolbar that ships, from a directory that names nobody.
//
// `build-toolbar.mjs` remaps the paths it can — the cargo and rustup homes, the crate — but
// Tauri also embeds its build context, the crate's absolute path, and `--remap-path-prefix`
// cannot rewrite that one. Built in place, the published Windows binary would carry
// `C:\Users\<name>\...` for everyone who installs it to read. The Linux release avoids this by
// building at `/build` inside Docker (see `release/`); Windows has no such container here, so
// this copies the crate to a neutral path on the system drive, builds it there with every home
// remapped, and then REFUSES to stage a binary that still contains the builder's user name or
// home directory — a check, not a hope.
//
// Usage (Windows): npm run build:release:windows
// It stages bin/colai-toolbar.built.exe and then runs stage-windows.mjs, which writes
// platforms/windows-x64/bin/{colai-toolbar.gz, .sha256, .build.json}.

import { spawnSync } from "node:child_process";
import { copyFileSync, cpSync, existsSync, readFileSync, renameSync, rmSync } from "node:fs";
import { userInfo } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

if (process.platform !== "win32") {
  console.error("colai: build-release-windows builds the Windows toolbar, on Windows. On Linux use `npm run build:release`.");
  process.exit(1);
}

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const home = process.env.USERPROFILE || process.env.HOME || "";
const user = userInfo().username;
// The system drive's root is the one writable path every Windows machine has that is not
// somebody's profile.
const neutral = join(`${process.env.SystemDrive || "C:"}\\`, "colai-build");
const crate = join(neutral, "toolbar", "src-tauri");
const target = join(neutral, "target");

console.error(`colai: copying the crate to ${neutral} so its build path names nobody…`);
rmSync(join(neutral, "toolbar"), { recursive: true, force: true });
cpSync(join(root, "toolbar"), join(neutral, "toolbar"), {
  recursive: true,
  filter: (src) => !/[\\/](target|node_modules)([\\/]|$)/.test(src),
});

const remap = [
  [process.env.CARGO_HOME || join(home, ".cargo"), "/cargo"],
  [process.env.RUSTUP_HOME || join(home, ".rustup"), "/rustup"],
  [neutral, "/colai"],
  [home, "/home"],
].map(([from, to]) => `--remap-path-prefix=${from}=${to}`);

console.error("colai: building the release toolbar (the first run compiles everything)…");
const built = spawnSync("cargo", ["build", "--release", "--locked"], {
  cwd: crate,
  stdio: "inherit",
  env: {
    ...process.env,
    CARGO_TARGET_DIR: target,
    RUSTFLAGS: [process.env.RUSTFLAGS ?? "", ...remap].filter(Boolean).join(" "),
  },
});
if (built.status !== 0) {
  console.error("colai: the release build failed; nothing was staged.");
  process.exit(built.status ?? 1);
}

const exe = join(target, "release", "colai-toolbar.exe");
const bytes = readFileSync(exe);
// Searched as both 8-bit and UTF-16LE text: Windows keeps some strings (resources, wide
// paths) as UTF-16, where a plain search for the name would never match.
const haystacks = [bytes.toString("latin1").toLowerCase(), bytes.toString("utf16le").toLowerCase()];
const needles = [...new Set([user, home].filter((s) => s && s.length >= 3).map((s) => s.toLowerCase()))];
const leaks = needles.filter((needle) => haystacks.some((hay) => hay.includes(needle)));
if (leaks.length) {
  console.error(`colai: REFUSING to stage — the binary still contains ${leaks.map((l) => `"${l}"`).join(" and ")}.`);
  console.error("Find where with: grep -aoE \".{0,60}<name>.{0,40}\" on the exe, and remap or remove that path.");
  process.exit(1);
}
console.error(`colai: checked — no trace of "${user}" or the home directory in the binary.`);

// Staged under a temporary name and then swapped in, so a running toolbar holding the old
// file open fails this step loudly rather than leaving a half-written binary behind.
const staged = join(root, "bin", "colai-toolbar.built.exe");
const arriving = `${staged}.arriving`;
copyFileSync(exe, arriving);
try {
  renameSync(arriving, staged);
} catch (error) {
  rmSync(arriving, { force: true });
  console.error(`colai: built and checked, but bin/ is in use (${error.code}). Quit the toolbar (colai-toolbar quit) and run this again.`);
  process.exit(1);
}

const stage = spawnSync(process.execPath, [join(root, "scripts", "stage-windows.mjs")], { stdio: "inherit" });
process.exit(stage.status ?? 1);
