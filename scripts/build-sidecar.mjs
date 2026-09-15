// Bundle the session sidecar into one file with nothing beside it.
//
// The sidecar drives `@anthropic-ai/claude-agent-sdk`, and that package resolves its own
// Claude Code binary through per-platform optional dependencies — the same arrangement the
// toolbar uses for itself. Installed normally it brings 217 MB of a second Claude Code.
//
// We never use it. The sidecar points at the `claude` the user already has, for two
// reasons: shipping a second copy to somebody who installed this *for* Claude Code is a
// strange thing to ask, and it would be a *different* Claude Code, with its own version and
// its own idea of who is logged in — which is exactly the property that means colai needs
// no API key.
//
// So the SDK is a build-time dependency, bundled to 1.6 MB, and the published package has
// no runtime dependency on it at all. Measured: the bundle runs a full image round trip in
// a directory with no `node_modules` in it.
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, statSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const build = join(root, "node_modules", ".bin", "esbuild");
if (!existsSync(build)) {
  console.error("colai: esbuild is not installed. Run npm install first.");
  process.exit(1);
}

const out = join(root, "agent", "sidecar.bundle.mjs");
mkdirSync(dirname(out), { recursive: true });

const ran = spawnSync(
  build,
  [
    join(root, "agent", "sidecar.mjs"),
    "--bundle",
    `--outfile=${out}`,
    "--platform=node",
    "--format=esm",
    // The floor the package claims, same as the plugin runtime.
    "--target=node22",
    "--log-level=warning",
  ],
  { stdio: "inherit" },
);
if (ran.status !== 0) {
  process.exit(ran.status ?? 1);
}

const megabytes = (bytes) => `${(bytes / 1024 / 1024).toFixed(1)} MB`;
console.log(`colai: session sidecar bundled at agent/sidecar.bundle.mjs (${megabytes(statSync(out).size)}).`);
