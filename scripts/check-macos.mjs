// Type-check the toolbar for macOS, from any machine.
//
// The macOS code paths are `#[cfg(target_os = "macos")]`, so a Windows or Linux build never
// compiles them, and the first anybody hears of a typo in one is a Mac that will not build.
// Rust can type-check a foreign target without that target's linker, SDK or C compiler —
// `cargo check` stops before codegen and never links — so this is the guard that runs here,
// in CI on Windows and Linux, and on the Mac itself.
//
// One thing gets in the way off a Mac: `objc2-exception-helper` compiles a few lines of
// Objective-C in its build script, through `cc`, and there is no Apple clang to hand it to.
// So off a Mac this writes a stand-in compiler that produces an empty object file, and a
// stand-in archiver that produces an empty archive. Nothing ever links against them — a
// check has nothing to link — they exist only so that one build script finishes.
//
//   node scripts/check-macos.mjs                 aarch64-apple-darwin (Apple Silicon)
//   node scripts/check-macos.mjs --x64           x86_64-apple-darwin (Intel)
//   node scripts/check-macos.mjs --target <t>    any other triple

import { spawnSync } from "node:child_process";
import { chmodSync, mkdirSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const crate = join(root, "toolbar", "src-tauri");

const argv = process.argv.slice(2);
const named = argv.indexOf("--target");
const target =
  named >= 0 && argv[named + 1]
    ? argv[named + 1]
    : argv.includes("--x64")
      ? "x86_64-apple-darwin"
      : "aarch64-apple-darwin";

const onWindows = process.platform === "win32";
const onMac = process.platform === "darwin";

console.error(
  `colai: type-checking the toolbar for ${target}. This is a check only — nothing is ` +
    "compiled to machine code or linked" +
    (onMac ? "." : ", and the stand-in C compiler exists only so objc2-exception-helper's build script finishes."),
);

const env = { ...process.env };

if (!onMac) {
  // The standard library for the target has to be there to check against.
  const installed = spawnSync("rustup", ["target", "list", "--installed"], { encoding: "utf8" });
  if (installed.error || installed.status !== 0) {
    console.error("colai: rustup is needed to check a foreign target, and it could not be run.");
    process.exit(1);
  }
  if (!installed.stdout.split(/\r?\n/).includes(target)) {
    console.error(`colai: the Rust standard library for ${target} is not installed. Add it with:`);
    console.error(`  rustup target add ${target}`);
    process.exit(1);
  }

  // `.cjs`, not `.js`: the temp directory is nobody's package, but a `"type": "module"` above
  // it somewhere would turn `require` into an error, and these are four lines each.
  const stubs = join(tmpdir(), "colai-macos-check");
  mkdirSync(stubs, { recursive: true });
  writeFileSync(
    join(stubs, "fake-cc.cjs"),
    [
      "// Stand-in C compiler for a macOS type-check off a Mac: an empty object file is all a",
      "// build script needs to finish, because `cargo check` never links.",
      'const fs = require("fs");',
      "const args = process.argv.slice(2);",
      'const i = args.indexOf("-o");',
      'if (i >= 0 && args[i + 1]) fs.writeFileSync(args[i + 1], "");',
      "// `-E` probes (compiler-family detection) expect something on stdout.",
      'if (args.includes("-E")) process.stdout.write("clang\\n");',
      "",
    ].join("\n"),
  );
  writeFileSync(
    join(stubs, "fake-ar.cjs"),
    [
      "// Stand-in archiver: an empty, valid ar archive at the first *.a argument.",
      'const fs = require("fs");',
      'const out = process.argv.slice(2).find((a) => a.endsWith(".a"));',
      'if (out) fs.writeFileSync(out, "!<arch>\\n");',
      "",
    ].join("\n"),
  );

  // `cc` runs the compiler as a program, so each stand-in needs a wrapper the OS can execute.
  const wrap = (name) => {
    if (onWindows) {
      const path = join(stubs, `${name}.cmd`);
      writeFileSync(path, `@node "%~dp0${name}.cjs" %*\r\n`);
      return path;
    }
    const path = join(stubs, name);
    writeFileSync(path, `#!/bin/sh\nexec node "$(dirname "$0")/${name}.cjs" "$@"\n`);
    chmodSync(path, 0o755);
    return path;
  };

  const triple = target.replaceAll("-", "_");
  env[`CC_${triple}`] = wrap("fake-cc");
  env[`AR_${triple}`] = wrap("fake-ar");
}

// A target directory of its own: a check for another OS shares nothing useful with the host
// build, and mixing them would make the host's next `cargo build` re-check the world.
env.CARGO_TARGET_DIR = join(crate, "target", "macos-check");

for (const extra of [[], ["--tests"]]) {
  const checked = spawnSync("cargo", ["check", "--locked", "--target", target, ...extra], {
    cwd: crate,
    env,
    stdio: "inherit",
  });
  if (checked.error) {
    console.error(`colai: cargo could not be run: ${String(checked.error)}`);
    process.exit(1);
  }
  if (checked.status !== 0) process.exit(checked.status ?? 1);
}

console.error(`colai: the toolbar type-checks for ${target}.`);
