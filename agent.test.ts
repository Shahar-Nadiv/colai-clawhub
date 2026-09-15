// What the session sidecar promises, independent of anything Rust.
//
// The sidecar is the whole backend on this host — it replaces 4,903 lines of OpenClaw
// Gateway client with 200 and a bundle. These are the properties that, if they broke,
// would break it quietly.
import { existsSync, readFileSync } from "node:fs";
import { describe, expect, it as test } from "vitest";

const manifest = JSON.parse(readFileSync(new URL("./package.json", import.meta.url), "utf8"));
const sidecar = readFileSync(new URL("./agent/sidecar.mjs", import.meta.url), "utf8");

describe("the session sidecar", () => {
  test("the SDK is a build-time dependency and never a runtime one", () => {
    /*
     * `@anthropic-ai/claude-agent-sdk` resolves its own Claude Code binary through
     * per-platform optional dependencies, and installed normally that is 217 MB of a
     * second Claude Code — measured. The sidecar drives the one the user already has, so
     * the bundle needs none of it.
     *
     * If this ever moves to `dependencies`, every person installing colai downloads a
     * Claude Code they already own, and nothing else fails to warn them.
     */
    expect(Object.keys(manifest.dependencies ?? {})).not.toContain("@anthropic-ai/claude-agent-sdk");
    expect(Object.keys(manifest.devDependencies ?? {})).toContain("@anthropic-ai/claude-agent-sdk");
    // And the bundle ships, not the source and not a directory that would sweep one in.
    expect(manifest.files).toContain("agent/sidecar.bundle.mjs");
    expect(manifest.files).not.toContain("agent/");
  });

  test("it drives the claude the user already logged into", () => {
    /*
     * This is the property that means colai needs no API key on this host: the SDK spawns
     * the binary the user authenticated, so `apiKeySource` comes back `none`. Letting the
     * SDK fall back to its own copy would be a different Claude Code with a different idea
     * of who is logged in.
     */
    expect(sidecar).toContain("pathToClaudeCodeExecutable");
    expect(sidecar).toContain("COLAI_CLAUDE");
  });

  test("it finds that binary without running anybody's shell", () => {
    /*
     * The obvious `sh -lc "command -v claude"` sources the user's rc files. On this machine
     * that died on a dangling snap env path and returned nothing — which the caller would
     * have read as "no claude installed". A toolbar launched from a desktop session has no
     * business running shell startup at all.
     */
    expect(sidecar).not.toMatch(/sh"?,\s*\[\s*"-lc"/);
    expect(sidecar).toContain("process.env.PATH");
  });

  test("composition stays in the UI, where the redaction is", () => {
    /*
     * `observed()`, `asGiven()`, `withoutHome()` and the `<observed>` fence all live in
     * toolbar-tools.js. A second place that assembles prompt text is a second place for a
     * fence to be forgotten.
     *
     * Read with the comments stripped. The first version of this test failed on the
     * sidecar's own comment explaining that the fence lives elsewhere — which is evidence
     * for the property, not against it.
     */
    const code = sidecar.replace(/\/\*[\s\S]*?\*\//g, " ").replace(/^\s*\/\/.*$/gm, " ");
    expect(code).not.toContain("<observed>");
    expect(code).not.toMatch(/\bwithoutHome\b|\basGiven\b|\bobserved\(/);
  });

  test("replies arrive in the shape the page already reads", () => {
    /*
     * `spokenBy` in `toolbar-answers.js` takes `{role: "assistant", content: [{type:
     * "text", text}]}` — the Messages API shape, which is what the SDK hands over. So the
     * sidecar forwards the assistant message whole rather than re-packing it.
     *
     * Asserted here because the temptation is to invent a tidier colai-shaped reply, and
     * that would be a second format for the same thing: two places to disagree, and the
     * disagreement shows up as a toolbar that silently draws nothing.
     */
    const code = sidecar.replace(/\/\*[\s\S]*?\*\//g, " ").replace(/^\s*\/\/.*$/gm, " ");
    expect(code).toContain("message: message.message");
    expect(code).toMatch(/event: "reply"[\s\S]{0,80}sessionKey/);
  });

  test("the doing pill gets the three fields it reads", () => {
    // `toolbar.js` drops any doing event without `name`, and uses `args` and `sessionKey`.
    const code = sidecar.replace(/\/\*[\s\S]*?\*\//g, " ").replace(/^\s*\/\/.*$/gm, " ");
    const doing = code.slice(code.indexOf('event: "doing"'));
    for (const field of ["name:", "args:", "sessionKey:"]) {
      expect(doing.slice(0, 200), `doing carries ${field}`).toContain(field);
    }
  });

  test("a bundle exists to ship, once it has been built", () => {
    // Not a hard failure in a fresh checkout — `npm run build:sidecar` makes it — but the
    // packaging test above is meaningless if nothing ever produces the file it names.
    expect(existsSync(new URL("./scripts/build-sidecar.mjs", import.meta.url))).toBe(true);
    expect(manifest.scripts["build:sidecar"]).toBe("node scripts/build-sidecar.mjs");
  });
});
