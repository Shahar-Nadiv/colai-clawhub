// Build the toolbar that actually ships.
//
// `build-toolbar.mjs` builds one for the machine it is run on, which is what you want
// while working on it and never what you want to publish: the binary inherits its build
// host's glibc as the oldest Linux it can run on, and Tauri embeds the build context
// path — the author's home directory — inside it. Neither is visible in the tarball and
// neither is recoverable by the person who installs it.
//
// So the published one is built somewhere neutral and old on purpose. Everything about
// that machine is in `release/Dockerfile`, and the result carries a note saying where it
// came from, which `check-shippable.mjs` reads before anything can be packed.

import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  copyFileSync,
  cpSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const IMAGE = "colai-release-build:jammy";

function run(command, args, options = {}) {
  const done = spawnSync(command, args, { stdio: "inherit", encoding: "utf8", ...options });
  if (done.error) {
    console.error(`colai: could not run ${command} — ${done.error.message}`);
    process.exit(1);
  }
  if (done.status !== 0) {
    process.exit(done.status ?? 1);
  }
  return done;
}

if (spawnSync("docker", ["version"], { stdio: "ignore" }).status !== 0) {
  console.error("colai: a release build needs Docker, and it is not running here.");
  console.error("Everything about the build machine is in release/Dockerfile.");
  process.exit(1);
}

console.log("colai: preparing the release image.");
run("docker", [
  "build",
  "-f",
  join(root, "release", "Dockerfile"),
  "-t",
  IMAGE,
  join(root, "release"),
]);

// Copied, not mounted. The path is part of the artifact — see the Dockerfile — so the
// crate has to be built somewhere that says nothing about who built it.
const work = mkdtempSync(join(tmpdir(), "colai-release-"));
try {
  mkdirSync(join(work, "out"), { recursive: true });
  // `target/` is left behind rather than copied and then deleted. It used to be copied
  // and then deleted, which is minutes of disk-to-disk for a directory the container has
  // no use for — it builds from nothing — and on a full disk it is the thing that fails
  // the build. A local `cargo test` puts three gigabytes of debug artifacts there, so
  // this is the ordinary case rather than the unlucky one.
  const notTheTarget = join(root, "toolbar", "src-tauri", "target");
  cpSync(join(root, "toolbar", "src-tauri"), join(work, "src-tauri"), {
    recursive: true,
    filter: (from) => from !== notTheTarget && !from.startsWith(`${notTheTarget}/`),
  });
  // `frontendDist` is `../ui`, so the page has to sit beside the crate exactly as it does
  // in the checkout, or the binary is built around an empty window.
  cpSync(join(root, "toolbar", "ui"), join(work, "ui"), { recursive: true });
  copyFileSync(join(root, "release", "build.sh"), join(work, "build.sh"));

  console.log("colai: building. The first run downloads and compiles everything.");
  const built = run(
    "docker",
    [
      "run",
      "--rm",
      "-v",
      `${work}:/build`,
      "-v",
      `${join(work, "out")}:/out`,
      IMAGE,
      "sh",
      "/build/build.sh",
    ],
    { stdio: ["ignore", "pipe", "inherit"] },
  );
  process.stdout.write(built.stdout ?? "");

  // The number that decides which machines this runs on, taken from the build's own
  // report rather than guessed from the image tag.
  const glibc = (built.stdout ?? "").match(/GLIBC_([0-9]+\.[0-9]+)\s*$/m)?.[1];
  if (!glibc) {
    console.error("colai: the build did not report the glibc it needs, so it is not trusted.");
    process.exit(1);
  }

  mkdirSync(join(root, "bin"), { recursive: true });
  const staged = join(root, "bin", "colai-toolbar");
  copyFileSync(join(work, "out", "colai-toolbar"), staged);

  const digest = createHash("sha256").update(readFileSync(staged)).digest("hex");
  writeFileSync(`${staged}.sha256`, `${digest}\n`);
  writeFileSync(
    `${staged}.build.json`,
    `${JSON.stringify({ image: IMAGE, glibc, at: new Date().toISOString().slice(0, 10), sha256: digest }, null, 2)}\n`,
  );

  console.log(`colai: staged at bin/colai-toolbar, runs on glibc ${glibc} and newer.`);
  console.log(`colai: sha256 ${digest}`);
} finally {
  // Belt to the braces of the container clearing its own `target/`: anything left that
  // this user cannot delete is a temporary directory, and losing it is not worth failing
  // a build that has already produced its artifact.
  try {
    rmSync(work, { recursive: true, force: true });
  } catch (error) {
    console.warn(`colai: could not remove ${work} — ${error.message}`);
  }
}
